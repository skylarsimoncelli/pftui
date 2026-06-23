//! Mechanical cycle-bottom signal suite — a deterministic N-of-7 confluence of
//! independent cycle-low confirmations.
//!
//! Each criterion is a faithful Pine-ported indicator evaluated at its NATURAL
//! timeframe and reduced to a boolean "is this firing now?":
//!
//! | key | source | natural TF | what it measures |
//! |---|---|---|---|
//! | `rsi_ma_turned_up`        | [`indicators::rsi_ma`]        | requested (monthly) | the RSI's own average ticked up |
//! | `rsi_ma_cross_above_rsi`  | [`indicators::rsi_ma`]        | requested | the RSI average reclaimed the RSI |
//! | `dss_turned_up`           | [`indicators::dss_bressert`]  | requested | double-smoothed stochastic ticked up |
//! | `dss_cross_above_trigger` | [`indicators::dss_bressert`]  | requested | DSS crossed its trigger |
//! | `dss_oversold`            | [`indicators::dss_bressert`]  | requested | DSS below 20 |
//! | `erf_green`               | [`indicators::ehlers_roofing`]| requested | roofing filter ≥ 0 |
//! | `erf_turned_up`           | [`indicators::ehlers_roofing`]| requested | roofing filter ticked up |
//! | `cyberbands_bullish`      | `cyber::bands`                | DAILY | momentum bands in the bullish state |
//! | `cyberdots_bullish`       | `cyber::dots`                 | weekly + monthly | strength dots net-bullish |
//! | `cyberline_reclaim`       | `cyber::line`                 | WEEKLY | price reclaimed the trackline |
//! | `pi_cycle_bottom` (bonus) | `cyber::pi_cycle`             | daily | Pi-Cycle bottom fired recently |
//!
//! Confluence = `met_count / total`. The suite is position-only / measurement —
//! it never emits a price target. All math is `f64`; no money flows through.

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
            other => anyhow::bail!(
                "unknown timeframe '{other}' — expected daily, weekly, or monthly"
            ),
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

/// One evaluated criterion row for display/itemization.
#[derive(Debug, Clone, Serialize)]
pub struct Criterion {
    /// Stable machine key (e.g. `rsi_ma_turned_up`).
    pub key: String,
    /// Human label (no practitioner names).
    pub label: String,
    /// Whether this criterion is firing on the latest bar.
    pub met: bool,
    /// One-line plain-language detail with the backing value(s).
    pub detail: String,
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
    pub erf_green: bool,
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

    /// Ordered criteria list (the 7 core + bonus pi-cycle when computable).
    pub criteria: Vec<Criterion>,
    /// How many of `total` core criteria are firing.
    pub met_count: usize,
    /// Total core criteria (7).
    pub total: usize,
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
    let erf_green = erf_series
        .as_ref()
        .and_then(|s| ehlers_roofing::is_green(s))
        .unwrap_or(false);
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

    // --- Assemble the ordered criteria list (7 core) ---
    let tf_label = timeframe.label();
    let mut criteria: Vec<Criterion> = Vec::new();
    criteria.push(Criterion {
        key: "rsi_ma_turned_up".into(),
        label: "RSI average turned up".into(),
        met: rsi_ma_turned_up,
        detail: format!(
            "{tf_label} RSI {} · RSI-avg {}",
            fmt(rsi),
            fmt(rsi_ma_v)
        ),
    });
    criteria.push(Criterion {
        key: "rsi_ma_cross_above_rsi".into(),
        label: "RSI average reclaimed the RSI".into(),
        met: rsi_ma_cross_above_rsi,
        detail: format!("{tf_label} RSI-avg {} vs RSI {}", fmt(rsi_ma_v), fmt(rsi)),
    });
    criteria.push(Criterion {
        key: "dss_turned_up".into(),
        label: "Double-smoothed stochastic turned up".into(),
        met: dss_turned_up,
        detail: format!("{tf_label} value {} (trigger {})", fmt(dss), fmt(dss_trigger)),
    });
    criteria.push(Criterion {
        key: "dss_cross_above_trigger".into(),
        label: "Double-smoothed stochastic crossed its trigger".into(),
        met: dss_cross_above_trigger,
        detail: format!(
            "{tf_label} value {} crossed trigger {}{}",
            fmt(dss),
            fmt(dss_trigger),
            if dss_oversold { " from oversold" } else { "" }
        ),
    });
    criteria.push(Criterion {
        key: "dss_oversold".into(),
        label: "Double-smoothed stochastic oversold (<20)".into(),
        met: dss_oversold,
        detail: format!("{tf_label} value {}", fmt(dss)),
    });
    criteria.push(Criterion {
        key: "erf_green".into(),
        label: "Roofing filter positive (green)".into(),
        met: erf_green,
        detail: format!("{tf_label} value {}", fmt(erf)),
    });
    criteria.push(Criterion {
        key: "erf_turned_up".into(),
        label: "Roofing filter turned up".into(),
        met: erf_turned_up,
        detail: format!("{tf_label} value {}", fmt(erf)),
    });
    // Cyber criteria (count toward the suite at their natural TFs).
    criteria.push(Criterion {
        key: "cyberbands_bullish".into(),
        label: "Daily momentum bands turned bullish".into(),
        met: cyberbands_bullish,
        detail: format!(
            "daily band state: {}",
            cyberbands_state.as_deref().unwrap_or("n/a")
        ),
    });
    criteria.push(Criterion {
        key: "cyberdots_bullish".into(),
        label: "Higher-timeframe strength dots net-bullish".into(),
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
    });
    criteria.push(Criterion {
        key: "cyberline_reclaim".into(),
        label: "Price reclaimed the weekly trackline".into(),
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
    });

