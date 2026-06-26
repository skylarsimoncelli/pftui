//! Rule **action algebra** + the phased target resolver (POSITIONING-MODELS.md
//! §3.2). This is STAGE P3a: it wires the action vocabulary and the existing
//! bounded-projection solver ([`super::solver::solve_targets`]) into a single
//! deterministic function — [`resolve_targets`] — that turns `base_policy` +
//! a set of **already-fired** rules into a reshaped two-layer (class → symbol)
//! target allocation.
//!
//! The `when` condition is deliberately STUBBED here ([`Condition`] has no
//! signal-expression variant yet): real signal-accessor evaluation is P3b. The
//! enum is shaped so P3b can add a `Signal(Expr)` variant without touching the
//! engine — the engine only ever calls [`Condition::eval`] with an
//! [`EvalContext`], and P3b widens that context (e.g. with a signal snapshot)
//! rather than changing the call site.
//!
//! ## Phased algebra (strict order, deterministic)
//! Within each phase, fired rules are applied in **priority ascending, then id**
//! order.
//! - **Phase A — anchors** (`SetTarget` on a `Class` key): overwrite the working
//!   class weight AND pin that class's projection box to `[v, v]`. A same-class
//!   same-priority double-set is a model-hygiene warning.
//! - **Phase B — tilts** (`Tilt`): a zero-sum transfer — `+by` onto the class,
//!   `-by` onto the offset class (`from` for a positive tilt, `to` for a negative
//!   one; default `cash`). Adjusts the desired vector only, never the box.
//! - **Phase C — symbol actions + gates**: record intra-class per-symbol target
//!   overrides (`Add`/`Trim`/`Exit`) and `GateBlock` vetoes. Gates set the
//!   ceiling to 0 **before** projection.
//! - **Phase D — bounded projection**: build class-layer [`SolveBucket`]s and
//!   project onto the box-constrained simplex; then split each class budget to
//!   its symbols (equal weight + overrides) and project the symbol layer inside
//!   that budget, clamping each symbol to `max_position`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::accessors::{AtDateCtx, Memo};
use super::rule_expr::{self, Expr};
use super::solver::{solve_targets, SolveBucket, SolveOutcome};
use super::{PortfolioModel, PricePanel, RebalanceCadence};
use crate::analytics::cycle_signals::SignalTimeframe;

/// A target the algebra can address: a whole class budget or a single symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetKey {
    Class(String),
    Symbol(String),
}

/// One rule action (POSITIONING-MODELS.md §3.2). `Set`/`Tilt` act on the class
/// layer; `Add`/`Trim`/`Exit` are intra-class symbol overrides; `GateBlock`
/// vetoes a class or symbol (ceiling 0 before projection).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Anchor: pin a class (or symbol) target to `weight`.
    SetTarget { key: TargetKey, weight: Decimal },
    /// Zero-sum delta `by` onto `class`, offset against `from` (`by > 0`) or
    /// `to` (`by < 0`); empty offset name defaults to the cash class.
    Tilt {
        class: String,
        by: Decimal,
        from: String,
        to: String,
    },
    /// Raise a symbol's intra-class target toward `up_to`.
    Add { symbol: String, up_to: Decimal },
    /// Lower a symbol's intra-class target to `to`.
    Trim { symbol: String, to: Decimal },
    /// Force a symbol's target to 0.
    Exit { symbol: String },
    /// Veto a class or symbol: ceiling 0 (applied before projection).
    GateBlock { key: TargetKey },
}

/// The `when` predicate. The date/index stubs are evaluated with no extra
/// context; [`Condition::Signal`] (P3b) evaluates a validated signal expression
/// against the [`SignalEnv`] the engine supplies — WITHOUT changing the engine's
/// single `Condition::eval(&mut EvalContext)` call site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    /// Always fires.
    Always,
    /// Never fires.
    Never,
    /// Fires on or after `date`.
    AfterDate(NaiveDate),
    /// Fires strictly before `date`.
    BeforeDate(NaiveDate),
    /// Fires when `(rebalance_index - offset)` is a non-negative multiple of `n`.
    EveryNthRebalance { n: usize, offset: usize },
    /// Fires when the validated signal expression is true at the rebalance date
    /// (point-in-time, COMPLETED-bucket signal reads — see [`super::accessors`]).
    Signal(Box<Expr>),
}

