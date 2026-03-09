use anyhow::Result;
use serde_json::json;

use crate::data::predictions::{MarketCategory, PredictionMarket};
use crate::db::backend::BackendConnection;
use crate::db::predictions_cache::get_cached_predictions_backend;

/// Run the `pftui predictions` command.
pub fn run(
    backend: &BackendConnection,
    category: Option<&str>,
    search: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    // Fetch all cached predictions up to limit
    let mut markets = get_cached_predictions_backend(backend, limit)?;

    if markets.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No cached prediction markets. Run `pftui refresh` first.");
        }
        return Ok(());
    }

    // Filter by category if specified
    if let Some(cat_str) = category {
        let cat_filter = parse_category(cat_str)?;
        markets.retain(|m| m.category == cat_filter);
    }

    // Filter by search query if specified
    if let Some(query) = search {
        let query_lower = query.to_lowercase();
        markets.retain(|m| m.question.to_lowercase().contains(&query_lower));
    }

    // Trim to final limit after filtering
    markets.truncate(limit);

    if json {
        print_json(&markets)?;
    } else {
        print_table(&markets);
    }

    Ok(())
}

/// Parse category string into enum.
fn parse_category(s: &str) -> Result<MarketCategory> {
    match s.to_lowercase().as_str() {
        "crypto" => Ok(MarketCategory::Crypto),
        "economics" | "econ" => Ok(MarketCategory::Economics),
        "geopolitics" | "geo" => Ok(MarketCategory::Geopolitics),
        "ai" => Ok(MarketCategory::AI),
        "other" => Ok(MarketCategory::Other),
        _ => anyhow::bail!(
            "Invalid category '{}'. Valid: crypto, economics, geopolitics, ai, other",
            s
        ),
    }
}

/// Print prediction markets as a formatted table.
fn print_table(markets: &[PredictionMarket]) {
    if markets.is_empty() {
        println!("No matching prediction markets found.");
        return;
    }

    // Calculate column widths
    let max_question_width = 70;
    let prob_width = 8;
    let cat_width = 6;
    let vol_width = 12;

    // Print header
    println!(
        "{:<width$}  {:>prob$}  {:>cat$}  {:>vol$}",
        "Question",
        "Prob%",
        "Cat",
        "Vol 24h",
        width = max_question_width,
        prob = prob_width,
        cat = cat_width,
        vol = vol_width,
    );
    println!("{}", "─".repeat(max_question_width + prob_width + cat_width + vol_width + 6));

    // Print rows
    for market in markets {
        let question = if market.question.len() > max_question_width {
            format!("{}...", &market.question[..max_question_width - 3])
        } else {
            market.question.clone()
        };

        let prob_pct = market.probability * 100.0;
        let vol_formatted = format_volume(market.volume_24h);

        println!(
            "{:<width$}  {:>prob$.1}%  {:>cat$}  {:>vol$}",
            question,
            prob_pct,
            market.category,
            vol_formatted,
            width = max_question_width,
            prob = prob_width - 1, // -1 for the % sign
            cat = cat_width,
            vol = vol_width,
        );
    }
}

/// Format volume with K/M suffix.
fn format_volume(volume: f64) -> String {
    if volume >= 1_000_000.0 {
        format!("{:.1}M", volume / 1_000_000.0)
    } else if volume >= 1_000.0 {
        format!("{:.1}K", volume / 1_000.0)
    } else {
        format!("{:.0}", volume)
    }
}

/// Print prediction markets as JSON.
fn print_json(markets: &[PredictionMarket]) -> Result<()> {
    let json_output = json!(markets
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "question": m.question,
                "probability": m.probability,
                "probability_pct": (m.probability * 100.0),
                "volume_24h": m.volume_24h,
                "category": format!("{:?}", m.category).to_lowercase(),
                "updated_at": m.updated_at,
            })
        })
        .collect::<Vec<_>>());

    println!("{}", serde_json::to_string_pretty(&json_output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::predictions_cache::{ensure_table, upsert_predictions};
    use rusqlite::Connection;
    fn to_backend(conn: Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_predictions_empty_cache() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let backend = to_backend(conn);

        let result = run(&backend, None, None, 10, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_predictions_with_data() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        let markets = vec![
            PredictionMarket {
                id: "test1".into(),
                question: "Will BTC reach $100k by end of 2026?".into(),
                probability: 0.45,
                volume_24h: 50000.0,
                category: MarketCategory::Crypto,
                updated_at: 1000000,
            },
            PredictionMarket {
                id: "test2".into(),
                question: "US recession in 2026?".into(),
                probability: 0.22,
                volume_24h: 30000.0,
                category: MarketCategory::Economics,
                updated_at: 1000000,
            },
        ];

        upsert_predictions(&conn, &markets).unwrap();
        let backend = to_backend(conn);

        let result = run(&backend, None, None, 10, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_predictions_category_filter() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        let markets = vec![
            PredictionMarket {
                id: "test1".into(),
                question: "BTC to $100k?".into(),
                probability: 0.45,
                volume_24h: 50000.0,
                category: MarketCategory::Crypto,
                updated_at: 1000000,
            },
            PredictionMarket {
                id: "test2".into(),
                question: "US recession?".into(),
                probability: 0.22,
                volume_24h: 30000.0,
                category: MarketCategory::Economics,
                updated_at: 1000000,
            },
        ];

        upsert_predictions(&conn, &markets).unwrap();
        let backend = to_backend(conn);

        let result = run(&backend, Some("crypto"), None, 10, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_predictions_search() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        let markets = vec![PredictionMarket {
            id: "test1".into(),
            question: "Will there be a recession in 2026?".into(),
            probability: 0.22,
            volume_24h: 30000.0,
            category: MarketCategory::Economics,
            updated_at: 1000000,
        }];

        upsert_predictions(&conn, &markets).unwrap();
        let backend = to_backend(conn);

        let result = run(&backend, None, Some("recession"), 10, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_category() {
        assert_eq!(parse_category("crypto").unwrap(), MarketCategory::Crypto);
        assert_eq!(
            parse_category("economics").unwrap(),
            MarketCategory::Economics
        );
        assert_eq!(parse_category("econ").unwrap(), MarketCategory::Economics);
        assert_eq!(
            parse_category("geopolitics").unwrap(),
            MarketCategory::Geopolitics
        );
        assert_eq!(parse_category("geo").unwrap(), MarketCategory::Geopolitics);
        assert_eq!(parse_category("ai").unwrap(), MarketCategory::AI);
        assert_eq!(parse_category("other").unwrap(), MarketCategory::Other);
        assert!(parse_category("invalid").is_err());
    }

    #[test]
    fn test_format_volume() {
        assert_eq!(format_volume(500.0), "500");
        assert_eq!(format_volume(1500.0), "1.5K");
        assert_eq!(format_volume(50000.0), "50.0K");
        assert_eq!(format_volume(1500000.0), "1.5M");
    }

    #[test]
    fn test_predictions_json_output() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        let markets = vec![PredictionMarket {
            id: "test1".into(),
            question: "Test market".into(),
            probability: 0.5,
            volume_24h: 10000.0,
            category: MarketCategory::Crypto,
            updated_at: 1000000,
        }];

        upsert_predictions(&conn, &markets).unwrap();
        let backend = to_backend(conn);

        let result = run(&backend, None, None, 10, true);
        assert!(result.is_ok());
    }
}
