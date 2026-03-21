//! Polymarket Gamma API client.
//!
//! Fetches prediction market data from Polymarket's free, no-key Gamma API.
//! Returns market questions, outcome probabilities, volume, and category.
//!
//! API: https://gamma-api.polymarket.com/markets

use anyhow::{anyhow, Result};
use serde::Deserialize;

/// A prediction market from Polymarket.
#[derive(Debug, Clone)]
pub struct PredictionMarket {
    pub market_id: String,
    pub question: String,
    pub outcome_yes_price: String, // probability as decimal string (0.0-1.0)
    pub outcome_no_price: String,
    pub volume: String,
    pub category: String,
    pub end_date: String, // ISO8601
}

#[derive(Debug, Deserialize)]
struct PolymarketResponse {
    id: String,
    question: String,
    #[serde(rename = "outcomePrices")]
    outcome_prices: String, // JSON array string: ["0.34", "0.66"]
    volume: String,
    category: Option<String>,
    #[serde(rename = "endDate")]
    end_date: String,
}

/// Fetch active prediction markets from Polymarket Gamma API.
///
/// Filters by:
/// - closed=false (only active markets)
/// - active=true
/// - Category filter (optional)
/// - Limit (default 50, max 100)
///
/// This is a blocking call — run in a background thread if called from TUI.
pub fn fetch_markets(category_filter: Option<&str>, limit: usize) -> Result<Vec<PredictionMarket>> {
    let limit = limit.min(100);
    let mut url = format!(
        "https://gamma-api.polymarket.com/markets?closed=false&active=true&limit={}",
        limit
    );

    if let Some(cat) = category_filter {
        url.push_str(&format!("&category={}", cat));
    }

    // Use blocking reqwest client (no tokio runtime needed)
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client
        .get(&url)
        .send()
        .map_err(|e| anyhow!("Polymarket API request failed: {}", e))?;

    let markets: Vec<PolymarketResponse> = response
        .json()
        .map_err(|e| anyhow!("Failed to parse Polymarket response: {}", e))?;

    let mut results = Vec::new();
    for m in markets {
        // Parse outcome_prices JSON array
        let prices: Vec<String> = serde_json::from_str(&m.outcome_prices)
            .unwrap_or_else(|_| vec!["0.5".to_string(), "0.5".to_string()]);

        let yes_price = prices.first().cloned().unwrap_or_else(|| "0.5".to_string());
        let no_price = prices.get(1).cloned().unwrap_or_else(|| "0.5".to_string());

        results.push(PredictionMarket {
            market_id: m.id,
            question: m.question,
            outcome_yes_price: yes_price,
            outcome_no_price: no_price,
            volume: m.volume,
            category: m.category.unwrap_or_else(|| "Other".to_string()),
            end_date: m.end_date,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_markets_basic() {
        // Live API test — should return at least 1 market
        let result = fetch_markets(None, 5);
        assert!(result.is_ok());
        let markets = result.unwrap();
        assert!(!markets.is_empty(), "Should fetch at least one market");
        assert!(!markets[0].question.is_empty());
    }

    #[test]
    fn test_fetch_markets_crypto_category() {
        let result = fetch_markets(Some("Crypto"), 5);
        assert!(result.is_ok());
    }
}
