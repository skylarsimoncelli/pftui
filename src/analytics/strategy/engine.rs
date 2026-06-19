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

/// The base exit rule (how a position closes absent a stop/target).
pub enum ExitKind {
    /// Exit at the first bar on/after `entry_date + days`.
    HoldDays(i64),
    /// Exit at the first bar (after entry) where this condition fires.
    Condition(Vec<Option<bool>>),
}

/// Full exit configuration: the base rule plus optional risk exits checked
/// intra-bar against the high/low. Percentages are whole numbers (15.0 = 15%).
pub struct ExitConfig {
    pub base: ExitKind,
    /// Hard stop: exit if the bar's low breaches entry·(1 − stop/100).
    pub stop_loss_pct: Option<f64>,
    /// Profit target: exit if the bar's high reaches entry·(1 + target/100).
    pub take_profit_pct: Option<f64>,
    /// Trailing stop: exit if the bar's low falls trailing% below the highest
    /// high seen since entry.
    pub trailing_pct: Option<f64>,
}

impl ExitConfig {
    pub fn new(base: ExitKind) -> Self {
        ExitConfig {
            base,
            stop_loss_pct: None,
            take_profit_pct: None,
            trailing_pct: None,
        }
    }
}

/// Execution realism: per-side trading frictions and fill timing. The default
/// (all zero, same-bar fill) reproduces the original cost-free behaviour, so
/// existing results are unchanged unless costs are explicitly requested.
#[derive(Debug, Clone, Copy, Default)]
pub struct CostModel {
    /// Commission per side as a percent of notional (0.1 = 0.1%). Charged on
    /// both entry and exit → a round-trip drag of `2 × commission_pct`.
    pub commission_pct: f64,
    /// Slippage per side as a percent: entries fill `slippage_pct` HIGHER and
    /// exits `slippage_pct` LOWER than the reference price (you cross the
    /// spread against yourself both ways).
    pub slippage_pct: f64,
    /// Bars to wait between the signal bar and the fill. 0 = fill at the
    /// signal bar's close (same-bar); 1 = fill at the NEXT bar's close — the
    /// honest default for a signal only known after its bar closes (removes the
    /// same-bar look-ahead that flatters the equity curve and the gauntlet).
    pub fill_delay_bars: usize,
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
    /// What closed the trade: "rule" | "stop" | "target" | "trailing".
    pub exit_reason: String,
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
    // --- tearsheet ---
    /// Mean / median win and loss magnitudes (percent).
    pub avg_win_pct: Option<f64>,
    pub avg_loss_pct: Option<f64>,
    /// Gross profit / gross loss. > 1 is profitable; >1.5 is good.
    pub profit_factor: Option<f64>,
    /// Win-rate-weighted expected return per trade (percent).
    pub expectancy_pct: Option<f64>,
    /// Payoff ratio (avg win / |avg loss|).
    pub payoff_ratio: Option<f64>,
    /// Downside-deviation risk-adjusted CAGR (annualized).
    pub sortino_ratio: Option<f64>,
    /// CAGR / |max drawdown| of the strategy equity curve.
    pub calmar_ratio: Option<f64>,
    /// Longest run of consecutive losing trades.
    pub max_consecutive_losses: usize,
    /// Count of trades closed by each exit reason.
    pub exit_reason_counts: std::collections::BTreeMap<String, usize>,
    pub benchmark_hold: BenchStats,
    /// Statistical honesty stats on the per-trade return series (none when
    /// there are too few trades to be meaningful).
    pub validation: Option<TradeValidation>,
    pub trades: Vec<Trade>,
}

