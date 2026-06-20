//! `pftui analytics cycles analyze` / `pftui analytics cycles ledger` —
//! deterministic multi-degree cycle-theory report (timing bands, translation
//! ledger, FLD/VTL, failed-cycle + inversion flags, nesting clarity).
//! Engine and parameter rationale live in `analytics::cycle_engine`;
//! doctrine in docs/CYCLE-THEORY.md. Position/timing only — never a price
//! prediction.

use anyhow::{bail, Result};
use serde_json::json;

use crate::analytics::cycle_engine::{self, CycleReport, DegreeStatus};
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Full-depth fetch — the metals series go back to 2000 (~6,700 rows).
const DEEP_LIMIT: u32 = 9000;
/// Below this row count the `<SYM>-USD` fallback series is tried (the held
/// `BTC` series is shallow; the deep series is `BTC-USD`).
const SHALLOW_THRESHOLD: usize = 400;

/// Load the deepest series for a symbol (SYM, falling back to SYM-USD per
/// the existing cycle-clock / structure precedent).
fn load_series(backend: &BackendConnection, symbol: &str) -> Result<(String, Vec<crate::models::price::HistoryRecord>)> {
    let sym = symbol.trim().to_uppercase();
    let primary = price_history::get_history_backend(backend, &sym, DEEP_LIMIT)?;
    if primary.len() >= SHALLOW_THRESHOLD || sym.contains('-') || sym.contains('=') {
        return Ok((sym, primary));
    }
    let alt_sym = format!("{sym}-USD");
    let alt = price_history::get_history_backend(backend, &alt_sym, DEEP_LIMIT)?;
    if alt.len() > primary.len() {
        Ok((alt_sym, alt))
    } else {
        Ok((sym, primary))
    }
}

fn build_report(backend: &BackendConnection, symbol: &str) -> Result<CycleReport> {
    let (series, history) = load_series(backend, symbol)?;
    if history.is_empty() {
        bail!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        );
    }
    let config = cycle_engine::default_config(symbol, &series);
    let Some(report) = cycle_engine::analyze(&config, &history) else {
        bail!(
            "insufficient history for a cycle read on {series} ({} daily rows)",
            history.len()
        );
    };
    Ok(report)
}

fn find_degree<'a>(report: &'a CycleReport, degree: &str) -> Result<&'a DegreeStatus> {
    let want = degree.trim().to_lowercase();
    report
        .degrees
        .iter()
        .find(|d| d.degree.eq_ignore_ascii_case(&want))
        .ok_or_else(|| {
            let available: Vec<&str> = report.degrees.iter().map(|d| d.degree.as_str()).collect();
            anyhow::anyhow!(
                "unknown degree '{degree}' for {} — available: {}",
                report.symbol,
                available.join(", ")
            )
        })
}

// ---------------------------------------------------------------------------
// analyze
// ---------------------------------------------------------------------------

pub fn run_analyze(
    backend: &BackendConnection,
    symbol: &str,
    degree: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let report = build_report(backend, symbol)?;

    if let Some(deg) = degree {
        let status = find_degree(&report, deg)?;
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "symbol": report.symbol,
                    "series": report.series,
                    "as_of": report.as_of,
                    "degree": status,
                    "note": "timing/position only — a window, never a date; never a price prediction",
                }))?
            );
        } else {
            println!("{}\n", report.composite_verdict);
            print_degree(status);
        }
        return Ok(());
    }

    if json_output {
        let mut value = serde_json::to_value(&report)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "note".to_string(),
                json!("timing/position only — a window, never a date; never a price prediction"),
            );
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("{}", report.composite_verdict);
    println!(
        "  series {} · {} bars · as of {} · close {}",
        report.series,
        report.bars,
        report.as_of,
        report.last_close.round_dp(2)
    );
    println!();
    for status in &report.degrees {
        print_degree(status);
        println!();
    }
    if let Some(btc) = &report.btc_clocks {
        println!("  BTC clocks (two framings, side by side — never merged):");
        println!("    [halving clock] {}", btc.halving_clock.verdict);
        println!(
            "    [halving clock] top window {}-{}d post-halving (ex-2013, n=3 — small-n); next halving {}",
            btc.top_window_post_halving_days[0],
            btc.top_window_post_halving_days[1],
            btc.next_halving_estimate
        );
        println!(
            "    [low-to-low]    last 4-year-degree low {} @ {} — age {} wk ({})",
            btc.low_to_low.last_low_date.as_deref().unwrap_or("?"),
            btc.low_to_low
                .last_low_price
                .map(|p| p.round_dp(0).to_string())
                .unwrap_or_else(|| "?".to_string()),
            btc.low_to_low
                .cycle_age_weeks
                .map(|w| w.to_string())
                .unwrap_or_else(|| "?".to_string()),
            btc.low_to_low
                .band_position
                .map(|p| p.label())
                .unwrap_or("no-band")
        );
        println!();
    }
    if let Some(gold) = &report.gold_clock {
        println!("  Gold long degree: {}", gold.folklore_label);
        println!("    [anchor clock] {}", gold.clock.verdict);
        println!();
    }
    if let Some(note) = &report.silver_note {
        println!("  Note: {note}\n");
    }
    println!("  Timing only — a window, never a date; the checklist confirms, the calendar does not.");
    Ok(())
}

