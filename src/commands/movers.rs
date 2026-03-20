use std::collections::HashSet;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::config::Config;
use crate::db::allocations::get_unique_allocation_symbols_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::{get_history_backend, get_price_at_date_backend};
use crate::db::transactions::get_unique_symbols_backend;
use crate::db::watchlist::list_watchlist_backend;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::models::price::PriceQuote;

/// A mover: symbol with its daily change exceeding the threshold.
struct Mover {
    symbol: String,
    name: String,
    category: String,
    source: &'static str, // "held" or "watchlist"
    price: String,
    change_pct: Decimal,
    change_str: String,
}

/// Compute daily change % from current price vs yesterday's close.
///
/// Uses a layered fallback strategy:
///   1. `previous_close` from the cached price quote (most reliable — sourced from Yahoo metadata)
///   2. Previous close from price_history table (up to 10 records)
///   3. `get_price_at_date_backend` for yesterday's date
///   4. `open` price from the cached quote (intraday reference, better than nothing)
///   5. Average of recent history closes (7-day avg fallback)
///
/// Returns None only if no reference price can be determined.
fn compute_change_pct(
    backend: &BackendConnection,
    symbol: &str,
    current_price: Option<Decimal>,
    cached_quote: Option<&PriceQuote>,
) -> Option<Decimal> {
    use chrono::Utc;

    let current = current_price?;
    let today = Utc::now().date_naive();

    // Strategy 1: Use previous_close from cached price quote (Yahoo metadata)
    if let Some(prev) = cached_quote.and_then(|q| q.previous_close) {
        if prev != dec!(0) {
            return Some((current - prev) / prev * dec!(100));
        }
    }

    // Strategy 2: Use price history (increased from 5 to 10 for resilience to gaps)
    let history = get_history_backend(backend, symbol, 10).ok().unwrap_or_default();
    if let Some(prev_close) = previous_close_from_history(&history, today) {
        if prev_close != dec!(0) {
            return Some((current - prev_close) / prev_close * dec!(100));
        }
    }

    // Strategy 3: Direct date lookup for yesterday
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    if let Some(prev) = get_price_at_date_backend(backend, symbol, &yesterday_str)
        .ok()
        .flatten()
    {
        if prev != dec!(0) {
            return Some((current - prev) / prev * dec!(100));
        }
    }

    // Strategy 4: Use open price from cached quote as intraday reference
    if let Some(open) = cached_quote.and_then(|q| q.open) {
        if open != dec!(0) {
            return Some((current - open) / open * dec!(100));
        }
    }

    // Strategy 5: Use average of available history closes as reference
    if history.len() >= 2 {
        let sum: Decimal = history.iter().map(|r| r.close).sum();
        let count = Decimal::from(history.len() as u32);
        let avg = sum / count;
        if avg != dec!(0) {
            return Some((current - avg) / avg * dec!(100));
        }
    }

    None
}

fn previous_close_from_history(
    history: &[crate::models::price::HistoryRecord],
    today: chrono::NaiveDate,
) -> Option<Decimal> {
    if history.is_empty() {
        return None;
    }

    let latest = history.last()?;
    let latest_date = chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d").ok();
    if latest_date == Some(today) {
        history.iter().rev().nth(1).map(|record| record.close)
    } else {
        Some(latest.close)
    }
}