/// A small slice of the validation gauntlet applied to a single strategy's
/// per-trade returns — enough to flag "is this distinguishable from luck?"
/// The full PBO / multiple-testing haircut applies to multi-config sweeps
/// (Phase 3 positioning), not a single rule.
#[derive(Debug, Clone, Serialize)]
pub struct TradeValidation {
    /// Per-trade dispersion ratio (mean / std of trade returns). NOT a
    /// time-based Sharpe — trades may have different holding periods, so this
    /// is not comparable across exit rules. Interpret as the consistency of
    /// the per-trade edge, not an annualized risk-adjusted return.
    pub trade_dispersion_ratio: Option<f64>,
    /// Probabilistic Sharpe vs a ZERO benchmark — P(the per-trade edge > 0).
    /// This is a SINGLE-rule PSR, NOT a deflated/multiple-testing-corrected
    /// statistic. Only populated when the sample is adequate (n >= 10); `None`
    /// (anecdotal) below that, because a confident-looking number on 3 trades
    /// is exactly the failure mode to avoid.
    pub psr_vs_zero: Option<f64>,
    /// Block-bootstrap 90% CI on the mean trade return (percent), deterministic.
    pub mean_return_ci_pct: Option<(f64, f64)>,
    /// True when the sample is too small to trust (n < 10).
    pub anecdotal: bool,
}

