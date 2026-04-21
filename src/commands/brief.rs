use std::collections::{HashMap, HashSet};

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::analytics::levels::{nearest_actionable_levels, ActionableLevelPair};
use crate::analytics::risk;
use crate::analytics::technicals::{
    load_or_compute_snapshots, load_or_compute_snapshots_backend, DEFAULT_TIMEFRAME,
};
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::backend::BackendConnection;
use crate::db::economic_cache;
use crate::db::price_cache::{get_all_cached_prices, get_cached_price};
use crate::db::price_history::{get_history, get_prices_at_date};
use crate::db::snapshots::get_all_portfolio_snapshots;
use crate::db::technical_levels;
use crate::db::technical_snapshots::TechnicalSnapshotRecord;
use crate::db::transactions::list_transactions;
use crate::indicators::correlation::compute_rolling_correlation;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

use crate::commands::correlations::{CorrelationBreak as CorrelationsBreak, interpret_break};
use crate::commands::scan::{compute_scan_highlights, compute_scan_highlights_sqlite, ScanHighlight};
use crate::db::scenario_contract_mappings;
use crate::db::scenarios::Scenario;

// ==================== Agent JSON Structures ====================

#[derive(Serialize)]
struct AgentBrief {
    timestamp: String,
    portfolio: PortfolioSummaryJson,
    positions: Vec<PositionJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    watchlist: Vec<WatchlistItemJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    movers: Vec<MoverJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    market_movers: Vec<MoverJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    macro_data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    news_summary: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    economic_data: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    predictions: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sentiment: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    alerts: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    drift: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    regime: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlations: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeframe_signal: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    technical_signals: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    overnight_changes: Vec<OvernightChangeJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scan_highlights: Vec<ScanHighlight>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scenarios: Vec<ScenarioSummaryJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<CalibrationSummaryJson>,
}

/// Compact scenario summary for the brief payload.
#[derive(Debug, Clone, Serialize)]
struct ScenarioSummaryJson {
    name: String,
    probability: f64,
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    updated_at: String,
}

/// Compact prediction-market calibration summary for the brief payload.
#[derive(Debug, Clone, Serialize)]
struct CalibrationSummaryJson {
    total_mappings: usize,
    significant_divergences: usize,
    mean_abs_divergence_pp: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entries: Vec<CalibrationEntryJson>,
}

/// One scenario ↔ contract calibration entry.
#[derive(Debug, Clone, Serialize)]
struct CalibrationEntryJson {
    scenario_name: String,
    scenario_pct: f64,
    market_pct: f64,
    divergence_pp: f64,
    significant: bool,
}

#[derive(Serialize)]
struct PortfolioSummaryJson {
    total_value: String,
    total_cost: String,
    total_gain: String,
    total_gain_pct: String,
    daily_pnl: Option<String>,
    daily_pnl_pct: Option<String>,
    base_currency: String,
}

#[derive(Serialize)]
struct PositionJson {
    symbol: String,
    name: String,
    category: String,
    quantity: String,
    current_price: Option<String>,
    total_cost: String,
    current_value: Option<String>,
    unrealized_gain: Option<String>,
    unrealized_gain_pct: Option<String>,
    daily_change: Option<String>,
    daily_change_pct: Option<String>,
    allocation_pct: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    technicals: Option<TechnicalSnapshotJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    levels: Option<ActionableLevelPair>,
}

#[derive(Serialize)]
struct TechnicalSnapshotJson {
    rsi: Option<String>,
    rsi_signal: Option<String>,
    macd: Option<String>,
    macd_signal: Option<String>,
    macd_histogram: Option<String>,
    sma_20: Option<String>,
    sma_50: Option<String>,
}

#[derive(Serialize)]
struct WatchlistItemJson {
    symbol: String,
    name: String,
    category: String,
    current_price: Option<String>,
    daily_change_pct: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    technicals: Option<TechnicalSnapshotJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    levels: Option<ActionableLevelPair>,
}

#[derive(Serialize)]
struct MoverJson {
    symbol: String,
    name: String,
    daily_change_pct: String,
    daily_change_abs: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    signals: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
struct OvernightChangeJson {
    symbol: String,
    name: String,
    category: String,
    previous_close: String,
    current_price: String,
    change_abs: String,
    change_pct: String,
    /// "held" for portfolio positions, "watchlist" for watchlist items
    source: String,
}

const DEFAULT_MARKET_MOVER_SYMBOLS: &[&str] = &[
    "NVDA", "TSLA", "AAPL", "MSFT", "AMZN", "META", "GOOG", "SPY", "QQQ", "XLE", "XOP", "CL=F",
    "GC=F", "SI=F", "HG=F",
];

#[derive(Debug, Clone)]
struct CorrelationPair {
    symbol_a: String,
    symbol_b: String,
    corr_30d: f64,
}

#[derive(Debug, Clone)]
struct CorrelationBreak {
    symbol_a: String,
    symbol_b: String,
    corr_7d: f64,
    corr_90d: f64,
    delta: f64,
}

#[derive(Debug, Clone, Default)]
struct CorrelationSummary {
    top_pairs: Vec<CorrelationPair>,
    active_breaks: Vec<CorrelationBreak>,
}

/// Build overnight change entries for held positions and watchlist items.
/// Shows previous close → current price with absolute and percentage change,
/// sorted by absolute change percentage descending (biggest movers first).
fn build_overnight_changes(
    positions: &[Position],
    watchlist_symbols: &[String],
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
) -> Vec<OvernightChangeJson> {
    let mut changes = Vec::new();

    // Held positions (skip cash)
    for pos in positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        if let (Some(current), Some(&prev)) = (pos.current_price, hist_1d.get(&pos.symbol)) {
            if prev > dec!(0) {
                let change_abs = current - prev;
                let change_pct = ((current - prev) / prev) * dec!(100);
                changes.push(OvernightChangeJson {
                    symbol: pos.symbol.clone(),
                    name: resolve_name(&pos.symbol),
                    category: format!("{:?}", pos.category),
                    previous_close: prev.to_string(),
                    current_price: current.to_string(),
                    change_abs: change_abs.round_dp(4).to_string(),
                    change_pct: change_pct.round_dp(2).to_string(),
                    source: "held".to_string(),
                });
            }
        }
    }

    // Watchlist items (only those not already in positions)
    let held_symbols: std::collections::HashSet<&str> =
        positions.iter().map(|p| p.symbol.as_str()).collect();
    for sym in watchlist_symbols {
        if held_symbols.contains(sym.as_str()) {
            continue;
        }
        if let (Some(&current), Some(&prev)) = (prices.get(sym), hist_1d.get(sym)) {
            if prev > dec!(0) {
                let change_abs = current - prev;
                let change_pct = ((current - prev) / prev) * dec!(100);
                changes.push(OvernightChangeJson {
                    symbol: sym.clone(),
                    name: resolve_name(sym),
                    category: format!(
                        "{:?}",
                        crate::models::asset_names::infer_category(sym)
                    ),
                    previous_close: prev.to_string(),
                    current_price: current.to_string(),
                    change_abs: change_abs.round_dp(4).to_string(),
                    change_pct: change_pct.round_dp(2).to_string(),
                    source: "watchlist".to_string(),
                });
            }
        }
    }

    // Sort by absolute change percentage descending (biggest movers first)
    changes.sort_by(|a, b| {
        let a_pct = a
            .change_pct
            .parse::<f64>()
            .unwrap_or(0.0)
            .abs();
        let b_pct = b
            .change_pct
            .parse::<f64>()
            .unwrap_or(0.0)
            .abs();
        b_pct.partial_cmp(&a_pct).unwrap_or(std::cmp::Ordering::Equal)
    });

    changes
}

fn split_cached_quotes(
    cached: Vec<crate::models::price::PriceQuote>,
) -> (HashMap<String, Decimal>, HashMap<String, Decimal>) {
    let mut prices = HashMap::new();
    let mut previous_close = HashMap::new();

    for quote in cached {
        if let Some(prev) = quote.previous_close {
            previous_close.insert(quote.symbol.clone(), prev);
        }
        prices.insert(quote.symbol, quote.price);
    }

    (prices, previous_close)
}

fn enrich_hist_1d_with_cached_previous_close(
    hist_1d: &HashMap<String, Decimal>,
    previous_close: &HashMap<String, Decimal>,
) -> HashMap<String, Decimal> {
    let mut merged = hist_1d.clone();
    for (symbol, prev_close) in previous_close {
        if *prev_close > dec!(0) {
            merged.entry(symbol.clone()).or_insert(*prev_close);
        }
    }
    merged
}

/// Agent mode: single comprehensive JSON blob
fn run_agent_mode(conn: &Connection, config: &Config) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let (prices, previous_close) = split_cached_quotes(cached);

    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = enrich_hist_1d_with_cached_previous_close(
        &get_prices_at_date(conn, &symbols, &yesterday_str).unwrap_or_default(),
        &previous_close,
    );

    // Load technicals for all symbols
    let technicals_data = compute_technicals_for_symbols(conn, &symbols);

    let base = &config.base_currency;
    let timestamp = Utc::now().to_rfc3339();

    let fx_rates = crate::db::fx_cache::get_all_fx_rates(conn).unwrap_or_default();

    // Compute positions
    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions(conn)?;
            compute_positions(&txs, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations(conn)?;
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };

    // Portfolio summary
    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value - total_cost;
    let total_gain_pct = pct_change(total_value, total_cost).unwrap_or(dec!(0));

    let mut daily_pnl = dec!(0);
    let mut has_daily = false;
    for pos in &positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        if let (Some(current), Some(&prev)) = (pos.current_price, hist_1d.get(&pos.symbol)) {
            if prev > dec!(0) {
                daily_pnl += (current - prev) * pos.quantity;
                has_daily = true;
            }
        }
    }

    let daily_pnl_pct = if has_daily && total_value > dec!(0) {
        Some((daily_pnl / (total_value - daily_pnl)) * dec!(100))
    } else {
        None
    };

    let portfolio_summary = PortfolioSummaryJson {
        total_value: total_value.to_string(),
        total_cost: total_cost.to_string(),
        total_gain: total_gain.to_string(),
        total_gain_pct: total_gain_pct.round_dp(2).to_string(),
        daily_pnl: if has_daily {
            Some(daily_pnl.to_string())
        } else {
            None
        },
        daily_pnl_pct: daily_pnl_pct.map(|p| p.round_dp(2).to_string()),
        base_currency: base.to_string(),
    };

    // Positions with technicals
    let positions_json: Vec<PositionJson> = positions
        .iter()
        .map(|pos| {
            let daily_change = if let (Some(current), Some(&prev)) =
                (pos.current_price, hist_1d.get(&pos.symbol))
            {
                if prev > dec!(0) {
                    Some((current - prev) * pos.quantity)
                } else {
                    None
                }
            } else {
                None
            };

            let daily_change_pct = if let (Some(current), Some(&prev)) =
                (pos.current_price, hist_1d.get(&pos.symbol))
            {
                pct_change(current, prev)
            } else {
                None
            };

            let allocation_pct = if total_value > dec!(0) {
                pos.current_value
                    .map(|v| ((v / total_value) * dec!(100)).round_dp(2))
            } else {
                None
            };

            let technicals_json = technicals_data
                .get(&pos.symbol)
                .map(|t| TechnicalSnapshotJson {
                    rsi: t.rsi_14.map(|v| format!("{:.1}", v)),
                    rsi_signal: t.rsi_14.map(|v| {
                        if v > 70.0 {
                            "overbought".to_string()
                        } else if v < 30.0 {
                            "oversold".to_string()
                        } else {
                            "neutral".to_string()
                        }
                    }),
                    macd: t.macd.map(|v| format!("{:.4}", v)),
                    macd_signal: t.macd_signal.map(|v| format!("{:.4}", v)),
                    macd_histogram: t.macd_histogram.map(|v| format!("{:.4}", v)),
                    sma_20: None, // Not available in current TechnicalSnapshot
                    sma_50: t.sma_50.map(|v| format!("{:.2}", v)),
                });
            let levels_json = pos
                .current_price
                .and_then(|price| get_nearest_levels_json(conn, &pos.symbol, price));

            PositionJson {
                symbol: pos.symbol.clone(),
                name: resolve_name(&pos.symbol),
                category: format!("{:?}", pos.category),
                quantity: pos.quantity.to_string(),
                current_price: pos.current_price.map(|p| p.to_string()),
                total_cost: pos.total_cost.to_string(),
                current_value: pos.current_value.map(|v| v.to_string()),
                unrealized_gain: pos.gain.map(|g| g.to_string()),
                unrealized_gain_pct: pos.gain_pct.map(|p| p.round_dp(2).to_string()),
                daily_change: daily_change.map(|c| c.to_string()),
                daily_change_pct: daily_change_pct.map(|p| p.round_dp(2).to_string()),
                allocation_pct: allocation_pct.map(|a| a.to_string()),
                technicals: technicals_json,
                levels: levels_json,
            }
        })
        .collect();

    // Watchlist
    let watchlist_json = get_watchlist_json(conn, &prices, &hist_1d, &technicals_data)?;

    // Technical signals for mover context and brief summary
    let recent_signals =
        crate::db::technical_signals::list_signals(conn, None, None, Some(100)).unwrap_or_default();
    let signal_map = build_signal_map(&recent_signals);
    let technical_signals_json = signals_to_json(&recent_signals);

    // Top movers (held positions)
    let movers_json = get_movers_json(&positions, &hist_1d, &signal_map);
    let watchlist_symbols: Vec<String> = watchlist_json.iter().map(|w| w.symbol.clone()).collect();
    let market_movers_json = get_market_movers_json(
        &positions,
        &watchlist_symbols,
        &prices,
        &hist_1d,
        &signal_map,
    );

    // Macro data (if available)
    let macro_data = get_macro_json(conn).ok();

    // Alerts (if available)
    let alerts_json = get_alerts_json(conn);

    // Drift (if available)
    let drift_json = get_drift_json(conn).ok();

    // Regime (if available)
    let regime_json = get_regime_json(conn).ok();
    let corr_summary = compute_correlation_summary(conn, &positions);
    let news_summary = get_news_summary_json(conn).unwrap_or_default();
    let economic_data = get_economic_data_json(conn).unwrap_or_default();
    let predictions = get_predictions_json(conn).unwrap_or_default();
    let sentiment = get_sentiment_json(conn).ok();
    let correlations = correlation_summary_to_json(&corr_summary);
    let timeframe_signal = get_top_timeframe_signal_json(conn).ok();

    // Overnight changes: previous close → current for all held + watchlist
    let overnight_changes =
        build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);

    // Scan highlights: big movers, trackline breaches, divergent gainers
    let scan_highlights = compute_scan_highlights_sqlite(conn).unwrap_or_default();

    // Scenarios: active macro scenarios with probabilities
    let scenarios = build_scenarios_json_sqlite(conn);

    // Calibration: scenario vs prediction market divergences
    let calibration = build_calibration_json_sqlite(conn);

    let brief = AgentBrief {
        timestamp,
        portfolio: portfolio_summary,
        positions: positions_json,
        watchlist: watchlist_json,
        movers: movers_json,
        market_movers: market_movers_json,
        macro_data,
        news_summary,
        economic_data,
        predictions,
        sentiment,
        alerts: alerts_json,
        drift: drift_json,
        regime: regime_json,
        correlations,
        timeframe_signal,
        technical_signals: technical_signals_json,
        overnight_changes,
        scan_highlights,
        scenarios,
        calibration,
    };

    let json = serde_json::to_string_pretty(&brief)?;
    println!("{}", json);
    Ok(())
}

