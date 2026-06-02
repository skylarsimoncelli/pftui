//! Capital-flows section renderer for the per-asset daily report (F59
//! scaffold).
//!
//! Emits a one-liner whenever the `capital_flows` table has at least one row
//! for `asset` within the last 7 days. The renderer is intentionally minimal
//! while the upstream provider integrations are pending — once a real ETF.com
//! or SEC EDGAR ingest lands, the renderer can be expanded to surface
//! per-source detail.
//!
//! Returns `None` when no qualifying rows exist so the assembler can call it
//! unconditionally per asset without branching.

use anyhow::Result;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db::backend::BackendConnection;
use crate::db::capital_flows::{self, FlowFilter};
use crate::report::build::daily::BuildContext;

/// Lookback window for the per-asset block, in days.
pub const FLOW_WINDOW_DAYS: i64 = 7;

/// Render the capital-flows block for `asset`. Returns `None` when there are
/// no rows in the last `FLOW_WINDOW_DAYS` days.
///
/// `BuildContext` currently does not pre-cache flow rows (the v1 scaffold
/// ingests very few rows from the noop provider), so the renderer hits the
/// SQLite store directly via the backend the assembler is already using.
/// This keeps the BuildContext slim while the real-provider integration is
/// still pending.
#[allow(dead_code)] // Consumed by the daily-report assembler hook (F59 follow-up)
pub fn render_capital_flows_block(
    ctx: &BuildContext,
    backend: &BackendConnection,
    asset: &str,
) -> Result<Option<String>> {
    let _ = ctx; // BuildContext currently carries no flow slot — reserved for future use.
    let since = (Utc::now().date_naive() - Duration::days(FLOW_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let conn = backend.sqlite();
    let rows = capital_flows::list(
        conn,
        &FlowFilter {
            asset: Some(asset),
            since: Some(&since),
            flow_type: None,
        },
    )?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut net = Decimal::ZERO;
    let mut inflow_count = 0usize;
    let mut outflow_count = 0usize;
    for row in &rows {
        let raw = Decimal::from_str(&row.amount_usd).unwrap_or(Decimal::ZERO);
        let is_outflow =
            row.flow_type.contains("redemption") || row.flow_type.contains("outflow");
        if is_outflow {
            net -= raw.abs();
            outflow_count += 1;
        } else {
            net += raw.abs();
            inflow_count += 1;
        }
    }

    let net_label = if net.is_sign_negative() { "OUT" } else { "IN" };
    Ok(Some(format!(
        "Capital flows ({asset}, last {days}d): {n} rows ({inflow} in / {outflow} out), net {label} ${net}",
        days = FLOW_WINDOW_DAYS,
        n = rows.len(),
        inflow = inflow_count,
        outflow = outflow_count,
        label = net_label,
        net = net.abs(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::flows::CapitalFlow;
    use rusqlite::Connection;
    use rust_decimal_macros::dec;

    fn fresh_backend() -> BackendConnection {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        capital_flows::ensure_table(&conn).expect("ensure_table");
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn returns_none_when_no_rows() {
        let backend = fresh_backend();
        let ctx = BuildContext::default();
        let block = render_capital_flows_block(&ctx, &backend, "SPY")
            .expect("render");
        assert!(block.is_none());
    }

    #[test]
    fn emits_one_liner_when_rows_present() {
        let backend = fresh_backend();
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        capital_flows::insert(
            backend.sqlite(),
            &CapitalFlow {
                asset: "SPY".to_string(),
                flow_type: "etf_creation".to_string(),
                amount_usd: dec!(1_000_000),
                period_start: today.clone(),
                period_end: today.clone(),
                source: "synthetic".to_string(),
            },
        )
        .expect("insert");
        capital_flows::insert(
            backend.sqlite(),
            &CapitalFlow {
                asset: "SPY".to_string(),
                flow_type: "etf_redemption".to_string(),
                amount_usd: dec!(250_000),
                period_start: today.clone(),
                period_end: today,
                source: "synthetic".to_string(),
            },
        )
        .expect("insert");
        let ctx = BuildContext::default();
        let block = render_capital_flows_block(&ctx, &backend, "SPY")
            .expect("render")
            .expect("Some block");
        assert!(block.contains("SPY"));
        assert!(block.contains("net IN"));
        assert!(block.contains("750000"));
    }
}
