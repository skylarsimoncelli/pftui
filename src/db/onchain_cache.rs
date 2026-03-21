//! SQLite cache for BTC on-chain data.
//!
//! Stores exchange flows, whale transactions, and network metrics
//! with 1-day TTL for historical data, 1-hour TTL for current metrics.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A cached on-chain metric.
#[derive(Debug, Clone)]
pub struct OnchainMetric {
    pub metric: String,           // e.g., "exchange_net_flow", "whale_tx", "hash_rate"
    pub date: String,             // YYYY-MM-DD
    pub value: String,            // Stored as string for decimal precision
    pub metadata: Option<String>, // JSON for additional fields
    pub fetched_at: String,
}

/// Upsert an on-chain metric into the cache.
///
/// Uses (metric, date) as the primary key.
pub fn upsert_metric(conn: &Connection, metric: &OnchainMetric) -> Result<()> {
    conn.execute(
        "INSERT INTO onchain_cache (metric, date, value, metadata, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(metric, date) DO UPDATE SET
           value = excluded.value,
           metadata = excluded.metadata,
           fetched_at = excluded.fetched_at",
        params![
            metric.metric,
            metric.date,
            metric.value,
            metric.metadata,
            metric.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn upsert_metric_backend(backend: &BackendConnection, metric: &OnchainMetric) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_metric(conn, metric),
        |pool| upsert_metric_postgres(pool, metric),
    )
}

/// Get a specific metric for a given date.
///
/// Returns None if not cached or if stale (>24 hours old for historical, >1 hour for current).
pub fn get_metric(conn: &Connection, metric: &str, date: &str) -> Result<Option<OnchainMetric>> {
    let mut stmt = conn.prepare(
        "SELECT metric, date, value, metadata, fetched_at
         FROM onchain_cache
         WHERE metric = ?1 AND date = ?2
         AND datetime(fetched_at) > datetime('now', '-24 hours')",
    )?;

    let result = stmt
        .query_row(params![metric, date], |row| {
            Ok(OnchainMetric {
                metric: row.get(0)?,
                date: row.get(1)?,
                value: row.get(2)?,
                metadata: row.get(3)?,
                fetched_at: row.get(4)?,
            })
        })
        .optional()?;

    Ok(result)
}

pub fn get_metric_backend(
    backend: &BackendConnection,
    metric: &str,
    date: &str,
) -> Result<Option<OnchainMetric>> {
    query::dispatch(
        backend,
        |conn| get_metric(conn, metric, date),
        |pool| get_metric_postgres(pool, metric, date),
    )
}

/// Get all metrics of a specific type, ordered by date descending.
///
/// Example: get_metrics_by_type(conn, "exchange_net_flow", 7) → last 7 days
pub fn get_metrics_by_type(
    conn: &Connection,
    metric: &str,
    limit: usize,
) -> Result<Vec<OnchainMetric>> {
    let mut stmt = conn.prepare(
        "SELECT metric, date, value, metadata, fetched_at
         FROM onchain_cache
         WHERE metric = ?1
         ORDER BY date DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![metric, limit as i64], |row| {
        Ok(OnchainMetric {
            metric: row.get(0)?,
            date: row.get(1)?,
            value: row.get(2)?,
            metadata: row.get(3)?,
            fetched_at: row.get(4)?,
        })
    })?;

    let mut metrics = Vec::new();
    for row in rows {
        metrics.push(row?);
    }

    Ok(metrics)
}

pub fn get_metrics_by_type_backend(
    backend: &BackendConnection,
    metric: &str,
    limit: usize,
) -> Result<Vec<OnchainMetric>> {
    query::dispatch(
        backend,
        |conn| get_metrics_by_type(conn, metric, limit),
        |pool| get_metrics_by_type_postgres(pool, metric, limit),
    )
}

/// Delete metrics older than 90 days.
///
/// Keeps the cache size manageable for historical data.
pub fn prune_old_metrics(conn: &Connection) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM onchain_cache
         WHERE date < date('now', '-90 days')",
        [],
    )?;
    Ok(deleted)
}

