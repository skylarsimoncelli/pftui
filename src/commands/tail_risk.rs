//! `analytics tail-risk <SYM>` — Extreme-Value-Theory (POT/GPD) tail risk for
//! an asset's daily returns: fat-tail-aware VaR / Expected-Shortfall plus the
//! GPD shape ξ (how heavy the left tail is), with the historical estimate
//! alongside for comparison.

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::evt::fit_evt_tail_risk;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    lookback_days: Option<u32>,
    threshold_pct: f64,
    json_output: bool,
) -> Result<()> {
    let resolved = resolve_alias(symbol);
    let limit = lookback_days.unwrap_or(u32::MAX);
    let hist = price_history::get_history(backend.sqlite(), &resolved, limit)?;
    if hist.len() < 101 {
        bail!(
            "not enough price history for '{symbol}' (resolved '{resolved}') — EVT needs ≥101 bars, have {}",
            hist.len()
        );
    }
    if !(80.0..=99.0).contains(&threshold_pct) {
        bail!("--threshold must be between 80 and 99 (percentile of the loss distribution); got {threshold_pct}");
    }
    // Daily simple returns from the close series (oldest→newest).
    let closes: Vec<f64> = hist.iter().map(|h| h.close.to_f64().unwrap_or(0.0)).collect();
    let returns: Vec<f64> = closes
        .windows(2)
        .filter_map(|w| if w[0] > 0.0 { Some(w[1] / w[0] - 1.0) } else { None })
        .collect();
    let as_of = hist.last().map(|h| h.date.clone()).unwrap_or_default();

    let evt = fit_evt_tail_risk(&returns, threshold_pct / 100.0)
        .ok_or_else(|| anyhow::anyhow!("could not fit EVT tail (need ≥100 daily returns)"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "tail-risk",
                "asset": symbol,
                "resolved_symbol": resolved,
                "as_of": as_of,
                "threshold_quantile": threshold_pct / 100.0,
                "evt": evt,
            }))?
        );
        return Ok(());
    }

    println!("═══ Tail Risk (EVT / Peaks-Over-Threshold) — {symbol} ({resolved}) ═══");
    println!("As of {as_of} · {} daily returns · threshold = {:.0}th pct of losses\n", evt.n_obs, threshold_pct);
    println!(
        "GPD fit:   ξ (shape) {:+.3} → {} | σ (scale) {:.2}% | {} exceedances over {:.2}% loss",
        evt.xi, evt.tail_class, evt.sigma_pct, evt.n_exceedances, evt.threshold_pct,
    );
    println!("1-day Value-at-Risk (loss you exceed with the stated probability):");
    println!(
        "  EVT:        95% {:.1}%  ·  99% {:.1}%  ·  99.9% {:.1}%",
        evt.var_95_pct, evt.var_99_pct, evt.var_999_pct,
    );
    println!(
        "  Historical: 99% {:.1}%   (EVT 99% is {:.2}× the empirical estimate)",
        evt.hist_var_99_pct,
        if evt.hist_var_99_pct > 0.0 { evt.var_99_pct / evt.hist_var_99_pct } else { 1.0 },
    );
    println!(
        "Expected Shortfall (avg loss BEYOND the 99% VaR):  EVT {:.1}%  ·  historical {:.1}%",
        evt.es_99_pct, evt.hist_es_99_pct,
    );
    println!();
    if !evt.reliable {
        println!("⚠ {}", evt.note);
    } else {
        println!("{}", evt.note);
    }
    Ok(())
}
