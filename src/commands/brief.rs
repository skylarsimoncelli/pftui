use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;
use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::analytics::risk;
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::economic_cache;
use crate::db::price_cache::{get_all_cached_prices, get_cached_price};
use crate::db::price_history::{get_history, get_prices_at_date};
use crate::db::snapshots::get_all_portfolio_snapshots;
use crate::db::transactions::list_transactions;
use crate::indicators::macd::{compute_macd, MacdResult};
use crate::indicators::rsi::compute_rsi;
use crate::indicators::sma::compute_sma;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

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
}

#[derive(Serialize)]
struct MoverJson {
    symbol: String,
    name: String,
    daily_change_pct: String,
    daily_change_abs: String,
}

/// Agent mode: single comprehensive JSON blob
fn run_agent_mode(conn: &Connection, config: &Config) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = get_prices_at_date(conn, &symbols, &yesterday_str).unwrap_or_default();

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
        daily_pnl: if has_daily { Some(daily_pnl.to_string()) } else { None },
        daily_pnl_pct: daily_pnl_pct.map(|p| p.round_dp(2).to_string()),
        base_currency: base.to_string(),
    };

    // Positions with technicals
    let positions_json: Vec<PositionJson> = positions
        .iter()
        .map(|pos| {
            let daily_change = if let (Some(current), Some(&prev)) = (pos.current_price, hist_1d.get(&pos.symbol)) {
                if prev > dec!(0) {
                    Some((current - prev) * pos.quantity)
                } else {
                    None
                }
            } else {
                None
            };

            let daily_change_pct = if let (Some(current), Some(&prev)) = (pos.current_price, hist_1d.get(&pos.symbol)) {
                pct_change(current, prev)
            } else {
                None
            };

            let allocation_pct = if total_value > dec!(0) {
                pos.current_value.map(|v| ((v / total_value) * dec!(100)).round_dp(2))
            } else {
                None
            };

            let technicals_json = technicals_data.get(&pos.symbol).map(|t| TechnicalSnapshotJson {
                rsi: t.rsi_14.map(|v| format!("{:.1}", v)),
                rsi_signal: t.rsi_14.map(|v| {
                    if v > 70.0 { "overbought".to_string() }
                    else if v < 30.0 { "oversold".to_string() }
                    else { "neutral".to_string() }
                }),
                macd: t.macd.as_ref().map(|m| format!("{:.4}", m.macd)),
                macd_signal: t.macd.as_ref().map(|m| format!("{:.4}", m.signal)),
                macd_histogram: t.macd.as_ref().map(|m| format!("{:.4}", m.histogram)),
                sma_20: None, // Not available in current TechnicalSnapshot
                sma_50: t.sma_50.map(|v| format!("{:.2}", v)),
            });

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
            }
        })
        .collect();

    // Watchlist
    let watchlist_json = get_watchlist_json(conn, &prices, &hist_1d, &technicals_data)?;

    // Top movers
    let movers_json = get_movers_json(&positions, &hist_1d);

    // Macro data (if available)
    let macro_data = get_macro_json(conn).ok();

    // Alerts (if available)
    let alerts_json = get_alerts_json(conn);

    // Drift (if available)
    let drift_json = get_drift_json(conn).ok();

    // Regime (if available)
    let regime_json = get_regime_json(conn).ok();
    let news_summary = get_news_summary_json(conn).unwrap_or_default();
    let economic_data = get_economic_data_json(conn).unwrap_or_default();
    let predictions = get_predictions_json(conn).unwrap_or_default();
    let sentiment = get_sentiment_json(conn).ok();

    let brief = AgentBrief {
        timestamp,
        portfolio: portfolio_summary,
        positions: positions_json,
        watchlist: watchlist_json,
        movers: movers_json,
        macro_data,
        news_summary,
        economic_data,
        predictions,
        sentiment,
        alerts: alerts_json,
        drift: drift_json,
        regime: regime_json,
    };

    let json = serde_json::to_string_pretty(&brief)?;
    println!("{}", json);
    Ok(())
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
            let daily_change_pct = if let (Some(current), Some(&prev)) = (current_price, hist_1d.get(&w.symbol)) {
                pct_change(current, prev)
            } else {
                None
            };

            let technicals_json = technicals_data.get(&w.symbol).map(|t| TechnicalSnapshotJson {
                rsi: t.rsi_14.map(|v| format!("{:.1}", v)),
                rsi_signal: t.rsi_14.map(|v| {
                    if v > 70.0 { "overbought".to_string() }
                    else if v < 30.0 { "oversold".to_string() }
                    else { "neutral".to_string() }
                }),
                macd: t.macd.as_ref().map(|m| format!("{:.4}", m.macd)),
                macd_signal: t.macd.as_ref().map(|m| format!("{:.4}", m.signal)),
                macd_histogram: t.macd.as_ref().map(|m| format!("{:.4}", m.histogram)),
                sma_20: None,
                sma_50: t.sma_50.map(|v| format!("{:.2}", v)),
            });

            WatchlistItemJson {
                symbol: w.symbol.clone(),
                name: resolve_name(&w.symbol),
                category: w.category.clone(),
                current_price: current_price.map(|p| p.to_string()),
                daily_change_pct: daily_change_pct.map(|p| p.round_dp(2).to_string()),
                technicals: technicals_json,
            }
        })
        .collect();
    Ok(items)
}

