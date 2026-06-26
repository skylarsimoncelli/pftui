//! Price-panel loader for the positioning simulator (POSITIONING-MODELS.md §3.3).
//!
//! Bridges a close-series source (the `price_history` table in production, or a
//! synthetic in-memory map in tests) to the simulator's [`PricePanel`]. The
//! source is abstracted behind [`CloseSeriesLoader`] so the whole load→simulate
//! path is testable WITHOUT a database (and without ever touching real money
//! data — only market closes).
//!
//! ## FX-symbol convention (assumption, documented)
//! Non-USD instruments need an FX series to mark/fill in `base_currency`. The
//! `price_history` table follows Yahoo's FX ticker convention `"{FROM}{TO}=X"`
//! (verified in `src/price/yahoo.rs`: `format!("{}USD=X", ccy)` and
//! `models/asset_names.rs`: `GBPUSD=X`, `EURUSD=X`). So to convert one unit of
//! `CCY` into `BASE` this loader looks up symbol `"{CCY}{BASE}=X"` (e.g. base
//! USD, GBP asset → `GBPUSD=X`). The series is read as TO-per-FROM units, matching
//! [`PricePanel::insert_fx`]. If that series is missing, the load ERRORS clearly
//! rather than silently marking the leg at FX=1.

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::actions::Condition;
use super::{rule_expr, CashYield, PortfolioModel, PricePanel};
use crate::regime::REGIME_YAHOO_SYMBOLS;

/// Minimum number of closes a tradable symbol must have for the backtest to be
/// honest (a 1-bar series can't even produce a next-close fill).
const MIN_BARS: usize = 2;

/// A source of `(date, close)` series for a symbol. Closes are `Decimal` (parsed
/// from the TEXT-stored prices). Returns an EMPTY vec for an unknown symbol
/// (the loader turns "no history" into a clear error with context).
pub trait CloseSeriesLoader {
    fn load_closes(&self, symbol: &str) -> Result<Vec<(NaiveDate, Decimal)>>;
}

/// FX ticker for converting `from_ccy` → `to_ccy` under the Yahoo convention.
pub fn fx_symbol(from_ccy: &str, to_ccy: &str) -> String {
    format!("{from_ccy}{to_ccy}=X")
}

