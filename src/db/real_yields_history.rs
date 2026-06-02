//! SQLite cache for real-yield curve data (TIPS, breakevens, G10 sovereign 10Y).
//!
//! Schema lives in `db::schema::run_migrations`:
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS real_yields_history (
//!     date       TEXT NOT NULL,
//!     series     TEXT NOT NULL,
//!     value      REAL NOT NULL,
//!     source     TEXT NOT NULL,
//!     fetched_at TEXT NOT NULL,
//!     PRIMARY KEY (date, series)
//! );
//! ```
//!
//! `value` is stored as `REAL` because these series are basis-point-precision
//! interest rates, not money — `rust_decimal` is not required per the project's
//! code-standards exception for yields.

use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::data::real_yields::RealYieldObservation;
use crate::db::backend::BackendConnection;
use crate::db::query;

/// One real-yield row read back out of the cache.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RealYieldRow {
    pub date: String,
    pub series: String,
    pub value: f64,
    pub source: String,
    pub fetched_at: String,
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS real_yields_history (
                date TEXT NOT NULL,
                series TEXT NOT NULL,
                value DOUBLE PRECISION NOT NULL,
                source TEXT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (date, series)
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

/// Upsert a single real-yield observation.
pub fn upsert_observation(conn: &Connection, obs: &RealYieldObservation) -> Result<()> {
    let fetched_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO real_yields_history (date, series, value, source, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, series) DO UPDATE SET
            value = excluded.value,
            source = excluded.source,
            fetched_at = excluded.fetched_at",
        params![
            obs.date,
            obs.series_id,
            obs.value,
            obs.source,
            fetched_at,
        ],
    )?;
    Ok(())
}

/// Batch upsert observations inside a single transaction.
pub fn upsert_observations(conn: &Connection, observations: &[RealYieldObservation]) -> Result<()> {
    if observations.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for obs in observations {
        upsert_observation(&tx, obs)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn upsert_observations_backend(
    backend: &BackendConnection,
    observations: &[RealYieldObservation],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_observations(conn, observations),
        |pool| upsert_observations_postgres(pool, observations),
    )
}

fn upsert_observations_postgres(
    pool: &PgPool,
    observations: &[RealYieldObservation],
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    if observations.is_empty() {
        return Ok(());
    }
    let fetched_at = chrono::Utc::now().to_rfc3339();
    crate::db::pg_runtime::block_on(async {
        let mut tx = pool.begin().await?;
        for obs in observations {
            sqlx::query(
                "INSERT INTO real_yields_history (date, series, value, source, fetched_at)
                 VALUES ($1, $2, $3, $4, $5::timestamptz)
                 ON CONFLICT(date, series) DO UPDATE SET
                    value = EXCLUDED.value,
                    source = EXCLUDED.source,
                    fetched_at = EXCLUDED.fetched_at",
            )
            .bind(&obs.date)
            .bind(&obs.series_id)
            .bind(obs.value)
            .bind(&obs.source)
            .bind(&fetched_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

/// Read history rows, optionally filtered by series, ordered by date asc then series.
pub fn fetch_history(
    conn: &Connection,
    series_filter: Option<&str>,
    since_date: Option<&str>,
) -> Result<Vec<RealYieldRow>> {
    let (sql, has_since): (&str, bool) = match (series_filter, since_date) {
        (Some(_), Some(_)) => (
            "SELECT date, series, value, source, fetched_at FROM real_yields_history
             WHERE series = ?1 AND date >= ?2 ORDER BY date ASC, series ASC",
            true,
        ),
        (Some(_), None) => (
            "SELECT date, series, value, source, fetched_at FROM real_yields_history
             WHERE series = ?1 ORDER BY date ASC, series ASC",
            false,
        ),
        (None, Some(_)) => (
            "SELECT date, series, value, source, fetched_at FROM real_yields_history
             WHERE date >= ?1 ORDER BY date ASC, series ASC",
            true,
        ),
        (None, None) => (
            "SELECT date, series, value, source, fetched_at FROM real_yields_history
             ORDER BY date ASC, series ASC",
            false,
        ),
    };

    let mut stmt = conn.prepare(sql)?;
    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<RealYieldRow> {
        Ok(RealYieldRow {
            date: row.get(0)?,
            series: row.get(1)?,
            value: row.get(2)?,
            source: row.get(3)?,
            fetched_at: row.get(4)?,
        })
    };
    let rows: Vec<RealYieldRow> = match (series_filter, since_date, has_since) {
        (Some(series), Some(since), _) => stmt
            .query_map(params![series, since], mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (Some(series), None, _) => stmt
            .query_map(params![series], mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (None, Some(since), _) => stmt
            .query_map(params![since], mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (None, None, _) => stmt
            .query_map([], mapper)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

pub fn fetch_history_backend(
    backend: &BackendConnection,
    series_filter: Option<&str>,
    since_date: Option<&str>,
) -> Result<Vec<RealYieldRow>> {
    query::dispatch(
        backend,
        |conn| fetch_history(conn, series_filter, since_date),
        |pool| fetch_history_postgres(pool, series_filter, since_date),
    )
}

fn fetch_history_postgres(
    pool: &PgPool,
    series_filter: Option<&str>,
    since_date: Option<&str>,
) -> Result<Vec<RealYieldRow>> {
    ensure_tables_postgres(pool)?;
    type Row = (String, String, f64, String, String);
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        match (series_filter, since_date) {
            (Some(s), Some(d)) => sqlx::query_as(
                "SELECT date, series, value, source, fetched_at::text FROM real_yields_history
                 WHERE series = $1 AND date >= $2 ORDER BY date ASC, series ASC",
            )
            .bind(s)
            .bind(d)
            .fetch_all(pool)
            .await,
            (Some(s), None) => sqlx::query_as(
                "SELECT date, series, value, source, fetched_at::text FROM real_yields_history
                 WHERE series = $1 ORDER BY date ASC, series ASC",
            )
            .bind(s)
            .fetch_all(pool)
            .await,
            (None, Some(d)) => sqlx::query_as(
                "SELECT date, series, value, source, fetched_at::text FROM real_yields_history
                 WHERE date >= $1 ORDER BY date ASC, series ASC",
            )
            .bind(d)
            .fetch_all(pool)
            .await,
            (None, None) => sqlx::query_as(
                "SELECT date, series, value, source, fetched_at::text FROM real_yields_history
                 ORDER BY date ASC, series ASC",
            )
            .fetch_all(pool)
            .await,
        }
    })?;
    Ok(rows
        .into_iter()
        .map(|r| RealYieldRow {
            date: r.0,
            series: r.1,
            value: r.2,
            source: r.3,
            fetched_at: r.4,
        })
        .collect())
}

/// Latest stored row for each series (used by the macro report block).
pub fn fetch_latest_per_series(conn: &Connection) -> Result<Vec<RealYieldRow>> {
    let mut stmt = conn.prepare(
        "SELECT r.date, r.series, r.value, r.source, r.fetched_at
         FROM real_yields_history r
         JOIN (
             SELECT series, MAX(date) AS max_date
             FROM real_yields_history
             GROUP BY series
         ) latest ON r.series = latest.series AND r.date = latest.max_date
         ORDER BY r.series ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(RealYieldRow {
                date: row.get(0)?,
                series: row.get(1)?,
                value: row.get(2)?,
                source: row.get(3)?,
                fetched_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn fetch_latest_per_series_backend(
    backend: &BackendConnection,
) -> Result<Vec<RealYieldRow>> {
    query::dispatch(
        backend,
        fetch_latest_per_series,
        fetch_latest_per_series_postgres,
    )
}

fn fetch_latest_per_series_postgres(pool: &PgPool) -> Result<Vec<RealYieldRow>> {
    ensure_tables_postgres(pool)?;
    type Row = (String, String, f64, String, String);
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT r.date, r.series, r.value, r.source, r.fetched_at::text
             FROM real_yields_history r
             JOIN (
                 SELECT series, MAX(date) AS max_date
                 FROM real_yields_history
                 GROUP BY series
             ) latest ON r.series = latest.series AND r.date = latest.max_date
             ORDER BY r.series ASC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| RealYieldRow {
            date: r.0,
            series: r.1,
            value: r.2,
            source: r.3,
            fetched_at: r.4,
        })
        .collect())
}

/// Count rows (used by `data status`-style introspection and tests).
pub fn count_rows(conn: &Connection) -> Result<u64> {
    let n: i64 =
        conn.query_row("SELECT COUNT(*) FROM real_yields_history", [], |r| r.get(0))?;
    Ok(n as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::real_yields::RealYieldObservation;
    use crate::db::schema;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().expect("open");
        schema::run_migrations(&c).expect("schema");
        c
    }

    fn obs(date: &str, series: &str, value: f64) -> RealYieldObservation {
        RealYieldObservation {
            series_id: series.into(),
            date: date.into(),
            value,
            source: "FRED".into(),
        }
    }

    #[test]
    fn upsert_and_history_roundtrip() {
        let c = mem();
        let rows = vec![
            obs("2026-04-01", "DFII10", 2.10),
            obs("2026-04-02", "DFII10", 2.15),
            obs("2026-04-01", "T10YIE", 2.40),
        ];
        upsert_observations(&c, &rows).expect("upsert");
        assert_eq!(count_rows(&c).expect("count"), 3);

        // Re-upsert overwrites — count stays at 3, value updates.
        upsert_observations(&c, &[obs("2026-04-02", "DFII10", 2.25)]).expect("re-up");
        assert_eq!(count_rows(&c).expect("count"), 3);
        let dfii10 = fetch_history(&c, Some("DFII10"), None).expect("history");
        assert_eq!(dfii10.len(), 2);
        assert_eq!(dfii10[1].value, 2.25);
    }

    #[test]
    fn fetch_latest_per_series_picks_most_recent_date() {
        let c = mem();
        upsert_observations(
            &c,
            &[
                obs("2026-04-01", "DFII10", 2.10),
                obs("2026-04-02", "DFII10", 2.20),
                obs("2026-04-01", "IRLTLT01DEM156N", 2.30),
            ],
        )
        .expect("upsert");
        let latest = fetch_latest_per_series(&c).expect("latest");
        assert_eq!(latest.len(), 2);
        let dfii = latest.iter().find(|r| r.series == "DFII10").unwrap();
        assert_eq!(dfii.date, "2026-04-02");
        assert_eq!(dfii.value, 2.20);
    }

    #[test]
    fn fetch_history_filters_by_since_date() {
        let c = mem();
        upsert_observations(
            &c,
            &[
                obs("2026-04-01", "DFII10", 2.10),
                obs("2026-04-02", "DFII10", 2.15),
                obs("2026-04-03", "DFII10", 2.20),
            ],
        )
        .expect("upsert");
        let recent = fetch_history(&c, Some("DFII10"), Some("2026-04-02")).expect("history");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].date, "2026-04-02");
    }
}
