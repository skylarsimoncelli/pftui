use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::db::price_cache::get_all_cached_prices;
use crate::db::watchlist::list_watchlist;
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

pub fn run(conn: &Connection, config: &crate::config::Config) -> Result<()> {
    let entries = list_watchlist(conn)?;

    if entries.is_empty() {
        println!("Watchlist is empty. Add symbols with: pftui watch <SYMBOL>");
        return Ok(());
    }

    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, (Decimal, String)> = cached
        .into_iter()
        .map(|q| (q.symbol, (q.price, q.fetched_at)))
        .collect();

    // Compute column widths for alignment
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    for entry in &entries {
        let name = resolve_name(&entry.symbol);
        let display_name = if name.is_empty() {
            entry.symbol.clone()
        } else {
            name
        };

        let csym = crate::config::currency_symbol(&config.base_currency);
        let (price_str, fetched_str) = match prices.get(&entry.symbol) {
            Some((price, fetched_at)) => {
                let p = format!("{}{}", csym, format_price(*price));
                let f = format_fetched_at(fetched_at);
                (p, f)
            }
            None => ("N/A".to_string(), "—".to_string()),
        };

        rows.push((
            entry.symbol.clone(),
            display_name,
            entry.category.clone(),
            price_str,
            fetched_str,
        ));
    }

    // Sort by symbol for consistent output
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    // Compute column widths
    let sym_w = rows.iter().map(|r| r.0.len()).max().unwrap_or(6).max(6);
    let name_w = rows.iter().map(|r| r.1.len()).max().unwrap_or(4).max(4);
    let cat_w = rows
        .iter()
        .map(|r| r.2.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let price_w = rows
        .iter()
        .map(|r| r.3.len())
        .max()
        .unwrap_or(5)
        .max(5);

    // Header
    println!(
        "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  Updated",
        "Symbol", "Name", "Category", "Price",
    );
    let total_w = sym_w + name_w + cat_w + price_w + 20;
    println!("  {}", "─".repeat(total_w));

    // Rows
    for (symbol, name, category, price, fetched) in &rows {
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {}",
            symbol, name, category, price, fetched,
        );
    }

    let priced = rows.iter().filter(|r| r.3 != "N/A").count();
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
        let result = run(&conn, &config);
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

        let result = run(&conn, &config);
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

        let result = run(&conn, &config);
        assert!(result.is_ok());
    }
}
