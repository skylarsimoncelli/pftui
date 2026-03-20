use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::{get_history_backend, get_price_at_date_backend};
use crate::db::transactions::get_unique_symbols_backend;
use crate::db::watchlist::list_watchlist_backend;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;

#[derive(Serialize)]
struct PriceRow {
    symbol: String,
    name: String,
    price: Option<Decimal>,
    change: Option<Decimal>,
    change_pct: Option<Decimal>,
    source: String,
    fetched_at: String,
}

/// Compute daily change using price history.
fn compute_prev_close(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
    current: Decimal,
) -> Option<Decimal> {
    // Try canonical symbol first
    if let Some(prev) = prev_close_for(backend, symbol) {
        return Some(current - prev);
    }
    // Try Yahoo-mapped symbol for crypto
    let yahoo_sym = match category {
        AssetCategory::Crypto => {
            if symbol.ends_with("-USD") {
                return None;
            }
            format!("{}-USD", symbol)
        }
        _ => return None,
    };
    prev_close_for(backend, &yahoo_sym).map(|prev| current - prev)
}

fn prev_close_for(backend: &BackendConnection, symbol: &str) -> Option<Decimal> {
    let today = chrono::Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();

    get_price_at_date_backend(backend, symbol, &yesterday_str)
        .ok()
        .flatten()
        .or_else(|| {
            let history = get_history_backend(backend, symbol, 3).ok()?;
            if history.len() >= 2 {
                Some(history[history.len() - 2].close)
            } else {
                None
            }
        })
}

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    // Collect all tracked symbols: portfolio holdings + watchlist
    let mut symbols: BTreeMap<String, AssetCategory> = BTreeMap::new();

    // Portfolio holdings
    let held = get_unique_symbols_backend(backend)?;
    for (sym, cat) in &held {
        symbols.insert(sym.clone(), *cat);
    }

    // Watchlist
    let watched = list_watchlist_backend(backend)?;
    for entry in &watched {
        let cat: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
        symbols.entry(entry.symbol.clone()).or_insert(cat);
    }

    if symbols.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No portfolio holdings or watchlist symbols found.");
        }
        return Ok(());
    }

    // Load all cached prices
    let cached = get_all_cached_prices_backend(backend)?;
    let price_map: std::collections::HashMap<String, (Decimal, String, String)> = cached
        .into_iter()
        .map(|q| (q.symbol, (q.price, q.source, q.fetched_at)))
        .collect();

    // Also build a set of Yahoo-mapped crypto symbols for lookup
    let mut yahoo_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (sym, cat) in &symbols {
        if *cat == AssetCategory::Crypto && !sym.ends_with("-USD") {
            yahoo_map.insert(format!("{}-USD", sym), sym.clone());
        }
    }

    let mut rows: Vec<PriceRow> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (sym, cat) in &symbols {
        if !seen.insert(sym.clone()) {
            continue;
        }

        let name = resolve_name(sym);
        let display_name = if name.is_empty() {
            sym.clone()
        } else {
            name
        };

        // Look up price: try canonical symbol first, then Yahoo-mapped crypto
        let quote = price_map.get(sym).or_else(|| {
            if *cat == AssetCategory::Crypto && !sym.ends_with("-USD") {
                price_map.get(&format!("{}-USD", sym))
            } else {
                None
            }
        });

        let (price, source, fetched_at) = match quote {
            Some((p, src, ts)) => (Some(*p), src.clone(), ts.clone()),
            None => (None, String::new(), String::new()),
        };

        let (change, change_pct) = match price {
            Some(current) => {
                let chg = compute_prev_close(backend, sym, *cat, current);
                let pct = chg.and_then(|c| {
                    let prev = current - c;
                    if prev == dec!(0) {
                        None
                    } else {
                        Some(c / prev * dec!(100))
                    }
                });
                (chg, pct)
            }
            None => (None, None),
        };

        rows.push(PriceRow {
            symbol: sym.clone(),
            name: display_name,
            price,
            change,
            change_pct,
            source,
            fetched_at,
        });
    }

    if json {
        let json_str = serde_json::to_string_pretty(&rows)?;
        println!("{}", json_str);
        return Ok(());
    }

    // Table output
    if rows.is_empty() {
        println!("No tracked symbols found.");
        return Ok(());
    }

    let sym_w = rows.iter().map(|r| r.symbol.len()).max().unwrap_or(6).max(6);
    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let price_w = rows
        .iter()
        .map(|r| format_decimal_opt(r.price).len())
        .max()
        .unwrap_or(5)
        .max(5);
    let chg_w = rows
        .iter()
        .map(|r| format_change_opt(r.change).len())
        .max()
        .unwrap_or(6)
        .max(6);
    let pct_w = rows
        .iter()
        .map(|r| format_pct_opt(r.change_pct).len())
        .max()
        .unwrap_or(8)
        .max(8);

    println!(
        "  {:<sym_w$}  {:<name_w$}  {:>price_w$}  {:>chg_w$}  {:>pct_w$}",
        "Symbol", "Name", "Price", "Change", "Chg %",
    );
    let total_w = sym_w + name_w + price_w + chg_w + pct_w + 16;
    println!("  {}", "\u{2500}".repeat(total_w));

    for r in &rows {
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:>price_w$}  {:>chg_w$}  {:>pct_w$}",
            r.symbol,
            r.name,
            format_decimal_opt(r.price),
            format_change_opt(r.change),
            format_pct_opt(r.change_pct),
        );
    }

    let priced = rows.iter().filter(|r| r.price.is_some()).count();
    let total = rows.len();
    println!();
    println!("  {}/{} symbols with cached prices.", priced, total);
    if priced < total {
        println!("  Run `pftui data refresh` to update missing prices.");
    }

    Ok(())
}

