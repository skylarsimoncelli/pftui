//! The daily-loop simulator (POSITIONING-MODELS.md §3.3).
//!
//! Calendar = union of all symbols' trading days. For each date `T`, oldest→
//! newest: (1) execute any fills scheduled for `T`; (2) MARK every position at
//! `T`'s close (carrying the last-known close for a held symbol that does not
//! trade on `T`, flagged non-tradable); (3) on a cadence date, run the bounded-
//! projection solver, split class weights to symbols equal-weight, and **schedule**
//! each order to fill at that symbol's next tradable close (`fill = next_close`).
//!
//! Money — cash, quantities, fill prices, commissions — is `Decimal`. The f64
//! metrics are derived from the daily curve *after* the ledger is closed.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::actions::{resolve_targets, EvalContext, Rule, SignalEnv, TargetResolution};
use super::metrics::{self, PortfolioMetrics};
use super::solver::{solve_targets, SolveBucket, SolveOutcome};
use super::{
    AssetSpec, CashYield, ClassTarget, PortfolioModel, PricePanel, RebalanceBandMode,
    RebalanceCadence, WithinClass,
};
use crate::analytics::cycle_signals::SignalTimeframe;

/// Default timeframe for a signal accessor written without an explicit `@tf`.
/// The cycle-confluence suites are tuned for the monthly cycle read (matching
/// the rest of pftui's cycle tooling), so an unqualified `cycle_bottom_met(...)`
/// reads the monthly snapshot.
const SIGNAL_DEFAULT_TF: SignalTimeframe = SignalTimeframe::Monthly;

/// One marked-to-market point on the daily equity curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyEquityPoint {
    pub date: NaiveDate,
    pub equity: Decimal,
    pub cash: Decimal,
    pub invested: Decimal,
    /// Drawdown vs the running peak, in percent (≤ 0, exact Decimal).
    pub drawdown_pct: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

/// A filled order, recorded on its **decision** date but carrying the realized
/// fill numbers from its (later) fill date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub symbol: String,
    pub side: Side,
    /// Quantity in **native** shares of the instrument.
    pub qty: Decimal,
    pub fill_date: NaiveDate,
    /// Fill price per share in the instrument's **native** currency (slipped).
    pub fill_price: Decimal,
    /// Currency the instrument is priced in.
    pub price_currency: String,
    /// FX rate native→base applied at the **fill date** (1 for base-currency assets).
    pub fx_rate: Decimal,
    /// Signed traded notional in the instrument's **native** currency.
    pub notional_native: Decimal,
    /// Signed traded notional converted to **base** currency (qty × fill_price ×
    /// fx, +buy / −sell). This is what hits the cash ledger.
    pub notional: Decimal,
    /// Commission in **base** currency.
    pub commission: Decimal,
}

/// A rebalance decision. `orders` fill at later dates; `pre_weights` are the
/// marked symbol weights at the decision; `post_weights` are the target weights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RebalanceEvent {
    pub date: NaiveDate,
    pub orders: Vec<Order>,
    /// Σ|traded notional| / equity at decision, in percent.
    pub turnover_pct: Decimal,
    pub total_cost: Decimal,
    pub pre_weights: Vec<(String, Decimal)>,
    pub post_weights: Vec<(String, Decimal)>,
    pub infeasible: bool,
    /// Symbols whose leg could NOT trade on this rebalance because they were
    /// non-tradable on `date` (missing close). The rest of the rebalance still
    /// executes (cash-only / partial); deferred legs fabricate **no** fill.
    pub deferred_legs: Vec<String>,
    /// Symbols whose leg WAS tradable on `date` but had **no future close** to
    /// fill against (the order would have nothing to settle on), so the order
    /// was dropped. Records the silent-drop observability gap at
    /// `panel.next_tradable() == None`. (The `post_weights` reconciliation for
    /// these residuals is a noted TODO — see the post_weights NOTE below.)
    pub dropped_legs: Vec<String>,
    /// Ids of the P3a rules that fired and were applied at this rebalance (in
    /// apply order). Empty for a rule-free rebalance.
    #[serde(default)]
    pub applied_rule_ids: Vec<String>,
}

/// Push the standard "infeasible solve → hold prior weights" event.
fn push_infeasible_event(
    events: &mut Vec<RebalanceEvent>,
    t: NaiveDate,
    pre_weights: Vec<(String, Decimal)>,
    applied_rule_ids: Vec<String>,
) {
    events.push(RebalanceEvent {
        date: t,
        orders: vec![],
        turnover_pct: dec!(0),
        total_cost: dec!(0),
        pre_weights,
        post_weights: vec![],
        infeasible: true,
        deferred_legs: vec![],
        dropped_legs: vec![],
        applied_rule_ids,
    });
}

/// The portfolio backtest report. Money is `Decimal`; metrics are f64 from the
/// daily curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioBacktestReport {
    pub daily_equity_curve: Vec<DailyEquityPoint>,
    pub rebalance_events: Vec<RebalanceEvent>,
    pub cagr_pct: f64,
    /// Max drawdown as a positive magnitude in percent.
    pub max_drawdown_pct: f64,
    pub ann_vol_pct: f64,
    pub time_in_cash_pct: f64,
    pub avg_turnover_pct_per_yr: f64,
    pub total_costs: Decimal,
    pub n_rebalances: usize,
    /// Full daily-curve metric block (P1). The flat fields above are retained for
    /// P0 compatibility and equal the matching `metrics` fields.
    pub metrics: PortfolioMetrics,
    /// The three reference curves, run through the same simulator path.
    pub benchmarks: Benchmarks,
}

/// One benchmark run: its own daily curve + metrics over the same calendar/costs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub daily_equity_curve: Vec<DailyEquityPoint>,
    pub metrics: PortfolioMetrics,
}

/// The three benchmarks (POSITIONING-MODELS.md §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Benchmarks {
    /// Buy the base-policy target weights once at the start, never rebalance.
    pub static_base_policy: BenchmarkResult,
    /// Base-policy weights rebalanced on the same cadence + cost model, no rules.
    /// (With P1 having no rules, this equals the main result — isolates
    /// rule-alpha from rebalance-harvesting once rules land in P3.)
    pub rebalanced_base_policy: BenchmarkResult,
    /// Equal weight across the non-cash universe symbols, rebalanced on cadence.
    pub equal_weight: BenchmarkResult,
}

/// Internal per-symbol holding (average-cost, no tax lots — mirrors
/// `models/position.rs`).
#[derive(Debug, Clone, Default)]
struct Holding {
    qty: Decimal,
    total_cost: Decimal,
}

/// An order scheduled at decision time, to be executed at `fill_date`. Carries a
/// back-reference to the [`RebalanceEvent`]/order slot so the realized fill can be
/// written back.
#[derive(Debug, Clone)]
struct PendingOrder {
    symbol: String,
    side: Side,
    /// Desired |notional| to trade at the fill, in **base** currency, pre
    /// cash-shortfall scaling.
    notional_abs: Decimal,
    fill_date: NaiveDate,
    /// Native-currency slipped fill price per share.
    fill_price: Decimal,
    /// Instrument's price currency (for fill-date FX resolution).
    price_currency: String,
    event_idx: usize,
    order_idx: usize,
}

const CASH_KEY: &str = "CASH";

/// Knobs the benchmark variants flip without touching the model semantics.
#[derive(Debug, Clone, Copy, Default)]
struct SimOptions {
    /// Stop generating new rebalances after this many cadence decisions (the
    /// `static_base_policy` benchmark buys once at the start, then holds).
    max_rebalances: Option<usize>,
    /// Evaluate `model.rules` through the P3a action algebra. The three
    /// benchmarks run with this OFF (base_policy only) so rule-alpha is isolated
    /// from rebalance-harvesting (POSITIONING-MODELS.md §3.3).
    apply_rules: bool,
}

/// One simulator run's raw output (before metrics/benchmarks).
struct SimRun {
    curve: Vec<DailyEquityPoint>,
    events: Vec<RebalanceEvent>,
    total_costs: Decimal,
}

