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

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::solver::{solve_targets, SolveBucket, SolveOutcome};
use super::{PortfolioModel, PricePanel, RebalanceBandMode, WithinClass};

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
    pub qty: Decimal,
    pub fill_date: NaiveDate,
    pub fill_price: Decimal,
    /// Signed traded notional at the fill price (qty × fill_price, +buy / −sell).
    pub notional: Decimal,
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
    /// Desired |notional| to trade at the fill (dollars), pre cash-shortfall scaling.
    notional_abs: Decimal,
    fill_date: NaiveDate,
    fill_price: Decimal,
    event_idx: usize,
    order_idx: usize,
}

const CASH_KEY: &str = "CASH";

/// Run the P0 simulation.
pub fn simulate(model: &PortfolioModel, panel: &PricePanel) -> Result<PortfolioBacktestReport> {
    let calendar = panel.calendar();

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

    for &t in &calendar {
        // --- 1. Execute fills scheduled for T (sells settle first, then buys,
        //        scaling buys down if cash is short). ---
        if let Some(orders) = pending.remove(&t) {
            execute_fills(
                orders,
                model,
                &mut cash,
                &mut holdings,
                &mut events,
                &mut total_costs,
            );
        }

        // --- 2. Refresh last-known close for symbols trading on T. ---
        for a in &model.universe {
            if let Some(c) = panel.close_on(&a.symbol, t) {
                last_close.insert(a.symbol.clone(), c);
            }
        }

        // --- 3. MARK every held position at T's close (carry last-known for
        //        non-tradable held symbols). ---
        let mut invested = dec!(0);
        for (sym, h) in &holdings {
            if h.qty == dec!(0) {
                continue;
            }
            if let Some(px) = last_close.get(sym) {
                invested += h.qty * *px;
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
        decide_rebalance(
            t,
            model,
            panel,
            equity,
            &holdings,
            &last_close,
            &class_symbols,
            cash,
            &mut events,
            &mut pending,
        )?;
    }

    let report = finalize(curve, events, total_costs);
    Ok(report)
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

#[allow(clippy::too_many_arguments)]
fn decide_rebalance(
    t: NaiveDate,
    model: &PortfolioModel,
    panel: &PricePanel,
    equity: Decimal,
    holdings: &HashMap<String, Holding>,
    last_close: &HashMap<String, Decimal>,
    class_symbols: &HashMap<String, Vec<String>>,
    cash: Decimal,
    events: &mut Vec<RebalanceEvent>,
    pending: &mut BTreeMap<NaiveDate, Vec<PendingOrder>>,
) -> Result<()> {
    // Current marked weights per symbol (+ CASH), for pre_weights.
    let mut pre_weights: Vec<(String, Decimal)> = Vec::new();
    for a in &model.universe {
        let v = holdings
            .get(&a.symbol)
            .filter(|h| h.qty != dec!(0))
            .and_then(|h| last_close.get(&a.symbol).map(|px| h.qty * *px))
            .unwrap_or(dec!(0));
        let w = if equity > dec!(0) {
            (v / equity).round_dp(8)
        } else {
            dec!(0)
        };
        pre_weights.push((a.symbol.clone(), w));
    }
    pre_weights.push((
        CASH_KEY.to_string(),
        if equity > dec!(0) {
            (cash / equity).round_dp(8)
        } else {
            dec!(0)
        },
    ));

    // Build the class buckets from the model's fixed targets and solve.
    let buckets: Vec<SolveBucket> = model
        .targets
        .iter()
        .map(|ct| SolveBucket::new(ct.class.clone(), ct.target, ct.floor, ct.ceiling))
        .collect();
    let solved = match solve_targets(&buckets)? {
        SolveOutcome::Infeasible => {
            events.push(RebalanceEvent {
                date: t,
                orders: vec![],
                turnover_pct: dec!(0),
                total_cost: dec!(0),
                pre_weights,
                post_weights: vec![],
                infeasible: true,
            });
            return Ok(());
        }
        SolveOutcome::Solved(w) => w,
    };
    let class_weight: HashMap<String, Decimal> = model
        .targets
        .iter()
        .map(|ct| ct.class.clone())
        .zip(solved.iter().copied())
        .collect();

    // Split each non-cash class weight equally across its TRADABLE symbols and
    // build orders = desired − current notional.
    debug_assert!(matches!(model.within_class, WithinClass::Equal));
    debug_assert!(matches!(model.rebalance_band_mode, RebalanceBandMode::ToTarget));

    let mut orders: Vec<Order> = Vec::new();
    let mut new_pending: Vec<PendingOrder> = Vec::new();
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
        let target_w = if tradable.contains(&&a.symbol) && n > dec!(0) {
            w_class / n
        } else {
            // Non-tradable symbol keeps its current weight (no order this T).
            let cur = holdings
                .get(&a.symbol)
                .filter(|h| h.qty != dec!(0))
                .and_then(|h| last_close.get(&a.symbol).map(|px| h.qty * *px))
                .unwrap_or(dec!(0));
            let w = if equity > dec!(0) { cur / equity } else { dec!(0) };
            post_weights.push((a.symbol.clone(), w.round_dp(8)));
            deployed_weight += w;
            continue;
        };
        post_weights.push((a.symbol.clone(), target_w.round_dp(8)));
        deployed_weight += target_w;

        let desired_notional = target_w * equity;
        let current_notional = holdings
            .get(&a.symbol)
            .filter(|h| h.qty != dec!(0))
            .and_then(|h| last_close.get(&a.symbol).map(|px| h.qty * *px))
            .unwrap_or(dec!(0));
        let order_notional = desired_notional - current_notional;
        if order_notional == dec!(0) {
            continue;
        }
        let (next_date, next_close) = match panel.next_tradable(&a.symbol, t) {
            Some(v) => v,
            None => continue, // no future close to fill against — drop (P0 edge).
        };
        let side = if order_notional > dec!(0) {
            Side::Buy
        } else {
            Side::Sell
        };
        let fill_price = match side {
            Side::Buy => next_close * (dec!(1) + model.slippage_pct),
            Side::Sell => next_close * (dec!(1) - model.slippage_pct),
        };
        let notional_abs = order_notional.abs();
        turnover_notional += notional_abs;
        let order_idx = orders.len();
        // Intent record (overwritten with realized numbers at fill).
        let qty_intent = if fill_price != dec!(0) {
            notional_abs / fill_price
        } else {
            dec!(0)
        };
        orders.push(Order {
            symbol: a.symbol.clone(),
            side,
            qty: qty_intent,
            fill_date: next_date,
            fill_price,
            notional: order_notional,
            commission: model.commission_pct * notional_abs,
        });
        new_pending.push(PendingOrder {
            symbol: a.symbol.clone(),
            side,
            notional_abs,
            fill_date: next_date,
            fill_price,
            event_idx,
            order_idx,
        });
    }

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
    });
    for p in new_pending {
        pending.entry(p.fill_date).or_default().push(p);
    }
    Ok(())
}

