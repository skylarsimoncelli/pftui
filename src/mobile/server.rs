use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    middleware,
    routing::get,
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rustls::crypto::CryptoProvider;
use serde::Serialize;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;

use crate::alerts::AlertStatus;
use crate::config::Config;
use crate::db;
use crate::db::backend::BackendConnection;
use crate::mobile::auth::{auth_middleware, health, MobileAuthState};
use crate::mobile::commands::certificate_fingerprint;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::web::view_model;

/// Shared state for the mobile API that holds a pre-opened database connection.
/// This avoids calling `open_from_config` (which uses `pg_runtime::block_on`)
/// from within async request handlers, preventing the "cannot start a runtime
/// from within a runtime" panic.
///
/// We store the `BackendConnection` inside a `std::sync::Mutex` because SQLite's
/// `Connection` is not `Send`. The mobile server only uses Postgres in practice,
/// but this keeps the type system happy while allowing the same code paths.
struct MobileAppState {
    backend: std::sync::Mutex<BackendConnection>,
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
    pub regime: Option<MobileRegimeSnapshot>,
    pub correlations: Vec<MobileCorrelationItem>,
    pub sentiment: Vec<MobileSentimentItem>,
    pub predictions: Vec<MobilePredictionItem>,
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
pub struct MobileRegimeSnapshot {
    pub regime: String,
    pub confidence: Option<f64>,
    pub drivers: Vec<String>,
    pub recorded_at: String,
    pub vix: Option<f64>,
    pub dxy: Option<f64>,
    pub yield_10y: Option<f64>,
    pub oil: Option<f64>,
    pub gold: Option<f64>,
    pub btc: Option<f64>,
}

#[derive(Serialize)]
pub struct MobileCorrelationItem {
    pub symbol_a: String,
    pub symbol_b: String,
    pub correlation: f64,
    pub period: String,
    pub recorded_at: String,
}

#[derive(Serialize)]
pub struct MobileSentimentItem {
    pub index_type: String,
    pub value: u8,
    pub classification: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct MobilePredictionItem {
    pub question: String,
    pub probability_pct: f64,
    pub category: String,
}

#[derive(Serialize)]
pub struct MobileDashboardResponse {
    pub generated_at: String,
    pub portfolio: MobilePortfolioResponse,
    pub analytics: MobileAnalyticsResponse,
    pub monitoring: MobileMonitoringResponse,
    pub situation: MobileSituationResponse,
}

#[derive(Serialize)]
pub struct MobileSituationResponse {
    pub title: String,
    pub subtitle: String,
    pub summary: Vec<MobileSituationStat>,
    pub watch_now: Vec<MobileSituationInsight>,
    pub portfolio_impacts: Vec<MobileSituationInsight>,
    pub risk_matrix: Vec<MobileRiskSignal>,
}

#[derive(Serialize)]
pub struct MobileSituationStat {
    pub label: String,
    pub value: String,
}

#[derive(Serialize)]
pub struct MobileSituationInsight {
    pub title: String,
    pub detail: String,
    pub value: String,
    pub severity: String,
}

#[derive(Serialize)]
pub struct MobileRiskSignal {
    pub label: String,
    pub detail: String,
    pub value: String,
    pub status: String,
    pub severity: String,
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
    pub server: MobileServerRuntime,
    pub database: MobileDatabaseHealth,
    pub daemon: MobileDaemonSnapshot,
    pub sources: Vec<MobileSourceStatus>,
}

#[derive(Serialize)]
pub struct MobileServerRuntime {
    pub pftui_version: String,
    pub backend: String,
    pub portfolio_mode: String,
    pub database_mode: String,
    pub mobile_port: u16,
    pub api_token_count: usize,
    pub session_ttl_hours: u64,
}

#[derive(Serialize)]
pub struct MobileDatabaseHealth {
    pub status: String,
    pub label: String,
    pub integrity: String,
    pub positions: usize,
    pub transactions: usize,
    pub watchlist: usize,
    pub tracked_prices: usize,
    pub stale_sources: usize,
    pub last_market_sync: Option<String>,
    pub last_news_sync: Option<String>,
}

#[derive(Serialize)]
pub struct MobileDaemonSnapshot {
    pub running: bool,
    pub status: String,
    pub cycle: u64,
    pub last_heartbeat: Option<String>,
    pub last_refresh_duration_secs: Option<f64>,
    pub interval_secs: u64,
    pub task_count: usize,
    pub error_count: usize,
    pub tasks: Vec<String>,
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

