//! Regime Asset Suggestions — maps regime score to historically strong/weak asset classes.
//!
//! Based on the composite regime score from `mod.rs`, this module provides
//! context about which asset categories historically perform well or poorly
//! in the current macro environment. This is NOT investment advice — it is
//! regime context for informational purposes.
//!
//! Portfolio-aware: flags how the user's current holdings align with the regime.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use crate::models::asset::AssetCategory;
use crate::models::position::Position;
use crate::regime::RegimeScore;

/// Full regime suggestion output.
#[derive(Debug, Clone)]
pub struct RegimeSuggestions {
    /// Assets historically strong in the current regime.
    pub strong: Vec<&'static str>,
    /// Assets historically weak in the current regime.
    pub weak: Vec<&'static str>,
    /// Portfolio alignment summary (None if no positions).
    pub alignment: Option<PortfolioAlignment>,
}

/// How the user's portfolio aligns with the current regime.
#[derive(Debug, Clone)]
pub struct PortfolioAlignment {
    /// Percentage of portfolio in regime-strong categories.
    pub strong_pct: Decimal,
    /// Percentage of portfolio in regime-weak categories.
    pub weak_pct: Decimal,
    /// Human-readable summary.
    pub summary: String,
}

/// Categories that historically perform well in risk-on regimes.
const RISK_ON_STRONG: &[&str] = &[
    "Growth stocks",
    "Crypto",
    "High-yield bonds",
    "Copper",
    "Emerging markets",
];

/// Categories that historically perform poorly in risk-on regimes.
const RISK_ON_WEAK: &[&str] = &[
    "Gold",
    "Treasuries",
    "USD",
    "Utilities",
    "Defensive equities",
];

/// Categories that historically perform well in risk-off regimes.
const RISK_OFF_STRONG: &[&str] = &[
    "Gold",
    "Silver",
    "Treasuries",
    "USD",
    "Utilities",
];

/// Categories that historically perform poorly in risk-off regimes.
const RISK_OFF_WEAK: &[&str] = &[
    "Growth stocks",
    "Crypto",
    "High-yield bonds",
    "Copper",
    "Emerging markets",
];

/// Compute regime-based asset suggestions.
pub fn compute_suggestions(
    regime: &RegimeScore,
    positions: &[Position],
) -> RegimeSuggestions {
    if !regime.has_data() {
        return RegimeSuggestions {
            strong: Vec::new(),
            weak: Vec::new(),
            alignment: None,
        };
    }

    let (strong, weak) = match regime.total {
        5..=9 => (RISK_ON_STRONG.to_vec(), RISK_ON_WEAK.to_vec()),
        2..=4 => (
            // Lean risk-on: narrower set
            vec!["Growth stocks", "Crypto", "Copper"],
            vec!["Gold", "Treasuries", "USD"],
        ),
        -1..=1 => (
            // Neutral: mixed signals, hedged is good
            vec!["Diversified", "Balanced"],
            vec![],
        ),
        -4..=-2 => (
            // Lean risk-off: narrower set
            vec!["Gold", "Treasuries", "USD"],
            vec!["Growth stocks", "Crypto", "High-yield bonds"],
        ),
        _ => (RISK_OFF_STRONG.to_vec(), RISK_OFF_WEAK.to_vec()),
    };

    let alignment = compute_alignment(regime, positions);

    RegimeSuggestions {
        strong,
        weak,
        alignment,
    }
}

