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
}
