use axum::{extract::Path, extract::Query, extract::State, http::StatusCode, response::Json};
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::alerts::AlertStatus;
use crate::config::Config;
use crate::db;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::models::transaction::Transaction;
use crate::tui::theme::{self, THEME_NAMES};
use crate::web::view_model;
use ratatui::style::Color;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::analytics::levels::{
    nearest_actionable_levels, select_actionable_level, ActionableLevelPair,
};
use crate::analytics::{deltas::SituationDeltaReport, situation::SituationSnapshot};

fn get_price_map_backend(
    backend: &crate::db::backend::BackendConnection,
) -> anyhow::Result<HashMap<String, Decimal>> {
    let cached = db::price_cache::get_all_cached_prices_backend(backend)?;
    Ok(cached.into_iter().map(|q| (q.symbol, q.price)).collect())
}

fn get_fx_rates_backend(
    backend: &crate::db::backend::BackendConnection,
) -> HashMap<String, Decimal> {
    crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default()
}

pub struct AppState {
    pub db_path: String,
    pub config: Config,
}

impl AppState {
    fn get_backend(&self) -> anyhow::Result<crate::db::backend::BackendConnection> {
        use std::path::Path;
        crate::db::backend::open_from_config(&self.config, Path::new(&self.db_path))
    }
}

#[derive(Serialize)]
pub struct PortfolioResponse {
    pub total_value: Option<Decimal>,
    pub total_cost: Decimal,
    pub total_gain: Option<Decimal>,
    pub total_gain_pct: Option<Decimal>,
    pub daily_change: Option<Decimal>,
    pub daily_change_pct: Option<Decimal>,
    pub positions: Vec<Position>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct PositionsResponse {
    pub positions: Vec<Position>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct WatchlistResponse {
    pub symbols: Vec<WatchlistItem>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct WatchlistItem {
    pub symbol: String,
    pub name: String,
    pub category: AssetCategory,
    pub current_price: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub technicals: Option<AssetTechnicalsResponse>,
    pub target_price: Option<Decimal>,
    pub target_direction: Option<String>,
    pub distance_pct: Option<Decimal>,
    pub target_hit: bool,
}

#[derive(Serialize)]
pub struct TransactionsResponse {
    pub transactions: Vec<Transaction>,
    pub sort_by: String,
    pub sort_order: String,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct MacroResponse {
    pub indicators: Vec<MacroIndicator>,
    pub sections: Vec<EconomySection>,
    pub top_movers: Vec<MacroIndicator>,
    pub market_breadth: MarketBreadth,
    pub economy_snapshot: EconomySnapshot,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize, Clone)]
pub struct MacroIndicator {
    pub symbol: String,
    pub name: String,
    pub value: Option<Decimal>,
    pub change_pct: Option<Decimal>,
}

#[derive(Serialize)]
pub struct MarketBreadth {
    pub up: usize,
    pub down: usize,
    pub flat: usize,
    pub avg_change_pct: Option<Decimal>,
    pub strongest: Option<MacroIndicator>,
    pub weakest: Option<MacroIndicator>,
}

#[derive(Serialize)]
pub struct EconomySnapshot {
    pub bls_metrics: Vec<BlsMetric>,
    pub sentiment: Vec<SentimentSnapshot>,
    pub upcoming_events: Vec<CalendarSnapshot>,
    pub predictions: Vec<PredictionSnapshot>,
}

#[derive(Serialize)]
pub struct BlsMetric {
    pub key: String,
    pub label: String,
    pub value: Decimal,
    pub date: String,
}

#[derive(Serialize)]
pub struct SentimentSnapshot {
    pub index_type: String,
    pub value: u8,
    pub classification: String,
    pub timestamp: i64,
}

#[derive(Serialize)]
pub struct CalendarSnapshot {
    pub date: String,
    pub name: String,
    pub impact: String,
    pub forecast: Option<String>,
}

#[derive(Serialize)]
pub struct PredictionSnapshot {
    pub question: String,
    pub probability_pct: Decimal,
    pub volume_24h: Decimal,
    pub category: String,
}

#[derive(Serialize)]
pub struct EconomySection {
    pub id: String,
    pub label: String,
    pub indicators: Vec<MacroIndicator>,
}

#[derive(Serialize)]
pub struct AlertsResponse {
    pub alerts: Vec<AlertItem>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct AlertItem {
    pub id: i64,
    pub kind: String,
    pub symbol: String,
    pub direction: String,
    pub threshold: String,
    pub rule_text: String,
    pub status: String,
    pub triggered_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AlertCreateRequest {
    pub rule_text: Option<String>,
    pub kind: Option<String>,
    pub symbol: Option<String>,
    pub direction: Option<String>,
    pub threshold: Option<String>,
    pub from_level: Option<String>,
}

#[derive(Serialize)]
pub struct AlertMutationResponse {
    pub ok: bool,
    pub id: Option<i64>,
    pub action: String,
}

#[derive(Serialize)]
pub struct ChartDataResponse {
    pub symbol: String,
    pub history: Vec<ChartPoint>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct ChartPoint {
    pub date: String,
    pub close: Decimal,
    pub volume: Option<u64>,
}

#[derive(Serialize)]
pub struct PerformanceResponse {
    pub daily_values: Vec<PortfolioValuePoint>,
    pub metrics: PerformanceMetrics,
    pub estimated: bool,
    pub coverage_pct: Decimal,
    pub source: String,
    pub benchmark_values: Option<Vec<PortfolioValuePoint>>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct PortfolioValuePoint {
    pub date: String,
    pub value: Decimal,
}

#[derive(Serialize)]
pub struct PerformanceMetrics {
    pub total_return_pct: Option<Decimal>,
    pub max_drawdown_pct: Option<Decimal>,
}

#[derive(Serialize)]
pub struct SummaryResponse {
    pub total_value: Option<Decimal>,
    pub position_count: usize,
    pub top_movers: Vec<Position>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct UiConfigResponse {
    pub tabs: Vec<&'static str>,
    pub themes: Vec<WebTheme>,
    pub current_theme: String,
    pub home_tab: String,
}

#[derive(Serialize)]
pub struct WebTheme {
    pub name: String,
    pub colors: WebThemeColors,
}

#[derive(Serialize)]
pub struct WebThemeColors {
    pub bg_primary: String,
    pub bg_secondary: String,
    pub bg_tertiary: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub text_accent: String,
    pub border: String,
    pub accent: String,
    pub green: String,
    pub red: String,
    pub yellow: String,
}

#[derive(Debug, Deserialize)]
pub struct PerformanceQuery {
    pub timeframe: Option<String>,
    pub benchmark: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionsQuery {
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub symbol: Option<String>,
    pub tx_type: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct NewsQuery {
    pub limit: Option<usize>,
    pub source: Option<String>,
    pub category: Option<String>,
    pub search: Option<String>,
    pub hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct JournalQuery {
    pub limit: Option<usize>,
    pub since: Option<String>,
    pub tag: Option<String>,
    pub symbol: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
}

#[derive(Serialize)]
pub struct NewsResponse {
    pub entries: Vec<NewsItem>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct NewsItem {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub source: String,
    pub category: String,
    pub published_at: i64,
    pub fetched_at: String,
}

#[derive(Serialize)]
pub struct JournalResponse {
    pub entries: Vec<crate::db::journal::JournalEntry>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Debug, Deserialize)]
pub struct JournalCreateRequest {
    pub timestamp: Option<String>,
    pub content: String,
    pub tag: Option<String>,
    pub symbol: Option<String>,
    pub conviction: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JournalUpdateRequest {
    pub content: Option<String>,
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct JournalMutationResponse {
    pub ok: bool,
    pub id: Option<i64>,
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub struct TransactionMutationRequest {
    pub symbol: String,
    pub category: String,
    pub tx_type: String,
    pub quantity: String,
    pub price_per: String,
    pub currency: Option<String>,
    pub date: String,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct TransactionMutationResponse {
    pub ok: bool,
    pub id: Option<i64>,
    pub action: String,
}

#[derive(Serialize)]
pub struct PreferenceResponse {
    pub ok: bool,
    pub home_tab: String,
}

#[derive(Debug, Deserialize)]
pub struct HomeTabRequest {
    pub home_tab: String,
}

#[derive(Debug, Deserialize)]
pub struct ThemeRequest {
    pub theme: String,
}

#[derive(Serialize)]
pub struct ThemeResponse {
    pub ok: bool,
    pub theme: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchItem>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize)]
pub struct SearchItem {
    pub symbol: String,
    pub name: String,
    pub category: AssetCategory,
    pub current_price: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub is_watchlisted: bool,
}

#[derive(Debug, Deserialize)]
pub struct WatchlistMutationRequest {
    pub symbol: String,
    pub category: Option<String>,
}

#[derive(Serialize)]
pub struct WatchlistMutationResponse {
    pub ok: bool,
    pub symbol: String,
    pub action: String,
}

#[derive(Serialize)]
pub struct AssetDetailResponse {
    pub symbol: String,
    pub history_symbol: String,
    pub name: String,
    pub category: AssetCategory,
    pub is_watchlisted: bool,
    pub alert_count: usize,
    pub current_price: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub week_change_pct: Option<Decimal>,
    pub month_change_pct: Option<Decimal>,
    pub year_change_pct: Option<Decimal>,
    pub range_52w_low: Option<Decimal>,
    pub range_52w_high: Option<Decimal>,
    pub latest_volume: Option<u64>,
    pub avg_volume_30d: Option<u64>,
    pub technicals: Option<AssetTechnicalsResponse>,
    pub levels: Option<ActionableLevelPair>,
    pub position: Option<AssetPositionSummary>,
    pub history: Vec<ChartPoint>,
    pub meta: view_model::ResponseMeta,
}

#[derive(Serialize, Clone)]
pub struct AssetTechnicalsResponse {
    pub timeframe: String,
    pub rsi_14: Option<f64>,
    pub macd: Option<f64>,
    pub macd_signal: Option<f64>,
    pub macd_histogram: Option<f64>,
    pub sma_20: Option<f64>,
    pub sma_50: Option<f64>,
    pub sma_200: Option<f64>,
    pub bollinger_upper: Option<f64>,
    pub bollinger_middle: Option<f64>,
    pub bollinger_lower: Option<f64>,
    pub range_52w_position: Option<f64>,
    pub volume_ratio_20: Option<f64>,
    pub volume_regime: Option<String>,
    pub above_sma_20: Option<bool>,
    pub above_sma_50: Option<bool>,
    pub above_sma_200: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_14: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_expansion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_range_ratio: Option<f64>,
    pub computed_at: String,
}

#[derive(Serialize)]
pub struct AssetPositionSummary {
    pub quantity: Decimal,
    pub current_value: Option<Decimal>,
    pub gain: Option<Decimal>,
    pub gain_pct: Option<Decimal>,
    pub allocation_pct: Option<Decimal>,
}

fn normalized_symbol(raw: &str) -> String {
    raw.trim().to_uppercase()
}

fn quote_symbol(symbol: &str, category: AssetCategory) -> String {
    view_model::watchlist_quote_symbol(symbol, category)
}

fn map_technical_snapshot(
    row: Option<crate::db::technical_snapshots::TechnicalSnapshotRecord>,
) -> Option<AssetTechnicalsResponse> {
    row.map(|row| AssetTechnicalsResponse {
        timeframe: row.timeframe,
        rsi_14: row.rsi_14,
        macd: row.macd,
        macd_signal: row.macd_signal,
        macd_histogram: row.macd_histogram,
        sma_20: row.sma_20,
        sma_50: row.sma_50,
        sma_200: row.sma_200,
        bollinger_upper: row.bollinger_upper,
        bollinger_middle: row.bollinger_middle,
        bollinger_lower: row.bollinger_lower,
        range_52w_position: row.range_52w_position,
        volume_ratio_20: row.volume_ratio_20,
        volume_regime: row.volume_regime,
        above_sma_20: row.above_sma_20,
        above_sma_50: row.above_sma_50,
        above_sma_200: row.above_sma_200,
        atr_14: row.atr_14,
        atr_ratio: row.atr_ratio,
        range_expansion: row.range_expansion,
        day_range_ratio: row.day_range_ratio,
        computed_at: row.computed_at,
    })
}

fn load_nearest_levels(
    backend: &crate::db::backend::BackendConnection,
    symbol: &str,
    fallback_symbol: &str,
    current_price: Option<Decimal>,
) -> Option<ActionableLevelPair> {
    let price = current_price?.to_string().parse::<f64>().ok()?;
    let levels = db::technical_levels::get_levels_for_symbol_backend(backend, symbol)
        .ok()
        .filter(|rows| !rows.is_empty())
        .or_else(|| {
            if fallback_symbol.eq_ignore_ascii_case(symbol) {
                None
            } else {
                db::technical_levels::get_levels_for_symbol_backend(backend, fallback_symbol)
                    .ok()
                    .filter(|rows| !rows.is_empty())
            }
        })?;
    let pair = nearest_actionable_levels(&levels, price);
    if pair.support.is_none() && pair.resistance.is_none() {
        None
    } else {
        Some(pair)
    }
}

fn stored_level_alert(
    backend: &crate::db::backend::BackendConnection,
    symbol: &str,
    selector: &str,
    label: Option<&str>,
) -> Result<(String, String, String, String), (StatusCode, String)> {
    let current_price = get_price_map_backend(backend)
        .ok()
        .and_then(|prices| prices.get(symbol).copied())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("No cached price available for {}", symbol),
            )
        })?;
    let current_f = current_price.to_string().parse::<f64>().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to parse cached price for {}", symbol),
        )
    })?;
    let levels =
        db::technical_levels::get_levels_for_symbol_backend(backend, symbol).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load stored levels: {}", e),
            )
        })?;
    if levels.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("No stored levels available for {}", symbol),
        ));
    }
    let selected = select_actionable_level(&levels, current_f, selector).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("No stored '{}' level available for {}", selector, symbol),
        )
    })?;
    let direction = if selected.price >= current_f {
        "above"
    } else {
        "below"
    }
    .to_string();
    let threshold = if selected.price >= 10000.0 {
        format!("{:.0}", selected.price)
    } else if selected.price >= 1.0 {
        format!("{:.2}", selected.price)
    } else {
        format!("{:.4}", selected.price)
    };
    let rule_text = label
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("{symbol} {direction} stored {selector} {threshold}"));
    Ok((symbol.to_string(), direction, threshold, rule_text))
}

