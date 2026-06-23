//! Cycle-bottom signal alert evaluation.
//!
//! Wires the mechanical 7-composite cycle-bottom signal suite
//! (`crate::analytics::cycle_signals`) into the alert engine so that
//! confluence-threshold and single-criterion conditions are raised
//! automatically on every `data refresh`.
//!
//! Two condition shapes are supported, both carried on the existing
//! `Technical` alert kind via the `condition` string (no new AlertKind,
//! no new storage — edge-triggering reuses the standard armed→triggered
//! fired-state machinery):
//!
//! 1. **Confluence threshold** — `cycle_bottom_<timeframe>_<N>`
//!    (e.g. `cycle_bottom_monthly_4`). Fires when the asset's met/7 read on
//!    `<timeframe>` reaches/exceeds `<N>`. `current_value` is the met count.
//!
//! 2. **Single criterion** — `cycle_criterion_<timeframe>_<criterion_key>`
//!    (e.g. `cycle_criterion_weekly_trend_line_reclaimed`). Fires when that
//!    one named composite criterion is met on `<timeframe>`.
//!
//! The evaluation here is pure (no DB): it takes an already-computed
//! `Option<CycleBottomSignals>` so the transition/edge-trigger semantics can
//! be unit-tested with synthetic engine output. Edge-triggering itself is
//! handled by the engine's standard `should_log_trigger` contract: a
//! non-recurring alert fires once when it newly becomes true (status flips
//! Armed→Triggered) and re-arming re-enables it; a recurring alert respects
//! its cooldown.

use rust_decimal::Decimal;
use serde_json::{json, Value};

use crate::analytics::cycle_signals::{CycleBottomSignals, SignalTimeframe};

/// Condition-string prefix for confluence-threshold alerts.
pub const CONFLUENCE_PREFIX: &str = "cycle_bottom_";
/// Condition-string prefix for single-criterion alerts.
pub const CRITERION_PREFIX: &str = "cycle_criterion_";

/// The 7 composite criterion keys, as emitted by the signal engine. Used to
/// disambiguate the timeframe token from the criterion key when parsing a
/// single-criterion condition (criterion keys themselves contain `_`).
pub const CRITERION_KEYS: [&str; 7] = [
    "momentum_turning_up",
    "momentum_above_price",
    "dss_bottoming",
    "roofing_confirming_up",
    "volatility_bands_bullish",
    "reversal_dots",
    "trend_line_reclaimed",
];

/// A parsed cycle-signal alert condition.
#[derive(Debug, Clone, PartialEq)]
pub enum CycleSignalCondition {
    /// Fire when met_count >= target on the given timeframe.
    Confluence {
        timeframe: SignalTimeframe,
        target: usize,
    },
    /// Fire when the named composite criterion is met on the given timeframe.
    Criterion {
        timeframe: SignalTimeframe,
        criterion_key: String,
    },
}

/// Returns true if a condition string is a cycle-signal alert condition.
pub fn is_cycle_signal_condition(condition: &str) -> bool {
    condition.starts_with(CONFLUENCE_PREFIX) || condition.starts_with(CRITERION_PREFIX)
}

/// Parse a cycle-signal condition string into its typed form.
///
/// Confluence: `cycle_bottom_<timeframe>_<N>`.
/// Criterion:  `cycle_criterion_<timeframe>_<criterion_key>`.
pub fn parse_condition(condition: &str) -> anyhow::Result<CycleSignalCondition> {
    if let Some(rest) = condition.strip_prefix(CRITERION_PREFIX) {
        // rest = "<timeframe>_<criterion_key>"
        let (tf_token, key) = rest.split_once('_').ok_or_else(|| {
            anyhow::anyhow!(
                "invalid cycle criterion condition '{condition}' — expected \
                 cycle_criterion_<timeframe>_<criterion_key>"
            )
        })?;
        let timeframe = SignalTimeframe::parse(tf_token)?;
        if !CRITERION_KEYS.contains(&key) {
            anyhow::bail!(
                "unknown cycle criterion key '{key}' — expected one of: {}",
                CRITERION_KEYS.join(", ")
            );
        }
        return Ok(CycleSignalCondition::Criterion {
            timeframe,
            criterion_key: key.to_string(),
        });
    }

    if let Some(rest) = condition.strip_prefix(CONFLUENCE_PREFIX) {
        // rest = "<timeframe>_<N>"
        let (tf_token, n_token) = rest.rsplit_once('_').ok_or_else(|| {
            anyhow::anyhow!(
                "invalid cycle confluence condition '{condition}' — expected \
                 cycle_bottom_<timeframe>_<N>"
            )
        })?;
        let timeframe = SignalTimeframe::parse(tf_token)?;
        let target: usize = n_token.parse().map_err(|_| {
            anyhow::anyhow!("invalid confluence target '{n_token}' in '{condition}' — expected 1..=7")
        })?;
        if target == 0 || target > 7 {
            anyhow::bail!("confluence target must be 1..=7, got {target}");
        }
        return Ok(CycleSignalCondition::Confluence { timeframe, target });
    }

    anyhow::bail!("not a cycle-signal condition: {condition}")
}

