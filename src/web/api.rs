use axum::{
    extract::Path,
    extract::Query,
    extract::State,
    http::StatusCode,
    response::Json,
};
use chrono::{Duration, NaiveDate, Utc};
use rusqlite::Connection;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::db;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::tui::theme::{self, THEME_NAMES};
use ratatui::style::Color;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// Helper function to convert cached prices to HashMap
fn get_price_map(conn: &Connection) -> anyhow::Result<HashMap<String, Decimal>> {
    let cached = db::price_cache::get_all_cached_prices(conn)?;
    Ok(cached.into_iter().map(|q| (q.symbol, q.price)).collect())
}

pub struct AppState {
    pub db_path: String,
    pub config: Config,
}

impl AppState {
    fn get_conn(&self) -> Result<Connection, rusqlite::Error> {
        use std::path::Path;
        db::open_db(Path::new(&self.db_path)).map_err(|e| {
            rusqlite::Error::InvalidParameterName(format!("{}", e))
        })
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
}

#[derive(Serialize)]
pub struct PositionsResponse {
    pub positions: Vec<Position>,
}

#[derive(Serialize)]
pub struct WatchlistResponse {
    pub symbols: Vec<WatchlistItem>,
}

#[derive(Serialize)]
pub struct WatchlistItem {
    pub symbol: String,
    pub name: String,
    pub category: AssetCategory,
    pub current_price: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub target_price: Option<Decimal>,
    pub target_direction: Option<String>,
}

#[derive(Serialize)]
pub struct TransactionsResponse {
    pub transactions: Vec<crate::models::transaction::Transaction>,
}

#[derive(Serialize)]
pub struct MacroResponse {
    pub indicators: Vec<MacroIndicator>,
}

#[derive(Serialize)]
pub struct MacroIndicator {
    pub symbol: String,
    pub name: String,
    pub value: Option<Decimal>,
    pub change_pct: Option<Decimal>,
}

#[derive(Serialize)]
pub struct AlertsResponse {
    pub alerts: Vec<AlertItem>,
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

#[derive(Serialize)]
pub struct ChartDataResponse {
    pub symbol: String,
    pub history: Vec<ChartPoint>,
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
}

#[derive(Serialize)]
pub struct UiConfigResponse {
    pub tabs: Vec<&'static str>,
    pub themes: Vec<WebTheme>,
    pub current_theme: String,
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
}

// Handlers

pub async fn get_portfolio(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PortfolioResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        let prices = get_price_map(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices)
    } else {
        let transactions = db::transactions::list_transactions(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        let prices = get_price_map(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices)
    };

    let total_value: Option<Decimal> = positions.iter().filter_map(|p| p.current_value).sum::<Decimal>().into();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value.map(|v| v - total_cost);
    let total_gain_pct = if total_cost > dec!(0) {
        total_gain.map(|g| (g / total_cost) * dec!(100))
    } else {
        None
    };

    // TODO: Compute daily change from price history
    let daily_change = None;
    let daily_change_pct = None;

    Ok(Json(PortfolioResponse {
        total_value,
        total_cost,
        total_gain,
        total_gain_pct,
        daily_change,
        daily_change_pct,
        positions,
    }))
}

pub async fn get_positions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PositionsResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        let prices = get_price_map(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices)
    } else {
        let transactions = db::transactions::list_transactions(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        let prices = get_price_map(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices)
    };

    Ok(Json(PositionsResponse { positions }))
}

pub async fn get_watchlist(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WatchlistResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let watchlist = db::watchlist::list_watchlist(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load watchlist: {}", e),
        )
    })?;

    let prices = get_price_map(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load prices: {}", e),
        )
    })?;

    let items: Vec<WatchlistItem> = watchlist
        .into_iter()
        .map(|w| {
            let current_price = prices.get(&w.symbol).copied();
            let category: AssetCategory = w.category.parse().unwrap_or(AssetCategory::Equity);
            let target_price = w.target_price.and_then(|t| t.parse::<Decimal>().ok());
            WatchlistItem {
                symbol: w.symbol.clone(),
                name: crate::models::asset_names::resolve_name(&w.symbol),
                category,
                current_price,
                day_change_pct: None, // TODO: compute from history
                target_price,
                target_direction: w.target_direction,
            }
        })
        .collect();

    Ok(Json(WatchlistResponse { symbols: items }))
}

