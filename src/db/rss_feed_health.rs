use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use sqlx::{PgPool, Row};

use crate::data::rss::RssFeed;
use crate::db::backend::BackendConnection;
use crate::db::query;

pub const DEGRADED_THRESHOLD: i64 = 5;
pub const DISABLED_THRESHOLD: i64 = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RssFeedHealth {
    pub feed_id: String,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_reason: Option<String>,
    pub consecutive_failures: i64,
    pub total_failures: i64,
    pub total_successes: i64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RssFeedHealthView {
    pub feed_id: String,
    pub feed_name: String,
    pub feed_url: String,
    pub category: String,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_reason: Option<String>,
    pub consecutive_failures: i64,
    pub total_failures: i64,
    pub total_successes: i64,
    pub status: String,
}

pub fn create_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS rss_feed_health (
            feed_id TEXT PRIMARY KEY,
            last_success_at TEXT,
            last_failure_at TEXT,
            last_failure_reason TEXT,
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            total_failures INTEGER NOT NULL DEFAULT 0,
            total_successes INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active', 'degraded', 'disabled'))
        );
        CREATE INDEX IF NOT EXISTS idx_rss_feed_health_status ON rss_feed_health(status);",
    )?;
    Ok(())
}

pub fn list_feed_health(conn: &Connection) -> Result<Vec<RssFeedHealth>> {
    create_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT feed_id, last_success_at, last_failure_at, last_failure_reason,
                consecutive_failures, total_failures, total_successes, status
         FROM rss_feed_health
         ORDER BY feed_id",
    )?;
    let rows = stmt.query_map([], row_to_health)?;
    let mut health = Vec::new();
    for row in rows {
        health.push(row?);
    }
    Ok(health)
}

pub fn list_feed_health_backend(backend: &BackendConnection) -> Result<Vec<RssFeedHealth>> {
    query::dispatch(backend, list_feed_health, list_feed_health_postgres)
}

pub fn health_for_feeds_backend(
    backend: &BackendConnection,
    feeds: &[RssFeed],
) -> Result<Vec<RssFeedHealthView>> {
    let stored = list_feed_health_backend(backend)?;
    Ok(health_for_feeds(feeds, &stored))
}

pub fn disabled_feed_ids_backend(backend: &BackendConnection) -> Result<HashSet<String>> {
    let rows = list_feed_health_backend(backend)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.status == "disabled")
        .map(|row| row.feed_id)
        .collect())
}

pub fn record_feed_success_backend(backend: &BackendConnection, feed_id: &str) -> Result<()> {
    query::dispatch(
        backend,
        |conn| record_feed_success(conn, feed_id),
        |pool| record_feed_success_postgres(pool, feed_id),
    )
}

pub fn record_feed_failure_backend(
    backend: &BackendConnection,
    feed_id: &str,
    reason: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| record_feed_failure(conn, feed_id, reason),
        |pool| record_feed_failure_postgres(pool, feed_id, reason),
    )
}

pub fn reset_feed_backend(backend: &BackendConnection, feed_id: &str) -> Result<()> {
    query::dispatch(
        backend,
        |conn| reset_feed(conn, feed_id),
        |pool| reset_feed_postgres(pool, feed_id),
    )
}

pub fn record_feed_success(conn: &Connection, feed_id: &str) -> Result<()> {
    create_table(conn)?;
    conn.execute(
        "INSERT INTO rss_feed_health
            (feed_id, last_success_at, consecutive_failures, total_successes, status)
         VALUES (?1, datetime('now'), 0, 1, 'active')
         ON CONFLICT(feed_id) DO UPDATE SET
            last_success_at = datetime('now'),
            consecutive_failures = 0,
            total_successes = total_successes + 1,
            status = 'active'",
        params![feed_id],
    )?;
    Ok(())
}

pub fn record_feed_failure(conn: &Connection, feed_id: &str, reason: &str) -> Result<()> {
    create_table(conn)?;
    let reason = truncate_reason(reason);
    let sql = format!(
        "INSERT INTO rss_feed_health
            (feed_id, last_failure_at, last_failure_reason, consecutive_failures, total_failures, status)
         VALUES (?1, datetime('now'), ?2, 1, 1, 'active')
         ON CONFLICT(feed_id) DO UPDATE SET
            last_failure_at = datetime('now'),
            last_failure_reason = ?2,
            consecutive_failures = consecutive_failures + 1,
            total_failures = total_failures + 1,
            status = CASE
                WHEN consecutive_failures + 1 >= {DISABLED_THRESHOLD} THEN 'disabled'
                WHEN consecutive_failures + 1 >= {DEGRADED_THRESHOLD} THEN 'degraded'
                ELSE 'active'
            END"
    );
    conn.execute(&sql, params![feed_id, reason])?;
    Ok(())
}

