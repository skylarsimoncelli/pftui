use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;

use super::{AlertDirection, AlertKind, AlertRule, AlertStatus};
use crate::db::alerts;
use crate::db::backend::BackendConnection;
use crate::db::correlation_snapshots;
use crate::db::price_cache;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history;
use crate::db::scan_queries;
use crate::db::triggered_alerts;
use crate::indicators::bollinger::compute_bollinger;
use crate::indicators::{compute_macd, compute_rsi, compute_sma};

/// Result of checking a single alert rule against current data.
#[derive(Debug, Clone)]
pub struct AlertCheckResult {
    pub rule: AlertRule,
    pub current_value: Option<Decimal>,
    /// True if the alert just transitioned from Armed → Triggered.
    pub newly_triggered: bool,
    /// Distance to trigger as a percentage (positive = not yet triggered).
    /// For armed alerts: how far from threshold.
    /// None if current value is unavailable.
    pub distance_pct: Option<Decimal>,
    pub trigger_data: Value,
}

/// Check all armed alerts against current cached prices.
///
/// Returns check results for all alerts (armed + already triggered).
/// Newly triggered alerts are updated to `Triggered` status in the DB.
pub fn check_alerts(conn: &Connection) -> Result<Vec<AlertCheckResult>> {
    ensure_review_date_alerts(conn)?;
    ensure_scan_query_change_alerts(conn)?;
    let all_alerts = alerts::list_alerts(conn)?;
    if all_alerts.is_empty() {
        return Ok(Vec::new());
    }

    let cached_prices = price_cache::get_all_cached_prices(conn)?;
    let price_map: HashMap<String, Decimal> = cached_prices
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    let default_cooldown = load_default_cooldown();
    let mut results = Vec::new();
    for alert in all_alerts {
        let result = check_single_alert_sqlite(conn, &alert, &price_map, default_cooldown)?;
        results.push(result);
    }
    Ok(results)
}

pub fn check_alerts_backend(
    backend: &BackendConnection,
    _conn: &Connection,
) -> Result<Vec<AlertCheckResult>> {
    ensure_review_date_alerts_backend(backend)?;
    ensure_scan_query_change_alerts_backend(backend)?;
    let all_alerts = alerts::list_alerts_backend(backend)?;
    if all_alerts.is_empty() {
        return Ok(Vec::new());
    }

    // Build a price map from the cache for quick lookups
    let cached_prices = get_all_cached_prices_backend(backend)?;
    let price_map: HashMap<String, Decimal> = cached_prices
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    let default_cooldown = load_default_cooldown();
    let mut results = Vec::new();

    for alert in all_alerts {
        let result = check_single_alert_backend(backend, &alert, &price_map, default_cooldown)?;
        results.push(result);
    }

    Ok(results)
}

pub fn check_alerts_backend_only(backend: &BackendConnection) -> Result<Vec<AlertCheckResult>> {
    if let Some(conn) = backend.sqlite_native() {
        return check_alerts_backend(backend, conn);
    }

    ensure_review_date_alerts_backend(backend)?;
    ensure_scan_query_change_alerts_backend(backend)?;
    let all_alerts = alerts::list_alerts_backend(backend)?;
    if all_alerts.is_empty() {
        return Ok(Vec::new());
    }

    let cached_prices = get_all_cached_prices_backend(backend)?;
    let price_map: HashMap<String, Decimal> = cached_prices
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    let default_cooldown = load_default_cooldown();
    let mut results = Vec::new();
    for alert in all_alerts {
        let result = check_single_alert_backend(backend, &alert, &price_map, default_cooldown)?;
        results.push(result);
    }
    Ok(results)
}

/// Load the configured default cooldown floor for recurring alerts.
fn load_default_cooldown() -> i64 {
    crate::config::load_config()
        .map(|c| c.alert_default_cooldown_minutes)
        .unwrap_or(30)
}

/// Check a single alert against the price map. Updates DB if newly triggered.
fn check_single_alert_sqlite(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
    default_cooldown: i64,
) -> Result<AlertCheckResult> {
    let evaluation = evaluate_alert_sqlite(conn, alert, price_map)?;
    finalize_sqlite_alert_result(conn, alert, evaluation, default_cooldown)
}

fn check_single_alert_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
    default_cooldown: i64,
) -> Result<AlertCheckResult> {
    let evaluation = evaluate_alert_backend(backend, alert, price_map)?;
    finalize_backend_alert_result(backend, alert, evaluation, default_cooldown)
}

#[derive(Debug, Clone)]
struct AlertEvaluation {
    current_value: Option<Decimal>,
    is_triggered: bool,
    distance_pct: Option<Decimal>,
    trigger_data: Value,
}

fn finalize_sqlite_alert_result(
    conn: &Connection,
    alert: &AlertRule,
    evaluation: AlertEvaluation,
    default_cooldown: i64,
) -> Result<AlertCheckResult> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let newly_triggered =
        should_log_trigger(conn, alert, evaluation.is_triggered, default_cooldown)?;
    if newly_triggered {
        let trigger_json = evaluation.trigger_data.to_string();
        triggered_alerts::add_triggered_alert(conn, alert.id, &now, &trigger_json)?;
        if alert.recurring {
            alerts::update_alert_status(conn, alert.id, AlertStatus::Armed, Some(&now))?;
        } else {
            alerts::update_alert_status(conn, alert.id, AlertStatus::Triggered, Some(&now))?;
        }
    }

    Ok(AlertCheckResult {
        rule: alert.clone(),
        current_value: evaluation.current_value,
        newly_triggered,
        distance_pct: evaluation.distance_pct,
        trigger_data: evaluation.trigger_data,
    })
}

fn finalize_backend_alert_result(
    backend: &BackendConnection,
    alert: &AlertRule,
    evaluation: AlertEvaluation,
    default_cooldown: i64,
) -> Result<AlertCheckResult> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let newly_triggered =
        should_log_trigger_backend(backend, alert, evaluation.is_triggered, default_cooldown)?;
    if newly_triggered {
        let trigger_json = evaluation.trigger_data.to_string();
        triggered_alerts::add_triggered_alert_backend(backend, alert.id, &now, &trigger_json)?;
        if alert.recurring {
            alerts::update_alert_status_backend(backend, alert.id, AlertStatus::Armed, Some(&now))?;
        } else {
            alerts::update_alert_status_backend(
                backend,
                alert.id,
                AlertStatus::Triggered,
                Some(&now),
            )?;
        }
    }

    Ok(AlertCheckResult {
        rule: alert.clone(),
        current_value: evaluation.current_value,
        newly_triggered,
        distance_pct: evaluation.distance_pct,
        trigger_data: evaluation.trigger_data,
    })
}

fn should_log_trigger(
    conn: &Connection,
    alert: &AlertRule,
    is_triggered: bool,
    default_cooldown: i64,
) -> Result<bool> {
    if !is_triggered {
        return Ok(false);
    }
    if !alert.recurring {
        return Ok(alert.status == AlertStatus::Armed);
    }

    // Use the greater of per-alert cooldown and the configured default floor.
    let effective_cooldown = effective_cooldown_minutes(alert.cooldown_minutes, default_cooldown);
    let recent = triggered_alerts::list_triggered_alerts(conn, None, false)?;
    let latest = recent.into_iter().find(|row| row.alert_id == alert.id);
    Ok(match latest {
        None => true,
        Some(row) => cooldown_elapsed(&row.triggered_at, effective_cooldown),
    })
}

