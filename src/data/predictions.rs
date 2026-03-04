use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Prediction market from Polymarket Gamma API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionMarket {
    pub id: String,
    pub question: String,
    pub probability: f64, // 0.0 to 1.0
    pub volume_24h: f64,
    pub category: MarketCategory,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarketCategory {
    #[serde(rename = "crypto")]
    Crypto,
    #[serde(rename = "economics")]
    Economics,
    #[serde(rename = "geopolitics")]
    Geopolitics,
    #[serde(rename = "ai")]
    AI,
    #[serde(rename = "other")]
    Other,
}

impl std::fmt::Display for MarketCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketCategory::Crypto => write!(f, "Crypto"),
            MarketCategory::Economics => write!(f, "Econ"),
            MarketCategory::Geopolitics => write!(f, "Geo"),
            MarketCategory::AI => write!(f, "AI"),
            MarketCategory::Other => write!(f, "Other"),
        }
    }
}

/// Response from Polymarket Gamma API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Used by fetch_polymarket_predictions (F17.3+)
struct GammaResponse {
    #[serde(default)]
    data: Vec<GammaMarket>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Used by fetch_polymarket_predictions (F17.3+)
struct GammaMarket {
    #[serde(rename = "condition_id")]
    condition_id: String,
    question: String,
    #[serde(rename = "outcome_prices")]
    outcome_prices: Vec<String>,
    volume_24h: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Fetch top prediction markets from Polymarket Gamma API.
/// Free, no auth required.
#[allow(dead_code)] // Infrastructure for F17.3+ (predictions CLI, refresh integration)
pub async fn fetch_polymarket_predictions() -> Result<Vec<PredictionMarket>> {
    let url = "https://gamma-api.polymarket.com/markets?limit=50&active=true";
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .context("Polymarket Gamma API request failed")?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Polymarket API returned status {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }

    let gamma_resp: GammaResponse = resp
        .json()
        .await
        .context("Failed to parse Polymarket response")?;

    let now = chrono::Utc::now().timestamp();

    let markets: Vec<PredictionMarket> = gamma_resp
        .data
        .into_iter()
        .filter_map(|m| {
            // Parse probability from first outcome price
            let prob = m.outcome_prices.first()?
                .parse::<f64>()
                .ok()?;

            // Parse volume
            let volume = m.volume_24h
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);

            // Infer category from tags/question
            let category = infer_category(&m.question, &m.tags);

            Some(PredictionMarket {
                id: m.condition_id,
                question: m.question,
                probability: prob,
                volume_24h: volume,
                category,
                updated_at: now,
            })
        })
        .collect();

    Ok(markets)
}

/// Infer market category from question text and tags.
#[allow(dead_code)] // Used by fetch_polymarket_predictions (F17.3+)
fn infer_category(question: &str, tags: &[String]) -> MarketCategory {
    let q_lower = question.to_lowercase();
    let tags_str = tags.join(" ").to_lowercase();
    let combined = format!("{} {}", q_lower, tags_str);

    if combined.contains("bitcoin")
        || combined.contains("btc")
        || combined.contains("ethereum")
        || combined.contains("eth")
        || combined.contains("crypto")
        || combined.contains("solana")
    {
        MarketCategory::Crypto
    } else if combined.contains("recession")
        || combined.contains("fed")
        || combined.contains("rate cut")
        || combined.contains("inflation")
        || combined.contains("gdp")
        || combined.contains("unemployment")
        || combined.contains("economy")
    {
        MarketCategory::Economics
    } else if combined.contains("war")
        || combined.contains("iran")
        || combined.contains("russia")
        || combined.contains("china")
        || combined.contains("election")
        || combined.contains("trump")
        || combined.contains("biden")
        || combined.contains("ukraine")
    {
        MarketCategory::Geopolitics
    } else if combined.contains(" ai ")
        || combined.contains("artificial intelligence")
        || combined.contains("chatgpt")
        || combined.contains("openai")
        || combined.starts_with("ai ")
        || combined.ends_with(" ai")
    {
        MarketCategory::AI
    } else {
        MarketCategory::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_category_crypto() {
        assert_eq!(
            infer_category("Will Bitcoin reach $100k by 2026?", &[]),
            MarketCategory::Crypto
        );
    }

    #[test]
    fn test_infer_category_economics() {
        assert_eq!(
            infer_category("Will US enter recession in 2026?", &[]),
            MarketCategory::Economics
        );
    }

    #[test]
    fn test_infer_category_geopolitics() {
        assert_eq!(
            infer_category("Will Russia and Ukraine reach ceasefire?", &[]),
            MarketCategory::Geopolitics
        );
    }

    #[test]
    fn test_infer_category_default() {
        assert_eq!(
            infer_category("Will it rain tomorrow?", &[]),
            MarketCategory::Other
        );
    }
}