/// The timeframe a condition runs on (so the caller knows which signal read to
/// compute).
pub fn condition_timeframe(condition: &str) -> anyhow::Result<SignalTimeframe> {
    Ok(match parse_condition(condition)? {
        CycleSignalCondition::Confluence { timeframe, .. } => timeframe,
        CycleSignalCondition::Criterion { timeframe, .. } => timeframe,
    })
}

/// Outcome of evaluating a cycle-signal condition against a computed read.
#[derive(Debug, Clone)]
pub struct CycleSignalEval {
    pub is_triggered: bool,
    /// For confluence: the met count. For criterion: 1 if met else 0.
    pub current_value: Option<Decimal>,
    pub trigger_data: Value,
}

/// Friendly, name-free asset label for the alert message.
pub fn friendly_asset(symbol: &str) -> String {
    let up = symbol.to_uppercase();
    let base = up.split(['/', ':']).next().unwrap_or(&up);
    match base {
        "BTC-USD" | "BTC" | "BTCUSD" | "XBT" => "Bitcoin".to_string(),
        "ETH-USD" | "ETH" | "ETHUSD" => "Ethereum".to_string(),
        "GC=F" | "XAU" | "XAUUSD" | "GLD" => "Gold".to_string(),
        "SI=F" | "XAG" | "XAGUSD" | "SLV" => "Silver".to_string(),
        other => other.to_string(),
    }
}

/// Human label for a composite criterion key (no practitioner names).
pub fn criterion_label(key: &str) -> &'static str {
    match key {
        "momentum_turning_up" => "momentum line turning up",
        "momentum_above_price" => "momentum line above price momentum",
        "dss_bottoming" => "double-smoothed stochastic bottoming",
        "roofing_confirming_up" => "roofing filter confirming up",
        "volatility_bands_bullish" => "volatility bands bullish",
        "reversal_dots" => "significant reversal dots",
        "trend_line_reclaimed" => "trend line reclaimed",
        _ => "cycle-bottom criterion",
    }
}