fn bounded_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
    std::cmp::min(limit.unwrap_or(default).max(1), max)
}

fn history_change_pct(
    history: &[crate::models::price::HistoryRecord],
    lookback_points: usize,
) -> Option<Decimal> {
    if history.len() <= lookback_points {
        return None;
    }
    let latest = history.last()?.close;
    let prev = history.get(history.len() - 1 - lookback_points)?.close;
    if prev == dec!(0) {
        return None;
    }
    Some((latest - prev) / prev * dec!(100))
}

fn day_change_pct_backend(
    backend: &crate::db::backend::BackendConnection,
    symbol: &str,
) -> Option<Decimal> {
    let history = db::price_history::get_history_backend(backend, symbol, 2).ok()?;
    history_change_pct(&history, 1)
}

fn portfolio_day_change(
    backend: &crate::db::backend::BackendConnection,
    positions: &[Position],
    total_value: Option<Decimal>,
) -> (Option<Decimal>, Option<Decimal>) {
    let mut prev_total = dec!(0);
    let mut has_non_cash = false;

    for pos in positions {
        if pos.category == AssetCategory::Cash {
            if let Some(v) = pos.current_value {
                prev_total += v;
            }
            continue;
        }

        has_non_cash = true;
        let history = match db::price_history::get_history_backend(backend, &pos.symbol, 2) {
            Ok(h) if h.len() >= 2 => h,
            _ => return (None, None),
        };
        let prev_price = history[history.len() - 2].close;
        prev_total += prev_price * pos.quantity;
    }

    if !has_non_cash {
        return (Some(dec!(0)), Some(dec!(0)));
    }

    let Some(curr_total) = total_value else {
        return (None, None);
    };
    if prev_total <= dec!(0) {
        return (None, None);
    }

    let daily_change = curr_total - prev_total;
    let daily_change_pct = (daily_change / prev_total) * dec!(100);
    (Some(daily_change), Some(daily_change_pct))
}

fn range_from_history(
    history: &[crate::models::price::HistoryRecord],
    points: usize,
) -> (Option<Decimal>, Option<Decimal>) {
    let slice = if history.len() > points {
        &history[history.len() - points..]
    } else {
        history
    };
    if slice.is_empty() {
        return (None, None);
    }
    let mut lo = slice[0].close;
    let mut hi = slice[0].close;
    for rec in slice.iter().skip(1) {
        if rec.close < lo {
            lo = rec.close;
        }
        if rec.close > hi {
            hi = rec.close;
        }
    }
    (Some(lo), Some(hi))
}

