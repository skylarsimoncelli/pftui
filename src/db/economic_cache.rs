//! SQLite cache for FRED economic indicator data.
//!
//! Stores both the latest value (for quick display) and historical observations
//! (for sparklines/trends). Aggressive caching — FRED data rarely changes intraday.

use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A cached economic indicator observation.
#[derive(Debug, Clone)]
pub struct EconomicObservation {
    pub series_id: String,
    pub date: String,
    pub value: Decimal,
    pub fetched_at: String,
}

/// Upsert a single economic observation into the cache.
///
/// Uses (series_id, date) as the primary key — updates value/fetched_at on conflict.
pub fn upsert_observation(conn: &Connection, obs: &EconomicObservation) -> Result<()> {
    conn.execute(
        "INSERT INTO economic_cache (series_id, date, value, fetched_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(series_id, date) DO UPDATE SET
           value = excluded.value,
           fetched_at = excluded.fetched_at",
        params![
            obs.series_id,
            obs.date,
            obs.value.to_string(),
            obs.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn upsert_observation_backend(backend: &BackendConnection, obs: &EconomicObservation) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_observation(conn, obs),
        |pool| upsert_observation_postgres(pool, obs),
    )
}

/// Batch upsert multiple observations.
pub fn upsert_observations(conn: &Connection, observations: &[EconomicObservation]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for obs in observations {
        upsert_observation(&tx, obs)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn upsert_observations_backend(
    backend: &BackendConnection,
    observations: &[EconomicObservation],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_observations(conn, observations),
        |pool| upsert_observations_postgres(pool, observations),
    )
}

/// Get the most recent observation for a series.
pub fn get_latest(conn: &Connection, series_id: &str) -> Result<Option<EconomicObservation>> {
    let mut stmt = conn.prepare(
        "SELECT series_id, date, value, fetched_at FROM economic_cache
         WHERE series_id = ?1
         ORDER BY date DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![series_id], |row| {
        Ok(EconomicObservation {
            series_id: row.get(0)?,
            date: row.get(1)?,
            value: row.get::<_, String>(2)?
                .parse()
                .unwrap_or(Decimal::ZERO),
            fetched_at: row.get(3)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn get_latest_backend(
    backend: &BackendConnection,
    series_id: &str,
) -> Result<Option<EconomicObservation>> {
    query::dispatch(
        backend,
        |conn| get_latest(conn, series_id),
        |pool| get_latest_postgres(pool, series_id),
    )
}

/// Get recent observations for a series, ordered by date ascending.
///
/// Useful for sparklines and trend analysis.
pub fn get_history(
    conn: &Connection,
    series_id: &str,
    limit: u32,
) -> Result<Vec<EconomicObservation>> {
    let mut stmt = conn.prepare(
        "SELECT series_id, date, value, fetched_at FROM economic_cache
         WHERE series_id = ?1
         ORDER BY date DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![series_id, limit], |row| {
        Ok(EconomicObservation {
            series_id: row.get(0)?,
            date: row.get(1)?,
            value: row.get::<_, String>(2)?
                .parse()
                .unwrap_or(Decimal::ZERO),
            fetched_at: row.get(3)?,
        })
    })?;

    // Collect in desc order, then reverse for asc
    let mut result: Vec<EconomicObservation> = Vec::new();
    for row in rows {
        result.push(row?);
    }
    result.reverse();
    Ok(result)
}

pub fn get_history_backend(
    backend: &BackendConnection,
    series_id: &str,
    limit: u32,
) -> Result<Vec<EconomicObservation>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, series_id, limit),
        |pool| get_history_postgres(pool, series_id, limit),
    )
}

/// Get latest observations for all cached series.
///
/// Returns one row per series (the most recent date).
pub fn get_all_latest(conn: &Connection) -> Result<Vec<EconomicObservation>> {
    let mut stmt = conn.prepare(
        "SELECT e.series_id, e.date, e.value, e.fetched_at
         FROM economic_cache e
         INNER JOIN (
             SELECT series_id, MAX(date) as max_date
             FROM economic_cache
             GROUP BY series_id
         ) latest ON e.series_id = latest.series_id AND e.date = latest.max_date
         ORDER BY e.series_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(EconomicObservation {
            series_id: row.get(0)?,
            date: row.get(1)?,
            value: row.get::<_, String>(2)?
                .parse()
                .unwrap_or(Decimal::ZERO),
            fetched_at: row.get(3)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_all_latest_backend(backend: &BackendConnection) -> Result<Vec<EconomicObservation>> {
    query::dispatch(backend, get_all_latest, get_all_latest_postgres)
}

/// Delete all observations for a series (useful for cache invalidation).
pub fn delete_series(conn: &Connection, series_id: &str) -> Result<u64> {
    let count = conn.execute(
        "DELETE FROM economic_cache WHERE series_id = ?1",
        params![series_id],
    )?;
    Ok(count as u64)
}

pub fn delete_series_backend(backend: &BackendConnection, series_id: &str) -> Result<u64> {
    query::dispatch(
        backend,
        |conn| delete_series(conn, series_id),
        |pool| delete_series_postgres(pool, series_id),
    )
}

/// Count total cached observations.
pub fn count_observations(conn: &Connection) -> Result<u64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM economic_cache",
        [],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

pub fn count_observations_backend(backend: &BackendConnection) -> Result<u64> {
    query::dispatch(backend, count_observations, count_observations_postgres)
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS economic_cache (
                series_id TEXT NOT NULL,
                date TEXT NOT NULL,
                value TEXT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (series_id, date)
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_observation_postgres(pool: &PgPool, obs: &EconomicObservation) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO economic_cache (series_id, date, value, fetched_at)
             VALUES ($1, $2, $3, $4::timestamptz)
             ON CONFLICT(series_id, date) DO UPDATE SET
               value = EXCLUDED.value,
               fetched_at = EXCLUDED.fetched_at",
        )
        .bind(&obs.series_id)
        .bind(&obs.date)
        .bind(obs.value.to_string())
        .bind(&obs.fetched_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_observations_postgres(pool: &PgPool, observations: &[EconomicObservation]) -> Result<()> {
    ensure_tables_postgres(pool)?;
    if observations.is_empty() {
        return Ok(());
    }
    crate::db::pg_runtime::block_on(async {
        let mut tx = pool.begin().await?;
        for obs in observations {
            sqlx::query(
                "INSERT INTO economic_cache (series_id, date, value, fetched_at)
                 VALUES ($1, $2, $3, $4::timestamptz)
                 ON CONFLICT(series_id, date) DO UPDATE SET
                   value = EXCLUDED.value,
                   fetched_at = EXCLUDED.fetched_at",
            )
            .bind(&obs.series_id)
            .bind(&obs.date)
            .bind(obs.value.to_string())
            .bind(&obs.fetched_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type EconRow = (String, String, String, String);

fn to_observation(row: EconRow) -> EconomicObservation {
    EconomicObservation {
        series_id: row.0,
        date: row.1,
        value: row.2.parse().unwrap_or(Decimal::ZERO),
        fetched_at: row.3,
    }
}

fn get_latest_postgres(pool: &PgPool, series_id: &str) -> Result<Option<EconomicObservation>> {
    ensure_tables_postgres(pool)?;
        let row: Option<EconRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT series_id, date, value, fetched_at::text
             FROM economic_cache
             WHERE series_id = $1
             ORDER BY date DESC
             LIMIT 1",
        )
        .bind(series_id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_observation))
}

fn get_history_postgres(pool: &PgPool, series_id: &str, limit: u32) -> Result<Vec<EconomicObservation>> {
    ensure_tables_postgres(pool)?;
        let mut rows: Vec<EconRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT series_id, date, value, fetched_at::text
             FROM economic_cache
             WHERE series_id = $1
             ORDER BY date DESC
             LIMIT $2",
        )
        .bind(series_id)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    })?;
    rows.reverse();
    Ok(rows.into_iter().map(to_observation).collect())
}

fn get_all_latest_postgres(pool: &PgPool) -> Result<Vec<EconomicObservation>> {
    ensure_tables_postgres(pool)?;
        let rows: Vec<EconRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT e.series_id, e.date, e.value, e.fetched_at::text
             FROM economic_cache e
             JOIN (
                 SELECT series_id, MAX(date) AS max_date
                 FROM economic_cache
                 GROUP BY series_id
             ) latest
             ON e.series_id = latest.series_id AND e.date = latest.max_date
             ORDER BY e.series_id",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(to_observation).collect())
}

fn delete_series_postgres(pool: &PgPool, series_id: &str) -> Result<u64> {
    ensure_tables_postgres(pool)?;
        let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM economic_cache WHERE series_id = $1")
            .bind(series_id)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected())
}

fn count_observations_postgres(pool: &PgPool) -> Result<u64> {
    ensure_tables_postgres(pool)?;
        let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM economic_cache")
            .fetch_one(pool)
            .await
    })?;
    Ok(count as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    fn make_obs(series: &str, date: &str, value: Decimal) -> EconomicObservation {
        EconomicObservation {
            series_id: series.to_string(),
            date: date.to_string(),
            value,
            fetched_at: "2026-03-04T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_upsert_and_get_latest() {
        let conn = open_in_memory();
        let obs = make_obs("DGS10", "2026-03-03", dec!(4.07));
        upsert_observation(&conn, &obs).unwrap();

        let latest = get_latest(&conn, "DGS10").unwrap().unwrap();
        assert_eq!(latest.value, dec!(4.07));
        assert_eq!(latest.date, "2026-03-03");
    }

    #[test]
    fn test_upsert_updates_existing() {
        let conn = open_in_memory();
        let obs1 = make_obs("DGS10", "2026-03-03", dec!(4.07));
        upsert_observation(&conn, &obs1).unwrap();

        let obs2 = EconomicObservation {
            value: dec!(4.10),
            fetched_at: "2026-03-04T01:00:00Z".to_string(),
            ..obs1
        };
        upsert_observation(&conn, &obs2).unwrap();

        let latest = get_latest(&conn, "DGS10").unwrap().unwrap();
        assert_eq!(latest.value, dec!(4.10));
    }

    #[test]
    fn test_get_latest_returns_most_recent() {
        let conn = open_in_memory();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-01", dec!(4.00))).unwrap();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-03", dec!(4.07))).unwrap();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-02", dec!(4.05))).unwrap();

        let latest = get_latest(&conn, "DGS10").unwrap().unwrap();
        assert_eq!(latest.date, "2026-03-03");
        assert_eq!(latest.value, dec!(4.07));
    }

    #[test]
    fn test_get_latest_empty() {
        let conn = open_in_memory();
        assert!(get_latest(&conn, "DGS10").unwrap().is_none());
    }

    #[test]
    fn test_get_history() {
        let conn = open_in_memory();
        upsert_observation(&conn, &make_obs("FEDFUNDS", "2026-01-01", dec!(3.50))).unwrap();
        upsert_observation(&conn, &make_obs("FEDFUNDS", "2026-02-01", dec!(3.25))).unwrap();
        upsert_observation(&conn, &make_obs("FEDFUNDS", "2026-03-01", dec!(3.00))).unwrap();

        let history = get_history(&conn, "FEDFUNDS", 10).unwrap();
        assert_eq!(history.len(), 3);
        // Should be ascending by date
        assert_eq!(history[0].date, "2026-01-01");
        assert_eq!(history[2].date, "2026-03-01");
    }

    #[test]
    fn test_get_history_limit() {
        let conn = open_in_memory();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-01", dec!(4.00))).unwrap();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-02", dec!(4.05))).unwrap();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-03", dec!(4.07))).unwrap();

        let history = get_history(&conn, "DGS10", 2).unwrap();
        assert_eq!(history.len(), 2);
        // Most recent 2, ascending
        assert_eq!(history[0].date, "2026-03-02");
        assert_eq!(history[1].date, "2026-03-03");
    }

    #[test]
    fn test_batch_upsert() {
        let conn = open_in_memory();
        let observations = vec![
            make_obs("DGS10", "2026-03-03", dec!(4.07)),
            make_obs("FEDFUNDS", "2026-03-01", dec!(3.50)),
            make_obs("UNRATE", "2026-02-01", dec!(4.1)),
        ];
        upsert_observations(&conn, &observations).unwrap();

        assert_eq!(count_observations(&conn).unwrap(), 3);
        assert!(get_latest(&conn, "DGS10").unwrap().is_some());
        assert!(get_latest(&conn, "FEDFUNDS").unwrap().is_some());
        assert!(get_latest(&conn, "UNRATE").unwrap().is_some());
    }

    #[test]
    fn test_get_all_latest() {
        let conn = open_in_memory();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-01", dec!(4.00))).unwrap();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-03", dec!(4.07))).unwrap();
        upsert_observation(&conn, &make_obs("FEDFUNDS", "2026-02-01", dec!(3.50))).unwrap();

        let all = get_all_latest(&conn).unwrap();
        assert_eq!(all.len(), 2);

        let dgs = all.iter().find(|o| o.series_id == "DGS10").unwrap();
        assert_eq!(dgs.date, "2026-03-03");
        assert_eq!(dgs.value, dec!(4.07));
    }

    #[test]
    fn test_delete_series() {
        let conn = open_in_memory();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-01", dec!(4.00))).unwrap();
        upsert_observation(&conn, &make_obs("DGS10", "2026-03-02", dec!(4.05))).unwrap();
        upsert_observation(&conn, &make_obs("FEDFUNDS", "2026-02-01", dec!(3.50))).unwrap();

        let deleted = delete_series(&conn, "DGS10").unwrap();
        assert_eq!(deleted, 2);
        assert!(get_latest(&conn, "DGS10").unwrap().is_none());
        assert!(get_latest(&conn, "FEDFUNDS").unwrap().is_some());
    }

    #[test]
    fn test_count_observations() {
        let conn = open_in_memory();
        assert_eq!(count_observations(&conn).unwrap(), 0);

        upsert_observation(&conn, &make_obs("DGS10", "2026-03-03", dec!(4.07))).unwrap();
        assert_eq!(count_observations(&conn).unwrap(), 1);
    }
}
