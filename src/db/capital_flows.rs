//! SQLite-backed store for capital-flow rows (F59 scaffold).
//!
//! Schema (idempotent via `ensure_table` and also mirrored into
//! `db::schema::run_migrations` for the canonical migration path):
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS capital_flows (
//!     id INTEGER PRIMARY KEY AUTOINCREMENT,
//!     asset TEXT NOT NULL,
//!     flow_type TEXT NOT NULL CHECK(flow_type IN (
//!         'etf_creation','etf_redemption','institutional_13f',
//!         'crypto_exchange_inflow','crypto_exchange_outflow'
//!     )),
//!     amount_usd TEXT NOT NULL,
//!     period_start TEXT NOT NULL,
//!     period_end TEXT NOT NULL,
//!     source TEXT NOT NULL,
//!     fetched_at TEXT NOT NULL
//! );
//! ```
//!
//! `amount_usd` is stored as TEXT (decimal string) per the project's
//! `rust_decimal` standard for monetary values.

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::data::flows::{validate_flow_type, CapitalFlow};

/// One persisted capital-flow row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapitalFlowRow {
    pub id: i64,
    pub asset: String,
    pub flow_type: String,
    pub amount_usd: String,
    pub period_start: String,
    pub period_end: String,
    pub source: String,
    pub fetched_at: String,
}

impl CapitalFlowRow {
    /// Parse the stored decimal-string `amount_usd` into a `Decimal`.
    pub fn amount_decimal(&self) -> Result<Decimal> {
        Decimal::from_str(&self.amount_usd)
            .map_err(|e| anyhow!("invalid amount_usd '{}': {e}", self.amount_usd))
    }
}

/// Idempotent table creation. Mirrors the schema in `schema::run_migrations`.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS capital_flows (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            asset TEXT NOT NULL,
            flow_type TEXT NOT NULL CHECK(flow_type IN (
                'etf_creation','etf_redemption','institutional_13f',
                'crypto_exchange_inflow','crypto_exchange_outflow'
            )),
            amount_usd TEXT NOT NULL,
            period_start TEXT NOT NULL,
            period_end TEXT NOT NULL,
            source TEXT NOT NULL,
            fetched_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_capital_flows_asset
            ON capital_flows(asset);
        CREATE INDEX IF NOT EXISTS idx_capital_flows_period_end
            ON capital_flows(period_end);",
    )?;
    Ok(())
}