fn should_log_trigger_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
    is_triggered: bool,
    default_cooldown: i64,
) -> Result<bool> {
    if !is_triggered {
        return Ok(false);
    }
    if !alert.recurring {
        return Ok(alert.status == AlertStatus::Armed);
    }
    // Use the greater of per-alert cooldown and the configured default floor.
    let effective_cooldown = effective_cooldown_minutes(alert.cooldown_minutes, default_cooldown);
    let recent = triggered_alerts::list_triggered_alerts_backend(backend, None, false)?;
    let latest = recent.into_iter().find(|row| row.alert_id == alert.id);
    Ok(match latest {
        None => true,
        Some(row) => cooldown_elapsed(&row.triggered_at, effective_cooldown),
    })
}

/// Compute effective cooldown: use the per-alert value if set (> 0),
/// otherwise fall back to the configured default floor.
fn effective_cooldown_minutes(per_alert: i64, default_floor: i64) -> i64 {
    if per_alert > 0 {
        per_alert
    } else {
        default_floor
    }
}

fn cooldown_elapsed(triggered_at: &str, cooldown_minutes: i64) -> bool {
    if cooldown_minutes <= 0 {
        return true;
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(triggered_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(triggered_at, "%Y-%m-%d %H:%M:%S")
                .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, chrono::Utc))
        });
    match parsed {
        Ok(dt) => chrono::Utc::now() >= dt + chrono::Duration::minutes(cooldown_minutes),
        Err(_) => true,
    }
}

fn evaluate_alert_sqlite(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    match alert.kind {
        AlertKind::Price => Ok(evaluate_price_alert(alert, price_map)),
        AlertKind::Allocation => Ok(AlertEvaluation {
            current_value: None,
            is_triggered: false,
            distance_pct: None,
            trigger_data: json!({ "kind": "allocation", "reason": "not_implemented" }),
        }),
        AlertKind::Indicator => evaluate_indicator_alert_sqlite(conn, alert, price_map),
        AlertKind::Technical => evaluate_technical_alert_sqlite(conn, alert, price_map),
        AlertKind::Macro => evaluate_macro_alert_sqlite(conn, alert, price_map),
        AlertKind::Scenario => Ok(AlertEvaluation {
            current_value: None,
            is_triggered: false, // Scenario alerts are pre-triggered at write time
            distance_pct: None,
            trigger_data: json!({ "kind": "scenario", "reason": "evaluated_at_write_time" }),
        }),
        AlertKind::Ratio => Ok(evaluate_ratio_alert(alert, price_map)),
    }
}

fn evaluate_alert_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    match alert.kind {
        AlertKind::Price => Ok(evaluate_price_alert(alert, price_map)),
        AlertKind::Allocation => Ok(AlertEvaluation {
            current_value: None,
            is_triggered: false,
            distance_pct: None,
            trigger_data: json!({ "kind": "allocation", "reason": "not_implemented" }),
        }),
        AlertKind::Indicator => evaluate_indicator_alert_backend(backend, alert, price_map),
        AlertKind::Technical => evaluate_technical_alert_backend(backend, alert, price_map),
        AlertKind::Macro => evaluate_macro_alert_backend(backend, alert),
        AlertKind::Scenario => Ok(AlertEvaluation {
            current_value: None,
            is_triggered: false, // Scenario alerts are pre-triggered at write time
            distance_pct: None,
            trigger_data: json!({ "kind": "scenario", "reason": "evaluated_at_write_time" }),
        }),
        AlertKind::Ratio => Ok(evaluate_ratio_alert(alert, price_map)),
    }
}

fn evaluate_price_alert(
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> AlertEvaluation {
    let threshold = Decimal::from_str(&alert.threshold).unwrap_or(Decimal::ZERO);
    let current = price_map.get(&alert.symbol).copied();
    let is_triggered = current
        .map(|value| match alert.direction {
            AlertDirection::Above => value >= threshold,
            AlertDirection::Below => value <= threshold,
        })
        .unwrap_or(false);
    let distance_pct =
        current.and_then(|value| compute_distance_pct(alert.direction, value, threshold));
    AlertEvaluation {
        current_value: current,
        is_triggered,
        distance_pct,
        trigger_data: json!({
            "symbol": alert.symbol,
            "current_value": current.map(|v| v.to_string()),
            "threshold": threshold.to_string()
        }),
    }
}

/// Evaluate a ratio alert: symbol is "NUMERATOR/DENOMINATOR", computes current ratio from price cache.
fn evaluate_ratio_alert(
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> AlertEvaluation {
    let threshold = Decimal::from_str(&alert.threshold).unwrap_or(Decimal::ZERO);
    let (numerator_sym, denominator_sym) = match alert.symbol.split_once('/') {
        Some((a, b)) => (a.trim().to_uppercase(), b.trim().to_uppercase()),
        None => {
            return AlertEvaluation {
                current_value: None,
                is_triggered: false,
                distance_pct: None,
                trigger_data: json!({
                    "symbol": alert.symbol,
                    "reason": "invalid_ratio_symbol_format",
                    "hint": "Expected NUMERATOR/DENOMINATOR (e.g. GC=F/CL=F)"
                }),
            };
        }
    };
    let numerator_price = price_map.get(&numerator_sym).copied();
    let denominator_price = price_map.get(&denominator_sym).copied();
    let ratio = match (numerator_price, denominator_price) {
        (Some(num), Some(den)) if !den.is_zero() => Some(num / den),
        _ => None,
    };
    let is_triggered = ratio
        .map(|value| match alert.direction {
            AlertDirection::Above => value >= threshold,
            AlertDirection::Below => value <= threshold,
        })
        .unwrap_or(false);
    let distance_pct = ratio.and_then(|value| compute_distance_pct(alert.direction, value, threshold));
    AlertEvaluation {
        current_value: ratio,
        is_triggered,
        distance_pct,
        trigger_data: json!({
            "numerator": numerator_sym,
            "denominator": denominator_sym,
            "numerator_price": numerator_price.map(|v| v.to_string()),
            "denominator_price": denominator_price.map(|v| v.to_string()),
            "ratio": ratio.map(|v| v.to_string()),
            "threshold": threshold.to_string()
        }),
    }
}

fn evaluate_indicator_alert(alert: &AlertRule) -> AlertEvaluation {
    if alert.symbol.starts_with("REVIEW:") {
        let review_date = chrono::NaiveDate::parse_from_str(&alert.threshold, "%Y-%m-%d").ok();
        let today = chrono::Utc::now().date_naive();
        let triggered = review_date.map(|d| today >= d).unwrap_or(false);
        return AlertEvaluation {
            current_value: None,
            is_triggered: triggered,
            distance_pct: None,
            trigger_data: json!({
                "symbol": alert.symbol,
                "review_date": alert.threshold,
                "today": today.to_string()
            }),
        };
    }

    AlertEvaluation {
        current_value: None,
        is_triggered: false,
        distance_pct: None,
        trigger_data: json!({ "symbol": alert.symbol, "reason": "unsupported_indicator_alert" }),
    }
}

fn evaluate_indicator_alert_sqlite(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    if alert.symbol.starts_with("REVIEW:") {
        return Ok(evaluate_indicator_alert(alert));
    }
    let (symbol, indicator) = split_indicator_symbol(&alert.symbol);
    let history = price_history::get_history(conn, &symbol, 240)?;
    Ok(evaluate_indicator_from_history(
        alert,
        &symbol,
        &indicator,
        price_map.get(&symbol).copied(),
        &history,
    ))
}

fn evaluate_indicator_alert_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    if alert.symbol.starts_with("REVIEW:") {
        return Ok(evaluate_indicator_alert(alert));
    }
    let (symbol, indicator) = split_indicator_symbol(&alert.symbol);
    let history = price_history::get_history_backend(backend, &symbol, 240)?;
    Ok(evaluate_indicator_from_history(
        alert,
        &symbol,
        &indicator,
        price_map.get(&symbol).copied(),
        &history,
    ))
}

fn evaluate_technical_alert_sqlite(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    if alert.condition.as_deref() == Some("correlation_break") {
        return evaluate_symbol_correlation_break_sqlite(conn, alert);
    }
    let history = price_history::get_history(conn, &alert.symbol, 240)?;
    Ok(evaluate_technical_from_history(
        alert,
        price_map.get(&alert.symbol).copied(),
        &history,
    ))
}

fn evaluate_technical_alert_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    if alert.condition.as_deref() == Some("correlation_break") {
        return evaluate_symbol_correlation_break_backend(backend, alert);
    }
    let history = price_history::get_history_backend(backend, &alert.symbol, 240)?;
    Ok(evaluate_technical_from_history(
        alert,
        price_map.get(&alert.symbol).copied(),
        &history,
    ))
}

