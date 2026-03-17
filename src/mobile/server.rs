use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use axum::{extract::State, middleware, routing::get, Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use rustls::crypto::CryptoProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::config::Config;
use crate::db;
use crate::mobile::auth::{auth_middleware, health, MobileAuthState};
use crate::mobile::commands::certificate_fingerprint;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::web::api::{self, AppState};

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

pub async fn run_server(db_path: String, config: Config) -> Result<()> {
    // Ensure rustls has a crypto provider before any TLS operations
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

    let app_state = Arc::new(AppState {
        db_path,
        config: config.clone(),
    });
    let auth_state = Arc::new(MobileAuthState::new(
        config.mobile.api_tokens.clone(),
        config.mobile.session_ttl_hours,
    ));

    let api_routes = Router::new()
        .route("/portfolio", get(get_portfolio))
        .route("/analytics", get(get_analytics))
        .route("/ui-config", get(api::get_ui_config))
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

async fn get_portfolio(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MobilePortfolioResponse>, (axum::http::StatusCode, String)> {
    let backend = crate::db::backend::open_from_config(&state.config, Path::new(&state.db_path))
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
    let prices = crate::db::price_cache::get_all_cached_prices_backend(&backend)
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect::<HashMap<_, _>>();

    let positions = load_positions(&backend, &state.config, &prices, &fx_rates).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load positions: {}", e),
        )
    })?;

    let total_value: Option<Decimal> = positions
        .iter()
        .filter_map(|position| position.current_value)
        .sum::<Decimal>()
        .into();
    let daily_change_pct = portfolio_day_change_pct(&backend, &positions, total_value);

    let mut mobile_positions: Vec<MobilePosition> = positions
        .into_iter()
        .map(|position| {
            let day_change_pct = position_day_change_pct(&backend, &position);
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

    Ok(Json(MobilePortfolioResponse {
        total_value,
        daily_change_pct,
        position_count: mobile_positions.len(),
        positions: mobile_positions,
    }))
}

async fn get_analytics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MobileAnalyticsResponse>, (axum::http::StatusCode, String)> {
    let backend = crate::db::backend::open_from_config(&state.config, Path::new(&state.db_path))
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            )
        })?;

    let configured = crate::db::mobile_timeframe_scores::list_scores_backend(&backend)
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

    Ok(Json(MobileAnalyticsResponse { timeframes }))
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

fn timeframe_label(value: &str) -> &'static str {
    match value {
        "low" => "Low Timeframe",
        "medium" => "Medium Timeframe",
        "high" => "High Timeframe",
        "macro" => "Macro Timeframe",
        _ => "Timeframe",
    }
}

fn position_day_change_pct(
    backend: &crate::db::backend::BackendConnection,
    position: &Position,
) -> Option<Decimal> {
    if position.category == AssetCategory::Cash {
        return Some(dec!(0));
    }

    let history =
        crate::db::price_history::get_history_backend(backend, &position.symbol, 2).ok()?;
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

fn portfolio_day_change_pct(
    backend: &crate::db::backend::BackendConnection,
    positions: &[Position],
    total_value: Option<Decimal>,
) -> Option<Decimal> {
    let Some(current_total) = total_value else {
        return None;
    };

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