/// Load every series the model needs into a [`PricePanel`], windowed to
/// `[from, to]` (inclusive; `None` = open-ended). Loads: each universe symbol's
/// closes; the cash-yield proxy's closes (if any); and an FX series per distinct
/// non-base price currency.
///
/// Errors clearly when a tradable symbol has no/too-short history in-window, or
/// when a required FX series is missing — never runs a degenerate backtest.
pub fn load_panel(
    loader: &dyn CloseSeriesLoader,
    model: &PortfolioModel,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<PricePanel> {
    let in_window =
        |d: NaiveDate| from.map(|f| d >= f).unwrap_or(true) && to.map(|t| d <= t).unwrap_or(true);

    let mut panel = PricePanel::new();
    let base = model.base_currency.as_str();

    // --- universe symbols ---
    for a in &model.universe {
        let series: Vec<(NaiveDate, Decimal)> = loader
            .load_closes(&a.symbol)?
            .into_iter()
            .filter(|(d, _)| in_window(*d))
            .collect();
        if series.is_empty() {
            bail!(
                "no price history for universe symbol '{}' in the requested window",
                a.symbol
            );
        }
        if series.len() < MIN_BARS {
            bail!(
                "universe symbol '{}' has only {} bar(s) in-window (need >= {}); refusing a degenerate backtest",
                a.symbol,
                series.len(),
                MIN_BARS
            );
        }
        panel.insert_series(a.symbol.clone(), series);
    }

    // --- cash-yield proxy (looked up directly; not part of the universe) ---
    if let CashYield::Proxy(sym) = &model.cash_yield {
        let series: Vec<(NaiveDate, Decimal)> = loader
            .load_closes(sym)?
            .into_iter()
            .filter(|(d, _)| in_window(*d))
            .collect();
        if series.is_empty() {
            bail!(
                "cash_yield_proxy '{}' has no price history in the requested window",
                sym
            );
        }
        panel.insert_series(sym.clone(), series);
    }

    // --- FX series for each distinct non-base price currency ---
    let mut currencies: BTreeSet<String> = BTreeSet::new();
    for a in &model.universe {
        if a.price_currency != base {
            currencies.insert(a.price_currency.clone());
        }
    }
    for ccy in &currencies {
        let sym = fx_symbol(ccy, base);
        let series: Vec<(NaiveDate, Decimal)> = loader
            .load_closes(&sym)?
            .into_iter()
            .filter(|(d, _)| in_window(*d))
            .collect();
        if series.is_empty() {
            bail!(
                "missing FX series '{}' needed to convert {}-priced assets into base {} (expected the Yahoo '{{FROM}}{{TO}}=X' ticker)",
                sym,
                ccy,
                base
            );
        }
        panel.insert_fx(ccy, base, series);
    }

    // --- macro REGIME series (only when a rule references a regime accessor) ---
    // The `regime_score()` accessor reads the cross-asset `REGIME_SYMBOLS` out of
    // the panel by their known tickers; they are NOT in the model universe, so the
    // panel must carry them. We load them on-demand (a model with no regime rule
    // pays nothing). If a regime rule IS present but a macro series can't be
    // sourced, FAIL LOUDLY here — never let the score silently read as 0/NaN.
    if model_uses_regime(model) {
        for &sym in REGIME_YAHOO_SYMBOLS {
            let series: Vec<(NaiveDate, Decimal)> = loader
                .load_closes(sym)?
                .into_iter()
                .filter(|(d, _)| in_window(*d))
                .collect();
            if series.is_empty() {
                bail!(
                    "a rule references regime_score() but macro symbol '{}' has no price history in the requested window (the regime composite needs all of: {})",
                    sym,
                    REGIME_YAHOO_SYMBOLS.join(", ")
                );
            }
            panel.insert_series(sym, series);
        }
    }

    Ok(panel)
}

/// Does any compiled rule's `when` reference a `regime_score()` accessor?
fn model_uses_regime(model: &PortfolioModel) -> bool {
    model.rules.iter().any(|r| match &r.when {
        Condition::Signal(expr) => rule_expr::uses_regime(expr),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    struct MapLoader(HashMap<String, Vec<(NaiveDate, Decimal)>>);
    impl CloseSeriesLoader for MapLoader {
        fn load_closes(&self, symbol: &str) -> Result<Vec<(NaiveDate, Decimal)>> {
            Ok(self.0.get(symbol).cloned().unwrap_or_default())
        }
    }

    fn usd_model() -> PortfolioModel {
        use super::super::{
            AssetSpec, ClassTarget, FillMode, RebalanceBandMode, RebalanceCadence, WithinClass,
        };
        PortfolioModel {
            base_currency: "USD".into(),
            initial_capital: dec!(1000),
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
            rules: vec![],
            no_average_down: false,
        }
    }

    #[test]
    fn loads_usd_panel() {
        let mut m = HashMap::new();
        m.insert(
            "SPY".to_string(),
            vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 2), dec!(101))],
        );
        let loader = MapLoader(m);
        let panel = load_panel(&loader, &usd_model(), None, None).unwrap();
        assert_eq!(panel.close_on("SPY", d(2024, 1, 2)), Some(dec!(101)));
    }

    #[test]
    fn errors_on_missing_history() {
        let loader = MapLoader(HashMap::new());
        let err = load_panel(&loader, &usd_model(), None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no price history"), "got: {err}");
    }

    #[test]
    fn errors_on_too_short_history() {
        let mut m = HashMap::new();
        m.insert("SPY".to_string(), vec![(d(2024, 1, 1), dec!(100))]);
        let loader = MapLoader(m);
        let err = load_panel(&loader, &usd_model(), None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("degenerate") || err.contains("bar"), "got: {err}");
    }

    #[test]
    fn loads_fx_for_non_base_currency() {
        use super::super::AssetSpec;
        let mut model = usd_model();
        model.universe = vec![AssetSpec::with_currency("LON", "equity", "GBP")];
        let mut m = HashMap::new();
        m.insert(
            "LON".to_string(),
            vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 2), dec!(100))],
        );
        m.insert(
            "GBPUSD=X".to_string(),
            vec![(d(2024, 1, 1), dec!(1.25)), (d(2024, 1, 2), dec!(1.30))],
        );
        let loader = MapLoader(m);
        let panel = load_panel(&loader, &model, None, None).unwrap();
        assert_eq!(panel.fx_rate("GBP", "USD", d(2024, 1, 2)), Some(dec!(1.30)));
    }

    /// End-to-end through the SAME path the CLI uses (resolve_str → load_panel →
    /// simulate) on a SYNTHETIC panel — no DB, only fabricated market closes.
    /// Asserts a sane report and determinism.
    #[test]
    fn spec_to_panel_to_simulate_is_sane_and_deterministic() {
        use super::super::engine::simulate;
        use super::super::spec::resolve_str;

        let spec = r#"
[model]
name = "syn-50-50"
version = 1
base_currency = "USD"
initial_capital = 100000

[universe]
assets = [ { symbol = "SYA", class = "equity" }, { symbol = "SYB", class = "bond" } ]
cash_class = "cash"

[base_policy]
targets = [ { class = "cash", target = 0.0, floor = 0.0, ceiling = 1.0 },
            { class = "equity", target = 0.5, floor = 0.0, ceiling = 1.0 },
            { class = "bond", target = 0.5, floor = 0.0, ceiling = 1.0 } ]
within_class = "equal"

[constraints]
rebalance_cadence = "monthly"
rebalance_band_mode = "to_target"
fill = "next_close"
commission_pct = 0.001
"#;
        let rm = resolve_str(spec).unwrap();

        // Synthetic monotone-up panel across three months.
        let mut m = HashMap::new();
        m.insert(
            "SYA".to_string(),
            vec![
                (d(2024, 1, 2), dec!(100)),
                (d(2024, 2, 1), dec!(110)),
                (d(2024, 3, 1), dec!(121)),
                (d(2024, 4, 1), dec!(133)),
            ],
        );
        m.insert(
            "SYB".to_string(),
            vec![
                (d(2024, 1, 2), dec!(50)),
                (d(2024, 2, 1), dec!(51)),
                (d(2024, 3, 1), dec!(52)),
                (d(2024, 4, 1), dec!(53)),
            ],
        );
        let loader = MapLoader(m);
        let panel = load_panel(&loader, &rm.model, None, None).unwrap();

        let a = simulate(&rm.model, &panel).unwrap();
        assert!(!a.daily_equity_curve.is_empty());
        assert!(a.n_rebalances >= 1);
        // Up market → positive final equity above start, finite metrics.
        let final_eq = a.daily_equity_curve.last().unwrap().equity;
        assert!(final_eq > dec!(0));
        assert!(a.metrics.cagr_pct.is_finite());
        assert!(a.metrics.max_drawdown_pct >= 0.0);

        // Determinism: byte-identical serialized report on a repeat run.
        let b = simulate(&rm.model, &panel).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    /// A regime rule on the model → load_panel ALSO sources every macro
    /// REGIME_SYMBOL into the panel (so `regime_score()` can read them as-of).
    #[test]
    fn loads_regime_symbols_when_rule_present() {
        use super::super::actions::{Action, Condition, Rule, TargetKey};
        use super::super::rule_expr::{CmpOp, Expr};
        let mut model = usd_model();
        // when = regime_score() <= -2  → set cash target to 0.6 (arbitrary action).
        let when = Condition::Signal(Box::new(Expr::Compare {
            op: CmpOp::Le,
            lhs: Box::new(Expr::Accessor {
                name: "regime_score".into(),
                args: vec![],
                tf: None,
            }),
            rhs: Box::new(Expr::Num(-2.0)),
        }));
        model.rules = vec![Rule::new(
            "risk-off",
            when,
            Action::SetTarget {
                key: TargetKey::Class("cash".into()),
                weight: dec!(0.6),
            },
            10,
            super::super::RebalanceCadence::Weekly,
        )];

        let mut m = HashMap::new();
        m.insert("SPY".to_string(), vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 2), dec!(101))]);
        for &sym in crate::regime::REGIME_YAHOO_SYMBOLS {
            m.insert(
                sym.to_string(),
                vec![(d(2024, 1, 1), dec!(10)), (d(2024, 1, 2), dec!(11))],
            );
        }
        let loader = MapLoader(m);
        let panel = load_panel(&loader, &model, None, None).unwrap();
        for &sym in crate::regime::REGIME_YAHOO_SYMBOLS {
            assert!(
                panel.close_on(sym, d(2024, 1, 2)).is_some(),
                "macro symbol '{sym}' must be loaded into the panel"
            );
        }
    }

    /// A regime rule but a missing macro series → load_panel fails LOUDLY (never a
    /// silent 0/NaN regime read).
    #[test]
    fn errors_when_regime_symbol_missing() {
        use super::super::actions::{Action, Condition, Rule, TargetKey};
        use super::super::rule_expr::{CmpOp, Expr};
        let mut model = usd_model();
        model.rules = vec![Rule::new(
            "risk-off",
            Condition::Signal(Box::new(Expr::Compare {
                op: CmpOp::Le,
                lhs: Box::new(Expr::Accessor {
                    name: "regime_score".into(),
                    args: vec![],
                    tf: None,
                }),
                rhs: Box::new(Expr::Num(-2.0)),
            })),
            Action::SetTarget {
                key: TargetKey::Class("cash".into()),
                weight: dec!(0.6),
            },
            10,
            super::super::RebalanceCadence::Weekly,
        )];
        // SPY present, but NO macro series at all.
        let mut m = HashMap::new();
        m.insert("SPY".to_string(), vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 2), dec!(101))]);
        let loader = MapLoader(m);
        let err = load_panel(&loader, &model, None, None).unwrap_err().to_string();
        assert!(err.contains("regime_score()") && err.contains("no price history"), "got: {err}");
    }

    #[test]
    fn errors_on_missing_fx() {
        use super::super::AssetSpec;
        let mut model = usd_model();
        model.universe = vec![AssetSpec::with_currency("LON", "equity", "GBP")];
        let mut m = HashMap::new();
        m.insert(
            "LON".to_string(),
            vec![(d(2024, 1, 1), dec!(100)), (d(2024, 1, 2), dec!(100))],
        );
        let loader = MapLoader(m);
        let err = load_panel(&loader, &model, None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("FX"), "got: {err}");
    }
}