fn volume_stats(history: &[crate::models::price::HistoryRecord]) -> (Option<u64>, Option<u64>) {
    if history.is_empty() {
        return (None, None);
    }
    let latest = history.last().and_then(|r| r.volume);
    let recent = if history.len() > 30 {
        &history[history.len() - 30..]
    } else {
        history
    };
    let mut sum: u128 = 0;
    let mut count: u128 = 0;
    for rec in recent {
        if let Some(v) = rec.volume {
            sum += v as u128;
            count += 1;
        }
    }
    let avg = if count == 0 {
        None
    } else {
        Some((sum / count) as u64)
    };
    (latest, avg)
}

fn series_label(series_id: &str) -> &'static str {
    match series_id {
        crate::data::bls::SERIES_CPI_U => "CPI (YoY index)",
        crate::data::bls::SERIES_UNEMPLOYMENT => "Unemployment Rate",
        crate::data::bls::SERIES_NFP => "Nonfarm Payrolls",
        crate::data::bls::SERIES_HOURLY_EARNINGS => "Hourly Earnings",
        _ => "BLS Series",
    }
}

// Handlers

pub async fn get_portfolio(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PortfolioResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
    let prices = crate::db::price_cache::get_all_cached_prices_backend(&backend)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect::<HashMap<_, _>>();

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices, &fx_rates)
    } else {
        let transactions = db::transactions::list_transactions_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices, &fx_rates)
    };

    let total_value: Option<Decimal> = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum::<Decimal>()
        .into();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value.map(|v| v - total_cost);
    let total_gain_pct = if total_cost > dec!(0) {
        total_gain.map(|g| (g / total_cost) * dec!(100))
    } else {
        None
    };

    let (daily_change, daily_change_pct) = portfolio_day_change(&backend, &positions, total_value);

    Ok(Json(PortfolioResponse {
        total_value,
        total_cost,
        total_gain,
        total_gain_pct,
        daily_change,
        daily_change_pct,
        positions,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_positions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PositionsResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
    let prices = crate::db::price_cache::get_all_cached_prices_backend(&backend)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect::<HashMap<_, _>>();

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices, &fx_rates)
    } else {
        let transactions = db::transactions::list_transactions_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices, &fx_rates)
    };

    Ok(Json(PositionsResponse {
        positions,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_watchlist(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WatchlistResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let watchlist = db::watchlist::list_watchlist_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load watchlist: {}", e),
        )
    })?;

    let prices = crate::db::price_cache::get_all_cached_prices_backend(&backend)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect::<HashMap<_, _>>();

    let items: Vec<WatchlistItem> = watchlist
        .into_iter()
        .map(|w| {
            let category: AssetCategory = w.category.parse().unwrap_or(AssetCategory::Equity);
            let quote_symbol = view_model::watchlist_quote_symbol(&w.symbol, category);
            let current_price = prices
                .get(&w.symbol)
                .copied()
                .or_else(|| prices.get(&quote_symbol).copied());
            let day_change_pct = day_change_pct_backend(&backend, &quote_symbol)
                .or_else(|| day_change_pct_backend(&backend, &w.symbol));
            let technicals = map_technical_snapshot(
                crate::db::technical_snapshots::get_latest_snapshot_backend(
                    &backend, &w.symbol, "1d",
                )
                .ok()
                .flatten(),
            );
            let target_price = w.target_price.and_then(|t| t.parse::<Decimal>().ok());
            let (distance_pct, target_hit) = view_model::compute_watchlist_proximity(
                current_price,
                target_price,
                w.target_direction.as_deref(),
            );
            WatchlistItem {
                symbol: w.symbol.clone(),
                name: crate::models::asset_names::resolve_name(&w.symbol),
                category,
                current_price,
                day_change_pct,
                technicals,
                target_price,
                target_direction: w.target_direction,
                distance_pct,
                target_hit,
            }
        })
        .collect();

    Ok(Json(WatchlistResponse {
        symbols: items,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn post_watchlist(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WatchlistMutationRequest>,
) -> Result<Json<WatchlistMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let symbol = normalized_symbol(&body.symbol);
    if symbol.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "symbol is required".to_string()));
    }
    let category = body
        .category
        .as_deref()
        .and_then(|raw| raw.parse::<AssetCategory>().ok())
        .unwrap_or_else(|| crate::models::asset_names::infer_category(&symbol));
    db::watchlist::add_to_watchlist_backend(&backend, &symbol, category).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update watchlist: {}", e),
        )
    })?;
    Ok(Json(WatchlistMutationResponse {
        ok: true,
        symbol,
        action: "added".to_string(),
    }))
}

pub async fn delete_watchlist(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<WatchlistMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let symbol = normalized_symbol(&symbol);
    if symbol.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "symbol is required".to_string()));
    }
    let removed = db::watchlist::remove_from_watchlist_backend(&backend, &symbol).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update watchlist: {}", e),
        )
    })?;
    Ok(Json(WatchlistMutationResponse {
        ok: removed,
        symbol,
        action: if removed {
            "removed".to_string()
        } else {
            "noop".to_string()
        },
    }))
}

pub async fn get_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let q = query.q.as_deref().unwrap_or("").trim();
    if q.is_empty() {
        return Ok(Json(SearchResponse {
            results: Vec::new(),
            meta: view_model::fresh_meta(60),
        }));
    }

    let limit = bounded_limit(query.limit, 30, 100);
    let watchlist_set: HashSet<String> = db::watchlist::list_watchlist_backend(&backend)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load watchlist: {}", e),
            )
        })?
        .into_iter()
        .map(|w| w.symbol)
        .collect();
    let prices = get_price_map_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load prices: {}", e),
        )
    })?;

    let mut results = Vec::new();
    for (symbol, name) in crate::models::asset_names::search_names(q)
        .into_iter()
        .take(limit)
    {
        let symbol = symbol.to_string();
        let category = crate::models::asset_names::infer_category(&symbol);
        let qsym = quote_symbol(&symbol, category);
        let current_price = prices
            .get(&symbol)
            .copied()
            .or_else(|| prices.get(&qsym).copied());
        let day_change_pct = day_change_pct_backend(&backend, &qsym)
            .or_else(|| day_change_pct_backend(&backend, &symbol));
        let is_watchlisted = watchlist_set.contains(&symbol);
        results.push(SearchItem {
            symbol,
            name: name.to_string(),
            category,
            current_price,
            day_change_pct,
            is_watchlisted,
        });
    }

    Ok(Json(SearchResponse {
        results,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_asset_detail(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<AssetDetailResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let symbol = normalized_symbol(&symbol);
    if symbol.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "symbol is required".to_string()));
    }
    let category = crate::models::asset_names::infer_category(&symbol);
    let qsym = quote_symbol(&symbol, category);
    let mut history =
        db::price_history::get_history_backend(&backend, &symbol, 365).unwrap_or_default();
    let mut history_symbol = symbol.clone();
    if history.is_empty() && qsym != symbol {
        let fallback =
            db::price_history::get_history_backend(&backend, &qsym, 365).unwrap_or_default();
        if !fallback.is_empty() {
            history = fallback;
            history_symbol = qsym.clone();
        }
    }
    let prices = get_price_map_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load prices: {}", e),
        )
    })?;
    let current_price = prices
        .get(&symbol)
        .copied()
        .or_else(|| prices.get(&qsym).copied())
        .or_else(|| history.last().map(|h| h.close));
    let day_change_pct = history_change_pct(&history, 1)
        .or_else(|| day_change_pct_backend(&backend, &history_symbol));
    let week_change_pct = history_change_pct(&history, 5);
    let month_change_pct = history_change_pct(&history, 21);
    let year_change_pct = history_change_pct(&history, 252);
    let (range_52w_low, range_52w_high) = range_from_history(&history, 252);
    let (latest_volume, avg_volume_30d) = volume_stats(&history);
    let alerts = db::alerts::list_alerts_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load alerts: {}", e),
        )
    })?;
    let alert_count = alerts
        .into_iter()
        .filter(|a| a.symbol.eq_ignore_ascii_case(&symbol))
        .count();
    let is_watchlisted = db::watchlist::is_watched_backend(&backend, &symbol).unwrap_or(false);

    let fx_rates = get_fx_rates_backend(&backend);

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices, &fx_rates)
    } else {
        let transactions = db::transactions::list_transactions_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices, &fx_rates)
    };
    let pos = positions
        .iter()
        .find(|p| p.symbol.eq_ignore_ascii_case(&symbol) || p.symbol.eq_ignore_ascii_case(&qsym));
    let position = pos.map(|p| AssetPositionSummary {
        quantity: p.quantity,
        current_value: p.current_value,
        gain: p.gain,
        gain_pct: p.gain_pct,
        allocation_pct: p.allocation_pct,
    });
    let name = crate::models::asset_names::resolve_name(&symbol);
    let history_points: Vec<ChartPoint> = history
        .into_iter()
        .map(|h| ChartPoint {
            date: h.date,
            close: h.close,
            volume: h.volume,
        })
        .collect();
    let technicals = map_technical_snapshot(
        crate::db::technical_snapshots::get_latest_snapshot_backend(&backend, &symbol, "1d")
            .ok()
            .flatten()
            .or_else(|| {
                crate::db::technical_snapshots::get_latest_snapshot_backend(&backend, &qsym, "1d")
                    .ok()
                    .flatten()
            }),
    );
    let levels = load_nearest_levels(&backend, &symbol, &qsym, current_price);

    Ok(Json(AssetDetailResponse {
        symbol: symbol.clone(),
        history_symbol,
        name,
        category,
        is_watchlisted,
        alert_count,
        current_price,
        day_change_pct,
        week_change_pct,
        month_change_pct,
        year_change_pct,
        range_52w_low,
        range_52w_high,
        latest_volume,
        avg_volume_30d,
        technicals,
        levels,
        position,
        history: history_points,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_transactions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TransactionsQuery>,
) -> Result<Json<TransactionsResponse>, (StatusCode, String)> {
    if state.config.is_percentage_mode() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Transactions not available in percentage mode".to_string(),
        ));
    }

    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let transactions = db::transactions::list_transactions_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load transactions: {}", e),
        )
    })?;

    let sort_by = view_model::TxSortField::from_str(query.sort_by.as_deref().unwrap_or("date"));
    let sort_order = view_model::SortOrder::from_str(query.sort_order.as_deref().unwrap_or("desc"));
    let mut transactions = view_model::apply_transaction_filters(
        transactions,
        query.symbol.as_deref(),
        query.tx_type.as_deref(),
        query.from.as_deref(),
        query.to.as_deref(),
    );
    view_model::sort_transactions(&mut transactions, sort_by, sort_order);

    if let Some(limit) = query.limit {
        transactions.truncate(limit);
    }

    Ok(Json(TransactionsResponse {
        transactions,
        sort_by: sort_by.as_str().to_string(),
        sort_order: sort_order.as_str().to_string(),
        meta: view_model::fresh_meta(60),
    }))
}