/// The signal-evaluation environment the engine threads into [`EvalContext`] at
/// each rebalance date: the price panel, the default timeframe for `@tf`-less
/// accessors, and a FRESH per-date snapshot [`Memo`] (no cross-date leakage).
#[derive(Debug)]
pub struct SignalEnv<'a> {
    pub panel: &'a PricePanel,
    pub default_tf: SignalTimeframe,
    pub memo: Memo,
}

impl<'a> SignalEnv<'a> {
    pub fn new(panel: &'a PricePanel, default_tf: SignalTimeframe) -> Self {
        Self {
            panel,
            default_tf,
            memo: Memo::new(),
        }
    }
}

/// The evaluation context handed to [`Condition::eval`]. The date/index drive
/// the stub conditions; `signal` (when present) carries the panel + memo a
/// [`Condition::Signal`] needs. Not `Copy` — it owns a mutable snapshot memo.
#[derive(Debug)]
pub struct EvalContext<'a> {
    /// The rebalance decision date.
    pub date: NaiveDate,
    /// 0-based index of this rebalance within the run.
    pub rebalance_index: usize,
    /// Signal-evaluation environment (None for stub-only callers/tests).
    pub signal: Option<SignalEnv<'a>>,
}

impl<'a> EvalContext<'a> {
    /// A stub-only context (no signal environment).
    pub fn stub(date: NaiveDate, rebalance_index: usize) -> Self {
        Self {
            date,
            rebalance_index,
            signal: None,
        }
    }
}

impl Condition {
    /// Evaluate the condition. Deterministic, side-effect-free apart from the
    /// per-date signal memo. Returns `Err` only on an internal inconsistency
    /// (a signal rule with no signal env) — never to silently skip a rule.
    pub fn eval(&self, ctx: &mut EvalContext) -> Result<bool> {
        Ok(match self {
            Condition::Always => true,
            Condition::Never => false,
            Condition::AfterDate(d) => ctx.date >= *d,
            Condition::BeforeDate(d) => ctx.date < *d,
            Condition::EveryNthRebalance { n, offset } => {
                if *n == 0 || ctx.rebalance_index < *offset {
                    false
                } else {
                    (ctx.rebalance_index - *offset).is_multiple_of(*n)
                }
            }
            Condition::Signal(expr) => {
                let date = ctx.date;
                let env = ctx.signal.as_mut().ok_or_else(|| {
                    anyhow!("signal rule evaluated without a signal environment (engine bug)")
                })?;
                let mut at = AtDateCtx {
                    as_of: date,
                    panel: env.panel,
                    default_tf: env.default_tf,
                    memo: &mut env.memo,
                };
                rule_expr::eval_bool(expr, &mut at)?
            }
        })
    }
}

/// A fully-resolved rule: a stub [`Condition`], one [`Action`], a priority, and
/// the rule's own rebalance cadence (a finer-grained rule fires only on its own
/// cadence boundaries — gated by the engine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub when: Condition,
    pub then: Action,
    pub priority: i64,
    pub cadence: RebalanceCadence,
}

impl Rule {
    pub fn new(
        id: impl Into<String>,
        when: Condition,
        then: Action,
        priority: i64,
        cadence: RebalanceCadence,
    ) -> Self {
        Self {
            id: id.into(),
            when,
            then,
            priority,
            cadence,
        }
    }
}

/// Outcome of [`resolve_targets`].
#[derive(Debug, Clone, PartialEq)]
pub enum TargetResolution {
    Resolved {
        /// Solved class-layer weights (includes cash); sums to 1.
        class_weights: BTreeMap<String, Decimal>,
        /// Solved per-symbol weights across the whole book (sums to `1 - cash`).
        symbol_weights: BTreeMap<String, Decimal>,
        /// Ids of the fired rules that were applied, in apply order.
        applied_rule_ids: Vec<String>,
        /// Model-hygiene warnings (e.g. same-key/same-priority double anchor).
        warnings: Vec<String>,
    },
    /// The class-layer box ∩ simplex was empty — the engine HOLDS prior weights.
    Infeasible {
        applied_rule_ids: Vec<String>,
        warnings: Vec<String>,
    },
}

