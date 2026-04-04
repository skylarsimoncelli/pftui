use std::collections::HashMap;
use std::sync::LazyLock;

static NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // Commodities
        ("GC=F", "Gold"),
        ("SI=F", "Silver"),
        ("CL=F", "Crude Oil"),
        ("BZ=F", "Brent Crude"),
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
        ("HOOD", "Robinhood"),
        ("RKLB", "Rocket Lab"),
        // Market indices
        ("^GSPC", "S&P 500"),
        ("^NDX", "Nasdaq 100"),
        ("^IXIC", "Nasdaq Comp"),
        ("^DJI", "Dow Jones"),
        ("^RUT", "Russell 2000"),
        ("^VIX", "CBOE VIX"),
        // Treasury yields
        ("^TNX", "10Y Treasury"),
        ("^TYX", "30Y Treasury"),
        ("^FVX", "5Y Treasury"),
        ("^IRX", "13W T-Bill"),
        // Dollar index
        ("DX-Y.NYB", "Dollar Index"),
        ("DXY", "Dollar Index"),
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
        ("ITA", "Aerospace & Defense ETF"),
        ("XAR", "S&P Aerospace & Defense ETF"),
        ("PPA", "Aerospace & Defense ETF"),
        ("HYG", "High Yield Corp"),
        ("LQD", "Invest Grade Corp"),
        // Forex
        ("GBPUSD=X", "GBP/USD"),
        ("EURUSD=X", "EUR/USD"),
        ("JPY=X", "USD/JPY"),
        ("CNY=X", "USD/CNY"),
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
    NAMES.get(symbol).map(|s| s.to_string()).unwrap_or_default()
}

/// Compute a fuzzy match score for `query` against a candidate string.
/// Returns 0 if no match. Higher = better match.
fn fuzzy_score(query: &str, candidate: &str) -> u32 {
    let q = query.to_lowercase();
    let c = candidate.to_lowercase();

    // Exact match
    if q == c {
        return 100;
    }

    // Prefix match
    if c.starts_with(&q) {
        return 80;
    }

    // Word-start match: query matches the start of any word in candidate
    // e.g., "depot" matches "Home Depot", "j" matches "Johnson & J"
    for word in c.split(|ch: char| !ch.is_alphanumeric()) {
        if !word.is_empty() && word.starts_with(&q) {
            return 65;
        }
    }

    // Substring match
    if c.contains(&q) {
        return 50;
    }

    // Subsequence match: all query chars appear in order in candidate
    // e.g., "btc" in "bitcoin cash" (b...t...c)
    if q.len() >= 2 {
        let mut q_iter = q.chars();
        let mut current = q_iter.next();
        for ch in c.chars() {
            if let Some(qch) = current {
                if ch == qch {
                    current = q_iter.next();
                }
            } else {
                break;
            }
        }
        if current.is_none() {
            // Score based on how tight the match is (shorter candidate = better)
            let len_ratio = (q.len() * 20) / c.len();
            return 10 + len_ratio as u32;
        }
    }

    0
}

