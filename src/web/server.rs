use axum::{
    middleware,
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, patch, post},
    Router,
};
use chrono::Utc;
use serde::Serialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};

use super::api::{
    delete_alert, delete_journal, delete_transaction, delete_watchlist, get_alerts,
    get_asset_detail, get_chart_data, get_deltas, get_home_tab, get_journal, get_macro, get_news,
    get_performance, get_portfolio, get_positions, get_search, get_situation, get_summary,
    get_transactions, get_ui_config, get_watchlist, patch_journal, patch_transaction, post_alert,
    post_alert_ack, post_alert_rearm, post_journal, post_transaction, post_watchlist, set_home_tab,
    set_theme, AppState,
};
use super::auth::{auth_middleware, get_csrf, get_session, login, logout, AuthState};
use crate::commands;
use crate::config::Config;
use crate::data::rss::{self, NewsCategory, RssFeed};

pub async fn run_server(
    db_path: String,
    config: Config,
    bind_addr: &str,
    port: u16,
    enable_auth: bool,
) -> anyhow::Result<()> {
    let app_state = Arc::new(AppState {
        db_path: db_path.clone(),
        config: config.clone(),
    });
    let auth_state = Arc::new(AuthState::new(enable_auth, bind_addr));
    spawn_background_workers(db_path.clone(), config.clone());

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
        .route("/watchlist", post(post_watchlist))
        .route("/watchlist/{symbol}", delete(delete_watchlist))
        .route("/transactions", get(get_transactions))
        .route("/transactions", post(post_transaction))
        .route("/transactions/{id}", patch(patch_transaction))
        .route("/transactions/{id}", delete(delete_transaction))
        .route("/search", get(get_search))
        .route("/macro", get(get_macro))
        .route("/alerts", get(get_alerts))
        .route("/alerts", post(post_alert))
        .route("/alerts/{id}", delete(delete_alert))
        .route("/alerts/{id}/ack", post(post_alert_ack))
        .route("/alerts/{id}/rearm", post(post_alert_rearm))
        .route("/news", get(get_news))
        .route("/journal", get(get_journal))
        .route("/journal", post(post_journal))
        .route("/journal/{id}", patch(patch_journal))
        .route("/journal/{id}", delete(delete_journal))
        .route("/asset/{symbol}", get(get_asset_detail))
        .route("/chart/{symbol}", get(get_chart_data))
        .route("/performance", get(get_performance))
        .route("/summary", get(get_summary))
        .route("/situation", get(get_situation))
        .route("/deltas", get(get_deltas))
        .route("/ui-config", get(get_ui_config))
        .route("/stream", get(get_stream))
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

fn spawn_background_workers(db_path: String, config: Config) {
    let refresh_db_path = db_path.clone();
    let refresh_config = config.clone();
    tokio::spawn(async move {
        run_price_refresh_loop(refresh_db_path, refresh_config).await;
    });

    tokio::spawn(async move {
        run_rss_ingest_loop(db_path, config).await;
    });
}

async fn run_price_refresh_loop(db_path: String, config: Config) {
    let refresh_interval = normalize_interval(config.refresh_interval, 60);
    println!(
        "   [bg] price refresh loop enabled (every {}s)",
        refresh_interval
    );
    let mut ticker = tokio::time::interval(StdDuration::from_secs(refresh_interval));
    loop {
        ticker.tick().await;
        let db_path = db_path.clone();
        let config = config.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let backend = crate::db::backend::open_from_config(&config, Path::new(&db_path))?;
            commands::refresh::run(&backend, &config, false)?;
            Ok(())
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("   [bg] refresh failed: {}", e),
            Err(e) => eprintln!("   [bg] refresh worker join failed: {}", e),
        }
    }
}

async fn run_rss_ingest_loop(db_path: String, config: Config) {
    let feeds = configured_rss_feeds(&config);
    if feeds.is_empty() {
        eprintln!("   [bg] RSS loop disabled: no valid feeds configured");
        return;
    }

    let poll_interval = normalize_interval(config.news_poll_interval, 600);
    println!(
        "   [bg] RSS ingest loop enabled (every {}s, {} feeds)",
        poll_interval,
        feeds.len()
    );

    let mut ticker = tokio::time::interval(StdDuration::from_secs(poll_interval));
    loop {
        ticker.tick().await;
        let items = rss::fetch_all_feeds(&feeds).await;
        let item_count = items.len();
        if item_count == 0 {
            continue;
        }

        let db_path = db_path.clone();
        let config = config.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
            let backend = crate::db::backend::open_from_config(&config, Path::new(&db_path))?;
            for item in items {
                crate::db::news_cache::insert_news_backend(
                    &backend,
                    &item.title,
                    &item.url,
                    &item.source,
                    item.category.as_str(),
                    item.published_at,
                )?;
            }
            let deleted = crate::db::news_cache::cleanup_old_news_backend(&backend)?;
            Ok(deleted)
        })
        .await;

        match result {
            Ok(Ok(deleted)) => {
                if deleted > 0 {
                    println!(
                        "   [bg] RSS ingested {} item(s), cleaned {} stale item(s)",
                        item_count, deleted
                    );
                }
            }
            Ok(Err(e)) => eprintln!("   [bg] RSS ingest failed: {}", e),
            Err(e) => eprintln!("   [bg] RSS worker join failed: {}", e),
        }
    }
}

