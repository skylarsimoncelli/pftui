//! `pftui analytics cycles bottom-signals` — mechanical cycle-bottom signal
//! suite. A deterministic N-of-7 confluence of independent, Pine-ported
//! cycle-low confirmations, each evaluated at its natural timeframe. Position
//! / measurement only — never a price prediction.

use anyhow::{bail, Context, Result};

use crate::analytics::cycle_signal_backtest::{self, DEFAULT_CONFLUENCE_THRESHOLDS};
use crate::analytics::cycle_signals::{self, SignalTimeframe};
use crate::commands::cli_json;
use crate::commands::cli_json::ErrorDetail;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Deep history depth (Pi-Cycle wants the full SMA471 + multi-cycle context).
const HISTORY_LIMIT: u32 = 8000;
/// Below this we try the `<SYM>-USD` fallback series (crypto often held as
/// `BTC` while the deep series is `BTC-USD`).
const SHALLOW_THRESHOLD: usize = 400;

/// Resolve common spoken aliases to their backend ticker (same convention as
/// the cycle clock / strategy aliases). Unknown inputs pass through uppercased.
fn resolve_alias(symbol: &str) -> String {
    match symbol.trim().to_lowercase().as_str() {
        "gold" => "GC=F".to_string(),
        "silver" => "SI=F".to_string(),
        other => other.to_uppercase(),
    }
}

/// Load history preferring the deeper of `SYM` / `SYM-USD`.
fn load_deep_history(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<(String, Vec<crate::models::price::HistoryRecord>)> {
    let sym = resolve_alias(symbol);
    let primary = price_history::get_history_backend(backend, &sym, HISTORY_LIMIT)?;
    if primary.len() >= SHALLOW_THRESHOLD || sym.contains('-') || sym.contains('=') {
        return Ok((sym, primary));
    }
    let alt_sym = format!("{sym}-USD");
    let alt = price_history::get_history_backend(backend, &alt_sym, HISTORY_LIMIT)?;
    if alt.len() > primary.len() {
        Ok((alt_sym, alt))
    } else {
        Ok((sym, primary))
    }
}

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    json_output: bool,
) -> Result<()> {
    let tf = SignalTimeframe::parse(timeframe)?;
    let (series, history) = load_deep_history(backend, symbol)?;
    if history.is_empty() {
        return Err(anyhow::anyhow!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        ))
        .context(ErrorDetail::new("no_history"));
    }
    let Some(sig) = cycle_signals::cycle_bottom_signals(&series, &history, tf) else {
        return Err(anyhow::anyhow!(
            "insufficient history for a {} cycle-bottom read on {} ({} daily rows; need {})",
            tf.label(),
            series,
            history.len(),
            cycle_signals::min_daily_bars()
        ))
        .context(ErrorDetail::with_bars(
            "insufficient_history",
            history.len(),
        ));
    };

    if json_output {
        let payload = serde_json::to_value(&sig)?;
        let payload = cli_json::envelope(
            payload,
            "analytics cycles bottom-signals",
            &sig.as_of,
            Some(&series),
        );
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_text(&sig, &series);
    }
    Ok(())
}

/// Reliability backtest: measure each criterion's lead/lag + hit-rate against
/// the verified cycle-low anchors over the full available history. Compute-only
/// — nothing is persisted.
pub fn run_backtest(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    window: Option<i64>,
    expectancy: bool,
    json_output: bool,
) -> Result<()> {
    // A zero match-window is meaningless (a firing would have to land EXACTLY
    // on the verified-low date), so reject it rather than silently clamping to
    // 1. clap already rejects negatives via the i64 parse path; this closes the
    // 0 hole. Omit --window entirely for the default window.
    if window == Some(0) {
        bail!(
            "--window 0 is not meaningful (a firing would have to land exactly on \
             the verified-low date); use a positive day count or omit --window for \
             the default ±{}-day window",
            cycle_signal_backtest::DEFAULT_WINDOW_BARS
        );
    }
    let tf = SignalTimeframe::parse(timeframe)?;
    let (series, history) = load_deep_history(backend, symbol)?;
    if history.is_empty() {
        return Err(anyhow::anyhow!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        ))
        .context(ErrorDetail::new("no_history"));
    }
    let Some(bt) = cycle_signal_backtest::run_backtest(
        symbol,
        &series,
        &history,
        tf,
        window,
        &DEFAULT_CONFLUENCE_THRESHOLDS,
        expectancy,
    ) else {
        return Err(anyhow::anyhow!(
            "insufficient history for a {} cycle-bottom backtest on {} ({} daily rows; need {})",
            tf.label(),
            series,
            history.len(),
            cycle_signals::min_daily_bars()
        ))
        .context(ErrorDetail::with_bars(
            "insufficient_history",
            history.len(),
        ));
    };

    if json_output {
        let payload = serde_json::to_value(&bt)?;
        let payload = cli_json::envelope(
            payload,
            "analytics cycles bottom-signals backtest",
            &bt.as_of,
            Some(&series),
        );
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_backtest(&bt);
    }
    Ok(())
}