fn parse_transaction_request(
    body: &TransactionMutationRequest,
) -> Result<crate::models::transaction::NewTransaction, (StatusCode, String)> {
    let symbol = normalized_symbol(&body.symbol);
    if symbol.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "symbol is required".to_string()));
    }
    let category = body
        .category
        .parse::<AssetCategory>()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid category: {}", e)))?;
    let tx_type = body
        .tx_type
        .parse::<crate::models::transaction::TxType>()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid tx_type: {}", e)))?;
    let quantity = body
        .quantity
        .parse::<Decimal>()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid quantity: {}", e)))?;
    let price_per = body
        .price_per
        .parse::<Decimal>()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid price_per: {}", e)))?;
    let date = body.date.trim().to_string();
    if date.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "date is required".to_string()));
    }
    Ok(crate::models::transaction::NewTransaction {
        symbol,
        category,
        tx_type,
        quantity,
        price_per,
        currency: body.currency.clone().unwrap_or_else(|| "USD".to_string()),
        date,
        notes: body
            .notes
            .clone()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty()),
    })
}

pub async fn post_transaction(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TransactionMutationRequest>,
) -> Result<Json<TransactionMutationResponse>, (StatusCode, String)> {
    if state.config.is_percentage_mode() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Transactions not available in percentage mode".to_string(),
        ));
    }
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let tx = parse_transaction_request(&body)?;
    let id = db::transactions::insert_transaction_backend(&backend, &tx).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create transaction: {}", e),
        )
    })?;
    Ok(Json(TransactionMutationResponse {
        ok: true,
        id: Some(id),
        action: "created".to_string(),
    }))
}

pub async fn patch_transaction(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<TransactionMutationRequest>,
) -> Result<Json<TransactionMutationResponse>, (StatusCode, String)> {
    if state.config.is_percentage_mode() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Transactions not available in percentage mode".to_string(),
        ));
    }
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let tx = parse_transaction_request(&body)?;
    let ok = db::transactions::update_transaction_backend(&backend, id, &tx).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update transaction: {}", e),
        )
    })?;
    Ok(Json(TransactionMutationResponse {
        ok,
        id: Some(id),
        action: if ok {
            "updated".to_string()
        } else {
            "noop".to_string()
        },
    }))
}

pub async fn delete_transaction(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<TransactionMutationResponse>, (StatusCode, String)> {
    if state.config.is_percentage_mode() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Transactions not available in percentage mode".to_string(),
        ));
    }
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let ok = db::transactions::delete_transaction_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete transaction: {}", e),
        )
    })?;
    Ok(Json(TransactionMutationResponse {
        ok,
        id: Some(id),
        action: if ok {
            "removed".to_string()
        } else {
            "noop".to_string()
        },
    }))
}

