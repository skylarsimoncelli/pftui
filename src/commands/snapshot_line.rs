//! `pftui data snapshot-line` — one deterministic market-context line.
//!
//! Format: `<YYYY-MM-DD> | SPX <close> | BTC <close> | GOLD <close> |
//! SILVER <close> | DXY <close> | VIX <close>` from the latest cached
//! closes (deep-series fallback for BTC; a series with no history is
//! omitted rather than invented).
//!
//! Epistemics rationale: every journal note or entry written with
//! `--stamp` carries the market state it was written under, so
//! retro-scoring and post-mortems are self-contextualizing — "what was the
//! tape when we believed this?" stops requiring a price-history join.

use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db::backend::BackendConnection;

/// Field label → ordered symbol fallback chain.
const FIELDS: &[(&str, &[&str])] = &[
    ("SPX", &["^GSPC"]),
    ("BTC", &["BTC", "BTC-USD"]),
    ("GOLD", &["GC=F"]),
    ("SILVER", &["SI=F"]),
    ("DXY", &["DX-Y.NYB"]),
    ("VIX", &["^VIX"]),
];

fn sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("data snapshot-line requires the SQLite backend"))
}

/// Latest close for the first symbol in `chain` that has any history.
fn latest_close(conn: &Connection, chain: &[&str]) -> Result<Option<(String, Decimal)>> {
    for sym in chain {
        let row: Option<String> = conn
            .prepare("SELECT close FROM price_history WHERE symbol = ?1 ORDER BY date DESC LIMIT 1")?
            .query_row([sym], |row| row.get(0))
            .ok();
        if let Some(close) = row.and_then(|c| Decimal::from_str(&c).ok()) {
            if !close.is_zero() {
                return Ok(Some((sym.to_string(), close)));
            }
        }
    }
    Ok(None)
}

fn fmt_close(v: Decimal) -> String {
    v.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .normalize()
        .to_string()
}

/// Resolved snapshot fields: `(label, symbol, close)`.
pub fn snapshot_fields(conn: &Connection) -> Result<Vec<(String, String, Decimal)>> {
    let mut out = Vec::new();
    for (label, chain) in FIELDS {
        if let Some((symbol, close)) = latest_close(conn, chain)? {
            out.push((label.to_string(), symbol, close));
        }
    }
    Ok(out)
}

/// Build the snapshot line for `date`. Missing series are omitted; returns
/// `None` when no field can be priced at all (empty price history).
pub fn build_snapshot_line(conn: &Connection, date: &str) -> Result<Option<String>> {
    let fields = snapshot_fields(conn)?;
    if fields.is_empty() {
        return Ok(None);
    }
    let parts: Vec<String> = fields
        .iter()
        .map(|(label, _, close)| format!("{label} {}", fmt_close(*close)))
        .collect();
    Ok(Some(format!("{date} | {}", parts.join(" | "))))
}

/// Today's snapshot line for `--stamp` prepending. Best-effort: any failure
/// (Postgres backend, empty history) degrades to `None` so a journal write
/// never fails because the stamp could not be built.
pub fn stamp_prefix(backend: &BackendConnection) -> Option<String> {
    let conn = backend.sqlite_native()?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    build_snapshot_line(conn, &today).ok().flatten()
}

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let conn = sqlite(backend)?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let line = build_snapshot_line(conn, &today)?;
    if json {
        let fields = snapshot_fields(conn)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "date": today,
                "line": line,
                "fields": fields
                    .iter()
                    .map(|(label, symbol, close)| serde_json::json!({
                        "label": label,
                        "symbol": symbol,
                        "close": fmt_close(*close),
                    }))
                    .collect::<Vec<_>>(),
            }))?
        );
    } else {
        match line {
            Some(line) => println!("{line}"),
            None => println!(
                "No cached price history for any snapshot series. Run `pftui data refresh --only prices` first."
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'test',
                PRIMARY KEY (symbol, date)
            );",
        )
        .expect("schema");
        conn
    }

    fn close(conn: &Connection, symbol: &str, date: &str, close: &str) {
        conn.execute(
            "INSERT OR REPLACE INTO price_history (symbol, date, close) VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol, date, close],
        )
        .expect("insert close");
    }

    #[test]
    fn full_line_format_is_deterministic() {
        let conn = test_conn();
        close(&conn, "^GSPC", "2026-06-10", "6543.21");
        close(&conn, "BTC", "2026-06-10", "104250.5");
        close(&conn, "GC=F", "2026-06-10", "3380.00");
        close(&conn, "SI=F", "2026-06-10", "36.125");
        close(&conn, "DX-Y.NYB", "2026-06-10", "98.45");
        close(&conn, "^VIX", "2026-06-10", "17.9");

        let line = build_snapshot_line(&conn, "2026-06-11").unwrap().unwrap();
        assert_eq!(
            line,
            "2026-06-11 | SPX 6543.21 | BTC 104250.5 | GOLD 3380 | SILVER 36.13 | DXY 98.45 | VIX 17.9"
        );
    }

    #[test]
    fn missing_series_are_omitted_not_invented() {
        let conn = test_conn();
        close(&conn, "^GSPC", "2026-06-10", "6500");
        close(&conn, "GC=F", "2026-06-10", "3380");

        let line = build_snapshot_line(&conn, "2026-06-11").unwrap().unwrap();
        assert_eq!(line, "2026-06-11 | SPX 6500 | GOLD 3380");
    }

    #[test]
    fn btc_falls_back_to_deep_series() {
        let conn = test_conn();
        close(&conn, "BTC-USD", "2026-06-10", "104000");

        let fields = snapshot_fields(&conn).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "BTC");
        assert_eq!(fields[0].1, "BTC-USD");

        let line = build_snapshot_line(&conn, "2026-06-11").unwrap().unwrap();
        assert_eq!(line, "2026-06-11 | BTC 104000");
    }

    #[test]
    fn bare_btc_preferred_over_deep_series() {
        let conn = test_conn();
        close(&conn, "BTC", "2026-06-10", "104100");
        close(&conn, "BTC-USD", "2026-06-10", "104000");
        let fields = snapshot_fields(&conn).unwrap();
        assert_eq!(fields[0].1, "BTC");
    }

    #[test]
    fn empty_history_yields_no_line() {
        let conn = test_conn();
        assert!(build_snapshot_line(&conn, "2026-06-11").unwrap().is_none());
    }

    #[test]
    fn latest_close_wins_over_older_rows() {
        let conn = test_conn();
        close(&conn, "^VIX", "2026-06-01", "25");
        close(&conn, "^VIX", "2026-06-10", "18");
        let line = build_snapshot_line(&conn, "2026-06-11").unwrap().unwrap();
        assert_eq!(line, "2026-06-11 | VIX 18");
    }
}
