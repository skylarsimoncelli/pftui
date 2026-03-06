use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::alerts::engine;
use crate::config::{Config, PortfolioMode};
use crate::data::{bls, calendar, comex, cot, onchain, predictions, rss, sentiment, worldbank};
use crate::db::allocations::{get_unique_allocation_symbols, list_allocations};
use crate::db::{bls_cache, calendar_cache, comex_cache, cot_cache, news_cache};
use crate::db::{onchain_cache, predictions_cache, sentiment_cache, worldbank_cache};
use crate::db::price_cache::{get_all_cached_prices, upsert_price};
use crate::db::snapshots::{upsert_portfolio_snapshot, upsert_position_snapshot};
use crate::db::transactions::{get_unique_symbols, list_transactions};
use crate::db::watchlist::get_watchlist_symbols;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations};
use crate::models::price::PriceQuote;
use crate::notify;
use crate::price::{coingecko, yahoo};
use crate::tui::views::economy;

/// Delay between sequential Yahoo Finance API requests to avoid rate limiting.
const YAHOO_RATE_LIMIT_DELAY: Duration = Duration::from_millis(100);

/// Freshness thresholds in seconds
const PRICE_FRESHNESS_SECS: i64 = 15 * 60; // 15 minutes
const NEWS_FRESHNESS_SECS: i64 = 10 * 60; // 10 minutes
const PREDICTIONS_FRESHNESS_SECS: i64 = 60 * 60; // 1 hour
const SENTIMENT_FRESHNESS_SECS: i64 = 60 * 60; // 1 hour
const CALENDAR_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours
const COT_FRESHNESS_SECS: i64 = 7 * 24 * 60 * 60; // 1 week
const COMEX_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours
const BLS_FRESHNESS_DAYS: i64 = 30; // 1 month
const WORLDBANK_FRESHNESS_DAYS: i64 = 30; // 30 days

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

    // Macro/economy symbols (DXY, VIX, oil, copper, yields, FX, etc.)
    for item in economy::economy_symbols() {
        let cat = economy::category_for_group(item.group);
        seen.entry(item.yahoo_symbol).or_insert(cat);
    }

    Ok(seen.into_iter().collect())
}