pub async fn get_macro(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MacroResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let prices = get_price_map_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load prices: {}", e),
        )
    })?;

    let indicator_specs = view_model::market_overview_symbols();
    let indicators: Vec<MacroIndicator> = indicator_specs
        .into_iter()
        .map(|spec| MacroIndicator {
            symbol: spec.symbol.clone(),
            name: spec.name,
            value: prices.get(&spec.symbol).copied(),
            change_pct: day_change_pct_backend(&backend, &spec.symbol),
        })
        .collect();

    let sections: Vec<EconomySection> = view_model::economy_sections()
        .into_iter()
        .map(|section| {
            let section_indicators = section
                .symbols
                .into_iter()
                .map(|spec| MacroIndicator {
                    symbol: spec.symbol.clone(),
                    name: spec.name,
                    value: prices.get(&spec.symbol).copied(),
                    change_pct: day_change_pct_backend(&backend, &spec.symbol),
                })
                .collect();
            EconomySection {
                id: section.id,
                label: section.label,
                indicators: section_indicators,
            }
        })
        .collect();

    let mut indicator_by_symbol: HashMap<String, MacroIndicator> = indicators
        .iter()
        .map(|i| (i.symbol.clone(), i.clone()))
        .collect();
    for section in &sections {
        for indicator in &section.indicators {
            indicator_by_symbol
                .entry(indicator.symbol.clone())
                .or_insert_with(|| indicator.clone());
        }
    }

    let changes: HashMap<String, Decimal> = indicator_by_symbol
        .values()
        .filter_map(|i| i.change_pct.map(|c| (i.symbol.clone(), c)))
        .collect();
    let movers = view_model::top_movers_from_map(&changes, 6);
    let top_movers: Vec<MacroIndicator> = movers
        .into_iter()
        .filter_map(|(symbol, change)| {
            indicator_by_symbol.get(&symbol).map(|i| {
                let mut out = i.clone();
                out.change_pct = Some(change);
                out
            })
        })
        .collect();

    let (up, down, flat, avg_change_pct) = if changes.is_empty() {
        (0, 0, 0, None)
    } else {
        let mut up = 0usize;
        let mut down = 0usize;
        let mut flat = 0usize;
        let mut sum = dec!(0);
        for change in changes.values() {
            if *change > dec!(0) {
                up += 1;
            } else if *change < dec!(0) {
                down += 1;
            } else {
                flat += 1;
            }
            sum += *change;
        }
        let avg = sum / Decimal::from(changes.len() as u64);
        (up, down, flat, Some(avg))
    };

    let strongest = changes
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|(symbol, _)| indicator_by_symbol.get(symbol))
        .cloned();
    let weakest = changes
        .iter()
        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|(symbol, _)| indicator_by_symbol.get(symbol))
        .cloned();

    let bls_series = [
        crate::data::bls::SERIES_CPI_U,
        crate::data::bls::SERIES_UNEMPLOYMENT,
        crate::data::bls::SERIES_NFP,
        crate::data::bls::SERIES_HOURLY_EARNINGS,
    ];
    let bls_metrics: Vec<BlsMetric> = bls_series
        .iter()
        .filter_map(|series| {
            db::bls_cache::get_latest_bls_data_backend(&backend, series)
                .ok()
                .flatten()
        })
        .map(|row| BlsMetric {
            key: row.series_id.clone(),
            label: series_label(&row.series_id).to_string(),
            value: row.value,
            date: row.date.format("%Y-%m-%d").to_string(),
        })
        .collect();

    let sentiment: Vec<SentimentSnapshot> = ["crypto", "traditional"]
        .iter()
        .filter_map(|kind| {
            db::sentiment_cache::get_latest_backend(&backend, kind)
                .ok()
                .flatten()
        })
        .map(|row| SentimentSnapshot {
            index_type: row.index_type,
            value: row.value,
            classification: row.classification,
            timestamp: row.timestamp,
        })
        .collect();

    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let upcoming_events: Vec<CalendarSnapshot> =
        db::calendar_cache::get_upcoming_events_backend(&backend, &today, 6)
            .unwrap_or_default()
            .into_iter()
            .map(|e| CalendarSnapshot {
                date: e.date,
                name: e.name,
                impact: e.impact,
                forecast: e.forecast,
            })
            .collect();

    let predictions: Vec<PredictionSnapshot> =
        db::predictions_cache::get_cached_predictions_backend(&backend, 5)
            .unwrap_or_default()
            .into_iter()
            .map(|p| PredictionSnapshot {
                question: p.question,
                probability_pct: Decimal::from_f64_retain(p.probability * 100.0).unwrap_or(dec!(0)),
                volume_24h: Decimal::from_f64_retain(p.volume_24h).unwrap_or(dec!(0)),
                category: p.category.to_string(),
            })
            .collect();

    Ok(Json(MacroResponse {
        indicators,
        sections,
        top_movers,
        market_breadth: MarketBreadth {
            up,
            down,
            flat,
            avg_change_pct,
            strongest,
            weakest,
        },
        economy_snapshot: EconomySnapshot {
            bls_metrics,
            sentiment,
            upcoming_events,
            predictions,
        },
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_alerts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AlertsResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let alerts_data = db::alerts::list_alerts_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load alerts: {}", e),
        )
    })?;

    let alerts: Vec<AlertItem> = alerts_data
        .into_iter()
        .map(|a| AlertItem {
            id: a.id,
            kind: a.kind.to_string(),
            symbol: a.symbol,
            direction: a.direction.to_string(),
            threshold: a.threshold,
            rule_text: a.rule_text,
            status: a.status.to_string(),
            triggered_at: a.triggered_at,
        })
        .collect();

    Ok(Json(AlertsResponse {
        alerts,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn post_alert(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AlertCreateRequest>,
) -> Result<Json<AlertMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let mut custom_rule_text: Option<String> = None;
    let parsed = if let Some(selector) = body.from_level.as_deref() {
        if let Some(kind) = body.kind.as_deref() {
            if !kind.eq_ignore_ascii_case("price") {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "from_level only supports price alerts".to_string(),
                ));
            }
        }
        let symbol = normalized_symbol(body.symbol.as_deref().unwrap_or(""));
        if symbol.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "symbol is required when using from_level".to_string(),
            ));
        }
        let (symbol, direction, threshold, rule_text) =
            stored_level_alert(&backend, &symbol, selector, body.rule_text.as_deref())?;
        custom_rule_text = Some(rule_text);
        crate::alerts::rules::parse_rule(&format!("{symbol} {direction} {threshold}")).map_err(
            |e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Failed to build level-based alert: {}", e),
                )
            },
        )?
    } else if let Some(rule_text) = body.rule_text.as_deref() {
        let parsed = crate::alerts::rules::parse_rule(rule_text)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid rule_text: {}", e)))?;
        if let Some(kind) = body.kind.as_deref() {
            let expected = kind
                .parse::<crate::alerts::AlertKind>()
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid kind: {}", e)))?;
            if parsed.kind != expected {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "rule_text kind '{}' does not match requested kind '{}'",
                        parsed.kind, expected
                    ),
                ));
            }
        }
        parsed
    } else {
        let symbol = normalized_symbol(body.symbol.as_deref().unwrap_or(""));
        let direction = body.direction.as_deref().unwrap_or("above").to_lowercase();
        let threshold = body.threshold.as_deref().unwrap_or("").trim().to_string();
        if symbol.is_empty() || threshold.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "symbol and threshold are required".to_string(),
            ));
        }
        let rule_text = format!("{} {} {}", symbol, direction, threshold);
        crate::alerts::rules::parse_rule(&rule_text).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid alert fields: {}", e),
            )
        })?
    };

    let kind = parsed.kind.to_string();
    let direction = parsed.direction.to_string();
    let threshold = parsed.threshold.to_string();
    let rule_text = custom_rule_text.unwrap_or_else(|| parsed.rule_text.clone());
    let id = db::alerts::add_alert_backend(
        &backend,
        db::alerts::NewAlert {
            kind: &kind,
            symbol: &parsed.symbol,
            direction: &direction,
            condition: None,
            threshold: &threshold,
            rule_text: &rule_text,
            recurring: false,
            cooldown_minutes: 0,
        },
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create alert: {}", e),
        )
    })?;

    Ok(Json(AlertMutationResponse {
        ok: true,
        id: Some(id),
        action: "created".to_string(),
    }))
}

pub async fn delete_alert(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let removed = db::alerts::remove_alert_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to remove alert: {}", e),
        )
    })?;
    Ok(Json(AlertMutationResponse {
        ok: removed,
        id: Some(id),
        action: if removed {
            "removed".to_string()
        } else {
            "noop".to_string()
        },
    }))
}

pub async fn post_alert_ack(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let ok = db::alerts::acknowledge_alert_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to acknowledge alert: {}", e),
        )
    })?;
    Ok(Json(AlertMutationResponse {
        ok,
        id: Some(id),
        action: "acknowledged".to_string(),
    }))
}

pub async fn post_alert_rearm(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let exists = db::alerts::get_alert_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load alert: {}", e),
        )
    })?;
    let Some(alert) = exists else {
        return Ok(Json(AlertMutationResponse {
            ok: false,
            id: Some(id),
            action: "noop".to_string(),
        }));
    };
    let ok = match alert.status {
        AlertStatus::Triggered | AlertStatus::Acknowledged => {
            db::alerts::rearm_alert_backend(&backend, id).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to rearm alert: {}", e),
                )
            })?
        }
        AlertStatus::Armed => false,
    };
    Ok(Json(AlertMutationResponse {
        ok,
        id: Some(id),
        action: "rearmed".to_string(),
    }))
}

pub async fn get_chart_data(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<ChartDataResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let history = db::price_history::get_history_backend(&backend, &symbol, 365).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load history: {}", e),
        )
    })?;

    let points: Vec<ChartPoint> = history
        .into_iter()
        .map(|h| ChartPoint {
            date: h.date,
            close: h.close,
            volume: h.volume,
        })
        .collect();

    Ok(Json(ChartDataResponse {
        symbol,
        history: points,
        meta: view_model::fresh_meta(300),
    }))
}