    // Bonus pi-cycle row, appended after the core 10 (only counts as bonus).
    let core_total = criteria.len();
    let met_count = criteria.iter().filter(|c| c.met).count();
    if pi.is_some() {
        criteria.push(Criterion {
            key: "pi_cycle_bottom".into(),
            label: "Pi-cycle bottom fired recently (bonus)".into(),
            met: pi_cycle_bottom,
            detail: match &pi_cycle_last_bottom {
                Some(d) => format!("last bottom {d}"),
                None => "no bottom in window".into(),
            },
        });
    }

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
        erf_green,
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
        met_count,
        total: core_total,
        verdict,
    })
}

/// True when `target` date is within the last `window` entries of `dates`.
fn within_recent(dates: &[String], target: &str, window: usize) -> bool {
    let start = dates.len().saturating_sub(window);
    dates[start..].iter().any(|d| d == target)
}

fn fmt(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}")).unwrap_or_else(|| "—".into())
}

fn build_verdict(
    tf: SignalTimeframe,
    met: usize,
    total: usize,
    criteria: &[Criterion],
) -> String {
    let firing: Vec<&str> = criteria
        .iter()
        .take(total) // core only
        .filter(|c| c.met)
        .map(|c| c.label.as_str())
        .collect();
    let strength = if met == 0 {
        "no cycle-bottom criteria firing"
    } else if met <= 2 {
        "early / weak cycle-bottom confluence"
    } else if met <= 4 {
        "building cycle-bottom confluence"
    } else if met <= 6 {
        "strong cycle-bottom confluence"
    } else {
        "very strong cycle-bottom confluence"
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
            let date = (start + chrono::Days::new(day)).format("%Y-%m-%d").to_string();
            out.push(record(&date, (price + noise).max(50.0)));
            day += 1;
        }
        // Sharp recovery rally.
        let base = price;
        for j in 1..=n_rally {
            let p = base + j as f64 * (600.0 / n_rally as f64);
            let noise = 6.0 * (j as f64 / 9.0).sin();
            let date = (start + chrono::Days::new(day)).format("%Y-%m-%d").to_string();
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
        assert_eq!(sig.total, 10, "10 core criteria");
        assert_eq!(sig.criteria.iter().take(sig.total).filter(|c| c.met).count(), sig.met_count);
        // After a strong recovery off a multi-year low, momentum criteria must
        // be firing — assert at least several of the ten light up.
        assert!(
            sig.met_count >= 4,
            "expected ≥4/10 at a clean V-bottom recovery, got {}: {:?}",
            sig.met_count,
            sig.criteria
                .iter()
                .filter(|c| c.met)
                .map(|c| c.key.clone())
                .collect::<Vec<_>>()
        );
        // RSI average must have turned up coming off the low.
        assert!(sig.rsi_ma_turned_up, "RSI average should be rising in recovery");
        // ERF should be green/rising after the rally.
        assert!(sig.erf_green || sig.erf_turned_up, "roofing filter should be constructive");
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
        assert_eq!(SignalTimeframe::parse("monthly").unwrap(), SignalTimeframe::Monthly);
        assert_eq!(SignalTimeframe::parse("1w").unwrap(), SignalTimeframe::Weekly);
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
        assert!(!sig.cyberbands_bullish, "bands should not be bullish mid-crash");
        assert!(
            !sig.cyberdots_bullish,
            "strength dots should not be net-bullish mid-crash"
        );
        assert!(sig.met_count <= 5, "a crash should not light up most criteria");
    }
}