/// Run the P1 simulation: main result + the three benchmarks + full metrics.
pub fn simulate(model: &PortfolioModel, panel: &PricePanel) -> Result<PortfolioBacktestReport> {
    // Main result = base policy + the P3a action algebra (rules applied).
    let main = run_sim(
        model,
        panel,
        SimOptions {
            max_rebalances: None,
            apply_rules: true,
        },
    )?;

    // Benchmark 1: static base policy — buy targets once, never rebalance, no rules.
    let static_run = run_sim(
        model,
        panel,
        SimOptions {
            max_rebalances: Some(1),
            apply_rules: false,
        },
    )?;
    // Benchmark 2: base policy rebalanced on cadence with NO rules — isolates
    // rule-alpha from rebalance-harvesting. Equals `main` exactly when the model
    // declares no rules (the rule-free path is byte-identical).
    let rebal_run = run_sim(model, panel, SimOptions::default())?;
    // Benchmark 3: equal weight across the non-cash universe, same cadence/costs.
    let eq_model = equal_weight_model(model);
    let eq_run = run_sim(&eq_model, panel, SimOptions::default())?;

    let benchmarks = Benchmarks {
        static_base_policy: benchmark_result(static_run),
        rebalanced_base_policy: benchmark_result(rebal_run),
        equal_weight: benchmark_result(eq_run),
    };

    let report = finalize(main.curve, main.events, main.total_costs, benchmarks);
    Ok(report)
}

fn benchmark_result(run: SimRun) -> BenchmarkResult {
    let metrics = metrics::compute(&run.curve, &run.events, run.total_costs);
    BenchmarkResult {
        daily_equity_curve: run.curve,
        metrics,
    }
}

/// Derive the equal-weight benchmark model: each non-cash universe symbol becomes
/// its own class at weight `1/N`, cash pinned to 0. Same cadence/cost/fill/FX/
/// cash-yield as the source model.
fn equal_weight_model(model: &PortfolioModel) -> PortfolioModel {
    let non_cash: Vec<&AssetSpec> = model
        .universe
        .iter()
        .filter(|a| a.class != model.cash_class)
        .collect();
    let n = non_cash.len();
    let mut universe = Vec::with_capacity(n);
    let mut targets = Vec::with_capacity(n + 1);
    if n > 0 {
        let w = Decimal::ONE / Decimal::from(n as u64);
        for a in &non_cash {
            let class = format!("eq::{}", a.symbol);
            universe.push(AssetSpec::with_currency(
                a.symbol.clone(),
                class.clone(),
                a.price_currency.clone(),
            ));
            targets.push(ClassTarget::new(class, w, dec!(0), dec!(1)));
        }
    }
    targets.push(ClassTarget::new(model.cash_class.clone(), dec!(0), dec!(0), dec!(1)));
    PortfolioModel {
        base_currency: model.base_currency.clone(),
        initial_capital: model.initial_capital,
        universe,
        cash_class: model.cash_class.clone(),
        targets,
        within_class: model.within_class,
        rebalance_cadence: model.rebalance_cadence,
        rebalance_band_mode: model.rebalance_band_mode,
        fill: model.fill,
        commission_pct: model.commission_pct,
        slippage_pct: model.slippage_pct,
        cash_yield: model.cash_yield.clone(),
        max_position: None,
        rules: vec![],
    }
}

/// Core daily loop. Returns the raw curve/events/costs; metrics & benchmarks are
/// layered on by [`simulate`].
fn run_sim(model: &PortfolioModel, panel: &PricePanel, opts: SimOptions) -> Result<SimRun> {
    let calendar = panel.calendar();
    let base = model.base_currency.as_str();

    // Native price currency per symbol, for FX conversion of marks & fills.
    let sym_ccy: HashMap<String, String> = model
        .universe
        .iter()
        .map(|a| (a.symbol.clone(), a.price_currency.clone()))
        .collect();

    let mut cash = model.initial_capital;
    let mut holdings: HashMap<String, Holding> = HashMap::new();
    let mut last_close: HashMap<String, Decimal> = HashMap::new();

    let mut curve: Vec<DailyEquityPoint> = Vec::with_capacity(calendar.len());
    let mut events: Vec<RebalanceEvent> = Vec::new();
    let mut pending: BTreeMap<NaiveDate, Vec<PendingOrder>> = BTreeMap::new();
    let mut total_costs = dec!(0);
    let mut peak = model.initial_capital;

    // Pre-group universe symbols by class for the equal-weight split.
    let mut class_symbols: HashMap<String, Vec<String>> = HashMap::new();
    for a in &model.universe {
        class_symbols
            .entry(a.class.clone())
            .or_default()
            .push(a.symbol.clone());
    }

    let mut seen_week: std::collections::BTreeSet<(i32, u32)> = std::collections::BTreeSet::new();
    let mut seen_month: std::collections::BTreeSet<(i32, u32)> = std::collections::BTreeSet::new();
    // Independent cadence-boundary tracking for per-rule cadence gating: a rule
    // fires only on the FIRST model-rebalance of its own cadence bucket. Computed
    // ONCE per rebalance (shared across all rules of a cadence) so two rules of the
    // same cadence both fire on the same boundary.
    let mut rule_seen_week: std::collections::BTreeSet<(i32, u32)> =
        std::collections::BTreeSet::new();
    let mut rule_seen_month: std::collections::BTreeSet<(i32, u32)> =
        std::collections::BTreeSet::new();
    // Cash-yield proxy: last seen close, to compound the daily return.
    let mut prev_proxy: Option<Decimal> = None;
    let mut n_decisions = 0usize;

    for &t in &calendar {
        // --- 0. Cash-yield accrual on the balance carried into T (before fills),
        //        using the proxy's daily return. Default `None` → no-op. ---
        if let CashYield::Proxy(sym) = &model.cash_yield {
            if let Some(c) = panel.close_on(sym, t) {
                if let Some(p) = prev_proxy {
                    if p > dec!(0) {
                        cash += cash * (c / p - dec!(1));
                    }
                }
                prev_proxy = Some(c);
            }
        }

        // --- 1. Execute fills scheduled for T (sells settle first, then buys,
        //        scaling buys down if cash is short). FX resolved at T. ---
        if let Some(orders) = pending.remove(&t) {
            execute_fills(
                orders,
                model,
                panel,
                base,
                &mut cash,
                &mut holdings,
                &mut events,
                &mut total_costs,
            );
        }

        // --- 2. Refresh last-known native close for symbols trading on T. ---
        for a in &model.universe {
            if let Some(c) = panel.close_on(&a.symbol, t) {
                last_close.insert(a.symbol.clone(), c);
            }
        }

        // --- 3. MARK every held position at T's close, FX-converted to base
        //        exactly once at FX@T (carry last-known native for non-tradable
        //        held symbols). ---
        let mut invested = dec!(0);
        for (sym, h) in &holdings {
            if h.qty == dec!(0) {
                continue;
            }
            if let Some(px) = last_close.get(sym) {
                let ccy = sym_ccy.get(sym).map(|s| s.as_str()).unwrap_or(base);
                let fx = panel.fx_rate(ccy, base, t).unwrap_or(Decimal::ONE);
                invested += h.qty * *px * fx;
            }
        }
        let equity = cash + invested;
        if equity > peak {
            peak = equity;
        }
        let drawdown_pct = if peak > dec!(0) {
            ((equity / peak) - dec!(1)) * dec!(100)
        } else {
            dec!(0)
        };
        curve.push(DailyEquityPoint {
            date: t,
            equity,
            cash,
            invested,
            drawdown_pct: drawdown_pct.round_dp(8),
        });

        // --- 4. Cadence decision. ---
        if !is_rebalance_date(t, model, &mut seen_week, &mut seen_month) {
            continue;
        }
        if let Some(max) = opts.max_rebalances {
            if n_decisions >= max {
                continue;
            }
        }
        // Per-rule cadence boundary flags, consumed once per rebalance.
        let iso = t.iso_week();
        let is_week_boundary = rule_seen_week.insert((iso.year(), iso.week()));
        let is_month_boundary = rule_seen_month.insert((t.year(), t.month()));
        let rebalance_index = n_decisions;
        n_decisions += 1;

        // Collect the rules that FIRE at T: `when` true AND the rule's own
        // cadence boundary coincides with T. Benchmarks pass `apply_rules = false`.
        // A FRESH signal env (with a fresh snapshot memo) is built per rebalance
        // date so a `(sym, tf)` signal is computed once and never leaks across
        // dates. Cadence is checked first so a rule off its boundary never even
        // computes a signal.
        let fired: Vec<Rule> = if opts.apply_rules && !model.rules.is_empty() {
            let mut ctx = EvalContext {
                date: t,
                rebalance_index,
                signal: Some(SignalEnv::new(panel, SIGNAL_DEFAULT_TF)),
            };
            let mut out = Vec::new();
            for r in &model.rules {
                if !cadence_active(r.cadence, is_week_boundary, is_month_boundary) {
                    continue;
                }
                if r.when.eval(&mut ctx)? {
                    out.push(r.clone());
                }
            }
            out
        } else {
            Vec::new()
        };

        decide_rebalance(
            t,
            model,
            panel,
            base,
            &sym_ccy,
            equity,
            &holdings,
            &last_close,
            &class_symbols,
            cash,
            &fired,
            &mut events,
            &mut pending,
        )?;
    }

    Ok(SimRun {
        curve,
        events,
        total_costs,
    })
}

