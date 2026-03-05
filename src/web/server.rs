use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use super::api::{
    get_alerts, get_chart_data, get_home_tab, get_journal, get_macro, get_news, get_performance,
    get_portfolio, get_positions, get_summary, get_transactions, get_ui_config, get_watchlist,
    set_home_tab, set_theme, AppState,
};
use super::auth::{auth_middleware, get_csrf, get_session, login, logout, AuthState};
use crate::config::Config;

pub async fn run_server(
    db_path: String,
    config: Config,
    bind_addr: &str,
    port: u16,
    enable_auth: bool,
) -> anyhow::Result<()> {
    let app_state = Arc::new(AppState { db_path, config });
    let auth_state = Arc::new(AuthState::new(enable_auth, bind_addr));

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
        .route("/news", get(get_news))
        .route("/journal", get(get_journal))
        .route("/chart/{symbol}", get(get_chart_data))
        .route("/performance", get(get_performance))
        .route("/summary", get(get_summary))
        .route("/ui-config", get(get_ui_config))
        .route("/home-tab", get(get_home_tab))
        .route("/home-tab", post(set_home_tab))
        .route("/theme", post(set_theme))
        .with_state(app_state.clone());

    // Main app with auth middleware
    let auth_routes = Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/session", get(get_session))
        .route("/csrf", get(get_csrf))
        .with_state(auth_state.clone());

    let app = Router::new()
        .route("/", get(serve_index))
        .nest("/auth", auth_routes)
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

async fn serve_index() -> axum::response::Html<String> {
    axum::response::Html(include_str!("static/index.html").to_string())
}
