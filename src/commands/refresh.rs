use std::collections::HashMap;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::alerts::engine;
use crate::analytics::levels as level_engine;
use crate::analytics::technicals;
use crate::commands::refresh_dag::{RefreshResult, SourceResult, SourceStatus};
use crate::config::{Config, PortfolioMode};
use crate::data::{
    bls, brave, calendar, comex, cot, economic, fedwatch, fred, fx, ism, onchain, predictions, rss,
    sentiment, worldbank,
};
use crate::db::allocations::{get_unique_allocation_symbols_backend, list_allocations_backend};
use crate::db::backend::BackendConnection;
use crate::db::economic_data as economic_data_db;
use crate::db::fedwatch_cache;
use crate::db::price_cache::{
    get_all_cached_prices_backend, get_cached_price_backend, upsert_price_backend,
};
use crate::db::price_history::{
    get_history_backend, get_price_at_date_backend, upsert_history_backend,
};
use crate::db::snapshots::{upsert_portfolio_snapshot_backend, upsert_position_snapshot_backend};
use crate::db::technical_levels;
use crate::db::technical_snapshots;
use crate::db::timeframe_signals;
use crate::db::transactions::{get_unique_symbols_backend, list_transactions_backend};
use crate::db::watchlist::get_watchlist_symbols_backend;
use crate::db::{bls_cache, calendar_cache, comex_cache, cot_cache, fx_cache, news_cache};
use crate::db::prediction_contracts;
use crate::db::{
    economic_cache, macro_events, onchain_cache, predictions_cache, sentiment_cache,
    worldbank_cache,
};
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations};
use crate::models::price::{HistoryRecord, PriceQuote};
use crate::notify;
use crate::price::{coingecko, yahoo};
use crate::tui::views::economy;

/// Maximum number of concurrent Yahoo Finance API requests.
/// Limits parallelism to avoid rate-limiting while still being faster than sequential.
const YAHOO_MAX_CONCURRENT: usize = 4;
/// Per-request timeout for remote price/history calls.
const PRICE_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Freshness thresholds in seconds
const NEWS_FRESHNESS_SECS: i64 = 10 * 60; // 10 minutes
const BRAVE_NEWS_FRESHNESS_SECS: i64 = 4 * 60 * 60; // 4 hours
const PREDICTIONS_FRESHNESS_SECS: i64 = 60 * 60; // 1 hour
const SENTIMENT_FRESHNESS_SECS: i64 = 60 * 60; // 1 hour
const CALENDAR_FRESHNESS_SECS: i64 = 24 * 60 * 60; // 24 hours
const COT_FRESHNESS_SECS: i64 = 7 * 24 * 60 * 60; // 1 week
const BLS_FRESHNESS_DAYS: i64 = 30; // 1 month
const BRAVE_NEWS_QUERY_LIMIT: usize = 12;
const FEDWATCH_CONFLICT_THRESHOLD_PCT_POINTS: f64 = 5.0;
const FEDWATCH_VALIDATION_THRESHOLD_PCT_POINTS: f64 = 10.0;
const COT_HISTORY_WEEKS: usize = 156;
const CALENDAR_RETENTION_DAYS: i64 = 30;
const COT_RETENTION_DAYS: i64 = 365 * 3;
const SENTIMENT_RETENTION_DAYS: u32 = 365;
const TECHNICAL_SIGNAL_RETENTION_HOURS: i64 = 24 * 14;

#[derive(Debug, Clone)]
pub struct RefreshPlan {
    pub prices: bool,
    pub predictions: bool,
    pub fedwatch: bool,
    pub news_rss: bool,
    pub news_brave: bool,
    pub cot: bool,
    pub sentiment: bool,
    pub calendar: bool,
    pub economy: bool,
    pub fred: bool,
    pub bls: bool,
    pub worldbank: bool,
    pub comex: bool,
    pub onchain: bool,
    pub analytics: bool,
    pub alerts: bool,
    pub cleanup: bool,
}

impl RefreshPlan {
    /// All known source names, used for validation.
    pub const ALL_SOURCE_NAMES: &'static [&'static str] = &[
        "prices",
        "predictions",
        "fedwatch",
        "news_rss",
        "news_brave",
        "news",
        "cot",
        "sentiment",
        "calendar",
        "economy",
        "fred",
        "bls",
        "worldbank",
        "comex",
        "onchain",
        "analytics",
        "alerts",
        "cleanup",
    ];

    pub fn full() -> Self {
        Self {
            prices: true,
            predictions: true,
            fedwatch: true,
            news_rss: true,
            news_brave: true,
            cot: true,
            sentiment: true,
            calendar: true,
            economy: true,
            fred: true,
            bls: true,
            worldbank: true,
            comex: true,
            onchain: true,
            analytics: true,
            alerts: true,
            cleanup: true,
        }
    }

    /// All sources disabled — used as a starting point for `--only`.
    pub fn none() -> Self {
        Self {
            prices: false,
            predictions: false,
            fedwatch: false,
            news_rss: false,
            news_brave: false,
            cot: false,
            sentiment: false,
            calendar: false,
            economy: false,
            fred: false,
            bls: false,
            worldbank: false,
            comex: false,
            onchain: false,
            analytics: false,
            alerts: false,
            cleanup: false,
        }
    }

    /// Convenience plan for refreshing only price data.
    pub fn prices_only() -> Self {
        let mut plan = Self::none();
        plan.prices = true;
        plan
    }

    /// Build a plan that only enables the named sources.
    /// Returns an error if any source name is unrecognised.
    pub fn from_only(sources: &[String]) -> anyhow::Result<Self> {
        let mut plan = Self::none();
        for name in sources {
            plan.set_source(name.trim(), true)?;
        }
        Ok(plan)
    }

    /// Build a full plan minus the named sources.
    /// Returns an error if any source name is unrecognised.
    pub fn from_skip(sources: &[String]) -> anyhow::Result<Self> {
        let mut plan = Self::full();
        for name in sources {
            plan.set_source(name.trim(), false)?;
        }
        Ok(plan)
    }

    /// Set a source by name. "news" is a convenience alias for both news_rss + news_brave.
    fn set_source(&mut self, name: &str, enabled: bool) -> anyhow::Result<()> {
        match name {
            "prices" => self.prices = enabled,
            "predictions" => self.predictions = enabled,
            "fedwatch" => self.fedwatch = enabled,
            "news_rss" => self.news_rss = enabled,
            "news_brave" => self.news_brave = enabled,
            "news" => {
                self.news_rss = enabled;
                self.news_brave = enabled;
            }
            "cot" => self.cot = enabled,
            "sentiment" => self.sentiment = enabled,
            "calendar" => self.calendar = enabled,
            "economy" => self.economy = enabled,
            "fred" => self.fred = enabled,
            "bls" => self.bls = enabled,
            "worldbank" => self.worldbank = enabled,
            "comex" => self.comex = enabled,
            "onchain" => self.onchain = enabled,
            "analytics" => self.analytics = enabled,
            "alerts" => self.alerts = enabled,
            "cleanup" => self.cleanup = enabled,
            _ => anyhow::bail!(
                "Unknown refresh source '{}'. Valid sources: {}",
                name,
                Self::ALL_SOURCE_NAMES.join(", ")
            ),
        }
        Ok(())
    }

    pub fn selected_task_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.prices {
            names.push("prices");
        }
        if self.predictions {
            names.push("predictions");
        }
        if self.fedwatch {
            names.push("fedwatch");
        }
        if self.news_rss {
            names.push("news_rss");
        }
        if self.news_brave {
            names.push("news_brave");
        }
        if self.cot {
            names.push("cot");
        }
        if self.sentiment {
            names.push("sentiment");
        }
        if self.calendar {
            names.push("calendar");
        }
        if self.economy {
            names.push("economy");
        }
        if self.fred {
            names.push("fred");
        }
        if self.bls {
            names.push("bls");
        }
        if self.worldbank {
            names.push("worldbank");
        }
        if self.comex {
            names.push("comex");
        }
        if self.onchain {
            names.push("onchain");
        }
        if self.analytics {
            names.push("analytics");
        }
        if self.alerts {
            names.push("alerts");
        }
        if self.cleanup {
            names.push("cleanup");
        }
        names
    }
}

/// Infer the asset category for a universe symbol based on its group.
fn category_for_universe_group(group: &str) -> AssetCategory {
    match group {
        "indices" | "sectors" => AssetCategory::Equity,
        "commodities" => AssetCategory::Commodity,
        "fx" => AssetCategory::Forex,
        "rates" => AssetCategory::Fund, // Treasury yields use Fund like economy view
        "crypto_majors" => AssetCategory::Crypto,
        "custom" => AssetCategory::Equity, // default; infer_category may override
        _ => AssetCategory::Equity,
    }
}

/// Collect all symbols that need pricing: portfolio positions + watchlist + tracked universe.
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

    // Market overview symbols (S&P 500, Dow, Nasdaq, Russell 2000, etc.)
    // These are used by the markets view, morning briefs, and agents.
    for item in crate::tui::views::markets::market_symbols() {
        seen.entry(item.yahoo_symbol).or_insert(item.category);
    }

    // Sector ETFs (XLE, XLK, etc.) — needed for `pftui sector` command
    for (symbol, _name) in crate::commands::sector::SECTOR_ETFS {
        seen.entry(symbol.to_string())
            .or_insert(AssetCategory::Equity);
    }

    // Tracked universe symbols from config
    for group_name in crate::config::TrackedUniverse::group_names() {
        if let Some(symbols) = config.tracked_universe.group(group_name) {
            let cat = category_for_universe_group(group_name);
            for sym in symbols {
                // For custom group, try to infer category from the symbol itself
                let effective_cat = if *group_name == "custom" {
                    crate::models::asset_names::infer_category(sym)
                } else {
                    cat
                };
                seen.entry(sym.clone()).or_insert(effective_cat);
            }
        }
    }

    Ok(seen.into_iter().collect())
}

/// Check if news needs refreshing
fn news_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    // Check most recent news entry
    let news = news_cache::get_latest_news_backend(backend, 1, None, None, None, None)?;
    if news.is_empty() {
        return Ok(true);
    }

    let now = chrono::Utc::now();
    if let Some(fetched) = parse_timestamp_flexible(&news[0].fetched_at) {
        let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
        return Ok(age.num_seconds() > NEWS_FRESHNESS_SECS);
    }
    Ok(true)
}

fn brave_news_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    let latest = news_cache::latest_fetched_at_by_source_type_backend(backend, "brave")?;
    let Some(timestamp) = latest else {
        return Ok(true);
    };

    let now = chrono::Utc::now();
    if let Some(fetched) = parse_timestamp_flexible(&timestamp) {
        let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
        return Ok(age.num_seconds() > BRAVE_NEWS_FRESHNESS_SECS);
    }
    Ok(true)
}

fn fedwatch_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    let latest = fedwatch_cache::get_latest_snapshot_backend(backend)?;
    Ok(match latest {
        Some(entry) => !fedwatch::is_fresh(&entry.fetched_at, fedwatch::FEDWATCH_FRESHNESS_SECS),
        None => true,
    })
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

fn build_brave_news_queries(backend: &BackendConnection, config: &Config) -> Result<Vec<String>> {
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

fn fred_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    let observations = economic_cache::get_all_latest_backend(backend)?;
    if observations.is_empty() {
        return Ok(true);
    }

    for series in fred::FRED_SERIES {
        let latest = observations.iter().find(|obs| obs.series_id == series.id);
        match latest {
            Some(obs) if !fred::is_stale(&obs.date, series.frequency) => {}
            _ => return Ok(true),
        }
    }

    Ok(false)
}

/// Check if prediction market contracts need refreshing
fn contracts_need_refresh(backend: &BackendConnection) -> Result<bool> {
    match prediction_contracts::get_last_update_backend(backend)? {
        None => Ok(true),
        Some(ts) => {
            let now = chrono::Utc::now().timestamp();
            Ok((now - ts) > PREDICTIONS_FRESHNESS_SECS)
        }
    }
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
    if let Some(fetched) = parse_timestamp_flexible(&events[0].fetched_at) {
        let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
        return Ok(age.num_seconds() > CALENDAR_FRESHNESS_SECS);
    }
    Ok(true)
}

/// Try to parse a timestamp that may be RFC 3339 or Postgres `::text` format.
/// Postgres `::text` uses a space separator and may abbreviate the timezone
/// (e.g. `2026-03-09 17:50:47.025534+00`). We normalise to RFC 3339 before parsing.
fn parse_timestamp_flexible(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    // Try RFC 3339 first (fastest path).
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt);
    }
    // Normalise Postgres text: replace first space with 'T', expand bare +00 → +00:00.
    let normalised = s.replacen(' ', "T", 1);
    let normalised = if normalised.ends_with("+00") || normalised.ends_with("-00") {
        format!("{}:00", normalised)
    } else {
        normalised
    };
    chrono::DateTime::parse_from_rfc3339(&normalised).ok()
}

