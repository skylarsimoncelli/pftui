use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::models::asset::AssetCategory;
use crate::models::asset_names::infer_category;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioPreset {
    Oil100,
    Btc40k,
    Gold6000,
    Gfc2008,
    OilCrisis1973,
}

/// Metadata for a built-in stress-test preset.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

/// Return metadata for every built-in preset.
pub fn list_presets() -> Vec<PresetInfo> {
    vec![
        PresetInfo {
            name: "Oil $100".to_string(),
            aliases: vec!["oil100".into(), "oilat100".into(), "oil100usd".into()],
            description: "Oil spikes to $100 — equities -4%, funds -3%, commodities +5%"
                .to_string(),
        },
        PresetInfo {
            name: "BTC 40k".to_string(),
            aliases: vec!["btc40k".into(), "bitcoin40k".into(), "btc40000".into()],
            description: "Bitcoin crashes to $40k — crypto -30%, equities -6%".to_string(),
        },
        PresetInfo {
            name: "Gold $6000".to_string(),
            aliases: vec!["gold6000".into(), "gold6k".into(), "gold6000usd".into()],
            description: "Gold surges to $6000 — commodities +22%, equities -12%".to_string(),
        },
        PresetInfo {
            name: "2008 GFC".to_string(),
            aliases: vec!["2008gfc".into(), "gfc2008".into()],
            description:
                "2008 financial crisis replay — equities -40%, funds -30%, crypto -55%, oil -35%, gold +15%"
                    .to_string(),
        },
        PresetInfo {
            name: "1973 Oil Crisis".to_string(),
            aliases: vec!["1973oilcrisis".into(), "oilcrisis1973".into()],
            description:
                "1973 oil crisis replay — oil +120%, commodities +35%, equities -32%, funds -25%, crypto -20%"
                    .to_string(),
        },
    ]
}

pub fn parse_preset(name: &str) -> Option<ScenarioPreset> {
    let key = normalize(name);
    match key.as_str() {
        "oil100" | "oilat100" | "oil100usd" => Some(ScenarioPreset::Oil100),
        "btc40k" | "bitcoin40k" | "btc40000" => Some(ScenarioPreset::Btc40k),
        "gold6000" | "gold6k" | "gold6000usd" => Some(ScenarioPreset::Gold6000),
        "2008gfc" | "gfc2008" => Some(ScenarioPreset::Gfc2008),
        "1973oilcrisis" | "oilcrisis1973" => Some(ScenarioPreset::OilCrisis1973),
        _ => None,
    }
}

pub fn apply_preset(
    preset: ScenarioPreset,
    prices: &HashMap<String, Decimal>,
) -> HashMap<String, Decimal> {
    let mut overrides = HashMap::new();
    match preset {
        ScenarioPreset::Oil100 => {
            apply_selector_target(&mut overrides, prices, "oil", dec!(100));
            apply_selector_pct_shock(&mut overrides, prices, "equity", dec!(-4));
            apply_selector_pct_shock(&mut overrides, prices, "fund", dec!(-3));
            apply_selector_pct_shock(&mut overrides, prices, "commodity", dec!(5));
        }
        ScenarioPreset::Btc40k => {
            apply_selector_target(&mut overrides, prices, "btc", dec!(40000));
            apply_selector_pct_shock(&mut overrides, prices, "crypto", dec!(-30));
            apply_selector_pct_shock(&mut overrides, prices, "equity", dec!(-6));
        }
        ScenarioPreset::Gold6000 => {
            apply_selector_target(&mut overrides, prices, "gold", dec!(6000));
            apply_selector_pct_shock(&mut overrides, prices, "commodity", dec!(22));
            apply_selector_pct_shock(&mut overrides, prices, "equity", dec!(-12));
        }
        ScenarioPreset::Gfc2008 => {
            apply_selector_pct_shock(&mut overrides, prices, "equity", dec!(-40));
            apply_selector_pct_shock(&mut overrides, prices, "fund", dec!(-30));
            apply_selector_pct_shock(&mut overrides, prices, "crypto", dec!(-55));
            apply_selector_pct_shock(&mut overrides, prices, "oil", dec!(-35));
            apply_selector_pct_shock(&mut overrides, prices, "gold", dec!(15));
        }
        ScenarioPreset::OilCrisis1973 => {
            apply_selector_pct_shock(&mut overrides, prices, "oil", dec!(120));
            apply_selector_pct_shock(&mut overrides, prices, "commodity", dec!(35));
            apply_selector_pct_shock(&mut overrides, prices, "equity", dec!(-32));
            apply_selector_pct_shock(&mut overrides, prices, "fund", dec!(-25));
            apply_selector_pct_shock(&mut overrides, prices, "crypto", dec!(-20));
        }
    }
    overrides
}