fn evaluate_macro_alert_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
) -> Result<AlertEvaluation> {
    let condition = alert.condition.as_deref().unwrap_or_default();
    let evaluation = crate::data::macro_alerts::evaluate_condition(backend, condition)?;
    Ok(AlertEvaluation {
        current_value: evaluation.current_value,
        is_triggered: evaluation.triggered,
        distance_pct: None,
        trigger_data: evaluation.trigger_data,
    })
}

fn evaluate_macro_alert_sqlite(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertEvaluation> {
    let condition = alert.condition.as_deref().unwrap_or_default();
    match condition {
        "regime_change" => {
            let history = crate::db::regime_snapshots::get_history(conn, Some(2))?;
            let triggered = history.len() >= 2 && history[0].regime != history[1].regime;
            Ok(AlertEvaluation {
                current_value: None,
                is_triggered: triggered,
                distance_pct: None,
                trigger_data: json!({
                    "previous_regime": history.get(1).map(|row| row.regime.clone()),
                    "current_regime": history.first().map(|row| row.regime.clone())
                }),
            })
        }
        "fear_greed_extreme" => {
            let reading = crate::db::sentiment_cache::get_latest(conn, "traditional_fng")?
                .or_else(|| {
                    crate::db::sentiment_cache::get_latest(conn, "traditional")
                        .ok()
                        .flatten()
                })
                .or_else(|| {
                    crate::db::sentiment_cache::get_latest(conn, "crypto_fng")
                        .ok()
                        .flatten()
                });
            let Some(reading) = reading else {
                return Ok(AlertEvaluation {
                    current_value: None,
                    is_triggered: false,
                    distance_pct: None,
                    trigger_data: json!({ "reason": "missing_sentiment" }),
                });
            };
            Ok(AlertEvaluation {
                current_value: Some(Decimal::from(reading.value)),
                is_triggered: reading.value <= 15 || reading.value >= 85,
                distance_pct: None,
                trigger_data: json!({
                    "index_type": reading.index_type,
                    "value": reading.value
                }),
            })
        }
        "yield_curve_inversion_change" => {
            let history = crate::db::economic_cache::get_history(conn, "T10Y2Y", 2)?;
            if history.len() < 2 {
                return Ok(AlertEvaluation {
                    current_value: None,
                    is_triggered: false,
                    distance_pct: None,
                    trigger_data: json!({ "reason": "insufficient_yield_curve_history" }),
                });
            }
            let previous = &history[history.len() - 2];
            let current = &history[history.len() - 1];
            let sign_change = (previous.value.is_sign_negative()
                && current.value.is_sign_positive())
                || (previous.value.is_sign_positive() && current.value.is_sign_negative())
                || (previous.value.is_zero() && !current.value.is_zero());
            Ok(AlertEvaluation {
                current_value: Some(current.value),
                is_triggered: sign_change,
                distance_pct: None,
                trigger_data: json!({
                    "previous": previous.value.to_string(),
                    "current": current.value.to_string()
                }),
            })
        }
        "vix_regime_shift" => {
            let current = price_map
                .get("^VIX")
                .copied()
                .or_else(|| price_map.get("VIX").copied());
            let history = price_history::get_history(conn, "^VIX", 2)?;
            Ok(evaluate_threshold_cross(
                current,
                history.iter().rev().nth(1).map(|r| r.close),
                &[20, 25, 30, 35],
                "vix",
            ))
        }
        "dxy_century_cross" => {
            let current = price_map
                .get("DX-Y.NYB")
                .copied()
                .or_else(|| price_map.get("DXY").copied());
            let history = price_history::get_history(conn, "DX-Y.NYB", 2)?;
            Ok(evaluate_threshold_cross(
                current,
                history.iter().rev().nth(1).map(|r| r.close),
                &[100],
                "dxy",
            ))
        }
        "correlation_regime_break" => {
            let rows_30 = correlation_snapshots::list_current(conn, Some("30d"))?;
            let rows_90 = correlation_snapshots::list_current(conn, Some("90d"))?;
            Ok(evaluate_correlation_break(rows_30, rows_90))
        }
        _ => Ok(AlertEvaluation {
            current_value: None,
            is_triggered: false,
            distance_pct: None,
            trigger_data: json!({ "reason": "unsupported_macro_condition", "condition": condition }),
        }),
    }
}

fn evaluate_technical_from_history(
    alert: &AlertRule,
    current_price: Option<Decimal>,
    history: &[crate::models::price::HistoryRecord],
) -> AlertEvaluation {
    let condition = alert.condition.as_deref().unwrap_or_default();
    let closes = closes_as_f64(history);
    let current_price = current_price.or_else(|| history.last().map(|row| row.close));

    match condition {
        "price_above_sma50" => evaluate_price_vs_sma(alert, current_price, &closes, 50, true),
        "price_below_sma50" => evaluate_price_vs_sma(alert, current_price, &closes, 50, false),
        "price_above_sma200" => evaluate_price_vs_sma(alert, current_price, &closes, 200, true),
        "price_below_sma200" => evaluate_price_vs_sma(alert, current_price, &closes, 200, false),
        "rsi_above_70" => evaluate_rsi(alert, &closes, 70.0, true),
        "rsi_below_30" => evaluate_rsi(alert, &closes, 30.0, false),
        "macd_cross_bullish" => evaluate_macd_cross(true, &closes),
        "macd_cross_bearish" => evaluate_macd_cross(false, &closes),
        "bollinger_break_upper" => evaluate_bollinger_break(true, current_price, &closes),
        "bollinger_break_lower" => evaluate_bollinger_break(false, current_price, &closes),
        "correlation_break" => AlertEvaluation {
            current_value: None,
            is_triggered: false,
            distance_pct: None,
            trigger_data: json!({ "condition": condition, "reason": "backend_snapshot_required" }),
        },
        _ if condition.starts_with("price_change_pct_above_") => {
            evaluate_price_change(alert, current_price, history, true)
        }
        _ if condition.starts_with("price_change_pct_below_") => {
            evaluate_price_change(alert, current_price, history, false)
        }
        _ => AlertEvaluation {
            current_value: None,
            is_triggered: false,
            distance_pct: None,
            trigger_data: json!({ "condition": condition, "reason": "unsupported_technical_condition" }),
        },
    }
}

fn evaluate_indicator_from_history(
    alert: &AlertRule,
    symbol: &str,
    indicator: &str,
    current_price: Option<Decimal>,
    history: &[crate::models::price::HistoryRecord],
) -> AlertEvaluation {
    let closes = closes_as_f64(history);
    let current_price = current_price.or_else(|| history.last().map(|row| row.close));
    let threshold = Decimal::from_str(&alert.threshold).unwrap_or(Decimal::ZERO);

    match indicator {
        "RSI" => {
            let rsi = compute_rsi(&closes, 14).iter().rev().find_map(|v| *v);
            let current_value = rsi.and_then(decimal_from_f64);
            let threshold_f64 = threshold
                .to_string()
                .parse::<f64>()
                .ok()
                .unwrap_or_default();
            let triggered = rsi
                .map(|value| match alert.direction {
                    AlertDirection::Above => value >= threshold_f64,
                    AlertDirection::Below => value <= threshold_f64,
                })
                .unwrap_or(false);
            AlertEvaluation {
                current_value,
                is_triggered: triggered,
                distance_pct: None,
                trigger_data: json!({
                    "symbol": symbol,
                    "indicator": "RSI",
                    "rsi_14": current_value.map(|v| v.to_string()),
                    "threshold": threshold.to_string()
                }),
            }
        }
        indicator if indicator.starts_with("SMA") || indicator == "SMA" => {
            let period = indicator
                .strip_prefix("SMA")
                .and_then(|value| value.parse::<usize>().ok())
                .or_else(|| threshold.to_string().parse::<usize>().ok())
                .unwrap_or(50);
            evaluate_price_vs_sma(
                alert,
                current_price,
                &closes,
                period,
                matches!(alert.direction, AlertDirection::Above),
            )
        }
        "MACD" => {
            let macd_series: Vec<_> = compute_macd(&closes, 12, 26, 9)
                .into_iter()
                .flatten()
                .collect();
            let latest = macd_series.last();
            let current_value = latest.and_then(|row| decimal_from_f64(row.macd));
            let triggered = current_value
                .map(|value| match alert.direction {
                    AlertDirection::Above => value >= threshold,
                    AlertDirection::Below => value <= threshold,
                })
                .unwrap_or(false);
            AlertEvaluation {
                current_value,
                is_triggered: triggered,
                distance_pct: None,
                trigger_data: json!({
                    "symbol": symbol,
                    "indicator": "MACD",
                    "macd": latest.map(|row| row.macd),
                    "signal": latest.map(|row| row.signal),
                    "histogram": latest.map(|row| row.histogram),
                    "threshold": threshold.to_string()
                }),
            }
        }
        "MACD_CROSS" => {
            evaluate_macd_cross(matches!(alert.direction, AlertDirection::Above), &closes)
        }
        "CHANGE_PCT" => evaluate_price_change(
            alert,
            current_price,
            history,
            matches!(alert.direction, AlertDirection::Above),
        ),
        _ => AlertEvaluation {
            current_value: None,
            is_triggered: false,
            distance_pct: None,
            trigger_data: json!({
                "symbol": symbol,
                "indicator": indicator,
                "reason": "unsupported_indicator_alert"
            }),
        },
    }
}

fn evaluate_price_vs_sma(
    _alert: &AlertRule,
    current_price: Option<Decimal>,
    closes: &[f64],
    period: usize,
    above: bool,
) -> AlertEvaluation {
    let sma = compute_sma(closes, period).iter().rev().find_map(|v| *v);
    let sma_decimal = sma.and_then(decimal_from_f64);
    let triggered = match (current_price, sma_decimal) {
        (Some(price), Some(sma)) if above => price > sma,
        (Some(price), Some(sma)) => price < sma,
        _ => false,
    };
    let distance_pct = match (current_price, sma_decimal) {
        (Some(price), Some(sma)) => compute_distance_pct(
            if above {
                AlertDirection::Above
            } else {
                AlertDirection::Below
            },
            price,
            sma,
        ),
        _ => None,
    };
    AlertEvaluation {
        current_value: current_price,
        is_triggered: triggered,
        distance_pct,
        trigger_data: json!({
            "current_price": current_price.map(|v| v.to_string()),
            "sma_period": period,
            "sma": sma_decimal.map(|v| v.to_string())
        }),
    }
}

fn evaluate_rsi(
    _alert: &AlertRule,
    closes: &[f64],
    threshold: f64,
    above: bool,
) -> AlertEvaluation {
    let rsi = compute_rsi(closes, 14).iter().rev().find_map(|v| *v);
    let current_value = rsi.and_then(decimal_from_f64);
    let triggered = rsi
        .map(|value| {
            if above {
                value >= threshold
            } else {
                value <= threshold
            }
        })
        .unwrap_or(false);
    AlertEvaluation {
        current_value,
        is_triggered: triggered,
        distance_pct: None,
        trigger_data: json!({
            "rsi_14": current_value.map(|v| v.to_string()),
            "threshold": threshold
        }),
    }
}

fn evaluate_macd_cross(bullish: bool, closes: &[f64]) -> AlertEvaluation {
    let macd_series: Vec<_> = compute_macd(closes, 12, 26, 9)
        .into_iter()
        .flatten()
        .collect();
    let triggered = if macd_series.len() >= 2 {
        let prev = &macd_series[macd_series.len() - 2];
        let curr = &macd_series[macd_series.len() - 1];
        if bullish {
            prev.macd <= prev.signal && curr.macd > curr.signal
        } else {
            prev.macd >= prev.signal && curr.macd < curr.signal
        }
    } else {
        false
    };
    let current_hist = macd_series
        .last()
        .and_then(|row| decimal_from_f64(row.histogram));
    AlertEvaluation {
        current_value: current_hist,
        is_triggered: triggered,
        distance_pct: None,
        trigger_data: json!({
            "macd": macd_series.last().map(|m| m.macd),
            "signal": macd_series.last().map(|m| m.signal),
            "histogram": macd_series.last().map(|m| m.histogram)
        }),
    }
}

fn evaluate_bollinger_break(
    upper: bool,
    current_price: Option<Decimal>,
    closes: &[f64],
) -> AlertEvaluation {
    let bb = compute_bollinger(closes, 20, 2.0)
        .into_iter()
        .flatten()
        .last();
    let band: Option<Decimal> = bb.and_then(|row| {
        if upper {
            decimal_from_f64(row.upper)
        } else {
            decimal_from_f64(row.lower)
        }
    });
    let triggered = match (current_price, band) {
        (Some(price), Some(level)) if upper => price > level,
        (Some(price), Some(level)) => price < level,
        _ => false,
    };
    AlertEvaluation {
        current_value: current_price,
        is_triggered: triggered,
        distance_pct: None,
        trigger_data: json!({
            "current_price": current_price.map(|v| v.to_string()),
            "band": band.map(|v| v.to_string()),
            "band_type": if upper { "upper" } else { "lower" }
        }),
    }
}

fn evaluate_price_change(
    alert: &AlertRule,
    current_price: Option<Decimal>,
    history: &[crate::models::price::HistoryRecord],
    above: bool,
) -> AlertEvaluation {
    let threshold = alert
        .condition
        .as_deref()
        .and_then(|condition| condition.rsplit('_').next())
        .and_then(|value| Decimal::from_str(value).ok())
        .unwrap_or(Decimal::ZERO);
    let previous = history.iter().rev().nth(1).map(|row| row.close);
    let current = current_price.or_else(|| history.last().map(|row| row.close));
    let pct_change = match (current, previous) {
        (Some(curr), Some(prev)) if !prev.is_zero() => {
            Some((curr - prev) / prev * Decimal::from(100))
        }
        _ => None,
    };
    let triggered = pct_change
        .map(|change| {
            if above {
                change >= threshold
            } else {
                change <= -threshold
            }
        })
        .unwrap_or(false);
    AlertEvaluation {
        current_value: pct_change,
        is_triggered: triggered,
        distance_pct: None,
        trigger_data: json!({
            "current_price": current.map(|v| v.to_string()),
            "previous_close": previous.map(|v| v.to_string()),
            "change_pct": pct_change.map(|v| v.to_string()),
            "threshold_pct": threshold.to_string()
        }),
    }
}

fn evaluate_threshold_cross(
    current: Option<Decimal>,
    previous: Option<Decimal>,
    thresholds: &[i64],
    label: &str,
) -> AlertEvaluation {
    let crossed = match (current, previous) {
        (Some(curr), Some(prev)) => thresholds.iter().find_map(|threshold| {
            let level = Decimal::from(*threshold);
            if prev < level && curr >= level {
                Some((*threshold, "up"))
            } else if prev > level && curr <= level {
                Some((*threshold, "down"))
            } else {
                None
            }
        }),
        _ => None,
    };
    AlertEvaluation {
        current_value: current,
        is_triggered: crossed.is_some(),
        distance_pct: None,
        trigger_data: json!({
            "series": label,
            "previous": previous.map(|v| v.to_string()),
            "current": current.map(|v| v.to_string()),
            "crossed_threshold": crossed.map(|v| v.0),
            "direction": crossed.map(|v| v.1)
        }),
    }
}

fn evaluate_correlation_break(
    rows_30: Vec<crate::db::correlation_snapshots::CorrelationSnapshot>,
    rows_90: Vec<crate::db::correlation_snapshots::CorrelationSnapshot>,
) -> AlertEvaluation {
    let mut best: Option<(String, String, f64)> = None;
    for short in &rows_30 {
        if let Some(long) = rows_90
            .iter()
            .find(|row| row.symbol_a == short.symbol_a && row.symbol_b == short.symbol_b)
        {
            let delta = (short.correlation - long.correlation).abs();
            if best
                .as_ref()
                .map(|(_, _, current)| delta > *current)
                .unwrap_or(true)
            {
                best = Some((short.symbol_a.clone(), short.symbol_b.clone(), delta));
            }
        }
    }
    let current_value = best
        .as_ref()
        .and_then(|(_, _, delta)| decimal_from_f64(*delta));
    AlertEvaluation {
        current_value,
        is_triggered: best
            .as_ref()
            .map(|(_, _, delta)| *delta >= 0.3)
            .unwrap_or(false),
        distance_pct: None,
        trigger_data: json!({
            "symbol_a": best.as_ref().map(|row| row.0.clone()),
            "symbol_b": best.as_ref().map(|row| row.1.clone()),
            "delta": best.as_ref().map(|row| row.2)
        }),
    }
}

fn evaluate_symbol_correlation_break_sqlite(
    conn: &Connection,
    alert: &AlertRule,
) -> Result<AlertEvaluation> {
    let (a, b) = alert_pair_symbols(&alert.symbol);
    let rows_30 = correlation_snapshots::list_current(conn, Some("30d"))?;
    let rows_90 = correlation_snapshots::list_current(conn, Some("90d"))?;
    Ok(evaluate_pair_correlation_break(&a, &b, rows_30, rows_90))
}

fn evaluate_symbol_correlation_break_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
) -> Result<AlertEvaluation> {
    let (a, b) = alert_pair_symbols(&alert.symbol);
    let rows_30 = correlation_snapshots::list_current_backend(backend, Some("30d"))?;
    let rows_90 = correlation_snapshots::list_current_backend(backend, Some("90d"))?;
    Ok(evaluate_pair_correlation_break(&a, &b, rows_30, rows_90))
}