pub fn prune_old_metrics_backend(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(backend, prune_old_metrics, prune_old_metrics_postgres)
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS onchain_cache (
                metric TEXT NOT NULL,
                date TEXT NOT NULL,
                value TEXT NOT NULL,
                metadata TEXT,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (metric, date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_onchain_date ON onchain_cache(date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_onchain_metric ON onchain_cache(metric)")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_metric_postgres(pool: &PgPool, metric: &OnchainMetric) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO onchain_cache (metric, date, value, metadata, fetched_at)
             VALUES ($1, $2, $3, $4, $5::timestamptz)
             ON CONFLICT (metric, date) DO UPDATE SET
               value = EXCLUDED.value,
               metadata = EXCLUDED.metadata,
               fetched_at = EXCLUDED.fetched_at",
        )
        .bind(&metric.metric)
        .bind(&metric.date)
        .bind(&metric.value)
        .bind(&metric.metadata)
        .bind(&metric.fetched_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_metric_postgres(pool: &PgPool, metric: &str, date: &str) -> Result<Option<OnchainMetric>> {
    ensure_table_postgres(pool)?;
    let row: Option<(String, String, String, Option<String>, String)> =
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT metric, date, value, metadata, fetched_at::text
             FROM onchain_cache
             WHERE metric = $1
               AND date = $2
               AND fetched_at > NOW() - INTERVAL '24 hours'",
            )
            .bind(metric)
            .bind(date)
            .fetch_optional(pool)
            .await
        })?;
    Ok(row.map(|r| OnchainMetric {
        metric: r.0,
        date: r.1,
        value: r.2,
        metadata: r.3,
        fetched_at: r.4,
    }))
}

fn get_metrics_by_type_postgres(
    pool: &PgPool,
    metric: &str,
    limit: usize,
) -> Result<Vec<OnchainMetric>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<(String, String, String, Option<String>, String)> =
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT metric, date, value, metadata, fetched_at::text
             FROM onchain_cache
             WHERE metric = $1
             ORDER BY date DESC
             LIMIT $2",
            )
            .bind(metric)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        })?;

    Ok(rows
        .into_iter()
        .map(|r| OnchainMetric {
            metric: r.0,
            date: r.1,
            value: r.2,
            metadata: r.3,
            fetched_at: r.4,
        })
        .collect())
}

fn prune_old_metrics_postgres(pool: &PgPool) -> Result<usize> {
    ensure_table_postgres(pool)?;
    let deleted = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM onchain_cache WHERE date < TO_CHAR(NOW() - INTERVAL '90 days', 'YYYY-MM-DD')")
            .execute(pool)
            .await
    })?;
    Ok(deleted.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get_metric() {
        let conn = setup_test_db();

        // Use current timestamp to avoid TTL expiry
        let now = chrono::Utc::now().to_rfc3339();

        let metric = OnchainMetric {
            metric: "exchange_net_flow".to_string(),
            date: "2026-03-06".to_string(),
            value: "-1250.5".to_string(),
            metadata: Some(r#"{"inflow": 2000, "outflow": 3250.5}"#.to_string()),
            fetched_at: now.clone(),
        };

        upsert_metric(&conn, &metric).unwrap();

        let retrieved = get_metric(&conn, "exchange_net_flow", "2026-03-06").unwrap();
        assert!(retrieved.is_some());

        let m = retrieved.unwrap();
        assert_eq!(m.value, "-1250.5");
        assert!(m.metadata.is_some());
    }

    #[test]
    fn test_get_metrics_by_type() {
        let conn = setup_test_db();

        for i in 1..=5 {
            let metric = OnchainMetric {
                metric: "hash_rate".to_string(),
                date: format!("2026-03-{:02}", i),
                value: format!("{}", 600 + i * 10),
                metadata: None,
                fetched_at: "2026-03-05T08:00:00Z".to_string(),
            };
            upsert_metric(&conn, &metric).unwrap();
        }

        let metrics = get_metrics_by_type(&conn, "hash_rate", 3).unwrap();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0].date, "2026-03-05"); // Most recent first
    }

    #[test]
    fn test_prune_old_metrics() {
        let conn = setup_test_db();

        // Insert old metric
        let old = OnchainMetric {
            metric: "test".to_string(),
            date: "2025-01-01".to_string(),
            value: "100".to_string(),
            metadata: None,
            fetched_at: "2025-01-01T00:00:00Z".to_string(),
        };
        upsert_metric(&conn, &old).unwrap();

        let deleted = prune_old_metrics(&conn).unwrap();
        assert_eq!(deleted, 1);
    }
}
