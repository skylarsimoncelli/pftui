use anyhow::Result;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

use crate::db::backend::BackendConnection;
use crate::db::{
    correlation_snapshots, economic_cache, price_cache, price_history, regime_snapshots,
    scenarios, sentiment_cache,
};

#[derive(Debug, Clone)]
pub struct MacroAlertEvaluation {
    pub triggered: bool,
    pub current_value: Option<Decimal>,
    pub trigger_data: Value,
}

pub fn evaluate_condition(
    backend: &BackendConnection,
    condition: &str,
    threshold: &str,
) -> Result<MacroAlertEvaluation> {
    match condition {
        "regime_change" => evaluate_regime_change(backend),
        "vix_regime_shift" => evaluate_vix_regime_shift(backend),
        "fear_greed_extreme" => evaluate_fear_greed_extreme(backend),
        "yield_curve_inversion_change" => evaluate_yield_curve_inversion_change(backend),
        "dxy_century_cross" => evaluate_dxy_century_cross(backend),
        "correlation_regime_break" => {
            let thresh = threshold.parse::<f64>().unwrap_or(DEFAULT_CORRELATION_BREAK_THRESHOLD);
            evaluate_correlation_regime_break(backend, thresh)
        }
        "scenario_probability_shift" => {
            let thresh_pp = threshold.parse::<f64>().unwrap_or(DEFAULT_SCENARIO_SHIFT_THRESHOLD_PP);
            evaluate_scenario_probability_shift(backend, thresh_pp)
        }
        other => Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "condition": other, "error": "unsupported_condition" }),
        }),
    }
}

/// Default correlation break threshold (delta between 30d and 90d correlation).
const DEFAULT_CORRELATION_BREAK_THRESHOLD: f64 = 0.3;

/// Default scenario probability shift threshold in percentage points.
const DEFAULT_SCENARIO_SHIFT_THRESHOLD_PP: f64 = 10.0;

fn evaluate_regime_change(backend: &BackendConnection) -> Result<MacroAlertEvaluation> {
    let history = regime_snapshots::get_history_backend(backend, Some(2))?;
    if history.len() < 2 {
        return Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "reason": "insufficient_regime_history" }),
        });
    }
    let current = &history[0];
    let previous = &history[1];
    Ok(MacroAlertEvaluation {
        triggered: current.regime != previous.regime,
        current_value: None,
        trigger_data: json!({
            "previous_regime": previous.regime,
            "current_regime": current.regime,
            "recorded_at": current.recorded_at
        }),
    })
}

fn evaluate_vix_regime_shift(backend: &BackendConnection) -> Result<MacroAlertEvaluation> {
    let current = price_cache::get_cached_price_backend(backend, "^VIX", "USD")?
        .map(|q| q.price)
        .or_else(|| {
            price_cache::get_cached_price_backend(backend, "VIX", "USD")
                .ok()
                .flatten()
                .map(|q| q.price)
        });
    let history = price_history::get_history_backend(backend, "^VIX", 2)?;
    let previous = history.iter().rev().nth(1).map(|r| r.close);
    let thresholds = [20, 25, 30, 35];
    let crossed = match (previous, current) {
        (Some(prev), Some(curr)) => thresholds.iter().find_map(|threshold| {
            let level = Decimal::from(*threshold);
            let crossed_up = prev < level && curr >= level;
            let crossed_down = prev > level && curr <= level;
            if crossed_up || crossed_down {
                Some((*threshold, if crossed_up { "up" } else { "down" }))
            } else {
                None
            }
        }),
        _ => None,
    };

    Ok(MacroAlertEvaluation {
        triggered: crossed.is_some(),
        current_value: current,
        trigger_data: json!({
            "current_vix": current.map(|v| v.to_string()),
            "previous_vix": previous.map(|v| v.to_string()),
            "crossed_threshold": crossed.map(|c| c.0),
            "direction": crossed.map(|c| c.1)
        }),
    })
}

fn evaluate_fear_greed_extreme(backend: &BackendConnection) -> Result<MacroAlertEvaluation> {
    let reading = sentiment_cache::get_latest_backend(backend, "traditional_fng")?
        .or_else(|| {
            sentiment_cache::get_latest_backend(backend, "traditional")
                .ok()
                .flatten()
        })
        .or_else(|| {
            sentiment_cache::get_latest_backend(backend, "crypto_fng")
                .ok()
                .flatten()
        });
    let Some(reading) = reading else {
        return Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "reason": "missing_sentiment" }),
        });
    };
    let value = Decimal::from(reading.value);
    Ok(MacroAlertEvaluation {
        triggered: reading.value <= 15 || reading.value >= 85,
        current_value: Some(value),
        trigger_data: json!({
            "index_type": reading.index_type,
            "value": reading.value,
            "classification": reading.classification
        }),
    })
}

fn evaluate_yield_curve_inversion_change(
    backend: &BackendConnection,
) -> Result<MacroAlertEvaluation> {
    let history = economic_cache::get_history_backend(backend, "T10Y2Y", 2)?;
    if history.len() < 2 {
        return Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "reason": "insufficient_yield_curve_history" }),
        });
    }
    let previous = &history[history.len() - 2];
    let current = &history[history.len() - 1];
    let sign_change = (previous.value.is_sign_negative() && current.value.is_sign_positive())
        || (previous.value.is_sign_positive() && current.value.is_sign_negative())
        || (previous.value.is_zero() && !current.value.is_zero());
    Ok(MacroAlertEvaluation {
        triggered: sign_change,
        current_value: Some(current.value),
        trigger_data: json!({
            "series": "T10Y2Y",
            "previous": previous.value.to_string(),
            "current": current.value.to_string(),
            "current_date": current.date
        }),
    })
}