fn alert_pair_symbols(symbol: &str) -> (String, String) {
    if let Some((a, b)) = symbol.split_once(':') {
        (a.to_uppercase(), b.to_uppercase())
    } else {
        (symbol.to_uppercase(), "SPY".to_string())
    }
}

fn evaluate_pair_correlation_break(
    symbol_a: &str,
    symbol_b: &str,
    rows_30: Vec<crate::db::correlation_snapshots::CorrelationSnapshot>,
    rows_90: Vec<crate::db::correlation_snapshots::CorrelationSnapshot>,
) -> AlertEvaluation {
    let short = rows_30.iter().find(|row| {
        (row.symbol_a == symbol_a && row.symbol_b == symbol_b)
            || (row.symbol_a == symbol_b && row.symbol_b == symbol_a)
    });
    let long = rows_90.iter().find(|row| {
        (row.symbol_a == symbol_a && row.symbol_b == symbol_b)
            || (row.symbol_a == symbol_b && row.symbol_b == symbol_a)
    });
    let delta = match (short, long) {
        (Some(a), Some(b)) => Some((a.correlation - b.correlation).abs()),
        _ => None,
    };
    AlertEvaluation {
        current_value: delta.and_then(decimal_from_f64),
        is_triggered: delta.map(|d| d >= 0.3).unwrap_or(false),
        distance_pct: None,
        trigger_data: json!({
            "symbol_a": symbol_a,
            "symbol_b": symbol_b,
            "delta": delta
        }),
    }
}

