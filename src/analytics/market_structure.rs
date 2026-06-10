//! Pure price-action market-structure engine (T1, gold post-mortem fix).
//!
//! Computes, from `price_history` daily closes alone (no snapshot cache, no
//! narrative inputs):
//!
//! - **Swing detection** — N-bar pivot highs/lows on closes. N=3 for daily
//!   (daily bars are noisy; a 3-bar confirmation window filters one-day
//!   wicks), N=2 for weekly/monthly (each bar already aggregates 5/21 daily
//!   bars, so less confirmation is needed and a wider window would lag a
//!   swing by months). A pivot is only *confirmed* once N bars have printed
//!   to its right.
//! - **Structure classification** — UPTREND (higher highs + higher lows),
//!   DOWNTREND (lower highs + lower lows), RANGE (mixed), from the last 4-6
//!   alternating swings.
//! - **Break-of-structure** — most recent support break (last confirmed
//!   swing low taken out on a close) and resistance break.
//! - **MA posture** — price vs fast/slow MAs (50d/200d daily, 10wk/40wk
//!   weekly, 10mo/20mo monthly), MA slope over the last 20 bars, and
//!   extension % vs the slow MA. Extension >20% above the slow MA on the
//!   daily timeframe trips standing rule 13's extension gate.
//!
//! Weekly and monthly bars are aggregated from daily history (ISO week /
//! calendar month; bar close = last daily close in the bucket).
//!
//! All prices are `rust_decimal::Decimal` — these are money values, not
//! indicator floats. The engine never predicts; it reads what printed.

use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::models::price::HistoryRecord;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Timeframe {
    Daily,
    Weekly,
    Monthly,
}

impl Timeframe {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "daily" | "1d" | "d" => Ok(Timeframe::Daily),
            "weekly" | "1w" | "w" => Ok(Timeframe::Weekly),
            "monthly" | "1mo" | "m" => Ok(Timeframe::Monthly),
            other => anyhow::bail!(
                "unknown timeframe '{other}' — expected daily, weekly, or monthly"
            ),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Timeframe::Daily => "DAILY",
            Timeframe::Weekly => "WEEKLY",
            Timeframe::Monthly => "MONTHLY",
        }
    }

    /// Pivot confirmation window (bars each side). See module docs.
    pub fn pivot_window(&self) -> usize {
        match self {
            Timeframe::Daily => 3,
            Timeframe::Weekly | Timeframe::Monthly => 2,
        }
    }

    /// (fast, slow) MA periods per timeframe.
    pub fn ma_periods(&self) -> (usize, usize) {
        match self {
            Timeframe::Daily => (50, 200),
            Timeframe::Weekly => (10, 40),
            Timeframe::Monthly => (10, 20),
        }
    }

    fn ma_unit(&self) -> &'static str {
        match self {
            Timeframe::Daily => "d",
            Timeframe::Weekly => "wk",
            Timeframe::Monthly => "mo",
        }
    }
}

