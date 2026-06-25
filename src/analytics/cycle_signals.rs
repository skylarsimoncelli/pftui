//! Mechanical cycle-bottom signal suite — a deterministic N-of-7 confluence of
//! independent cycle-low confirmations.
//!
//! The read collapses 10 atomic Pine-ported sub-signals into **7 composite
//! criteria** (each scored 0/1), plus a non-counted bonus:
//!
//! | # | composite | atomic sub-signals (components) | natural TF |
//! |---|---|---|---|
//! | 1 | Momentum line turning up          | `rsi_ma_turned_up` | requested (monthly) |
//! | 2 | Momentum line above price momentum| `rsi_ma_cross_above_rsi` | requested |
//! | 3 | Double-smoothed stochastic bottoming | `dss_turned_up` && `dss_cross_above_trigger` (context: `dss_oversold`) | requested |
//! | 4 | Roofing filter confirming up      | `erf_bottom_zone` && `erf_turned_up` | requested |
//! | 5 | Volatility bands bullish          | `cyberbands_bullish` | DAILY |
//! | 6 | Significant reversal dots         | `cyberdots_bullish` | weekly + monthly |
//! | 7 | Trend line reclaimed              | `cyberline_reclaim` | WEEKLY |
//! | bonus | Pi-cycle bottom (not counted) | `pi_cycle_bottom` | daily |
//!
//! Confluence = `met_count / 7`. The atomic booleans + their oscillator values
//! are preserved on the typed struct and emitted as `components[]` per criterion
//! so nothing is lost. The suite is position-only / measurement — it never emits
//! a price target. All math is `f64`; no money flows through.

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use crate::analytics::cyber::{self, bands::QbState};
use crate::indicators::{dss_bressert, ehlers_roofing, rsi_ma};
use crate::models::price::HistoryRecord;

/// Requested evaluation timeframe for the TF-relative criteria
/// (RSI-MA / DSS / ERF). The fixed-TF criteria (bands=daily, dots=weekly+
/// monthly, line=weekly, pi=daily) always run on their own aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalTimeframe {
    Daily,
    Weekly,
    Monthly,
}

impl SignalTimeframe {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "daily" | "1d" | "d" => Ok(SignalTimeframe::Daily),
            "weekly" | "1w" | "w" => Ok(SignalTimeframe::Weekly),
            "monthly" | "1mo" | "m" => Ok(SignalTimeframe::Monthly),
            other => {
                anyhow::bail!("unknown timeframe '{other}' — expected daily, weekly, or monthly")
            }
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SignalTimeframe::Daily => "daily",
            SignalTimeframe::Weekly => "weekly",
            SignalTimeframe::Monthly => "monthly",
        }
    }
}

/// One atomic sub-signal backing a composite criterion: the raw boolean plus
/// its numeric oscillator reading (when one exists). Exposed in JSON so nothing
/// from the underlying 10-primitive read is lost.
#[derive(Debug, Clone, Serialize)]
pub struct Component {
    /// Stable machine key (e.g. `dss_turned_up`).
    pub key: String,
    /// Human label (no practitioner names).
    pub label: String,
    /// Whether this sub-signal is active on the latest bar.
    pub met: bool,
    /// Backing oscillator value, when this sub-signal has a numeric reading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Previous-bar value for edge conditions. Present when the native series
    /// can expose the latest two bars; lets agents quantify turn/cross distance
    /// without recomputing indicators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_value: Option<f64>,
    /// Current comparison value (trigger, paired oscillator, zero-line, price,
    /// or threshold depending on the component).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison_value: Option<f64>,
    /// Previous comparison value for cross conditions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_comparison_value: Option<f64>,
    /// Signed distance to the component's trigger on the latest bar. Positive
    /// means the latest value is on the met side of the threshold/cross line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_to_trigger: Option<f64>,
}

/// One evaluated COMPOSITE criterion row (1 of 7) for display/itemization.
#[derive(Debug, Clone, Serialize)]
pub struct Criterion {
    /// Stable machine key (e.g. `momentum_turning_up`).
    pub key: String,
    /// Human label (no practitioner names).
    pub label: String,
    /// Whether this composite criterion is firing on the latest bar.
    pub met: bool,
    /// One-line plain-language detail with the backing value(s).
    pub detail: String,
    /// Atomic sub-signals that make up this composite (+ context flags).
    pub components: Vec<Component>,
}

/// A focused, operator-facing watch item for the core cycle-bottom checklist.
/// These are the four monthly primitives that matter most for the cycle-low
/// report; the broader N/7 suite keeps the additional confluence criteria.
#[derive(Debug, Clone, Serialize)]
pub struct WatchItem {
    /// Stable machine key matching the related composite criterion.
    pub key: String,
    /// Human label (no practitioner names).
    pub label: String,
    /// Whether the full watch item is firing.
    pub met: bool,
    /// Number of required atomic subconditions currently met.
    pub met_components: usize,
    /// Number of required atomic subconditions.
    pub total_components: usize,
    /// One-line plain-language detail with the backing value(s).
    pub detail: String,
    /// Required atomic subconditions that make up this watch item.
    pub components: Vec<Component>,
}

/// The non-counted bonus signal (Pi-Cycle bottom on daily).
#[derive(Debug, Clone, Serialize)]
pub struct BonusSignal {
    pub key: String,
    pub label: String,
    pub met: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_bottom: Option<String>,
}

/// Full cycle-bottom signal read.
#[derive(Debug, Clone, Serialize)]
pub struct CycleBottomSignals {
    pub symbol: String,
    /// The requested timeframe (drives RSI-MA / DSS / ERF).
    pub timeframe: SignalTimeframe,
    pub as_of: String,

    // RSI + RSI-MA (requested TF).
    pub rsi: Option<f64>,
    pub rsi_ma: Option<f64>,
    pub rsi_ma_turned_up: bool,
    pub rsi_ma_cross_above_rsi: bool,

    // DSS Bressert (requested TF).
    pub dss: Option<f64>,
    pub dss_trigger: Option<f64>,
    pub dss_turned_up: bool,
    pub dss_cross_above_trigger: bool,
    pub dss_oversold: bool,

    // Ehlers roofing filter (requested TF).
    pub erf: Option<f64>,
    pub erf_positive: bool,
    /// Backward-compatible alias for `erf_positive`.
    pub erf_green: bool,
    pub erf_bottom_zone: bool,
    pub erf_turned_up: bool,

    // CyberBands (daily).
    pub cyberbands_state: Option<String>,
    pub cyberbands_bullish: bool,

    // CyberDots (weekly + monthly).
    pub cyberdots_weekly_strength: Option<u8>,
    pub cyberdots_monthly_strength: Option<u8>,
    pub cyberdots_bullish: bool,

    // CyberLine (weekly).
    pub cyberline_value: Option<f64>,
    pub cyberline_price_above: Option<bool>,
    pub cyberline_reclaim: bool,

    // Pi-cycle bottom (daily, bonus).
    pub pi_cycle_bottom: bool,
    pub pi_cycle_last_bottom: Option<String>,

