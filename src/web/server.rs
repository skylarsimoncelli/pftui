use axum::{
    extract::State,
    middleware,
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use super::api::{
    get_alerts, get_chart_data, get_macro, get_performance, get_portfolio, get_positions,
    get_summary, get_transactions, get_ui_config, get_watchlist, AppState,
};
use super::auth::{auth_middleware, AuthState};
use crate::config::Config;

pub async fn run_server(
    db_path: String,
    config: Config,
    bind_addr: &str,
    port: u16,
    enable_auth: bool,
) -> anyhow::Result<()> {
    let app_state = Arc::new(AppState { db_path, config });
    let auth_state = Arc::new(AuthState::new(enable_auth));

    // CORS configuration for local development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // API routes
    let api_routes = Router::new()
        .route("/portfolio", get(get_portfolio))
        .route("/positions", get(get_positions))
        .route("/watchlist", get(get_watchlist))
        .route("/transactions", get(get_transactions))
        .route("/macro", get(get_macro))
        .route("/alerts", get(get_alerts))
        .route("/chart/{symbol}", get(get_chart_data))
        .route("/performance", get(get_performance))
        .route("/summary", get(get_summary))
        .route("/ui-config", get(get_ui_config))
        .with_state(app_state.clone());

    // Main app with auth middleware
    let app = Router::new()
        .route("/", get(serve_index))
        .nest("/api", api_routes)
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        .layer(cors)
        .with_state(auth_state);

    let addr: SocketAddr = format!("{}:{}", bind_addr, port).parse()?;
    
    println!("🚀 pftui web dashboard starting...");
    println!("   Listening on http://{}", addr);
    println!("   Dashboard: http://{}:{}", bind_addr, port);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_index(
    State(state): State<Arc<AuthState>>,
) -> axum::response::Html<String> {
    // Inject auth token so the in-browser app can call protected /api endpoints.
    // This keeps auth enabled by default while making `pftui web` usable out of the box.
    let token = state.token.as_deref().unwrap_or("");
    let html = include_str!("static/index.html")
        .replace("__PFTUI_AUTH_TOKEN__", token);
    axum::response::Html(html)
}