fn evaluate_dxy_century_cross(backend: &BackendConnection) -> Result<MacroAlertEvaluation> {
    let current = price_cache::get_cached_price_backend(backend, "DX-Y.NYB", "USD")?
        .map(|q| q.price)
        .or_else(|| {
            price_cache::get_cached_price_backend(backend, "DXY", "USD")
                .ok()
                .flatten()
                .map(|q| q.price)
        });
    let history = price_history::get_history_backend(backend, "DX-Y.NYB", 2)?;
    let previous = history.iter().rev().nth(1).map(|r| r.close);
    let level = Decimal::from(100);
    let crossed = match (previous, current) {
        (Some(prev), Some(curr)) if prev < level && curr >= level => Some("up"),
        (Some(prev), Some(curr)) if prev > level && curr <= level => Some("down"),
        _ => None,
    };
    Ok(MacroAlertEvaluation {
        triggered: crossed.is_some(),
        current_value: current,
        trigger_data: json!({
            "previous_dxy": previous.map(|v| v.to_string()),
            "current_dxy": current.map(|v| v.to_string()),
            "direction": crossed
        }),
    })
}

fn evaluate_correlation_regime_break(
    backend: &BackendConnection,
    threshold: f64,
) -> Result<MacroAlertEvaluation> {
    let rows_30 = correlation_snapshots::list_current_backend(backend, Some("30d"))?;
    let rows_90 = correlation_snapshots::list_current_backend(backend, Some("90d"))?;
    let mut best: Option<(String, String, f64)> = None;

    for short in &rows_30 {
        if let Some(long) = rows_90
            .iter()
            .find(|row| row.symbol_a == short.symbol_a && row.symbol_b == short.symbol_b)
        {
            let delta = (short.correlation - long.correlation).abs();
            if best.as_ref().map(|(_, _, d)| delta > *d).unwrap_or(true) {
                best = Some((short.symbol_a.clone(), short.symbol_b.clone(), delta));
            }
        }
    }

    let Some((symbol_a, symbol_b, delta)) = best else {
        return Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "reason": "missing_correlation_snapshots" }),
        });
    };

    let current_value = Decimal::from_str(&format!("{delta:.6}")).ok();
    Ok(MacroAlertEvaluation {
        triggered: delta >= threshold,
        current_value,
        trigger_data: json!({
            "symbol_a": symbol_a,
            "symbol_b": symbol_b,
            "delta": delta,
            "threshold": threshold
        }),
    })
}

fn evaluate_scenario_probability_shift(
    backend: &BackendConnection,
    threshold_pp: f64,
) -> Result<MacroAlertEvaluation> {
    let active = scenarios::list_scenarios_backend(backend, Some("active"))?;
    if active.is_empty() {
        return Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "reason": "no_active_scenarios" }),
        });
    }

    // Find the largest recent shift across all active scenarios
    let mut best_shift: Option<(String, f64, f64, f64, Option<String>)> = None; // (name, old, new, delta, driver)

    for scenario in &active {
        let history = scenarios::get_history_backend(backend, scenario.id, Some(2))?;
        if history.len() < 2 {
            continue;
        }
        // history is ordered DESC (newest first)
        let current_prob = history[0].probability;
        let previous_prob = history[1].probability;
        let delta = current_prob - previous_prob;
        let abs_delta = delta.abs();

        if abs_delta > best_shift.as_ref().map(|(_, _, _, d, _)| d.abs()).unwrap_or(0.0) {
            best_shift = Some((
                scenario.name.clone(),
                previous_prob,
                current_prob,
                delta,
                history[0].driver.clone(),
            ));
        }
    }

    let Some((name, old_prob, new_prob, delta, driver)) = best_shift else {
        return Ok(MacroAlertEvaluation {
            triggered: false,
            current_value: None,
            trigger_data: json!({ "reason": "no_scenario_history" }),
        });
    };

    let abs_delta = delta.abs();
    let current_value = Decimal::from_str(&format!("{abs_delta:.1}")).ok();

    Ok(MacroAlertEvaluation {
        triggered: abs_delta >= threshold_pp,
        current_value,
        trigger_data: json!({
            "scenario_name": name,
            "old_probability": old_prob,
            "new_probability": new_prob,
            "delta_pp": delta,
            "threshold_pp": threshold_pp,
            "driver": driver
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_correlation_break_threshold() {
        assert!((DEFAULT_CORRELATION_BREAK_THRESHOLD - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_default_scenario_shift_threshold() {
        assert!((DEFAULT_SCENARIO_SHIFT_THRESHOLD_PP - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_evaluate_condition_unknown_condition() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let eval = evaluate_condition(&backend, "nonexistent_condition", "0").unwrap();
        assert!(!eval.triggered);
        assert_eq!(eval.trigger_data["error"], "unsupported_condition");
    }

    #[test]
    fn test_evaluate_scenario_shift_no_scenarios() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let eval = evaluate_scenario_probability_shift(&backend, 10.0).unwrap();
        assert!(!eval.triggered);
        assert_eq!(eval.trigger_data["reason"], "no_active_scenarios");
    }
}
