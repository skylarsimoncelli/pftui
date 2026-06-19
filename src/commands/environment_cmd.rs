//! CLI for the Environment Engine — `analytics environment current` and
//! `analytics analog`. Loads daily closes from `price_history`, builds the
//! environment feature vector (`analytics::environment`), and runs the analog
//! engine (`analytics::analog`).

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::analytics::environment::{self, ENV_SYMBOLS};
use crate::analytics::regime_quad::Quad;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::analytics::{analog, cycle_clock, positioning};
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Load a symbol's full oldest-first `(date, close)` series from price_history.
fn load_series(conn: &Connection, symbol: &str) -> Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT date, close FROM price_history WHERE symbol = ?1 AND close IS NOT NULL ORDER BY date ASC",
    )?;
    let rows = stmt.query_map([symbol], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (d, raw) = r?;
        if let Ok(v) = raw.parse::<f64>() {
            out.push((d, v));
        }
    }
    Ok(out)
}

fn build_env(conn: &Connection) -> Result<environment::EnvironmentSeries> {
    let mut series = BTreeMap::new();
    for sym in ENV_SYMBOLS {
        series.insert(sym.to_string(), load_series(conn, sym)?);
    }
    environment::build(&series)
}

pub fn run_current(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let env = build_env(backend.sqlite())?;
    if env.is_empty() {
        bail!("no environment vectors computed (insufficient history)");
    }
    let (date, vec) = env
        .latest()
        .ok_or_else(|| anyhow::anyhow!("no environment vectors computed"))?;

    if json_output {
        let features: serde_json::Map<String, serde_json::Value> = env
            .feature_names
            .iter()
            .zip(vec.iter())
            .map(|(n, v)| (n.clone(), json!((v * 1000.0).round() / 1000.0)))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "environment current",
                "as_of": date,
                "history_days": env.len(),
                "features_zscored": features,
                "note": "z-scores are expanding-window (no look-ahead); +/- = standard deviations from the historical norm"
            }))?
        );
        return Ok(());
    }

    let quad = env.regime_quads.last().cloned().unwrap_or_default();
    println!("═══ Macro Environment — {} ═══", date);
    println!(
        "Regime quad (growth×inflation): {}",
        crate::analytics::regime_quad::Quad::from_short(&quad).label()
    );
    println!("(expanding-window z-scores: how far each reading sits from its historical norm)");
    println!("{} days of history\n", env.len());
    for (name, v) in env.feature_names.iter().zip(vec.iter()) {
        let bar = sd_bar(*v);
        println!("  {:<16} {:>6.2}σ  {}", name, v, bar);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_analog(
    backend: &BackendConnection,
    asset: &str,
    horizon_days: i64,
    k: usize,
    exclude_days: i64,
    json_output: bool,
) -> Result<()> {
    let conn = backend.sqlite();
    let env = build_env(conn)?;
    let resolved = resolve_alias(asset);
    let target = load_series(conn, &resolved)?;
    if target.is_empty() {
        bail!("no price history for '{asset}' (resolved '{resolved}')");
    }
    let report = analog::run(&env, &resolved, &target, horizon_days, k, exclude_days)
        .ok_or_else(|| anyhow::anyhow!("insufficient data to compute analogs"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "analog",
                "asset": asset,
                "report": report,
            }))?
        );
        return Ok(());
    }

    println!(
        "═══ Closest historic environments to {} ═══",
        report.query_date
    );
    println!(
        "Today's regime quad: {}",
        crate::analytics::regime_quad::Quad::from_short(&report.query_regime).label()
    );
    println!(
        "Target: {} | horizon: {}d | k={} nearest macro analogs | mean distance {:.2}",
        report.target_asset, report.horizon_days, report.k, report.mean_distance
    );
    println!();
    let fmt = |o: Option<f64>| o.map(|v| format!("{v:+.1}%")).unwrap_or_else(|| "—".into());
    println!(
        "{} forward returns over the {} nearest analogs:",
        report.target_asset, report.n_with_forward
    );
    println!(
        "  median {} | mean {} | p25 {} | p75 {} | up-rate {}",
        fmt(report.median_forward_pct),
        fmt(report.mean_forward_pct),
        fmt(report.p25_forward_pct),
        fmt(report.p75_forward_pct),
        report
            .up_rate_pct
            .map(|v| format!("{v:.0}%"))
            .unwrap_or_else(|| "—".into()),
    );
    if let Some((lo, hi)) = report.mean_forward_ci_pct {
        println!("  mean 90% CI [{lo:+.1}%, {hi:+.1}%]");
    }
    println!("  {}", report.note);
    println!();
    println!("Nearest analog dates (closest first):");
    println!("{:<12} {:>9} {:<11} {:>12}", "Date", "Distance", "Regime", "Fwd return");
    for a in report.analogs.iter().take(15) {
        println!(
            "{:<12} {:>9.2} {:<11} {:>12}",
            a.date,
            a.distance,
            a.regime,
            a.forward_return_pct
                .map(|v| format!("{v:+.1}%"))
                .unwrap_or_else(|| "—".into())
        );
    }
    Ok(())
}

