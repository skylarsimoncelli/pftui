use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::get_unique_allocation_symbols;
use crate::db::price_cache::upsert_price;
use crate::db::transactions::get_unique_symbols;
use crate::db::watchlist::get_watchlist_symbols;
use crate::models::asset::AssetCategory;
use crate::models::price::PriceQuote;
use crate::price::{coingecko, yahoo};

/// Delay between sequential Yahoo Finance API requests to avoid rate limiting.
const YAHOO_RATE_LIMIT_DELAY: Duration = Duration::from_millis(100);

/// Collect all symbols that need pricing: portfolio positions + watchlist.
fn collect_symbols(
    conn: &Connection,
    config: &Config,
) -> Result<Vec<(String, AssetCategory)>> {
    let mut seen = HashMap::new();

    // Portfolio symbols (transactions or allocations depending on mode)
    let portfolio_symbols = match config.portfolio_mode {
        PortfolioMode::Full => get_unique_symbols(conn)?,
        PortfolioMode::Percentage => get_unique_allocation_symbols(conn)?,
    };
    for (sym, cat) in portfolio_symbols {
        seen.entry(sym).or_insert(cat);
    }

    // Watchlist symbols
    let watchlist_symbols = get_watchlist_symbols(conn)?;
    for (sym, cat) in watchlist_symbols {
        seen.entry(sym).or_insert(cat);
    }

    Ok(seen.into_iter().collect())
}

/// Format a crypto symbol for Yahoo Finance (append -USD if not already present).
fn yahoo_crypto_symbol(symbol: &str) -> String {
    let upper = symbol.to_uppercase();
    if upper.ends_with("-USD") {
        upper
    } else {
        format!("{}-USD", upper)
    }
}

/// Fetch prices for all given symbols and return the results.
async fn fetch_all_prices(
    symbols: &[(String, AssetCategory)],
    config: &Config,
) -> (Vec<PriceQuote>, Vec<String>) {
    let mut quotes = Vec::new();
    let mut errors = Vec::new();

    let mut yahoo_symbols = Vec::new();
    let mut crypto_symbols = Vec::new();

    for (sym, cat) in symbols {
        match cat {
            AssetCategory::Cash => {
                // Cash is always 1:1
                quotes.push(PriceQuote {
                    symbol: sym.clone(),
                    price: dec!(1),
                    currency: "USD".to_string(),
                    source: "static".to_string(),
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                });
            }
            AssetCategory::Crypto => crypto_symbols.push(sym.clone()),
            _ => yahoo_symbols.push(sym.clone()),
        }
    }

    // Forex rate if non-USD base currency
    if config.base_currency != "USD" {
        let pair = format!("USD{}=X", config.base_currency);
        yahoo_symbols.push(pair);
    }

    // Fetch Yahoo prices with rate limiting (~100ms between requests)
    for (i, sym) in yahoo_symbols.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(YAHOO_RATE_LIMIT_DELAY).await;
        }
        match yahoo::fetch_price(sym).await {
            Ok(quote) => quotes.push(quote),
            Err(e) => errors.push(format!("{}: {}", sym, e)),
        }
    }

    // Fetch crypto: CoinGecko batch first, Yahoo fallback
    if !crypto_symbols.is_empty() {
        let mut cg_ok = false;
        match coingecko::fetch_prices(&crypto_symbols).await {
            Ok(cg_quotes) if !cg_quotes.is_empty() => {
                for q in cg_quotes {
                    quotes.push(q);
                }
                cg_ok = true;
            }
            Ok(_) => {
                errors.push("CoinGecko returned empty, falling back to Yahoo".to_string());
            }
            Err(e) => {
                errors.push(format!("CoinGecko batch failed: {}, falling back to Yahoo", e));
            }
        }

        if !cg_ok {
            for (i, sym) in crypto_symbols.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(YAHOO_RATE_LIMIT_DELAY).await;
                }
                let yahoo_sym = yahoo_crypto_symbol(sym);
                match yahoo::fetch_price(&yahoo_sym).await {
                    Ok(mut quote) => {
                        quote.symbol = sym.clone();
                        quotes.push(quote);
                    }
                    Err(e) => {
                        errors.push(format!("{}: CoinGecko + Yahoo both failed: {}", sym, e));
                    }
                }
            }
        }
    }

    (quotes, errors)
}

/// Format a price for display: compact representation.
fn format_price(price: Decimal, sym: &str) -> String {
    if price >= dec!(10000) {
        format!("{}{}", sym, price.round_dp(0))
    } else if price >= dec!(100) {
        format!("{}{}", sym, price.round_dp(1))
    } else if price >= dec!(1) {
        format!("{}{}", sym, price.round_dp(2))
    } else {
        format!("{}{}", sym, price.round_dp(4))
    }
}

