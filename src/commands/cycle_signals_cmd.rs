//! `pftui analytics cycles bottom-signals` — mechanical cycle-bottom signal
//! suite. A deterministic N-of-7 confluence of independent, Pine-ported
//! cycle-low confirmations, each evaluated at its natural timeframe. Position
//! / measurement only — never a price prediction.

use anyhow::{bail, Context, Result};

use crate::analytics::cycle_signal_backtest::{
    self, TriggerMode, TriggerSide, DEFAULT_CONFLUENCE_THRESHOLDS, DETREND_TRAILING_DAYS,
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
pub(crate) fn resolve_alias(symbol: &str) -> String {
    match symbol.trim().to_lowercase().as_str() {
        "gold" => "GC=F".to_string(),
        "silver" => "SI=F".to_string(),
        other => other.to_uppercase(),
    }
}

/// Load history preferring the deeper of `SYM` / `SYM-USD`.
pub(crate) fn load_deep_history(
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
#[allow(clippy::too_many_arguments)]
pub fn run_backtest(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    window: Option<i64>,
    expectancy: bool,
    detrend: bool,
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
    // Detrending only reshapes the expectancy block, so `--detrend` implies
    // `--expectancy` (it is meaningless without it).
    let expectancy = expectancy || detrend;
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
        detrend,
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

/// Cycle-TOP reliability (native cycle highs) + forward-return expectancy
/// backtest. Compute-only.
#[allow(clippy::too_many_arguments)]
pub fn run_top_backtest(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    window: Option<i64>,
    expectancy: bool,
    detrend: bool,
    json_output: bool,
) -> Result<()> {
    if window == Some(0) {
        bail!(
            "--window 0 is not meaningful (a firing would have to land exactly on \
             the verified/swing-high date); use a positive day count or omit --window for \
             the default ±{}-day window",
            cycle_signal_backtest::DEFAULT_WINDOW_BARS
        );
    }
    // `--detrend` implies `--expectancy` (it only reshapes that block).
    let expectancy = expectancy || detrend;
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
        expectancy,
        detrend,
    ) else {
        return Err(anyhow::anyhow!(
            "insufficient history for a {} cycle-top backtest on {} ({} daily rows; need {})",
            tf.label(),
            series,
            history.len(),
            cycle_signals::min_daily_bars()
        ))
        .context(ErrorDetail::with_bars("insufficient_history", history.len()));
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

// --- trigger-backtest event study (cycle-low / cycle-high) ---
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

fn print_top_text(sig: &cycle_signals::CycleTopSignals, series: &str) {
    println!(
        "Cycle-Top Signals — {} ({} timeframe, series {})",
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
        println!("  {mark} {:<48} {}  (bonus — not counted)", b.label, b.detail);
    }
    println!();
    println!("  {}/{} confluence", sig.met_count, sig.total);
    println!();
    println!("  {}", sig.verdict);
}

/// Cycle-TOP backtest renderer — mirrors [`print_backtest`] but headlines the
/// price-structure-only nature of tops (no doctrine anchors).
fn print_top_backtest(bt: &cycle_signal_backtest::CycleSignalBacktest) {
    println!(
        "Cycle-Top Signal Reliability — {} ({} timeframe, series {})",
        bt.symbol,
        bt.timeframe.label(),
        bt.series
    );
    println!(
        "  {} daily bars · as of {} · ±{}-day match window",
        bt.bars, bt.as_of, bt.window_days
    );
    // Anchor basis MUST agree with the body (build_top_headline / build_top_caveat):
    // top reliability is graded against NATIVE CYCLE HIGHS — the maximum close
    // between each verified low-to-low pair. Mirror the bottom renderer truthfully.
    if bt.anchors.is_empty() {
        println!("  native cycle-high anchors: none (no completed low-to-low cycle yet)");
    } else {
        println!(
            "  native cycle-high anchors (max close between verified lows): {}",
            bt.anchors.join(", ")
        );
    }
    if !bt.unverified_anchors.is_empty() {
        println!(
            "  (unverified documented dates: {})",
            bt.unverified_anchors.join(", ")
        );
    }
    println!();
    println!("  Per-criterion firings:");
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
    if let Some(exp) = &bt.expectancy {
        // The two blocks grade against DIFFERENT ground truths: reliability vs
        // native cycle highs, expectancy vs price-structure swing highs. Spell
        // out the two counts so the operator never conflates the closeness numbers.
        println!();
        println!(
            "  Note: reliability closeness is vs {} native cycle high(s); expectancy closeness \
             is vs {} price-structure swing high(s) — two different ground truths.",
            bt.anchors.len(),
            exp.price_structure_anchors.len()
        );
        print_top_expectancy(exp);
    }
}

/// Render the cycle-TOP forward-return expectancy block (price-structure highs).
/// Compact scale-context tag appended INSIDE the lift parens so a lift value can
/// never be read alone: `, ~0.4σ, base σ 180%`. Rendered only when the horizon
/// carries an `effect_size`; the baseline dispersion is looked up for the same
/// horizon. Effect size is a directional scale check, not a significance test.
fn sigma_context(
    exp: &cycle_signal_backtest::CycleSignalExpectancy,
    h: &cycle_signal_backtest::HorizonReturn,
) -> String {
    let Some(es) = h.effect_size else {
        return String::new();
    };
    let base_sigma = exp
        .baseline
        .iter()
        .find(|b| b.horizon_days == h.horizon_days)
        .and_then(|b| b.stdev_return_pct);
    match base_sigma {
        Some(s) => format!(", ~{:.1}σ, base σ {:.0}%", es, s),
        None => format!(", ~{:.1}σ", es),
    }
}

fn print_top_expectancy(exp: &cycle_signal_backtest::CycleSignalExpectancy) {
    use cycle_signal_backtest::ExpectancyRow;
    println!();
    println!("  ── Forward-return expectancy (price-structure swing highs) ──");
    if exp.detrended {
        println!(
            "  Mode: DRIFT-DETRENDED (excess over trailing {}d local drift) — returns are \
             excess-over-trend, not raw",
            exp.detrend_trailing_days.unwrap_or(DETREND_TRAILING_DAYS)
        );
    }
    if exp.price_structure_anchors.is_empty() {
        println!("  price-structure swing highs: none derived");
    } else {
        println!(
            "  price-structure swing highs ({}d pivot, ≥{}% decline): {}",
            exp.price_low_pivot_window,
            exp.price_low_prominence_pct.normalize(),
            exp.price_structure_anchors.join(", ")
        );
    }
    println!("  anchors used: {}", exp.anchors_used);
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
                let neg = h
                    .negative_rate_pct
                    .map(|n| format!("[{:.0}%↓]", n))
                    .unwrap_or_default();
                let lift = h
                    .lift_vs_baseline_pct
                    .map(|l| format!("(lift {:+.1}{})", l, sigma_context(exp, h)))
                    .unwrap_or_default();
                format!("{}d {mean}{neg}{lift}", h.horizon_days)
            })
            .collect::<Vec<_>>()
            .join("  ");
        let close = match (
            r.closeness.median_price_gap_pct,
            r.closeness.median_lead_lag_days,
            r.closeness.confidence_pct,
        ) {
            (Some(gap), Some(days), Some(conf)) => format!(
                " · {} firings, {} matched (conf {:.0}%), median {:+.1}% / {:+}d to high",
                r.firings, r.closeness.matched_firings, conf, gap, days
            ),
            _ => format!(" · {} firings, no in-window high match", r.firings),
        };
        // On a sub-significance sample, detrended figures (e.g. "365d -310.6%")
        // print to 1 decimal and read more authoritative than the count warrants.
        // Don't touch the underlying values — just flag the excess basis explicitly
        // so the operator reads the magnitude as directional excess, not a forecast.
        let warn = if r.low_firings {
            if exp.detrended {
                format!(
                    " (n={} — too few firings; excess, directional only)",
                    r.firings
                )
            } else {
                format!(" (n={} — too few firings; directional only)", r.firings)
            }
        } else {
            String::new()
        };
        format!("    {:<42} {horizons}{close}{warn}", r.label)
    };
    println!(
        "  closeness sign convention: \"+Nd to high\" = signal fired N days AFTER the swing high \
         (+ = confirmation/lag, NOT predictive lead; − = fired before the high)."
    );
    println!("  Confluence expectancy ([N%↓] = forward-return NEGATIVE rate = top hit-rate):");
    for r in &exp.confluence {
        println!("{}", row_line(r));
    }
    println!();
    println!("  Per-criterion expectancy:");
    for r in &exp.criteria {
        println!("{}", row_line(r));
    }
    println!();
    println!("  {}", exp.caveat);
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
    if exp.detrended {
        println!(
            "  Mode: DRIFT-DETRENDED (excess over trailing {}d local drift) — returns are \
             excess-over-trend, not raw",
            exp.detrend_trailing_days.unwrap_or(DETREND_TRAILING_DAYS)
        );
    }
    if exp.price_structure_anchors.is_empty() {
        println!("  price-structure swing lows: none derived");
    } else {
        println!(
            "  price-structure swing lows ({}d pivot, ≥{}% recovery): {}",
            exp.price_low_pivot_window,
            exp.price_low_prominence_pct.normalize(),
            exp.price_structure_anchors.join(", ")
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
                    .map(|l| format!("(lift {:+.1}{})", l, sigma_context(exp, h)))
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
        // On a sub-significance sample, detrended figures (e.g. "365d -310.6%")
        // print to 1 decimal and read more authoritative than the count warrants.
        // Don't touch the underlying values — just flag the excess basis explicitly
        // so the operator reads the magnitude as directional excess, not a forecast.
        let warn = if r.low_firings {
            if exp.detrended {
                format!(
                    " (n={} — too few firings; excess, directional only)",
                    r.firings
                )
            } else {
                format!(" (n={} — too few firings; directional only)", r.firings)
            }
        } else {
            String::new()
        };
        format!("    {:<42} {horizons}{close}{warn}", r.label)
    };
    println!(
        "  closeness sign convention: \"+Nd to low\" = signal fired N days AFTER the swing low \
         (+ = confirmation/lag, NOT predictive lead; − = fired before the low)."
    );
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
