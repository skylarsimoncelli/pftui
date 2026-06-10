//! Cyber Dots — faithful Rust port of the operator's PineScript v6 indicator
//! (© skyenettech, MPL-2.0). Canonical source committed verbatim at
//! `docs/reference/cyber-dots.pine`; that file is the spec for this module.
//!
//! # Pine block → Rust map
//!
//! | Pine block | Rust |
//! |---|---|
//! | `F_DEMA` | [`primitives::dema`] |
//! | `F_Gaussian` | `bands::gaussian_filter` |
//! | `F_SMMA` (quirk) | `bands::smma_quirk` |
//! | CyberBands Gaussian Channel + `var QB` state machine | [`bands::compute_gaussian_bands`] |
//! | CyberBands Zone Based (`band_th`/`band_lum`/`multiScaleEMA`, timeframe adaptation) | [`bands::compute_zone_bands`] |
//! | CyberLine Volatility Weighted (`cyberLine*` VIDYA) | `line::vidya_series` (len 18, Medium) |
//! | CyberLine Donchian / Hybrid | `line::donchian_trend_series` / [`line::compute_line`] |
//! | CyberDots SuperTrend + VMA(4) + SMA(18) strength dots | [`dots::compute_dots`] |
//! | Reversal signals (BB 20/2.0 + `topConf*`/`botConf*`) | [`reversal::compute_reversals`] |
//! | Pi Cycle (`pi*`) | [`pi_cycle::compute_pi_cycle`] |
//! | MTF RSI zones + RSI-extreme candles | [`mtf::compute_mtf`] |
//! | 3-line strike, `bindex`/`sindex` exhaustion, breakout arrows | [`breakout::compute_breakouts`] |
//!
//! # Documented adaptations (everything else is a literal port)
//!
//! 1. **MTF ladder on daily data** — Pine's 1D ladder is `[240min, 1W, 1M,
//!    3M]`; 240-minute bars don't exist on daily history, so the 240min slot
//!    degrades to the current (daily) RSI. With Pine's own skip rules
//!    (tf3/tf4 auto-pass at ≥240min charts) the effective daily gate is
//!    `daily AND weekly`; weekly runs gate `weekly AND daily AND monthly`.
//!    See `mtf.rs`.
//! 2. **SuperTrend direction seed** — the Pine's `dir := nz(dir[1])` (no
//!    default) evaluates to 0 on bar 0 and the transitions only fire from
//!    the ±1 states, so a literal port would pin `dir ≡ 0` forever and kill
//!    the third dot-strength condition. We seed `dir = 1` per the canonical
//!    SuperTrend (`nz(dir[1], dir)`), which the rest of the script assumes
//!    (`dir ∈ {1, −1}`). See `dots.rs`.
//! 3. **VIDYA div-by-zero guards** — Pine emits na and reseeds from 0 via
//!    `nz` when a denominator is 0; we carry the previous value instead.
//!    Identical on real data (denominators are never exactly 0 after
//!    warm-up); only pathological flat synthetic inputs differ. See
//!    `line.rs`.
//! 4. **`F_SMMA` dead branch** — the Pine's `na(SMMA[1]) ? dema : …` reseed
//!    branch is dead code (earlier bars always seeded SMMA with `src`); we
//!    port the live semantics (seed with src, then smooth). See `bands.rs`.
//! 5. **Missing OHLC fields** — `price_history` rows may lack open/high/low
//!    (ratio series). Fallbacks: `open ← previous close`, `high ←
//!    max(open, close)`, `low ← min(open, close)` — candles degrade to
//!    close-to-close bodies; close-only math (bands, line, pi, MTF RSI) is
//!    unaffected.
//! 6. **Dot events** — Pine plots a dot shape on every active bar; the dated
//!    signal list emits only dot-run ONSETS so trends don't flood it. The
//!    per-bar state is still exposed (`up_dot`/`down_dot`, strengths).
//! 7. **Warm-up** — Pine's leading-na windows are reproduced with
//!    `Option<f64>` prefixes / zero seeds exactly where the script relies on
//!    `nz`; with the ≥400-bar histories the engine requires for a verdict,
//!    every recursion has fully converged regardless of seed.
//!
//! Indicator internals are `f64` (precedent: `analytics::technicals`);
//! rendered price levels are rounded via `primitives::round_level`. No money
//! flows through this module.

