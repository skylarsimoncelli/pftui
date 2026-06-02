//! CLI handlers for `pftui data flows {refresh,show}` and
//! `pftui analytics flows summary` (F59 scaffold).
//!
//! All three commands degrade gracefully when the configured provider is
//! the default `NoopProvider` — the refresh path logs that no provider is
//! configured and writes no rows; show/summary read whatever has been
//! persisted (typically zero rows on a fresh install).

use anyhow::Result;
use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::data::flows;
use crate::db::backend::BackendConnection;
use crate::db::capital_flows::{self, AssetFlowAggregate, CapitalFlowRow, FlowFilter};

#[derive(Debug, Serialize)]
pub struct RefreshOutput {
    pub provider: String,
    pub asset_filter: Option<String>,
    pub fetched: usize,
    pub inserted: usize,
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct ShowOutput {
    pub asset: Option<String>,
    pub since: Option<String>,
    pub row_count: usize,
    pub rows: Vec<CapitalFlowRow>,
}

#[derive(Debug, Serialize)]
pub struct SummaryOutput {
    pub since: String,
    pub asset_count: usize,
    pub assets: Vec<AssetFlowAggregate>,
}

/// `pftui data flows refresh [--asset SPY] [--json]`
pub fn refresh(backend: &BackendConnection, asset: Option<String>, json: bool) -> Result<()> {
    let provider = flows::provider_from_env();
    let result = provider.fetch(asset.as_deref())?;
    let conn = backend.sqlite();
    let inserted = capital_flows::insert_many(conn, &result.flows)?;
    let out = RefreshOutput {
        provider: provider.name().to_string(),
        asset_filter: asset,
        fetched: result.flows.len(),
        inserted,
        note: result.note,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "capital flows refresh: provider={} fetched={} inserted={} note={}",
            out.provider, out.fetched, out.inserted, out.note
        );
    }
    Ok(())
}