/// Check if prices need refreshing
fn prices_need_refresh(conn: &Connection) -> Result<bool> {
    let prices = get_all_cached_prices(conn)?;
    if prices.is_empty() {
        return Ok(true);
    }
    
    // Check if any price is older than threshold
    let now = chrono::Utc::now();
    for quote in prices {
        if let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(&quote.fetched_at) {
            let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
            if age.num_seconds() > PRICE_FRESHNESS_SECS {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Check if news needs refreshing
fn news_needs_refresh(conn: &Connection) -> Result<bool> {
    // Check most recent news entry
    let news = news_cache::get_latest_news(conn, 1, None, None, None, None)?;
    if news.is_empty() {
        return Ok(true);
    }
    
    let now = chrono::Utc::now();
    if let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(&news[0].fetched_at) {
        let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
        return Ok(age.num_seconds() > NEWS_FRESHNESS_SECS);
    }
    Ok(true)
}

/// Check if predictions need refreshing
fn predictions_need_refresh(conn: &Connection) -> Result<bool> {
    match predictions_cache::get_last_update(conn)? {
        None => Ok(true),
        Some(ts) => {
            let now = chrono::Utc::now().timestamp();
            Ok((now - ts) > PREDICTIONS_FRESHNESS_SECS)
        }
    }
}

/// Check if sentiment needs refreshing
fn sentiment_needs_refresh(conn: &Connection) -> Result<bool> {
    // Check both crypto and traditional FNG
    let crypto = sentiment_cache::get_latest(conn, "crypto_fng")?;
    let trad = sentiment_cache::get_latest(conn, "traditional_fng")?;
    
    if crypto.is_none() || trad.is_none() {
        return Ok(true);
    }
    
    let now = chrono::Utc::now().timestamp();
    if let Some(c) = crypto {
        if (now - c.timestamp) > SENTIMENT_FRESHNESS_SECS {
            return Ok(true);
        }
    }
    if let Some(t) = trad {
        if (now - t.timestamp) > SENTIMENT_FRESHNESS_SECS {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if calendar needs refreshing
fn calendar_needs_refresh(conn: &Connection) -> Result<bool> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let events = calendar_cache::get_upcoming_events(conn, &today, 10)?;
    if events.is_empty() {
        return Ok(true);
    }
    
    // Check fetched_at timestamp of the first event
    let now = chrono::Utc::now();
    if let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(&events[0].fetched_at) {
        let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
        return Ok(age.num_seconds() > CALENDAR_FRESHNESS_SECS);
    }
    Ok(true)
}

/// Check if COT needs refreshing
fn cot_needs_refresh(conn: &Connection) -> Result<bool> {
    let reports = cot_cache::get_all_latest(conn)?;
    if reports.is_empty() {
        return Ok(true);
    }
    
    let now = chrono::Utc::now();
    for report in reports {
        if let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(&report.fetched_at) {
            let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
            if age.num_seconds() > COT_FRESHNESS_SECS {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Check if COMEX needs refreshing
fn comex_needs_refresh(conn: &Connection) -> Result<bool> {
    // Check common metals
    for symbol in &["GC", "SI", "HG", "PL"] {
        if !comex_cache::has_fresh_data(conn, symbol)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if BLS needs refreshing
fn bls_needs_refresh(conn: &Connection) -> Result<bool> {
    // Check a few key series
    for series in &["CUUR0000SA0", "CUSR0000SA0", "LNS14000000"] {
        if !bls_cache::is_cache_fresh(conn, series, BLS_FRESHNESS_DAYS)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if World Bank needs refreshing
fn worldbank_needs_refresh(conn: &Connection) -> Result<bool> {
    worldbank_cache::needs_refresh(conn)
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

pub fn run(conn: &Connection, config: &Config, notify: bool) -> Result<()> {
    println!("Refreshing all data sources...\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    // 1. Prices
    if prices_need_refresh(conn)? {
        let symbols = collect_symbols(conn, config)?;
        if !symbols.is_empty() {
            let non_cash: Vec<_> = symbols
                .iter()
                .filter(|(_, cat)| *cat != AssetCategory::Cash)
                .collect();
            
            let (quotes, _errors) = rt.block_on(fetch_all_prices(&symbols, config));
            
            for quote in &quotes {
                if let Err(e) = upsert_price(conn, quote) {
                    eprintln!("Failed to cache {}: {}", quote.symbol, e);
                }
            }
            
            let fetched: Vec<_> = quotes
                .iter()
                .filter(|q| q.source != "static")
                .collect();
            
            println!("✓ Prices ({} symbols)", fetched.len());
        } else {
            println!("⊘ Prices (no symbols)");
        }
    } else {
        println!("⊘ Prices (fresh, skipping)");
    }

    // 2. Predictions (Polymarket)
    if predictions_need_refresh(conn)? {
        match rt.block_on(predictions::fetch_polymarket_predictions()) {
            Ok(markets) => {
                predictions_cache::upsert_predictions(conn, &markets)?;
                println!("✓ Predictions ({} markets)", markets.len());
            }
            Err(e) => {
                println!("✗ Predictions (failed: {})", e);
            }
        }
    } else {
        println!("⊘ Predictions (fresh, skipping)");
    }

    // 3. News (RSS feeds)
    if news_needs_refresh(conn)? {
        let feeds = rss::default_feeds();
        let items = rt.block_on(rss::fetch_all_feeds(&feeds));
        
        let mut inserted = 0;
        for item in &items {
            let category_str = match item.category {
                rss::NewsCategory::Macro => "macro",
                rss::NewsCategory::Crypto => "crypto",
                rss::NewsCategory::Commodities => "commodities",
                rss::NewsCategory::Geopolitics => "geopolitics",
                rss::NewsCategory::Markets => "markets",
            };
            
            if news_cache::insert_news(
                conn,
                &item.title,
                &item.url,
                &item.source,
                category_str,
                item.published_at,
            ).is_ok() {
                inserted += 1;
            }
        }
        println!("✓ News ({} articles)", inserted);
    } else {
        println!("⊘ News (fresh, skipping)");
    }

    // 4. COT (CFTC)
    if cot_needs_refresh(conn)? {
        let mut total = 0;
        
        // Key commodities
        for (_name, code) in &[
            ("Gold", "088691"),
            ("Silver", "084691"),
            ("Copper", "085692"),
            ("Crude Oil", "067651"),
            ("S&P 500", "13874+"),
        ] {
            match cot::fetch_latest_report(code) {
                Ok(report) => {
                    let entry = crate::db::cot_cache::CotCacheEntry {
                        cftc_code: report.cftc_code.clone(),
                        report_date: report.report_date.clone(),
                        open_interest: report.open_interest,
                        managed_money_long: report.managed_money_long,
                        managed_money_short: report.managed_money_short,
                        managed_money_net: report.managed_money_net,
                        commercial_long: report.commercial_long,
                        commercial_short: report.commercial_short,
                        commercial_net: report.commercial_net,
                        fetched_at: chrono::Utc::now().to_rfc3339(),
                    };
                    cot_cache::upsert_report(conn, &entry)?;
                    total += 1;
                }
                Err(_) => {
                    // Continue on error
                }
            }
        }
        
        if total > 0 {
            println!("✓ COT ({} reports)", total);
        } else {
            println!("✗ COT (all failed)");
        }
    } else {
        println!("⊘ COT (fresh, skipping)");
    }

    // 5. Sentiment (Fear & Greed)
    if sentiment_needs_refresh(conn)? {
        let mut count = 0;
        
        if let Ok(crypto) = sentiment::fetch_crypto_fng() {
            let reading = crate::db::sentiment_cache::SentimentReading {
                index_type: "crypto_fng".to_string(),
                value: crypto.value,
                classification: crypto.classification.clone(),
                timestamp: crypto.timestamp,
                fetched_at: chrono::Utc::now().to_rfc3339(),
            };
            sentiment_cache::upsert_reading(conn, &reading)?;
            count += 1;
        }
        
        if let Ok(trad) = sentiment::fetch_traditional_fng() {
            let reading = crate::db::sentiment_cache::SentimentReading {
                index_type: "traditional_fng".to_string(),
                value: trad.value,
                classification: trad.classification.clone(),
                timestamp: trad.timestamp,
                fetched_at: chrono::Utc::now().to_rfc3339(),
            };
            sentiment_cache::upsert_reading(conn, &reading)?;
            count += 1;
        }
        
        println!("✓ Sentiment ({} indices)", count);
    } else {
        println!("⊘ Sentiment (fresh, skipping)");
    }

    // 6. Calendar (TradingEconomics)
    if calendar_needs_refresh(conn)? {
        match calendar::fetch_events(7) {
            Ok(events) => {
                for event in &events {
                    let _ = calendar_cache::upsert_event(
                        conn,
                        &event.date,
                        &event.name,
                        &event.impact,
                        event.previous.as_deref(),
                        event.forecast.as_deref(),
                        &event.event_type,
                        event.symbol.as_deref(),
                    );
                }
                println!("✓ Calendar ({} events)", events.len());
            }
            Err(e) => {
                println!("✗ Calendar (failed: {})", e);
            }
        }
    } else {
        println!("⊘ Calendar (fresh, skipping)");
    }

    // 7. BLS
    if bls_needs_refresh(conn)? {
        match rt.block_on(bls::fetch_all_key_series()) {
            Ok(data) => {
                bls_cache::upsert_bls_data(conn, &data)?;
                println!("✓ BLS ({} series)", data.len());
            }
            Err(e) => {
                println!("✗ BLS (failed: {})", e);
            }
        }
    } else {
        println!("⊘ BLS (fresh, skipping)");
    }

    // 8. World Bank
    if worldbank_needs_refresh(conn)? {
        match rt.block_on(worldbank::fetch_all_indicators()) {
            Ok(data) => {
                worldbank_cache::upsert_worldbank_data(conn, &data)?;
                println!("✓ World Bank ({} indicators)", data.len());
            }
            Err(e) => {
                println!("✗ World Bank (failed: {})", e);
            }
        }
    } else {
        println!("⊘ World Bank (fresh, skipping)");
    }

    // 9. COMEX
    if comex_needs_refresh(conn)? {
        let results = comex::fetch_all_inventories();
        let mut count = 0;
        
        for (symbol, result) in results {
            if let Ok(inv) = result {
                let entry = crate::db::comex_cache::ComexCacheEntry {
                    symbol: symbol.clone(),
                    date: inv.date.clone(),
                    registered: inv.registered,
                    eligible: inv.eligible,
                    total: inv.total,
                    reg_ratio: inv.reg_ratio,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                };
                if comex_cache::upsert_inventory(conn, &entry).is_ok() {
                    count += 1;
                }
            }
        }
        
        if count > 0 {
            println!("✓ COMEX ({} metals)", count);
        } else {
            println!("✗ COMEX (all failed)");
        }
    } else {
        println!("⊘ COMEX (fresh, skipping)");
    }

    // 10. On-chain (Blockchair)
    // On-chain data is always fetched since it's diverse and doesn't have a simple freshness check
    // We'll fetch but not fail the whole command if it errors
    match onchain::fetch_network_metrics() {
        Ok(metrics) => {
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let metric = crate::db::onchain_cache::OnchainMetric {
                metric: "network".to_string(),
                date: today,
                value: metrics.hash_rate.to_string(),
                metadata: Some(serde_json::json!({
                    "difficulty": metrics.difficulty,
                    "blocks_24h": metrics.blocks_24h,
                    "mempool_size": metrics.mempool_size,
                    "avg_fee_sat_b": metrics.avg_fee_sat_b,
                }).to_string()),
                fetched_at: chrono::Utc::now().to_rfc3339(),
            };
            let _ = onchain_cache::upsert_metric(conn, &metric);
            println!("✓ On-chain (network metrics)");
        }
        Err(e) => {
            println!("✗ On-chain (failed: {})", e);
        }
    }

    // Store daily portfolio snapshot
    if let Err(e) = store_portfolio_snapshot(conn, config) {
        eprintln!("\nWarning: failed to store portfolio snapshot: {}", e);
    }

    // Check for newly triggered alerts
    match engine::check_alerts(conn) {
        Ok(results) => {
            let newly_triggered = engine::get_newly_triggered(&results);
            if !newly_triggered.is_empty() {
                println!("\n🔔 Alerts Triggered:");
                for result in &newly_triggered {
                    let dir_emoji = match result.rule.direction {
                        crate::alerts::AlertDirection::Above => "↑",
                        crate::alerts::AlertDirection::Below => "↓",
                    };
                    let current_str = result
                        .current_value
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "N/A".to_string());
                    println!(
                        "  {} {} {} {} (current: {})",
                        dir_emoji,
                        result.rule.symbol,
                        result.rule.kind,
                        result.rule.threshold,
                        current_str
                    );

                    // Send OS notification if --notify flag is set
                    if notify {
                        let title = format!("pftui Alert: {}", result.rule.symbol);
                        let body = format!(
                            "{} {} {} (current: {})",
                            result.rule.kind,
                            dir_emoji,
                            result.rule.threshold,
                            current_str
                        );
                        if let Err(e) = notify::send_notification(&title, &body) {
                            eprintln!("  Warning: failed to send notification: {}", e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("\nWarning: failed to check alerts: {}", e);
        }
    }

    println!("\nRefresh complete.");
    Ok(())
}

/// Compute current positions and store a daily portfolio snapshot.
fn store_portfolio_snapshot(conn: &Connection, config: &Config) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let transactions = list_transactions(conn)?;
            if transactions.is_empty() {
                return Ok(());
            }
            compute_positions(&transactions, &prices)
        }
        PortfolioMode::Percentage => {
            let allocations = list_allocations(conn)?;
            if allocations.is_empty() {
                return Ok(());
            }
            compute_positions_from_allocations(&allocations, &prices)
        }
    };

    if positions.is_empty() {
        return Ok(());
    }

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut total_value = dec!(0);
    let mut cash_value = dec!(0);
    let mut invested_value = dec!(0);

    for pos in &positions {
        let value = pos.current_value.unwrap_or(dec!(0));
        total_value += value;
        if pos.category == AssetCategory::Cash {
            cash_value += value;
        } else {
            invested_value += value;
        }
    }

    // Store portfolio-level snapshot
    upsert_portfolio_snapshot(conn, &today, total_value, cash_value, invested_value)?;

    // Store per-position snapshots
    for pos in &positions {
        let price = pos.current_price.unwrap_or(dec!(0));
        let value = pos.current_value.unwrap_or(dec!(0));
        upsert_position_snapshot(conn, &today, &pos.symbol, pos.quantity, price, value)?;
    }

    let snap_count = positions.len();
    println!(
        "Snapshot stored: {} ({} position{}).",
        today,
        snap_count,
        if snap_count == 1 { "" } else { "s" },
    );

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
}
