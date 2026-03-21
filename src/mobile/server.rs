use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use axum::{extract::State, http::StatusCode, middleware, routing::get, Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rustls::crypto::CryptoProvider;
use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::config::Config;
use crate::db;
use crate::db::backend::BackendConnection;
use crate::mobile::auth::{auth_middleware, health, MobileAuthState};
use crate::mobile::commands::certificate_fingerprint;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::web::view_model;

/// Shared state for the mobile API server.
///
/// We store a `PgPool` directly (not `BackendConnection`) because:
/// 1. The mobile API requires Postgres — SQLite is not supported.
/// 2. PgPool is `Clone + Send + Sync`, making it safe to share across
///    async handlers and move into `spawn_blocking` closures.
/// 3. DB query functions use `pg_runtime::block_on()` internally, which
///    panics if called from within a tokio runtime. By running DB work
///    on the blocking thread pool via `spawn_blocking`, we avoid nesting
///    runtimes.
struct MobileAppState {
    pool: sqlx::PgPool,
    config: Config,
}

#[derive(Serialize)]
pub struct MobilePortfolioResponse {
    pub total_value: Option<Decimal>,
    pub daily_change_pct: Option<Decimal>,
    pub position_count: usize,
    pub positions: Vec<MobilePosition>,
}

#[derive(Serialize)]
pub struct MobilePosition {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub current_price: Option<Decimal>,
    pub current_value: Option<Decimal>,
    pub allocation_pct: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
}

#[derive(Serialize)]
pub struct MobileAnalyticsResponse {
    pub timeframes: Vec<MobileTimeframeOutlook>,
}

#[derive(Serialize)]
pub struct MobileTimeframeOutlook {
    pub timeframe: String,
    pub label: String,
    pub score: f64,
    pub summary: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Serialize)]
pub struct MobileDashboardResponse {
    pub generated_at: String,
    pub portfolio: MobilePortfolioResponse,
    pub analytics: MobileAnalyticsResponse,
    pub monitoring: MobileMonitoringResponse,
}

#[derive(Serialize)]
pub struct MobileMonitoringResponse {
    pub latest_timeframe_signal: Option<MobileLatestSignal>,
    pub technical_signal_count: usize,
    pub triggered_alert_count: usize,
    pub market_pulse: Vec<MobileMarketPulseItem>,
    pub watchlist: Vec<MobileWatchlistItem>,
    pub news: Vec<MobileNewsItem>,
    pub system: MobileSystemSnapshot,
}

#[derive(Serialize)]
pub struct MobileLatestSignal {
    pub signal_type: String,
    pub severity: String,
    pub description: String,
    pub detected_at: String,
}

#[derive(Serialize)]
pub struct MobileMarketPulseItem {
    pub symbol: String,
    pub name: String,
    pub value: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
}

#[derive(Serialize)]
pub struct MobileWatchlistItem {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub current_price: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub target_price: Option<Decimal>,
    pub distance_pct: Option<Decimal>,
    pub target_hit: bool,
}

#[derive(Serialize)]
pub struct MobileNewsItem {
    pub title: String,
    pub source: String,
    pub published_at: String,
    pub source_type: String,
}

#[derive(Serialize)]
pub struct MobileSystemSnapshot {
    pub daemon: MobileDaemonSnapshot,
    pub sources: Vec<MobileSourceStatus>,
}

#[derive(Serialize)]
pub struct MobileDaemonSnapshot {
    pub running: bool,
    pub status: String,
    pub last_heartbeat: Option<String>,
}

#[derive(Serialize)]
pub struct MobileSourceStatus {
    pub name: String,
    pub status: String,
    pub freshness: String,
    pub last_fetch: Option<String>,
    pub records: usize,
}

