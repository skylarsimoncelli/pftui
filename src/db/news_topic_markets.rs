use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::db::backend::BackendConnection;
use crate::db::query;

const DEFAULT_TOPIC: &str = "other";

const NEWS_TOPIC_MARKET_SEEDS: &[(&str, &str, Option<&str>, &str)] = &[
    (
        "iran-hormuz",
        "polymarket-iran-ceasefire-2026",
        Some("polymarket-oil-above-100-EOM"),
        "Middle East escalation and Hormuz/oil shock checks",
    ),
    (
        "fed-policy",
        "kalshi-fed-hold-jun-2026",
        Some("kalshi-fed-cut-jul-2026"),
        "Fed hold/cut path cross-checks",
    ),
    (
        "inflation",
        "kalshi-cpi-above-3-jun-2026",
        Some("polymarket-us-inflation-above-3-2026"),
        "Inflation reacceleration checks",
    ),
    (
        "oil-energy",
        "polymarket-oil-above-100-EOM",
        Some("polymarket-opec-output-cut-2026"),
        "Crude and energy supply shock checks",
    ),
    (
        "crypto",
        "polymarket-bitcoin-above-100k-2026",
        Some("polymarket-eth-above-5k-2026"),
        "Crypto directional consensus checks",
    ),
    (
        "equities",
        "polymarket-spy-ath-2026",
        Some("polymarket-nasdaq-ath-2026"),
        "Equity risk appetite checks",
    ),
    (
        "geopolitics",
        "polymarket-russia-ukraine-ceasefire-2026",
        Some("polymarket-china-taiwan-conflict-2026"),
        "General geopolitical risk checks",
    ),
    (
        "ai",
        "polymarket-openai-ipo-2026",
        Some("polymarket-ai-model-breakthrough-2026"),
        "AI-cycle consensus checks",
    ),
    (
        "macro-growth",
        "polymarket-us-recession-2026",
        Some("polymarket-gdp-negative-2026"),
        "Growth and recession consensus checks",
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewsTopicMarket {
    pub topic: String,
    pub primary_market_id: String,
    pub secondary_market_id: Option<String>,
    pub last_updated: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundNewsMarket {
    pub role: String,
    pub contract_id: String,
    pub available: bool,
    pub exchange: Option<String>,
    pub event_id: Option<String>,
    pub event_title: Option<String>,
    pub question: Option<String>,
    pub category: Option<String>,
    pub probability: Option<f64>,
    pub probability_pct: Option<f64>,
    pub volume_24h: Option<f64>,
    pub liquidity: Option<f64>,
    pub end_date: Option<String>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone)]
struct ContractSnapshot {
    contract_id: String,
    exchange: String,
    event_id: String,
    event_title: String,
    question: String,
    category: String,
    last_price: f64,
    volume_24h: f64,
    liquidity: f64,
    end_date: Option<String>,
    updated_at: i64,
}

pub fn normalize_topic(value: &str) -> Result<String> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    if normalized.is_empty() {
        bail!("news topic cannot be empty");
    }
    if normalized
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        Ok(normalized)
    } else {
        bail!(
            "invalid news topic '{}'; use lowercase letters, digits, hyphens, or spaces",
            value
        )
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn topic_haystack(
    title: &str,
    category: &str,
    description: Option<&str>,
    extra_snippets: &[String],
) -> String {
    let mut parts = vec![title.trim(), category.trim()];
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

pub fn classify_news_topic(
    title: &str,
    category: &str,
    description: Option<&str>,
    extra_snippets: &[String],
) -> String {
    let haystack = topic_haystack(title, category, description, extra_snippets);
    if haystack.trim().is_empty() {
        return DEFAULT_TOPIC.to_string();
    }

    let scored_topics: [(&str, &[&str]); 9] = [
        (
            "iran-hormuz",
            &[
                "iran",
                "hormuz",
                "strait of hormuz",
                "tehran",
                "israel strike",
                "iran ceasefire",
                "israel ceasefire",
                "gaza ceasefire",
                "middle east ceasefire",
                "red sea",
                "houthi",
                "hezbollah",
            ],
        ),
        (
            "fed-policy",
            &[
                "fed",
                "fomc",
                "powell",
                "rate cut",
                "rate hold",
                "rates unchanged",
                "fed funds",
                "treasury yields",
                "dot plot",
            ],
        ),
        (
            "inflation",
            &[
                "inflation",
                "cpi",
                "ppi",
                "pce",
                "core prices",
                "price pressure",
                "tariff",
            ],
        ),
        (
            "oil-energy",
            &["oil", "crude", "brent", "wti", "opec", "energy", "gasoline"],
        ),
        (
            "crypto",
            &[
                "bitcoin",
                "btc",
                "crypto",
                "ethereum",
                "eth",
                "stablecoin",
                "coinbase",
                "etf inflows",
            ],
        ),
        (
            "equities",
            &[
                "equities",
                "stocks",
                "s&p",
                "spx",
                "nasdaq",
                "earnings",
                "market rally",
                "wall street",
            ],
        ),
        (
            "ai",
            &[
                "artificial intelligence",
                " ai ",
                "openai",
                "nvidia",
                "chips",
                "semiconductor",
            ],
        ),
        (
            "macro-growth",
            &[
                "recession",
                "gdp",
                "growth",
                "jobless",
                "payrolls",
                "unemployment",
                "pmi",
            ],
        ),
        (
            "geopolitics",
            &[
                "geopolitics",
                "war",
                "sanctions",
                "ukraine",
                "russia",
                "china",
                "taiwan",
            ],
        ),
    ];

    let padded = format!(" {haystack} ");
    for (topic, needles) in scored_topics {
        if contains_any(&padded, needles) {
            return topic.to_string();
        }
    }

    match category.trim().to_ascii_lowercase().as_str() {
        "crypto" => "crypto",
        "commodities" | "commodity" => "oil-energy",
        "geopolitics" | "politics" => "geopolitics",
        "macro" | "economics" | "economy" => "macro-growth",
        "markets" | "market" => "equities",
        _ => DEFAULT_TOPIC,
    }
    .to_string()
}

pub fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS news_topic_markets (
            topic TEXT PRIMARY KEY,
            primary_market_id TEXT NOT NULL,
            secondary_market_id TEXT,
            last_updated TEXT NOT NULL DEFAULT (datetime('now')),
            notes TEXT
        );
        CREATE TABLE IF NOT EXISTS news_topic_market_seed_state (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            seeded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    seed_defaults(conn)?;
    Ok(())
}

fn seed_defaults(conn: &Connection) -> Result<()> {
    let seeded: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM news_topic_market_seed_state WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if seeded {
        return Ok(());
    }

    for (topic, primary, secondary, notes) in NEWS_TOPIC_MARKET_SEEDS {
        conn.execute(
            "INSERT OR IGNORE INTO news_topic_markets
             (topic, primary_market_id, secondary_market_id, last_updated, notes)
             VALUES (?1, ?2, ?3, datetime('now'), ?4)",
            params![topic, primary, secondary, notes],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO news_topic_market_seed_state (id, seeded_at)
         VALUES (1, datetime('now'))",
        [],
    )?;
    Ok(())
}

pub fn ensure_news_cache_topic_column(conn: &Connection) -> Result<()> {
    let has_topic: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = 'topic'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_topic {
        conn.execute_batch(
            "ALTER TABLE news_cache ADD COLUMN topic TEXT NOT NULL DEFAULT 'other'",
        )?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_news_topic ON news_cache(topic);")?;
    Ok(())
}

pub fn backfill_news_cache_topics(conn: &Connection) -> Result<()> {
    ensure_news_cache_topic_column(conn)?;
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, title, category, description, extra_snippets
             FROM news_cache
             WHERE topic = '' OR topic = 'other'",
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

    for (id, title, category, description, snippets_json) in rows {
        let snippets = serde_json::from_str::<Vec<String>>(&snippets_json).unwrap_or_default();
        let topic = classify_news_topic(&title, &category, Some(&description), &snippets);
        conn.execute(
            "UPDATE news_cache SET topic = ?1 WHERE id = ?2",
            params![topic, id],
        )?;
    }
    Ok(())
}

pub fn list_topic_markets(conn: &Connection) -> Result<Vec<NewsTopicMarket>> {
    ensure_tables(conn)?;
    let mut stmt = conn.prepare(
        "SELECT topic, primary_market_id, secondary_market_id, last_updated, notes
         FROM news_topic_markets
         ORDER BY topic ASC",
    )?;
    let rows = stmt.query_map([], topic_market_from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn topic_market_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NewsTopicMarket> {
    Ok(NewsTopicMarket {
        topic: row.get(0)?,
        primary_market_id: row.get(1)?,
        secondary_market_id: row.get(2)?,
        last_updated: row.get(3)?,
        notes: row.get(4)?,
    })
}

pub fn set_topic_market(
    conn: &Connection,
    topic: &str,
    primary_market_id: &str,
    secondary_market_id: Option<&str>,
    notes: Option<&str>,
) -> Result<NewsTopicMarket> {
    ensure_tables(conn)?;
    let topic = normalize_topic(topic)?;
    let primary_market_id = primary_market_id.trim();
    if primary_market_id.is_empty() {
        bail!("primary market id cannot be empty");
    }
    let secondary_market_id = secondary_market_id
        .map(str::trim)
        .filter(|value| !value.is_empty());

    conn.execute(
        "INSERT INTO news_topic_markets
         (topic, primary_market_id, secondary_market_id, last_updated, notes)
         VALUES (?1, ?2, ?3, datetime('now'), ?4)
         ON CONFLICT(topic) DO UPDATE SET
            primary_market_id = excluded.primary_market_id,
            secondary_market_id = excluded.secondary_market_id,
            notes = excluded.notes,
            last_updated = datetime('now')",
        params![topic, primary_market_id, secondary_market_id, notes],
    )?;

    get_topic_market(conn, &topic)?
        .ok_or_else(|| anyhow::anyhow!("failed to read topic-market mapping after write"))
}

pub fn remove_topic_market(conn: &Connection, topic: &str) -> Result<bool> {
    ensure_tables(conn)?;
    let topic = normalize_topic(topic)?;
    let deleted = conn.execute(
        "DELETE FROM news_topic_markets WHERE topic = ?1",
        params![topic],
    )?;
    Ok(deleted > 0)
}

fn get_topic_market(conn: &Connection, topic: &str) -> Result<Option<NewsTopicMarket>> {
    conn.query_row(
        "SELECT topic, primary_market_id, secondary_market_id, last_updated, notes
         FROM news_topic_markets
         WHERE topic = ?1",
        params![topic],
        topic_market_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn bound_markets_by_topic(
    conn: &Connection,
    topics: &[String],
) -> Result<HashMap<String, Vec<BoundNewsMarket>>> {
    ensure_tables(conn)?;
    let unique_topics: Vec<String> = topics
        .iter()
        .filter_map(|topic| normalize_topic(topic).ok())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if unique_topics.is_empty() {
        return Ok(HashMap::new());
    }

    let mappings = list_topic_markets(conn)?;
    let mapping_by_topic: HashMap<_, _> = mappings
        .into_iter()
        .filter(|mapping| unique_topics.contains(&mapping.topic))
        .map(|mapping| (mapping.topic.clone(), mapping))
        .collect();
    if mapping_by_topic.is_empty() {
        return Ok(HashMap::new());
    }

    let mut contract_ids = HashSet::new();
    for mapping in mapping_by_topic.values() {
        contract_ids.insert(mapping.primary_market_id.clone());
        if let Some(secondary) = &mapping.secondary_market_id {
            contract_ids.insert(secondary.clone());
        }
    }
    let contract_map = contract_snapshots(conn, &contract_ids.into_iter().collect::<Vec<_>>())?;

    let mut result = HashMap::new();
    for topic in unique_topics {
        let Some(mapping) = mapping_by_topic.get(&topic) else {
            continue;
        };
        let mut bound = Vec::new();
        bound.push(bound_market(
            "primary",
            &mapping.primary_market_id,
            contract_map.get(&mapping.primary_market_id),
        ));
        if let Some(secondary) = &mapping.secondary_market_id {
            bound.push(bound_market(
                "secondary",
                secondary,
                contract_map.get(secondary),
            ));
        }
        result.insert(topic, bound);
    }
    Ok(result)
}

fn contract_snapshots(
    conn: &Connection,
    contract_ids: &[String],
) -> Result<HashMap<String, ContractSnapshot>> {
    let mut result = HashMap::new();
    if contract_ids.is_empty() {
        return Ok(result);
    }

    let mut sql = String::from(
        "SELECT contract_id, exchange, event_id, event_title, question, category,
                last_price, volume_24h, liquidity, end_date, updated_at
         FROM prediction_market_contracts
         WHERE contract_id IN (",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    for (idx, contract_id) in contract_ids.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
        params_vec.push(Box::new(contract_id.clone()));
    }
    sql.push(')');
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(ContractSnapshot {
            contract_id: row.get(0)?,
            exchange: row.get(1)?,
            event_id: row.get(2)?,
            event_title: row.get(3)?,
            question: row.get(4)?,
            category: row.get(5)?,
            last_price: row.get(6)?,
            volume_24h: row.get(7)?,
            liquidity: row.get(8)?,
            end_date: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    for row in rows {
        let snapshot = row?;
        result.insert(snapshot.contract_id.clone(), snapshot);
    }
    Ok(result)
}

fn bound_market(
    role: &str,
    contract_id: &str,
    snapshot: Option<&ContractSnapshot>,
) -> BoundNewsMarket {
    if let Some(snapshot) = snapshot {
        BoundNewsMarket {
            role: role.to_string(),
            contract_id: contract_id.to_string(),
            available: true,
            exchange: Some(snapshot.exchange.clone()),
            event_id: Some(snapshot.event_id.clone()),
            event_title: Some(snapshot.event_title.clone()),
            question: Some(snapshot.question.clone()),
            category: Some(snapshot.category.clone()),
            probability: Some(snapshot.last_price),
            probability_pct: Some(snapshot.last_price * 100.0),
            volume_24h: Some(snapshot.volume_24h),
            liquidity: Some(snapshot.liquidity),
            end_date: snapshot.end_date.clone(),
            updated_at: Some(snapshot.updated_at),
        }
    } else {
        BoundNewsMarket {
            role: role.to_string(),
            contract_id: contract_id.to_string(),
            available: false,
            exchange: None,
            event_id: None,
            event_title: None,
            question: None,
            category: None,
            probability: None,
            probability_pct: None,
            volume_24h: None,
            liquidity: None,
            end_date: None,
            updated_at: None,
        }
    }
}

pub fn list_topic_markets_backend(backend: &BackendConnection) -> Result<Vec<NewsTopicMarket>> {
    query::dispatch(backend, list_topic_markets, list_topic_markets_postgres)
}

pub fn set_topic_market_backend(
    backend: &BackendConnection,
    topic: &str,
    primary_market_id: &str,
    secondary_market_id: Option<&str>,
    notes: Option<&str>,
) -> Result<NewsTopicMarket> {
    query::dispatch(
        backend,
        |conn| set_topic_market(conn, topic, primary_market_id, secondary_market_id, notes),
        |pool| {
            set_topic_market_postgres(pool, topic, primary_market_id, secondary_market_id, notes)
        },
    )
}

pub fn remove_topic_market_backend(backend: &BackendConnection, topic: &str) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| remove_topic_market(conn, topic),
        |pool| remove_topic_market_postgres(pool, topic),
    )
}

pub fn bound_markets_by_topic_backend(
    backend: &BackendConnection,
    topics: &[String],
) -> Result<HashMap<String, Vec<BoundNewsMarket>>> {
    query::dispatch(
        backend,
        |conn| bound_markets_by_topic(conn, topics),
        |pool| bound_markets_by_topic_postgres(pool, topics),
    )
}

pub fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_topic_markets (
                topic TEXT PRIMARY KEY,
                primary_market_id TEXT NOT NULL,
                secondary_market_id TEXT,
                last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                notes TEXT
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_topic_market_seed_state (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                seeded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        seed_defaults_postgres(pool).await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

async fn seed_defaults_postgres(pool: &PgPool) -> Result<()> {
    let seeded: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM news_topic_market_seed_state WHERE id = 1)",
    )
    .fetch_one(pool)
    .await?;
    if seeded {
        return Ok(());
    }

    for (topic, primary, secondary, notes) in NEWS_TOPIC_MARKET_SEEDS {
        sqlx::query(
            "INSERT INTO news_topic_markets
             (topic, primary_market_id, secondary_market_id, last_updated, notes)
             VALUES ($1, $2, $3, NOW(), $4)
             ON CONFLICT(topic) DO NOTHING",
        )
        .bind(topic)
        .bind(primary)
        .bind(secondary)
        .bind(notes)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO news_topic_market_seed_state (id, seeded_at)
         VALUES (1, NOW())
         ON CONFLICT(id) DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub fn ensure_news_cache_topic_column_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "ALTER TABLE news_cache ADD COLUMN IF NOT EXISTS topic TEXT NOT NULL DEFAULT 'other'",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_news_topic ON news_cache(topic)")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

pub fn backfill_news_cache_topics_postgres(pool: &PgPool) -> Result<()> {
    ensure_news_cache_topic_column_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT id, title, category, description, extra_snippets
             FROM news_cache
             WHERE topic = '' OR topic = 'other'",
        )
        .fetch_all(pool)
        .await?;

        for row in rows {
            let id: i64 = row.try_get("id")?;
            let title: String = row.try_get("title")?;
            let category: String = row.try_get("category")?;
            let description: String = row.try_get("description")?;
            let snippets_json: String = row.try_get("extra_snippets")?;
            let snippets = serde_json::from_str::<Vec<String>>(&snippets_json).unwrap_or_default();
            let topic = classify_news_topic(&title, &category, Some(&description), &snippets);
            sqlx::query("UPDATE news_cache SET topic = $1 WHERE id = $2")
                .bind(topic)
                .bind(id)
                .execute(pool)
                .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_topic_markets_postgres(pool: &PgPool) -> Result<Vec<NewsTopicMarket>> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT topic, primary_market_id, secondary_market_id, last_updated::text, notes
             FROM news_topic_markets
             ORDER BY topic ASC",
        )
        .fetch_all(pool)
        .await
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(NewsTopicMarket {
                topic: row.try_get(0)?,
                primary_market_id: row.try_get(1)?,
                secondary_market_id: row.try_get(2)?,
                last_updated: row.try_get(3)?,
                notes: row.try_get(4)?,
            })
        })
        .collect()
}

fn set_topic_market_postgres(
    pool: &PgPool,
    topic: &str,
    primary_market_id: &str,
    secondary_market_id: Option<&str>,
    notes: Option<&str>,
) -> Result<NewsTopicMarket> {
    ensure_tables_postgres(pool)?;
    let topic = normalize_topic(topic)?;
    let primary_market_id = primary_market_id.trim();
    if primary_market_id.is_empty() {
        bail!("primary market id cannot be empty");
    }
    let secondary_market_id = secondary_market_id
        .map(str::trim)
        .filter(|value| !value.is_empty());

    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO news_topic_markets
             (topic, primary_market_id, secondary_market_id, last_updated, notes)
             VALUES ($1, $2, $3, NOW(), $4)
             ON CONFLICT(topic) DO UPDATE SET
                primary_market_id = EXCLUDED.primary_market_id,
                secondary_market_id = EXCLUDED.secondary_market_id,
                notes = EXCLUDED.notes,
                last_updated = NOW()",
        )
        .bind(&topic)
        .bind(primary_market_id)
        .bind(secondary_market_id)
        .bind(notes)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;

    get_topic_market_postgres(pool, &topic)?
        .ok_or_else(|| anyhow::anyhow!("failed to read topic-market mapping after write"))
}

fn remove_topic_market_postgres(pool: &PgPool, topic: &str) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let topic = normalize_topic(topic)?;
    let deleted = crate::db::pg_runtime::block_on(async {
        let result = sqlx::query("DELETE FROM news_topic_markets WHERE topic = $1")
            .bind(topic)
            .execute(pool)
            .await?;
        Ok::<_, sqlx::Error>(result.rows_affected() > 0)
    })?;
    Ok(deleted)
}

fn get_topic_market_postgres(pool: &PgPool, topic: &str) -> Result<Option<NewsTopicMarket>> {
    let row = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT topic, primary_market_id, secondary_market_id, last_updated::text, notes
             FROM news_topic_markets
             WHERE topic = $1",
        )
        .bind(topic)
        .fetch_optional(pool)
        .await
    })?;

    row.map(|row| {
        Ok(NewsTopicMarket {
            topic: row.try_get(0)?,
            primary_market_id: row.try_get(1)?,
            secondary_market_id: row.try_get(2)?,
            last_updated: row.try_get(3)?,
            notes: row.try_get(4)?,
        })
    })
    .transpose()
}

fn bound_markets_by_topic_postgres(
    pool: &PgPool,
    topics: &[String],
) -> Result<HashMap<String, Vec<BoundNewsMarket>>> {
    ensure_tables_postgres(pool)?;
    let unique_topics: Vec<String> = topics
        .iter()
        .filter_map(|topic| normalize_topic(topic).ok())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if unique_topics.is_empty() {
        return Ok(HashMap::new());
    }

    let mappings = list_topic_markets_postgres(pool)?;
    let mapping_by_topic: HashMap<_, _> = mappings
        .into_iter()
        .filter(|mapping| unique_topics.contains(&mapping.topic))
        .map(|mapping| (mapping.topic.clone(), mapping))
        .collect();
    if mapping_by_topic.is_empty() {
        return Ok(HashMap::new());
    }

    let mut contract_ids = HashSet::new();
    for mapping in mapping_by_topic.values() {
        contract_ids.insert(mapping.primary_market_id.clone());
        if let Some(secondary) = &mapping.secondary_market_id {
            contract_ids.insert(secondary.clone());
        }
    }
    let contract_ids = contract_ids.into_iter().collect::<Vec<_>>();
    let snapshots = contract_snapshots_postgres(pool, &contract_ids)?;

    let mut result = HashMap::new();
    for topic in unique_topics {
        let Some(mapping) = mapping_by_topic.get(&topic) else {
            continue;
        };
        let mut bound = Vec::new();
        bound.push(bound_market(
            "primary",
            &mapping.primary_market_id,
            snapshots.get(&mapping.primary_market_id),
        ));
        if let Some(secondary) = &mapping.secondary_market_id {
            bound.push(bound_market(
                "secondary",
                secondary,
                snapshots.get(secondary),
            ));
        }
        result.insert(topic, bound);
    }
    Ok(result)
}

fn contract_snapshots_postgres(
    pool: &PgPool,
    contract_ids: &[String],
) -> Result<HashMap<String, ContractSnapshot>> {
    if contract_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT contract_id, exchange, event_id, event_title, question, category,
                    last_price, volume_24h, liquidity, end_date, updated_at
             FROM prediction_market_contracts
             WHERE contract_id = ANY($1)",
        )
        .bind(contract_ids)
        .fetch_all(pool)
        .await
    })?;

    let mut result = HashMap::new();
    for row in rows {
        let snapshot = ContractSnapshot {
            contract_id: row.try_get(0)?,
            exchange: row.try_get(1)?,
            event_id: row.try_get(2)?,
            event_title: row.try_get(3)?,
            question: row.try_get(4)?,
            category: row.try_get(5)?,
            last_price: row.try_get(6)?,
            volume_24h: row.try_get(7)?,
            liquidity: row.try_get(8)?,
            end_date: row.try_get(9)?,
            updated_at: row.try_get(10)?,
        };
        result.insert(snapshot.contract_id.clone(), snapshot);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::run_migrations;

    #[test]
    fn topic_classifier_matches_core_fixtures() {
        assert_eq!(
            classify_news_topic(
                "Oil jumps after Iran threatens Hormuz shipping",
                "geopolitics",
                None,
                &[]
            ),
            "iran-hormuz"
        );
        assert_eq!(
            classify_news_topic("Fed leaves rates unchanged", "macro", None, &[]),
            "fed-policy"
        );
        assert_eq!(
            classify_news_topic(
                "CPI surprise revives inflation concerns",
                "macro",
                None,
                &[]
            ),
            "inflation"
        );
        assert_eq!(
            classify_news_topic(
                "Bitcoin ETF inflows lift crypto markets",
                "crypto",
                None,
                &[]
            ),
            "crypto"
        );
        assert_eq!(
            classify_news_topic(
                "Ukraine ceasefire talks resume after overnight strikes",
                "geopolitics",
                None,
                &[]
            ),
            "geopolitics"
        );
    }

    #[test]
    fn list_set_remove_topic_market_roundtrips() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let row = set_topic_market(
            &conn,
            "custom topic",
            "contract-primary",
            Some("contract-secondary"),
            Some("operator mapping"),
        )
        .unwrap();
        assert_eq!(row.topic, "custom-topic");
        assert_eq!(row.primary_market_id, "contract-primary");
        assert_eq!(
            row.secondary_market_id.as_deref(),
            Some("contract-secondary")
        );

        let rows = list_topic_markets(&conn).unwrap();
        assert!(rows.iter().any(|row| row.topic == "custom-topic"));
        assert!(remove_topic_market(&conn, "custom-topic").unwrap());
    }

    #[test]
    fn bound_markets_include_cached_pricing_and_missing_contracts() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO prediction_market_contracts
             (contract_id, exchange, event_id, event_title, question, category, last_price, volume_24h, liquidity, end_date, updated_at)
             VALUES (?1, 'polymarket', 'evt-1', 'Fed decision', 'Will the Fed hold?', 'economics', 0.62, 1000.0, 5000.0, NULL, 1711670000)",
            params!["fed-contract"],
        )
        .unwrap();
        set_topic_market(
            &conn,
            "fed-policy",
            "fed-contract",
            Some("missing-contract"),
            None,
        )
        .unwrap();

        let topics = vec!["fed-policy".to_string()];
        let bound = bound_markets_by_topic(&conn, &topics).unwrap();
        let rows = bound.get("fed-policy").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].available);
        assert_eq!(rows[0].probability_pct, Some(62.0));
        assert!(!rows[1].available);
    }
}