pub async fn get_performance(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PerformanceQuery>,
) -> Result<Json<PerformanceResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let mut snapshots =
        db::snapshots::get_all_portfolio_snapshots_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load portfolio snapshots: {}", e),
            )
        })?;

    let days = match query.timeframe.as_deref().unwrap_or("3m") {
        "1w" => 7,
        "1m" => 30,
        "3m" => 90,
        "6m" => 180,
        "1y" => 365,
        "5y" => 1825,
        _ => 90,
    };

    let cutoff = Utc::now().date_naive() - Duration::days(days);
    if !snapshots.is_empty() {
        snapshots.retain(|s| {
            NaiveDate::parse_from_str(&s.date, "%Y-%m-%d")
                .map(|d| d >= cutoff)
                .unwrap_or(true)
        });
    }

    let expected_points = std::cmp::max(days / 3, 1) as usize;

    let mut daily_values: Vec<PortfolioValuePoint> = snapshots
        .iter()
        .map(|s| PortfolioValuePoint {
            date: s.date.clone(),
            value: s.total_value,
        })
        .collect();
    let mut source = "snapshots".to_string();
    let mut estimated = false;

    // Fallback when snapshot history is unavailable:
    // rebuild an approximate portfolio curve from current holdings and per-symbol history.
    if daily_values.len() < 2 {
        source = "estimated_history".to_string();
        estimated = true;
        let fx_rates = get_fx_rates_backend(&backend);
        let positions = if state.config.is_percentage_mode() {
            let allocations = db::allocations::list_allocations_backend(&backend).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load allocations: {}", e),
                )
            })?;
            let prices = get_price_map_backend(&backend).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load prices: {}", e),
                )
            })?;
            compute_positions_from_allocations(&allocations, &prices, &fx_rates)
        } else {
            let transactions =
                db::transactions::list_transactions_backend(&backend).map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load transactions: {}", e),
                    )
                })?;
            let prices = get_price_map_backend(&backend).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load prices: {}", e),
                )
            })?;
            compute_positions(&transactions, &prices, &fx_rates)
        };

        let mut by_date: BTreeMap<String, Decimal> = BTreeMap::new();
        for pos in positions.into_iter().filter(|p| p.quantity > dec!(0)) {
            let history =
                db::price_history::get_history_backend(&backend, &pos.symbol, days as u32 + 14)
                    .unwrap_or_default();
            for rec in history {
                if let Ok(d) = NaiveDate::parse_from_str(&rec.date, "%Y-%m-%d") {
                    if d >= cutoff {
                        let v = pos.quantity * rec.close;
                        by_date
                            .entry(rec.date)
                            .and_modify(|acc| *acc += v)
                            .or_insert(v);
                    }
                }
            }
        }

        if !by_date.is_empty() {
            daily_values = by_date
                .into_iter()
                .map(|(date, value)| PortfolioValuePoint { date, value })
                .collect();
        }
    }

    let total_return_pct = if daily_values.len() >= 2 {
        let start = daily_values.first().map(|p| p.value).unwrap_or(dec!(0));
        let end = daily_values.last().map(|p| p.value).unwrap_or(dec!(0));
        if start > dec!(0) {
            Some(((end - start) / start) * dec!(100))
        } else {
            None
        }
    } else {
        None
    };

    let mut max_drawdown_pct: Option<Decimal> = None;
    if !daily_values.is_empty() {
        let mut peak = daily_values[0].value;
        let mut worst = dec!(0);
        for point in &daily_values {
            if point.value > peak {
                peak = point.value;
            }
            if peak > dec!(0) {
                let dd = ((point.value - peak) / peak) * dec!(100);
                if dd < worst {
                    worst = dd;
                }
            }
        }
        max_drawdown_pct = Some(worst);
    }

    let coverage_pct = if expected_points == 0 {
        dec!(0)
    } else {
        let pct = (daily_values.len() as i64 * 100) / expected_points as i64;
        Decimal::from(pct.clamp(0, 100))
    };

    let benchmark_values = if query.benchmark.as_deref() == Some("spx") && !daily_values.is_empty()
    {
        let bench_history =
            db::price_history::get_history_backend(&backend, "^GSPC", days as u32 + 14)
                .unwrap_or_default();
        let base_portfolio = daily_values[0].value;
        let mut base_bench: Option<Decimal> = None;
        let mut series = Vec::new();
        for rec in bench_history {
            if let Ok(d) = NaiveDate::parse_from_str(&rec.date, "%Y-%m-%d") {
                if d < cutoff {
                    continue;
                }
            }
            if base_bench.is_none() {
                base_bench = Some(rec.close);
            }
            if let Some(base) = base_bench {
                if base > dec!(0) {
                    series.push(PortfolioValuePoint {
                        date: rec.date,
                        value: (rec.close / base) * base_portfolio,
                    });
                }
            }
        }
        if series.len() >= 2 {
            Some(series)
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(PerformanceResponse {
        daily_values,
        metrics: PerformanceMetrics {
            total_return_pct,
            max_drawdown_pct,
        },
        estimated,
        coverage_pct,
        source,
        benchmark_values,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_news(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NewsQuery>,
) -> Result<Json<NewsResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let entries = db::news_cache::get_latest_news_backend(
        &backend,
        query.limit.unwrap_or(150),
        query.source.as_deref(),
        query.category.as_deref(),
        query.search.as_deref(),
        query.hours,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load news: {}", e),
        )
    })?;
    let entries = entries
        .into_iter()
        .map(|e| NewsItem {
            id: e.id,
            title: e.title,
            url: e.url,
            source: e.source,
            category: e.category,
            published_at: e.published_at,
            fetched_at: e.fetched_at,
        })
        .collect();
    Ok(Json(NewsResponse {
        entries,
        meta: view_model::fresh_meta(300),
    }))
}

pub async fn get_journal(
    State(state): State<Arc<AppState>>,
    Query(query): Query<JournalQuery>,
) -> Result<Json<JournalResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let entries = if let Some(search) = query.search.as_deref() {
        db::journal::search_entries_backend(&backend, search, query.since.as_deref(), query.limit)
    } else {
        db::journal::list_entries_backend(
            &backend,
            query.limit,
            query.since.as_deref(),
            query.tag.as_deref(),
            query.symbol.as_deref(),
            query.status.as_deref(),
        )
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load journal: {}", e),
        )
    })?;

    Ok(Json(JournalResponse {
        entries,
        meta: view_model::fresh_meta(300),
    }))
}

pub async fn post_journal(
    State(state): State<Arc<AppState>>,
    Json(body): Json<JournalCreateRequest>,
) -> Result<Json<JournalMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let content = body.content.trim();
    if content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "content is required".to_string()));
    }
    let entry = crate::db::journal::NewJournalEntry {
        timestamp: body.timestamp.unwrap_or_else(|| Utc::now().to_rfc3339()),
        content: content.to_string(),
        tag: body
            .tag
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        symbol: body
            .symbol
            .map(|v| normalized_symbol(&v))
            .filter(|v| !v.is_empty()),
        conviction: body
            .conviction
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        status: body
            .status
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "open".to_string()),
    };
    let id = db::journal::add_entry_backend(&backend, &entry).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create journal entry: {}", e),
        )
    })?;
    Ok(Json(JournalMutationResponse {
        ok: true,
        id: Some(id),
        action: "created".to_string(),
    }))
}

pub async fn patch_journal(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<JournalUpdateRequest>,
) -> Result<Json<JournalMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let existing = db::journal::get_entry_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load journal entry: {}", e),
        )
    })?;
    if existing.is_none() {
        return Ok(Json(JournalMutationResponse {
            ok: false,
            id: Some(id),
            action: "noop".to_string(),
        }));
    }
    let content = body
        .content
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let status = body
        .status
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if content.is_none() && status.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "at least one of content or status must be provided".to_string(),
        ));
    }
    db::journal::update_entry_backend(&backend, id, content, status).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update journal entry: {}", e),
        )
    })?;
    Ok(Json(JournalMutationResponse {
        ok: true,
        id: Some(id),
        action: "updated".to_string(),
    }))
}

pub async fn delete_journal(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<JournalMutationResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;
    let existing = db::journal::get_entry_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load journal entry: {}", e),
        )
    })?;
    if existing.is_none() {
        return Ok(Json(JournalMutationResponse {
            ok: false,
            id: Some(id),
            action: "noop".to_string(),
        }));
    }
    db::journal::remove_entry_backend(&backend, id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to remove journal entry: {}", e),
        )
    })?;
    Ok(Json(JournalMutationResponse {
        ok: true,
        id: Some(id),
        action: "removed".to_string(),
    }))
}

pub async fn get_home_tab() -> Result<Json<PreferenceResponse>, (StatusCode, String)> {
    let cfg = crate::config::load_config().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load config: {}", e),
        )
    })?;
    let home_tab = normalize_home_tab(&cfg.home_tab);
    Ok(Json(PreferenceResponse { ok: true, home_tab }))
}

fn normalize_home_tab(input: &str) -> String {
    let tab = input.to_lowercase();
    if tab == "watchlist" {
        "watchlist".to_string()
    } else {
        "positions".to_string()
    }
}

pub async fn set_home_tab(
    Json(body): Json<HomeTabRequest>,
) -> Result<Json<PreferenceResponse>, (StatusCode, String)> {
    let normalized = normalize_home_tab(&body.home_tab);
    if body.home_tab.to_lowercase() != normalized {
        return Err((
            StatusCode::BAD_REQUEST,
            "home tab must be 'positions' or 'watchlist'".to_string(),
        ));
    }

    let mut cfg = crate::config::load_config().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load config: {}", e),
        )
    })?;
    cfg.home_tab = normalized.clone();
    crate::config::save_config(&cfg).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save config: {}", e),
        )
    })?;

    Ok(Json(PreferenceResponse {
        ok: true,
        home_tab: normalized,
    }))
}

