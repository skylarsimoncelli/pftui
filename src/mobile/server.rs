use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use axum::{
    extract::{Query, State},
    middleware,
    routing::get,
    Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::Serialize;

use crate::config::Config;
use crate::mobile::auth::{auth_middleware, health, MobileAuthState};
use crate::mobile::commands::certificate_fingerprint;
use crate::web::api::{
    self, AppState, MacroResponse, PerformanceQuery, PerformanceResponse, SummaryResponse,
};

#[derive(Serialize)]
pub struct AnalyticsResponse {
    pub summary: SummaryResponse,
    pub macro_view: MacroResponse,
    pub performance: PerformanceResponse,
}

pub async fn run_server(db_path: String, config: Config) -> Result<()> {
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
        .route("/portfolio", get(api::get_portfolio))
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
    let fingerprint = certificate_fingerprint(std::path::Path::new(&cert_path))?;

    println!("📱 pftui mobile API starting...");
    println!("   Listening on https://{}", addr);
    println!("   TLS fingerprint: {}", fingerprint);
    println!("   iOS setup: enter host or host:port, API token, then verify this fingerprint");

    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

async fn get_analytics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AnalyticsResponse>, (axum::http::StatusCode, String)> {
    let summary = api::get_summary(State(state.clone())).await?.0;
    let macro_view = api::get_macro(State(state.clone())).await?.0;
    let performance = api::get_performance(
        State(state),
        Query(PerformanceQuery {
            timeframe: Some("3m".to_string()),
            benchmark: None,
        }),
    )
    .await?
    .0;

    Ok(Json(AnalyticsResponse {
        summary,
        macro_view,
        performance,
    }))
}