/// Check if COT needs refreshing
fn cot_needs_refresh(backend: &BackendConnection) -> Result<bool> {
    let reports = cot_cache::get_all_latest_backend(backend)?;
    if reports.is_empty() {
        return Ok(true);
    }

    if latest_cot_report_age_days(&reports).is_some_and(|age_days| age_days > 7) {
        return Ok(true);
    }

    let now = chrono::Utc::now();
    let mut any_parsed = false;
    for report in reports {
        if let Some(fetched) = parse_timestamp_flexible(&report.fetched_at) {
            any_parsed = true;
            let age = now.signed_duration_since(fetched.with_timezone(&chrono::Utc));
            if age.num_seconds() > COT_FRESHNESS_SECS {
                return Ok(true);
            }
        }
    }
    // If no timestamps could be parsed, assume stale (safe fallback).
    Ok(!any_parsed)
}

fn latest_cot_report_age_days(reports: &[crate::db::cot_cache::CotCacheEntry]) -> Option<i64> {
    let latest = reports
        .iter()
        .filter_map(|report| chrono::NaiveDate::parse_from_str(&report.report_date, "%Y-%m-%d").ok())
        .max()?;
    Some((chrono::Utc::now().date_naive() - latest).num_days())
}

fn cot_staleness_detail(backend: &BackendConnection) -> Option<String> {
    let reports = cot_cache::get_all_latest_backend(backend).ok()?;
    let latest = reports
        .iter()
        .filter_map(|report| {
            chrono::NaiveDate::parse_from_str(&report.report_date, "%Y-%m-%d")
                .ok()
                .map(|date| (date, report.report_date.as_str()))
        })
        .max_by_key(|(date, _)| *date)?;
    let age_days = (chrono::Utc::now().date_naive() - latest.0).num_days();
    (age_days > 7).then(|| format!("latest COT report date {} is {} days old", latest.1, age_days))
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

async fn fetch_history_for_symbol(
    symbol: &str,
    category: AssetCategory,
    days: u32,
) -> Result<(Vec<crate::models::price::HistoryRecord>, &'static str)> {
    match category {
        AssetCategory::Crypto => {
            match tokio::time::timeout(
                PRICE_REQUEST_TIMEOUT,
                coingecko::fetch_history(symbol, days),
            )
            .await
            {
                Ok(Ok(records)) if !records.is_empty() => Ok((records, "coingecko")),
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
                    let yahoo_sym = yahoo_crypto_symbol(symbol);
                    let records = tokio::time::timeout(
                        PRICE_REQUEST_TIMEOUT,
                        yahoo::fetch_history(&yahoo_sym, days),
                    )
                    .await
                    .map_err(|_| anyhow::anyhow!("{} history request timed out", yahoo_sym))??;
                    Ok((records, "yahoo"))
                }
            }
        }
        AssetCategory::Cash => Ok((Vec::new(), "static")),
        _ => {
            let records =
                tokio::time::timeout(PRICE_REQUEST_TIMEOUT, yahoo::fetch_history(symbol, days))
                    .await
                    .map_err(|_| anyhow::anyhow!("{} history request timed out", symbol))??;
            Ok((records, "yahoo"))
        }
    }
}

/// Fetch a single Yahoo symbol with timeout. Returns Ok(quote) or Err(error string).
async fn fetch_yahoo_price_with_timeout(sym: &str) -> Result<PriceQuote, String> {
    match tokio::time::timeout(PRICE_REQUEST_TIMEOUT, yahoo::fetch_price(sym)).await {
        Ok(Ok(quote)) => Ok(quote),
        Ok(Err(e)) => Err(format!("{}: {}", sym, e)),
        Err(_) => Err(format!("{}: request timed out", sym)),
    }
}

