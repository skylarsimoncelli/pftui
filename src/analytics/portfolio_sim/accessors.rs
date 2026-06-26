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
use crate::regime::{compute_regime, RegimeScore, REGIME_YAHOO_SYMBOLS};

/// Which mechanical confluence snapshot a cycle accessor reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotKind {
    /// `cycle_bottom_signals(...).met_count`
    CycleBottom,
    /// `cycle_top_signals(...).met_count`
    CycleTop,
}

/// What an accessor computes. A `Cycle` accessor takes ONE symbol arg and reads a
/// per-symbol confluence snapshot; a `Regime` accessor takes ZERO args and reads
/// the cross-asset risk-on/off composite (sourced from the macro `REGIME_SYMBOLS`
/// the panel carries — see [`regime_at_date`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessorImpl {
    /// Per-symbol cycle confluence (`cycle_bottom_met`/`cycle_top_met`).
    Cycle(SnapshotKind),
    /// The as-of regime composite total (`regime_score`).
    Regime,
}

/// One registry entry. Adding a new accessor is a single row here plus (if it
/// reads a new snapshot) an [`AccessorImpl`] arm in [`eval_accessor`].
#[derive(Debug, Clone, Copy)]
pub struct AccessorDef {
    pub name: &'static str,
    pub arity: usize,
    pub imp: AccessorImpl,
}

/// The accessor registry. **M1/M2 stage:** the two cycle-confluence counters plus
/// the regime composite. (`cyber_*`/`stage_proxy` are deferred to later stages —
/// each is a one-row addition here.)
static REGISTRY: &[AccessorDef] = &[
    AccessorDef {
        name: "cycle_bottom_met",
        arity: 1,
        imp: AccessorImpl::Cycle(SnapshotKind::CycleBottom),
    },
    AccessorDef {
        name: "cycle_top_met",
        arity: 1,
        imp: AccessorImpl::Cycle(SnapshotKind::CycleTop),
    },
    AccessorDef {
        name: "regime_score",
        arity: 0,
        imp: AccessorImpl::Regime,
    },
];

/// Does `name` resolve to a regime (macro-series) accessor? Used by the panel
/// loader to decide whether it must also source the `REGIME_SYMBOLS` series.
pub fn is_regime_accessor(name: &str) -> bool {
    matches!(lookup(name).map(|d| d.imp), Some(AccessorImpl::Regime))
}

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
    /// The date-keyed regime composite, computed at most once per date. `Some(v)`
    /// where `v` is the total as f64, or `NaN` when the macro history is too thin.
    regime: Option<f64>,
    /// Number of regime computations (cache misses) — exposed so tests can prove
    /// the macro composite is computed only once even with several regime reads.
    pub regime_compute_count: usize,
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
    match def.imp {
        AccessorImpl::Cycle(kind) => {
            let symbol = &args[0];
            let tf = tf.unwrap_or(ctx.default_tf);
            let met = snapshot_met(kind, symbol, tf, ctx);
            Ok(match met {
                Some(m) => m as f64,
                None => f64::NAN, // insufficient history → comparison false → no fire
            })
        }
        // `regime_score()` is symbol-independent and date-keyed; `@tf` is
        // meaningless for it (regime is a daily cross-asset read) and ignored.
        AccessorImpl::Regime => Ok(regime_score_memo(ctx)),
    }
}

/// The memoized as-of regime composite total for `ctx.as_of`. Computed at most
/// once per date (date-keyed, symbol-independent) so multiple regime accessors in
/// one rule set share the work. Returns `NaN` when the macro history is too thin
/// to form a regime read (`active_count < 3`) → the rule does not fire.
fn regime_score_memo(ctx: &mut AtDateCtx) -> f64 {
    if let Some(v) = ctx.memo.regime {
        return v;
    }
    let score = regime_at_date(ctx.panel, ctx.as_of);
    let v = if score.has_data() {
        score.total as f64
    } else {
        f64::NAN
    };
    ctx.memo.regime = Some(v);
    ctx.memo.regime_compute_count += 1;
    v
}

