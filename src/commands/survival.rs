//! `analytics survival --asset SYM` — drawdown survival & recovery: the TIME
//! and SOLVENCY axis (Triple Penance + risk-of-ruin) that complements the
//! depth-only EVT/CDaR risk views. Read-only over `price_history`.

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::strategy::resolver::resolve_alias;
use crate::analytics::{drawdown_metrics, survival};
use crate::db::backend::BackendConnection;
use crate::db::price_history;

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    budget_pct: f64,
    confidence: f64,
    lookback: usize,
    json_output: bool,
) -> Result<()> {
    if budget_pct <= 0.0 || budget_pct >= 100.0 {
        bail!("--budget is a drawdown percent and must be in (0, 100) (got {budget_pct})");
    }
    if !(0.5..1.0).contains(&confidence) {
        bail!("--confidence must be in [0.5, 1.0) (got {confidence})");
    }
    let resolved = resolve_alias(symbol);
    let hist = price_history::get_history(backend.sqlite(), &resolved, u32::MAX)?;
    let mut closes: Vec<f64> = hist.iter().filter_map(|b| b.close.to_f64()).filter(|c| *c > 0.0).collect();
    if lookback > 0 && closes.len() > lookback + 1 {
        closes = closes.split_off(closes.len() - (lookback + 1));
    }
    if closes.len() < 31 {
        bail!("not enough price history for '{symbol}' (resolved '{resolved}') — need ≥31 closes");
    }
    let as_of = hist.last().map(|h| h.date.clone()).unwrap_or_default();

    // Daily log returns drive the drift/vol; CDaR-95 (path drawdown) drives the
    // recovery cliff — the same equity curve `drawdown_metrics` uses.
    let log_rets: Vec<f64> = closes.windows(2).filter(|w| w[0] > 0.0 && w[1] > 0.0).map(|w| (w[1] / w[0]).ln()).collect();
    let cdar95 = drawdown_metrics::compute(&closes, None, 0.0).map(|d| d.cdar_95);

    let s = survival::compute(&log_rets, cdar95, budget_pct, confidence)
        .ok_or_else(|| anyhow::anyhow!("not enough return data to model survival"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "survival",
                "asset": symbol,
                "resolved_symbol": resolved,
                "as_of": as_of,
                "survival": s,
            }))?
        );
        return Ok(());
    }

    let pct = |o: Option<f64>| o.map(|v| format!("{:.1}%", v * 100.0)).unwrap_or_else(|| "—".into());
    let days = |o: Option<f64>| {
        o.map(|d| {
            if d >= 365.0 {
                format!("{:.0}d (~{:.1}y)", d, d / 365.25)
            } else {
                format!("{d:.0}d")
            }
        })
        .unwrap_or_else(|| "—".into())
    };

    println!("═══ Drawdown Survival — {symbol} ({resolved}) ═══");
    println!("As of {as_of} · {} daily returns\n", log_rets.len());
    println!(
        "Drift/vol:   μ {:+.3}%/day · σ {:.2}%/day · lag-1 autocorr φ {:+.2}",
        s.mu_pct, s.sigma_pct, s.phi
    );
    println!("Regime:      {}", s.regime);
    if s.reliable {
        println!(
            "Max DD @{:.0}%: {} i.i.d. · {} AR(1) (serial-correlation corrected)",
            s.confidence * 100.0,
            pct(s.max_dd_iid),
            pct(s.max_dd_ar1),
        );
        println!(
            "Underwater:  to-trough {} i.i.d · {} AR(1) · total time-under-water {} i.i.d · {} AR(1)",
            days(s.time_to_dd_days),
            days(s.time_to_dd_ar1_days),
            days(s.max_tuw_iid_days),
            days(s.max_tuw_ar1_days),
        );
    } else {
        println!("Max DD/TuW:  (unavailable — non-positive drift; see regime)");
    }
    if let (Some(cdar), Some(rec)) = (s.cdar95, s.recovery_required_at_cdar95) {
        println!(
            "Recovery:    a {:.0}% CDaR-95 drawdown needs +{:.0}% to erase (D/(1−D) cliff)",
            cdar * 100.0,
            rec * 100.0,
        );
    }
    println!(
        "Risk of ruin: {:.1}% chance of ever breaching the {:.0}% drawdown budget",
        s.ruin_prob * 100.0,
        s.budget_pct,
    );
    println!("\n(depth is measured by EVT/CDaR; this is the TIME + solvency complement)");
    Ok(())
}
