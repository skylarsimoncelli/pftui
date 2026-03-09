use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::alerts::engine;
use crate::config::{Config, PortfolioMode};
use crate::data::{
    bls, brave, calendar, comex, cot, economic, fx, onchain, predictions, rss, sentiment, worldbank,
};
use crate::db::allocations::{get_unique_allocation_symbols_backend, list_allocations_backend};
use crate::db::backend::BackendConnection;
use crate::db::economic_data as economic_data_db;
use crate::db::price_cache::{
    get_all_cached_prices_backend, get_cached_price_backend, upsert_price_backend,
};
use crate::db::price_history::get_price_at_date_backend;
use crate::db::snapshots::{upsert_portfolio_snapshot_backend, upsert_position_snapshot_backend};
use crate::db::timeframe_signals;
use crate::db::transactions::{get_unique_symbols_backend, list_transactions_backend};
use crate::db::watchlist::get_watchlist_symbols_backend;
use crate::db::{bls_cache, calendar_cache, comex_cache, cot_cache, fx_cache, news_cache};
use crate::db::{onchain_cache, predictions_cache, sentiment_cache, worldbank_cache};
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
const BLS_FRESHNESS_DAYS: i64 = 30; // 1 month
const BRAVE_NEWS_QUERY_LIMIT: usize = 12;

/// Collect all symbols that need pricing: portfolio positions + watchlist.
fn collect_symbols(
    backend: &BackendConnection,
    config: &Config,
) -> Result<Vec<(String, AssetCategory)>> {
    let mut seen = HashMap::new();

    // Portfolio symbols (transactions or allocations depending on mode)
    let portfolio_symbols = match config.portfolio_mode {
        PortfolioMode::Full => get_unique_symbols_backend(backend)?,
        PortfolioMode::Percentage => get_unique_allocation_symbols_backend(backend)?,
    };
    for (sym, cat) in portfolio_symbols {
        seen.entry(sym).or_insert(cat);
    }

    // Watchlist symbols
    let watchlist_symbols = get_watchlist_symbols_backend(backend)?;
    for (sym, cat) in watchlist_symbols {
        seen.entry(sym).or_insert(cat);
    }

    // Macro/economy symbols (DXY, VIX, oil, copper, yields, FX, etc.)
    for item in economy::economy_symbols() {
        let cat = economy::category_for_group(item.group);
        seen.entry(item.yahoo_symbol).or_insert(cat);
    }

    // Sector ETFs (XLE, XLK, etc.) — needed for `pftui sector` command
    for (symbol, _name) in crate::commands::sector::SECTOR_ETFS {
        seen.entry(symbol.to_string()).or_insert(AssetCategory::Equity);
    }

    Ok(seen.into_iter().collect())
}

