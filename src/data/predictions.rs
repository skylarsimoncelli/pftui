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
    #[serde(rename = "volume24hr", default)]
    volume_24hr: f64,
    #[serde(default)]
    active: bool,
    #[serde(default)]
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

    let gamma_resp: GammaResponse = serde_json::from_str(&text).context(format!(
        "Failed to parse Polymarket response. First 500 chars: {}",
        &text.chars().take(500).collect::<String>()
    ))?;

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
        .take(50) // Limit to top 50 by volume
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

// ── Tag-based event fetching (F55.2) ────────────────────────────────

use crate::db::prediction_contracts::PredictionContract;

/// Macro-relevant Polymarket tag slugs.
/// Each maps to a category label stored in prediction_market_contracts.
const MACRO_TAG_SLUGS: &[(&str, &str)] = &[
    ("fed", "economics"),
    ("economics", "economics"),
    ("interest-rates", "economics"),
    ("recession", "economics"),
    ("inflation", "economics"),
    ("geopolitics", "geopolitics"),
    ("politics", "geopolitics"),
    ("iran", "geopolitics"),
    ("war", "geopolitics"),
    ("bitcoin", "crypto"),
    ("crypto", "crypto"),
    ("ai", "ai"),
    ("ipo", "finance"),
    ("stocks", "finance"),
];

/// Gamma events API response structure.
#[derive(Debug, Deserialize)]
struct GammaEvent {
    id: String,
    title: String,
    #[serde(default)]
    markets: Vec<GammaEventMarket>,
}

/// A market within a Gamma event.
#[derive(Debug, Deserialize)]
struct GammaEventMarket {
    #[serde(rename = "conditionId")]
    condition_id: String,
    question: String,
    #[serde(rename = "outcomePrices", default)]
    outcome_prices: String, // JSON string: "[\"0.42\", \"0.58\"]" — absent for resolved markets
    #[serde(rename = "volume24hr", default)]
    volume_24hr: f64,
    #[serde(rename = "liquidityNum", default)]
    liquidity_num: f64,
    #[serde(rename = "endDate", default)]
    end_date: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
}

/// Fetch macro-relevant prediction market contracts from Polymarket Gamma events API.
///
/// Queries multiple tag slugs (fed, economics, geopolitics, politics, bitcoin, crypto, ai),
/// deduplicates by condition_id, and returns structured `PredictionContract`s sorted by volume.
/// Free, no auth required.
pub async fn fetch_polymarket_contracts() -> Result<Vec<PredictionContract>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let now = chrono::Utc::now().timestamp();
    let mut seen = std::collections::HashSet::new();
    let mut contracts = Vec::new();

    for &(tag_slug, category) in MACRO_TAG_SLUGS {
        let url = format!(
            "https://gamma-api.polymarket.com/events?limit=25&active=true&closed=false&tag_slug={}&order=volume24hr&ascending=false",
            tag_slug
        );

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Warning: Polymarket tag '{}' fetch failed: {}", tag_slug, e);
                continue;
            }
        };

        if !resp.status().is_success() {
            continue;
        }

        let text = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let events: Vec<GammaEvent> = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for event in events {
            for market in event.markets {
                // Skip closed/inactive
                if market.closed || !market.active {
                    continue;
                }

                // Skip entertainment
                if is_entertainment_market(&market.question)
                    || is_entertainment_market(&event.title)
                {
                    continue;
                }

                // Deduplicate across tags
                if !seen.insert(market.condition_id.clone()) {
                    continue;
                }

                // Parse probability from outcome_prices
                let prob = parse_yes_probability(&market.outcome_prices).unwrap_or(0.0);

                contracts.push(PredictionContract {
                    contract_id: market.condition_id,
                    exchange: "polymarket".to_string(),
                    event_id: event.id.clone(),
                    event_title: event.title.clone(),
                    question: market.question,
                    category: category.to_string(),
                    last_price: prob,
                    volume_24h: market.volume_24hr,
                    liquidity: market.liquidity_num,
                    end_date: market.end_date,
                    updated_at: now,
                });
            }
        }
    }

    // Sort by volume descending
    contracts.sort_by(|a, b| b.volume_24h.partial_cmp(&a.volume_24h).unwrap_or(std::cmp::Ordering::Equal));

    Ok(contracts)
}

/// Parse the "Yes" probability from Polymarket's outcome_prices JSON string.
/// Format: "[\"0.42\", \"0.58\"]" where index 0 is "Yes" price.
fn parse_yes_probability(outcome_prices: &str) -> Option<f64> {
    let prices: Vec<String> = serde_json::from_str(outcome_prices).ok()?;
    prices.first()?.parse::<f64>().ok()
}

/// Check if a market question is entertainment/sports (should be filtered out).
fn is_entertainment_market(question: &str) -> bool {
    let q_lower = question.to_lowercase();

    // Use word-boundary-aware matching for short acronyms to avoid false positives
    // (e.g. "nfl" matching "conflict", "mlb" matching "Xmlb...")
    let has_word = |word: &str| -> bool {
        q_lower.split(|c: char| !c.is_alphanumeric()).any(|w| w == word)
    };

    // Explicit entertainment/sports signals
    q_lower.contains("gta vi")
        || q_lower.contains("grand theft auto")
        || q_lower.contains("rihanna")
        || q_lower.contains("album")
        || q_lower.contains("playboi carti")
        || q_lower.contains("jesus christ return")
        || has_word("nfl")
        || has_word("nba")
        || has_word("nhl")
        || has_word("mlb")
        || has_word("fifa")
        || q_lower.contains("world cup")
        || q_lower.contains("march madness")
        || has_word("ncaa")
        || has_word("uefa")
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

    #[test]
    fn test_parse_yes_probability_valid() {
        assert!((parse_yes_probability(r#"["0.42", "0.58"]"#).unwrap() - 0.42).abs() < 0.001);
    }

    #[test]
    fn test_parse_yes_probability_zero() {
        assert!((parse_yes_probability(r#"["0.00", "1.00"]"#).unwrap() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_yes_probability_one() {
        assert!((parse_yes_probability(r#"["1.00", "0.00"]"#).unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_yes_probability_invalid() {
        assert!(parse_yes_probability("not json").is_none());
    }

    #[test]
    fn test_parse_yes_probability_empty_array() {
        assert!(parse_yes_probability("[]").is_none());
    }

    #[test]
    fn test_macro_tag_slugs_non_empty() {
        assert!(!MACRO_TAG_SLUGS.is_empty());
        for &(slug, cat) in MACRO_TAG_SLUGS {
            assert!(!slug.is_empty());
            assert!(!cat.is_empty());
        }
    }
}
