use axum::{
    extract::Path,
    extract::State,
    http::StatusCode,
    response::Json,
};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::db;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
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
    State(_state): State<Arc<AppState>>,
) -> Result<Json<PerformanceResponse>, (StatusCode, String)> {
    // TODO: Implement portfolio value history computation
    Ok(Json(PerformanceResponse {
        daily_values: vec![],
        metrics: PerformanceMetrics {
            total_return_pct: None,
            max_drawdown_pct: None,
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