/// Check if prices need refreshing
fn prices_need_refresh(backend: &BackendConnection) -> Result<bool> {
    let prices = get_all_cached_prices_backend(backend)?;
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
fn news_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    // Check most recent news entry
    let news = news_cache::get_latest_news_backend(backend, 1, None, None, None, None)?;
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

fn compute_daily_change_pct(
    backend: &BackendConnection,
    symbol: &str,
    current: Decimal,
) -> Option<Decimal> {
    let today = chrono::Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let prev = get_price_at_date_backend(backend, symbol, &yesterday_str).ok()??;
    if prev == dec!(0) {
        return None;
    }
    Some(((current - prev) / prev * dec!(100)).abs())
}

fn build_brave_news_queries(
    backend: &BackendConnection,
    config: &Config,
) -> Result<Vec<String>> {
    let mut queries = if config.brave_news_queries.is_empty() {
        vec![
            "stock market today".to_string(),
            "federal reserve interest rates monetary policy".to_string(),
            "bitcoin cryptocurrency regulation".to_string(),
            "gold silver precious metals price".to_string(),
            "oil OPEC energy crude".to_string(),
            "geopolitics international trade war sanctions".to_string(),
        ]
    } else {
        config.brave_news_queries.clone()
    };

    let mut symbols = Vec::new();
    let mut seen = HashSet::new();

    for (sym, cat) in match config.portfolio_mode {
        PortfolioMode::Full => get_unique_symbols_backend(backend)?,
        PortfolioMode::Percentage => get_unique_allocation_symbols_backend(backend)?,
    } {
        if cat != AssetCategory::Cash && seen.insert(sym.clone()) {
            symbols.push(sym);
        }
    }
    for (sym, cat) in get_watchlist_symbols_backend(backend)? {
        if cat != AssetCategory::Cash && seen.insert(sym.clone()) {
            symbols.push(sym);
        }
    }

    let price_map: HashMap<String, Decimal> = get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    for sym in symbols {
        if let Some(current) = price_map.get(&sym).copied() {
            if let Some(abs_change) = compute_daily_change_pct(backend, &sym, current) {
                if abs_change >= dec!(3) {
                    queries.push(format!("{} stock news", sym));
                }
            }
        }
    }

    queries.truncate(BRAVE_NEWS_QUERY_LIMIT);
    Ok(queries)
}

/// Check if predictions need refreshing
fn predictions_need_refresh(backend: &BackendConnection) -> Result<bool> {
    match predictions_cache::get_last_update_backend(backend)? {
        None => Ok(true),
        Some(ts) => {
            let now = chrono::Utc::now().timestamp();
            Ok((now - ts) > PREDICTIONS_FRESHNESS_SECS)
        }
    }
}

/// Check if sentiment needs refreshing
fn sentiment_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    // Check both crypto and traditional FNG
    let crypto = sentiment_cache::get_latest_backend(backend, "crypto_fng")?;
    let trad = sentiment_cache::get_latest_backend(backend, "traditional_fng")?;

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
fn calendar_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let events = calendar_cache::get_upcoming_events_backend(backend, &today, 10)?;
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
fn cot_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    let reports = cot_cache::get_all_latest_backend(backend)?;
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
fn comex_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    // Check common metals
    for symbol in &["GC", "SI", "HG", "PL"] {
        if !comex_cache::has_fresh_data_backend(backend, symbol)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if BLS needs refreshing
fn bls_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    // Check a few key series
    for series in &["CUUR0000SA0", "CUSR0000SA0", "LNS14000000"] {
        if !bls_cache::is_cache_fresh_backend(backend, series, BLS_FRESHNESS_DAYS)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if World Bank needs refreshing
fn worldbank_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    worldbank_cache::needs_refresh_backend(backend)
}

/// Format a price for display: compact representation.
#[cfg(test)]
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
                    pre_market_price: None,
                    post_market_price: None,
                    post_market_change_percent: None,
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
                errors.push(format!(
                    "CoinGecko batch failed: {}, falling back to Yahoo",
                    e
                ));
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

struct RefreshLock {
    path: std::path::PathBuf,
}

impl RefreshLock {
    fn acquire() -> Result<Self> {
        let lock_path = refresh_lock_path();

        // Ensure parent directory exists
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Check if lock exists and is recent
        if lock_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&lock_path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(elapsed) = modified.elapsed() {
                        if elapsed.as_secs() < 300 {
                            // Lock is less than 5 minutes old
                            anyhow::bail!("Refresh already in progress");
                        }
                    }
                }
            }
            // Stale lock, remove it
            let _ = std::fs::remove_file(&lock_path);
        }

        // Create lock file
        std::fs::write(&lock_path, "")?;

        Ok(RefreshLock { path: lock_path })
    }
}

