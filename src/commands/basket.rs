//! `analytics basket weights --assets A,B,C [--method ...] [--lookback N]` —
//! risk-aware portfolio weights (equal / inverse-vol / risk-parity) over a
//! basket's common price history, with per-asset risk contributions and the
//! diversification ratio. Read-only over `price_history`; no portfolio data.

use std::collections::HashMap;

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::basket::{allocate, Method};
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Per-asset close keyed by date (positive closes only).
fn dated_closes(backend: &BackendConnection, resolved: &str) -> Result<HashMap<String, f64>> {
    let hist = price_history::get_history(backend.sqlite(), resolved, u32::MAX)?;
    Ok(hist
        .into_iter()
        .filter_map(|r| r.close.to_f64().map(|c| (r.date, c)))
        .filter(|(_, c)| *c > 0.0)
        .collect())
}

pub fn run(backend: &BackendConnection, assets: &str, method: &str, lookback: usize, json_output: bool) -> Result<()> {
    let method = Method::parse(method).ok_or_else(|| {
        anyhow::anyhow!("unknown --method '{method}' (use: equal | inverse-vol | risk-parity)")
    })?;
    // Resolve + de-dup the basket, preserving input order.
    let mut symbols: Vec<String> = Vec::new();
    for raw in assets.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let r = resolve_alias(raw);
        if !symbols.contains(&r) {
            symbols.push(r);
        }
    }
    if symbols.len() < 2 {
        bail!("need at least 2 distinct assets in --assets (got {})", symbols.len());
    }

    // Load each asset's dated closes, then intersect on the COMMON date axis so
    // every return spans the same calendar interval across assets.
    let maps: Vec<HashMap<String, f64>> = symbols
        .iter()
        .map(|s| dated_closes(backend, s))
        .collect::<Result<_>>()?;
    for (sym, m) in symbols.iter().zip(&maps) {
        if m.is_empty() {
            bail!("no price history for '{sym}' — check the symbol/alias or run `pftui data refresh`");
        }
    }
    let mut common: Vec<String> = maps[0].keys().cloned().collect();
    common.retain(|d| maps[1..].iter().all(|m| m.contains_key(d)));
    common.sort();
    // Keep only the most-recent `lookback` common dates (lookback+1 closes →
    // lookback returns) when a window is requested.
    if lookback > 0 && common.len() > lookback + 1 {
        common = common.split_off(common.len() - (lookback + 1));
    }
    if common.len() < 21 {
        bail!(
            "only {} common dates across the basket — need ≥21 common dates (≥20 aligned returns) for a covariance estimate (assets may not overlap in time)",
            common.len()
        );
    }

    // Consecutive-day simple returns over the common axis, per asset.
    let series: Vec<Vec<f64>> = maps
        .iter()
        .map(|m| {
            common
                .windows(2)
                .map(|w| m[&w[1]] / m[&w[0]] - 1.0)
                .collect::<Vec<f64>>()
        })
        .collect();

    let alloc = allocate(&symbols, &series, method)
        .ok_or_else(|| anyhow::anyhow!("not enough aligned data to allocate"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "basket weights",
                "requested": assets,
                "as_of": common.last(),
                "lookback_obs": alloc.n_obs,
                "allocation": alloc,
            }))?
        );
        return Ok(());
    }

    println!("═══ Basket Allocation — {} ({}) ═══", alloc.method, symbols.join(", "));
    println!(
        "{} common daily returns · as of {}\n",
        alloc.n_obs,
        common.last().map(|s| s.as_str()).unwrap_or("—")
    );
    println!("{:<12} {:>8} {:>10} {:>14}", "Asset", "Weight", "Vol/yr", "Risk-Contrib");
    for w in &alloc.weights {
        println!(
            "{:<12} {:>7.1}% {:>9.1}% {:>13.1}%",
            w.symbol,
            w.weight * 100.0,
            w.vol_pct,
            w.risk_contribution * 100.0,
        );
    }
    println!(
        "\nPortfolio vol {:.1}%/yr · diversification ratio {:.2} (higher = more benefit captured)",
        alloc.portfolio_vol_pct, alloc.diversification_ratio
    );
    Ok(())
}
