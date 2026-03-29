use anyhow::Result;
use serde_json::json;

use crate::data::predictions::{MarketCategory, PredictionMarket};
use crate::db::backend::BackendConnection;
use crate::db::prediction_contracts;
use crate::db::predictions_cache::get_cached_predictions_backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CategorySelector {
    Exact(MarketCategory),
    Finance,
    Macro,
}

/// Run the `pftui predictions` command.
///
/// Prefers the enriched prediction_market_contracts table (F55.2) when populated.
/// Falls back to the legacy predictions_cache table if contracts table is empty.
pub fn run(
    backend: &BackendConnection,
    category: Option<&str>,
    search: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    // Try enriched contracts table first (F55.2)
    // Gracefully fall back if table doesn't exist yet (pre-migration DBs)
    let cat_filter = category.and_then(resolve_category_for_contracts);
    let contracts = prediction_contracts::get_contracts_backend(
        backend,
        cat_filter.as_deref(),
        search,
        limit,
    )
    .unwrap_or_default();

    if !contracts.is_empty() {
        if json {
            print_contracts_json(&contracts)?;
        } else {
            print_contracts_table(&contracts);
        }
        return Ok(());
    }

    // Fall back to legacy predictions_cache
    let mut markets = get_cached_predictions_backend(backend, limit)?;

    if markets.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No cached prediction markets. Run `pftui refresh` first.");
        }
        return Ok(());
    }

    // Filter by category - default to finance-relevant (macro) if not specified
    let cat_str = category.unwrap_or("macro");
    let selectors = parse_category_selectors(cat_str)?;
    markets.retain(|m| market_matches_any_selector(m, &selectors));

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

/// Map user-facing category names to the db category stored in prediction_market_contracts.
/// Returns None for "macro" (no filter — show all macro-relevant categories),
/// or a specific category string for exact matches.
fn resolve_category_for_contracts(category: &str) -> Option<String> {
    match category.to_lowercase().as_str() {
        "macro" | "finance" | "all" => None, // show everything (already filtered by tag at fetch time)
        "crypto" => Some("crypto".to_string()),
        "economics" | "econ" => Some("economics".to_string()),
        "geopolitics" | "geo" | "politics" => Some("geopolitics".to_string()),
        "ai" => Some("ai".to_string()),
        other => Some(other.to_string()),
    }
}

fn parse_category_selectors(s: &str) -> Result<Vec<CategorySelector>> {
    let mut selectors = Vec::new();
    for raw in s.split('|') {
        let token = raw.trim().to_lowercase();
        if token.is_empty() {
            continue;
        }
        let selector = match token.as_str() {
            "crypto" => CategorySelector::Exact(MarketCategory::Crypto),
            "economics" | "econ" => CategorySelector::Exact(MarketCategory::Economics),
            "geopolitics" | "geo" => CategorySelector::Exact(MarketCategory::Geopolitics),
            "ai" => CategorySelector::Exact(MarketCategory::AI),
            "other" => CategorySelector::Exact(MarketCategory::Other),
            "finance" => CategorySelector::Finance,
            "macro" => CategorySelector::Macro,
            _ => anyhow::bail!(
                "Invalid category '{}'. Valid: crypto, economics, geopolitics, ai, other, finance, macro. Multiple allowed via '|'.",
                token
            ),
        };
        if !selectors.contains(&selector) {
            selectors.push(selector);
        }
    }

    if selectors.is_empty() {
        anyhow::bail!(
            "Category filter is empty. Valid: crypto, economics, geopolitics, ai, other, finance, macro."
        );
    }

    Ok(selectors)
}

fn market_matches_selector(market: &PredictionMarket, selector: CategorySelector) -> bool {
    match selector {
        CategorySelector::Exact(c) => market.category == c,
        CategorySelector::Finance => {
            matches!(
                market.category,
                MarketCategory::Economics | MarketCategory::Crypto
            )
        }
        CategorySelector::Macro => matches!(
            market.category,
            MarketCategory::Economics | MarketCategory::Geopolitics | MarketCategory::Crypto
        ),
    }
}

