//! SQLite cache for RSS news items.
//!
//! Stores news items with 48-hour retention.
//! Deduplicates by URL.
//! Query by source, category, search term, or time range.

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct NewsEntry {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub source: String,
    pub source_type: String,
    pub symbol_tag: Option<String>,
    pub source_domain: String,
    pub source_tier: i64,
    pub source_tier_inferred: bool,
    pub source_independence: NewsSourceIndependence,
    pub description: String,
    pub extra_snippets: Vec<String>,
    pub category: String,
    pub published_at: i64,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsSourceIndependence {
    Independent,
    Wire,
    Restatement,
    Rumor,
    Unknown,
}

impl NewsSourceIndependence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::Wire => "wire",
            Self::Restatement => "restatement",
            Self::Rumor => "rumor",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "independent" => Ok(Self::Independent),
            "wire" => Ok(Self::Wire),
            "restatement" => Ok(Self::Restatement),
            "rumor" | "rumour" => Ok(Self::Rumor),
            "unknown" => Ok(Self::Unknown),
            other => bail!(
                "invalid source independence '{other}'; expected independent, wire, restatement, rumor, or unknown"
            ),
        }
    }

    fn from_stored(value: &str) -> Self {
        Self::parse(value).unwrap_or(Self::Unknown)
    }
}

impl std::fmt::Display for NewsSourceIndependence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn parse_news_source_independence_filter(value: &str) -> Result<Vec<NewsSourceIndependence>> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(NewsSourceIndependence::parse)
        .collect::<Result<Vec<_>>>()?;
    if values.is_empty() {
        bail!("source-independence filter cannot be empty")
    }
    Ok(values)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NewsSourceTier {
    pub domain: String,
    pub tier: i64,
    pub notes: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsSourceClassification {
    pub domain: String,
    pub tier: i64,
    pub inferred: bool,
}

const SOURCE_TIER_SEEDS: &[(&str, i64, &str)] = &[
    ("reuters.com", 1, "primary wire"),
    ("bloomberg.com", 1, "primary wire"),
    ("apnews.com", 1, "primary wire"),
    ("ft.com", 1, "primary financial press"),
    ("wsj.com", 1, "primary financial press"),
    ("nytimes.com", 2, "major outlet"),
    ("cnbc.com", 2, "major outlet"),
    ("theguardian.com", 2, "major outlet"),
    ("economist.com", 2, "major outlet"),
    ("seekingalpha.com", 3, "aggregator"),
    ("marketwatch.com", 3, "aggregator"),
    ("finance.yahoo.com", 3, "aggregator"),
    ("yahoo.com", 3, "aggregator"),
    ("coindesk.com", 3, "crypto trade outlet"),
    ("cointelegraph.com", 3, "crypto trade outlet"),
    ("decrypt.co", 3, "crypto trade outlet"),
    ("theblock.co", 3, "crypto trade outlet"),
    ("zerohedge.com", 4, "blog/unverified"),
    ("substack.com", 4, "blog platform"),
    ("medium.com", 4, "blog platform"),
];

fn strip_host_prefixes(host: &str) -> String {
    let mut value = host
        .trim()
        .trim_matches('.')
        .trim_start_matches("www.")
        .trim_start_matches("m.")
        .to_ascii_lowercase();
    while value.starts_with('.') {
        value.remove(0);
    }
    value
}

pub fn normalize_source_domain(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | ','));
    if trimmed.is_empty() {
        return None;
    }

    let parse_candidates = if trimmed.contains("://") {
        vec![trimmed.to_string()]
    } else {
        vec![format!("https://{}", trimmed), trimmed.to_string()]
    };

    for candidate in parse_candidates {
        if let Ok(parsed) = reqwest::Url::parse(&candidate) {
            if let Some(host) = parsed.host_str() {
                let domain = strip_host_prefixes(host);
                if domain.contains('.') {
                    return Some(domain);
                }
            }
        }
    }

    let host = trimmed
        .split("://")
        .last()
        .unwrap_or(trimmed)
        .split('/')
        .next()
        .unwrap_or(trimmed)
        .split('?')
        .next()
        .unwrap_or(trimmed)
        .split('#')
        .next()
        .unwrap_or(trimmed)
        .split(':')
        .next()
        .unwrap_or(trimmed);
    let domain = strip_host_prefixes(host);
    if domain.contains('.') {
        Some(domain)
    } else {
        None
    }
}

fn known_source_domain(source: &str) -> Option<&'static str> {
    let lower = source.to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect();

    if compact.contains("reuters") {
        Some("reuters.com")
    } else if compact.contains("bloomberg") {
        Some("bloomberg.com")
    } else if compact == "ap" || compact.contains("associatedpress") {
        Some("apnews.com")
    } else if compact.contains("financialtimes") || compact == "ft" {
        Some("ft.com")
    } else if compact.contains("wallstreetjournal") || compact == "wsj" {
        Some("wsj.com")
    } else if compact.contains("nytimes") || compact.contains("newyorktimes") {
        Some("nytimes.com")
    } else if compact.contains("cnbc") {
        Some("cnbc.com")
    } else if compact.contains("guardian") {
        Some("theguardian.com")
    } else if compact.contains("economist") {
        Some("economist.com")
    } else if compact.contains("seekingalpha") {
        Some("seekingalpha.com")
    } else if compact.contains("marketwatch") {
        Some("marketwatch.com")
    } else if compact.contains("yahoofinance") {
        Some("finance.yahoo.com")
    } else if compact.contains("yahoo") {
        Some("yahoo.com")
    } else if compact.contains("coindesk") {
        Some("coindesk.com")
    } else if compact.contains("cointelegraph") {
        Some("cointelegraph.com")
    } else if compact.contains("decrypt") {
        Some("decrypt.co")
    } else if compact.contains("theblock") {
        Some("theblock.co")
    } else if compact.contains("zerohedge") {
        Some("zerohedge.com")
    } else if compact.contains("substack") {
        Some("substack.com")
    } else {
        None
    }
}

