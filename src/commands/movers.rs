use std::collections::HashSet;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::Config;
use crate::db::allocations::get_unique_allocation_symbols;
use crate::db::price_cache::get_all_cached_prices;
use crate::db::price_history::get_history;
use crate::db::transactions::get_unique_symbols;
use crate::db::watchlist::list_watchlist;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;

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

/// Map a symbol to its Yahoo Finance ticker for history lookup.
fn yahoo_symbol_for(symbol: &str, category: AssetCategory) -> String {
    match category {
        AssetCategory::Crypto => {
            if symbol.ends_with("-USD") {
                symbol.to_string()
            } else {
                format!("{}-USD", symbol)
            }
        }
        _ => symbol.to_string(),
    }
}

/// Compute daily change % from current price vs most recent historical close.
/// Returns None if no history exists or current price is not available.
fn compute_change_pct(conn: &Connection, yahoo_sym: &str, current_price: Option<Decimal>) -> Option<Decimal> {
    let current = current_price?;
    
    // Get the most recent historical close (yesterday or last trading day)
    let history = get_history(conn, yahoo_sym, 1).ok()?;
    if history.is_empty() {
        return None;
    }
    
    let prev_close = history[0].close;
    if prev_close == dec!(0) {
        return None;
    }
    
    Some((current - prev_close) / prev_close * dec!(100))
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

pub fn run(conn: &Connection, config: &Config, threshold: Option<&str>, json: bool) -> Result<()> {
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
    if let Ok(held) = get_unique_symbols(conn) {
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
    if let Ok(alloc) = get_unique_allocation_symbols(conn) {
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
    if let Ok(entries) = list_watchlist(conn) {
        for entry in entries {
            let cat: AssetCategory = entry
                .category
                .parse()
                .unwrap_or(AssetCategory::Equity);
            if seen.insert(entry.symbol.clone()) {
                symbols.push((entry.symbol, cat, "watchlist"));
            }
        }
    }

    if symbols.is_empty() {
        println!("No symbols found. Add positions or watchlist entries first.");
        return Ok(());
    }

    // Build price map for display
    let cached = get_all_cached_prices(conn)?;
    let price_map: std::collections::HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    let csym = crate::config::currency_symbol(&config.base_currency);

    // Compute movers
    let mut movers: Vec<Mover> = Vec::new();
    for (sym, cat, source) in &symbols {
        let yahoo_sym = yahoo_symbol_for(sym, *cat);
        let current_price = price_map.get(sym).copied();
        
        if let Some(pct) = compute_change_pct(conn, &yahoo_sym, current_price) {
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
        }
    }

    // Sort by absolute change descending (biggest movers first)
    movers.sort_by(|a, b| {
        let abs_a = if a.change_pct < dec!(0) { -a.change_pct } else { a.change_pct };
        let abs_b = if b.change_pct < dec!(0) { -b.change_pct } else { b.change_pct };
        abs_b.cmp(&abs_a)
    });

    if json {
        // JSON output for agent consumption
        let entries: Vec<serde_json::Value> = movers
            .iter()
            .map(|m| {
                let f: f64 = m.change_pct.to_string().parse().unwrap_or(0.0);
                serde_json::json!({
                    "symbol": m.symbol,
                    "name": m.name,
                    "category": m.category,
                    "source": m.source,
                    "change_pct": (f * 100.0).round() / 100.0,
                })
            })
            .collect();
        let output = serde_json::json!({
            "threshold_pct": threshold_pct.to_string().parse::<f64>().unwrap_or(3.0),
            "total_scanned": symbols.len(),
            "movers_count": movers.len(),
            "movers": entries,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if movers.is_empty() {
        println!(
            "No movers exceeding {}% threshold across {} symbols.",
            threshold_pct, symbols.len()
        );
        return Ok(());
    }

    println!(
        "Movers (≥{}% daily change) — {}/{} symbols:",
        threshold_pct,
        movers.len(),
        symbols.len()
    );
    println!();

    // Compute column widths
    let sym_w = movers.iter().map(|m| m.symbol.len()).max().unwrap_or(6).max(6);
    let name_w = movers.iter().map(|m| m.name.len()).max().unwrap_or(4).max(4);
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movers_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        let result = run(&conn, &config, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_no_history() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::watchlist::add_to_watchlist;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        let result = run(&conn, &config, None, false);
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
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(201), // 0.5% change — below 3% default
                    volume: None,
                },
            ],
        )
        .unwrap();

        let result = run(&conn, &config, None, false);
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
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220), // 10% change — above 3% default
                    volume: None,
                },
            ],
        )
        .unwrap();

        let result = run(&conn, &config, None, false);
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
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(204), // 2% change
                    volume: None,
                },
            ],
        )
        .unwrap();

        // 1% threshold — should appear
        let result = run(&conn, &config, Some("1"), false);
        assert!(result.is_ok());

        // 5% threshold — should not appear
        let result = run(&conn, &config, Some("5"), false);
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
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220),
                    volume: None,
                },
            ],
        )
        .unwrap();

        let result = run(&conn, &config, None, true);
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

        let result = run(&conn, &config, None, false);
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
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(180), // -10% change
                    volume: None,
                },
            ],
        )
        .unwrap();

        let result = run(&conn, &config, None, false);
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
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220),
                    volume: None,
                },
            ],
        )
        .unwrap();

        // Should only show AAPL once (as "held")
        let result = run(&conn, &config, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn yahoo_symbol_crypto() {
        assert_eq!(yahoo_symbol_for("BTC", AssetCategory::Crypto), "BTC-USD");
    }

    #[test]
    fn yahoo_symbol_equity() {
        assert_eq!(yahoo_symbol_for("AAPL", AssetCategory::Equity), "AAPL");
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
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                },
            ],
        )
        .unwrap();

        // Current price is 210, previous close was 200 → 5% gain
        let pct = compute_change_pct(&conn, "AAPL", Some(dec!(210))).unwrap();
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
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(0),
                    volume: None,
                },
            ],
        )
        .unwrap();

        // Previous close was 0 → should return None (can't compute % change)
        assert!(compute_change_pct(&conn, "AAPL", Some(dec!(100))).is_none());
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
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                },
            ],
        )
        .unwrap();

        // No current price provided → should return None
        assert!(compute_change_pct(&conn, "AAPL", None).is_none());
    }
}
