//! `analytics regime-break --asset SYM` — CUSUM change-point detection on an
//! asset's daily returns: when did the drift regime last structurally break, and
//! is a fresh break forming now?

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::changepoint::detect_regime_breaks;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    lookback: Option<u32>,
    k_sigma: f64,
    h_sigma: f64,
    json_output: bool,
) -> Result<()> {
    if k_sigma <= 0.0 || h_sigma <= 0.0 {
        bail!("--k and --h must be positive (sigma multiples)");
    }
    let resolved = resolve_alias(symbol);
    let limit = lookback.map(|n| n + 1).unwrap_or(u32::MAX);
    let hist = price_history::get_history(backend.sqlite(), &resolved, limit)?;
    let mut dates = Vec::with_capacity(hist.len());
    let mut rets = Vec::with_capacity(hist.len());
    for w in hist.windows(2) {
        if let (Some(p0), Some(p1)) = (w[0].close.to_f64(), w[1].close.to_f64()) {
            if p0 > 0.0 {
                dates.push(w[1].date.clone());
                rets.push(p1 / p0 - 1.0);
            }
        }
    }
    if rets.len() < 30 {
        bail!(
            "not enough price history for '{symbol}' (resolved '{resolved}') — need ≥30 returns, have {}",
            rets.len()
        );
    }
    let as_of = hist.last().map(|h| h.date.clone()).unwrap_or_default();
    let rb = detect_regime_breaks(&dates, &rets, k_sigma, h_sigma)
        .ok_or_else(|| anyhow::anyhow!("could not run change-point detection"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "regime-break",
                "asset": symbol,
                "resolved_symbol": resolved,
                "as_of": as_of,
                "regime_break": rb,
            }))?
        );
        return Ok(());
    }

    println!("═══ Regime-Break (CUSUM change-point) — {symbol} ({resolved}) ═══");
    println!(
        "As of {as_of} · {} returns · μ {:+.3}%/d · σ {:.3}% · k={:.1}σ h={:.1}σ\n",
        rb.n_obs, rb.mean_return_pct, rb.sigma_pct, rb.k_sigma, rb.h_sigma
    );
    println!("{}.\n", rb.interpretation);
    println!(
        "Building now: up-shift {:.0}% of threshold · down-shift {:.0}% of threshold",
        rb.building_up_pct, rb.building_down_pct
    );
    if !rb.change_points.is_empty() {
        let show = 8.min(rb.change_points.len());
        println!("\nLast {show} regime breaks:");
        for cp in rb.change_points.iter().rev().take(show).rev() {
            println!("  {} {} ({} bars ago)", cp.date, cp.direction, cp.bars_ago);
        }
    }
    Ok(())
}
