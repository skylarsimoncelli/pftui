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

/// Polymarket Gamma API returns a flat array, not a wrapped object.
type GammaResponse = Vec<GammaMarket>;

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Used by fetch_polymarket_predictions (F17.3+)
struct GammaMarket {
    #[serde(rename = "conditionId")]
    condition_id: String,
    question: String,
    #[serde(rename = "outcomePrices")]
    outcome_prices: String, // JSON string: "[\"0.42\", \"0.58\"]"
    #[serde(rename = "volume24hr")]
    volume_24hr: f64,
    active: bool,
    closed: bool,
}

/// Fetch top prediction markets from Polymarket Gamma API.
/// Free, no auth required.
#[allow(dead_code)] // Infrastructure for F17.3+ (predictions CLI, refresh integration)
pub async fn fetch_polymarket_predictions() -> Result<Vec<PredictionMarket>> {
    // Fetch open markets (active=true, closed=false), sorted by volume
    let url = "https://gamma-api.polymarket.com/markets?limit=100&active=true&closed=false";
    
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

    let text = resp.text().await?;
    
    let gamma_resp: GammaResponse = serde_json::from_str(&text)
        .context(format!("Failed to parse Polymarket response. First 500 chars: {}", 
            &text.chars().take(500).collect::<String>()))?;

    let now = chrono::Utc::now().timestamp();

    let markets: Vec<PredictionMarket> = gamma_resp
        .into_iter()
        // Filter out closed/resolved markets (redundant with URL param but defensive)
        .filter(|m| m.active && !m.closed)
        // Filter out entertainment/sports markets
        .filter(|m| !is_entertainment_market(&m.question))
        .filter_map(|m| {
            // Parse outcome_prices JSON string: "[\"0.42\", \"0.58\"]"
            let prices: Vec<String> = serde_json::from_str(&m.outcome_prices).ok()?;
            let prob = prices.first()?.parse::<f64>().ok()?;

            // Infer category from question text
            let category = infer_category_from_question(&m.question);

            Some(PredictionMarket {
                id: m.condition_id,
                question: m.question,
                probability: prob,
                volume_24h: m.volume_24hr,
                category,
                updated_at: now,
            })
        })
        .take(50)  // Limit to top 50 by volume
        .collect();

    Ok(markets)
}

/// Save daily probability snapshots for tracked prediction markets.
/// Call this after fetching and caching predictions to build historical data.
#[allow(dead_code)] // Infrastructure for F17.4+ (prediction sparklines)
pub fn save_daily_snapshots(
    conn: &rusqlite::Connection,
    markets: &[PredictionMarket],
) -> Result<()> {
    use crate::db::predictions_history;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let records: Vec<_> = markets
        .iter()
        .map(|m| (m.id.clone(), today.clone(), m.probability))
        .collect();

    predictions_history::batch_insert_history(conn, &records)?;
    Ok(())
}

/// Infer market category from question text.
fn infer_category_from_question(question: &str) -> MarketCategory {
    let q_lower = question.to_lowercase();

    if q_lower.contains("bitcoin")
        || q_lower.contains("btc")
        || q_lower.contains("ethereum")
        || q_lower.contains("eth")
        || q_lower.contains("crypto")
        || q_lower.contains("solana")
    {
        MarketCategory::Crypto
    } else if q_lower.contains("recession")
        || q_lower.contains("fed")
        || q_lower.contains("fomc")
        || q_lower.contains("federal reserve")
        || q_lower.contains("rate cut")
        || q_lower.contains("rate hike")
        || q_lower.contains("inflation")
        || q_lower.contains("gdp")
        || q_lower.contains("unemployment")
        || q_lower.contains("economy")
    {
        MarketCategory::Economics
    } else if q_lower.contains("war")
        || q_lower.contains("iran")
        || q_lower.contains("russia")
        || q_lower.contains("china")
        || q_lower.contains("election")
        || q_lower.contains("trump")
        || q_lower.contains("biden")
        || q_lower.contains("ukraine")
        || q_lower.contains("ceasefire")
        || q_lower.contains("gaza")
        || q_lower.contains("israel")
        || q_lower.contains("middle east")
        || q_lower.contains("invasion")
        || q_lower.contains("taiwan")
    {
        MarketCategory::Geopolitics
    } else if q_lower.contains(" ai ")
        || q_lower.contains("artificial intelligence")
        || q_lower.contains("chatgpt")
        || q_lower.contains("openai")
        || q_lower.starts_with("ai ")
        || q_lower.ends_with(" ai")
    {
        MarketCategory::AI
    } else {
        MarketCategory::Other
    }
}

/// Check if a market question is entertainment/sports (should be filtered out).
fn is_entertainment_market(question: &str) -> bool {
    let q_lower = question.to_lowercase();
    
    // Explicit entertainment/sports signals
    q_lower.contains("gta vi")
        || q_lower.contains("grand theft auto")
        || q_lower.contains("rihanna")
        || q_lower.contains("album")
        || q_lower.contains("playboi carti")
        || q_lower.contains("jesus christ return")
        || q_lower.contains("nfl")
        || q_lower.contains("nba")
        || q_lower.contains("nhl")
        || q_lower.contains("mlb")
        || q_lower.contains("fifa")
        || q_lower.contains("world cup")
        || q_lower.contains("march madness")
        || q_lower.contains("ncaa")
        || q_lower.contains("uefa")
        || q_lower.contains("soccer")
        || q_lower.contains("super bowl")
        || q_lower.contains("champions league")
        || q_lower.contains("olympics")
        || q_lower.contains("movie")
        || q_lower.contains("film")
        || q_lower.contains("actor")
        || q_lower.contains("actress")
        || q_lower.contains("convicted")
        || q_lower.contains("weinstein")
        || q_lower.contains("celebrity")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_category_crypto() {
        assert_eq!(
            infer_category_from_question("Will Bitcoin reach $100k by 2026?"),
            MarketCategory::Crypto
        );
    }

    #[test]
    fn test_infer_category_economics() {
        assert_eq!(
            infer_category_from_question("Will US enter recession in 2026?"),
            MarketCategory::Economics
        );
    }

    #[test]
    fn test_infer_category_geopolitics() {
        assert_eq!(
            infer_category_from_question("Will Russia and Ukraine reach ceasefire?"),
            MarketCategory::Geopolitics
        );
    }

    #[test]
    fn test_infer_category_default() {
        assert_eq!(
            infer_category_from_question("Will it rain tomorrow?"),
            MarketCategory::Other
        );
    }
}
