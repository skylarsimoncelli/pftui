use std::collections::HashMap;
use std::sync::LazyLock;

static NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // Commodities
        ("GC=F", "Gold"),
        ("SI=F", "Silver"),
        ("CL=F", "Crude Oil"),
        ("PL=F", "Platinum"),
        ("PA=F", "Palladium"),
        ("HG=F", "Copper"),
        ("NG=F", "Natural Gas"),
        ("UX1!", "Uranium"),
        ("URA", "Uranium ETF"),
        ("URNM", "Uranium Miners"),
        ("U.UN", "Sprott Uranium"),
        ("ZW=F", "Wheat"),
        ("ZC=F", "Corn"),
        ("ZS=F", "Soybeans"),
        // Major crypto
        ("BTC", "Bitcoin"),
        ("ETH", "Ethereum"),
        ("SOL", "Solana"),
        ("ADA", "Cardano"),
        ("DOT", "Polkadot"),
        ("DOGE", "Dogecoin"),
        ("AVAX", "Avalanche"),
        ("MATIC", "Polygon"),
        ("POL", "Polygon"),
        ("LINK", "Chainlink"),
        ("UNI", "Uniswap"),
        ("ATOM", "Cosmos"),
        ("XRP", "Ripple"),
        ("LTC", "Litecoin"),
        ("BCH", "Bitcoin Cash"),
        ("NEAR", "NEAR"),
        ("FIL", "Filecoin"),
        ("APT", "Aptos"),
        ("ARB", "Arbitrum"),
        ("OP", "Optimism"),
        ("SUI", "Sui"),
        ("SEI", "Sei"),
        ("TIA", "Celestia"),
        ("INJ", "Injective"),
        ("RENDER", "Render"),
        ("RNDR", "Render"),
        ("FET", "Fetch.ai"),
        ("GRT", "The Graph"),
        ("AAVE", "Aave"),
        ("MKR", "Maker"),
        ("CRV", "Curve"),
        ("SNX", "Synthetix"),
        ("COMP", "Compound"),
        ("LDO", "Lido"),
        ("RPL", "Rocket Pool"),
        ("PEPE", "Pepe"),
        ("SHIB", "Shiba Inu"),
        ("BONK", "Bonk"),
        ("WIF", "dogwifhat"),
        ("JUP", "Jupiter"),
        ("RAY", "Raydium"),
        ("ONDO", "Ondo"),
        ("PENDLE", "Pendle"),
        ("ENA", "Ethena"),
        ("EIGEN", "EigenLayer"),
        ("STRK", "Starknet"),
        ("ZK", "zkSync"),
        ("W", "Wormhole"),
        ("JTO", "Jito"),
        ("TRX", "Tron"),
        ("TON", "Toncoin"),
        ("BNB", "BNB"),
        ("XLM", "Stellar"),
        ("ALGO", "Algorand"),
        // Major equities
        ("AAPL", "Apple"),
        ("MSFT", "Microsoft"),
        ("GOOGL", "Alphabet"),
        ("GOOG", "Alphabet"),
        ("AMZN", "Amazon"),
        ("NVDA", "NVIDIA"),
        ("META", "Meta"),
        ("TSLA", "Tesla"),
        ("BRK-B", "Berkshire B"),
        ("JPM", "JPMorgan"),
        ("V", "Visa"),
        ("UNH", "UnitedHealth"),
        ("MA", "Mastercard"),
        ("JNJ", "Johnson & J"),
        ("PG", "Procter & G"),
        ("HD", "Home Depot"),
        ("DIS", "Disney"),
        ("NFLX", "Netflix"),
        ("AMD", "AMD"),
        ("INTC", "Intel"),
        ("CRM", "Salesforce"),
        ("PYPL", "PayPal"),
        ("COIN", "Coinbase"),
        ("MSTR", "MicroStrat"),
        ("PLTR", "Palantir"),
        // Popular ETFs/Funds
        ("SPY", "S&P 500"),
        ("QQQ", "Nasdaq 100"),
        ("IWM", "Russell 2000"),
        ("VTI", "Total Market"),
        ("VOO", "S&P 500"),
        ("VT", "Total World"),
        ("VXUS", "Intl Stock"),
        ("BND", "Total Bond"),
        ("GLD", "Gold ETF"),
        ("SLV", "Silver ETF"),
        ("TLT", "20yr Treasury"),
        ("IEF", "7-10yr Treasury"),
        ("ARKK", "ARK Innov"),
        ("XLE", "Energy ETF"),
        ("XLF", "Financial ETF"),
        // Forex
        ("USDGBP=X", "USD/GBP"),
        ("USDEUR=X", "USD/EUR"),
        ("USDJPY=X", "USD/JPY"),
        ("USDCAD=X", "USD/CAD"),
        ("USDAUD=X", "USD/AUD"),
        ("USDCHF=X", "USD/CHF"),
        // Currencies (cash)
        ("USD", "US Dollar"),
        ("GBP", "Pound"),
        ("EUR", "Euro"),
        ("JPY", "Yen"),
        ("CAD", "Can Dollar"),
        ("AUD", "Aus Dollar"),
        ("CHF", "Swiss Franc"),
    ])
});

