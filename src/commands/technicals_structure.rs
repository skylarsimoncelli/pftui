//! `pftui analytics technicals structure <SYM>` — pure price-action
//! market-structure read (swings, trend classification, break-of-structure,
//! MA posture) computed straight from `price_history`. See
//! `analytics::market_structure` for the engine and parameter rationale.

use anyhow::{bail, Result};
use serde_json::json;

use crate::analytics::market_structure::{self, StructureRead, SwingKind, Timeframe};
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// History depth fetched for structure analysis (~10 years of daily bars —
/// enough for monthly aggregation with slow-MA slope context).
const HISTORY_LIMIT: u32 = 2600;
/// Below this row count we try the `<SYM>-USD` fallback series (crypto is
/// often held as `BTC` while the deep series is `BTC-USD`).
const SHALLOW_THRESHOLD: usize = 400;

/// Load history for a symbol, preferring the deeper of `SYM` / `SYM-USD`.
pub fn load_deep_history(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<(String, Vec<crate::models::price::HistoryRecord>)> {
    let sym = symbol.to_uppercase();
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
    let tf = Timeframe::parse(timeframe)?;
    let (series, history) = load_deep_history(backend, symbol)?;
    if history.is_empty() {
        bail!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        );
    }

    let Some(read) = market_structure::analyze(&series, tf, &history) else {
        bail!(
            "insufficient history for a {} structure read on {} ({} daily rows)",
            tf.label().to_lowercase(),
            series,
            history.len()
        );
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload(&read, &series))?);
    } else {
        print_text(&read, &series);
    }
    Ok(())
}

fn payload(read: &StructureRead, series: &str) -> serde_json::Value {
    let mut value = serde_json::to_value(read).unwrap_or_else(|_| json!({}));
    if let Some(obj) = value.as_object_mut() {
        obj.insert("series_used".to_string(), json!(series));
    }
    // Standard envelope (additive — keeps existing `symbol`/`series_used`/`verdict`).
    crate::commands::cli_json::envelope(
        value,
        "technicals structure",
        &read.last_bar_date,
        Some(series),
    )
}

fn print_text(read: &StructureRead, series: &str) {
    println!(
        "Market structure — {} ({} bars, series {})",
        read.timeframe.label(),
        read.bars_analyzed,
        series
    );
    println!();
    println!("  {}", read.verdict);
    println!();
    println!("  Structure: {}", read.structure.label());
    println!(
        "  Last close: {} ({})",
        read.last_close.round_dp(2),
        read.last_bar_date
    );
    if !read.swings.is_empty() {
        println!("  Swings (pivot window N={}):", read.pivot_window);
        for s in &read.swings {
            println!(
                "    {} {:>4} {} @ {}",
                s.date,
                s.label.as_deref().unwrap_or("—"),
                match s.kind {
                    SwingKind::High => "high",
                    SwingKind::Low => "low ",
                },
                s.price.round_dp(2)
            );
        }
    }
    if let Some(b) = &read.last_support_break {
        println!(
            "  Support break: {} (swing low of {}) broken on close {}",
            b.level.round_dp(2),
            b.swing_date,
            b.date
        );
    }
    if let Some(b) = &read.last_resistance_break {
        println!(
            "  Resistance break: {} (swing high of {}) broken on close {}",
            b.level.round_dp(2),
            b.swing_date,
            b.date
        );
    }
    let ma = &read.ma;
    let fmt_ma = |v: &Option<rust_decimal::Decimal>| {
        v.map(|d| d.round_dp(2).to_string()).unwrap_or_else(|| "n/a".into())
    };
    let fmt_slope = |s: &Option<market_structure::Slope>| {
        s.map(|x| match x {
            market_structure::Slope::Rising => "rising",
            market_structure::Slope::Falling => "falling",
            market_structure::Slope::Flat => "flat",
        })
        .unwrap_or("n/a")
    };
    println!(
        "  MA posture: fast {}={} ({}), slow {}={} ({})",
        ma.fast_period,
        fmt_ma(&ma.fast_ma),
        fmt_slope(&ma.fast_slope),
        ma.slow_period,
        fmt_ma(&ma.slow_ma),
        fmt_slope(&ma.slow_slope),
    );
    if let Some(ext) = ma.extension_pct_vs_slow {
        println!(
            "  Extension vs {}-bar MA: {:+}%{}",
            ma.slow_period,
            ext,
            if ma.rule13_extension_gate {
                "  ⚠ >20% — standing rule 13 extension gate"
            } else {
                ""
            }
        );
    }
}