pub async fn run_server(backend: BackendConnection, config: Config) -> Result<()> {
    let _ = CryptoProvider::install_default(rustls::crypto::ring::default_provider());

    if !config.mobile.enabled {
        bail!(
            "Mobile API is disabled. Run `pftui system mobile enable` first or set `mobile.enabled = true`."
        );
    }

    if config.mobile.api_tokens.is_empty() {
        bail!(
            "No mobile API tokens configured. Run `pftui system mobile token generate --permission read --name ios` first."
        );
    }
    let cert_path = config.mobile.cert_path.clone().ok_or_else(|| {
        anyhow!("mobile.cert_path is missing; re-run `pftui system mobile enable`")
    })?;
    let key_path = config.mobile.key_path.clone().ok_or_else(|| {
        anyhow!("mobile.key_path is missing; re-run `pftui system mobile enable`")
    })?;

    let pool = backend
        .postgres_pool()
        .ok_or_else(|| anyhow!("Mobile API server requires the Postgres backend"))?
        .clone();
    let app_state = Arc::new(MobileAppState {
        pool,
        config: config.clone(),
    });
    let auth_state = Arc::new(MobileAuthState::new(
        config.mobile.api_tokens.clone(),
        config.mobile.session_ttl_hours,
    ));

    let api_routes = Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/portfolio", get(get_portfolio))
        .route("/analytics", get(get_analytics))
        .route("/ui-config", get(get_ui_config_mobile))
        .with_state(app_state);

    let app = Router::new()
        .route("/health", get(health))
        .nest("/api", api_routes)
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        .with_state(auth_state);

    let addr: SocketAddr = format!("{}:{}", config.mobile.bind, config.mobile.port).parse()?;
    let tls = RustlsConfig::from_pem_file(cert_path.clone(), key_path.clone()).await?;
    let fingerprint = certificate_fingerprint(Path::new(&cert_path))?;

    println!("📱 pftui mobile API starting...");
    println!("   Listening on https://{}", addr);
    println!("   TLS fingerprint: {}", fingerprint);
    println!("   iOS setup: enter host or host:port, API token, then verify this fingerprint");

    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

/// Run sync DB work on the blocking thread pool so that `pg_runtime::block_on`
/// (used internally by every Postgres query function) does not nest inside the
/// axum server's tokio runtime.
async fn blocking_db<F, T>(state: &Arc<MobileAppState>, f: F) -> Result<T, (StatusCode, String)>
where
    F: FnOnce(&BackendConnection, &Config) -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    let pool = state.pool.clone();
    let config = state.config.clone();
    tokio::task::spawn_blocking(move || {
        let backend = BackendConnection::Postgres { pool };
        f(&backend, &config)
    })
    .await
    .map_err(|e| internal_error(anyhow::anyhow!("task join error: {}", e)))?
    .map_err(internal_error)
}

async fn get_dashboard(
    State(state): State<Arc<MobileAppState>>,
) -> Result<Json<MobileDashboardResponse>, (StatusCode, String)> {
    let resp = blocking_db(&state, |backend, config| {
        let portfolio = portfolio_payload(backend, config)?;
        let analytics = analytics_payload(backend);
        let monitoring = monitoring_payload(backend)?;
        Ok(MobileDashboardResponse {
            generated_at: Utc::now().to_rfc3339(),
            portfolio,
            analytics,
            monitoring,
        })
    })
    .await?;
    Ok(Json(resp))
}

async fn get_portfolio(
    State(state): State<Arc<MobileAppState>>,
) -> Result<Json<MobilePortfolioResponse>, (StatusCode, String)> {
    let resp = blocking_db(&state, portfolio_payload).await?;
    Ok(Json(resp))
}

async fn get_analytics(
    State(state): State<Arc<MobileAppState>>,
) -> Result<Json<MobileAnalyticsResponse>, (StatusCode, String)> {
    let resp = blocking_db(&state, |backend, _config| Ok(analytics_payload(backend))).await?;
    Ok(Json(resp))
}

async fn get_ui_config_mobile(
    State(state): State<Arc<MobileAppState>>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "base_currency": state.config.base_currency,
        "theme": state.config.theme,
    }))
}

