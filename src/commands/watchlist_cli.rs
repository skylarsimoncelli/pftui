use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::db::price_cache::get_all_cached_prices;
use crate::db::price_history::get_history;
use crate::db::watchlist::list_watchlist;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;

/// Format a decimal value with commas as thousands separators.
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

/// Map a watchlist symbol to its Yahoo Finance ticker.
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

/// Compute daily change % from price history for a symbol.
fn compute_change_pct(conn: &Connection, yahoo_sym: &str) -> Option<Decimal> {
    let history = get_history(conn, yahoo_sym, 2).ok()?;
    if history.len() < 2 {
        return None;
    }
    let prev = &history[history.len() - 2];
    let latest = &history[history.len() - 1];
    if prev.close == dec!(0) {
        return None;
    }
    Some((latest.close - prev.close) / prev.close * dec!(100))
}

pub fn run(conn: &Connection, config: &crate::config::Config, approaching: Option<&str>) -> Result<()> {
    let entries = list_watchlist(conn)?;

    if entries.is_empty() {
        println!("Watchlist is empty. Add symbols with: pftui watch <SYMBOL>");
        return Ok(());
    }

    // Parse approaching threshold (percentage)
    let approaching_pct: Option<Decimal> = approaching.and_then(|s| {
        let cleaned = s.replace('%', "");
        Decimal::from_str_exact(&cleaned).ok()
    });

    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, (Decimal, String)> = cached
        .into_iter()
        .map(|q| (q.symbol, (q.price, q.fetched_at)))
        .collect();

    // Row: symbol, name, category, price, change, target, proximity, fetched
    struct WatchRow {
        symbol: String,
        name: String,
        category: String,
        price: String,
        change: String,
        target: String,
        proximity: String,
        fetched: String,
        proximity_pct: Option<Decimal>,
    }

    let mut rows: Vec<WatchRow> = Vec::new();
    for entry in &entries {
        let name = resolve_name(&entry.symbol);
        let display_name = if name.is_empty() {
            entry.symbol.clone()
        } else {
            name
        };

        let cat: AssetCategory = entry
            .category
            .parse()
            .unwrap_or(AssetCategory::Equity);

        let csym = crate::config::currency_symbol(&config.base_currency);
        let current_price = prices.get(&entry.symbol).map(|(p, _)| *p);
        let (price_str, fetched_str) = match prices.get(&entry.symbol) {
            Some((price, fetched_at)) => {
                let p = format!("{}{}", csym, format_price(*price));
                let f = format_fetched_at(fetched_at);
                (p, f)
            }
            None => ("N/A".to_string(), "—".to_string()),
        };

        // Compute daily change %
        let yahoo_sym = yahoo_symbol_for(&entry.symbol, cat);
        let change_str = match compute_change_pct(conn, &yahoo_sym) {
            Some(pct) => {
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                format!("{:+.2}%", f)
            }
            None => "---".to_string(),
        };

        // Target and proximity
        let (target_str, proximity_str, proximity_pct) = match (
            &entry.target_price,
            &entry.target_direction,
            current_price,
        ) {
            (Some(tp), Some(dir), Some(cur)) => {
                if let Ok(target_dec) = Decimal::from_str_exact(tp) {
                    if target_dec.is_zero() {
                        ("---".to_string(), "---".to_string(), None)
                    } else {
                        let dist_pct = match dir.as_str() {
                            "below" => (cur - target_dec) / target_dec * dec!(100),
                            "above" => (target_dec - cur) / cur * dec!(100),
                            _ => dec!(0),
                        };
                        let dist_f: f64 = dist_pct.to_string().parse().unwrap_or(0.0);
                        let prox = if dist_f <= 0.0 {
                            "🎯 HIT".to_string()
                        } else {
                            format!("{:.1}% away", dist_f)
                        };
                        let tgt_str = format!("{} {}{}", dir, csym, format_price(target_dec));
                        (tgt_str, prox, Some(dist_pct))
                    }
                } else {
                    ("---".to_string(), "---".to_string(), None)
                }
            }
            (Some(tp), Some(dir), None) => {
                if let Ok(target_dec) = Decimal::from_str_exact(tp) {
                    let tgt_str = format!("{} {}{}", dir, csym, format_price(target_dec));
                    (tgt_str, "N/A".to_string(), None)
                } else {
                    ("---".to_string(), "---".to_string(), None)
                }
            }
            _ => ("---".to_string(), "---".to_string(), None),
        };

        rows.push(WatchRow {
            symbol: entry.symbol.clone(),
            name: display_name,
            category: entry.category.clone(),
            price: price_str,
            change: change_str,
            target: target_str,
            proximity: proximity_str,
            fetched: fetched_str,
            proximity_pct,
        });
    }

    // Filter by approaching threshold if set
    if let Some(threshold) = approaching_pct {
        rows.retain(|r| match r.proximity_pct {
            Some(pct) => pct >= dec!(0) && pct <= threshold,
            None => false,
        });
        if rows.is_empty() {
            println!("No watchlist symbols within {}% of their target.", threshold);
            return Ok(());
        }
    }

    // Sort by symbol for consistent output
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    // Check if any rows have targets
    let has_targets = rows.iter().any(|r| r.target != "---");

    // Compute column widths
    let sym_w = rows.iter().map(|r| r.symbol.len()).max().unwrap_or(6).max(6);
    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let cat_w = rows
        .iter()
        .map(|r| r.category.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let price_w = rows
        .iter()
        .map(|r| r.price.len())
        .max()
        .unwrap_or(5)
        .max(5);
    let chg_w = rows
        .iter()
        .map(|r| r.change.len())
        .max()
        .unwrap_or(8)
        .max(8);

    if has_targets {
        let tgt_w = rows.iter().map(|r| r.target.len()).max().unwrap_or(6).max(6);
        let prox_w = rows.iter().map(|r| r.proximity.len()).max().unwrap_or(9).max(9);

        // Header
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  {:>tgt_w$}  {:>prox_w$}  Updated",
            "Symbol", "Name", "Category", "Price", "1D Chg %", "Target", "Proximity",
        );
        let total_w = sym_w + name_w + cat_w + price_w + chg_w + tgt_w + prox_w + 30;
        println!("  {}", "─".repeat(total_w));

        for r in &rows {
            println!(
                "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  {:>tgt_w$}  {:>prox_w$}  {}",
                r.symbol, r.name, r.category, r.price, r.change, r.target, r.proximity, r.fetched,
            );
        }
    } else {
        // Header (no target columns)
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  Updated",
            "Symbol", "Name", "Category", "Price", "1D Chg %",
        );
        let total_w = sym_w + name_w + cat_w + price_w + chg_w + 24;
        println!("  {}", "─".repeat(total_w));

        for r in &rows {
            println!(
                "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  {}",
                r.symbol, r.name, r.category, r.price, r.change, r.fetched,
            );
        }
    }

    let priced = rows.iter().filter(|r| r.price != "N/A").count();
    let total = rows.len();
    if priced < total {
        println!();
        println!(
            "  {}/{} missing prices. Run `pftui refresh` to update.",
            total - priced,
            total
        );
    }

    Ok(())
}

