//! `pftui analytics cycles bottom-signals` — mechanical cycle-bottom signal
//! suite. A deterministic N-of-N confluence of independent, Pine-ported
//! cycle-low confirmations, each evaluated at its natural timeframe. Position
//! / measurement only — never a price prediction.

use anyhow::{bail, Result};

use crate::analytics::cycle_signals::{self, SignalTimeframe};
use crate::commands::cli_json;
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
        bail!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        );
    }
    let Some(sig) = cycle_signals::cycle_bottom_signals(&series, &history, tf) else {
        bail!(
            "insufficient history for a {} cycle-bottom read on {} ({} daily rows)",
            tf.label(),
            series,
            history.len()
        );
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

fn print_text(sig: &cycle_signals::CycleBottomSignals, series: &str) {
    println!(
        "Cycle-Bottom Signals — {} ({} timeframe, series {})",
        sig.symbol,
        sig.timeframe.label(),
        series
    );
    println!("  as of {}", sig.as_of);
    println!();
    for c in &sig.criteria {
        let mark = if c.met { "✓" } else { "✗" };
        println!("  {mark} {:<46} {}", c.label, c.detail);
    }
    println!();
    println!("  {}/{} confluence", sig.met_count, sig.total);
    println!();
    println!("  {}", sig.verdict);
}