/// One aggregated bar (close-based; weekly/monthly bars carry the last
/// daily close of the bucket and the bucket's last date).
#[derive(Debug, Clone, Serialize)]
pub struct Bar {
    pub date: String,
    pub close: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwingKind {
    High,
    Low,
}

#[derive(Debug, Clone, Serialize)]
pub struct Swing {
    pub date: String,
    pub price: Decimal,
    pub kind: SwingKind,
    /// HH/LH for highs, HL/LL for lows, relative to the previous swing of
    /// the same kind. None for the first swing of its kind in the window.
    pub label: Option<String>,
    #[serde(skip)]
    pub bar_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureClass {
    Uptrend,
    Downtrend,
    Range,
    Insufficient,
}

impl StructureClass {
    pub fn label(&self) -> &'static str {
        match self {
            StructureClass::Uptrend => "uptrend",
            StructureClass::Downtrend => "downtrend",
            StructureClass::Range => "range",
            StructureClass::Insufficient => "insufficient-swings",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakEvent {
    /// Date of the close that broke the level.
    pub date: String,
    /// The swing level that was broken.
    pub level: Decimal,
    /// Date of the swing that set the broken level.
    pub swing_date: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Slope {
    Rising,
    Falling,
    Flat,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaPosture {
    pub fast_period: usize,
    pub slow_period: usize,
    pub fast_ma: Option<Decimal>,
    pub slow_ma: Option<Decimal>,
    pub above_fast: Option<bool>,
    pub above_slow: Option<bool>,
    /// Slope of each MA over the last 20 bars.
    pub fast_slope: Option<Slope>,
    pub slow_slope: Option<Slope>,
    /// % above (+) / below (−) the slow MA.
    pub extension_pct_vs_slow: Option<Decimal>,
    /// Standing rule 13: >20% above the slow (200-bar) MA = extension gate.
    pub rule13_extension_gate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructureRead {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub bars_analyzed: usize,
    pub last_close: Decimal,
    pub last_bar_date: String,
    pub pivot_window: usize,
    pub structure: StructureClass,
    /// Last 4-6 alternating swings, oldest first.
    pub swings: Vec<Swing>,
    pub last_support_break: Option<BreakEvent>,
    pub last_resistance_break: Option<BreakEvent>,
    pub ma: MaPosture,
    pub verdict: String,
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Aggregate daily history into bars for the requested timeframe.
/// History must be oldest-first (as `price_history` getters return it).
pub fn aggregate(history: &[HistoryRecord], timeframe: Timeframe) -> Vec<Bar> {
    match timeframe {
        Timeframe::Daily => history
            .iter()
            .map(|r| Bar {
                date: r.date.clone(),
                close: r.close,
            })
            .collect(),
        Timeframe::Weekly | Timeframe::Monthly => {
            let mut bars: Vec<Bar> = Vec::new();
            let mut current_key: Option<(i32, u32)> = None;
            for row in history {
                let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
                    continue;
                };
                let key = match timeframe {
                    Timeframe::Weekly => {
                        let iso = date.iso_week();
                        (iso.year(), iso.week())
                    }
                    Timeframe::Monthly => (date.year(), date.month()),
                    Timeframe::Daily => unreachable!(),
                };
                if current_key == Some(key) {
                    if let Some(last) = bars.last_mut() {
                        last.date = row.date.clone();
                        last.close = row.close;
                    }
                } else {
                    current_key = Some(key);
                    bars.push(Bar {
                        date: row.date.clone(),
                        close: row.close,
                    });
                }
            }
            bars
        }
    }
}

// ---------------------------------------------------------------------------
// Swings
// ---------------------------------------------------------------------------

/// N-bar pivot detection on closes. A pivot high requires the close to be
/// at least every close in the N bars to its left and strictly above every
/// close in the N bars to its right (right-strict so flat stretches don't
/// emit a pivot per bar; ties resolve to the earliest bar). Pivots in the
/// last N bars are unconfirmed and not emitted. Consecutive same-kind
/// pivots are compressed to the more extreme one so the output alternates.
pub fn detect_swings(bars: &[Bar], n: usize) -> Vec<Swing> {
    if bars.len() < 2 * n + 1 {
        return Vec::new();
    }
    let mut raw: Vec<Swing> = Vec::new();
    for i in n..bars.len() - n {
        let c = bars[i].close;
        let left = &bars[i - n..i];
        let right = &bars[i + 1..=i + n];
        let is_high =
            left.iter().all(|b| c >= b.close) && right.iter().all(|b| c > b.close);
        let is_low =
            left.iter().all(|b| c <= b.close) && right.iter().all(|b| c < b.close);
        if is_high {
            raw.push(Swing {
                date: bars[i].date.clone(),
                price: c,
                kind: SwingKind::High,
                label: None,
                bar_index: i,
            });
        }
        if is_low && !is_high {
            raw.push(Swing {
                date: bars[i].date.clone(),
                price: c,
                kind: SwingKind::Low,
                label: None,
                bar_index: i,
            });
        }
    }

    // Compress consecutive same-kind swings: keep the higher high / lower low.
    let mut alternating: Vec<Swing> = Vec::new();
    for swing in raw {
        match alternating.last_mut() {
            Some(prev) if prev.kind == swing.kind => {
                let replace = match swing.kind {
                    SwingKind::High => swing.price >= prev.price,
                    SwingKind::Low => swing.price <= prev.price,
                };
                if replace {
                    *prev = swing;
                }
            }
            _ => alternating.push(swing),
        }
    }
    alternating
}

/// Label each swing HH/LH/HL/LL relative to the previous swing of the same
/// kind, in place. Expects swings oldest-first.
fn label_swings(swings: &mut [Swing]) {
    let mut prev_high: Option<Decimal> = None;
    let mut prev_low: Option<Decimal> = None;
    for swing in swings.iter_mut() {
        match swing.kind {
            SwingKind::High => {
                swing.label = prev_high.map(|p| {
                    if swing.price > p { "HH" } else { "LH" }.to_string()
                });
                prev_high = Some(swing.price);
            }
            SwingKind::Low => {
                swing.label = prev_low.map(|p| {
                    if swing.price < p { "LL" } else { "HL" }.to_string()
                });
                prev_low = Some(swing.price);
            }
        }
    }
}

/// Classify structure from the last 4-6 alternating swings: UPTREND needs
/// ascending highs AND ascending lows, DOWNTREND descending both, anything
/// mixed is RANGE. Fewer than 2 highs + 2 lows in the window → Insufficient.
pub fn classify_structure(swings: &[Swing]) -> StructureClass {
    let window: Vec<&Swing> = swings.iter().rev().take(6).collect();
    let mut highs: Vec<Decimal> = window
        .iter()
        .filter(|s| s.kind == SwingKind::High)
        .map(|s| s.price)
        .collect();
    let mut lows: Vec<Decimal> = window
        .iter()
        .filter(|s| s.kind == SwingKind::Low)
        .map(|s| s.price)
        .collect();
    // window was collected newest-first; restore chronological order
    highs.reverse();
    lows.reverse();

    if highs.len() < 2 || lows.len() < 2 {
        return StructureClass::Insufficient;
    }

    let ascending = |v: &[Decimal]| v.windows(2).all(|w| w[1] > w[0]);
    let descending = |v: &[Decimal]| v.windows(2).all(|w| w[1] < w[0]);

    if ascending(&highs) && ascending(&lows) {
        StructureClass::Uptrend
    } else if descending(&highs) && descending(&lows) {
        StructureClass::Downtrend
    } else {
        StructureClass::Range
    }
}

// ---------------------------------------------------------------------------
// Break-of-structure
// ---------------------------------------------------------------------------

/// Walk the bars chronologically, activating each swing once it is
/// confirmed (N bars after the pivot), and record a break whenever a close
/// takes out the active swing low (support break) or swing high
/// (resistance break). Returns the most recent break of each kind.
pub fn detect_breaks(
    bars: &[Bar],
    swings: &[Swing],
    n: usize,
) -> (Option<BreakEvent>, Option<BreakEvent>) {
    let mut support_break: Option<BreakEvent> = None;
    let mut resistance_break: Option<BreakEvent> = None;
    let mut active_low: Option<&Swing> = None;
    let mut active_high: Option<&Swing> = None;
    let mut swing_iter = swings.iter().peekable();

    for (i, bar) in bars.iter().enumerate() {
        // Activate swings confirmed at this bar.
        while let Some(s) = swing_iter.peek() {
            if s.bar_index + n <= i {
                match s.kind {
                    SwingKind::Low => active_low = Some(s),
                    SwingKind::High => active_high = Some(s),
                }
                swing_iter.next();
            } else {
                break;
            }
        }
        if let Some(low) = active_low {
            if bar.close < low.price {
                support_break = Some(BreakEvent {
                    date: bar.date.clone(),
                    level: low.price,
                    swing_date: low.date.clone(),
                });
                active_low = None;
            }
        }
        if let Some(high) = active_high {
            if bar.close > high.price {
                resistance_break = Some(BreakEvent {
                    date: bar.date.clone(),
                    level: high.price,
                    swing_date: high.date.clone(),
                });
                active_high = None;
            }
        }
    }
    (support_break, resistance_break)
}

// ---------------------------------------------------------------------------
// MA posture
// ---------------------------------------------------------------------------

fn sma_at(closes: &[Decimal], period: usize, end: usize) -> Option<Decimal> {
    if end < period || end > closes.len() {
        return None;
    }
    let slice = &closes[end - period..end];
    let sum: Decimal = slice.iter().copied().sum();
    Some(sum / Decimal::from(period))
}

fn slope_over(closes: &[Decimal], period: usize, lookback: usize) -> Option<Slope> {
    let now = sma_at(closes, period, closes.len())?;
    let then = sma_at(closes, period, closes.len().checked_sub(lookback)?)?;
    Some(if now > then {
        Slope::Rising
    } else if now < then {
        Slope::Falling
    } else {
        Slope::Flat
    })
}

const SLOPE_LOOKBACK_BARS: usize = 20;
const RULE13_EXTENSION_GATE_PCT: i64 = 20;

pub fn ma_posture(bars: &[Bar], timeframe: Timeframe) -> MaPosture {
    let (fast_period, slow_period) = timeframe.ma_periods();
    let closes: Vec<Decimal> = bars.iter().map(|b| b.close).collect();
    let last_close = closes.last().copied();

    let fast_ma = sma_at(&closes, fast_period, closes.len());
    let slow_ma = sma_at(&closes, slow_period, closes.len());
    let extension_pct_vs_slow = match (last_close, slow_ma) {
        (Some(c), Some(ma)) if ma > Decimal::ZERO => {
            Some(((c - ma) / ma * Decimal::from(100)).round_dp(1))
        }
        _ => None,
    };
    let rule13_extension_gate = extension_pct_vs_slow
        .map(|e| e > Decimal::from(RULE13_EXTENSION_GATE_PCT))
        .unwrap_or(false);

    MaPosture {
        fast_period,
        slow_period,
        fast_ma: fast_ma.map(|d| d.round_dp(2)),
        slow_ma: slow_ma.map(|d| d.round_dp(2)),
        above_fast: match (last_close, fast_ma) {
            (Some(c), Some(ma)) => Some(c >= ma),
            _ => None,
        },
        above_slow: match (last_close, slow_ma) {
            (Some(c), Some(ma)) => Some(c >= ma),
            _ => None,
        },
        fast_slope: slope_over(&closes, fast_period, SLOPE_LOOKBACK_BARS),
        slow_slope: slope_over(&closes, slow_period, SLOPE_LOOKBACK_BARS),
        extension_pct_vs_slow,
        rule13_extension_gate,
    }
}

// ---------------------------------------------------------------------------
// Entry point + verdict
// ---------------------------------------------------------------------------

/// Full structure read for one symbol/timeframe. Returns None when the
/// aggregated bar series is too short for even pivot detection.
pub fn analyze(
    symbol: &str,
    timeframe: Timeframe,
    history: &[HistoryRecord],
) -> Option<StructureRead> {
    let bars = aggregate(history, timeframe);
    let n = timeframe.pivot_window();
    if bars.len() < 2 * n + 2 {
        return None;
    }

    let mut swings = detect_swings(&bars, n);
    label_swings(&mut swings);
    let structure = classify_structure(&swings);
    let (support_break, resistance_break) = detect_breaks(&bars, &swings, n);
    let ma = ma_posture(&bars, timeframe);

    let last = bars.last()?;
    let last_close = last.close;
    let last_bar_date = last.date.clone();

    // Keep only the last 6 swings in the output payload.
    let tail_start = swings.len().saturating_sub(6);
    let swings: Vec<Swing> = swings[tail_start..].to_vec();

    let verdict = build_verdict(
        timeframe,
        structure,
        &swings,
        &support_break,
        &resistance_break,
        &ma,
    );

    Some(StructureRead {
        symbol: symbol.to_uppercase(),
        timeframe,
        bars_analyzed: bars.len(),
        last_close,
        last_bar_date,
        pivot_window: n,
        structure,
        swings,
        last_support_break: support_break,
        last_resistance_break: resistance_break,
        ma,
        verdict,
    })
}

fn fmt_price(p: Decimal) -> String {
    let rounded = if p >= Decimal::from(1000) {
        p.round_dp(0)
    } else {
        p.round_dp(2).normalize()
    };
    let s = rounded.to_string();
    // thousands separators on the integer part
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s, None),
    };
    let (sign, digits) = match int_part.strip_prefix('-') {
        Some(d) => ("-", d.to_string()),
        None => ("", int_part),
    };
    let mut grouped = String::new();
    for (idx, ch) in digits.chars().enumerate() {
        let remaining = digits.len() - idx;
        grouped.push(ch);
        if remaining > 1 && (remaining - 1) % 3 == 0 {
            grouped.push(',');
        }
    }
    match frac_part {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

fn fmt_date(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| d.format("%b-%d").to_string())
        .unwrap_or_else(|_| date.to_string())
}

fn build_verdict(
    timeframe: Timeframe,
    structure: StructureClass,
    swings: &[Swing],
    support_break: &Option<BreakEvent>,
    resistance_break: &Option<BreakEvent>,
    ma: &MaPosture,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Structure + most recent high/low swings with labels.
    let last_high = swings.iter().rev().find(|s| s.kind == SwingKind::High);
    let last_low = swings.iter().rev().find(|s| s.kind == SwingKind::Low);
    let mut head = structure.label().to_string();
    let mut swing_bits: Vec<String> = Vec::new();
    if let Some(h) = last_high {
        swing_bits.push(format!(
            "{} {} {}",
            h.label.as_deref().unwrap_or("high"),
            fmt_price(h.price),
            fmt_date(&h.date)
        ));
    }
    if let Some(l) = last_low {
        swing_bits.push(format!(
            "{} {} {}",
            l.label.as_deref().unwrap_or("low"),
            fmt_price(l.price),
            fmt_date(&l.date)
        ));
    }
    if !swing_bits.is_empty() {
        head.push_str(&format!(" ({})", swing_bits.join(", ")));
    }
    parts.push(head);

    // MA posture: combined fast/slow positioning + slow slope.
    let unit = timeframe.ma_unit();
    let slope_word = |s: Option<Slope>| match s {
        Some(Slope::Rising) => "rising",
        Some(Slope::Falling) => "falling",
        Some(Slope::Flat) => "flat",
        None => "?",
    };
    match (ma.above_fast, ma.above_slow) {
        (Some(true), Some(true)) => parts.push(format!(
            "above {} {}{unit}/{}{unit} MAs",
            slope_word(ma.slow_slope),
            ma.fast_period,
            ma.slow_period
        )),
        (Some(false), Some(false)) => parts.push(format!(
            "below {} {}{unit}/{}{unit} MAs",
            slope_word(ma.slow_slope),
            ma.fast_period,
            ma.slow_period
        )),
        (Some(f), Some(s)) => parts.push(format!(
            "{} {}{unit} MA, {} {} {}{unit} MA",
            if f { "above" } else { "below" },
            ma.fast_period,
            if s { "above" } else { "below" },
            slope_word(ma.slow_slope),
            ma.slow_period
        )),
        _ => {}
    }

    // Most recent break-of-structure (whichever printed later).
    let break_part = match (support_break, resistance_break) {
        (Some(s), Some(r)) => {
            if s.date >= r.date {
                Some(format!(
                    "support {} broken {}",
                    fmt_price(s.level),
                    fmt_date(&s.date)
                ))
            } else {
                Some(format!(
                    "resistance {} broken {}",
                    fmt_price(r.level),
                    fmt_date(&r.date)
                ))
            }
        }
        (Some(s), None) => Some(format!(
            "support {} broken {}",
            fmt_price(s.level),
            fmt_date(&s.date)
        )),
        (None, Some(r)) => Some(format!(
            "resistance {} broken {}",
            fmt_price(r.level),
            fmt_date(&r.date)
        )),
        (None, None) => None,
    };
    if let Some(b) = break_part {
        parts.push(b);
    }

    // Extension vs slow MA (standing rule 13 gate when >20% above).
    if let Some(ext) = ma.extension_pct_vs_slow {
        let mut e = format!(
            "extension {ext:+}% vs {}{unit} MA",
            ma.slow_period,
            unit = unit
        );
        if ma.rule13_extension_gate {
            e.push_str(" (>20% — standing rule 13 extension gate)");
        }
        parts.push(e);
    }

    format!("{}: {}", timeframe.label(), parts.join(", "))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Build daily history starting 2026-01-01 from a slice of close prices
    /// (consecutive calendar days — date arithmetic, not trading calendar).
    fn history_from(closes: &[i64]) -> Vec<HistoryRecord> {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        closes
            .iter()
            .enumerate()
            .map(|(i, c)| HistoryRecord {
                date: (start + chrono::Duration::days(i as i64))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from(*c),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect()
    }

    /// Zig-zag generator: walks linearly between waypoints, 4 bars per leg.
    fn zigzag(waypoints: &[i64]) -> Vec<i64> {
        let mut closes = Vec::new();
        for w in waypoints.windows(2) {
            let (a, b) = (w[0], w[1]);
            for step in 0..4 {
                closes.push(a + (b - a) * step / 4);
            }
        }
        closes.push(*waypoints.last().unwrap());
        closes
    }

    #[test]
    fn uptrend_classified_from_hh_hl() {
        // HL 100→ HH 120 → HL 110 → HH 135 → HL 122 → HH 150
        let closes = zigzag(&[100, 120, 110, 135, 122, 150, 140]);
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        assert_eq!(read.structure, StructureClass::Uptrend, "{:?}", read.swings);
        // Swing labels should include HH and HL
        let labels: Vec<&str> = read
            .swings
            .iter()
            .filter_map(|s| s.label.as_deref())
            .collect();
        assert!(labels.contains(&"HH"), "labels: {labels:?}");
        assert!(labels.contains(&"HL"), "labels: {labels:?}");
    }

    #[test]
    fn downtrend_classified_from_lh_ll() {
        let closes = zigzag(&[150, 122, 135, 110, 120, 95, 105]);
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        assert_eq!(
            read.structure,
            StructureClass::Downtrend,
            "{:?}",
            read.swings
        );
        let labels: Vec<&str> = read
            .swings
            .iter()
            .filter_map(|s| s.label.as_deref())
            .collect();
        assert!(labels.contains(&"LH"), "labels: {labels:?}");
        assert!(labels.contains(&"LL"), "labels: {labels:?}");
        assert!(read.verdict.starts_with("DAILY: downtrend"), "{}", read.verdict);
    }

    #[test]
    fn range_classified_from_mixed_swings() {
        // Oscillates between the same band: highs ~120, lows ~100.
        let closes = zigzag(&[100, 120, 100, 120, 100, 120, 110]);
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        assert_eq!(read.structure, StructureClass::Range, "{:?}", read.swings);
    }

    #[test]
    fn support_break_detected_on_close_below_swing_low() {
        // Establish a swing low at 110, rally, then close through it.
        let closes = zigzag(&[100, 130, 110, 140, 90, 95]);
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        let brk = read
            .last_support_break
            .expect("support break should be detected");
        assert_eq!(brk.level, dec!(110));
        assert!(read.verdict.contains("support 110 broken"), "{}", read.verdict);
    }

    #[test]
    fn resistance_break_detected_on_close_above_swing_high() {
        let closes = zigzag(&[120, 100, 115, 95, 130, 125]);
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        let brk = read
            .last_resistance_break
            .expect("resistance break should be detected");
        assert_eq!(brk.level, dec!(115));
    }

    #[test]
    fn insufficient_history_returns_none() {
        let history = history_from(&[100, 101, 102]);
        assert!(analyze("TEST", Timeframe::Daily, &history).is_none());
    }

    #[test]
    fn too_few_swings_classified_insufficient() {
        // Monotonic ramp: no pivots at all.
        let closes: Vec<i64> = (100..160).collect();
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        assert_eq!(read.structure, StructureClass::Insufficient);
        assert!(read.swings.is_empty());
    }

    #[test]
    fn weekly_aggregation_buckets_by_iso_week() {
        // 28 consecutive days = 5 ISO-week buckets (Jan 1 2026 is a Thursday).
        let closes: Vec<i64> = (0..28).map(|i| 100 + i).collect();
        let history = history_from(&closes);
        let bars = aggregate(&history, Timeframe::Weekly);
        assert_eq!(bars.len(), 5, "{bars:?}");
        // Each weekly close = last daily close of the bucket.
        assert_eq!(bars.last().unwrap().close, Decimal::from(127));
    }

    #[test]
    fn monthly_aggregation_buckets_by_month() {
        let closes: Vec<i64> = (0..62).map(|i| 100 + i).collect();
        let history = history_from(&closes);
        let bars = aggregate(&history, Timeframe::Monthly);
        assert_eq!(bars.len(), 3); // Jan, Feb, Mar 2026
        assert_eq!(bars[0].close, Decimal::from(130)); // Jan-31 close
    }

    #[test]
    fn ma_posture_extension_and_rule13_gate() {
        // 200 flat bars at 100, then a spike to 130 (=> ~+30% vs 200dma).
        let mut closes = vec![100i64; 219];
        closes.push(130);
        let history = history_from(&closes);
        let bars = aggregate(&history, Timeframe::Daily);
        let ma = ma_posture(&bars, Timeframe::Daily);
        let ext = ma.extension_pct_vs_slow.unwrap();
        assert!(ext > Decimal::from(20), "extension {ext}");
        assert!(ma.rule13_extension_gate);
        assert_eq!(ma.above_slow, Some(true));
    }

    #[test]
    fn ma_slope_falling_in_decline() {
        // Long steady decline: both MAs falling, price below both.
        let closes: Vec<i64> = (0..260).map(|i| 600 - i).collect();
        let history = history_from(&closes);
        let bars = aggregate(&history, Timeframe::Daily);
        let ma = ma_posture(&bars, Timeframe::Daily);
        assert_eq!(ma.slow_slope, Some(Slope::Falling));
        assert_eq!(ma.fast_slope, Some(Slope::Falling));
        assert_eq!(ma.above_fast, Some(false));
        assert_eq!(ma.above_slow, Some(false));
        assert!(ma.extension_pct_vs_slow.unwrap() < Decimal::ZERO);
    }

    #[test]
    fn timeframe_parse_accepts_aliases() {
        assert_eq!(Timeframe::parse("daily").unwrap(), Timeframe::Daily);
        assert_eq!(Timeframe::parse("1w").unwrap(), Timeframe::Weekly);
        assert_eq!(Timeframe::parse("MONTHLY").unwrap(), Timeframe::Monthly);
        assert!(Timeframe::parse("hourly").is_err());
    }

    #[test]
    fn fmt_price_groups_thousands() {
        assert_eq!(fmt_price(dec!(4285.4)), "4,285");
        assert_eq!(fmt_price(dec!(104250)), "104,250");
        assert_eq!(fmt_price(dec!(36.50)), "36.5");
    }

    #[test]
    fn verdict_mentions_structure_ma_and_extension() {
        let closes = zigzag(&[150, 122, 135, 110, 120, 95, 105]);
        let history = history_from(&closes);
        let read = analyze("TEST", Timeframe::Daily, &history).unwrap();
        assert!(read.verdict.starts_with("DAILY:"), "{}", read.verdict);
        assert!(read.verdict.contains("downtrend"), "{}", read.verdict);
    }
}