/// Execute one fill date's batch: sells settle first (adding cash), then buys are
/// scaled down proportionally if cash is short. Realized fill numbers are written
/// back into the originating [`RebalanceEvent`].
fn execute_fills(
    mut orders: Vec<PendingOrder>,
    model: &PortfolioModel,
    cash: &mut Decimal,
    holdings: &mut HashMap<String, Holding>,
    events: &mut [RebalanceEvent],
    total_costs: &mut Decimal,
) {
    // Deterministic order: by event then order index.
    orders.sort_by_key(|o| (o.event_idx, o.order_idx));

    // Sells first.
    for o in orders.iter().filter(|o| matches!(o.side, Side::Sell)) {
        let h = holdings.entry(o.symbol.clone()).or_default();
        // Shares implied by the desired sell notional, clamped to holdings.
        let mut qty = if o.fill_price != dec!(0) {
            o.notional_abs / o.fill_price
        } else {
            dec!(0)
        };
        if qty > h.qty {
            qty = h.qty;
        }
        let notional = qty * o.fill_price;
        let commission = model.commission_pct * notional;
        // Reduce average-cost basis proportionally.
        if h.qty > dec!(0) {
            let avg = h.total_cost / h.qty;
            h.qty -= qty;
            h.total_cost = avg * h.qty;
        }
        *cash += notional - commission;
        *total_costs += commission;
        write_back(events, o, qty, notional, commission, Side::Sell);
    }

    // Buys: scale down proportionally if total need exceeds available cash.
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
        let notional = o.notional_abs * scale;
        let commission = model.commission_pct * notional;
        let qty = if o.fill_price != dec!(0) {
            notional / o.fill_price
        } else {
            dec!(0)
        };
        let h = holdings.entry(o.symbol.clone()).or_default();
        h.qty += qty;
        h.total_cost += notional; // cost basis at the slipped fill price
        *cash -= notional + commission;
        *total_costs += commission;
        write_back(events, o, qty, notional, commission, Side::Buy);
    }
}