    // Initialize tracing subscriber for access logging (M-4)
    let subscriber = tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter("tower_http=debug,mobile_api=info")
        .compact()
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    let app_state = Arc::new(MobileAppState {
        backend: std::sync::Mutex::new(backend),
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
        // M-4: Access logging (outermost — sees all requests including auth failures)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        // H-2: Body size limit (1 MB)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        // H-2: Concurrency limit (50 concurrent requests)
        .layer(tower::limit::ConcurrencyLimitLayer::new(50))
        .with_state(auth_state);

    let addr: SocketAddr = format!("{}:{}", config.mobile.bind, config.mobile.port).parse()?;
    let tls = RustlsConfig::from_pem_file(cert_path.clone(), key_path.clone()).await?;
    let fingerprint = certificate_fingerprint(Path::new(&cert_path))?;

    println!("📱 pftui mobile API starting...");
    println!("   Listening on https://{}", addr);
    println!("   TLS fingerprint: {}", fingerprint);
    println!("   iOS setup: enter host or host:port, API token, then verify this fingerprint");

    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}

async fn get_dashboard(
    State(state): State<Arc<MobileAppState>>,
) -> Result<Json<MobileDashboardResponse>, (StatusCode, String)> {
    let backend = state
        .backend
        .lock()
        .map_err(|e| internal_error(anyhow::anyhow!("{}", e)))?;
    let portfolio = portfolio_payload(&backend, &state.config).map_err(internal_error)?;
    let analytics = analytics_payload(&backend);
    let monitoring = monitoring_payload(&backend, &state.config, &state.db_path).map_err(internal_error)?;
    let situation = situation_payload(&portfolio, &analytics, &monitoring);

    Ok(Json(MobileDashboardResponse {
        generated_at: Utc::now().to_rfc3339(),
        portfolio,
        analytics,
        monitoring,
        situation,
    }))
}

async fn get_portfolio(
    State(state): State<Arc<MobileAppState>>,
) -> Result<Json<MobilePortfolioResponse>, (StatusCode, String)> {
    let backend = state
        .backend
        .lock()
        .map_err(|e| internal_error(anyhow::anyhow!("{}", e)))?;
    Ok(Json(
        portfolio_payload(&backend, &state.config).map_err(internal_error)?,
    ))
}

async fn get_analytics(
    State(state): State<Arc<MobileAppState>>,
) -> Result<Json<MobileAnalyticsResponse>, (StatusCode, String)> {
    let backend = state
        .backend
        .lock()
        .map_err(|e| internal_error(anyhow::anyhow!("{}", e)))?;
    Ok(Json(analytics_payload(&backend)))
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

    let regime = crate::db::regime_snapshots::get_current_backend(backend)
        .unwrap_or(None)
        .map(|row| MobileRegimeSnapshot {
            regime: row.regime,
            confidence: row.confidence,
            drivers: parse_driver_list(row.drivers.as_deref()),
            recorded_at: row.recorded_at,
            vix: row.vix,
            dxy: row.dxy,
            yield_10y: row.yield_10y,
            oil: row.oil,
            gold: row.gold,
            btc: row.btc,
        });

    let correlations = crate::db::correlation_snapshots::list_current_backend(backend, Some("30d"))
        .unwrap_or_default()
        .into_iter()
        .take(4)
        .map(|row| MobileCorrelationItem {
            symbol_a: row.symbol_a,
            symbol_b: row.symbol_b,
            correlation: row.correlation,
            period: row.period,
            recorded_at: row.recorded_at,
        })
        .collect();

    let sentiment = ["crypto", "traditional"]
        .into_iter()
        .filter_map(|kind| {
            crate::db::sentiment_cache::get_latest_backend(backend, kind)
                .ok()
                .flatten()
        })
        .map(|row| MobileSentimentItem {
            index_type: row.index_type,
            value: row.value,
            classification: row.classification,
            updated_at: row.fetched_at,
        })
        .collect();

    let predictions = crate::db::predictions_cache::get_cached_predictions_backend(backend, 3)
        .unwrap_or_default()
        .into_iter()
        .map(|row| MobilePredictionItem {
            question: row.question,
            probability_pct: row.probability * 100.0,
            category: row.category.to_string(),
        })
        .collect();

    MobileAnalyticsResponse {
        timeframes,
        regime,
        correlations,
        sentiment,
        predictions,
    }
}

fn monitoring_payload(
    backend: &crate::db::backend::BackendConnection,
    config: &Config,
    db_path: &str,
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
        system: system_snapshot(backend, config, db_path)?,
    })
}

fn situation_payload(
    portfolio: &MobilePortfolioResponse,
    analytics: &MobileAnalyticsResponse,
    monitoring: &MobileMonitoringResponse,
) -> MobileSituationResponse {
    let average_score = average_timeframe_score(&analytics.timeframes);
    let watch_now = situation_watch_now(analytics, monitoring, average_score);
    let portfolio_impacts = situation_portfolio_impacts(portfolio);
    let risk_matrix = situation_risk_matrix(analytics, monitoring);
    let title = monitoring
        .latest_timeframe_signal
        .as_ref()
        .map(|signal| pretty_signal(&signal.signal_type))
        .or_else(|| {
            analytics
                .regime
                .as_ref()
                .map(|regime| pretty_signal(&regime.regime))
        })
        .unwrap_or_else(|| "Situation Stable".to_string());
    let subtitle = monitoring
        .latest_timeframe_signal
        .as_ref()
        .map(|signal| signal.description.clone())
        .unwrap_or_else(|| {
            format!(
                "{} positions • {} tracked layers",
                portfolio.position_count,
                analytics.timeframes.len()
            )
        });

    MobileSituationResponse {
        title,
        subtitle,
        summary: vec![
            MobileSituationStat {
                label: "Avg Score".to_string(),
                value: format!("{:.0}", average_score),
            },
            MobileSituationStat {
                label: "Alerts".to_string(),
                value: monitoring.triggered_alert_count.to_string(),
            },
            MobileSituationStat {
                label: "Tech Signals".to_string(),
                value: monitoring.technical_signal_count.to_string(),
            },
            MobileSituationStat {
                label: "Stale Sources".to_string(),
                value: monitoring.system.database.stale_sources.to_string(),
            },
        ],
        watch_now,
        portfolio_impacts,
        risk_matrix,
    }
}

fn system_snapshot(
    backend: &crate::db::backend::BackendConnection,
    config: &Config,
    db_path: &str,
) -> Result<MobileSystemSnapshot> {
    let daemon = crate::commands::daemon::read_status()
        .map(|status| MobileDaemonSnapshot {
            running: status.running,
            status: status.status,
            cycle: status.cycle,
            last_heartbeat: status.last_heartbeat,
            last_refresh_duration_secs: status.last_refresh_duration_secs,
            interval_secs: status.interval_secs,
            task_count: status.tasks.len(),
            error_count: status.errors.len(),
            tasks: status.tasks,
        })
        .unwrap_or(MobileDaemonSnapshot {
            running: false,
            status: "stopped".to_string(),
            cycle: 0,
            last_heartbeat: None,
            last_refresh_duration_secs: None,
            interval_secs: 0,
            task_count: 0,
            error_count: 0,
            tasks: Vec::new(),
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

    let sources = vec![
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
    ];

    let positions = crate::db::transactions::get_unique_symbols_backend(backend)
        .map(|rows| rows.len())
        .unwrap_or(0);
    let transactions = crate::db::transactions::count_transactions_backend(backend)
        .map(|count| count.max(0) as usize)
        .unwrap_or(0);
    let watchlist = crate::db::watchlist::list_watchlist_backend(backend)
        .map(|rows| rows.len())
        .unwrap_or(0);
    let stale_sources = sources.iter().filter(|source| source.status != "fresh").count();
    let integrity = database_integrity(backend);
    let database = MobileDatabaseHealth {
        status: database_health_status(&integrity, stale_sources),
        label: database_label(backend, db_path, config),
        integrity,
        positions,
        transactions,
        watchlist,
        tracked_prices: prices.len(),
        stale_sources,
        last_market_sync: latest_price_fetch.map(|ts| ts.to_rfc3339()),
        last_news_sync: latest_news_fetch.map(|ts| ts.to_rfc3339()),
    };

    Ok(MobileSystemSnapshot {
        server: MobileServerRuntime {
            pftui_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: backend_name(config).to_string(),
            portfolio_mode: if config.is_percentage_mode() {
                "percentage".to_string()
            } else {
                "full".to_string()
            },
            database_mode: if config.effective_postgres_read_only() {
                "read-only".to_string()
            } else {
                "read-write".to_string()
            },
            mobile_port: config.mobile.port,
            api_token_count: config.mobile.api_tokens.len(),
            session_ttl_hours: config.mobile.session_ttl_hours,
        },
        database,
        daemon,
        sources,
    })
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
    eprintln!("[mobile-api] Internal error: {:?}", error);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error".to_string(),
    )
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

fn parse_driver_list(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
}

fn pretty_signal(raw: &str) -> String {
    raw.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn average_timeframe_score(timeframes: &[MobileTimeframeOutlook]) -> f64 {
    if timeframes.is_empty() {
        return 0.0;
    }
    timeframes.iter().map(|row| row.score).sum::<f64>() / timeframes.len() as f64
}

fn severity_weight(value: &str) -> i32 {
    match value {
        "critical" => 3,
        "elevated" | "warning" => 2,
        _ => 1,
    }
}

fn situation_watch_now(
    analytics: &MobileAnalyticsResponse,
    monitoring: &MobileMonitoringResponse,
    average_score: f64,
) -> Vec<MobileSituationInsight> {
    let mut items = Vec::new();

    if let Some(signal) = &monitoring.latest_timeframe_signal {
        items.push(MobileSituationInsight {
            title: pretty_signal(&signal.signal_type),
            detail: signal.description.clone(),
            value: signal.severity.clone(),
            severity: normalize_severity(&signal.severity).to_string(),
        });
    }

    if let Some(regime) = &analytics.regime {
        items.push(MobileSituationInsight {
            title: format!("Regime: {}", pretty_signal(&regime.regime)),
            detail: regime.drivers.iter().take(2).cloned().collect::<Vec<_>>().join(" • "),
            value: regime
                .confidence
                .map(|value| format!("{}%", (value * 100.0).round() as i32))
                .unwrap_or_else(|| "—".to_string()),
            severity: "normal".to_string(),
        });
    }

    if let Some(strongest) = monitoring.market_pulse.iter().max_by(|left, right| {
        change_magnitude(right.day_change_pct).total_cmp(&change_magnitude(left.day_change_pct))
    }) {
        let change = strongest
            .day_change_pct
            .map(|value| value.round_dp(2).to_string())
            .unwrap_or_else(|| strongest.value.map(|value| value.round_dp(2).to_string()).unwrap_or_else(|| "—".to_string()));
        let severity = if change_magnitude(strongest.day_change_pct) >= 2.5 {
            "critical"
        } else {
            "elevated"
        };
        items.push(MobileSituationInsight {
            title: format!("{} is leading the tape", strongest.symbol),
            detail: strongest.name.clone(),
            value: change,
            severity: severity.to_string(),
        });
    }

    if monitoring.triggered_alert_count > 0 {
        items.push(MobileSituationInsight {
            title: format!("{} live alerts need triage", monitoring.triggered_alert_count),
            detail: "Triggered rules are active in the current monitoring stack.".to_string(),
            value: "alert".to_string(),
            severity: "critical".to_string(),
        });
    }

    if monitoring.system.database.stale_sources > 0 {
        items.push(MobileSituationInsight {
            title: format!(
                "{} stale data sources",
                monitoring.system.database.stale_sources
            ),
            detail: "Operational trust is degraded until the slow feeds refresh.".to_string(),
            value: "ops".to_string(),
            severity: "elevated".to_string(),
        });
    }

    if average_score <= -15.0 {
        items.push(MobileSituationInsight {
            title: "Average timeframe tone is soft".to_string(),
            detail: "Cross-layer analytics are leaning defensive.".to_string(),
            value: format!("{average_score:.0}"),
            severity: if average_score <= -35.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }

    items.sort_by(|left, right| {
        severity_weight(&right.severity)
            .cmp(&severity_weight(&left.severity))
            .then_with(|| left.title.cmp(&right.title))
    });
    items.truncate(6);
    items
}

fn situation_portfolio_impacts(
    portfolio: &MobilePortfolioResponse,
) -> Vec<MobileSituationInsight> {
    let mut items: Vec<_> = portfolio
        .positions
        .iter()
        .map(|position| {
            let day_change = position
                .day_change_pct
                .map(|value| value.round_dp(2).to_string())
                .unwrap_or_else(|| "—".to_string());
            let magnitude = position.day_change_pct.map(|value| value.abs()).unwrap_or(dec!(0));
            let severity = if magnitude >= dec!(3) {
                "elevated"
            } else {
                "normal"
            };
            (
                change_magnitude(position.day_change_pct),
                MobileSituationInsight {
                    title: position.symbol.clone(),
                    detail: format!(
                        "{} • {} allocation",
                        position.name,
                        position
                            .allocation_pct
                            .map(|value| value.round_dp(2).to_string())
                            .unwrap_or_else(|| "—".to_string())
                    ),
                    value: day_change,
                    severity: severity.to_string(),
                },
            )
        })
        .collect();

    items.sort_by(|left, right| right.0.total_cmp(&left.0));
    items.truncate(6);
    items.into_iter().map(|(_, item)| item).collect()
}

fn situation_risk_matrix(
    analytics: &MobileAnalyticsResponse,
    monitoring: &MobileMonitoringResponse,
) -> Vec<MobileRiskSignal> {
    let mut rows = Vec::new();

    if let Some(regime) = &analytics.regime {
        if let Some(vix) = regime.vix {
            rows.push(MobileRiskSignal {
                label: "Volatility".to_string(),
                detail: "Equity stress proxy".to_string(),
                value: format!("{vix:.1}"),
                status: if vix >= 20.0 { "warning" } else { "fresh" }.to_string(),
                severity: if vix >= 25.0 {
                    "critical"
                } else if vix >= 20.0 {
                    "elevated"
                } else {
                    "normal"
                }
                .to_string(),
            });
        }
        if let Some(dxy) = regime.dxy {
            rows.push(MobileRiskSignal {
                label: "Dollar".to_string(),
                detail: "Funding and global pressure".to_string(),
                value: format!("{dxy:.1}"),
                status: if dxy >= 105.0 { "warning" } else { "fresh" }.to_string(),
                severity: if dxy >= 106.0 {
                    "critical"
                } else if dxy >= 105.0 {
                    "elevated"
                } else {
                    "normal"
                }
                .to_string(),
            });
        }
    }

    if let Some(item) = monitoring
        .market_pulse
        .iter()
        .find(|item| item.symbol.contains("BTC") || item.name.to_lowercase().contains("bitcoin"))
    {
        let change = item
            .day_change_pct
            .map(|value| value.round_dp(2).to_string())
            .unwrap_or_else(|| "—".to_string());
        let move_pct = change_magnitude(item.day_change_pct);
        rows.push(MobileRiskSignal {
            label: "Crypto Risk".to_string(),
            detail: "High-beta sentiment read".to_string(),
            value: change,
            status: if move_pct <= -2.5 { "warning" } else { "fresh" }.to_string(),
            severity: if move_pct <= -4.0 {
                "critical"
            } else if move_pct <= -2.5 {
                "elevated"
            } else {
                "normal"
            }
            .to_string(),
        });
    }

    if let Some(sentiment) = analytics
        .sentiment
        .iter()
        .find(|row| row.index_type.eq_ignore_ascii_case("crypto"))
    {
        rows.push(MobileRiskSignal {
            label: "Crypto Sentiment".to_string(),
            detail: pretty_signal(&sentiment.classification),
            value: sentiment.value.to_string(),
            status: if sentiment.value <= 25 { "warning" } else { "fresh" }.to_string(),
            severity: if sentiment.value <= 20 {
                "critical"
            } else if sentiment.value <= 25 {
                "elevated"
            } else {
                "normal"
            }
            .to_string(),
        });
    }

    if let Some(macro_row) = analytics.timeframes.iter().find(|row| row.timeframe == "macro") {
        rows.push(MobileRiskSignal {
            label: "Macro Stack".to_string(),
            detail: "Long-cycle conviction".to_string(),
            value: format!("{:.0}", macro_row.score),
            status: if macro_row.score < -15.0 { "warning" } else { "fresh" }.to_string(),
            severity: if macro_row.score < -35.0 {
                "critical"
            } else if macro_row.score < -15.0 {
                "elevated"
            } else {
                "normal"
            }
            .to_string(),
        });
    }

    rows
}

fn normalize_severity(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "warning" | "notable" | "elevated" => "elevated",
        _ => "normal",
    }
}

fn change_magnitude(value: Option<Decimal>) -> f64 {
    value
        .and_then(|number| number.to_string().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn backend_name(config: &Config) -> &'static str {
    match config.database_backend {
        crate::config::DatabaseBackend::Sqlite => "sqlite",
        crate::config::DatabaseBackend::Postgres => "postgres",
    }
}

fn database_label(
    backend: &crate::db::backend::BackendConnection,
    db_path: &str,
    config: &Config,
) -> String {
    match backend {
        crate::db::backend::BackendConnection::Sqlite { .. } => Path::new(db_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pftui.db")
            .to_string(),
        crate::db::backend::BackendConnection::Postgres { .. } => config
            .database_url
            .as_deref()
            .and_then(postgres_label_from_url)
            .unwrap_or_else(|| "postgres".to_string()),
    }
}

fn postgres_label_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let host_and_db = trimmed.split('@').nth(1).unwrap_or(trimmed);
    let host = host_and_db.split('/').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn database_integrity(backend: &crate::db::backend::BackendConnection) -> String {
    match backend {
        crate::db::backend::BackendConnection::Sqlite { conn } => conn
            .query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))
            .unwrap_or_else(|_| "check failed".to_string()),
        crate::db::backend::BackendConnection::Postgres { pool } => {
            crate::db::pg_runtime::block_on(async {
                sqlx::query("SELECT 1").fetch_one(pool).await.map(|_| ())
            })
            .map(|_| "connected".to_string())
            .unwrap_or_else(|_| "check failed".to_string())
        }
    }
}

fn database_health_status(integrity: &str, stale_sources: usize) -> String {
    if integrity != "ok" && integrity != "connected" {
        "critical".to_string()
    } else if stale_sources > 0 {
        "warning".to_string()
    } else {
        "healthy".to_string()
    }
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
    use super::{
        database_health_status, parse_driver_list, postgres_label_from_url, relative_time,
        source_status, timeframe_label, timestamp_to_rfc3339,
    };
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

    #[test]
    fn parse_driver_list_reads_json_arrays() {
        assert_eq!(
            parse_driver_list(Some("[\"dollar strength\",\"oil shock\"]")),
            vec!["dollar strength".to_string(), "oil shock".to_string()]
        );
        assert!(parse_driver_list(Some("not-json")).is_empty());
    }

    #[test]
    fn postgres_label_uses_host_and_port() {
        assert_eq!(
            postgres_label_from_url("postgres://user:pass@db.example:5432/pftui"),
            Some("db.example:5432".to_string())
        );
    }

    #[test]
    fn database_health_reflects_integrity_and_freshness() {
        assert_eq!(database_health_status("ok", 0), "healthy");
        assert_eq!(database_health_status("ok", 1), "warning");
        assert_eq!(database_health_status("check failed", 0), "critical");
    }
}