/// Is `t` a cadence date? Weekly = first calendar date seen in its ISO week;
/// monthly = first date seen in its (year, month). Documented choice: **first**
/// trading day of the bucket.
fn is_rebalance_date(
    t: NaiveDate,
    model: &PortfolioModel,
    seen_week: &mut std::collections::BTreeSet<(i32, u32)>,
    seen_month: &mut std::collections::BTreeSet<(i32, u32)>,
) -> bool {
    match model.rebalance_cadence {
        super::RebalanceCadence::Weekly => {
            let iso = t.iso_week();
            seen_week.insert((iso.year(), iso.week()))
        }
        super::RebalanceCadence::Monthly => seen_month.insert((t.year(), t.month())),
    }
}

/// Does a rule of `cadence` fire on THIS model-rebalance? A weekly rule fires on
/// every model-rebalance that opens a new ISO week; a monthly rule only on the
/// first model-rebalance of each calendar month. (A rule whose cadence is finer
/// than the model's fires every model-rebalance.)
fn cadence_active(cadence: RebalanceCadence, is_week_boundary: bool, is_month_boundary: bool) -> bool {
    match cadence {
        RebalanceCadence::Weekly => is_week_boundary,
        RebalanceCadence::Monthly => is_month_boundary,
    }
}

/// Marked **base-currency** value of a held symbol at FX@T (0 if flat/unknown).
fn marked_base_value(
    sym: &str,
    holdings: &HashMap<String, Holding>,
    last_close: &HashMap<String, Decimal>,
    sym_ccy: &HashMap<String, String>,
    base: &str,
    panel: &PricePanel,
    t: NaiveDate,
) -> Decimal {
    holdings
        .get(sym)
        .filter(|h| h.qty != dec!(0))
        .and_then(|h| {
            last_close.get(sym).map(|px| {
                let ccy = sym_ccy.get(sym).map(|s| s.as_str()).unwrap_or(base);
                let fx = panel.fx_rate(ccy, base, t).unwrap_or(Decimal::ONE);
                h.qty * *px * fx
            })
        })
        .unwrap_or(dec!(0))
}

#[allow(clippy::too_many_arguments)]
fn decide_rebalance(
    t: NaiveDate,
    model: &PortfolioModel,
    panel: &PricePanel,
    base: &str,
    sym_ccy: &HashMap<String, String>,
    equity: Decimal,
    holdings: &HashMap<String, Holding>,
    last_close: &HashMap<String, Decimal>,
    class_symbols: &HashMap<String, Vec<String>>,
    cash: Decimal,
    fired: &[Rule],
    events: &mut Vec<RebalanceEvent>,
    pending: &mut BTreeMap<NaiveDate, Vec<PendingOrder>>,
) -> Result<()> {
    // Symbols tradable on the decision date T (have a close at T). The signal
    // rule path splits each class budget across these only, so a non-tradable
    // symbol's share flows to its tradable peers (consistent with the rule-free
    // equal-split below), never silently to cash.
    let tradable: BTreeSet<String> = model
        .universe
        .iter()
        .filter(|a| panel.close_on(&a.symbol, t).is_some())
        .map(|a| a.symbol.clone())
        .collect();

    // Current marked weights per symbol (+ CASH), for pre_weights.
    let mut pre_weights: Vec<(String, Decimal)> = Vec::new();
    let mut current_symbol_weights: BTreeMap<String, Decimal> = BTreeMap::new();
    for a in &model.universe {
        let v = marked_base_value(&a.symbol, holdings, last_close, sym_ccy, base, panel, t);
        let w = if equity > dec!(0) {
            (v / equity).round_dp(8)
        } else {
            dec!(0)
        };
        pre_weights.push((a.symbol.clone(), w));
        current_symbol_weights.insert(a.symbol.clone(), w);
    }
    pre_weights.push((
        CASH_KEY.to_string(),
        if equity > dec!(0) {
            (cash / equity).round_dp(8)
        } else {
            dec!(0)
        },
    ));

    // Compute the desired class weights (and, when rules fire, per-symbol target
    // overrides) via the P3a action algebra. The rule-FREE path is byte-identical
    // to the old direct-solve equal-split: `resolve_targets` with no rules makes
    // the same `solve_targets` call on the same base_policy buckets, and the
    // engine keeps its tradable equal-split for the untouched symbols.
    let mut applied_rule_ids: Vec<String> = Vec::new();
    let (class_weight, symbol_targets): (HashMap<String, Decimal>, Option<BTreeMap<String, Decimal>>) =
        if fired.is_empty() {
            // OLD path verbatim — proven byte-identical by the 36 existing tests.
            let buckets: Vec<SolveBucket> = model
                .targets
                .iter()
                .map(|ct| SolveBucket::new(ct.class.clone(), ct.target, ct.floor, ct.ceiling))
                .collect();
            let solved = match solve_targets(&buckets)? {
                SolveOutcome::Infeasible => {
                    push_infeasible_event(events, t, pre_weights, applied_rule_ids);
                    return Ok(());
                }
                SolveOutcome::Solved(w) => w,
            };
            let cw = model
                .targets
                .iter()
                .map(|ct| ct.class.clone())
                .zip(solved.iter().copied())
                .collect();
            (cw, None)
        } else {
            match resolve_targets(model, &current_symbol_weights, &tradable, fired)? {
                TargetResolution::Infeasible {
                    applied_rule_ids: ids,
                    ..
                } => {
                    push_infeasible_event(events, t, pre_weights, ids);
                    return Ok(());
                }
                TargetResolution::Resolved {
                    class_weights,
                    symbol_weights,
                    applied_rule_ids: ids,
                    ..
                } => {
                    applied_rule_ids = ids;
                    let cw: HashMap<String, Decimal> = class_weights.into_iter().collect();
                    (cw, Some(symbol_weights))
                }
            }
        };

    // Split each non-cash class weight equally across its TRADABLE symbols and
    // build orders = desired − current notional.
    debug_assert!(matches!(model.within_class, WithinClass::Equal));
    debug_assert!(matches!(model.rebalance_band_mode, RebalanceBandMode::ToTarget));

    let mut orders: Vec<Order> = Vec::new();
    let mut new_pending: Vec<PendingOrder> = Vec::new();
    let mut deferred_legs: Vec<String> = Vec::new();
    let mut dropped_legs: Vec<String> = Vec::new();
    let mut turnover_notional = dec!(0);
    let mut post_weights: Vec<(String, Decimal)> = Vec::new();
    let mut deployed_weight = dec!(0);
    let event_idx = events.len();

    for a in &model.universe {
        if a.class == model.cash_class {
            continue;
        }
        let w_class = class_weight.get(&a.class).copied().unwrap_or(dec!(0));
        // Tradable symbols in this class on date T.
        let tradable: Vec<&String> = class_symbols
            .get(&a.class)
            .map(|syms| {
                syms.iter()
                    .filter(|s| panel.close_on(s, t).is_some())
                    .collect()
            })
            .unwrap_or_default();
        let n = Decimal::from(tradable.len());
        // Current marked base-currency value of this leg (FX@T).
        let current_notional =
            marked_base_value(&a.symbol, holdings, last_close, sym_ccy, base, panel, t);
        let target_w = if tradable.contains(&&a.symbol) && n > dec!(0) {
            // Rule-driven path: use the resolved per-symbol target (which already
            // applied the within-class split + symbol overrides + max_position).
            // Rule-free path: the original tradable equal-split (byte-identical).
            match &symbol_targets {
                Some(st) => st.get(&a.symbol).copied().unwrap_or(dec!(0)),
                None => w_class / n,
            }
        } else {
            // Non-tradable on T: NEVER fabricate a fill. Keep the current weight
            // and DEFER the leg (the rest of the rebalance still executes).
            let cur = current_notional;
            let w = if equity > dec!(0) { cur / equity } else { dec!(0) };
            post_weights.push((a.symbol.clone(), w.round_dp(8)));
            deployed_weight += w;
            let held = holdings.get(&a.symbol).map(|h| h.qty != dec!(0)).unwrap_or(false);
            if w_class > dec!(0) || held {
                // A trade was wanted but the leg can't trade today → deferred.
                deferred_legs.push(a.symbol.clone());
            }
            continue;
        };
        post_weights.push((a.symbol.clone(), target_w.round_dp(8)));
        deployed_weight += target_w;

        // Order is sized in BASE currency: desired − current marked base value.
        let desired_notional = target_w * equity;
        let order_notional = desired_notional - current_notional;
        if order_notional == dec!(0) {
            continue;
        }
        let (next_date, next_close) = match panel.next_tradable(&a.symbol, t) {
            Some(v) => v,
            None => {
                // No future close to fill against — drop the leg. Record it so
                // the silent-drop is observable instead of vanishing.
                dropped_legs.push(a.symbol.clone());
                continue;
            }
        };
        let side = if order_notional > dec!(0) {
            Side::Buy
        } else {
            Side::Sell
        };
        // Native-currency slipped fill price (FX is applied at the fill date).
        let fill_price = match side {
            Side::Buy => next_close * (dec!(1) + model.slippage_pct),
            Side::Sell => next_close * (dec!(1) - model.slippage_pct),
        };
        let notional_abs = order_notional.abs(); // base currency
        turnover_notional += notional_abs;
        let order_idx = orders.len();
        let ccy = sym_ccy
            .get(&a.symbol)
            .cloned()
            .unwrap_or_else(|| base.to_string());
        // Intent record (overwritten with realized fill-date FX numbers at fill).
        // Estimate native qty using FX@T just so the pre-fill record is sane.
        let fx_est = panel.fx_rate(&ccy, base, t).unwrap_or(Decimal::ONE);
        let fill_price_base_est = fill_price * fx_est;
        let qty_intent = if fill_price_base_est != dec!(0) {
            notional_abs / fill_price_base_est
        } else {
            dec!(0)
        };
        orders.push(Order {
            symbol: a.symbol.clone(),
            side,
            qty: qty_intent,
            fill_date: next_date,
            fill_price,
            price_currency: ccy.clone(),
            fx_rate: fx_est,
            notional_native: qty_intent * fill_price,
            notional: order_notional,
            commission: model.commission_pct * notional_abs,
        });
        new_pending.push(PendingOrder {
            symbol: a.symbol.clone(),
            side,
            notional_abs,
            fill_date: next_date,
            fill_price,
            price_currency: ccy,
            event_idx,
            order_idx,
        });
    }

    // NOTE: `post_weights` is a *reporting* field of intended targets, not ledger
    // truth. When a leg is deferred (non-tradable) or dropped (no future close),
    // the residual lands in this CASH figure and can OVERSTATE cash vs. what the
    // realized fills produce. The ledger (cash/holdings) is computed independently
    // at fill time and is the source of truth — post_weights is never read back.
    post_weights.push((CASH_KEY.to_string(), (dec!(1) - deployed_weight).round_dp(8)));

    let turnover_pct = if equity > dec!(0) {
        (turnover_notional / equity * dec!(100)).round_dp(8)
    } else {
        dec!(0)
    };
    let est_cost: Decimal = orders.iter().map(|o| o.commission).sum();

    events.push(RebalanceEvent {
        date: t,
        orders,
        turnover_pct,
        total_cost: est_cost,
        pre_weights,
        post_weights,
        infeasible: false,
        deferred_legs,
        dropped_legs,
        applied_rule_ids,
    });
    for p in new_pending {
        pending.entry(p.fill_date).or_default().push(p);
    }
    Ok(())
}