fn write_back(
    events: &mut [RebalanceEvent],
    o: &PendingOrder,
    qty: Decimal,
    notional: Decimal,
    commission: Decimal,
    side: Side,
) {
    if let Some(ev) = events.get_mut(o.event_idx) {
        if let Some(ord) = ev.orders.get_mut(o.order_idx) {
            ord.qty = qty;
            ord.notional = match side {
                Side::Buy => notional,
                Side::Sell => -notional,
            };
            ord.commission = commission;
        }
        // Keep the event's recorded total_cost consistent with realized fills.
        ev.total_cost = ev.orders.iter().map(|x| x.commission).sum();
    }
}

/// Derive the f64 metrics from the closed daily curve.
fn finalize(
    curve: Vec<DailyEquityPoint>,
    events: Vec<RebalanceEvent>,
    total_costs: Decimal,
) -> PortfolioBacktestReport {
    let n_rebalances = events.len();
    let equities: Vec<f64> = curve
        .iter()
        .map(|p| p.equity.to_f64().unwrap_or(0.0))
        .collect();

    // CAGR over the wall-clock span of the curve.
    let cagr_pct = if curve.len() >= 2 {
        let first = equities[0];
        let last = *equities.last().unwrap_or(&0.0);
        let days = (curve.last().unwrap().date - curve[0].date).num_days() as f64;
        let years = days / 365.25;
        if first > 0.0 && last > 0.0 && years > 0.0 {
            ((last / first).powf(1.0 / years) - 1.0) * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Max drawdown (positive magnitude).
    let max_drawdown_pct = curve
        .iter()
        .map(|p| p.drawdown_pct.to_f64().unwrap_or(0.0))
        .fold(0.0_f64, |acc, dd| acc.min(dd))
        .abs();

    // Annualized vol from daily log returns (sample std × √252).
    let mut logrets: Vec<f64> = Vec::with_capacity(equities.len());
    for w in equities.windows(2) {
        if w[0] > 0.0 && w[1] > 0.0 {
            logrets.push((w[1] / w[0]).ln());
        }
    }
    let ann_vol_pct = if logrets.len() >= 2 {
        let mean = logrets.iter().sum::<f64>() / logrets.len() as f64;
        let var = logrets.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
            / (logrets.len() as f64 - 1.0);
        var.sqrt() * (252.0_f64).sqrt() * 100.0
    } else {
        0.0
    };

    // Time in cash = average cash weight across the curve.
    let time_in_cash_pct = if curve.is_empty() {
        0.0
    } else {
        let s: f64 = curve
            .iter()
            .map(|p| {
                if p.equity > dec!(0) {
                    (p.cash / p.equity).to_f64().unwrap_or(0.0)
                } else {
                    0.0
                }
            })
            .sum();
        s / curve.len() as f64 * 100.0
    };

    // Average turnover per year = Σ event turnover% / years.
    let avg_turnover_pct_per_yr = if curve.len() >= 2 {
        let days = (curve.last().unwrap().date - curve[0].date).num_days() as f64;
        let years = days / 365.25;
        let total_turn: f64 = events
            .iter()
            .map(|e| e.turnover_pct.to_f64().unwrap_or(0.0))
            .sum();
        if years > 0.0 {
            total_turn / years
        } else {
            total_turn
        }
    } else {
        0.0
    };

    PortfolioBacktestReport {
        daily_equity_curve: curve,
        rebalance_events: events,
        cagr_pct,
        max_drawdown_pct,
        ann_vol_pct,
        time_in_cash_pct,
        avg_turnover_pct_per_yr,
        total_costs,
        n_rebalances,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::portfolio_sim::{
        AssetSpec, ClassTarget, FillMode, PortfolioModel, RebalanceCadence,
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
}