fn normalize_interval(value: u64, fallback: u64) -> u64 {
    if value == 0 {
        fallback
    } else {
        value
    }
}

fn parse_news_category(raw: &str) -> Option<NewsCategory> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "macro" => Some(NewsCategory::Macro),
        "crypto" => Some(NewsCategory::Crypto),
        "commodities" => Some(NewsCategory::Commodities),
        "geopolitics" => Some(NewsCategory::Geopolitics),
        "markets" => Some(NewsCategory::Markets),
        _ => None,
    }
}

fn configured_rss_feeds(config: &Config) -> Vec<RssFeed> {
    if config.custom_news_feeds.is_empty() {
        return rss::default_feeds();
    }

    let mut feeds = Vec::new();
    for feed in &config.custom_news_feeds {
        let Some(category) = parse_news_category(&feed.category) else {
            eprintln!(
                "   [bg] skipping feed '{}' due to unknown category '{}'",
                feed.name, feed.category
            );
            continue;
        };
        feeds.push(RssFeed {
            name: feed.name.clone(),
            url: feed.url.clone(),
            category,
        });
    }
    feeds
}

async fn serve_index() -> axum::response::Html<String> {
    axum::response::Html(include_str!("static/index.html").to_string())
}

#[derive(Serialize)]
struct StreamPayload {
    ts: String,
    message: String,
}

fn stream_event_type_and_message(i: usize) -> (&'static str, &'static str) {
    if i.is_multiple_of(6) {
        ("panel_invalidate", "refresh_visible_panels")
    } else if i.is_multiple_of(3) {
        ("quote_update", "quote_snapshot_updated")
    } else if i.is_multiple_of(2) {
        ("health", "stream_ok")
    } else {
        ("heartbeat", "alive")
    }
}

async fn get_stream() -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let ticker = tokio::time::interval(StdDuration::from_secs(5));
    let mut i: usize = 0;
    let stream = IntervalStream::new(ticker).map(move |_| {
        let tick = i;
        i += 1;
        let (event_name, message) = stream_event_type_and_message(tick);
        let payload = StreamPayload {
            ts: Utc::now().to_rfc3339(),
            message: message.to_string(),
        };
        let data = serde_json::to_string(&payload)
            .unwrap_or_else(|_| "{\"ts\":\"\",\"message\":\"serialization_error\"}".to_string());
        Ok(Event::default().event(event_name).data(data))
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(StdDuration::from_secs(10))
            .text("keepalive"),
    )
}

#[cfg(test)]
mod tests {
    use super::{configured_rss_feeds, parse_news_category, stream_event_type_and_message};
    use crate::config::{Config, CustomNewsFeed};

    #[test]
    fn stream_event_mapping_contract() {
        assert_eq!(
            stream_event_type_and_message(0),
            ("panel_invalidate", "refresh_visible_panels")
        );
        assert_eq!(
            stream_event_type_and_message(3),
            ("quote_update", "quote_snapshot_updated")
        );
        assert_eq!(stream_event_type_and_message(2), ("health", "stream_ok"));
        assert_eq!(stream_event_type_and_message(1), ("heartbeat", "alive"));
    }

    #[test]
    fn parse_news_category_contract() {
        assert!(parse_news_category("markets").is_some());
        assert!(parse_news_category("Crypto").is_some());
        assert!(parse_news_category("unknown").is_none());
    }

    #[test]
    fn configured_rss_feeds_uses_defaults_when_empty() {
        let config = Config::default();
        let feeds = configured_rss_feeds(&config);
        assert!(!feeds.is_empty());
    }

    #[test]
    fn configured_rss_feeds_filters_invalid_custom_categories() {
        let config = Config {
            custom_news_feeds: vec![
                CustomNewsFeed {
                    name: "Good".to_string(),
                    url: "https://example.com/rss.xml".to_string(),
                    category: "markets".to_string(),
                },
                CustomNewsFeed {
                    name: "Bad".to_string(),
                    url: "https://example.com/bad.xml".to_string(),
                    category: "invalid".to_string(),
                },
            ],
            ..Default::default()
        };
        let feeds = configured_rss_feeds(&config);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].name, "Good");
    }
}