/// `pftui data flows show [--asset SPY] [--since 30d] [--json]`
pub fn show(
    backend: &BackendConnection,
    asset: Option<String>,
    since: Option<String>,
    json: bool,
) -> Result<()> {
    let since_date = since.as_deref().map(parse_since_to_date).transpose()?;
    let conn = backend.sqlite();
    let rows = capital_flows::list(
        conn,
        &FlowFilter {
            asset: asset.as_deref(),
            since: since_date.as_deref(),
            flow_type: None,
        },
    )?;
    let out = ShowOutput {
        asset: asset.clone(),
        since: since_date.clone(),
        row_count: rows.len(),
        rows,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if out.rows.is_empty() {
        let asset_label = asset.unwrap_or_else(|| "any".to_string());
        let since_label = since_date.unwrap_or_else(|| "all-time".to_string());
        println!("No capital flows for asset={asset_label} since={since_label}.");
    } else {
        println!(
            "Capital flows ({} rows){}{}:",
            out.rows.len(),
            asset.as_ref().map(|a| format!(" asset={a}")).unwrap_or_default(),
            since_date
                .as_ref()
                .map(|s| format!(" since={s}"))
                .unwrap_or_default()
        );
        for row in &out.rows {
            println!(
                "  {}  {:<24} {:>16}  {} → {}  ({})",
                row.asset, row.flow_type, row.amount_usd, row.period_start, row.period_end, row.source
            );
        }
    }
    Ok(())
}

/// `pftui analytics flows summary [--since 7d] [--json]`
pub fn summary(backend: &BackendConnection, since: String, json: bool) -> Result<()> {
    let since_date = parse_since_to_date(&since)?;
    let conn = backend.sqlite();
    let assets = capital_flows::aggregate_by_asset(conn, &since_date)?;
    let out = SummaryOutput {
        since: since_date.clone(),
        asset_count: assets.len(),
        assets,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if out.assets.is_empty() {
        println!("No capital flows since {}.", out.since);
    } else {
        println!("Capital flows summary since {}:", out.since);
        for a in &out.assets {
            println!(
                "  {:<8}  n={:>3}  net={:>16}  top_in={:>16}  top_out={:>16}",
                a.asset, a.flow_count, a.net_flow_usd, a.top_inflow_usd, a.top_outflow_usd
            );
        }
    }
    Ok(())
}

/// Parse a `since` argument as either an absolute YYYY-MM-DD date or a
/// relative `Nd`/`Nw`/`Nm` window (months treated as 30 days).
pub(crate) fn parse_since_to_date(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(d.format("%Y-%m-%d").to_string());
    }
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (num_part, unit) = trimmed.split_at(split_at);
    let n: i64 = num_part.parse().map_err(|_| {
        anyhow::anyhow!(
            "invalid --since: expected NNd/NNw/NNm or YYYY-MM-DD, got '{}'",
            input
        )
    })?;
    let days = match unit {
        "d" | "" => n,
        "w" => n * 7,
        "m" => n * 30,
        other => anyhow::bail!("unknown --since unit '{}' (use d/w/m or YYYY-MM-DD)", other),
    };
    let date = Utc::now().date_naive() - Duration::days(days);
    Ok(date.format("%Y-%m-%d").to_string())
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

    fn seed_btc_and_spy(backend: &BackendConnection) {
        let conn = backend.sqlite();
        capital_flows::insert(
            conn,
            &CapitalFlow {
                asset: "SPY".to_string(),
                flow_type: "etf_creation".to_string(),
                amount_usd: dec!(2_000_000),
                period_start: "2026-06-01".to_string(),
                period_end: "2026-06-01".to_string(),
                source: "synthetic".to_string(),
            },
        )
        .expect("seed spy creation");
        capital_flows::insert(
            conn,
            &CapitalFlow {
                asset: "SPY".to_string(),
                flow_type: "etf_redemption".to_string(),
                amount_usd: dec!(500_000),
                period_start: "2026-06-02".to_string(),
                period_end: "2026-06-02".to_string(),
                source: "synthetic".to_string(),
            },
        )
        .expect("seed spy redemption");
        capital_flows::insert(
            conn,
            &CapitalFlow {
                asset: "BTC".to_string(),
                flow_type: "crypto_exchange_outflow".to_string(),
                amount_usd: dec!(800_000),
                period_start: "2026-06-01".to_string(),
                period_end: "2026-06-01".to_string(),
                source: "synthetic".to_string(),
            },
        )
        .expect("seed btc outflow");
    }

    #[test]
    fn parse_since_handles_iso_and_relative() {
        assert_eq!(parse_since_to_date("2026-04-01").unwrap(), "2026-04-01");
        let _ = parse_since_to_date("30d").expect("30d");
        let _ = parse_since_to_date("2w").expect("2w");
        let _ = parse_since_to_date("3m").expect("3m");
        assert!(parse_since_to_date("bogus").is_err());
        assert!(parse_since_to_date("7q").is_err());
    }

    #[test]
    fn refresh_with_noop_provider_inserts_nothing_and_logs_note() {
        // Force the noop provider via the env var regardless of test env.
        std::env::set_var("PFTUI_FLOWS_PROVIDER", "noop");
        let backend = fresh_backend();
        refresh(&backend, None, true).expect("refresh");
        let rows = capital_flows::list(backend.sqlite(), &FlowFilter::default()).expect("list");
        assert!(rows.is_empty());
    }

    #[test]
    fn summary_aggregates_across_fixture_rows() {
        let backend = fresh_backend();
        seed_btc_and_spy(&backend);
        // Use a 1-year window so all seeded rows are in scope.
        let agg = capital_flows::aggregate_by_asset(backend.sqlite(), "2025-01-01")
            .expect("aggregate");
        assert_eq!(agg.len(), 2);
        let spy = agg.iter().find(|a| a.asset == "SPY").expect("spy");
        // 2M - 500k = 1.5M
        assert!(spy.net_flow_usd.starts_with("1500000"));
        let btc = agg.iter().find(|a| a.asset == "BTC").expect("btc");
        // outflow 800k → -800000
        assert!(btc.net_flow_usd.starts_with("-800000"));
    }
}
