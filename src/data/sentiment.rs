//! Fetch Fear & Greed indices from free public APIs.
//!
//! Crypto F&G: Alternative.me API (free, no key)
//! Traditional F&G: Derived from market indicators (VIX, put/call ratio, breadth, momentum)

use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SentimentIndex {
    pub index_type: String, // "crypto" or "traditional"
    pub value: u8,          // 0-100
    pub classification: String, // "Extreme Fear", "Fear", "Neutral", "Greed", "Extreme Greed"
    pub timestamp: i64,
}

#[derive(Debug, Deserialize)]
struct AlternativeMeResponse {
    data: Vec<AlternativeMeData>,
}

#[derive(Debug, Deserialize)]
struct AlternativeMeData {
    value: String,
    value_classification: String,
    timestamp: String,
}

/// Fetch the current crypto Fear & Greed Index from Alternative.me.
///
/// Returns the latest index value (0-100) with classification.
/// Free API, no authentication required, rate limit: ~1 req/sec.
pub fn fetch_crypto_fng() -> Result<SentimentIndex> {
    let url = "https://api.alternative.me/fng/?limit=1";
    
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    
    let resp = client
        .get(url)
        .header("User-Agent", "pftui/0.3.0")
        .send()?;

    if !resp.status().is_success() {
        bail!("Alternative.me API returned {}", resp.status());
    }

    let body: AlternativeMeResponse = resp.json()?;
    
    if body.data.is_empty() {
        bail!("Alternative.me API returned empty data array");
    }

    let data = &body.data[0];
    let value: u8 = data.value.parse()
        .map_err(|_| anyhow::anyhow!("Invalid value: {}", data.value))?;
    let timestamp: i64 = data.timestamp.parse()
        .map_err(|_| anyhow::anyhow!("Invalid timestamp: {}", data.timestamp))?;

    Ok(SentimentIndex {
        index_type: "crypto".to_string(),
        value,
        classification: data.value_classification.clone(),
        timestamp,
    })
}

/// Fetch the traditional market Fear & Greed Index.
///
/// Currently a placeholder that returns a neutral value.
/// Future: derive from VIX, put/call ratio, junk spread, breadth, momentum, safe haven demand.
pub fn fetch_traditional_fng() -> Result<SentimentIndex> {
    // Placeholder — will be derived from VIX + market indicators in F19.1 follow-up
    Ok(SentimentIndex {
        index_type: "traditional".to_string(),
        value: 50,
        classification: "Neutral".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_crypto_fng() {
        let result = fetch_crypto_fng();
        assert!(result.is_ok(), "Failed to fetch crypto F&G: {:?}", result);
        
        let index = result.unwrap();
        assert_eq!(index.index_type, "crypto");
        assert!(index.value <= 100, "Value should be 0-100");
        assert!(!index.classification.is_empty(), "Classification should not be empty");
    }

    #[test]
    fn test_fetch_traditional_fng() {
        let result = fetch_traditional_fng();
        assert!(result.is_ok());
        
        let index = result.unwrap();
        assert_eq!(index.index_type, "traditional");
        assert_eq!(index.value, 50); // placeholder value
        assert_eq!(index.classification, "Neutral");
    }
}