/// Map pftui AssetCategory to regime-relevant groupings.
fn category_regime_class(category: AssetCategory, regime_total: i8) -> RegimeClass {
    // In risk-on (positive total): Equity/Crypto/Fund are strong, Commodity is mixed
    // In risk-off (negative total): Commodity/Forex/Cash are strong, Equity/Crypto are weak
    let is_risk_on = regime_total >= 2;
    let is_risk_off = regime_total <= -2;

    match category {
        AssetCategory::Equity => {
            if is_risk_on {
                RegimeClass::Strong
            } else if is_risk_off {
                RegimeClass::Weak
            } else {
                RegimeClass::Neutral
            }
        }
        AssetCategory::Crypto => {
            if is_risk_on {
                RegimeClass::Strong
            } else if is_risk_off {
                RegimeClass::Weak
            } else {
                RegimeClass::Neutral
            }
        }
        AssetCategory::Commodity => {
            // Commodities are complex: gold is risk-off, copper is risk-on
            // Treat as neutral since we can't distinguish individual commodities
            RegimeClass::Neutral
        }
        AssetCategory::Fund => {
            // Funds are mixed — depends on underlying
            RegimeClass::Neutral
        }
        AssetCategory::Forex => {
            if is_risk_off {
                RegimeClass::Strong
            } else if is_risk_on {
                RegimeClass::Weak
            } else {
                RegimeClass::Neutral
            }
        }
        AssetCategory::Cash => {
            if is_risk_off {
                RegimeClass::Strong
            } else {
                RegimeClass::Neutral
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegimeClass {
    Strong,
    Weak,
    Neutral,
}

/// Compute how the user's portfolio aligns with the current regime.
fn compute_alignment(
    regime: &RegimeScore,
    positions: &[Position],
) -> Option<PortfolioAlignment> {
    if positions.is_empty() {
        return None;
    }

    let total_value: Decimal = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum();

    if total_value <= dec!(0) {
        return None;
    }

    let mut category_values: HashMap<AssetCategory, Decimal> = HashMap::new();
    for pos in positions {
        if let Some(val) = pos.current_value {
            *category_values.entry(pos.category).or_insert(dec!(0)) += val;
        }
    }

    let mut strong_value = dec!(0);
    let mut weak_value = dec!(0);

    for (&cat, &val) in &category_values {
        match category_regime_class(cat, regime.total) {
            RegimeClass::Strong => strong_value += val,
            RegimeClass::Weak => weak_value += val,
            RegimeClass::Neutral => {}
        }
    }

    let strong_pct = (strong_value * dec!(100)) / total_value;
    let weak_pct = (weak_value * dec!(100)) / total_value;

    let summary = if strong_pct > dec!(50) {
        format!("{}% in regime-favored assets — well positioned", strong_pct.round())
    } else if weak_pct > dec!(50) {
        format!("{}% in regime-headwind assets — exposed", weak_pct.round())
    } else {
        "Balanced across regime factors".to_string()
    };

    Some(PortfolioAlignment {
        strong_pct,
        weak_pct,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regime::RegimeSignal;

    fn make_regime(total: i8, active: u8) -> RegimeScore {
        RegimeScore {
            signals: vec![RegimeSignal {
                name: "test",
                label: "test".into(),
                score: total.signum(),
            }],
            total,
            active_count: active,
        }
    }

    fn make_position(symbol: &str, category: AssetCategory, value: Decimal) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: dec!(1),
            avg_cost: value,
            total_cost: value,
            currency: "USD".to_string(),
            current_price: Some(value),
            current_value: Some(value),
            gain: Some(dec!(0)),
            gain_pct: Some(dec!(0)),
            allocation_pct: None,
            native_currency: None,
            fx_rate: None,
        }
    }

    #[test]
    fn risk_on_suggests_growth() {
        let regime = make_regime(7, 9);
        let suggestions = compute_suggestions(&regime, &[]);
        assert!(suggestions.strong.contains(&"Growth stocks"));
        assert!(suggestions.strong.contains(&"Crypto"));
        assert!(suggestions.weak.contains(&"Gold"));
        assert!(suggestions.weak.contains(&"Treasuries"));
    }

    #[test]
    fn risk_off_suggests_gold() {
        let regime = make_regime(-6, 9);
        let suggestions = compute_suggestions(&regime, &[]);
        assert!(suggestions.strong.contains(&"Gold"));
        assert!(suggestions.strong.contains(&"Treasuries"));
        assert!(suggestions.weak.contains(&"Crypto"));
        assert!(suggestions.weak.contains(&"Growth stocks"));
    }

    #[test]
    fn neutral_regime_balanced() {
        let regime = make_regime(0, 5);
        let suggestions = compute_suggestions(&regime, &[]);
        assert!(suggestions.strong.contains(&"Diversified"));
        assert!(suggestions.weak.is_empty());
    }

    #[test]
    fn lean_risk_on_narrower() {
        let regime = make_regime(3, 7);
        let suggestions = compute_suggestions(&regime, &[]);
        assert_eq!(suggestions.strong.len(), 3);
        assert_eq!(suggestions.weak.len(), 3);
    }

    #[test]
    fn lean_risk_off_narrower() {
        let regime = make_regime(-3, 7);
        let suggestions = compute_suggestions(&regime, &[]);
        assert_eq!(suggestions.strong.len(), 3);
        assert_eq!(suggestions.weak.len(), 3);
    }

    #[test]
    fn no_data_returns_empty() {
        let regime = make_regime(0, 0);
        let suggestions = compute_suggestions(&regime, &[]);
        assert!(suggestions.strong.is_empty());
        assert!(suggestions.weak.is_empty());
        assert!(suggestions.alignment.is_none());
    }

    #[test]
    fn alignment_well_positioned_risk_on() {
        let regime = make_regime(7, 9);
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(7000)),
            make_position("BTC", AssetCategory::Crypto, dec!(3000)),
        ];
        let suggestions = compute_suggestions(&regime, &positions);
        let alignment = suggestions.alignment.unwrap();
        // All equity + crypto = strong in risk-on
        assert_eq!(alignment.strong_pct, dec!(100));
        assert_eq!(alignment.weak_pct, dec!(0));
        assert!(alignment.summary.contains("well positioned"));
    }

    #[test]
    fn alignment_exposed_risk_off() {
        let regime = make_regime(-6, 9);
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(8000)),
            make_position("BTC", AssetCategory::Crypto, dec!(2000)),
        ];
        let suggestions = compute_suggestions(&regime, &positions);
        let alignment = suggestions.alignment.unwrap();
        // All equity + crypto = weak in risk-off
        assert_eq!(alignment.weak_pct, dec!(100));
        assert!(alignment.summary.contains("exposed"));
    }

    #[test]
    fn alignment_balanced_mixed() {
        let regime = make_regime(5, 9);
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(4000)),
            make_position("GC=F", AssetCategory::Commodity, dec!(3000)),
            make_position("USD", AssetCategory::Cash, dec!(3000)),
        ];
        let suggestions = compute_suggestions(&regime, &positions);
        let alignment = suggestions.alignment.unwrap();
        // Equity=strong (40%), Commodity=neutral, Cash=neutral
        assert_eq!(alignment.strong_pct, dec!(40));
    }

    #[test]
    fn alignment_empty_positions() {
        let regime = make_regime(5, 9);
        let suggestions = compute_suggestions(&regime, &[]);
        assert!(suggestions.alignment.is_none());
    }

    #[test]
    fn category_classes_risk_on() {
        assert_eq!(
            category_regime_class(AssetCategory::Equity, 5),
            RegimeClass::Strong
        );
        assert_eq!(
            category_regime_class(AssetCategory::Crypto, 5),
            RegimeClass::Strong
        );
        assert_eq!(
            category_regime_class(AssetCategory::Forex, 5),
            RegimeClass::Weak
        );
        assert_eq!(
            category_regime_class(AssetCategory::Commodity, 5),
            RegimeClass::Neutral
        );
    }

    #[test]
    fn category_classes_risk_off() {
        assert_eq!(
            category_regime_class(AssetCategory::Equity, -5),
            RegimeClass::Weak
        );
        assert_eq!(
            category_regime_class(AssetCategory::Crypto, -5),
            RegimeClass::Weak
        );
        assert_eq!(
            category_regime_class(AssetCategory::Forex, -5),
            RegimeClass::Strong
        );
        assert_eq!(
            category_regime_class(AssetCategory::Cash, -5),
            RegimeClass::Strong
        );
    }

    #[test]
    fn category_classes_neutral() {
        assert_eq!(
            category_regime_class(AssetCategory::Equity, 0),
            RegimeClass::Neutral
        );
        assert_eq!(
            category_regime_class(AssetCategory::Cash, 0),
            RegimeClass::Neutral
        );
    }
}