fn source_key(source: &str) -> Option<String> {
    let lowered = source.trim().to_ascii_lowercase();
    let parts: Vec<_> = lowered
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("-"))
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn article_haystack(
    title: &str,
    source: &str,
    description: Option<&str>,
    extra_snippets: &[String],
) -> String {
    let mut parts = vec![title.trim(), source.trim()];
    if let Some(description) = description {
        parts.push(description.trim());
    }
    for snippet in extra_snippets {
        parts.push(snippet.trim());
    }
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

pub fn classify_news_source_independence(
    title: &str,
    source: &str,
    description: Option<&str>,
    extra_snippets: &[String],
) -> NewsSourceIndependence {
    let haystack = article_haystack(title, source, description, extra_snippets);
    if haystack.trim().is_empty() {
        return NewsSourceIndependence::Unknown;
    }

    let rumor_patterns = [
        "according to people familiar",
        "people familiar with the matter",
        "people familiar with the talks",
        "people familiar with the plans",
        "person familiar with the matter",
        "anonymous source",
        "anonymous sources",
        "unnamed source",
        "unnamed sources",
        "according to sources",
        "sources said",
        "source said",
        "reportedly",
    ];
    if contains_any(&haystack, &rumor_patterns) {
        return NewsSourceIndependence::Rumor;
    }

    let restatement_patterns = [
        "said in a statement",
        "according to a statement",
        "according to the statement",
        "the statement said",
        "statement said",
        "said in the statement",
        "said in a press release",
        "according to a press release",
        "press release said",
        "announced in a statement",
        "said on x",
        "said on twitter",
        "wrote on x",
        "posted on x",
    ];
    if contains_any(&haystack, &restatement_patterns) {
        return NewsSourceIndependence::Restatement;
    }

    let wire_header_patterns = [
        "(reuters)",
        "(bloomberg)",
        "(ap)",
        " reuters - ",
        " bloomberg - ",
        " associated press - ",
    ];
    let source_key = source_key(source).unwrap_or_default();
    let padded_haystack = format!(" {haystack} ");
    if contains_any(&padded_haystack, &wire_header_patterns)
        || matches!(
            source_key.as_str(),
            "reuters" | "ap" | "associated-press" | "associatedpress"
        )
    {
        return NewsSourceIndependence::Wire;
    }

    NewsSourceIndependence::Independent
}

pub fn source_domain_for(url: &str, source: &str) -> String {
    normalize_source_domain(url)
        .or_else(|| known_source_domain(source).map(str::to_string))
        .or_else(|| normalize_source_domain(source))
        .or_else(|| source_key(source))
        .unwrap_or_else(|| "unknown".to_string())
}

fn normalize_source_mapping_domain(value: &str) -> Option<String> {
    normalize_source_domain(value)
        .or_else(|| known_source_domain(value).map(str::to_string))
        .or_else(|| source_key(value))
}

fn domain_candidates(domain: &str) -> Vec<String> {
    let domain = strip_host_prefixes(domain);
    let labels: Vec<_> = domain.split('.').filter(|part| !part.is_empty()).collect();
    let mut candidates = Vec::new();
    for idx in 0..labels.len() {
        let candidate = labels[idx..].join(".");
        if candidate.contains('.') && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    if candidates.is_empty() && !domain.is_empty() {
        candidates.push(domain);
    }
    candidates
}

fn validate_source_tier(tier: i64) -> Result<()> {
    if (1..=4).contains(&tier) {
        Ok(())
    } else {
        bail!("source tier must be between 1 and 4")
    }
}

fn news_column_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = ?1")?
        .query_row(params![name], |row| row.get::<_, i64>(0))
        .unwrap_or(0);
    Ok(count > 0)
}

fn ensure_source_tier_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS news_source_tiers (
            domain TEXT PRIMARY KEY,
            tier INTEGER NOT NULL CHECK(tier BETWEEN 1 AND 4),
            notes TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    if !news_column_exists(conn, "source_domain")? {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN source_domain TEXT NOT NULL DEFAULT ''",
        )?;
    }
    if !news_column_exists(conn, "source_tier")? {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN source_tier INTEGER NOT NULL DEFAULT 3 CHECK(source_tier BETWEEN 1 AND 4)",
        )?;
    }
    if !news_column_exists(conn, "source_tier_inferred")? {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN source_tier_inferred INTEGER NOT NULL DEFAULT 1 CHECK(source_tier_inferred IN (0, 1))",
        )?;
    }
    if !news_column_exists(conn, "source_independence")? {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN source_independence TEXT NOT NULL DEFAULT 'unknown' CHECK(source_independence IN ('independent','wire','restatement','rumor','unknown'))",
        )?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_news_source_domain ON news_cache(source_domain);
         CREATE INDEX IF NOT EXISTS idx_news_source_tier ON news_cache(source_tier);
         CREATE INDEX IF NOT EXISTS idx_news_source_independence ON news_cache(source_independence);",
    )?;

    Ok(())
}

