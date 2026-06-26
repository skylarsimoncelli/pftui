//! P0 portfolio positioning simulator — minimum-correct, in-memory backtest of a
//! fixed-weight portfolio model. See `docs/POSITIONING-MODELS.md` §3.3/§3.4 and
//! the "P0" stage in §4.
//!
//! Scope (P0): USD-only, a small universe of USD-priced symbols + cash, **fixed**
//! class targets with floor/ceiling, weekly or monthly cadence, `to_target`
//! rebalancing, next-close fills with commission + slippage, a `Decimal` ledger,
//! and a daily mark-to-market equity curve. Deliberately **excludes** TOML/DB,
//! the signal DSL, FX, `no_average_down`, Monte-Carlo, benchmarks and the CLI —
//! those are P1+.
//!
//! The two load-bearing correctness properties:
//! 1. Deterministic **bounded-projection** solve with infeasibility detection
//!    ([`solver::solve_targets`]).
//! 2. **Lookahead-safe** timing: a rebalance decision uses data through date `T`'s
//!    close, but each order fills at that symbol's **next** tradable close.

// The P0 module is a complete, tested engine but has no production consumer yet
// (the `analytics models` CLI lands in P1). Allow dead code so the public API can
// exist ahead of its wiring without polluting the workspace warning budget.
#![allow(dead_code)]

pub mod accessors;
pub mod actions;
pub mod engine;
pub mod loader;
pub mod metrics;
pub mod optimize;
pub mod rule_expr;
pub mod solver;
pub mod spec;
pub mod stage_proxy;

#[allow(unused_imports)]
pub use actions::{
    resolve_targets, Action, Condition, EvalContext, Rule, SignalEnv, TargetKey, TargetResolution,
};

#[allow(unused_imports)]
pub use engine::{
    simulate, BenchmarkResult, Benchmarks, DailyEquityPoint, Order, PortfolioBacktestReport,
    RebalanceEvent, Side,
};
#[allow(unused_imports)]
pub use metrics::PortfolioMetrics;
#[allow(unused_imports)]
pub use solver::{solve_targets, SolveBucket, SolveOutcome};

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// How a class's weight is split across the symbols in that class. P0 supports
/// only equal weighting across the *tradable* symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WithinClass {
    Equal,
}

/// Rebalance cadence. P0 supports weekly (first trading day of each ISO week
/// present in the calendar) and monthly (first trading day of each calendar
/// month).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebalanceCadence {
    Weekly,
    Monthly,
}

/// Band mode. P0 supports only `ToTarget` (always move every leg to its target).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebalanceBandMode {
    ToTarget,
}

/// Fill timing. P0 supports only the lookahead-safe next-close fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillMode {
    NextClose,
}

/// A symbol in the model's universe, tagged with its class and the currency its
/// prices are quoted in (P1: marks/fills FX-convert this → `base_currency`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSpec {
    pub symbol: String,
    pub class: String,
    /// Currency the panel quotes this symbol's closes in. Defaults to `"USD"`;
    /// use [`AssetSpec::with_currency`] for a non-USD instrument.
    pub price_currency: String,
}

impl AssetSpec {
    /// USD-priced asset (P0-compatible default).
    pub fn new(symbol: impl Into<String>, class: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            class: class.into(),
            price_currency: "USD".to_string(),
        }
    }

    /// Asset priced in `price_currency`.
    pub fn with_currency(
        symbol: impl Into<String>,
        class: impl Into<String>,
        price_currency: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            class: class.into(),
            price_currency: price_currency.into(),
        }
    }
}

/// A fixed class target with its box constraints. Fractions in `0..=1`; the full
/// set (cash included) must sum to 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassTarget {
    pub class: String,
    pub target: Decimal,
    pub floor: Decimal,
    pub ceiling: Decimal,
}

impl ClassTarget {
    pub fn new(
        class: impl Into<String>,
        target: Decimal,
        floor: Decimal,
        ceiling: Decimal,
    ) -> Self {
        Self {
            class: class.into(),
            target,
            floor,
            ceiling,
        }
    }
}

/// How the cash bucket earns (or doesn't) carry between days.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CashYield {
    /// Cash earns nothing (P0 behaviour).
    #[default]
    None,
    /// Cash earns the daily return of this panel symbol (e.g. `BIL`).
    Proxy(String),
}

/// A P0/P1 portfolio model: a fixed diversification structure with no rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioModel {
    pub base_currency: String,
    /// Starting capital, all held as cash on the first calendar date.
    pub initial_capital: Decimal,
    pub universe: Vec<AssetSpec>,
    pub cash_class: String,
    /// Fixed class targets, including the cash class. Must sum (with cash) to 1.
    pub targets: Vec<ClassTarget>,
    pub within_class: WithinClass,
    pub rebalance_cadence: RebalanceCadence,
    pub rebalance_band_mode: RebalanceBandMode,
    pub fill: FillMode,
    /// Commission as a fraction of |fill notional|, debited from cash.
    pub commission_pct: Decimal,
    /// Slippage as a fraction of the fill close (buys pay more, sells receive less).
    pub slippage_pct: Decimal,
    /// How the cash bucket accrues carry. Default `None` preserves P0 behaviour.
    #[serde(default)]
    pub cash_yield: CashYield,
    /// Per-symbol absolute weight ceiling, enforced by the symbol-layer
    /// projection in [`actions::resolve_targets`]. `None` → no cap (P0/P1
    /// behaviour preserved).
    #[serde(default)]
    pub max_position: Option<Decimal>,
    /// Rebalancing rules (P3a action algebra). Default empty → byte-identical to
    /// the rule-free base_policy path.
    #[serde(default)]
    pub rules: Vec<actions::Rule>,
    /// `no_average_down` veto (POSITIONING-MODELS.md §3.2 step 8): when true, the
    /// engine BLOCKS a BUY leg for a symbol whose position is already open and
    /// whose fill price (base) is below the position's average entry cost (base) —
    /// "don't add below your average". Default `false` preserves prior behaviour.
    #[serde(default)]
    pub no_average_down: bool,
}

