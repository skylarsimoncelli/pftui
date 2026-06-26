//! Point-in-time **signal-accessor registry** for the positioning rule engine
//! (POSITIONING-MODELS.md §3.2 step 2 + §3.4 lookahead policy).
//!
//! An accessor is a named, typed scalar reader — `cycle_bottom_met('BTC-USD')`,
//! `cycle_top_met('GC=F')@weekly` — that the `when` evaluator
//! ([`super::rule_expr`]) calls at a single rebalance date `T`. Each accessor
//! returns an `f64`; insufficient history returns `NaN` (see below).
//!
//! ## The COMPLETED-bucket lookahead policy (the load-bearing rule) [R]
//! A rebalance decision at date `T` may use only data whose bucket has fully
//! COMPLETED on/before `T` (§3.4). For a signal at timeframe `tf` the symbol's
//! daily history is trimmed to a cutoff date before the underlying signal fn
//! ever sees it:
//!
//! | tf      | cutoff = last fully-completed bucket ≤ T                       |
//! |---------|----------------------------------------------------------------|
//! | daily   | `T` (every bar through T's close)                              |
//! | weekly  | the most recent ISO-week **Sunday ≤ T** (the prior week's end  |
//! |         | when T is mid-week; T itself when T is a Sunday)               |
//! | monthly | the last calendar-day of the most recent **fully-ended month** |
//! |         | (prior month's last day when T is mid-month; T when T is EOM)  |
//!
//! Trimming the *daily* history to this cutoff guarantees the signal fn's own
//! weekly/monthly aggregation can never form a PARTIAL final bucket out of bars
//! that lie inside the in-progress week/month at `T`. This trim is implemented
//! ONCE here so every accessor inherits it — no per-call ad-hoc slicing.
//!
//! ## Insufficient history → rule does not fire
//! When the trimmed history is too shallow the signal fn returns `None`; the
//! accessor then returns `f64::NAN`, and [`super::rule_expr::compare`] makes any
//! comparison touching a `NaN` FALSE. A rule that can't be computed is therefore
//! treated as not-firing (the safe default), never silently true.
//!
//! ## Memoization
//! Within one rebalance date a `(kind, symbol, tf)` snapshot is computed at most
//! once and cached in [`Memo`]; two rules reading the same accessor share the
//! result. The engine builds a FRESH [`Memo`] per date, so there is no
//! cross-date leakage.

use std::collections::HashMap;

use anyhow::{bail, Result};
use chrono::{Datelike, Days, NaiveDate};

use super::PricePanel;
use crate::analytics::cycle_signals::{cycle_bottom_signals, cycle_top_signals, SignalTimeframe};
use crate::models::price::HistoryRecord;

/// Which mechanical confluence snapshot an accessor reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotKind {
    /// `cycle_bottom_signals(...).met_count`
    CycleBottom,
    /// `cycle_top_signals(...).met_count`
    CycleTop,
}

/// One registry entry. Adding a new accessor is a single row here plus (if it
/// reads a new snapshot) a `SnapshotKind` arm in [`snapshot_met`].
#[derive(Debug, Clone, Copy)]
pub struct AccessorDef {
    pub name: &'static str,
    pub arity: usize,
    pub kind: SnapshotKind,
}

/// The accessor registry. **M2 stage:** the two cycle-confluence counters.
/// (`regime_score`/`cyber_*`/`stage_proxy` are deferred to later stages — each
/// is a one-row addition here.)
static REGISTRY: &[AccessorDef] = &[
    AccessorDef {
        name: "cycle_bottom_met",
        arity: 1,
        kind: SnapshotKind::CycleBottom,
    },
    AccessorDef {
        name: "cycle_top_met",
        arity: 1,
        kind: SnapshotKind::CycleTop,
    },
];

/// Look up an accessor definition by name.
pub fn lookup(name: &str) -> Option<&'static AccessorDef> {
    REGISTRY.iter().find(|d| d.name == name)
}

/// Names of all registered accessors (for error messages).
pub fn known_names() -> Vec<&'static str> {
    REGISTRY.iter().map(|d| d.name).collect()
}

/// Per-rebalance-date snapshot cache. Built fresh by the engine at each date.
#[derive(Debug, Default)]
pub struct Memo {
    cache: HashMap<(SnapshotKind, String, SignalTimeframe), Option<usize>>,
    /// Number of actual signal computations (cache misses) — exposed so tests
    /// can prove a `(sym, tf)` snapshot is computed only once per date.
    pub compute_count: usize,
}

impl Memo {
    pub fn new() -> Self {
        Self::default()
    }
}

/// The point-in-time evaluation context handed to each accessor.
pub struct AtDateCtx<'a> {
    /// The rebalance decision date `T`.
    pub as_of: NaiveDate,
    /// Price panel (daily closes per symbol).
    pub panel: &'a PricePanel,
    /// Timeframe used when an accessor omits `@tf`.
    pub default_tf: SignalTimeframe,
    /// Per-date snapshot cache.
    pub memo: &'a mut Memo,
}