/// Format a fetched_at timestamp into a human-readable relative time.
fn format_fetched_at(fetched_at: &str) -> String {
    // Parse ISO 8601 timestamp
    let parsed = chrono::DateTime::parse_from_rfc3339(fetched_at)
        .or_else(|_| chrono::DateTime::parse_from_str(fetched_at, "%Y-%m-%dT%H:%M:%S%.fZ"))
        .or_else(|_| {
            chrono::DateTime::parse_from_str(
                &format!("{}+00:00", fetched_at),
                "%Y-%m-%dT%H:%M:%S%.f%:z",
            )
        });

    match parsed {
        Ok(dt) => {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(dt.with_timezone(&chrono::Utc));
            let secs = diff.num_seconds();

            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                let mins = secs / 60;
                format!("{}m ago", mins)
            } else if secs < 86400 {
                let hours = secs / 3600;
                format!("{}h ago", hours)
            } else {
                let days = secs / 86400;
                format!("{}d ago", days)
            }
        }
        Err(_) => fetched_at.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_price_large() {
        assert_eq!(format_price(dec!(1234.56)), "1,234.56");
    }

    #[test]
    fn format_price_small() {
        assert_eq!(format_price(dec!(0.0045)), "0.0045");
    }

    #[test]
    fn format_price_medium() {
        assert_eq!(format_price(dec!(42.50)), "42.50");
    }

    #[test]
    fn format_price_zero() {
        assert_eq!(format_price(dec!(0)), "0.0000");
    }

    #[test]
    fn format_price_very_large() {
        assert_eq!(format_price(dec!(98765.43)), "98,765.43");
    }

    #[test]
    fn format_fetched_at_recent() {
        let now = chrono::Utc::now();
        let ts = now.to_rfc3339();
        let result = format_fetched_at(&ts);
        assert_eq!(result, "just now");
    }

    #[test]
    fn format_fetched_at_minutes() {
        let now = chrono::Utc::now() - chrono::Duration::minutes(15);
        let ts = now.to_rfc3339();
        let result = format_fetched_at(&ts);
        assert_eq!(result, "15m ago");
    }

    #[test]
    fn format_fetched_at_hours() {
        let now = chrono::Utc::now() - chrono::Duration::hours(3);
        let ts = now.to_rfc3339();
        let result = format_fetched_at(&ts);
        assert_eq!(result, "3h ago");
    }

    #[test]
    fn format_fetched_at_days() {
        let now = chrono::Utc::now() - chrono::Duration::days(2);
        let ts = now.to_rfc3339();
        let result = format_fetched_at(&ts);
        assert_eq!(result, "2d ago");
    }

    #[test]
    fn format_fetched_at_invalid() {
        assert_eq!(format_fetched_at("not-a-date"), "not-a-date");
    }

    #[test]
    fn watchlist_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        let result = run(&conn, &config, None);
        assert!(result.is_ok());
    }

    #[test]
    fn watchlist_with_entries_no_prices() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::asset::AssetCategory;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();

        let result = run(&conn, &config, None);
        assert!(result.is_ok());
    }

    #[test]
    fn watchlist_with_entries_and_prices() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::asset::AssetCategory;
        use crate::models::price::PriceQuote;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(195.50),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-02T20:00:00Z".to_string(),
            },
        )
        .unwrap();

        let result = run(&conn, &config, None);
        assert!(result.is_ok());
    }

    #[test]
    fn yahoo_symbol_crypto() {
        assert_eq!(
            yahoo_symbol_for("BTC", AssetCategory::Crypto),
            "BTC-USD"
        );
    }

    #[test]
    fn yahoo_symbol_crypto_already_suffixed() {
        assert_eq!(
            yahoo_symbol_for("BTC-USD", AssetCategory::Crypto),
            "BTC-USD"
        );
    }

    #[test]
    fn yahoo_symbol_equity() {
        assert_eq!(
            yahoo_symbol_for("AAPL", AssetCategory::Equity),
            "AAPL"
        );
    }

    #[test]
    fn yahoo_symbol_commodity() {
        assert_eq!(
            yahoo_symbol_for("GC=F", AssetCategory::Commodity),
            "GC=F"
        );
    }

    #[test]
    fn change_pct_no_history() {
        let conn = crate::db::open_in_memory();
        assert!(compute_change_pct(&conn, "AAPL").is_none());
    }

    #[test]
    fn change_pct_single_record() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-03".to_string(),
                close: dec!(195.50),
                volume: None,
            }],
        )
        .unwrap();

        assert!(compute_change_pct(&conn, "AAPL").is_none());
    }

    #[test]
    fn change_pct_two_records() {
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
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(210),
                    volume: None,
                },
            ],
        )
        .unwrap();

        let pct = compute_change_pct(&conn, "AAPL").unwrap();
        assert_eq!(pct, dec!(5));
    }

    #[test]
    fn change_pct_negative() {
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
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(190),
                    volume: None,
                },
            ],
        )
        .unwrap();

        let pct = compute_change_pct(&conn, "AAPL").unwrap();
        assert_eq!(pct, dec!(-5));
    }

    #[test]
    fn change_pct_zero_prev_close() {
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
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(100),
                    volume: None,
                },
            ],
        )
        .unwrap();

        assert!(compute_change_pct(&conn, "AAPL").is_none());
    }

    #[test]
    fn watchlist_with_history_shows_change() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::asset::AssetCategory;
        use crate::models::price::{HistoryRecord, PriceQuote};

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(210),
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
                    close: dec!(210),
                    volume: None,
                },
            ],
        )
        .unwrap();

        let result = run(&conn, &config, None);
        assert!(result.is_ok());
    }
}
