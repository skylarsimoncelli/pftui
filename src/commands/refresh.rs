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
    bls, brave, calendar, comex, cot, economic, fedwatch, fred, fx, ism, onchain, predictions,
    real_yields as real_yields_data, rss, sentiment, worldbank,
};
use crate::db::allocations::{get_unique_allocation_symbols_backend, list_allocations_backend};
use crate::db::backend::BackendConnection;
use crate::db::economic_data as economic_data_db;
use crate::db::fedwatch_cache;
use crate::db::price_cache::{
    get_all_cached_prices_backend, get_cached_price_backend, upsert_price_backend,
};
use crate::db::price_guard;
use crate::db::price_history::{
    get_history_backend, get_latest_close_before_backend, get_price_at_date_backend,
};
use crate::db::snapshots::{upsert_portfolio_snapshot_backend, upsert_position_snapshot_backend};
use crate::db::technical_levels;
use crate::db::technical_snapshots;
use crate::db::timeframe_signals;
use crate::db::transactions::{get_unique_symbols_backend, list_transactions_backend};
use crate::db::watchlist::get_watchlist_symbols_backend;
use crate::db::{
    bls_cache, calendar_cache, comex_cache, cot_cache, fx_cache, news_cache, rss_feed_health,
};
use crate::db::prediction_contracts;
use crate::db::{
    economic_cache, macro_events, onchain_cache, predictions_cache, sentiment_cache,
    worldbank_cache,
};
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations};
use crate::models::price::{HistoryRecord, PriceQuote};
use crate::notify;
use crate::price::{coingecko, geckoterminal, mempool, yahoo};
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
    pub real_yields: bool,
    pub bls: bool,
    pub worldbank: bool,
    pub comex: bool,
    pub onchain: bool,
    pub flows: bool,
    pub analytics: bool,
    pub alerts: bool,
    pub cleanup: bool,
    pub options: bool,
    /// Symbols (uppercased on use) whose >20% d/d prints are admitted past
    /// the price-ingest plausibility guard this run — the operator override
    /// for genuine gap events (`pftui data refresh --accept-outlier SYM`).
    pub accept_outliers: Vec<String>,
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
        "real_yields",
        "bls",
        "worldbank",
        "comex",
        "onchain",
        "flows",
        "analytics",
        "alerts",
        "cleanup",
        "options",
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
            real_yields: true,
            bls: true,
            worldbank: true,
            comex: true,
            onchain: true,
            flows: true,
            analytics: true,
            alerts: true,
            cleanup: true,
            options: true,
            accept_outliers: Vec::new(),
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
            real_yields: false,
            bls: false,
            worldbank: false,
            comex: false,
            onchain: false,
            flows: false,
            analytics: false,
            alerts: false,
            cleanup: false,
            options: false,
            accept_outliers: Vec::new(),
        }
    }

    /// Convenience plan for refreshing only price data.
    pub fn prices_only() -> Self {
        let mut plan = Self::none();
        plan.prices = true;
        plan
    }

    /// Attach the operator's `--accept-outlier` symbol list to this plan.
    pub fn with_accept_outliers(mut self, symbols: Vec<String>) -> Self {
        self.accept_outliers = symbols;
        self
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
            "real_yields" => self.real_yields = enabled,
            "bls" => self.bls = enabled,
            "worldbank" => self.worldbank = enabled,
            "comex" => self.comex = enabled,
            "onchain" => self.onchain = enabled,
            "flows" => self.flows = enabled,
            "analytics" => self.analytics = enabled,
            "alerts" => self.alerts = enabled,
            "cleanup" => self.cleanup = enabled,
            "options" => self.options = enabled,
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
        if self.real_yields {
            names.push("real_yields");
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
        if self.flows {
            names.push("flows");
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
        if self.options {
            names.push("options");
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

pub(crate) fn build_brave_news_queries(
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

fn fred_keyless_fallbacks_need_refresh(backend: &BackendConnection) -> Result<bool> {
    let observations = economic_cache::get_all_latest_backend(backend)?;
    if observations.is_empty() {
        return Ok(true);
    }

    for series_id in ["DGS10_YAHOO", "GDPNOW_WEB"] {
        let latest = observations.iter().find(|obs| obs.series_id == series_id);
        match latest {
            Some(obs) if !fred::is_series_stale(series_id, &obs.date) => {}
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

    let now = chrono::Utc::now();
    let expected_report_date = cot::expected_latest_report_date(now);
    let latest_report_date = latest_cot_report_date(&reports);

    if latest_report_date.is_some_and(|date| date < expected_report_date) {
        return Ok(true);
    }

    if latest_cot_report_age_days(&reports).is_some_and(|age_days| age_days > 7) {
        return Ok(true);
    }

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
    let latest = latest_cot_report_date(reports)?;
    Some((chrono::Utc::now().date_naive() - latest).num_days())
}

fn latest_cot_report_date(
    reports: &[crate::db::cot_cache::CotCacheEntry],
) -> Option<chrono::NaiveDate> {
    reports
        .iter()
        .filter_map(|report| chrono::NaiveDate::parse_from_str(&report.report_date, "%Y-%m-%d").ok())
        .max()
}

fn cot_staleness_detail(backend: &BackendConnection) -> Option<String> {
    let reports = cot_cache::get_all_latest_backend(backend).ok()?;
    let latest = latest_cot_report_date(&reports)?;
    let expected = cot::expected_latest_report_date(chrono::Utc::now());
    let age_days = (chrono::Utc::now().date_naive() - latest).num_days();

    if latest < expected {
        Some(format!(
            "latest COT report date {} lags expected report date {}",
            latest.format("%Y-%m-%d"),
            expected.format("%Y-%m-%d")
        ))
    } else if age_days > 7 {
        Some(format!(
            "latest COT report date {} is {} days old",
            latest.format("%Y-%m-%d"),
            age_days
        ))
    } else {
        None
    }
}

/// Build an age-stamped stale-cache note for COT when the live fetch fails.
///
/// Mirrors the COMEX/supply last-good-cache pattern: if `cached_reports` holds
/// prior reports, return an explicit "serving cached data as of <date>" note so
/// downstream report code never silently cites stale COT data. Returns `None`
/// when there is no cache to fall back to.
fn cot_stale_cache_note(
    cached_reports: &[crate::db::cot_cache::CotCacheEntry],
    today: chrono::NaiveDate,
) -> Option<String> {
    let as_of = latest_cot_report_date(cached_reports)?;
    let age_days = (today - as_of).num_days();
    Some(format!(
        "COT live fetch failed; serving cached data as of {} ({} days old)",
        as_of.format("%Y-%m-%d"),
        age_days
    ))
}

/// Build an age-stamped stale-cache note for options when all live fetches fail.
///
/// `latest_fetched_ats` is the set of per-symbol most-recent `fetched_at`
/// strings from `options_chain_snapshots`. Returns a note referencing the most
/// recent cached snapshot, or `None` when nothing is cached.
fn options_stale_cache_note(latest_fetched_ats: &[String]) -> Option<String> {
    let as_of = latest_fetched_ats.iter().max()?;
    Some(format!(
        "Options live fetch failed; serving cached snapshot as of {}",
        as_of
    ))
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

// ── Last-resort SPOT-ONLY fallbacks (redundant price sources) ───────────────
//
// Yahoo stays primary (OHLCV + history). These sources fire only when every
// primary fetch for the symbol failed during `data refresh`:
//   BTC:  coingecko → yahoo → mempool.space   (third in chain)
//   gold: yahoo → GeckoTerminal XAUt/USDT pool (on-chain proxy, see
//         src/price/geckoterminal.rs for the XAUT-proxy caveat)
// Every fallback price passes a divergence guard against the last stored
// close before it is allowed into price_cache/price_history.

/// Maximum % a fallback spot price may diverge from the last stored close.
/// Beyond this the price is REJECTED — a broken proxy must never silently
/// poison price_history.
const SPOT_FALLBACK_MAX_DIVERGENCE_PCT: Decimal = dec!(5);

/// Which last-resort spot fallback (if any) serves a given symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpotFallbackSource {
    /// mempool.space `/api/v1/prices` — BTC/USD spot.
    MempoolBtc,
    /// GeckoTerminal XAUt/USDT pool — on-chain proxy for spot gold.
    GeckoTerminalXaut,
}

impl SpotFallbackSource {
    /// Provenance string stored in price_cache.source / price_history.source.
    fn provenance(self) -> &'static str {
        match self {
            SpotFallbackSource::MempoolBtc => "mempool.space",
            SpotFallbackSource::GeckoTerminalXaut => "geckoterminal-xaut",
        }
    }
}

/// Map a symbol to its spot-only fallback source, if one exists.
/// BTC (canonical or the BTC-USD deep alias) → mempool.space; the gold
/// futures series GC=F → GeckoTerminal XAUT pool.
fn spot_fallback_for_symbol(symbol: &str) -> Option<SpotFallbackSource> {
    match symbol.to_uppercase().as_str() {
        "BTC" | "BTC-USD" => Some(SpotFallbackSource::MempoolBtc),
        "GC=F" => Some(SpotFallbackSource::GeckoTerminalXaut),
        _ => None,
    }
}

/// Select the symbols that still need a last-resort spot fallback after the
/// primary (and secondary) stages: attempted, fallback-eligible, and absent
/// from the fetched-quotes set. Primary succeeded → no fallback call.
fn spot_fallback_targets(
    symbols: &[(String, AssetCategory)],
    fetched: &HashSet<String>,
) -> Vec<(String, SpotFallbackSource)> {
    symbols
        .iter()
        .filter(|(_, cat)| *cat != AssetCategory::Cash)
        .filter(|(sym, _)| !fetched.contains(sym))
        .filter_map(|(sym, _)| spot_fallback_for_symbol(sym).map(|src| (sym.clone(), src)))
        .collect()
}

/// Divergence guard for fallback spot prices.
///
/// - `Ok(Some(pct))` — accepted; pct is the absolute % move vs the last close
/// - `Ok(None)` — accepted with no stored close to validate against
/// - `Err(pct)` — REJECTED: diverges more than
///   [`SPOT_FALLBACK_MAX_DIVERGENCE_PCT`] from the last stored close
fn check_spot_fallback_divergence(
    candidate: Decimal,
    last_close: Option<Decimal>,
) -> std::result::Result<Option<Decimal>, Decimal> {
    let last = match last_close {
        Some(c) if !c.is_zero() => c,
        _ => return Ok(None),
    };
    let pct = ((candidate - last) / last * dec!(100)).abs();
    if pct > SPOT_FALLBACK_MAX_DIVERGENCE_PCT {
        Err(pct)
    } else {
        Ok(Some(pct))
    }
}

/// Last stored close per fallback-eligible symbol — the divergence-guard
/// baseline, gathered before the async fetch stage.
fn spot_fallback_last_closes(
    backend: &BackendConnection,
    symbols: &[(String, AssetCategory)],
) -> HashMap<String, Decimal> {
    let mut map = HashMap::new();
    for (sym, _) in symbols {
        if spot_fallback_for_symbol(sym).is_none() {
            continue;
        }
        if let Ok(history) = get_history_backend(backend, sym, 1) {
            if let Some(rec) = history.last() {
                map.insert(sym.clone(), rec.close);
            }
        }
    }
    map
}

/// Build the summary-line suffix for fallback-sourced quotes, e.g.
/// `, 2 via fallback: BTC←mempool.space (block 953254), GC=F←geckoterminal-xaut`.
fn fallback_summary_suffix(notes: &[String]) -> String {
    if notes.is_empty() {
        String::new()
    } else {
        format!(", {} via fallback: {}", notes.len(), notes.join(", "))
    }
}

/// Run the last-resort spot fallbacks for every eligible symbol that has no
/// live quote yet. Accepted prices are appended to `quotes` (with fallback
/// provenance in `source`); rejections and failures are appended to `errors`.
/// Returns human-readable provenance notes for the summary line.
async fn fetch_spot_fallbacks(
    symbols: &[(String, AssetCategory)],
    last_closes: &HashMap<String, Decimal>,
    quotes: &mut Vec<PriceQuote>,
    errors: &mut Vec<String>,
) -> Vec<String> {
    let fetched: HashSet<String> = quotes
        .iter()
        .filter(|q| q.source != "static")
        .map(|q| q.symbol.clone())
        .collect();

    let mut notes = Vec::new();
    for (sym, source) in spot_fallback_targets(symbols, &fetched) {
        let result = match source {
            SpotFallbackSource::MempoolBtc => {
                tokio::time::timeout(PRICE_REQUEST_TIMEOUT, mempool::fetch_btc_spot_usd()).await
            }
            SpotFallbackSource::GeckoTerminalXaut => {
                tokio::time::timeout(PRICE_REQUEST_TIMEOUT, geckoterminal::fetch_xaut_usd()).await
            }
        };
        let price = match result {
            Ok(Ok(price)) => price,
            Ok(Err(e)) => {
                errors.push(format!(
                    "{}: {} fallback failed: {}",
                    sym,
                    source.provenance(),
                    e
                ));
                continue;
            }
            Err(_) => {
                errors.push(format!(
                    "{}: {} fallback timed out",
                    sym,
                    source.provenance()
                ));
                continue;
            }
        };

        match check_spot_fallback_divergence(price, last_closes.get(&sym).copied()) {
            Err(pct) => {
                // Loud quarantine-style rejection — never silently poison
                // price_history with a dislocated proxy price.
                errors.push(format!(
                    "{}: DIVERGENCE GUARD REJECTED {} fallback price {} — {:.2}% from last stored close (limit {}%); not stored",
                    sym,
                    source.provenance(),
                    price,
                    pct,
                    SPOT_FALLBACK_MAX_DIVERGENCE_PCT
                ));
            }
            Ok(checked) => {
                quotes.push(PriceQuote {
                    symbol: sym.clone(),
                    price,
                    currency: "USD".to_string(),
                    source: source.provenance().to_string(),
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                    pre_market_price: None,
                    post_market_price: None,
                    post_market_change_percent: None,
                    previous_close: None,
                });
                let mut note = format!("{}←{}", sym, source.provenance());
                if source == SpotFallbackSource::MempoolBtc {
                    // Bonus provenance field — best-effort, failure tolerated.
                    if let Ok(Ok(height)) =
                        tokio::time::timeout(PRICE_REQUEST_TIMEOUT, mempool::fetch_block_height())
                            .await
                    {
                        note.push_str(&format!(" (block {})", height));
                    }
                }
                if checked.is_none() {
                    note.push_str(" (no stored close to validate against)");
                }
                notes.push(note);
            }
        }
    }
    notes
}

// ── Price-ingest plausibility guard (today's close stamping) ────────────────
//
// price_history is the canonical L1 series. Two structural rules apply at
// the stamping stage:
//
// 1. NEVER stamp a stale cached price onto today's date. A failed fetch
//    means today's bar simply does not get written — the cached value's true
//    dated row already exists, and the spot layer (price_cache, with its
//    fetched_at timestamp) keeps serving it as an explicitly stale quote.
//    (P1 bug 2026-06-11: a 6-day-old cached BTC close was stamped onto the
//    report date with source='cache' and fired false market-structure
//    verdicts downstream.)
// 2. Every live print passes the day-over-day plausibility guard
//    (db::price_guard): >20% d/d is SUSPECT and must be corroborated by an
//    independent secondary source (within 5%) or explicitly admitted via
//    `--accept-outlier SYM`; otherwise it is rejected loudly.

/// Fetch an independent same-day spot quote to corroborate a SUSPECT print.
/// BTC: mempool.space — unless the suspect print itself came from
/// mempool.space, in which case CoinGecko is consulted instead. Gold
/// futures (GC=F): GeckoTerminal XAUT proxy. Other symbols have no wired
/// secondary and return None (the guard then rejects, override required).
fn fetch_corroboration_spot(
    rt: &tokio::runtime::Runtime,
    symbol: &str,
    primary_source: &str,
) -> Option<Decimal> {
    match symbol.to_uppercase().as_str() {
        "BTC" | "BTC-USD" => {
            if primary_source == "mempool.space" {
                rt.block_on(async {
                    tokio::time::timeout(
                        PRICE_REQUEST_TIMEOUT,
                        coingecko::fetch_prices(&["BTC".to_string()]),
                    )
                    .await
                    .ok()?
                    .ok()?
                    .first()
                    .map(|q| q.price)
                })
            } else {
                rt.block_on(async {
                    tokio::time::timeout(PRICE_REQUEST_TIMEOUT, mempool::fetch_btc_spot_usd())
                        .await
                        .ok()?
                        .ok()
                })
            }
        }
        "GC=F" => {
            if primary_source == "geckoterminal-xaut" {
                None
            } else {
                rt.block_on(async {
                    tokio::time::timeout(PRICE_REQUEST_TIMEOUT, geckoterminal::fetch_xaut_usd())
                        .await
                        .ok()?
                        .ok()
                })
            }
        }
        _ => None,
    }
}

/// Outcome of stamping today's live quotes into price_history.
#[derive(Debug, Default)]
struct HistoryStampSummary {
    /// Rows written (guard-accepted).
    ok: usize,
    /// DB write errors.
    err: usize,
    /// Sample DB-error strings (max 3).
    examples: Vec<String>,
    /// Loud per-print guard rejection lines.
    guard_warnings: Vec<String>,
    /// Informational accept notes (corroborated / overridden prints).
    guard_notes: Vec<String>,
}

/// Stamp today's close for every LIVE quote into price_history, through the
/// plausibility guard. Quotes are the only input — symbols whose fetch
/// failed are simply absent and get NO row for today (rule 1 above).
///
/// `fetch_secondary(symbol, primary_source)` is invoked lazily, only when a
/// print is suspect and not already overridden — tests inject a mock.
fn stamp_live_quotes_into_history(
    backend: &BackendConnection,
    quotes: &[PriceQuote],
    today: &str,
    accept_outliers: &HashSet<String>,
    fetch_secondary: &mut dyn FnMut(&str, &str) -> Option<Decimal>,
) -> HistoryStampSummary {
    let mut summary = HistoryStampSummary::default();
    for quote in quotes {
        if quote.source == "static" {
            continue;
        }
        let accept_outlier = accept_outliers.contains(&quote.symbol.to_uppercase());
        // Lazy secondary lookup: only hit the network when the print is
        // suspect and the operator has not already overridden it.
        let secondary = if accept_outlier {
            None
        } else {
            match get_latest_close_before_backend(backend, &quote.symbol, today) {
                Ok(Some((_, prev))) if prev > Decimal::ZERO => {
                    let pct = price_guard::signed_change_pct(quote.price, prev).abs();
                    if pct > price_guard::MAX_DD_CHANGE_PCT {
                        fetch_secondary(&quote.symbol, &quote.source)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        let record = HistoryRecord {
            date: today.to_string(),
            close: quote.price,
            volume: None,
            open: None,
            high: None,
            low: None,
        };
        match price_guard::upsert_history_guarded_backend(
            backend,
            &quote.symbol,
            &quote.source,
            &[record],
            secondary,
            accept_outlier,
        ) {
            Ok(outcome) => {
                summary.ok += outcome.accepted;
                summary
                    .guard_warnings
                    .extend(outcome.rejections.iter().map(|r| r.warning_line()));
                summary.guard_notes.extend(outcome.corroborated);
                summary.guard_notes.extend(outcome.overridden);
            }
            Err(e) => {
                summary.err += 1;
                if summary.examples.len() < 3 {
                    summary.examples.push(format!("{}: {}", quote.symbol, e));
                }
            }
        }
    }
    summary
}

/// Fetch prices for all given symbols and return
/// `(quotes, errors, fallback_notes)`.
///
/// Yahoo Finance requests are limited to [`YAHOO_MAX_CONCURRENT`] in-flight
/// at a time via a [`tokio::sync::Semaphore`], providing ~4× speedup over
/// the previous sequential loop while staying well within rate limits.
///
/// `last_closes` carries the last stored close per fallback-eligible symbol
/// and feeds the spot-fallback divergence guard (see [`fetch_spot_fallbacks`]).
async fn fetch_all_prices(
    symbols: &[(String, AssetCategory)],
    config: &Config,
    last_closes: &HashMap<String, Decimal>,
) -> (Vec<PriceQuote>, Vec<String>, Vec<String>) {
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

    // Last-resort spot fallbacks for symbols every primary stage missed
    // (BTC ← mempool.space, GC=F ← GeckoTerminal XAUT), divergence-guarded.
    let fallback_notes = fetch_spot_fallbacks(symbols, last_closes, &mut quotes, &mut errors).await;

    (quotes, errors, fallback_notes)
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
    run_with_output(backend, config, notify, true, &RefreshPlan::full(), None)
}

/// Run refresh with a custom plan (verbose human output).
pub fn run_with_plan(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    plan: &RefreshPlan,
    timeout_secs: Option<u64>,
) -> Result<()> {
    run_with_output(backend, config, notify, true, plan, timeout_secs)
}

/// Run refresh with a custom plan and output structured JSON metrics.
pub fn run_json_with_plan(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    plan: &RefreshPlan,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let result = run_pipeline(backend, config, notify, false, plan, timeout_secs)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

pub fn run_quiet(backend: &BackendConnection, config: &Config, notify: bool) -> Result<()> {
    run_with_output(backend, config, notify, false, &RefreshPlan::full(), None)
}

pub fn run_quiet_with_plan(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    plan: &RefreshPlan,
    timeout_secs: Option<u64>,
) -> Result<()> {
    run_with_output(backend, config, notify, false, plan, timeout_secs)
}

fn run_with_output(
    backend: &BackendConnection,
    config: &Config,
    notify: bool,
    verbose: bool,
    plan: &RefreshPlan,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let result = run_pipeline(backend, config, notify, verbose, plan, timeout_secs)?;
    if result.status == crate::commands::refresh_dag::RefreshRunStatus::Partial && verbose {
        if let Some(message) = &result.message {
            info_ln!(verbose, "\n⚠ {}", message);
        }
        info_ln!(
            verbose,
            "Completed sources before timeout: {}",
            result.completed_sources.join(", ")
        );
        if !result.failed_sources.is_empty() {
            info_ln!(
                verbose,
                "Failed sources before timeout: {}",
                result.failed_sources.join(", ")
            );
        }
    }
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
    timeout_secs: Option<u64>,
) -> Result<RefreshResult> {
    let _lock = RefreshLock::acquire()?;
    let pipeline_start = Instant::now();
    let deadline = timeout_secs.map(|secs| pipeline_start + Duration::from_secs(secs));
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
    let fred_due = if !plan.fred {
        false
    } else if fred_api_key_str.is_some() {
        fred_needs_refresh(backend).unwrap_or(true)
    } else {
        fred_keyless_fallbacks_need_refresh(backend).unwrap_or(true)
    };
    let worldbank_due = plan.worldbank && worldbank_needs_refresh(backend).unwrap_or(true);
    let comex_due = plan.comex && comex_needs_refresh(backend).unwrap_or(true);

    // Build Brave news queries before entering async context (needs DB)
    let brave_queries = if brave_refresh {
        build_brave_news_queries(backend, config).unwrap_or_default()
    } else {
        Vec::new()
    };
    let configured_rss_feeds = if rss_refresh {
        rss::configured_feeds(config)
    } else {
        Vec::new()
    };
    let disabled_feed_ids = if rss_refresh {
        rss_feed_health::disabled_feed_ids_backend(backend).unwrap_or_default()
    } else {
        HashSet::new()
    };
    let (rss_feeds_to_fetch, rss_disabled_feeds): (Vec<_>, Vec<_>) = configured_rss_feeds
        .into_iter()
        .partition(|feed| !disabled_feed_ids.contains(&feed.feed_id()));

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

            if api_key.is_empty() {
                match fred::fetch_dgs10_yahoo_fallback().await {
                    Ok(fallback) => results.push(("DGS10_YAHOO", Ok(vec![fallback]))),
                    Err(e) => results.push(("DGS10_YAHOO", Err(e))),
                }
                match fred::fetch_gdpnow_web_fallback().await {
                    Ok(fallback) => results.push(("GDPNOW_WEB", Ok(vec![fallback]))),
                    Err(e) => results.push(("GDPNOW_WEB", Err(e))),
                }
                return Some((results, start.elapsed()));
            }

            for series in fred::FRED_SERIES {
                match fred::fetch_series(api_key, series.id, 24).await {
                    Ok(obs) if !obs.is_empty() => {
                        let latest_stale =
                            obs.first().map(|row| fred::is_series_stale(series.id, &row.date)).unwrap_or(false);
                        results.push((series.id, Ok(obs)));
                        if series.id == "DGS10" && latest_stale {
                            match fred::fetch_dgs10_yahoo_fallback().await {
                                Ok(fallback) => results.push(("DGS10_YAHOO", Ok(vec![fallback]))),
                                Err(e) => results.push(("DGS10_YAHOO", Err(e))),
                            }
                        }
                        if series.id == "GDPNOW" && latest_stale {
                            match fred::fetch_gdpnow_web_fallback().await {
                                Ok(fallback) => results.push(("GDPNOW_WEB", Ok(vec![fallback]))),
                                Err(e) => results.push(("GDPNOW_WEB", Err(e))),
                            }
                        }
                    }
                    Ok(_) => {
                        if series.id == "DGS10" {
                            match fred::fetch_dgs10_yahoo_fallback().await {
                                Ok(fallback) => results.push(("DGS10_YAHOO", Ok(vec![fallback]))),
                                Err(e) => results.push(("DGS10_YAHOO", Err(e))),
                            }
                        }
                        if series.id == "GDPNOW" {
                            match fred::fetch_gdpnow_web_fallback().await {
                                Ok(fallback) => results.push(("GDPNOW_WEB", Ok(vec![fallback]))),
                                Err(e) => results.push(("GDPNOW_WEB", Err(e))),
                            }
                        }
                    }
                    Err(e) => {
                        if series.id == "DGS10" {
                            match fred::fetch_dgs10_yahoo_fallback().await {
                                Ok(fallback) => {
                                    results.push((series.id, Err(e)));
                                    results.push(("DGS10_YAHOO", Ok(vec![fallback])));
                                }
                                Err(fallback_err) => {
                                    results.push((series.id, Err(e)));
                                    results.push(("DGS10_YAHOO", Err(fallback_err)));
                                }
                            }
                        } else if series.id == "GDPNOW" {
                            match fred::fetch_gdpnow_web_fallback().await {
                                Ok(fallback) => {
                                    results.push((series.id, Err(e)));
                                    results.push(("GDPNOW_WEB", Ok(vec![fallback])));
                                }
                                Err(fallback_err) => {
                                    results.push((series.id, Err(e)));
                                    results.push(("GDPNOW_WEB", Err(fallback_err)));
                                }
                            }
                        } else {
                            results.push((series.id, Err(e)));
                        }
                    }
                }
            }
            Some((results, start.elapsed()))
        };

        let rss_fut = async {
            if !rss_refresh {
                return None;
            }
            let start = Instant::now();
            let mut report = rss::fetch_all_feeds_detailed(&rss_feeds_to_fetch).await;
            report.skipped = rss_disabled_feeds
                .iter()
                .map(|feed| rss::FeedSkipped {
                    feed_name: feed.name.clone(),
                    feed_url: feed.url.clone(),
                    reason: "disabled after repeated failures".to_string(),
                })
                .collect();
            Some((report, start.elapsed()))
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

    if let Some(result) =
        maybe_stop_for_timeout(deadline, pipeline_start, verbose, &dag_result, "layer 0 FX rates")
    {
        return Ok(result);
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

    // Enrichment passes that depend on freshly-written news: recompute the
    // news-silence baselines (per-topic, per-weekday), append today's
    // narrative-vs-money divergence per active scenario, and replay the
    // news-source accuracy ledger for any newly-scored predictions tagged
    // with a `source_article_id`. Each pass is best-effort — failures are
    // logged but do not abort the refresh.
    run_news_enrichment_passes(backend, verbose, rss_refresh || brave_refresh);

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

    // Real-yields curve (TIPS, breakevens, G10 sovereign 10Y) — synchronous,
    // gracefully degrades when the FRED API key is absent or offline.
    store_real_yields_result(
        backend,
        config,
        verbose,
        plan.real_yields,
        &rt,
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

    // Options + GEX (synchronous)
    store_options_result(backend, verbose, plan.options, &mut dag_result);
    // Capital flows (F59 scaffold — provider configured via PFTUI_FLOWS_PROVIDER)
    store_flows_result(backend, verbose, plan.flows, &mut dag_result);

    if let Some(result) = maybe_stop_for_timeout(
        deadline,
        pipeline_start,
        verbose,
        &dag_result,
        "layer 0 independent sources",
    ) {
        return Ok(result);
    }

    // ── DAG Layer 1: Prices ────────────────────────────────────────────
    let symbols = if plan.prices {
        collect_symbols(backend, config)?
    } else {
        Vec::new()
    };
    if plan.prices && !symbols.is_empty() {
        let price_start = Instant::now();
        let last_closes = spot_fallback_last_closes(backend, &symbols);
        let (quotes, errors, fallback_notes) =
            rt.block_on(fetch_all_prices(&symbols, config, &last_closes));

        for quote in &quotes {
            if let Err(e) = upsert_price_backend(backend, quote) {
                warn_ln!(verbose, "Failed to cache {}: {}", quote.symbol, e);
            }
        }
        // Stamp today's close into price_history — LIVE quotes only, through
        // the plausibility guard. Symbols whose fetch failed get NO row for
        // today: a stale cached price must never be persisted as a new dated
        // close (it stays available as an explicitly stale spot via
        // price_cache). See the guard rules above stamp_live_quotes_into_history.
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let accept_outliers: HashSet<String> = plan
            .accept_outliers
            .iter()
            .map(|s| s.trim().to_uppercase())
            .collect();
        let mut secondary_fetch =
            |symbol: &str, primary: &str| fetch_corroboration_spot(&rt, symbol, primary);
        let stamp = stamp_live_quotes_into_history(
            backend,
            &quotes,
            &today,
            &accept_outliers,
            &mut secondary_fetch,
        );
        for warning in &stamp.guard_warnings {
            warn_ln!(verbose, "{}", warning);
        }
        for note in &stamp.guard_notes {
            info_ln!(verbose, "  price guard: {}", note);
        }
        if stamp.err > 0 {
            info_ln!(
                verbose,
                "⚠ Price history stamp issues: {} writes failed ({} ok). Sample: {}",
                stamp.err,
                stamp.ok,
                stamp.examples.join(" | ")
            );
        }

        let fetched_count = quotes.iter().filter(|q| q.source != "static").count();
        let total_attempted = symbols
            .iter()
            .filter(|(_, cat)| *cat != AssetCategory::Cash)
            .count();
        let error_count = errors.len();

        let fallback_suffix = fallback_summary_suffix(&fallback_notes);
        if fetched_count > 0 && error_count == 0 {
            info_ln!(verbose, "✓ Prices ({} symbols{})", fetched_count, fallback_suffix);
        } else if fetched_count > 0 && error_count > 0 {
            info_ln!(
                verbose,
                "⚠ Prices ({}/{} symbols — {} failed{})",
                fetched_count,
                total_attempted,
                error_count,
                fallback_suffix
            );
        } else {
            info_ln!(verbose, "✗ Prices (no live quotes fetched)");
        }
        // Divergence-guard rejections are loud: a dislocated fallback proxy
        // must never silently poison price_history.
        for rejection in errors.iter().filter(|e| e.contains("DIVERGENCE GUARD REJECTED")) {
            warn_ln!(verbose, "⚠ {}", rejection);
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
            detail: {
                let mut parts = Vec::new();
                if !fallback_notes.is_empty() {
                    parts.push(format!("via fallback: {}", fallback_notes.join(", ")));
                }
                if !stamp.guard_warnings.is_empty() {
                    parts.push(stamp.guard_warnings.join(" | "));
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("; "))
                }
            },
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
            if let Ok((records, source)) = rt.block_on(fetch_history_for_symbol(sym, *cat, 180)) {
                if records.is_empty() {
                    continue;
                }
                // Backfill batches go through the same plausibility guard,
                // checked bar-by-bar (no single-print secondary applies).
                if let Ok(outcome) = price_guard::upsert_history_guarded_backend(
                    backend,
                    sym,
                    source,
                    &records,
                    None,
                    accept_outliers.contains(&sym.to_uppercase()),
                ) {
                    if outcome.accepted > 0 {
                        history_updated += 1;
                    }
                    for rejection in &outcome.rejections {
                        warn_ln!(verbose, "{} (backfill)", rejection.warning_line());
                    }
                }
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

    if let Some(result) =
        maybe_stop_for_timeout(deadline, pipeline_start, verbose, &dag_result, "layer 1 prices")
    {
        return Ok(result);
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

    if let Some(result) = maybe_stop_for_timeout(
        deadline,
        pipeline_start,
        verbose,
        &dag_result,
        "layer 2 analytics snapshots",
    ) {
        return Ok(result);
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

    if let Some(result) = maybe_stop_for_timeout(
        deadline,
        pipeline_start,
        verbose,
        &dag_result,
        "layer 3 portfolio and alerts",
    ) {
        return Ok(result);
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

    // Tail step: mechanically score pending predictions from their
    // falsification rules against the daily closes refreshed above. This
    // system has no daemon, so refresh is the recurring surface that keeps
    // the prediction learning loop closed. Idempotent — decided predictions
    // are never overwritten.
    let autoscore_start = Instant::now();
    match crate::commands::predict::auto_score_for_refresh(backend) {
        Ok(summary) => {
            info_ln!(
                verbose,
                "✓ Prediction auto-score: {} scored ({} correct / {} wrong)",
                summary.scored,
                summary.correct,
                summary.wrong
            );
            dag_result.add(SourceResult {
                name: "prediction_autoscore".to_string(),
                label: "Prediction Auto-Score".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: Some(summary.scored),
                duration_ms: autoscore_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: Some(format!(
                    "{} correct / {} wrong / {} skipped / {} failed",
                    summary.correct, summary.wrong, summary.skipped, summary.failures
                )),
            });
        }
        Err(e) => {
            info_ln!(verbose, "  ⚠ Prediction auto-score failed: {}", e);
            dag_result.add(SourceResult {
                name: "prediction_autoscore".to_string(),
                label: "Prediction Auto-Score".to_string(),
                status: SourceStatus::Failed,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: None,
                duration_ms: autoscore_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: None,
            });
        }
    }

    // Tail step: fill recommendation-ledger forward returns
    // (fwd_{30,90,180}d_pct) for any priced row whose horizon has elapsed,
    // against the daily closes refreshed above. Same rationale as the
    // prediction auto-score: no daemon, refresh is the recurring surface,
    // and an unscored recommendation ledger is how add-into-a-drawdown went
    // unnoticed for 5 months. Idempotent — scored horizons are never
    // overwritten.
    let recscore_start = Instant::now();
    match crate::commands::recommendations::auto_score_for_refresh(backend) {
        Ok(summary) => {
            info_ln!(
                verbose,
                "✓ Recommendation forward-score: {} horizon(s) filled across {} row(s)",
                summary.horizons_filled,
                summary.rows_updated
            );
            dag_result.add(SourceResult {
                name: "recommendation_forward_score".to_string(),
                label: "Recommendation Forward-Score".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: Some(summary.horizons_filled),
                duration_ms: recscore_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: Some(format!(
                    "{} candidate row(s), {} horizon cell(s) filled, {} row(s) updated",
                    summary.candidates, summary.horizons_filled, summary.rows_updated
                )),
            });
        }
        Err(e) => {
            info_ln!(verbose, "  ⚠ Recommendation forward-score failed: {}", e);
            dag_result.add(SourceResult {
                name: "recommendation_forward_score".to_string(),
                label: "Recommendation Forward-Score".to_string(),
                status: SourceStatus::Failed,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: None,
                duration_ms: recscore_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: None,
            });
        }
    }

    // Tail step: retroactive forecast scoring — score analyst_view_history
    // rows at their layer's canonical horizons (src/research/forecast_scoring)
    // against the daily closes refreshed above. Same rationale as the
    // prediction auto-score: no daemon, refresh is the recurring surface.
    // Idempotent — scored cells are never re-examined; pendings fill as
    // horizons elapse.
    let forecast_start = Instant::now();
    match crate::commands::research_forecasts::auto_score_for_refresh(backend) {
        Ok(summary) => {
            info_ln!(
                verbose,
                "✓ Forecast retro-score: {} newly scored ({} pending, {} unscorable); corpus {} scored / {}",
                summary.newly_scored,
                summary.pending,
                summary.unscorable,
                summary.corpus_scored_total,
                summary.corpus_total
            );
            dag_result.add(SourceResult {
                name: "forecast_retro_score".to_string(),
                label: "Forecast Retro-Score".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: Some(summary.newly_scored),
                duration_ms: forecast_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: Some(format!(
                    "{} examined, {} newly scored ({} neutral), {} pending, {} unscorable",
                    summary.examined,
                    summary.newly_scored,
                    summary.neutral_scored,
                    summary.pending,
                    summary.unscorable
                )),
            });
        }
        Err(e) => {
            info_ln!(verbose, "  ⚠ Forecast retro-score failed: {}", e);
            dag_result.add(SourceResult {
                name: "forecast_retro_score".to_string(),
                label: "Forecast Retro-Score".to_string(),
                status: SourceStatus::Failed,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: None,
                duration_ms: forecast_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: None,
            });
        }
    }

    // Tail step: misalignment detection over the freshly scored forecast
    // corpus (must run AFTER the retro-score). A canonical layer whose
    // current wrong-sign streak on one asset reaches the threshold gets an
    // ACTIVE misalignment: probation in convergence, prediction confidence
    // capped, counted into run_health. Recovery (a scored hit) is detected
    // here too. Idempotent.
    let misalign_start = Instant::now();
    match crate::commands::research_forecasts::detect_misalignments_for_refresh(backend) {
        Ok(summary) => {
            if summary.active.is_empty() {
                info_ln!(verbose, "✓ Forecast misalignment detector: none active");
            } else {
                info_ln!(
                    verbose,
                    "⚠ {} active forecast misalignment(s): {}",
                    summary.active.len(),
                    crate::db::forecast_misalignments::format_active_brief(&summary.active)
                );
            }
            let mut detail_parts: Vec<String> = Vec::new();
            if !summary.newly_detected.is_empty() {
                detail_parts.push(format!("new: {}", summary.newly_detected.join(", ")));
            }
            if !summary.newly_recovered.is_empty() {
                detail_parts.push(format!("recovered: {}", summary.newly_recovered.join(", ")));
            }
            if !summary.active.is_empty() {
                detail_parts.push(format!(
                    "active: {}",
                    crate::db::forecast_misalignments::format_active_brief(&summary.active)
                ));
            }
            dag_result.add(SourceResult {
                name: "forecast_misalignment_detector".to_string(),
                label: "Forecast Misalignment Detector".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: Some(summary.active.len()),
                duration_ms: misalign_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: if detail_parts.is_empty() {
                    Some("no active misalignments".to_string())
                } else {
                    Some(detail_parts.join("; "))
                },
            });
        }
        Err(e) => {
            info_ln!(verbose, "  ⚠ Forecast misalignment detection failed: {}", e);
            dag_result.add(SourceResult {
                name: "forecast_misalignment_detector".to_string(),
                label: "Forecast Misalignment Detector".to_string(),
                status: SourceStatus::Failed,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: None,
                duration_ms: misalign_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: None,
            });
        }
    }

    // Idempotent: classify today's regime from the latest scenario state and
    // upsert into `regime_history`. Safe to run multiple times per day; the
    // most recent classification wins via `ON CONFLICT(date) DO UPDATE`.
    let regime_start = Instant::now();
    match crate::db::regime_history::record_today_backend(backend) {
        Ok(Some(regime)) => {
            info_ln!(verbose, "  ↳ Classified regime as '{}'", regime);
            dag_result.add(SourceResult {
                name: "regime_history".to_string(),
                label: "Regime Classification".to_string(),
                status: SourceStatus::Ok,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: Some(1),
                duration_ms: regime_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: None,
                detail: Some(regime),
            });
        }
        Ok(None) => {
            // Postgres backend — skip silently.
        }
        Err(e) => {
            info_ln!(verbose, "  ⚠ Regime classification failed: {}", e);
            dag_result.add(SourceResult {
                name: "regime_history".to_string(),
                label: "Regime Classification".to_string(),
                status: SourceStatus::Failed,
                items_attempted: None,
                items_failed: None,
                failed_symbols: None,
                items_updated: None,
                duration_ms: regime_start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: None,
            });
        }
    }

    // Tail step: housekeeping surfacing — one summary line when curation debt
    // is due (thesis sections past review_by, stale analyst views). Read-only;
    // any error here is swallowed so housekeeping can never fail a refresh.
    if let Some(line) = housekeeping_line(backend) {
        info_ln!(verbose, "{}", line);
    }

    info_ln!(verbose, "\nRefresh complete.");
    dag_result.finalize(pipeline_start.elapsed());
    Ok(dag_result)
}

/// One-line housekeeping summary for the `data refresh` tail: counts thesis
/// sections past their `review_by` date and stale analyst views (default
/// detector thresholds). Returns `None` when nothing is due or when either
/// query fails — housekeeping must never break a refresh.
pub(crate) fn housekeeping_line(backend: &BackendConnection) -> Option<String> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let thesis_due = crate::db::thesis::list_thesis_backend(backend)
        .map(|entries| {
            entries
                .iter()
                .filter(|e| matches!(e.review_by.as_deref(), Some(d) if d <= today.as_str()))
                .count()
        })
        .unwrap_or(0);
    let stale_views =
        crate::commands::views_stale::count_stale_for_refresh(backend).unwrap_or(0);
    if thesis_due == 0 && stale_views == 0 {
        return None;
    }
    Some(format!(
        "🧹 housekeeping: {} thesis section(s) past review, {} stale view(s) — see `analytics thesis review-due` / `analytics views stale`",
        thesis_due, stale_views
    ))
}

fn maybe_stop_for_timeout(
    deadline: Option<Instant>,
    pipeline_start: Instant,
    verbose: bool,
    dag_result: &RefreshResult,
    stage_label: &str,
) -> Option<RefreshResult> {
    let deadline = deadline?;
    if Instant::now() < deadline {
        return None;
    }

    let message = format!(
        "refresh timeout reached after {}. Returning partial results.",
        stage_label
    );
    info_ln!(verbose, "\n⚠ {}", message);
    let mut partial = dag_result.clone();
    partial.finalize_partial(pipeline_start.elapsed(), message);
    Some(partial)
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

                // One-time/idempotent deep-history backfill: the daily path
                // only accumulates one crypto_fng history row per refresh, so
                // sentiment_history stays shallow and starves historical-analog
                // parallels. If we have fewer than the threshold of rows, pull
                // the full Alternative.me history (limit=0 → ~2018→present) and
                // INSERT OR IGNORE it. Safe to re-run; only crypto_fng (CNN's
                // traditional_fng has no deep-history endpoint).
                const CRYPTO_FNG_BACKFILL_THRESHOLD: u32 = 400;
                let existing = sentiment_cache::count_history_backend(backend, "crypto_fng")
                    .unwrap_or(0);
                if existing < CRYPTO_FNG_BACKFILL_THRESHOLD {
                    match crate::data::sentiment::fetch_crypto_fng_history(0) {
                        Ok(history) => {
                            let rows: Vec<(i64, u8, String)> = history
                                .into_iter()
                                .map(|r| (r.timestamp, r.value, r.classification))
                                .collect();
                            match sentiment_cache::backfill_history_backend(
                                backend,
                                "crypto_fng",
                                &rows,
                            ) {
                                Ok(n) => info_ln!(
                                    verbose,
                                    "✓ Sentiment crypto_fng backfill (+{} history rows)",
                                    n
                                ),
                                Err(e) => warn_ln!(
                                    verbose,
                                    "crypto_fng history backfill persist failed: {}",
                                    e
                                ),
                            }
                        }
                        Err(e) => {
                            warn_ln!(verbose, "crypto_fng history backfill fetch failed: {}", e)
                        }
                    }
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

/// Forward-population for the news-related enrichment tables. Called
/// after `store_news_result` writes any new articles. Each pass is wrapped
/// in `Result` and a failure is logged but does not abort the refresh — the
/// goal is steady-state population, not a hard dependency.
fn run_news_enrichment_passes(
    backend: &BackendConnection,
    verbose: bool,
    news_refreshed: bool,
) {
    if !news_refreshed {
        return;
    }
    // (1) news_silence_baselines: rebuild from trailing 90d news_cache.
    if let Err(err) =
        crate::commands::news_silence::rebuild_baselines_silent(backend, 90)
    {
        warn_ln!(verbose, "news-silence baseline refresh failed: {}", err);
    }
    // (R3 cull) narrative_money_history append removed: the table was
    // write-only (no reader) — the live narrative-divergence report computes
    // from news_cache + predictions directly. Saves a per-refresh write.
    // (2) news_source_accuracy: replay the trailing-30d window so any newly
    // scored predictions get reflected in the ledger.
    if let Err(err) = crate::db::news_source_accuracy::backfill_accuracy_backend(
        backend,
        Some(30),
        false,
    ) {
        warn_ln!(verbose, "news-source accuracy refresh failed: {}", err);
    }
}

fn warn_unhealthy_rss_feeds(backend: &BackendConnection, verbose: bool) {
    if !verbose {
        return;
    }
    let Ok(rows) = rss_feed_health::list_feed_health_backend(backend) else {
        return;
    };
    let degraded: Vec<_> = rows
        .iter()
        .filter(|row| row.status == "degraded")
        .collect();
    let disabled: Vec<_> = rows
        .iter()
        .filter(|row| row.status == "disabled")
        .collect();

    if !degraded.is_empty() {
        warn_ln!(
            verbose,
            "⚠ {} RSS feed{} degraded: {}",
            degraded.len(),
            if degraded.len() == 1 { "" } else { "s" },
            degraded
                .iter()
                .map(|row| feed_health_label(row))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
    if !disabled.is_empty() {
        warn_ln!(
            verbose,
            "⚠ {} RSS feed{} disabled: {}",
            disabled.len(),
            if disabled.len() == 1 { "" } else { "s" },
            disabled
                .iter()
                .map(|row| feed_health_label(row))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
}

fn feed_health_label(row: &rss_feed_health::RssFeedHealth) -> String {
    let last = row
        .last_failure_at
        .as_deref()
        .map(|value| format!("; last failure {value}"))
        .unwrap_or_default();
    let reason = row
        .last_failure_reason
        .as_deref()
        .map(|value| format!("; {value}"))
        .unwrap_or_default();
    format!(
        "{} ({} consecutive failures{}{})",
        row.feed_id, row.consecutive_failures, last, reason
    )
}

#[allow(clippy::too_many_arguments)]
fn store_news_result(
    backend: &BackendConnection,
    verbose: bool,
    rss_refresh: bool,
    brave_refresh: bool,
    in_plan: bool,
    rss_data: Option<(crate::data::rss::FeedFetchReport, Duration)>,
    brave_news_data: BraveNewsData,
    dag_result: &mut RefreshResult,
) {
    let mut inserted = 0usize;
    let mut brave_inserted = 0usize;
    let mut brave_query_count = 0usize;
    let mut news_elapsed = Duration::ZERO;
    let mut rss_feed_errors: Vec<String> = Vec::new();
    let mut rss_feeds_attempted = 0usize;
    let mut rss_feeds_disabled = 0usize;
    let mut brave_query_errors = 0usize;

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
                    brave_query_errors += 1;
                    warn_ln!(verbose, "Brave news query failed ({}): {}", query, e);
                }
            }
        }
    }

    if let Some((report, elapsed)) = rss_data {
        if elapsed > news_elapsed {
            news_elapsed = elapsed;
        }
        rss_feeds_attempted = report.attempted;
        rss_feeds_disabled = report.skipped.len();
        for skipped in &report.skipped {
            warn_ln!(
                verbose,
                "RSS feed disabled, skipping ({}): {}",
                skipped.feed_name,
                skipped.reason
            );
        }
        for success in &report.successes {
            if let Err(err) = rss_feed_health::record_feed_success_backend(
                backend,
                &success.feed_name,
            ) {
                warn_ln!(
                    verbose,
                    "RSS feed health update failed ({}): {}",
                    success.feed_name,
                    err
                );
            }
        }
        for err in &report.errors {
            warn_ln!(
                verbose,
                "RSS feed failed ({}): {}",
                err.feed_name,
                err.error
            );
            if let Err(update_err) =
                rss_feed_health::record_feed_failure_backend(backend, &err.feed_name, &err.error)
            {
                warn_ln!(
                    verbose,
                    "RSS feed health update failed ({}): {}",
                    err.feed_name,
                    update_err
                );
            }
            rss_feed_errors.push(format!("{} ({})", err.feed_name, err.feed_url));
        }
        warn_unhealthy_rss_feeds(backend, verbose);
        for item in &report.items {
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
                item.description.as_deref(),
                &[],
            )
            .is_ok()
            {
                inserted += 1;
            }
        }
    }

    if brave_refresh || rss_refresh {
        let total_attempted = brave_query_count + rss_feeds_attempted;
        let total_failed = brave_query_errors + rss_feed_errors.len();
        let only_disabled_rss =
            rss_refresh && !brave_refresh && rss_feeds_attempted == 0 && rss_feeds_disabled > 0;
        let status = if only_disabled_rss {
            SourceStatus::Skipped
        } else if inserted == 0 && total_failed > 0 {
            SourceStatus::Failed
        } else if total_failed > 0 || inserted == 0 {
            SourceStatus::PartialSuccess
        } else {
            SourceStatus::Ok
        };
        let error = if only_disabled_rss {
            Some("all configured RSS feeds are disabled".to_string())
        } else if total_failed > 0 {
            Some(format!(
                "news ingest degraded: {} feed/query failures, {} article(s) inserted",
                total_failed, inserted
            ))
        } else if inserted == 0 {
            Some("news ingest returned zero articles".to_string())
        } else {
            None
        };
        let detail = if !rss_feed_errors.is_empty() || rss_feeds_disabled > 0 {
            let mut parts = Vec::new();
            if !rss_feed_errors.is_empty() {
                parts.push(format!("rss feed failures: {}", rss_feed_errors.join(" | ")));
            }
            if rss_feeds_disabled > 0 {
                parts.push(format!("rss feeds disabled: {}", rss_feeds_disabled));
            }
            Some(parts.join("; "))
        } else {
            None
        };
        if brave_refresh && rss_refresh {
            info_ln!(
                verbose,
                "{} News ({} articles from {} Brave queries + RSS, {} failures)",
                match status {
                    SourceStatus::Ok => "✓",
                    SourceStatus::PartialSuccess => "⚠",
                    SourceStatus::Failed => "✗",
                    SourceStatus::Skipped => "⊘",
                    SourceStatus::Deferred => "↷",
                },
                inserted,
                brave_query_count,
                total_failed
            );
        } else if brave_refresh {
            info_ln!(
                verbose,
                "{} News ({} articles from {} Brave queries, {} failures)",
                match status {
                    SourceStatus::Ok => "✓",
                    SourceStatus::PartialSuccess => "⚠",
                    SourceStatus::Failed => "✗",
                    SourceStatus::Skipped => "⊘",
                    SourceStatus::Deferred => "↷",
                },
                brave_inserted,
                brave_query_count,
                total_failed
            );
        } else {
            info_ln!(
                verbose,
                "{} News ({} articles via RSS, {} feed failures)",
                match status {
                    SourceStatus::Ok => "✓",
                    SourceStatus::PartialSuccess => "⚠",
                    SourceStatus::Failed => "✗",
                    SourceStatus::Skipped => "⊘",
                    SourceStatus::Deferred => "↷",
                },
                inserted,
                rss_feed_errors.len()
            );
            if rss_feeds_disabled > 0 {
                warn_ln!(verbose, "{} RSS feed(s) disabled and skipped", rss_feeds_disabled);
            }
        }
        dag_result.add(SourceResult {
            name: "news".to_string(),
            label: "News".to_string(),
            status,
            items_attempted: if total_attempted > 0 {
                Some(total_attempted)
            } else {
                None
            },
            items_failed: if total_failed > 0 {
                Some(total_failed)
            } else {
                None
            },
            failed_symbols: None,
            items_updated: Some(inserted),
            duration_ms: news_elapsed.as_millis() as u64,
            reason: None,
            age_minutes: None,
            error,
            detail,
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
            // Live fetch failed for every contract. Fall back to last-good
            // cache (mirrors the COMEX/supply pattern): if cot_cache still
            // holds prior reports, surface them with an explicit as-of date
            // so downstream report code never silently cites stale COT data.
            let cached_reports = cot_cache::get_all_latest_backend(backend).unwrap_or_default();
            if let Some(note) = cot_stale_cache_note(&cached_reports, chrono::Utc::now().date_naive())
            {
                warn_ln!(verbose, "⚠ COT ({note})");
                dag_result.add(SourceResult {
                    name: "cot".to_string(),
                    label: "COT (CFTC)".to_string(),
                    status: SourceStatus::Ok,
                    items_attempted: Some(cot::COT_CONTRACTS.len()),
                    items_failed: Some(failed_contracts.len()),
                    failed_symbols: (!failed_contracts.is_empty()).then_some(failed_contracts),
                    items_updated: Some(0),
                    duration_ms: cot_start.elapsed().as_millis() as u64,
                    reason: Some(note.clone()),
                    age_minutes: None,
                    error: None,
                    detail: Some(staleness_detail.unwrap_or(note)),
                });
            } else {
                info_ln!(verbose, "✗ COT (all failed, no cache)");
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
                    error: Some("all contracts failed, no cache".to_string()),
                    detail: staleness_detail,
                });
            }
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
                            quarantined: false,
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
                        quarantined: false,
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
                        if economic_cache::get_latest_backend(backend, series_id)
                            .ok()
                            .flatten()
                            .is_some()
                        {
                            cache_fallback_count += 1;
                        }
                    }
                }
            }

            let status = if failed_series.is_empty() {
                SourceStatus::Ok
            } else if updated > 0 {
                SourceStatus::PartialSuccess
            } else {
                SourceStatus::Failed
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
                items_failed: Some(failed_series.len()),
                failed_symbols: Some(failed_series.clone()),
                items_updated: Some(updated),
                duration_ms: elapsed.as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: if failed_series.is_empty() {
                    None
                } else {
                    Some(format!("{} series failed", failed_series.len()))
                },
                detail: detail_msg,
            });
        }
    } else if in_plan {
        if has_key {
            info_ln!(verbose, "⊘ FRED (fresh, skipping)");
        } else {
            info_ln!(verbose, "⊘ FRED (keyless fallbacks fresh, skipping)");
        }
        dag_result.add(SourceResult {
            name: "fred".to_string(),
            label: "FRED".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some(if has_key {
                "fresh".to_string()
            } else {
                "keyless fallbacks fresh".to_string()
            }),
            age_minutes: None,
            error: None,
            detail: if has_key {
                None
            } else {
                Some("primary FRED series require an API key".to_string())
            },
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

fn store_real_yields_result(
    backend: &BackendConnection,
    config: &Config,
    verbose: bool,
    in_plan: bool,
    rt: &tokio::runtime::Runtime,
    dag_result: &mut RefreshResult,
) {
    if !in_plan {
        info_ln!(verbose, "⊘ real-yields (skipped)");
        dag_result.add(SourceResult {
            name: "real_yields".to_string(),
            label: "Real Yields".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("not in plan".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
        return;
    }

    let start = Instant::now();
    let api_key = config
        .fred_api_key
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if api_key.is_empty() {
        info_ln!(verbose, "⊘ real-yields (no FRED key — degraded)");
        dag_result.add(SourceResult {
            name: "real_yields".to_string(),
            label: "Real Yields".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: start.elapsed().as_millis() as u64,
            reason: Some("fred_api_key absent".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
        return;
    }

    let series_ids = real_yields_data::all_series_ids();
    let mut all_obs: Vec<real_yields_data::RealYieldObservation> = Vec::new();
    let mut series_with_data = 0usize;
    for sid in &series_ids {
        // 400-day window: the G10 OECD long-rate series are MONTHLY, so a short
        // window can return zero recent prints; a wider one guarantees several
        // monthly observations are present for the as-of forward-fill join.
        match rt.block_on(real_yields_data::fetch_series_history(&api_key, sid, 400)) {
            Ok(obs) if !obs.is_empty() => {
                series_with_data += 1;
                all_obs.extend(obs);
            }
            _ => {}
        }
    }

    match crate::db::real_yields_history::upsert_observations_backend(backend, &all_obs) {
        Ok(()) => {
            info_ln!(
                verbose,
                "✓ real-yields ({} obs, {}/{} series)",
                all_obs.len(),
                series_with_data,
                series_ids.len()
            );
            dag_result.add(SourceResult {
                name: "real_yields".to_string(),
                label: "Real Yields".to_string(),
                status: if series_with_data == 0 {
                    SourceStatus::Failed
                } else {
                    SourceStatus::Ok
                },
                items_attempted: Some(series_ids.len()),
                items_failed: Some(series_ids.len().saturating_sub(series_with_data)),
                failed_symbols: None,
                items_updated: Some(all_obs.len()),
                duration_ms: start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: if series_with_data == 0 {
                    Some("no observations returned".to_string())
                } else {
                    None
                },
                detail: Some(format!(
                    "{}/{} series returned data; {} rows persisted",
                    series_with_data,
                    series_ids.len(),
                    all_obs.len()
                )),
            });
        }
        Err(e) => {
            info_ln!(verbose, "✗ real-yields (cache write failed: {})", e);
            dag_result.add(SourceResult {
                name: "real_yields".to_string(),
                label: "Real Yields".to_string(),
                status: SourceStatus::Failed,
                items_attempted: Some(series_ids.len()),
                items_failed: Some(series_ids.len()),
                failed_symbols: None,
                items_updated: None,
                duration_ms: start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: None,
            });
        }
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

/// Refresh the Yahoo options chain + GEX snapshot for the default
/// symbol set. SQLite-only; degrades silently on a Postgres backend.
fn store_options_result(
    backend: &BackendConnection,
    verbose: bool,
    in_plan: bool,
    dag_result: &mut RefreshResult,
) {
    let start = Instant::now();
    if !in_plan {
        info_ln!(verbose, "⊘ Options (cadence deferred)");
        dag_result.add(SourceResult {
            name: "options".to_string(),
            label: "Options chain + GEX".to_string(),
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
        return;
    }
    let Some(conn) = backend.sqlite_native() else {
        info_ln!(verbose, "⊘ Options (postgres backend; sqlite-only)");
        dag_result.add(SourceResult {
            name: "options".to_string(),
            label: "Options chain + GEX".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("postgres backend not yet supported".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
        return;
    };

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            warn_ln!(verbose, "Options refresh tokio build failed: {}", e);
            return;
        }
    };

    let mut count = 0;
    let mut failed: Vec<String> = Vec::new();
    for sym in crate::data::options::DEFAULT_OPTIONS_SYMBOLS {
        match rt.block_on(crate::data::options::fetch_options_chain(sym)) {
            Ok(snapshot) => {
                let gex = crate::data::options::compute_gex(&snapshot);
                if let Err(e) = crate::db::options_chain_snapshots::insert_chain(
                    conn,
                    &snapshot.rows,
                    &snapshot.fetched_at,
                ) {
                    warn_ln!(verbose, "Options chain persist failed for {}: {}", sym, e);
                    failed.push((*sym).to_string());
                    continue;
                }
                if let Err(e) = crate::db::gex_snapshots::insert(conn, &gex) {
                    warn_ln!(verbose, "GEX persist failed for {}: {}", sym, e);
                    failed.push((*sym).to_string());
                    continue;
                }
                count += 1;
            }
            Err(e) => {
                warn_ln!(verbose, "Options fetch failed for {}: {}", sym, e);
                failed.push((*sym).to_string());
            }
        }
    }

    if count > 0 {
        info_ln!(verbose, "✓ Options ({} chains)", count);
        dag_result.add(SourceResult {
            name: "options".to_string(),
            label: "Options chain + GEX".to_string(),
            status: SourceStatus::Ok,
            items_attempted: Some(crate::data::options::DEFAULT_OPTIONS_SYMBOLS.len()),
            items_failed: Some(failed.len()),
            failed_symbols: if failed.is_empty() {
                None
            } else {
                Some(failed)
            },
            items_updated: Some(count),
            duration_ms: start.elapsed().as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
        return;
    }

    // All live fetches failed. Fall back to last-good cache (mirrors the
    // COMEX/supply pattern): if options_chain_snapshots still holds prior
    // snapshots, surface them with an explicit as-of date so downstream
    // report code never silently cites stale options/GEX data.
    let latest_fetched_ats: Vec<String> = crate::data::options::DEFAULT_OPTIONS_SYMBOLS
        .iter()
        .filter_map(|sym| {
            crate::db::options_chain_snapshots::latest_fetched_at(conn, sym)
                .ok()
                .flatten()
        })
        .collect();

    if let Some(note) = options_stale_cache_note(&latest_fetched_ats) {
        warn_ln!(verbose, "⚠ Options ({note})");
        dag_result.add(SourceResult {
            name: "options".to_string(),
            label: "Options chain + GEX".to_string(),
            status: SourceStatus::Ok,
            items_attempted: Some(crate::data::options::DEFAULT_OPTIONS_SYMBOLS.len()),
            items_failed: Some(failed.len()),
            failed_symbols: if failed.is_empty() {
                None
            } else {
                Some(failed)
            },
            items_updated: Some(0),
            duration_ms: start.elapsed().as_millis() as u64,
            reason: Some(note.clone()),
            age_minutes: None,
            error: None,
            detail: Some(note),
        });
    } else {
        info_ln!(verbose, "✗ Options (no chains persisted, no cache)");
        dag_result.add(SourceResult {
            name: "options".to_string(),
            label: "Options chain + GEX".to_string(),
            status: SourceStatus::Failed,
            items_attempted: Some(crate::data::options::DEFAULT_OPTIONS_SYMBOLS.len()),
            items_failed: Some(failed.len()),
            failed_symbols: if failed.is_empty() {
                None
            } else {
                Some(failed)
            },
            items_updated: Some(0),
            duration_ms: start.elapsed().as_millis() as u64,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
    }
}

/// Persist capital-flow observations from the configured provider (F59 scaffold).
///
/// Defaults to the `NoopProvider`, which logs "capital flows provider not
/// configured" and inserts zero rows. The live `etf_com_csv` (HTML
/// scraper) and `sec_edgar_13f` providers each enforce their own
/// cadence throttle: `etf_com_csv` skips when a row sourced from
/// `etf.com/` was inserted within the last 12 hours; `sec_edgar_13f`
/// skips when an `institutional_13f` row was inserted within the last
/// 80 days. Provider failures are surfaced via the DAG result rather
/// than panicking.
fn store_flows_result(
    backend: &BackendConnection,
    verbose: bool,
    in_plan: bool,
    dag_result: &mut RefreshResult,
) {
    let start = Instant::now();
    if !in_plan {
        info_ln!(verbose, "⊘ flows (skipped)");
        dag_result.add(SourceResult {
            name: "flows".to_string(),
            label: "Capital Flows".to_string(),
            status: SourceStatus::Skipped,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: None,
            duration_ms: 0,
            reason: Some("not in plan".to_string()),
            age_minutes: None,
            error: None,
            detail: None,
        });
        return;
    }

    let provider = crate::data::flows::provider_from_env();
    let provider_name = provider.name().to_string();

    // Quarterly-cadence guard for the SEC EDGAR provider: 13F-HR
    // filings only update once per quarter, so re-walking the
    // submissions feed on every refresh is wasted bandwidth (and a
    // good way to get a 429 from data.sec.gov). Skip when the most
    // recent successful fetch landed within ~80 days.
    // Daily-cadence guard for the ETF.com HTML scraper: re-running
    // within 12 hours produces effectively the same daily row and
    // wastes bandwidth on a fragile public page. Skip when the most
    // recent successful fetch landed within 12 hours.
    if provider_name == "etf_com_csv" {
        match crate::db::capital_flows::latest_fetched_at_for_source_prefix(
            backend.sqlite(),
            "etf.com/",
        ) {
            Ok(Some(last)) => {
                if let Some(age_hours) = crate::data::flows::hours_since_rfc3339(&last) {
                    if age_hours < 12 {
                        let note = format!(
                            "provider={provider_name}; cadence deferred (last fetch {age_hours}h ago, 12h throttle)"
                        );
                        info_ln!(verbose, "⊘ flows ({note})");
                        dag_result.add(SourceResult {
                            name: "flows".to_string(),
                            label: "Capital Flows".to_string(),
                            status: SourceStatus::Skipped,
                            items_attempted: Some(0),
                            items_failed: Some(0),
                            failed_symbols: None,
                            items_updated: Some(0),
                            duration_ms: start.elapsed().as_millis() as u64,
                            reason: Some("daily-cadence throttle".to_string()),
                            age_minutes: Some(age_hours * 60),
                            error: None,
                            detail: Some(note),
                        });
                        return;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                info_ln!(verbose, "flows cadence check failed (continuing): {e}");
            }
        }
    }

    if provider_name == "sec_edgar_13f" {
        match crate::db::capital_flows::latest_fetched_at_for_type(
            backend.sqlite(),
            "institutional_13f",
        ) {
            Ok(Some(last)) => {
                if let Some(age_days) = crate::data::flows::days_since_rfc3339(&last) {
                    if age_days < 80 {
                        let note = format!(
                            "provider={provider_name}; throttled (last fetch {age_days}d ago, quarterly cadence)"
                        );
                        info_ln!(verbose, "⊘ flows ({note})");
                        dag_result.add(SourceResult {
                            name: "flows".to_string(),
                            label: "Capital Flows".to_string(),
                            status: SourceStatus::Skipped,
                            items_attempted: Some(0),
                            items_failed: Some(0),
                            failed_symbols: None,
                            items_updated: Some(0),
                            duration_ms: start.elapsed().as_millis() as u64,
                            reason: Some("quarterly-cadence throttle".to_string()),
                            age_minutes: Some(age_days * 24 * 60),
                            error: None,
                            detail: Some(note),
                        });
                        return;
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                info_ln!(verbose, "flows cadence check failed (continuing): {e}");
            }
        }
    }

    match provider.fetch(None) {
        Ok(result) => {
            let mut inserted = 0usize;
            let mut insert_error: Option<String> = None;
            for flow in &result.flows {
                match crate::db::capital_flows::insert(backend.sqlite(), flow) {
                    Ok(_) => inserted += 1,
                    Err(e) => {
                        insert_error = Some(e.to_string());
                        break;
                    }
                }
            }
            let status = if insert_error.is_some() {
                SourceStatus::Failed
            } else if result.flows.is_empty() {
                // Noop / no-data path: still Ok — the pipeline succeeded.
                SourceStatus::Ok
            } else {
                SourceStatus::Ok
            };
            let note = if result.note.is_empty() {
                format!("provider={provider_name}")
            } else {
                format!("provider={provider_name}; {}", result.note)
            };
            info_ln!(
                verbose,
                "✓ flows ({} fetched, {} inserted; {})",
                result.flows.len(),
                inserted,
                note
            );
            dag_result.add(SourceResult {
                name: "flows".to_string(),
                label: "Capital Flows".to_string(),
                status,
                items_attempted: Some(result.flows.len()),
                items_failed: Some(result.flows.len().saturating_sub(inserted)),
                failed_symbols: None,
                items_updated: Some(inserted),
                duration_ms: start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: insert_error,
                detail: Some(note),
            });
        }
        Err(e) => {
            info_ln!(verbose, "✗ flows (provider {} failed: {})", provider_name, e);
            dag_result.add(SourceResult {
                name: "flows".to_string(),
                label: "Capital Flows".to_string(),
                status: SourceStatus::Failed,
                items_attempted: Some(0),
                items_failed: Some(0),
                failed_symbols: None,
                items_updated: Some(0),
                duration_ms: start.elapsed().as_millis() as u64,
                reason: None,
                age_minutes: None,
                error: Some(e.to_string()),
                detail: Some(format!("provider={provider_name}")),
            });
        }
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

    fn housekeeping_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn housekeeping_line_absent_when_nothing_due() {
        let backend = housekeeping_backend();
        assert_eq!(housekeeping_line(&backend), None);

        // A thesis section with a FUTURE review date is not due.
        crate::db::thesis::upsert_thesis_backend(&backend, "gold", "thesis text", Some("high"))
            .unwrap();
        crate::db::thesis::set_review_by_backend(&backend, "gold", Some("2999-01-01")).unwrap();
        assert_eq!(housekeeping_line(&backend), None);
    }

    #[test]
    fn housekeeping_line_present_when_thesis_review_due() {
        let backend = housekeeping_backend();
        crate::db::thesis::upsert_thesis_backend(&backend, "gold", "thesis text", Some("high"))
            .unwrap();
        crate::db::thesis::set_review_by_backend(&backend, "gold", Some("2020-01-01")).unwrap();
        let line = housekeeping_line(&backend).expect("expected housekeeping line");
        assert!(line.contains("1 thesis section(s) past review"), "{line}");
        assert!(line.contains("0 stale view(s)"), "{line}");
        assert!(line.contains("analytics thesis review-due"), "{line}");
    }

    #[test]
    fn housekeeping_line_counts_stale_views() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        // Held BTC position.
        let tx = crate::models::transaction::NewTransaction {
            symbol: "BTC".to_string(),
            category: crate::models::asset::AssetCategory::Crypto,
            tx_type: crate::models::transaction::TxType::Buy,
            quantity: dec!(1),
            price_per: dec!(50000),
            currency: "USD".to_string(),
            date: "2026-01-01".to_string(),
            notes: None,
        };
        crate::db::transactions::insert_transaction_backend(&backend, &tx).unwrap();
        // 40-day-old medium view with a +25% move since.
        crate::db::analyst_views::upsert_view_backend(
            &backend, "medium", "BTC", "bull", 3, "synthetic", None, None, None,
        )
        .unwrap();
        let stamp = (chrono::Utc::now() - chrono::Duration::days(40))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let view_date: String = stamp.chars().take(10).collect();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        {
            let conn = backend.sqlite_native().unwrap();
            conn.execute(
                "UPDATE analyst_views SET updated_at = ?1 WHERE analyst = 'medium' AND asset = 'BTC'",
                rusqlite::params![stamp],
            )
            .unwrap();
            for (date, close) in [(view_date.as_str(), dec!(80000)), (today.as_str(), dec!(100000))]
            {
                crate::db::price_history::upsert_history(
                    conn,
                    "BTC",
                    "test",
                    &[crate::models::price::HistoryRecord {
                        date: date.to_string(),
                        close,
                        volume: None,
                        open: None,
                        high: None,
                        low: None,
                    }],
                )
                .unwrap();
            }
        }
        let line = housekeeping_line(&backend).expect("expected housekeeping line");
        assert!(line.contains("1 stale view(s)"), "{line}");
        assert!(line.contains("analytics views stale"), "{line}");
    }

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

    // ── Spot-fallback chain selection ───────────────────────────────────

    #[test]
    fn spot_fallback_symbol_mapping() {
        assert_eq!(
            spot_fallback_for_symbol("BTC"),
            Some(SpotFallbackSource::MempoolBtc)
        );
        assert_eq!(
            spot_fallback_for_symbol("btc-usd"),
            Some(SpotFallbackSource::MempoolBtc)
        );
        assert_eq!(
            spot_fallback_for_symbol("GC=F"),
            Some(SpotFallbackSource::GeckoTerminalXaut)
        );
        // No fallback for anything else — spot-only last resorts, not a
        // general-purpose price source.
        assert_eq!(spot_fallback_for_symbol("ETH"), None);
        assert_eq!(spot_fallback_for_symbol("SPY"), None);
        assert_eq!(spot_fallback_for_symbol("SI=F"), None);
    }

    #[test]
    fn spot_fallback_not_selected_when_primary_succeeded() {
        let symbols = vec![
            ("BTC".to_string(), AssetCategory::Crypto),
            ("GC=F".to_string(), AssetCategory::Commodity),
        ];
        // Both symbols have live quotes from the primary chain.
        let fetched: HashSet<String> = ["BTC", "GC=F"].iter().map(|s| s.to_string()).collect();
        assert!(spot_fallback_targets(&symbols, &fetched).is_empty());
    }

    #[test]
    fn spot_fallback_selected_when_primary_failed() {
        let symbols = vec![
            ("BTC".to_string(), AssetCategory::Crypto),
            ("GC=F".to_string(), AssetCategory::Commodity),
            ("SPY".to_string(), AssetCategory::Equity),
        ];
        // Nothing fetched — BTC and GC=F get fallbacks, SPY has none.
        let fetched = HashSet::new();
        let targets = spot_fallback_targets(&symbols, &fetched);
        assert_eq!(targets.len(), 2);
        assert!(targets.contains(&("BTC".to_string(), SpotFallbackSource::MempoolBtc)));
        assert!(targets.contains(&("GC=F".to_string(), SpotFallbackSource::GeckoTerminalXaut)));
    }

    #[test]
    fn spot_fallback_only_for_the_failed_symbol() {
        let symbols = vec![
            ("BTC".to_string(), AssetCategory::Crypto),
            ("GC=F".to_string(), AssetCategory::Commodity),
        ];
        // BTC succeeded via CoinGecko/Yahoo; only gold needs the fallback.
        let fetched: HashSet<String> = std::iter::once("BTC".to_string()).collect();
        let targets = spot_fallback_targets(&symbols, &fetched);
        assert_eq!(
            targets,
            vec![("GC=F".to_string(), SpotFallbackSource::GeckoTerminalXaut)]
        );
    }

    #[test]
    fn spot_fallback_skips_cash() {
        let symbols = vec![("BTC".to_string(), AssetCategory::Cash)];
        assert!(spot_fallback_targets(&symbols, &HashSet::new()).is_empty());
    }

    // ── Divergence guard ────────────────────────────────────────────────

    #[test]
    fn divergence_guard_accepts_two_percent() {
        // 62580 → 63831.6 is exactly +2%
        let res = check_spot_fallback_divergence(dec!(63831.6), Some(dec!(62580)));
        assert_eq!(res, Ok(Some(dec!(2))));
    }

    #[test]
    fn divergence_guard_rejects_six_percent() {
        // 4000 → 4240 is +6% — REJECTED, never stored
        let res = check_spot_fallback_divergence(dec!(4240), Some(dec!(4000)));
        assert_eq!(res, Err(dec!(6)));
    }

    #[test]
    fn divergence_guard_rejects_six_percent_down_move() {
        let res = check_spot_fallback_divergence(dec!(3760), Some(dec!(4000)));
        assert_eq!(res, Err(dec!(6)));
    }

    #[test]
    fn divergence_guard_boundary_five_percent_accepted() {
        let res = check_spot_fallback_divergence(dec!(4200), Some(dec!(4000)));
        assert_eq!(res, Ok(Some(dec!(5))));
    }

    #[test]
    fn divergence_guard_accepts_without_baseline() {
        // No stored close (or a zero close) → nothing to validate against.
        assert_eq!(check_spot_fallback_divergence(dec!(62631), None), Ok(None));
        assert_eq!(
            check_spot_fallback_divergence(dec!(62631), Some(Decimal::ZERO)),
            Ok(None)
        );
    }

    // ── Summary-line provenance ─────────────────────────────────────────

    #[test]
    fn fallback_suffix_empty_when_no_fallbacks() {
        assert_eq!(fallback_summary_suffix(&[]), "");
    }

    #[test]
    fn fallback_suffix_lists_provenance() {
        let notes = vec![
            "BTC←mempool.space (block 953254)".to_string(),
            "GC=F←geckoterminal-xaut".to_string(),
        ];
        assert_eq!(
            fallback_summary_suffix(&notes),
            ", 2 via fallback: BTC←mempool.space (block 953254), GC=F←geckoterminal-xaut"
        );
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

        // Re-evaluate — both indicators are still active (PR #710 keeps
        // 'triggered' indicators in the refresh loop so last_value/last_checked
        // stay current). The already-triggered one continues to satisfy >3000,
        // and the <2500 one still does not trigger.
        let (checked2, triggered2) = evaluate_situation_indicators(&backend, false).unwrap();
        assert_eq!(checked2, 2);
        assert_eq!(triggered2, 1);
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

    fn cot_entry(report_date: &str) -> crate::db::cot_cache::CotCacheEntry {
        crate::db::cot_cache::CotCacheEntry {
            cftc_code: "088691".to_string(),
            report_date: report_date.to_string(),
            open_interest: 0,
            managed_money_long: 0,
            managed_money_short: 0,
            managed_money_net: 0,
            commercial_long: 0,
            commercial_short: 0,
            commercial_net: 0,
            fetched_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn cot_stale_cache_note_age_stamps_when_cache_present() {
        // Fetch failed but cache has a report dated 10 days before "today".
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 30).unwrap();
        let reports = vec![cot_entry("2026-03-20"), cot_entry("2026-03-13")];
        let note = cot_stale_cache_note(&reports, today).expect("expected stale note");
        assert!(note.contains("2026-03-20"), "uses newest report date: {note}");
        assert!(note.contains("10 days old"), "age-stamped: {note}");
        assert!(note.to_lowercase().contains("cached"));
    }

    #[test]
    fn cot_stale_cache_note_none_when_no_cache() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 30).unwrap();
        assert!(cot_stale_cache_note(&[], today).is_none());
    }

    #[test]
    fn options_stale_cache_note_uses_most_recent_snapshot() {
        let fetched = vec![
            "2026-03-28T10:00:00Z".to_string(),
            "2026-03-29T10:00:00Z".to_string(),
            "2026-03-27T10:00:00Z".to_string(),
        ];
        let note = options_stale_cache_note(&fetched).expect("expected stale note");
        assert!(note.contains("2026-03-29T10:00:00Z"), "uses newest: {note}");
        assert!(note.to_lowercase().contains("cached"));
    }

    #[test]
    fn options_stale_cache_note_none_when_no_cache() {
        assert!(options_stale_cache_note(&[]).is_none());
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
        assert_eq!(plan.selected_task_names().len(), 20);
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
        assert_eq!(plan.selected_task_names().len(), 18);
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

    #[test]
    fn fred_keyless_fallbacks_need_refresh_only_tracks_supported_series() {
        let conn = crate::db::open_in_memory();
        let backend = crate::db::backend::BackendConnection::Sqlite { conn };

        assert!(fred_keyless_fallbacks_need_refresh(&backend).unwrap());

        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        for (series_id, value) in [("DGS10_YAHOO", dec!(4.21)), ("GDPNOW_WEB", dec!(2.1))] {
            economic_cache::upsert_observation_backend(
                &backend,
                &economic_cache::EconomicObservation {
                    series_id: series_id.to_string(),
                    date: today.clone(),
                    value,
                    fetched_at: "2026-04-21T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        }

        assert!(!fred_keyless_fallbacks_need_refresh(&backend).unwrap());
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

    #[test]
    fn store_news_result_marks_failed_when_all_rss_feeds_fail() {
        let backend = crate::db::backend::BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };
        let mut dag_result = RefreshResult::new();

        store_news_result(
            &backend,
            false,
            true,
            false,
            true,
            Some((
                rss::FeedFetchReport {
                    items: vec![],
                    errors: vec![rss::FeedError {
                        feed_name: "Bloomberg Commodities".to_string(),
                        feed_url: "https://feeds.bloomberg.com/commodities/news.rss".to_string(),
                        error: "404 Not Found".to_string(),
                    }],
                    attempted: 1,
                    ..rss::FeedFetchReport::default()
                },
                Duration::from_secs(1),
            )),
            None,
            &mut dag_result,
        );

        assert_eq!(dag_result.sources.len(), 1);
        assert_eq!(dag_result.sources[0].status, SourceStatus::Failed);
        assert_eq!(dag_result.sources[0].items_updated, Some(0));
        assert_eq!(dag_result.sources[0].items_failed, Some(1));
        assert!(dag_result.sources[0]
            .error
            .as_deref()
            .is_some_and(|msg| msg.contains("news ingest degraded")));

        let feed_health = rss_feed_health::list_feed_health_backend(&backend).unwrap();
        assert_eq!(feed_health.len(), 1);
        assert_eq!(feed_health[0].feed_id, "Bloomberg Commodities");
        assert_eq!(feed_health[0].consecutive_failures, 1);
        assert_eq!(feed_health[0].total_failures, 1);
    }

    #[test]
    fn store_news_result_skips_when_all_rss_feeds_disabled() {
        let backend = crate::db::backend::BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };
        let mut dag_result = RefreshResult::new();

        store_news_result(
            &backend,
            false,
            true,
            false,
            true,
            Some((
                rss::FeedFetchReport {
                    attempted: 0,
                    skipped: vec![rss::FeedSkipped {
                        feed_name: "Bloomberg Commodities".to_string(),
                        feed_url: "https://feeds.bloomberg.com/commodities/news.rss".to_string(),
                        reason: "disabled after repeated failures".to_string(),
                    }],
                    ..rss::FeedFetchReport::default()
                },
                Duration::from_secs(1),
            )),
            None,
            &mut dag_result,
        );

        assert_eq!(dag_result.sources.len(), 1);
        assert_eq!(dag_result.sources[0].status, SourceStatus::Skipped);
        assert_eq!(
            dag_result.sources[0].error.as_deref(),
            Some("all configured RSS feeds are disabled")
        );
    }

    #[test]
    fn maybe_stop_for_timeout_returns_partial_result_after_deadline() {
        let mut dag_result = RefreshResult::new();
        dag_result.add(SourceResult {
            name: "prices".to_string(),
            label: "Prices".to_string(),
            status: SourceStatus::Ok,
            items_attempted: None,
            items_failed: None,
            failed_symbols: None,
            items_updated: Some(12),
            duration_ms: 100,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });

        let start = Instant::now() - Duration::from_secs(2);
        let deadline = Some(Instant::now() - Duration::from_secs(1));
        let partial =
            maybe_stop_for_timeout(deadline, start, false, &dag_result, "layer 0 independent")
                .expect("expected partial result");

        assert_eq!(
            partial.status,
            crate::commands::refresh_dag::RefreshRunStatus::Partial
        );
        assert_eq!(partial.completed_sources, vec!["prices".to_string()]);
        assert!(
            partial
                .message
                .as_deref()
                .is_some_and(|msg| msg.contains("layer 0 independent"))
        );
    }

    #[test]
    fn store_fred_result_persists_keyless_fallback_rows() {
        let backend = crate::db::backend::BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };
        let mut dag_result = RefreshResult::new();

        store_fred_result(
            &backend,
            false,
            true,
            true,
            false,
            Some((
                vec![(
                    "DGS10_YAHOO",
                    Ok(vec![crate::data::fred::FredObservation {
                        series_id: "DGS10_YAHOO".to_string(),
                        date: chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string(),
                        value: dec!(4.27),
                    }]),
                )],
                Duration::from_secs(1),
            )),
            &mut dag_result,
        );

        let cached = economic_cache::get_latest_backend(&backend, "DGS10_YAHOO")
            .unwrap()
            .expect("fallback row should be cached");
        assert_eq!(cached.value, dec!(4.27));
        assert_eq!(dag_result.sources.len(), 1);
        assert_eq!(dag_result.sources[0].status, SourceStatus::Ok);
        assert_eq!(dag_result.sources[0].items_updated, Some(1));
        assert_eq!(dag_result.sources[0].items_failed, Some(0));
    }

    #[test]
    fn store_fred_result_marks_failed_when_all_series_fall_back_to_cache() {
        let backend = crate::db::backend::BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };
        let mut dag_result = RefreshResult::new();

        economic_cache::upsert_observation_backend(
            &backend,
            &economic_cache::EconomicObservation {
                series_id: "DGS10".to_string(),
                date: "2026-04-18".to_string(),
                value: dec!(4.18),
                fetched_at: "2026-04-18T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        store_fred_result(
            &backend,
            false,
            true,
            true,
            true,
            Some((
                vec![("DGS10", Err(anyhow::anyhow!("FRED API returned client error 403")))],
                Duration::from_secs(1),
            )),
            &mut dag_result,
        );

        assert_eq!(dag_result.sources.len(), 1);
        assert_eq!(dag_result.sources[0].status, SourceStatus::Failed);
        assert_eq!(dag_result.sources[0].items_updated, Some(0));
        assert_eq!(dag_result.sources[0].items_failed, Some(1));
        assert_eq!(
            dag_result.sources[0].failed_symbols.as_deref(),
            Some(&["DGS10".to_string()][..])
        );
        assert!(dag_result.sources[0]
            .detail
            .as_deref()
            .is_some_and(|msg| msg.contains("cache fallback for 1")));
    }

    // ── Price-history stamping: stale-cache no-stamp + plausibility guard ──

    use rust_decimal::Decimal;

    fn quote(symbol: &str, price: Decimal, source: &str) -> PriceQuote {
        PriceQuote {
            symbol: symbol.to_string(),
            price,
            currency: "USD".to_string(),
            source: source.to_string(),
            fetched_at: chrono::Utc::now().to_rfc3339(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
        }
    }

    fn seed_history(backend: &BackendConnection, symbol: &str, date: &str, close: Decimal) {
        crate::db::price_history::upsert_history(
            backend.sqlite_native().unwrap(),
            symbol,
            "test",
            &[HistoryRecord {
                date: date.to_string(),
                close,
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();
    }

    fn history_close(backend: &BackendConnection, symbol: &str, date: &str) -> Option<Decimal> {
        backend
            .sqlite_native()
            .unwrap()
            .query_row(
                "SELECT close FROM price_history WHERE symbol=?1 AND date=?2",
                rusqlite::params![symbol, date],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|s| s.parse().ok())
    }

    /// THE regression test for the 2026-06-11 P1: a failed live fetch must
    /// NOT stamp the symbol's stale cached price onto today's date. The
    /// cached BTC quote sits in price_cache; only live quotes get a dated
    /// row.
    #[test]
    fn failed_fetch_does_not_stamp_cached_price_as_todays_close() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        // Stale spot cache entry for BTC-USD (the Jun-5 close, fetch failing
        // ever since) + its true dated row.
        crate::db::price_cache::upsert_price_backend(
            &backend,
            &quote("BTC-USD", dec!(77414), "coingecko"),
        )
        .unwrap();
        seed_history(&backend, "BTC-USD", "2026-06-05", dec!(77414));

        // Today's refresh: only AAPL came back live; BTC-USD fetch failed.
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        seed_history(&backend, "AAPL", "2026-06-05", dec!(200));
        let quotes = vec![quote("AAPL", dec!(201), "yahoo")];
        let mut no_secondary = |_: &str, _: &str| None;
        let stamp = stamp_live_quotes_into_history(
            &backend,
            &quotes,
            &today,
            &HashSet::new(),
            &mut no_secondary,
        );

        assert_eq!(stamp.ok, 1, "only the live AAPL quote is stamped");
        assert_eq!(history_close(&backend, "AAPL", &today), Some(dec!(201)));
        // The bug shape: NO BTC-USD row may exist for today.
        assert_eq!(history_close(&backend, "BTC-USD", &today), None);
        assert!(stamp.guard_warnings.is_empty());
    }

    #[test]
    fn static_quotes_are_never_stamped() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let quotes = vec![quote("CAD", dec!(1), "static")];
        let mut no_secondary = |_: &str, _: &str| None;
        let stamp = stamp_live_quotes_into_history(
            &backend,
            &quotes,
            &today,
            &HashSet::new(),
            &mut no_secondary,
        );
        assert_eq!(stamp.ok, 0);
        assert_eq!(history_close(&backend, "CAD", &today), None);
    }

    #[test]
    fn suspect_print_rejected_when_secondary_contradicts() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        seed_history(&backend, "BTC-USD", "2026-06-05", dec!(62064));
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        // Corrupt +24.7% print; mock secondary says the old level is right.
        let quotes = vec![quote("BTC-USD", dec!(77414), "yahoo")];
        let mut secondary_calls = 0usize;
        let mut secondary = |sym: &str, _primary: &str| {
            secondary_calls += 1;
            assert_eq!(sym, "BTC-USD");
            Some(dec!(62580))
        };
        let stamp = stamp_live_quotes_into_history(
            &backend,
            &quotes,
            &today,
            &HashSet::new(),
            &mut secondary,
        );
        assert_eq!(secondary_calls, 1, "secondary consulted exactly once");
        assert_eq!(stamp.ok, 0);
        assert_eq!(history_close(&backend, "BTC-USD", &today), None);
        assert_eq!(stamp.guard_warnings.len(), 1);
        let line = &stamp.guard_warnings[0];
        assert!(line.contains("price guard"), "{line}");
        assert!(line.contains("BTC-USD print 77,414 rejected"), "{line}");
        assert!(line.contains("secondary says 62,580"), "{line}");
    }

    #[test]
    fn suspect_print_accepted_when_secondary_corroborates() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        seed_history(&backend, "BTC-USD", "2026-06-10", dec!(100000));
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        // Genuine -35% crash; mock secondary confirms within 5%.
        let quotes = vec![quote("BTC-USD", dec!(65000), "coingecko")];
        let mut secondary = |_: &str, _: &str| Some(dec!(64800));
        let stamp = stamp_live_quotes_into_history(
            &backend,
            &quotes,
            &today,
            &HashSet::new(),
            &mut secondary,
        );
        assert_eq!(stamp.ok, 1);
        assert!(stamp.guard_warnings.is_empty());
        assert_eq!(stamp.guard_notes.len(), 1, "corroboration noted");
        assert_eq!(history_close(&backend, "BTC-USD", &today), Some(dec!(65000)));
    }

    #[test]
    fn suspect_print_rejected_when_no_secondary_available() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        seed_history(&backend, "OBSCURE", "2026-06-10", dec!(10));
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let quotes = vec![quote("OBSCURE", dec!(15), "yahoo")];
        let mut no_secondary = |_: &str, _: &str| None;
        let stamp = stamp_live_quotes_into_history(
            &backend,
            &quotes,
            &today,
            &HashSet::new(),
            &mut no_secondary,
        );
        assert_eq!(stamp.ok, 0);
        assert_eq!(history_close(&backend, "OBSCURE", &today), None);
        assert_eq!(stamp.guard_warnings.len(), 1);
        assert!(
            stamp.guard_warnings[0].contains("--accept-outlier OBSCURE"),
            "{}",
            stamp.guard_warnings[0]
        );
    }

    #[test]
    fn accept_outlier_override_stamps_genuine_gap() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        seed_history(&backend, "OBSCURE", "2026-06-10", dec!(10));
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let quotes = vec![quote("OBSCURE", dec!(4), "yahoo")];
        let accept: HashSet<String> = ["OBSCURE".to_string()].into_iter().collect();
        let mut secondary_calls = 0usize;
        let mut secondary = |_: &str, _: &str| {
            secondary_calls += 1;
            None
        };
        let stamp =
            stamp_live_quotes_into_history(&backend, &quotes, &today, &accept, &mut secondary);
        assert_eq!(secondary_calls, 0, "override skips the secondary fetch");
        assert_eq!(stamp.ok, 1);
        assert!(stamp.guard_warnings.is_empty());
        assert_eq!(stamp.guard_notes.len(), 1, "override noted");
        assert_eq!(history_close(&backend, "OBSCURE", &today), Some(dec!(4)));
    }

    #[test]
    fn normal_print_does_not_consult_secondary() {
        use rust_decimal_macros::dec;
        let backend = housekeeping_backend();
        seed_history(&backend, "AAPL", "2026-06-10", dec!(200));
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let quotes = vec![quote("AAPL", dec!(205), "yahoo")];
        let mut secondary_calls = 0usize;
        let mut secondary = |_: &str, _: &str| {
            secondary_calls += 1;
            None
        };
        let stamp = stamp_live_quotes_into_history(
            &backend,
            &quotes,
            &today,
            &HashSet::new(),
            &mut secondary,
        );
        assert_eq!(secondary_calls, 0);
        assert_eq!(stamp.ok, 1);
    }
}