    /// Ordered list of the 7 composite criteria.
    pub criteria: Vec<Criterion>,
    /// Focused four-item cycle-bottom watch list with atomic progress.
    pub core_watch: Vec<WatchItem>,
    /// How many of `total` composite criteria are firing.
    pub met_count: usize,
    /// Total composite criteria (always 7).
    pub total: usize,
    /// Non-counted bonus signal (Pi-Cycle bottom), present when computable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonus: Option<BonusSignal>,
    /// One-line plain-language verdict.
    pub verdict: String,
}

/// Full cycle-TOP signal read — the symmetric mirror of [`CycleBottomSignals`].
/// Same struct shape (criteria[], components[], core_watch[], met_count/total)
/// so downstream code and renderers stay symmetric; only the polarity of every
/// sub-signal is flipped (turn-DOWNs, cross-BELOWs, top-zone, bearish bands,
/// net-bearish dots, trend line LOST). Position-only / measurement; no target.
#[derive(Debug, Clone, Serialize)]
pub struct CycleTopSignals {
    pub symbol: String,
    /// The requested timeframe (drives RSI-MA / DSS / ERF).
    pub timeframe: SignalTimeframe,
    pub as_of: String,

    // RSI + RSI-MA (requested TF).
    pub rsi: Option<f64>,
    pub rsi_ma: Option<f64>,
    pub rsi_ma_turned_down: bool,
    pub rsi_ma_cross_below_rsi: bool,

    // DSS Bressert (requested TF).
    pub dss: Option<f64>,
    pub dss_trigger: Option<f64>,
    pub dss_turned_down: bool,
    pub dss_cross_below_trigger: bool,
    pub dss_overbought: bool,

    // Ehlers roofing filter (requested TF).
    pub erf: Option<f64>,
    /// True when the roofing filter is in the negative (red) zone.
    pub erf_negative: bool,
    pub erf_top_zone: bool,
    pub erf_turned_down: bool,

    // CyberBands (daily).
    pub cyberbands_state: Option<String>,
    pub cyberbands_bearish: bool,

    // CyberDots (weekly + monthly).
    pub cyberdots_weekly_down_strength: Option<u8>,
    pub cyberdots_monthly_down_strength: Option<u8>,
    pub cyberdots_bearish: bool,

    // CyberLine (weekly).
    pub cyberline_value: Option<f64>,
    pub cyberline_price_above: Option<bool>,
    pub cyberline_lost: bool,

    // Pi-cycle top (daily, bonus).
    pub pi_cycle_top: bool,
    pub pi_cycle_last_top: Option<String>,

    /// Ordered list of the 7 composite criteria.
    pub criteria: Vec<Criterion>,
    /// Focused four-item cycle-top watch list with atomic progress.
    pub core_watch: Vec<WatchItem>,
    /// How many of `total` composite criteria are firing.
    pub met_count: usize,
    /// Total composite criteria (always 7).
    pub total: usize,
    /// Non-counted bonus signal (Pi-Cycle top), present when computable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonus: Option<BonusSignal>,
    /// One-line plain-language verdict.
    pub verdict: String,
}

// ---------------------------------------------------------------------------
// OHLC aggregation (close + high + low + dates) at daily/weekly/monthly.
// Mirrors the cyber module's OHLC fallbacks (open←prev close, high←max(o,c),
// low←min(o,c)) so DSS gets a real high/low channel even on close-only series.
// ---------------------------------------------------------------------------

struct Ohlc {
    dates: Vec<String>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
}

