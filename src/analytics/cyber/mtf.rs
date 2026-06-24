//! Multi-timeframe RSI zones + extreme candles — component F of the port.
//!
//! Pine mapping (`getDynamicTimeframes`, `redZone`/`greenZone`,
//! `rsi_UpSignal`/`rsi_DownSignal`, `isRSIExtremeHigh/Low` blocks):
//!
//! The Pine ladder at a 1D chart is `[240min, 1W, 1M, 3M]` with skip rules
//! `currentTFMin < 60 ? tf3-check : true` and `currentTFMin < 240 ?
//! tf4-check : true` — at daily and above, the tf3/tf4 checks auto-pass.
//!
//! **Documented adaptation:** 240-minute bars do not exist on daily data.
//! For daily runs the ladder is `[daily(=current), weekly, monthly]` — the
//! 240min slot degrades to the current (daily) RSI, making it a duplicate of
//! the chart-RSI check, and the monthly slot occupies the auto-passing tf3
//! position (computed for display, never gating). Effective gate at daily:
//! `RSI6(daily) AND RSI6(weekly)`. For weekly runs the Pine ladder is
//! `[1D, 1M, 3M, 12M]` with both skips active ⇒ effective gate:
//! `RSI6(weekly) AND RSI6(daily) AND RSI6(monthly)`. Monthly runs use the
//! same available daily/weekly/monthly ladder, anchored on monthly as current.
//!
//! Higher-timeframe RSI mirrors `request.security` developing-bar semantics:
//! at daily bar `t`, the weekly RSI is computed over completed weekly closes
//! plus the in-progress week whose close is the current daily close.
//! Weekly/monthly bars aggregate daily history by ISO week / calendar month
//! (same bucketing as `analytics::market_structure::aggregate`).
//!
//! - `redZone`/`greenZone`: all gating RSI(6) > 72 / < 28.
//! - Breakout signal: `not zone and ta.barssince(zone) < 2` — fires on the
//!   first bar after the zone is exited (na — never in zone — compares
//!   false, so no signal without a prior zone).
//! - RSI-extreme candle flag: RSI(14) > 85 / < 15 across the same gating
//!   timeframes (Pine `isRSIExtremeHigh/Low` with identical skip rules).

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use super::primitives;
use super::CyberTimeframe;

const RSI_LEN: usize = 6;
const RSI_HIGH: f64 = 72.0;
const RSI_LOW: f64 = 28.0;
const EXTREME_LEN: usize = 14;
const EXTREME_HIGH: f64 = 85.0;
const EXTREME_LOW: f64 = 15.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LadderTf {
    Daily,
    Weekly,
    Monthly,
}

/// A dated zone-exit breakout signal.
#[derive(Debug, Clone, Serialize)]
pub struct MtfSignalEvent {
    pub date: String,
    /// "up" (green zone exited) or "down" (red zone exited).
    pub direction: String,
}

/// MTF RSI read for the latest bar.
#[derive(Debug, Clone, Serialize)]
pub struct MtfRsiRead {
    /// RSI(6) on the run timeframe (daily for daily runs, weekly for weekly).
    pub rsi6_current: Option<f64>,
    pub rsi6_daily: Option<f64>,
    pub rsi6_weekly: Option<f64>,
    /// Monthly RSI(6) — gating only on weekly runs (display otherwise).
    pub rsi6_monthly: Option<f64>,
    /// "red" | "green" | "neutral" on the latest bar.
    pub zone: String,
    /// Which timeframes gate the zone on this run.
    pub gating: Vec<String>,
    /// RSI(14) extreme-candle flag: "high" | "low" | "none".
    pub extreme: String,
    /// Recent zone-exit breakout signals, oldest first (capped).
    pub recent_signals: Vec<MtfSignalEvent>,
    /// Per-bar zone-exit series (internal — feeds the breakout component).
    #[serde(skip)]
    pub up_signal_series: Vec<bool>,
    #[serde(skip)]
    pub dn_signal_series: Vec<bool>,
    /// Per-bar zone-membership series (internal — the research signal
    /// registry derives zone ENTER transitions from these).
    #[serde(skip)]
    pub green_series: Vec<bool>,
    #[serde(skip)]
    pub red_series: Vec<bool>,
}

