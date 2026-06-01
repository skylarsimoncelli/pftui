use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::{news_cache, query};

pub const VALID_TOPICS: &[&str] = &[
    "fed",
    "inflation",
    "geopolitics",
    "commodities",
    "crypto",
    "equities",
    "other",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewsSourceAccuracyRow {
    pub source_domain: String,
    pub topic: String,
    pub n_predictions_implied: i64,
    pub n_correct: i64,
    pub n_wrong: i64,
    pub n_partial: i64,
    pub last_updated: String,
    pub hit_rate_pct: f64,
    pub weight: f64,
}

#[derive(Debug, Clone)]
struct AccuracyEvent {
    source_domain: String,
    topic: String,
    outcome: String,
}

pub fn normalize_topic(value: Option<&str>) -> Result<String> {
    let raw = value.unwrap_or("other").trim().to_ascii_lowercase();
    let topic = match raw.as_str() {
        "" | "other" => "other",
        "fed" | "fomc" | "rate" | "rates" | "fed-policy" | "fed_policy" => "fed",
        "inflation" | "cpi" | "ppi" | "pce" | "prices" => "inflation",
        "geopolitics" | "geopolitical" | "iran" | "hormuz" | "war" | "conflict"
        | "china" | "russia" | "ukraine" | "middle-east" | "middle_east" => "geopolitics",
        "commodities" | "commodity" | "oil" | "gold" | "silver" | "copper" | "energy" => {
            "commodities"
        }
        "crypto" | "btc" | "bitcoin" | "eth" | "ethereum" | "sol" | "solana" => "crypto",
        "equities" | "equity" | "stocks" | "stock" | "spx" | "s&p" | "nasdaq" | "qqq" => {
            "equities"
        }
        _ => bail!(
            "invalid news prediction topic '{}'. Valid: {}. Common aliases such as iran->geopolitics, cpi->inflation, btc->crypto are accepted.",
            raw,
            VALID_TOPICS.join(", ")
        ),
    };
    Ok(topic.to_string())
}

fn normalize_domain_filter(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        news_cache::normalize_source_domain(trimmed).or_else(|| {
            Some(
                trimmed
                    .trim_start_matches("www.")
                    .trim_matches('.')
                    .to_ascii_lowercase(),
            )
            .filter(|domain| !domain.is_empty())
        })
    })
}

fn validate_outcome(value: &str) -> Result<()> {
    match value {
        "pending" | "correct" | "partial" | "wrong" => Ok(()),
        _ => bail!("invalid prediction outcome '{}'", value),
    }
}