fn market_matches_any_selector(market: &PredictionMarket, selectors: &[CategorySelector]) -> bool {
    selectors
        .iter()
        .copied()
        .any(|selector| market_matches_selector(market, selector))
}

// ── Enriched contracts output (F55.2) ────────────────────────────────

/// Print prediction contracts as a formatted table with exchange & liquidity.
fn print_contracts_table(contracts: &[prediction_contracts::PredictionContract]) {
    if contracts.is_empty() {
        println!("No matching prediction market contracts found.");
        return;
    }

    let max_q = 55;
    let max_event = 30;

    // Print header
    println!(
        "{:<qw$}  {:<ew$}  {:>7}  {:>8}  {:>10}  {:>6}",
        "Question",
        "Event",
        "Prob%",
        "Vol 24h",
        "Liquidity",
        "Cat",
        qw = max_q,
        ew = max_event,
    );
    println!("{}", "─".repeat(max_q + max_event + 7 + 8 + 10 + 6 + 10));

    for c in contracts {
        let question = if c.question.len() > max_q {
            format!("{}...", &c.question[..max_q - 3])
        } else {
            c.question.clone()
        };

        let event = if c.event_title.len() > max_event {
            format!("{}...", &c.event_title[..max_event - 3])
        } else {
            c.event_title.clone()
        };

        let prob_pct = c.last_price * 100.0;

        println!(
            "{:<qw$}  {:<ew$}  {:>6.1}%  {:>8}  {:>10}  {:>6}",
            question,
            event,
            prob_pct,
            format_volume(c.volume_24h),
            format_volume(c.liquidity),
            &c.category[..c.category.len().min(6)],
            qw = max_q,
            ew = max_event,
        );
    }
}

/// Print prediction contracts as JSON.
fn print_contracts_json(contracts: &[prediction_contracts::PredictionContract]) -> Result<()> {
    let json_output = json!(contracts
        .iter()
        .map(|c| {
            json!({
                "contract_id": c.contract_id,
                "exchange": c.exchange,
                "event_id": c.event_id,
                "event_title": c.event_title,
                "question": c.question,
                "category": c.category,
                "probability": c.last_price,
                "probability_pct": (c.last_price * 100.0),
                "volume_24h": c.volume_24h,
                "liquidity": c.liquidity,
                "end_date": c.end_date,
                "updated_at": c.updated_at,
            })
        })
        .collect::<Vec<_>>());

    println!("{}", serde_json::to_string_pretty(&json_output)?);
    Ok(())
}