/// In-memory price panel: per-symbol `date → close`. The simulator's master
/// calendar is the **union** of all symbols' dates (never anchored to one asset).
#[derive(Debug, Clone, Default)]
pub struct PricePanel {
    closes: BTreeMap<String, BTreeMap<chrono::NaiveDate, Decimal>>,
    /// FX series keyed by ordered pair `"<FROM><TO>"` (e.g. `"GBPUSD"` = how many
    /// units of TO one unit of FROM buys), `date → rate`.
    fx: BTreeMap<String, BTreeMap<chrono::NaiveDate, Decimal>>,
}

impl PricePanel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a full close series for a symbol.
    pub fn insert_series(
        &mut self,
        symbol: impl Into<String>,
        series: impl IntoIterator<Item = (chrono::NaiveDate, Decimal)>,
    ) {
        let map = self.closes.entry(symbol.into()).or_default();
        for (d, c) in series {
            map.insert(d, c);
        }
    }

    /// Close for `symbol` on exactly `date`, if present.
    pub fn close_on(&self, symbol: &str, date: chrono::NaiveDate) -> Option<Decimal> {
        self.closes.get(symbol).and_then(|m| m.get(&date)).copied()
    }

    /// Every `(date, close)` for `symbol` with `date <= cutoff`, oldest-first.
    /// The signal-accessor adapter uses this to build a COMPLETED-bucket-trimmed
    /// daily history (see [`accessors::completed_bucket_history`]).
    pub fn closes_through(
        &self,
        symbol: &str,
        cutoff: chrono::NaiveDate,
    ) -> Vec<(chrono::NaiveDate, Decimal)> {
        self.closes
            .get(symbol)
            .map(|m| {
                m.range((std::ops::Bound::Unbounded, std::ops::Bound::Included(cutoff)))
                    .map(|(d, c)| (*d, *c))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The first `(date, close)` for `symbol` strictly after `after`.
    pub fn next_tradable(
        &self,
        symbol: &str,
        after: chrono::NaiveDate,
    ) -> Option<(chrono::NaiveDate, Decimal)> {
        self.closes.get(symbol).and_then(|m| {
            m.range((
                std::ops::Bound::Excluded(after),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .map(|(d, c)| (*d, *c))
        })
    }

    /// Insert an FX series for an ordered currency pair. `pair` is the
    /// concatenation `"<FROM><TO>"` (e.g. `"GBPUSD"`), and each `rate` is the
    /// number of TO units per one FROM unit on that date.
    pub fn insert_fx(
        &mut self,
        from_ccy: &str,
        to_ccy: &str,
        series: impl IntoIterator<Item = (chrono::NaiveDate, Decimal)>,
    ) {
        let key = format!("{from_ccy}{to_ccy}");
        let map = self.fx.entry(key).or_default();
        for (d, r) in series {
            map.insert(d, r);
        }
    }

    /// FX rate converting one unit of `from_ccy` into `to_ccy` **as of `date`**
    /// (the mark/fill date — never the decision date). Same currency → `1`.
    /// Looks up the pair on/just-before `date` (carry-forward last known rate);
    /// falls back to the inverse of the reverse pair. `None` if neither known.
    pub fn fx_rate(
        &self,
        from_ccy: &str,
        to_ccy: &str,
        date: chrono::NaiveDate,
    ) -> Option<Decimal> {
        if from_ccy == to_ccy {
            return Some(Decimal::ONE);
        }
        let as_of = |key: &str| -> Option<Decimal> {
            self.fx.get(key).and_then(|m| {
                m.range((std::ops::Bound::Unbounded, std::ops::Bound::Included(date)))
                    .next_back()
                    .map(|(_, r)| *r)
            })
        };
        if let Some(r) = as_of(&format!("{from_ccy}{to_ccy}")) {
            return Some(r);
        }
        // Inverse pair: invert the reverse rate.
        as_of(&format!("{to_ccy}{from_ccy}")).and_then(|r| {
            if r != Decimal::ZERO {
                Some(Decimal::ONE / r)
            } else {
                None
            }
        })
    }

    /// Sorted union of all symbols' trading dates — the master calendar.
    pub fn calendar(&self) -> Vec<chrono::NaiveDate> {
        let mut set: std::collections::BTreeSet<chrono::NaiveDate> = std::collections::BTreeSet::new();
        for m in self.closes.values() {
            set.extend(m.keys().copied());
        }
        set.into_iter().collect()
    }
}