fn run_agent_mode_backend(backend: &BackendConnection, config: &Config) -> Result<()> {
    let cached = crate::db::price_cache::get_all_cached_prices_backend(backend)?;
    let (prices, previous_close) = split_cached_quotes(cached);

    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = enrich_hist_1d_with_cached_previous_close(
        &crate::db::price_history::get_prices_at_date_backend(backend, &symbols, &yesterday_str)
            .unwrap_or_default(),
        &previous_close,
    );
    let technicals_data = compute_technicals_for_symbols_backend(backend, &symbols);

    let base = &config.base_currency;
    let timestamp = Utc::now().to_rfc3339();
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = crate::db::transactions::list_transactions_backend(backend)?;
            compute_positions(&txs, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocs = crate::db::allocations::list_allocations_backend(backend)?;
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value - total_cost;
    let total_gain_pct = pct_change(total_value, total_cost).unwrap_or(dec!(0));

    let mut daily_pnl = dec!(0);
    let mut has_daily = false;
    for pos in &positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        if let (Some(current), Some(&prev)) = (pos.current_price, hist_1d.get(&pos.symbol)) {
            if prev > dec!(0) {
                daily_pnl += (current - prev) * pos.quantity;
                has_daily = true;
            }
        }
    }

    let daily_pnl_pct = if has_daily && total_value > dec!(0) {
        Some((daily_pnl / (total_value - daily_pnl)) * dec!(100))
    } else {
        None
    };

    let portfolio_summary = PortfolioSummaryJson {
        total_value: total_value.to_string(),
        total_cost: total_cost.to_string(),
        total_gain: total_gain.to_string(),
        total_gain_pct: total_gain_pct.round_dp(2).to_string(),
        daily_pnl: if has_daily {
            Some(daily_pnl.to_string())
        } else {
            None
        },
        daily_pnl_pct: daily_pnl_pct.map(|p| p.round_dp(2).to_string()),
        base_currency: base.to_string(),
    };

    let positions_json: Vec<PositionJson> = positions
        .iter()
        .map(|pos| {
            let daily_change = if let (Some(current), Some(&prev)) =
                (pos.current_price, hist_1d.get(&pos.symbol))
            {
                if prev > dec!(0) {
                    Some((current - prev) * pos.quantity)
                } else {
                    None
                }
            } else {
                None
            };
            let daily_change_pct = if let (Some(current), Some(&prev)) =
                (pos.current_price, hist_1d.get(&pos.symbol))
            {
                pct_change(current, prev)
            } else {
                None
            };
            let allocation_pct = if total_value > dec!(0) {
                pos.current_value
                    .map(|v| ((v / total_value) * dec!(100)).round_dp(2))
            } else {
                None
            };
            let technicals_json = technicals_data
                .get(&pos.symbol)
                .map(|t| TechnicalSnapshotJson {
                    rsi: t.rsi_14.map(|v| format!("{:.1}", v)),
                    rsi_signal: t.rsi_14.map(|v| {
                        if v > 70.0 {
                            "overbought".to_string()
                        } else if v < 30.0 {
                            "oversold".to_string()
                        } else {
                            "neutral".to_string()
                        }
                    }),
                    macd: t.macd.map(|v| format!("{:.4}", v)),
                    macd_signal: t.macd_signal.map(|v| format!("{:.4}", v)),
                    macd_histogram: t.macd_histogram.map(|v| format!("{:.4}", v)),
                    sma_20: None,
                    sma_50: t.sma_50.map(|v| format!("{:.2}", v)),
                });
            let levels_json = pos
                .current_price
                .and_then(|price| get_nearest_levels_json_backend(backend, &pos.symbol, price));

            PositionJson {
                symbol: pos.symbol.clone(),
                name: resolve_name(&pos.symbol),
                category: format!("{:?}", pos.category),
                quantity: pos.quantity.to_string(),
                current_price: pos.current_price.map(|p| p.to_string()),
                total_cost: pos.total_cost.to_string(),
                current_value: pos.current_value.map(|v| v.to_string()),
                unrealized_gain: pos.gain.map(|g| g.to_string()),
                unrealized_gain_pct: pos.gain_pct.map(|p| p.round_dp(2).to_string()),
                daily_change: daily_change.map(|c| c.to_string()),
                daily_change_pct: daily_change_pct.map(|p| p.round_dp(2).to_string()),
                allocation_pct: allocation_pct.map(|a| a.to_string()),
                technicals: technicals_json,
                levels: levels_json,
            }
        })
        .collect();

    let watchlist_json = get_watchlist_json_backend(backend, &prices, &hist_1d, &technicals_data)?;

    // Technical signals for mover context and brief summary
    let recent_signals =
        crate::db::technical_signals::list_signals_backend(backend, None, None, Some(100))
            .unwrap_or_default();
    let signal_map = build_signal_map(&recent_signals);
    let technical_signals_json = signals_to_json(&recent_signals);

    let movers_json = get_movers_json(&positions, &hist_1d, &signal_map);
    let watchlist_symbols: Vec<String> = watchlist_json.iter().map(|w| w.symbol.clone()).collect();
    let market_movers_json = get_market_movers_json(
        &positions,
        &watchlist_symbols,
        &prices,
        &hist_1d,
        &signal_map,
    );
    let macro_data = get_macro_json_backend(backend).ok();
    let alerts_json = get_alerts_json_backend(backend);
    let drift_json = get_drift_json_backend(backend).ok();
    let regime_json = get_regime_json_backend(backend).ok();
    let corr_summary = compute_correlation_summary_backend(backend, &positions);
    let news_summary = get_news_summary_json_backend(backend).unwrap_or_default();
    let economic_data = get_economic_data_json_backend(backend).unwrap_or_default();
    let predictions = get_predictions_json_backend(backend).unwrap_or_default();
    let sentiment = get_sentiment_json_backend(backend).ok();
    let correlations = correlation_summary_to_json(&corr_summary);
    let timeframe_signal = get_top_timeframe_signal_json_backend(backend).ok();

    // Overnight changes: previous close → current for all held + watchlist
    let overnight_changes =
        build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);

    // Scan highlights: big movers, trackline breaches, divergent gainers
    let scan_highlights = compute_scan_highlights(backend).unwrap_or_default();

    // Scenarios: active macro scenarios with probabilities
    let scenarios = build_scenarios_json_backend(backend);

    // Calibration: scenario vs prediction market divergences
    let calibration = build_calibration_json_backend(backend);

    let brief = AgentBrief {
        timestamp,
        portfolio: portfolio_summary,
        positions: positions_json,
        watchlist: watchlist_json,
        movers: movers_json,
        market_movers: market_movers_json,
        macro_data,
        news_summary,
        economic_data,
        predictions,
        sentiment,
        alerts: alerts_json,
        drift: drift_json,
        regime: regime_json,
        correlations,
        timeframe_signal,
        technical_signals: technical_signals_json,
        overnight_changes,
        scan_highlights,
        scenarios,
        calibration,
    };
    println!("{}", serde_json::to_string_pretty(&brief)?);
    Ok(())
}