fn to_f64(d: rust_decimal::Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

fn build_daily_ohlc(history: &[HistoryRecord]) -> Ohlc {
    let n = history.len();
    let mut o = Ohlc {
        dates: Vec::with_capacity(n),
        high: Vec::with_capacity(n),
        low: Vec::with_capacity(n),
        close: Vec::with_capacity(n),
    };
    let mut prev_close: Option<f64> = None;
    for r in history {
        let close = to_f64(r.close);
        let open = r.open.map(to_f64).or(prev_close).unwrap_or(close);
        let high = r.high.map(to_f64).unwrap_or_else(|| open.max(close));
        let low = r.low.map(to_f64).unwrap_or_else(|| open.min(close));
        o.dates.push(r.date.clone());
        o.high.push(high.max(open.max(close)));
        o.low.push(low.min(open.min(close)));
        o.close.push(close);
        prev_close = Some(close);
    }
    o
}

/// Aggregate daily OHLC into weekly (ISO week) or monthly (calendar month)
/// bars: high=max, low=min, close/date=last of the period.
fn aggregate_ohlc(daily: &Ohlc, tf: SignalTimeframe) -> Ohlc {
    if tf == SignalTimeframe::Daily {
        return Ohlc {
            dates: daily.dates.clone(),
            high: daily.high.clone(),
            low: daily.low.clone(),
            close: daily.close.clone(),
        };
    }
    let mut o = Ohlc {
        dates: Vec::new(),
        high: Vec::new(),
        low: Vec::new(),
        close: Vec::new(),
    };
    let mut current: Option<(i32, u32)> = None;
    for i in 0..daily.dates.len() {
        let Ok(date) = NaiveDate::parse_from_str(&daily.dates[i], "%Y-%m-%d") else {
            continue;
        };
        let key = match tf {
            SignalTimeframe::Weekly => {
                let iso = date.iso_week();
                (iso.year(), iso.week())
            }
            SignalTimeframe::Monthly => (date.year(), date.month()),
            SignalTimeframe::Daily => unreachable!(),
        };
        if current == Some(key) {
            let last = o.dates.len() - 1;
            o.dates[last] = daily.dates[i].clone();
            o.high[last] = o.high[last].max(daily.high[i]);
            o.low[last] = o.low[last].min(daily.low[i]);
            o.close[last] = daily.close[i];
        } else {
            current = Some(key);
            o.dates.push(daily.dates[i].clone());
            o.high.push(daily.high[i]);
            o.low.push(daily.low[i]);
            o.close.push(daily.close[i]);
        }
    }
    o
}

/// Minimum daily bars for a meaningful read (monthly needs deep history for
/// the SMA/Gaussian chains; we require enough daily rows to build ~30 monthly
/// bars). Below this the engine returns `None`.
const MIN_DAILY_BARS: usize = 120;

/// Minimum daily bars for a meaningful read — exposed so the reliability
/// backtest can size its rolling-evaluation start point identically.
pub fn min_daily_bars() -> usize {
    MIN_DAILY_BARS
}

/// Compute the full cycle-bottom signal suite for `symbol` over DAILY
/// `history` (oldest-first). `timeframe` drives the RSI-MA / DSS / ERF
/// criteria; the cyber criteria run on their own fixed aggregations.
/// Returns `None` when history is too shallow.
pub fn cycle_bottom_signals(
    symbol: &str,
    history: &[HistoryRecord],
    timeframe: SignalTimeframe,
) -> Option<CycleBottomSignals> {
    if history.len() < MIN_DAILY_BARS {
        return None;
    }
    let daily = build_daily_ohlc(history);
    if daily.dates.is_empty() {
        return None;
    }
    let as_of = daily.dates.last().cloned().unwrap_or_default();
    let last_close = *daily.close.last()?;

    // --- Requested-TF aggregation for RSI-MA / DSS / ERF ---
    let tf_bars = aggregate_ohlc(&daily, timeframe);
    let weekly = aggregate_ohlc(&daily, SignalTimeframe::Weekly);
    let monthly = aggregate_ohlc(&daily, SignalTimeframe::Monthly);

    // RSI + RSI-MA.
    let rsi_state = rsi_ma::compute_rsi_ma_default(&tf_bars.close);
    let rsi = rsi_state.as_ref().and_then(rsi_ma::current_rsi);
    let rsi_ma_v = rsi_state.as_ref().and_then(rsi_ma::current_rsi_ma);
    let rsi_pair = rsi_state.as_ref().and_then(|s| last_two_opt(&s.rsi));
    let rsi_ma_pair = rsi_state.as_ref().and_then(|s| last_two_opt(&s.rsi_ma));
    let rsi_ma_turned_up = rsi_state
        .as_ref()
        .and_then(rsi_ma::ma_turned_up)
        .unwrap_or(false);
    let rsi_ma_cross_above_rsi = rsi_state
        .as_ref()
        .and_then(rsi_ma::ma_crossed_above_rsi)
        .unwrap_or(false);

    // DSS Bressert.
    let dss_state = dss_bressert::compute_dss_default(&tf_bars.close, &tf_bars.high, &tf_bars.low);
    let dss = dss_state.as_ref().and_then(dss_bressert::current_dss);
    let dss_trigger = dss_state.as_ref().and_then(dss_bressert::current_trigger);
    let dss_pair = dss_state.as_ref().and_then(|s| last_two_opt(&s.dss));
    let dss_trigger_pair = dss_state.as_ref().and_then(|s| last_two_opt(&s.trigger));
    let dss_turned_up = dss_state
        .as_ref()
        .and_then(dss_bressert::turned_up)
        .unwrap_or(false);
    let dss_cross_above_trigger = dss_state
        .as_ref()
        .and_then(dss_bressert::crossed_above_trigger)
        .unwrap_or(false);
    let dss_oversold = dss_state
        .as_ref()
        .and_then(|s| dss_bressert::is_oversold(s, 20.0))
        .unwrap_or(false);

    // Ehlers roofing filter.
    let erf_series = ehlers_roofing::compute_erf_default(&tf_bars.close);
    let erf = erf_series.as_ref().and_then(|s| ehlers_roofing::current(s));
    let erf_pair = erf_series.as_ref().and_then(|s| last_two_f64(s));
    let erf_positive = erf_series
        .as_ref()
        .and_then(|s| ehlers_roofing::is_green(s))
        .unwrap_or(false);
    let erf_bottom_zone = erf.map(|v| v < 0.0).unwrap_or(false);
    let erf_turned_up = erf_series
        .as_ref()
        .and_then(|s| ehlers_roofing::turned_up(s))
        .unwrap_or(false);

    // --- CyberBands on DAILY (operator: most relevant on daily) ---
    let bands = cyber::bands::compute_gaussian_bands(&daily.close, &daily.dates, 5);
    let cyberbands_state = bands.as_ref().map(|b| b.qb.label().to_string());
    let cyberbands_bullish = bands
        .as_ref()
        .map(|b| b.qb == QbState::Bullish)
        .unwrap_or(false);

    // --- CyberDots on WEEKLY + MONTHLY (relevant on higher TFs) ---
    let dots_weekly =
        cyber::dots::compute_dots(&weekly.close, &weekly.high, &weekly.low, &weekly.dates, 5);
    let dots_monthly = cyber::dots::compute_dots(
        &monthly.close,
        &monthly.high,
        &monthly.low,
        &monthly.dates,
        5,
    );
    // Net-bullish when an up-dot is active and stronger than any down-dot, on
    // either higher timeframe.
    let dot_bullish = |d: &cyber::dots::DotsRead| d.up_dot && d.up_strength >= d.down_strength;
    let cyberdots_weekly_strength = dots_weekly.as_ref().map(|d| d.up_strength);
    let cyberdots_monthly_strength = dots_monthly.as_ref().map(|d| d.up_strength);
    let cyberdots_bullish = dots_weekly.as_ref().map(dot_bullish).unwrap_or(false)
        || dots_monthly.as_ref().map(dot_bullish).unwrap_or(false);

    // --- CyberLine on WEEKLY (weekly reclaim = "bear basically over") ---
    let line = cyber::line::compute_line(&weekly.close, &weekly.high, &weekly.low, &weekly.dates);
    let cyberline_value = line.as_ref().map(|l| l.value);
    let cyberline_price_above = line.as_ref().map(|l| l.price_above);
    // Reclaim = price above the weekly line, OR a fresh bullish (above) cross
    // on the most recent weekly bar.
    let line_crosses = cyber::line::compute_line_crosses(&weekly.close, &weekly.dates);
    let fresh_above_cross = line_crosses
        .last()
        .map(|c| c.direction == "above" && weekly.dates.last() == Some(&c.date))
        .unwrap_or(false);
    let cyberline_reclaim =
        line.as_ref().map(|l| l.price_above).unwrap_or(false) || fresh_above_cross;

    // --- Pi-cycle bottom on DAILY (bonus) ---
    let pi = cyber::pi_cycle::compute_pi_cycle(&daily.close, &daily.dates);
    let pi_cycle_last_bottom = pi.as_ref().and_then(|p| p.last_bottom.clone());
    // "Recently" = within the last 120 daily bars (≈4 months — a bottom that
    // just fired still qualifies the current low).
    let pi_cycle_bottom = pi
        .as_ref()
        .and_then(|p| p.last_bottom.as_ref())
        .map(|d| within_recent(&daily.dates, d, 120))
        .unwrap_or(false);

    // --- Collapse the 10 atomic sub-signals into 7 composite criteria ---
    let tf_label = timeframe.label();
    let mut criteria: Vec<Criterion> = Vec::new();

    // 1. Momentum line turning up.
    criteria.push(Criterion {
        key: "momentum_turning_up".into(),
        label: "Momentum line turning up".into(),
        met: rsi_ma_turned_up,
        detail: format!("{tf_label} RSI {} · RSI-avg {}", fmt(rsi), fmt(rsi_ma_v)),
        components: vec![Component {
            key: "rsi_ma_turned_up".into(),
            label: "RSI average ticked up".into(),
            met: rsi_ma_turned_up,
            value: rsi_ma_v,
            previous_value: rsi_ma_pair.map(|(prev, _)| prev),
            comparison_value: None,
            previous_comparison_value: None,
            distance_to_trigger: rsi_ma_pair.map(|(prev, cur)| cur - prev),
        }],
    });

    // 2. Momentum line crossed above price momentum.
    criteria.push(Criterion {
        key: "momentum_above_price".into(),
        label: "Momentum line crossed above price momentum".into(),
        met: rsi_ma_cross_above_rsi,
        detail: format!("{tf_label} RSI-avg {} vs RSI {}", fmt(rsi_ma_v), fmt(rsi)),
        components: vec![Component {
            key: "rsi_ma_cross_above_rsi".into(),
            label: "RSI average reclaimed the RSI".into(),
            met: rsi_ma_cross_above_rsi,
            value: rsi_ma_v,
            previous_value: rsi_ma_pair.map(|(prev, _)| prev),
            comparison_value: rsi,
            previous_comparison_value: rsi_pair.map(|(prev, _)| prev),
            distance_to_trigger: rsi_ma_v.zip(rsi).map(|(ma, r)| ma - r),
        }],
    });

    // 3. Double-smoothed stochastic bottoming = turned up AND crossed trigger.
    //    (Oversold is a qualifying CONTEXT flag, not a firing condition.)
    let dss_bottoming = dss_turned_up && dss_cross_above_trigger;
    criteria.push(Criterion {
        key: "dss_bottoming".into(),
        label: "Double-smoothed stochastic bottoming".into(),
        met: dss_bottoming,
        detail: format!(
            "{tf_label} value {} vs trigger {}{}",
            fmt(dss),
            fmt(dss_trigger),
            if dss_oversold { " (oversold)" } else { "" }
        ),
        components: vec![
            Component {
                key: "dss_turned_up".into(),
                label: "DSS ticked up".into(),
                met: dss_turned_up,
                value: dss,
                previous_value: dss_pair.map(|(prev, _)| prev),
                comparison_value: None,
                previous_comparison_value: None,
                distance_to_trigger: dss_pair.map(|(prev, cur)| cur - prev),
            },
            Component {
                key: "dss_cross_above_trigger".into(),
                label: "DSS crossed above trigger".into(),
                met: dss_cross_above_trigger,
                value: dss,
                previous_value: dss_pair.map(|(prev, _)| prev),
                comparison_value: dss_trigger,
                previous_comparison_value: dss_trigger_pair.map(|(prev, _)| prev),
                distance_to_trigger: dss.zip(dss_trigger).map(|(d, t)| d - t),
            },
            Component {
                key: "dss_oversold".into(),
                label: "DSS oversold (<20) — context".into(),
                met: dss_oversold,
                value: dss,
                previous_value: dss_pair.map(|(prev, _)| prev),
                comparison_value: Some(20.0),
                previous_comparison_value: Some(20.0),
                distance_to_trigger: dss.map(|d| 20.0 - d),
            },
        ],
    });

    // 4. Roofing filter confirming up = bottom-zone (negative) AND turned up.
    let erf_confirming = erf_bottom_zone && erf_turned_up;
    criteria.push(Criterion {
        key: "roofing_confirming_up".into(),
        label: "Roofing filter confirming up".into(),
        met: erf_confirming,
        detail: format!("{tf_label} value {}", fmt(erf)),
        components: vec![
            Component {
                key: "erf_bottom_zone".into(),
                label: "Roofing filter in bottom zone (<0)".into(),
                met: erf_bottom_zone,
                value: erf,
                previous_value: erf_pair.map(|(prev, _)| prev),
                comparison_value: Some(0.0),
                previous_comparison_value: Some(0.0),
                distance_to_trigger: erf.map(|v| -v),
            },
            Component {
                key: "erf_turned_up".into(),
                label: "Roofing filter ticked up".into(),
                met: erf_turned_up,
                value: erf,
                previous_value: erf_pair.map(|(prev, _)| prev),
                comparison_value: None,
                previous_comparison_value: None,
                distance_to_trigger: erf_pair.map(|(prev, cur)| cur - prev),
            },
        ],
    });

    // 5. Volatility bands bullish (daily).
    criteria.push(Criterion {
        key: "volatility_bands_bullish".into(),
        label: "Volatility bands bullish (daily)".into(),
        met: cyberbands_bullish,
        detail: format!(
            "daily band state: {}",
            cyberbands_state.as_deref().unwrap_or("n/a")
        ),
        components: vec![Component {
            key: "cyberbands_bullish".into(),
            label: "Daily momentum bands in bullish state".into(),
            met: cyberbands_bullish,
            value: None,
            previous_value: None,
            comparison_value: None,
            previous_comparison_value: None,
            distance_to_trigger: None,
        }],
    });

    // 6. Significant reversal dots (weekly/monthly).
    criteria.push(Criterion {
        key: "reversal_dots".into(),
        label: "Significant reversal dots (weekly/monthly)".into(),
        met: cyberdots_bullish,
        detail: format!(
            "weekly up {} · monthly up {}",
            cyberdots_weekly_strength
                .map(|s| s.to_string())
                .unwrap_or_else(|| "n/a".into()),
            cyberdots_monthly_strength
                .map(|s| s.to_string())
                .unwrap_or_else(|| "n/a".into())
        ),
        components: vec![Component {
            key: "cyberdots_bullish".into(),
            label: "Higher-timeframe strength dots net-bullish".into(),
            met: cyberdots_bullish,
            value: cyberdots_weekly_strength
                .or(cyberdots_monthly_strength)
                .map(|s| s as f64),
            previous_value: None,
            comparison_value: None,
            previous_comparison_value: None,
            distance_to_trigger: None,
        }],
    });

    // 7. Trend line reclaimed (weekly).
    criteria.push(Criterion {
        key: "trend_line_reclaimed".into(),
        label: "Trend line reclaimed (weekly)".into(),
        met: cyberline_reclaim,
        detail: format!(
            "weekly line {} · price {} ({})",
            fmt(cyberline_value),
            fmt(Some(last_close)),
            match cyberline_price_above {
                Some(true) => "above",
                Some(false) => "below",
                None => "n/a",
            }
        ),
        components: vec![Component {
            key: "cyberline_reclaim".into(),
            label: "Price reclaimed the weekly trackline".into(),
            met: cyberline_reclaim,
            value: cyberline_value,
            previous_value: None,
            comparison_value: Some(last_close),
            previous_comparison_value: None,
            distance_to_trigger: cyberline_value.map(|line| last_close - line),
        }],
    });

    let core_total = 7usize;
    debug_assert_eq!(criteria.len(), core_total, "must emit exactly 7 composites");
    let core_watch = build_core_watch(&criteria);
    let met_count = criteria.iter().filter(|c| c.met).count();

    // Bonus pi-cycle (daily) — reported separately, NEVER counted in the 7.
    let bonus = pi.as_ref().map(|_| BonusSignal {
        key: "pi_cycle_bottom".into(),
        label: "Pi-cycle bottom fired recently (bonus)".into(),
        met: pi_cycle_bottom,
        detail: match &pi_cycle_last_bottom {
            Some(d) => format!("last bottom {d}"),
            None => "no bottom in window".into(),
        },
        last_bottom: pi_cycle_last_bottom.clone(),
    });

    let verdict = build_verdict(timeframe, met_count, core_total, &criteria);

    Some(CycleBottomSignals {
        symbol: symbol.to_string(),
        timeframe,
        as_of,
        rsi,
        rsi_ma: rsi_ma_v,
        rsi_ma_turned_up,
        rsi_ma_cross_above_rsi,
        dss,
        dss_trigger,
        dss_turned_up,
        dss_cross_above_trigger,
        dss_oversold,
        erf,
        erf_positive,
        erf_green: erf_positive,
        erf_bottom_zone,
        erf_turned_up,
        cyberbands_state,
        cyberbands_bullish,
        cyberdots_weekly_strength,
        cyberdots_monthly_strength,
        cyberdots_bullish,
        cyberline_value,
        cyberline_price_above,
        cyberline_reclaim,
        pi_cycle_bottom,
        pi_cycle_last_bottom,
        criteria,
        core_watch,
        met_count,
        total: core_total,
        bonus,
        verdict,
    })
}

/// Compute the full cycle-TOP signal suite — the symmetric mirror of
/// [`cycle_bottom_signals`]. Reuses the same atomic indicator computations and
/// reads the bearish/topping side of each (turn-DOWNs, cross-BELOWs, top zone,
/// bearish bands, net-bearish dots, weekly trend line LOST). Returns `None`
/// when history is too shallow. Position-only / measurement; no price target.
pub fn cycle_top_signals(
    symbol: &str,
    history: &[HistoryRecord],
    timeframe: SignalTimeframe,
) -> Option<CycleTopSignals> {
    if history.len() < MIN_DAILY_BARS {
        return None;
    }
    let daily = build_daily_ohlc(history);
    if daily.dates.is_empty() {
        return None;
    }
    let as_of = daily.dates.last().cloned().unwrap_or_default();
    let last_close = *daily.close.last()?;

    let tf_bars = aggregate_ohlc(&daily, timeframe);
    let weekly = aggregate_ohlc(&daily, SignalTimeframe::Weekly);
    let monthly = aggregate_ohlc(&daily, SignalTimeframe::Monthly);

    // RSI + RSI-MA (topping side).
    let rsi_state = rsi_ma::compute_rsi_ma_default(&tf_bars.close);
    let rsi = rsi_state.as_ref().and_then(rsi_ma::current_rsi);
    let rsi_ma_v = rsi_state.as_ref().and_then(rsi_ma::current_rsi_ma);
    let rsi_pair = rsi_state.as_ref().and_then(|s| last_two_opt(&s.rsi));
    let rsi_ma_pair = rsi_state.as_ref().and_then(|s| last_two_opt(&s.rsi_ma));
    let rsi_ma_turned_down = rsi_state
        .as_ref()
        .and_then(rsi_ma::ma_turned_down)
        .unwrap_or(false);
    let rsi_ma_cross_below_rsi = rsi_state
        .as_ref()
        .and_then(rsi_ma::ma_crossed_below_rsi)
        .unwrap_or(false);

    // DSS Bressert (topping side).
    let dss_state = dss_bressert::compute_dss_default(&tf_bars.close, &tf_bars.high, &tf_bars.low);
    let dss = dss_state.as_ref().and_then(dss_bressert::current_dss);
    let dss_trigger = dss_state.as_ref().and_then(dss_bressert::current_trigger);
    let dss_pair = dss_state.as_ref().and_then(|s| last_two_opt(&s.dss));
    let dss_trigger_pair = dss_state.as_ref().and_then(|s| last_two_opt(&s.trigger));
    let dss_turned_down = dss_state
        .as_ref()
        .and_then(dss_bressert::turned_down)
        .unwrap_or(false);
    let dss_cross_below_trigger = dss_state
        .as_ref()
        .and_then(dss_bressert::crossed_below_trigger)
        .unwrap_or(false);
    let dss_overbought = dss_state
        .as_ref()
        .and_then(|s| dss_bressert::is_overbought(s, 80.0))
        .unwrap_or(false);

    // Ehlers roofing filter (topping side).
    let erf_series = ehlers_roofing::compute_erf_default(&tf_bars.close);
    let erf = erf_series.as_ref().and_then(|s| ehlers_roofing::current(s));
    let erf_pair = erf_series.as_ref().and_then(|s| last_two_f64(s));
    let erf_negative = erf.map(|v| v < 0.0).unwrap_or(false);
    let erf_top_zone = erf.map(|v| v > 0.0).unwrap_or(false);
    let erf_turned_down = erf_series
        .as_ref()
        .and_then(|s| ehlers_roofing::turned_down(s))
        .unwrap_or(false);

    // --- CyberBands on DAILY (bearish state) ---
    let bands = cyber::bands::compute_gaussian_bands(&daily.close, &daily.dates, 5);
    let cyberbands_state = bands.as_ref().map(|b| b.qb.label().to_string());
    let cyberbands_bearish = bands
        .as_ref()
        .map(|b| b.qb == QbState::Bearish)
        .unwrap_or(false);

    // --- CyberDots on WEEKLY + MONTHLY (net-bearish) ---
    let dots_weekly =
        cyber::dots::compute_dots(&weekly.close, &weekly.high, &weekly.low, &weekly.dates, 5);
    let dots_monthly = cyber::dots::compute_dots(
        &monthly.close,
        &monthly.high,
        &monthly.low,
        &monthly.dates,
        5,
    );
    // Net-bearish when a down-dot is active and stronger than any up-dot, on
    // either higher timeframe.
    let dot_bearish = |d: &cyber::dots::DotsRead| d.down_dot && d.down_strength >= d.up_strength;
    let cyberdots_weekly_down_strength = dots_weekly.as_ref().map(|d| d.down_strength);
    let cyberdots_monthly_down_strength = dots_monthly.as_ref().map(|d| d.down_strength);
    let cyberdots_bearish = dots_weekly.as_ref().map(dot_bearish).unwrap_or(false)
        || dots_monthly.as_ref().map(dot_bearish).unwrap_or(false);

    // --- CyberLine on WEEKLY (weekly line LOST = "bull basically over") ---
    let line = cyber::line::compute_line(&weekly.close, &weekly.high, &weekly.low, &weekly.dates);
    let cyberline_value = line.as_ref().map(|l| l.value);
    let cyberline_price_above = line.as_ref().map(|l| l.price_above);
    let line_crosses = cyber::line::compute_line_crosses(&weekly.close, &weekly.dates);
    let fresh_below_cross = line_crosses
        .last()
        .map(|c| c.direction == "below" && weekly.dates.last() == Some(&c.date))
        .unwrap_or(false);
    // Lost = price below the weekly line, OR a fresh bearish (below) cross on
    // the most recent weekly bar.
    let cyberline_lost =
        line.as_ref().map(|l| !l.price_above).unwrap_or(false) || fresh_below_cross;

    // --- Pi-cycle top on DAILY (bonus) ---
    let pi = cyber::pi_cycle::compute_pi_cycle(&daily.close, &daily.dates);
    let pi_cycle_last_top = pi.as_ref().and_then(|p| p.last_top.clone());
    let pi_cycle_top = pi
        .as_ref()
        .and_then(|p| p.last_top.as_ref())
        .map(|d| within_recent(&daily.dates, d, 120))
        .unwrap_or(false);

    // --- Collapse the 10 atomic sub-signals into 7 composite criteria ---
    let tf_label = timeframe.label();
    let mut criteria: Vec<Criterion> = Vec::new();

    // 1. Momentum line turning down.
    criteria.push(Criterion {
        key: "momentum_turning_down".into(),
        label: "Momentum line turning down".into(),
        met: rsi_ma_turned_down,
        detail: format!("{tf_label} RSI {} · RSI-avg {}", fmt(rsi), fmt(rsi_ma_v)),
        components: vec![Component {
            key: "rsi_ma_turned_down".into(),
            label: "RSI average ticked down".into(),
            met: rsi_ma_turned_down,
            value: rsi_ma_v,
            previous_value: rsi_ma_pair.map(|(prev, _)| prev),
            comparison_value: None,
            previous_comparison_value: None,
            distance_to_trigger: rsi_ma_pair.map(|(prev, cur)| prev - cur),
        }],
    });

    // 2. Momentum line crossed below price momentum.
    criteria.push(Criterion {
        key: "momentum_below_price".into(),
        label: "Momentum line crossed below price momentum".into(),
        met: rsi_ma_cross_below_rsi,
        detail: format!("{tf_label} RSI-avg {} vs RSI {}", fmt(rsi_ma_v), fmt(rsi)),
        components: vec![Component {
            key: "rsi_ma_cross_below_rsi".into(),
            label: "RSI average lost the RSI".into(),
            met: rsi_ma_cross_below_rsi,
            value: rsi_ma_v,
            previous_value: rsi_ma_pair.map(|(prev, _)| prev),
            comparison_value: rsi,
            previous_comparison_value: rsi_pair.map(|(prev, _)| prev),
            distance_to_trigger: rsi_ma_v.zip(rsi).map(|(ma, r)| r - ma),
        }],
    });

    // 3. Double-smoothed stochastic topping = turned down AND crossed trigger.
    //    (Overbought is a qualifying CONTEXT flag, not a firing condition.)
    let dss_topping = dss_turned_down && dss_cross_below_trigger;
    criteria.push(Criterion {
        key: "dss_topping".into(),
        label: "Double-smoothed stochastic topping".into(),
        met: dss_topping,
        detail: format!(
            "{tf_label} value {} vs trigger {}{}",
            fmt(dss),
            fmt(dss_trigger),
            if dss_overbought { " (overbought)" } else { "" }
        ),
        components: vec![
            Component {
                key: "dss_turned_down".into(),
                label: "DSS ticked down".into(),
                met: dss_turned_down,
                value: dss,
                previous_value: dss_pair.map(|(prev, _)| prev),
                comparison_value: None,
                previous_comparison_value: None,
                distance_to_trigger: dss_pair.map(|(prev, cur)| prev - cur),
            },
            Component {
                key: "dss_cross_below_trigger".into(),
                label: "DSS crossed below trigger".into(),
                met: dss_cross_below_trigger,
                value: dss,
                previous_value: dss_pair.map(|(prev, _)| prev),
                comparison_value: dss_trigger,
                previous_comparison_value: dss_trigger_pair.map(|(prev, _)| prev),
                distance_to_trigger: dss.zip(dss_trigger).map(|(d, t)| t - d),
            },
            Component {
                key: "dss_overbought".into(),
                label: "DSS overbought (>80) — context".into(),
                met: dss_overbought,
                value: dss,
                previous_value: dss_pair.map(|(prev, _)| prev),
                comparison_value: Some(80.0),
                previous_comparison_value: Some(80.0),
                distance_to_trigger: dss.map(|d| d - 80.0),
            },
        ],
    });

    // 4. Roofing filter confirming down = top-zone (positive) AND turned down.
    let erf_confirming = erf_top_zone && erf_turned_down;
    criteria.push(Criterion {
        key: "roofing_confirming_down".into(),
        label: "Roofing filter confirming down".into(),
        met: erf_confirming,
        detail: format!("{tf_label} value {}", fmt(erf)),
        components: vec![
            Component {
                key: "erf_top_zone".into(),
                label: "Roofing filter in top zone (>0)".into(),
                met: erf_top_zone,
                value: erf,
                previous_value: erf_pair.map(|(prev, _)| prev),
                comparison_value: Some(0.0),
                previous_comparison_value: Some(0.0),
                distance_to_trigger: erf,
            },
            Component {
                key: "erf_turned_down".into(),
                label: "Roofing filter ticked down".into(),
                met: erf_turned_down,
                value: erf,
                previous_value: erf_pair.map(|(prev, _)| prev),
                comparison_value: None,
                previous_comparison_value: None,
                distance_to_trigger: erf_pair.map(|(prev, cur)| prev - cur),
            },
        ],
    });

    // 5. Volatility bands bearish (daily).
    criteria.push(Criterion {
        key: "volatility_bands_bearish".into(),
        label: "Volatility bands bearish (daily)".into(),
        met: cyberbands_bearish,
        detail: format!(
            "daily band state: {}",
            cyberbands_state.as_deref().unwrap_or("n/a")
        ),
        components: vec![Component {
            key: "cyberbands_bearish".into(),
            label: "Daily momentum bands in bearish state".into(),
            met: cyberbands_bearish,
            value: None,
            previous_value: None,
            comparison_value: None,
            previous_comparison_value: None,
            distance_to_trigger: None,
        }],
    });

    // 6. Significant reversal dots (weekly/monthly).
    criteria.push(Criterion {
        key: "reversal_dots_bearish".into(),
        label: "Significant reversal dots bearish (weekly/monthly)".into(),
        met: cyberdots_bearish,
        detail: format!(
            "weekly down {} · monthly down {}",
            cyberdots_weekly_down_strength
                .map(|s| s.to_string())
                .unwrap_or_else(|| "n/a".into()),
            cyberdots_monthly_down_strength
                .map(|s| s.to_string())
                .unwrap_or_else(|| "n/a".into())
        ),
        components: vec![Component {
            key: "cyberdots_bearish".into(),
            label: "Higher-timeframe strength dots net-bearish".into(),
            met: cyberdots_bearish,
            value: cyberdots_weekly_down_strength
                .or(cyberdots_monthly_down_strength)
                .map(|s| s as f64),
            previous_value: None,
            comparison_value: None,
            previous_comparison_value: None,
            distance_to_trigger: None,
        }],
    });

    // 7. Trend line lost (weekly).
    criteria.push(Criterion {
        key: "trend_line_lost".into(),
        label: "Trend line lost (weekly)".into(),
        met: cyberline_lost,
        detail: format!(
            "weekly line {} · price {} ({})",
            fmt(cyberline_value),
            fmt(Some(last_close)),
            match cyberline_price_above {
                Some(true) => "above",
                Some(false) => "below",
                None => "n/a",
            }
        ),
        components: vec![Component {
            key: "cyberline_lost".into(),
            label: "Price lost the weekly trackline".into(),
            met: cyberline_lost,
            value: cyberline_value,
            previous_value: None,
            comparison_value: Some(last_close),
            previous_comparison_value: None,
            distance_to_trigger: cyberline_value.map(|line| line - last_close),
        }],
    });

    let core_total = 7usize;
    debug_assert_eq!(criteria.len(), core_total, "must emit exactly 7 composites");
    let core_watch = build_top_core_watch(&criteria);
    let met_count = criteria.iter().filter(|c| c.met).count();

    // Bonus pi-cycle top (daily) — reported separately, NEVER counted in the 7.
    let bonus = pi.as_ref().map(|_| BonusSignal {
        key: "pi_cycle_top".into(),
        label: "Pi-cycle top fired recently (bonus)".into(),
        met: pi_cycle_top,
        detail: match &pi_cycle_last_top {
            Some(d) => format!("last top {d}"),
            None => "no top in window".into(),
        },
        last_bottom: pi_cycle_last_top.clone(),
    });

    let verdict = build_top_verdict(timeframe, met_count, core_total, &criteria);

    Some(CycleTopSignals {
        symbol: symbol.to_string(),
        timeframe,
        as_of,
        rsi,
        rsi_ma: rsi_ma_v,
        rsi_ma_turned_down,
        rsi_ma_cross_below_rsi,
        dss,
        dss_trigger,
        dss_turned_down,
        dss_cross_below_trigger,
        dss_overbought,
        erf,
        erf_negative,
        erf_top_zone,
        erf_turned_down,
        cyberbands_state,
        cyberbands_bearish,
        cyberdots_weekly_down_strength,
        cyberdots_monthly_down_strength,
        cyberdots_bearish,
        cyberline_value,
        cyberline_price_above,
        cyberline_lost,
        pi_cycle_top,
        pi_cycle_last_top,
        criteria,
        core_watch,
        met_count,
        total: core_total,
        bonus,
        verdict,
    })
}

/// True when `target` date is within the last `window` entries of `dates`.
fn within_recent(dates: &[String], target: &str, window: usize) -> bool {
    let start = dates.len().saturating_sub(window);
    dates[start..].iter().any(|d| d == target)
}

fn last_two_opt(series: &[Option<f64>]) -> Option<(f64, f64)> {
    let n = series.len();
    if n < 2 {
        return None;
    }
    Some((series[n - 2]?, series[n - 1]?))
}

fn last_two_f64(series: &[f64]) -> Option<(f64, f64)> {
    let n = series.len();
    if n < 2 {
        return None;
    }
    Some((series[n - 2], series[n - 1]))
}

fn build_core_watch(criteria: &[Criterion]) -> Vec<WatchItem> {
    const CORE_KEYS: [&str; 4] = [
        "momentum_turning_up",
        "momentum_above_price",
        "dss_bottoming",
        "roofing_confirming_up",
    ];
    criteria
        .iter()
        .filter(|c| CORE_KEYS.contains(&c.key.as_str()))
        .map(|c| {
            let required_components: Vec<Component> = c
                .components
                .iter()
                .filter(|component| component.key != "dss_oversold")
                .cloned()
                .collect();
            let met_components = required_components
                .iter()
                .filter(|component| component.met)
                .count();
            WatchItem {
                key: c.key.clone(),
                label: c.label.clone(),
                met: c.met,
                met_components,
                total_components: required_components.len(),
                detail: c.detail.clone(),
                components: required_components,
            }
        })
        .collect()
}

fn build_top_core_watch(criteria: &[Criterion]) -> Vec<WatchItem> {
    const CORE_KEYS: [&str; 4] = [
        "momentum_turning_down",
        "momentum_below_price",
        "dss_topping",
        "roofing_confirming_down",
    ];
    criteria
        .iter()
        .filter(|c| CORE_KEYS.contains(&c.key.as_str()))
        .map(|c| {
            let required_components: Vec<Component> = c
                .components
                .iter()
                .filter(|component| component.key != "dss_overbought")
                .cloned()
                .collect();
            let met_components = required_components
                .iter()
                .filter(|component| component.met)
                .count();
            WatchItem {
                key: c.key.clone(),
                label: c.label.clone(),
                met: c.met,
                met_components,
                total_components: required_components.len(),
                detail: c.detail.clone(),
                components: required_components,
            }
        })
        .collect()
}

fn fmt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}")).unwrap_or_else(|| "—".into())
}

