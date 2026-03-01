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