/// Evaluate accessor `name(args)[@tf]` at `ctx.as_of`. Returns the metric value
/// as `f64`, or `f64::NAN` when history is too shallow (rule won't fire).
pub fn eval_accessor(
    name: &str,
    args: &[String],
    tf: Option<SignalTimeframe>,
    ctx: &mut AtDateCtx,
) -> Result<f64> {
    let def = lookup(name).ok_or_else(|| anyhow::anyhow!("unknown signal accessor '{name}'"))?;
    if args.len() != def.arity {
        bail!(
            "accessor '{name}' takes {} argument(s), got {}",
            def.arity,
            args.len()
        );
    }
    let symbol = &args[0];
    let tf = tf.unwrap_or(ctx.default_tf);
    let met = snapshot_met(def.kind, symbol, tf, ctx);
    Ok(match met {
        Some(m) => m as f64,
        None => f64::NAN, // insufficient history → comparison is false → no fire
    })
}

/// Compute (memoized) the `met_count` of `kind`'s confluence snapshot for
/// `symbol` at timeframe `tf`, on the COMPLETED-bucket-trimmed daily history.
fn snapshot_met(
    kind: SnapshotKind,
    symbol: &str,
    tf: SignalTimeframe,
    ctx: &mut AtDateCtx,
) -> Option<usize> {
    let key = (kind, symbol.to_string(), tf);
    if let Some(cached) = ctx.memo.cache.get(&key) {
        return *cached;
    }
    let history = completed_bucket_history(ctx.panel, symbol, tf, ctx.as_of);
    ctx.memo.compute_count += 1;
    let met = match kind {
        SnapshotKind::CycleBottom => {
            cycle_bottom_signals(symbol, &history, tf).map(|s| s.met_count)
        }
        SnapshotKind::CycleTop => cycle_top_signals(symbol, &history, tf).map(|s| s.met_count),
    };
    ctx.memo.cache.insert(key, met);
    met
}

/// The cutoff date = last day of the last fully-completed bucket ≤ `as_of`.
/// See the module docs for the exact per-timeframe rule.
pub fn completed_bucket_cutoff(tf: SignalTimeframe, as_of: NaiveDate) -> NaiveDate {
    match tf {
        SignalTimeframe::Daily => as_of,
        SignalTimeframe::Weekly => {
            // Most recent ISO-week Sunday on/before `as_of`. ISO weeks run
            // Mon..Sun, so the Sunday is the week's END. num_days_from_sunday()
            // is 0 on Sunday, 1..6 Mon..Sat → step back to that Sunday.
            let back = as_of.weekday().num_days_from_sunday() as u64;
            as_of - Days::new(back)
        }
        SignalTimeframe::Monthly => {
            let first_this = first_of_month(as_of);
            let last_this = last_of_month(as_of);
            if as_of == last_this {
                // `as_of` is the final day of its month → that month completed.
                as_of
            } else {
                // Mid-month → last completed month is the previous one.
                first_this
                    .pred_opt()
                    .unwrap_or(first_this)
            }
        }
    }
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

fn last_of_month(d: NaiveDate) -> NaiveDate {
    let (y, m) = (d.year(), d.month());
    let next_first = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    };
    next_first
        .and_then(|f| f.pred_opt())
        .unwrap_or(d)
}