fn closes_as_f64(history: &[crate::models::price::HistoryRecord]) -> Vec<f64> {
    history
        .iter()
        .filter_map(|row| row.close.to_string().parse::<f64>().ok())
        .collect()
}

fn split_indicator_symbol(symbol: &str) -> (String, String) {
    match symbol.rsplit_once(' ') {
        Some((asset, indicator)) => (asset.to_uppercase(), indicator.to_uppercase()),
        None => (symbol.to_uppercase(), String::new()),
    }
}

fn decimal_from_f64(value: f64) -> Option<Decimal> {
    Decimal::from_str(&format!("{value:.6}")).ok()
}

fn compute_distance_pct(
    direction: AlertDirection,
    current: Decimal,
    threshold: Decimal,
) -> Option<Decimal> {
    if threshold.is_zero() {
        return None;
    }
    Some(match direction {
        AlertDirection::Above => (threshold - current) / threshold * Decimal::from(100),
        AlertDirection::Below => (current - threshold) / threshold * Decimal::from(100),
    })
}

fn ensure_review_date_alerts(conn: &Connection) -> Result<()> {
    let annotations = crate::db::annotations::list_annotations(conn)?;
    if annotations.is_empty() {
        return Ok(());
    }

    let existing = alerts::list_alerts(conn)?;
    for ann in annotations {
        let Some(review_date) = ann.review_date.clone() else {
            continue;
        };
        if chrono::NaiveDate::parse_from_str(&review_date, "%Y-%m-%d").is_err() {
            continue;
        }
        let symbol_key = format!("REVIEW:{}", ann.symbol.to_uppercase());
        let rule_text = format!(
            "Review {} thesis by {}",
            ann.symbol.to_uppercase(),
            review_date
        );

        if let Some(existing_alert) = existing
            .iter()
            .find(|a| a.kind == AlertKind::Indicator && a.symbol == symbol_key)
        {
            // Keep alert synced when review date changes.
            if existing_alert.threshold != review_date || existing_alert.rule_text != rule_text {
                conn.execute(
                    "UPDATE alerts
                     SET threshold = ?1, rule_text = ?2, status = 'armed', triggered_at = NULL
                     WHERE id = ?3",
                    rusqlite::params![review_date, rule_text, existing_alert.id],
                )?;
            }
        } else {
            let _ = alerts::add_alert(
                conn,
                alerts::NewAlert {
                    kind: "indicator",
                    symbol: &symbol_key,
                    direction: "below",
                    condition: None,
                    threshold: &review_date,
                    rule_text: &rule_text,
                    recurring: false,
                    cooldown_minutes: 0,
                },
            )?;
        }
    }
    Ok(())
}