fn portfolio_payload(
    backend: &crate::db::backend::BackendConnection,
    config: &Config,
) -> Result<MobilePortfolioResponse> {
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let prices = crate::db::price_cache::get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect::<HashMap<_, _>>();

    let positions = load_positions(backend, config, &prices, &fx_rates)?;

    let total_value: Option<Decimal> = positions
        .iter()
        .filter_map(|position| position.current_value)
        .sum::<Decimal>()
        .into();
    let daily_change_pct = portfolio_day_change_pct(backend, &positions, total_value);

    let mut mobile_positions: Vec<MobilePosition> = positions
        .into_iter()
        .map(|position| {
            let day_change_pct = if position.category == AssetCategory::Cash {
                Some(dec!(0))
            } else {
                day_change_pct_backend(backend, &position.symbol)
            };

            MobilePosition {
                symbol: position.symbol,
                name: position.name,
                category: position.category.to_string(),
                current_price: position.current_price,
                current_value: position.current_value,
                allocation_pct: position.allocation_pct,
                day_change_pct,
            }
        })
        .collect();

    mobile_positions.sort_by(|left, right| {
        right
            .current_value
            .unwrap_or(dec!(0))
            .cmp(&left.current_value.unwrap_or(dec!(0)))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    Ok(MobilePortfolioResponse {
        total_value,
        daily_change_pct,
        position_count: mobile_positions.len(),
        positions: mobile_positions,
    })
}

fn analytics_payload(backend: &crate::db::backend::BackendConnection) -> MobileAnalyticsResponse {
    let configured = crate::db::mobile_timeframe_scores::list_scores_backend(backend)
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row.timeframe.clone(), row))
        .collect::<HashMap<_, _>>();

    let timeframes = ["low", "medium", "high", "macro"]
        .into_iter()
        .map(|timeframe| {
            if let Some(row) = configured.get(timeframe) {
                MobileTimeframeOutlook {
                    timeframe: row.timeframe.clone(),
                    label: timeframe_label(timeframe).to_string(),
                    score: row.score,
                    summary: row.summary.clone(),
                    updated_at: Some(row.updated_at.clone()),
                }
            } else {
                MobileTimeframeOutlook {
                    timeframe: timeframe.to_string(),
                    label: timeframe_label(timeframe).to_string(),
                    score: 0.0,
                    summary: None,
                    updated_at: None,
                }
            }
        })
        .collect();

    MobileAnalyticsResponse { timeframes }
}

fn monitoring_payload(
    backend: &crate::db::backend::BackendConnection,
) -> Result<MobileMonitoringResponse> {
    let prices = crate::db::price_cache::get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect::<HashMap<_, _>>();

    let latest_timeframe_signal = crate::db::timeframe_signals::latest_signal_backend(backend)?
        .map(|signal| MobileLatestSignal {
            signal_type: signal.signal_type,
            severity: signal.severity,
            description: signal.description,
            detected_at: signal.detected_at,
        });

    let technical_signal_count =
        crate::db::technical_signals::list_signals_backend(backend, None, None, Some(200))
            .map(|rows| rows.len())
            .unwrap_or(0);
    let triggered_alert_count = crate::db::alerts::list_alerts_backend(backend)
        .map(|rows| {
            rows.into_iter()
                .filter(|row| row.status == AlertStatus::Triggered)
                .count()
        })
        .unwrap_or(0);

    let market_pulse = view_model::market_overview_symbols()
        .into_iter()
        .take(6)
        .map(|spec| MobileMarketPulseItem {
            symbol: spec.symbol.clone(),
            name: spec.name,
            value: prices.get(&spec.symbol).copied(),
            day_change_pct: day_change_pct_backend(backend, &spec.symbol),
        })
        .collect();

    let watchlist = db::watchlist::list_watchlist_backend(backend)?
        .into_iter()
        .take(8)
        .map(|item| {
            let category: AssetCategory = item.category.parse().unwrap_or(AssetCategory::Equity);
            let quote_symbol = view_model::watchlist_quote_symbol(&item.symbol, category);
            let current_price = prices
                .get(&item.symbol)
                .copied()
                .or_else(|| prices.get(&quote_symbol).copied());
            let target_price = item
                .target_price
                .and_then(|value| value.parse::<Decimal>().ok());
            let (distance_pct, target_hit) = view_model::compute_watchlist_proximity(
                current_price,
                target_price,
                item.target_direction.as_deref(),
            );

            MobileWatchlistItem {
                symbol: item.symbol.clone(),
                name: crate::models::asset_names::resolve_name(&item.symbol),
                category: category.to_string(),
                current_price,
                day_change_pct: day_change_pct_backend(backend, &quote_symbol)
                    .or_else(|| day_change_pct_backend(backend, &item.symbol)),
                target_price,
                distance_pct,
                target_hit,
            }
        })
        .collect();

    let news =
        crate::db::news_cache::get_latest_news_backend(backend, 5, None, None, None, Some(72))?
            .into_iter()
            .map(|entry| MobileNewsItem {
                title: entry.title,
                source: entry.source,
                published_at: timestamp_to_rfc3339(entry.published_at),
                source_type: entry.source_type,
            })
            .collect();

    Ok(MobileMonitoringResponse {
        latest_timeframe_signal,
        technical_signal_count,
        triggered_alert_count,
        market_pulse,
        watchlist,
        news,
        system: system_snapshot(backend),
    })
}