pub fn reset_feed(conn: &Connection, feed_id: &str) -> Result<()> {
    create_table(conn)?;
    conn.execute(
        "INSERT INTO rss_feed_health
            (feed_id, consecutive_failures, total_failures, total_successes, status)
         VALUES (?1, 0, 0, 0, 'active')
         ON CONFLICT(feed_id) DO UPDATE SET
            consecutive_failures = 0,
            last_failure_at = NULL,
            last_failure_reason = NULL,
            status = 'active'",
        params![feed_id],
    )?;
    Ok(())
}

fn row_to_health(row: &rusqlite::Row<'_>) -> rusqlite::Result<RssFeedHealth> {
    Ok(RssFeedHealth {
        feed_id: row.get(0)?,
        last_success_at: row.get(1)?,
        last_failure_at: row.get(2)?,
        last_failure_reason: row.get(3)?,
        consecutive_failures: row.get(4)?,
        total_failures: row.get(5)?,
        total_successes: row.get(6)?,
        status: row.get(7)?,
    })
}

pub fn health_for_feeds(feeds: &[RssFeed], stored: &[RssFeedHealth]) -> Vec<RssFeedHealthView> {
    let mut rows_by_id: HashMap<String, RssFeedHealth> = stored
        .iter()
        .map(|row| (row.feed_id.clone(), row.clone()))
        .collect();
    let mut views = Vec::new();

    for feed in feeds {
        let feed_id = feed.feed_id();
        let row = rows_by_id
            .remove(&feed_id)
            .unwrap_or_else(|| default_health(&feed_id));
        views.push(view_for_feed(feed, row));
    }

    for row in rows_by_id.into_values() {
        views.push(RssFeedHealthView {
            feed_name: row.feed_id.clone(),
            feed_url: String::new(),
            category: "unknown".to_string(),
            feed_id: row.feed_id.clone(),
            last_success_at: row.last_success_at,
            last_failure_at: row.last_failure_at,
            last_failure_reason: row.last_failure_reason,
            consecutive_failures: row.consecutive_failures,
            total_failures: row.total_failures,
            total_successes: row.total_successes,
            status: row.status,
        });
    }

    views.sort_by(|a, b| a.feed_name.cmp(&b.feed_name));
    views
}

fn view_for_feed(feed: &RssFeed, row: RssFeedHealth) -> RssFeedHealthView {
    RssFeedHealthView {
        feed_id: row.feed_id,
        feed_name: feed.name.clone(),
        feed_url: feed.url.clone(),
        category: feed.category.as_str().to_string(),
        last_success_at: row.last_success_at,
        last_failure_at: row.last_failure_at,
        last_failure_reason: row.last_failure_reason,
        consecutive_failures: row.consecutive_failures,
        total_failures: row.total_failures,
        total_successes: row.total_successes,
        status: row.status,
    }
}

fn default_health(feed_id: &str) -> RssFeedHealth {
    RssFeedHealth {
        feed_id: feed_id.to_string(),
        last_success_at: None,
        last_failure_at: None,
        last_failure_reason: None,
        consecutive_failures: 0,
        total_failures: 0,
        total_successes: 0,
        status: "active".to_string(),
    }
}

fn truncate_reason(reason: &str) -> String {
    reason.chars().take(500).collect()
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS rss_feed_health (
                feed_id TEXT PRIMARY KEY,
                last_success_at TIMESTAMPTZ,
                last_failure_at TIMESTAMPTZ,
                last_failure_reason TEXT,
                consecutive_failures BIGINT NOT NULL DEFAULT 0,
                total_failures BIGINT NOT NULL DEFAULT 0,
                total_successes BIGINT NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active'
                    CHECK(status IN ('active', 'degraded', 'disabled'))
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_rss_feed_health_status ON rss_feed_health(status)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_feed_health_postgres(pool: &PgPool) -> Result<Vec<RssFeedHealth>> {
    ensure_table_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT feed_id, last_success_at::TEXT, last_failure_at::TEXT, last_failure_reason,
                    consecutive_failures, total_failures, total_successes, status
             FROM rss_feed_health
             ORDER BY feed_id",
        )
        .fetch_all(pool)
        .await
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(RssFeedHealth {
                feed_id: row.try_get(0)?,
                last_success_at: row.try_get(1)?,
                last_failure_at: row.try_get(2)?,
                last_failure_reason: row.try_get(3)?,
                consecutive_failures: row.try_get(4)?,
                total_failures: row.try_get(5)?,
                total_successes: row.try_get(6)?,
                status: row.try_get(7)?,
            })
        })
        .collect()
}

