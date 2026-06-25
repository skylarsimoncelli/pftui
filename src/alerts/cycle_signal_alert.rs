//! Cycle-bottom signal alert evaluation.
//!
//! Wires the mechanical 7-composite cycle-bottom signal suite
//! (`crate::analytics::cycle_signals`) into the alert engine so that
//! confluence-threshold and single-criterion conditions are raised
//! automatically on every `data refresh`.
//!
//! Three condition shapes are supported, all carried on the existing
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
//! 3. **Single component** — `cycle_component_<timeframe>_<component_key>`
//!    (e.g. `cycle_component_monthly_erf_turned_up`). Fires when that one
//!    atomic subcondition is met on `<timeframe>`.
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

use crate::analytics::cycle_signals::{CycleBottomSignals, CycleTopSignals, SignalTimeframe};

/// Condition-string prefix for confluence-threshold alerts.
pub const CONFLUENCE_PREFIX: &str = "cycle_bottom_";
/// Condition-string prefix for single-criterion alerts.
pub const CRITERION_PREFIX: &str = "cycle_criterion_";
/// Condition-string prefix for atomic component alerts.
pub const COMPONENT_PREFIX: &str = "cycle_component_";

/// Condition-string prefix for cycle-TOP confluence-threshold alerts.
pub const TOP_CONFLUENCE_PREFIX: &str = "cycle_top_";
/// Condition-string prefix for cycle-TOP single-criterion alerts.
pub const TOP_CRITERION_PREFIX: &str = "cycle_top_criterion_";
/// Condition-string prefix for cycle-TOP atomic component alerts.
pub const TOP_COMPONENT_PREFIX: &str = "cycle_top_component_";

/// Which side of the cycle an alert condition watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Bottom,
    Top,
}

/// The 7 cycle-TOP composite criterion keys, as emitted by the top engine.
pub const TOP_CRITERION_KEYS: [&str; 7] = [
    "momentum_turning_down",
    "momentum_below_price",
    "dss_topping",
    "roofing_confirming_down",
    "volatility_bands_bearish",
    "reversal_dots_bearish",
    "trend_line_lost",
];

/// Alertable atomic component keys emitted inside the cycle-TOP criteria.
pub const TOP_COMPONENT_KEYS: [&str; 12] = [
    "rsi_ma_turned_down",
    "rsi_ma_cross_below_rsi",
    "dss_turned_down",
    "dss_cross_below_trigger",
    "dss_overbought",
    "erf_top_zone",
    "erf_turned_down",
    "cyberbands_bearish",
    "cyberdots_bearish",
    "cyberline_lost",
    "pi_cycle_top",
    "erf_negative",
];

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

/// Alertable atomic component keys emitted inside the cycle-bottom criteria.
pub const COMPONENT_KEYS: [&str; 12] = [
    "rsi_ma_turned_up",
    "rsi_ma_cross_above_rsi",
    "dss_turned_up",
    "dss_cross_above_trigger",
    "dss_oversold",
    "erf_bottom_zone",
    "erf_turned_up",
    "cyberbands_bullish",
    "cyberdots_bullish",
    "cyberline_reclaim",
    "pi_cycle_bottom",
    "erf_positive",
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
    /// Fire when the named atomic component is met on the given timeframe.
    Component {
        timeframe: SignalTimeframe,
        component_key: String,
    },
}

/// Returns true if a condition string is a cycle-signal alert condition (either
/// polarity — bottom or top).
pub fn is_cycle_signal_condition(condition: &str) -> bool {
    condition.starts_with(CONFLUENCE_PREFIX)
        || condition.starts_with(CRITERION_PREFIX)
        || condition.starts_with(COMPONENT_PREFIX)
        || condition.starts_with(TOP_CONFLUENCE_PREFIX)
}

/// Polarity (bottom vs top) of a cycle-signal condition. `cycle_top_*` (incl.
/// the criterion/component sub-prefixes, which start with `cycle_top_`) is Top;
/// everything else recognised by [`is_cycle_signal_condition`] is Bottom.
pub fn condition_polarity(condition: &str) -> anyhow::Result<Polarity> {
    if condition.starts_with(TOP_CONFLUENCE_PREFIX) {
        Ok(Polarity::Top)
    } else if is_cycle_signal_condition(condition) {
        Ok(Polarity::Bottom)
    } else {
        anyhow::bail!("not a cycle-signal condition: {condition}")
    }
}