fn get_watchlist_json_backend(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<Vec<WatchlistItemJson>> {
    let watchlist = crate::db::watchlist::list_watchlist_backend(backend)?;
    Ok(watchlist
        .iter()
        .map(|w| {
            let current_price = prices.get(&w.symbol).copied();
            let daily_change_pct =
                if let (Some(current), Some(&prev)) = (current_price, hist_1d.get(&w.symbol)) {
                    pct_change(current, prev)
                } else {
                    None
                };
            let technicals_json = technicals_data
                .get(&w.symbol)
                .map(|t| TechnicalSnapshotJson {
                    rsi: t.rsi_14.map(|v| format!("{:.1}", v)),
                    rsi_signal: t.rsi_14.map(|v| {
                        if v > 70.0 {
                            "overbought".to_string()
                        } else if v < 30.0 {
                            "oversold".to_string()
                        } else {
                            "neutral".to_string()
                        }
                    }),
                    macd: t.macd.map(|v| format!("{:.4}", v)),
                    macd_signal: t.macd_signal.map(|v| format!("{:.4}", v)),
                    macd_histogram: t.macd_histogram.map(|v| format!("{:.4}", v)),
                    sma_20: None,
                    sma_50: t.sma_50.map(|v| format!("{:.2}", v)),
                });
            let levels = current_price
                .and_then(|p| get_nearest_levels_json_backend(backend, &w.symbol, p));
            WatchlistItemJson {
                symbol: w.symbol.clone(),
                name: resolve_name(&w.symbol),
                category: w.category.clone(),
                current_price: current_price.map(|p| p.to_string()),
                daily_change_pct: daily_change_pct.map(|p| p.round_dp(2).to_string()),
                technicals: technicals_json,
                levels,
            }
        })
        .collect())
}

fn get_macro_json_backend(backend: &BackendConnection) -> Result<serde_json::Value> {
    let mut macro_map = serde_json::Map::new();
    let macro_symbols = vec![
        ("DX-Y.NYB", "Dollar Index"),
        ("^VIX", "VIX"),
        ("^TNX", "10Y Treasury"),
        ("CL=F", "Crude Oil"),
        ("GC=F", "Gold"),
        ("SI=F", "Silver"),
        ("HG=F", "Copper"),
    ];
    for (symbol, name) in macro_symbols {
        if let Ok(Some(quote)) =
            crate::db::price_cache::get_cached_price_backend(backend, symbol, "USD")
        {
            let mut item = serde_json::Map::new();
            item.insert(
                "name".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            item.insert(
                "price".to_string(),
                serde_json::Value::String(quote.price.to_string()),
            );
            item.insert(
                "fetched_at".to_string(),
                serde_json::Value::String(quote.fetched_at),
            );
            macro_map.insert(symbol.to_string(), serde_json::Value::Object(item));
        }
    }
    if macro_map.is_empty() {
        anyhow::bail!("No macro data available");
    }
    Ok(serde_json::Value::Object(macro_map))
}

fn get_alerts_json_backend(backend: &BackendConnection) -> Vec<serde_json::Value> {
    match crate::alerts::engine::check_alerts_backend_only(backend) {
        Ok(results) => results
            .iter()
            .filter(|r| r.newly_triggered)
            .map(|r| {
                serde_json::json!({
                    "kind": format!("{:?}", r.rule.kind),
                    "symbol": r.rule.symbol,
                    "direction": format!("{:?}", r.rule.direction),
                    "threshold": r.rule.threshold,
                    "current_value": r.current_value.map(|v| v.to_string()),
                    "rule_text": r.rule.rule_text,
                    "newly_triggered": r.newly_triggered,
                    "distance_pct": r.distance_pct.map(|d| d.round_dp(2).to_string()),
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn get_drift_json_backend(backend: &BackendConnection) -> Result<serde_json::Value> {
    let allocs = crate::db::allocations::list_allocations_backend(backend)?;
    if allocs.is_empty() {
        anyhow::bail!("No allocations (not in percentage mode)");
    }
    let cached = crate::db::price_cache::get_all_cached_prices_backend(backend)?;
    let prices: HashMap<String, Decimal> =
        cached.into_iter().map(|q| (q.symbol, q.price)).collect();
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions_from_allocations(&allocs, &prices, &fx_rates);
    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    if total_value <= dec!(0) {
        anyhow::bail!("No priced positions");
    }

    let mut drift_items = Vec::new();
    for pos in positions {
        if let Some(current_value) = pos.current_value {
            let current_pct = (current_value / total_value) * dec!(100);
            if let Some(alloc) = allocs.iter().find(|a| a.symbol == pos.symbol) {
                let target_pct = alloc.allocation_pct;
                let drift = current_pct - target_pct;
                if drift.abs() > dec!(1.0) {
                    drift_items.push(serde_json::json!({
                        "symbol": pos.symbol,
                        "target_pct": target_pct.to_string(),
                        "current_pct": current_pct.round_dp(2).to_string(),
                        "drift": drift.round_dp(2).to_string(),
                    }));
                }
            }
        }
    }
    Ok(serde_json::json!({ "items": drift_items, "has_drift": !drift_items.is_empty() }))
}

fn get_regime_json_backend(backend: &BackendConnection) -> Result<serde_json::Value> {
    if let Some(snapshot) = crate::db::regime_snapshots::get_current_backend(backend)? {
        Ok(serde_json::json!({
            "regime": snapshot.regime,
            "confidence": snapshot.confidence,
            "drivers": snapshot.drivers,
            "recorded_at": snapshot.recorded_at,
            "vix": snapshot.vix,
            "dxy": snapshot.dxy,
            "yield_10y": snapshot.yield_10y,
            "oil": snapshot.oil,
            "gold": snapshot.gold,
            "btc": snapshot.btc,
        }))
    } else {
        anyhow::bail!("No regime data available")
    }
}

fn get_news_summary_json_backend(backend: &BackendConnection) -> Result<Vec<serde_json::Value>> {
    let items =
        crate::db::news_cache::get_latest_news_backend(backend, 10, None, None, None, None)?;
    Ok(items
        .into_iter()
        .map(|n| serde_json::json!({
            "title": n.title, "url": n.url, "source": n.source, "source_type": n.source_type,
            "description": n.description, "extra_snippets": n.extra_snippets, "published_at": n.published_at,
        }))
        .collect())
}

fn get_economic_data_json_backend(backend: &BackendConnection) -> Result<Vec<serde_json::Value>> {
    let rows = crate::db::economic_data::get_all_backend(backend)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "indicator": r.indicator,
                "value": r.value.to_string(),
                "previous": r.previous.map(|v| v.to_string()),
                "change": r.change.map(|v| v.to_string()),
                "source_url": r.source_url,
                "fetched_at": r.fetched_at,
            })
        })
        .collect())
}

fn get_predictions_json_backend(backend: &BackendConnection) -> Result<Vec<serde_json::Value>> {
    let rows = crate::db::predictions_cache::get_cached_predictions_backend(backend, 5)?;
    Ok(rows
        .into_iter()
        .map(|p| serde_json::json!({
            "id": p.id, "question": p.question, "probability": p.probability, "volume_24h": p.volume_24h, "category": p.category,
        }))
        .collect())
}

fn get_sentiment_json_backend(backend: &BackendConnection) -> Result<serde_json::Value> {
    let crypto = crate::db::sentiment_cache::get_latest_backend(backend, "crypto_fng")?;
    let traditional = crate::db::sentiment_cache::get_latest_backend(backend, "traditional_fng")?;
    Ok(serde_json::json!({
        "crypto_fng": crypto.map(|r| serde_json::json!({"value": r.value, "classification": r.classification, "timestamp": r.timestamp})),
        "traditional_fng": traditional.map(|r| serde_json::json!({"value": r.value, "classification": r.classification, "timestamp": r.timestamp})),
    }))
}

fn get_top_timeframe_signal_json_backend(backend: &BackendConnection) -> Result<serde_json::Value> {
    if let Some(sig) = crate::db::timeframe_signals::latest_signal_backend(backend)? {
        Ok(serde_json::json!({
            "id": sig.id,
            "signal_type": sig.signal_type,
            "layers": sig.layers,
            "assets": sig.assets,
            "description": sig.description,
            "severity": sig.severity,
            "detected_at": sig.detected_at,
        }))
    } else {
        anyhow::bail!("No timeframe signals available")
    }
}

fn get_nearest_levels_json(
    conn: &Connection,
    symbol: &str,
    current_price: Decimal,
) -> Option<ActionableLevelPair> {
    let levels = technical_levels::get_levels_for_symbol(conn, symbol).ok()?;
    if levels.is_empty() {
        return None;
    }
    let price = current_price.to_string().parse::<f64>().ok()?;
    let pair = nearest_actionable_levels(&levels, price);
    if pair.support.is_none() && pair.resistance.is_none() {
        None
    } else {
        Some(pair)
    }
}

fn get_nearest_levels_json_backend(
    backend: &BackendConnection,
    symbol: &str,
    current_price: Decimal,
) -> Option<ActionableLevelPair> {
    let levels = technical_levels::get_levels_for_symbol_backend(backend, symbol).ok()?;
    if levels.is_empty() {
        return None;
    }
    let price = current_price.to_string().parse::<f64>().ok()?;
    let pair = nearest_actionable_levels(&levels, price);
    if pair.support.is_none() && pair.resistance.is_none() {
        None
    } else {
        Some(pair)
    }
}

fn get_watchlist_json(
    conn: &Connection,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<Vec<WatchlistItemJson>> {
    use crate::db::watchlist::list_watchlist;

    let watchlist = list_watchlist(conn)?;
    let items: Vec<WatchlistItemJson> = watchlist
        .iter()
        .map(|w| {
            let current_price = prices.get(&w.symbol).copied();
            let daily_change_pct =
                if let (Some(current), Some(&prev)) = (current_price, hist_1d.get(&w.symbol)) {
                    pct_change(current, prev)
                } else {
                    None
                };

            let technicals_json = technicals_data
                .get(&w.symbol)
                .map(|t| TechnicalSnapshotJson {
                    rsi: t.rsi_14.map(|v| format!("{:.1}", v)),
                    rsi_signal: t.rsi_14.map(|v| {
                        if v > 70.0 {
                            "overbought".to_string()
                        } else if v < 30.0 {
                            "oversold".to_string()
                        } else {
                            "neutral".to_string()
                        }
                    }),
                    macd: t.macd.map(|v| format!("{:.4}", v)),
                    macd_signal: t.macd_signal.map(|v| format!("{:.4}", v)),
                    macd_histogram: t.macd_histogram.map(|v| format!("{:.4}", v)),
                    sma_20: None,
                    sma_50: t.sma_50.map(|v| format!("{:.2}", v)),
                });

            let levels = current_price
                .and_then(|p| get_nearest_levels_json(conn, &w.symbol, p));

            WatchlistItemJson {
                symbol: w.symbol.clone(),
                name: resolve_name(&w.symbol),
                category: w.category.clone(),
                current_price: current_price.map(|p| p.to_string()),
                daily_change_pct: daily_change_pct.map(|p| p.round_dp(2).to_string()),
                technicals: technicals_json,
                levels,
            }
        })
        .collect();
    Ok(items)
}

/// Build a map of symbol → signal descriptions from recent technical signals.
fn build_signal_map(
    signals: &[crate::db::technical_signals::TechnicalSignalRecord],
) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for sig in signals {
        map.entry(sig.symbol.clone())
            .or_default()
            .push(sig.description.clone());
    }
    map
}

/// Serialize technical signals to JSON values for the brief.
fn signals_to_json(
    signals: &[crate::db::technical_signals::TechnicalSignalRecord],
) -> Vec<serde_json::Value> {
    signals
        .iter()
        .map(|s| {
            serde_json::json!({
                "symbol": s.symbol,
                "type": s.signal_type,
                "direction": s.direction,
                "severity": s.severity,
                "description": s.description,
                "detected_at": s.detected_at,
            })
        })
        .collect()
}

fn get_movers_json(
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
    signal_map: &HashMap<String, Vec<String>>,
) -> Vec<MoverJson> {
    let mut movers: Vec<(String, String, Decimal)> = Vec::new();

    for pos in positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        if let (Some(current), Some(&prev)) = (pos.current_price, hist_1d.get(&pos.symbol)) {
            if let Some(pct) = pct_change(current, prev) {
                movers.push((pos.symbol.clone(), resolve_name(&pos.symbol), pct));
            }
        }
    }

    movers.sort_by(|a, b| b.2.abs().cmp(&a.2.abs()));
    movers.truncate(5);

    movers
        .into_iter()
        .map(|(symbol, name, pct)| {
            let signals = signal_map.get(&symbol).cloned().unwrap_or_default();
            MoverJson {
                symbol,
                name,
                daily_change_pct: pct.round_dp(2).to_string(),
                daily_change_abs: pct.abs().round_dp(2).to_string(),
                signals,
            }
        })
        .collect()
}

fn get_market_movers_json(
    positions: &[Position],
    watchlist_symbols: &[String],
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    signal_map: &HashMap<String, Vec<String>>,
) -> Vec<MoverJson> {
    let held: HashSet<String> = positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .map(|p| p.symbol.to_ascii_uppercase())
        .collect();

    let mut candidates: HashSet<String> = HashSet::new();
    for symbol in watchlist_symbols {
        candidates.insert(symbol.to_ascii_uppercase());
    }
    for symbol in DEFAULT_MARKET_MOVER_SYMBOLS {
        candidates.insert((*symbol).to_string());
    }

    let mut movers: Vec<(String, String, Decimal)> = Vec::new();
    for symbol in candidates {
        if held.contains(&symbol) {
            continue;
        }
        if let (Some(current), Some(&prev)) = (prices.get(&symbol), hist_1d.get(&symbol)) {
            if let Some(pct) = pct_change(*current, prev) {
                movers.push((symbol.clone(), resolve_name(&symbol), pct));
            }
        }
    }

    movers.sort_by(|a, b| b.2.abs().cmp(&a.2.abs()));
    movers.truncate(5);
    movers
        .into_iter()
        .map(|(symbol, name, pct)| {
            let signals = signal_map.get(&symbol).cloned().unwrap_or_default();
            MoverJson {
                symbol,
                name,
                daily_change_pct: pct.round_dp(2).to_string(),
                daily_change_abs: pct.abs().round_dp(2).to_string(),
                signals,
            }
        })
        .collect()
}

fn get_macro_json(conn: &Connection) -> Result<serde_json::Value> {
    // Try to get macro data from the macro command
    use crate::db::price_cache::get_cached_price;

    let mut macro_map = serde_json::Map::new();

    // Standard macro symbols
    let macro_symbols = vec![
        ("DX-Y.NYB", "Dollar Index"),
        ("^VIX", "VIX"),
        ("^TNX", "10Y Treasury"),
        ("CL=F", "Crude Oil"),
        ("GC=F", "Gold"),
        ("SI=F", "Silver"),
        ("HG=F", "Copper"),
    ];

    for (symbol, name) in macro_symbols {
        if let Ok(Some(quote)) = get_cached_price(conn, symbol, "USD") {
            let mut item = serde_json::Map::new();
            item.insert(
                "name".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            item.insert(
                "price".to_string(),
                serde_json::Value::String(quote.price.to_string()),
            );
            item.insert(
                "fetched_at".to_string(),
                serde_json::Value::String(quote.fetched_at),
            );
            macro_map.insert(symbol.to_string(), serde_json::Value::Object(item));
        }
    }

    if macro_map.is_empty() {
        anyhow::bail!("No macro data available");
    }

    Ok(serde_json::Value::Object(macro_map))
}

fn get_alerts_json(conn: &Connection) -> Vec<serde_json::Value> {
    use crate::alerts::engine::check_alerts;

    match check_alerts(conn) {
        Ok(results) => results
            .iter()
            .filter(|r| r.newly_triggered)
            .map(|r| {
                serde_json::json!({
                    "kind": format!("{:?}", r.rule.kind),
                    "symbol": r.rule.symbol,
                    "direction": format!("{:?}", r.rule.direction),
                    "threshold": r.rule.threshold,
                    "current_value": r.current_value.map(|v| v.to_string()),
                    "rule_text": r.rule.rule_text,
                    "newly_triggered": r.newly_triggered,
                    "distance_pct": r.distance_pct.map(|d| d.round_dp(2).to_string()),
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn get_drift_json(conn: &Connection) -> Result<serde_json::Value> {
    // Simplified drift data - just return whether drift exists
    use crate::db::allocations::list_allocations;
    use crate::db::price_cache::get_all_cached_prices;

    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        anyhow::bail!("No allocations (not in percentage mode)");
    }

    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> =
        cached.into_iter().map(|q| (q.symbol, q.price)).collect();

    let fx_rates = crate::db::fx_cache::get_all_fx_rates(conn).unwrap_or_default();
    let positions = compute_positions_from_allocations(&allocs, &prices, &fx_rates);
    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();

    if total_value <= dec!(0) {
        anyhow::bail!("No priced positions");
    }

    let mut drift_items = Vec::new();
    for pos in positions {
        if let Some(current_value) = pos.current_value {
            let current_pct = (current_value / total_value) * dec!(100);
            if let Some(alloc) = allocs.iter().find(|a| a.symbol == pos.symbol) {
                let target_pct = alloc.allocation_pct;
                let drift = current_pct - target_pct;
                if drift.abs() > dec!(1.0) {
                    drift_items.push(serde_json::json!({
                        "symbol": pos.symbol,
                        "target_pct": target_pct.to_string(),
                        "current_pct": current_pct.round_dp(2).to_string(),
                        "drift": drift.round_dp(2).to_string(),
                    }));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "items": drift_items,
        "has_drift": !drift_items.is_empty(),
    }))
}

fn get_regime_json(conn: &Connection) -> Result<serde_json::Value> {
    if let Some(snapshot) = crate::db::regime_snapshots::get_current(conn)? {
        Ok(serde_json::json!({
            "regime": snapshot.regime,
            "confidence": snapshot.confidence,
            "drivers": snapshot.drivers,
            "recorded_at": snapshot.recorded_at,
            "vix": snapshot.vix,
            "dxy": snapshot.dxy,
            "yield_10y": snapshot.yield_10y,
            "oil": snapshot.oil,
            "gold": snapshot.gold,
            "btc": snapshot.btc,
        }))
    } else {
        anyhow::bail!("No regime data available")
    }
}

fn get_news_summary_json(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let items = crate::db::news_cache::get_latest_news(conn, 10, None, None, None, None)?;
    Ok(items
        .into_iter()
        .map(|n| {
            serde_json::json!({
                "title": n.title,
                "url": n.url,
                "source": n.source,
                "source_type": n.source_type,
                "description": n.description,
                "extra_snippets": n.extra_snippets,
                "published_at": n.published_at,
            })
        })
        .collect())
}

fn get_economic_data_json(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let rows = crate::db::economic_data::get_all(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "indicator": r.indicator,
                "value": r.value.to_string(),
                "previous": r.previous.map(|v| v.to_string()),
                "change": r.change.map(|v| v.to_string()),
                "source_url": r.source_url,
                "fetched_at": r.fetched_at,
            })
        })
        .collect())
}

fn get_predictions_json(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let rows = crate::db::predictions_cache::get_cached_predictions(conn, 5)?;
    Ok(rows
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "question": p.question,
                "probability": p.probability,
                "volume_24h": p.volume_24h,
                "category": p.category,
            })
        })
        .collect())
}

fn get_sentiment_json(conn: &Connection) -> Result<serde_json::Value> {
    let crypto = crate::db::sentiment_cache::get_latest(conn, "crypto_fng")?;
    let traditional = crate::db::sentiment_cache::get_latest(conn, "traditional_fng")?;
    Ok(serde_json::json!({
        "crypto_fng": crypto.map(|r| serde_json::json!({
            "value": r.value,
            "classification": r.classification,
            "timestamp": r.timestamp,
        })),
        "traditional_fng": traditional.map(|r| serde_json::json!({
            "value": r.value,
            "classification": r.classification,
            "timestamp": r.timestamp,
        })),
    }))
}

fn get_top_timeframe_signal_json(conn: &Connection) -> Result<serde_json::Value> {
    if let Some(sig) = crate::db::timeframe_signals::latest_signal(conn)? {
        Ok(serde_json::json!({
            "id": sig.id,
            "signal_type": sig.signal_type,
            "layers": sig.layers,
            "assets": sig.assets,
            "description": sig.description,
            "severity": sig.severity,
            "detected_at": sig.detected_at,
        }))
    } else {
        anyhow::bail!("No timeframe signals available")
    }
}

/// Format a decimal with commas as thousands separators.
fn fmt_commas(value: Decimal, dp: u32) -> String {
    let rounded = value.round_dp(dp);
    let s = format!("{:.prec$}", rounded, prec = dp as usize);

    let (integer_part, decimal_part) = if let Some(dot_pos) = s.find('.') {
        (&s[..dot_pos], Some(&s[dot_pos..]))
    } else {
        (s.as_str(), None)
    };

    let (sign, digits) = if let Some(stripped) = integer_part.strip_prefix('-') {
        ("-", stripped)
    } else {
        ("", integer_part)
    };

    let mut result = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    let formatted_int: String = result.chars().rev().collect();

    match decimal_part {
        Some(dec) => format!("{}{}{}", sign, formatted_int, dec),
        None => format!("{}{}", sign, formatted_int),
    }
}

/// Format a currency value with symbol prefix.
fn fmt_currency(value: Decimal, dp: u32, base: &str) -> String {
    let sym = crate::config::currency_symbol(base);
    format!("{}{}", sym, fmt_commas(value, dp))
}

/// Compute percent change between two values.
fn pct_change(current: Decimal, previous: Decimal) -> Option<Decimal> {
    if previous > dec!(0) {
        let pct = ((current - previous) / previous) * dec!(100);
        // Plausibility guard: reject anomalous changes from corrupt price data
        if crate::models::price::is_plausible_daily_change(pct) {
            Some(pct)
        } else {
            None
        }
    } else {
        None
    }
}

fn compute_portfolio_day_pct(
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
) -> Option<Decimal> {
    let mut day_pnl = dec!(0);
    let mut prev_value = dec!(0);
    let mut has = false;

    for pos in positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(v) => v,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(v) => *v,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        day_pnl += (current - prev) * pos.quantity;
        prev_value += prev * pos.quantity;
        has = true;
    }

    if has && prev_value > dec!(0) {
        Some((day_pnl / prev_value) * dec!(100))
    } else {
        None
    }
}

fn compute_symbol_day_pct(conn: &Connection, symbol: &str) -> Option<Decimal> {
    let current = get_cached_price(conn, symbol, "USD").ok()??.price;
    let yesterday = (Utc::now().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let prev = get_prices_at_date(conn, &[symbol.to_string()], &yesterday)
        .ok()?
        .get(symbol)
        .copied()?;
    pct_change(current, prev)
}

fn compute_symbol_day_pct_backend(backend: &BackendConnection, symbol: &str) -> Option<Decimal> {
    let current = crate::db::price_cache::get_cached_price_backend(backend, symbol, "USD")
        .ok()??
        .price;
    let yesterday = (Utc::now().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let prev = crate::db::price_history::get_prices_at_date_backend(
        backend,
        &[symbol.to_string()],
        &yesterday,
    )
    .ok()?
    .get(symbol)
    .copied()?;
    pct_change(current, prev)
}

fn print_benchmark_comparison(
    conn: &Connection,
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
) {
    let portfolio_1d = compute_portfolio_day_pct(positions, hist_1d);
    let benchmark_1d = compute_symbol_day_pct(conn, "SPY");

    println!("## Benchmark (SPY)\n");
    match (portfolio_1d, benchmark_1d) {
        (Some(p), Some(b)) => {
            let spread = p - b;
            println!("- Portfolio 1D: {:+.2}%", p);
            println!("- SPY 1D: {:+.2}%", b);
            println!("- Relative: {:+.2}%\n", spread);
        }
        _ => {
            println!("- Benchmark comparison unavailable (run `pftui refresh` and ensure SPY price history exists).\n");
        }
    }
}

fn print_benchmark_comparison_backend(
    backend: &BackendConnection,
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
) {
    let portfolio_1d = compute_portfolio_day_pct(positions, hist_1d);
    let benchmark_1d = compute_symbol_day_pct_backend(backend, "SPY");

    println!("## Benchmark (SPY)\n");
    match (portfolio_1d, benchmark_1d) {
        (Some(p), Some(b)) => {
            let spread = p - b;
            println!("- Portfolio 1D: {:+.2}%", p);
            println!("- SPY 1D: {:+.2}%", b);
            println!("- Relative: {:+.2}%\n", spread);
        }
        _ => {
            println!("- Benchmark comparison unavailable (run `pftui refresh` and ensure SPY price history exists).\n");
        }
    }
}

fn run_internal(
    conn: &Connection,
    config: &Config,
    technicals: bool,
    agent: bool,
    cached_only: bool,
) -> Result<()> {
    if agent {
        return run_agent_mode(conn, config);
    }
    if cached_only {
        eprintln!("Note: cached-only mode enabled; brief is rendered from local cache.");
    }
    let cached = get_all_cached_prices(conn)?;
    let (prices, previous_close) = split_cached_quotes(cached);

    // Get 1-day historical prices for top movers
    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = enrich_hist_1d_with_cached_previous_close(
        &get_prices_at_date(conn, &symbols, &yesterday_str).unwrap_or_default(),
        &previous_close,
    );

    // Load price history for technicals if requested
    let technicals_data = if technicals {
        compute_technicals_for_symbols(conn, &symbols)
    } else {
        HashMap::new()
    };

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, &prices, &hist_1d, &technicals_data),
        PortfolioMode::Percentage => {
            run_percentage(conn, config, &prices, &hist_1d, &technicals_data)
        }
    }
}

pub fn run_backend(
    backend: &BackendConnection,
    config: &Config,
    technicals: bool,
    agent: bool,
    cached_only: bool,
) -> Result<()> {
    match backend {
        BackendConnection::Sqlite { conn } => {
            run_internal(conn, config, technicals, agent, cached_only)
        }
        BackendConnection::Postgres { .. } => {
            run_backend_native(backend, config, technicals, agent, cached_only)
        }
    }
}

fn run_backend_native(
    backend: &BackendConnection,
    config: &Config,
    technicals: bool,
    agent: bool,
    cached_only: bool,
) -> Result<()> {
    if agent {
        return run_agent_mode_backend(backend, config);
    }
    if cached_only {
        eprintln!("Note: cached-only mode enabled; brief is rendered from local cache.");
    }

    let cached = crate::db::price_cache::get_all_cached_prices_backend(backend)?;
    let (prices, previous_close) = split_cached_quotes(cached);
    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = enrich_hist_1d_with_cached_previous_close(
        &crate::db::price_history::get_prices_at_date_backend(backend, &symbols, &yesterday_str)
            .unwrap_or_default(),
        &previous_close,
    );

    let technicals_data = if technicals {
        compute_technicals_for_symbols_backend(backend, &symbols)
    } else {
        HashMap::new()
    };

    match config.portfolio_mode {
        PortfolioMode::Full => {
            run_full_backend(backend, config, &prices, &hist_1d, &technicals_data)
        }
        PortfolioMode::Percentage => {
            run_percentage_backend(backend, config, &prices, &hist_1d, &technicals_data)
        }
    }
}