fn record_feed_success_postgres(pool: &PgPool, feed_id: &str) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO rss_feed_health
                (feed_id, last_success_at, consecutive_failures, total_successes, status)
             VALUES ($1, NOW(), 0, 1, 'active')
             ON CONFLICT(feed_id) DO UPDATE SET
                last_success_at = NOW(),
                consecutive_failures = 0,
                total_successes = rss_feed_health.total_successes + 1,
                status = 'active'",
        )
        .bind(feed_id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn record_feed_failure_postgres(pool: &PgPool, feed_id: &str, reason: &str) -> Result<()> {
    ensure_table_postgres(pool)?;
    let reason = truncate_reason(reason);
    let sql = format!(
        "INSERT INTO rss_feed_health
            (feed_id, last_failure_at, last_failure_reason, consecutive_failures, total_failures, status)
         VALUES ($1, NOW(), $2, 1, 1, 'active')
         ON CONFLICT(feed_id) DO UPDATE SET
            last_failure_at = NOW(),
            last_failure_reason = $2,
            consecutive_failures = rss_feed_health.consecutive_failures + 1,
            total_failures = rss_feed_health.total_failures + 1,
            status = CASE
                WHEN rss_feed_health.consecutive_failures + 1 >= {DISABLED_THRESHOLD} THEN 'disabled'
                WHEN rss_feed_health.consecutive_failures + 1 >= {DEGRADED_THRESHOLD} THEN 'degraded'
                ELSE 'active'
            END"
    );
    crate::db::pg_runtime::block_on(async {
        sqlx::query(&sql)
            .bind(feed_id)
            .bind(reason)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn reset_feed_postgres(pool: &PgPool, feed_id: &str) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO rss_feed_health
                (feed_id, consecutive_failures, total_failures, total_successes, status)
             VALUES ($1, 0, 0, 0, 'active')
             ON CONFLICT(feed_id) DO UPDATE SET
                consecutive_failures = 0,
                last_failure_at = NULL,
                last_failure_reason = NULL,
                status = 'active'",
        )
        .bind(feed_id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn conn() -> Connection {
        open_in_memory()
    }

    #[test]
    fn failure_increments_and_degrades_at_threshold() {
        let conn = conn();
        for _ in 0..4 {
            record_feed_failure(&conn, "Bloomberg Commodities", "parse error").unwrap();
        }
        let row = list_feed_health(&conn).unwrap().remove(0);
        assert_eq!(row.consecutive_failures, 4);
        assert_eq!(row.status, "active");

        record_feed_failure(&conn, "Bloomberg Commodities", "parse error").unwrap();
        let row = list_feed_health(&conn).unwrap().remove(0);
        assert_eq!(row.consecutive_failures, DEGRADED_THRESHOLD);
        assert_eq!(row.status, "degraded");
    }

    #[test]
    fn failure_disables_at_threshold() {
        let conn = conn();
        for _ in 0..DISABLED_THRESHOLD {
            record_feed_failure(&conn, "Bloomberg Commodities", "parse error").unwrap();
        }
        let row = list_feed_health(&conn).unwrap().remove(0);
        assert_eq!(row.consecutive_failures, DISABLED_THRESHOLD);
        assert_eq!(row.status, "disabled");
    }

    #[test]
    fn success_resets_consecutive_failures_and_status() {
        let conn = conn();
        for _ in 0..DEGRADED_THRESHOLD {
            record_feed_failure(&conn, "Bloomberg Commodities", "parse error").unwrap();
        }
        record_feed_success(&conn, "Bloomberg Commodities").unwrap();

        let row = list_feed_health(&conn).unwrap().remove(0);
        assert_eq!(row.consecutive_failures, 0);
        assert_eq!(row.total_failures, DEGRADED_THRESHOLD);
        assert_eq!(row.total_successes, 1);
        assert_eq!(row.status, "active");
        assert!(row.last_success_at.is_some());
    }

    #[test]
    fn reset_reenables_disabled_feed() {
        let conn = conn();
        for _ in 0..DISABLED_THRESHOLD {
            record_feed_failure(&conn, "Bloomberg Commodities", "parse error").unwrap();
        }
        reset_feed(&conn, "Bloomberg Commodities").unwrap();

        let row = list_feed_health(&conn).unwrap().remove(0);
        assert_eq!(row.consecutive_failures, 0);
        assert_eq!(row.status, "active");
        assert!(row.last_failure_at.is_none());
        assert!(row.last_failure_reason.is_none());
    }

    #[test]
    fn disabled_feed_ids_backend_returns_disabled_rows_only() {
        let backend = BackendConnection::Sqlite { conn: conn() };
        for _ in 0..DISABLED_THRESHOLD {
            record_feed_failure_backend(&backend, "Bloomberg Commodities", "parse error").unwrap();
        }
        record_feed_failure_backend(&backend, "Bloomberg Markets", "transient").unwrap();

        let disabled = disabled_feed_ids_backend(&backend).unwrap();
        assert!(disabled.contains("Bloomberg Commodities"));
        assert!(!disabled.contains("Bloomberg Markets"));
    }

    #[test]
    fn health_for_feeds_includes_default_active_rows() {
        let rows = health_for_feeds(
            &[RssFeed {
                name: "Bloomberg Markets".to_string(),
                url: "https://example.com/rss".to_string(),
                category: crate::data::rss::NewsCategory::Markets,
            }],
            &[],
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].feed_id, "Bloomberg Markets");
        assert_eq!(rows[0].status, "active");
        assert_eq!(rows[0].category, "markets");
    }
}