/// Stable apply-order for fired rules: priority ascending, then id ascending.
fn sorted_rules(fired: &[Rule]) -> Vec<&Rule> {
    let mut v: Vec<&Rule> = fired.iter().collect();
    v.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
    v
}

/// A per-symbol intra-class override recorded in Phase C.
#[derive(Debug, Clone, Copy)]
struct SymbolOverride {
    /// Desired absolute weight for this symbol.
    desired: Decimal,
    /// Hard ceiling on this symbol's absolute weight (`None` → only `max_position`).
    ceiling: Option<Decimal>,
}

/// Resolve `base_policy` + already-fired `fired_rules` into a two-layer target
/// allocation (POSITIONING-MODELS.md §3.2). `current_symbol_weights` is the
/// pre-rebalance marked weight per symbol; in P3a the `Equal` within-class split
/// is drift-to-target so it is accepted but not consumed (P3b/band-modes use it).
pub fn resolve_targets(
    model: &PortfolioModel,
    current_symbol_weights: &BTreeMap<String, Decimal>,
    tradable: &BTreeSet<String>,
    fired_rules: &[Rule],
) -> Result<TargetResolution> {
    let _ = current_symbol_weights; // reserved for P3b band/drift semantics
    let cash = &model.cash_class;
    let mut warnings: Vec<String> = Vec::new();

    // Seed the working class vector W = base_policy targets, and the per-class
    // projection box from the base bands. Order follows `model.targets`.
    let mut w: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut floor: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut ceiling: BTreeMap<String, Decimal> = BTreeMap::new();
    for ct in &model.targets {
        w.insert(ct.class.clone(), ct.target);
        floor.insert(ct.class.clone(), ct.floor);
        ceiling.insert(ct.class.clone(), ct.ceiling);
    }

    let ordered = sorted_rules(fired_rules);
    let applied_rule_ids: Vec<String> = ordered.iter().map(|r| r.id.clone()).collect();

    // Per-class anchor pin (Phase A) and per-symbol overrides / gates (Phase C).
    let mut pinned: BTreeMap<String, (i64, Decimal)> = BTreeMap::new();
    let mut sym_overrides: BTreeMap<String, SymbolOverride> = BTreeMap::new();
    let mut gated_classes: BTreeMap<String, ()> = BTreeMap::new();

    // ---- Phase A: anchors (SetTarget on a Class key) ----
    for r in &ordered {
        if let Action::SetTarget {
            key: TargetKey::Class(c),
            weight,
        } = &r.then
        {
            if let Some((prev_prio, _)) = pinned.get(c) {
                if *prev_prio == r.priority {
                    warnings.push(format!(
                        "class '{c}' set_target twice at priority {} (rule '{}'): last write wins",
                        r.priority, r.id
                    ));
                }
            }
            if w.contains_key(c) {
                w.insert(c.clone(), *weight);
                pinned.insert(c.clone(), (r.priority, *weight));
            } else {
                warnings.push(format!(
                    "rule '{}' set_target on unknown class '{c}' (ignored)",
                    r.id
                ));
            }
        }
    }

    // ---- Phase B: tilts (zero-sum class delta) ----
    for r in &ordered {
        if let Action::Tilt {
            class,
            by,
            from,
            to,
        } = &r.then
        {
            let offset = if by.is_sign_negative() {
                pick_offset(to, cash)
            } else {
                pick_offset(from, cash)
            };
            if !w.contains_key(class) {
                warnings.push(format!(
                    "rule '{}' tilt on unknown class '{class}' (ignored)",
                    r.id
                ));
                continue;
            }
            if !w.contains_key(&offset) {
                warnings.push(format!(
                    "rule '{}' tilt offset class '{offset}' is unknown (ignored)",
                    r.id
                ));
                continue;
            }
            *w.get_mut(class).expect("class present") += *by;
            *w.get_mut(&offset).expect("offset present") -= *by;
        }
    }

    // ---- Phase C: symbol overrides + gates ----
    for r in &ordered {
        match &r.then {
            Action::Add { symbol, up_to } => {
                sym_overrides.insert(
                    symbol.clone(),
                    SymbolOverride {
                        desired: *up_to,
                        ceiling: Some(*up_to),
                    },
                );
            }
            Action::Trim { symbol, to } => {
                sym_overrides.insert(
                    symbol.clone(),
                    SymbolOverride {
                        desired: *to,
                        ceiling: Some(*to),
                    },
                );
            }
            Action::Exit { symbol } => {
                sym_overrides.insert(
                    symbol.clone(),
                    SymbolOverride {
                        desired: dec!(0),
                        ceiling: Some(dec!(0)),
                    },
                );
            }
            Action::SetTarget {
                key: TargetKey::Symbol(s),
                weight,
            } => {
                sym_overrides.insert(
                    s.clone(),
                    SymbolOverride {
                        desired: *weight,
                        ceiling: Some(*weight),
                    },
                );
            }
            Action::GateBlock { key } => match key {
                TargetKey::Class(c) => {
                    gated_classes.insert(c.clone(), ());
                }
                TargetKey::Symbol(s) => {
                    sym_overrides.insert(
                        s.clone(),
                        SymbolOverride {
                            desired: dec!(0),
                            ceiling: Some(dec!(0)),
                        },
                    );
                }
            },
            _ => {}
        }
    }

    // ---- Phase D (class layer): bounded projection onto box ∩ simplex ----
    let mut class_buckets: Vec<SolveBucket> = Vec::with_capacity(model.targets.len());
    for ct in &model.targets {
        let desired = *w.get(&ct.class).expect("seeded");
        let (lo, hi) = if gated_classes.contains_key(&ct.class) {
            (dec!(0), dec!(0)) // gate veto BEFORE projection
        } else if let Some((_, v)) = pinned.get(&ct.class) {
            (*v, *v) // anchor pins the box exactly
        } else {
            (
                *floor.get(&ct.class).expect("seeded"),
                *ceiling.get(&ct.class).expect("seeded"),
            )
        };
        // Clamp the desired into the box so the solver's pre-check (box well-formed)
        // is the only feasibility authority; the simplex feasibility is checked there.
        let desired = clamp(desired, lo, hi);
        class_buckets.push(SolveBucket::new(ct.class.clone(), desired, lo, hi));
    }

    let class_solution = match solve_targets(&class_buckets)? {
        SolveOutcome::Infeasible => {
            return Ok(TargetResolution::Infeasible {
                applied_rule_ids,
                warnings,
            });
        }
        SolveOutcome::Solved(weights) => weights,
    };

    let mut class_weights: BTreeMap<String, Decimal> = BTreeMap::new();
    for (ct, weight) in model.targets.iter().zip(class_solution.iter()) {
        class_weights.insert(ct.class.clone(), *weight);
    }

    // ---- Phase D (symbol layer): split each class budget, project under caps ----
    let mut symbol_weights: BTreeMap<String, Decimal> = BTreeMap::new();
    for ct in &model.targets {
        if ct.class == *cash {
            continue; // cash holds no tradable symbol
        }
        let budget = *class_weights.get(&ct.class).expect("solved");
        // Split the class budget across this class's TRADABLE symbols only, so a
        // non-tradable symbol's share flows to its tradable peers (matching the
        // engine's rule-free equal-split). A missing symbol does NOT park its
        // budget in cash here — that was the P3a carry-forward inconsistency.
        let symbols: Vec<&String> = model
            .universe
            .iter()
            .filter(|a| a.class == ct.class && tradable.contains(&a.symbol))
            .map(|a| &a.symbol)
            .collect();
        if symbols.is_empty() {
            continue;
        }
        let solved = split_class_budget(&symbols, budget, &sym_overrides, model.max_position)?;
        for (sym, weight) in symbols.iter().zip(solved.iter()) {
            symbol_weights.insert((*sym).clone(), *weight);
        }
    }

    Ok(TargetResolution::Resolved {
        class_weights,
        symbol_weights,
        applied_rule_ids,
        warnings,
    })
}