fn format_decimal_opt(v: Option<Decimal>) -> String {
    match v {
        Some(d) => {
            let dp = if d.abs() >= dec!(1) { 2 } else { 4 };
            format!("{:.prec$}", d.round_dp(dp), prec = dp as usize)
        }
        None => "N/A".to_string(),
    }
}

fn format_change_opt(v: Option<Decimal>) -> String {
    match v {
        Some(d) => {
            let dp = if d.abs() >= dec!(1) { 2 } else { 4 };
            format!("{:+.prec$}", d.round_dp(dp), prec = dp as usize)
        }
        None => "---".to_string(),
    }
}

fn format_pct_opt(v: Option<Decimal>) -> String {
    match v {
        Some(d) => {
            let f: f64 = d.to_string().parse().unwrap_or(0.0);
            format!("{:+.2}%", f)
        }
        None => "---".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::backend::BackendConnection;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn prices_empty_db() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, false);
        assert!(result.is_ok());
    }

    #[test]
    fn prices_empty_db_json() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, true);
        assert!(result.is_ok());
    }

    #[test]
    fn prices_with_watchlist_and_holdings() {
        let conn = open_in_memory();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;
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
                fetched_at: "2026-03-18T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
                open: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC-USD".to_string(),
                price: dec!(84000),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-18T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
                open: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);

        // Table output
        let result = run(&backend, false);
        assert!(result.is_ok());

        // JSON output
        let result = run(&backend, true);
        assert!(result.is_ok());
    }

    #[test]
    fn format_decimal_opt_large() {
        assert_eq!(format_decimal_opt(Some(dec!(1234.56))), "1234.56");
    }

    #[test]
    fn format_decimal_opt_small() {
        assert_eq!(format_decimal_opt(Some(dec!(0.0045))), "0.0045");
    }

    #[test]
    fn format_decimal_opt_none() {
        assert_eq!(format_decimal_opt(None), "N/A");
    }

    #[test]
    fn format_change_opt_positive() {
        assert_eq!(format_change_opt(Some(dec!(5.25))), "+5.25");
    }

    #[test]
    fn format_change_opt_negative() {
        assert_eq!(format_change_opt(Some(dec!(-3.10))), "-3.10");
    }

    #[test]
    fn format_pct_opt_positive() {
        assert_eq!(format_pct_opt(Some(dec!(2.5))), "+2.50%");
    }

    #[test]
    fn format_pct_opt_none() {
        assert_eq!(format_pct_opt(None), "---");
    }
}
