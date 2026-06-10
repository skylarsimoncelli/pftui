//! `pftui analytics technicals cyber <SYM>` — composite Cyber Dots read.
//!
//! Faithful Rust port of the operator's PineScript indicator (canonical
//! source: `docs/reference/cyber-dots.pine`). See `analytics::cyber` for the
//! engine, the Pine→Rust block map, and the documented adaptations.

use anyhow::{bail, Result};
use serde_json::json;

use crate::analytics::cyber::{self, CyberSnapshot, CyberTimeframe};
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// History depth fetched for the cyber engine. Deeper than the structure
/// command's window because the Pi Cycle component carries the full
/// historical fire list (SMA471 + multi-cycle context).
const HISTORY_LIMIT: u32 = 8000;
/// Below this row count we try the `<SYM>-USD` fallback series (crypto is
/// often held as `BTC` while the deep series is `BTC-USD`).
const SHALLOW_THRESHOLD: usize = 400;

/// Load history preferring the deeper of `SYM` / `SYM-USD` (same fallback
/// rule as `technicals_structure::load_deep_history`, deeper window).
fn load_deep_history(
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
    lookback_signals: usize,
    json_output: bool,
) -> Result<()> {
    let tf = CyberTimeframe::parse(timeframe)?;
    let (series, history) = load_deep_history(backend, symbol)?;
    if history.is_empty() {
        bail!(
            "no price history for {} — run `pftui data refresh` or check the symbol",
            symbol.to_uppercase()
        );
    }
    let Some(snap) = cyber::analyze(&series, tf, &history, lookback_signals) else {
        bail!(
            "insufficient history for a {} cyber read on {} ({} daily rows)",
            tf.label(),
            series,
            history.len()
        );
    };

    if json_output {
        let mut value = serde_json::to_value(&snap)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("series_used".to_string(), json!(series));
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print_text(&snap, &series);
    }
    Ok(())
}

fn print_text(snap: &CyberSnapshot, series: &str) {
    println!(
        "Cyber Dots — {} ({} bars, series {})",
        snap.timeframe.label(),
        snap.bars,
        series
    );
    println!();
    println!("  {}", snap.verdict);
    println!();
    println!("  Last close: {} ({})", snap.last_close, snap.last_date);

    if let Some(b) = &snap.bands_gaussian {
        println!();
        println!("  CyberBands (Gaussian Channel):");
        println!(
            "    SMMA {}  ·  upper {}  ·  lower {}",
            b.smma, b.upper, b.lower
        );
        match &b.qb_since {
            Some(since) => println!(
                "    QB {} since {} ({} bars)",
                b.qb.label(),
                since,
                b.qb_bars
            ),
            None => println!("    QB {}", b.qb.label()),
        }
    }
    if let Some(z) = &snap.bands_zone {
        println!("  CyberBands (Zone Based, MA {}/{} ×{}):", z.adapted_ma1, z.adapted_ma2, z.tf_multiplier);
        println!(
            "    zone {}  ·  inner {} / {}  ·  outer {} / {}  ·  EMA bias {}",
            z.zone.label(),
            z.inner_lower,
            z.inner_upper,
            z.outer_lower,
            z.outer_upper,
            if z.ema_bias_bullish { "bullish" } else { "bearish" }
        );
    }
    if let Some(l) = &snap.line {
        println!();
        println!("  CyberLine (Volatility Weighted, len 18):");
        println!(
            "    value {}  ·  slope {}  ·  price {}",
            l.value,
            l.slope.label(),
            if l.price_above { "above" } else { "below" }
        );
        if let Some(c) = &l.last_cross {
            println!("    last cross: {} on {}", c.direction, c.date);
        }
        if let (Some(d), Some(h)) = (l.donchian_value, l.hybrid_value) {
            println!("    donchian {d}  ·  hybrid {h}");
        }
    }
    if let Some(d) = &snap.dots {
        println!();
        println!("  CyberDots (Medium sensitivity):");
        println!(
            "    up {}/3{}  ·  down {}/3{}  ·  supertrend {}{}",
            d.up_strength,
            if d.up_dot { " ●" } else { "" },
            d.down_strength,
            if d.down_dot { " ●" } else { "" },
            if d.supertrend_dir == 1 { "long" } else { "short" },
            d.supertrend_stop
                .map(|s| format!(" (stop {s})"))
                .unwrap_or_default()
        );
        println!(
            "    VMA {} ({:.2}% away)  ·  SMA18 {} ({}% away)",
            d.vma,
            d.vma_distance_pct,
            d.sma18.map(|v| v.to_string()).unwrap_or_else(|| "—".into()),
            d.sma_distance_pct
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".into())
        );
    }
    if let Some(m) = &snap.mtf_rsi {
        println!();
        println!("  MTF RSI (len 6, gates: {}):", m.gating.join("+"));
        println!(
            "    zone {}  ·  daily {}  ·  weekly {}  ·  monthly {}  ·  RSI14 extreme: {}",
            m.zone,
            fmt_opt(m.rsi6_daily),
            fmt_opt(m.rsi6_weekly),
            fmt_opt(m.rsi6_monthly),
            m.extreme
        );
    }
    if let Some(p) = &snap.pi_cycle {
        println!();
        println!("  Pi Cycle (daily closes, {} bars):", p.daily_bars);
        println!(
            "    top ratio {} (1.0 = trigger)  ·  bottom ratio {}",
            fmt_opt(p.top_ratio),
            fmt_opt(p.bottom_ratio)
        );
        println!(
            "    last top: {}  ·  last bottom: {}",
            p.last_top.as_deref().unwrap_or("never in window"),
            p.last_bottom.as_deref().unwrap_or("never in window")
        );
        if !p.top_fires.is_empty() {
            println!("    π top fires: {}", p.top_fires.join(", "));
        }
        if !p.bottom_fires.is_empty() {
            println!("    π bottom fires: {}", p.bottom_fires.join(", "));
        }
    }
    if let Some(r) = &snap.reversal {
        if !r.recent.is_empty() {
            println!();
            println!("  Reversal signals (BB 20/2.0):");
            for e in &r.recent {
                println!(
                    "    {} {} {} — {} (trigger {})",
                    e.date,
                    if e.kind == "top" { "T" } else { "B" },
                    e.kind,
                    e.status,
                    e.trigger_level
                );
            }
        }
    }
    if let Some(b) = &snap.breakout {
        println!();
        println!(
            "  Breakout: counters bull {} / bear {}{}",
            b.bull_counter,
            b.bear_counter,
            b.latest_arrow
                .as_deref()
                .map(|a| format!("  ·  {} arrow on latest bar", a))
                .unwrap_or_default()
        );
    }
    if !snap.signals.is_empty() {
        println!();
        println!("  Recent signals (newest first):");
        for s in &snap.signals {
            let glyph = match s.direction.as_deref() {
                Some("up") | Some("bull") | Some("bullish") | Some("above") => "▲",
                Some("down") | Some("bear") | Some("bearish") | Some("below") => "▼",
                _ => "·",
            };
            println!("    {} {} [{}] {}", s.date, glyph, s.component, s.detail);
        }
    }
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}")).unwrap_or_else(|| "—".into())
}