/// Bucket key for a date under a ladder timeframe.
fn bucket_key(date: NaiveDate, tf: LadderTf) -> (i32, u32) {
    match tf {
        LadderTf::Daily => (date.year(), date.ordinal()),
        LadderTf::Weekly => {
            let iso = date.iso_week();
            (iso.year(), iso.week())
        }
        LadderTf::Monthly => (date.year(), date.month()),
    }
}

/// Per-current-bar RSI of a ladder timeframe with developing-bar semantics.
///
/// `bar_dates` must be a subset of `daily_dates` (each current bar carries
/// the date of its last daily row — true for both daily and aggregated
/// weekly runs).
fn ladder_rsi_per_bar(
    daily_dates: &[NaiveDate],
    daily_closes: &[f64],
    bar_dates: &[NaiveDate],
    tf: LadderTf,
    len: usize,
) -> Vec<Option<f64>> {
    let n_daily = daily_dates.len();
    // Bucket ordinal per daily row + final close per bucket.
    let mut ord = Vec::with_capacity(n_daily);
    let mut bucket_closes: Vec<f64> = Vec::new();
    let mut current: Option<(i32, u32)> = None;
    for i in 0..n_daily {
        let key = bucket_key(daily_dates[i], tf);
        if current != Some(key) {
            current = Some(key);
            bucket_closes.push(daily_closes[i]);
        } else if let Some(last) = bucket_closes.last_mut() {
            *last = daily_closes[i];
        }
        ord.push(bucket_closes.len() - 1);
    }

    // Walking pointer: daily index of each bar date.
    let mut out = Vec::with_capacity(bar_dates.len());
    let mut di = 0usize;
    for bd in bar_dates {
        while di + 1 < n_daily && daily_dates[di + 1] <= *bd {
            di += 1;
        }
        // daily_dates[di] is the last daily row on/before bd.
        let o = ord[di];
        // Completed buckets before the developing one + the developing close
        // (= the daily close as of this bar).
        let mut series: Vec<f64> = bucket_closes[..o].to_vec();
        series.push(daily_closes[di]);
        out.push(primitives::rsi_last(&series, len));
    }
    out
}