pub async fn set_theme(
    Json(body): Json<ThemeRequest>,
) -> Result<Json<ThemeResponse>, (StatusCode, String)> {
    let selected = body.theme.trim();
    if !THEME_NAMES.contains(&selected) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown theme '{}'", selected),
        ));
    }

    let mut cfg = crate::config::load_config().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load config: {}", e),
        )
    })?;
    cfg.theme = selected.to_string();
    crate::config::save_config(&cfg).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save config: {}", e),
        )
    })?;

    Ok(Json(ThemeResponse {
        ok: true,
        theme: selected.to_string(),
    }))
}

pub async fn get_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SummaryResponse>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let fx_rates = get_fx_rates_backend(&backend);

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        let prices = get_price_map_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices, &fx_rates)
    } else {
        let transactions = db::transactions::list_transactions_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        let prices = get_price_map_backend(&backend).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices, &fx_rates)
    };

    let total_value: Option<Decimal> = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum::<Decimal>()
        .into();

    // Get top 5 movers by absolute gain_pct
    let mut movers = positions.clone();
    movers.sort_by(|a, b| {
        let a_abs = a.gain_pct.unwrap_or(dec!(0)).abs();
        let b_abs = b.gain_pct.unwrap_or(dec!(0)).abs();
        b_abs
            .partial_cmp(&a_abs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    movers.truncate(5);

    Ok(Json(SummaryResponse {
        total_value,
        position_count: positions.len(),
        top_movers: movers,
        meta: view_model::fresh_meta(60),
    }))
}

pub async fn get_situation(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SituationSnapshot>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let snapshot = crate::analytics::situation::build_snapshot_backend(&backend).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to build situation snapshot: {}", e),
        )
    })?;

    Ok(Json(snapshot))
}

pub async fn get_deltas(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SituationDeltaReport>, (StatusCode, String)> {
    let backend = state.get_backend().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let report = crate::analytics::deltas::build_report_backend(
        &backend,
        crate::analytics::deltas::DeltaWindow::LastRefresh,
        true,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to build delta report: {}", e),
        )
    })?;

    Ok(Json(report))
}

fn color_to_hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => "#000000".to_string(),
        Color::White => "#ffffff".to_string(),
        Color::Red => "#ff0000".to_string(),
        Color::Green => "#00ff00".to_string(),
        Color::Blue => "#0000ff".to_string(),
        Color::Yellow => "#ffff00".to_string(),
        Color::Magenta => "#ff00ff".to_string(),
        Color::Cyan => "#00ffff".to_string(),
        Color::Gray => "#808080".to_string(),
        Color::DarkGray => "#404040".to_string(),
        _ => "#7f7f7f".to_string(),
    }
}

fn web_theme(name: &str) -> WebTheme {
    let t = theme::theme_by_name(name);
    WebTheme {
        name: name.to_string(),
        colors: WebThemeColors {
            bg_primary: color_to_hex(t.surface_0),
            bg_secondary: color_to_hex(t.surface_1),
            bg_tertiary: color_to_hex(t.surface_2),
            text_primary: color_to_hex(t.text_primary),
            text_secondary: color_to_hex(t.text_secondary),
            text_muted: color_to_hex(t.text_muted),
            text_accent: color_to_hex(t.text_accent),
            border: color_to_hex(t.border_inactive),
            accent: color_to_hex(t.border_active),
            green: color_to_hex(t.gain_green),
            red: color_to_hex(t.loss_red),
            yellow: color_to_hex(t.stale_yellow),
        },
    }
}