fn ensure_scan_query_change_alerts(conn: &Connection) -> Result<()> {
    let queries = scan_queries::list_scan_queries(conn)?;
    if queries.is_empty() {
        return Ok(());
    }

    for q in queries {
        let current = crate::commands::scan::count_matches(conn, &q.filter_expr)? as i64;
        let previous: Option<i64> = conn
            .query_row(
                "SELECT last_count FROM scan_alert_state WHERE name = ?1",
                rusqlite::params![q.name],
                |r| r.get(0),
            )
            .ok();

        if let Some(prev) = previous {
            if prev != current {
                let rule_text = format!(
                    "Scan '{}' result count changed: {} -> {}",
                    q.name, prev, current
                );
                let symbol = format!("SCAN:{}", q.name.to_uppercase());
                let current_text = current.to_string();
                let id = alerts::add_alert(
                    conn,
                    alerts::NewAlert {
                        kind: "indicator",
                        symbol: &symbol,
                        direction: "above",
                        condition: None,
                        threshold: &current_text,
                        rule_text: &rule_text,
                        recurring: false,
                        cooldown_minutes: 0,
                    },
                )?;
                let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                alerts::update_alert_status(conn, id, AlertStatus::Triggered, Some(&now))?;
            }
        }

        conn.execute(
            "INSERT INTO scan_alert_state (name, last_count, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(name) DO UPDATE
             SET last_count = excluded.last_count,
                 updated_at = datetime('now')",
            rusqlite::params![q.name, current],
        )?;
    }

    Ok(())
}

fn ensure_review_date_alerts_backend(backend: &BackendConnection) -> Result<()> {
    let annotations = crate::db::annotations::list_annotations_backend(backend)?;
    if annotations.is_empty() {
        return Ok(());
    }

    let existing = alerts::list_alerts_backend(backend)?;
    for ann in annotations {
        let Some(review_date) = ann.review_date.clone() else {
            continue;
        };
        if chrono::NaiveDate::parse_from_str(&review_date, "%Y-%m-%d").is_err() {
            continue;
        }
        let symbol_key = format!("REVIEW:{}", ann.symbol.to_uppercase());
        let rule_text = format!(
            "Review {} thesis by {}",
            ann.symbol.to_uppercase(),
            review_date
        );

        if let Some(existing_alert) = existing
            .iter()
            .find(|a| a.kind == AlertKind::Indicator && a.symbol == symbol_key)
        {
            if existing_alert.threshold != review_date || existing_alert.rule_text != rule_text {
                crate::db::query::dispatch(
                    backend,
                    |conn| {
                        conn.execute(
                            "UPDATE alerts
                             SET threshold = ?1, rule_text = ?2, status = 'armed', triggered_at = NULL
                             WHERE id = ?3",
                            rusqlite::params![review_date, rule_text, existing_alert.id],
                        )?;
                        Ok(())
                    },
                    |pool| {
                        crate::db::pg_runtime::block_on(async {
                            sqlx::query(
                                "UPDATE alerts
                                 SET threshold = $1, rule_text = $2, status = 'armed', triggered_at = NULL
                                 WHERE id = $3",
                            )
                            .bind(&review_date)
                            .bind(&rule_text)
                            .bind(existing_alert.id)
                            .execute(pool)
                            .await
                        })?;
                        Ok(())
                    },
                )?;
            }
        } else {
            let _ = alerts::add_alert_backend(
                backend,
                alerts::NewAlert {
                    kind: "indicator",
                    symbol: &symbol_key,
                    direction: "below",
                    condition: None,
                    threshold: &review_date,
                    rule_text: &rule_text,
                    recurring: false,
                    cooldown_minutes: 0,
                },
            )?;
        }
    }
    Ok(())
}

fn ensure_scan_query_change_alerts_backend(backend: &BackendConnection) -> Result<()> {
    let queries = scan_queries::list_scan_queries_backend(backend)?;
    if queries.is_empty() {
        return Ok(());
    }

    for q in queries {
        let current = crate::commands::scan::count_matches_backend(backend, &q.filter_expr)? as i64;
        let previous = crate::db::query::dispatch(
            backend,
            |conn| {
                let prev: Option<i64> = conn
                    .query_row(
                        "SELECT last_count FROM scan_alert_state WHERE name = ?1",
                        rusqlite::params![q.name],
                        |r| r.get(0),
                    )
                    .ok();
                Ok(prev)
            },
            |pool| {
                let prev = crate::db::pg_runtime::block_on(async {
                    sqlx::query_scalar::<_, Option<i64>>(
                        "SELECT last_count FROM scan_alert_state WHERE name = $1",
                    )
                    .bind(&q.name)
                    .fetch_optional(pool)
                    .await
                })?;
                Ok(prev.flatten())
            },
        )?;

        if let Some(prev) = previous {
            if prev != current {
                let rule_text = format!(
                    "Scan '{}' result count changed: {} -> {}",
                    q.name, prev, current
                );
                let symbol = format!("SCAN:{}", q.name.to_uppercase());
                let current_text = current.to_string();
                let id = alerts::add_alert_backend(
                    backend,
                    alerts::NewAlert {
                        kind: "indicator",
                        symbol: &symbol,
                        direction: "above",
                        condition: None,
                        threshold: &current_text,
                        rule_text: &rule_text,
                        recurring: false,
                        cooldown_minutes: 0,
                    },
                )?;
                let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                alerts::update_alert_status_backend(
                    backend,
                    id,
                    AlertStatus::Triggered,
                    Some(&now),
                )?;
            }
        }

        crate::db::query::dispatch(
            backend,
            |conn| {
                conn.execute(
                    "INSERT INTO scan_alert_state (name, last_count, updated_at)
                     VALUES (?1, ?2, datetime('now'))
                     ON CONFLICT(name) DO UPDATE
                     SET last_count = excluded.last_count,
                         updated_at = datetime('now')",
                    rusqlite::params![q.name, current],
                )?;
                Ok(())
            },
            |pool| {
                crate::db::pg_runtime::block_on(async {
                    sqlx::query(
                        "INSERT INTO scan_alert_state (name, last_count, updated_at)
                         VALUES ($1, $2, NOW())
                         ON CONFLICT(name) DO UPDATE
                         SET last_count = EXCLUDED.last_count,
                             updated_at = NOW()",
                    )
                    .bind(&q.name)
                    .bind(current)
                    .execute(pool)
                    .await
                })?;
                Ok(())
            },
        )?;
    }

    Ok(())
}

/// Get only newly triggered alerts (convenience for refresh output).
#[allow(dead_code)] // Used by F6.5+ (TUI badge, refresh integration)
pub fn get_newly_triggered(results: &[AlertCheckResult]) -> Vec<&AlertCheckResult> {
    results.iter().filter(|r| r.newly_triggered).collect()
}

/// Get armed alerts with their distance to trigger (convenience for status display).
#[allow(dead_code)] // Used by F6.5+ (TUI status bar)
pub fn get_armed_with_distance(results: &[AlertCheckResult]) -> Vec<&AlertCheckResult> {
    results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Armed && !r.newly_triggered)
        .collect()
}