/// **RegimeAtDate** — the as-of regime risk-on/off composite (POSITIONING-MODELS.md
/// §3.4). Builds the per-symbol macro history map from the panel trimmed to closes
/// with `date <= as_of`, and the "latest" price map from each series' last close
/// `<= as_of`, then feeds the existing [`compute_regime`]. Because every series is
/// truncated to `<= as_of` BEFORE `compute_regime` sees it, every signal (VIX
/// level/trend, yield trend, curve, DXY trend, gold/SPX, BTC/SPX corr, HY, copper/
/// gold) reads only data through `as_of`'s close — "latest" == "latest as of T", so
/// no signal can peek beyond `T`. Regime is a daily read, so the cutoff is `as_of`
/// itself (no completed-bucket lag).
pub fn regime_at_date(panel: &PricePanel, as_of: NaiveDate) -> RegimeScore {
    let mut prices: HashMap<String, rust_decimal::Decimal> = HashMap::new();
    let mut history: HashMap<String, Vec<HistoryRecord>> = HashMap::new();
    for &sym in REGIME_YAHOO_SYMBOLS {
        let closes = panel.closes_through(sym, as_of); // oldest-first, all <= as_of
        if closes.is_empty() {
            continue;
        }
        // "Latest" scalar price = the last close on/before as_of.
        if let Some((_, last)) = closes.last() {
            prices.insert(sym.to_string(), *last);
        }
        let recs: Vec<HistoryRecord> = closes
            .into_iter()
            .map(|(d, c)| HistoryRecord {
                date: d.format("%Y-%m-%d").to_string(),
                close: c,
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        history.insert(sym.to_string(), recs);
    }
    compute_regime(&prices, &history)
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

    // --- regime accessor ---------------------------------------------------

    /// Insert `n` daily bars for `sym` starting `start`, value = `f(i)`.
    fn fill(p: &mut PricePanel, sym: &str, start: NaiveDate, n: u64, f: impl Fn(u64) -> f64) {
        use rust_decimal::prelude::FromPrimitive;
        let series: Vec<(NaiveDate, Decimal)> = (0..n)
            .map(|i| {
                (
                    start + Days::new(i),
                    Decimal::from_f64(f(i)).unwrap().round_dp(4),
                )
            })
            .collect();
        p.insert_series(sym, series);
    }

    /// A risk-OFF macro panel: VIX high & rising, 10Y falling, curve inverted,
    /// DXY rising, gold/SPX rising, BTC/SPX anti-correlated, HY falling, Cu/Au
    /// falling. `n` daily bars from `start`. Engineered to score strongly negative.
    fn risk_off_panel(start: NaiveDate, n: u64) -> PricePanel {
        let mut p = PricePanel::new();
        fill(&mut p, "^VIX", start, n, |i| 25.0 + 0.10 * i as f64); // high, rising
        fill(&mut p, "^TNX", start, n, |i| 5.0 - 0.01 * i as f64); // falling
        fill(&mut p, "^IRX", start, n, |_| 6.0); // > TNX → inverted curve
        fill(&mut p, "DX-Y.NYB", start, n, |i| 100.0 + 0.05 * i as f64); // rising
        fill(&mut p, "GC=F", start, n, |i| 2000.0 + 1.0 * i as f64); // gold rising
        fill(&mut p, "^GSPC", start, n, |i| 4800.0 - 2.0 * i as f64); // SPX falling
        fill(&mut p, "BTC-USD", start, n, |i| 60000.0 + 10.0 * i as f64); // up vs SPX down → neg corr
        fill(&mut p, "HYG", start, n, |i| 80.0 - 0.05 * i as f64); // falling
        fill(&mut p, "LQD", start, n, |_| 110.0);
        fill(&mut p, "HG=F", start, n, |i| 4.5 - 0.005 * i as f64); // copper falling
        p
    }

    #[test]
    fn regime_accessor_insufficient_yields_nan() {
        // No macro series at all → regime has no data → NaN sentinel.
        let panel = PricePanel::new();
        let mut memo = Memo::new();
        let mut ctx = AtDateCtx {
            as_of: d(2024, 6, 12),
            panel: &panel,
            default_tf: SignalTimeframe::Monthly,
            memo: &mut memo,
        };
        let v = eval_accessor("regime_score", &[], None, &mut ctx).unwrap();
        assert!(v.is_nan(), "no macro history → regime must be the NaN sentinel");
    }

    #[test]
    fn regime_accessor_arity_zero_rejects_argument() {
        // A symbol arg to the zero-arity regime accessor is an error, not a silent
        // ignore (caught at eval time as well as at validate time).
        let panel = risk_off_panel(d(2024, 1, 1), 60);
        let mut memo = Memo::new();
        let mut ctx = AtDateCtx {
            as_of: d(2024, 2, 20),
            panel: &panel,
            default_tf: SignalTimeframe::Monthly,
            memo: &mut memo,
        };
        let err = eval_accessor("regime_score", &["SPY".to_string()], None, &mut ctx)
            .unwrap_err()
            .to_string();
        assert!(err.contains("takes 0 argument"), "got: {err}");
    }

    #[test]
    fn regime_score_is_negative_on_risk_off_panel() {
        let panel = risk_off_panel(d(2024, 1, 1), 60);
        let score = regime_at_date(&panel, d(2024, 2, 20));
        assert!(score.has_data(), "60 bars must yield an active regime read");
        assert!(
            score.total <= -2,
            "engineered risk-off panel must score <= -2, got {}",
            score.total
        );
    }

    #[test]
    fn regime_memo_computes_once_per_date() {
        let panel = risk_off_panel(d(2024, 1, 1), 60);
        let mut memo = Memo::new();
        let mut ctx = AtDateCtx {
            as_of: d(2024, 2, 20),
            panel: &panel,
            default_tf: SignalTimeframe::Monthly,
            memo: &mut memo,
        };
        let a = eval_accessor("regime_score", &[], None, &mut ctx).unwrap();
        let b = eval_accessor("regime_score", &[], None, &mut ctx).unwrap();
        assert_eq!(a, b);
        assert_eq!(ctx.memo.regime_compute_count, 1, "regime computed exactly once");
    }

    /// Lookahead guard: the regime score at `T` must NOT change when strictly-future
    /// macro bars are appended. This is the as-of invariance that proves no signal
    /// peeks beyond `T` (every series is trimmed to `<= T` before `compute_regime`).
    #[test]
    fn regime_at_date_is_future_data_invariant() {
        let t = d(2024, 2, 20);
        let panel = risk_off_panel(d(2024, 1, 1), 51); // bars through 2024-02-20
        let before = regime_at_date(&panel, t).total;

        // Append 40 strictly-future bars (continuing the risk-off trend AND, for
        // good measure, flipping some series so a peek would visibly change T).
        let mut panel2 = panel.clone();
        let future_start = d(2024, 2, 21);
        fill(&mut panel2, "^VIX", future_start, 40, |i| 12.0 - 0.05 * i as f64); // would flip low/falling
        fill(&mut panel2, "^TNX", future_start, 40, |i| 3.0 + 0.02 * i as f64); // would flip rising
        fill(&mut panel2, "DX-Y.NYB", future_start, 40, |i| 105.0 - 0.05 * i as f64); // would flip falling
        let after = regime_at_date(&panel2, t).total;

        assert_eq!(
            before, after,
            "regime at T changed after appending future bars → lookahead leak"
        );
    }
}
