//! SQLite cache for GEX (Gamma Exposure) summary snapshots.
//!
//! See `src/data/options.rs` for the computation. Schema lives in
//! `src/db/schema.rs`. Append-only — each `data options refresh`
//! emits one row per symbol.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::data::options::GexSummary;

pub fn insert(conn: &Connection, gex: &GexSummary) -> Result<()> {
    conn.execute(
        "INSERT INTO gex_snapshots (
            symbol, gex_flip_strike, total_gamma_call,
            total_gamma_put, max_pain, fetched_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            gex.symbol,
            gex.gex_flip_strike,
            gex.total_gamma_call,
            gex.total_gamma_put,
            gex.max_pain,
            gex.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn latest(conn: &Connection, symbol: &str) -> Result<Option<GexSummary>> {
    let upper = symbol.to_uppercase();
    let mut stmt = conn.prepare(
        "SELECT symbol, gex_flip_strike, total_gamma_call,
                total_gamma_put, max_pain, fetched_at
         FROM gex_snapshots
         WHERE symbol = ?1
         ORDER BY fetched_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![upper])?;
    if let Some(row) = rows.next()? {
        Ok(Some(GexSummary {
            symbol: row.get(0)?,
            gex_flip_strike: row.get::<_, Option<f64>>(1)?,
            total_gamma_call: row.get(2)?,
            total_gamma_put: row.get(3)?,
            max_pain: row.get::<_, Option<f64>>(4)?,
            fetched_at: row.get(5)?,
        }))
    } else {
        Ok(None)
    }
}

/// Read all symbols with at least one GEX snapshot.
pub fn list_symbols(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT symbol FROM gex_snapshots ORDER BY symbol")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn fresh_conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        schema::run_migrations(&c).unwrap();
        c
    }

    #[test]
    fn insert_and_read_latest() {
        let c = fresh_conn();
        let g = GexSummary {
            symbol: "SPY".into(),
            gex_flip_strike: Some(550.0),
            total_gamma_call: 12_345.6,
            total_gamma_put: 9_876.5,
            max_pain: Some(548.0),
            fetched_at: "2026-06-02T00:00:00Z".into(),
        };
        insert(&c, &g).unwrap();
        let back = latest(&c, "SPY").unwrap().expect("row");
        assert_eq!(back.symbol, "SPY");
        assert_eq!(back.gex_flip_strike, Some(550.0));
        assert_eq!(back.max_pain, Some(548.0));
    }

    #[test]
    fn latest_returns_most_recent() {
        let c = fresh_conn();
        insert(
            &c,
            &GexSummary {
                symbol: "QQQ".into(),
                gex_flip_strike: Some(500.0),
                total_gamma_call: 1.0,
                total_gamma_put: 1.0,
                max_pain: Some(499.0),
                fetched_at: "2026-06-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        insert(
            &c,
            &GexSummary {
                symbol: "QQQ".into(),
                gex_flip_strike: Some(510.0),
                total_gamma_call: 2.0,
                total_gamma_put: 2.0,
                max_pain: Some(509.0),
                fetched_at: "2026-06-02T00:00:00Z".into(),
            },
        )
        .unwrap();
        let g = latest(&c, "QQQ").unwrap().expect("row");
        assert_eq!(g.gex_flip_strike, Some(510.0));
        assert_eq!(list_symbols(&c).unwrap(), vec!["QQQ".to_string()]);
    }
}
