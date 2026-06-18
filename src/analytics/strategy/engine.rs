//! Backtest engine — interpret a boolean condition series two ways:
//!
//! 1. **Trades** (`simulate_trades`): the rising edge of the entry condition
//!    (`false`/unknown → `true`) opens one position at that bar's close; the
//!    exit rule (hold N days, or an exit condition's first firing) closes it.
//!    One position at a time — no pyramiding, no overlap.
//! 2. **Segments** (`segment_stats`): treat the condition as a regime mask
//!    and compare forward daily returns earned while the mask is on vs off.
//!    This answers "asset X's behaviour in state S" (e.g. gold while a rate
//!    proxy is above its long MA = "hiking").
//!
//! Returns are `f64` percentages / growth multiples — statistics over price
//! ratios, not monetary balances (cf. `research::event_study`).

use chrono::NaiveDate;
use serde::Serialize;

/// How a position is closed.
pub enum ExitKind {
    /// Exit at the first bar on/after `entry_date + days`.
    HoldDays(i64),
    /// Exit at the first bar (after entry) where this condition fires.
    Condition(Vec<Option<bool>>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub entry_date: String,
    pub entry_price: f64,
    pub exit_date: String,
    pub exit_price: f64,
    pub return_pct: f64,
    pub bars_held: usize,
    pub days_held: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchStats {
    pub first_date: String,
    pub last_date: String,
    pub years: f64,
    pub total_return_pct: f64,
    pub cagr_pct: Option<f64>,
    pub max_drawdown_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeReport {
    pub n_trades: usize,
    pub n_open_skipped: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub win_rate_pct: Option<f64>,
    pub mean_return_pct: Option<f64>,
    pub median_return_pct: Option<f64>,
    pub best_return_pct: Option<f64>,
    pub worst_return_pct: Option<f64>,
    /// Compounded growth across closed trades, as a percentage.
    pub total_return_pct: f64,
    pub cagr_pct: Option<f64>,
    pub max_drawdown_pct: f64,
    pub time_in_market_pct: f64,
    pub avg_days_held: Option<f64>,
    pub benchmark_hold: BenchStats,
    pub trades: Vec<Trade>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentStats {
    pub label: String,
    /// Bars where the mask was on AND a forward 1-bar return was measurable.
    pub n_days: usize,
    pub share_of_days_pct: f64,
    /// Number of contiguous on-runs.
    pub episodes: usize,
    pub mean_daily_return_pct: Option<f64>,
    pub annualized_return_pct: Option<f64>,
    /// Compounded return earned only on the in-state bars.
    pub compounded_return_pct: Option<f64>,
    pub up_day_share_pct: Option<f64>,
}

const TRADING_DAYS: f64 = 252.0;

fn parse_dates(dates: &[String]) -> Vec<Option<NaiveDate>> {
    dates
        .iter()
        .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect()
}

/// Simulate one-position-at-a-time trades from an entry-edge and an exit rule.
pub fn simulate_trades(
    dates: &[String],
    closes: &[Option<f64>],
    entry: &[Option<bool>],
    exit: &ExitKind,
) -> (Vec<Trade>, usize) {
    let n = dates.len().min(closes.len()).min(entry.len());
    let parsed = parse_dates(dates);
    let mut trades = Vec::new();
    let mut open_skipped = 0usize;
    let mut i = 1usize; // edge needs a previous bar
    while i < n {
        let fired = entry[i] == Some(true) && entry[i - 1] != Some(true);
        if !fired {
            i += 1;
            continue;
        }
        let (entry_price, entry_date, entry_nd) = match (closes[i], parsed[i]) {
            (Some(p), Some(d)) => (p, dates[i].clone(), d),
            _ => {
                i += 1;
                continue;
            }
        };
        // Find the exit bar j > i.
        let mut j = i + 1;
        let mut exit_idx = None;
        while j < n {
            let close_ok = closes[j].is_some();
            let hit = match exit {
                ExitKind::HoldDays(days) => match (parsed[j], close_ok) {
                    (Some(dj), true) => (dj - entry_nd).num_days() >= *days,
                    _ => false,
                },
                ExitKind::Condition(c) => c.get(j).copied().flatten() == Some(true) && close_ok,
            };
            if hit {
                exit_idx = Some(j);
                break;
            }
            j += 1;
        }
        match exit_idx {
            Some(j) => {
                let exit_price = closes[j].unwrap();
                let exit_nd = parsed[j].unwrap();
                let return_pct = (exit_price / entry_price - 1.0) * 100.0;
                trades.push(Trade {
                    entry_date,
                    entry_price,
                    exit_date: dates[j].clone(),
                    exit_price,
                    return_pct,
                    bars_held: j - i,
                    days_held: (exit_nd - entry_nd).num_days(),
                });
                i = j + 1; // no overlapping positions
            }
            None => {
                // Position never closed within data — exclude from realized stats.
                open_skipped += 1;
                break;
            }
        }
    }
    (trades, open_skipped)
}

pub fn trade_report(
    dates: &[String],
    closes: &[Option<f64>],
    trades: Vec<Trade>,
    open_skipped: usize,
) -> TradeReport {
    let n = trades.len();
    let mut rets: Vec<f64> = trades.iter().map(|t| t.return_pct).collect();
    let win_count = rets.iter().filter(|r| **r > 0.0).count();
    let loss_count = rets.iter().filter(|r| **r < 0.0).count();

    let mean = if n > 0 {
        Some(rets.iter().sum::<f64>() / n as f64)
    } else {
        None
    };
    let mut sorted = rets.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if n > 0 {
        Some(if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        })
    } else {
        None
    };
    let best = sorted.last().copied();
    let worst = sorted.first().copied();

    // Compounded equity across closed trades + drawdown of that curve.
    let mut equity = 1.0f64;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    for t in &trades {
        equity *= 1.0 + t.return_pct / 100.0;
        if equity > peak {
            peak = equity;
        }
        let dd = (equity / peak - 1.0) * 100.0;
        if dd < max_dd {
            max_dd = dd;
        }
    }
    let total_return_pct = (equity - 1.0) * 100.0;

    let bench = buy_hold(dates, closes);
    let cagr_pct = bench
        .years
        .gt(&0.0)
        .then(|| (equity.powf(1.0 / bench.years) - 1.0) * 100.0)
        .filter(|v| v.is_finite());

    let total_bars = dates.len().max(1);
    let bars_in_market: usize = trades.iter().map(|t| t.bars_held).sum();
    let time_in_market_pct = bars_in_market as f64 / total_bars as f64 * 100.0;
    let avg_days_held = if n > 0 {
        Some(trades.iter().map(|t| t.days_held).sum::<i64>() as f64 / n as f64)
    } else {
        None
    };

    rets.clear();
    TradeReport {
        n_trades: n,
        n_open_skipped: open_skipped,
        win_count,
        loss_count,
        win_rate_pct: (n > 0).then(|| win_count as f64 / n as f64 * 100.0),
        mean_return_pct: mean,
        median_return_pct: median,
        best_return_pct: best,
        worst_return_pct: worst,
        total_return_pct,
        cagr_pct,
        max_drawdown_pct: max_dd,
        time_in_market_pct,
        avg_days_held,
        benchmark_hold: bench,
        trades,
    }
}

/// Buy-and-hold benchmark over the full master axis.
pub fn buy_hold(dates: &[String], closes: &[Option<f64>]) -> BenchStats {
    let parsed = parse_dates(dates);
    let first = (0..closes.len()).find(|&i| closes[i].is_some());
    let last = (0..closes.len()).rev().find(|&i| closes[i].is_some());
    let (fi, li) = match (first, last) {
        (Some(a), Some(b)) if b > a => (a, b),
        _ => {
            return BenchStats {
                first_date: dates.first().cloned().unwrap_or_default(),
                last_date: dates.last().cloned().unwrap_or_default(),
                years: 0.0,
                total_return_pct: 0.0,
                cagr_pct: None,
                max_drawdown_pct: 0.0,
            }
        }
    };
    let p0 = closes[fi].unwrap();
    let p1 = closes[li].unwrap();
    let total = (p1 / p0 - 1.0) * 100.0;
    let years = match (parsed[fi], parsed[li]) {
        (Some(a), Some(b)) => (b - a).num_days() as f64 / 365.25,
        _ => 0.0,
    };
    let cagr = (years > 0.0 && p0 > 0.0)
        .then(|| ((p1 / p0).powf(1.0 / years) - 1.0) * 100.0)
        .filter(|v| v.is_finite());

    // Max drawdown of the daily close curve.
    let mut peak = f64::MIN;
    let mut max_dd = 0.0f64;
    for c in closes.iter().flatten() {
        if *c > peak {
            peak = *c;
        }
        if peak > 0.0 {
            let dd = (c / peak - 1.0) * 100.0;
            if dd < max_dd {
                max_dd = dd;
            }
        }
    }
    BenchStats {
        first_date: dates[fi].clone(),
        last_date: dates[li].clone(),
        years,
        total_return_pct: total,
        cagr_pct: cagr,
        max_drawdown_pct: max_dd,
    }
}

/// Forward 1-bar returns partitioned by a regime mask.
pub fn segment_stats(
    label: &str,
    closes: &[Option<f64>],
    mask: &[Option<bool>],
    want: bool,
) -> SegmentStats {
    let n = closes.len().min(mask.len());
    // Forward 1-bar return at bar i is realized holding i -> i+1.
    let mut total_eval = 0usize; // bars with a measurable forward return and a known mask
    let mut selected: Vec<f64> = Vec::new();
    let mut compounded = 1.0f64;
    let mut up_days = 0usize;
    let mut episodes = 0usize;
    let mut prev_on = false;
    for i in 0..n.saturating_sub(1) {
        let (c0, c1) = match (closes[i], closes[i + 1]) {
            (Some(a), Some(b)) if a > 0.0 => (a, b),
            _ => {
                prev_on = false;
                continue;
            }
        };
        let m = match mask[i] {
            Some(b) => b,
            None => {
                prev_on = false;
                continue;
            }
        };
        total_eval += 1;
        let on = m == want;
        if on {
            let r = c1 / c0 - 1.0;
            selected.push(r * 100.0);
            compounded *= 1.0 + r;
            if r > 0.0 {
                up_days += 1;
            }
            if !prev_on {
                episodes += 1;
            }
        }
        prev_on = on;
    }
    let nd = selected.len();
    let mean = (nd > 0).then(|| selected.iter().sum::<f64>() / nd as f64);
    let annualized = (nd > 0)
        .then(|| (compounded.powf(TRADING_DAYS / nd as f64) - 1.0) * 100.0)
        .filter(|v| v.is_finite());
    SegmentStats {
        label: label.to_string(),
        n_days: nd,
        share_of_days_pct: if total_eval > 0 {
            nd as f64 / total_eval as f64 * 100.0
        } else {
            0.0
        },
        episodes,
        mean_daily_return_pct: mean,
        annualized_return_pct: annualized,
        compounded_return_pct: (nd > 0).then_some((compounded - 1.0) * 100.0),
        up_day_share_pct: (nd > 0).then(|| up_days as f64 / nd as f64 * 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2020-{:02}-01", (i % 12) + 1)).collect()
    }

    #[test]
    fn single_trade_hold_one_bar_condition() {
        // closes: enter at bar1 (edge), exit next bar via condition.
        let d = vec![
            "2020-01-01".to_string(),
            "2020-01-02".to_string(),
            "2020-01-03".to_string(),
        ];
        let closes = vec![Some(100.0), Some(110.0), Some(121.0)];
        let entry = vec![Some(false), Some(true), Some(false)];
        let exit = ExitKind::Condition(vec![Some(false), Some(false), Some(true)]);
        let (trades, open) = simulate_trades(&d, &closes, &entry, &exit);
        assert_eq!(open, 0);
        assert_eq!(trades.len(), 1);
        let t = &trades[0];
        assert_eq!(t.entry_price, 110.0);
        assert_eq!(t.exit_price, 121.0);
        assert!((t.return_pct - 10.0).abs() < 1e-9);
    }

    #[test]
    fn hold_days_exit_picks_first_bar_past_horizon() {
        let d = vec![
            "2020-01-01".to_string(),
            "2020-01-05".to_string(),
            "2020-01-20".to_string(),
        ];
        let closes = vec![Some(10.0), Some(12.0), Some(15.0)];
        let entry = vec![Some(false), Some(true), Some(false)];
        let exit = ExitKind::HoldDays(10);
        let (trades, _) = simulate_trades(&d, &closes, &entry, &exit);
        assert_eq!(trades.len(), 1);
        // entry 01-05 @12, exit first bar >= +10d = 01-20 @15.
        assert_eq!(trades[0].exit_date, "2020-01-20");
        assert!((trades[0].return_pct - 25.0).abs() < 1e-9);
    }

    #[test]
    fn no_overlapping_positions() {
        let d = dates(6);
        let closes = vec![
            Some(10.0),
            Some(11.0),
            Some(12.0),
            Some(13.0),
            Some(14.0),
            Some(15.0),
        ];
        // entry condition true on bars 1..=4 (one rising edge at bar1).
        let entry = vec![
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
            Some(false),
        ];
        // exit fires every bar; first exit after entry closes the single trade.
        let exit = ExitKind::Condition(vec![Some(true); 6]);
        let (trades, _) = simulate_trades(&d, &closes, &entry, &exit);
        // Edge at bar1 -> exit bar2. Next edge would need a false->true flip;
        // entry stays true so no new edge until after it resets.
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].entry_date, d[1]);
    }

    #[test]
    fn segment_partitions_by_mask() {
        let closes = vec![Some(100.0), Some(110.0), Some(99.0), Some(108.9)];
        // mask on at bars 0 and 2.
        let mask = vec![Some(true), Some(false), Some(true), Some(false)];
        let s = segment_stats("on", &closes, &mask, true);
        // forward returns: bar0 +10%, bar2 +10% (selected); 2 episodes.
        assert_eq!(s.n_days, 2);
        assert_eq!(s.episodes, 2);
        assert!((s.mean_daily_return_pct.unwrap() - 10.0).abs() < 1e-6);
    }
}