pub mod bands;
pub mod breakout;
pub mod dots;
pub mod line;
pub mod mtf;
pub mod pi_cycle;
pub mod primitives;
pub mod reversal;

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use crate::models::price::HistoryRecord;

/// Run timeframe. Weekly bars are aggregated from daily history by ISO week
/// (same bucketing as `analytics::market_structure`); Pi Cycle always runs
/// on daily closes (the Pine requests "1D" explicitly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CyberTimeframe {
    Daily,
    Weekly,
}

impl CyberTimeframe {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "daily" | "1d" | "d" => Ok(CyberTimeframe::Daily),
            "weekly" | "1w" | "w" => Ok(CyberTimeframe::Weekly),
            other => anyhow::bail!("unknown timeframe '{other}' — expected daily or weekly"),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            CyberTimeframe::Daily => "daily",
            CyberTimeframe::Weekly => "weekly",
        }
    }
}

/// One dated signal event in the merged cross-component list.
#[derive(Debug, Clone, Serialize)]
pub struct SignalEvent {
    pub date: String,
    /// Component: "bands" | "line" | "dots" | "reversal" | "pi-cycle" |
    /// "mtf-rsi" | "breakout".
    pub component: String,
    pub kind: String,
    pub direction: Option<String>,
    pub detail: String,
}

/// Composite Cyber Dots snapshot for one symbol/timeframe.
#[derive(Debug, Clone, Serialize)]
pub struct CyberSnapshot {
    pub symbol: String,
    pub timeframe: CyberTimeframe,
    pub bars: usize,
    pub last_date: String,
    pub last_close: f64,
    /// One-line composite verdict.
    pub verdict: String,
    pub bands_gaussian: Option<bands::GaussianBandsRead>,
    pub bands_zone: Option<bands::ZoneBandsRead>,
    pub line: Option<line::LineRead>,
    pub dots: Option<dots::DotsRead>,
    pub reversal: Option<reversal::ReversalRead>,
    pub pi_cycle: Option<pi_cycle::PiCycleRead>,
    pub mtf_rsi: Option<mtf::MtfRsiRead>,
    pub breakout: Option<breakout::BreakoutRead>,
    /// Merged most-recent dated signal events, newest first.
    pub signals: Vec<SignalEvent>,
}

/// OHLC working arrays built from daily history (adaptation 5 fallbacks).
struct Series {
    dates: Vec<String>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
}