fn outcome_counts(outcome: &str) -> (i64, i64, i64) {
    match outcome {
        "correct" => (1, 0, 0),
        "wrong" => (0, 1, 0),
        "partial" => (0, 0, 1),
        _ => (0, 0, 0),
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn row_from_counts(
    source_domain: String,
    topic: String,
    n_predictions_implied: i64,
    n_correct: i64,
    n_wrong: i64,
    n_partial: i64,
    last_updated: String,
) -> NewsSourceAccuracyRow {
    let scored = n_correct + n_wrong + n_partial;
    let hit_rate = if scored > 0 {
        ((n_correct as f64) + 0.5 * (n_partial as f64)) / scored as f64
    } else {
        0.0
    };
    NewsSourceAccuracyRow {
        source_domain,
        topic,
        n_predictions_implied,
        n_correct,
        n_wrong,
        n_partial,
        last_updated,
        hit_rate_pct: round2(hit_rate * 100.0),
        weight: round2(hit_rate),
    }
}

pub fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS news_source_accuracy (
            source_domain TEXT NOT NULL,
            topic TEXT NOT NULL
                CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
            n_predictions_implied INTEGER NOT NULL DEFAULT 0,
            n_correct INTEGER NOT NULL DEFAULT 0,
            n_wrong INTEGER NOT NULL DEFAULT 0,
            n_partial INTEGER NOT NULL DEFAULT 0,
            last_updated TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(source_domain, topic)
        );
        CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_topic
            ON news_source_accuracy(topic);

        CREATE TABLE IF NOT EXISTS news_source_accuracy_events (
            prediction_id INTEGER PRIMARY KEY REFERENCES user_predictions(id) ON DELETE CASCADE,
            source_article_id INTEGER REFERENCES news_cache(id),
            source_domain TEXT NOT NULL,
            topic TEXT NOT NULL
                CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
            outcome TEXT NOT NULL CHECK(outcome IN ('correct','partial','wrong')),
            scored_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_events_source
            ON news_source_accuracy_events(source_domain, topic);
        CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_events_scored
            ON news_source_accuracy_events(scored_at);",
    )?;
    Ok(())
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_source_accuracy (
                source_domain TEXT NOT NULL,
                topic TEXT NOT NULL
                    CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
                n_predictions_implied BIGINT NOT NULL DEFAULT 0,
                n_correct BIGINT NOT NULL DEFAULT 0,
                n_wrong BIGINT NOT NULL DEFAULT 0,
                n_partial BIGINT NOT NULL DEFAULT 0,
                last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY(source_domain, topic)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_topic
             ON news_source_accuracy(topic)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_source_accuracy_events (
                prediction_id BIGINT PRIMARY KEY,
                source_article_id BIGINT,
                source_domain TEXT NOT NULL,
                topic TEXT NOT NULL
                    CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
                outcome TEXT NOT NULL CHECK(outcome IN ('correct','partial','wrong')),
                scored_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_events_source
             ON news_source_accuracy_events(source_domain, topic)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_news_source_accuracy_events_scored
             ON news_source_accuracy_events(scored_at)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn lookup_source_article_domain(conn: &Connection, source_article_id: i64) -> Result<String> {
    let row = conn
        .query_row(
            "SELECT source_domain, url, source FROM news_cache WHERE id = ?1",
            params![source_article_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;

    let Some((source_domain, url, source)) = row else {
        bail!(
            "source_article_id {} does not exist in news_cache",
            source_article_id
        );
    };
    let source_domain = source_domain.trim();
    if source_domain.is_empty() {
        Ok(news_cache::source_domain_for(&url, &source))
    } else {
        Ok(source_domain.to_ascii_lowercase())
    }
}

fn get_existing_event(conn: &Connection, prediction_id: i64) -> Result<Option<AccuracyEvent>> {
    conn.query_row(
        "SELECT source_domain, topic, outcome
         FROM news_source_accuracy_events
         WHERE prediction_id = ?1",
        params![prediction_id],
        |row| {
            Ok(AccuracyEvent {
                source_domain: row.get(0)?,
                topic: row.get(1)?,
                outcome: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn apply_aggregate_delta(
    conn: &Connection,
    source_domain: &str,
    topic: &str,
    outcome: &str,
    delta: i64,
) -> Result<()> {
    let (correct, wrong, partial) = outcome_counts(outcome);
    if delta >= 0 {
        conn.execute(
            "INSERT INTO news_source_accuracy
             (source_domain, topic, n_predictions_implied, n_correct, n_wrong, n_partial, last_updated)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, datetime('now'))
             ON CONFLICT(source_domain, topic) DO UPDATE SET
                n_predictions_implied = n_predictions_implied + 1,
                n_correct = n_correct + excluded.n_correct,
                n_wrong = n_wrong + excluded.n_wrong,
                n_partial = n_partial + excluded.n_partial,
                last_updated = datetime('now')",
            params![source_domain, topic, correct, wrong, partial],
        )?;
    } else {
        conn.execute(
            "UPDATE news_source_accuracy
             SET n_predictions_implied = MAX(n_predictions_implied - 1, 0),
                 n_correct = MAX(n_correct - ?3, 0),
                 n_wrong = MAX(n_wrong - ?4, 0),
                 n_partial = MAX(n_partial - ?5, 0),
                 last_updated = datetime('now')
             WHERE source_domain = ?1 AND topic = ?2",
            params![source_domain, topic, correct, wrong, partial],
        )?;
        conn.execute(
            "DELETE FROM news_source_accuracy
             WHERE source_domain = ?1
               AND topic = ?2
               AND n_predictions_implied = 0
               AND n_correct = 0
               AND n_wrong = 0
               AND n_partial = 0",
            params![source_domain, topic],
        )?;
    }
    Ok(())
}

pub fn sync_prediction_outcome(
    conn: &Connection,
    prediction_id: i64,
    source_article_id: Option<i64>,
    topic: &str,
    outcome: &str,
) -> Result<()> {
    ensure_tables(conn)?;
    validate_outcome(outcome)?;

    if let Some(existing) = get_existing_event(conn, prediction_id)? {
        apply_aggregate_delta(
            conn,
            &existing.source_domain,
            &existing.topic,
            &existing.outcome,
            -1,
        )?;
        conn.execute(
            "DELETE FROM news_source_accuracy_events WHERE prediction_id = ?1",
            params![prediction_id],
        )?;
    }

    if outcome == "pending" {
        return Ok(());
    }

    let Some(article_id) = source_article_id else {
        return Ok(());
    };
    if article_id <= 0 {
        bail!("source_article_id must be positive");
    }

    let topic = normalize_topic(Some(topic))?;
    let source_domain = lookup_source_article_domain(conn, article_id)?;
    apply_aggregate_delta(conn, &source_domain, &topic, outcome, 1)?;
    conn.execute(
        "INSERT INTO news_source_accuracy_events
         (prediction_id, source_article_id, source_domain, topic, outcome, scored_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        params![prediction_id, article_id, source_domain, topic, outcome],
    )?;
    Ok(())
}

pub fn list_accuracy(
    conn: &Connection,
    domain: Option<&str>,
    topic: Option<&str>,
    window_days: Option<i64>,
) -> Result<Vec<NewsSourceAccuracyRow>> {
    ensure_tables(conn)?;
    let domain = normalize_domain_filter(domain);
    let topic = topic
        .map(|value| normalize_topic(Some(value)))
        .transpose()?;

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut sql = if window_days.is_some() {
        "SELECT source_domain, topic,
                COUNT(*) AS n_predictions_implied,
                SUM(CASE WHEN outcome = 'correct' THEN 1 ELSE 0 END) AS n_correct,
                SUM(CASE WHEN outcome = 'wrong' THEN 1 ELSE 0 END) AS n_wrong,
                SUM(CASE WHEN outcome = 'partial' THEN 1 ELSE 0 END) AS n_partial,
                MAX(scored_at) AS last_updated
         FROM news_source_accuracy_events
         WHERE 1=1"
            .to_string()
    } else {
        "SELECT source_domain, topic, n_predictions_implied, n_correct, n_wrong, n_partial, last_updated
         FROM news_source_accuracy
         WHERE 1=1"
            .to_string()
    };

    if let Some(domain) = domain {
        sql.push_str(" AND source_domain = ?");
        params_vec.push(Box::new(domain));
    }
    if let Some(topic) = topic {
        sql.push_str(" AND topic = ?");
        params_vec.push(Box::new(topic));
    }
    if let Some(days) = window_days {
        if days <= 0 {
            bail!("window-days must be positive");
        }
        sql.push_str(" AND datetime(scored_at) >= datetime('now', ?)");
        params_vec.push(Box::new(format!("-{} days", days)));
    }
    if window_days.is_some() {
        sql.push_str(" GROUP BY source_domain, topic");
    }
    sql.push_str(" ORDER BY topic ASC, source_domain ASC");

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(row_from_counts(
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    })?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn rank_sources(
    conn: &Connection,
    topic: Option<&str>,
    window_days: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<NewsSourceAccuracyRow>> {
    let mut rows = list_accuracy(conn, None, topic, window_days)?;
    rows.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.n_predictions_implied.cmp(&a.n_predictions_implied))
            .then_with(|| a.source_domain.cmp(&b.source_domain))
    });
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
    Ok(rows)
}

pub fn list_accuracy_backend(
    backend: &BackendConnection,
    domain: Option<&str>,
    topic: Option<&str>,
    window_days: Option<i64>,
) -> Result<Vec<NewsSourceAccuracyRow>> {
    query::dispatch(
        backend,
        |conn| list_accuracy(conn, domain, topic, window_days),
        |pool| list_accuracy_postgres(pool, domain, topic, window_days),
    )
}

pub fn rank_sources_backend(
    backend: &BackendConnection,
    topic: Option<&str>,
    window_days: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<NewsSourceAccuracyRow>> {
    query::dispatch(
        backend,
        |conn| rank_sources(conn, topic, window_days, limit),
        |pool| rank_sources_postgres(pool, topic, window_days, limit),
    )
}

pub(crate) fn sync_prediction_outcome_postgres(
    pool: &PgPool,
    prediction_id: i64,
    source_article_id: Option<i64>,
    topic: &str,
    outcome: &str,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    validate_outcome(outcome)?;
    let topic = normalize_topic(Some(topic))?;

    crate::db::pg_runtime::block_on(async {
        let existing: Option<(String, String, String)> = sqlx::query_as(
            "SELECT source_domain, topic, outcome
             FROM news_source_accuracy_events
             WHERE prediction_id = $1",
        )
        .bind(prediction_id)
        .fetch_optional(pool)
        .await?;

        if let Some((source_domain, old_topic, old_outcome)) = existing {
            apply_aggregate_delta_postgres(pool, &source_domain, &old_topic, &old_outcome, -1)
                .await?;
            sqlx::query("DELETE FROM news_source_accuracy_events WHERE prediction_id = $1")
                .bind(prediction_id)
                .execute(pool)
                .await?;
        }

        if outcome == "pending" {
            return Ok::<(), anyhow::Error>(());
        }

        let Some(article_id) = source_article_id else {
            return Ok::<(), anyhow::Error>(());
        };
        if article_id <= 0 {
            bail!("source_article_id must be positive");
        }

        let article: Option<(String, String, String)> = sqlx::query_as(
            "SELECT source_domain, url, source
             FROM news_cache
             WHERE id = $1",
        )
        .bind(article_id)
        .fetch_optional(pool)
        .await?;

        let Some((source_domain, url, source)) = article else {
            bail!(
                "source_article_id {} does not exist in news_cache",
                article_id
            );
        };
        let source_domain = if source_domain.trim().is_empty() {
            news_cache::source_domain_for(&url, &source)
        } else {
            source_domain.trim().to_ascii_lowercase()
        };

        apply_aggregate_delta_postgres(pool, &source_domain, &topic, outcome, 1).await?;
        sqlx::query(
            "INSERT INTO news_source_accuracy_events
             (prediction_id, source_article_id, source_domain, topic, outcome, scored_at)
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(prediction_id)
        .bind(article_id)
        .bind(&source_domain)
        .bind(&topic)
        .bind(outcome)
        .execute(pool)
        .await?;

        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

#[allow(dead_code)]
pub fn sync_prediction_outcome_backend(
    backend: &BackendConnection,
    prediction_id: i64,
    source_article_id: Option<i64>,
    topic: &str,
    outcome: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| sync_prediction_outcome(conn, prediction_id, source_article_id, topic, outcome),
        |pool| {
            sync_prediction_outcome_postgres(pool, prediction_id, source_article_id, topic, outcome)
        },
    )
}

async fn apply_aggregate_delta_postgres(
    pool: &PgPool,
    source_domain: &str,
    topic: &str,
    outcome: &str,
    delta: i64,
) -> Result<()> {
    let (correct, wrong, partial) = outcome_counts(outcome);
    if delta >= 0 {
        sqlx::query(
            "INSERT INTO news_source_accuracy
             (source_domain, topic, n_predictions_implied, n_correct, n_wrong, n_partial, last_updated)
             VALUES ($1, $2, 1, $3, $4, $5, NOW())
             ON CONFLICT(source_domain, topic) DO UPDATE SET
                n_predictions_implied = news_source_accuracy.n_predictions_implied + 1,
                n_correct = news_source_accuracy.n_correct + EXCLUDED.n_correct,
                n_wrong = news_source_accuracy.n_wrong + EXCLUDED.n_wrong,
                n_partial = news_source_accuracy.n_partial + EXCLUDED.n_partial,
                last_updated = NOW()",
        )
        .bind(source_domain)
        .bind(topic)
        .bind(correct)
        .bind(wrong)
        .bind(partial)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE news_source_accuracy
             SET n_predictions_implied = GREATEST(n_predictions_implied - 1, 0),
                 n_correct = GREATEST(n_correct - $3, 0),
                 n_wrong = GREATEST(n_wrong - $4, 0),
                 n_partial = GREATEST(n_partial - $5, 0),
                 last_updated = NOW()
             WHERE source_domain = $1 AND topic = $2",
        )
        .bind(source_domain)
        .bind(topic)
        .bind(correct)
        .bind(wrong)
        .bind(partial)
        .execute(pool)
        .await?;
        sqlx::query(
            "DELETE FROM news_source_accuracy
             WHERE source_domain = $1
               AND topic = $2
               AND n_predictions_implied = 0
               AND n_correct = 0
               AND n_wrong = 0
               AND n_partial = 0",
        )
        .bind(source_domain)
        .bind(topic)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn list_accuracy_postgres(
    pool: &PgPool,
    domain: Option<&str>,
    topic: Option<&str>,
    window_days: Option<i64>,
) -> Result<Vec<NewsSourceAccuracyRow>> {
    ensure_tables_postgres(pool)?;
    let domain = normalize_domain_filter(domain);
    let topic = topic
        .map(|value| normalize_topic(Some(value)))
        .transpose()?;
    if let Some(days) = window_days {
        if days <= 0 {
            bail!("window-days must be positive");
        }
    }

    let rows: Vec<(String, String, i64, i64, i64, i64, String)> = crate::db::pg_runtime::block_on(
        async {
            if window_days.is_some() {
                sqlx::query_as(
                    "SELECT source_domain, topic,
                            COUNT(*)::BIGINT AS n_predictions_implied,
                            SUM(CASE WHEN outcome = 'correct' THEN 1 ELSE 0 END)::BIGINT AS n_correct,
                            SUM(CASE WHEN outcome = 'wrong' THEN 1 ELSE 0 END)::BIGINT AS n_wrong,
                            SUM(CASE WHEN outcome = 'partial' THEN 1 ELSE 0 END)::BIGINT AS n_partial,
                            MAX(scored_at)::text AS last_updated
                     FROM news_source_accuracy_events
                     WHERE ($1::TEXT IS NULL OR source_domain = $1)
                       AND ($2::TEXT IS NULL OR topic = $2)
                       AND ($3::BIGINT IS NULL OR scored_at >= NOW() - ($3::BIGINT * INTERVAL '1 day'))
                     GROUP BY source_domain, topic
                     ORDER BY topic ASC, source_domain ASC",
                )
                .bind(domain.as_deref())
                .bind(topic.as_deref())
                .bind(window_days)
                .fetch_all(pool)
                .await
            } else {
                sqlx::query_as(
                    "SELECT source_domain, topic, n_predictions_implied, n_correct, n_wrong, n_partial, last_updated::text
                     FROM news_source_accuracy
                     WHERE ($1::TEXT IS NULL OR source_domain = $1)
                       AND ($2::TEXT IS NULL OR topic = $2)
                     ORDER BY topic ASC, source_domain ASC",
                )
                .bind(domain.as_deref())
                .bind(topic.as_deref())
                .fetch_all(pool)
                .await
            }
        },
    )?;

    Ok(rows
        .into_iter()
        .map(|row| row_from_counts(row.0, row.1, row.2, row.3, row.4, row.5, row.6))
        .collect())
}

fn rank_sources_postgres(
    pool: &PgPool,
    topic: Option<&str>,
    window_days: Option<i64>,
    limit: Option<usize>,
) -> Result<Vec<NewsSourceAccuracyRow>> {
    let mut rows = list_accuracy_postgres(pool, None, topic, window_days)?;
    rows.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.n_predictions_implied.cmp(&a.n_predictions_implied))
            .then_with(|| a.source_domain.cmp(&b.source_domain))
    });
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
    Ok(rows)
}

/// Summary of a `rebuild-accuracy` backfill pass.
#[derive(Debug, Clone, Serialize)]
pub struct AccuracyBackfillReport {
    pub since_days: Option<i64>,
    pub scanned: usize,
    pub synced: usize,
    pub skipped_missing_article: usize,
    pub skipped_other: usize,
    pub dry_run: bool,
}

/// Walk `user_predictions` rows that have a non-pending outcome AND a
/// `source_article_id`, optionally restricted to the trailing `since_days`
/// window, and replay `sync_prediction_outcome` for each. Idempotent:
/// re-running produces no double-counting because `sync_prediction_outcome`
/// deletes any prior event row before inserting.
pub fn backfill_accuracy(
    conn: &Connection,
    since_days: Option<i64>,
    dry_run: bool,
) -> Result<AccuracyBackfillReport> {
    ensure_tables(conn)?;
    if let Some(days) = since_days {
        if days <= 0 {
            bail!("since-days must be positive");
        }
    }

    let mut sql = String::from(
        "SELECT id, topic, source_article_id, outcome
         FROM user_predictions
         WHERE source_article_id IS NOT NULL
           AND outcome IN ('correct','partial','wrong')",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(days) = since_days {
        sql.push_str(
            " AND (scored_at IS NULL
                   OR datetime(scored_at) >= datetime('now', ?))",
        );
        params_vec.push(Box::new(format!("-{} days", days)));
    }
    sql.push_str(" ORDER BY id ASC");

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut report = AccuracyBackfillReport {
        since_days,
        scanned: 0,
        synced: 0,
        skipped_missing_article: 0,
        skipped_other: 0,
        dry_run,
    };

    for (id, topic, source_article_id, outcome) in rows {
        report.scanned += 1;
        let Some(article_id) = source_article_id else {
            report.skipped_other += 1;
            continue;
        };
        if dry_run {
            // Validate inputs without mutating
            if validate_outcome(&outcome).is_err() {
                report.skipped_other += 1;
                continue;
            }
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM news_cache WHERE id = ?1",
                    params![article_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;
            if !exists {
                report.skipped_missing_article += 1;
                continue;
            }
            report.synced += 1;
            continue;
        }
        match sync_prediction_outcome(conn, id, Some(article_id), &topic, &outcome) {
            Ok(()) => report.synced += 1,
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("does not exist in news_cache") {
                    report.skipped_missing_article += 1;
                } else {
                    report.skipped_other += 1;
                }
            }
        }
    }

    Ok(report)
}

pub fn backfill_accuracy_backend(
    backend: &BackendConnection,
    since_days: Option<i64>,
    dry_run: bool,
) -> Result<AccuracyBackfillReport> {
    query::dispatch(
        backend,
        |conn| backfill_accuracy(conn, since_days, dry_run),
        |_pool| {
            bail!("rebuild-accuracy is only supported on the SQLite backend in this release")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn topic_aliases_normalize_to_fixed_enum() {
        assert_eq!(normalize_topic(Some("iran")).unwrap(), "geopolitics");
        assert_eq!(normalize_topic(Some("CPI")).unwrap(), "inflation");
        assert_eq!(normalize_topic(Some("BTC")).unwrap(), "crypto");
        assert_eq!(normalize_topic(None).unwrap(), "other");
    }

    #[test]
    fn scoring_source_attributed_prediction_updates_ledger() {
        let conn = db::open_in_memory();
        news_cache::insert_news(
            &conn,
            "Fed signals July cut",
            "https://www.bloomberg.com/news/fed-cut",
            "Bloomberg",
            "macro",
            1_709_610_000,
        )
        .unwrap();
        let article_id: i64 = conn
            .query_row(
                "SELECT id FROM news_cache WHERE url = ?1",
                params!["https://www.bloomberg.com/news/fed-cut"],
                |row| row.get(0),
            )
            .unwrap();

        let prediction_id = crate::db::user_predictions::add_prediction_with_details(
            &conn,
            "Fed odds keep rising",
            None,
            None,
            Some("medium"),
            None,
            None,
            None,
            None,
            &[],
            Some("fed"),
            Some(article_id),
        )
        .unwrap();

        sync_prediction_outcome(&conn, prediction_id, Some(article_id), "fed", "correct").unwrap();
        let rows = list_accuracy(&conn, Some("bloomberg.com"), Some("fed"), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_domain, "bloomberg.com");
        assert_eq!(rows[0].n_predictions_implied, 1);
        assert_eq!(rows[0].n_correct, 1);
        assert_eq!(rows[0].hit_rate_pct, 100.0);
        assert_eq!(rows[0].weight, 1.0);
    }

    #[test]
    fn rescoring_replaces_prior_source_accuracy_event() {
        let conn = db::open_in_memory();
        news_cache::insert_news(
            &conn,
            "Oil shock risk fades",
            "https://www.reuters.com/markets/oil",
            "Reuters",
            "macro",
            1_709_610_000,
        )
        .unwrap();
        let article_id: i64 = conn
            .query_row(
                "SELECT id FROM news_cache WHERE url = ?1",
                params!["https://www.reuters.com/markets/oil"],
                |row| row.get(0),
            )
            .unwrap();

        let prediction_id = crate::db::user_predictions::add_prediction_with_details(
            &conn,
            "Oil shock risk fades",
            None,
            None,
            Some("medium"),
            None,
            None,
            None,
            None,
            &[],
            Some("commodities"),
            Some(article_id),
        )
        .unwrap();

        sync_prediction_outcome(
            &conn,
            prediction_id,
            Some(article_id),
            "commodities",
            "wrong",
        )
        .unwrap();
        sync_prediction_outcome(
            &conn,
            prediction_id,
            Some(article_id),
            "commodities",
            "partial",
        )
        .unwrap();

        let rows = list_accuracy(&conn, Some("reuters.com"), Some("commodities"), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].n_predictions_implied, 1);
        assert_eq!(rows[0].n_wrong, 0);
        assert_eq!(rows[0].n_partial, 1);
        assert_eq!(rows[0].hit_rate_pct, 50.0);
    }

    #[test]
    fn backfill_replays_scored_predictions_and_is_idempotent() {
        let conn = db::open_in_memory();
        news_cache::insert_news(
            &conn,
            "Fed cuts coming",
            "https://www.bloomberg.com/news/fed-cut-2",
            "Bloomberg",
            "macro",
            1_709_610_000,
        )
        .unwrap();
        let article_id: i64 = conn
            .query_row(
                "SELECT id FROM news_cache WHERE url = ?1",
                params!["https://www.bloomberg.com/news/fed-cut-2"],
                |row| row.get(0),
            )
            .unwrap();

        let pid = crate::db::user_predictions::add_prediction_with_details(
            &conn,
            "Fed cuts in July",
            None,
            None,
            Some("medium"),
            None,
            None,
            None,
            None,
            &[],
            Some("fed"),
            Some(article_id),
        )
        .unwrap();
        // Direct outcome update without going through the score path — this
        // simulates a prediction that was scored before sync_prediction_outcome
        // was wired into the score path.
        conn.execute(
            "UPDATE user_predictions
             SET outcome = 'correct', scored_at = datetime('now')
             WHERE id = ?1",
            params![pid],
        )
        .unwrap();

        // Pre-backfill: ledger should be empty
        let before = list_accuracy(&conn, None, None, None).unwrap();
        assert!(before.is_empty(), "ledger should be empty before backfill");

        // Dry-run: counts scanned but does not mutate
        let dry = backfill_accuracy(&conn, None, true).unwrap();
        assert_eq!(dry.scanned, 1);
        assert_eq!(dry.synced, 1);
        let still_empty = list_accuracy(&conn, None, None, None).unwrap();
        assert!(still_empty.is_empty(), "dry-run must not mutate ledger");

        // Real run: produces one ledger row
        let real = backfill_accuracy(&conn, None, false).unwrap();
        assert_eq!(real.synced, 1);
        let after = list_accuracy(&conn, None, None, None).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].n_correct, 1);

        // Idempotency: re-running does not double-count
        let again = backfill_accuracy(&conn, None, false).unwrap();
        assert_eq!(again.synced, 1);
        let after2 = list_accuracy(&conn, None, None, None).unwrap();
        assert_eq!(after2.len(), 1);
        assert_eq!(after2[0].n_predictions_implied, 1);
        assert_eq!(after2[0].n_correct, 1);
    }
}
