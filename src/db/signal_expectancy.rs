//! `signal_expectancy` — persisted event-study expectancy rows (L2 derived,
//! deterministically rebuildable via `pftui research backtest`).
//!
//! PK `(signal_id, signal_version, asset, horizon_days, as_of)`: stats bind
//! to the emitter version (registry versioning rule) and to the walk-forward
//! `as_of` so report citations are lookahead-free.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ExpectancyRow {
    pub signal_id: String,
    pub signal_version: String,
    pub asset: String,
    pub horizon_days: i64,
    pub as_of: String,
    pub n_total: i64,
    pub n_evaluable: i64,
    pub n_nonoverlap: i64,
    pub hit_rate: Option<f64>,
    pub baseline_hit_rate: Option<f64>,
    pub hit_lift: Option<f64>,
    pub mean_pct: Option<f64>,
    pub baseline_mean_pct: Option<f64>,
    pub mean_lift: Option<f64>,
    pub median_pct: Option<f64>,
    pub p25: Option<f64>,
    pub p75: Option<f64>,
    pub mae_mean: Option<f64>,
    pub mae_worst: Option<f64>,
    pub mfe_mean: Option<f64>,
    pub p_value: Option<f64>,
    pub significant: bool,
    pub computed_at: Option<String>,
}

/// Upsert (INSERT OR REPLACE on the PK) a batch of expectancy rows.
pub fn upsert_rows(conn: &Connection, rows: &[ExpectancyRow]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO signal_expectancy (
            signal_id, signal_version, asset, horizon_days, as_of,
            n_total, n_evaluable, n_nonoverlap,
            hit_rate, baseline_hit_rate, hit_lift,
            mean_pct, baseline_mean_pct, mean_lift,
            median_pct, p25, p75,
            mae_mean, mae_worst, mfe_mean,
            p_value, significant, computed_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,datetime('now'))",
    )?;
    for r in rows {
        stmt.execute(params![
            r.signal_id,
            r.signal_version,
            r.asset,
            r.horizon_days,
            r.as_of,
            r.n_total,
            r.n_evaluable,
            r.n_nonoverlap,
            r.hit_rate,
            r.baseline_hit_rate,
            r.hit_lift,
            r.mean_pct,
            r.baseline_mean_pct,
            r.mean_lift,
            r.median_pct,
            r.p25,
            r.p75,
            r.mae_mean,
            r.mae_worst,
            r.mfe_mean,
            r.p_value,
            r.significant as i64,
        ])?;
    }
    Ok(())
}

fn row_from_stmt(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExpectancyRow> {
    Ok(ExpectancyRow {
        signal_id: row.get(0)?,
        signal_version: row.get(1)?,
        asset: row.get(2)?,
        horizon_days: row.get(3)?,
        as_of: row.get(4)?,
        n_total: row.get(5)?,
        n_evaluable: row.get(6)?,
        n_nonoverlap: row.get(7)?,
        hit_rate: row.get(8)?,
        baseline_hit_rate: row.get(9)?,
        hit_lift: row.get(10)?,
        mean_pct: row.get(11)?,
        baseline_mean_pct: row.get(12)?,
        mean_lift: row.get(13)?,
        median_pct: row.get(14)?,
        p25: row.get(15)?,
        p75: row.get(16)?,
        mae_mean: row.get(17)?,
        mae_worst: row.get(18)?,
        mfe_mean: row.get(19)?,
        p_value: row.get(20)?,
        significant: row.get::<_, i64>(21)? != 0,
        computed_at: row.get(22)?,
    })
}

const COLS: &str = "signal_id, signal_version, asset, horizon_days, as_of,
    n_total, n_evaluable, n_nonoverlap,
    hit_rate, baseline_hit_rate, hit_lift,
    mean_pct, baseline_mean_pct, mean_lift,
    median_pct, p25, p75,
    mae_mean, mae_worst, mfe_mean,
    p_value, significant, computed_at";

/// Rows at the LATEST as_of per (signal_id, signal_version, asset),
/// optionally filtered by signal and/or asset.
pub fn latest_rows(
    conn: &Connection,
    signal: Option<&str>,
    asset: Option<&str>,
) -> Result<Vec<ExpectancyRow>> {
    let sql = format!(
        "SELECT {COLS} FROM signal_expectancy s1
         WHERE as_of = (
             SELECT MAX(as_of) FROM signal_expectancy s2
             WHERE s2.signal_id = s1.signal_id
               AND s2.signal_version = s1.signal_version
               AND s2.asset = s1.asset
         )
           AND (?1 IS NULL OR signal_id = ?1)
           AND (?2 IS NULL OR asset = ?2)
         ORDER BY signal_id, asset, horizon_days"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![signal, asset], row_from_stmt)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap_or_else(|e| panic!("mem db: {e}"));
        crate::db::schema::run_migrations(&conn).unwrap_or_else(|e| panic!("migrations: {e}"));
        conn
    }

    fn row(signal: &str, asset: &str, horizon: i64, as_of: &str) -> ExpectancyRow {
        ExpectancyRow {
            signal_id: signal.to_string(),
            signal_version: "1".to_string(),
            asset: asset.to_string(),
            horizon_days: horizon,
            as_of: as_of.to_string(),
            n_total: 12,
            n_evaluable: 11,
            n_nonoverlap: 10,
            hit_rate: Some(0.7),
            baseline_hit_rate: Some(0.55),
            hit_lift: Some(0.15),
            mean_pct: Some(3.2),
            baseline_mean_pct: Some(1.1),
            mean_lift: Some(2.1),
            median_pct: Some(2.8),
            p25: Some(-1.0),
            p75: Some(6.0),
            mae_mean: Some(-4.5),
            mae_worst: Some(-12.0),
            mfe_mean: Some(7.7),
            p_value: Some(0.03),
            significant: true,
            computed_at: None,
        }
    }

    #[test]
    fn upsert_and_latest_roundtrip() {
        let conn = test_conn();
        upsert_rows(
            &conn,
            &[
                row("sig_a", "DEMO", 30, "2026-01-01"),
                row("sig_a", "DEMO", 30, "2026-02-01"),
                row("sig_a", "DEMO", 90, "2026-02-01"),
                row("sig_b", "DEMO", 30, "2026-01-15"),
            ],
        )
        .unwrap_or_else(|e| panic!("upsert: {e}"));

        let rows = latest_rows(&conn, None, Some("DEMO")).unwrap_or_default();
        // sig_a latest as_of = 2026-02-01 (2 horizons) + sig_b (1 horizon).
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .filter(|r| r.signal_id == "sig_a")
            .all(|r| r.as_of == "2026-02-01"));

        let only_a = latest_rows(&conn, Some("sig_a"), None).unwrap_or_default();
        assert_eq!(only_a.len(), 2);
        assert!(only_a[0].significant);
        assert_eq!(only_a[0].mae_worst, Some(-12.0));
    }

    #[test]
    fn upsert_replaces_on_pk_conflict() {
        let conn = test_conn();
        let mut r = row("sig_a", "DEMO", 30, "2026-01-01");
        upsert_rows(&conn, std::slice::from_ref(&r)).unwrap_or_else(|e| panic!("{e}"));
        r.mean_pct = Some(9.9);
        upsert_rows(&conn, std::slice::from_ref(&r)).unwrap_or_else(|e| panic!("{e}"));
        let rows = latest_rows(&conn, Some("sig_a"), Some("DEMO")).unwrap_or_default();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mean_pct, Some(9.9));
    }
}