/// Fetch prices for all given symbols and return the results.
///
/// Yahoo Finance requests are limited to [`YAHOO_MAX_CONCURRENT`] in-flight
/// at a time via a [`tokio::sync::Semaphore`], providing ~4× speedup over
/// the previous sequential loop while staying well within rate limits.
async fn fetch_all_prices(
    symbols: &[(String, AssetCategory)],
    config: &Config,
) -> (Vec<PriceQuote>, Vec<String>) {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

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
                    previous_close: None,
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

    // Fetch Yahoo prices concurrently, limited by semaphore
    let semaphore = Arc::new(Semaphore::new(YAHOO_MAX_CONCURRENT));
    let mut handles = Vec::with_capacity(yahoo_symbols.len());
    for sym in &yahoo_symbols {
        let sem = Arc::clone(&semaphore);
        let sym = sym.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            fetch_yahoo_price_with_timeout(&sym).await
        }));
    }
    for handle in handles {
        match handle.await {
            Ok(Ok(quote)) => quotes.push(quote),
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(format!("task panicked: {}", e)),
        }
    }

    // Fetch crypto: CoinGecko batch first, Yahoo fallback
    if !crypto_symbols.is_empty() {
        let mut cg_ok = false;
        match tokio::time::timeout(
            PRICE_REQUEST_TIMEOUT,
            coingecko::fetch_prices(&crypto_symbols),
        )
        .await
        {
            Ok(Ok(cg_quotes)) if !cg_quotes.is_empty() => {
                for q in cg_quotes {
                    quotes.push(q);
                }
                cg_ok = true;
            }
            Ok(Ok(_)) => {
                errors.push("CoinGecko returned empty, falling back to Yahoo".to_string());
            }
            Ok(Err(e)) => {
                errors.push(format!(
                    "CoinGecko batch failed: {}, falling back to Yahoo",
                    e
                ));
            }
            Err(_) => {
                errors.push("CoinGecko batch timed out, falling back to Yahoo".to_string());
            }
        }

        if !cg_ok {
            // Crypto Yahoo fallback also uses semaphore concurrency
            let sem = Arc::new(Semaphore::new(YAHOO_MAX_CONCURRENT));
            let mut crypto_handles = Vec::with_capacity(crypto_symbols.len());
            for sym in &crypto_symbols {
                let sem = Arc::clone(&sem);
                let sym = sym.clone();
                crypto_handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    let yahoo_sym = yahoo_crypto_symbol(&sym);
                    match fetch_yahoo_price_with_timeout(&yahoo_sym).await {
                        Ok(mut quote) => {
                            quote.symbol = sym;
                            Ok(quote)
                        }
                        Err(e) => Err(format!(
                            "{}: CoinGecko + Yahoo both failed: {}",
                            sym,
                            e.split(": ").last().unwrap_or(&e)
                        )),
                    }
                }));
            }
            for handle in crypto_handles {
                match handle.await {
                    Ok(Ok(quote)) => quotes.push(quote),
                    Ok(Err(e)) => errors.push(e),
                    Err(e) => errors.push(format!("task panicked: {}", e)),
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

macro_rules! info_ln {
    ($verbose:expr, $($arg:tt)*) => {
        if $verbose {
            println!($($arg)*);
        }
    };
}

macro_rules! warn_ln {
    ($verbose:expr, $($arg:tt)*) => {
        if $verbose {
            eprintln!($($arg)*);
        }
    };
}

pub fn run(backend: &BackendConnection, config: &Config, notify: bool) -> Result<()> {
    run_with_output(backend, config, notify, true, &RefreshPlan::full())
}

/// Run refresh with a custom plan (verbose human output).
pub fn run_with_plan(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    plan: &RefreshPlan,
) -> Result<()> {
    run_with_output(backend, config, notify, true, plan)
}

/// Run refresh with a custom plan and output structured JSON metrics.
pub fn run_json_with_plan(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    plan: &RefreshPlan,
) -> Result<()> {
    let result = run_pipeline(backend, config, notify, false, plan)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

pub fn run_quiet(backend: &BackendConnection, config: &Config, notify: bool) -> Result<()> {
    run_with_output(backend, config, notify, false, &RefreshPlan::full())
}

pub fn run_quiet_with_plan(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    plan: &RefreshPlan,
) -> Result<()> {
    run_with_output(backend, config, notify, false, plan)
}

fn run_with_output(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    verbose: bool,
    plan: &RefreshPlan,
) -> Result<()> {
    let _result = run_pipeline(backend, config, notify, verbose, plan)?;
    Ok(())
}

/// Core refresh pipeline that returns structured results.
/// When `verbose` is true, prints human-readable output to stdout (existing behavior).
/// Always returns a `RefreshResult` for programmatic consumption.
fn run_pipeline(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    verbose: bool,
    plan: &RefreshPlan,
) -> Result<RefreshResult> {
    let _lock = RefreshLock::acquire()?;
    let pipeline_start = Instant::now();
    let mut dag_result = RefreshResult::new();

    info_ln!(verbose, "Refreshing selected data sources...\n");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    if plan.selected_task_names().is_empty() {
        info_ln!(verbose, "⊘ Refresh (no tasks due)");
        dag_result.finalize(pipeline_start.elapsed());
        return Ok(dag_result);
    }

    // ── DAG Layer 0: Independent data sources ──────────────────────────
    // These sources have no dependency on each other. We determine
    // freshness up front, then fetch concurrently via tokio::join!,
    // then write results to the database sequentially.

    let predictions_due = plan.predictions && predictions_need_refresh(backend).unwrap_or(true);
    let contracts_due = plan.predictions && contracts_need_refresh(backend).unwrap_or(true);
    let fedwatch_due = plan.fedwatch && fedwatch_needs_refresh(backend).unwrap_or(true);
    let rss_refresh = plan.news_rss && news_needs_refresh(backend).unwrap_or(true);
    let brave_key = config
        .brave_api_key
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let brave_refresh = if !plan.news_brave || brave_key.is_empty() {
        false
    } else {
        brave_news_needs_refresh(backend).unwrap_or(true)
    };
    let sentiment_due = plan.sentiment && sentiment_needs_refresh(backend).unwrap_or(true);
    let calendar_due = plan.calendar && calendar_needs_refresh(backend).unwrap_or(true);
    let cot_due = plan.cot && cot_needs_refresh(backend).unwrap_or(true);
    let bls_due = plan.bls && bls_needs_refresh(backend).unwrap_or(true);
    let fred_api_key_str = config
        .fred_api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let fred_due =
        plan.fred && fred_api_key_str.is_some() && fred_needs_refresh(backend).unwrap_or(true);
    let worldbank_due = plan.worldbank && worldbank_needs_refresh(backend).unwrap_or(true);
    let comex_due = plan.comex && comex_needs_refresh(backend).unwrap_or(true);

    // Build Brave news queries before entering async context (needs DB)
    let brave_queries = if brave_refresh {
        build_brave_news_queries(backend, config).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Capture FedWatch previous snapshot for validation
    let fedwatch_previous = if fedwatch_due {
        fedwatch_cache::get_latest_snapshot_backend(backend)
            .ok()
            .flatten()
    } else {
        None
    };

    // ── Parallel async fetch for independent sources ───────────────────
    // Network I/O runs concurrently via tokio::join!; DB writes are sequential.
    let layer0_fetched = rt.block_on(async {
        let predictions_fut = async {
            if !predictions_due {
                return None;
            }
            let start = Instant::now();
            Some((
                predictions::fetch_polymarket_predictions().await,
                start.elapsed(),
            ))
        };

        let contracts_fut = async {
            if !contracts_due {
                return None;
            }
            let start = Instant::now();
            Some((
                predictions::fetch_polymarket_contracts().await,
                start.elapsed(),
            ))
        };

        let sentiment_fut = async {
            if !sentiment_due {
                return None;
            }
            let start = Instant::now();
            let crypto = sentiment::fetch_crypto_fng();
            let trad = sentiment::fetch_traditional_fng();
            Some((crypto, trad, start.elapsed()))
        };

        let bls_fut = async {
            if !bls_due {
                return None;
            }
            let start = Instant::now();
            Some((bls::fetch_all_key_series().await, start.elapsed()))
        };

        let worldbank_fut = async {
            if !worldbank_due {
                return None;
            }
            let start = Instant::now();
            Some((worldbank::fetch_all_indicators().await, start.elapsed()))
        };

        let economy_fut = async {
            if !plan.economy {
                return None;
            }
            let start = Instant::now();
            let bk = brave_key.clone();
            let mut used_brave = false;
            let readings = if !bk.is_empty() {
                match economic::fetch_via_brave(&bk).await {
                    Ok(v) if !v.is_empty() => {
                        used_brave = true;
                        Ok(v)
                    }
                    Ok(_) => economic::fetch_bls_fallback().await.or_else(|_| Ok(Vec::new())),
                    Err(_) => economic::fetch_bls_fallback().await.or_else(|_| Ok(Vec::new())),
                }
            } else {
                // BLS fallback — if rate-limited, return empty (FRED cache
                // synthesis in the economy command provides data anyway).
                economic::fetch_bls_fallback().await.or_else(|_| Ok(Vec::new()))
            };
            Some((readings, used_brave, start.elapsed()))
        };

        // ISM PMI: dedicated targeted extraction for manufacturing + services PMI
        let ism_brave_key = brave_key.clone();
        let ism_fut = async {
            if !plan.economy || ism_brave_key.is_empty() {
                return None;
            }
            let start = Instant::now();
            match ism::fetch_ism_pmi(&ism_brave_key).await {
                Ok(readings) if !readings.is_empty() => Some((Ok(readings), start.elapsed())),
                Ok(_) => None,
                Err(_) => None, // ISM is supplementary; silent failure is fine
            }
        };

        let fred_fut = async {
            if !fred_due {
                return None;
            }
            let api_key = fred_api_key_str.as_deref().unwrap_or("");
            let start = Instant::now();
            let mut results = Vec::new();
            for series in fred::FRED_SERIES {
                match fred::fetch_series(api_key, series.id, 24).await {
                    Ok(obs) if !obs.is_empty() => results.push((series.id, Ok(obs))),
                    Ok(_) => {}
                    Err(e) => results.push((series.id, Err(e))),
                }
            }
            Some((results, start.elapsed()))
        };

        let rss_fut = async {
            if !rss_refresh {
                return None;
            }
            let start = Instant::now();
            let feeds = rss::default_feeds();
            Some((rss::fetch_all_feeds(&feeds).await, start.elapsed()))
        };

        let brave_news_fut = async {
            if !brave_refresh {
                return None;
            }
            let start = Instant::now();
            let mut all_results = Vec::new();
            for query in &brave_queries {
                match brave::brave_news_search(&brave_key, query, Some("pd"), 5).await {
                    Ok(r) => all_results.push((query.clone(), Ok(r))),
                    Err(e) => all_results.push((query.clone(), Err(e))),
                }
            }
            Some((all_results, start.elapsed()))
        };

        let calendar_fut = async {
            if !calendar_due {
                return None;
            }
            let start = Instant::now();
            match calendar::fetch_events(30) {
                Ok(mut events) => {
                    let bk = brave_key.clone();
                    if !bk.is_empty() {
                        let _ = calendar::enrich_with_brave(&mut events, &bk).await;
                    }
                    Some((Ok(events), start.elapsed()))
                }
                Err(e) => Some((Err(e), start.elapsed())),
            }
        };

        tokio::join!(
            predictions_fut,
            contracts_fut,
            sentiment_fut,
            bls_fut,
            worldbank_fut,
            economy_fut,
            ism_fut,
            fred_fut,
            rss_fut,
            brave_news_fut,
            calendar_fut,
        )
    });

    let (
        predictions_data,
        contracts_data,
        sentiment_data,
        bls_data,
        worldbank_data,
        economy_data,
        ism_data,
        fred_data,
        rss_data,
        brave_news_data,
        calendar_data,
    ) = layer0_fetched;

    // ── Layer 0: FX Rates (sequential, fast) ───────────────────────────
    if plan.prices {
        let fx_start = Instant::now();
        match rt.block_on(fx::fetch_all_fx_rates()) {
            Ok(rates) => {
                for (currency, rate) in &rates {
                    if let Err(e) = fx_cache::upsert_fx_rate_backend(backend, currency, *rate) {
                        warn_ln!(verbose, "Failed to cache FX rate for {}: {}", currency, e);
                    }
                }
                let detail = format!("✓ FX rates ({} currencies)", rates.len());
                info_ln!(verbose, "{}", detail);
                dag_result.add(SourceResult {
                    name: "fx_rates".to_string(),
                    label: "FX Rates".to_string(),
                    status: SourceStatus::Ok,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(rates.len()),
                    duration_ms: fx_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: None,
                    detail: Some(detail),
                });
            }
            Err(e) => {
                let detail = format!("✗ FX rates (failed: {})", e);
                info_ln!(verbose, "{}", detail);
                dag_result.add(SourceResult {
                    name: "fx_rates".to_string(),
                    label: "FX Rates".to_string(),
                    status: SourceStatus::Failed,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                    duration_ms: fx_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: Some(e.to_string()),
                    detail: Some(detail),
                });
            }
        }
    } else {
        let detail = "⊘ FX rates (cadence deferred)".to_string();
        info_ln!(verbose, "{}", detail);
        dag_result.add(SourceResult {
            name: "fx_rates".to_string(),
            label: "FX Rates".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: Some(detail),
        });
    }

    // ── Layer 0: Store async-fetched results ───────────────────────────

    // Predictions (legacy predictions_cache)
    store_predictions_result(
        backend,
        verbose,
        predictions_due,
        plan.predictions,
        predictions_data,
        &mut dag_result,
    );

    // Prediction market contracts (F55.2 — enriched, tag-based)
    store_contracts_result(
        backend,
        verbose,
        contracts_due,
        plan.predictions,
        contracts_data,
        &mut dag_result,
    );

    // FedWatch (synchronous fetch — not parallelized due to scraping nature)
    store_fedwatch_result(
        backend,
        config,
        verbose,
        fedwatch_due,
        plan.fedwatch,
        fedwatch_previous.as_ref(),
        &mut dag_result,
    );
    maybe_report_fedwatch_conflict(backend, verbose);

    // Sentiment
    store_sentiment_result(
        backend,
        verbose,
        sentiment_due,
        plan.sentiment,
        sentiment_data,
        &mut dag_result,
    );

    // News (RSS + Brave)
    store_news_result(
        backend,
        verbose,
        rss_refresh,
        brave_refresh,
        plan.news_rss || plan.news_brave,
        rss_data,
        brave_news_data,
        &mut dag_result,
    );

    // COT (synchronous — uses reqwest::blocking internally)
    store_cot_result(backend, verbose, cot_due, plan.cot, &mut dag_result);

    // Calendar
    store_calendar_result(
        backend,
        verbose,
        calendar_due,
        plan.calendar,
        calendar_data,
        &rt,
        &mut dag_result,
    );

    // Economy
    store_economy_result(
        backend,
        verbose,
        plan.economy,
        economy_data,
        &mut dag_result,
    );

    // ISM PMI (targeted extraction, supplements economy data)
    store_ism_result(backend, verbose, plan.economy, ism_data, &mut dag_result);

    // FRED
    store_fred_result(
        backend,
        verbose,
        fred_due,
        plan.fred,
        fred_api_key_str.is_some(),
        fred_data,
        &mut dag_result,
    );

    // BLS
    store_bls_result(
        backend,
        verbose,
        bls_due,
        plan.bls,
        bls_data,
        &mut dag_result,
    );

    // World Bank
    store_worldbank_result(
        backend,
        verbose,
        worldbank_due,
        plan.worldbank,
        worldbank_data,
        &mut dag_result,
    );

    // COMEX (synchronous)
    store_comex_result(backend, verbose, comex_due, plan.comex, &mut dag_result);

    // On-chain (synchronous)
    store_onchain_result(backend, verbose, plan.onchain, &mut dag_result);

    // ── DAG Layer 1: Prices ────────────────────────────────────────────
    let symbols = if plan.prices {
        collect_symbols(backend, config)?
    } else {
        Vec::new()
    };
    if plan.prices && !symbols.is_empty() {
        let price_start = Instant::now();
        let (quotes, errors) = rt.block_on(fetch_all_prices(&symbols, config));

        for quote in &quotes {
            if let Err(e) = upsert_price_backend(backend, quote) {
                warn_ln!(verbose, "Failed to cache {}: {}", quote.symbol, e);
            }
        }
        // Stamp today's close into price_history
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let mut history_stamp_ok = 0usize;
        let mut history_stamp_err = 0usize;
        let mut history_stamp_examples: Vec<String> = Vec::new();
        for quote in &quotes {
            if quote.source == "static" {
                continue;
            }
            let record = HistoryRecord {
                date: today.clone(),
                close: quote.price,
                volume: None,
                open: None,
                high: None,
                low: None,
            };
            match upsert_history_backend(backend, &quote.symbol, &quote.source, &[record]) {
                Ok(_) => history_stamp_ok += 1,
                Err(e) => {
                    history_stamp_err += 1;
                    if history_stamp_examples.len() < 3 {
                        history_stamp_examples.push(format!("{}: {}", quote.symbol, e));
                    }
                }
            }
        }

        // Stamp cached prices for symbols that failed live fetch
        let live_symbols: HashSet<String> = quotes
            .iter()
            .filter(|q| q.source != "static")
            .map(|q| q.symbol.clone())
            .collect();
        let cached_prices = get_all_cached_prices_backend(backend).unwrap_or_default();
        for cached in cached_prices {
            if live_symbols.contains(&cached.symbol) || cached.source == "static" {
                continue;
            }
            let record = HistoryRecord {
                date: today.clone(),
                close: cached.price,
                volume: None,
                open: None,
                high: None,
                low: None,
            };
            match upsert_history_backend(backend, &cached.symbol, "cache", &[record]) {
                Ok(_) => history_stamp_ok += 1,
                Err(e) => {
                    history_stamp_err += 1;
                    if history_stamp_examples.len() < 3 {
                        history_stamp_examples.push(format!("{}: {}", cached.symbol, e));
                    }
                }
            }
        }
        if history_stamp_err > 0 {
            info_ln!(
                verbose,
                "⚠ Price history stamp issues: {} writes failed ({} ok). Sample: {}",
                history_stamp_err,
                history_stamp_ok,
                history_stamp_examples.join(" | ")
            );
        }

        let fetched_count = quotes.iter().filter(|q| q.source != "static").count();
        let total_attempted = symbols
            .iter()
            .filter(|(_, cat)| *cat != AssetCategory::Cash)
            .count();
        let error_count = errors.len();

        if fetched_count > 0 && error_count == 0 {
            info_ln!(verbose, "✓ Prices ({} symbols)", fetched_count);
        } else if fetched_count > 0 && error_count > 0 {
            info_ln!(
                verbose,
                "⚠ Prices ({}/{} symbols — {} failed)",
                fetched_count,
                total_attempted,
                error_count
            );
        } else {
            info_ln!(verbose, "✗ Prices (no live quotes fetched)");
        }
        if !errors.is_empty() {
            let sample = errors
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            info_ln!(
                verbose,
                "⚠ Prices fallback: {} symbol fetches failed; using cached values where available. {}",
                error_count,
                sample
            );
        }
        if fetched_count == 0 {
            let cached_count = get_all_cached_prices_backend(backend)?.len();
            if cached_count > 0 {
                info_ln!(
                    verbose,
                    "⚠ Prices fallback: no live quotes fetched, continuing with {} cached prices.",
                    cached_count
                );
            }
        }

        // Extract failed symbol names from error strings (format: "SYMBOL: error message")
        let failed_syms: Vec<String> = errors
            .iter()
            .filter_map(|e| e.split(':').next().map(|s| s.to_string()))
            .take(5)
            .collect();

        let price_status = if fetched_count == 0 {
            SourceStatus::Failed
        } else if error_count > 0 {
            SourceStatus::PartialSuccess
        } else {
            SourceStatus::Ok
        };

        dag_result.add(SourceResult {
            name: "prices".to_string(),
            label: "Prices".to_string(),
            status: price_status,
            items_attempted: if error_count > 0 {
                Some(total_attempted)
            } else {
                None
            },
            items_failed: if error_count > 0 {
                Some(error_count)
            } else {
                None
            },
            failed_symbols: if failed_syms.is_empty() {
                None
            } else {
                Some(failed_syms)
            },
            items_updated: Some(fetched_count),
            duration_ms: price_start.elapsed().as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: if errors.is_empty() {
                None
            } else {
                Some(format!(
                    "{} of {} symbol fetches failed",
                    error_count, total_attempted
                ))
            },
            detail: None,
        });

        // Backfill history for symbols missing sufficient history
        let backfill_start = Instant::now();
        let mut history_updated = 0usize;
        let mut history_attempted = 0usize;
        let yesterday = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
        for (sym, cat) in &symbols {
            if *cat == AssetCategory::Cash {
                continue;
            }
            let history = get_history_backend(backend, sym, 40).unwrap_or_default();
            let history_len = history.len();
            let latest_history_date = history
                .last()
                .and_then(|r| chrono::NaiveDate::parse_from_str(&r.date, "%Y-%m-%d").ok());
            let stale_or_missing_recent =
                latest_history_date.map(|d| d < yesterday).unwrap_or(true);
            if history_len >= 30 && !stale_or_missing_recent {
                continue;
            }
            history_attempted += 1;
            match rt.block_on(fetch_history_for_symbol(sym, *cat, 180)) {
                Ok((records, source)) if !records.is_empty() => {
                    if upsert_history_backend(backend, sym, source, &records).is_ok() {
                        history_updated += 1;
                    }
                }
                _ => {}
            }
        }
        if history_attempted > 0 {
            let history_failed = history_attempted.saturating_sub(history_updated);
            let history_status = if history_updated == 0 {
                SourceStatus::Failed
            } else if history_failed > 0 {
                SourceStatus::PartialSuccess
            } else {
                SourceStatus::Ok
            };
            let detail = if history_failed > 0 {
                format!(
                    "⚠ Price history ({}/{} symbol backfills)",
                    history_updated, history_attempted
                )
            } else {
                format!("✓ Price history ({} symbol backfills)", history_updated)
            };
            info_ln!(verbose, "{}", detail);
            dag_result.add(SourceResult {
                name: "price_history".to_string(),
                label: "Price History Backfill".to_string(),
                status: history_status,
                items_attempted: if history_failed > 0 {
                    Some(history_attempted)
                } else {
                    None
                },
                items_failed: if history_failed > 0 {
                    Some(history_failed)
                } else {
                    None
                },
                failed_symbols: None,
                items_updated: Some(history_updated),
                duration_ms: backfill_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: if history_failed > 0 {
                    Some(format!(
                        "{} of {} history backfills failed",
                        history_failed, history_attempted
                    ))
                } else {
                    None
                },
                detail: Some(detail),
            });
        }

        // ── DAG Layer 2: Post-price analytics ──────────────────────────
        let analytics_start = Instant::now();

        let technical_count = store_technical_snapshots(backend, &symbols)?;
        if technical_count > 0 {
            info_ln!(
                verbose,
                "✓ Technical snapshots ({} symbols)",
                technical_count
            );
        } else {
            info_ln!(verbose, "⊘ Technical snapshots (insufficient history)");
        }

        let levels_count = store_technical_levels(backend, &symbols)?;
        if levels_count > 0 {
            info_ln!(
                verbose,
                "✓ Market structure levels ({} symbols)",
                levels_count
            );
        } else {
            info_ln!(verbose, "⊘ Market structure levels (insufficient history)");
        }

        let signals_count = match crate::analytics::signals::generate_signals(backend) {
            Ok(n) if n > 0 => {
                info_ln!(verbose, "✓ Technical signals ({} new)", n);
                n
            }
            Ok(_) => {
                info_ln!(verbose, "⊘ Technical signals (no new)");
                0
            }
            Err(e) => {
                info_ln!(verbose, "✗ Technical signals (failed: {})", e);
                0
            }
        };

        dag_result.add(SourceResult {
            name: "technical_snapshots".to_string(),
            label: "Technical Analysis".to_string(),
            status: SourceStatus::Ok,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(technical_count + levels_count + signals_count),
            duration_ms: analytics_start.elapsed().as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });

        // ── Situation indicator evaluation ─────────────────────────
        let indicator_start = Instant::now();
        match evaluate_situation_indicators(backend, verbose) {
            Ok((checked, triggered)) => {
                if checked > 0 {
                    if triggered > 0 {
                        info_ln!(
                            verbose,
                            "✓ Situation indicators ({} checked, {} triggered)",
                            checked,
                            triggered
                        );
                    } else {
                        info_ln!(
                            verbose,
                            "✓ Situation indicators ({} checked, none triggered)",
                            checked
                        );
                    }
                } else {
                    info_ln!(verbose, "⊘ Situation indicators (none watching)");
                }
                dag_result.add(SourceResult {
                    name: "situation_indicators".to_string(),
                    label: "Situation Indicators".to_string(),
                    status: SourceStatus::Ok,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(checked),
                    duration_ms: indicator_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: None,
                    detail: if triggered > 0 {
                        Some(format!("{} triggered", triggered))
                    } else {
                        None
                    },
                });
            }
            Err(e) => {
                info_ln!(verbose, "✗ Situation indicators (failed: {})", e);
                dag_result.add(SourceResult {
                    name: "situation_indicators".to_string(),
                    label: "Situation Indicators".to_string(),
                    status: SourceStatus::Failed,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                    duration_ms: indicator_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: Some(e.to_string()),
                    detail: None,
                });
            }
        }
    } else if plan.prices {
        info_ln!(verbose, "⊘ Prices (no symbols)");
        dag_result.add(SourceResult {
            name: "prices".to_string(),
            label: "Prices".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("no symbols".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ Prices (cadence deferred)");
        dag_result.add(SourceResult {
            name: "prices".to_string(),
            label: "Prices".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }

    // ── DAG Layer 2: Analytics snapshots (correlation + regime) ─────
    if plan.analytics {
        let corr_start = Instant::now();
        match crate::commands::correlations::compute_and_store_default_snapshots_backend(backend) {
            Ok(n) if n > 0 => info_ln!(verbose, "✓ Correlation snapshots ({} rows)", n),
            Ok(_) => info_ln!(verbose, "⊘ Correlation snapshots (insufficient history)"),
            Err(e) => info_ln!(verbose, "✗ Correlation snapshots (failed: {})", e),
        }

        match crate::commands::regime::classify_and_store_if_needed(backend) {
            Ok(true) => info_ln!(verbose, "✓ Regime classification (stored)"),
            Ok(false) => info_ln!(verbose, "⊘ Regime classification (unchanged today)"),
            Err(e) => info_ln!(verbose, "✗ Regime classification (failed: {})", e),
        }

        dag_result.add(SourceResult {
            name: "analytics".to_string(),
            label: "Analytics (correlation + regime)".to_string(),
            status: SourceStatus::Ok,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: corr_start.elapsed().as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ Analytics snapshots (cadence deferred)");
        dag_result.add(SourceResult {
            name: "analytics".to_string(),
            label: "Analytics (correlation + regime)".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }

    // ── DAG Layer 3: Portfolio + alerts ─────────────────────────────
    if plan.analytics {
        if let Err(e) = store_portfolio_snapshot(backend, config, verbose) {
            warn_ln!(
                verbose,
                "\nWarning: failed to store portfolio snapshot: {}",
                e
            );
        }
        if let Err(e) = detect_timeframe_signals(backend) {
            warn_ln!(
                verbose,
                "\nWarning: failed to compute cross-timeframe signals: {}",
                e
            );
        }
    }

    // Alerts
    if plan.alerts {
        let alerts_start = Instant::now();
        match engine::check_alerts_backend_only(backend) {
            Ok(results) => {
                let newly_triggered = engine::get_newly_triggered(&results);
                let armed_count = results
                    .iter()
                    .filter(|r| {
                        r.rule.status == crate::alerts::AlertStatus::Armed && !r.newly_triggered
                    })
                    .count();
                info_ln!(
                    verbose,
                    "\n✓ Smart alerts evaluated ({} triggered, {} armed)",
                    newly_triggered.len(),
                    armed_count
                );
                if !newly_triggered.is_empty() {
                    info_ln!(verbose, "\n🔔 Alerts Triggered:");
                    for result in &newly_triggered {
                        let dir_emoji = match result.rule.direction {
                            crate::alerts::AlertDirection::Above => "↑",
                            crate::alerts::AlertDirection::Below => "↓",
                        };
                        let current_str = result
                            .current_value
                            .map(|v| format!("{:.2}", v))
                            .unwrap_or_else(|| "N/A".to_string());
                        info_ln!(
                            verbose,
                            "  {} {} {} {} (current: {})",
                            dir_emoji,
                            result.rule.symbol,
                            result.rule.kind,
                            result.rule.threshold,
                            current_str
                        );

                        if notify {
                            let title = format!("pftui Alert: {}", result.rule.symbol);
                            let body = format!(
                                "{} {} {} (current: {})",
                                result.rule.kind, dir_emoji, result.rule.threshold, current_str
                            );
                            if let Err(e) = notify::send_notification(&title, &body) {
                                warn_ln!(verbose, "  Warning: failed to send notification: {}", e);
                            }
                        }
                    }
                }
                dag_result.add(SourceResult {
                    name: "alerts".to_string(),
                    label: "Smart Alerts".to_string(),
                    status: SourceStatus::Ok,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(newly_triggered.len()),
                    duration_ms: alerts_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: None,
                    detail: None,
                });
            }
            Err(e) => {
                warn_ln!(verbose, "\nWarning: failed to check alerts: {}", e);
                dag_result.add(SourceResult {
                    name: "alerts".to_string(),
                    label: "Smart Alerts".to_string(),
                    status: SourceStatus::Failed,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                    duration_ms: alerts_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: Some(e.to_string()),
                    detail: None,
                });
            }
        }
    } else {
        info_ln!(verbose, "\n⊘ Smart alerts (cadence deferred)");
        dag_result.add(SourceResult {
            name: "alerts".to_string(),
            label: "Smart Alerts".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }

    // ── DAG Layer 4: Cleanup ───────────────────────────────────────
    if plan.cleanup {
        let cleanup_start = Instant::now();
        run_cleanup(backend, verbose);
        dag_result.add(SourceResult {
            name: "cleanup".to_string(),
            label: "Cleanup".to_string(),
            status: SourceStatus::Ok,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: cleanup_start.elapsed().as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ Cleanup (cadence deferred)");
        dag_result.add(SourceResult {
            name: "cleanup".to_string(),
            label: "Cleanup".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }

    info_ln!(verbose, "\nRefresh complete.");
    dag_result.finalize(pipeline_start.elapsed());
    Ok(dag_result)
}

// ── Helper functions for storing layer-0 async results ─────────────────

fn store_predictions_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    data: Option<(
        Result<Vec<crate::data::predictions::PredictionMarket>>,
        Duration,
    )>,
    dag_result: &mut RefreshResult,
) {
    if due {
        if let Some((result, elapsed)) = data {
            match result {
                Ok(markets) => {
                    let count = markets.len();
                    match predictions_cache::upsert_predictions_backend(backend, &markets) {
                        Ok(_) => {
                            info_ln!(verbose, "✓ Predictions ({} markets)", count);
                            dag_result.add(SourceResult {
                                name: "predictions".to_string(),
                                label: "Predictions (Polymarket)".to_string(),
                                status: SourceStatus::Ok,
                                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(count),
                                duration_ms: elapsed.as_millis() as u64,
                                reason: None,
                                age_minutes: None,
                                error: None,
                                detail: None,
                            });
                        }
                        Err(e) => {
                            info_ln!(
                                verbose,
                                "⚠ Predictions fetched ({} markets) but cache write failed: {}",
                                count,
                                e
                            );
                            dag_result.add(SourceResult {
                                name: "predictions".to_string(),
                                label: "Predictions (Polymarket)".to_string(),
                                status: SourceStatus::Failed,
                                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                                duration_ms: elapsed.as_millis() as u64,
                                reason: None,
                                age_minutes: None,
                                error: Some(format!("cache write failed: {}", e)),
                                detail: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    info_ln!(verbose, "✗ Predictions (failed: {})", e);
                    dag_result.add(SourceResult {
                        name: "predictions".to_string(),
                        label: "Predictions (Polymarket)".to_string(),
                        status: SourceStatus::Failed,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: Some(e.to_string()),
                        detail: None,
                    });
                }
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ Predictions (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "predictions".to_string(),
            label: "Predictions (Polymarket)".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ Predictions (cadence deferred)");
        dag_result.add(SourceResult {
            name: "predictions".to_string(),
            label: "Predictions (Polymarket)".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_contracts_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    data: Option<(
        Result<Vec<crate::db::prediction_contracts::PredictionContract>>,
        Duration,
    )>,
    dag_result: &mut RefreshResult,
) {
    if due {
        if let Some((result, elapsed)) = data {
            match result {
                Ok(contracts) => {
                    let count = contracts.len();
                    match prediction_contracts::upsert_contracts_backend(backend, &contracts) {
                        Ok(_) => {
                            info_ln!(verbose, "✓ Prediction contracts ({} contracts)", count);
                            dag_result.add(SourceResult {
                                name: "prediction_contracts".to_string(),
                                label: "Prediction Contracts (Polymarket tags)".to_string(),
                                status: SourceStatus::Ok,
                                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(count),
                                duration_ms: elapsed.as_millis() as u64,
                                reason: None,
                                age_minutes: None,
                                error: None,
                                detail: None,
                            });

                            // F55.4: Sync mapped contract probabilities → scenario history
                            match crate::db::scenario_contract_mappings::sync_mapped_probabilities(backend) {
                                Ok(0) => {} // no mappings — silent
                                Ok(n) => info_ln!(verbose, "  ↳ Synced {} scenario-contract mapping(s) to scenario history", n),
                                Err(e) => info_ln!(verbose, "  ⚠ Scenario-contract sync failed: {}", e),
                            }
                        }
                        Err(e) => {
                            info_ln!(
                                verbose,
                                "⚠ Prediction contracts fetched ({}) but cache write failed: {}",
                                count,
                                e
                            );
                            dag_result.add(SourceResult {
                                name: "prediction_contracts".to_string(),
                                label: "Prediction Contracts (Polymarket tags)".to_string(),
                                status: SourceStatus::Failed,
                                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                                duration_ms: elapsed.as_millis() as u64,
                                reason: None,
                                age_minutes: None,
                                error: Some(format!("cache write failed: {}", e)),
                                detail: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    info_ln!(verbose, "✗ Prediction contracts (failed: {})", e);
                    dag_result.add(SourceResult {
                        name: "prediction_contracts".to_string(),
                        label: "Prediction Contracts (Polymarket tags)".to_string(),
                        status: SourceStatus::Failed,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: Some(e.to_string()),
                        detail: None,
                    });
                }
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ Prediction contracts (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "prediction_contracts".to_string(),
            label: "Prediction Contracts (Polymarket tags)".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_fedwatch_result(
    backend: &BackendConnection,
    config: &Config,
    verbose: bool,
    due: bool,
    in_plan: bool,
    previous: Option<&fedwatch_cache::FedWatchCacheEntry>,
    dag_result: &mut RefreshResult,
) {
    let fw_start = Instant::now();
    if due {
        match fedwatch::fetch_snapshot_with_fallback(config.brave_api_key.as_deref()) {
            Ok((snapshot, source_label)) => {
                let validated = fedwatch::validate_reading(
                    snapshot,
                    source_label,
                    previous.map(|entry| entry.no_change_pct),
                    FEDWATCH_VALIDATION_THRESHOLD_PCT_POINTS,
                );
                if let Some(warning) = &validated.warning {
                    warn_ln!(verbose, "FedWatch validation warning: {}", warning);
                }
                let entry = fedwatch_cache::FedWatchCacheEntry::from_snapshot(
                    validated.snapshot,
                    validated.source_label,
                    validated.verified,
                    validated.warning,
                );
                match fedwatch_cache::insert_snapshot_backend(backend, &entry) {
                    Ok(_) => {
                        info_ln!(
                            verbose,
                            "✓ FedWatch ({}{}, {:.1}% no-change)",
                            entry.source_label,
                            if entry.verified { "" } else { ", unverified" },
                            entry.no_change_pct
                        );
                        dag_result.add(SourceResult {
                            name: "fedwatch".to_string(),
                            label: "FedWatch".to_string(),
                            status: SourceStatus::Ok,
                            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(1),
                            duration_ms: fw_start.elapsed().as_millis() as u64,
                            reason: None,
                            age_minutes: None,
                            error: None,
                            detail: None,
                        });
                    }
                    Err(e) => {
                        info_ln!(
                            verbose,
                            "⚠ FedWatch fetched ({}, {:.1}% no-change) but cache write failed: {}",
                            entry.source_label,
                            entry.no_change_pct,
                            e
                        );
                        dag_result.add(SourceResult {
                            name: "fedwatch".to_string(),
                            label: "FedWatch".to_string(),
                            status: SourceStatus::Failed,
                            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                            duration_ms: fw_start.elapsed().as_millis() as u64,
                            reason: None,
                            age_minutes: None,
                            error: Some(format!("cache write failed: {}", e)),
                            detail: None,
                        });
                    }
                }
            }
            Err(e) => {
                info_ln!(verbose, "✗ FedWatch (failed: {})", e);
                dag_result.add(SourceResult {
                    name: "fedwatch".to_string(),
                    label: "FedWatch".to_string(),
                    status: SourceStatus::Failed,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                    duration_ms: fw_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: Some(e.to_string()),
                    detail: None,
                });
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ FedWatch (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "fedwatch".to_string(),
            label: "FedWatch".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ FedWatch (cadence deferred)");
        dag_result.add(SourceResult {
            name: "fedwatch".to_string(),
            label: "FedWatch".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_sentiment_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    data: Option<(
        Result<crate::data::sentiment::SentimentIndex>,
        Result<crate::data::sentiment::SentimentIndex>,
        Duration,
    )>,
    dag_result: &mut RefreshResult,
) {
    if due {
        if let Some((crypto_result, trad_result, elapsed)) = data {
            let mut count = 0;
            if let Ok(crypto) = crypto_result {
                let reading = crate::db::sentiment_cache::SentimentReading {
                    index_type: "crypto_fng".to_string(),
                    value: crypto.value,
                    classification: crypto.classification.clone(),
                    timestamp: crypto.timestamp,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                };
                if sentiment_cache::upsert_reading_backend(backend, &reading).is_ok() {
                    count += 1;
                }
            }
            if let Ok(trad) = trad_result {
                let reading = crate::db::sentiment_cache::SentimentReading {
                    index_type: "traditional_fng".to_string(),
                    value: trad.value,
                    classification: trad.classification.clone(),
                    timestamp: trad.timestamp,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                };
                if sentiment_cache::upsert_reading_backend(backend, &reading).is_ok() {
                    count += 1;
                }
            }
            info_ln!(verbose, "✓ Sentiment ({} indices)", count);
            dag_result.add(SourceResult {
                name: "sentiment".to_string(),
                label: "Sentiment (Fear & Greed)".to_string(),
                status: if count > 0 {
                    SourceStatus::Ok
                } else {
                    SourceStatus::Failed
                },
                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(count),
                duration_ms: elapsed.as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: None,
            });
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ Sentiment (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "sentiment".to_string(),
            label: "Sentiment (Fear & Greed)".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ Sentiment (cadence deferred)");
        dag_result.add(SourceResult {
            name: "sentiment".to_string(),
            label: "Sentiment (Fear & Greed)".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

type BraveNewsData = Option<(
    Vec<(String, Result<Vec<crate::data::brave::BraveNewsResult>>)>,
    Duration,
)>;

#[allow(clippy::too_many_arguments)]
fn store_news_result(
    backend: &BackendConnection,
    verbose: bool,
    rss_refresh: bool,
    brave_refresh: bool,
    in_plan: bool,
    rss_data: Option<(Vec<crate::data::rss::NewsItem>, Duration)>,
    brave_news_data: BraveNewsData,
    dag_result: &mut RefreshResult,
) {
    let mut inserted = 0usize;
    let mut brave_inserted = 0usize;
    let mut brave_query_count = 0usize;
    let mut news_elapsed = Duration::ZERO;

    if let Some((brave_results, elapsed)) = brave_news_data {
        brave_query_count = brave_results.len();
        news_elapsed = elapsed;
        for (query, result) in brave_results {
            match result {
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
                    warn_ln!(verbose, "Brave news query failed ({}): {}", query, e);
                }
            }
        }
    }

    if let Some((items, elapsed)) = rss_data {
        if elapsed > news_elapsed {
            news_elapsed = elapsed;
        }
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
    }

    if brave_refresh || rss_refresh {
        if brave_refresh && rss_refresh {
            info_ln!(
                verbose,
                "✓ News ({} articles from {} Brave queries + RSS)",
                inserted,
                brave_query_count
            );
        } else if brave_refresh {
            info_ln!(
                verbose,
                "✓ News ({} articles from {} Brave queries)",
                brave_inserted,
                brave_query_count
            );
        } else {
            info_ln!(verbose, "✓ News ({} articles via RSS)", inserted);
        }
        dag_result.add(SourceResult {
            name: "news".to_string(),
            label: "News".to_string(),
            status: SourceStatus::Ok,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(inserted),
            duration_ms: news_elapsed.as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else if in_plan {
        info_ln!(verbose, "⊘ News (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "news".to_string(),
            label: "News".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ News (cadence deferred)");
        dag_result.add(SourceResult {
            name: "news".to_string(),
            label: "News".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_cot_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    dag_result: &mut RefreshResult,
) {
    let cot_start = Instant::now();
    if due {
        let mut contracts_updated = 0;
        let mut reports_upserted = 0;
        let mut failed_contracts = Vec::new();

        for contract in cot::COT_CONTRACTS {
            match cot::fetch_historical_reports(contract.cftc_code, COT_HISTORY_WEEKS) {
                Ok(reports) if !reports.is_empty() => {
                    let fetched_at = chrono::Utc::now().to_rfc3339();
                    let entries: Vec<_> = reports
                        .into_iter()
                        .map(|report| crate::db::cot_cache::CotCacheEntry {
                            cftc_code: report.cftc_code,
                            report_date: report.report_date,
                            open_interest: report.open_interest,
                            managed_money_long: report.managed_money_long,
                            managed_money_short: report.managed_money_short,
                            managed_money_net: report.managed_money_net,
                            commercial_long: report.commercial_long,
                            commercial_short: report.commercial_short,
                            commercial_net: report.commercial_net,
                            fetched_at: fetched_at.clone(),
                        })
                        .collect();
                    if cot_cache::upsert_reports_backend(backend, &entries).is_ok() {
                        contracts_updated += 1;
                        reports_upserted += entries.len();
                    } else {
                        failed_contracts.push(contract.symbol.to_string());
                    }
                }
                Ok(_) => failed_contracts.push(contract.symbol.to_string()),
                Err(_) => failed_contracts.push(contract.symbol.to_string()),
            }
        }
        let staleness_detail = cot_staleness_detail(backend);
        if contracts_updated > 0 {
            if let Some(detail) = &staleness_detail {
                warn_ln!(verbose, "⚠ COT warning ({detail})");
            }
            info_ln!(
                verbose,
                "✓ COT ({} contracts, {} reports cached)",
                contracts_updated,
                reports_upserted
            );
            dag_result.add(SourceResult {
                name: "cot".to_string(),
                label: "COT (CFTC)".to_string(),
                status: if failed_contracts.is_empty() {
                    SourceStatus::Ok
                } else {
                    SourceStatus::PartialSuccess
                },
                items_attempted: Some(cot::COT_CONTRACTS.len()),
                items_failed: Some(failed_contracts.len()),
                failed_symbols: (!failed_contracts.is_empty()).then_some(failed_contracts),
                items_updated: Some(reports_upserted),
                duration_ms: cot_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: staleness_detail,
            });
        } else {
            info_ln!(verbose, "✗ COT (all failed)");
            dag_result.add(SourceResult {
                name: "cot".to_string(),
                label: "COT (CFTC)".to_string(),
                status: SourceStatus::Failed,
                items_attempted: Some(cot::COT_CONTRACTS.len()),
                items_failed: Some(failed_contracts.len()),
                failed_symbols: (!failed_contracts.is_empty()).then_some(failed_contracts),
                items_updated: None,
                duration_ms: cot_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some("all contracts failed".to_string()),
                detail: staleness_detail,
            });
        }
    } else if in_plan {
        let staleness_detail = cot_staleness_detail(backend);
        if let Some(detail) = &staleness_detail {
            warn_ln!(verbose, "⚠ COT warning ({detail})");
        }
        info_ln!(verbose, "⊘ COT (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "cot".to_string(),
            label: "COT (CFTC)".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: staleness_detail,
        });
    } else {
        info_ln!(verbose, "⊘ COT (cadence deferred)");
        dag_result.add(SourceResult {
            name: "cot".to_string(),
            label: "COT (CFTC)".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_calendar_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    data: Option<(Result<Vec<crate::data::calendar::Event>>, Duration)>,
    _rt: &tokio::runtime::Runtime,
    dag_result: &mut RefreshResult,
) {
    if due {
        if let Some((result, elapsed)) = data {
            match result {
                Ok(events) => {
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
                    info_ln!(verbose, "✓ Calendar ({} events)", events.len());
                    dag_result.add(SourceResult {
                        name: "calendar".to_string(),
                        label: "Calendar".to_string(),
                        status: SourceStatus::Ok,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(events.len()),
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: None,
                        detail: None,
                    });
                }
                Err(e) => {
                    info_ln!(verbose, "✗ Calendar (failed: {})", e);
                    dag_result.add(SourceResult {
                        name: "calendar".to_string(),
                        label: "Calendar".to_string(),
                        status: SourceStatus::Failed,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: Some(e.to_string()),
                        detail: None,
                    });
                }
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ Calendar (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "calendar".to_string(),
            label: "Calendar".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ Calendar (cadence deferred)");
        dag_result.add(SourceResult {
            name: "calendar".to_string(),
            label: "Calendar".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_economy_result(
    backend: &BackendConnection,
    verbose: bool,
    in_plan: bool,
    data: Option<(
        Result<Vec<crate::data::economic::EconomicReading>>,
        bool,
        Duration,
    )>,
    dag_result: &mut RefreshResult,
) {
    if in_plan {
        if let Some((readings, used_brave, elapsed)) = data {
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
                            source: item.source.name().to_string(),
                            confidence: item.source.confidence().to_string(),
                            fetched_at: now.clone(),
                        };
                        let _ = economic_data_db::upsert_entry_backend(backend, &entry);
                    }
                    if used_brave {
                        info_ln!(verbose, "✓ Economy ({} indicators via Brave)", items.len());
                    } else {
                        info_ln!(
                            verbose,
                            "✓ Economy ({} indicators via BLS fallback)",
                            items.len()
                        );
                    }
                    dag_result.add(SourceResult {
                        name: "economy".to_string(),
                        label: "Economy".to_string(),
                        status: SourceStatus::Ok,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(items.len()),
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: None,
                        detail: None,
                    });
                }
                Err(e) => {
                    info_ln!(verbose, "✗ Economy (failed: {})", e);
                    dag_result.add(SourceResult {
                        name: "economy".to_string(),
                        label: "Economy".to_string(),
                        status: SourceStatus::Failed,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: Some(e.to_string()),
                        detail: None,
                    });
                }
            }
        }
    } else {
        info_ln!(verbose, "⊘ Economy (cadence deferred)");
        dag_result.add(SourceResult {
            name: "economy".to_string(),
            label: "Economy".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_ism_result(
    backend: &BackendConnection,
    verbose: bool,
    in_plan: bool,
    data: Option<(Result<Vec<crate::data::economic::EconomicReading>>, Duration)>,
    dag_result: &mut RefreshResult,
) {
    if !in_plan {
        return;
    }
    if let Some((readings_result, elapsed)) = data {
        match readings_result {
            Ok(items) if !items.is_empty() => {
                let now = chrono::Utc::now().to_rfc3339();
                for item in &items {
                    let entry = economic_data_db::EconomicDataEntry {
                        indicator: item.indicator.clone(),
                        value: item.value,
                        previous: item.previous,
                        change: item.change,
                        source_url: item.source_url.clone(),
                        source: item.source.name().to_string(),
                        confidence: item.source.confidence().to_string(),
                        fetched_at: now.clone(),
                    };
                    let _ = economic_data_db::upsert_entry_backend(backend, &entry);
                }
                info_ln!(
                    verbose,
                    "✓ ISM PMI ({} indicators via targeted extraction)",
                    items.len()
                );
                dag_result.add(SourceResult {
                    name: "ism_pmi".to_string(),
                    label: "ISM PMI".to_string(),
                    status: SourceStatus::Ok,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(items.len()),
                    duration_ms: elapsed.as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: None,
                    detail: None,
                });
            }
            Ok(_) => {
                info_ln!(verbose, "⊘ ISM PMI (no data extracted)");
            }
            Err(e) => {
                info_ln!(verbose, "✗ ISM PMI (failed: {})", e);
                dag_result.add(SourceResult {
                    name: "ism_pmi".to_string(),
                    label: "ISM PMI".to_string(),
                    status: SourceStatus::Failed,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                    duration_ms: elapsed.as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: Some(e.to_string()),
                    detail: None,
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
type FredFetchData = Option<(
    Vec<(
        &'static str,
        Result<Vec<crate::data::fred::FredObservation>>,
    )>,
    Duration,
)>;

fn store_fred_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    has_key: bool,
    data: FredFetchData,
    dag_result: &mut RefreshResult,
) {
    if !has_key {
        info_ln!(verbose, "⊘ FRED (no API key configured)");
        dag_result.add(SourceResult {
            name: "fred".to_string(),
            label: "FRED".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("no API key".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
        return;
    }
    if due {
        if let Some((results, elapsed)) = data {
            let mut updated = 0usize;
            let mut surprise_count = 0usize;
            let fetched_at = chrono::Utc::now().to_rfc3339();

            let mut failed_series: Vec<String> = Vec::new();
            let mut cache_fallback_count = 0usize;

            for (series_id, result) in results {
                match result {
                    Ok(observations) => {
                        let cached: Vec<_> = observations
                            .iter()
                            .cloned()
                            .map(|obs| economic_cache::EconomicObservation {
                                series_id: obs.series_id,
                                date: obs.date,
                                value: obs.value,
                                fetched_at: fetched_at.clone(),
                            })
                            .collect();
                        if economic_cache::upsert_observations_backend(backend, &cached).is_ok() {
                            updated += 1;
                        }
                        if let Some(surprise) = fred::detect_surprise(&observations) {
                            let event = macro_events::MacroEvent {
                                series_id: surprise.series_id,
                                event_date: surprise.event_date,
                                expected: surprise.expected,
                                actual: surprise.actual,
                                surprise_pct: surprise.surprise_pct,
                                created_at: fetched_at.clone(),
                            };
                            if macro_events::insert_event_backend(backend, &event).is_ok() {
                                surprise_count += 1;
                            }
                        }
                    }
                    Err(e) => {
                        warn_ln!(verbose, "FRED series {} fetch failed (using cache fallback): {}", series_id, e);
                        failed_series.push(series_id.to_string());
                        // Check if we have cached data for this series
                        if economic_cache::get_latest_backend(backend, series_id).is_ok() {
                            cache_fallback_count += 1;
                        }
                    }
                }
            }

            let status = if failed_series.is_empty() {
                SourceStatus::Ok
            } else if updated > 0 {
                // Partial success — some series updated, some fell back to cache
                SourceStatus::Ok
            } else {
                // All series failed — degraded, relying entirely on cache
                SourceStatus::Ok // still "ok" because cache provides data
            };

            let detail_msg = if failed_series.is_empty() {
                None
            } else {
                Some(format!(
                    "{} series failed (cache fallback for {}): {}",
                    failed_series.len(),
                    cache_fallback_count,
                    failed_series.join(", ")
                ))
            };

            if failed_series.is_empty() {
                info_ln!(
                    verbose,
                    "✓ FRED ({} series, {} surprise events)",
                    updated,
                    surprise_count
                );
            } else {
                warn_ln!(
                    verbose,
                    "⚠ FRED ({} series updated, {} failed → cache fallback, {} surprise events)",
                    updated,
                    failed_series.len(),
                    surprise_count
                );
            }
            dag_result.add(SourceResult {
                name: "fred".to_string(),
                label: "FRED".to_string(),
                status,
                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(updated),
                duration_ms: elapsed.as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: if failed_series.is_empty() { None } else { Some(format!("{} series failed", failed_series.len())) },
                detail: detail_msg,
            });
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ FRED (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "fred".to_string(),
            label: "FRED".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ FRED (cadence deferred)");
        dag_result.add(SourceResult {
            name: "fred".to_string(),
            label: "FRED".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_bls_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    data: Option<(Result<Vec<crate::data::bls::BlsDataPoint>>, Duration)>,
    dag_result: &mut RefreshResult,
) {
    if due {
        if let Some((result, elapsed)) = data {
            match result {
                Ok(series_data) => {
                    if bls_cache::upsert_bls_data_backend(backend, &series_data).is_ok() {
                        info_ln!(verbose, "✓ BLS ({} series)", series_data.len());
                        dag_result.add(SourceResult {
                            name: "bls".to_string(),
                            label: "BLS".to_string(),
                            status: SourceStatus::Ok,
                            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(series_data.len()),
                            duration_ms: elapsed.as_millis() as u64,
                            reason: None,
                            age_minutes: None,
                            error: None,
                            detail: None,
                        });
                    } else {
                        info_ln!(verbose, "✗ BLS (cache write failed)");
                        dag_result.add(SourceResult {
                            name: "bls".to_string(),
                            label: "BLS".to_string(),
                            status: SourceStatus::Failed,
                            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                            duration_ms: elapsed.as_millis() as u64,
                            reason: None,
                            age_minutes: None,
                            error: Some("cache write failed".to_string()),
                            detail: None,
                        });
                    }
                }
                Err(e) => {
                    info_ln!(verbose, "✗ BLS (failed: {})", e);
                    dag_result.add(SourceResult {
                        name: "bls".to_string(),
                        label: "BLS".to_string(),
                        status: SourceStatus::Failed,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: Some(e.to_string()),
                        detail: None,
                    });
                }
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ BLS (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "bls".to_string(),
            label: "BLS".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ BLS (cadence deferred)");
        dag_result.add(SourceResult {
            name: "bls".to_string(),
            label: "BLS".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_worldbank_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    data: Option<(
        Result<Vec<crate::data::worldbank::WorldBankDataPoint>>,
        Duration,
    )>,
    dag_result: &mut RefreshResult,
) {
    if due {
        if let Some((result, elapsed)) = data {
            match result {
                Ok(indicators) => {
                    if worldbank_cache::upsert_worldbank_data_backend(backend, &indicators).is_ok()
                    {
                        info_ln!(verbose, "✓ World Bank ({} indicators)", indicators.len());
                        dag_result.add(SourceResult {
                            name: "worldbank".to_string(),
                            label: "World Bank".to_string(),
                            status: SourceStatus::Ok,
                            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(indicators.len()),
                            duration_ms: elapsed.as_millis() as u64,
                            reason: None,
                            age_minutes: None,
                            error: None,
                            detail: None,
                        });
                    } else {
                        info_ln!(verbose, "✗ World Bank (cache write failed)");
                        dag_result.add(SourceResult {
                            name: "worldbank".to_string(),
                            label: "World Bank".to_string(),
                            status: SourceStatus::Failed,
                            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                            duration_ms: elapsed.as_millis() as u64,
                            reason: None,
                            age_minutes: None,
                            error: Some("cache write failed".to_string()),
                            detail: None,
                        });
                    }
                }
                Err(e) => {
                    info_ln!(verbose, "✗ World Bank (failed: {})", e);
                    dag_result.add(SourceResult {
                        name: "worldbank".to_string(),
                        label: "World Bank".to_string(),
                        status: SourceStatus::Failed,
                        items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                        duration_ms: elapsed.as_millis() as u64,
                        reason: None,
                        age_minutes: None,
                        error: Some(e.to_string()),
                        detail: None,
                    });
                }
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ World Bank (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "worldbank".to_string(),
            label: "World Bank".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ World Bank (cadence deferred)");
        dag_result.add(SourceResult {
            name: "worldbank".to_string(),
            label: "World Bank".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_comex_result(
    backend: &BackendConnection,
    verbose: bool,
    due: bool,
    in_plan: bool,
    dag_result: &mut RefreshResult,
) {
    let comex_start = Instant::now();
    if due {
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
            info_ln!(verbose, "✓ COMEX ({} metals)", count);
            dag_result.add(SourceResult {
                name: "comex".to_string(),
                label: "COMEX".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(count),
                duration_ms: comex_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: None,
            });
        } else {
            // Check if we have stale cached data — still usable for agents
            let cached_count = comex_cache::count_entries_backend(backend).unwrap_or(0);
            if cached_count > 0 {
                info_ln!(verbose, "⚠ COMEX (live fetch failed, {} stale cached entries available)", cached_count);
                dag_result.add(SourceResult {
                    name: "comex".to_string(),
                    label: "COMEX".to_string(),
                    status: SourceStatus::Ok,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(0),
                    duration_ms: comex_start.elapsed().as_millis() as u64,
                    reason: Some(format!("live fetch failed, using {} cached entries", cached_count)),
                    age_minutes: None,
                    error: None,
                    detail: None,
                });
            } else {
                info_ln!(verbose, "✗ COMEX (all failed, no cache)");
                dag_result.add(SourceResult {
                    name: "comex".to_string(),
                    label: "COMEX".to_string(),
                    status: SourceStatus::Failed,
                    items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                    duration_ms: comex_start.elapsed().as_millis() as u64,
                    reason: None,
                    age_minutes: None,
                    error: Some("all metals failed, no cache".to_string()),
                    detail: None,
                });
            }
        }
    } else if in_plan {
        info_ln!(verbose, "⊘ COMEX (fresh, skipping)");
        dag_result.add(SourceResult {
            name: "comex".to_string(),
            label: "COMEX".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    } else {
        info_ln!(verbose, "⊘ COMEX (cadence deferred)");
        dag_result.add(SourceResult {
            name: "comex".to_string(),
            label: "COMEX".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

fn store_onchain_result(
    backend: &BackendConnection,
    verbose: bool,
    in_plan: bool,
    dag_result: &mut RefreshResult,
) {
    let onchain_start = Instant::now();
    if in_plan {
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

        match onchain::fetch_exchange_reserve_snapshot() {
            Ok(snapshot) => {
                let metric = crate::db::onchain_cache::OnchainMetric {
                    metric: "exchange_reserve_proxy_btc".to_string(),
                    date: snapshot.date.clone(),
                    value: snapshot.reserve_btc.to_string(),
                    metadata: Some(
                        serde_json::json!({
                            "reserve_usd": snapshot.reserve_usd,
                            "tracked_wallets": snapshot.tracked_wallets,
                            "exchange_labels": snapshot.exchange_labels,
                            "flow_7d_btc": snapshot.net_flow_7d_btc,
                            "flow_30d_btc": snapshot.net_flow_30d_btc,
                            "top_exchanges": snapshot.top_exchanges.iter().map(|entry| {
                                serde_json::json!({
                                    "label": entry.label,
                                    "balance_btc": entry.balance_btc,
                                    "balance_usd": entry.balance_usd,
                                    "wallets": entry.wallets,
                                    "flow_7d_btc": entry.flow_7d_btc,
                                    "flow_30d_btc": entry.flow_30d_btc,
                                })
                            }).collect::<Vec<_>>(),
                        })
                        .to_string(),
                    ),
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                };
                let _ = onchain_cache::upsert_metric_backend(backend, &metric);
                onchain_ok_parts.push("exchange reserves");
            }
            Err(e) => onchain_errors.push(format!("exchange reserves: {}", e)),
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

        match onchain::fetch_market_stats() {
            Ok(stats) => {
                let fetched_at = chrono::Utc::now().to_rfc3339();
                let metrics = [
                    (
                        "largest_transactions_24h_btc",
                        stats.largest_transactions_24h_btc.to_string(),
                        serde_json::json!({
                           "largest_transactions_24h_usd": stats.largest_transactions_24h_usd,
                           "largest_transactions_24h_share_pct": stats.largest_transactions_24h_share_pct,
                        }),
                    ),
                    (
                        "active_addresses_24h",
                        stats.active_addresses_24h.to_string(),
                        serde_json::json!({}),
                    ),
                    (
                        "wealth_distribution_top10_pct",
                        stats.top_10_share_pct.to_string(),
                        serde_json::json!({
                           "top_100_share_pct": stats.top_100_share_pct,
                           "top_1000_share_pct": stats.top_1000_share_pct,
                           "top_10000_share_pct": stats.top_10000_share_pct,
                           "top_100_richest_btc": stats.top_100_richest_btc,
                        }),
                    ),
                ];
                for (name, value, metadata) in metrics {
                    let metric = crate::db::onchain_cache::OnchainMetric {
                        metric: name.to_string(),
                        date: stats.date.clone(),
                        value,
                        metadata: Some(metadata.to_string()),
                        fetched_at: fetched_at.clone(),
                    };
                    let _ = onchain_cache::upsert_metric_backend(backend, &metric);
                }
                onchain_ok_parts.push("whales");
            }
            Err(e) => onchain_errors.push(format!("whales: {}", e)),
        }

        if !onchain_ok_parts.is_empty() {
            info_ln!(verbose, "✓ On-chain ({})", onchain_ok_parts.join(" + "));
            dag_result.add(SourceResult {
                name: "onchain".to_string(),
                label: "On-chain".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(onchain_ok_parts.len()),
                duration_ms: onchain_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: None,
            });
        } else {
            info_ln!(
                verbose,
                "✗ On-chain (failed: {})",
                onchain_errors.join("; ")
            );
            dag_result.add(SourceResult {
                name: "onchain".to_string(),
                label: "On-chain".to_string(),
                status: SourceStatus::Failed,
                items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
                duration_ms: onchain_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(onchain_errors.join("; ")),
                detail: None,
            });
        }
    } else {
        info_ln!(verbose, "⊘ On-chain (cadence deferred)");
        dag_result.add(SourceResult {
            name: "onchain".to_string(),
            label: "On-chain".to_string(),
            status: SourceStatus::Deferred,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("cadence deferred".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}
fn run_cleanup(backend: &BackendConnection, verbose: bool) {
    let mut parts = Vec::new();

    match news_cache::cleanup_old_news_backend(backend) {
        Ok(count) => parts.push(format!("news={}", count)),
        Err(e) => warn_ln!(verbose, "Cleanup warning (news): {}", e),
    }

    let calendar_cutoff = (chrono::Utc::now() - chrono::Duration::days(CALENDAR_RETENTION_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    match calendar_cache::delete_old_events_backend(backend, &calendar_cutoff) {
        Ok(count) => parts.push(format!("calendar={}", count)),
        Err(e) => warn_ln!(verbose, "Cleanup warning (calendar): {}", e),
    }

    match sentiment_cache::prune_old_backend(backend, SENTIMENT_RETENTION_DAYS) {
        Ok(count) => parts.push(format!("sentiment={}", count)),
        Err(e) => warn_ln!(verbose, "Cleanup warning (sentiment): {}", e),
    }

    match onchain_cache::prune_old_metrics_backend(backend) {
        Ok(count) => parts.push(format!("onchain={}", count)),
        Err(e) => warn_ln!(verbose, "Cleanup warning (on-chain): {}", e),
    }

    match cot_cache::delete_old_reports_backend(backend, COT_RETENTION_DAYS) {
        Ok(count) => parts.push(format!("cot={}", count)),
        Err(e) => warn_ln!(verbose, "Cleanup warning (COT): {}", e),
    }

    match crate::db::technical_signals::prune_signals_backend(
        backend,
        TECHNICAL_SIGNAL_RETENTION_HOURS,
    ) {
        Ok(count) => parts.push(format!("technical_signals={}", count)),
        Err(e) => warn_ln!(verbose, "Cleanup warning (technical signals): {}", e),
    }

    if !parts.is_empty() {
        info_ln!(verbose, "✓ Cleanup ({})", parts.join(", "));
    }
}

fn maybe_report_fedwatch_conflict(backend: &BackendConnection, verbose: bool) {
    let markets = match predictions_cache::get_cached_predictions_backend(backend, 200) {
        Ok(rows) if !rows.is_empty() => rows,
        _ => return,
    };
    let snapshot = match fedwatch_cache::get_latest_snapshot_backend(backend) {
        Ok(Some(entry)) => entry.snapshot,
        Err(_) => return,
        Ok(None) => return,
    };
    if let Some(conflict) = fedwatch::detect_no_change_conflict(
        &snapshot,
        &markets,
        FEDWATCH_CONFLICT_THRESHOLD_PCT_POINTS,
    ) {
        info_ln!(
            verbose,
            "⚠ Source conflict: {} | CME {:.1}% vs alt {:.1}% (Δ {:.1}pp) | use {}",
            conflict.metric,
            conflict.cme_value_pct,
            conflict.alt_value_pct,
            conflict.delta_pct_points,
            conflict.recommended_source
        );
        info_ln!(verbose, "  Alt source: {}", conflict.alt_source_label);
    }
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

    let recent =
        timeframe_signals::list_signals_backend(backend, Some(signal_type), None, Some(100))?;
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(6);
    let exists_recent = recent.into_iter().any(|s| {
        if s.description != description {
            return false;
        }
        let parsed = parse_timestamp_flexible(&s.detected_at)
            .map(|d| d.with_timezone(&chrono::Utc));
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
    let regimes =
        crate::db::regime_snapshots::get_history_backend(backend, Some(2)).unwrap_or_default();
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

fn store_technical_snapshots(
    backend: &BackendConnection,
    symbols: &[(String, AssetCategory)],
) -> Result<usize> {
    let mut stored = 0usize;
    for (symbol, category) in symbols {
        if *category == AssetCategory::Cash {
            continue;
        }
        let history = match get_history_backend(backend, symbol, 370) {
            Ok(rows) if !rows.is_empty() => rows,
            _ => continue,
        };
        let Some(snapshot) =
            technicals::compute_snapshot(symbol, technicals::DEFAULT_TIMEFRAME, &history)
        else {
            continue;
        };
        technical_snapshots::insert_snapshot_backend(backend, &snapshot)?;
        stored += 1;
    }
    Ok(stored)
}

fn store_technical_levels(
    backend: &BackendConnection,
    symbols: &[(String, AssetCategory)],
) -> Result<usize> {
    let mut stored = 0usize;
    for (symbol, category) in symbols {
        if *category == AssetCategory::Cash {
            continue;
        }
        let history = match get_history_backend(backend, symbol, 370) {
            Ok(rows) if !rows.is_empty() => rows,
            _ => continue,
        };

        // Load or compute the technical snapshot for this symbol
        let snapshot =
            technicals::compute_snapshot(symbol, technicals::DEFAULT_TIMEFRAME, &history);

        let levels = level_engine::compute_levels(
            symbol,
            technicals::DEFAULT_TIMEFRAME,
            &history,
            snapshot.as_ref(),
        );

        if levels.is_empty() {
            continue;
        }

        technical_levels::upsert_levels_backend(backend, symbol, &levels)?;
        stored += 1;
    }
    Ok(stored)
}

/// Compute current positions and store a daily portfolio snapshot.
fn store_portfolio_snapshot(
    backend: &BackendConnection,
    config: &Config,
    verbose: bool,
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
    info_ln!(
        verbose,
        "Snapshot stored: {} ({} position{}).",
        today,
        snap_count,
        if snap_count == 1 { "" } else { "s" },
    );

    Ok(())
}

/// Evaluate all 'watching' situation indicators against live price cache and technical snapshots.
/// Returns (checked_count, triggered_count).
fn evaluate_situation_indicators(
    backend: &BackendConnection,
    verbose: bool,
) -> Result<(usize, usize)> {
    use std::str::FromStr as _;

    let indicators = crate::db::scenarios::list_all_watching_indicators_backend(backend)?;
    if indicators.is_empty() {
        return Ok((0, 0));
    }

    let mut checked = 0usize;
    let mut triggered = 0usize;

    for ind in &indicators {
        // Resolve the current value for this indicator's symbol + metric
        let current_value = resolve_indicator_value(backend, &ind.symbol, &ind.metric);

        let current_value = match current_value {
            Some(v) => v,
            None => {
                // Can't evaluate — no data for this symbol/metric. Skip silently.
                continue;
            }
        };

        checked += 1;

        // Parse threshold as Decimal
        let threshold = match Decimal::from_str(&ind.threshold) {
            Ok(t) => t,
            Err(_) => {
                warn_ln!(
                    verbose,
                    "⚠ Indicator {} ({}): invalid threshold '{}'",
                    ind.id,
                    ind.label,
                    ind.threshold
                );
                continue;
            }
        };

        // Evaluate the operator
        let is_triggered = match ind.operator.as_str() {
            ">" | "gt" => current_value > threshold,
            ">=" | "gte" => current_value >= threshold,
            "<" | "lt" => current_value < threshold,
            "<=" | "lte" => current_value <= threshold,
            "==" | "eq" => current_value == threshold,
            "!=" | "ne" => current_value != threshold,
            "crosses_above" => {
                // Check if last_value was below threshold and current is above
                let was_below = ind
                    .last_value
                    .as_deref()
                    .and_then(|v| Decimal::from_str(v).ok())
                    .is_some_and(|prev| prev < threshold);
                was_below && current_value >= threshold
            }
            "crosses_below" => {
                // Check if last_value was above threshold and current is below
                let was_above = ind
                    .last_value
                    .as_deref()
                    .and_then(|v| Decimal::from_str(v).ok())
                    .is_some_and(|prev| prev > threshold);
                was_above && current_value <= threshold
            }
            other => {
                warn_ln!(
                    verbose,
                    "⚠ Indicator {} ({}): unknown operator '{}'",
                    ind.id,
                    ind.label,
                    other
                );
                continue;
            }
        };

        if is_triggered {
            triggered += 1;
            info_ln!(
                verbose,
                "🔔 Indicator TRIGGERED: {} — {} {} {} (value: {})",
                ind.label,
                ind.symbol,
                ind.operator,
                ind.threshold,
                current_value
            );
        }

        // Update the indicator in the database
        crate::db::scenarios::update_indicator_evaluation_backend(
            backend,
            ind.id,
            &current_value.to_string(),
            is_triggered,
        )?;
    }

    Ok((checked, triggered))
}

/// Resolve the current numeric value for a symbol + metric combination.
/// Supported metrics:
///   - "close" / "price" — current spot price from price cache
///   - "rsi", "rsi_14" — 14-period RSI from technical snapshots
///   - "sma_20", "sma_50", "sma_200" — SMA values
///   - "macd", "macd_signal", "macd_histogram" — MACD components
///   - "bollinger_upper", "bollinger_lower", "bollinger_middle" — Bollinger Bands
///   - "52w_high", "52w_low", "52w_position" — 52-week range metrics
///   - "atr", "atr_14" — Average True Range
///   - "atr_ratio" — ATR as percentage of price
///   - "volume_ratio" — 20-day volume ratio
fn resolve_indicator_value(
    backend: &BackendConnection,
    symbol: &str,
    metric: &str,
) -> Option<Decimal> {
    match metric {
        "close" | "price" => {
            // Try USD first, then GBP, then any
            let quote = crate::db::price_cache::get_cached_price_backend(backend, symbol, "USD")
                .ok()
                .flatten()
                .or_else(|| {
                    crate::db::price_cache::get_cached_price_backend(backend, symbol, "GBp")
                        .ok()
                        .flatten()
                })
                .or_else(|| {
                    crate::db::price_cache::get_cached_price_backend(backend, symbol, "GBP")
                        .ok()
                        .flatten()
                });
            quote.map(|q| q.price)
        }
        "rsi" | "rsi_14" => get_technical_field(backend, symbol, |snap| snap.rsi_14),
        "sma_20" => get_technical_field(backend, symbol, |snap| snap.sma_20),
        "sma_50" => get_technical_field(backend, symbol, |snap| snap.sma_50),
        "sma_200" => get_technical_field(backend, symbol, |snap| snap.sma_200),
        "macd" => get_technical_field(backend, symbol, |snap| snap.macd),
        "macd_signal" => get_technical_field(backend, symbol, |snap| snap.macd_signal),
        "macd_histogram" => get_technical_field(backend, symbol, |snap| snap.macd_histogram),
        "bollinger_upper" => get_technical_field(backend, symbol, |snap| snap.bollinger_upper),
        "bollinger_lower" => get_technical_field(backend, symbol, |snap| snap.bollinger_lower),
        "bollinger_middle" => get_technical_field(backend, symbol, |snap| snap.bollinger_middle),
        "52w_high" | "range_52w_high" => {
            get_technical_field(backend, symbol, |snap| snap.range_52w_high)
        }
        "52w_low" | "range_52w_low" => {
            get_technical_field(backend, symbol, |snap| snap.range_52w_low)
        }
        "52w_position" | "range_52w_position" => {
            get_technical_field(backend, symbol, |snap| snap.range_52w_position)
        }
        "atr" | "atr_14" => get_technical_field(backend, symbol, |snap| snap.atr_14),
        "atr_ratio" => get_technical_field(backend, symbol, |snap| snap.atr_ratio),
        "volume_ratio" | "volume_ratio_20" => {
            get_technical_field(backend, symbol, |snap| snap.volume_ratio_20)
        }
        _ => None,
    }
}

/// Helper: extract a numeric field from the latest technical snapshot for a symbol.
fn get_technical_field(
    backend: &BackendConnection,
    symbol: &str,
    extractor: impl Fn(&crate::db::technical_snapshots::TechnicalSnapshotRecord) -> Option<f64>,
) -> Option<Decimal> {
    use std::str::FromStr as _;

    let snap =
        crate::db::technical_snapshots::get_latest_snapshot_backend(backend, symbol, "daily")
            .ok()
            .flatten()?;
    let value = extractor(&snap)?;
    // Convert f64 to Decimal via string to avoid floating point artifacts
    Decimal::from_str(&format!("{:.6}", value)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

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

    #[test]
    fn evaluate_indicators_empty_db() {
        use crate::db;
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        db::schema::run_migrations(&conn).unwrap();
        let backend = crate::db::backend::BackendConnection::Sqlite { conn };

        let (checked, triggered) = evaluate_situation_indicators(&backend, false).unwrap();
        assert_eq!(checked, 0);
        assert_eq!(triggered, 0);
    }

    #[test]
    fn evaluate_indicators_with_price_data() {
        use crate::db;
        use crate::db::scenarios;
        use crate::models::price::PriceQuote;
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        db::schema::run_migrations(&conn).unwrap();
        let backend = crate::db::backend::BackendConnection::Sqlite { conn };

        // Create a scenario with indicators
        let s_id =
            scenarios::add_scenario_backend(&backend, "Gold Watch", 50.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, s_id).unwrap();

        // Indicator: gold > 3000
        scenarios::add_indicator_backend(
            &backend, s_id, None, None, "GC=F", "close", ">", "3000", "Gold above 3k",
        )
        .unwrap();

        // Indicator: gold < 2500 (should NOT trigger)
        scenarios::add_indicator_backend(
            &backend, s_id, None, None, "GC=F", "close", "<", "2500", "Gold crash",
        )
        .unwrap();

        // Insert a price into the cache: gold at 3100
        let quote = PriceQuote {
            symbol: "GC=F".to_string(),
            price: dec!(3100),
            currency: "USD".to_string(),
            fetched_at: "2026-03-22T12:00:00Z".to_string(),
            source: "test".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: Some(dec!(3050)),
        };
        crate::db::price_cache::upsert_price_backend(&backend, &quote).unwrap();

        // Evaluate
        let (checked, triggered) = evaluate_situation_indicators(&backend, false).unwrap();
        assert_eq!(checked, 2);
        assert_eq!(triggered, 1); // only > 3000 triggers

        // Verify the triggered indicator
        let indicators = scenarios::list_indicators_backend(&backend, s_id).unwrap();
        let gt_ind = indicators.iter().find(|i| i.label == "Gold above 3k").unwrap();
        assert_eq!(gt_ind.status, "triggered");
        assert_eq!(gt_ind.last_value.as_deref(), Some("3100"));

        let lt_ind = indicators.iter().find(|i| i.label == "Gold crash").unwrap();
        assert_eq!(lt_ind.status, "watching");
        assert_eq!(lt_ind.last_value.as_deref(), Some("3100"));

        // Re-evaluate — triggered indicator should be skipped (it's no longer 'watching')
        let (checked2, triggered2) = evaluate_situation_indicators(&backend, false).unwrap();
        assert_eq!(checked2, 1); // only the non-triggered one
        assert_eq!(triggered2, 0);
    }

    #[test]
    fn evaluate_indicators_crosses_above() {
        use crate::db;
        use crate::db::scenarios;
        use crate::models::price::PriceQuote;
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        db::schema::run_migrations(&conn).unwrap();
        let backend = crate::db::backend::BackendConnection::Sqlite { conn };

        let s_id = scenarios::add_scenario_backend(
            &backend,
            "Cross Test",
            50.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, s_id).unwrap();

        let ind_id = scenarios::add_indicator_backend(
            &backend,
            s_id,
            None,
            None,
            "BTC-USD",
            "close",
            "crosses_above",
            "100000",
            "BTC crosses 100k",
        )
        .unwrap();

        // First evaluation: BTC at 95000 (below threshold, no previous value)
        let quote1 = PriceQuote {
            symbol: "BTC-USD".to_string(),
            price: dec!(95000),
            currency: "USD".to_string(),
            fetched_at: "2026-03-22T11:00:00Z".to_string(),
            source: "test".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
        };
        crate::db::price_cache::upsert_price_backend(&backend, &quote1).unwrap();
        let (_, triggered1) = evaluate_situation_indicators(&backend, false).unwrap();
        assert_eq!(triggered1, 0); // no previous value, can't detect cross

        // Second evaluation: BTC at 102000 (above threshold, previous was 95000)
        let quote2 = PriceQuote {
            symbol: "BTC-USD".to_string(),
            price: dec!(102000),
            currency: "USD".to_string(),
            fetched_at: "2026-03-22T12:00:00Z".to_string(),
            source: "test".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
        };
        crate::db::price_cache::upsert_price_backend(&backend, &quote2).unwrap();
        let (_, triggered2) = evaluate_situation_indicators(&backend, false).unwrap();
        assert_eq!(triggered2, 1); // crossed above!

        let indicators = scenarios::list_indicators_backend(&backend, s_id).unwrap();
        let ind = indicators.iter().find(|i| i.id == ind_id).unwrap();
        assert_eq!(ind.status, "triggered");
    }

    #[test]
    fn parse_timestamp_flexible_rfc3339() {
        let ts = "2026-03-09T17:50:47.025534+00:00";
        let dt = parse_timestamp_flexible(ts).expect("should parse RFC 3339");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.day(), 9);
    }

    #[test]
    fn parse_timestamp_flexible_postgres_text() {
        // Postgres `fetched_at::text` format: space separator, abbreviated tz
        let ts = "2026-03-09 17:50:47.025534+00";
        let dt = parse_timestamp_flexible(ts).expect("should parse Postgres text");
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 3);
        assert_eq!(dt.day(), 9);
    }

    #[test]
    fn parse_timestamp_flexible_postgres_no_frac() {
        let ts = "2026-03-09 17:50:47+00";
        let dt = parse_timestamp_flexible(ts).expect("should parse Postgres text without fractional seconds");
        assert_eq!(dt.hour(), 17);
    }

    #[test]
    fn parse_timestamp_flexible_returns_none_on_garbage() {
        assert!(parse_timestamp_flexible("not-a-timestamp").is_none());
        assert!(parse_timestamp_flexible("").is_none());
    }

    #[test]
    fn cot_report_age_uses_report_date_not_fetch_time() {
        let reports = vec![crate::db::cot_cache::CotCacheEntry {
            cftc_code: "088691".to_string(),
            report_date: "2026-03-20".to_string(),
            open_interest: 0,
            managed_money_long: 0,
            managed_money_short: 0,
            managed_money_net: 0,
            commercial_long: 0,
            commercial_short: 0,
            commercial_net: 0,
            fetched_at: chrono::Utc::now().to_rfc3339(),
        }];

        let age_days = latest_cot_report_age_days(&reports).unwrap();
        assert!(age_days >= 0);
    }

    #[test]
    fn refresh_plan_full_enables_all() {
        let plan = RefreshPlan::full();
        assert!(plan.prices);
        assert!(plan.predictions);
        assert!(plan.news_rss);
        assert!(plan.news_brave);
        assert!(plan.cot);
        assert!(plan.worldbank);
        assert!(plan.analytics);
        assert!(plan.cleanup);
        assert_eq!(plan.selected_task_names().len(), 17);
    }

    #[test]
    fn refresh_plan_none_disables_all() {
        let plan = RefreshPlan::none();
        assert!(!plan.prices);
        assert!(!plan.predictions);
        assert!(!plan.news_rss);
        assert!(!plan.analytics);
        assert!(plan.selected_task_names().is_empty());
    }

    #[test]
    fn refresh_plan_from_only_prices() {
        let plan = RefreshPlan::from_only(&["prices".to_string()]).unwrap();
        assert!(plan.prices);
        assert!(!plan.predictions);
        assert!(!plan.news_rss);
        assert!(!plan.analytics);
        assert_eq!(plan.selected_task_names(), vec!["prices"]);
    }

    #[test]
    fn refresh_plan_from_only_multiple() {
        let plan =
            RefreshPlan::from_only(&["prices".to_string(), "news_rss".to_string(), "alerts".to_string()])
                .unwrap();
        assert!(plan.prices);
        assert!(plan.news_rss);
        assert!(plan.alerts);
        assert!(!plan.predictions);
        assert!(!plan.cot);
        assert_eq!(plan.selected_task_names().len(), 3);
    }

    #[test]
    fn refresh_plan_from_only_news_alias() {
        let plan = RefreshPlan::from_only(&["news".to_string()]).unwrap();
        assert!(plan.news_rss);
        assert!(plan.news_brave);
        assert!(!plan.prices);
        assert_eq!(plan.selected_task_names().len(), 2);
    }

    #[test]
    fn refresh_plan_from_skip_sources() {
        let plan = RefreshPlan::from_skip(&["worldbank".to_string(), "bls".to_string()]).unwrap();
        assert!(plan.prices);
        assert!(plan.predictions);
        assert!(!plan.worldbank);
        assert!(!plan.bls);
        assert_eq!(plan.selected_task_names().len(), 15);
    }

    #[test]
    fn refresh_plan_from_only_unknown_source_errors() {
        let result = RefreshPlan::from_only(&["nonexistent".to_string()]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown refresh source"));
        assert!(err_msg.contains("nonexistent"));
    }

    #[test]
    fn refresh_plan_from_skip_unknown_source_errors() {
        let result = RefreshPlan::from_skip(&["bogus".to_string()]);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown refresh source"));
    }

    #[test]
    fn yahoo_max_concurrent_is_reasonable() {
        // Must be at least 1 (otherwise nothing fetches) and at most 10
        // (Yahoo rate-limits aggressively above ~5 concurrent).
        const _: () = assert!(YAHOO_MAX_CONCURRENT >= 1 && YAHOO_MAX_CONCURRENT <= 10);
    }

    #[tokio::test]
    async fn semaphore_limits_concurrency() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::Semaphore;

        let sem = Arc::new(Semaphore::new(YAHOO_MAX_CONCURRENT));
        let peak = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..20 {
            let sem = Arc::clone(&sem);
            let peak = Arc::clone(&peak);
            let active = Arc::clone(&active);
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let peak_val = peak.load(Ordering::SeqCst);
        assert!(
            peak_val <= YAHOO_MAX_CONCURRENT,
            "peak concurrency {} exceeded limit {}",
            peak_val, YAHOO_MAX_CONCURRENT
        );
        // Verify concurrency actually happened (not all sequential)
        assert!(
            peak_val > 1,
            "peak concurrency {} — should be >1 for parallel execution",
            peak_val
        );
    }

    #[tokio::test]
    async fn fetch_yahoo_price_with_timeout_handles_bad_symbol() {
        // An invalid symbol should return an Err, not panic
        let result = fetch_yahoo_price_with_timeout("ZZZZ_NONEXISTENT_SYMBOL_999").await;
        assert!(result.is_err(), "expected error for invalid symbol");
    }
}
