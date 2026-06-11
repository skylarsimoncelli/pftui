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
///
/// `geo` enables the curated geopolitics relevance filter: keyword-matched
/// contracts only, with stale contracts excluded (resolving > 12 months out,
/// already past resolution, or zero 24h volume). Geo mode spans all
/// categories — Polymarket's own category labels under-tag geopolitics.
pub fn run(
    backend: &BackendConnection,
    category: Option<&str>,
    search: Option<&str>,
    geo: bool,
    limit: usize,
    json: bool,
) -> Result<()> {
    // Try enriched contracts table first (F55.2)
    // Gracefully fall back if table doesn't exist yet (pre-migration DBs)
    let cat_filter = if geo {
        None // geo relevance is keyword-driven, not category-driven
    } else {
        category.and_then(resolve_category_for_contracts)
    };
    // Geo mode filters in-process, so over-fetch then truncate post-filter.
    let fetch_limit = if geo { limit.max(500) } else { limit };
    let mut contracts = prediction_contracts::get_contracts_backend(
        backend,
        cat_filter.as_deref(),
        search,
        fetch_limit,
    )
    .unwrap_or_default();

    if !contracts.is_empty() {
        if geo {
            let today = chrono::Local::now().date_naive();
            contracts.retain(|c| geo_keep_contract(c, today));
            contracts.truncate(limit);
        }
        if contracts.is_empty() {
            if json {
                println!("[]");
            } else {
                println!("No geopolitics-relevant contracts after relevance + staleness filtering.");
            }
            return Ok(());
        }
        if json {
            print_contracts_json(&contracts)?;
        } else {
            print_contracts_table(&contracts);
        }
        return Ok(());
    }

    // Fall back to legacy predictions_cache
    let mut markets = get_cached_predictions_backend(backend, fetch_limit)?;

    if markets.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No cached prediction markets. Run `pftui refresh` first.");
        }
        return Ok(());
    }

    if geo {
        // Legacy rows carry no end_date — keyword + volume filters only.
        markets.retain(|m| is_geo_relevant(&m.question) && m.volume_24h > 0.0);
    } else {
        // Filter by category - default to finance-relevant (macro) if not specified
        let cat_str = category.unwrap_or("macro");
        let selectors = parse_category_selectors(cat_str)?;
        markets.retain(|m| market_matches_any_selector(m, &selectors));
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

// ── Geopolitics relevance filter (`--geo`) ───────────────────────────

/// Curated geopolitics keyword list. Single-word terms match on word
/// boundaries (so "war" never matches "software"); multi-word terms match
/// as case-insensitive substrings.
const GEO_KEYWORDS: &[&str] = &[
    // conflict vocabulary
    "war",
    "ceasefire",
    "truce",
    "invasion",
    "invade",
    "strike",
    "airstrike",
    "missile",
    "drone",
    "blockade",
    "escalation",
    "mobilization",
    "ballistic",
    "hostage",
    "annex",
    "coup",
    // instruments of state pressure
    "sanctions",
    "embargo",
    "tariff",
    "nuclear",
    "enrichment",
    "treaty",
    "nato",
    "opec",
    "peace deal",
    "regime change",
    // named regions / actors
    "taiwan",
    "china",
    "iran",
    "israel",
    "gaza",
    "hezbollah",
    "houthi",
    "russia",
    "ukraine",
    "kremlin",
    "putin",
    "zelensky",
    "north korea",
    "venezuela",
    "syria",
    "lebanon",
    "red sea",
    "south china sea",
    "hormuz",
    "strait",
];

/// Maximum months ahead a contract may resolve and still count as actionable.
const GEO_MAX_MONTHS_OUT: u32 = 12;

/// True when `text` matches the curated geopolitics keyword list.
fn is_geo_relevant(text: &str) -> bool {
    let lower = text.to_lowercase();
    let words: std::collections::HashSet<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    GEO_KEYWORDS.iter().any(|kw| {
        if kw.contains(' ') {
            lower.contains(kw)
        } else {
            words.contains(kw)
        }
    })
}

/// Parse the leading YYYY-MM-DD of an ISO8601 end_date string.
fn parse_end_date(end_date: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(end_date.get(..10)?, "%Y-%m-%d").ok()
}

/// Best-effort deadline extraction from question text ("by April 30",
/// "through May 22, 2026", "before December 31?"). Cached contracts often
/// carry an empty `end_date`; without this fallback, long-resolved markets
/// ("ceasefire by April 30?") would pass the staleness filter forever. A
/// year-less date assumes the current year — Polymarket questions name the
/// year whenever it is not the current one.
fn question_deadline(text: &str, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    use chrono::Datelike;
    let lower = text.to_lowercase();
    let re = regex::Regex::new(
        r"\b(?:by|through|on|before|until)\s+(january|february|march|april|may|june|july|august|september|october|november|december)\s+(\d{1,2})(?:,?\s+(\d{4}))?",
    )
    .ok()?;
    let caps = re.captures(&lower)?;
    let month = match &caps[1] {
        "january" => 1,
        "february" => 2,
        "march" => 3,
        "april" => 4,
        "may" => 5,
        "june" => 6,
        "july" => 7,
        "august" => 8,
        "september" => 9,
        "october" => 10,
        "november" => 11,
        _ => 12,
    };
    let day: u32 = caps[2].parse().ok()?;
    let year: i32 = caps
        .get(3)
        .and_then(|y| y.as_str().parse().ok())
        .unwrap_or_else(|| today.year());
    chrono::NaiveDate::from_ymd_opt(year, month, day)
}

/// Geo retention rule for enriched contracts: keyword relevance on
/// question + event title, plus staleness exclusion — already past
/// resolution, resolving more than 12 months out, or zero 24h volume.
/// Staleness reads `end_date` when present, falling back to a deadline
/// parsed from the question text; contracts with neither are kept
/// (staleness cannot be judged).
fn geo_keep_contract(
    c: &prediction_contracts::PredictionContract,
    today: chrono::NaiveDate,
) -> bool {
    if !is_geo_relevant(&c.question) && !is_geo_relevant(&c.event_title) {
        return false;
    }
    if c.volume_24h <= 0.0 {
        return false;
    }
    let resolution = c
        .end_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .and_then(parse_end_date)
        .or_else(|| question_deadline(&c.question, today));
    if let Some(end) = resolution {
        if end < today {
            return false; // already resolved / past resolution date
        }
        let horizon = today
            .checked_add_months(chrono::Months::new(GEO_MAX_MONTHS_OUT))
            .unwrap_or(chrono::NaiveDate::MAX);
        if end > horizon {
            return false; // too far out to be actionable
        }
    }
    true
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

        let result = run(&backend, None, None, false, 10, false);
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

        let result = run(&backend, None, None, false, 10, false);
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

        let result = run(&backend, Some("crypto"), None, false, 10, false);
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

        let result = run(&backend, None, Some("recession"), false, 10, false);
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

        let result = run(&backend, None, None, false, 10, true);
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
        let result = run(&backend, None, None, false, 10, true);
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
        let result = run(&backend, None, None, false, 10, false);
        assert!(result.is_ok());
    }

    // ── Geo filter tests ─────────────────────────────────────────────

    fn geo_contract(
        id: &str,
        question: &str,
        event_title: &str,
        volume_24h: f64,
        end_date: Option<&str>,
    ) -> prediction_contracts::PredictionContract {
        prediction_contracts::PredictionContract {
            contract_id: id.to_string(),
            exchange: "polymarket".to_string(),
            event_id: "e1".to_string(),
            event_title: event_title.to_string(),
            question: question.to_string(),
            category: "politics".to_string(),
            last_price: 0.5,
            volume_24h,
            liquidity: 100_000.0,
            end_date: end_date.map(|s| s.to_string()),
            updated_at: 1_000_000,
        }
    }

    #[test]
    fn geo_keywords_match_on_word_boundaries() {
        assert!(is_geo_relevant("Will the Russia-Ukraine war end in 2026?"));
        assert!(is_geo_relevant("Iran nuclear enrichment above 60%?"));
        assert!(is_geo_relevant("US strike on Houthi positions?"));
        assert!(is_geo_relevant("New tariff on China goods?"));
        // multi-word terms
        assert!(is_geo_relevant("Incident in the South China Sea this year?"));
        assert!(is_geo_relevant("North Korea missile test?"));
        // word-boundary discipline: no substring false positives
        assert!(!is_geo_relevant("Best software stock of 2026?"));
        assert!(!is_geo_relevant("Will the Warriors win the title?"));
        assert!(!is_geo_relevant("Fed rate cut by September?"));
        assert!(!is_geo_relevant("Dronefield Inc IPO above $10?"));
    }

    #[test]
    fn geo_filter_excludes_far_dated_resolved_and_zero_volume() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 11).unwrap();
        // Relevant, active, in-window → kept.
        assert!(geo_keep_contract(
            &geo_contract("c1", "Iran ceasefire by July?", "Middle East", 5000.0, Some("2026-08-01T00:00:00Z")),
            today
        ));
        // Relevance can come from the event title.
        assert!(geo_keep_contract(
            &geo_contract("c2", "Resolution before September?", "Russia-Ukraine ceasefire", 5000.0, Some("2026-09-01")),
            today
        ));
        // Not geo-relevant → dropped.
        assert!(!geo_keep_contract(
            &geo_contract("c3", "Fed cuts rates in July?", "FOMC July", 5000.0, Some("2026-08-01")),
            today
        ));
        // Already past resolution date → dropped.
        assert!(!geo_keep_contract(
            &geo_contract("c4", "Iran ceasefire by May?", "Middle East", 5000.0, Some("2026-05-01")),
            today
        ));
        // Resolving more than 12 months out → dropped.
        assert!(!geo_keep_contract(
            &geo_contract("c5", "Taiwan invasion by 2030?", "Taiwan", 5000.0, Some("2029-12-31")),
            today
        ));
        // Exactly at the 12-month horizon → kept.
        assert!(geo_keep_contract(
            &geo_contract("c6", "Taiwan blockade within a year?", "Taiwan", 5000.0, Some("2027-06-11")),
            today
        ));
        // Zero 24h volume → dropped (stale market).
        assert!(!geo_keep_contract(
            &geo_contract("c7", "Iran sanctions lifted?", "Iran", 0.0, Some("2026-08-01")),
            today
        ));
        // Missing end_date and no parsable question deadline → kept.
        assert!(geo_keep_contract(
            &geo_contract("c8", "NATO Article 5 invoked?", "NATO", 5000.0, None),
            today
        ));
        // Missing end_date but the question names a past deadline → dropped
        // (the live cache carries resolved markets with empty end_date).
        assert!(!geo_keep_contract(
            &geo_contract("c9", "US x Iran ceasefire by April 30?", "Iran", 5000.0, None),
            today
        ));
        assert!(!geo_keep_contract(
            &geo_contract(
                "c10",
                "Israel x Hezbollah Ceasefire extended by April 26, 2026?",
                "Middle East",
                5000.0,
                Some(""), // empty string end_date — treated as missing
            ),
            today
        ));
        // Question deadline in the future → kept.
        assert!(geo_keep_contract(
            &geo_contract("c11", "Russia x Ukraine ceasefire by June 30, 2026?", "Ukraine", 5000.0, None),
            today
        ));
    }

    #[test]
    fn question_deadline_parsing() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 11).unwrap();
        let d = |s: &str| question_deadline(s, today);
        assert_eq!(
            d("US x Iran ceasefire by April 30?"),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 30)
        );
        assert_eq!(
            d("Ceasefire extended by April 26, 2026?"),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 26)
        );
        assert_eq!(
            d("Will the Iran ceasefire continue through May 22?"),
            chrono::NaiveDate::from_ymd_opt(2026, 5, 22)
        );
        assert_eq!(
            d("US x Iran permanent peace deal by December 31, 2027?"),
            chrono::NaiveDate::from_ymd_opt(2027, 12, 31)
        );
        assert_eq!(d("NATO Article 5 invoked?"), None);
    }

    #[test]
    fn geo_run_path_filters_contracts_table() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
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
        let far = chrono::Local::now().date_naive() + chrono::Duration::days(60);
        let far = far.format("%Y-%m-%d").to_string();
        for (id, q) in [
            ("g1", "Russia-Ukraine ceasefire by year end?"),
            ("g2", "Fed cuts rates in September?"),
        ] {
            conn.execute(
                "INSERT INTO prediction_market_contracts
                 (contract_id, exchange, event_id, event_title, question, category,
                  last_price, volume_24h, liquidity, end_date, updated_at)
                 VALUES (?1, 'polymarket', 'e1', 'Event', ?2, 'politics',
                         0.4, 9000.0, 50000.0, ?3, 1000000)",
                rusqlite::params![id, q, far],
            )
            .unwrap();
        }
        let backend = to_backend(conn);
        // Smoke: geo mode runs clean over a mixed contracts table.
        assert!(run(&backend, None, None, true, 10, true).is_ok());
    }

    #[test]
    fn geo_run_path_filters_legacy_cache() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let markets = vec![
            PredictionMarket {
                id: "g1".into(),
                question: "Iran ceasefire by August?".into(),
                probability: 0.3,
                volume_24h: 9000.0,
                category: MarketCategory::Geopolitics,
                updated_at: 1_000_000,
            },
            PredictionMarket {
                id: "g2".into(),
                question: "BTC above 150k?".into(),
                probability: 0.2,
                volume_24h: 9000.0,
                category: MarketCategory::Crypto,
                updated_at: 1_000_000,
            },
        ];
        upsert_predictions(&conn, &markets).unwrap();
        let backend = to_backend(conn);
        assert!(run(&backend, None, None, true, 10, true).is_ok());
    }
}
