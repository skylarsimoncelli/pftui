//! SQLite cache for per-strike options-chain snapshots.
//!
//! See `src/data/options.rs` for the upstream fetcher. The schema for
//! `options_chain_snapshots` lives in `src/db/schema.rs`.
//!
//! Postgres backend is not yet wired for this table — Yahoo options
//! ingestion is the SQLite primary path. Postgres callers should
//! treat absence-of-rows as "feature disabled in this backend".

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::data::options::OptionsStrikeRow;

/// Insert one strike-row snapshot. Each insert is a new historical
/// row (no upsert) so we keep an append-only ingest log.
pub fn insert_row(conn: &Connection, row: &OptionsStrikeRow, fetched_at: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO options_chain_snapshots (
            symbol, strike, expiry, dte,
            oi_calls, oi_puts, vol_calls, vol_puts,
            iv_atm, fetched_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            row.symbol,
            row.strike,
            row.expiry,
            row.dte,
            row.oi_calls,
            row.oi_puts,
            row.vol_calls,
            row.vol_puts,
            // Use the call-side IV as the per-row IV (puts/calls IV
            // are close in practice; ATM IV is captured separately
            // on the gex_snapshots row).
            row.iv_call,
            fetched_at,
        ],
    )?;
    Ok(())
}

/// Bulk insert all rows from a snapshot inside one transaction.
pub fn insert_chain(
    conn: &Connection,
    rows: &[OptionsStrikeRow],
    fetched_at: &str,
) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    for row in rows {
        insert_row(&tx, row, fetched_at)?;
    }
    tx.commit()?;
    Ok(rows.len())
}

/// Read the most recently ingested chain (all rows for the latest
/// `fetched_at`) for `symbol`. Returns `Ok(vec![])` when no rows
/// have been ingested yet.
pub fn latest_chain(conn: &Connection, symbol: &str) -> Result<Vec<OptionsStrikeRow>> {
    let upper = symbol.to_uppercase();
    let latest_at: Option<String> = conn
        .query_row(
            "SELECT MAX(fetched_at) FROM options_chain_snapshots WHERE symbol = ?1",
            params![upper],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or(None);
    let Some(at) = latest_at else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT symbol, strike, expiry, dte, oi_calls, oi_puts,
                vol_calls, vol_puts, iv_atm
         FROM options_chain_snapshots
         WHERE symbol = ?1 AND fetched_at = ?2
         ORDER BY strike ASC",
    )?;
    let rows = stmt
        .query_map(params![upper, at], |row| {
            Ok(OptionsStrikeRow {
                symbol: row.get(0)?,
                strike: row.get(1)?,
                expiry: row.get(2)?,
                dte: row.get(3)?,
                oi_calls: row.get(4)?,
                oi_puts: row.get(5)?,
                vol_calls: row.get(6)?,
                vol_puts: row.get(7)?,
                iv_call: row.get(8)?,
                iv_put: None,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Return the `fetched_at` of the most recent ingest, if any.
pub fn latest_fetched_at(conn: &Connection, symbol: &str) -> Result<Option<String>> {
    let upper = symbol.to_uppercase();
    let row: Option<String> = conn
        .query_row(
            "SELECT MAX(fetched_at) FROM options_chain_snapshots WHERE symbol = ?1",
            params![upper],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or(None);
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn fresh_conn() -> Connection {
        let c = Connection::open_in_memory().expect("conn");
        schema::run_migrations(&c).expect("migrations");
        c
    }

    #[test]
    fn insert_and_read_back_chain() {
        let c = fresh_conn();
        let rows = vec![
            OptionsStrikeRow {
                symbol: "SPY".into(),
                strike: 540.0,
                expiry: "2026-06-20".into(),
                dte: 14,
                oi_calls: 100,
                oi_puts: 0,
                vol_calls: 0,
                vol_puts: 0,
                iv_call: Some(0.18),
                iv_put: None,
            },
            OptionsStrikeRow {
                symbol: "SPY".into(),
                strike: 560.0,
                expiry: "2026-06-20".into(),
                dte: 14,
                oi_calls: 0,
                oi_puts: 200,
                vol_calls: 0,
                vol_puts: 0,
                iv_call: None,
                iv_put: Some(0.19),
            },
        ];
        insert_chain(&c, &rows, "2026-06-02T00:00:00Z").unwrap();
        let back = latest_chain(&c, "SPY").unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].strike, 540.0);
        assert_eq!(back[1].strike, 560.0);
    }

    #[test]
    fn latest_chain_uses_latest_fetched_at() {
        let c = fresh_conn();
        let r1 = OptionsStrikeRow {
            symbol: "QQQ".into(),
            strike: 500.0,
            expiry: "2026-06-20".into(),
            dte: 14,
            oi_calls: 100,
            oi_puts: 50,
            vol_calls: 0,
            vol_puts: 0,
            iv_call: Some(0.20),
            iv_put: None,
        };
        insert_row(&c, &r1, "2026-06-01T00:00:00Z").unwrap();
        let mut r2 = r1.clone();
        r2.oi_calls = 999;
        insert_row(&c, &r2, "2026-06-02T00:00:00Z").unwrap();
        let back = latest_chain(&c, "QQQ").unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].oi_calls, 999);
    }
}