// ── Legacy predictions_cache output ──────────────────────────────────

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
    println!(
        "{}",
        "─".repeat(max_question_width + prob_width + cat_width + vol_width + 6)
    );

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
        assert_eq!(
            parse_category_selectors("crypto").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::Crypto)]
        );
        assert_eq!(
            parse_category_selectors("economics").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::Economics)]
        );
        assert_eq!(
            parse_category_selectors("econ").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::Economics)]
        );
        assert_eq!(
            parse_category_selectors("geopolitics").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::Geopolitics)]
        );
        assert_eq!(
            parse_category_selectors("geo").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::Geopolitics)]
        );
        assert_eq!(
            parse_category_selectors("ai").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::AI)]
        );
        assert_eq!(
            parse_category_selectors("other").unwrap(),
            vec![CategorySelector::Exact(MarketCategory::Other)]
        );
        assert_eq!(
            parse_category_selectors("finance").unwrap(),
            vec![CategorySelector::Finance]
        );
        assert_eq!(
            parse_category_selectors("macro").unwrap(),
            vec![CategorySelector::Macro]
        );
        assert!(parse_category_selectors("invalid").is_err());
    }

    #[test]
    fn test_parse_category_pipe_list() {
        let selectors = parse_category_selectors("geopolitics|finance|macro").unwrap();
        assert_eq!(
            selectors,
            vec![
                CategorySelector::Exact(MarketCategory::Geopolitics),
                CategorySelector::Finance,
                CategorySelector::Macro,
            ]
        );
    }

    #[test]
    fn test_market_matches_macro_selector() {
        let crypto = PredictionMarket {
            id: "m1".into(),
            question: "BTC question".into(),
            probability: 0.5,
            volume_24h: 1.0,
            category: MarketCategory::Crypto,
            updated_at: 0,
        };
        let geo = PredictionMarket {
            id: "m2".into(),
            question: "Geo question".into(),
            probability: 0.5,
            volume_24h: 1.0,
            category: MarketCategory::Geopolitics,
            updated_at: 0,
        };
        let other = PredictionMarket {
            id: "m3".into(),
            question: "Other question".into(),
            probability: 0.5,
            volume_24h: 1.0,
            category: MarketCategory::Other,
            updated_at: 0,
        };

        assert!(market_matches_selector(&crypto, CategorySelector::Macro));
        assert!(market_matches_selector(&geo, CategorySelector::Macro));
        assert!(!market_matches_selector(&other, CategorySelector::Macro));
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

    #[test]
    fn test_resolve_category_macro_returns_none() {
        assert!(resolve_category_for_contracts("macro").is_none());
        assert!(resolve_category_for_contracts("finance").is_none());
        assert!(resolve_category_for_contracts("all").is_none());
    }

    #[test]
    fn test_resolve_category_specific() {
        assert_eq!(
            resolve_category_for_contracts("crypto"),
            Some("crypto".to_string())
        );
        assert_eq!(
            resolve_category_for_contracts("economics"),
            Some("economics".to_string())
        );
        assert_eq!(
            resolve_category_for_contracts("econ"),
            Some("economics".to_string())
        );
        assert_eq!(
            resolve_category_for_contracts("geopolitics"),
            Some("geopolitics".to_string())
        );
        assert_eq!(
            resolve_category_for_contracts("geo"),
            Some("geopolitics".to_string())
        );
        assert_eq!(
            resolve_category_for_contracts("ai"),
            Some("ai".to_string())
        );
    }

    #[test]
    fn test_contracts_preferred_over_legacy() {
        // When contracts table is populated, run() should use it instead of predictions_cache
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        // Create the contracts table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS prediction_market_contracts (
                contract_id TEXT PRIMARY KEY,
                exchange TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_title TEXT NOT NULL,
                question TEXT NOT NULL,
                category TEXT NOT NULL,
                last_price REAL NOT NULL,
                volume_24h REAL NOT NULL,
                liquidity REAL NOT NULL,
                end_date TEXT,
                updated_at INTEGER NOT NULL
            )",
        )
        .unwrap();

        // Insert a contract
        conn.execute(
            "INSERT INTO prediction_market_contracts
             (contract_id, exchange, event_id, event_title, question, category,
              last_price, volume_24h, liquidity, end_date, updated_at)
             VALUES ('c1', 'polymarket', 'e1', 'Fed April', 'Will Fed cut?', 'economics',
                     0.12, 500000.0, 1000000.0, '2026-05-01', 1711670000)",
            [],
        )
        .unwrap();

        let backend = to_backend(conn);

        // Should succeed and use contracts table (json output)
        let result = run(&backend, None, None, 10, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_contracts_fallback_to_legacy() {
        // When contracts table is empty, should fall back to predictions_cache
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        // Create empty contracts table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS prediction_market_contracts (
                contract_id TEXT PRIMARY KEY,
                exchange TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_title TEXT NOT NULL,
                question TEXT NOT NULL,
                category TEXT NOT NULL,
                last_price REAL NOT NULL,
                volume_24h REAL NOT NULL,
                liquidity REAL NOT NULL,
                end_date TEXT,
                updated_at INTEGER NOT NULL
            )",
        )
        .unwrap();

        // Insert legacy data
        let markets = vec![PredictionMarket {
            id: "test1".into(),
            question: "BTC to 100k?".into(),
            probability: 0.45,
            volume_24h: 50000.0,
            category: MarketCategory::Crypto,
            updated_at: 1000000,
        }];
        upsert_predictions(&conn, &markets).unwrap();

        let backend = to_backend(conn);

        // Should succeed using legacy table
        let result = run(&backend, None, None, 10, false);
        assert!(result.is_ok());
    }
}
