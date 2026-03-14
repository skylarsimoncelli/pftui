use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationSnapshot {
    pub id: i64,
    pub symbol_a: String,
    pub symbol_b: String,
    pub correlation: f64,
    pub period: String,
    pub recorded_at: String,
}

impl CorrelationSnapshot {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol_a: row.get(1)?,
            symbol_b: row.get(2)?,
            correlation: row.get(3)?,
            period: row.get(4)?,
            recorded_at: row.get(5)?,
        })
    }
}

pub fn store_snapshot(
    conn: &Connection,
    symbol_a: &str,
    symbol_b: &str,
    correlation: f64,
    period: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO correlation_snapshots (symbol_a, symbol_b, correlation, period)
         VALUES (?, ?, ?, ?)",
        params![symbol_a, symbol_b, correlation, period],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_current(conn: &Connection, period: Option<&str>) -> Result<Vec<CorrelationSnapshot>> {
    let query = if let Some(p) = period {
        format!(
            "SELECT c.id, c.symbol_a, c.symbol_b, c.correlation, c.period, c.recorded_at
             FROM correlation_snapshots c
             INNER JOIN (
               SELECT symbol_a, symbol_b, period, MAX(recorded_at) AS max_ts
               FROM correlation_snapshots
               WHERE period = '{}'
               GROUP BY symbol_a, symbol_b, period
             ) latest ON c.symbol_a = latest.symbol_a
                       AND c.symbol_b = latest.symbol_b
                       AND c.period = latest.period
                       AND c.recorded_at = latest.max_ts
             ORDER BY ABS(c.correlation) DESC",
            p.replace('"', "''")
        )
    } else {
        "SELECT c.id, c.symbol_a, c.symbol_b, c.correlation, c.period, c.recorded_at
         FROM correlation_snapshots c
         INNER JOIN (
           SELECT symbol_a, symbol_b, period, MAX(recorded_at) AS max_ts
           FROM correlation_snapshots
           GROUP BY symbol_a, symbol_b, period
         ) latest ON c.symbol_a = latest.symbol_a
                   AND c.symbol_b = latest.symbol_b
                   AND c.period = latest.period
                   AND c.recorded_at = latest.max_ts
         ORDER BY ABS(c.correlation) DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], CorrelationSnapshot::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_history(
    conn: &Connection,
    symbol_a: &str,
    symbol_b: &str,
    period: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<CorrelationSnapshot>> {
    let mut query = format!(
        "SELECT id, symbol_a, symbol_b, correlation, period, recorded_at
         FROM correlation_snapshots
         WHERE symbol_a = '{}' AND symbol_b = '{}'",
        symbol_a.replace('"', "''"),
        symbol_b.replace('"', "''")
    );

    if let Some(p) = period {
        query.push_str(&format!(" AND period = '{}'", p.replace('"', "''")));
    }

    query.push_str(" ORDER BY recorded_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], CorrelationSnapshot::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn store_snapshot_backend(
    backend: &BackendConnection,
    symbol_a: &str,
    symbol_b: &str,
    correlation: f64,
    period: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| store_snapshot(conn, symbol_a, symbol_b, correlation, period),
        |pool| store_snapshot_postgres(pool, symbol_a, symbol_b, correlation, period),
    )
}

#[allow(dead_code)]
pub fn list_current_backend(
    backend: &BackendConnection,
    period: Option<&str>,
) -> Result<Vec<CorrelationSnapshot>> {
    query::dispatch(
        backend,
        |conn| list_current(conn, period),
        |pool| list_current_postgres(pool, period),
    )
}

pub fn get_history_backend(
    backend: &BackendConnection,
    symbol_a: &str,
    symbol_b: &str,
    period: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<CorrelationSnapshot>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, symbol_a, symbol_b, period, limit),
        |pool| get_history_postgres(pool, symbol_a, symbol_b, period, limit),
    )
}

type CorrelationRow = (i64, String, String, f64, String, String);

fn from_pg_row(row: CorrelationRow) -> CorrelationSnapshot {
    CorrelationSnapshot {
        id: row.0,
        symbol_a: row.1,
        symbol_b: row.2,
        correlation: row.3,
        period: row.4,
        recorded_at: row.5,
    }
}

fn store_snapshot_postgres(
    pool: &PgPool,
    symbol_a: &str,
    symbol_b: &str,
    correlation: f64,
    period: &str,
) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO correlation_snapshots (symbol_a, symbol_b, correlation, period)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(symbol_a)
        .bind(symbol_b)
        .bind(correlation)
        .bind(period)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

#[allow(dead_code)]
fn list_current_postgres(pool: &PgPool, period: Option<&str>) -> Result<Vec<CorrelationSnapshot>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<CorrelationRow> = if let Some(p) = period {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT DISTINCT ON (symbol_a, symbol_b, period)
                    id, symbol_a, symbol_b, correlation, period, recorded_at::text
                 FROM correlation_snapshots
                 WHERE period = $1
                 ORDER BY symbol_a, symbol_b, period, recorded_at DESC",
            )
            .bind(p)
            .fetch_all(pool)
            .await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT DISTINCT ON (symbol_a, symbol_b, period)
                    id, symbol_a, symbol_b, correlation, period, recorded_at::text
                 FROM correlation_snapshots
                 ORDER BY symbol_a, symbol_b, period, recorded_at DESC",
            )
            .fetch_all(pool)
            .await
        })?
    };
    let mut out: Vec<CorrelationSnapshot> = rows.into_iter().map(from_pg_row).collect();
    out.sort_by(|a, b| {
        b.correlation
            .abs()
            .partial_cmp(&a.correlation.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

fn get_history_postgres(
    pool: &PgPool,
    symbol_a: &str,
    symbol_b: &str,
    period: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<CorrelationSnapshot>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<CorrelationRow> = match (period, limit) {
        (Some(p), Some(n)) => crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, symbol_a, symbol_b, correlation, period, recorded_at::text
                 FROM correlation_snapshots
                 WHERE symbol_a = $1 AND symbol_b = $2 AND period = $3
                 ORDER BY recorded_at DESC
                 LIMIT $4",
            )
            .bind(symbol_a)
            .bind(symbol_b)
            .bind(p)
            .bind(n as i64)
            .fetch_all(pool)
            .await
        })?,
        (Some(p), None) => crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, symbol_a, symbol_b, correlation, period, recorded_at::text
                 FROM correlation_snapshots
                 WHERE symbol_a = $1 AND symbol_b = $2 AND period = $3
                 ORDER BY recorded_at DESC",
            )
            .bind(symbol_a)
            .bind(symbol_b)
            .bind(p)
            .fetch_all(pool)
            .await
        })?,
        (None, Some(n)) => crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, symbol_a, symbol_b, correlation, period, recorded_at::text
                 FROM correlation_snapshots
                 WHERE symbol_a = $1 AND symbol_b = $2
                 ORDER BY recorded_at DESC
                 LIMIT $3",
            )
            .bind(symbol_a)
            .bind(symbol_b)
            .bind(n as i64)
            .fetch_all(pool)
            .await
        })?,
        (None, None) => crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, symbol_a, symbol_b, correlation, period, recorded_at::text
                 FROM correlation_snapshots
                 WHERE symbol_a = $1 AND symbol_b = $2
                 ORDER BY recorded_at DESC",
            )
            .bind(symbol_a)
            .bind(symbol_b)
            .fetch_all(pool)
            .await
        })?,
    };
    Ok(rows.into_iter().map(from_pg_row).collect())
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS correlation_snapshots (
                id BIGSERIAL PRIMARY KEY,
                symbol_a TEXT NOT NULL,
                symbol_b TEXT NOT NULL,
                correlation DOUBLE PRECISION NOT NULL,
                period TEXT NOT NULL DEFAULT '30d',
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_corr_snap_pair
             ON correlation_snapshots(symbol_a, symbol_b)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_corr_snap_date
             ON correlation_snapshots(recorded_at)",
        )
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

    #[test]
    fn list_current_returns_latest_per_pair_and_period() {
        let conn = open_in_memory();
        let first_id = store_snapshot(&conn, "BTC", "SPY", 0.42, "30d").unwrap();
        conn.execute(
            "UPDATE correlation_snapshots SET recorded_at = '2026-03-01T00:00:00Z' WHERE id = ?1",
            rusqlite::params![first_id],
        )
        .unwrap();
        store_snapshot(&conn, "BTC", "SPY", 0.61, "30d").unwrap();
        store_snapshot(&conn, "BTC", "GC=F", -0.25, "30d").unwrap();

        let rows = list_current(&conn, Some("30d")).unwrap();
        assert_eq!(rows.len(), 2);
        let btc_spy = rows
            .iter()
            .find(|r| r.symbol_a == "BTC" && r.symbol_b == "SPY")
            .unwrap();
        assert!((btc_spy.correlation - 0.61).abs() < f64::EPSILON);
    }
}