/// Execute one fill date's batch: sells settle first (adding cash), then buys are
/// scaled down proportionally if cash is short. Realized fill numbers are written
/// back into the originating [`RebalanceEvent`].
#[allow(clippy::too_many_arguments)]
fn execute_fills(
    mut orders: Vec<PendingOrder>,
    model: &PortfolioModel,
    panel: &PricePanel,
    base: &str,
    cash: &mut Decimal,
    holdings: &mut HashMap<String, Holding>,
    events: &mut [RebalanceEvent],
    total_costs: &mut Decimal,
) {
    // Deterministic order: by event then order index.
    orders.sort_by_key(|o| (o.event_idx, o.order_idx));

    // FX native→base resolved at the FILL date (not the decision date). For
    // base-currency assets this is 1, so the per-share base price == native.
    let fill_px_base = |o: &PendingOrder| -> (Decimal, Decimal) {
        let fx = panel
            .fx_rate(&o.price_currency, base, o.fill_date)
            .unwrap_or(Decimal::ONE);
        (o.fill_price * fx, fx)
    };

    // Sells first. `qty` is NATIVE shares; cash settles in BASE.
    for o in orders.iter().filter(|o| matches!(o.side, Side::Sell)) {
        let (px_base, fx) = fill_px_base(o);
        let h = holdings.entry(o.symbol.clone()).or_default();
        // Native shares implied by the desired BASE sell notional, clamped.
        let mut qty = if px_base != dec!(0) {
            o.notional_abs / px_base
        } else {
            dec!(0)
        };
        if qty > h.qty {
            qty = h.qty;
        }
        let notional_base = qty * px_base; // base proceeds
        let commission = model.commission_pct * notional_base;
        // Reduce average-cost basis (base) proportionally.
        if h.qty > dec!(0) {
            let avg = h.total_cost / h.qty;
            h.qty -= qty;
            h.total_cost = avg * h.qty;
        }
        *cash += notional_base - commission;
        *total_costs += commission;
        write_back(events, o, qty, notional_base, commission, fx, Side::Sell);
    }

    // Buys: scale down proportionally if total BASE need exceeds available cash.
    let buys: Vec<&PendingOrder> = orders.iter().filter(|o| matches!(o.side, Side::Buy)).collect();
    let total_need: Decimal = buys
        .iter()
        .map(|o| o.notional_abs * (dec!(1) + model.commission_pct))
        .sum();
    let scale = if total_need > *cash && total_need > dec!(0) {
        *cash / total_need
    } else {
        dec!(1)
    };
    for o in buys {
        let (px_base, fx) = fill_px_base(o);
        let notional_base = o.notional_abs * scale;
        let commission = model.commission_pct * notional_base;
        let qty = if px_base != dec!(0) {
            notional_base / px_base
        } else {
            dec!(0)
        };
        let h = holdings.entry(o.symbol.clone()).or_default();
        h.qty += qty;
        h.total_cost += notional_base; // cost basis in BASE at the slipped fill
        *cash -= notional_base + commission;
        *total_costs += commission;
        write_back(events, o, qty, notional_base, commission, fx, Side::Buy);
    }
}

fn write_back(
    events: &mut [RebalanceEvent],
    o: &PendingOrder,
    qty: Decimal,
    notional_base: Decimal,
    commission: Decimal,
    fx: Decimal,
    side: Side,
) {
    if let Some(ev) = events.get_mut(o.event_idx) {
        if let Some(ord) = ev.orders.get_mut(o.order_idx) {
            ord.qty = qty;
            ord.fx_rate = fx;
            let notional_native = qty * o.fill_price;
            ord.notional_native = match side {
                Side::Buy => notional_native,
                Side::Sell => -notional_native,
            };
            ord.notional = match side {
                Side::Buy => notional_base,
                Side::Sell => -notional_base,
            };
            ord.commission = commission;
        }
        // Keep the event's recorded total_cost consistent with realized fills.
        ev.total_cost = ev.orders.iter().map(|x| x.commission).sum();
    }
}