/// Count of currently triggered (not yet acknowledged) alerts.
#[allow(dead_code)] // Used by F6.5+ (TUI alert badge)
pub fn triggered_count(conn: &Connection) -> Result<i64> {
    alerts::count_by_status(conn, AlertStatus::Triggered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::price_cache;
    use crate::db::price_history;
    use crate::models::price::PriceQuote;

    fn new_alert<'a>(
        kind: &'a str,
        symbol: &'a str,
        direction: &'a str,
        condition: Option<&'a str>,
        threshold: &'a str,
        rule_text: &'a str,
    ) -> alerts::NewAlert<'a> {
        alerts::NewAlert {
            kind,
            symbol,
            direction,
            condition,
            threshold,
            rule_text,
            recurring: false,
            cooldown_minutes: 0,
        }
    }

    fn setup_test_db() -> Connection {
        let conn = open_in_memory();
        // Insert some test prices
        price_cache::upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".to_string(),
                price: Decimal::from(5600),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        price_cache::upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: Decimal::from(68000),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_check_empty_alerts() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_check_price_alert_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // GC=F is at 5600, alert for above 5500 should trigger
        alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
        assert_eq!(results[0].current_value, Some(Decimal::from(5600)));
    }

    #[test]
    fn test_check_price_alert_not_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // GC=F is at 5600, alert for above 6000 should NOT trigger
        alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "6000", "GC=F above 6000"),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_check_below_alert_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // BTC is at 68000, alert for below 70000 should trigger
        alerts::add_alert(
            conn,
            new_alert("price", "BTC", "below", None, "70000", "BTC below 70000"),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
    }

    #[test]
    fn test_already_triggered_not_newly() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let id = alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        // Manually mark as triggered
        alerts::update_alert_status(conn, id, AlertStatus::Triggered, Some("2026-03-04")).unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_distance_calculation() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // GC=F at 5600, threshold 6000 above → distance = (6000-5600)/6000 * 100 ≈ 6.67%
        alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "6000", "GC=F above 6000"),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        let dist = results[0].distance_pct.unwrap();
        // (6000 - 5600) / 6000 * 100 = 6.666...
        assert!(dist > Decimal::from(6) && dist < Decimal::from(7));
    }

    #[test]
    fn test_unknown_symbol_no_crash() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        alerts::add_alert(
            conn,
            new_alert(
                "price",
                "UNKNOWN",
                "above",
                None,
                "100",
                "UNKNOWN above 100",
            ),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
        assert!(results[0].current_value.is_none());
    }

    #[test]
    fn test_get_newly_triggered_filter() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // One that will trigger, one that won't
        alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "6000", "GC=F above 6000"),
        )
        .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        let newly = get_newly_triggered(&results);
        assert_eq!(newly.len(), 1);
        assert_eq!(newly[0].rule.threshold, "5500");
    }

    #[test]
    fn test_triggered_count() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        assert_eq!(triggered_count(conn).unwrap(), 0);

        alerts::add_alert(
            conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        check_alerts_backend(&backend, conn).unwrap(); // triggers it

        assert_eq!(triggered_count(conn).unwrap(), 1);
    }

    #[test]
    fn test_review_date_alert_auto_created_and_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let ann = crate::db::annotations::Annotation {
            symbol: "GC=F".to_string(),
            thesis: "Test".to_string(),
            invalidation: None,
            review_date: Some("2000-01-01".to_string()),
            target_price: None,
            updated_at: String::new(),
        };
        crate::db::annotations::upsert_annotation(conn, &ann).unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results.iter().any(|r| r.rule.symbol == "REVIEW:GC=F"));
        assert!(results.iter().any(|r| r.rule.symbol == "REVIEW:GC=F"
            && (r.newly_triggered || r.rule.status == AlertStatus::Triggered)));
    }

    #[test]
    fn test_scan_query_change_creates_triggered_alert() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        crate::db::scan_queries::upsert_scan_query(conn, "equities", "category == equity").unwrap();

        // First check seeds baseline state and should not emit change alert.
        let _ = check_alerts_backend(&backend, conn).unwrap();
        assert!(alerts::list_alerts(conn)
            .unwrap()
            .iter()
            .all(|a| !a.symbol.starts_with("SCAN:")));

        // Add one matching position and check again -> should emit triggered scan alert.
        crate::db::transactions::insert_transaction(
            conn,
            &crate::models::transaction::NewTransaction {
                symbol: "AAPL".to_string(),
                category: crate::models::asset::AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: Decimal::from(2),
                price_per: Decimal::from(180),
                currency: "USD".to_string(),
                date: "2026-03-09".to_string(),
                notes: None,
            },
        )
        .unwrap();
        let _ = check_alerts_backend(&backend, conn).unwrap();
        let all = alerts::list_alerts(conn).unwrap();
        let scan_alerts: Vec<_> = all
            .iter()
            .filter(|a| a.symbol.starts_with("SCAN:"))
            .collect();
        assert_eq!(scan_alerts.len(), 1);
        assert_eq!(scan_alerts[0].status, AlertStatus::Triggered);
    }

    #[test]
    fn test_trackline_breach_scan_query_creates_triggered_alert() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        crate::db::scan_queries::upsert_scan_query(
            conn,
            "sma50-breaches",
            "trackline_breach contains below_sma50",
        )
        .unwrap();

        let _ = check_alerts_backend(&backend, conn).unwrap();

        crate::db::transactions::insert_transaction(
            conn,
            &crate::models::transaction::NewTransaction {
                symbol: "AAPL".to_string(),
                category: crate::models::asset::AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: Decimal::from(2),
                price_per: Decimal::from(180),
                currency: "USD".to_string(),
                date: "2026-03-09".to_string(),
                notes: None,
            },
        )
        .unwrap();
        price_cache::upsert_price(
            conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: Decimal::from(95),
                currency: "USD".to_string(),
                fetched_at: "2026-03-10T00:00:00Z".to_string(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        let base_date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let history: Vec<crate::models::price::HistoryRecord> = (0..60)
            .map(|day| crate::models::price::HistoryRecord {
                date: (base_date + chrono::Duration::days(day))
                    .format("%Y-%m-%d")
                    .to_string(),
                close: Decimal::from(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        price_history::upsert_history(conn, "AAPL", "test", &history).unwrap();
        assert_eq!(
            crate::commands::scan::count_matches(conn, "trackline_breach contains below_sma50")
                .unwrap(),
            1
        );

        let _ = check_alerts_backend(&backend, conn).unwrap();
        let all = alerts::list_alerts(conn).unwrap();
        let scan_alerts: Vec<_> = all
            .iter()
            .filter(|a| a.symbol == "SCAN:SMA50-BREACHES")
            .collect();
        assert_eq!(scan_alerts.len(), 1);
        assert_eq!(scan_alerts[0].status, AlertStatus::Triggered);
    }

    #[test]
    fn test_technical_price_below_sma50_triggers() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let mut history = Vec::new();
        for day in 0..60 {
            let close = if day == 59 {
                Decimal::from(50)
            } else {
                Decimal::from(100)
            };
            history.push(crate::models::price::HistoryRecord {
                date: format!("2026-{:02}-{:02}", 1 + (day / 28), 1 + (day % 28)),
                close,
                volume: None,
                open: None,
                high: None,
                low: None,
            });
        }
        crate::db::price_history::upsert_history(conn, "TECH", "test", &history).unwrap();
        crate::db::price_cache::upsert_price(
            conn,
            &PriceQuote {
                symbol: "TECH".to_string(),
                price: Decimal::from(50),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        alerts::add_alert(
            conn,
            new_alert(
                "technical",
                "TECH",
                "below",
                Some("price_below_sma50"),
                "50",
                "TECH below SMA50",
            ),
        )
        .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results
            .iter()
            .any(|row| { row.rule.kind == AlertKind::Technical && row.newly_triggered }));
    }

    #[test]
    fn test_indicator_rsi_alert_evaluates_current_value() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let mut history = Vec::new();
        for day in 0..30 {
            history.push(crate::models::price::HistoryRecord {
                date: format!("2026-02-{:02}", day + 1),
                close: Decimal::from(100 + day),
                volume: None,
                open: None,
                high: None,
                low: None,
            });
        }
        crate::db::price_history::upsert_history(conn, "RSI", "test", &history).unwrap();
        alerts::add_alert(
            conn,
            new_alert(
                "indicator",
                "RSI RSI",
                "above",
                None,
                "70",
                "RSI RSI above 70",
            ),
        )
        .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        let row = results
            .iter()
            .find(|row| row.rule.symbol == "RSI RSI")
            .unwrap();
        assert!(row.current_value.is_some());
        assert!(row.newly_triggered);
    }

    #[test]
    fn test_indicator_sma_cross_rule_triggers() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let mut history = Vec::new();
        for day in 0..60 {
            let close = if day == 59 {
                Decimal::from(50)
            } else {
                Decimal::from(100)
            };
            history.push(crate::models::price::HistoryRecord {
                date: format!("2026-{:02}-{:02}", 1 + (day / 28), 1 + (day % 28)),
                close,
                volume: None,
                open: None,
                high: None,
                low: None,
            });
        }
        crate::db::price_history::upsert_history(conn, "SMA", "test", &history).unwrap();
        crate::db::price_cache::upsert_price(
            conn,
            &PriceQuote {
                symbol: "SMA".to_string(),
                price: Decimal::from(50),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        let parsed = crate::alerts::rules::parse_rule("SMA below SMA50").unwrap();
        alerts::add_alert(
            conn,
            new_alert(
                "indicator",
                &parsed.symbol,
                &parsed.direction.to_string(),
                None,
                &parsed.threshold.to_string(),
                &parsed.rule_text,
            ),
        )
        .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results
            .iter()
            .any(|row| row.rule.symbol == "SMA SMA50" && row.newly_triggered));
    }

    #[test]
    fn test_indicator_change_pct_rule_triggers() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let history = vec![
            crate::models::price::HistoryRecord {
                date: "2026-03-01".to_string(),
                close: Decimal::from(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            crate::models::price::HistoryRecord {
                date: "2026-03-02".to_string(),
                close: Decimal::from(107),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];
        crate::db::price_history::upsert_history(conn, "MOVE", "test", &history).unwrap();
        crate::db::price_cache::upsert_price(
            conn,
            &PriceQuote {
                symbol: "MOVE".to_string(),
                price: Decimal::from(107),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        let parsed = crate::alerts::rules::parse_rule("MOVE change above 5%").unwrap();
        alerts::add_alert(
            conn,
            new_alert(
                "indicator",
                &parsed.symbol,
                &parsed.direction.to_string(),
                None,
                &parsed.threshold.to_string(),
                &parsed.rule_text,
            ),
        )
        .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results
            .iter()
            .any(|row| row.rule.symbol == "MOVE CHANGE_PCT" && row.newly_triggered));
    }

    #[test]
    fn effective_cooldown_uses_per_alert_when_set() {
        assert_eq!(effective_cooldown_minutes(60, 30), 60);
        assert_eq!(effective_cooldown_minutes(240, 30), 240);
    }

    #[test]
    fn effective_cooldown_falls_back_to_default_when_zero() {
        assert_eq!(effective_cooldown_minutes(0, 30), 30);
        assert_eq!(effective_cooldown_minutes(0, 60), 60);
    }

    #[test]
    fn effective_cooldown_zero_when_both_zero() {
        // If the global default is also 0, no cooldown is enforced.
        assert_eq!(effective_cooldown_minutes(0, 0), 0);
    }

    #[test]
    fn cooldown_elapsed_respects_window() {
        let now = chrono::Utc::now();
        let five_min_ago = (now - chrono::Duration::minutes(5))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        // 5 minutes ago, cooldown 30 minutes → NOT elapsed
        assert!(!cooldown_elapsed(&five_min_ago, 30));
        // 5 minutes ago, cooldown 3 minutes → elapsed
        assert!(cooldown_elapsed(&five_min_ago, 3));
        // cooldown 0 → always elapsed
        assert!(cooldown_elapsed(&five_min_ago, 0));
    }

    #[test]
    fn recurring_alert_suppressed_within_default_cooldown() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // Create a recurring alert with cooldown_minutes=0 (relies on default)
        let id = alerts::add_alert(
            conn,
            alerts::NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5500",
                rule_text: "GC=F above 5500 (recurring)",
                recurring: true,
                cooldown_minutes: 0,
            },
        )
        .unwrap();

        // First check with default_cooldown=30 → should trigger
        let price_map: HashMap<String, Decimal> =
            [("GC=F".to_string(), Decimal::from(5600))].into();
        let result = check_single_alert_sqlite(
            conn,
            &alerts::get_alert(conn, id).unwrap().unwrap(),
            &price_map,
            30,
        )
        .unwrap();
        assert!(result.newly_triggered);

        // Second check immediately → should be suppressed by the 30-minute default cooldown
        let alert = alerts::get_alert(conn, id).unwrap().unwrap();
        let result2 = check_single_alert_sqlite(conn, &alert, &price_map, 30).unwrap();
        assert!(!result2.newly_triggered);
    }

    #[test]
    fn recurring_alert_fires_immediately_when_default_cooldown_zero() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        let id = alerts::add_alert(
            conn,
            alerts::NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5500",
                rule_text: "GC=F above 5500 (recurring, no cooldown)",
                recurring: true,
                cooldown_minutes: 0,
            },
        )
        .unwrap();

        let price_map: HashMap<String, Decimal> =
            [("GC=F".to_string(), Decimal::from(5600))].into();

        // First trigger
        let result = check_single_alert_sqlite(
            conn,
            &alerts::get_alert(conn, id).unwrap().unwrap(),
            &price_map,
            0, // default cooldown also 0
        )
        .unwrap();
        assert!(result.newly_triggered);

        // Second trigger immediately → should also fire (no cooldown)
        let alert = alerts::get_alert(conn, id).unwrap().unwrap();
        let result2 = check_single_alert_sqlite(conn, &alert, &price_map, 0).unwrap();
        assert!(result2.newly_triggered);
    }

    #[test]
    fn test_ratio_alert_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // GC=F is at 5600, BTC is at 68000. Ratio GC=F/BTC = 5600/68000 ≈ 0.082
        // Alert for ratio below 0.1 should trigger
        alerts::add_alert(
            conn,
            new_alert(
                "ratio",
                "GC=F/BTC",
                "below",
                None,
                "0.1",
                "GC=F/BTC below 0.1",
            ),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
        assert!(results[0].current_value.is_some());
    }

    #[test]
    fn test_ratio_alert_not_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // GC=F=5600, BTC=68000. Ratio = 0.082. Alert for above 0.1 should NOT trigger.
        alerts::add_alert(
            conn,
            new_alert(
                "ratio",
                "GC=F/BTC",
                "above",
                None,
                "0.1",
                "GC=F/BTC above 0.1",
            ),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_ratio_alert_above_triggered() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // BTC=68000, GC=F=5600. Ratio BTC/GC=F = 68000/5600 ≈ 12.14
        // Alert for above 12 should trigger.
        alerts::add_alert(
            conn,
            new_alert(
                "ratio",
                "BTC/GC=F",
                "above",
                None,
                "12",
                "BTC/GC=F above 12",
            ),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
    }

    #[test]
    fn test_ratio_alert_missing_denominator() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // UNKNOWN not in price cache → ratio can't be computed → not triggered
        alerts::add_alert(
            conn,
            new_alert(
                "ratio",
                "GC=F/UNKNOWN",
                "above",
                None,
                "1",
                "GC=F/UNKNOWN above 1",
            ),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
        assert!(results[0].current_value.is_none());
    }

    #[test]
    fn test_ratio_alert_invalid_symbol_format() {
        let backend = BackendConnection::Sqlite {
            conn: setup_test_db(),
        };
        let conn = backend.sqlite();
        // Symbol without '/' → invalid format → not triggered
        alerts::add_alert(
            conn,
            new_alert(
                "ratio",
                "NOSEPARATOR",
                "above",
                None,
                "1",
                "NOSEPARATOR above 1",
            ),
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }
}