fn system_snapshot(backend: &crate::db::backend::BackendConnection) -> MobileSystemSnapshot {
    let daemon = crate::commands::daemon::read_status()
        .map(|status| MobileDaemonSnapshot {
            running: status.running,
            status: status.status,
            last_heartbeat: status.last_heartbeat,
        })
        .unwrap_or(MobileDaemonSnapshot {
            running: false,
            status: "stopped".to_string(),
            last_heartbeat: None,
        });

    let prices = crate::db::price_cache::get_all_cached_prices_backend(backend).unwrap_or_default();
    let latest_price_fetch = prices
        .iter()
        .filter_map(|quote| parse_timestamp(&quote.fetched_at))
        .max();

    let news =
        crate::db::news_cache::get_latest_news_backend(backend, 50, None, None, None, Some(72))
            .unwrap_or_default();
    let latest_news_fetch = news
        .iter()
        .filter_map(|entry| parse_timestamp(&entry.fetched_at))
        .max();

    let predictions = crate::db::predictions_cache::get_cached_predictions_backend(backend, 200)
        .unwrap_or_default();
    let latest_prediction_fetch = crate::db::predictions_cache::get_last_update_backend(backend)
        .ok()
        .flatten()
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

    let sentiments = ["crypto", "traditional"]
        .into_iter()
        .filter_map(|kind| {
            crate::db::sentiment_cache::get_latest_backend(backend, kind)
                .ok()
                .flatten()
        })
        .collect::<Vec<_>>();
    let latest_sentiment_fetch = sentiments
        .iter()
        .filter_map(|entry| parse_timestamp(&entry.fetched_at))
        .max();

    MobileSystemSnapshot {
        daemon,
        sources: vec![
            source_status("Prices", latest_price_fetch, prices.len(), 15 * 60),
            source_status("News", latest_news_fetch, news.len(), 30 * 60),
            source_status(
                "Predictions",
                latest_prediction_fetch,
                predictions.len(),
                2 * 60 * 60,
            ),
            source_status(
                "Sentiment",
                latest_sentiment_fetch,
                sentiments.len(),
                2 * 60 * 60,
            ),
        ],
    }
}

fn source_status(
    name: &str,
    last_fetch: Option<DateTime<Utc>>,
    records: usize,
    fresh_within_secs: i64,
) -> MobileSourceStatus {
    let now = Utc::now();
    let freshness = last_fetch
        .map(|ts| relative_time(ts, now))
        .unwrap_or_else(|| "never".to_string());
    let status = match (records, last_fetch) {
        (0, _) => "empty",
        (_, Some(ts)) if now.signed_duration_since(ts).num_seconds() <= fresh_within_secs => {
            "fresh"
        }
        _ => "stale",
    };

    MobileSourceStatus {
        name: name.to_string(),
        status: status.to_string(),
        freshness,
        last_fetch: last_fetch.map(|ts| ts.to_rfc3339()),
        records,
    }
}