/// Evaluate a parsed cycle-signal condition against an already-computed read.
///
/// `signals` is `None` when history was too shallow to compute — the alert is
/// simply not triggered (no panic, no false fire).
pub fn evaluate(
    symbol: &str,
    parsed: &CycleSignalCondition,
    signals: Option<&CycleBottomSignals>,
) -> CycleSignalEval {
    let asset = friendly_asset(symbol);
    let Some(sig) = signals else {
        return CycleSignalEval {
            is_triggered: false,
            current_value: None,
            trigger_data: json!({
                "kind": "cycle_bottom_signal",
                "reason": "insufficient_history",
                "symbol": symbol,
            }),
        };
    };

    match parsed {
        CycleSignalCondition::Confluence { timeframe, target } => {
            let met = sig.met_count;
            let is_triggered = met >= *target;
            CycleSignalEval {
                is_triggered,
                current_value: Some(Decimal::from(met)),
                trigger_data: json!({
                    "kind": "cycle_bottom_confluence",
                    "symbol": symbol,
                    "asset": asset,
                    "timeframe": timeframe.label(),
                    "met_count": met,
                    "total": sig.total,
                    "target": target,
                    "as_of": sig.as_of,
                    "message": format!(
                        "{asset} {} cycle-bottom signals {met}/{} (≥{target} target met)",
                        timeframe.label(),
                        sig.total
                    ),
                }),
            }
        }
        CycleSignalCondition::Criterion {
            timeframe,
            criterion_key,
        } => {
            let criterion = sig.criteria.iter().find(|c| &c.key == criterion_key);
            let is_triggered = criterion.map(|c| c.met).unwrap_or(false);
            CycleSignalEval {
                is_triggered,
                current_value: Some(Decimal::from(if is_triggered { 1u8 } else { 0u8 })),
                trigger_data: json!({
                    "kind": "cycle_bottom_criterion",
                    "symbol": symbol,
                    "asset": asset,
                    "timeframe": timeframe.label(),
                    "criterion_key": criterion_key,
                    "criterion_label": criterion_label(criterion_key),
                    "met": is_triggered,
                    "met_count": sig.met_count,
                    "total": sig.total,
                    "as_of": sig.as_of,
                    "message": format!(
                        "{asset} {} {} (cycle-bottom signals {}/{})",
                        timeframe.label(),
                        criterion_label(criterion_key),
                        sig.met_count,
                        sig.total
                    ),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::cycle_signals::{Criterion, CycleBottomSignals, SignalTimeframe};

    fn synthetic_signals(timeframe: SignalTimeframe, met_keys: &[&str]) -> CycleBottomSignals {
        let criteria: Vec<Criterion> = CRITERION_KEYS
            .iter()
            .map(|k| Criterion {
                key: k.to_string(),
                label: k.to_string(),
                met: met_keys.contains(k),
                detail: String::new(),
                components: vec![],
            })
            .collect();
        let met_count = criteria.iter().filter(|c| c.met).count();
        CycleBottomSignals {
            symbol: "BTC-USD".to_string(),
            timeframe,
            as_of: "2026-06-01".to_string(),
            rsi: None,
            rsi_ma: None,
            rsi_ma_turned_up: false,
            rsi_ma_cross_above_rsi: false,
            dss: None,
            dss_trigger: None,
            dss_turned_up: false,
            dss_cross_above_trigger: false,
            dss_oversold: false,
            erf: None,
            erf_green: false,
            erf_turned_up: false,
            cyberbands_state: None,
            cyberbands_bullish: false,
            cyberdots_weekly_strength: None,
            cyberdots_monthly_strength: None,
            cyberdots_bullish: false,
            cyberline_value: None,
            cyberline_price_above: None,
            cyberline_reclaim: false,
            pi_cycle_bottom: false,
            pi_cycle_last_bottom: None,
            criteria,
            met_count,
            total: 7,
            bonus: None,
            verdict: String::new(),
        }
    }

    #[test]
    fn parse_confluence_condition() {
        let parsed = parse_condition("cycle_bottom_monthly_4").unwrap();
        assert_eq!(
            parsed,
            CycleSignalCondition::Confluence {
                timeframe: SignalTimeframe::Monthly,
                target: 4,
            }
        );
    }

    #[test]
    fn parse_criterion_condition() {
        let parsed = parse_condition("cycle_criterion_weekly_trend_line_reclaimed").unwrap();
        assert_eq!(
            parsed,
            CycleSignalCondition::Criterion {
                timeframe: SignalTimeframe::Weekly,
                criterion_key: "trend_line_reclaimed".to_string(),
            }
        );
    }

    #[test]
    fn parse_rejects_unknown_criterion() {
        assert!(parse_condition("cycle_criterion_weekly_not_a_real_key").is_err());
    }

    #[test]
    fn parse_rejects_out_of_range_target() {
        assert!(parse_condition("cycle_bottom_monthly_0").is_err());
        assert!(parse_condition("cycle_bottom_monthly_8").is_err());
    }

    #[test]
    fn confluence_triggers_at_or_above_target() {
        let parsed = parse_condition("cycle_bottom_monthly_3").unwrap();
        let sig = synthetic_signals(
            SignalTimeframe::Monthly,
            &["momentum_turning_up", "dss_bottoming", "reversal_dots"],
        );
        let eval = evaluate("BTC-USD", &parsed, Some(&sig));
        assert!(eval.is_triggered);
        assert_eq!(eval.current_value, Some(Decimal::from(3)));
    }

    #[test]
    fn confluence_below_target_does_not_trigger() {
        let parsed = parse_condition("cycle_bottom_monthly_4").unwrap();
        let sig = synthetic_signals(
            SignalTimeframe::Monthly,
            &["momentum_turning_up", "dss_bottoming"],
        );
        let eval = evaluate("BTC-USD", &parsed, Some(&sig));
        assert!(!eval.is_triggered);
        assert_eq!(eval.current_value, Some(Decimal::from(2)));
    }

    #[test]
    fn single_criterion_triggers_only_when_met() {
        let parsed = parse_condition("cycle_criterion_weekly_trend_line_reclaimed").unwrap();
        let met = synthetic_signals(SignalTimeframe::Weekly, &["trend_line_reclaimed"]);
        assert!(evaluate("BTC-USD", &parsed, Some(&met)).is_triggered);

        let not_met = synthetic_signals(SignalTimeframe::Weekly, &["dss_bottoming"]);
        assert!(!evaluate("BTC-USD", &parsed, Some(&not_met)).is_triggered);
    }

    #[test]
    fn insufficient_history_never_triggers() {
        let parsed = parse_condition("cycle_bottom_monthly_1").unwrap();
        let eval = evaluate("BTC-USD", &parsed, None);
        assert!(!eval.is_triggered);
        assert_eq!(eval.current_value, None);
    }

    #[test]
    fn friendly_asset_is_name_free() {
        assert_eq!(friendly_asset("BTC-USD"), "Bitcoin");
        assert_eq!(friendly_asset("GC=F"), "Gold");
        assert_eq!(friendly_asset("SI=F"), "Silver");
        assert_eq!(friendly_asset("AAPL"), "AAPL");
    }

    #[test]
    fn message_text_contains_friendly_name_and_read() {
        let parsed = parse_condition("cycle_bottom_monthly_2").unwrap();
        let sig = synthetic_signals(
            SignalTimeframe::Monthly,
            &["momentum_turning_up", "dss_bottoming"],
        );
        let eval = evaluate("BTC-USD", &parsed, Some(&sig));
        let msg = eval.trigger_data["message"].as_str().unwrap();
        assert!(msg.contains("Bitcoin"));
        assert!(msg.contains("2/7"));
        // No practitioner names leak into user-facing text.
        for name in ["Loukas", "Bressert", "Hurst", "Ehlers"] {
            assert!(!msg.contains(name), "name leak: {msg}");
        }
    }
}