/// Split a single class `budget` across its `symbols` (equal weight + overrides),
/// then project the symbol layer onto the box ∩ {Σ = budget}, clamping each
/// symbol to `max_position`. Reuses [`solve_targets`] by scaling the layer to a
/// unit simplex and rescaling the solution back to `budget`.
fn split_class_budget(
    symbols: &[&String],
    budget: Decimal,
    overrides: &BTreeMap<String, SymbolOverride>,
    max_position: Option<Decimal>,
) -> Result<Vec<Decimal>> {
    let n = symbols.len();
    if budget <= dec!(0) {
        return Ok(vec![dec!(0); n]);
    }
    let equal = budget / Decimal::from(n as u64);
    let mut buckets: Vec<SolveBucket> = Vec::with_capacity(n);
    for sym in symbols {
        let ov = overrides.get(*sym);
        let desired_abs = ov.map(|o| o.desired).unwrap_or(equal);
        // Absolute ceiling = tightest of the override cap and max_position; never
        // larger than the class budget (a symbol can't exceed its class budget).
        let mut ceil_abs = budget;
        if let Some(mp) = max_position {
            ceil_abs = ceil_abs.min(mp);
        }
        if let Some(o) = ov {
            if let Some(c) = o.ceiling {
                ceil_abs = ceil_abs.min(c);
            }
        }
        // Scale into the unit simplex (Σ = 1) for the solver. The desired is NOT
        // pre-clamped to the ceiling — the solver enforces the box, and clamping
        // here would weaken the symbol's pull and mis-distribute the redistribution.
        let desired = desired_abs.max(dec!(0)) / budget;
        let ceiling = (ceil_abs / budget).min(dec!(1));
        buckets.push(SolveBucket::new((*sym).clone(), desired, dec!(0), ceiling));
    }
    match solve_targets(&buckets)? {
        SolveOutcome::Infeasible => {
            // The caps cannot absorb the class budget. Deterministic fallback:
            // give every symbol its ceiling and let the class hold the rest in
            // cash (the symbol layer never fabricates over-cap weight).
            Ok(buckets
                .iter()
                .map(|b| (b.ceiling * budget).round_dp(super::solver::WEIGHT_DP))
                .collect())
        }
        SolveOutcome::Solved(unit) => Ok(unit
            .iter()
            .map(|u| (*u * budget).round_dp(super::solver::WEIGHT_DP))
            .collect()),
    }
}

