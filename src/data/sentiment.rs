//! Fetch Fear & Greed indices from free public APIs.
//!
//! Crypto F&G: Alternative.me API (free, no key)
//! Traditional F&G: Derived from market indicators (VIX, put/call ratio, breadth, momentum)

use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SentimentIndex {
    pub index_type: String,     // "crypto" or "traditional"
    pub value: u8,              // 0-100
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
    let mut readings = fetch_crypto_fng_history(1)?;
    if readings.is_empty() {
        bail!("Alternative.me API returned empty data array");
    }
    Ok(readings.remove(0))
}

/// Fetch crypto Fear & Greed history from Alternative.me.
///
/// `limit` controls how many readings to request. Alternative.me supports
/// `?limit=0` to return the FULL history (~2018→present), which we use to
/// backfill `sentiment_history` deep enough for historical-analog parallels.
/// Readings are returned newest-first (index 0 is the latest).
/// Free API, no authentication required, rate limit: ~1 req/sec.
pub fn fetch_crypto_fng_history(limit: u32) -> Result<Vec<SentimentIndex>> {
    let url = format!("https://api.alternative.me/fng/?limit={}", limit);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(&url).header("User-Agent", "pftui/0.3.0").send()?;

    if !resp.status().is_success() {
        bail!("Alternative.me API returned {}", resp.status());
    }

    let body: AlternativeMeResponse = resp.json()?;
    parse_crypto_fng_response(&body)
}

/// Parse an Alternative.me response body into sentiment readings.
///
/// Factored out for unit testing without network access.
fn parse_crypto_fng_response(body: &AlternativeMeResponse) -> Result<Vec<SentimentIndex>> {
    let mut out = Vec::with_capacity(body.data.len());
    for data in &body.data {
        let value: u8 = data
            .value
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid value: {}", data.value))?;
        let timestamp: i64 = data
            .timestamp
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid timestamp: {}", data.timestamp))?;
        out.push(SentimentIndex {
            index_type: "crypto".to_string(),
            value,
            classification: data.value_classification.clone(),
            timestamp,
        });
    }
    Ok(out)
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
    #[ignore = "hits Alternative.me network API; run explicitly"]
    fn test_fetch_crypto_fng() {
        let result = fetch_crypto_fng();
        assert!(result.is_ok(), "Failed to fetch crypto F&G: {:?}", result);

        let index = result.unwrap();
        assert_eq!(index.index_type, "crypto");
        assert!(index.value <= 100, "Value should be 0-100");
        assert!(
            !index.classification.is_empty(),
            "Classification should not be empty"
        );
    }

    #[test]
    fn test_parse_crypto_fng_multi_row() {
        // Synthetic multi-row Alternative.me payload (no network).
        let json = r#"{
            "data": [
                {"value": "30", "value_classification": "Fear", "timestamp": "1700000000"},
                {"value": "55", "value_classification": "Neutral", "timestamp": "1699913600"},
                {"value": "72", "value_classification": "Greed", "timestamp": "1699827200"}
            ]
        }"#;
        let body: AlternativeMeResponse = serde_json::from_str(json).unwrap();
        let readings = parse_crypto_fng_response(&body).unwrap();
        assert_eq!(readings.len(), 3);
        // newest-first
        assert_eq!(readings[0].value, 30);
        assert_eq!(readings[0].classification, "Fear");
        assert_eq!(readings[0].timestamp, 1700000000);
        assert_eq!(readings[2].value, 72);
        assert!(readings.iter().all(|r| r.index_type == "crypto"));
    }

    #[test]
    fn test_parse_crypto_fng_empty() {
        let body: AlternativeMeResponse = serde_json::from_str(r#"{"data": []}"#).unwrap();
        let readings = parse_crypto_fng_response(&body).unwrap();
        assert!(readings.is_empty());
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
