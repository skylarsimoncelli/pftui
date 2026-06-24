//! `pftui analytics cycles bottom-signals` — mechanical cycle-bottom signal
//! suite. A deterministic N-of-7 confluence of independent, Pine-ported
//! cycle-low confirmations, each evaluated at its natural timeframe. Position
//! / measurement only — never a price prediction.

use anyhow::{bail, Context, Result};

use crate::analytics::cycle_signal_backtest::{
    self, TriggerMode, TriggerSide, DEFAULT_CONFLUENCE_THRESHOLDS,
};
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

pub fn run_top(
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
    let Some(sig) = cycle_signals::cycle_top_signals(&series, &history, tf) else {
        return Err(anyhow::anyhow!(
            "insufficient history for a {} cycle-high read on {} ({} daily rows; need {})",
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
            "analytics cycles top-signals",
            &sig.as_of,
            Some(&series),
        );
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_top_text(&sig, &series);
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

pub fn run_top_backtest(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    window: Option<i64>,
    json_output: bool,
) -> Result<()> {
    if window == Some(0) {
        bail!(
            "--window 0 is not meaningful (a firing would have to land exactly on \
             the verified-high date); use a positive day count or omit --window for \
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
    let Some(bt) = cycle_signal_backtest::run_top_backtest(
        symbol,
        &series,
        &history,
        tf,
        window,
        &DEFAULT_CONFLUENCE_THRESHOLDS,
    ) else {
        return Err(anyhow::anyhow!(
            "insufficient history for a {} cycle-high backtest on {} ({} daily rows; need {})",
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
            "analytics cycles top-signals backtest",
            &bt.as_of,
            Some(&series),
        );
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_top_backtest(&bt);
    }
    Ok(())
}

fn parse_trigger_keys(raw: &[String]) -> Vec<String> {
    raw.iter()
        .flat_map(|s| s.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_trigger_mode(raw: &str) -> Result<TriggerMode> {
    match raw.trim().to_lowercase().as_str() {
        "all" => Ok(TriggerMode::All),
        "any" => Ok(TriggerMode::Any),
        other => bail!("unknown trigger mode '{other}' — expected all or any"),
    }
}

fn parse_horizons(raw: &str) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (num, mult) = match part.chars().last() {
            Some('d') | Some('D') => (&part[..part.len() - 1], 1),
            Some('w') | Some('W') => (&part[..part.len() - 1], 7),
            Some('m') | Some('M') => (&part[..part.len() - 1], 30),
            Some('y') | Some('Y') => (&part[..part.len() - 1], 365),
            Some(c) if c.is_ascii_digit() => (part, 1),
            _ => bail!("invalid horizon '{part}' — use e.g. 7d,30d,52w,1y"),
        };
        let n: i64 = num
            .parse()
            .with_context(|| format!("invalid horizon '{part}'"))?;
        if n <= 0 {
            bail!("invalid horizon '{part}' — horizons must be positive");
        }
        out.push(n * mult);
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        bail!("provide at least one positive horizon");
    }
    Ok(out)
}

pub fn run_trigger_backtest(options: TriggerBacktestCommandOptions<'_>) -> Result<()> {
    let TriggerBacktestCommandOptions {
        backend,
        symbol,
        side,
        timeframe,
        triggers,
        mode,
        horizons,
        window,
        json_output,
    } = options;
    if window == Some(0) {
        bail!(
            "--window 0 is not meaningful; use a positive day count or omit --window for \
             the default ±{}-day window",
            cycle_signal_backtest::DEFAULT_WINDOW_BARS
        );
    }
    let tf = SignalTimeframe::parse(timeframe)?;
    let trigger_keys = parse_trigger_keys(triggers);
    if trigger_keys.is_empty() {
        bail!("provide at least one --trigger key");
    }
    let mode = parse_trigger_mode(mode)?;
    let horizons = parse_horizons(horizons)?;
    let (series, history) = load_deep_history(backend, symbol)?;
    if history.is_empty() {
        return Err(anyhow::anyhow!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        ))
        .context(ErrorDetail::new("no_history"));
    }
    let Some(bt) = cycle_signal_backtest::run_trigger_backtest(
        cycle_signal_backtest::CycleTriggerBacktestRequest {
            symbol,
            series: &series,
            history: &history,
            side,
            timeframe: tf,
            trigger_keys: &trigger_keys,
            mode,
            horizons_days: &horizons,
            window_days: window,
        },
    ) else {
        return Err(anyhow::anyhow!(
            "insufficient history for a {} cycle trigger backtest on {} ({} daily rows; need {})",
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
    if !bt.unknown_trigger_keys.is_empty() {
        bail!(
            "unknown trigger key(s): {}. Use `bottom-signals --json` or `top-signals --json` \
             to inspect criteria[].key and criteria[].components[].key",
            bt.unknown_trigger_keys.join(", ")
        );
    }

    if json_output {
        let command = match side {
            TriggerSide::Bottom => "analytics cycles bottom-signals trigger-backtest",
            TriggerSide::Top => "analytics cycles top-signals trigger-backtest",
        };
        let payload = serde_json::to_value(&bt)?;
        let payload = cli_json::envelope(payload, command, &bt.as_of, Some(&series));
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_trigger_backtest(&bt);
    }
    Ok(())
}

pub struct TriggerBacktestCommandOptions<'a> {
    pub backend: &'a BackendConnection,
    pub symbol: &'a str,
    pub side: TriggerSide,
    pub timeframe: &'a str,
    pub triggers: &'a [String],
    pub mode: &'a str,
    pub horizons: &'a str,
    pub window: Option<i64>,
    pub json_output: bool,
}

fn print_trigger_backtest(bt: &cycle_signal_backtest::CycleTriggerBacktest) {
    let side = match bt.side {
        TriggerSide::Bottom => "Cycle-Low",
        TriggerSide::Top => "Cycle-High",
    };
    println!(
        "{side} Trigger Backtest — {} ({} timeframe, series {})",
        bt.symbol,
        bt.timeframe.label(),
        bt.series
    );
    println!(
        "  triggers: {} ({:?}) · horizons: {}",
        bt.trigger_keys.join(", "),
        bt.mode,
        bt.horizon_stats
            .iter()
            .map(|h| format!("{}d", h.horizon_days))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  {}", bt.summary);
    println!();
    println!("  Forward returns:");
    for h in &bt.horizon_stats {
        println!(
            "    {:>4}d  n={} good={} good_rate={} mean={} median={}",
            h.horizon_days,
            h.resolved,
            h.good,
            fmt_pct(h.good_rate),
            fmt_pct(h.mean_return_pct.map(|v| v / 100.0)),
            fmt_pct(h.median_return_pct.map(|v| v / 100.0)),
        );
    }
    println!();
    println!("  Events:");
    for e in &bt.events {
        println!(
            "    {} close {} · anchor {} · timing {} · price distance {}",
            e.fired_on,
            e.close,
            e.matched_anchor.as_deref().unwrap_or("none"),
            e.lead_lag_days
                .map(|d| format!("{d:+}d"))
                .unwrap_or_else(|| "n/a".to_string()),
            e.price_distance_from_anchor_pct
                .map(|p| format!("{p:+.1}%"))
                .unwrap_or_else(|| "n/a".to_string()),
        );
    }
    println!();
    println!("  {}", bt.caveat);
}

fn fmt_pct(value: Option<f64>) -> String {
    value
        .map(|v| format!("{:.0}%", v * 100.0))
        .unwrap_or_else(|| "n/a".to_string())
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
}

fn print_top_backtest(bt: &cycle_signal_backtest::CycleSignalBacktest) {
    println!(
        "Cycle-High Signal Reliability — {} ({} timeframe, series {})",
        bt.symbol,
        bt.timeframe.label(),
        bt.series
    );
    println!(
        "  {} daily bars · as of {} · ±{}-day match window",
        bt.bars, bt.as_of, bt.window_days
    );
    if bt.anchors.is_empty() {
        println!("  completed native cycle-high anchors: none");
    } else {
        println!(
            "  completed native cycle-high anchors: {}",
            bt.anchors.join(", ")
        );
    }
    if !bt.unverified_anchors.is_empty() {
        println!(
            "  (unverified documented low anchors used to derive highs: {})",
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
    println!("  {}", bt.caveat);
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

fn print_top_text(sig: &cycle_signals::CycleTopSignals, series: &str) {
    println!(
        "Cycle-High Signals — {} ({} timeframe, series {})",
        sig.symbol,
        sig.timeframe.label(),
        series
    );
    println!("  as of {}", sig.as_of);
    println!();
    println!("  Core exhaustion-watch progress:");
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