/// Insert one `CapitalFlow`. Returns the new row id.
pub fn insert(conn: &Connection, flow: &CapitalFlow) -> Result<i64> {
    ensure_table(conn)?;
    validate_flow_type(&flow.flow_type)?;
    let fetched_at = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO capital_flows
            (asset, flow_type, amount_usd, period_start, period_end, source, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            flow.asset,
            flow.flow_type,
            flow.amount_usd.to_string(),
            flow.period_start,
            flow.period_end,
            flow.source,
            fetched_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Batch insert. Returns the count actually inserted.
pub fn insert_many(conn: &Connection, flows: &[CapitalFlow]) -> Result<usize> {
    ensure_table(conn)?;
    let mut n = 0usize;
    for flow in flows {
        insert(conn, flow)?;
        n += 1;
    }
    Ok(n)
}

/// Filters for the `list` query.
#[derive(Debug, Default, Clone)]
pub struct FlowFilter<'a> {
    pub asset: Option<&'a str>,
    /// ISO date (YYYY-MM-DD) — only rows with `period_end >= since` are returned.
    pub since: Option<&'a str>,
    pub flow_type: Option<&'a str>,
}

/// List capital_flows matching the filter, newest period_end first.
pub fn list(conn: &Connection, filter: &FlowFilter) -> Result<Vec<CapitalFlowRow>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, asset, flow_type, amount_usd, period_start, period_end, source, fetched_at
         FROM capital_flows WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(asset) = filter.asset {
        sql.push_str(" AND asset = ?");
        args.push(Box::new(asset.to_string()));
    }
    if let Some(since) = filter.since {
        sql.push_str(" AND period_end >= ?");
        args.push(Box::new(since.to_string()));
    }
    if let Some(flow_type) = filter.flow_type {
        sql.push_str(" AND flow_type = ?");
        args.push(Box::new(flow_type.to_string()));
    }
    sql.push_str(" ORDER BY period_end DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(CapitalFlowRow {
                id: row.get(0)?,
                asset: row.get(1)?,
                flow_type: row.get(2)?,
                amount_usd: row.get(3)?,
                period_start: row.get(4)?,
                period_end: row.get(5)?,
                source: row.get(6)?,
                fetched_at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Per-asset aggregate over a rolling window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetFlowAggregate {
    pub asset: String,
    pub flow_count: i64,
    /// Sum of every flow row, with `*_redemption` and `*_outflow` rows
    /// counted as negative.
    pub net_flow_usd: String,
    pub top_inflow_usd: String,
    pub top_outflow_usd: String,
}

/// Compute a per-asset aggregate of `capital_flows` rows where
/// `period_end >= since`. Returns a vector sorted by asset name for
/// deterministic output.
pub fn aggregate_by_asset(conn: &Connection, since: &str) -> Result<Vec<AssetFlowAggregate>> {
    let filter = FlowFilter {
        asset: None,
        since: Some(since),
        flow_type: None,
    };
    let rows = list(conn, &filter)?;
    let mut buckets: BTreeMap<String, Vec<(Decimal, bool)>> = BTreeMap::new();
    for row in rows {
        let signed = signed_amount(&row)?;
        buckets
            .entry(row.asset)
            .or_default()
            .push((signed, row.flow_type.contains("redemption") || row.flow_type.contains("outflow")));
    }

    let mut out = Vec::with_capacity(buckets.len());
    for (asset, entries) in buckets {
        let mut net = Decimal::ZERO;
        let mut top_inflow = Decimal::ZERO;
        let mut top_outflow = Decimal::ZERO;
        for (signed, is_outflow) in &entries {
            net += *signed;
            if *is_outflow {
                if *signed < top_outflow {
                    top_outflow = *signed;
                }
            } else if *signed > top_inflow {
                top_inflow = *signed;
            }
        }
        out.push(AssetFlowAggregate {
            asset,
            flow_count: entries.len() as i64,
            net_flow_usd: net.to_string(),
            top_inflow_usd: top_inflow.to_string(),
            top_outflow_usd: top_outflow.to_string(),
        });
    }
    Ok(out)
}

/// Return the most-recent `fetched_at` (RFC3339) for any row matching
/// `flow_type`, or `None` when the table has no such row. Used by the
/// refresh hook to enforce per-provider cadence throttles (e.g.
/// `sec_edgar_13f` is quarterly).
pub fn latest_fetched_at_for_type(
    conn: &Connection,
    flow_type: &str,
) -> Result<Option<String>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT fetched_at FROM capital_flows WHERE flow_type = ?1
         ORDER BY fetched_at DESC LIMIT 1",
    )?;
    let mut rows = stmt.query(params![flow_type])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get::<_, String>(0)?))
    } else {
        Ok(None)
    }
}