pub async fn get_transactions(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TransactionsResponse>, (StatusCode, String)> {
    if state.config.is_percentage_mode() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Transactions not available in percentage mode".to_string(),
        ));
    }

    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let transactions = db::transactions::list_transactions(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load transactions: {}", e),
        )
    })?;

    Ok(Json(TransactionsResponse { transactions }))
}

pub async fn get_macro(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MacroResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let prices = get_price_map(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load prices: {}", e),
        )
    })?;

    let macro_symbols = vec![
        ("^GSPC", "S&P 500"),
        ("^IXIC", "Nasdaq"),
        ("^VIX", "VIX"),
        ("GC=F", "Gold"),
        ("SI=F", "Silver"),
        ("BTC", "Bitcoin"),
        ("DX-Y.NYB", "US Dollar Index"),
        ("^TNX", "10Y Treasury"),
    ];

    let indicators: Vec<MacroIndicator> = macro_symbols
        .into_iter()
        .map(|(symbol, name)| MacroIndicator {
            symbol: symbol.to_string(),
            name: name.to_string(),
            value: prices.get(symbol).copied(),
            change_pct: None, // TODO: compute from history
        })
        .collect();

    Ok(Json(MacroResponse { indicators }))
}

pub async fn get_alerts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AlertsResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let alerts_data = db::alerts::list_alerts(&conn).map_err(|e| {
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

    Ok(Json(AlertsResponse { alerts }))
}

pub async fn get_chart_data(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> Result<Json<ChartDataResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let history = db::price_history::get_history(&conn, &symbol, 365).map_err(|e| {
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
    }))
}

pub async fn get_performance(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PerformanceQuery>,
) -> Result<Json<PerformanceResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let mut snapshots = db::snapshots::get_all_portfolio_snapshots(&conn).map_err(|e| {
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

    if !snapshots.is_empty() {
        let cutoff = Utc::now().date_naive() - Duration::days(days);
        snapshots.retain(|s| {
            NaiveDate::parse_from_str(&s.date, "%Y-%m-%d")
                .map(|d| d >= cutoff)
                .unwrap_or(true)
        });
    }

    let daily_values: Vec<PortfolioValuePoint> = snapshots
        .iter()
        .map(|s| PortfolioValuePoint {
            date: s.date.clone(),
            value: s.total_value,
        })
        .collect();

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

    Ok(Json(PerformanceResponse {
        daily_values,
        metrics: PerformanceMetrics {
            total_return_pct,
            max_drawdown_pct,
        },
    }))
}

pub async fn get_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SummaryResponse>, (StatusCode, String)> {
    let conn = state.get_conn().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
    })?;

    let positions = if state.config.is_percentage_mode() {
        let allocations = db::allocations::list_allocations(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load allocations: {}", e),
            )
        })?;
        let prices = get_price_map(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions_from_allocations(&allocations, &prices)
    } else {
        let transactions = db::transactions::list_transactions(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load transactions: {}", e),
            )
        })?;
        let prices = get_price_map(&conn).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load prices: {}", e),
            )
        })?;
        compute_positions(&transactions, &prices)
    };

    let total_value: Option<Decimal> = positions.iter().filter_map(|p| p.current_value).sum::<Decimal>().into();
    
    // Get top 5 movers by absolute gain_pct
    let mut movers = positions.clone();
    movers.sort_by(|a, b| {
        let a_abs = a.gain_pct.unwrap_or(dec!(0)).abs();
        let b_abs = b.gain_pct.unwrap_or(dec!(0)).abs();
        b_abs.partial_cmp(&a_abs).unwrap_or(std::cmp::Ordering::Equal)
    });
    movers.truncate(5);

    Ok(Json(SummaryResponse {
        total_value,
        position_count: positions.len(),
        top_movers: movers,
    }))
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
    let tabs = vec![
        "Positions",
        "Transactions",
        "Markets",
        "Economy",
        "Watchlist",
        "News",
        "Journal",
    ];
    let themes = THEME_NAMES.iter().map(|n| web_theme(n)).collect();
    Ok(Json(UiConfigResponse {
        tabs,
        themes,
        current_theme: state.config.theme.clone(),
    }))
}