pub fn run(conn: &Connection, config: &Config) -> Result<()> {
    let symbols = collect_symbols(conn, config)?;
    if symbols.is_empty() {
        println!("No symbols to refresh. Add positions with `pftui setup` or `pftui add-tx`, or watch symbols with `pftui watch`.");
        return Ok(());
    }

    let non_cash: Vec<_> = symbols
        .iter()
        .filter(|(_, cat)| *cat != AssetCategory::Cash)
        .collect();
    let total = non_cash.len();
    let cash_count = symbols.len() - total;

    println!(
        "Refreshing {} symbol{}{}...",
        total,
        if total == 1 { "" } else { "s" },
        if cash_count > 0 {
            format!(" (+{} cash)", cash_count)
        } else {
            String::new()
        }
    );

    // Build a tokio runtime for the async fetch
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let (quotes, errors) = rt.block_on(fetch_all_prices(&symbols, config));

    // Write to price cache
    let mut cached_count = 0;
    for quote in &quotes {
        if let Err(e) = upsert_price(conn, quote) {
            eprintln!("Failed to cache {}: {}", quote.symbol, e);
        } else {
            cached_count += 1;
        }
    }

    // Print results
    let mut fetched: Vec<_> = quotes
        .iter()
        .filter(|q| q.source != "static")
        .collect();
    fetched.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    if fetched.is_empty() && errors.is_empty() {
        println!("No symbols needed price fetching (cash only).");
        return Ok(());
    }

    // Print each fetched price
    for q in &fetched {
        let csym = crate::config::currency_symbol(&config.base_currency);
        println!("  {} {} ({})", q.symbol, format_price(q.price, csym), q.source);
    }

    // Print errors
    for err in &errors {
        eprintln!("  ✗ {}", err);
    }

    // Summary line
    let ok_count = fetched.len();
    let err_count = symbols
        .iter()
        .filter(|(sym, cat)| {
            *cat != AssetCategory::Cash
                && !quotes.iter().any(|q| q.symbol == *sym)
        })
        .count();

    if err_count > 0 {
        println!(
            "Refreshed {}/{} symbols ({} failed). {} cached.",
            ok_count, total, err_count, cached_count
        );
    } else {
        println!(
            "Refreshed {} symbol{}. {} cached.",
            ok_count,
            if ok_count == 1 { "" } else { "s" },
            cached_count
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_price_large() {
        assert_eq!(format_price(dec!(84200), "$"), "$84200");
        assert_eq!(format_price(dec!(84200.56), "$"), "$84201");
    }

    #[test]
    fn format_price_medium() {
        assert_eq!(format_price(dec!(189.50), "$"), "$189.5");
        assert_eq!(format_price(dec!(5278.30), "$"), "$5278.3");
    }

    #[test]
    fn format_price_small() {
        assert_eq!(format_price(dec!(1.2345), "$"), "$1.23");
        assert_eq!(format_price(dec!(42.99), "$"), "$42.99");
    }

    #[test]
    fn format_price_very_small() {
        assert_eq!(format_price(dec!(0.5678), "$"), "$0.5678");
        assert_eq!(format_price(dec!(0.00012345), "$"), "$0.0001");
    }

    #[test]
    fn format_price_euro() {
        assert_eq!(format_price(dec!(189.50), "€"), "€189.5");
        assert_eq!(format_price(dec!(42.99), "€"), "€42.99");
    }

    #[test]
    fn yahoo_crypto_symbol_appends() {
        assert_eq!(yahoo_crypto_symbol("BTC"), "BTC-USD");
        assert_eq!(yahoo_crypto_symbol("eth"), "ETH-USD");
    }

    #[test]
    fn yahoo_crypto_symbol_no_double() {
        assert_eq!(yahoo_crypto_symbol("BTC-USD"), "BTC-USD");
    }

    #[test]
    fn collect_symbols_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let symbols = collect_symbols(&conn, &config).unwrap();
        assert!(symbols.is_empty());
    }

    #[test]
    fn collect_symbols_from_transactions() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();
        let symbols = collect_symbols(&conn, &config).unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].0, "AAPL");
    }

    #[test]
    fn collect_symbols_deduplicates_watchlist_and_portfolio() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        use crate::db::transactions::insert_transaction;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(50000),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();
        add_to_watchlist(&conn, "ETH", AssetCategory::Crypto).unwrap();

        let symbols = collect_symbols(&conn, &config).unwrap();
        assert_eq!(symbols.len(), 2);
        let sym_names: Vec<_> = symbols.iter().map(|(s, _)| s.as_str()).collect();
        assert!(sym_names.contains(&"BTC"));
        assert!(sym_names.contains(&"ETH"));
    }

    #[test]
    fn collect_symbols_percentage_mode() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };
        use crate::db::allocations::insert_allocation;
        insert_allocation(&conn, "GC=F", AssetCategory::Commodity, dec!(50)).unwrap();
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(50)).unwrap();

        let symbols = collect_symbols(&conn, &config).unwrap();
        assert_eq!(symbols.len(), 2);
    }
}
