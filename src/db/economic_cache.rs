//! SQLite cache for FRED economic indicator data.
//!
//! Stores both the latest value (for quick display) and historical observations
//! (for sparklines/trends). Aggressive caching — FRED data rarely changes intraday.

use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};

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

/// Batch upsert multiple observations.
pub fn upsert_observations(conn: &Connection, observations: &[EconomicObservation]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for obs in observations {
        upsert_observation(&tx, obs)?;
    }
    tx.commit()?;
    Ok(())
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

/// Delete all observations for a series (useful for cache invalidation).
pub fn delete_series(conn: &Connection, series_id: &str) -> Result<u64> {
    let count = conn.execute(
        "DELETE FROM economic_cache WHERE series_id = ?1",
        params![series_id],
    )?;
    Ok(count as u64)
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