/// Parse a cycle-signal condition string into its typed form.
///
/// Confluence: `cycle_bottom_<timeframe>_<N>`.
/// Criterion:  `cycle_criterion_<timeframe>_<criterion_key>`.
/// Component:  `cycle_component_<timeframe>_<component_key>`.
pub fn parse_condition(condition: &str) -> anyhow::Result<CycleSignalCondition> {
    // --- Cycle-TOP conditions first (their prefixes all start `cycle_top_`,
    //     so the criterion/component sub-prefixes must be tried before the
    //     bare `cycle_top_` confluence prefix). ---
    if let Some(rest) = condition.strip_prefix(TOP_COMPONENT_PREFIX) {
        let (tf_token, key) = rest.split_once('_').ok_or_else(|| {
            anyhow::anyhow!(
                "invalid cycle-top component condition '{condition}' — expected \
                 cycle_top_component_<timeframe>_<component_key>"
            )
        })?;
        let timeframe = SignalTimeframe::parse(tf_token)?;
        if !TOP_COMPONENT_KEYS.contains(&key) {
            anyhow::bail!(
                "unknown cycle-top component key '{key}' — expected one of: {}",
                TOP_COMPONENT_KEYS.join(", ")
            );
        }
        return Ok(CycleSignalCondition::Component {
            timeframe,
            component_key: key.to_string(),
        });
    }

    if let Some(rest) = condition.strip_prefix(TOP_CRITERION_PREFIX) {
        let (tf_token, key) = rest.split_once('_').ok_or_else(|| {
            anyhow::anyhow!(
                "invalid cycle-top criterion condition '{condition}' — expected \
                 cycle_top_criterion_<timeframe>_<criterion_key>"
            )
        })?;
        let timeframe = SignalTimeframe::parse(tf_token)?;
        if !TOP_CRITERION_KEYS.contains(&key) {
            anyhow::bail!(
                "unknown cycle-top criterion key '{key}' — expected one of: {}",
                TOP_CRITERION_KEYS.join(", ")
            );
        }
        return Ok(CycleSignalCondition::Criterion {
            timeframe,
            criterion_key: key.to_string(),
        });
    }

    if let Some(rest) = condition.strip_prefix(TOP_CONFLUENCE_PREFIX) {
        let (tf_token, n_token) = rest.rsplit_once('_').ok_or_else(|| {
            anyhow::anyhow!(
                "invalid cycle-top confluence condition '{condition}' — expected \
                 cycle_top_<timeframe>_<N>"
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

    if let Some(rest) = condition.strip_prefix(COMPONENT_PREFIX) {
        // rest = "<timeframe>_<component_key>"
        let (tf_token, key) = rest.split_once('_').ok_or_else(|| {
            anyhow::anyhow!(
                "invalid cycle component condition '{condition}' — expected \
                 cycle_component_<timeframe>_<component_key>"
            )
        })?;
        let timeframe = SignalTimeframe::parse(tf_token)?;
        if !COMPONENT_KEYS.contains(&key) {
            anyhow::bail!(
                "unknown cycle component key '{key}' — expected one of: {}",
                COMPONENT_KEYS.join(", ")
            );
        }
        return Ok(CycleSignalCondition::Component {
            timeframe,
            component_key: key.to_string(),
        });
    }

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
            anyhow::anyhow!(
                "invalid confluence target '{n_token}' in '{condition}' — expected 1..=7"
            )
        })?;
        if target == 0 || target > 7 {
            anyhow::bail!("confluence target must be 1..=7, got {target}");
        }
        return Ok(CycleSignalCondition::Confluence { timeframe, target });
    }

    anyhow::bail!("not a cycle-signal condition: {condition}")
}

/// Validate a cycle-signal condition string at arm-time.
///
/// Only applies to conditions carrying the `cycle_bottom_` / `cycle_criterion_`
/// prefixes — ALL other (non-cycle) `Technical` conditions are not this
/// function's concern and must be validated/passed elsewhere; callers gate on
/// [`is_cycle_signal_condition`] before invoking this.
///
/// Returns `Ok(())` for a structurally-valid cycle condition, or an error whose
/// message lists the full valid set (timeframes, N range, the 7 criterion keys)
/// so the operator can correct a typo without reading the docs.
///
/// Validation grammar:
/// - timeframe ∈ {daily, weekly, monthly}
/// - `cycle_bottom_<tf>_<N>`        → N ∈ 1..=7
/// - `cycle_criterion_<tf>_<key>`   → key ∈ the 7 [`CRITERION_KEYS`]
/// - `cycle_component_<tf>_<key>`   → key ∈ [`COMPONENT_KEYS`]
pub fn validate_condition(condition: &str) -> anyhow::Result<()> {
    parse_condition(condition).map(|_| ()).map_err(|e| {
        anyhow::anyhow!(
            "{e}\n\nValid cycle-bottom conditions:\n  \
             Confluence threshold — cycle_bottom_<timeframe>_<N>\n  \
             Single criterion    — cycle_criterion_<timeframe>_<key>\n  \
             Single component    — cycle_component_<timeframe>_<key>\n  \
             Criterion keys: {}\n  \
             Component keys: {}\n\n\
             Valid cycle-top conditions:\n  \
             Confluence threshold — cycle_top_<timeframe>_<N>\n  \
             Single criterion    — cycle_top_criterion_<timeframe>_<key>\n  \
             Single component    — cycle_top_component_<timeframe>_<key>\n  \
             Criterion keys: {}\n  \
             Component keys: {}\n\n\
             Timeframes: daily | weekly | monthly\n  \
             N (confluence target): 1..=7",
            CRITERION_KEYS.join(", "),
            COMPONENT_KEYS.join(", "),
            TOP_CRITERION_KEYS.join(", "),
            TOP_COMPONENT_KEYS.join(", ")
        )
    })
}

/// The timeframe a condition runs on (so the caller knows which signal read to
/// compute).
pub fn condition_timeframe(condition: &str) -> anyhow::Result<SignalTimeframe> {
    Ok(match parse_condition(condition)? {
        CycleSignalCondition::Confluence { timeframe, .. } => timeframe,
        CycleSignalCondition::Criterion { timeframe, .. } => timeframe,
        CycleSignalCondition::Component { timeframe, .. } => timeframe,
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

/// Human label for an atomic component key (no practitioner names).
pub fn component_label(key: &str) -> &'static str {
    match key {
        "rsi_ma_turned_up" => "RSI average ticked up",
        "rsi_ma_cross_above_rsi" => "RSI average reclaimed the RSI",
        "dss_turned_up" => "stochastic ticked up",
        "dss_cross_above_trigger" => "stochastic crossed above trigger",
        "dss_oversold" => "stochastic oversold",
        "erf_bottom_zone" => "roofing filter in bottom zone",
        "erf_turned_up" => "roofing filter ticked up",
        "erf_positive" => "roofing filter positive",
        "cyberbands_bullish" => "daily momentum bands bullish",
        "cyberdots_bullish" => "higher-timeframe strength dots bullish",
        "cyberline_reclaim" => "weekly trend line reclaimed",
        "pi_cycle_bottom" => "cycle-bottom bonus fired recently",
        _ => "cycle-bottom component",
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
        CycleSignalCondition::Component {
            timeframe,
            component_key,
        } => {
            let component = find_component(sig, component_key);
            let is_triggered = component
                .map(|component| component.0)
                .unwrap_or_else(|| component_fallback(sig, component_key).unwrap_or(false));
            let value = component
                .and_then(|component| component.1)
                .or_else(|| component_value_fallback(sig, component_key));
            CycleSignalEval {
                is_triggered,
                current_value: Some(Decimal::from(if is_triggered { 1u8 } else { 0u8 })),
                trigger_data: json!({
                    "kind": "cycle_bottom_component",
                    "symbol": symbol,
                    "asset": asset,
                    "timeframe": timeframe.label(),
                    "component_key": component_key,
                    "component_label": component_label(component_key),
                    "met": is_triggered,
                    "value": value,
                    "met_count": sig.met_count,
                    "total": sig.total,
                    "as_of": sig.as_of,
                    "message": format!(
                        "{asset} {} {} (cycle-bottom signals {}/{})",
                        timeframe.label(),
                        component_label(component_key),
                        sig.met_count,
                        sig.total
                    ),
                }),
            }
        }
    }
}

/// Human label for a cycle-TOP composite criterion key (no practitioner names).
pub fn top_criterion_label(key: &str) -> &'static str {
    match key {
        "momentum_turning_down" => "momentum line turning down",
        "momentum_below_price" => "momentum line below price momentum",
        "dss_topping" => "double-smoothed stochastic topping",
        "roofing_confirming_down" => "roofing filter confirming down",
        "volatility_bands_bearish" => "volatility bands bearish",
        "reversal_dots_bearish" => "significant reversal dots bearish",
        "trend_line_lost" => "trend line lost",
        _ => "cycle-top criterion",
    }
}

/// Human label for a cycle-TOP atomic component key (no practitioner names).
pub fn top_component_label(key: &str) -> &'static str {
    match key {
        "rsi_ma_turned_down" => "RSI average ticked down",
        "rsi_ma_cross_below_rsi" => "RSI average lost the RSI",
        "dss_turned_down" => "stochastic ticked down",
        "dss_cross_below_trigger" => "stochastic crossed below trigger",
        "dss_overbought" => "stochastic overbought",
        "erf_top_zone" => "roofing filter in top zone",
        "erf_turned_down" => "roofing filter ticked down",
        "erf_negative" => "roofing filter negative",
        "cyberbands_bearish" => "daily momentum bands bearish",
        "cyberdots_bearish" => "higher-timeframe strength dots bearish",
        "cyberline_lost" => "weekly trend line lost",
        "pi_cycle_top" => "cycle-top bonus fired recently",
        _ => "cycle-top component",
    }
}

/// Evaluate a parsed cycle-signal condition against an already-computed cycle-
/// TOP read — the symmetric mirror of [`evaluate`]. `signals` is `None` when
/// history was too shallow (no trigger).
pub fn evaluate_top(
    symbol: &str,
    parsed: &CycleSignalCondition,
    signals: Option<&CycleTopSignals>,
) -> CycleSignalEval {
    let asset = friendly_asset(symbol);
    let Some(sig) = signals else {
        return CycleSignalEval {
            is_triggered: false,
            current_value: None,
            trigger_data: json!({
                "kind": "cycle_top_signal",
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
                    "kind": "cycle_top_confluence",
                    "symbol": symbol,
                    "asset": asset,
                    "timeframe": timeframe.label(),
                    "met_count": met,
                    "total": sig.total,
                    "target": target,
                    "as_of": sig.as_of,
                    "message": format!(
                        "{asset} {} cycle-top signals {met}/{} (≥{target} target met)",
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
                    "kind": "cycle_top_criterion",
                    "symbol": symbol,
                    "asset": asset,
                    "timeframe": timeframe.label(),
                    "criterion_key": criterion_key,
                    "criterion_label": top_criterion_label(criterion_key),
                    "met": is_triggered,
                    "met_count": sig.met_count,
                    "total": sig.total,
                    "as_of": sig.as_of,
                    "message": format!(
                        "{asset} {} {} (cycle-top signals {}/{})",
                        timeframe.label(),
                        top_criterion_label(criterion_key),
                        sig.met_count,
                        sig.total
                    ),
                }),
            }
        }
        CycleSignalCondition::Component {
            timeframe,
            component_key,
        } => {
            let component = find_top_component(sig, component_key);
            let is_triggered = component
                .map(|component| component.0)
                .unwrap_or_else(|| top_component_fallback(sig, component_key).unwrap_or(false));
            let value = component
                .and_then(|component| component.1)
                .or_else(|| top_component_value_fallback(sig, component_key));
            CycleSignalEval {
                is_triggered,
                current_value: Some(Decimal::from(if is_triggered { 1u8 } else { 0u8 })),
                trigger_data: json!({
                    "kind": "cycle_top_component",
                    "symbol": symbol,
                    "asset": asset,
                    "timeframe": timeframe.label(),
                    "component_key": component_key,
                    "component_label": top_component_label(component_key),
                    "met": is_triggered,
                    "value": value,
                    "met_count": sig.met_count,
                    "total": sig.total,
                    "as_of": sig.as_of,
                    "message": format!(
                        "{asset} {} {} (cycle-top signals {}/{})",
                        timeframe.label(),
                        top_component_label(component_key),
                        sig.met_count,
                        sig.total
                    ),
                }),
            }
        }
    }
}

fn find_top_component(sig: &CycleTopSignals, component_key: &str) -> Option<(bool, Option<f64>)> {
    sig.criteria
        .iter()
        .flat_map(|criterion| criterion.components.iter())
        .find(|component| component.key == component_key)
        .map(|component| (component.met, component.value))
}

fn top_component_fallback(sig: &CycleTopSignals, component_key: &str) -> Option<bool> {
    match component_key {
        "erf_negative" => Some(sig.erf_negative),
        "pi_cycle_top" => Some(sig.pi_cycle_top),
        _ => None,
    }
}

fn top_component_value_fallback(sig: &CycleTopSignals, component_key: &str) -> Option<f64> {
    match component_key {
        "erf_negative" => sig.erf,
        _ => None,
    }
}

fn find_component(sig: &CycleBottomSignals, component_key: &str) -> Option<(bool, Option<f64>)> {
    sig.criteria
        .iter()
        .flat_map(|criterion| criterion.components.iter())
        .find(|component| component.key == component_key)
        .map(|component| (component.met, component.value))
}

fn component_fallback(sig: &CycleBottomSignals, component_key: &str) -> Option<bool> {
    match component_key {
        "erf_positive" => Some(sig.erf_positive),
        "pi_cycle_bottom" => Some(sig.pi_cycle_bottom),
        _ => None,
    }
}

fn component_value_fallback(sig: &CycleBottomSignals, component_key: &str) -> Option<f64> {
    match component_key {
        "erf_positive" => sig.erf,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::cycle_signals::{
        Component, Criterion, CycleBottomSignals, CycleTopSignals, SignalTimeframe, WatchItem,
    };

    fn synthetic_signals(timeframe: SignalTimeframe, met_keys: &[&str]) -> CycleBottomSignals {
        let criteria: Vec<Criterion> = CRITERION_KEYS
            .iter()
            .map(|k| Criterion {
                key: k.to_string(),
                label: k.to_string(),
                met: met_keys.contains(k),
                detail: String::new(),
                components: synthetic_components(k, met_keys),
            })
            .collect();
        let met_count = criteria.iter().filter(|c| c.met).count();
        let core_watch: Vec<WatchItem> = criteria
            .iter()
            .take(4)
            .map(|c| WatchItem {
                key: c.key.clone(),
                label: c.label.clone(),
                met: c.met,
                met_components: c
                    .components
                    .iter()
                    .filter(|component| component.met)
                    .count(),
                total_components: c.components.len(),
                detail: c.detail.clone(),
                components: c.components.clone(),
            })
            .collect();
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
            erf_positive: false,
            erf_green: false,
            erf_bottom_zone: false,
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
            core_watch,
            met_count,
            total: 7,
            bonus: None,
            verdict: String::new(),
        }
    }

    fn synthetic_components(criterion_key: &str, met_keys: &[&str]) -> Vec<Component> {
        let keys: &[&str] = match criterion_key {
            "momentum_turning_up" => &["rsi_ma_turned_up"],
            "momentum_above_price" => &["rsi_ma_cross_above_rsi"],
            "dss_bottoming" => &["dss_turned_up", "dss_cross_above_trigger"],
            "roofing_confirming_up" => &["erf_bottom_zone", "erf_turned_up"],
            "volatility_bands_bullish" => &["cyberbands_bullish"],
            "reversal_dots" => &["cyberdots_bullish"],
            "trend_line_reclaimed" => &["cyberline_reclaim"],
            _ => &[],
        };
        keys.iter()
            .map(|key| Component {
                key: (*key).to_string(),
                label: (*key).to_string(),
                met: met_keys.contains(key),
                value: None,
                previous_value: None,
                comparison_value: None,
                previous_comparison_value: None,
                distance_to_trigger: None,
            })
            .collect()
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
    fn parse_component_condition() {
        let parsed = parse_condition("cycle_component_monthly_erf_bottom_zone").unwrap();
        assert_eq!(
            parsed,
            CycleSignalCondition::Component {
                timeframe: SignalTimeframe::Monthly,
                component_key: "erf_bottom_zone".to_string(),
            }
        );
    }

    #[test]
    fn parse_rejects_unknown_criterion() {
        assert!(parse_condition("cycle_criterion_weekly_not_a_real_key").is_err());
    }

    #[test]
    fn validate_rejects_all_invalid_shapes() {
        // Bad timeframe (yearly is not a SignalTimeframe).
        assert!(validate_condition("cycle_bottom_yearly_4").is_err());
        // N = 0 and N > 7 can never fire.
        assert!(validate_condition("cycle_bottom_monthly_0").is_err());
        assert!(validate_condition("cycle_bottom_monthly_8").is_err());
        // Unknown criterion key.
        assert!(validate_condition("cycle_criterion_weekly_bogus_key").is_err());
        // Unknown component key.
        assert!(validate_condition("cycle_component_weekly_bogus_key").is_err());
        // Non-numeric target.
        assert!(validate_condition("cycle_bottom_monthly_x").is_err());
    }

    #[test]
    fn validate_error_lists_the_valid_set() {
        let err = validate_condition("cycle_bottom_monthly_8")
            .unwrap_err()
            .to_string();
        // The message must guide the operator: timeframes, N range, every key.
        assert!(err.contains("daily"));
        assert!(err.contains("weekly"));
        assert!(err.contains("monthly"));
        assert!(err.contains("1..=7"));
        for key in CRITERION_KEYS {
            assert!(
                err.contains(key),
                "valid-set message omitted key {key}: {err}"
            );
        }
        for key in COMPONENT_KEYS {
            assert!(
                err.contains(key),
                "valid-set message omitted key {key}: {err}"
            );
        }
    }

    #[test]
    fn validate_accepts_every_valid_confluence_target() {
        for tf in ["daily", "weekly", "monthly"] {
            for n in 1..=7 {
                let cond = format!("cycle_bottom_{tf}_{n}");
                assert!(
                    validate_condition(&cond).is_ok(),
                    "valid confluence rejected: {cond}"
                );
            }
        }
    }

    #[test]
    fn non_cycle_conditions_are_not_recognized_as_cycle() {
        // These are real Technical conditions handled elsewhere; the validator
        // gate (is_cycle_signal_condition) must NOT claim them, so they pass
        // through arm-time unchanged.
        for cond in [
            "price_below_sma200",
            "price_above_sma50",
            "rsi_above_70",
            "rsi_below_30",
            "price_change_pct_above_5",
        ] {
            assert!(
                !is_cycle_signal_condition(cond),
                "non-cycle condition wrongly claimed by cycle validator: {cond}"
            );
        }
    }

    #[test]
    fn validate_accepts_every_known_criterion_key() {
        for tf in ["daily", "weekly", "monthly"] {
            for key in CRITERION_KEYS {
                let cond = format!("cycle_criterion_{tf}_{key}");
                assert!(
                    validate_condition(&cond).is_ok(),
                    "valid criterion rejected: {cond}"
                );
            }
        }
    }

    #[test]
    fn validate_accepts_every_known_component_key() {
        for tf in ["daily", "weekly", "monthly"] {
            for key in COMPONENT_KEYS {
                let cond = format!("cycle_component_{tf}_{key}");
                assert!(
                    validate_condition(&cond).is_ok(),
                    "valid component rejected: {cond}"
                );
            }
        }
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
    fn single_component_triggers_only_when_met() {
        let parsed = parse_condition("cycle_component_monthly_erf_bottom_zone").unwrap();
        let met = synthetic_signals(SignalTimeframe::Monthly, &["erf_bottom_zone"]);
        assert!(evaluate("BTC-USD", &parsed, Some(&met)).is_triggered);

        let not_met = synthetic_signals(SignalTimeframe::Monthly, &["erf_turned_up"]);
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

    // ---- Cycle-TOP alert conditions (symmetric mirror) -------------------

    fn synthetic_top_signals(timeframe: SignalTimeframe, met_keys: &[&str]) -> CycleTopSignals {
        let criteria: Vec<Criterion> = TOP_CRITERION_KEYS
            .iter()
            .map(|k| Criterion {
                key: k.to_string(),
                label: k.to_string(),
                met: met_keys.contains(k),
                detail: String::new(),
                components: synthetic_top_components(k, met_keys),
            })
            .collect();
        let met_count = criteria.iter().filter(|c| c.met).count();
        let core_watch: Vec<WatchItem> = criteria
            .iter()
            .take(4)
            .map(|c| WatchItem {
                key: c.key.clone(),
                label: c.label.clone(),
                met: c.met,
                met_components: c.components.iter().filter(|x| x.met).count(),
                total_components: c.components.len(),
                detail: c.detail.clone(),
                components: c.components.clone(),
            })
            .collect();
        CycleTopSignals {
            symbol: "BTC-USD".to_string(),
            timeframe,
            as_of: "2026-06-01".to_string(),
            rsi: None,
            rsi_ma: None,
            rsi_ma_turned_down: false,
            rsi_ma_cross_below_rsi: false,
            dss: None,
            dss_trigger: None,
            dss_turned_down: false,
            dss_cross_below_trigger: false,
            dss_overbought: false,
            erf: None,
            erf_negative: false,
            erf_top_zone: false,
            erf_turned_down: false,
            cyberbands_state: None,
            cyberbands_bearish: false,
            cyberdots_weekly_down_strength: None,
            cyberdots_monthly_down_strength: None,
            cyberdots_bearish: false,
            cyberline_value: None,
            cyberline_price_above: None,
            cyberline_lost: false,
            pi_cycle_top: false,
            pi_cycle_last_top: None,
            criteria,
            core_watch,
            met_count,
            total: 7,
            bonus: None,
            verdict: String::new(),
        }
    }

    fn synthetic_top_components(criterion_key: &str, met_keys: &[&str]) -> Vec<Component> {
        let keys: &[&str] = match criterion_key {
            "momentum_turning_down" => &["rsi_ma_turned_down"],
            "momentum_below_price" => &["rsi_ma_cross_below_rsi"],
            "dss_topping" => &["dss_turned_down", "dss_cross_below_trigger"],
            "roofing_confirming_down" => &["erf_top_zone", "erf_turned_down"],
            "volatility_bands_bearish" => &["cyberbands_bearish"],
            "reversal_dots_bearish" => &["cyberdots_bearish"],
            "trend_line_lost" => &["cyberline_lost"],
            _ => &[],
        };
        keys.iter()
            .map(|key| Component {
                key: (*key).to_string(),
                label: (*key).to_string(),
                met: met_keys.contains(key),
                value: None,
                previous_value: None,
                comparison_value: None,
                previous_comparison_value: None,
                distance_to_trigger: None,
            })
            .collect()
    }

    #[test]
    fn parse_top_confluence_condition() {
        let parsed = parse_condition("cycle_top_monthly_4").unwrap();
        assert_eq!(
            parsed,
            CycleSignalCondition::Confluence {
                timeframe: SignalTimeframe::Monthly,
                target: 4,
            }
        );
        assert_eq!(
            condition_polarity("cycle_top_monthly_4").unwrap(),
            Polarity::Top
        );
        assert_eq!(
            condition_polarity("cycle_bottom_monthly_4").unwrap(),
            Polarity::Bottom
        );
    }

    #[test]
    fn parse_top_criterion_and_component() {
        let crit = parse_condition("cycle_top_criterion_weekly_trend_line_lost").unwrap();
        assert_eq!(
            crit,
            CycleSignalCondition::Criterion {
                timeframe: SignalTimeframe::Weekly,
                criterion_key: "trend_line_lost".to_string(),
            }
        );
        assert_eq!(
            condition_polarity("cycle_top_criterion_weekly_trend_line_lost").unwrap(),
            Polarity::Top
        );
        let comp = parse_condition("cycle_top_component_monthly_erf_turned_down").unwrap();
        assert_eq!(
            comp,
            CycleSignalCondition::Component {
                timeframe: SignalTimeframe::Monthly,
                component_key: "erf_turned_down".to_string(),
            }
        );
    }

    #[test]
    fn parse_rejects_unknown_top_keys() {
        assert!(parse_condition("cycle_top_criterion_weekly_bogus").is_err());
        assert!(parse_condition("cycle_top_component_weekly_bogus").is_err());
        assert!(parse_condition("cycle_top_yearly_4").is_err());
        assert!(parse_condition("cycle_top_monthly_8").is_err());
        assert!(parse_condition("cycle_top_monthly_0").is_err());
    }

    #[test]
    fn validate_accepts_every_top_condition() {
        for tf in ["daily", "weekly", "monthly"] {
            for n in 1..=7 {
                assert!(validate_condition(&format!("cycle_top_{tf}_{n}")).is_ok());
            }
            for key in TOP_CRITERION_KEYS {
                assert!(validate_condition(&format!("cycle_top_criterion_{tf}_{key}")).is_ok());
            }
            for key in TOP_COMPONENT_KEYS {
                assert!(validate_condition(&format!("cycle_top_component_{tf}_{key}")).is_ok());
            }
        }
    }

    #[test]
    fn validate_error_lists_top_keys() {
        let err = validate_condition("cycle_top_monthly_8").unwrap_err().to_string();
        for key in TOP_CRITERION_KEYS {
            assert!(err.contains(key), "top valid-set omitted {key}: {err}");
        }
        for key in TOP_COMPONENT_KEYS {
            assert!(err.contains(key), "top valid-set omitted {key}: {err}");
        }
    }

    #[test]
    fn top_confluence_triggers_at_or_above_target() {
        let parsed = parse_condition("cycle_top_monthly_3").unwrap();
        let sig = synthetic_top_signals(
            SignalTimeframe::Monthly,
            &["momentum_turning_down", "dss_topping", "reversal_dots_bearish"],
        );
        let eval = evaluate_top("BTC-USD", &parsed, Some(&sig));
        assert!(eval.is_triggered);
        assert_eq!(eval.current_value, Some(Decimal::from(3)));
        assert_eq!(eval.trigger_data["kind"], "cycle_top_confluence");
        let msg = eval.trigger_data["message"].as_str().unwrap();
        assert!(msg.contains("Bitcoin") && msg.contains("cycle-top"));
    }

    #[test]
    fn top_single_criterion_and_component_trigger_only_when_met() {
        let parsed = parse_condition("cycle_top_criterion_weekly_trend_line_lost").unwrap();
        let met = synthetic_top_signals(SignalTimeframe::Weekly, &["trend_line_lost"]);
        assert!(evaluate_top("BTC-USD", &parsed, Some(&met)).is_triggered);
        let not_met = synthetic_top_signals(SignalTimeframe::Weekly, &["dss_topping"]);
        assert!(!evaluate_top("BTC-USD", &parsed, Some(&not_met)).is_triggered);

        let cparsed = parse_condition("cycle_top_component_monthly_erf_top_zone").unwrap();
        let cmet = synthetic_top_signals(SignalTimeframe::Monthly, &["erf_top_zone"]);
        assert!(evaluate_top("BTC-USD", &cparsed, Some(&cmet)).is_triggered);
        let cnot = synthetic_top_signals(SignalTimeframe::Monthly, &["erf_turned_down"]);
        assert!(!evaluate_top("BTC-USD", &cparsed, Some(&cnot)).is_triggered);
    }

    #[test]
    fn top_insufficient_history_never_triggers() {
        let parsed = parse_condition("cycle_top_monthly_1").unwrap();
        let eval = evaluate_top("BTC-USD", &parsed, None);
        assert!(!eval.is_triggered);
        assert_eq!(eval.current_value, None);
        assert_eq!(eval.trigger_data["kind"], "cycle_top_signal");
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