fn to_f64(d: rust_decimal::Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

fn build_daily_series(history: &[HistoryRecord]) -> Series {
    let n = history.len();
    let mut s = Series {
        dates: Vec::with_capacity(n),
        open: Vec::with_capacity(n),
        high: Vec::with_capacity(n),
        low: Vec::with_capacity(n),
        close: Vec::with_capacity(n),
    };
    let mut prev_close: Option<f64> = None;
    for r in history {
        let close = to_f64(r.close);
        let open = r
            .open
            .map(to_f64)
            .or(prev_close)
            .unwrap_or(close);
        let high = r.high.map(to_f64).unwrap_or_else(|| open.max(close));
        let low = r.low.map(to_f64).unwrap_or_else(|| open.min(close));
        s.dates.push(r.date.clone());
        s.open.push(open);
        s.high.push(high.max(open.max(close)));
        s.low.push(low.min(open.min(close)));
        s.close.push(close);
        prev_close = Some(close);
    }
    s
}

/// Aggregate the daily series into ISO-week bars (open = first open,
/// high = max, low = min, close/date = last).
fn aggregate_weekly(daily: &Series) -> Series {
    let mut s = Series {
        dates: Vec::new(),
        open: Vec::new(),
        high: Vec::new(),
        low: Vec::new(),
        close: Vec::new(),
    };
    let mut current: Option<(i32, u32)> = None;
    for i in 0..daily.dates.len() {
        let Ok(date) = NaiveDate::parse_from_str(&daily.dates[i], "%Y-%m-%d") else {
            continue;
        };
        let iso = date.iso_week();
        let key = (iso.year(), iso.week());
        if current == Some(key) {
            let last = s.dates.len() - 1;
            s.dates[last] = daily.dates[i].clone();
            s.high[last] = s.high[last].max(daily.high[i]);
            s.low[last] = s.low[last].min(daily.low[i]);
            s.close[last] = daily.close[i];
        } else {
            current = Some(key);
            s.dates.push(daily.dates[i].clone());
            s.open.push(daily.open[i]);
            s.high.push(daily.high[i]);
            s.low.push(daily.low[i]);
            s.close.push(daily.close[i]);
        }
    }
    s
}

/// Minimum current-timeframe bars for a meaningful composite read (bands SD
/// window + SMMA + Gaussian chain, with full recursion convergence).
const MIN_BARS: usize = 60;

/// Run the full Cyber Dots engine. `history` is DAILY price history,
/// oldest-first (as `price_history` getters return it). Returns `None` when
/// fewer than [`MIN_BARS`] bars exist on the run timeframe.
pub fn analyze(
    symbol: &str,
    timeframe: CyberTimeframe,
    history: &[HistoryRecord],
    lookback_signals: usize,
) -> Option<CyberSnapshot> {
    let daily = build_daily_series(history);
    if daily.dates.is_empty() {
        return None;
    }
    let bars_owned;
    let bars: &Series = match timeframe {
        CyberTimeframe::Daily => &daily,
        CyberTimeframe::Weekly => {
            bars_owned = aggregate_weekly(&daily);
            &bars_owned
        }
    };
    let n = bars.dates.len();
    if n < MIN_BARS {
        return None;
    }
    let cap = lookback_signals.max(1);

    let bands_gaussian = bands::compute_gaussian_bands(&bars.close, &bars.dates, cap);
    let bands_zone = bands::compute_zone_bands(&bars.close, timeframe);
    let line = line::compute_line(&bars.close, &bars.high, &bars.low, &bars.dates);
    let dots = dots::compute_dots(&bars.close, &bars.high, &bars.low, &bars.dates, cap);
    let reversal = reversal::compute_reversals(&bars.close, &bars.high, &bars.low, &bars.dates, cap);
    let pi = pi_cycle::compute_pi_cycle(&daily.close, &daily.dates);
    let mtf = mtf::compute_mtf(&daily.dates, &daily.close, &bars.dates, timeframe, cap);

    let empty_qb: Vec<i8> = Vec::new();
    let qb_series = bands_gaussian
        .as_ref()
        .map(|b| &b.qb_series)
        .unwrap_or(&empty_qb);
    let empty_bool: Vec<bool> = Vec::new();
    let (rsi_up, rsi_dn) = mtf
        .as_ref()
        .map(|m| (&m.up_signal_series, &m.dn_signal_series))
        .unwrap_or((&empty_bool, &empty_bool));
    let breakout = breakout::compute_breakouts(
        &bars.open,
        &bars.high,
        &bars.low,
        &bars.close,
        &bars.dates,
        qb_series,
        rsi_up,
        rsi_dn,
        cap,
    );

    let signals = merge_signals(
        bands_gaussian.as_ref(),
        line.as_ref(),
        dots.as_ref(),
        reversal.as_ref(),
        pi.as_ref(),
        mtf.as_ref(),
        breakout.as_ref(),
        lookback_signals,
    );

    let last = n - 1;
    let snapshot_core = (
        bars.dates[last].clone(),
        primitives::round_level(bars.close[last]),
    );
    let verdict = build_verdict(
        timeframe,
        bands_gaussian.as_ref(),
        line.as_ref(),
        dots.as_ref(),
        mtf.as_ref(),
        pi.as_ref(),
        signals.first(),
    );

    Some(CyberSnapshot {
        symbol: symbol.to_string(),
        timeframe,
        bars: n,
        last_date: snapshot_core.0,
        last_close: snapshot_core.1,
        verdict,
        bands_gaussian,
        bands_zone,
        line,
        dots,
        reversal,
        pi_cycle: pi,
        mtf_rsi: mtf,
        breakout,
        signals,
    })
}

#[allow(clippy::too_many_arguments)]
fn merge_signals(
    bands: Option<&bands::GaussianBandsRead>,
    line: Option<&line::LineRead>,
    dots: Option<&dots::DotsRead>,
    reversal: Option<&reversal::ReversalRead>,
    pi: Option<&pi_cycle::PiCycleRead>,
    mtf: Option<&mtf::MtfRsiRead>,
    breakout: Option<&breakout::BreakoutRead>,
    limit: usize,
) -> Vec<SignalEvent> {
    let mut all: Vec<SignalEvent> = Vec::new();
    if let Some(b) = bands {
        for t in &b.transitions {
            all.push(SignalEvent {
                date: t.date.clone(),
                component: "bands".into(),
                kind: "qb-flip".into(),
                direction: Some(t.to.label().into()),
                detail: format!("QB {} → {}", t.from.label(), t.to.label()),
            });
        }
    }
    if let Some(l) = line {
        if let Some(c) = &l.last_cross {
            all.push(SignalEvent {
                date: c.date.clone(),
                component: "line".into(),
                kind: "price-cross".into(),
                direction: Some(c.direction.clone()),
                detail: format!("price crossed {} CyberLine", c.direction),
            });
        }
    }
    if let Some(d) = dots {
        for e in &d.recent_dots {
            all.push(SignalEvent {
                date: e.date.clone(),
                component: "dots".into(),
                kind: "dot-onset".into(),
                direction: Some(e.direction.clone()),
                detail: format!("{} dot run begins (strength {})", e.direction, e.strength),
            });
        }
    }
    if let Some(r) = reversal {
        for e in &r.recent {
            all.push(SignalEvent {
                date: e.date.clone(),
                component: "reversal".into(),
                kind: format!("{}-reversal", e.kind),
                direction: Some(if e.kind == "top" { "bearish".into() } else { "bullish".into() }),
                detail: format!(
                    "{} reversal ({}) — trigger {}",
                    e.kind, e.status, e.trigger_level
                ),
            });
        }
    }
    if let Some(p) = pi {
        for d in &p.top_fires {
            all.push(SignalEvent {
                date: d.clone(),
                component: "pi-cycle".into(),
                kind: "pi-top".into(),
                direction: Some("bearish".into()),
                detail: "Pi Cycle TOP (SMA111 rose through 2×SMA350)".into(),
            });
        }
        for d in &p.bottom_fires {
            all.push(SignalEvent {
                date: d.clone(),
                component: "pi-cycle".into(),
                kind: "pi-bottom".into(),
                direction: Some("bullish".into()),
                detail: "Pi Cycle BOTTOM (0.745×SMA471 crossed above EMA150)".into(),
            });
        }
    }
    if let Some(m) = mtf {
        for e in &m.recent_signals {
            all.push(SignalEvent {
                date: e.date.clone(),
                component: "mtf-rsi".into(),
                kind: "zone-exit".into(),
                direction: Some(e.direction.clone()),
                detail: format!("RSI {} zone exited", if e.direction == "up" { "green" } else { "red" }),
            });
        }
    }
    if let Some(b) = breakout {
        for e in &b.recent {
            all.push(SignalEvent {
                date: e.date.clone(),
                component: "breakout".into(),
                kind: "arrow".into(),
                direction: Some(e.direction.clone()),
                detail: format!("{} breakout arrow ({})", e.direction, e.signals.join("+")),
            });
        }
    }
    // Newest first; stable component order within a date.
    all.sort_by(|a, b| b.date.cmp(&a.date));
    all.truncate(limit);
    all
}

fn build_verdict(
    timeframe: CyberTimeframe,
    bands: Option<&bands::GaussianBandsRead>,
    line: Option<&line::LineRead>,
    dots: Option<&dots::DotsRead>,
    mtf: Option<&mtf::MtfRsiRead>,
    pi: Option<&pi_cycle::PiCycleRead>,
    latest_signal: Option<&SignalEvent>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = bands {
        match &b.qb_since {
            Some(since) => parts.push(format!(
                "QB {} since {} ({} bars)",
                b.qb.label(),
                since,
                b.qb_bars
            )),
            None => parts.push(format!("QB {}", b.qb.label())),
        }
    }
    if let Some(l) = line {
        parts.push(format!(
            "line {}, price {}",
            l.slope.label(),
            if l.price_above { "above" } else { "below" }
        ));
    }
    if let Some(d) = dots {
        let dot = if d.up_dot {
            format!("up {}/3 ●", d.up_strength)
        } else if d.down_dot {
            format!("down {}/3 ●", d.down_strength)
        } else {
            format!("none (up {}, down {})", d.up_strength, d.down_strength)
        };
        parts.push(format!("dots {dot}"));
    }
    if let Some(m) = mtf {
        let mut z = format!("MTF-RSI {}", m.zone);
        if m.extreme != "none" {
            z.push_str(&format!(" (extreme-{})", m.extreme));
        }
        parts.push(z);
    }
    if let Some(p) = pi {
        let top = p
            .top_ratio
            .map(|r| format!("{r:.2}"))
            .unwrap_or_else(|| "—".into());
        let bottom = p
            .bottom_ratio
            .map(|r| format!("{r:.2}"))
            .unwrap_or_else(|| "—".into());
        parts.push(format!("Pi top {top} / bottom {bottom} of trigger"));
    }
    if let Some(s) = latest_signal {
        parts.push(format!("last signal: {} {} {}", s.date, s.component, s.kind));
    }
    format!("CYBER ({}): {}", timeframe.label(), parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn record(date: &str, close: f64) -> HistoryRecord {
        HistoryRecord {
            date: date.to_string(),
            close: Decimal::from_str(&format!("{close:.4}")).unwrap_or_default(),
            volume: None,
            open: Decimal::from_str(&format!("{:.4}", close - 0.5)).ok(),
            high: Decimal::from_str(&format!("{:.4}", close + 1.0)).ok(),
            low: Decimal::from_str(&format!("{:.4}", close - 1.5)).ok(),
        }
    }

    fn synthetic_history(n: usize) -> Vec<HistoryRecord> {
        let start = NaiveDate::from_ymd_opt(2023, 1, 2).unwrap_or_default();
        (0..n)
            .map(|i| {
                let date = (start + chrono::Days::new(i as u64))
                    .format("%Y-%m-%d")
                    .to_string();
                // Trending tape with a wave so every component has signal.
                let close = 100.0 + i as f64 * 0.3 + 8.0 * (i as f64 / 15.0).sin();
                record(&date, close)
            })
            .collect()
    }

    #[test]
    fn analyze_produces_all_sections_on_deep_history() {
        let history = synthetic_history(700);
        let snap = analyze("TEST", CyberTimeframe::Daily, &history, 10).expect("snapshot");
        assert_eq!(snap.timeframe, CyberTimeframe::Daily);
        assert!(snap.bands_gaussian.is_some());
        assert!(snap.bands_zone.is_some());
        assert!(snap.line.is_some());
        assert!(snap.dots.is_some());
        assert!(snap.reversal.is_some());
        assert!(snap.pi_cycle.is_some(), "700 daily bars fit the top pair");
        assert!(snap.mtf_rsi.is_some());
        assert!(snap.breakout.is_some());
        assert!(snap.verdict.starts_with("CYBER (daily):"));
        assert!(snap.verdict.contains("QB"));
        assert!(snap.signals.len() <= 10);
    }

    #[test]
    fn weekly_aggregation_runs_components_on_weekly_bars() {
        let history = synthetic_history(700);
        let snap = analyze("TEST", CyberTimeframe::Weekly, &history, 10).expect("snapshot");
        assert_eq!(snap.timeframe, CyberTimeframe::Weekly);
        assert!(snap.bars >= 60 && snap.bars <= 110, "≈100 ISO weeks, got {}", snap.bars);
        // Pi Cycle still runs on the DAILY series regardless of timeframe.
        assert!(snap.pi_cycle.is_some());
        assert_eq!(
            snap.pi_cycle.as_ref().map(|p| p.daily_bars),
            Some(700)
        );
        // Weekly MTF gates on three timeframes.
        assert_eq!(
            snap.mtf_rsi.as_ref().map(|m| m.gating.len()),
            Some(3)
        );
    }

    #[test]
    fn determinism_same_input_identical_output() {
        let history = synthetic_history(600);
        let a = analyze("TEST", CyberTimeframe::Daily, &history, 10).expect("a");
        let b = analyze("TEST", CyberTimeframe::Daily, &history, 10).expect("b");
        let ja = serde_json::to_string(&a).unwrap_or_default();
        let jb = serde_json::to_string(&b).unwrap_or_default();
        assert!(!ja.is_empty());
        assert_eq!(ja, jb, "engine must be fully deterministic");
    }

    #[test]
    fn shallow_history_returns_none() {
        let history = synthetic_history(40);
        assert!(analyze("TEST", CyberTimeframe::Daily, &history, 10).is_none());
    }

    #[test]
    fn signals_are_newest_first_and_capped() {
        let history = synthetic_history(700);
        let snap = analyze("TEST", CyberTimeframe::Daily, &history, 5).expect("snapshot");
        assert!(snap.signals.len() <= 5);
        for w in snap.signals.windows(2) {
            assert!(w[0].date >= w[1].date, "signals must be newest first");
        }
    }
}