/// Derive the metric block from the closed daily curve and assemble the report.
/// The flat P0 fields are kept and sourced from the same [`PortfolioMetrics`].
fn finalize(
    curve: Vec<DailyEquityPoint>,
    events: Vec<RebalanceEvent>,
    total_costs: Decimal,
    benchmarks: Benchmarks,
) -> PortfolioBacktestReport {
    let m = metrics::compute(&curve, &events, total_costs);
    PortfolioBacktestReport {
        daily_equity_curve: curve,
        rebalance_events: events,
        cagr_pct: m.cagr_pct,
        max_drawdown_pct: m.max_drawdown_pct,
        ann_vol_pct: m.ann_vol_pct,
        time_in_cash_pct: m.time_in_cash_pct,
        avg_turnover_pct_per_yr: m.avg_turnover_pct_per_yr,
        total_costs: m.total_costs,
        n_rebalances: m.n_rebalances,
        metrics: m,
        benchmarks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::portfolio_sim::{
        AssetSpec, CashYield, ClassTarget, FillMode, PortfolioModel, RebalanceCadence,
    };

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// Hand-checkable ledger: 2 assets + cash, one weekly rebalance, flat prices,
    /// 0 slippage, 0.1% commission, $100,000 start.
    #[test]
    fn hand_computed_ledger() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![
                AssetSpec::new("AAA", "alpha"),
                AssetSpec::new("BBB", "beta"),
            ],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.2), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(0.4), dec!(0), dec!(1)),
                ClassTarget::new("beta", dec!(0.4), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0.001),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        // All dates 2024-01-01..05 fall in ISO week 1 → exactly one rebalance.
        let mut panel = PricePanel::new();
        let aaa = vec![
            (d(2024, 1, 1), dec!(100)),
            (d(2024, 1, 2), dec!(100)),
            (d(2024, 1, 3), dec!(100)),
            (d(2024, 1, 4), dec!(100)),
            (d(2024, 1, 5), dec!(100)),
        ];
        let bbb = vec![
            (d(2024, 1, 1), dec!(50)),
            (d(2024, 1, 2), dec!(50)),
            (d(2024, 1, 3), dec!(50)),
            (d(2024, 1, 4), dec!(50)),
            (d(2024, 1, 5), dec!(50)),
        ];
        panel.insert_series("AAA", aaa);
        panel.insert_series("BBB", bbb);

        let rep = simulate(&model, &panel).unwrap();

        // Exactly one rebalance, decided Jan 1, fills Jan 2.
        assert_eq!(rep.n_rebalances, 1);
        let ev = &rep.rebalance_events[0];
        assert_eq!(ev.date, d(2024, 1, 1));
        assert!(!ev.infeasible);
        assert_eq!(ev.orders.len(), 2);
        for o in &ev.orders {
            // NEXT-close fill timing, not the decision date.
            assert_eq!(o.fill_date, d(2024, 1, 2));
        }
        // AAA: $40,000 at $100 → 400 shares, $40 commission.
        let aaa_o = ev.orders.iter().find(|o| o.symbol == "AAA").unwrap();
        assert_eq!(aaa_o.qty, dec!(400));
        assert_eq!(aaa_o.commission, dec!(40));
        // BBB: $40,000 at $50 → 800 shares, $40 commission.
        let bbb_o = ev.orders.iter().find(|o| o.symbol == "BBB").unwrap();
        assert_eq!(bbb_o.qty, dec!(800));
        assert_eq!(bbb_o.commission, dec!(40));

        // Turnover at decision = 80,000 / 100,000 = 80%.
        assert_eq!(ev.turnover_pct, dec!(80));

        // Ledger after the Jan 2 fill: cash = 100,000 − 80,000 − 80 = 19,920.
        let jan2 = rep
            .daily_equity_curve
            .iter()
            .find(|p| p.date == d(2024, 1, 2))
            .unwrap();
        assert_eq!(jan2.cash, dec!(19920));
        assert_eq!(jan2.invested, dec!(80000));
        // Commission reduced equity: 100,000 − 80 = 99,920.
        assert_eq!(jan2.equity, dec!(99920));

        // Jan 1 (pre-fill) equity is still all cash.
        let jan1 = &rep.daily_equity_curve[0];
        assert_eq!(jan1.cash, dec!(100000));
        assert_eq!(jan1.equity, dec!(100000));

        // Final equity holds at 99,920 (flat prices).
        let last = rep.daily_equity_curve.last().unwrap();
        assert_eq!(last.equity, dec!(99920));
        assert_eq!(rep.total_costs, dec!(80));

        // pre_weights: 100% cash at decision.
        let cash_pre = ev.pre_weights.iter().find(|(k, _)| k == CASH_KEY).unwrap();
        assert_eq!(cash_pre.1, dec!(1));
    }

    /// Declining single-asset curve with a known 50% max drawdown.
    #[test]
    fn max_drawdown_on_declining_curve() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(1000),
            universe: vec![AssetSpec::new("AAA", "alpha")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(1), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "AAA",
            vec![
                (d(2024, 1, 1), dec!(10)),
                (d(2024, 1, 2), dec!(10)),
                (d(2024, 1, 3), dec!(8)),
                (d(2024, 1, 4), dec!(5)),
                (d(2024, 1, 5), dec!(8)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();
        // Buy 100 shares at $10 on Jan 2; equity 1000 → 800 → 500 → 800.
        let jan4 = rep
            .daily_equity_curve
            .iter()
            .find(|p| p.date == d(2024, 1, 4))
            .unwrap();
        assert_eq!(jan4.equity, dec!(500));
        // Max drawdown magnitude = 50%.
        assert!((rep.max_drawdown_pct - 50.0).abs() < 1e-9);
    }

    /// time-in-cash and turnover sanity: 100% cash model never trades.
    #[test]
    fn all_cash_never_trades() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(1000),
            universe: vec![AssetSpec::new("AAA", "alpha")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(1), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(0), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0.001),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "AAA",
            vec![
                (d(2024, 1, 1), dec!(10)),
                (d(2024, 1, 2), dec!(11)),
                (d(2024, 1, 3), dec!(12)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();
        // alpha target is 0 → desired notional 0 → no orders ever.
        assert_eq!(rep.rebalance_events[0].orders.len(), 0);
        assert_eq!(rep.total_costs, dec!(0));
        assert!((rep.time_in_cash_pct - 100.0).abs() < 1e-9);
        for p in &rep.daily_equity_curve {
            assert_eq!(p.equity, dec!(1000));
        }
    }

    /// Determinism: identical inputs → byte-identical serialized report.
    #[test]
    fn deterministic_repeat() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![
                AssetSpec::new("AAA", "alpha"),
                AssetSpec::new("BBB", "beta"),
            ],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.2), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(0.4), dec!(0), dec!(1)),
                ClassTarget::new("beta", dec!(0.4), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Monthly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0.0005),
            slippage_pct: dec!(0.001),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "AAA",
            vec![
                (d(2024, 1, 2), dec!(100)),
                (d(2024, 1, 15), dec!(110)),
                (d(2024, 2, 1), dec!(120)),
                (d(2024, 2, 15), dec!(115)),
                (d(2024, 3, 1), dec!(130)),
            ],
        );
        panel.insert_series(
            "BBB",
            vec![
                (d(2024, 1, 2), dec!(50)),
                (d(2024, 1, 15), dec!(48)),
                (d(2024, 2, 1), dec!(52)),
                (d(2024, 2, 15), dec!(55)),
                (d(2024, 3, 1), dec!(53)),
            ],
        );
        let a = simulate(&model, &panel).unwrap();
        let b = simulate(&model, &panel).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    /// Base = USD, holding a GBP-priced asset, with a step in GBPUSD. The base
    /// mark is HAND-COMPUTED on two dates; the FX move alone changes equity.
    ///
    /// Setup: $100,000, 100% into `LON` (GBP). LON close flat at 100 GBP.
    /// GBPUSD: Jan1=1.25, Jan2=1.25, Jan3=1.50. Decision Jan1 → fill Jan2 at
    /// 100 GBP × 1.25 = $125/sh → 100,000/125 = **800 shares**.
    /// Mark Jan2: 800 × 100 × 1.25 = **$100,000**. Mark Jan3 (FX→1.50):
    /// 800 × 100 × 1.50 = **$120,000** — +$20,000 from FX alone.
    #[test]
    fn fx_hand_computed_base_mark() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![AssetSpec::with_currency("LON", "alpha", "GBP")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(1), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "LON",
            vec![
                (d(2024, 1, 1), dec!(100)),
                (d(2024, 1, 2), dec!(100)),
                (d(2024, 1, 3), dec!(100)),
            ],
        );
        panel.insert_fx(
            "GBP",
            "USD",
            vec![
                (d(2024, 1, 1), dec!(1.25)),
                (d(2024, 1, 2), dec!(1.25)),
                (d(2024, 1, 3), dec!(1.50)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();

        // Order: 800 native shares, fill-date FX 1.25, $100,000 base / £80,000 native.
        let ev = &rep.rebalance_events[0];
        let o = ev.orders.iter().find(|o| o.symbol == "LON").unwrap();
        assert_eq!(o.qty, dec!(800));
        assert_eq!(o.fx_rate, dec!(1.25));
        assert_eq!(o.price_currency, "GBP");
        assert_eq!(o.notional, dec!(100000)); // base
        assert_eq!(o.notional_native, dec!(80000)); // GBP

        let jan2 = rep
            .daily_equity_curve
            .iter()
            .find(|p| p.date == d(2024, 1, 2))
            .unwrap();
        assert_eq!(jan2.cash, dec!(0));
        assert_eq!(jan2.invested, dec!(100000));
        assert_eq!(jan2.equity, dec!(100000));

        let jan3 = rep
            .daily_equity_curve
            .iter()
            .find(|p| p.date == d(2024, 1, 3))
            .unwrap();
        // FX-only revaluation: 800 × 100 GBP × 1.50 = $120,000.
        assert_eq!(jan3.invested, dec!(120000));
        assert_eq!(jan3.equity, dec!(120000));
    }

    /// Cash-yield proxy: all-cash model, rising `BIL`, no positions. Cash
    /// compounds the proxy's daily return: 1000 → 1000×1.1 → 1100×1.1 = 1210.
    #[test]
    fn cash_yield_proxy_compounds() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(1000),
            universe: vec![AssetSpec::new("AAA", "alpha")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(1), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(0), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::Proxy("BIL".into()),
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "AAA",
            vec![
                (d(2024, 1, 1), dec!(10)),
                (d(2024, 1, 2), dec!(10)),
                (d(2024, 1, 3), dec!(10)),
            ],
        );
        // BIL rises 10% per day (proxy not in the universe — looked up directly).
        panel.insert_series(
            "BIL",
            vec![
                (d(2024, 1, 1), dec!(100)),
                (d(2024, 1, 2), dec!(110)),
                (d(2024, 1, 3), dec!(121)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();
        let eq = |day: u32| {
            rep.daily_equity_curve
                .iter()
                .find(|p| p.date == d(2024, 1, day))
                .unwrap()
                .equity
        };
        assert_eq!(eq(1), dec!(1000)); // no accrual on first proxy observation
        assert_eq!(eq(2), dec!(1100)); // +10%
        assert_eq!(eq(3), dec!(1210)); // +10% compounded
    }

    /// A leg that is non-tradable on the rebalance date is DEFERRED (no fabricated
    /// fill); the tradable leg still executes and the ledger stays balanced.
    #[test]
    fn non_tradable_leg_is_deferred() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(1000),
            universe: vec![
                AssetSpec::new("AAA", "alpha"),
                AssetSpec::new("BBB", "beta"),
            ],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(0.5), dec!(0), dec!(1)),
                ClassTarget::new("beta", dec!(0.5), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        // AAA has a GAP on the Jan1 decision date (only trades Jan2/Jan3).
        panel.insert_series(
            "AAA",
            vec![(d(2024, 1, 2), dec!(10)), (d(2024, 1, 3), dec!(10))],
        );
        panel.insert_series(
            "BBB",
            vec![
                (d(2024, 1, 1), dec!(10)),
                (d(2024, 1, 2), dec!(10)),
                (d(2024, 1, 3), dec!(10)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();
        let ev = &rep.rebalance_events[0];
        assert_eq!(ev.date, d(2024, 1, 1));
        // AAA deferred; only BBB generates an order.
        assert_eq!(ev.deferred_legs, vec!["AAA".to_string()]);
        assert_eq!(ev.orders.len(), 1);
        assert_eq!(ev.orders[0].symbol, "BBB");
        // BBB buys $500 → 50 shares at $10 on Jan2. Ledger: cash 500 + invested 500.
        let jan2 = rep
            .daily_equity_curve
            .iter()
            .find(|p| p.date == d(2024, 1, 2))
            .unwrap();
        assert_eq!(jan2.cash, dec!(500));
        assert_eq!(jan2.invested, dec!(500));
        assert_eq!(jan2.equity, jan2.cash + jan2.invested);
        assert_eq!(jan2.equity, dec!(1000));
    }

    /// Cash-shortfall: 100%-invested target with a 25% commission makes the buy
    /// need exceed cash → buys scale down so cash lands EXACTLY 0, never negative.
    #[test]
    fn cash_shortfall_lands_exactly_zero() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(1000),
            universe: vec![AssetSpec::new("AAA", "alpha")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(1), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0.25), // total_need = 1000×1.25 = 1250 > 1000 cash
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "AAA",
            vec![
                (d(2024, 1, 1), dec!(10)),
                (d(2024, 1, 2), dec!(10)),
                (d(2024, 1, 3), dec!(10)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();
        // scale = 1000/1250 = 0.8 → notional 800, commission 200, cash → 0.
        let jan2 = rep
            .daily_equity_curve
            .iter()
            .find(|p| p.date == d(2024, 1, 2))
            .unwrap();
        assert_eq!(jan2.cash, dec!(0));
        // Never negative on any day.
        for p in &rep.daily_equity_curve {
            assert!(p.cash >= dec!(0), "cash went negative on {}", p.date);
        }
    }

    /// Sanity: with no rules (P1), the main result equals the
    /// `rebalanced_base_policy` benchmark — same curve and same metrics.
    #[test]
    fn empty_rules_equals_rebalanced_benchmark() {
        let model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![
                AssetSpec::new("AAA", "alpha"),
                AssetSpec::new("BBB", "beta"),
            ],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.2), dec!(0), dec!(1)),
                ClassTarget::new("alpha", dec!(0.4), dec!(0), dec!(1)),
                ClassTarget::new("beta", dec!(0.4), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Monthly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0.001),
            slippage_pct: dec!(0.001),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        let mut panel = PricePanel::new();
        panel.insert_series(
            "AAA",
            vec![
                (d(2024, 1, 2), dec!(100)),
                (d(2024, 2, 1), dec!(120)),
                (d(2024, 3, 1), dec!(130)),
            ],
        );
        panel.insert_series(
            "BBB",
            vec![
                (d(2024, 1, 2), dec!(50)),
                (d(2024, 2, 1), dec!(52)),
                (d(2024, 3, 1), dec!(53)),
            ],
        );
        let rep = simulate(&model, &panel).unwrap();
        assert_eq!(
            rep.daily_equity_curve,
            rep.benchmarks.rebalanced_base_policy.daily_equity_curve
        );
        assert_eq!(rep.metrics, rep.benchmarks.rebalanced_base_policy.metrics);
        // The static benchmark (buy-once) should differ from the rebalanced one
        // here (prices move between rebalances), proving it's a distinct path.
        assert_ne!(
            rep.benchmarks.static_base_policy.daily_equity_curve,
            rep.benchmarks.rebalanced_base_policy.daily_equity_curve
        );
    }

    use crate::analytics::portfolio_sim::actions::{
        Action, Condition, Rule, TargetKey,
    };

    fn shift_model(rules: Vec<Rule>) -> PortfolioModel {
        PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![AssetSpec::new("SPY", "equity")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.5), dec!(0), dec!(1)),
                ClassTarget::new("equity", dec!(0.5), dec!(0), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules,
        }
    }

    /// Weekly Mondays, SPY flat at $100; a tilt fires from 2024-01-15 onward.
    fn weekly_panel() -> PricePanel {
        let mut panel = PricePanel::new();
        panel.insert_series(
            "SPY",
            vec![
                (d(2024, 1, 1), dec!(100)),
                (d(2024, 1, 8), dec!(100)),
                (d(2024, 1, 15), dec!(100)),
                (d(2024, 1, 22), dec!(100)),
            ],
        );
        panel
    }

    /// End-to-end: a stub `AfterDate` rule reshapes the allocation on/after it
    /// fires, vs an identical no-rule run; the ledger stays balanced throughout.
    #[test]
    fn rule_shifts_allocation_end_to_end() {
        let tilt = Rule::new(
            "tilt-eq",
            Condition::AfterDate(d(2024, 1, 15)),
            Action::Tilt {
                class: "equity".into(),
                by: dec!(0.3),
                from: "cash".into(),
                to: "cash".into(),
            },
            10,
            RebalanceCadence::Weekly,
        );
        let with_rule = simulate(&shift_model(vec![tilt]), &weekly_panel()).unwrap();
        let no_rule = simulate(&shift_model(vec![]), &weekly_panel()).unwrap();

        let spy_post = |rep: &PortfolioBacktestReport, day: u32| -> Decimal {
            let ev = rep
                .rebalance_events
                .iter()
                .find(|e| e.date == d(2024, 1, day))
                .unwrap();
            ev.post_weights.iter().find(|(k, _)| k == "SPY").unwrap().1
        };

        // Before the rule date: equity target 0.5 in both runs, no rule applied.
        let ev_wk1 = with_rule
            .rebalance_events
            .iter()
            .find(|e| e.date == d(2024, 1, 1))
            .unwrap();
        assert!(ev_wk1.applied_rule_ids.is_empty());
        assert_eq!(spy_post(&with_rule, 1), dec!(0.5));

        // On/after the rule date: the tilt fires and equity lands 0.8.
        let ev_wk3 = with_rule
            .rebalance_events
            .iter()
            .find(|e| e.date == d(2024, 1, 15))
            .unwrap();
        assert_eq!(ev_wk3.applied_rule_ids, vec!["tilt-eq".to_string()]);
        assert_eq!(spy_post(&with_rule, 15), dec!(0.8));
        // The no-rule run stays at 0.5 on the same date — the rule moved it.
        assert_eq!(spy_post(&no_rule, 15), dec!(0.5));

        // Ledger balanced every day: equity == cash + invested, cash never < 0.
        for p in &with_rule.daily_equity_curve {
            assert_eq!(p.equity, p.cash + p.invested);
            assert!(p.cash >= dec!(0));
        }
    }

    /// Cadence gate: a MONTHLY rule in a WEEKLY model fires only on the first
    /// model-rebalance of each calendar month.
    #[test]
    fn monthly_rule_in_weekly_model_fires_on_month_boundaries() {
        let monthly = Rule::new(
            "monthly-tilt",
            Condition::Always,
            Action::Tilt {
                class: "equity".into(),
                by: dec!(0.1),
                from: "cash".into(),
                to: "cash".into(),
            },
            10,
            RebalanceCadence::Monthly,
        );
        let mut panel = PricePanel::new();
        panel.insert_series(
            "SPY",
            vec![
                (d(2024, 1, 1), dec!(100)),
                (d(2024, 1, 8), dec!(100)),
                (d(2024, 1, 15), dec!(100)),
                (d(2024, 1, 22), dec!(100)),
                (d(2024, 1, 29), dec!(100)),
                (d(2024, 2, 5), dec!(100)),
                (d(2024, 2, 12), dec!(100)),
                (d(2024, 2, 19), dec!(100)),
                (d(2024, 2, 26), dec!(100)),
            ],
        );
        let rep = simulate(&shift_model(vec![monthly]), &panel).unwrap();
        let fired_dates: Vec<NaiveDate> = rep
            .rebalance_events
            .iter()
            .filter(|e| !e.applied_rule_ids.is_empty())
            .map(|e| e.date)
            .collect();
        // First rebalance of January (Jan 1) and of February (Feb 5) only.
        assert_eq!(fired_dates, vec![d(2024, 1, 1), d(2024, 2, 5)]);
    }

    /// Infeasible rule set → the rebalance is flagged infeasible, NO orders are
    /// emitted, and the book holds prior weights (here: stays all cash).
    #[test]
    fn infeasible_rule_holds_prior_weights() {
        let mut model = PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![
                AssetSpec::new("SPY", "equity"),
                AssetSpec::new("IEF", "bond"),
            ],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.4), dec!(0.2), dec!(1)),
                ClassTarget::new("equity", dec!(0.3), dec!(0), dec!(1)),
                ClassTarget::new("bond", dec!(0.3), dec!(0.2), dec!(1)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules: vec![],
        };
        // Pin equity to 0.9 → Σfloor (0.9 + 0.2 + 0.2) = 1.3 > 1 → infeasible.
        model.rules = vec![Rule::new(
            "pin-eq",
            Condition::Always,
            Action::SetTarget {
                key: TargetKey::Class("equity".into()),
                weight: dec!(0.9),
            },
            10,
            RebalanceCadence::Weekly,
        )];
        let mut panel = PricePanel::new();
        panel.insert_series(
            "SPY",
            vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 8), dec!(100))],
        );
        panel.insert_series(
            "IEF",
            vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 8), dec!(100))],
        );
        let rep = simulate(&model, &panel).unwrap();
        let ev = &rep.rebalance_events[0];
        assert!(ev.infeasible);
        assert!(ev.orders.is_empty());
        // Book held: equity stays the all-cash initial capital throughout.
        for p in &rep.daily_equity_curve {
            assert_eq!(p.equity, dec!(100000));
            assert_eq!(p.invested, dec!(0));
        }
    }

    /// Plant a deep V-bottom daily series: ~800-day decline then ~300-day rally.
    /// (Mirrors the cycle_signals V-bottom fixture; the mechanical cycle-bottom
    /// confluence crosses 5/7 on the monthly read in mid-2020.)
    fn planted_v_series(start: NaiveDate, n_decline: usize, n_rally: usize) -> Vec<(NaiveDate, Decimal)> {
        use chrono::Days;
        use rust_decimal::prelude::FromPrimitive;
        let mut out = Vec::with_capacity(n_decline + n_rally);
        let mut price = 1000.0_f64;
        for i in 0..n_decline {
            price = 1000.0 - i as f64 * (820.0 / n_decline as f64);
            let noise = 8.0 * (i as f64 / 11.0).sin();
            let p = (price + noise).max(50.0);
            out.push((start + Days::new(i as u64), Decimal::from_f64(p).unwrap().round_dp(2)));
        }
        let base = price;
        for j in 1..=n_rally {
            let p = base + j as f64 * (700.0 / n_rally as f64);
            let noise = 6.0 * (j as f64 / 9.0).sin();
            out.push((
                start + Days::new((n_decline + j) as u64),
                Decimal::from_f64(p + noise).unwrap().round_dp(2),
            ));
        }
        out
    }

    /// M2 end-to-end: a signal rule fires on a PLANTED cycle bottom and tilts the
    /// book toward hard money vs an otherwise-identical no-rule run, the ledger
    /// stays balanced, and the rule does NOT fire during the long decline.
    #[test]
    fn signal_rule_tilts_on_planted_cycle_bottom() {
        // cash 0.8 / hard_money 0.2 (HMX). The rule tilts +0.20 into hard money
        // when the monthly cycle-bottom confluence is >= 5.
        let base = |rules: Vec<Rule>| PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![AssetSpec::new("HMX", "hard_money")],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.8), dec!(0), dec!(1)),
                ClassTarget::new("hard_money", dec!(0.2), dec!(0), dec!(0.6)),
            ],
            within_class: WithinClass::Equal,
            rebalance_cadence: RebalanceCadence::Weekly,
            rebalance_band_mode: RebalanceBandMode::ToTarget,
            fill: FillMode::NextClose,
            commission_pct: dec!(0),
            slippage_pct: dec!(0),
            cash_yield: CashYield::None,
            max_position: None,
            rules,
        };
        let tilt_rule = Rule::new(
            "add-hm",
            Condition::Signal(Box::new(
                crate::analytics::portfolio_sim::rule_expr::Expr::Compare {
                    op: crate::analytics::portfolio_sim::rule_expr::CmpOp::Ge,
                    lhs: Box::new(crate::analytics::portfolio_sim::rule_expr::Expr::Accessor {
                        name: "cycle_bottom_met".into(),
                        args: vec!["HMX".into()],
                        tf: None,
                    }),
                    rhs: Box::new(crate::analytics::portfolio_sim::rule_expr::Expr::Num(5.0)),
                },
            )),
            Action::Tilt {
                class: "hard_money".into(),
                by: dec!(0.2),
                from: "cash".into(),
                to: "cash".into(),
            },
            10,
            RebalanceCadence::Weekly,
        );

        let series = planted_v_series(d(2018, 1, 1), 800, 300);
        let mut panel = PricePanel::new();
        panel.insert_series("HMX", series);

        let ruled = simulate(&base(vec![tilt_rule]), &panel).unwrap();
        let plain = simulate(&base(vec![]), &panel).unwrap();

        // (1) Ledger balanced on every day (both runs).
        for p in ruled.daily_equity_curve.iter().chain(plain.daily_equity_curve.iter()) {
            assert_eq!(p.equity, p.cash + p.invested, "ledger must balance at {}", p.date);
        }

        // (2) Early stretch does NOT fire: the first rebalance has too little
        //     history (insufficient → NaN → no fire), and a mid-decline 2019
        //     rebalance is below threshold.
        assert!(
            ruled.rebalance_events[0].applied_rule_ids.is_empty(),
            "first rebalance must not fire (insufficient history)"
        );
        let fired_in_2019 = ruled
            .rebalance_events
            .iter()
            .filter(|e| e.date.year() == 2019)
            .any(|e| !e.applied_rule_ids.is_empty());
        assert!(!fired_in_2019, "rule must not fire during the 2019 decline");

        // (3) The rule DOES fire on the planted bottom (mid-2020), tagging events.
        let firing: Vec<&RebalanceEvent> = ruled
            .rebalance_events
            .iter()
            .filter(|e| e.applied_rule_ids.iter().any(|id| id == "add-hm"))
            .collect();
        assert!(!firing.is_empty(), "rule must fire on the planted cycle bottom");
        assert!(
            firing.iter().all(|e| e.date.year() == 2020),
            "firing window should sit at the mid-2020 bottom, got {:?}",
            firing.iter().map(|e| e.date).collect::<Vec<_>>()
        );

        // (4) Allocation tilts toward hard money: the ruled run pushes HMX above
        //     the 0.2 baseline at least once; the no-rule run never does.
        let hmx_post = |rep: &PortfolioBacktestReport| -> Decimal {
            rep.rebalance_events
                .iter()
                .flat_map(|e| e.post_weights.iter())
                .filter(|(k, _)| k == "HMX")
                .map(|(_, w)| *w)
                .max()
                .unwrap_or(dec!(0))
        };
        let ruled_max = hmx_post(&ruled);
        let plain_max = hmx_post(&plain);
        assert!(
            ruled_max > plain_max,
            "ruled HMX max weight {ruled_max} must exceed no-rule max {plain_max}"
        );
        assert!(
            ruled_max > dec!(0.2),
            "tilt should raise HMX above its 0.2 base, got {ruled_max}"
        );
        // The no-rule run holds the base 0.2 target (no tilt ever).
        assert!(plain_max <= dec!(0.2000001), "no-rule HMX must stay at base, got {plain_max}");
    }

    /// M1 end-to-end (the regime model coming alive). A SYNTHETIC panel carries the
    /// macro REGIME_SYMBOLS engineered to be risk-ON for the first ~half and
    /// risk-OFF after a known flip date (VIX spikes, yields fall, curve inverts,
    /// DXY rises, …). The M1 spec's `regime_score()`-driven rules must:
    ///  - NOT fire risk-off on the first rebalance (insufficient macro history → NaN),
    ///  - fire the risk-off (raise-bonds) rule only on/after the flip date,
    ///  - lift the bond (IEF) weight above the no-rule base in the risk-off regime,
    ///  - lower IEF below base in the risk-on regime (risk-on rule),
    ///  - keep the ledger balanced throughout.
    #[test]
    fn m1_regime_rule_shifts_allocation() {
        use rust_decimal::prelude::FromPrimitive;

        // The shipped M1 spec is the source of truth for the model + rules.
        let toml = include_str!("../../../models/m1-regime-balanced.toml");
        let rm = crate::analytics::portfolio_sim::spec::resolve_str(toml).unwrap();
        let model = rm.model.clone();
        let mut no_rule = model.clone();
        no_rule.rules = vec![];

        // --- build the synthetic macro + universe panel ---
        let start = d(2024, 1, 1); // a Monday
        let n: u64 = 120;
        let flip: u64 = 60; // global bar 60 == 2024-03-01: risk-on → risk-off
        let macro_val = |sym: &str, i: u64| -> f64 {
            let on = i < flip;
            let j = if on { i } else { i - flip } as f64;
            match sym {
                "^VIX" => if on { 18.0 - 0.05 * j } else { 25.0 + 0.10 * j },
                "^TNX" => if on { 3.0 + 0.01 * j } else { 5.0 - 0.01 * j },
                "^IRX" => if on { 2.0 } else { 6.0 },
                "DX-Y.NYB" => if on { 105.0 - 0.02 * j } else { 100.0 + 0.05 * j },
                "GC=F" => if on { 2000.0 } else { 2000.0 + 1.0 * j },
                "^GSPC" => if on { 4000.0 + 2.0 * j } else { 4800.0 - 2.0 * j },
                "BTC-USD" => if on { 50000.0 + 10.0 * j } else { 60000.0 + 10.0 * j },
                "HYG" => if on { 75.0 + 0.05 * j } else { 80.0 - 0.05 * j },
                "LQD" => 110.0,
                "HG=F" => if on { 4.0 + 0.005 * j } else { 4.5 - 0.005 * j },
                _ => 100.0,
            }
        };
        let mut panel = PricePanel::new();
        for &sym in crate::regime::REGIME_YAHOO_SYMBOLS {
            let series: Vec<(NaiveDate, Decimal)> = (0..n)
                .map(|i| {
                    (
                        start + chrono::Days::new(i),
                        Decimal::from_f64(macro_val(sym, i)).unwrap().round_dp(4),
                    )
                })
                .collect();
            panel.insert_series(sym, series);
        }
        // Tradable universe: SPY (rising), IEF (flat). Present on every date.
        let spy: Vec<(NaiveDate, Decimal)> = (0..n)
            .map(|i| (start + chrono::Days::new(i), Decimal::from_f64(400.0 + 0.1 * i as f64).unwrap().round_dp(4)))
            .collect();
        let ief: Vec<(NaiveDate, Decimal)> = (0..n)
            .map(|i| (start + chrono::Days::new(i), dec!(100)))
            .collect();
        panel.insert_series("SPY", spy);
        panel.insert_series("IEF", ief);

        let ruled = simulate(&model, &panel).unwrap();
        let plain = simulate(&no_rule, &panel).unwrap();

        // (0) Ledger balanced on every day (both runs).
        for p in ruled.daily_equity_curve.iter().chain(plain.daily_equity_curve.iter()) {
            assert_eq!(p.equity, p.cash + p.invested, "ledger must balance at {}", p.date);
            assert!(p.cash >= dec!(0));
        }

        // (1) First rebalance (2024-01-01): too little macro history → regime NaN →
        //     NEITHER regime rule fires.
        let first = &ruled.rebalance_events[0];
        assert_eq!(first.date, start);
        assert!(
            first.applied_rule_ids.is_empty(),
            "first rebalance must not fire (insufficient macro history)"
        );

        // (2) The risk-off rule fires, and ONLY on/after the flip date (2024-03-01).
        let off_fires: Vec<&RebalanceEvent> = ruled
            .rebalance_events
            .iter()
            .filter(|e| e.applied_rule_ids.iter().any(|id| id == "tilt-to-bonds-on-risk-off"))
            .collect();
        assert!(!off_fires.is_empty(), "risk-off rule must fire in the risk-off regime");
        assert!(
            off_fires.iter().all(|e| e.date >= d(2024, 3, 1)),
            "risk-off must fire only on/after the flip, got {:?}",
            off_fires.iter().map(|e| e.date).collect::<Vec<_>>()
        );

        // (3) The risk-on rule fires in the early (risk-on) regime, before the flip.
        let on_fires: Vec<NaiveDate> = ruled
            .rebalance_events
            .iter()
            .filter(|e| e.applied_rule_ids.iter().any(|id| id == "tilt-to-equity-on-risk-on"))
            .map(|e| e.date)
            .collect();
        assert!(!on_fires.is_empty(), "risk-on rule must fire in the risk-on regime");
        assert!(
            on_fires.iter().any(|&dt| dt < d(2024, 3, 1)),
            "risk-on must fire before the flip, got {on_fires:?}"
        );

        // post-rebalance weight of `sym` on `date` (intended target weight).
        let post = |rep: &PortfolioBacktestReport, date: NaiveDate, sym: &str| -> Decimal {
            rep.rebalance_events
                .iter()
                .find(|e| e.date == date)
                .and_then(|e| e.post_weights.iter().find(|(k, _)| k == sym).map(|(_, w)| *w))
                .unwrap_or(dec!(-1))
        };

        // (4) Deep risk-off rebalance (2024-04-22, last clean OFF): the bond (IEF)
        //     weight RISES above the no-rule base (0.30) and above the plain run.
        let off_date = d(2024, 4, 22);
        let ief_off_ruled = post(&ruled, off_date, "IEF");
        let ief_off_plain = post(&plain, off_date, "IEF");
        assert_eq!(ief_off_plain, dec!(0.30000000), "no-rule bond holds base 0.30");
        assert!(
            ief_off_ruled > ief_off_plain && ief_off_ruled > dec!(0.30),
            "risk-off must lift IEF above base: ruled={ief_off_ruled} plain={ief_off_plain}"
        );
        // tilt_size 0.15 → bond 0.30 + 0.15 = 0.45.
        assert_eq!(ief_off_ruled, dec!(0.45000000));

        // (5) Deep risk-on rebalance (2024-02-26, clean ON): the risk-on rule lowers
        //     IEF below base (bond 0.30 − 0.15 = 0.15).
        let on_date = d(2024, 2, 26);
        let ief_on_ruled = post(&ruled, on_date, "IEF");
        assert_eq!(post(&plain, on_date, "IEF"), dec!(0.30000000));
        assert!(
            ief_on_ruled < dec!(0.30),
            "risk-on must lower IEF below base, got {ief_on_ruled}"
        );
        assert_eq!(ief_on_ruled, dec!(0.15000000));
    }
}