/// Derive a coarse cycle lean (score, detail) from the cycle clock for the
/// assets that have one (BTC, gold). None for everything else.
fn cycle_lean(conn: &Connection, resolved: &str) -> Option<(f64, String)> {
    let up = resolved.to_uppercase();
    let history = price_history::get_history(conn, resolved, u32::MAX).ok()?;
    if history.len() < 200 {
        return None;
    }
    if up.contains("BTC") {
        let c = cycle_clock::btc_cycle_clock(resolved, &history)?;
        let mut score = 0.0f64;
        // Accumulation lean only when the cycle is genuinely near its low — being
        // far below the prior ATH is NOT itself bullish (it is equally the
        // Loukas major-top "lower high" condition). Gate on Loukas-band
        // proximity and an undervalued Mayer Multiple (price < 200d MA).
        let near_band = c
            .loukas
            .as_ref()
            .map(|l| l.in_band || (l.weeks_to_band_start > 0 && l.weeks_to_band_start <= 12))
            .unwrap_or(false);
        let cheap = c
            .mayer_multiple
            .map(|m| m < rust_decimal::Decimal::ONE)
            .unwrap_or(false);
        if near_band && cheap {
            score += 0.25; // measured accumulation zone (low band + below 200d MA)
        } else if near_band || cheap {
            score += 0.1;
        }
        Some((score.clamp(-1.0, 1.0), c.verdict))
    } else if up.contains("GC=F") || up.contains("GOLD") {
        let c = cycle_clock::gold_cycle_clock(resolved, &history)?;
        // Early in the cycle (before the half-cycle mark) = mild accumulation lean.
        let score = if c.past_half_cycle == Some(false) { 0.1 } else { 0.0 };
        Some((score, c.verdict))
    } else {
        None
    }
}

pub fn run_positioning(
    backend: &BackendConnection,
    asset: &str,
    horizon_days: i64,
    k: usize,
    json_output: bool,
) -> Result<()> {
    let conn = backend.sqlite();
    let env = build_env(conn)?;
    let resolved = resolve_alias(asset);
    let target = load_series(conn, &resolved)?;
    if target.is_empty() {
        bail!("no price history for '{asset}' (resolved '{resolved}')");
    }
    let analog_rep = analog::run(&env, &resolved, &target, horizon_days, k, 90)
        .ok_or_else(|| anyhow::anyhow!("insufficient data to compute analogs"))?;
    let quad = Quad::from_short(&analog_rep.query_regime);
    let cycle = cycle_lean(conn, &resolved);
    let card = positioning::synthesize(asset, &analog_rep.query_date, &analog_rep, quad, cycle);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "positioning",
                "card": card,
            }))?
        );
        return Ok(());
    }

    println!("═══ Positioning — {} ({}) ═══", asset, card.as_of);
    println!(
        "Stance: {}  |  confidence {:.0}%  |  blend {:+.2}  |  regime {}",
        card.stance.label(),
        card.confidence_pct,
        card.blend_score,
        card.regime.to_uppercase(),
    );
    println!();
    println!("Drivers:");
    for d in &card.drivers {
        println!(
            "  {:<24} {:+.2} (w{:.0}%)  {}",
            d.name,
            d.score,
            d.weight * 100.0,
            d.detail
        );
    }
    println!();
    if let Some(m) = card.analog_median_forward_pct {
        let ci = card
            .analog_ci_pct
            .map(|(lo, hi)| format!(" (90% CI [{lo:+.1}%, {hi:+.1}%])"))
            .unwrap_or_default();
        println!(
            "Measured anchor: {} analogs, median forward {:+.1}%{}",
            card.analog_n, m, ci
        );
    }
    println!("Honesty: {}", card.honesty_note);
    Ok(())
}

/// A tiny text bar showing how many standard deviations a z-score is.
fn sd_bar(z: f64) -> String {
    let n = (z.abs().min(4.0) * 2.0).round() as usize;
    let ch = if z >= 0.0 { '+' } else { '-' };
    std::iter::repeat_n(ch, n).collect()
}