pub fn apply_selector_target(
    overrides: &mut HashMap<String, Decimal>,
    prices: &HashMap<String, Decimal>,
    selector: &str,
    target: Decimal,
) -> usize {
    let symbols = match_selector(selector, prices);
    for symbol in &symbols {
        overrides.insert(symbol.clone(), target);
    }
    symbols.len()
}

pub fn apply_selector_pct_shock(
    overrides: &mut HashMap<String, Decimal>,
    prices: &HashMap<String, Decimal>,
    selector: &str,
    pct_shock: Decimal,
) -> usize {
    let symbols = match_selector(selector, prices);
    let multiplier = dec!(1) + (pct_shock / dec!(100));
    for symbol in &symbols {
        let base = overrides
            .get(symbol)
            .copied()
            .or_else(|| prices.get(symbol).copied())
            .unwrap_or(Decimal::ZERO);
        let shocked = (base * multiplier).max(Decimal::ZERO);
        overrides.insert(symbol.clone(), shocked);
    }
    symbols.len()
}

pub fn match_selector(selector: &str, prices: &HashMap<String, Decimal>) -> Vec<String> {
    let upper = selector.trim().to_uppercase();
    if upper.is_empty() {
        return Vec::new();
    }

    if prices.contains_key(&upper) {
        return vec![upper];
    }

    prices
        .keys()
        .filter(|s| selector_matches_symbol(&upper, s))
        .cloned()
        .collect()
}

fn selector_matches_symbol(selector_upper: &str, symbol: &str) -> bool {
    let symbol_upper = symbol.to_uppercase();
    match selector_upper {
        "ALL" | "PORTFOLIO" => true,
        "BTC" | "BITCOIN" => symbol_upper.contains("BTC"),
        "GOLD" => symbol_upper == "GC=F" || symbol_upper == "GLD" || symbol_upper.contains("XAU"),
        "OIL" | "CRUDE" => {
            symbol_upper == "CL=F"
                || symbol_upper == "BZ=F"
                || symbol_upper == "USO"
                || symbol_upper == "XLE"
        }
        "CRYPTO" => infer_category(&symbol_upper) == AssetCategory::Crypto,
        "COMMODITY" | "COMMODITIES" => infer_category(&symbol_upper) == AssetCategory::Commodity,
        "EQUITY" | "STOCK" | "STOCKS" => infer_category(&symbol_upper) == AssetCategory::Equity,
        "FUND" | "ETF" => infer_category(&symbol_upper) == AssetCategory::Fund,
        "FOREX" | "FX" => infer_category(&symbol_upper) == AssetCategory::Forex,
        "CASH" => infer_category(&symbol_upper) == AssetCategory::Cash,
        _ => false,
    }
}

fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_prices() -> HashMap<String, Decimal> {
        HashMap::from([
            ("BTC".to_string(), dec!(90000)),
            ("GC=F".to_string(), dec!(2200)),
            ("CL=F".to_string(), dec!(78)),
            ("AAPL".to_string(), dec!(210)),
            ("SPY".to_string(), dec!(500)),
        ])
    }

    #[test]
    fn parses_presets() {
        assert_eq!(parse_preset("Oil $100"), Some(ScenarioPreset::Oil100));
        assert_eq!(parse_preset("BTC 40k"), Some(ScenarioPreset::Btc40k));
        assert_eq!(parse_preset("2008 GFC"), Some(ScenarioPreset::Gfc2008));
    }

    #[test]
    fn selector_matches_categories() {
        let prices = sample_prices();
        let crypto = match_selector("crypto", &prices);
        assert_eq!(crypto.len(), 1);
        assert!(crypto.contains(&"BTC".to_string()));

        let commodity = match_selector("commodity", &prices);
        assert!(commodity.contains(&"GC=F".to_string()));
        assert!(commodity.contains(&"CL=F".to_string()));
    }

    #[test]
    fn applies_pct_shock() {
        let prices = sample_prices();
        let mut ov = HashMap::new();
        let n = apply_selector_pct_shock(&mut ov, &prices, "equity", dec!(-10));
        assert_eq!(n, 1);
        assert_eq!(ov.get("AAPL"), Some(&dec!(189)));
    }

    #[test]
    fn applies_named_scenario() {
        let prices = sample_prices();
        let ov = apply_preset(ScenarioPreset::Btc40k, &prices);
        assert_eq!(ov.get("BTC"), Some(&dec!(28000))); // 40k then -30%
    }

    #[test]
    fn list_presets_returns_all() {
        let presets = list_presets();
        assert_eq!(presets.len(), 5);
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Oil $100"));
        assert!(names.contains(&"BTC 40k"));
        assert!(names.contains(&"Gold $6000"));
        assert!(names.contains(&"2008 GFC"));
        assert!(names.contains(&"1973 Oil Crisis"));
    }

    #[test]
    fn list_presets_all_parseable() {
        let presets = list_presets();
        for p in &presets {
            assert!(
                parse_preset(&p.name).is_some(),
                "Preset name '{}' should be parseable by parse_preset()",
                p.name
            );
        }
    }
}