fn run_full(
    conn: &Connection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<()> {
    let txs = list_transactions(conn)?;
    if txs.is_empty() {
        println!("# Portfolio Brief\n\nNo positions. Add one with: `pftui add-tx`");
        return Ok(());
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates(conn).unwrap_or_default();
    let positions = compute_positions(&txs, prices, &fx_rates);
    if positions.is_empty() {
        println!("# Portfolio Brief\n\nNo open positions.");
        return Ok(());
    }

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value - total_cost;
    let total_gain_pct = pct_change(total_value, total_cost).unwrap_or(dec!(0));
    let base = &config.base_currency;

    let priced_count = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .count();
    let total_count = positions.len();

    // Compute daily P&L
    let mut daily_pnl = dec!(0);
    let mut has_daily = false;
    for pos in &positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(p) => p,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(p) => *p,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        daily_pnl += (current - prev) * pos.quantity;
        has_daily = true;
    }

    // Date header
    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);

    // Total value line
    let sign = if total_gain >= dec!(0) { "+" } else { "" };
    println!(
        "**{}** ({}{} / {}{}%)",
        fmt_currency(total_value, 2, base),
        sign,
        fmt_commas(total_gain, 2),
        sign,
        total_gain_pct.round_dp(1),
    );

    // Daily P&L line
    if has_daily {
        let day_sign = if daily_pnl >= dec!(0) { "+" } else { "" };
        let day_pct = if total_value > dec!(0) {
            (daily_pnl / (total_value - daily_pnl)) * dec!(100)
        } else {
            dec!(0)
        };
        println!(
            "**1D:** {}{} ({}{}%)",
            day_sign,
            fmt_currency(daily_pnl.abs(), 2, base),
            day_sign,
            day_pct.round_dp(2),
        );
    }
    print_risk_summary(conn, &positions);
    print_benchmark_comparison(conn, &positions, hist_1d);
    print_correlation_summary(conn, &positions);
    println!();

    // Category allocation
    print_category_allocation(&positions, total_value);

    // What changed today: movers + threshold crossings + triggered alerts
    print_what_changed_today(conn, &positions, hist_1d, base);

    // P&L attribution (by dollar amount)
    print_pnl_attribution(&positions, hist_1d, base);

    // Position table
    print_position_table_full(&positions, base, hist_1d);

    // Technicals section
    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }
    print_key_levels(conn, &positions, base);

    // Warnings
    if priced_count < total_count {
        let missing = total_count - priced_count;
        println!(
            "\n> ⚠️ {}/{} positions missing prices. Run `pftui refresh`.",
            missing, total_count
        );
    }

    Ok(())
}

fn run_percentage(
    conn: &Connection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<()> {
    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        println!("# Portfolio Brief\n\nNo allocations. Run: `pftui setup`");
        return Ok(());
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates(conn).unwrap_or_default();
    let positions = compute_positions_from_allocations(&allocs, prices, &fx_rates);
    let base = &config.base_currency;

    let priced: Vec<_> = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .collect();
    if priced.is_empty() {
        println!("# Portfolio Brief\n\nNo prices cached. Run `pftui refresh` first.");
        return Ok(());
    }

    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);
    println!("*Percentage mode (allocation-based)*\n");
    print_risk_summary(conn, &positions);
    print_benchmark_comparison(conn, &positions, hist_1d);
    print_correlation_summary(conn, &positions);
    println!();

    // Category allocation (use raw pct since no total value)
    print_category_allocation_pct(&positions);

    // What changed today: movers + threshold crossings + triggered alerts
    print_what_changed_today(conn, &positions, hist_1d, base);

    // P&L attribution (by dollar amount)
    print_pnl_attribution(&positions, hist_1d, base);

    // Position table for percentage mode
    println!("## Positions\n");
    println!("| Symbol | Category | Price | 1D | Alloc |");
    println!("|--------|----------|------:|---:|------:|");
    for pos in &positions {
        let price_str = pos
            .current_price
            .map(|p| fmt_currency(p, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "—".to_string());
        let name = resolve_name(&pos.symbol);
        let symbol_display = if name.is_empty() {
            pos.symbol.clone()
        } else {
            format!("{} ({})", pos.symbol, name)
        };
        let day_str = if pos.category == AssetCategory::Cash {
            "—".to_string()
        } else {
            match (pos.current_price, hist_1d.get(&pos.symbol)) {
                (Some(current), Some(prev)) if *prev > dec!(0) => {
                    let pct = ((current - prev) / prev) * dec!(100);
                    format!("{:+.1}%", pct)
                }
                _ => "—".to_string(),
            }
        };
        println!(
            "| {} | {} | {} | {} | {} |",
            symbol_display, pos.category, price_str, day_str, alloc_str
        );
    }

    // Technicals section
    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }
    print_key_levels(conn, &positions, base);

    let missing = positions.len() - priced.len();
    if missing > 0 {
        println!(
            "\n> ⚠️ {}/{} positions missing prices. Run `pftui refresh`.",
            missing,
            positions.len()
        );
    }

    Ok(())
}

fn run_full_backend(
    backend: &BackendConnection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<()> {
    let txs = crate::db::transactions::list_transactions_backend(backend)?;
    if txs.is_empty() {
        println!("# Portfolio Brief\n\nNo positions. Add one with: `pftui add-tx`");
        return Ok(());
    }
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions(&txs, prices, &fx_rates);
    if positions.is_empty() {
        println!("# Portfolio Brief\n\nNo open positions.");
        return Ok(());
    }

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value - total_cost;
    let total_gain_pct = pct_change(total_value, total_cost).unwrap_or(dec!(0));
    let base = &config.base_currency;
    let priced_count = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .count();
    let total_count = positions.len();

    let mut daily_pnl = dec!(0);
    let mut has_daily = false;
    for pos in &positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(p) => p,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(p) => *p,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        daily_pnl += (current - prev) * pos.quantity;
        has_daily = true;
    }

    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);
    let sign = if total_gain >= dec!(0) { "+" } else { "" };
    println!(
        "**{}** ({}{} / {}{}%)",
        fmt_currency(total_value, 2, base),
        sign,
        fmt_commas(total_gain, 2),
        sign,
        total_gain_pct.round_dp(1),
    );
    if has_daily {
        let day_sign = if daily_pnl >= dec!(0) { "+" } else { "" };
        let day_pct = if total_value > dec!(0) {
            (daily_pnl / (total_value - daily_pnl)) * dec!(100)
        } else {
            dec!(0)
        };
        println!(
            "**1D:** {}{} ({}{}%)",
            day_sign,
            fmt_currency(daily_pnl.abs(), 2, base),
            day_sign,
            day_pct.round_dp(2),
        );
    }
    print_risk_summary_backend(backend, &positions);
    print_benchmark_comparison_backend(backend, &positions, hist_1d);
    print_correlation_summary_backend(backend, &positions);
    println!();

    print_category_allocation(&positions, total_value);
    print_what_changed_today_backend(backend, &positions, hist_1d, base);
    print_pnl_attribution(&positions, hist_1d, base);
    print_position_table_full(&positions, base, hist_1d);

    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }
    print_key_levels_backend(backend, &positions, base);
    if priced_count < total_count {
        let missing = total_count - priced_count;
        println!(
            "\n> ⚠️ {}/{} positions missing prices. Run `pftui refresh`.",
            missing, total_count
        );
    }
    Ok(())
}

fn run_percentage_backend(
    backend: &BackendConnection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<()> {
    let allocs = crate::db::allocations::list_allocations_backend(backend)?;
    if allocs.is_empty() {
        println!("# Portfolio Brief\n\nNo allocations. Run: `pftui setup`");
        return Ok(());
    }
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions_from_allocations(&allocs, prices, &fx_rates);
    let base = &config.base_currency;

    let priced: Vec<_> = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .collect();
    if priced.is_empty() {
        println!("# Portfolio Brief\n\nNo prices cached. Run `pftui refresh` first.");
        return Ok(());
    }

    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);
    println!("*Percentage mode (allocation-based)*\n");
    print_risk_summary_backend(backend, &positions);
    print_benchmark_comparison_backend(backend, &positions, hist_1d);
    print_correlation_summary_backend(backend, &positions);
    println!();

    print_category_allocation_pct(&positions);
    print_what_changed_today_backend(backend, &positions, hist_1d, base);
    print_pnl_attribution(&positions, hist_1d, base);

    println!("## Positions\n");
    println!("| Symbol | Category | Price | 1D | Alloc |");
    println!("|--------|----------|------:|---:|------:|");
    for pos in &positions {
        let price_str = pos
            .current_price
            .map(|p| fmt_currency(p, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "—".to_string());
        let name = resolve_name(&pos.symbol);
        let symbol_display = if name.is_empty() {
            pos.symbol.clone()
        } else {
            format!("{} ({})", pos.symbol, name)
        };
        let day_str = if pos.category == AssetCategory::Cash {
            "—".to_string()
        } else {
            match (pos.current_price, hist_1d.get(&pos.symbol)) {
                (Some(current), Some(prev)) if *prev > dec!(0) => {
                    let pct = ((current - prev) / prev) * dec!(100);
                    format!("{:+.1}%", pct)
                }
                _ => "—".to_string(),
            }
        };
        println!(
            "| {} | {} | {} | {} | {} |",
            symbol_display, pos.category, price_str, day_str, alloc_str
        );
    }

    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }
    print_key_levels_backend(backend, &positions, base);
    let missing = positions.len() - priced.len();
    if missing > 0 {
        println!(
            "\n> ⚠️ {}/{} positions missing prices. Run `pftui refresh`.",
            missing,
            positions.len()
        );
    }
    Ok(())
}

fn print_risk_summary(conn: &Connection, positions: &[Position]) {
    let snapshots = get_all_portfolio_snapshots(conn).unwrap_or_default();
    let portfolio_values: Vec<Decimal> = snapshots.iter().map(|s| s.total_value).collect();

    let live_values: Vec<Decimal> = positions.iter().filter_map(|p| p.current_value).collect();
    let concentration_values: Vec<Decimal> = if live_values.is_empty() {
        positions.iter().filter_map(|p| p.allocation_pct).collect()
    } else {
        live_values
    };

    let ffr_pct = economic_cache::get_latest(conn, "FEDFUNDS")
        .ok()
        .flatten()
        .map(|o| o.value);

    let metrics = risk::compute_risk_metrics(&portfolio_values, &concentration_values, ffr_pct);
    let vol = metrics
        .annualized_volatility_pct
        .map(|v| format!("{:.1}%", v))
        .unwrap_or_else(|| "N/A".to_string());
    let var95 = metrics
        .historical_var_95_pct
        .map(|v| format!("{:.1}%", v))
        .unwrap_or_else(|| "N/A".to_string());
    let concentration = match metrics.herfindahl_index {
        Some(h) if h >= dec!(0.25) => format!("HIGH ({:.3})", h),
        Some(h) if h >= dec!(0.15) => format!("MODERATE ({:.3})", h),
        Some(h) => format!("LOW ({:.3})", h),
        None => "N/A".to_string(),
    };

    println!(
        "**Risk:** vol {} · VaR95 {} · concentration {}",
        vol, var95, concentration
    );
}

fn print_risk_summary_backend(backend: &BackendConnection, positions: &[Position]) {
    let snapshots =
        crate::db::snapshots::get_all_portfolio_snapshots_backend(backend).unwrap_or_default();
    let portfolio_values: Vec<Decimal> = snapshots.iter().map(|s| s.total_value).collect();

    let live_values: Vec<Decimal> = positions.iter().filter_map(|p| p.current_value).collect();
    let concentration_values: Vec<Decimal> = if live_values.is_empty() {
        positions.iter().filter_map(|p| p.allocation_pct).collect()
    } else {
        live_values
    };

    let ffr_pct = crate::db::economic_cache::get_latest_backend(backend, "FEDFUNDS")
        .ok()
        .flatten()
        .map(|o| o.value);

    let metrics = risk::compute_risk_metrics(&portfolio_values, &concentration_values, ffr_pct);
    let vol = metrics
        .annualized_volatility_pct
        .map(|v| format!("{:.1}%", v))
        .unwrap_or_else(|| "N/A".to_string());
    let var95 = metrics
        .historical_var_95_pct
        .map(|v| format!("{:.1}%", v))
        .unwrap_or_else(|| "N/A".to_string());
    let concentration = match metrics.herfindahl_index {
        Some(h) if h >= dec!(0.25) => format!("HIGH ({:.3})", h),
        Some(h) if h >= dec!(0.15) => format!("MODERATE ({:.3})", h),
        Some(h) => format!("LOW ({:.3})", h),
        None => "N/A".to_string(),
    };

    println!(
        "**Risk:** vol {} · VaR95 {} · concentration {}",
        vol, var95, concentration
    );
}

// ──────────────────────────────────────────────────────────────
// Technicals
// ──────────────────────────────────────────────────────────────

/// Snapshot of technical indicator values for a single symbol.
type TechnicalSnapshot = TechnicalSnapshotRecord;

/// Label the RSI value for quick reading.
fn rsi_label(rsi: f64) -> &'static str {
    if rsi >= 70.0 {
        "overbought"
    } else if rsi <= 30.0 {
        "oversold"
    } else {
        "neutral"
    }
}

/// Label the MACD signal.
fn macd_label(histogram: f64) -> &'static str {
    if histogram > 0.0 {
        "bullish"
    } else if histogram < 0.0 {
        "bearish"
    } else {
        "neutral"
    }
}

/// Compute technical indicators for a list of symbols from cached price history.
fn compute_technicals_for_symbols(
    conn: &Connection,
    symbols: &[String],
) -> HashMap<String, TechnicalSnapshot> {
    load_or_compute_snapshots(conn, symbols, DEFAULT_TIMEFRAME)
}

fn compute_technicals_for_symbols_backend(
    backend: &BackendConnection,
    symbols: &[String],
) -> HashMap<String, TechnicalSnapshot> {
    load_or_compute_snapshots_backend(backend, symbols, DEFAULT_TIMEFRAME)
}

/// Print a technicals section for all positions that have indicator data.
fn print_technicals_section(
    positions: &[Position],
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) {
    // Only show positions that have technicals (skip cash)
    let relevant: Vec<&Position> = positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash && technicals_data.contains_key(&p.symbol))
        .collect();

    if relevant.is_empty() {
        return;
    }

    println!("## Technicals\n");
    println!("| Symbol | RSI(14) | Signal | MACD | Hist | SMA(50) | SMA(200) |");
    println!("|--------|--------:|--------|-----:|-----:|--------:|---------:|");

    for pos in &relevant {
        let snap = match technicals_data.get(&pos.symbol) {
            Some(s) => s,
            None => continue,
        };

        let rsi_str = snap
            .rsi_14
            .map(|v| format!("{:.1}", v))
            .unwrap_or_else(|| "—".to_string());

        let rsi_sig = snap
            .rsi_14
            .map(|v| rsi_label(v).to_string())
            .unwrap_or_else(|| "—".to_string());

        let macd_str = snap
            .macd
            .map(|m| format!("{:.2}", m))
            .unwrap_or_else(|| "—".to_string());

        let hist_str = snap
            .macd_histogram
            .map(|hist| {
                let sign = if hist >= 0.0 { "+" } else { "" };
                format!("{}{:.2} ({})", sign, hist, macd_label(hist))
            })
            .unwrap_or_else(|| "—".to_string());

        let sma50_str = snap
            .sma_50
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());

        let sma200_str = snap
            .sma_200
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());

        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            pos.symbol, rsi_str, rsi_sig, macd_str, hist_str, sma50_str, sma200_str,
        );
    }
    println!();
}

// ──────────────────────────────────────────────────────────────
// Shared markdown sections
// ──────────────────────────────────────────────────────────────

fn print_category_allocation(positions: &[Position], total_value: Decimal) {
    let mut categories: HashMap<AssetCategory, Decimal> = HashMap::new();

    for pos in positions {
        if let Some(val) = pos.current_value {
            *categories.entry(pos.category).or_insert(dec!(0)) += val;
        }
    }

    if categories.is_empty() || total_value <= dec!(0) {
        return;
    }

    let mut sorted: Vec<_> = categories.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("## Allocation\n");

    let parts: Vec<String> = sorted
        .iter()
        .map(|(cat, val)| {
            let pct = (val / total_value * dec!(100)).round_dp(0);
            format!("**{}** {}%", format_category(cat), pct)
        })
        .collect();

    println!("{}\n", parts.join(" · "));
}

fn print_category_allocation_pct(positions: &[Position]) {
    let mut categories: HashMap<AssetCategory, Decimal> = HashMap::new();

    for pos in positions {
        if let Some(alloc) = pos.allocation_pct {
            *categories.entry(pos.category).or_insert(dec!(0)) += alloc;
        }
    }

    if categories.is_empty() {
        return;
    }

    let mut sorted: Vec<_> = categories.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("## Allocation\n");

    let parts: Vec<String> = sorted
        .iter()
        .map(|(cat, pct)| format!("**{}** {}%", format_category(cat), pct.round_dp(0)))
        .collect();

    println!("{}\n", parts.join(" · "));
}

