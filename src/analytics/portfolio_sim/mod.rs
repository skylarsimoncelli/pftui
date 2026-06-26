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

pub mod engine;
pub mod solver;

#[allow(unused_imports)]
pub use engine::{simulate, DailyEquityPoint, Order, PortfolioBacktestReport, RebalanceEvent, Side};
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

/// A symbol in the model's universe, tagged with its class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSpec {
    pub symbol: String,
    pub class: String,
}

impl AssetSpec {
    pub fn new(symbol: impl Into<String>, class: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            class: class.into(),
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

/// A P0 portfolio model: a fixed diversification structure with no rules.
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
}

/// In-memory price panel: per-symbol `date → close`. The simulator's master
/// calendar is the **union** of all symbols' dates (never anchored to one asset).
#[derive(Debug, Clone, Default)]
pub struct PricePanel {
    closes: BTreeMap<String, BTreeMap<chrono::NaiveDate, Decimal>>,
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

    /// Sorted union of all symbols' trading dates — the master calendar.
    pub fn calendar(&self) -> Vec<chrono::NaiveDate> {
        let mut set: std::collections::BTreeSet<chrono::NaiveDate> = std::collections::BTreeSet::new();
        for m in self.closes.values() {
            set.extend(m.keys().copied());
        }
        set.into_iter().collect()
    }
}