/// Sign-apply the `amount_usd` based on whether the flow type indicates
/// an outflow/redemption.
fn signed_amount(row: &CapitalFlowRow) -> Result<Decimal> {
    let raw = row.amount_decimal()?;
    let is_outflow = row.flow_type.contains("redemption") || row.flow_type.contains("outflow");
    let positive = raw.abs();
    Ok(if is_outflow { -positive } else { positive })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        ensure_table(&conn).expect("ensure_table");
        conn
    }

    fn mk_flow(asset: &str, flow_type: &str, amount: Decimal, end: &str) -> CapitalFlow {
        CapitalFlow {
            asset: asset.to_string(),
            flow_type: flow_type.to_string(),
            amount_usd: amount,
            period_start: end.to_string(),
            period_end: end.to_string(),
            source: "synthetic-test".to_string(),
        }
    }

    #[test]
    fn ensure_table_is_idempotent() {
        let conn = fresh_conn();
        ensure_table(&conn).expect("second call");
        ensure_table(&conn).expect("third call");
    }

    #[test]
    fn insert_then_list_round_trip() {
        let conn = fresh_conn();
        let id = insert(
            &conn,
            &mk_flow("SPY", "etf_creation", dec!(1_500_000), "2026-06-01"),
        )
        .expect("insert");
        assert!(id > 0);
        let rows = list(&conn, &FlowFilter::default()).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset, "SPY");
        assert_eq!(rows[0].flow_type, "etf_creation");
        assert_eq!(rows[0].amount_decimal().expect("amount"), dec!(1_500_000));
    }

    #[test]
    fn insert_rejects_unknown_flow_type() {
        let conn = fresh_conn();
        let bad = CapitalFlow {
            asset: "SPY".to_string(),
            flow_type: "spaceship_landing".to_string(),
            amount_usd: dec!(1),
            period_start: "2026-06-01".to_string(),
            period_end: "2026-06-01".to_string(),
            source: "synthetic".to_string(),
        };
        assert!(insert(&conn, &bad).is_err());
    }

    #[test]
    fn list_filters_by_asset_since_and_type() {
        let conn = fresh_conn();
        insert(
            &conn,
            &mk_flow("SPY", "etf_creation", dec!(2_000_000), "2026-06-01"),
        )
        .expect("a");
        insert(
            &conn,
            &mk_flow("SPY", "etf_redemption", dec!(500_000), "2026-05-15"),
        )
        .expect("b");
        insert(
            &conn,
            &mk_flow("BTC", "crypto_exchange_outflow", dec!(800_000), "2026-06-02"),
        )
        .expect("c");

        let spy = list(
            &conn,
            &FlowFilter {
                asset: Some("SPY"),
                since: None,
                flow_type: None,
            },
        )
        .expect("list spy");
        assert_eq!(spy.len(), 2);

        let recent_spy = list(
            &conn,
            &FlowFilter {
                asset: Some("SPY"),
                since: Some("2026-05-20"),
                flow_type: None,
            },
        )
        .expect("list spy recent");
        assert_eq!(recent_spy.len(), 1);
        assert_eq!(recent_spy[0].period_end, "2026-06-01");

        let creations = list(
            &conn,
            &FlowFilter {
                asset: None,
                since: None,
                flow_type: Some("etf_creation"),
            },
        )
        .expect("list creations");
        assert_eq!(creations.len(), 1);
    }

    #[test]
    fn aggregate_by_asset_signs_outflows_negative_and_finds_extremes() {
        let conn = fresh_conn();
        // SPY: +2M creation, +1M creation, -500k redemption → net +2.5M
        insert(
            &conn,
            &mk_flow("SPY", "etf_creation", dec!(2_000_000), "2026-06-01"),
        )
        .expect("a");
        insert(
            &conn,
            &mk_flow("SPY", "etf_creation", dec!(1_000_000), "2026-06-02"),
        )
        .expect("b");
        insert(
            &conn,
            &mk_flow("SPY", "etf_redemption", dec!(500_000), "2026-06-03"),
        )
        .expect("c");
        // BTC: outflow then inflow
        insert(
            &conn,
            &mk_flow("BTC", "crypto_exchange_outflow", dec!(800_000), "2026-06-01"),
        )
        .expect("d");
        insert(
            &conn,
            &mk_flow("BTC", "crypto_exchange_inflow", dec!(300_000), "2026-06-02"),
        )
        .expect("e");

        let agg = aggregate_by_asset(&conn, "2026-05-01").expect("aggregate");
        // Sorted alphabetically by asset.
        assert_eq!(agg.len(), 2);
        assert_eq!(agg[0].asset, "BTC");
        let btc_net = Decimal::from_str(&agg[0].net_flow_usd).expect("btc net");
        assert_eq!(btc_net, dec!(-500_000));
        let btc_top_in = Decimal::from_str(&agg[0].top_inflow_usd).expect("btc top in");
        assert_eq!(btc_top_in, dec!(300_000));
        let btc_top_out = Decimal::from_str(&agg[0].top_outflow_usd).expect("btc top out");
        assert_eq!(btc_top_out, dec!(-800_000));

        assert_eq!(agg[1].asset, "SPY");
        let spy_net = Decimal::from_str(&agg[1].net_flow_usd).expect("spy net");
        assert_eq!(spy_net, dec!(2_500_000));
    }

    #[test]
    fn latest_fetched_at_returns_none_for_empty_and_populated_for_match() {
        let conn = fresh_conn();
        assert!(
            latest_fetched_at_for_type(&conn, "institutional_13f")
                .expect("query empty")
                .is_none()
        );
        insert(
            &conn,
            &mk_flow("AAPL_CUSIP", "institutional_13f", dec!(150_000_000), "2026-03-31"),
        )
        .expect("insert");
        let latest = latest_fetched_at_for_type(&conn, "institutional_13f")
            .expect("query populated");
        let stamp = latest.expect("some value present");
        // RFC3339 prefix sanity check.
        assert!(stamp.len() >= 10);
        // Other flow_types still see None.
        assert!(
            latest_fetched_at_for_type(&conn, "etf_creation")
                .expect("query empty")
                .is_none()
        );
    }

    #[test]
    fn aggregate_window_excludes_older_rows() {
        let conn = fresh_conn();
        insert(
            &conn,
            &mk_flow("SPY", "etf_creation", dec!(1_000_000), "2026-01-01"),
        )
        .expect("old");
        insert(
            &conn,
            &mk_flow("SPY", "etf_creation", dec!(750_000), "2026-06-01"),
        )
        .expect("new");
        let agg = aggregate_by_asset(&conn, "2026-05-01").expect("aggregate");
        assert_eq!(agg.len(), 1);
        let net = Decimal::from_str(&agg[0].net_flow_usd).expect("net");
        assert_eq!(net, dec!(750_000));
    }
}