pub async fn get_ui_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<UiConfigResponse>, (StatusCode, String)> {
    let runtime_cfg = crate::config::load_config().unwrap_or_else(|_| state.config.clone());
    let tabs = view_model::tabs_for_config(&runtime_cfg);
    let themes = THEME_NAMES.iter().map(|n| web_theme(n)).collect();
    Ok(Json(UiConfigResponse {
        tabs,
        themes,
        current_theme: runtime_cfg.theme,
        home_tab: normalize_home_tab(&runtime_cfg.home_tab),
    }))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        bounded_limit, delete_alert, delete_journal, delete_transaction, delete_watchlist,
        get_asset_detail, history_change_pct, normalized_symbol, patch_journal, patch_transaction,
        post_alert, post_alert_ack, post_alert_rearm, post_journal, post_transaction,
        post_watchlist, range_from_history, volume_stats, AlertCreateRequest,
        AlertMutationResponse, AppState, JournalCreateRequest, JournalMutationResponse,
        JournalUpdateRequest, TransactionMutationRequest, TransactionMutationResponse,
        WatchlistMutationRequest, WatchlistMutationResponse,
    };
    use crate::models::price::HistoryRecord;
    use axum::extract::Path;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::Json;
    use rust_decimal_macros::dec;

    struct TestCtx {
        _tmp_dir: PathBuf,
        state: Arc<AppState>,
    }

    static TEST_DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn setup_test_ctx() -> TestCtx {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ctr = TEST_DB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_dir = std::env::temp_dir().join(format!(
            "pftui-web-api-tests-{}-{}-{}",
            std::process::id(),
            nonce,
            ctr
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let db_path = tmp_dir.join("test.db");
        let _ = crate::db::open_db(&db_path).unwrap();
        TestCtx {
            _tmp_dir: tmp_dir,
            state: Arc::new(AppState {
                db_path: db_path.to_string_lossy().to_string(),
                config: crate::config::Config::default(),
            }),
        }
    }

    fn run_async<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn watchlist_mutation_contract() {
        run_async(async {
            let ctx = setup_test_ctx();
            let created: Json<WatchlistMutationResponse> = post_watchlist(
                State(ctx.state.clone()),
                Json(WatchlistMutationRequest {
                    symbol: " msft ".to_string(),
                    category: Some("equity".to_string()),
                }),
            )
            .await
            .unwrap();
            assert!(created.0.ok);
            assert_eq!(created.0.symbol, "MSFT");
            assert_eq!(created.0.action, "added");

            let removed = delete_watchlist(State(ctx.state.clone()), Path("MSFT".to_string()))
                .await
                .unwrap();
            assert!(removed.0.ok);
            assert_eq!(removed.0.action, "removed");
        });
    }

    #[test]
    fn alert_mutation_contract() {
        run_async(async {
            let ctx = setup_test_ctx();
            let created: Json<AlertMutationResponse> = post_alert(
                State(ctx.state.clone()),
                Json(AlertCreateRequest {
                    rule_text: Some("MSFT above 400".to_string()),
                    kind: None,
                    symbol: None,
                    direction: None,
                    threshold: None,
                    from_level: None,
                }),
            )
            .await
            .unwrap();
            assert!(created.0.ok);
            assert_eq!(created.0.action, "created");
            let id = created.0.id.unwrap();

            let backend = ctx.state.get_backend().unwrap();
            crate::db::alerts::update_alert_status_backend(
                &backend,
                id,
                crate::alerts::AlertStatus::Triggered,
                Some("2026-03-05T12:00:00Z"),
            )
            .unwrap();

            let acked = post_alert_ack(State(ctx.state.clone()), Path(id))
                .await
                .unwrap();
            assert!(acked.0.ok);
            assert_eq!(acked.0.action, "acknowledged");

            let rearmed = post_alert_rearm(State(ctx.state.clone()), Path(id))
                .await
                .unwrap();
            assert!(rearmed.0.ok);
            assert_eq!(rearmed.0.action, "rearmed");

            let removed = delete_alert(State(ctx.state.clone()), Path(id))
                .await
                .unwrap();
            assert!(removed.0.ok);
            assert_eq!(removed.0.action, "removed");
        });
    }

    #[test]
    fn alert_mutation_contract_supports_stored_levels() {
        run_async(async {
            let ctx = setup_test_ctx();
            let backend = ctx.state.get_backend().unwrap();
            crate::db::price_cache::upsert_price_backend(
                &backend,
                &crate::models::price::PriceQuote {
                    symbol: "MSFT".to_string(),
                    price: dec!(410),
                    currency: "USD".to_string(),
                    fetched_at: "2026-03-18T12:00:00Z".to_string(),
                    source: "test".to_string(),
                    pre_market_price: None,
                    post_market_price: None,
                    post_market_change_percent: None,
                    previous_close: None,
                },
            )
            .unwrap();
            crate::db::technical_levels::upsert_levels_backend(
                &backend,
                "MSFT",
                &[crate::db::technical_levels::TechnicalLevelRecord {
                    id: None,
                    symbol: "MSFT".to_string(),
                    level_type: "resistance".to_string(),
                    price: 420.0,
                    strength: 0.8,
                    source_method: "test".to_string(),
                    timeframe: "1d".to_string(),
                    notes: Some("test resistance".to_string()),
                    computed_at: "2026-03-18T12:00:00Z".to_string(),
                }],
            )
            .unwrap();

            let created: Json<AlertMutationResponse> = post_alert(
                State(ctx.state.clone()),
                Json(AlertCreateRequest {
                    rule_text: None,
                    kind: Some("price".to_string()),
                    symbol: Some("MSFT".to_string()),
                    direction: None,
                    threshold: None,
                    from_level: Some("resistance".to_string()),
                }),
            )
            .await
            .unwrap();
            assert!(created.0.ok);

            let alerts = crate::db::alerts::list_alerts_backend(&backend).unwrap();
            assert_eq!(alerts.len(), 1);
            assert_eq!(alerts[0].threshold, "420.00");
            assert_eq!(alerts[0].direction.to_string(), "above");
        });
    }

    #[test]
    fn asset_detail_includes_nearest_levels() {
        run_async(async {
            let ctx = setup_test_ctx();
            let backend = ctx.state.get_backend().unwrap();
            crate::db::price_cache::upsert_price_backend(
                &backend,
                &crate::models::price::PriceQuote {
                    symbol: "MSFT".to_string(),
                    price: dec!(410),
                    currency: "USD".to_string(),
                    fetched_at: "2026-03-18T12:00:00Z".to_string(),
                    source: "test".to_string(),
                    pre_market_price: None,
                    post_market_price: None,
                    post_market_change_percent: None,
                    previous_close: None,
                },
            )
            .unwrap();
            crate::db::price_history::upsert_history_backend(
                &backend,
                "MSFT",
                "test",
                &[
                    crate::models::price::HistoryRecord {
                        date: "2026-03-17".to_string(),
                        close: dec!(400),
                        volume: Some(1_000),
                        open: None,
                        high: None,
                        low: None,
                    },
                    crate::models::price::HistoryRecord {
                        date: "2026-03-18".to_string(),
                        close: dec!(410),
                        volume: Some(1_200),
                        open: None,
                        high: None,
                        low: None,
                    },
                ],
            )
            .unwrap();
            crate::db::technical_levels::upsert_levels_backend(
                &backend,
                "MSFT",
                &[
                    crate::db::technical_levels::TechnicalLevelRecord {
                        id: None,
                        symbol: "MSFT".to_string(),
                        level_type: "support".to_string(),
                        price: 395.0,
                        strength: 0.6,
                        source_method: "test".to_string(),
                        timeframe: "1d".to_string(),
                        notes: Some("support".to_string()),
                        computed_at: "2026-03-18T12:00:00Z".to_string(),
                    },
                    crate::db::technical_levels::TechnicalLevelRecord {
                        id: None,
                        symbol: "MSFT".to_string(),
                        level_type: "resistance".to_string(),
                        price: 420.0,
                        strength: 0.8,
                        source_method: "test".to_string(),
                        timeframe: "1d".to_string(),
                        notes: Some("resistance".to_string()),
                        computed_at: "2026-03-18T12:00:00Z".to_string(),
                    },
                ],
            )
            .unwrap();

            let detail = get_asset_detail(State(ctx.state.clone()), Path("MSFT".to_string()))
                .await
                .unwrap();
            let levels = detail.0.levels.unwrap();
            assert_eq!(levels.support.unwrap().price, 395.0);
            assert_eq!(levels.resistance.unwrap().price, 420.0);
        });
    }

    #[test]
    fn journal_mutation_contract() {
        run_async(async {
            let ctx = setup_test_ctx();
            let created: Json<JournalMutationResponse> = post_journal(
                State(ctx.state.clone()),
                Json(JournalCreateRequest {
                    timestamp: Some("2026-03-05T12:00:00Z".to_string()),
                    content: "Test journal entry".to_string(),
                    tag: Some("thesis".to_string()),
                    symbol: Some("msft".to_string()),
                    conviction: None,
                    status: Some("open".to_string()),
                }),
            )
            .await
            .unwrap();
            assert!(created.0.ok);
            assert_eq!(created.0.action, "created");
            let id = created.0.id.unwrap();

            let updated = patch_journal(
                State(ctx.state.clone()),
                Path(id),
                Json(JournalUpdateRequest {
                    content: Some("Updated entry".to_string()),
                    status: Some("validated".to_string()),
                }),
            )
            .await
            .unwrap();
            assert!(updated.0.ok);
            assert_eq!(updated.0.action, "updated");

            let removed = delete_journal(State(ctx.state.clone()), Path(id))
                .await
                .unwrap();
            assert!(removed.0.ok);
            assert_eq!(removed.0.action, "removed");

            let noop = delete_journal(State(ctx.state.clone()), Path(id))
                .await
                .unwrap();
            assert!(!noop.0.ok);
            assert_eq!(noop.0.action, "noop");
        });
    }

    #[test]
    fn transaction_mutation_contract() {
        run_async(async {
            let ctx = setup_test_ctx();
            let created: Json<TransactionMutationResponse> = post_transaction(
                State(ctx.state.clone()),
                Json(TransactionMutationRequest {
                    symbol: "MSFT".to_string(),
                    category: "equity".to_string(),
                    tx_type: "buy".to_string(),
                    quantity: "3".to_string(),
                    price_per: "400".to_string(),
                    currency: Some("USD".to_string()),
                    date: "2026-03-05".to_string(),
                    notes: None,
                }),
            )
            .await
            .unwrap();
            assert!(created.0.ok);
            assert_eq!(created.0.action, "created");
            let id = created.0.id.unwrap();

            let patched = patch_transaction(
                State(ctx.state.clone()),
                Path(id),
                Json(TransactionMutationRequest {
                    symbol: "MSFT".to_string(),
                    category: "equity".to_string(),
                    tx_type: "sell".to_string(),
                    quantity: "1".to_string(),
                    price_per: "410".to_string(),
                    currency: Some("USD".to_string()),
                    date: "2026-03-06".to_string(),
                    notes: Some("take profit".to_string()),
                }),
            )
            .await
            .unwrap();
            assert!(patched.0.ok);
            assert_eq!(patched.0.action, "updated");

            let removed = delete_transaction(State(ctx.state.clone()), Path(id))
                .await
                .unwrap();
            assert!(removed.0.ok);
            assert_eq!(removed.0.action, "removed");

            let bad = post_transaction(
                State(ctx.state.clone()),
                Json(TransactionMutationRequest {
                    symbol: "AAPL".to_string(),
                    category: "invalid-category".to_string(),
                    tx_type: "buy".to_string(),
                    quantity: "1".to_string(),
                    price_per: "100".to_string(),
                    currency: Some("USD".to_string()),
                    date: "2026-03-06".to_string(),
                    notes: None,
                }),
            )
            .await;
            match bad {
                Ok(_) => panic!("expected BAD_REQUEST for invalid category"),
                Err((status, _)) => assert_eq!(status, StatusCode::BAD_REQUEST),
            }
        });
    }

    #[test]
    fn normalized_symbol_uppercases_and_trims() {
        assert_eq!(normalized_symbol("  aapl "), "AAPL");
    }

    #[test]
    fn bounded_limit_clamps_range() {
        assert_eq!(bounded_limit(None, 30, 100), 30);
        assert_eq!(bounded_limit(Some(0), 30, 100), 1);
        assert_eq!(bounded_limit(Some(150), 30, 100), 100);
    }

    #[test]
    fn history_change_pct_uses_lookback_point() {
        let h = vec![
            HistoryRecord {
                date: "2026-01-01".to_string(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-01-02".to_string(),
                close: dec!(110),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-01-03".to_string(),
                close: dec!(120),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];
        let day = history_change_pct(&h, 1).unwrap();
        assert!(day > dec!(9));
        assert!(day < dec!(10));
        assert_eq!(history_change_pct(&h, 2), Some(dec!(20)));
        assert!(history_change_pct(&h, 3).is_none());
    }

    #[test]
    fn range_and_volume_stats_read_recent_data() {
        let h = vec![
            HistoryRecord {
                date: "2026-01-01".to_string(),
                close: dec!(100),
                volume: Some(10),
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-01-02".to_string(),
                close: dec!(90),
                volume: Some(20),
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-01-03".to_string(),
                close: dec!(130),
                volume: Some(30),
                open: None,
                high: None,
                low: None,
            },
        ];
        let (lo, hi) = range_from_history(&h, 252);
        assert_eq!(lo, Some(dec!(90)));
        assert_eq!(hi, Some(dec!(130)));

        let (latest, avg) = volume_stats(&h);
        assert_eq!(latest, Some(30));
        assert_eq!(avg, Some(20));
    }
}