fn print_backtest(bt: &cycle_signal_backtest::CycleSignalBacktest) {
    println!(
        "Cycle-Bottom Signal Reliability — {} ({} timeframe, series {})",
        bt.symbol,
        bt.timeframe.label(),
        bt.series
    );
    println!(
        "  {} daily bars · as of {} · ±{}-day match window",
        bt.bars, bt.as_of, bt.window_days
    );
    if bt.anchors.is_empty() {
        println!("  verified cycle-low anchors: none");
    } else {
        println!("  verified cycle-low anchors: {}", bt.anchors.join(", "));
    }
    if !bt.unverified_anchors.is_empty() {
        println!(
            "  (unverified documented dates: {})",
            bt.unverified_anchors.join(", ")
        );
    }
    println!();
    println!("  Per-criterion reliability:");
    for c in &bt.criteria {
        println!("    {:<46} {}", c.label, c.summary);
    }
    println!();
    println!("  Confluence (N/7):");
    for c in &bt.confluence {
        println!("    {:<46} {}", c.label, c.summary);
    }
    println!();
    println!("  {}", bt.headline);
    println!();
    if bt.small_n {
        println!("  ⚠ {}", bt.caveat);
    } else {
        println!("  {}", bt.caveat);
    }
    if let Some(exp) = &bt.expectancy {
        print_expectancy(exp);
    }
}

/// Render the asset-agnostic forward-return expectancy block.
fn print_expectancy(exp: &cycle_signal_backtest::CycleSignalExpectancy) {
    use cycle_signal_backtest::ExpectancyRow;
    println!();
    println!("  ── Forward-return expectancy (asset-agnostic) ──");
    if exp.price_structure_lows.is_empty() {
        println!("  price-structure swing lows: none derived");
    } else {
        println!(
            "  price-structure swing lows ({}d pivot, ≥{}% recovery): {}",
            exp.price_low_pivot_window,
            exp.price_low_prominence_pct.normalize(),
            exp.price_structure_lows.join(", ")
        );
    }
    println!(
        "  anchors used: {}{}",
        exp.anchors_used,
        if exp.doctrine_anchors_used {
            " (incl. doctrine)"
        } else {
            ""
        }
    );
    // Baseline line.
    let base = exp
        .baseline
        .iter()
        .map(|b| {
            format!(
                "{}d {}",
                b.horizon_days,
                b.mean_return_pct
                    .map(|m| format!("{:+.1}%", m))
                    .unwrap_or_else(|| "n/a".into())
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    println!("  baseline mean fwd return:  {base}");
    println!();
    let row_line = |r: &ExpectancyRow| {
        let horizons = r
            .horizons
            .iter()
            .map(|h| {
                let mean = h
                    .mean_return_pct
                    .map(|m| format!("{:+.1}%", m))
                    .unwrap_or_else(|| "n/a".into());
                let lift = h
                    .lift_vs_baseline_pct
                    .map(|l| format!("(lift {:+.1})", l))
                    .unwrap_or_default();
                format!("{}d {mean}{lift}", h.horizon_days)
            })
            .collect::<Vec<_>>()
            .join("  ");
        let close = match (
            r.closeness.median_price_gap_pct,
            r.closeness.median_lead_lag_days,
            r.closeness.confidence_pct,
        ) {
            (Some(gap), Some(days), Some(conf)) => format!(
                " · {} firings, {} matched (conf {:.0}%), median {:+.1}% / {:+}d to low",
                r.firings, r.closeness.matched_firings, conf, gap, days
            ),
            _ => format!(" · {} firings, no in-window low match", r.firings),
        };
        format!("    {:<42} {horizons}{close}", r.label)
    };
    println!("  Confluence expectancy:");
    for r in &exp.confluence {
        println!("{}", row_line(r));
    }
    println!();
    println!("  Per-criterion expectancy:");
    for r in &exp.criteria {
        println!("{}", row_line(r));
    }
    println!();
    if exp.small_n || exp.insufficient_anchors {
        println!("  ⚠ {}", exp.caveat);
    } else {
        println!("  {}", exp.caveat);
    }
}

fn print_text(sig: &cycle_signals::CycleBottomSignals, series: &str) {
    println!(
        "Cycle-Bottom Signals — {} ({} timeframe, series {})",
        sig.symbol,
        sig.timeframe.label(),
        series
    );
    println!("  as of {}", sig.as_of);
    println!();
    println!("  Core cycle-watch progress:");
    for item in &sig.core_watch {
        let mark = if item.met { "✓" } else { "·" };
        println!(
            "  {mark} {:<48} {}/{}  {}",
            item.label, item.met_components, item.total_components, item.detail
        );
        for component in &item.components {
            let sub_mark = if component.met { "✓" } else { "·" };
            println!(
                "      {sub_mark} {:<44} {}",
                component.label,
                component
                    .value
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "—".into())
            );
        }
    }
    println!();
    println!("  Full confluence suite:");
    for c in &sig.criteria {
        let mark = if c.met { "✓" } else { "✗" };
        println!("  {mark} {:<48} {}", c.label, c.detail);
    }
    if let Some(b) = &sig.bonus {
        let mark = if b.met { "✓" } else { "✗" };
        println!(
            "  {mark} {:<48} {}  (bonus — not counted)",
            b.label, b.detail
        );
    }
    println!();
    println!("  {}/{} confluence", sig.met_count, sig.total);
    println!();
    println!("  {}", sig.verdict);
}