fn seed_source_tiers(conn: &Connection) -> Result<()> {
    let count = conn.query_row("SELECT COUNT(*) FROM news_source_tiers", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if count > 0 {
        return Ok(());
    }

    for (domain, tier, notes) in SOURCE_TIER_SEEDS {
        conn.execute(
            "INSERT INTO news_source_tiers (domain, tier, notes, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(domain) DO NOTHING",
            params![domain, tier, notes],
        )?;
    }

    Ok(())
}

fn lookup_source_tier_inner(conn: &Connection, domain: &str) -> Result<(i64, bool)> {
    for candidate in domain_candidates(domain) {
        let tier = conn.query_row(
            "SELECT tier FROM news_source_tiers WHERE domain = ?1",
            params![candidate],
            |row| row.get::<_, i64>(0),
        );
        match tier {
            Ok(tier) => return Ok((tier, false)),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok((3, true))
}

pub fn lookup_source_tier(conn: &Connection, domain: &str) -> Result<(i64, bool)> {
    ensure_source_tier_schema(conn)?;
    seed_source_tiers(conn)?;
    lookup_source_tier_inner(conn, domain)
}

pub fn classify_news_source(
    conn: &Connection,
    url: &str,
    source: &str,
) -> Result<NewsSourceClassification> {
    ensure_source_tier_schema(conn)?;
    seed_source_tiers(conn)?;
    let domain = source_domain_for(url, source);
    let (tier, inferred) = lookup_source_tier_inner(conn, &domain)?;
    Ok(NewsSourceClassification {
        domain,
        tier,
        inferred,
    })
}

pub fn classify_news_source_backend(
    backend: &BackendConnection,
    url: &str,
    source: &str,
) -> Result<NewsSourceClassification> {
    query::dispatch(
        backend,
        |conn| classify_news_source(conn, url, source),
        |pool| classify_news_source_postgres(pool, url, source),
    )
}

fn backfill_news_source_tiers(conn: &Connection) -> Result<()> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, url, source
             FROM news_cache
             WHERE source_domain = ''",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut rows = Vec::new();
        for row in mapped {
            rows.push(row?);
        }
        rows
    };

    for (id, url, source) in rows {
        let classification = classify_news_source(conn, &url, &source)?;
        conn.execute(
            "UPDATE news_cache
             SET source_domain = ?1, source_tier = ?2, source_tier_inferred = ?3
             WHERE id = ?4",
            params![
                classification.domain,
                classification.tier,
                if classification.inferred { 1 } else { 0 },
                id
            ],
        )?;
    }

    Ok(())
}

fn backfill_news_source_independence(conn: &Connection) -> Result<()> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, title, source, description, extra_snippets
             FROM news_cache
             WHERE source_independence = 'unknown' OR source_independence = ''",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut rows = Vec::new();
        for row in mapped {
            rows.push(row?);
        }
        rows
    };

    for (id, title, source, description, snippets_json) in rows {
        let snippets = serde_json::from_str::<Vec<String>>(&snippets_json).unwrap_or_default();
        let independence =
            classify_news_source_independence(&title, &source, Some(&description), &snippets);
        conn.execute(
            "UPDATE news_cache
             SET source_independence = ?1
             WHERE id = ?2",
            params![independence.as_str(), id],
        )?;
    }

    Ok(())
}

pub fn ensure_source_tier_tables(conn: &Connection) -> Result<()> {
    ensure_source_tier_schema(conn)?;
    seed_source_tiers(conn)?;
    backfill_news_source_tiers(conn)?;
    backfill_news_source_independence(conn)?;
    Ok(())
}

pub fn list_news_source_tiers(conn: &Connection) -> Result<Vec<NewsSourceTier>> {
    ensure_source_tier_schema(conn)?;
    seed_source_tiers(conn)?;

    let mut stmt = conn.prepare(
        "SELECT domain, tier, notes, updated_at
         FROM news_source_tiers
         ORDER BY tier ASC, domain ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(NewsSourceTier {
            domain: row.get(0)?,
            tier: row.get(1)?,
            notes: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;

    let mut tiers = Vec::new();
    for row in rows {
        tiers.push(row?);
    }
    Ok(tiers)
}

pub fn list_news_source_tiers_backend(backend: &BackendConnection) -> Result<Vec<NewsSourceTier>> {
    query::dispatch(
        backend,
        list_news_source_tiers,
        list_news_source_tiers_postgres,
    )
}

pub fn set_news_source_tier(
    conn: &Connection,
    domain: &str,
    tier: i64,
    notes: Option<&str>,
) -> Result<NewsSourceTier> {
    validate_source_tier(tier)?;
    ensure_source_tier_schema(conn)?;
    seed_source_tiers(conn)?;
    let domain = normalize_source_mapping_domain(domain)
        .ok_or_else(|| anyhow::anyhow!("source domain cannot be empty"))?;

    conn.execute(
        "INSERT INTO news_source_tiers (domain, tier, notes, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(domain) DO UPDATE
         SET tier = excluded.tier,
             notes = excluded.notes,
             updated_at = datetime('now')",
        params![domain, tier, notes],
    )?;

    let child_pattern = format!("%.{}", domain);
    conn.execute(
        "UPDATE news_cache
         SET source_tier = ?2, source_tier_inferred = 0
         WHERE source_domain = ?1 OR source_domain LIKE ?3",
        params![domain, tier, child_pattern],
    )?;

    get_news_source_tier(conn, &domain)
}

pub fn set_news_source_tier_backend(
    backend: &BackendConnection,
    domain: &str,
    tier: i64,
    notes: Option<&str>,
) -> Result<NewsSourceTier> {
    query::dispatch(
        backend,
        |conn| set_news_source_tier(conn, domain, tier, notes),
        |pool| set_news_source_tier_postgres(pool, domain, tier, notes),
    )
}

pub fn remove_news_source_tier(conn: &Connection, domain: &str) -> Result<bool> {
    ensure_source_tier_schema(conn)?;
    let domain = normalize_source_mapping_domain(domain)
        .ok_or_else(|| anyhow::anyhow!("source domain cannot be empty"))?;
    let deleted = conn.execute(
        "DELETE FROM news_source_tiers WHERE domain = ?1",
        params![domain],
    )?;

    let child_pattern = format!("%.{}", domain);
    conn.execute(
        "UPDATE news_cache
         SET source_tier = 3, source_tier_inferred = 1
         WHERE source_domain = ?1 OR source_domain LIKE ?2",
        params![domain, child_pattern],
    )?;

    Ok(deleted > 0)
}

pub fn remove_news_source_tier_backend(backend: &BackendConnection, domain: &str) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| remove_news_source_tier(conn, domain),
        |pool| remove_news_source_tier_postgres(pool, domain),
    )
}

fn get_news_source_tier(conn: &Connection, domain: &str) -> Result<NewsSourceTier> {
    Ok(conn.query_row(
        "SELECT domain, tier, notes, updated_at
         FROM news_source_tiers
         WHERE domain = ?1",
        params![domain],
        |row| {
            Ok(NewsSourceTier {
                domain: row.get(0)?,
                tier: row.get(1)?,
                notes: row.get(2)?,
                updated_at: row.get(3)?,
            })
        },
    )?)
}

/// Insert a news item into the cache.
///
/// Deduplicates by URL (ignores duplicates).
pub fn insert_news(
    conn: &Connection,
    title: &str,
    url: &str,
    source: &str,
    category: &str,
    published_at: i64,
) -> Result<()> {
    insert_news_with_source_type(
        conn,
        title,
        url,
        source,
        "rss",
        None,
        category,
        published_at,
        None,
        &[],
    )
}

pub fn insert_news_backend(
    backend: &BackendConnection,
    title: &str,
    url: &str,
    source: &str,
    category: &str,
    published_at: i64,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| insert_news(conn, title, url, source, category, published_at),
        |pool| insert_news_postgres(pool, title, url, source, category, published_at),
    )
}