fn pick_offset(name: &str, cash: &str) -> String {
    if name.trim().is_empty() {
        cash.to_string()
    } else {
        name.to_string()
    }
}

#[inline]
fn clamp(v: Decimal, lo: Decimal, hi: Decimal) -> Decimal {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::portfolio_sim::{
        AssetSpec, CashYield, ClassTarget, FillMode, PortfolioModel, RebalanceBandMode,
        RebalanceCadence, WithinClass,
    };

    /// A clean base model: cash .4 / equity .3 (SPY) / bond .3 (IEF), wide bands.
    fn base_model() -> PortfolioModel {
        PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(100000),
            universe: vec![
                AssetSpec::new("SPY", "equity"),
                AssetSpec::new("IEF", "bond"),
            ],
            cash_class: "cash".into(),
            targets: vec![
                ClassTarget::new("cash", dec!(0.4), dec!(0), dec!(1)),
                ClassTarget::new("equity", dec!(0.3), dec!(0), dec!(1)),
                ClassTarget::new("bond", dec!(0.3), dec!(0), dec!(1)),
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
            no_average_down: false,
        }
    }

    fn cw(res: &TargetResolution, class: &str) -> Decimal {
        match res {
            TargetResolution::Resolved { class_weights, .. } => class_weights[class],
            _ => panic!("expected Resolved"),
        }
    }
    fn sw(res: &TargetResolution, sym: &str) -> Decimal {
        match res {
            TargetResolution::Resolved { symbol_weights, .. } => {
                symbol_weights.get(sym).copied().unwrap_or(dec!(0))
            }
            _ => panic!("expected Resolved"),
        }
    }

    fn rule(id: &str, then: Action) -> Rule {
        Rule::new(id, Condition::Always, then, 10, RebalanceCadence::Weekly)
    }

    /// Every universe symbol is tradable (the P3a default — all dates present).
    fn all_tradable(m: &PortfolioModel) -> BTreeSet<String> {
        m.universe.iter().map(|a| a.symbol.clone()).collect()
    }

    /// (a) Empty rules → base_policy weights unchanged.
    #[test]
    fn empty_rules_unchanged() {
        let m = base_model();
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&[]).unwrap();
        assert_eq!(cw(&res, "cash"), dec!(0.40000000));
        assert_eq!(cw(&res, "equity"), dec!(0.30000000));
        assert_eq!(cw(&res, "bond"), dec!(0.30000000));
        assert_eq!(sw(&res, "SPY"), dec!(0.30000000));
        assert_eq!(sw(&res, "IEF"), dec!(0.30000000));
    }

    /// (b) SetTarget{Class(bond), 0.5} pins bond; cash/equity renormalize.
    /// Pinned bond = 0.5; remaining 0.5 split: desired cash .4, equity .3 →
    /// project onto Σ=0.5 → cash .3, equity .2.
    #[test]
    fn set_target_class_anchor() {
        let m = base_model();
        let rules = vec![rule(
            "anchor-bond",
            Action::SetTarget {
                key: TargetKey::Class("bond".into()),
                weight: dec!(0.5),
            },
        )];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert_eq!(cw(&res, "bond"), dec!(0.50000000));
        assert_eq!(cw(&res, "cash"), dec!(0.30000000));
        assert_eq!(cw(&res, "equity"), dec!(0.20000000));
        assert_eq!(sw(&res, "IEF"), dec!(0.50000000));
        assert_eq!(sw(&res, "SPY"), dec!(0.20000000));
    }

    /// (c) Tilt{equity, +0.10, from=cash} → cash −0.10, equity +0.10, exact.
    #[test]
    fn tilt_is_zero_sum_exact() {
        let m = base_model();
        let rules = vec![rule(
            "tilt-eq",
            Action::Tilt {
                class: "equity".into(),
                by: dec!(0.10),
                from: "cash".into(),
                to: "cash".into(),
            },
        )];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert_eq!(cw(&res, "cash"), dec!(0.30000000));
        assert_eq!(cw(&res, "equity"), dec!(0.40000000));
        assert_eq!(cw(&res, "bond"), dec!(0.30000000));
        assert_eq!(sw(&res, "SPY"), dec!(0.40000000));
    }

    /// (d) GateBlock{Symbol(BTC)} → BTC 0, redistributed within hard_money.
    /// hard_money budget 0.2 over {BTC, GOLD}: gate BTC → GOLD takes 0.2.
    #[test]
    fn gate_symbol_redistributes_within_class() {
        let mut m = base_model();
        m.universe = vec![
            AssetSpec::new("BTC", "hard_money"),
            AssetSpec::new("GOLD", "hard_money"),
        ];
        m.targets = vec![
            ClassTarget::new("cash", dec!(0.8), dec!(0), dec!(1)),
            ClassTarget::new("hard_money", dec!(0.2), dec!(0), dec!(1)),
        ];
        let rules = vec![rule(
            "gate-btc",
            Action::GateBlock {
                key: TargetKey::Symbol("BTC".into()),
            },
        )];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert_eq!(cw(&res, "hard_money"), dec!(0.20000000));
        assert_eq!(sw(&res, "BTC"), dec!(0));
        assert_eq!(sw(&res, "GOLD"), dec!(0.20000000));
    }

    /// (e) Exit{symbol} → that symbol 0, peer takes the class budget.
    #[test]
    fn exit_zeros_symbol() {
        let mut m = base_model();
        m.universe = vec![
            AssetSpec::new("BTC", "hard_money"),
            AssetSpec::new("GOLD", "hard_money"),
        ];
        m.targets = vec![
            ClassTarget::new("cash", dec!(0.8), dec!(0), dec!(1)),
            ClassTarget::new("hard_money", dec!(0.2), dec!(0), dec!(1)),
        ];
        let rules = vec![rule("exit-btc", Action::Exit { symbol: "BTC".into() })];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert_eq!(sw(&res, "BTC"), dec!(0));
        assert_eq!(sw(&res, "GOLD"), dec!(0.20000000));
    }

    /// (f) max_position forces the symbol-layer projection to clamp + redistribute.
    /// equity budget 0.5 over {SPY, VTI}, max_position 0.3, Add SPY up_to 0.45 →
    /// desired [0.45, 0.25] projected onto Σ=0.5 under ceiling 0.3 → SPY 0.3, VTI 0.2.
    #[test]
    fn max_position_clamps_and_redistributes() {
        let mut m = base_model();
        m.universe = vec![
            AssetSpec::new("SPY", "equity"),
            AssetSpec::new("VTI", "equity"),
        ];
        m.targets = vec![
            ClassTarget::new("cash", dec!(0.5), dec!(0), dec!(1)),
            ClassTarget::new("equity", dec!(0.5), dec!(0), dec!(1)),
        ];
        m.max_position = Some(dec!(0.3));
        let rules = vec![rule(
            "add-spy",
            Action::Add {
                symbol: "SPY".into(),
                up_to: dec!(0.45),
            },
        )];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert_eq!(sw(&res, "SPY"), dec!(0.30000000));
        assert_eq!(sw(&res, "VTI"), dec!(0.20000000));
    }

    /// (g) An infeasible rule set → Infeasible (engine holds). Pin equity 0.9 while
    /// cash floor 0.2 + bond floor 0.2 force Σfloor = 1.3 > 1.
    #[test]
    fn infeasible_rule_set() {
        let mut m = base_model();
        m.targets = vec![
            ClassTarget::new("cash", dec!(0.4), dec!(0.2), dec!(1)),
            ClassTarget::new("equity", dec!(0.3), dec!(0), dec!(1)),
            ClassTarget::new("bond", dec!(0.3), dec!(0.2), dec!(1)),
        ];
        let rules = vec![rule(
            "pin-eq",
            Action::SetTarget {
                key: TargetKey::Class("equity".into()),
                weight: dec!(0.9),
            },
        )];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert!(matches!(res, TargetResolution::Infeasible { .. }));
    }

    /// Determinism: priority/id ordering is stable; repeated resolves are equal.
    #[test]
    fn deterministic_order() {
        let m = base_model();
        let rules = vec![
            Rule::new(
                "b-tilt",
                Condition::Always,
                Action::Tilt {
                    class: "equity".into(),
                    by: dec!(0.05),
                    from: "cash".into(),
                    to: "cash".into(),
                },
                10,
                RebalanceCadence::Weekly,
            ),
            Rule::new(
                "a-tilt",
                Condition::Always,
                Action::Tilt {
                    class: "bond".into(),
                    by: dec!(0.05),
                    from: "cash".into(),
                    to: "cash".into(),
                },
                5,
                RebalanceCadence::Weekly,
            ),
        ];
        let r1 = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        let r2 = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        assert_eq!(r1, r2);
        // cash .4 −0.05 −0.05 = .3; equity .35; bond .35.
        assert_eq!(cw(&r1, "cash"), dec!(0.30000000));
        assert_eq!(cw(&r1, "equity"), dec!(0.35000000));
        assert_eq!(cw(&r1, "bond"), dec!(0.35000000));
    }

    /// Same-class, same-priority double anchor emits a hygiene warning.
    #[test]
    fn double_anchor_warns() {
        let m = base_model();
        let rules = vec![
            Rule::new(
                "anchor-a",
                Condition::Always,
                Action::SetTarget {
                    key: TargetKey::Class("bond".into()),
                    weight: dec!(0.4),
                },
                10,
                RebalanceCadence::Weekly,
            ),
            Rule::new(
                "anchor-b",
                Condition::Always,
                Action::SetTarget {
                    key: TargetKey::Class("bond".into()),
                    weight: dec!(0.5),
                },
                10,
                RebalanceCadence::Weekly,
            ),
        ];
        let res = resolve_targets(&m, &BTreeMap::new(), &all_tradable(&m),&rules).unwrap();
        if let TargetResolution::Resolved { warnings, .. } = &res {
            assert!(warnings.iter().any(|w| w.contains("twice at priority")));
        } else {
            panic!("expected Resolved");
        }
        // Last write (priority tie → id order, anchor-b) wins: bond 0.5.
        assert_eq!(cw(&res, "bond"), dec!(0.50000000));
    }

    /// Stub condition evaluation.
    #[test]
    fn condition_eval_stub() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        let mut ctx = EvalContext::stub(d(2024, 6, 15), 4);
        assert!(Condition::Always.eval(&mut ctx).unwrap());
        assert!(!Condition::Never.eval(&mut ctx).unwrap());
        assert!(Condition::AfterDate(d(2024, 6, 1)).eval(&mut ctx).unwrap());
        assert!(!Condition::AfterDate(d(2024, 7, 1)).eval(&mut ctx).unwrap());
        assert!(Condition::BeforeDate(d(2024, 7, 1)).eval(&mut ctx).unwrap());
        assert!(!Condition::BeforeDate(d(2024, 6, 1)).eval(&mut ctx).unwrap());
        // index 4, n=2, offset=0 → (4-0)%2==0 → fires.
        assert!(Condition::EveryNthRebalance { n: 2, offset: 0 }
            .eval(&mut ctx)
            .unwrap());
        // index 4, n=2, offset=1 → (4-1)%2==1 → no.
        assert!(!Condition::EveryNthRebalance { n: 2, offset: 1 }
            .eval(&mut ctx)
            .unwrap());
    }

    /// A signal rule with NO signal environment is an engine bug, not a silent
    /// skip — it must error rather than quietly evaluate to false.
    #[test]
    fn signal_without_env_errors() {
        let d = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let mut ctx = EvalContext::stub(d, 0);
        let cond = Condition::Signal(Box::new(crate::analytics::portfolio_sim::rule_expr::Expr::Num(
            1.0,
        )));
        assert!(cond.eval(&mut ctx).is_err());
    }

    /// The P3a carry-forward fix: when a class budget is split, a NON-tradable
    /// symbol's share flows to its tradable peer instead of leaking to cash.
    /// hard_money budget 0.2 over {BTC, GOLD}; BTC non-tradable on T (omitted
    /// from the tradable set) → GOLD takes the whole 0.2 (a fired rule path).
    #[test]
    fn non_tradable_symbol_share_redistributes_to_peers() {
        let mut m = base_model();
        m.universe = vec![
            AssetSpec::new("BTC", "hard_money"),
            AssetSpec::new("GOLD", "hard_money"),
        ];
        m.targets = vec![
            ClassTarget::new("cash", dec!(0.8), dec!(0), dec!(1)),
            ClassTarget::new("hard_money", dec!(0.2), dec!(0), dec!(1)),
        ];
        // A trivially-firing rule routes through the symbol_targets (rule) path.
        let rules = vec![rule(
            "tilt-noop",
            Action::Tilt {
                class: "hard_money".into(),
                by: dec!(0),
                from: "cash".into(),
                to: "cash".into(),
            },
        )];
        // Only GOLD is tradable on this date.
        let tradable: BTreeSet<String> = ["GOLD".to_string()].into_iter().collect();
        let res = resolve_targets(&m, &BTreeMap::new(), &tradable, &rules).unwrap();
        assert_eq!(cw(&res, "hard_money"), dec!(0.20000000));
        // BTC absent (non-tradable) → 0; GOLD absorbs the full class budget.
        assert_eq!(sw(&res, "BTC"), dec!(0));
        assert_eq!(sw(&res, "GOLD"), dec!(0.20000000));
    }
}