fn refresh_lock_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("PFTUI_REFRESH_LOCK_DIR") {
        return std::path::Path::new(&path).join("refresh.lock");
    }
    std::path::Path::new(&std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".local/share/pftui/refresh.lock")
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
) -> Result<()> {
    let _lock = RefreshLock::acquire()?;

    println!("Refreshing all data sources...\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    // 1. FX Rates
    match rt.block_on(fx::fetch_all_fx_rates()) {
        Ok(rates) => {
            for (currency, rate) in &rates {
                if let Err(e) = fx_cache::upsert_fx_rate_backend(backend, currency, *rate) {
                    eprintln!("Failed to cache FX rate for {}: {}", currency, e);
                }
            }
            println!("✓ FX rates ({} currencies)", rates.len());
        }
        Err(e) => {
            println!("✗ FX rates (failed: {})", e);
        }
    }

    // 2. Prices
    if prices_need_refresh(backend)? {
        let symbols = collect_symbols(backend, config)?;
        if !symbols.is_empty() {
            let _non_cash: Vec<_> = symbols
                .iter()
                .filter(|(_, cat)| *cat != AssetCategory::Cash)
                .collect();

            let (quotes, _errors) = rt.block_on(fetch_all_prices(&symbols, config));

            for quote in &quotes {
                if let Err(e) = upsert_price_backend(backend, quote) {
                    eprintln!("Failed to cache {}: {}", quote.symbol, e);
                }
            }

            let fetched: Vec<_> = quotes.iter().filter(|q| q.source != "static").collect();

            println!("✓ Prices ({} symbols)", fetched.len());
        } else {
            println!("⊘ Prices (no symbols)");
        }
    } else {
        println!("⊘ Prices (fresh, skipping)");
    }

    // 3. Predictions (Polymarket)
    // 3a. Correlation snapshots + regime classification
    match crate::commands::correlations::compute_and_store_default_snapshots_backend(backend) {
        Ok(n) if n > 0 => println!("✓ Correlation snapshots ({} rows)", n),
        Ok(_) => println!("⊘ Correlation snapshots (insufficient history)"),
        Err(e) => println!("✗ Correlation snapshots (failed: {})", e),
    }

    match crate::commands::regime::classify_and_store_if_needed(backend) {
        Ok(true) => println!("✓ Regime classification (stored)"),
        Ok(false) => println!("⊘ Regime classification (unchanged today)"),
        Err(e) => println!("✗ Regime classification (failed: {})", e),
    }

    // 3. Predictions (Polymarket)
    if predictions_need_refresh(backend)? {
        match rt.block_on(predictions::fetch_polymarket_predictions()) {
            Ok(markets) => {
                predictions_cache::upsert_predictions_backend(backend, &markets)?;
                println!("✓ Predictions ({} markets)", markets.len());
            }
            Err(e) => {
                println!("✗ Predictions (failed: {})", e);
            }
        }
    } else {
        println!("⊘ Predictions (fresh, skipping)");
    }

    // 4. News (Brave primary when configured, RSS supplements)
    if news_needs_refresh(backend)? {
        let mut inserted = 0usize;
        let mut brave_inserted = 0usize;
        let brave_key = config
            .brave_api_key
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();

        if !brave_key.is_empty() {
            let queries = build_brave_news_queries(backend, config)?;
            for query in &queries {
                match rt.block_on(brave::brave_news_search(&brave_key, query, Some("pd"), 5)) {
                    Ok(results) => {
                        for item in &results {
                            let source = item.source.as_deref().unwrap_or("Brave");
                            if news_cache::insert_news_with_source_type_backend(
                                backend,
                                &item.title,
                                &item.url,
                                source,
                                "brave",
                                None,
                                "markets",
                                chrono::Utc::now().timestamp(),
                                Some(&item.description),
                                &item.extra_snippets,
                            )
                            .is_ok()
                            {
                                inserted += 1;
                                brave_inserted += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Brave news query failed ({}): {}", query, e);
                    }
                }
            }
        }

        let feeds = rss::default_feeds();
        let items = rt.block_on(rss::fetch_all_feeds(&feeds));
        for item in &items {
            let category_str = match item.category {
                rss::NewsCategory::Macro => "macro",
                rss::NewsCategory::Crypto => "crypto",
                rss::NewsCategory::Commodities => "commodities",
                rss::NewsCategory::Geopolitics => "geopolitics",
                rss::NewsCategory::Markets => "markets",
            };

            if news_cache::insert_news_with_source_type_backend(
                backend,
                &item.title,
                &item.url,
                &item.source,
                "rss",
                None,
                category_str,
                item.published_at,
                None,
                &[],
            )
            .is_ok()
            {
                inserted += 1;
            }
        }
        if brave_inserted > 0 {
            println!("✓ News ({} articles via Brave + RSS)", inserted);
        } else {
            println!("✓ News ({} articles via RSS)", inserted);
        }
    } else {
        println!("⊘ News (fresh, skipping)");
    }

    // 5. COT (CFTC)
    if cot_needs_refresh(backend)? {
        let mut total = 0;

        for contract in cot::COT_CONTRACTS {
            match cot::fetch_latest_report(contract.cftc_code) {
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
                    cot_cache::upsert_report_backend(backend, &entry)?;
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

    // 6. Sentiment (Fear & Greed)
    if sentiment_needs_refresh(backend)? {
        let mut count = 0;

        if let Ok(crypto) = sentiment::fetch_crypto_fng() {
            let reading = crate::db::sentiment_cache::SentimentReading {
                index_type: "crypto_fng".to_string(),
                value: crypto.value,
                classification: crypto.classification.clone(),
                timestamp: crypto.timestamp,
                fetched_at: chrono::Utc::now().to_rfc3339(),
            };
            sentiment_cache::upsert_reading_backend(backend, &reading)?;
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
            sentiment_cache::upsert_reading_backend(backend, &reading)?;
            count += 1;
        }

        println!("✓ Sentiment ({} indices)", count);
    } else {
        println!("⊘ Sentiment (fresh, skipping)");
    }

    // 7. Calendar (TradingEconomics)
    if calendar_needs_refresh(backend)? {
        match calendar::fetch_events(7) {
            Ok(mut events) => {
                let brave_key = config
                    .brave_api_key
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !brave_key.is_empty() {
                    let _ = rt.block_on(calendar::enrich_with_brave(&mut events, &brave_key));
                }
                for event in &events {
                    let _ = calendar_cache::upsert_event_backend(
                        backend,
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

    // 9. Economy indicators (Brave primary, BLS fallback)
    {
        let brave_key = config
            .brave_api_key
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let mut used_brave = false;
        let readings = if !brave_key.is_empty() {
            match rt.block_on(economic::fetch_via_brave(&brave_key)) {
                Ok(v) if !v.is_empty() => {
                    used_brave = true;
                    Ok(v)
                }
                Ok(_) => rt.block_on(economic::fetch_bls_fallback()),
                Err(_) => rt.block_on(economic::fetch_bls_fallback()),
            }
        } else {
            rt.block_on(economic::fetch_bls_fallback())
        };

        match readings {
            Ok(items) => {
                let now = chrono::Utc::now().to_rfc3339();
                for item in &items {
                    let entry = economic_data_db::EconomicDataEntry {
                        indicator: item.indicator.clone(),
                        value: item.value,
                        previous: item.previous,
                        change: item.change,
                        source_url: item.source_url.clone(),
                        fetched_at: now.clone(),
                    };
                    let _ = economic_data_db::upsert_entry_backend(backend, &entry);
                }
                if used_brave {
                    println!("✓ Economy ({} indicators via Brave)", items.len());
                } else {
                    println!("✓ Economy ({} indicators via BLS fallback)", items.len());
                }
            }
            Err(e) => println!("✗ Economy (failed: {})", e),
        }
    }

    // 9. BLS
    if bls_needs_refresh(backend)? {
        match rt.block_on(bls::fetch_all_key_series()) {
            Ok(data) => {
                bls_cache::upsert_bls_data_backend(backend, &data)?;
                println!("✓ BLS ({} series)", data.len());
            }
            Err(e) => {
                println!("✗ BLS (failed: {})", e);
            }
        }
    } else {
        println!("⊘ BLS (fresh, skipping)");
    }

    // 10. World Bank
    if worldbank_needs_refresh(backend)? {
        match rt.block_on(worldbank::fetch_all_indicators()) {
            Ok(data) => {
                worldbank_cache::upsert_worldbank_data_backend(backend, &data)?;
                println!("✓ World Bank ({} indicators)", data.len());
            }
            Err(e) => {
                println!("✗ World Bank (failed: {})", e);
            }
        }
    } else {
        println!("⊘ World Bank (fresh, skipping)");
    }

    // 11. COMEX
    if comex_needs_refresh(backend)? {
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
                if comex_cache::upsert_inventory_backend(backend, &entry).is_ok() {
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

    // 13. On-chain (network + ETF flows)
    let mut onchain_ok_parts = Vec::new();
    let mut onchain_errors = Vec::new();

    match onchain::fetch_network_metrics() {
        Ok(metrics) => {
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let metric = crate::db::onchain_cache::OnchainMetric {
                metric: "network".to_string(),
                date: today,
                value: metrics.hash_rate.to_string(),
                metadata: Some(
                    serde_json::json!({
                        "difficulty": metrics.difficulty,
                        "blocks_24h": metrics.blocks_24h,
                        "mempool_size": metrics.mempool_size,
                        "avg_fee_sat_b": metrics.avg_fee_sat_b,
                    })
                    .to_string(),
                ),
                fetched_at: chrono::Utc::now().to_rfc3339(),
            };
            let _ = onchain_cache::upsert_metric_backend(backend, &metric);
            onchain_ok_parts.push("network");
        }
        Err(e) => onchain_errors.push(format!("network: {}", e)),
    }

    match onchain::fetch_etf_flows() {
        Ok(flows) => {
            let fetched_at = chrono::Utc::now().to_rfc3339();
            for flow in &flows {
                let metric = crate::db::onchain_cache::OnchainMetric {
                    metric: format!("etf_flow_{}", flow.fund),
                    date: flow.date.clone(),
                    value: flow.net_flow_btc.to_string(),
                    metadata: Some(
                        serde_json::json!({
                            "fund": flow.fund,
                            "net_flow_usd": flow.net_flow_usd,
                    })
                    .to_string(),
                ),
                fetched_at: fetched_at.clone(),
            };
                let _ = onchain_cache::upsert_metric_backend(backend, &metric);
            }
            onchain_ok_parts.push("etf flows");
        }
        Err(e) => onchain_errors.push(format!("etf flows: {}", e)),
    }

    if !onchain_ok_parts.is_empty() {
        println!("✓ On-chain ({})", onchain_ok_parts.join(" + "));
    } else {
        println!("✗ On-chain (failed: {})", onchain_errors.join("; "));
    }

    // Store daily portfolio snapshot
    if let Err(e) = store_portfolio_snapshot(backend, config) {
        eprintln!("\nWarning: failed to store portfolio snapshot: {}", e);
    }
    if let Err(e) = detect_timeframe_signals(backend) {
        eprintln!(
            "\nWarning: failed to compute cross-timeframe signals: {}",
            e
        );
    }

    // Check for newly triggered alerts
    match engine::check_alerts_backend_only(backend) {
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
                            result.rule.kind, dir_emoji, result.rule.threshold, current_str
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

fn classify_regime_bias(backend: &BackendConnection) -> Option<String> {
    let vix = get_cached_price_backend(backend, "^VIX", "USD")
        .ok()
        .flatten()
        .map(|q| q.price);
    let dxy = get_cached_price_backend(backend, "DX-Y.NYB", "USD")
        .ok()
        .flatten()
        .map(|q| q.price);
    let spy = get_cached_price_backend(backend, "SPY", "USD")
        .ok()
        .flatten()
        .map(|q| q.price);
    let gold = get_cached_price_backend(backend, "GC=F", "USD")
        .ok()
        .flatten()
        .map(|q| q.price);

    let mut risk_off = 0;
    let mut risk_on = 0;

    if let Some(v) = vix {
        if v > dec!(25) {
            risk_off += 1;
        } else if v < dec!(20) {
            risk_on += 1;
        }
    }
    if let Some(d) = dxy {
        if d > dec!(102) {
            risk_off += 1;
        } else if d < dec!(99) {
            risk_on += 1;
        }
    }
    if let (Some(s), Some(g)) = (spy, gold) {
        if g > dec!(0) && s > dec!(0) {
            let ratio = g / s;
            if ratio > dec!(1.0) {
                risk_off += 1;
            } else {
                risk_on += 1;
            }
        }
    }

    if risk_off >= 2 {
        Some("bear".to_string())
    } else if risk_on >= 2 {
        Some("bull".to_string())
    } else {
        Some("neutral".to_string())
    }
}

fn top_scenario_bias(backend: &BackendConnection) -> Option<String> {
    let top = crate::db::scenarios::list_scenarios_backend(backend, Some("active"))
        .ok()?
        .into_iter()
        .next()?;
    let text = format!(
        "{} {} {}",
        top.name,
        top.description.unwrap_or_default(),
        top.asset_impact.unwrap_or_default()
    )
    .to_lowercase();
    if text.contains("war")
        || text.contains("stagflation")
        || text.contains("recession")
        || text.contains("crisis")
        || text.contains("risk-off")
    {
        Some("bear".to_string())
    } else if text.contains("growth") || text.contains("soft landing") || text.contains("risk-on") {
        Some("bull".to_string())
    } else {
        Some("neutral".to_string())
    }
}

fn trend_layer_bias(backend: &BackendConnection) -> Option<String> {
    let mut bull = 0usize;
    let mut bear = 0usize;

    let rows = crate::db::trends::list_trends_backend(backend, Some("active"), None).ok()?;
    for row in rows.into_iter().take(25) {
        let direction = row.direction.to_lowercase();
        let impact = row.asset_impact.unwrap_or_default().to_lowercase();
        if direction.contains("accelerating") || impact.contains("bullish") {
            bull += 1;
        }
        if direction.contains("decelerating")
            || direction.contains("reversing")
            || impact.contains("bearish")
        {
            bear += 1;
        }
    }

    if bear > bull {
        Some("bear".to_string())
    } else if bull > bear {
        Some("bull".to_string())
    } else {
        Some("neutral".to_string())
    }
}

fn structural_layer_bias(backend: &BackendConnection) -> Option<String> {
    let top = crate::db::structural::list_outcomes_backend(backend)
        .ok()?
        .into_iter()
        .find(|o| o.status == "active")?;
    let text = format!(
        "{} {} {}",
        top.name,
        top.description.unwrap_or_default(),
        top.asset_implications.unwrap_or_default()
    )
    .to_lowercase();
    if text.contains("decline")
        || text.contains("fragmentation")
        || text.contains("crisis")
        || text.contains("bearish")
    {
        Some("bear".to_string())
    } else if text.contains("dominance") || text.contains("bullish") || text.contains("expansion") {
        Some("bull".to_string())
    } else {
        Some("neutral".to_string())
    }
}

fn maybe_insert_signal(
    backend: &BackendConnection,
    signal_type: &str,
    layers: &[String],
    assets: &[String],
    description: &str,
    severity: &str,
) -> Result<()> {
    let layers_json = serde_json::to_string(layers)?;
    let assets_json = serde_json::to_string(assets)?;

    let recent = timeframe_signals::list_signals_backend(backend, Some(signal_type), None, Some(100))?;
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(6);
    let exists_recent = recent.into_iter().any(|s| {
        if s.description != description {
            return false;
        }
        let parsed = chrono::DateTime::parse_from_rfc3339(&s.detected_at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .ok();
        parsed.is_some_and(|dt| dt >= cutoff)
    });
    if exists_recent {
        return Ok(());
    }

    let _ = timeframe_signals::add_signal_backend(
        backend,
        signal_type,
        &layers_json,
        &assets_json,
        description,
        severity,
    )?;
    Ok(())
}

fn detect_timeframe_signals(backend: &BackendConnection) -> Result<()> {
    let low = classify_regime_bias(backend).unwrap_or_else(|| "neutral".to_string());
    let medium = top_scenario_bias(backend).unwrap_or_else(|| "neutral".to_string());
    let high = trend_layer_bias(backend).unwrap_or_else(|| "neutral".to_string());
    let macro_bias = structural_layer_bias(backend).unwrap_or_else(|| "neutral".to_string());

    let layers = vec![
        format!("low:{}", low),
        format!("medium:{}", medium),
        format!("high:{}", high),
        format!("macro:{}", macro_bias),
    ];
    let values = [
        low.clone(),
        medium.clone(),
        high.clone(),
        macro_bias.clone(),
    ];

    let bull_count = values.iter().filter(|v| v.as_str() == "bull").count();
    let bear_count = values.iter().filter(|v| v.as_str() == "bear").count();
    let neutral_count = values.iter().filter(|v| v.as_str() == "neutral").count();

    if bull_count >= 3 || bear_count >= 3 {
        let stance = if bull_count >= 3 {
            "bullish"
        } else {
            "bearish"
        };
        let severity = if bull_count == 4 || bear_count == 4 {
            "critical"
        } else {
            "notable"
        };
        let description = format!(
            "{} cross-timeframe alignment ({} / 4 layers agree)",
            stance,
            bull_count.max(bear_count)
        );
        maybe_insert_signal(
            backend,
            "alignment",
            &layers,
            &["SPY".to_string(), "BTC".to_string(), "GC=F".to_string()],
            &description,
            severity,
        )?;
    } else if bull_count >= 1 && bear_count >= 1 && neutral_count <= 1 {
        let description = format!(
            "cross-timeframe divergence (bull={} bear={} neutral={})",
            bull_count, bear_count, neutral_count
        );
        maybe_insert_signal(
            backend,
            "divergence",
            &layers,
            &["SPY".to_string(), "BTC".to_string(), "GC=F".to_string()],
            &description,
            "notable",
        )?;
    }
    let regimes = crate::db::regime_snapshots::get_history_backend(backend, Some(2)).unwrap_or_default();
    if regimes.len() == 2 {
        let curr = &regimes[0].regime;
        let prev = &regimes[1].regime;
        if curr != prev {
            let description = format!("regime transition detected: {} -> {}", prev, curr);
            maybe_insert_signal(
                backend,
                "transition",
                &["low".to_string()],
                &["SPY".to_string(), "^VIX".to_string()],
                &description,
                "critical",
            )?;
        }
    }

    Ok(())
}

/// Compute current positions and store a daily portfolio snapshot.
fn store_portfolio_snapshot(
    backend: &BackendConnection,
    config: &Config,
) -> Result<()> {
    let cached = get_all_cached_prices_backend(backend)?;
    let prices: HashMap<String, Decimal> =
        cached.into_iter().map(|q| (q.symbol, q.price)).collect();

    let fx_rates = fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let transactions = list_transactions_backend(backend)?;
            if transactions.is_empty() {
                return Ok(());
            }
            compute_positions(&transactions, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocations = list_allocations_backend(backend)?;
            if allocations.is_empty() {
                return Ok(());
            }
            compute_positions_from_allocations(&allocations, &prices, &fx_rates)
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
    upsert_portfolio_snapshot_backend(backend, &today, total_value, cash_value, invested_value)?;

    // Store per-position snapshots
    for pos in &positions {
        let price = pos.current_price.unwrap_or(dec!(0));
        let value = pos.current_value.unwrap_or(dec!(0));
        upsert_position_snapshot_backend(backend, &today, &pos.symbol, pos.quantity, price, value)?;
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

    #[test]
    fn refresh_lock_prevents_concurrent_runs() {
        let tmp =
            std::env::temp_dir().join(format!("pftui-refresh-lock-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        std::env::set_var("PFTUI_REFRESH_LOCK_DIR", tmp.to_string_lossy().to_string());

        let first = RefreshLock::acquire().expect("first lock should succeed");
        let second = RefreshLock::acquire();
        assert!(
            second.is_err(),
            "second lock should fail while first is held"
        );

        drop(first);
        let third = RefreshLock::acquire();
        assert!(third.is_ok(), "lock should be acquirable after release");
        drop(third);

        std::env::remove_var("PFTUI_REFRESH_LOCK_DIR");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