/// Compute the MTF RSI zones + extreme flags over the full current-TF series.
pub fn compute_mtf(
    daily_dates_str: &[String],
    daily_closes: &[f64],
    bar_dates_str: &[String],
    timeframe: CyberTimeframe,
    max_events: usize,
) -> Option<MtfRsiRead> {
    let daily_dates: Vec<NaiveDate> = daily_dates_str
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();
    if daily_dates.len() != daily_dates_str.len() || daily_dates.is_empty() {
        return None;
    }
    let bar_dates: Vec<NaiveDate> = bar_dates_str
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();
    if bar_dates.len() != bar_dates_str.len() || bar_dates.is_empty() {
        return None;
    }
    let n = bar_dates.len();

    // Gating ladder per run timeframe (module docs).
    let (gating_tfs, gating_labels): (Vec<LadderTf>, Vec<String>) = match timeframe {
        CyberTimeframe::Daily => (
            // current(daily) + tf1(240m→daily, adaptation) + tf2(weekly);
            // the duplicate daily check is harmless and kept implicit.
            vec![LadderTf::Daily, LadderTf::Weekly],
            vec!["daily".into(), "weekly".into()],
        ),
        CyberTimeframe::Weekly => (
            vec![LadderTf::Weekly, LadderTf::Daily, LadderTf::Monthly],
            vec!["weekly".into(), "daily".into(), "monthly".into()],
        ),
        CyberTimeframe::Monthly => (
            vec![LadderTf::Monthly, LadderTf::Weekly, LadderTf::Daily],
            vec!["monthly".into(), "weekly".into(), "daily".into()],
        ),
    };

    // RSI(6) + RSI(14) per gating TF, plus display TFs.
    let rsi6_daily = ladder_rsi_per_bar(
        &daily_dates,
        daily_closes,
        &bar_dates,
        LadderTf::Daily,
        RSI_LEN,
    );
    let rsi6_weekly = ladder_rsi_per_bar(
        &daily_dates,
        daily_closes,
        &bar_dates,
        LadderTf::Weekly,
        RSI_LEN,
    );
    let rsi6_monthly = ladder_rsi_per_bar(
        &daily_dates,
        daily_closes,
        &bar_dates,
        LadderTf::Monthly,
        RSI_LEN,
    );
    let series6_for = |tf: LadderTf| -> &Vec<Option<f64>> {
        match tf {
            LadderTf::Daily => &rsi6_daily,
            LadderTf::Weekly => &rsi6_weekly,
            LadderTf::Monthly => &rsi6_monthly,
        }
    };

    let mut red = vec![false; n];
    let mut green = vec![false; n];
    for t in 0..n {
        let vals: Vec<Option<f64>> = gating_tfs.iter().map(|tf| series6_for(*tf)[t]).collect();
        // Pine na comparisons are false: any undefined RSI ⇒ no zone.
        red[t] = vals
            .iter()
            .all(|v| v.map(|x| x > RSI_HIGH).unwrap_or(false));
        green[t] = vals.iter().all(|v| v.map(|x| x < RSI_LOW).unwrap_or(false));
    }

    // barssince-based zone-exit signals.
    let mut up_signal_series = vec![false; n];
    let mut dn_signal_series = vec![false; n];
    let mut last_green: Option<usize> = None;
    let mut last_red: Option<usize> = None;
    let mut events: Vec<MtfSignalEvent> = Vec::new();
    for t in 0..n {
        if !green[t] {
            if let Some(g) = last_green {
                up_signal_series[t] = t - g < 2;
            }
        }
        if !red[t] {
            if let Some(r) = last_red {
                dn_signal_series[t] = t - r < 2;
            }
        }
        if up_signal_series[t] {
            events.push(MtfSignalEvent {
                date: bar_dates_str[t].clone(),
                direction: "up".to_string(),
            });
        }
        if dn_signal_series[t] {
            events.push(MtfSignalEvent {
                date: bar_dates_str[t].clone(),
                direction: "down".to_string(),
            });
        }
        if green[t] {
            last_green = Some(t);
        }
        if red[t] {
            last_red = Some(t);
        }
    }
    if events.len() > max_events {
        events.drain(..events.len() - max_events);
    }

    // RSI(14) extreme-candle flag on the latest bar only.
    let last = n - 1;
    let last_bar_slice = &bar_dates[last..=last];
    let extreme = {
        let mut hi = true;
        let mut lo = true;
        for tf in &gating_tfs {
            let v =
                ladder_rsi_per_bar(&daily_dates, daily_closes, last_bar_slice, *tf, EXTREME_LEN)[0];
            hi &= v.map(|x| x > EXTREME_HIGH).unwrap_or(false);
            lo &= v.map(|x| x < EXTREME_LOW).unwrap_or(false);
        }
        if hi {
            "high"
        } else if lo {
            "low"
        } else {
            "none"
        }
    };

    let zone = if red[last] {
        "red"
    } else if green[last] {
        "green"
    } else {
        "neutral"
    };
    let current = match timeframe {
        CyberTimeframe::Daily => rsi6_daily[last],
        CyberTimeframe::Weekly => rsi6_weekly[last],
        CyberTimeframe::Monthly => rsi6_monthly[last],
    };
    let round1 = |v: Option<f64>| v.map(|x| (x * 10.0).round() / 10.0);

    Some(MtfRsiRead {
        rsi6_current: round1(current),
        rsi6_daily: round1(rsi6_daily[last]),
        rsi6_weekly: round1(rsi6_weekly[last]),
        rsi6_monthly: round1(rsi6_monthly[last]),
        zone: zone.to_string(),
        gating: gating_labels,
        extreme: extreme.to_string(),
        recent_signals: events,
        up_signal_series,
        dn_signal_series,
        green_series: green,
        red_series: red,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Consecutive calendar dates starting 2024-01-01 (covers many ISO weeks).
    fn day_dates(n: usize) -> Vec<String> {
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap_or_default();
        (0..n)
            .map(|i| {
                (start + chrono::Days::new(i as u64))
                    .format("%Y-%m-%d")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn monotonic_ramp_hits_red_zone_on_daily_and_weekly() {
        // Straight-up tape: RSI(6) = 100 on daily AND weekly ⇒ red zone.
        let n = 120;
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let dates = day_dates(n);
        let read =
            compute_mtf(&dates, &closes, &dates, CyberTimeframe::Daily, 10).expect("mtf computes");
        assert_eq!(read.zone, "red");
        assert!(read.rsi6_daily.unwrap_or_default() > 99.0);
        assert!(read.rsi6_weekly.unwrap_or_default() > 99.0);
        assert_eq!(read.gating, vec!["daily".to_string(), "weekly".to_string()]);
        // Monotonic up at RSI14 too ⇒ extreme high candle.
        assert_eq!(read.extreme, "high");
    }

    #[test]
    fn zone_exit_fires_breakout_signal_within_two_bars() {
        // Crash tape (green zone) then two strong up bars: the first
        // non-green bar with barssince(green) == 1 fires the up signal.
        let mut closes: Vec<f64> = (0..80).map(|i| 500.0 - 4.0 * i as f64).collect();
        closes.push(400.0); // sharp recovery bar — RSI(6) jumps well off the lows
        closes.push(430.0);
        let n = closes.len();
        let dates = day_dates(n);
        let read =
            compute_mtf(&dates, &closes, &dates, CyberTimeframe::Daily, 10).expect("mtf computes");
        // The crash itself must register green-zone bars before the exit.
        assert!(
            read.recent_signals.iter().any(|s| s.direction == "up"),
            "signals: {:?} zone {}",
            read.recent_signals,
            read.zone
        );
        let last_up = read
            .recent_signals
            .iter()
            .rev()
            .find(|s| s.direction == "up")
            .map(|s| s.date.clone());
        assert!(last_up.is_some());
    }

    #[test]
    fn weekly_run_gates_on_three_timeframes() {
        let n = 400;
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let daily_dates = day_dates(n);
        // Weekly bar dates = last daily date of each ISO week.
        let mut weekly_dates: Vec<String> = Vec::new();
        let mut current_week: Option<(i32, u32)> = None;
        for d in &daily_dates {
            let nd = NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap_or_default();
            let key = (nd.iso_week().year(), nd.iso_week().week());
            if current_week == Some(key) {
                if let Some(lastd) = weekly_dates.last_mut() {
                    *lastd = d.clone();
                }
            } else {
                current_week = Some(key);
                weekly_dates.push(d.clone());
            }
        }
        let read = compute_mtf(
            &daily_dates,
            &closes,
            &weekly_dates,
            CyberTimeframe::Weekly,
            10,
        )
        .expect("mtf computes");
        assert_eq!(
            read.gating,
            vec![
                "weekly".to_string(),
                "daily".to_string(),
                "monthly".to_string()
            ]
        );
        // Monotonic ramp: all three gates read 100 ⇒ red.
        assert_eq!(read.zone, "red");
        assert!(read.rsi6_monthly.unwrap_or_default() > 99.0);
    }

    #[test]
    fn neutral_when_no_zone_and_no_signal_without_prior_zone() {
        // Gentle alternation: RSI mid-range, never in a zone — barssince is
        // na (never) so no exit signal can fire.
        let closes: Vec<f64> = (0..100)
            .map(|i| 100.0 + if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let dates = day_dates(100);
        let read =
            compute_mtf(&dates, &closes, &dates, CyberTimeframe::Daily, 10).expect("mtf computes");
        assert_eq!(read.zone, "neutral");
        assert!(read.recent_signals.is_empty());
        assert_eq!(read.extreme, "none");
    }
}