fn print_degree(d: &DegreeStatus) {
    println!("  [{}] {}", d.degree, d.verdict);
    if let Some(b) = &d.band {
        println!(
            "    band: mean {} σ {} | P15 {} P85 {} bars | operative [{}, {}] ({}) | n {} (window {})",
            b.mean_bars,
            b.sd_bars,
            b.p15_bars,
            b.p85_bars,
            b.band_lo_bars,
            b.band_hi_bars,
            b.band_basis,
            b.n_cycles_total,
            b.n_cycles_window
        );
    }
    if let Some(top) = &d.current_top {
        println!(
            "    current top: {} @ {} (intraday high; bar {} of expected {} → provisional translation {})",
            top.date,
            top.price.round_dp(2),
            top.bars_from_low,
            d.expected_len_bars,
            top.provisional_translation_pct
                .map(|p| format!("{p:.2}"))
                .unwrap_or_else(|| "?".to_string())
        );
    }
    if let Some(f) = &d.fld {
        let cross = f
            .last_cross
            .as_ref()
            .map(|c| {
                let target = match (c.target, c.achieved_pct) {
                    // Once the post-cross extreme reaches/exceeds the 2× measured
                    // move, "% achieved" balloons (a target hit then run past
                    // can read 800%+), which looks like a bug. Cap the display at
                    // "target reached" and show the overshoot as a clean +N%.
                    (Some(t), Some(a)) if a >= 100.0 => {
                        format!(" → target {} (REACHED, +{:.0}% past)", t.round_dp(2), a - 100.0)
                    }
                    (Some(t), Some(a)) => {
                        format!(" → target {} ({a:.0}% achieved)", t.round_dp(2))
                    }
                    (Some(t), None) => format!(" → target {}", t.round_dp(2)),
                    _ => " (target degenerate — cross printed on the extreme)".to_string(),
                };
                format!(
                    " — last cross {} {} @ {}{}{}",
                    c.dir,
                    c.date,
                    c.cross_price.round_dp(2),
                    target,
                    if c.active { ", active" } else { ", inactive" }
                )
            })
            .unwrap_or_default();
        println!(
            "    FLD (offset {} bars, floor(len/2)): price {} @ {}{}",
            f.offset_bars,
            f.price_side,
            f.value.round_dp(2),
            cross
        );
    }
    if let Some(v) = &d.vtl {
        println!(
            "    VTL: {} {} / {} {} → value {} ({}); break confirms {}",
            v.anchors[0].date,
            v.anchors[0].price.round_dp(2),
            v.anchors[1].date,
            v.anchors[1].price.round_dp(2),
            v.value_at_last_bar.round_dp(2),
            if !v.valid {
                "invalid — cuts price between anchors"
            } else if v.broken {
                "BROKEN"
            } else {
                "holding"
            },
            v.break_confirms
        );
    }
    if !d.ledger.is_empty() {
        let summary: Vec<String> = d
            .ledger
            .iter()
            .map(|e| {
                format!(
                    "{}{}",
                    e.class.as_deref().unwrap_or("?"),
                    if e.failed { "(failed)" } else { "" }
                )
            })
            .collect();
        println!(
            "    ledger (oldest→newest): {}{}",
            summary.join(" "),
            if d.translation_warning {
                " — ⚠ first LT after RT string (canonical top warning)"
            } else {
                ""
            }
        );
    }
    if let Some(note) = &d.inversion_note {
        println!("    possible inversion: {note}");
    }
    if !d.clarity_issues.is_empty() {
        println!(
            "    clarity {}: {}",
            d.clarity.label(),
            d.clarity_issues.join("; ")
        );
    }
}

// ---------------------------------------------------------------------------
// ledger
// ---------------------------------------------------------------------------

pub fn run_ledger(
    backend: &BackendConnection,
    symbol: &str,
    degree: &str,
    json_output: bool,
) -> Result<()> {
    let report = build_report(backend, symbol)?;
    let status = find_degree(&report, degree)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "symbol": report.symbol,
                "series": report.series,
                "as_of": report.as_of,
                "degree": status.degree,
                "expected_len_bars": status.expected_len_bars,
                "ledger": status.ledger,
                "translation_warning": status.translation_warning,
                "rt_string_intact": status.rt_string_intact,
                "current_top": status.current_top,
                "note": "translation: RT = top past midpoint (bull signature), LT = top before midpoint (bear signature), MID = 0.5±0.05",
            }))?
        );
        return Ok(());
    }

    println!(
        "Translation ledger — {} {} degree (last {} completed cycles)\n",
        report.symbol,
        status.degree,
        status.ledger.len()
    );
    println!(
        "  {:<12} {:<12} {:>5}  {:<12} {:>6}  {:<5} failed",
        "start", "end", "bars", "top", "trans", "class"
    );
    for e in &status.ledger {
        println!(
            "  {:<12} {:<12} {:>5}  {:<12} {:>6}  {:<5} {}",
            e.start_date,
            e.end_date,
            e.len_bars,
            e.top_date.as_deref().unwrap_or("?"),
            e.translation_pct
                .map(|p| format!("{p:.2}"))
                .unwrap_or_else(|| "?".to_string()),
            e.class.as_deref().unwrap_or("?"),
            if e.failed { "FAILED" } else { "-" }
        );
    }
    println!();
    if status.translation_warning {
        println!("  ⚠ first LT after an RT string — canonical larger-degree top warning (§9)");
    } else if status.rt_string_intact {
        println!("  RT string intact — bull signature persists (§9)");
    }
    if let Some(top) = &status.current_top {
        println!(
            "  current cycle top so far: {} @ {} (provisional translation {})",
            top.date,
            top.price.round_dp(2),
            top.provisional_translation_pct
                .map(|p| format!("{p:.2}"))
                .unwrap_or_else(|| "?".to_string())
        );
    }
    Ok(())
}