/// Search for assets by fuzzy match on ticker or name (case-insensitive).
/// Matches via: exact, prefix, word-start, substring, and subsequence.
/// Results ranked by match quality. Returns (ticker, display_name) pairs.
pub fn search_names(query: &str) -> Vec<(&'static str, &'static str)> {
    if query.trim().is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(u32, &'static str, &'static str)> = NAMES
        .iter()
        .filter_map(|(ticker, name)| {
            let ticker_score = fuzzy_score(query, ticker);
            let name_score = fuzzy_score(query, name);
            let best = ticker_score.max(name_score);
            if best > 0 {
                Some((best, *ticker, *name))
            } else {
                None
            }
        })
        .collect();

    // Sort by score descending, then alphabetically by ticker
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));

    scored.into_iter().map(|(_, t, n)| (t, n)).collect()
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

    // Forex pairs (=X suffix) and Dollar Index (DX-Y.NYB / DXY alias)
    if upper.ends_with("=X") {
        return AssetCategory::Forex;
    }
    if upper == "DX-Y.NYB" || upper == "DXY" {
        return AssetCategory::Forex;
    }

    // Market indices (^ prefix: ^GSPC, ^VIX, ^TNX, ^DJI, etc.)
    if upper.starts_with('^') {
        return AssetCategory::Fund;
    }

    // Known crypto (check coingecko mapping)
    if coingecko::ticker_to_coingecko_id(&upper).is_some() {
        return AssetCategory::Crypto;
    }

    // Known ETFs/Funds
    if matches!(
        upper.as_str(),
        "SPY"
            | "QQQ"
            | "IWM"
            | "VTI"
            | "VOO"
            | "VT"
            | "VXUS"
            | "BND"
            | "GLD"
            | "SLV"
            | "TLT"
            | "IEF"
            | "ARKK"
            | "XLE"
            | "XLF"
            | "ITA"
            | "XAR"
            | "PPA"
            | "URA"
            | "URNM"
            | "HYG"
            | "LQD"
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

    #[test]
    fn search_names_substring_match() {
        // "old" should match "Gold" and "Gold ETF" via substring on name
        let results = search_names("old");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"GC=F")); // Gold
        assert!(tickers.contains(&"GLD")); // Gold ETF
    }

    #[test]
    fn search_names_word_start_match() {
        // "depot" matches "Home Depot" via word-start
        let results = search_names("depot");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"HD"));
    }

    #[test]
    fn search_names_subsequence_match() {
        // "nflx" doesn't prefix-match "Netflix" but subsequence matches
        // Actually NFLX is a ticker so it prefix-matches. Use a different example.
        // "slna" subsequence matches "Solana" (s-l-n-a in s-o-l-a-n-a)
        let results = search_names("slna");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"SOL"));
    }

    #[test]
    fn search_names_empty_query() {
        let results = search_names("");
        assert!(results.is_empty());
        let results = search_names("   ");
        assert!(results.is_empty());
    }

    #[test]
    fn search_names_ranking_exact_over_prefix() {
        // "ETH" exact ticker should rank above "EIGEN" (prefix on name "EigenLayer" = no,
        // but "ETH" exact = 100 vs others)
        let results = search_names("ETH");
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "ETH");
    }

    #[test]
    fn search_names_ranking_prefix_over_substring() {
        // "Gold" prefix-matches the name "Gold" (GC=F) and "Gold ETF" (GLD)
        // Both are prefix score 80. "Palladium" contains no "Gold".
        let results = search_names("Gold");
        assert!(results.len() >= 2);
        // First results should be the prefix matches
        let top2: Vec<&str> = results.iter().take(2).map(|(t, _)| *t).collect();
        assert!(top2.contains(&"GC=F") || top2.contains(&"GLD"));
    }

    #[test]
    fn fuzzy_score_exact() {
        assert_eq!(fuzzy_score("btc", "BTC"), 100);
        assert_eq!(fuzzy_score("Bitcoin", "bitcoin"), 100);
    }

    #[test]
    fn fuzzy_score_prefix() {
        assert_eq!(fuzzy_score("bit", "Bitcoin"), 80);
        assert_eq!(fuzzy_score("AA", "AAPL"), 80);
    }

    #[test]
    fn fuzzy_score_word_start() {
        assert_eq!(fuzzy_score("depot", "Home Depot"), 65);
    }

    #[test]
    fn fuzzy_score_substring() {
        assert_eq!(fuzzy_score("old", "Gold"), 50);
        // "inu" matches word-start of "Inu" in "Shiba Inu" → 65
        assert_eq!(fuzzy_score("inu", "Shiba Inu"), 65);
        // True substring: "hib" is mid-word in "Shiba"
        assert_eq!(fuzzy_score("hib", "Shiba Inu"), 50);
    }

    #[test]
    fn fuzzy_score_subsequence() {
        let score = fuzzy_score("slna", "Solana");
        assert!(
            score > 0 && score < 50,
            "subsequence score should be 10-30, got {}",
            score
        );
    }

    #[test]
    fn fuzzy_score_no_match() {
        assert_eq!(fuzzy_score("xyz", "Bitcoin"), 0);
        assert_eq!(fuzzy_score("zzz", "Apple"), 0);
    }

    #[test]
    fn resolve_name_macro_indicators() {
        assert_eq!(resolve_name("^GSPC"), "S&P 500");
        assert_eq!(resolve_name("^VIX"), "CBOE VIX");
        assert_eq!(resolve_name("^TNX"), "10Y Treasury");
        assert_eq!(resolve_name("^TYX"), "30Y Treasury");
        assert_eq!(resolve_name("^FVX"), "5Y Treasury");
        assert_eq!(resolve_name("^IRX"), "13W T-Bill");
        assert_eq!(resolve_name("DX-Y.NYB"), "Dollar Index");
        assert_eq!(resolve_name("DXY"), "Dollar Index");
        assert_eq!(resolve_name("GBPUSD=X"), "GBP/USD");
        assert_eq!(resolve_name("EURUSD=X"), "EUR/USD");
    }

    #[test]
    fn resolve_name_additional_assets() {
        assert_eq!(resolve_name("^NDX"), "Nasdaq 100");
        assert_eq!(resolve_name("^DJI"), "Dow Jones");
        assert_eq!(resolve_name("^RUT"), "Russell 2000");
        assert_eq!(resolve_name("BZ=F"), "Brent Crude");
        assert_eq!(resolve_name("HOOD"), "Robinhood");
        assert_eq!(resolve_name("RKLB"), "Rocket Lab");
        assert_eq!(resolve_name("HYG"), "High Yield Corp");
        assert_eq!(resolve_name("LQD"), "Invest Grade Corp");
        assert_eq!(resolve_name("JPY=X"), "USD/JPY");
        assert_eq!(resolve_name("CNY=X"), "USD/CNY");
    }

    #[test]
    fn infer_category_market_indices() {
        assert_eq!(infer_category("^GSPC"), AssetCategory::Fund);
        assert_eq!(infer_category("^VIX"), AssetCategory::Fund);
        assert_eq!(infer_category("^TNX"), AssetCategory::Fund);
        assert_eq!(infer_category("^DJI"), AssetCategory::Fund);
        assert_eq!(infer_category("^RUT"), AssetCategory::Fund);
        assert_eq!(infer_category("^TYX"), AssetCategory::Fund);
        assert_eq!(infer_category("^FVX"), AssetCategory::Fund);
        assert_eq!(infer_category("^IRX"), AssetCategory::Fund);
    }

    #[test]
    fn infer_category_dollar_index() {
        assert_eq!(infer_category("DX-Y.NYB"), AssetCategory::Forex);
        assert_eq!(infer_category("DXY"), AssetCategory::Forex);
        assert_eq!(infer_category("dx-y.nyb"), AssetCategory::Forex);
        assert_eq!(infer_category("dxy"), AssetCategory::Forex);
    }

    #[test]
    fn infer_category_credit_etfs() {
        assert_eq!(infer_category("HYG"), AssetCategory::Fund);
        assert_eq!(infer_category("LQD"), AssetCategory::Fund);
    }

    #[test]
    fn search_names_finds_macro_indicators() {
        let results = search_names("VIX");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"^VIX"));

        let results = search_names("Dollar");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"DX-Y.NYB") || tickers.contains(&"DXY"));

        let results = search_names("Treasury");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"^TNX"));
    }

    #[test]
    fn search_names_finds_gbpusd() {
        let results = search_names("GBP/USD");
        let tickers: Vec<&str> = results.iter().map(|(t, _)| *t).collect();
        assert!(tickers.contains(&"GBPUSD=X"));
    }
}