pub fn resolve_name(symbol: &str) -> String {
    NAMES
        .get(symbol)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Search for assets by prefix match on ticker or name (case-insensitive).
/// Returns (ticker, display_name) pairs.
pub fn search_names(query: &str) -> Vec<(&'static str, &'static str)> {
    let query_upper = query.to_uppercase();
    let query_lower = query.to_lowercase();

    let mut results: Vec<(&str, &str)> = NAMES
        .iter()
        .filter(|(ticker, name)| {
            ticker.to_uppercase().starts_with(&query_upper)
                || name.to_lowercase().starts_with(&query_lower)
        })
        .map(|(k, v)| (*k, *v))
        .collect();

    // Sort: exact ticker matches first, then alphabetically
    results.sort_by(|(a_tick, _), (b_tick, _)| {
        let a_exact = a_tick.to_uppercase() == query_upper;
        let b_exact = b_tick.to_uppercase() == query_upper;
        b_exact.cmp(&a_exact).then_with(|| a_tick.cmp(b_tick))
    });

    results
}

use crate::models::asset::AssetCategory;
use crate::price::coingecko;

/// Infer the likely asset category from a symbol.
pub fn infer_category(symbol: &str) -> AssetCategory {
    let upper = symbol.to_uppercase();

    // Cash currencies
    if matches!(
        upper.as_str(),
        "USD" | "GBP" | "EUR" | "JPY" | "CAD" | "AUD" | "CHF" | "NZD" | "SGD" | "HKD"
    ) {
        return AssetCategory::Cash;
    }

    // Futures → Commodity
    if upper.ends_with("=F") || upper.ends_with("!") {
        return AssetCategory::Commodity;
    }

    // Forex pairs
    if upper.ends_with("=X") {
        return AssetCategory::Forex;
    }

    // Known crypto (check coingecko mapping)
    if coingecko::ticker_to_coingecko_id(&upper).is_some() {
        return AssetCategory::Crypto;
    }

    // Known ETFs/Funds
    if matches!(
        upper.as_str(),
        "SPY" | "QQQ" | "IWM" | "VTI" | "VOO" | "VT" | "VXUS" | "BND" | "GLD" | "SLV"
            | "TLT" | "IEF" | "ARKK" | "XLE" | "XLF" | "URA" | "URNM"
    ) {
        return AssetCategory::Fund;
    }

    AssetCategory::Equity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_name_known_symbol() {
        assert_eq!(resolve_name("BTC"), "Bitcoin");
        assert_eq!(resolve_name("AAPL"), "Apple");
        assert_eq!(resolve_name("GC=F"), "Gold");
    }

    #[test]
    fn resolve_name_unknown_symbol() {
        assert_eq!(resolve_name("ZZZZZ"), "");
    }

    #[test]
    fn infer_category_cash() {
        assert_eq!(infer_category("USD"), AssetCategory::Cash);
        assert_eq!(infer_category("GBP"), AssetCategory::Cash);
        assert_eq!(infer_category("EUR"), AssetCategory::Cash);
        assert_eq!(infer_category("JPY"), AssetCategory::Cash);
    }

    #[test]
    fn infer_category_commodity() {
        assert_eq!(infer_category("GC=F"), AssetCategory::Commodity);
        assert_eq!(infer_category("CL=F"), AssetCategory::Commodity);
        assert_eq!(infer_category("UX1!"), AssetCategory::Commodity);
    }

    #[test]
    fn infer_category_forex() {
        assert_eq!(infer_category("USDGBP=X"), AssetCategory::Forex);
        assert_eq!(infer_category("USDJPY=X"), AssetCategory::Forex);
    }

    #[test]
    fn infer_category_crypto() {
        assert_eq!(infer_category("BTC"), AssetCategory::Crypto);
        assert_eq!(infer_category("ETH"), AssetCategory::Crypto);
        assert_eq!(infer_category("SOL"), AssetCategory::Crypto);
    }

    #[test]
    fn infer_category_fund() {
        assert_eq!(infer_category("SPY"), AssetCategory::Fund);
        assert_eq!(infer_category("QQQ"), AssetCategory::Fund);
        assert_eq!(infer_category("VTI"), AssetCategory::Fund);
    }

    #[test]
    fn infer_category_equity_default() {
        assert_eq!(infer_category("AAPL"), AssetCategory::Equity);
        assert_eq!(infer_category("MSFT"), AssetCategory::Equity);
        assert_eq!(infer_category("TSLA"), AssetCategory::Equity);
    }

    #[test]
    fn infer_category_case_insensitive() {
        assert_eq!(infer_category("usd"), AssetCategory::Cash);
        assert_eq!(infer_category("btc"), AssetCategory::Crypto);
        assert_eq!(infer_category("gc=f"), AssetCategory::Commodity);
    }

    #[test]
    fn search_names_by_ticker_prefix() {
        let results = search_names("AA");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"AAPL"));
        assert!(tickers.contains(&"AAVE"));
    }

    #[test]
    fn search_names_by_name_prefix() {
        let results = search_names("Bit");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"BTC"));
        assert!(tickers.contains(&"BCH"));
    }

    #[test]
    fn search_names_exact_match_first() {
        let results = search_names("BTC");
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "BTC");
    }

    #[test]
    fn search_names_no_match() {
        let results = search_names("ZZZZZ");
        assert!(results.is_empty());
    }

    #[test]
    fn search_names_case_insensitive() {
        let results_upper = search_names("AAPL");
        let results_lower = search_names("aapl");
        assert!(!results_upper.is_empty());
        assert_eq!(results_upper.len(), results_lower.len());
    }
}