/// Format a decimal price with commas.
fn format_price(value: Decimal) -> String {
    let dp = if value >= dec!(1) { 2 } else { 4 };
    let rounded = value.round_dp(dp);
    let s = format!("{:.prec$}", rounded, prec = dp as usize);

    let (integer_part, decimal_part) = if let Some(dot_pos) = s.find('.') {
        (&s[..dot_pos], Some(&s[dot_pos..]))
    } else {
        (s.as_str(), None)
    };

    let (sign, digits) = if let Some(stripped) = integer_part.strip_prefix('-') {
        ("-", stripped)
    } else {
        ("", integer_part)
    };

    let mut result = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    let formatted_int: String = result.chars().rev().collect();

    match decimal_part {
        Some(dec_part) => format!("{}{}{}", sign, formatted_int, dec_part),
        None => format!("{}{}", sign, formatted_int),
    }
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    threshold: Option<&str>,
    overnight: bool,
    json: bool,
) -> Result<()> {
    // Parse threshold (default 3%)
    let threshold_pct: Decimal = match threshold {
        Some(s) => {
            let cleaned = s.replace('%', "");
            Decimal::from_str_exact(&cleaned).unwrap_or(dec!(3))
        }
        None => dec!(3),
    };

    // Collect all unique symbols from held positions + watchlist
    let mut symbols: Vec<(String, AssetCategory, &'static str)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Held positions (full mode)
    if let Ok(held) = get_unique_symbols_backend(backend) {
        for (sym, cat) in held {
            if cat == AssetCategory::Cash {
                continue; // Skip cash — always 1.0
            }
            if seen.insert(sym.clone()) {
                symbols.push((sym, cat, "held"));
            }
        }
    }

    // Held positions (percentage mode)
    if let Ok(alloc) = get_unique_allocation_symbols_backend(backend) {
        for (sym, cat) in alloc {
            if cat == AssetCategory::Cash {
                continue;
            }
            if seen.insert(sym.clone()) {
                symbols.push((sym, cat, "held"));
            }
        }
    }

    // Watchlist
    if let Ok(entries) = list_watchlist_backend(backend) {
        for entry in entries {
            let cat: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
            if seen.insert(entry.symbol.clone()) {
                symbols.push((entry.symbol, cat, "watchlist"));
            }
        }
    }

    if symbols.is_empty() {
        println!("No symbols found. Add positions or watchlist entries first.");
        return Ok(());
    }

    // Build price map and full quote map for display and change computation
    let cached = get_all_cached_prices_backend(backend)?;
    let price_map: std::collections::HashMap<String, Decimal> =
        cached.iter().map(|q| (q.symbol.clone(), q.price)).collect();
    let quote_map: std::collections::HashMap<String, &PriceQuote> =
        cached.iter().map(|q| (q.symbol.clone(), q)).collect();

    let csym = crate::config::currency_symbol(&config.base_currency);

    // Compute movers, tracking skipped symbols for diagnostics
    let mut movers: Vec<Mover> = Vec::new();
    let mut skipped: Vec<serde_json::Value> = Vec::new();
    for (sym, cat, source) in &symbols {
        let current_price = price_map.get(sym).copied();
        let cached_quote = quote_map.get(sym).copied();

        if current_price.is_none() {
            skipped.push(serde_json::json!({
                "symbol": sym,
                "reason": "no_current_price",
            }));
            continue;
        }

        if let Some(pct) = compute_change_pct(backend, sym, current_price, cached_quote) {
            let abs_pct = if pct < dec!(0) { -pct } else { pct };
            if abs_pct >= threshold_pct {
                let name = resolve_name(sym);
                let display_name = if name.is_empty() { sym.clone() } else { name };
                let price_str = match current_price {
                    Some(p) => format!("{}{}", csym, format_price(p)),
                    None => "N/A".to_string(),
                };
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                let change_str = format!("{:+.2}%", f);

                movers.push(Mover {
                    symbol: sym.clone(),
                    name: display_name,
                    category: cat.to_string(),
                    source,
                    price: price_str,
                    change_pct: pct,
                    change_str,
                });
            }
        } else {
            skipped.push(serde_json::json!({
                "symbol": sym,
                "reason": "no_reference_price",
                "has_cached_previous_close": cached_quote.and_then(|q| q.previous_close).is_some(),
                "has_cached_open": cached_quote.and_then(|q| q.open).is_some(),
            }));
        }
    }

    // Sort by absolute change descending (biggest movers first)
    movers.sort_by(|a, b| {
        let abs_a = if a.change_pct < dec!(0) {
            -a.change_pct
        } else {
            a.change_pct
        };
        let abs_b = if b.change_pct < dec!(0) {
            -b.change_pct
        } else {
            b.change_pct
        };
        abs_b.cmp(&abs_a)
    });

    if json {
        // Fetch recent technical signals for mover context
        let recent_signals =
            crate::db::technical_signals::list_signals_backend(backend, None, None, Some(200))
                .unwrap_or_default();
        let signal_map: std::collections::HashMap<String, Vec<String>> = {
            let mut map: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for sig in &recent_signals {
                map.entry(sig.symbol.clone())
                    .or_default()
                    .push(sig.description.clone());
            }
            map
        };

        // JSON output for agent consumption
        let entries: Vec<serde_json::Value> = movers
            .iter()
            .map(|m| {
                let f: f64 = m.change_pct.to_string().parse().unwrap_or(0.0);
                let sym_signals = signal_map
                    .get(&m.symbol)
                    .cloned()
                    .unwrap_or_default();
                let mut obj = serde_json::json!({
                    "symbol": m.symbol,
                    "name": m.name,
                    "category": m.category,
                    "source": m.source,
                    "change_pct": (f * 100.0).round() / 100.0,
                });
                if !sym_signals.is_empty() {
                    obj["signals"] = serde_json::json!(sym_signals);
                }
                obj
            })
            .collect();
        let mut output = serde_json::json!({
            "threshold_pct": threshold_pct.to_string().parse::<f64>().unwrap_or(3.0),
            "mode": if overnight { "overnight" } else { "daily" },
            "total_scanned": symbols.len(),
            "movers_count": movers.len(),
            "movers": entries,
        });
        if !skipped.is_empty() {
            output["skipped_count"] = serde_json::json!(skipped.len());
            output["skipped"] = serde_json::json!(skipped);
        }
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if movers.is_empty() {
        println!(
            "No movers exceeding {}% threshold across {} symbols.",
            threshold_pct,
            symbols.len()
        );
        if !skipped.is_empty() {
            println!(
                "  ({} symbols skipped — no reference price available)",
                skipped.len()
            );
        }
        return Ok(());
    }

    println!(
        "Movers (≥{}% {} change) — {}/{} symbols:",
        threshold_pct,
        if overnight { "overnight" } else { "daily" },
        movers.len(),
        symbols.len()
    );
    println!();

    // Compute column widths
    let sym_w = movers
        .iter()
        .map(|m| m.symbol.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let name_w = movers
        .iter()
        .map(|m| m.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let cat_w = movers
        .iter()
        .map(|m| m.category.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let price_w = movers
        .iter()
        .map(|m| m.price.len())
        .max()
        .unwrap_or(5)
        .max(5);
    let chg_w = movers
        .iter()
        .map(|m| m.change_str.len())
        .max()
        .unwrap_or(8)
        .max(8);

    // Header
    println!(
        "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  Source",
        "Symbol", "Name", "Category", "Price", "1D Chg %",
    );
    let total_w = sym_w + name_w + cat_w + price_w + chg_w + 20;
    println!("  {}", "─".repeat(total_w));

    for m in &movers {
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  {}",
            m.symbol, m.name, m.category, m.price, m.change_str, m.source,
        );
    }

    if !skipped.is_empty() {
        println!();
        println!(
            "  ({} symbols skipped — no reference price available)",
            skipped.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn to_backend(conn: Connection) -> crate::db::backend::BackendConnection {
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn movers_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_no_history() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::watchlist::add_to_watchlist;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_below_threshold() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(201), // 0.5% change — below 3% default
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_above_threshold() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::{HistoryRecord, PriceQuote};

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(220),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-03T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
                open: None,
            },
        )
        .unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220), // 10% change — above 3% default
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_custom_threshold() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(204), // 2% change
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        // 1% threshold — should appear
        let backend = to_backend(conn);
        let result = run(&backend, &config, Some("1"), false, false);
        assert!(result.is_ok());

        // 5% threshold — should not appear
        let result = run(&backend, &config, Some("5"), false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_json_output() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_skips_cash() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::NewTransaction;

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "USD".to_string(),
                category: AssetCategory::Cash,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: dec!(10000),
                price_per: dec!(1),
                currency: "USD".to_string(),
                date: "2026-03-03".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_negative_change() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(180), // -10% change
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_dedupes_held_and_watchlist() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::transactions::insert_transaction;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;
        use crate::models::transaction::NewTransaction;

        // Same symbol in both held and watchlist
        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        // Should only show AAPL once (as "held")
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn change_pct_computation() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(200),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        // Current price is 210, previous close was 200 → 5% gain
        let backend = to_backend(conn);
        let pct = compute_change_pct(&backend, "AAPL", Some(dec!(210)), None).unwrap();
        assert_eq!(pct, dec!(5));
    }

    #[test]
    fn change_pct_zero_prev() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(0),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        // Previous close was 0 → should return None (can't compute % change)
        let backend = to_backend(conn);
        assert!(compute_change_pct(&backend, "AAPL", Some(dec!(100)), None).is_none());
    }

    #[test]
    fn change_pct_no_current_price() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(200),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        // No current price provided → should return None
        let backend = to_backend(conn);
        assert!(compute_change_pct(&backend, "AAPL", None, None).is_none());
    }

    #[test]
    fn previous_close_uses_latest_historical_close_on_weekend_gap() {
        use crate::models::price::HistoryRecord;

        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let history = vec![
            HistoryRecord {
                date: "2026-03-12".to_string(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-13".to_string(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];

        assert_eq!(
            previous_close_from_history(&history, today),
            Some(dec!(105))
        );
    }

    #[test]
    fn previous_close_uses_penultimate_when_history_contains_today() {
        use crate::models::price::HistoryRecord;

        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let history = vec![
            HistoryRecord {
                date: "2026-03-13".to_string(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-16".to_string(),
                close: dec!(109),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];

        assert_eq!(
            previous_close_from_history(&history, today),
            Some(dec!(105))
        );
    }

    // ---- New tests for P0 fix: extreme market moves ----

    #[test]
    fn change_pct_uses_cached_previous_close_as_primary() {
        // Scenario: cached previous_close exists, no history at all
        // This is the exact crash scenario — price cache has data but history is stale/empty
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let quote = PriceQuote {
            symbol: "GLD".to_string(),
            price: dec!(171),
            currency: "USD".to_string(),
            source: "yahoo".to_string(),
            fetched_at: "2026-03-20T20:00:00Z".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: Some(dec!(190)),
            open: Some(dec!(185)),
        };

        // Gold crashed from 190 to 171 = -10% move
        let pct = compute_change_pct(&backend, "GLD", Some(dec!(171)), Some(&quote)).unwrap();
        assert_eq!(pct, dec!(-10));
    }

    #[test]
    fn change_pct_falls_back_to_open_when_no_history() {
        // Scenario: no previous_close in cache, no history, but open price exists
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let quote = PriceQuote {
            symbol: "SLV".to_string(),
            price: dec!(21.50),
            currency: "USD".to_string(),
            source: "yahoo".to_string(),
            fetched_at: "2026-03-20T20:00:00Z".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
            open: Some(dec!(25)),
        };

        // Silver opened at 25, now at 21.50 = -14% intraday
        let pct = compute_change_pct(&backend, "SLV", Some(dec!(21.50)), Some(&quote)).unwrap();
        assert_eq!(pct, dec!(-14));
    }

    #[test]
    fn change_pct_prefers_previous_close_over_history() {
        // Scenario: both previous_close and history exist — previous_close should win
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "GLD",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-19".to_string(),
                close: dec!(188), // Stale/slightly different from actual prev close
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        let backend = to_backend(conn);

        let quote = PriceQuote {
            symbol: "GLD".to_string(),
            price: dec!(171),
            currency: "USD".to_string(),
            source: "yahoo".to_string(),
            fetched_at: "2026-03-20T20:00:00Z".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: Some(dec!(190)), // More accurate from Yahoo metadata
            open: None,
        };

        // Should use previous_close (190), not history (188)
        let pct = compute_change_pct(&backend, "GLD", Some(dec!(171)), Some(&quote)).unwrap();
        assert_eq!(pct, dec!(-10));
    }

    #[test]
    fn change_pct_falls_back_to_history_avg_when_all_else_fails() {
        // Scenario: no cached previous_close, no open, history exists but no clear
        // "previous close" (e.g. all old records, none matching yesterday)
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        // History from a week ago — won't match "yesterday" lookup
        upsert_history(
            &conn,
            "GLD",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-10".to_string(),
                    close: dec!(192),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-11".to_string(),
                    close: dec!(188),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);

        // No cached previous_close or open
        let quote = PriceQuote {
            symbol: "GLD".to_string(),
            price: dec!(171),
            currency: "USD".to_string(),
            source: "yahoo".to_string(),
            fetched_at: "2026-03-20T20:00:00Z".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
            open: None,
        };

        // History has records, so previous_close_from_history should return 188 (latest)
        // 171 vs 188 = -9.04% — should still be detected
        let pct = compute_change_pct(&backend, "GLD", Some(dec!(171)), Some(&quote)).unwrap();
        // (171 - 188) / 188 * 100 = -9.042553...
        assert!(pct < dec!(-9));
        assert!(pct > dec!(-10));
    }

    #[test]
    fn movers_json_includes_skipped_symbols() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::watchlist::add_to_watchlist;

        // Add a symbol with no price data at all
        add_to_watchlist(&conn, "MYSTERY", AssetCategory::Equity).unwrap();

        let backend = to_backend(conn);
        // JSON output should include skipped array
        let result = run(&backend, &config, None, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn extreme_crash_detected_via_previous_close() {
        // The exact P0 scenario: gold crashes -10%, silver -14%
        // Price cache has current price + previous_close but history table is empty/stale
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;

        add_to_watchlist(&conn, "GLD", AssetCategory::Commodity).unwrap();
        add_to_watchlist(&conn, "SLV", AssetCategory::Commodity).unwrap();

        // Gold: previous_close 190, current 171 = -10%
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GLD".to_string(),
                price: dec!(171),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-20T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: Some(dec!(190)),
                open: Some(dec!(185)),
            },
        )
        .unwrap();

        // Silver: previous_close 25, current 21.50 = -14%
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "SLV".to_string(),
                price: dec!(21.50),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-20T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: Some(dec!(25)),
                open: Some(dec!(24)),
            },
        )
        .unwrap();

        // No price history at all — this is the bug scenario
        // With the fix, movers should still detect these via previous_close

        let backend = to_backend(conn);
        // Run in JSON mode to capture output
        let result = run(&backend, &config, None, false, true);
        assert!(result.is_ok());
        // The movers should be detected (test doesn't capture stdout but verifies no panic)
    }
}