fn build_verdict(tf: SignalTimeframe, met: usize, total: usize, criteria: &[Criterion]) -> String {
    let firing: Vec<&str> = criteria
        .iter()
        .take(total) // core only
        .filter(|c| c.met)
        .map(|c| c.label.as_str())
        .collect();
    // Verdict bands on the 0..7 composite scale.
    let strength = if met == 0 {
        "no cycle-bottom criteria firing"
    } else if met <= 2 {
        "early / weak cycle-bottom confluence"
    } else if met <= 4 {
        "building cycle-bottom confluence"
    } else if met <= 6 {
        "strong cycle-bottom confluence"
    } else {
        "very strong cycle-bottom confluence (all 7)"
    };
    if firing.is_empty() {
        format!("{} suite: {met}/{total} — {strength}", tf.label())
    } else {
        format!(
            "{} suite: {met}/{total} — {strength} ({})",
            tf.label(),
            firing.join("; ")
        )
    }
}

fn build_top_verdict(
    tf: SignalTimeframe,
    met: usize,
    total: usize,
    criteria: &[Criterion],
) -> String {
    let firing: Vec<&str> = criteria
        .iter()
        .take(total)
        .filter(|c| c.met)
        .map(|c| c.label.as_str())
        .collect();
    let strength = if met == 0 {
        "no cycle-top criteria firing"
    } else if met <= 2 {
        "early / weak cycle-top confluence"
    } else if met <= 4 {
        "building cycle-top confluence"
    } else if met <= 6 {
        "strong cycle-top confluence"
    } else {
        "very strong cycle-top confluence (all 7)"
    };
    if firing.is_empty() {
        format!("{} suite: {met}/{total} — {strength}", tf.label())
    } else {
        format!(
            "{} suite: {met}/{total} — {strength} ({})",
            tf.label(),
            firing.join("; ")
        )
    }
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
            open: None,
            high: None,
            low: None,
        }
    }

    /// Build a deep daily history: a long multi-year decline into a sharp
    /// V-bottom and recovery — the regime where cycle-bottom criteria fire.
    fn v_bottom_history(n_decline: usize, n_rally: usize) -> Vec<HistoryRecord> {
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let mut out = Vec::new();
        let mut day = 0u64;
        // Decline with mild noise so RSI/DSS get oversold.
        let mut price = 1000.0;
        for i in 0..n_decline {
            price = 1000.0 - i as f64 * (700.0 / n_decline as f64);
            let noise = 8.0 * (i as f64 / 11.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, (price + noise).max(50.0)));
            day += 1;
        }
        // Sharp recovery rally.
        let base = price;
        for j in 1..=n_rally {
            let p = base + j as f64 * (600.0 / n_rally as f64);
            let noise = 6.0 * (j as f64 / 9.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, p + noise));
            day += 1;
        }
        out
    }

    #[test]
    fn shallow_history_returns_none() {
        let h = v_bottom_history(40, 10);
        assert!(cycle_bottom_signals("TEST", &h, SignalTimeframe::Monthly).is_none());
    }

    #[test]
    fn v_bottom_fires_multiple_criteria() {
        // ~3 years of daily decline then a strong multi-month rally.
        let h = v_bottom_history(750, 250);
        let sig = cycle_bottom_signals("TEST", &h, SignalTimeframe::Monthly)
            .expect("signals on deep history");
        assert_eq!(sig.timeframe, SignalTimeframe::Monthly);
        assert_eq!(sig.total, 7, "7 composite criteria");
        assert_eq!(sig.criteria.len(), 7, "exactly 7 criteria rows");
        assert_eq!(sig.criteria.iter().filter(|c| c.met).count(), sig.met_count);
        // Every composite must expose its atomic components.
        assert!(sig.criteria.iter().all(|c| !c.components.is_empty()));
        // After a strong recovery off a multi-year low, momentum criteria must
        // be firing — assert at least a third of the seven light up.
        assert!(
            sig.met_count >= 3,
            "expected ≥3/7 at a clean V-bottom recovery, got {}: {:?}",
            sig.met_count,
            sig.criteria
                .iter()
                .filter(|c| c.met)
                .map(|c| c.key.clone())
                .collect::<Vec<_>>()
        );
        // RSI average must have turned up coming off the low.
        assert!(
            sig.rsi_ma_turned_up,
            "RSI average should be rising in recovery"
        );
        assert_eq!(sig.core_watch.len(), 4, "four focused watch items");
        assert!(sig
            .core_watch
            .iter()
            .all(|item| item.met_components <= item.total_components));
        let ma_component = sig
            .criteria
            .iter()
            .flat_map(|c| c.components.iter())
            .find(|c| c.key == "rsi_ma_turned_up")
            .expect("rsi ma component");
        assert!(
            ma_component.previous_value.is_some(),
            "edge components expose previous-bar values"
        );
        assert!(
            ma_component.distance_to_trigger.is_some(),
            "edge components expose signed trigger distance"
        );
        let dss_cross = sig
            .criteria
            .iter()
            .flat_map(|c| c.components.iter())
            .find(|c| c.key == "dss_cross_above_trigger")
            .expect("dss cross component");
        assert!(
            dss_cross.comparison_value.is_some() && dss_cross.previous_comparison_value.is_some(),
            "cross components expose current and previous comparison values"
        );
        // ERF should have a usable bottom-zone or rising read after the rally.
        assert!(
            sig.erf_bottom_zone || sig.erf_positive || sig.erf_turned_up,
            "roofing filter should be constructive"
        );
        assert!(sig.verdict.contains("monthly suite"));
    }

    #[test]
    fn determinism_identical_output() {
        let h = v_bottom_history(600, 200);
        let a = cycle_bottom_signals("TEST", &h, SignalTimeframe::Monthly).expect("a");
        let b = cycle_bottom_signals("TEST", &h, SignalTimeframe::Monthly).expect("b");
        let ja = serde_json::to_string(&a).unwrap();
        let jb = serde_json::to_string(&b).unwrap();
        assert_eq!(ja, jb, "engine must be deterministic");
    }

    #[test]
    fn timeframe_parse() {
        assert_eq!(
            SignalTimeframe::parse("monthly").unwrap(),
            SignalTimeframe::Monthly
        );
        assert_eq!(
            SignalTimeframe::parse("1w").unwrap(),
            SignalTimeframe::Weekly
        );
        assert_eq!(SignalTimeframe::parse("d").unwrap(), SignalTimeframe::Daily);
        assert!(SignalTimeframe::parse("yearly").is_err());
    }

    #[test]
    fn downtrend_few_criteria() {
        // Pure strictly-monotonic decline ending at the low — the final close
        // is the lowest bar, so price is below every lagging line and the DSS
        // is pinned oversold. Bottom-confirmation criteria (turn-ups, reclaim)
        // must stay cold.
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h: Vec<HistoryRecord> = (0..900)
            .map(|i| {
                let date = (start + chrono::Days::new(i as u64))
                    .format("%Y-%m-%d")
                    .to_string();
                record(&date, 2000.0 - i as f64 * 2.0)
            })
            .collect();
        let sig = cycle_bottom_signals("TEST", &h, SignalTimeframe::Monthly).expect("sig");
        assert!(
            !sig.cyberline_reclaim,
            "price should not reclaim the weekly line mid-crash"
        );
        assert!(
            !sig.cyberbands_bullish,
            "bands should not be bullish mid-crash"
        );
        assert!(
            !sig.cyberdots_bullish,
            "strength dots should not be net-bullish mid-crash"
        );
        assert!(
            sig.met_count <= 3,
            "a crash should not light up most criteria"
        );
    }

    // ---- Cycle-TOP suite (symmetric mirror) ------------------------------

    /// Build a deep daily history: a long multi-year ADVANCE into a sharp
    /// inverted-V top and selloff — the regime where cycle-top criteria fire.
    fn inverted_v_history(n_rally: usize, n_decline: usize) -> Vec<HistoryRecord> {
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let mut out = Vec::new();
        let mut day = 0u64;
        let mut price = 300.0;
        // Advance with mild noise so RSI/DSS get overbought.
        for i in 0..n_rally {
            price = 300.0 + i as f64 * (700.0 / n_rally as f64);
            let noise = 8.0 * (i as f64 / 11.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, (price + noise).max(50.0)));
            day += 1;
        }
        // Sharp selloff.
        let base = price;
        for j in 1..=n_decline {
            let p = (base - j as f64 * (600.0 / n_decline as f64)).max(50.0);
            let noise = 6.0 * (j as f64 / 9.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, (p + noise).max(50.0)));
            day += 1;
        }
        out
    }

    #[test]
    fn top_shallow_history_returns_none() {
        let h = inverted_v_history(40, 10);
        assert!(cycle_top_signals("TEST", &h, SignalTimeframe::Monthly).is_none());
    }

    #[test]
    fn inverted_v_fires_multiple_top_criteria() {
        let h = inverted_v_history(750, 250);
        let sig = cycle_top_signals("TEST", &h, SignalTimeframe::Monthly)
            .expect("signals on deep history");
        assert_eq!(sig.timeframe, SignalTimeframe::Monthly);
        assert_eq!(sig.total, 7, "7 composite criteria");
        assert_eq!(sig.criteria.len(), 7, "exactly 7 criteria rows");
        assert_eq!(sig.criteria.iter().filter(|c| c.met).count(), sig.met_count);
        assert!(sig.criteria.iter().all(|c| !c.components.is_empty()));
        assert!(
            sig.met_count >= 3,
            "expected ≥3/7 at a clean inverted-V top, got {}: {:?}",
            sig.met_count,
            sig.criteria
                .iter()
                .filter(|c| c.met)
                .map(|c| c.key.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            sig.rsi_ma_turned_down,
            "RSI average should be falling into the selloff"
        );
        assert_eq!(sig.core_watch.len(), 4, "four focused watch items");
        assert!(sig
            .core_watch
            .iter()
            .all(|item| item.met_components <= item.total_components));
        assert!(sig.verdict.contains("monthly suite"));
        // Top criterion keys present (symmetric with bottom shape).
        let keys: Vec<&str> = sig.criteria.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"momentum_turning_down"));
        assert!(keys.contains(&"dss_topping"));
        assert!(keys.contains(&"trend_line_lost"));
    }

    #[test]
    fn top_determinism_identical_output() {
        let h = inverted_v_history(600, 200);
        let a = cycle_top_signals("TEST", &h, SignalTimeframe::Monthly).expect("a");
        let b = cycle_top_signals("TEST", &h, SignalTimeframe::Monthly).expect("b");
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "engine must be deterministic"
        );
    }

    #[test]
    fn pure_uptrend_few_top_criteria() {
        // Strictly-monotonic advance ending at the high — the final close is
        // the highest bar, price is above every lagging line, DSS pinned
        // overbought. Top-confirmation criteria (turn-downs, line lost) stay cold.
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h: Vec<HistoryRecord> = (0..900)
            .map(|i| {
                let date = (start + chrono::Days::new(i as u64))
                    .format("%Y-%m-%d")
                    .to_string();
                record(&date, 200.0 + i as f64 * 2.0)
            })
            .collect();
        let sig = cycle_top_signals("TEST", &h, SignalTimeframe::Monthly).expect("sig");
        assert!(
            !sig.cyberline_lost,
            "price should not lose the weekly line mid-rally"
        );
        assert!(
            !sig.cyberbands_bearish,
            "bands should not be bearish mid-rally"
        );
        assert!(
            !sig.cyberdots_bearish,
            "strength dots should not be net-bearish mid-rally"
        );
        assert!(
            sig.met_count <= 3,
            "a clean uptrend should not light up most top criteria, got {}",
            sig.met_count
        );
    }
}