fn load_positions(
    backend: &crate::db::backend::BackendConnection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    fx_rates: &HashMap<String, Decimal>,
) -> Result<Vec<Position>> {
    if config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations_backend(backend)?;
        Ok(compute_positions_from_allocations(
            &allocations,
            prices,
            fx_rates,
        ))
    } else {
        let transactions = db::transactions::list_transactions_backend(backend)?;
        Ok(compute_positions(&transactions, prices, fx_rates))
    }
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn timeframe_label(value: &str) -> &'static str {
    match value {
        "low" => "Low Timeframe",
        "medium" => "Medium Timeframe",
        "high" => "High Timeframe",
        "macro" => "Macro Timeframe",
        _ => "Timeframe",
    }
}

fn day_change_pct_backend(
    backend: &crate::db::backend::BackendConnection,
    symbol: &str,
) -> Option<Decimal> {
    let history = crate::db::price_history::get_history_backend(backend, symbol, 2).ok()?;
    if history.len() < 2 {
        return None;
    }
    let latest = history.last()?.close;
    let previous = history.get(history.len() - 2)?.close;
    if previous == dec!(0) {
        return None;
    }
    Some((latest - previous) / previous * dec!(100))
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }

    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
}

fn relative_time(timestamp: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = now.signed_duration_since(timestamp).num_seconds();
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

fn timestamp_to_rfc3339(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

fn portfolio_day_change_pct(
    backend: &crate::db::backend::BackendConnection,
    positions: &[Position],
    total_value: Option<Decimal>,
) -> Option<Decimal> {
    let current_total = total_value?;

    let mut previous_total = dec!(0);
    let mut has_non_cash = false;

    for position in positions {
        if position.category == AssetCategory::Cash {
            if let Some(value) = position.current_value {
                previous_total += value;
            }
            continue;
        }

        has_non_cash = true;
        let history =
            match crate::db::price_history::get_history_backend(backend, &position.symbol, 2) {
                Ok(history) if history.len() >= 2 => history,
                _ => return None,
            };
        previous_total += history[history.len() - 2].close * position.quantity;
    }

    if !has_non_cash || previous_total <= dec!(0) {
        return Some(dec!(0));
    }

    Some((current_total - previous_total) / previous_total * dec!(100))
}

#[cfg(test)]
mod tests {
    use super::{relative_time, source_status, timeframe_label, timestamp_to_rfc3339};
    use chrono::{TimeZone, Utc};

    #[test]
    fn timeframe_labels_cover_known_values() {
        assert_eq!(timeframe_label("low"), "Low Timeframe");
        assert_eq!(timeframe_label("macro"), "Macro Timeframe");
        assert_eq!(timeframe_label("other"), "Timeframe");
    }

    #[test]
    fn source_status_marks_empty_and_stale() {
        let old = Utc.with_ymd_and_hms(2026, 3, 19, 9, 0, 0).unwrap();

        let empty = source_status("News", None, 0, 300);
        assert_eq!(empty.status, "empty");
        assert_eq!(empty.freshness, "never");

        let stale = source_status("Prices", Some(old), 10, 60);
        assert_eq!(stale.status, "stale");
        assert!(stale.last_fetch.is_some());
    }

    #[test]
    fn relative_time_uses_compact_units() {
        let now = Utc.with_ymd_and_hms(2026, 3, 19, 12, 0, 0).unwrap();
        let ts = Utc.with_ymd_and_hms(2026, 3, 19, 11, 58, 0).unwrap();
        assert_eq!(relative_time(ts, now), "2m ago");
    }

    #[test]
    fn unix_timestamp_formats_as_rfc3339() {
        assert_eq!(timestamp_to_rfc3339(0), "1970-01-01T00:00:00+00:00");
    }
}