fn get_movers_json(positions: &[Position], hist_1d: &HashMap<String, Decimal>) -> Vec<MoverJson> {
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
        .map(|(symbol, name, pct)| MoverJson {
            symbol,
            name,
            daily_change_pct: pct.round_dp(2).to_string(),
            daily_change_abs: pct.abs().round_dp(2).to_string(),
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
            item.insert("name".to_string(), serde_json::Value::String(name.to_string()));
            item.insert("price".to_string(), serde_json::Value::String(quote.price.to_string()));
            item.insert("fetched_at".to_string(), serde_json::Value::String(quote.fetched_at));
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
        Ok(results) => {
            results.iter().filter(|r| r.newly_triggered).map(|r| {
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
            }).collect()
        }
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
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();
    
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

fn get_regime_json(_conn: &Connection) -> Result<serde_json::Value> {
    // Placeholder - regime module doesn't exist yet
    // Return empty object for now
    Ok(serde_json::json!({}))
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
        Some(((current - previous) / previous) * dec!(100))
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

pub fn run(conn: &Connection, config: &Config, technicals: bool, agent: bool) -> Result<()> {
    if agent {
        return run_agent_mode(conn, config);
    }
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    // Get 1-day historical prices for top movers
    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = get_prices_at_date(conn, &symbols, &yesterday_str).unwrap_or_default();

    // Load price history for technicals if requested
    let technicals_data = if technicals {
        compute_technicals_for_symbols(conn, &symbols)
    } else {
        HashMap::new()
    };

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, &prices, &hist_1d, &technicals_data),
        PortfolioMode::Percentage => run_percentage(conn, config, &prices, &hist_1d, &technicals_data),
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

    let priced_count = positions.iter().filter(|p| p.current_price.is_some()).count();
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

    let priced: Vec<_> = positions.iter().filter(|p| p.current_price.is_some()).collect();
    if priced.is_empty() {
        println!("# Portfolio Brief\n\nNo prices cached. Run `pftui refresh` first.");
        return Ok(());
    }

    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);
    println!("*Percentage mode (allocation-based)*\n");
    print_risk_summary(conn, &positions);
    print_benchmark_comparison(conn, &positions, hist_1d);
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
        println!("| {} | {} | {} | {} | {} |", symbol_display, pos.category, price_str, day_str, alloc_str);
    }

    // Technicals section
    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }

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
        positions
            .iter()
            .filter_map(|p| p.allocation_pct)
            .collect()
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

// ──────────────────────────────────────────────────────────────
// Technicals
// ──────────────────────────────────────────────────────────────

/// Snapshot of technical indicator values for a single symbol.
#[derive(Debug)]
struct TechnicalSnapshot {
    rsi_14: Option<f64>,
    macd: Option<MacdResult>,
    sma_50: Option<f64>,
    sma_200: Option<f64>,
}

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
fn macd_label(m: &MacdResult) -> &'static str {
    if m.histogram > 0.0 {
        "bullish"
    } else if m.histogram < 0.0 {
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
    let mut result = HashMap::new();

    for symbol in symbols {
        // Need at least 200 days for SMA-200; fetch 250 to be safe
        let history = match get_history(conn, symbol, 250) {
            Ok(h) if h.len() >= 14 => h,
            _ => continue,
        };

        let closes: Vec<f64> = history
            .iter()
            .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
            .collect();

        let rsi_values = compute_rsi(&closes, 14);
        let rsi_14 = rsi_values.iter().rev().find_map(|v| *v);

        let macd_values = compute_macd(&closes, 12, 26, 9);
        let macd = macd_values.iter().rev().find_map(|v| *v);

        let sma_50_values = compute_sma(&closes, 50);
        let sma_50 = sma_50_values.iter().rev().find_map(|v| *v);

        let sma_200_values = compute_sma(&closes, 200);
        let sma_200 = sma_200_values.iter().rev().find_map(|v| *v);

        result.insert(
            symbol.clone(),
            TechnicalSnapshot {
                rsi_14,
                macd,
                sma_50,
                sma_200,
            },
        );
    }

    result
}

/// Print a technicals section for all positions that have indicator data.
fn print_technicals_section(
    positions: &[Position],
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) {
    // Only show positions that have technicals (skip cash)
    let relevant: Vec<&Position> = positions
        .iter()
        .filter(|p| {
            p.category != AssetCategory::Cash && technicals_data.contains_key(&p.symbol)
        })
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
            .map(|m| format!("{:.2}", m.macd))
            .unwrap_or_else(|| "—".to_string());

        let hist_str = snap
            .macd
            .map(|m| {
                let sign = if m.histogram >= 0.0 { "+" } else { "" };
                format!("{}{:.2} ({})", sign, m.histogram, macd_label(&m))
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
        .map(|(cat, pct)| {
            format!("**{}** {}%", format_category(cat), pct.round_dp(0))
        })
        .collect();

    println!("{}\n", parts.join(" · "));
}

fn print_top_movers(
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
    base: &str,
) {
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
    movers.sort_by(|a, b| b.2.abs().partial_cmp(&a.2.abs()).unwrap_or(std::cmp::Ordering::Equal));

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

fn print_alerts(conn: &Connection) {
    use crate::alerts::engine::check_alerts;
    
    let results = match check_alerts(conn) {
        Ok(r) => r,
        Err(_) => return, // Silently skip if check fails
    };
    
    // Separate triggered and armed alerts
    let triggered: Vec<_> = results.iter()
        .filter(|r| r.rule.status == AlertStatus::Triggered)
        .collect();
    
    let armed_near: Vec<_> = results.iter()
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
    
    // Show triggered alerts first
    if !triggered.is_empty() {
        for result in triggered {
            let current = result.current_value
                .map(|v| v.round_dp(2).to_string())
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "🔴 **TRIGGERED** — {} (current: {})",
                result.rule.rule_text,
                current
            );
        }
    }
    
    // Show near-threshold armed alerts
    if !armed_near.is_empty() {
        for result in armed_near {
            let distance = result.distance_pct.unwrap().abs().round_dp(1);
            let current = result.current_value
                .map(|v| v.round_dp(2).to_string())
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "🟡 **NEAR** — {} (current: {}, {}% away)",
                result.rule.rule_text,
                current,
                distance
            );
        }
    }
    
    println!();
}

fn print_pnl_attribution(
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
    base: &str,
) {
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
    contributions.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));

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
        println!(
            "- **{}**: {}{} {}",
            label,
            sign,
            fmt_commas(*pnl, 2),
            base,
        );
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
            symbol_display, pos.category, pos.quantity, price_str, value_str, gain_str, day_str,
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
    fn brief_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let result = run(&conn, &config, false, false);
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

        let result = run(&conn, &config, false, false);
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
        },
        )
        .unwrap();

        let result = run(&conn, &config, false, false);
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
        },
        )
        .unwrap();

        let result = run(&conn, &config, false, false);
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

        let result = run(&conn, &config, false, false);
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
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200)), Some(dec!(100000))),
            make_position("GOOG", AssetCategory::Equity, dec!(5), dec!(100), Some(dec!(90)), Some(dec!(100000))),
            make_position("BTC", AssetCategory::Crypto, dec!(1), dec!(30000), Some(dec!(85000)), Some(dec!(100000))),
        ];

        let mut hist_1d: HashMap<String, Decimal> = HashMap::new();
        hist_1d.insert("AAPL".to_string(), dec!(195));
        hist_1d.insert("GOOG".to_string(), dec!(100));
        hist_1d.insert("BTC".to_string(), dec!(83000));

        // Verify it doesn't panic — output goes to stdout
        print_top_movers(&positions, &hist_1d, "USD");
    }

    #[test]
    fn category_allocation_groups_correctly() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(100), Some(dec!(150)), Some(dec!(2600))),
            make_position("GOOG", AssetCategory::Equity, dec!(5), dec!(100), Some(dec!(120)), Some(dec!(2600))),
            make_position("BTC", AssetCategory::Crypto, dec!(1), dec!(500), Some(dec!(1000)), Some(dec!(2600))),
        ];

        // Verify it doesn't panic — output goes to stdout
        print_category_allocation(&positions, dec!(2600));
    }

    #[test]
    fn technicals_section_skips_cash_positions() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200)), Some(dec!(100000))),
            make_position("USD", AssetCategory::Cash, dec!(50000), dec!(1), Some(dec!(1)), Some(dec!(100000))),
        ];

        let mut technicals = HashMap::new();
        technicals.insert(
            "AAPL".to_string(),
            TechnicalSnapshot {
                rsi_14: Some(55.0),
                macd: Some(MacdResult { macd: 1.5, signal: 1.0, histogram: 0.5 }),
                sma_50: Some(190.0),
                sma_200: Some(175.0),
            },
        );

        // Should not panic and should skip USD
        print_technicals_section(&positions, &technicals);
    }

    #[test]
    fn technicals_section_empty_data_produces_no_output() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200)), Some(dec!(100000))),
        ];

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
        let bullish = MacdResult { macd: 1.0, signal: 0.5, histogram: 0.5 };
        assert_eq!(macd_label(&bullish), "bullish");
        let bearish = MacdResult { macd: -1.0, signal: -0.5, histogram: -0.5 };
        assert_eq!(macd_label(&bearish), "bearish");
        let neutral = MacdResult { macd: 0.0, signal: 0.0, histogram: 0.0 };
        assert_eq!(macd_label(&neutral), "neutral");
    }

    #[test]
    fn brief_with_technicals_flag() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

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
        },
        )
        .unwrap();

        // With technicals=true, should succeed (no history means no indicators displayed)
        let result = run(&conn, &config, true, false);
        assert!(result.is_ok());
    }
}