fn trade_validation(trades: &[Trade]) -> Option<TradeValidation> {
    use crate::research::validation as v;
    if trades.len() < 3 {
        return None;
    }
    // Per-trade returns as fractions.
    let rets: Vec<f64> = trades.iter().map(|t| t.return_pct / 100.0).collect();
    let trade_dispersion_ratio = v::sharpe(&rets);
    let anecdotal = trades.len() < 10;
    // Only report a PSR when the sample is adequate — never put a confident %
    // on a handful of trades.
    let psr_vs_zero = (!anecdotal)
        .then(|| {
            v::moments(&rets).map(|m| {
                let sr = if m.std > 0.0 { m.mean / m.std } else { 0.0 };
                v::probabilistic_sharpe_ratio(sr, 0.0, m.n, m.skew, m.kurtosis)
            })
        })
        .flatten()
        .filter(|p| p.is_finite());
    // Deterministic seed from the trade span so the CI is reproducible.
    let seed = v::seed_from_str(&format!(
        "{}:{}:{}",
        trades.first().map(|t| t.entry_date.as_str()).unwrap_or(""),
        trades.last().map(|t| t.exit_date.as_str()).unwrap_or(""),
        trades.len()
    ));
    let block = (rets.len() as f64).powf(1.0 / 3.0).max(2.0);
    let mean_return_ci_pct = v::block_bootstrap_ci(
        &rets,
        |s| Some(s.iter().sum::<f64>() / s.len() as f64),
        1000,
        block,
        0.10,
        seed,
    )
    .map(|(lo, _p, hi)| (lo * 100.0, hi * 100.0));
    Some(TradeValidation {
        trade_dispersion_ratio,
        psr_vs_zero,
        mean_return_ci_pct,
        anecdotal,
    })
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

/// Simulate one-position-at-a-time trades. Risk exits (stop/target/trailing)
/// are checked INTRA-BAR against `highs`/`lows` (falling back to the close when
/// OHLC is unavailable); the base rule exits at the close. On a bar where both
/// a downside (stop/trailing) and the upside target could trigger, the downside
/// is taken (conservative — we cannot know intra-bar order).
pub fn simulate_trades(
    dates: &[String],
    closes: &[Option<f64>],
    highs: &[Option<f64>],
    lows: &[Option<f64>],
    entry: &[Option<bool>],
    exit: &ExitConfig,
    cost: &CostModel,
) -> (Vec<Trade>, usize) {
    let n = dates.len().min(closes.len()).min(entry.len());
    let parsed = parse_dates(dates);
    let slip = cost.slippage_pct / 100.0;
    let round_trip_commission = 2.0 * cost.commission_pct / 100.0;
    let mut trades = Vec::new();
    let mut open_skipped = 0usize;
    let mut i = 1usize; // edge needs a previous bar
    while i < n {
        let fired = entry[i] == Some(true) && entry[i - 1] != Some(true);
        if !fired {
            i += 1;
            continue;
        }
        // The fill lands `fill_delay_bars` after the signal bar (next-bar fill
        // removes same-bar look-ahead). The reference price is that bar's close;
        // slippage moves the actual entry fill against us (higher).
        let fill_bar = i + cost.fill_delay_bars;
        let (entry_price, entry_date, entry_nd) = match (closes.get(fill_bar).copied().flatten(), parsed.get(fill_bar).copied().flatten()) {
            (Some(p), Some(d)) if p > 0.0 => (p * (1.0 + slip), dates[fill_bar].clone(), d),
            _ => {
                i += 1;
                continue;
            }
        };
        let stop_price = exit.stop_loss_pct.map(|p| entry_price * (1.0 - p / 100.0));
        let target_price = exit.take_profit_pct.map(|p| entry_price * (1.0 + p / 100.0));
        let mut peak = entry_price; // highest high since entry, for the trailing stop

        // Walk bars j > fill_bar, taking the first exit.
        let mut outcome: Option<(usize, f64, &'static str)> = None; // (idx, exit_price, reason)
        let mut j = fill_bar + 1;
        while j < n {
            let Some(close_j) = closes[j] else {
                j += 1;
                continue;
            };
            let high_j = highs[j].unwrap_or(close_j).max(close_j);
            let low_j = lows[j].unwrap_or(close_j).min(close_j);
            peak = peak.max(high_j);

            // Downside first (conservative).
            if let Some(sp) = stop_price {
                if low_j <= sp {
                    outcome = Some((j, sp, "stop"));
                    break;
                }
            }
            if let Some(tr) = exit.trailing_pct {
                let trail = peak * (1.0 - tr / 100.0);
                if low_j <= trail && trail < entry_price.max(peak) {
                    outcome = Some((j, trail, "trailing"));
                    break;
                }
            }
            if let Some(tp) = target_price {
                if high_j >= tp {
                    outcome = Some((j, tp, "target"));
                    break;
                }
            }
            // Base rule (exits at the close).
            let rule_hit = match &exit.base {
                ExitKind::HoldDays(days) => parsed[j].map(|dj| (dj - entry_nd).num_days() >= *days).unwrap_or(false),
                ExitKind::Condition(c) => c.get(j).copied().flatten() == Some(true),
            };
            if rule_hit {
                outcome = Some((j, close_j, "rule"));
                break;
            }
            j += 1;
        }

        match outcome {
            Some((j, exit_ref, reason)) => {
                let exit_nd = parsed[j].unwrap_or(entry_nd);
                // Slippage moves the exit fill against us (lower); commission is
                // a round-trip drag on the net return.
                let exit_price = exit_ref * (1.0 - slip);
                let return_pct =
                    (exit_price / entry_price - 1.0 - round_trip_commission) * 100.0;
                trades.push(Trade {
                    entry_date,
                    entry_price,
                    exit_date: dates[j].clone(),
                    exit_price,
                    return_pct,
                    bars_held: j - fill_bar,
                    days_held: (exit_nd - entry_nd).num_days(),
                    exit_reason: reason.to_string(),
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

    // --- tearsheet ---
    let wins: Vec<f64> = rets.iter().copied().filter(|r| *r > 0.0).collect();
    let losses: Vec<f64> = rets.iter().copied().filter(|r| *r < 0.0).collect();
    let avg_win_pct = (!wins.is_empty()).then(|| wins.iter().sum::<f64>() / wins.len() as f64);
    let avg_loss_pct = (!losses.is_empty()).then(|| losses.iter().sum::<f64>() / losses.len() as f64);
    let gross_profit: f64 = wins.iter().sum();
    let gross_loss: f64 = losses.iter().map(|l| l.abs()).sum();
    let profit_factor = (gross_loss > 0.0).then(|| gross_profit / gross_loss).map(|p| p + 0.0); // normalize -0.0
    let expectancy_pct = mean; // mean per-trade return already is the expectancy
    let payoff_ratio = match (avg_win_pct, avg_loss_pct) {
        (Some(w), Some(l)) if l.abs() > 0.0 => Some(w / l.abs()),
        _ => None,
    };
    // Sortino: CAGR over downside deviation of per-trade returns (annualized by
    // the trade frequency).
    let sortino_ratio = (n >= 3).then(|| {
        let mean_r = rets.iter().sum::<f64>() / n as f64 / 100.0;
        let downside = rets
            .iter()
            .map(|r| r / 100.0)
            .filter(|r| *r < 0.0)
            .map(|r| r * r)
            .sum::<f64>()
            / n as f64;
        let dd = downside.sqrt();
        if dd > 0.0 {
            mean_r / dd
        } else {
            f64::INFINITY
        }
    }).filter(|v| v.is_finite());
    let calmar_ratio = match cagr_pct {
        Some(c) if max_dd < 0.0 => Some(c / max_dd.abs()),
        _ => None,
    };
    // Longest consecutive-loss streak.
    let mut max_consecutive_losses = 0usize;
    let mut streak = 0usize;
    for t in &trades {
        if t.return_pct < 0.0 {
            streak += 1;
            max_consecutive_losses = max_consecutive_losses.max(streak);
        } else {
            streak = 0;
        }
    }
    let mut exit_reason_counts: std::collections::BTreeMap<String, usize> = Default::default();
    for t in &trades {
        *exit_reason_counts.entry(t.exit_reason.clone()).or_insert(0) += 1;
    }

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
        avg_win_pct,
        avg_loss_pct,
        profit_factor,
        expectancy_pct,
        payoff_ratio,
        sortino_ratio,
        calmar_ratio,
        max_consecutive_losses,
        exit_reason_counts,
        benchmark_hold: bench,
        validation: trade_validation(&trades),
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
        let exit = ExitConfig::new(ExitKind::Condition(vec![Some(false), Some(false), Some(true)]));
        let hl = vec![None; closes.len()];
        let (trades, open) = simulate_trades(&d, &closes, &hl, &hl, &entry, &exit, &CostModel::default());
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
        let exit = ExitConfig::new(ExitKind::HoldDays(10));
        let hl = vec![None; closes.len()];
        let (trades, _) = simulate_trades(&d, &closes, &hl, &hl, &entry, &exit, &CostModel::default());
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
        let exit = ExitConfig::new(ExitKind::Condition(vec![Some(true); 6]));
        let hl = vec![None; closes.len()];
        let (trades, _) = simulate_trades(&d, &closes, &hl, &hl, &entry, &exit, &CostModel::default());
        // Edge at bar1 -> exit bar2. Next edge would need a false->true flip;
        // entry stays true so no new edge until after it resets.
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].entry_date, d[1]);
    }

    #[test]
    fn stop_loss_fires_intra_bar_on_the_low() {
        // Enter at bar1 @100; bar2 low dips to 80 (a 15% stop sits at 85).
        let d = dates(4);
        let closes = vec![Some(100.0), Some(100.0), Some(95.0), Some(95.0)];
        let highs = vec![Some(100.0), Some(100.0), Some(98.0), Some(96.0)];
        let lows = vec![Some(100.0), Some(100.0), Some(80.0), Some(94.0)];
        let entry = vec![Some(false), Some(true), Some(false), Some(false)];
        let mut exit = ExitConfig::new(ExitKind::HoldDays(365));
        exit.stop_loss_pct = Some(15.0);
        let (trades, _) = simulate_trades(&d, &closes, &highs, &lows, &entry, &exit, &CostModel::default());
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].exit_reason, "stop");
        assert!((trades[0].exit_price - 85.0).abs() < 1e-9); // exits at the stop, not the low
        assert!((trades[0].return_pct + 15.0).abs() < 1e-9);
    }

    #[test]
    fn null_ohlc_bar_falls_back_to_close_not_a_phantom_stop() {
        // bar2 has NO high/low (None) but its close rose to 110. A 15% stop
        // (at 85) must NOT fire on the missing-OHLC bar; it should fall back to
        // the bar's own close (110), so the trade survives to the rule exit.
        let d = dates(4);
        let closes = vec![Some(100.0), Some(100.0), Some(110.0), Some(120.0)];
        let highs = vec![Some(100.0), Some(100.0), None, Some(121.0)];
        let lows = vec![Some(100.0), Some(100.0), None, Some(119.0)];
        let entry = vec![Some(false), Some(true), Some(false), Some(false)];
        let mut exit = ExitConfig::new(ExitKind::HoldDays(2));
        exit.stop_loss_pct = Some(15.0);
        let (trades, _) = simulate_trades(&d, &closes, &highs, &lows, &entry, &exit, &CostModel::default());
        assert_eq!(trades.len(), 1);
        assert_ne!(trades[0].exit_reason, "stop", "no phantom stop on a NULL-OHLC bar");
    }

    #[test]
    fn take_profit_fires_intra_bar_on_the_high() {
        let d = dates(4);
        let closes = vec![Some(100.0), Some(100.0), Some(105.0), Some(105.0)];
        let highs = vec![Some(100.0), Some(100.0), Some(140.0), Some(106.0)];
        let lows = vec![Some(100.0), Some(100.0), Some(99.0), Some(104.0)];
        let entry = vec![Some(false), Some(true), Some(false), Some(false)];
        let mut exit = ExitConfig::new(ExitKind::HoldDays(365));
        exit.take_profit_pct = Some(30.0);
        let (trades, _) = simulate_trades(&d, &closes, &highs, &lows, &entry, &exit, &CostModel::default());
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].exit_reason, "target");
        assert!((trades[0].exit_price - 130.0).abs() < 1e-9);
    }

    #[test]
    fn commission_and_slippage_drag_the_return() {
        // Enter at bar1 @100, exit via condition at bar2 @110 = +10% gross.
        let d = vec![
            "2020-01-01".to_string(),
            "2020-01-02".to_string(),
            "2020-01-03".to_string(),
        ];
        let closes = vec![Some(100.0), Some(100.0), Some(110.0)];
        let entry = vec![Some(false), Some(true), Some(false)];
        let exit = ExitConfig::new(ExitKind::Condition(vec![Some(false), Some(false), Some(true)]));
        let hl = vec![None; closes.len()];
        // 0.1% commission/side + 0.05% slippage/side.
        let cost = CostModel {
            commission_pct: 0.1,
            slippage_pct: 0.05,
            fill_delay_bars: 0,
        };
        let (trades, _) = simulate_trades(&d, &closes, &hl, &hl, &entry, &exit, &cost);
        assert_eq!(trades.len(), 1);
        let t = &trades[0];
        // entry fill 100·1.0005 = 100.05; exit fill 110·0.9995 = 109.945.
        // gross = 109.945/100.05 − 1 = 0.098901; minus 0.002 round-trip commission.
        let expected = (109.945 / 100.05 - 1.0 - 0.002) * 100.0;
        assert!((t.return_pct - expected).abs() < 1e-6, "got {}", t.return_pct);
        // Strictly worse than the cost-free +10%.
        assert!(t.return_pct < 10.0);
    }

    #[test]
    fn next_bar_fill_enters_one_bar_after_the_signal() {
        // Signal (rising edge) at bar1; with fill_delay 1 the entry is bar2.
        let d = vec![
            "2020-01-01".to_string(),
            "2020-01-02".to_string(),
            "2020-01-03".to_string(),
            "2020-01-04".to_string(),
        ];
        let closes = vec![Some(100.0), Some(105.0), Some(110.0), Some(121.0)];
        let entry = vec![Some(false), Some(true), Some(false), Some(false)];
        let exit = ExitConfig::new(ExitKind::Condition(vec![
            Some(false),
            Some(false),
            Some(false),
            Some(true),
        ]));
        let hl = vec![None; closes.len()];
        let cost = CostModel {
            commission_pct: 0.0,
            slippage_pct: 0.0,
            fill_delay_bars: 1,
        };
        let (trades, _) = simulate_trades(&d, &closes, &hl, &hl, &entry, &exit, &cost);
        assert_eq!(trades.len(), 1);
        // Entry at bar2 (@110, the bar AFTER the signal), exit bar3 @121 = +10%.
        assert_eq!(trades[0].entry_date, "2020-01-03");
        assert_eq!(trades[0].entry_price, 110.0);
        assert!((trades[0].return_pct - 10.0).abs() < 1e-9);
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