/// Insert a news item with an explicit source type ("rss" or "brave").
#[allow(clippy::too_many_arguments)]
pub fn insert_news_with_source_type(
    conn: &Connection,
    title: &str,
    url: &str,
    source: &str,
    source_type: &str,
    symbol_tag: Option<&str>,
    category: &str,
    published_at: i64,
    description: Option<&str>,
    extra_snippets: &[String],
) -> Result<()> {
    let snippets_json = serde_json::to_string(extra_snippets).unwrap_or_else(|_| "[]".to_string());
    let classification = classify_news_source(conn, url, source)?;
    let independence =
        classify_news_source_independence(title, source, description, extra_snippets);
    conn.execute(
        "INSERT OR IGNORE INTO news_cache
         (title, url, source, source_type, symbol_tag, source_domain, source_tier, source_tier_inferred, source_independence, description, extra_snippets, category, published_at, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'))",
        params![
            title,
            url,
            source,
            source_type,
            symbol_tag,
            classification.domain,
            classification.tier,
            if classification.inferred { 1 } else { 0 },
            independence.as_str(),
            description.unwrap_or(""),
            snippets_json,
            category,
            published_at
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_news_with_source_type_backend(
    backend: &BackendConnection,
    title: &str,
    url: &str,
    source: &str,
    source_type: &str,
    symbol_tag: Option<&str>,
    category: &str,
    published_at: i64,
    description: Option<&str>,
    extra_snippets: &[String],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| {
            insert_news_with_source_type(
                conn,
                title,
                url,
                source,
                source_type,
                symbol_tag,
                category,
                published_at,
                description,
                extra_snippets,
            )
        },
        |pool| {
            insert_news_with_source_type_postgres(
                pool,
                title,
                url,
                source,
                source_type,
                symbol_tag,
                category,
                published_at,
                description,
                extra_snippets,
            )
        },
    )
}

/// Get latest N news items, optionally filtered.
///
/// Filters can be combined (AND logic).
pub fn get_latest_news(
    conn: &Connection,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
) -> Result<Vec<NewsEntry>> {
    get_latest_news_filtered(
        conn,
        limit,
        source_filter,
        category_filter,
        search_term,
        hours_back,
        None,
    )
}

/// Get latest N news items with optional source-independence filtering.
///
/// Filters can be combined (AND logic).
pub fn get_latest_news_filtered(
    conn: &Connection,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
    independence_filter: Option<&[NewsSourceIndependence]>,
) -> Result<Vec<NewsEntry>> {
    ensure_source_tier_tables(conn)?;
    let mut sql = "SELECT id, title, url, source, source_type, symbol_tag, source_domain, source_tier, source_tier_inferred, source_independence, description, extra_snippets, category, published_at, fetched_at
                   FROM news_cache
                   WHERE 1=1".to_string();

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(source) = source_filter {
        sql.push_str(" AND source = ?");
        params_vec.push(Box::new(source.to_string()));
    }

    if let Some(category) = category_filter {
        sql.push_str(" AND category = ?");
        params_vec.push(Box::new(category.to_string()));
    }

    if let Some(term) = search_term {
        sql.push_str(" AND title LIKE ?");
        params_vec.push(Box::new(format!("%{}%", term)));
    }

    if let Some(hours) = hours_back {
        sql.push_str(" AND published_at > ?");
        let cutoff = chrono::Utc::now().timestamp() - (hours * 3600);
        params_vec.push(Box::new(cutoff));
    }

    if let Some(independence_values) = independence_filter.filter(|values| !values.is_empty()) {
        sql.push_str(" AND source_independence IN (");
        for (idx, value) in independence_values.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
            params_vec.push(Box::new(value.as_str().to_string()));
        }
        sql.push(')');
    }

    sql.push_str(" ORDER BY published_at DESC LIMIT ?");
    params_vec.push(Box::new(limit as i64));

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(NewsEntry {
            id: row.get(0)?,
            title: row.get(1)?,
            url: row.get(2)?,
            source: row.get(3)?,
            source_type: row.get(4)?,
            symbol_tag: row.get(5)?,
            source_domain: row.get(6)?,
            source_tier: row.get(7)?,
            source_tier_inferred: row.get::<_, i64>(8)? != 0,
            source_independence: NewsSourceIndependence::from_stored(&row.get::<_, String>(9)?),
            description: row.get(10)?,
            extra_snippets: serde_json::from_str::<Vec<String>>(&row.get::<_, String>(11)?)
                .unwrap_or_default(),
            category: row.get(12)?,
            published_at: row.get(13)?,
            fetched_at: row.get(14)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }

    Ok(entries)
}

pub fn get_latest_news_backend(
    backend: &BackendConnection,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
) -> Result<Vec<NewsEntry>> {
    get_latest_news_filtered_backend(
        backend,
        limit,
        source_filter,
        category_filter,
        search_term,
        hours_back,
        None,
    )
}

pub fn get_latest_news_filtered_backend(
    backend: &BackendConnection,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
    independence_filter: Option<&[NewsSourceIndependence]>,
) -> Result<Vec<NewsEntry>> {
    query::dispatch(
        backend,
        |conn| {
            get_latest_news_filtered(
                conn,
                limit,
                source_filter,
                category_filter,
                search_term,
                hours_back,
                independence_filter,
            )
        },
        |pool| {
            get_latest_news_postgres(
                pool,
                limit,
                source_filter,
                category_filter,
                search_term,
                hours_back,
                independence_filter,
            )
        },
    )
}

/// Delete news older than 48 hours.
pub fn cleanup_old_news(conn: &Connection) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp() - (48 * 3600);
    let deleted = conn.execute(
        "DELETE FROM news_cache WHERE published_at < ?1",
        params![cutoff],
    )?;
    Ok(deleted)
}

pub fn cleanup_old_news_backend(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(backend, cleanup_old_news, cleanup_old_news_postgres)
}

/// Get unique sources currently in cache.
pub fn get_sources(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT source FROM news_cache ORDER BY source")?;
    let rows = stmt.query_map([], |row| row.get(0))?;

    let mut sources = Vec::new();
    for row in rows {
        sources.push(row?);
    }

    Ok(sources)
}

pub fn get_sources_backend(backend: &BackendConnection) -> Result<Vec<String>> {
    query::dispatch(backend, get_sources, get_sources_postgres)
}

pub fn latest_fetched_at_by_source_type(
    conn: &Connection,
    source_type: &str,
) -> Result<Option<String>> {
    let value = conn.query_row(
        "SELECT fetched_at
         FROM news_cache
         WHERE source_type = ?1
         ORDER BY datetime(fetched_at) DESC
         LIMIT 1",
        params![source_type],
        |row| row.get(0),
    );

    match value {
        Ok(timestamp) => Ok(Some(timestamp)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn latest_fetched_at_by_source_type_backend(
    backend: &BackendConnection,
    source_type: &str,
) -> Result<Option<String>> {
    query::dispatch(
        backend,
        |conn| latest_fetched_at_by_source_type(conn, source_type),
        |pool| latest_fetched_at_by_source_type_postgres(pool, source_type),
    )
}

async fn seed_source_tiers_postgres(pool: &PgPool) -> Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM news_source_tiers")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    for (domain, tier, notes) in SOURCE_TIER_SEEDS {
        sqlx::query(
            "INSERT INTO news_source_tiers (domain, tier, notes, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (domain) DO NOTHING",
        )
        .bind(domain)
        .bind(tier)
        .bind(notes)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn lookup_source_tier_postgres_inner(pool: &PgPool, domain: &str) -> Result<(i64, bool)> {
    for candidate in domain_candidates(domain) {
        let tier: Option<i64> =
            sqlx::query_scalar("SELECT tier::BIGINT FROM news_source_tiers WHERE domain = $1")
                .bind(candidate)
                .fetch_optional(pool)
                .await?;
        if let Some(tier) = tier {
            return Ok((tier, false));
        }
    }

    Ok((3, true))
}

fn classify_news_source_postgres(
    pool: &PgPool,
    url: &str,
    source: &str,
) -> Result<NewsSourceClassification> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        let domain = source_domain_for(url, source);
        let (tier, inferred) = lookup_source_tier_postgres_inner(pool, &domain).await?;
        Ok::<_, anyhow::Error>(NewsSourceClassification {
            domain,
            tier,
            inferred,
        })
    })
}

async fn backfill_news_source_tiers_postgres(pool: &PgPool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id, url, source
         FROM news_cache
         WHERE source_domain = ''",
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let id: i64 = row.try_get(0)?;
        let url: String = row.try_get(1)?;
        let source: String = row.try_get(2)?;
        let domain = source_domain_for(&url, &source);
        let (tier, inferred) = lookup_source_tier_postgres_inner(pool, &domain).await?;
        sqlx::query(
            "UPDATE news_cache
             SET source_domain = $1, source_tier = $2, source_tier_inferred = $3
             WHERE id = $4",
        )
        .bind(domain)
        .bind(tier)
        .bind(inferred)
        .bind(id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn backfill_news_source_independence_postgres(pool: &PgPool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id, title, source, description, extra_snippets
         FROM news_cache
         WHERE source_independence = 'unknown' OR source_independence = ''",
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let id: i64 = row.try_get(0)?;
        let title: String = row.try_get(1)?;
        let source: String = row.try_get(2)?;
        let description: String = row.try_get(3)?;
        let snippets_json: String = row.try_get(4)?;
        let snippets = serde_json::from_str::<Vec<String>>(&snippets_json).unwrap_or_default();
        let independence =
            classify_news_source_independence(&title, &source, Some(&description), &snippets);
        sqlx::query(
            "UPDATE news_cache
             SET source_independence = $1
             WHERE id = $2",
        )
        .bind(independence.as_str())
        .bind(id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

fn list_news_source_tiers_postgres(pool: &PgPool) -> Result<Vec<NewsSourceTier>> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT domain, tier::BIGINT, notes, updated_at::text
             FROM news_source_tiers
             ORDER BY tier ASC, domain ASC",
        )
        .fetch_all(pool)
        .await
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(NewsSourceTier {
                domain: row.try_get(0)?,
                tier: row.try_get(1)?,
                notes: row.try_get(2)?,
                updated_at: row.try_get(3)?,
            })
        })
        .collect()
}

fn set_news_source_tier_postgres(
    pool: &PgPool,
    domain: &str,
    tier: i64,
    notes: Option<&str>,
) -> Result<NewsSourceTier> {
    validate_source_tier(tier)?;
    let domain = normalize_source_mapping_domain(domain)
        .ok_or_else(|| anyhow::anyhow!("source domain cannot be empty"))?;
    ensure_tables_postgres(pool)?;
    let row = crate::db::pg_runtime::block_on(async {
        let row = sqlx::query(
            "INSERT INTO news_source_tiers (domain, tier, notes, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (domain) DO UPDATE
             SET tier = EXCLUDED.tier,
                 notes = EXCLUDED.notes,
                 updated_at = NOW()
             RETURNING domain, tier::BIGINT, notes, updated_at::text",
        )
        .bind(&domain)
        .bind(tier)
        .bind(notes)
        .fetch_one(pool)
        .await?;

        let child_pattern = format!("%.{}", domain);
        sqlx::query(
            "UPDATE news_cache
             SET source_tier = $2, source_tier_inferred = FALSE
             WHERE source_domain = $1 OR source_domain LIKE $3",
        )
        .bind(&domain)
        .bind(tier)
        .bind(child_pattern)
        .execute(pool)
        .await?;

        Ok::<_, sqlx::Error>(row)
    })?;

    Ok(NewsSourceTier {
        domain: row.try_get(0)?,
        tier: row.try_get(1)?,
        notes: row.try_get(2)?,
        updated_at: row.try_get(3)?,
    })
}

fn remove_news_source_tier_postgres(pool: &PgPool, domain: &str) -> Result<bool> {
    let domain = normalize_source_mapping_domain(domain)
        .ok_or_else(|| anyhow::anyhow!("source domain cannot be empty"))?;
    ensure_tables_postgres(pool)?;
    let deleted = crate::db::pg_runtime::block_on(async {
        let result = sqlx::query("DELETE FROM news_source_tiers WHERE domain = $1")
            .bind(&domain)
            .execute(pool)
            .await?;

        let child_pattern = format!("%.{}", domain);
        sqlx::query(
            "UPDATE news_cache
             SET source_tier = 3, source_tier_inferred = TRUE
             WHERE source_domain = $1 OR source_domain LIKE $2",
        )
        .bind(&domain)
        .bind(child_pattern)
        .execute(pool)
        .await?;

        Ok::<_, sqlx::Error>(result.rows_affected() > 0)
    })?;
    Ok(deleted)
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_cache (
                id BIGSERIAL PRIMARY KEY,
                title TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                source TEXT NOT NULL,
                source_type TEXT NOT NULL DEFAULT 'rss',
                symbol_tag TEXT,
                source_domain TEXT NOT NULL DEFAULT '',
                source_tier INTEGER NOT NULL DEFAULT 3 CHECK(source_tier BETWEEN 1 AND 4),
                source_tier_inferred BOOLEAN NOT NULL DEFAULT TRUE,
                source_independence TEXT NOT NULL DEFAULT 'unknown'
                    CHECK(source_independence IN ('independent','wire','restatement','rumor','unknown')),
                description TEXT NOT NULL DEFAULT '',
                extra_snippets TEXT NOT NULL DEFAULT '[]',
                category TEXT NOT NULL DEFAULT 'general',
                published_at BIGINT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_source_tiers (
                domain TEXT PRIMARY KEY,
                tier INTEGER NOT NULL CHECK(tier BETWEEN 1 AND 4),
                notes TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("ALTER TABLE news_cache ADD COLUMN IF NOT EXISTS source_domain TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE news_cache ADD COLUMN IF NOT EXISTS source_tier INTEGER NOT NULL DEFAULT 3 CHECK(source_tier BETWEEN 1 AND 4)")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE news_cache ADD COLUMN IF NOT EXISTS source_tier_inferred BOOLEAN NOT NULL DEFAULT TRUE")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE news_cache ADD COLUMN IF NOT EXISTS source_independence TEXT NOT NULL DEFAULT 'unknown' CHECK(source_independence IN ('independent','wire','restatement','rumor','unknown'))")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_news_source_domain ON news_cache(source_domain)",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_news_source_tier ON news_cache(source_tier)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_news_source_independence ON news_cache(source_independence)")
            .execute(pool)
            .await?;
        seed_source_tiers_postgres(pool).await?;
        backfill_news_source_tiers_postgres(pool).await?;
        backfill_news_source_independence_postgres(pool).await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

fn insert_news_postgres(
    pool: &PgPool,
    title: &str,
    url: &str,
    source: &str,
    category: &str,
    published_at: i64,
) -> Result<()> {
    insert_news_with_source_type_postgres(
        pool,
        title,
        url,
        source,
        "rss",
        None,
        category,
        published_at,
        None,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_news_with_source_type_postgres(
    pool: &PgPool,
    title: &str,
    url: &str,
    source: &str,
    source_type: &str,
    symbol_tag: Option<&str>,
    category: &str,
    published_at: i64,
    description: Option<&str>,
    extra_snippets: &[String],
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let snippets_json = serde_json::to_string(extra_snippets).unwrap_or_else(|_| "[]".to_string());
    let classification = classify_news_source_postgres(pool, url, source)?;
    let independence =
        classify_news_source_independence(title, source, description, extra_snippets);
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO news_cache
             (title, url, source, source_type, symbol_tag, source_domain, source_tier, source_tier_inferred, source_independence, description, extra_snippets, category, published_at, fetched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NOW())
             ON CONFLICT (url) DO UPDATE
             SET description = CASE
                 WHEN news_cache.description = '' AND EXCLUDED.description != ''
                 THEN EXCLUDED.description
                 ELSE news_cache.description
             END,
             source_domain = EXCLUDED.source_domain,
             source_tier = EXCLUDED.source_tier,
             source_tier_inferred = EXCLUDED.source_tier_inferred,
             source_independence = EXCLUDED.source_independence",
        )
        .bind(title)
        .bind(url)
        .bind(source)
        .bind(source_type)
        .bind(symbol_tag)
        .bind(classification.domain)
        .bind(classification.tier)
        .bind(classification.inferred)
        .bind(independence.as_str())
        .bind(description.unwrap_or(""))
        .bind(snippets_json)
        .bind(category)
        .bind(published_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_latest_news_postgres(
    pool: &PgPool,
    limit: usize,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
    search_term: Option<&str>,
    hours_back: Option<i64>,
    independence_filter: Option<&[NewsSourceIndependence]>,
) -> Result<Vec<NewsEntry>> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT id, title, url, source, source_type, symbol_tag, source_domain, source_tier::BIGINT, source_tier_inferred, source_independence, description, extra_snippets, category, published_at, fetched_at::text
             FROM news_cache
             WHERE TRUE",
        );

        if let Some(source) = source_filter {
            qb.push(" AND source = ").push_bind(source);
        }
        if let Some(category) = category_filter {
            qb.push(" AND category = ").push_bind(category);
        }
        if let Some(term) = search_term {
            qb.push(" AND title ILIKE ")
                .push_bind(format!("%{}%", term));
        }
        if let Some(hours) = hours_back {
            let cutoff = chrono::Utc::now().timestamp() - (hours * 3600);
            qb.push(" AND published_at > ").push_bind(cutoff);
        }
        if let Some(independence_values) = independence_filter.filter(|values| !values.is_empty()) {
            qb.push(" AND source_independence IN (");
            let mut separated = qb.separated(", ");
            for value in independence_values {
                separated.push_bind(value.as_str());
            }
            separated.push_unseparated(")");
        }

        qb.push(" ORDER BY published_at DESC LIMIT ")
            .push_bind(limit as i64);
        qb.build().fetch_all(pool).await
    })?;

    rows.into_iter()
        .map(|row| {
            let snippets_json: String = row.try_get(11)?;
            Ok(NewsEntry {
                id: row.try_get(0)?,
                title: row.try_get(1)?,
                url: row.try_get(2)?,
                source: row.try_get(3)?,
                source_type: row.try_get(4)?,
                symbol_tag: row.try_get(5)?,
                source_domain: row.try_get(6)?,
                source_tier: row.try_get(7)?,
                source_tier_inferred: row.try_get(8)?,
                source_independence: NewsSourceIndependence::from_stored(
                    &row.try_get::<String, _>(9)?,
                ),
                description: row.try_get(10)?,
                extra_snippets: serde_json::from_str::<Vec<String>>(&snippets_json)
                    .unwrap_or_default(),
                category: row.try_get(12)?,
                published_at: row.try_get(13)?,
                fetched_at: row.try_get(14)?,
            })
        })
        .collect()
}

fn cleanup_old_news_postgres(pool: &PgPool) -> Result<usize> {
    ensure_tables_postgres(pool)?;
    let cutoff = chrono::Utc::now().timestamp() - (48 * 3600);
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM news_cache WHERE published_at < $1")
            .bind(cutoff)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected() as usize)
}

fn get_sources_postgres(pool: &PgPool) -> Result<Vec<String>> {
    ensure_tables_postgres(pool)?;
    let values = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT source
             FROM news_cache
             ORDER BY source",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(values)
}

fn latest_fetched_at_by_source_type_postgres(
    pool: &PgPool,
    source_type: &str,
) -> Result<Option<String>> {
    ensure_tables_postgres(pool)?;
    let value = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, String>(
            "SELECT fetched_at::text
             FROM news_cache
             WHERE source_type = $1
             ORDER BY fetched_at DESC
             LIMIT 1",
        )
        .bind(source_type)
        .fetch_optional(pool)
        .await
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::run_migrations;

    #[test]
    fn test_insert_and_query_news() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "Bitcoin hits $100k",
            "https://example.com/btc-100k",
            "CoinDesk",
            "crypto",
            1709610000,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bitcoin hits $100k");
        assert_eq!(items[0].source, "CoinDesk");
        assert_eq!(items[0].source_domain, "example.com");
        assert_eq!(items[0].source_tier, 3);
        assert!(items[0].source_tier_inferred);
        assert_eq!(
            items[0].source_independence,
            NewsSourceIndependence::Independent
        );
    }

    #[test]
    fn source_tier_lookup_matches_seeded_domains() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let reuters =
            classify_news_source(&conn, "https://www.reuters.com/markets/rates", "Reuters")
                .unwrap();
        assert_eq!(reuters.domain, "reuters.com");
        assert_eq!(reuters.tier, 1);
        assert!(!reuters.inferred);

        let substack =
            classify_news_source(&conn, "https://macro.substack.com/p/post", "Substack").unwrap();
        assert_eq!(substack.domain, "macro.substack.com");
        assert_eq!(substack.tier, 4);
        assert!(!substack.inferred);
    }

    #[test]
    fn unknown_domain_defaults_to_tier_three_inferred() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let classification =
            classify_news_source(&conn, "https://unknown.example/news", "Unknown Source").unwrap();

        assert_eq!(classification.domain, "unknown.example");
        assert_eq!(classification.tier, 3);
        assert!(classification.inferred);
    }

    #[test]
    fn source_independence_classifier_matches_hand_tagged_fixtures() {
        let empty_snippets: Vec<String> = Vec::new();
        let fixtures = vec![
            (
                "Fed leaves rates unchanged after two-day meeting",
                "Bloomberg Markets",
                "Reporters reviewed the statement and market reaction after the decision.",
                NewsSourceIndependence::Independent,
            ),
            (
                "(Reuters) - Oil rises as OPEC supply cuts tighten market",
                "Reuters",
                "",
                NewsSourceIndependence::Wire,
            ),
            (
                "Company says buyback was approved",
                "MarketWatch",
                "The company said in a statement that the board approved a buyback.",
                NewsSourceIndependence::Restatement,
            ),
            (
                "Treasury plan discussed by advisers",
                "CNBC",
                "The plan is being debated, according to people familiar with the matter.",
                NewsSourceIndependence::Rumor,
            ),
            (
                "Chip stocks climb after suppliers report stronger orders",
                "Financial Times",
                "Filings and interviews with suppliers point to a pickup in orders.",
                NewsSourceIndependence::Independent,
            ),
            (
                "(Bloomberg) -- Dollar slips as traders price July cut",
                "Bloomberg",
                "",
                NewsSourceIndependence::Wire,
            ),
            (
                "Minister announces tariff review",
                "The Guardian",
                "The minister wrote on X that the review would begin next week.",
                NewsSourceIndependence::Restatement,
            ),
            (
                "Bank merger talks reportedly restart",
                "Yahoo Finance",
                "Negotiations have reportedly restarted after collapsing last month.",
                NewsSourceIndependence::Rumor,
            ),
            (
                "AP - US jobless claims edge lower",
                "AP",
                "",
                NewsSourceIndependence::Wire,
            ),
            (
                "Copper inventories fall for third week",
                "Bloomberg Commodities",
                "Exchange data showed inventories falling across major warehouses.",
                NewsSourceIndependence::Independent,
            ),
            (
                "Candidate rejects currency-devaluation claim",
                "Reuters",
                "The campaign said in a press release that the claim was false.",
                NewsSourceIndependence::Restatement,
            ),
            (
                "Exchange explores new listing rules",
                "CoinDesk",
                "Anonymous sources said the exchange is weighing a rule change.",
                NewsSourceIndependence::Rumor,
            ),
            (
                "Inflation swaps move after CPI surprise",
                "Wall Street Journal",
                "Market pricing moved after the CPI release beat consensus forecasts.",
                NewsSourceIndependence::Independent,
            ),
            (
                "Company denies financing stress",
                "Seeking Alpha",
                "The statement said liquidity remained strong.",
                NewsSourceIndependence::Restatement,
            ),
            (
                "Bonds rally as growth fears deepen",
                "Reuters",
                "Reuters - Bonds rallied as investors bought duration.",
                NewsSourceIndependence::Wire,
            ),
            (
                "Crypto lender weighs sale",
                "The Block",
                "A person familiar with the matter said a sale is being considered.",
                NewsSourceIndependence::Rumor,
            ),
            (
                "Gold ETF holdings rise with real yields lower",
                "Financial Times",
                "Fund-flow data compiled by the outlet showed two weeks of inflows.",
                NewsSourceIndependence::Independent,
            ),
            (
                "Prime minister comments on sanctions",
                "BBC",
                "The prime minister said on Twitter that sanctions would remain.",
                NewsSourceIndependence::Restatement,
            ),
            (
                "Markets steady before Fed minutes",
                "CNBC",
                "Traders awaited minutes while equity futures held narrow ranges.",
                NewsSourceIndependence::Independent,
            ),
            (
                "Central bank leadership change considered",
                "Bloomberg",
                "The change is being discussed, according to sources.",
                NewsSourceIndependence::Rumor,
            ),
        ];

        let mut correct = 0usize;
        for (title, source, description, expected) in &fixtures {
            let actual = classify_news_source_independence(
                title,
                source,
                Some(description),
                &empty_snippets,
            );
            if actual == *expected {
                correct += 1;
            }
        }

        let accuracy = correct as f64 / fixtures.len() as f64;
        assert!(
            accuracy >= 0.80,
            "classifier accuracy {accuracy:.2} below 80% on fixture set"
        );
    }

    #[test]
    fn source_tier_backfill_updates_existing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO news_cache
             (title, url, source, category, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "Fed wire",
                "https://www.reuters.com/markets/fed",
                "Reuters",
                "macro",
                1709610000
            ],
        )
        .unwrap();

        ensure_source_tier_tables(&conn).unwrap();
        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items[0].source_domain, "reuters.com");
        assert_eq!(items[0].source_tier, 1);
        assert!(!items[0].source_tier_inferred);
        assert_eq!(items[0].source_independence, NewsSourceIndependence::Wire);
    }

    #[test]
    fn source_tier_set_and_remove_manage_reference_rows() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let row = set_news_source_tier(&conn, "blog.example", 4, Some("unverified")).unwrap();
        assert_eq!(row.domain, "blog.example");
        assert_eq!(row.tier, 4);

        let classification =
            classify_news_source(&conn, "https://blog.example/post", "Blog Example").unwrap();
        assert_eq!(classification.tier, 4);
        assert!(!classification.inferred);

        assert!(remove_news_source_tier(&conn, "blog.example").unwrap());
        let classification =
            classify_news_source(&conn, "https://blog.example/post", "Blog Example").unwrap();
        assert_eq!(classification.tier, 3);
        assert!(classification.inferred);
    }

    #[test]
    fn test_deduplication() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "News 1",
            "https://example.com/a",
            "Reuters",
            "macro",
            1709610000,
        )
        .unwrap();

        // Try inserting same URL again
        insert_news(
            &conn,
            "News 1 (duplicate)",
            "https://example.com/a",
            "Reuters",
            "macro",
            1709610001,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1); // Only one entry, duplicate ignored
    }

    #[test]
    fn filters_by_source_independence() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news_with_source_type(
            &conn,
            "Independent market report",
            "https://example.com/independent",
            "Bloomberg Markets",
            "rss",
            None,
            "markets",
            1709610000,
            Some("Exchange data showed inventories falling."),
            &[],
        )
        .unwrap();
        insert_news_with_source_type(
            &conn,
            "Company comments on buyback",
            "https://example.com/restatement",
            "MarketWatch",
            "rss",
            None,
            "markets",
            1709610001,
            Some("The company said in a statement that the buyback was approved."),
            &[],
        )
        .unwrap();

        let items = get_latest_news_filtered(
            &conn,
            10,
            None,
            None,
            None,
            None,
            Some(&[NewsSourceIndependence::Independent]),
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].source_independence,
            NewsSourceIndependence::Independent
        );
    }

    #[test]
    fn test_filter_by_source() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "Reuters headline",
            "https://example.com/r1",
            "Reuters",
            "macro",
            1709610000,
        )
        .unwrap();
        insert_news(
            &conn,
            "CoinDesk headline",
            "https://example.com/c1",
            "CoinDesk",
            "crypto",
            1709610000,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, Some("CoinDesk"), None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, "CoinDesk");
    }

    #[test]
    fn test_search_term() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news(
            &conn,
            "Bitcoin rally continues",
            "https://example.com/btc",
            "CoinDesk",
            "crypto",
            1709610000,
        )
        .unwrap();
        insert_news(
            &conn,
            "Gold prices drop",
            "https://example.com/gold",
            "Reuters",
            "commodities",
            1709610000,
        )
        .unwrap();

        let items = get_latest_news(&conn, 10, None, None, Some("Bitcoin"), None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Bitcoin rally continues");
    }

    #[test]
    fn test_cleanup_old_news() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let now = chrono::Utc::now().timestamp();
        let three_days_ago = now - (3 * 24 * 3600);

        insert_news(
            &conn,
            "Old news",
            "https://example.com/old",
            "Reuters",
            "macro",
            three_days_ago,
        )
        .unwrap();

        insert_news(
            &conn,
            "Fresh news",
            "https://example.com/fresh",
            "Reuters",
            "macro",
            now,
        )
        .unwrap();

        let deleted = cleanup_old_news(&conn).unwrap();
        assert_eq!(deleted, 1);

        let items = get_latest_news(&conn, 10, None, None, None, None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Fresh news");
    }

    #[test]
    fn test_latest_fetched_at_by_source_type() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        insert_news_with_source_type(
            &conn,
            "Brave headline",
            "https://example.com/brave",
            "Brave",
            "brave",
            None,
            "markets",
            1709610000,
            None,
            &[],
        )
        .unwrap();
        insert_news(
            &conn,
            "RSS headline",
            "https://example.com/rss",
            "Reuters",
            "markets",
            1709610000,
        )
        .unwrap();

        assert!(latest_fetched_at_by_source_type(&conn, "brave")
            .unwrap()
            .is_some());
        assert!(latest_fetched_at_by_source_type(&conn, "rss")
            .unwrap()
            .is_some());
        assert!(latest_fetched_at_by_source_type(&conn, "missing")
            .unwrap()
            .is_none());
    }
}