/// Build the symbol's daily [`HistoryRecord`] history trimmed to the last
/// completed bucket ≤ `as_of`. Close-only (the cycle signal fn derives an OHLC
/// fallback). The single chokepoint that makes every accessor lookahead-safe.
pub fn completed_bucket_history(
    panel: &PricePanel,
    symbol: &str,
    tf: SignalTimeframe,
    as_of: NaiveDate,
) -> Vec<HistoryRecord> {
    let cutoff = completed_bucket_cutoff(tf, as_of);
    panel
        .closes_through(symbol, cutoff)
        .into_iter()
        .map(|(d, close)| HistoryRecord {
            date: d.format("%Y-%m-%d").to_string(),
            close,
            volume: None,
            open: None,
            high: None,
            low: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// A daily panel for `sym` spanning `[start, end]`, one bar per calendar day,
    /// price = 100 + day-index (so each date is uniquely identifiable).
    fn daily_panel(sym: &str, start: NaiveDate, n: u64) -> PricePanel {
        let mut p = PricePanel::new();
        let series: Vec<(NaiveDate, Decimal)> = (0..n)
            .map(|i| {
                (
                    start + Days::new(i),
                    Decimal::from(100 + i as i64),
                )
            })
            .collect();
        p.insert_series(sym, series);
        p
    }

    #[test]
    fn weekly_cutoff_is_prior_sunday_midweek() {
        // 2024-06-12 is a Wednesday. The most recent completed ISO week ended
        // Sunday 2024-06-09. Mid-week bars (Mon 10, Tue 11, Wed 12) must NOT
        // appear in the trimmed history.
        let as_of = d(2024, 6, 12);
        assert_eq!(as_of.weekday(), chrono::Weekday::Wed);
        assert_eq!(
            completed_bucket_cutoff(SignalTimeframe::Weekly, as_of),
            d(2024, 6, 9)
        );
        let panel = daily_panel("X", d(2024, 5, 1), 60);
        let hist = completed_bucket_history(&panel, "X", SignalTimeframe::Weekly, as_of);
        let dates: Vec<&str> = hist.iter().map(|r| r.date.as_str()).collect();
        assert!(dates.contains(&"2024-06-09"), "completed week end included");
        for peek in ["2024-06-10", "2024-06-11", "2024-06-12"] {
            assert!(
                !dates.contains(&peek),
                "partial-week bar {peek} must not be visible (lookahead!)"
            );
        }
    }

    #[test]
    fn weekly_cutoff_on_sunday_includes_that_week() {
        // 2024-06-09 is a Sunday → that week has ended → cutoff == as_of.
        let as_of = d(2024, 6, 9);
        assert_eq!(as_of.weekday(), chrono::Weekday::Sun);
        assert_eq!(
            completed_bucket_cutoff(SignalTimeframe::Weekly, as_of),
            as_of
        );
    }

    #[test]
    fn monthly_cutoff_is_prior_month_end_midmonth() {
        // Mid-month 2024-06-15 → last completed month is May → cutoff 2024-05-31.
        let as_of = d(2024, 6, 15);
        assert_eq!(
            completed_bucket_cutoff(SignalTimeframe::Monthly, as_of),
            d(2024, 5, 31)
        );
        let panel = daily_panel("X", d(2024, 1, 1), 200);
        let hist = completed_bucket_history(&panel, "X", SignalTimeframe::Monthly, as_of);
        let dates: Vec<&str> = hist.iter().map(|r| r.date.as_str()).collect();
        assert!(dates.contains(&"2024-05-31"), "completed month end included");
        assert!(
            !dates.iter().any(|x| x.starts_with("2024-06")),
            "no June (in-progress month) bar may be visible (lookahead!)"
        );
    }

    #[test]
    fn monthly_cutoff_on_month_end_includes_that_month() {
        let as_of = d(2024, 5, 31);
        assert_eq!(
            completed_bucket_cutoff(SignalTimeframe::Monthly, as_of),
            d(2024, 5, 31)
        );
    }

    #[test]
    fn daily_cutoff_is_as_of() {
        let as_of = d(2024, 6, 12);
        assert_eq!(completed_bucket_cutoff(SignalTimeframe::Daily, as_of), as_of);
        let panel = daily_panel("X", d(2024, 6, 1), 30);
        let hist = completed_bucket_history(&panel, "X", SignalTimeframe::Daily, as_of);
        let last = hist.last().unwrap();
        assert_eq!(last.date, "2024-06-12");
        // Nothing after `as_of`.
        assert!(!hist.iter().any(|r| r.date.as_str() > "2024-06-12"));
    }

    #[test]
    fn insufficient_history_yields_nan() {
        // Far too few bars for cycle_bottom_signals (needs >= 120 daily).
        let panel = daily_panel("X", d(2024, 1, 1), 30);
        let mut memo = Memo::new();
        let mut ctx = AtDateCtx {
            as_of: d(2024, 1, 30),
            panel: &panel,
            default_tf: SignalTimeframe::Monthly,
            memo: &mut memo,
        };
        let v = eval_accessor("cycle_bottom_met", &["X".to_string()], None, &mut ctx).unwrap();
        assert!(v.is_nan(), "shallow history must yield the NaN sentinel");
    }

    #[test]
    fn memo_computes_snapshot_once_per_date() {
        // Deep enough history that the snapshot computes (Some). Two reads of the
        // same (sym, tf) must hit the cache → exactly one computation.
        let panel = daily_panel("X", d(2019, 1, 1), 900);
        let mut memo = Memo::new();
        let mut ctx = AtDateCtx {
            as_of: d(2021, 6, 12),
            panel: &panel,
            default_tf: SignalTimeframe::Monthly,
            memo: &mut memo,
        };
        let a = eval_accessor("cycle_bottom_met", &["X".to_string()], None, &mut ctx).unwrap();
        let b = eval_accessor("cycle_bottom_met", &["X".to_string()], None, &mut ctx).unwrap();
        assert_eq!(a, b, "memoized reads must be identical");
        assert_eq!(ctx.memo.compute_count, 1, "snapshot computed exactly once");
        // A different timeframe is a distinct snapshot → a second computation.
        let _ = eval_accessor(
            "cycle_bottom_met",
            &["X".to_string()],
            Some(SignalTimeframe::Weekly),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(ctx.memo.compute_count, 2);
    }

}