fn print_top_movers(positions: &[Position], hist_1d: &HashMap<String, Decimal>, base: &str) {
    let mut movers: Vec<(&str, Decimal, Decimal)> = Vec::new(); // (symbol, current, pct_change)

    for pos in positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(p) => p,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(p) => *p,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        let pct = ((current - prev) / prev) * dec!(100);
        movers.push((&pos.symbol, current, pct));
    }

    if movers.is_empty() {
        return;
    }

    // Sort by absolute change descending
    movers.sort_by(|a, b| {
        b.2.abs()
            .partial_cmp(&a.2.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("## Top Movers (1D)\n");

    let count = movers.len().min(5);
    for (symbol, current, pct) in &movers[..count] {
        let direction = if *pct >= dec!(0) { "📈" } else { "📉" };
        let name = resolve_name(symbol);
        let label = if name.is_empty() {
            symbol.to_string()
        } else {
            format!("{} ({})", symbol, name)
        };
        println!(
            "- {} **{}** {} ({:+.1}%)",
            direction,
            label,
            fmt_currency(*current, 2, base),
            pct,
        );
    }
    println!();
}

fn print_what_changed_today(
    conn: &Connection,
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
    base: &str,
) {
    println!("## What Changed Today\n");
    print_top_movers(positions, hist_1d, base);
    print_alerts(conn);
}

fn print_what_changed_today_backend(
    backend: &BackendConnection,
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
    base: &str,
) {
    println!("## What Changed Today\n");
    print_top_movers(positions, hist_1d, base);
    print_alerts_backend(backend);
}

fn print_key_levels(conn: &Connection, positions: &[Position], base: &str) {
    println!("## Key Levels\n");
    print_key_levels_rows(
        positions.iter().filter_map(|pos| {
            if pos.category == AssetCategory::Cash {
                return None;
            }
            let price = pos.current_price?;
            let levels = get_nearest_levels_json(conn, &pos.symbol, price)?;
            Some((pos, levels))
        }),
        base,
    );
}

fn print_key_levels_backend(backend: &BackendConnection, positions: &[Position], base: &str) {
    println!("## Key Levels\n");
    print_key_levels_rows(
        positions.iter().filter_map(|pos| {
            if pos.category == AssetCategory::Cash {
                return None;
            }
            let price = pos.current_price?;
            let levels = get_nearest_levels_json_backend(backend, &pos.symbol, price)?;
            Some((pos, levels))
        }),
        base,
    );
}

fn print_key_levels_rows<'a, I>(rows: I, base: &str)
where
    I: Iterator<Item = (&'a Position, ActionableLevelPair)>,
{
    let rows: Vec<_> = rows.take(8).collect();
    if rows.is_empty() {
        println!("No stored market structure levels available.\n");
        return;
    }

    println!("| Symbol | Spot | Support | Resistance |");
    println!("|--------|-----:|--------:|-----------:|");
    for (pos, levels) in rows {
        let spot = pos
            .current_price
            .map(|price| fmt_currency(price, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let support = levels
            .support
            .as_ref()
            .map(format_actionable_level_cell)
            .unwrap_or_else(|| "—".to_string());
        let resistance = levels
            .resistance
            .as_ref()
            .map(format_actionable_level_cell)
            .unwrap_or_else(|| "—".to_string());

        println!(
            "| {} | {} | {} | {} |",
            pos.symbol, spot, support, resistance
        );
    }
    println!();
}

fn format_actionable_level_cell(level: &crate::analytics::levels::ActionableLevel) -> String {
    format!(
        "{} ({}, {:.1}%)",
        format_level_value(level.price),
        level.level_type.replace('_', " "),
        level.distance_pct
    )
}

fn format_level_value(price: f64) -> String {
    if price >= 10000.0 {
        format!("{:.0}", price)
    } else if price >= 100.0 {
        format!("{:.1}", price)
    } else if price >= 1.0 {
        format!("{:.2}", price)
    } else {
        format!("{:.4}", price)
    }
}

fn print_correlation_summary(conn: &Connection, positions: &[Position]) {
    let summary = compute_correlation_summary(conn, positions);
    if summary.top_pairs.is_empty() && summary.active_breaks.is_empty() {
        return;
    }

    println!("## Correlations\n");

    if !summary.top_pairs.is_empty() {
        println!("**Top Pairs (30d):**");
        for pair in summary.top_pairs.iter().take(5) {
            println!(
                "- {}-{}: {:+.2}",
                pair.symbol_a, pair.symbol_b, pair.corr_30d
            );
        }
    }

    if !summary.active_breaks.is_empty() {
        if !summary.top_pairs.is_empty() {
            println!();
        }
        println!("**Active Breaks (7d vs 90d):**");
        for brk in summary.active_breaks.iter().take(5) {
            let interp = interpret_break(&to_correlations_break(brk));
            let severity_badge = match interp.severity.as_str() {
                "severe" => "🔴",
                "moderate" => "🟡",
                _ => "🟢",
            };
            println!(
                "- {} {}-{}: Δ{:+.2} (7d {:+.2} vs 90d {:+.2}) — {}",
                severity_badge, brk.symbol_a, brk.symbol_b, brk.delta, brk.corr_7d,
                brk.corr_90d, interp.interpretation,
            );
        }
    }

    println!();
}

fn print_correlation_summary_backend(backend: &BackendConnection, positions: &[Position]) {
    let summary = compute_correlation_summary_backend(backend, positions);
    if summary.top_pairs.is_empty() && summary.active_breaks.is_empty() {
        return;
    }

    println!("## Correlations\n");
    if !summary.top_pairs.is_empty() {
        println!("**Top Pairs (30d):**");
        for pair in summary.top_pairs.iter().take(5) {
            println!(
                "- {}-{}: {:+.2}",
                pair.symbol_a, pair.symbol_b, pair.corr_30d
            );
        }
    }

    if !summary.active_breaks.is_empty() {
        if !summary.top_pairs.is_empty() {
            println!();
        }
        println!("**Active Breaks (7d vs 90d):**");
        for brk in summary.active_breaks.iter().take(5) {
            let interp = interpret_break(&to_correlations_break(brk));
            let severity_badge = match interp.severity.as_str() {
                "severe" => "🔴",
                "moderate" => "🟡",
                _ => "🟢",
            };
            println!(
                "- {} {}-{}: Δ{:+.2} (7d {:+.2} vs 90d {:+.2}) — {}",
                severity_badge, brk.symbol_a, brk.symbol_b, brk.delta, brk.corr_7d,
                brk.corr_90d, interp.interpretation,
            );
        }
    }
    println!();
}

/// Convert a brief-internal CorrelationBreak to the correlations module's type
/// so we can reuse `interpret_break()` for severity/interpretation/signal.
fn to_correlations_break(b: &CorrelationBreak) -> CorrelationsBreak {
    CorrelationsBreak {
        symbol_a: b.symbol_a.clone(),
        symbol_b: b.symbol_b.clone(),
        corr_7d: Some(b.corr_7d),
        corr_90d: Some(b.corr_90d),
        break_delta: b.delta,
    }
}

fn correlation_summary_to_json(summary: &CorrelationSummary) -> Option<serde_json::Value> {
    if summary.top_pairs.is_empty() && summary.active_breaks.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "top_pairs_30d": summary.top_pairs.iter().map(|p| {
            serde_json::json!({
                "symbol_a": p.symbol_a,
                "symbol_b": p.symbol_b,
                "corr_30d": p.corr_30d,
            })
        }).collect::<Vec<_>>(),
        "active_breaks": summary.active_breaks.iter().map(|b| {
            let interp = interpret_break(&to_correlations_break(b));
            serde_json::json!({
                "symbol_a": b.symbol_a,
                "symbol_b": b.symbol_b,
                "corr_7d": b.corr_7d,
                "corr_90d": b.corr_90d,
                "delta": b.delta,
                "severity": interp.severity,
                "interpretation": interp.interpretation,
                "signal": interp.signal,
            })
        }).collect::<Vec<_>>(),
    }))
}

fn compute_correlation_summary(conn: &Connection, positions: &[Position]) -> CorrelationSummary {
    const WINDOW_SHORT: usize = 7;
    const WINDOW_MAIN: usize = 30;
    const WINDOW_LONG: usize = 90;
    const BREAK_THRESHOLD: f64 = 0.30;

    let symbols: Vec<String> = positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .map(|p| p.symbol.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if symbols.len() < 2 {
        return CorrelationSummary::default();
    }

    let mut price_map: HashMap<String, Vec<f64>> = HashMap::new();
    for symbol in &symbols {
        if let Ok(history) = get_history(conn, symbol, WINDOW_LONG as u32 + 40) {
            let closes: Vec<f64> = history
                .into_iter()
                .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
                .filter(|v| *v > 0.0)
                .collect();
            if closes.len() > WINDOW_MAIN {
                price_map.insert(symbol.clone(), closes);
            }
        }
    }

    let mut top_pairs = Vec::new();
    let mut active_breaks = Vec::new();
    let mut symbols_sorted: Vec<String> = price_map.keys().cloned().collect();
    symbols_sorted.sort();

    for i in 0..symbols_sorted.len() {
        for j in (i + 1)..symbols_sorted.len() {
            let a = &symbols_sorted[i];
            let b = &symbols_sorted[j];
            let prices_a = match price_map.get(a) {
                Some(v) => v,
                None => continue,
            };
            let prices_b = match price_map.get(b) {
                Some(v) => v,
                None => continue,
            };

            let min_len = prices_a.len().min(prices_b.len());
            if min_len < WINDOW_LONG + 1 {
                continue;
            }
            let aligned_a = &prices_a[prices_a.len() - min_len..];
            let aligned_b = &prices_b[prices_b.len() - min_len..];

            let c30 = latest_corr(aligned_a, aligned_b, WINDOW_MAIN);
            let c7 = latest_corr(aligned_a, aligned_b, WINDOW_SHORT);
            let c90 = latest_corr(aligned_a, aligned_b, WINDOW_LONG);

            if let Some(corr_30d) = c30 {
                top_pairs.push(CorrelationPair {
                    symbol_a: a.clone(),
                    symbol_b: b.clone(),
                    corr_30d,
                });
            }

            if let (Some(corr_7d), Some(corr_90d)) = (c7, c90) {
                let delta = corr_7d - corr_90d;
                if delta.abs() >= BREAK_THRESHOLD {
                    active_breaks.push(CorrelationBreak {
                        symbol_a: a.clone(),
                        symbol_b: b.clone(),
                        corr_7d,
                        corr_90d,
                        delta,
                    });
                }
            }
        }
    }

    top_pairs.sort_by(|a, b| {
        b.corr_30d
            .abs()
            .partial_cmp(&a.corr_30d.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_pairs.truncate(5);

    active_breaks.sort_by(|a, b| {
        b.delta
            .abs()
            .partial_cmp(&a.delta.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    active_breaks.truncate(5);

    CorrelationSummary {
        top_pairs,
        active_breaks,
    }
}

fn compute_correlation_summary_backend(
    backend: &BackendConnection,
    positions: &[Position],
) -> CorrelationSummary {
    const WINDOW_SHORT: usize = 7;
    const WINDOW_MAIN: usize = 30;
    const WINDOW_LONG: usize = 90;
    const BREAK_THRESHOLD: f64 = 0.30;

    let symbols: Vec<String> = positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .map(|p| p.symbol.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if symbols.len() < 2 {
        return CorrelationSummary::default();
    }

    let mut price_map: HashMap<String, Vec<f64>> = HashMap::new();
    for symbol in &symbols {
        if let Ok(history) =
            crate::db::price_history::get_history_backend(backend, symbol, WINDOW_LONG as u32 + 40)
        {
            let closes: Vec<f64> = history
                .into_iter()
                .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
                .filter(|v| *v > 0.0)
                .collect();
            if closes.len() > WINDOW_MAIN {
                price_map.insert(symbol.clone(), closes);
            }
        }
    }

    let mut top_pairs = Vec::new();
    let mut active_breaks = Vec::new();
    let mut symbols_sorted: Vec<String> = price_map.keys().cloned().collect();
    symbols_sorted.sort();

    for i in 0..symbols_sorted.len() {
        for j in (i + 1)..symbols_sorted.len() {
            let a = &symbols_sorted[i];
            let b = &symbols_sorted[j];
            let prices_a = match price_map.get(a) {
                Some(v) => v,
                None => continue,
            };
            let prices_b = match price_map.get(b) {
                Some(v) => v,
                None => continue,
            };

            let min_len = prices_a.len().min(prices_b.len());
            if min_len < WINDOW_LONG + 1 {
                continue;
            }
            let aligned_a = &prices_a[prices_a.len() - min_len..];
            let aligned_b = &prices_b[prices_b.len() - min_len..];

            let c30 = latest_corr(aligned_a, aligned_b, WINDOW_MAIN);
            let c7 = latest_corr(aligned_a, aligned_b, WINDOW_SHORT);
            let c90 = latest_corr(aligned_a, aligned_b, WINDOW_LONG);

            if let Some(corr_30d) = c30 {
                top_pairs.push(CorrelationPair {
                    symbol_a: a.clone(),
                    symbol_b: b.clone(),
                    corr_30d,
                });
            }

            if let (Some(corr_7d), Some(corr_90d)) = (c7, c90) {
                let delta = corr_7d - corr_90d;
                if delta.abs() >= BREAK_THRESHOLD {
                    active_breaks.push(CorrelationBreak {
                        symbol_a: a.clone(),
                        symbol_b: b.clone(),
                        corr_7d,
                        corr_90d,
                        delta,
                    });
                }
            }
        }
    }

    top_pairs.sort_by(|a, b| {
        b.corr_30d
            .abs()
            .partial_cmp(&a.corr_30d.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_pairs.truncate(5);

    active_breaks.sort_by(|a, b| {
        b.delta
            .abs()
            .partial_cmp(&a.delta.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    active_breaks.truncate(5);

    CorrelationSummary {
        top_pairs,
        active_breaks,
    }
}

fn latest_corr(prices_a: &[f64], prices_b: &[f64], window: usize) -> Option<f64> {
    if prices_a.len() != prices_b.len() || prices_a.len() < window + 1 {
        return None;
    }
    compute_rolling_correlation(prices_a, prices_b, window)
        .into_iter()
        .rev()
        .flatten()
        .next()
}

/// Render alert check results as markdown, deduplicating repeated triggered alerts
/// by symbol. When multiple triggered alerts share the same symbol, show only the
/// most recent one with a count annotation (e.g. "+3 more").
fn render_alerts_markdown(results: &[crate::alerts::engine::AlertCheckResult]) {
    use std::collections::BTreeMap;

    // Separate triggered and armed alerts
    let triggered: Vec<_> = results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Triggered)
        .collect();

    let armed_near: Vec<_> = results
        .iter()
        .filter(|r| {
            r.rule.status == AlertStatus::Armed
                && r.distance_pct.is_some()
                && r.distance_pct.unwrap().abs() <= dec!(5) // Within 5%
        })
        .collect();

    if triggered.is_empty() && armed_near.is_empty() {
        return; // No alerts to show
    }

    println!("## Alerts\n");

    // Deduplicate triggered alerts by symbol — keep the most recent per symbol
    if !triggered.is_empty() {
        // Group by symbol, keeping the most recent (highest id) per symbol
        let mut by_symbol: BTreeMap<&str, (usize, &crate::alerts::engine::AlertCheckResult)> =
            BTreeMap::new();
        for result in &triggered {
            let entry = by_symbol
                .entry(result.rule.symbol.as_str())
                .or_insert((0, result));
            entry.0 += 1;
            // Keep the alert with the highest id (most recent)
            if result.rule.id > entry.1.rule.id {
                entry.1 = result;
            }
        }

        for (count, result) in by_symbol.values() {
            let current = result
                .current_value
                .map(|v| v.round_dp(2).to_string())
                .unwrap_or_else(|| "N/A".to_string());
            if *count > 1 {
                println!(
                    "🔴 **TRIGGERED** — {} (current: {}) (+{} more)",
                    result.rule.rule_text,
                    current,
                    count - 1
                );
            } else {
                println!(
                    "🔴 **TRIGGERED** — {} (current: {})",
                    result.rule.rule_text, current
                );
            }
        }
    }

    // Show near-threshold armed alerts (no dedup needed — armed alerts don't accumulate)
    if !armed_near.is_empty() {
        for result in armed_near {
            let distance = result.distance_pct.unwrap().abs().round_dp(1);
            let current = result
                .current_value
                .map(|v| v.round_dp(2).to_string())
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "🟡 **NEAR** — {} (current: {}, {}% away)",
                result.rule.rule_text, current, distance
            );
        }
    }

    println!();
}

fn print_alerts(conn: &Connection) {
    use crate::alerts::engine::check_alerts;

    let results = match check_alerts(conn) {
        Ok(r) => r,
        Err(_) => return, // Silently skip if check fails
    };

    render_alerts_markdown(&results);
}

fn print_alerts_backend(backend: &BackendConnection) {
    let results = match crate::alerts::engine::check_alerts_backend_only(backend) {
        Ok(r) => r,
        Err(_) => return,
    };

    render_alerts_markdown(&results);
}

fn print_pnl_attribution(positions: &[Position], hist_1d: &HashMap<String, Decimal>, base: &str) {
    let mut contributions: Vec<(&str, Decimal)> = Vec::new(); // (symbol, dollar_pnl)

    for pos in positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(p) => p,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(p) => *p,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        let pnl = (current - prev) * pos.quantity;
        contributions.push((&pos.symbol, pnl));
    }

    if contributions.is_empty() {
        return;
    }

    // Sort by absolute dollar contribution descending
    contributions.sort_by(|a, b| {
        b.1.abs()
            .partial_cmp(&a.1.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("## P&L Attribution (1D)\n");

    // Show top 5 contributors by absolute dollar amount
    let count = contributions.len().min(5);
    for (symbol, pnl) in &contributions[..count] {
        let sign = if *pnl >= dec!(0) { "+" } else { "" };
        let name = resolve_name(symbol);
        let label = if name.is_empty() {
            symbol.to_string()
        } else {
            format!("{} ({})", symbol, name)
        };
        println!("- **{}**: {}{} {}", label, sign, fmt_commas(*pnl, 2), base,);
    }
    println!();
}

fn print_position_table_full(
    positions: &[Position],
    base: &str,
    hist_1d: &HashMap<String, Decimal>,
) {
    println!("## Positions\n");
    println!("| Symbol | Category | Qty | Price | Value | Gain | 1D | Alloc |");
    println!("|--------|----------|----:|------:|------:|-----:|---:|------:|");

    for pos in positions {
        let name = resolve_name(&pos.symbol);
        let currency_prefix = if let Some(ref curr) = pos.native_currency {
            let symbol = match curr.as_str() {
                "GBP" => "£",
                "EUR" => "€",
                "JPY" => "¥",
                "CAD" => "C$",
                "AUD" => "A$",
                "CHF" => "₣",
                _ => curr.as_str(),
            };
            format!("[{}] ", symbol)
        } else {
            String::new()
        };
        let symbol_display = if name.is_empty() {
            format!("{}{}", currency_prefix, pos.symbol)
        } else {
            format!("{}{} ({})", currency_prefix, pos.symbol, name)
        };
        let price_str = pos
            .current_price
            .map(|p| fmt_currency(p, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let value_str = pos
            .current_value
            .map(|v| fmt_currency(v, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let gain_str = pos
            .gain_pct
            .map(|g| format!("{:+.1}%", g))
            .unwrap_or_else(|| "—".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "—".to_string());

        // 1D change
        let day_str = if pos.category == AssetCategory::Cash {
            "—".to_string()
        } else {
            match (pos.current_price, hist_1d.get(&pos.symbol)) {
                (Some(current), Some(prev)) if *prev > dec!(0) => {
                    let pct = ((current - prev) / prev) * dec!(100);
                    format!("{:+.1}%", pct)
                }
                _ => "—".to_string(),
            }
        };

        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            symbol_display,
            pos.category,
            pos.quantity,
            price_str,
            value_str,
            gain_str,
            day_str,
            alloc_str,
        );
    }
}

fn format_category(cat: &AssetCategory) -> &'static str {
    match cat {
        AssetCategory::Equity => "Equity",
        AssetCategory::Crypto => "Crypto",
        AssetCategory::Forex => "Forex",
        AssetCategory::Cash => "Cash",
        AssetCategory::Commodity => "Commodity",
        AssetCategory::Fund => "Fund",
    }
}

// ── Scenario + Calibration helpers ──────────────────────────────────

fn build_scenarios_json_sqlite(conn: &Connection) -> Vec<ScenarioSummaryJson> {
    let scenarios = crate::db::scenarios::list_scenarios(conn, Some("active")).unwrap_or_default();
    scenarios_to_summary(scenarios)
}

fn build_scenarios_json_backend(backend: &BackendConnection) -> Vec<ScenarioSummaryJson> {
    let scenarios =
        crate::db::scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    scenarios_to_summary(scenarios)
}

fn scenarios_to_summary(scenarios: Vec<Scenario>) -> Vec<ScenarioSummaryJson> {
    let mut out: Vec<ScenarioSummaryJson> = scenarios
        .into_iter()
        .map(|s| ScenarioSummaryJson {
            name: s.name,
            probability: s.probability,
            phase: s.phase,
            description: s.description,
            updated_at: s.updated_at,
        })
        .collect();
    // Sort by probability descending for quick scanning
    out.sort_by(|a, b| {
        b.probability
            .partial_cmp(&a.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn build_calibration_json_backend(backend: &BackendConnection) -> Option<CalibrationSummaryJson> {
    let mappings = scenario_contract_mappings::list_enriched_backend(backend).ok()?;
    build_calibration_from_mappings(&mappings)
}

fn build_calibration_json_sqlite(conn: &Connection) -> Option<CalibrationSummaryJson> {
    let mappings = scenario_contract_mappings::list_enriched(conn).ok()?;
    build_calibration_from_mappings(&mappings)
}

fn build_calibration_from_mappings(
    mappings: &[scenario_contract_mappings::EnrichedMapping],
) -> Option<CalibrationSummaryJson> {
    if mappings.is_empty() {
        return None;
    }
    let threshold = 15.0_f64;
    let mut entries: Vec<CalibrationEntryJson> = mappings
        .iter()
        .map(|m| {
            let scenario_pct = m.scenario_probability;
            let market_pct = m.contract_probability * 100.0;
            let divergence = scenario_pct - market_pct;
            let abs_div = divergence.abs();
            CalibrationEntryJson {
                scenario_name: m.scenario_name.clone(),
                scenario_pct,
                market_pct: (market_pct * 100.0).round() / 100.0,
                divergence_pp: (divergence * 100.0).round() / 100.0,
                significant: abs_div > threshold,
            }
        })
        .collect();
    // Sort by absolute divergence descending
    entries.sort_by(|a, b| {
        b.divergence_pp
            .abs()
            .partial_cmp(&a.divergence_pp.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let significant_count = entries.iter().filter(|e| e.significant).count();
    let abs_divs: Vec<f64> = entries.iter().map(|e| e.divergence_pp.abs()).collect();
    let mean_abs = if abs_divs.is_empty() {
        0.0
    } else {
        ((abs_divs.iter().sum::<f64>() / abs_divs.len() as f64) * 100.0).round() / 100.0
    };

    Some(CalibrationSummaryJson {
        total_mappings: entries.len(),
        significant_divergences: significant_count,
        mean_abs_divergence_pp: mean_abs,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::asset::AssetCategory;

    #[test]
    fn fmt_commas_basic() {
        assert_eq!(fmt_commas(dec!(1234567.89), 2), "1,234,567.89");
    }

    #[test]
    fn fmt_commas_small() {
        assert_eq!(fmt_commas(dec!(42.50), 2), "42.50");
    }

    #[test]
    fn fmt_commas_negative() {
        assert_eq!(fmt_commas(dec!(-1234.56), 2), "-1,234.56");
    }

    #[test]
    fn fmt_commas_zero() {
        assert_eq!(fmt_commas(dec!(0), 2), "0.00");
    }

    #[test]
    fn fmt_currency_usd() {
        assert_eq!(fmt_currency(dec!(1234.56), 2, "USD"), "$1,234.56");
    }

    #[test]
    fn fmt_currency_gbp() {
        assert_eq!(fmt_currency(dec!(1234.56), 2, "GBP"), "£1,234.56");
    }

    #[test]
    fn fmt_currency_eur() {
        assert_eq!(fmt_currency(dec!(500.00), 2, "EUR"), "€500.00");
    }

    #[test]
    fn fmt_currency_unknown() {
        // Unknown currencies use the code as prefix
        assert_eq!(fmt_currency(dec!(100.00), 2, "XYZ"), "XYZ100.00");
    }

    #[test]
    fn pct_change_positive() {
        let result = pct_change(dec!(110), dec!(100));
        assert_eq!(result, Some(dec!(10)));
    }

    #[test]
    fn pct_change_negative() {
        let result = pct_change(dec!(90), dec!(100));
        assert_eq!(result, Some(dec!(-10)));
    }

    #[test]
    fn pct_change_zero_base() {
        let result = pct_change(dec!(100), dec!(0));
        assert_eq!(result, None);
    }

    #[test]
    fn pct_change_rejects_anomalous_values() {
        // Corrupt previous close near zero → absurd percentage (e.g. 224,632%)
        let result = pct_change(dec!(84000), dec!(37));
        assert_eq!(
            result, None,
            "Should reject >500% change as implausible data"
        );
    }

    #[test]
    fn pct_change_allows_legitimate_extreme() {
        // -50% crash is legitimate
        let result = pct_change(dec!(50), dec!(100));
        assert_eq!(result, Some(dec!(-50)));
    }

    #[test]
    fn brief_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let result = run_internal(&conn, &config, false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_with_positions_no_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let result = run_internal(&conn, &config, false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_with_positions_and_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::price_cache::upsert_price;
        use crate::db::transactions::insert_transaction;
        use crate::models::price::PriceQuote;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(30000),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(200),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(85000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let result = run_internal(&conn, &config, false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_percentage_mode() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };

        use crate::db::allocations::insert_allocation;
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(50)).unwrap();
        insert_allocation(&conn, "GC=F", AssetCategory::Commodity, dec!(50)).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(85000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".to_string(),
                price: dec!(2500),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let result = run_internal(&conn, &config, false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_percentage_mode_no_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };

        use crate::db::allocations::insert_allocation;
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(50)).unwrap();

        let result = run_internal(&conn, &config, false, false, false);
        assert!(result.is_ok());
    }

    fn make_position(
        symbol: &str,
        category: AssetCategory,
        qty: Decimal,
        avg_cost: Decimal,
        current_price: Option<Decimal>,
        total_value_for_alloc: Option<Decimal>,
    ) -> Position {
        let total_cost = qty * avg_cost;
        let current_value = current_price.map(|p| p * qty);
        let gain = current_value.map(|v| v - total_cost);
        let gain_pct = if total_cost > dec!(0) {
            gain.map(|g| (g / total_cost) * dec!(100))
        } else {
            None
        };
        let allocation_pct = match (current_value, total_value_for_alloc) {
            (Some(v), Some(tv)) if tv > dec!(0) => Some((v / tv) * dec!(100)),
            _ => None,
        };
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: qty,
            avg_cost,
            total_cost,
            currency: "USD".to_string(),
            current_price,
            current_value,
            gain,
            gain_pct,
            allocation_pct,
            native_currency: None,
            fx_rate: None,
        }
    }

    #[test]
    fn top_movers_sorts_by_absolute_change() {
        let positions = vec![
            make_position(
                "AAPL",
                AssetCategory::Equity,
                dec!(10),
                dec!(150),
                Some(dec!(200)),
                Some(dec!(100000)),
            ),
            make_position(
                "GOOG",
                AssetCategory::Equity,
                dec!(5),
                dec!(100),
                Some(dec!(90)),
                Some(dec!(100000)),
            ),
            make_position(
                "BTC",
                AssetCategory::Crypto,
                dec!(1),
                dec!(30000),
                Some(dec!(85000)),
                Some(dec!(100000)),
            ),
        ];

        let mut hist_1d: HashMap<String, Decimal> = HashMap::new();
        hist_1d.insert("AAPL".to_string(), dec!(195));
        hist_1d.insert("GOOG".to_string(), dec!(100));
        hist_1d.insert("BTC".to_string(), dec!(83000));

        // Verify it doesn't panic — output goes to stdout
        print_top_movers(&positions, &hist_1d, "USD");
    }

    #[test]
    fn market_movers_exclude_held_symbols() {
        let positions = vec![
            make_position(
                "AAPL",
                AssetCategory::Equity,
                dec!(10),
                dec!(100),
                Some(dec!(110)),
                Some(dec!(10000)),
            ),
            make_position(
                "BTC",
                AssetCategory::Crypto,
                dec!(1),
                dec!(80000),
                Some(dec!(85000)),
                Some(dec!(10000)),
            ),
        ];

        let watchlist_symbols = vec!["NVDA".to_string(), "AAPL".to_string(), "TSLA".to_string()];
        let mut prices = HashMap::new();
        prices.insert("AAPL".to_string(), dec!(110));
        prices.insert("NVDA".to_string(), dec!(120));
        prices.insert("TSLA".to_string(), dec!(180));

        let mut hist_1d = HashMap::new();
        hist_1d.insert("AAPL".to_string(), dec!(100));
        hist_1d.insert("NVDA".to_string(), dec!(100));
        hist_1d.insert("TSLA".to_string(), dec!(200));

        let empty_signals: HashMap<String, Vec<String>> = HashMap::new();
        let movers = get_market_movers_json(
            &positions,
            &watchlist_symbols,
            &prices,
            &hist_1d,
            &empty_signals,
        );
        assert!(!movers.iter().any(|m| m.symbol == "AAPL"));
        assert!(movers.iter().any(|m| m.symbol == "NVDA"));
        assert!(movers.iter().any(|m| m.symbol == "TSLA"));
    }

    #[test]
    fn market_movers_sorted_by_absolute_change() {
        let positions = vec![make_position(
            "AAPL",
            AssetCategory::Equity,
            dec!(1),
            dec!(100),
            Some(dec!(100)),
            Some(dec!(1000)),
        )];

        let watchlist_symbols = vec![
            "NVDA".to_string(),
            "TSLA".to_string(),
            "XLE".to_string(),
            "SPY".to_string(),
        ];
        let mut prices = HashMap::new();
        prices.insert("NVDA".to_string(), dec!(130)); // +30%
        prices.insert("TSLA".to_string(), dec!(75)); // -25%
        prices.insert("XLE".to_string(), dec!(105)); // +5%
        prices.insert("SPY".to_string(), dec!(97)); // -3%

        let mut hist_1d = HashMap::new();
        hist_1d.insert("NVDA".to_string(), dec!(100));
        hist_1d.insert("TSLA".to_string(), dec!(100));
        hist_1d.insert("XLE".to_string(), dec!(100));
        hist_1d.insert("SPY".to_string(), dec!(100));

        let empty_signals: HashMap<String, Vec<String>> = HashMap::new();
        let movers = get_market_movers_json(
            &positions,
            &watchlist_symbols,
            &prices,
            &hist_1d,
            &empty_signals,
        );
        assert_eq!(movers.first().map(|m| m.symbol.as_str()), Some("NVDA"));
        assert_eq!(movers.get(1).map(|m| m.symbol.as_str()), Some("TSLA"));
    }

    #[test]
    fn category_allocation_groups_correctly() {
        let positions = vec![
            make_position(
                "AAPL",
                AssetCategory::Equity,
                dec!(10),
                dec!(100),
                Some(dec!(150)),
                Some(dec!(2600)),
            ),
            make_position(
                "GOOG",
                AssetCategory::Equity,
                dec!(5),
                dec!(100),
                Some(dec!(120)),
                Some(dec!(2600)),
            ),
            make_position(
                "BTC",
                AssetCategory::Crypto,
                dec!(1),
                dec!(500),
                Some(dec!(1000)),
                Some(dec!(2600)),
            ),
        ];

        // Verify it doesn't panic — output goes to stdout
        print_category_allocation(&positions, dec!(2600));
    }

    #[test]
    fn technicals_section_skips_cash_positions() {
        let positions = vec![
            make_position(
                "AAPL",
                AssetCategory::Equity,
                dec!(10),
                dec!(150),
                Some(dec!(200)),
                Some(dec!(100000)),
            ),
            make_position(
                "USD",
                AssetCategory::Cash,
                dec!(50000),
                dec!(1),
                Some(dec!(1)),
                Some(dec!(100000)),
            ),
        ];

        let mut technicals = HashMap::new();
        technicals.insert(
            "AAPL".to_string(),
            TechnicalSnapshot {
                symbol: "AAPL".to_string(),
                timeframe: DEFAULT_TIMEFRAME.to_string(),
                rsi_14: Some(55.0),
                macd: Some(1.5),
                macd_signal: Some(1.0),
                macd_histogram: Some(0.5),
                sma_20: Some(195.0),
                sma_50: Some(190.0),
                sma_200: Some(175.0),
                bollinger_upper: None,
                bollinger_middle: None,
                bollinger_lower: None,
                range_52w_low: None,
                range_52w_high: None,
                range_52w_position: None,
                volume_avg_20: None,
                volume_ratio_20: None,
                volume_regime: None,
                above_sma_20: None,
                above_sma_50: None,
                above_sma_200: None,
                atr_14: None,
                atr_ratio: None,
                range_expansion: None,
                day_range_ratio: None,
                computed_at: "2026-03-17T00:00:00Z".to_string(),
            },
        );

        // Should not panic and should skip USD
        print_technicals_section(&positions, &technicals);
    }

    #[test]
    fn technicals_section_empty_data_produces_no_output() {
        let positions = vec![make_position(
            "AAPL",
            AssetCategory::Equity,
            dec!(10),
            dec!(150),
            Some(dec!(200)),
            Some(dec!(100000)),
        )];

        let technicals: HashMap<String, TechnicalSnapshot> = HashMap::new();

        // Should not produce output when no technicals data
        print_technicals_section(&positions, &technicals);
    }

    #[test]
    fn rsi_label_categories() {
        assert_eq!(rsi_label(75.0), "overbought");
        assert_eq!(rsi_label(70.0), "overbought");
        assert_eq!(rsi_label(25.0), "oversold");
        assert_eq!(rsi_label(30.0), "oversold");
        assert_eq!(rsi_label(50.0), "neutral");
    }

    #[test]
    fn macd_label_categories() {
        assert_eq!(macd_label(0.5), "bullish");
        assert_eq!(macd_label(-0.5), "bearish");
        assert_eq!(macd_label(0.0), "neutral");
    }

    #[test]
    fn brief_with_technicals_flag() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::price_cache::upsert_price;
        use crate::db::transactions::insert_transaction;
        use crate::models::price::PriceQuote;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(200),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        // With technicals=true, should succeed (no history means no indicators displayed)
        let result = run_internal(&conn, &config, true, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn build_signal_map_groups_by_symbol() {
        let signals = vec![
            crate::db::technical_signals::TechnicalSignalRecord {
                id: 1,
                symbol: "BTC-USD".to_string(),
                signal_type: "rsi_oversold".to_string(),
                direction: "bullish".to_string(),
                severity: "notable".to_string(),
                trigger_price: Some(60000.0),
                description: "RSI(14) at 28.5 — oversold".to_string(),
                timeframe: "1d".to_string(),
                detected_at: "2026-03-19T08:00:00Z".to_string(),
            },
            crate::db::technical_signals::TechnicalSignalRecord {
                id: 2,
                symbol: "BTC-USD".to_string(),
                signal_type: "bb_squeeze".to_string(),
                direction: "neutral".to_string(),
                severity: "notable".to_string(),
                trigger_price: None,
                description: "Bollinger Band squeeze — width 3.2%".to_string(),
                timeframe: "1d".to_string(),
                detected_at: "2026-03-19T08:00:00Z".to_string(),
            },
            crate::db::technical_signals::TechnicalSignalRecord {
                id: 3,
                symbol: "GC=F".to_string(),
                signal_type: "52w_high".to_string(),
                direction: "bullish".to_string(),
                severity: "critical".to_string(),
                trigger_price: Some(3050.0),
                description: "Within 1% of 52-week high".to_string(),
                timeframe: "1d".to_string(),
                detected_at: "2026-03-19T08:00:00Z".to_string(),
            },
        ];
        let map = build_signal_map(&signals);
        assert_eq!(map.get("BTC-USD").map(|v| v.len()), Some(2));
        assert_eq!(map.get("GC=F").map(|v| v.len()), Some(1));
        assert!(!map.contains_key("AAPL"));
    }

    #[test]
    fn movers_include_signals_when_present() {
        let positions = vec![make_position(
            "BTC-USD",
            AssetCategory::Crypto,
            dec!(1),
            dec!(60000),
            Some(dec!(66000)),
            Some(dec!(1000)),
        )];
        let mut hist_1d = HashMap::new();
        hist_1d.insert("BTC-USD".to_string(), dec!(60000));

        let mut signal_map: HashMap<String, Vec<String>> = HashMap::new();
        signal_map.insert(
            "BTC-USD".to_string(),
            vec!["RSI(14) at 28.5 — oversold".to_string()],
        );

        let movers = get_movers_json(&positions, &hist_1d, &signal_map);
        assert_eq!(movers.len(), 1);
        assert_eq!(movers[0].signals.len(), 1);
        assert_eq!(movers[0].signals[0], "RSI(14) at 28.5 — oversold");
    }

    #[test]
    fn movers_omit_signals_when_absent() {
        let positions = vec![make_position(
            "AAPL",
            AssetCategory::Equity,
            dec!(10),
            dec!(100),
            Some(dec!(110)),
            Some(dec!(1000)),
        )];
        let mut hist_1d = HashMap::new();
        hist_1d.insert("AAPL".to_string(), dec!(100));

        let empty_signals: HashMap<String, Vec<String>> = HashMap::new();
        let movers = get_movers_json(&positions, &hist_1d, &empty_signals);
        assert_eq!(movers.len(), 1);
        assert!(movers[0].signals.is_empty());
    }

    #[test]
    fn signals_to_json_serializes_correctly() {
        let signals = vec![crate::db::technical_signals::TechnicalSignalRecord {
            id: 1,
            symbol: "GC=F".to_string(),
            signal_type: "52w_high".to_string(),
            direction: "bullish".to_string(),
            severity: "critical".to_string(),
            trigger_price: Some(3050.0),
            description: "Within 1% of 52-week high".to_string(),
            timeframe: "1d".to_string(),
            detected_at: "2026-03-19T08:00:00Z".to_string(),
        }];
        let json_values = signals_to_json(&signals);
        assert_eq!(json_values.len(), 1);
        assert_eq!(json_values[0]["symbol"], "GC=F");
        assert_eq!(json_values[0]["type"], "52w_high");
        assert_eq!(json_values[0]["direction"], "bullish");
        assert_eq!(json_values[0]["severity"], "critical");
    }

    #[test]
    fn overnight_changes_includes_held_positions() {
        let positions = vec![
            make_position(
                "BTC",
                AssetCategory::Crypto,
                dec!(1),
                dec!(80000),
                Some(dec!(85000)),
                Some(dec!(85000)),
            ),
            make_position(
                "AAPL",
                AssetCategory::Equity,
                dec!(10),
                dec!(150),
                Some(dec!(160)),
                Some(dec!(1600)),
            ),
        ];
        let watchlist_symbols = vec![];
        let prices = HashMap::new();
        let mut hist_1d = HashMap::new();
        hist_1d.insert("BTC".to_string(), dec!(83000));
        hist_1d.insert("AAPL".to_string(), dec!(155));

        let changes = build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| c.symbol == "BTC" && c.source == "held"));
        assert!(changes.iter().any(|c| c.symbol == "AAPL" && c.source == "held"));
    }

    #[test]
    fn overnight_changes_skips_cash() {
        let positions = vec![make_position(
            "USD",
            AssetCategory::Cash,
            dec!(10000),
            dec!(1),
            Some(dec!(1)),
            Some(dec!(10000)),
        )];
        let watchlist_symbols = vec![];
        let prices = HashMap::new();
        let mut hist_1d = HashMap::new();
        hist_1d.insert("USD".to_string(), dec!(1));

        let changes = build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);
        assert!(changes.is_empty());
    }

    #[test]
    fn overnight_changes_includes_watchlist_excludes_held() {
        let positions = vec![make_position(
            "BTC",
            AssetCategory::Crypto,
            dec!(1),
            dec!(80000),
            Some(dec!(85000)),
            Some(dec!(85000)),
        )];
        let watchlist_symbols = vec!["BTC".to_string(), "NVDA".to_string()];
        let mut prices = HashMap::new();
        prices.insert("NVDA".to_string(), dec!(130));
        let mut hist_1d = HashMap::new();
        hist_1d.insert("BTC".to_string(), dec!(83000));
        hist_1d.insert("NVDA".to_string(), dec!(120));

        let changes = build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);
        // BTC from positions, NVDA from watchlist (BTC not duplicated)
        assert_eq!(changes.len(), 2);
        assert_eq!(
            changes.iter().filter(|c| c.symbol == "BTC").count(),
            1,
            "BTC should appear only once (from held)"
        );
        assert!(changes.iter().any(|c| c.symbol == "NVDA" && c.source == "watchlist"));
    }

    #[test]
    fn overnight_changes_sorted_by_abs_pct() {
        let positions = vec![
            make_position(
                "AAPL",
                AssetCategory::Equity,
                dec!(10),
                dec!(150),
                Some(dec!(153)), // +2%
                Some(dec!(1530)),
            ),
            make_position(
                "BTC",
                AssetCategory::Crypto,
                dec!(1),
                dec!(80000),
                Some(dec!(88000)), // +10%
                Some(dec!(88000)),
            ),
            make_position(
                "TSLA",
                AssetCategory::Equity,
                dec!(5),
                dec!(200),
                Some(dec!(190)), // -5%
                Some(dec!(950)),
            ),
        ];
        let watchlist_symbols = vec![];
        let prices = HashMap::new();
        let mut hist_1d = HashMap::new();
        hist_1d.insert("AAPL".to_string(), dec!(150));
        hist_1d.insert("BTC".to_string(), dec!(80000));
        hist_1d.insert("TSLA".to_string(), dec!(200));

        let changes = build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);
        assert_eq!(changes.len(), 3);
        // Sorted by absolute pct: BTC (10%), TSLA (5%), AAPL (2%)
        assert_eq!(changes[0].symbol, "BTC");
        assert_eq!(changes[1].symbol, "TSLA");
        assert_eq!(changes[2].symbol, "AAPL");
    }

    #[test]
    fn overnight_changes_computes_correct_values() {
        let positions = vec![make_position(
            "GC=F",
            AssetCategory::Commodity,
            dec!(10),
            dec!(2000),
            Some(dec!(2050)),
            Some(dec!(20500)),
        )];
        let watchlist_symbols = vec![];
        let prices = HashMap::new();
        let mut hist_1d = HashMap::new();
        hist_1d.insert("GC=F".to_string(), dec!(2000));

        let changes = build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);
        assert_eq!(changes.len(), 1);
        let gold = &changes[0];
        assert_eq!(gold.previous_close, "2000");
        assert_eq!(gold.current_price, "2050");
        assert_eq!(gold.change_abs, "50");
        assert_eq!(gold.change_pct, "2.50");
        assert_eq!(gold.source, "held");
        assert_eq!(gold.category, "Commodity");
    }

    #[test]
    fn overnight_changes_skips_no_history() {
        let positions = vec![make_position(
            "BTC",
            AssetCategory::Crypto,
            dec!(1),
            dec!(80000),
            Some(dec!(85000)),
            Some(dec!(85000)),
        )];
        let watchlist_symbols = vec![];
        let prices = HashMap::new();
        let hist_1d = HashMap::new(); // No history

        let changes = build_overnight_changes(&positions, &watchlist_symbols, &prices, &hist_1d);
        assert!(changes.is_empty());
    }

    #[test]
    fn enrich_hist_1d_uses_cached_previous_close_when_history_missing() {
        let hist_1d = HashMap::new();
        let previous_close = HashMap::from([
            ("GC=F".to_string(), dec!(3225.40)),
            ("SI=F".to_string(), dec!(0)),
        ]);

        let merged = enrich_hist_1d_with_cached_previous_close(&hist_1d, &previous_close);

        assert_eq!(merged.get("GC=F"), Some(&dec!(3225.40)));
        assert!(!merged.contains_key("SI=F"));
    }

    #[test]
    fn enrich_hist_1d_preserves_existing_history_over_cached_previous_close() {
        let hist_1d = HashMap::from([("GC=F".to_string(), dec!(3200.10))]);
        let previous_close = HashMap::from([("GC=F".to_string(), dec!(3225.40))]);

        let merged = enrich_hist_1d_with_cached_previous_close(&hist_1d, &previous_close);

        assert_eq!(merged.get("GC=F"), Some(&dec!(3200.10)));
    }

    // -- Alert deduplication tests --

    use crate::alerts::engine::AlertCheckResult;
    use crate::alerts::{AlertDirection, AlertKind, AlertRule, AlertStatus};

    fn make_triggered_alert(id: i64, symbol: &str, rule_text: &str) -> AlertCheckResult {
        AlertCheckResult {
            rule: AlertRule {
                id,
                kind: AlertKind::Indicator,
                symbol: symbol.to_string(),
                direction: AlertDirection::Above,
                condition: None,
                threshold: "5".to_string(),
                status: AlertStatus::Triggered,
                rule_text: rule_text.to_string(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-03-30T18:00:00Z".to_string(),
                triggered_at: Some("2026-03-30T18:00:00Z".to_string()),
            },
            current_value: Some(dec!(42)),
            newly_triggered: false,
            distance_pct: None,
            trigger_data: serde_json::json!({}),
        }
    }

    fn make_armed_alert(id: i64, symbol: &str, distance: Decimal) -> AlertCheckResult {
        AlertCheckResult {
            rule: AlertRule {
                id,
                kind: AlertKind::Price,
                symbol: symbol.to_string(),
                direction: AlertDirection::Below,
                condition: None,
                threshold: "100".to_string(),
                status: AlertStatus::Armed,
                rule_text: format!("{} below 100", symbol),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-03-30T18:00:00Z".to_string(),
                triggered_at: None,
            },
            current_value: Some(dec!(105)),
            newly_triggered: false,
            distance_pct: Some(distance),
            trigger_data: serde_json::json!({}),
        }
    }

    #[test]
    fn render_alerts_dedup_groups_same_symbol() {
        // 5 triggered alerts for the same SCAN:BIG-LOSERS symbol
        let results = [
            make_triggered_alert(100, "SCAN:BIG-LOSERS", "count changed: 5 -> 4"),
            make_triggered_alert(101, "SCAN:BIG-LOSERS", "count changed: 4 -> 5"),
            make_triggered_alert(102, "SCAN:BIG-LOSERS", "count changed: 5 -> 4"),
            make_triggered_alert(103, "SCAN:BIG-LOSERS", "count changed: 4 -> 5"),
            make_triggered_alert(104, "SCAN:BIG-LOSERS", "count changed: 5 -> 4"),
        ];

        // Verify the dedup logic by checking the BTreeMap grouping
        use std::collections::BTreeMap;
        let triggered: Vec<_> = results
            .iter()
            .filter(|r| r.rule.status == AlertStatus::Triggered)
            .collect();
        let mut by_symbol: BTreeMap<&str, (usize, &AlertCheckResult)> = BTreeMap::new();
        for result in &triggered {
            let entry = by_symbol
                .entry(result.rule.symbol.as_str())
                .or_insert((0, result));
            entry.0 += 1;
            if result.rule.id > entry.1.rule.id {
                entry.1 = result;
            }
        }

        assert_eq!(by_symbol.len(), 1, "should group into 1 entry");
        let (count, latest) = by_symbol.get("SCAN:BIG-LOSERS").unwrap();
        assert_eq!(*count, 5, "should count all 5");
        assert_eq!(latest.rule.id, 104, "should keep the most recent (highest id)");
    }

    #[test]
    fn render_alerts_dedup_keeps_distinct_symbols_separate() {
        let results = [
            make_triggered_alert(100, "SCAN:BIG-LOSERS", "count changed: 5 -> 4"),
            make_triggered_alert(101, "SCAN:BIG-LOSERS", "count changed: 4 -> 5"),
            make_triggered_alert(200, "SCENARIO:WAR", "probability shifted -10pp"),
            make_triggered_alert(300, "GC=F", "gold above 5000"),
        ];

        use std::collections::BTreeMap;
        let triggered: Vec<_> = results
            .iter()
            .filter(|r| r.rule.status == AlertStatus::Triggered)
            .collect();
        let mut by_symbol: BTreeMap<&str, (usize, &AlertCheckResult)> = BTreeMap::new();
        for result in &triggered {
            let entry = by_symbol
                .entry(result.rule.symbol.as_str())
                .or_insert((0, result));
            entry.0 += 1;
            if result.rule.id > entry.1.rule.id {
                entry.1 = result;
            }
        }

        assert_eq!(by_symbol.len(), 3, "3 distinct symbols");
        assert_eq!(by_symbol.get("SCAN:BIG-LOSERS").unwrap().0, 2);
        assert_eq!(by_symbol.get("SCENARIO:WAR").unwrap().0, 1);
        assert_eq!(by_symbol.get("GC=F").unwrap().0, 1);
    }

    #[test]
    fn render_alerts_single_triggered_no_count_suffix() {
        // A single triggered alert should NOT show "+N more"
        let results = [make_triggered_alert(100, "GC=F", "gold above 5000")];

        use std::collections::BTreeMap;
        let triggered: Vec<_> = results
            .iter()
            .filter(|r| r.rule.status == AlertStatus::Triggered)
            .collect();
        let mut by_symbol: BTreeMap<&str, (usize, &AlertCheckResult)> = BTreeMap::new();
        for result in &triggered {
            let entry = by_symbol
                .entry(result.rule.symbol.as_str())
                .or_insert((0, result));
            entry.0 += 1;
            if result.rule.id > entry.1.rule.id {
                entry.1 = result;
            }
        }

        let (count, _latest) = by_symbol.get("GC=F").unwrap();
        assert_eq!(*count, 1, "single alert, no dedup suffix");
    }

    #[test]
    fn render_alerts_empty_results_no_output() {
        // No triggered or near-armed alerts
        let results: [AlertCheckResult; 0] = [];
        // render_alerts_markdown should just return without printing
        // (we test this by checking the filter logic returns empty)
        let triggered: Vec<_> = results
            .iter()
            .filter(|r| r.rule.status == AlertStatus::Triggered)
            .collect();
        let armed_near: Vec<_> = results
            .iter()
            .filter(|r| {
                r.rule.status == AlertStatus::Armed
                    && r.distance_pct.is_some()
                    && r.distance_pct.unwrap().abs() <= dec!(5)
            })
            .collect();
        assert!(triggered.is_empty());
        assert!(armed_near.is_empty());
    }

    #[test]
    fn render_alerts_armed_near_not_deduplicated() {
        // Armed alerts with different symbols should each appear
        let results = [
            make_armed_alert(1, "GC=F", dec!(3)),
            make_armed_alert(2, "BTC", dec!(2)),
            make_armed_alert(3, "SI=F", dec!(4)),
        ];

        let armed_near: Vec<_> = results
            .iter()
            .filter(|r| {
                r.rule.status == AlertStatus::Armed
                    && r.distance_pct.is_some()
                    && r.distance_pct.unwrap().abs() <= dec!(5)
            })
            .collect();

        assert_eq!(armed_near.len(), 3, "all 3 armed alerts within threshold");
    }

    #[test]
    fn render_alerts_armed_beyond_threshold_excluded() {
        let results = [
            make_armed_alert(1, "GC=F", dec!(3)),   // within 5%
            make_armed_alert(2, "BTC", dec!(10)),    // beyond 5%
        ];

        let armed_near: Vec<_> = results
            .iter()
            .filter(|r| {
                r.rule.status == AlertStatus::Armed
                    && r.distance_pct.is_some()
                    && r.distance_pct.unwrap().abs() <= dec!(5)
            })
            .collect();

        assert_eq!(armed_near.len(), 1, "only GC=F within 5% threshold");
        assert_eq!(armed_near[0].rule.symbol, "GC=F");
    }

    #[test]
    fn watchlist_item_json_includes_levels_field() {
        use crate::analytics::levels::{ActionableLevel, ActionableLevelPair};

        let item = WatchlistItemJson {
            symbol: "TSLA".to_string(),
            name: "Tesla Inc".to_string(),
            category: "equity".to_string(),
            current_price: Some("250.00".to_string()),
            daily_change_pct: Some("2.50".to_string()),
            technicals: None,
            levels: Some(ActionableLevelPair {
                support: Some(ActionableLevel {
                    symbol: "TSLA".to_string(),
                    level_type: "support".to_string(),
                    price: 240.0,
                    strength: 0.75,
                    source_method: "swing".to_string(),
                    timeframe: "1d".to_string(),
                    notes: Some("Swing low (tested 2x)".to_string()),
                    computed_at: "2026-04-01".to_string(),
                    distance_pct: 4.0,
                }),
                resistance: Some(ActionableLevel {
                    symbol: "TSLA".to_string(),
                    level_type: "resistance".to_string(),
                    price: 270.0,
                    strength: 0.8,
                    source_method: "swing".to_string(),
                    timeframe: "1d".to_string(),
                    notes: Some("Swing high (tested 1x)".to_string()),
                    computed_at: "2026-04-01".to_string(),
                    distance_pct: 8.0,
                }),
            }),
        };

        let json = serde_json::to_value(&item).unwrap();
        assert!(json.get("levels").is_some(), "levels field should be present when populated");
        let levels = json.get("levels").unwrap();
        assert!(levels.get("support").is_some());
        assert!(levels.get("resistance").is_some());
        assert_eq!(
            levels["support"]["price"].as_f64().unwrap(),
            240.0
        );
        assert_eq!(
            levels["resistance"]["price"].as_f64().unwrap(),
            270.0
        );
    }

    #[test]
    fn watchlist_item_json_omits_levels_when_none() {
        let item = WatchlistItemJson {
            symbol: "AAPL".to_string(),
            name: "Apple Inc".to_string(),
            category: "equity".to_string(),
            current_price: Some("180.00".to_string()),
            daily_change_pct: None,
            technicals: None,
            levels: None,
        };

        let json = serde_json::to_value(&item).unwrap();
        assert!(
            json.get("levels").is_none(),
            "levels field should be omitted when None (skip_serializing_if)"
        );
    }

    #[test]
    fn watchlist_levels_in_brief_json_integration() {
        use crate::db::technical_levels::TechnicalLevelRecord;

        let conn = crate::db::open_in_memory();
        let config = Config::default();

        // Add a watchlist item
        crate::db::watchlist::add_to_watchlist(&conn, "NVDA", AssetCategory::Equity).unwrap();

        // Insert a price
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "NVDA".to_string(),
                price: dec!(800),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2026-04-01T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        // Insert technical levels for the watchlist symbol
        crate::db::technical_levels::upsert_levels(
            &conn,
            "NVDA",
            &[
                TechnicalLevelRecord {
                    id: None,
                    symbol: "NVDA".to_string(),
                    level_type: "support".to_string(),
                    price: 750.0,
                    strength: 0.8,
                    source_method: "swing".to_string(),
                    timeframe: "1d".to_string(),
                    notes: Some("Swing low".to_string()),
                    computed_at: "2026-04-01T00:00:00Z".to_string(),
                },
                TechnicalLevelRecord {
                    id: None,
                    symbol: "NVDA".to_string(),
                    level_type: "resistance".to_string(),
                    price: 850.0,
                    strength: 0.7,
                    source_method: "swing".to_string(),
                    timeframe: "1d".to_string(),
                    notes: Some("Swing high".to_string()),
                    computed_at: "2026-04-01T00:00:00Z".to_string(),
                },
            ],
        )
        .unwrap();

        // Run brief in JSON mode — should succeed with watchlist levels populated
        let result = run_internal(&conn, &config, true, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn agent_brief_scan_highlights_serialized_when_present() {
        let highlight = ScanHighlight {
            symbol: "BTC-USD".to_string(),
            name: "Bitcoin".to_string(),
            scan_type: "big_mover".to_string(),
            detail: "+6.2% daily change".to_string(),
            value_pct: Some(6.2),
            severity: "elevated".to_string(),
        };
        let brief = AgentBrief {
            timestamp: "2026-04-02T10:00:00Z".to_string(),
            portfolio: PortfolioSummaryJson {
                total_value: "100000".to_string(),
                total_cost: "90000".to_string(),
                total_gain: "10000".to_string(),
                total_gain_pct: "11.11".to_string(),
                daily_pnl: None,
                daily_pnl_pct: None,
                base_currency: "USD".to_string(),
            },
            positions: vec![],
            watchlist: vec![],
            movers: vec![],
            market_movers: vec![],
            macro_data: None,
            news_summary: vec![],
            economic_data: vec![],
            predictions: vec![],
            sentiment: None,
            alerts: vec![],
            drift: None,
            regime: None,
            correlations: None,
            timeframe_signal: None,
            technical_signals: vec![],
            overnight_changes: vec![],
            scan_highlights: vec![highlight],
            scenarios: vec![],
            calibration: None,
        };
        let json = serde_json::to_string(&brief).unwrap();
        assert!(json.contains("scan_highlights"), "scan_highlights should appear when non-empty");
        assert!(json.contains("big_mover"));
        assert!(json.contains("BTC-USD"));
    }

    #[test]
    fn agent_brief_scan_highlights_omitted_when_empty() {
        let brief = AgentBrief {
            timestamp: "2026-04-02T10:00:00Z".to_string(),
            portfolio: PortfolioSummaryJson {
                total_value: "100000".to_string(),
                total_cost: "90000".to_string(),
                total_gain: "10000".to_string(),
                total_gain_pct: "11.11".to_string(),
                daily_pnl: None,
                daily_pnl_pct: None,
                base_currency: "USD".to_string(),
            },
            positions: vec![],
            watchlist: vec![],
            movers: vec![],
            market_movers: vec![],
            macro_data: None,
            news_summary: vec![],
            economic_data: vec![],
            predictions: vec![],
            sentiment: None,
            alerts: vec![],
            drift: None,
            regime: None,
            correlations: None,
            timeframe_signal: None,
            technical_signals: vec![],
            overnight_changes: vec![],
            scan_highlights: vec![],
            scenarios: vec![],
            calibration: None,
        };
        let json = serde_json::to_string(&brief).unwrap();
        assert!(
            !json.contains("scan_highlights"),
            "scan_highlights should be omitted from JSON when empty"
        );
        assert!(
            !json.contains("scenarios"),
            "scenarios should be omitted from JSON when empty"
        );
        assert!(
            !json.contains("calibration"),
            "calibration should be omitted from JSON when None"
        );
    }

    #[test]
    fn scenarios_to_summary_sorts_by_probability() {
        let scenarios = vec![
            Scenario {
                id: 1,
                name: "Low Prob".to_string(),
                probability: 15.0,
                description: Some("Low".to_string()),
                asset_impact: None,
                triggers: None,
                historical_precedent: None,
                status: "active".to_string(),
                created_at: "2026-04-01".to_string(),
                updated_at: "2026-04-01".to_string(),
                phase: "hypothesis".to_string(),
                resolved_at: None,
                resolution_notes: None,
            },
            Scenario {
                id: 2,
                name: "High Prob".to_string(),
                probability: 90.0,
                description: Some("High".to_string()),
                asset_impact: None,
                triggers: None,
                historical_precedent: None,
                status: "active".to_string(),
                created_at: "2026-04-01".to_string(),
                updated_at: "2026-04-01".to_string(),
                phase: "active".to_string(),
                resolved_at: None,
                resolution_notes: None,
            },
        ];
        let result = scenarios_to_summary(scenarios);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "High Prob");
        assert_eq!(result[0].probability, 90.0);
        assert_eq!(result[1].name, "Low Prob");
        assert_eq!(result[1].probability, 15.0);
    }

    #[test]
    fn scenarios_to_summary_empty() {
        let result = scenarios_to_summary(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn scenario_summary_json_serialization() {
        let s = ScenarioSummaryJson {
            name: "Test".to_string(),
            probability: 42.0,
            phase: "hypothesis".to_string(),
            description: Some("A test scenario".to_string()),
            updated_at: "2026-04-02".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"probability\":42.0"));
        assert!(json.contains("\"phase\":\"hypothesis\""));
    }

    #[test]
    fn calibration_from_mappings_empty() {
        let result = build_calibration_from_mappings(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn calibration_from_mappings_with_data() {
        let mappings = vec![
            scenario_contract_mappings::EnrichedMapping {
                mapping_id: 1,
                scenario_id: 1,
                scenario_name: "Recession".to_string(),
                scenario_probability: 80.0,
                contract_id: "c1".to_string(),
                contract_question: "Will there be a recession?".to_string(),
                contract_category: "economics".to_string(),
                contract_probability: 0.40, // 40%
                divergence_pp: 40.0,
            },
            scenario_contract_mappings::EnrichedMapping {
                mapping_id: 2,
                scenario_id: 2,
                scenario_name: "Rate Cut".to_string(),
                scenario_probability: 50.0,
                contract_id: "c2".to_string(),
                contract_question: "Will Fed cut?".to_string(),
                contract_category: "economics".to_string(),
                contract_probability: 0.48, // 48%
                divergence_pp: 2.0,
            },
        ];
        let result = build_calibration_from_mappings(&mappings).unwrap();
        assert_eq!(result.total_mappings, 2);
        assert_eq!(result.significant_divergences, 1); // 80-40=40pp is > 15
        // Sorted by abs divergence desc: Recession (40pp) before Rate Cut (2pp)
        assert_eq!(result.entries[0].scenario_name, "Recession");
        assert!(result.entries[0].significant);
        assert_eq!(result.entries[1].scenario_name, "Rate Cut");
        assert!(!result.entries[1].significant);
    }

    #[test]
    fn calibration_entry_json_divergence_sign() {
        let mappings = vec![scenario_contract_mappings::EnrichedMapping {
            mapping_id: 1,
            scenario_id: 1,
            scenario_name: "Underestimate".to_string(),
            scenario_probability: 20.0,
            contract_id: "c1".to_string(),
            contract_question: "Test?".to_string(),
            contract_category: "economics".to_string(),
            contract_probability: 0.60, // 60% → divergence = 20 - 60 = -40pp
            divergence_pp: -40.0,
        }];
        let result = build_calibration_from_mappings(&mappings).unwrap();
        assert!(result.entries[0].divergence_pp < 0.0);
        assert!(result.entries[0].significant);
    }

    #[test]
    fn correlation_break_json_includes_severity() {
        let summary = CorrelationSummary {
            top_pairs: vec![],
            active_breaks: vec![CorrelationBreak {
                symbol_a: "BTC-USD".to_string(),
                symbol_b: "GC=F".to_string(),
                corr_7d: 0.85,
                corr_90d: 0.10,
                delta: 0.75,
            }],
        };
        let json = correlation_summary_to_json(&summary).unwrap();
        let breaks = json["active_breaks"].as_array().unwrap();
        assert_eq!(breaks.len(), 1);
        let brk = &breaks[0];
        assert!(brk["severity"].is_string());
        assert!(brk["interpretation"].is_string());
        assert!(brk["signal"].is_string());
        // delta 0.75 => severe
        assert_eq!(brk["severity"].as_str().unwrap(), "severe");
    }

    #[test]
    fn correlation_break_json_moderate_severity() {
        let summary = CorrelationSummary {
            top_pairs: vec![],
            active_breaks: vec![CorrelationBreak {
                symbol_a: "^GSPC".to_string(),
                symbol_b: "DX-Y.NYB".to_string(),
                corr_7d: 0.20,
                corr_90d: -0.35,
                delta: 0.55,
            }],
        };
        let json = correlation_summary_to_json(&summary).unwrap();
        let brk = &json["active_breaks"][0];
        assert_eq!(brk["severity"].as_str().unwrap(), "moderate");
        // Interpretation should be a non-empty string
        assert!(!brk["interpretation"].as_str().unwrap().is_empty());
    }

    #[test]
    fn correlation_break_json_minor_severity() {
        let summary = CorrelationSummary {
            top_pairs: vec![],
            active_breaks: vec![CorrelationBreak {
                symbol_a: "TSLA".to_string(),
                symbol_b: "NVDA".to_string(),
                corr_7d: 0.60,
                corr_90d: 0.25,
                delta: 0.35,
            }],
        };
        let json = correlation_summary_to_json(&summary).unwrap();
        let brk = &json["active_breaks"][0];
        assert_eq!(brk["severity"].as_str().unwrap(), "minor");
    }

    #[test]
    fn correlation_break_json_preserves_existing_fields() {
        let summary = CorrelationSummary {
            top_pairs: vec![CorrelationPair {
                symbol_a: "BTC-USD".to_string(),
                symbol_b: "GC=F".to_string(),
                corr_30d: 0.42,
            }],
            active_breaks: vec![CorrelationBreak {
                symbol_a: "SI=F".to_string(),
                symbol_b: "GC=F".to_string(),
                corr_7d: 0.90,
                corr_90d: 0.20,
                delta: 0.70,
            }],
        };
        let json = correlation_summary_to_json(&summary).unwrap();
        // top_pairs still has corr_30d
        let pair = &json["top_pairs_30d"][0];
        assert_eq!(pair["symbol_a"].as_str().unwrap(), "BTC-USD");
        assert!((pair["corr_30d"].as_f64().unwrap() - 0.42).abs() < 0.01);
        // breaks still have the original fields
        let brk = &json["active_breaks"][0];
        assert_eq!(brk["symbol_a"].as_str().unwrap(), "SI=F");
        assert!((brk["corr_7d"].as_f64().unwrap() - 0.90).abs() < 0.01);
        assert!((brk["corr_90d"].as_f64().unwrap() - 0.20).abs() < 0.01);
        assert!((brk["delta"].as_f64().unwrap() - 0.70).abs() < 0.01);
        // AND has the new enrichment fields
        assert!(brk["severity"].is_string());
        assert!(brk["interpretation"].is_string());
        assert!(brk["signal"].is_string());
    }

    #[test]
    fn to_correlations_break_maps_fields_correctly() {
        let brief_break = CorrelationBreak {
            symbol_a: "BTC-USD".to_string(),
            symbol_b: "GC=F".to_string(),
            corr_7d: 0.85,
            corr_90d: 0.10,
            delta: 0.75,
        };
        let corr_break = to_correlations_break(&brief_break);
        assert_eq!(corr_break.symbol_a, "BTC-USD");
        assert_eq!(corr_break.symbol_b, "GC=F");
        assert_eq!(corr_break.corr_7d, Some(0.85));
        assert_eq!(corr_break.corr_90d, Some(0.10));
        assert!((corr_break.break_delta - 0.75).abs() < f64::EPSILON);
    }
}
