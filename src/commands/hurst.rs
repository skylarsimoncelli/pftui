//! `analytics hurst --asset SYM` — Hurst exponent (R/S) trending-vs-mean-
//! reverting regime gauge over an asset's log returns.

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::hurst_rs::hurst;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    lookback: Option<u32>,
    json_output: bool,
) -> Result<()> {
    let resolved = resolve_alias(symbol);
    // Pull a bit more than the lookback so the most-recent window is full.
    let limit = lookback.map(|n| n + 1).unwrap_or(u32::MAX);
    let hist = price_history::get_history(backend.sqlite(), &resolved, limit)?;
    // Log returns (stationary series R/S requires).
    let mut rets: Vec<f64> = Vec::with_capacity(hist.len());
    for w in hist.windows(2) {
        if let (Some(p0), Some(p1)) = (w[0].close.to_f64(), w[1].close.to_f64()) {
            if p0 > 0.0 && p1 > 0.0 {
                rets.push((p1 / p0).ln());
            }
        }
    }
    if rets.len() < 64 {
        bail!(
            "not enough price history for '{symbol}' (resolved '{resolved}') — Hurst R/S needs ≥64 log returns, have {}",
            rets.len()
        );
    }
    let as_of = hist.last().map(|h| h.date.clone()).unwrap_or_default();
    let h = hurst(&rets).ok_or_else(|| anyhow::anyhow!("could not fit Hurst (insufficient windows)"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "hurst",
                "asset": symbol,
                "resolved_symbol": resolved,
                "as_of": as_of,
                "hurst": h,
            }))?
        );
        return Ok(());
    }

    println!("═══ Hurst Exponent (R/S) — {symbol} ({resolved}) ═══");
    println!("As of {as_of} · {} log returns · windows {:?}\n", h.n_obs, h.windows);
    println!(
        "H = {:.3} ({})   [uncorrected {:.3}]",
        h.h, h.regime, h.h_uncorrected
    );
    println!("\n{}.", h.interpretation);
    Ok(())
}
