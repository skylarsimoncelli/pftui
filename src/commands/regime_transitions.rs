//! Regime Transition Probability Scoring
//!
//! Analyzes the current regime state, signal momentum, and historical transition
//! patterns to score the probability of transitioning to each possible regime.
//! Surfaces key drivers, confirmation triggers, and invalidation conditions.
//!
//! `pftui analytics regime-transitions [--json]`

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_cached_price_backend;
use crate::db::price_history::get_history_backend;
use crate::db::regime_snapshots;

/// All possible regime states.
const REGIMES: &[&str] = &[
    "risk-on",
    "lean risk-on",
    "neutral",
    "lean risk-off",
    "risk-off",
    "crisis",
    "stagflation",
    "transition",
];

/// A scored potential regime transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionCandidate {
    /// Target regime state.
    pub target_regime: String,
    /// Probability score 0.0–1.0.
    pub probability: f64,
    /// Probability label (high/medium/low/minimal).
    pub probability_label: String,
    /// Key signals driving this transition.
    pub drivers: Vec<String>,
    /// What would confirm the transition.
    pub confirmations: Vec<String>,
    /// What would invalidate the transition.
    pub invalidations: Vec<String>,
}

/// Full regime transition probability report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionReport {
    /// Current regime state.
    pub current_regime: String,
    /// Current regime confidence.
    pub current_confidence: f64,
    /// Days in current regime.
    pub days_in_regime: i64,
    /// Regime stability score 0.0–1.0 (higher = more stable, less likely to transition).
    pub stability: f64,
    /// Signal momentum summary.
    pub signal_momentum: SignalMomentum,
    /// Transition candidates sorted by probability (descending).
    pub transitions: Vec<TransitionCandidate>,
    /// Historical transition frequency.
    pub historical_context: HistoricalContext,
}

/// Momentum of key regime signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalMomentum {
    pub vix_direction: Option<String>,
    pub dxy_direction: Option<String>,
    pub yield_direction: Option<String>,
    pub equity_direction: Option<String>,
    pub gold_direction: Option<String>,
    pub oil_direction: Option<String>,
    /// Count of signals shifting toward risk-on.
    pub risk_on_momentum: u8,
    /// Count of signals shifting toward risk-off.
    pub risk_off_momentum: u8,
}

/// Historical regime transition patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalContext {
    /// Total snapshots in history.
    pub total_snapshots: usize,
    /// Total transitions detected.
    pub total_transitions: usize,
    /// Average days between transitions (None if < 2 transitions).
    pub avg_days_between_transitions: Option<f64>,
    /// Most common transition (e.g. "risk-off → transition").
    pub most_common_transition: Option<String>,
}

/// Fetch a price as f64 from cache.
fn price_f64(backend: &BackendConnection, symbol: &str) -> Option<f64> {
    get_cached_price_backend(backend, symbol, "USD")
        .ok()
        .flatten()
        .and_then(|q| q.price.to_string().parse::<f64>().ok())
}

/// Compute 5-day directional trend: "rising", "falling", or None.
fn trend_5d(backend: &BackendConnection, symbol: &str) -> Option<String> {
    let rows = get_history_backend(backend, symbol, 10).ok()?;
    if rows.len() < 6 {
        return None;
    }
    let latest = rows.last()?.close.to_string().parse::<f64>().ok()?;
    let past = rows[rows.len() - 6].close.to_string().parse::<f64>().ok()?;
    if past.abs() < f64::EPSILON {
        return None;
    }
    let pct = (latest - past) / past * 100.0;
    if pct > 0.5 {
        Some("rising".to_string())
    } else if pct < -0.5 {
        Some("falling".to_string())
    } else {
        Some("flat".to_string())
    }
}

/// Compute signal momentum from current market data.
fn compute_momentum(backend: &BackendConnection) -> SignalMomentum {
    let vix_dir = trend_5d(backend, "^VIX");
    let dxy_dir = trend_5d(backend, "DX-Y.NYB");
    let yield_dir = trend_5d(backend, "^TNX");
    let eq_dir = trend_5d(backend, "^GSPC").or_else(|| trend_5d(backend, "SPY"));
    let gold_dir = trend_5d(backend, "GC=F");
    let oil_dir = trend_5d(backend, "CL=F");

    let mut risk_on: u8 = 0;
    let mut risk_off: u8 = 0;

    // VIX falling = risk-on momentum
    match vix_dir.as_deref() {
        Some("falling") => risk_on += 1,
        Some("rising") => risk_off += 1,
        _ => {}
    }
    // DXY falling = risk-on
    match dxy_dir.as_deref() {
        Some("falling") => risk_on += 1,
        Some("rising") => risk_off += 1,
        _ => {}
    }
    // Yields rising = risk-on (growth expectations)
    match yield_dir.as_deref() {
        Some("rising") => risk_on += 1,
        Some("falling") => risk_off += 1,
        _ => {}
    }
    // Equities rising = risk-on
    match eq_dir.as_deref() {
        Some("rising") => risk_on += 1,
        Some("falling") => risk_off += 1,
        _ => {}
    }
    // Gold rising = risk-off
    match gold_dir.as_deref() {
        Some("rising") => risk_off += 1,
        Some("falling") => risk_on += 1,
        _ => {}
    }
    // Oil rising = inflationary pressure (risk-off)
    match oil_dir.as_deref() {
        Some("rising") => risk_off += 1,
        Some("falling") => risk_on += 1,
        _ => {}
    }

    SignalMomentum {
        vix_direction: vix_dir,
        dxy_direction: dxy_dir,
        yield_direction: yield_dir,
        equity_direction: eq_dir,
        gold_direction: gold_dir,
        oil_direction: oil_dir,
        risk_on_momentum: risk_on,
        risk_off_momentum: risk_off,
    }
}

/// Map regime label from RegimeScore (computed) to a canonical string.
fn normalize_regime(r: &str) -> &str {
    match r.to_lowercase().as_str() {
        "risk-on" | "risk_on" => "risk-on",
        "lean risk-on" | "lean_risk_on" | "lean-risk-on" => "lean risk-on",
        "neutral" => "neutral",
        "lean risk-off" | "lean_risk_off" | "lean-risk-off" => "lean risk-off",
        "risk-off" | "risk_off" => "risk-off",
        "crisis" => "crisis",
        "stagflation" => "stagflation",
        _ => "transition",
    }
}

/// Compute regime ordering on a risk-on to risk-off scale.
/// Higher = more risk-on.
fn regime_order(r: &str) -> i8 {
    match normalize_regime(r) {
        "risk-on" => 4,
        "lean risk-on" => 3,
        "neutral" => 2,
        "lean risk-off" => 1,
        "risk-off" => 0,
        "crisis" => -1,
        "stagflation" => -1,
        _ => 2, // transition ~ neutral
    }
}

/// Distance between two regimes on the risk scale.
fn regime_distance(from: &str, to: &str) -> u8 {
    let a = regime_order(from);
    let b = regime_order(to);
    (a - b).unsigned_abs()
}

/// Label a probability.
fn probability_label(p: f64) -> String {
    if p >= 0.6 {
        "high".to_string()
    } else if p >= 0.3 {
        "medium".to_string()
    } else if p >= 0.1 {
        "low".to_string()
    } else {
        "minimal".to_string()
    }
}

/// Build transition candidates based on current state and momentum.
fn build_candidates(
    current: &str,
    confidence: f64,
    momentum: &SignalMomentum,
    vix: Option<f64>,
    oil: Option<f64>,
    gold_dir: &Option<String>,
    eq_dir: &Option<String>,
) -> Vec<TransitionCandidate> {
    let mut candidates = Vec::new();

    for &target in REGIMES {
        if normalize_regime(target) == normalize_regime(current) {
            continue;
        }

        let distance = regime_distance(current, target);
        let mut prob: f64 = 0.0;
        let mut drivers = Vec::new();
        let mut confirmations = Vec::new();
        let mut invalidations = Vec::new();

        // Base probability: closer regimes are more likely transitions
        let base = match distance {
            0 => continue, // same regime
            1 => 0.20,
            2 => 0.10,
            3 => 0.05,
            _ => 0.02,
        };
        prob += base;

        // Momentum alignment: does momentum point toward this target?
        let target_order = regime_order(target);
        let current_order = regime_order(current);
        let momentum_toward_risk_on = momentum.risk_on_momentum > momentum.risk_off_momentum;
        let momentum_toward_risk_off = momentum.risk_off_momentum > momentum.risk_on_momentum;

        if target_order > current_order && momentum_toward_risk_on {
            let boost = (momentum.risk_on_momentum as f64 - momentum.risk_off_momentum as f64)
                * 0.06;
            prob += boost;
            drivers.push(format!(
                "{} of 6 signals shifting risk-on",
                momentum.risk_on_momentum
            ));
        } else if target_order < current_order && momentum_toward_risk_off {
            let boost = (momentum.risk_off_momentum as f64 - momentum.risk_on_momentum as f64)
                * 0.06;
            prob += boost;
            drivers.push(format!(
                "{} of 6 signals shifting risk-off",
                momentum.risk_off_momentum
            ));
        } else if target_order != current_order {
            // Momentum opposes this transition — dampen
            prob *= 0.5;
        }

        // Current confidence: low confidence = higher transition probability
        if confidence < 0.5 {
            prob += 0.10;
            drivers.push(format!(
                "Current regime confidence low ({:.0}%)",
                confidence * 100.0
            ));
        }

        // Special regime checks (crisis, stagflation)
        match normalize_regime(target) {
            "crisis" => {
                if let Some(v) = vix {
                    if v > 25.0 {
                        prob += 0.15;
                        drivers.push(format!("VIX elevated at {:.1}", v));
                    }
                    if v > 30.0 {
                        prob += 0.10;
                    }
                }
                if let Some(o) = oil {
                    if o > 85.0 {
                        prob += 0.10;
                        drivers.push(format!("Oil elevated at ${:.0}", o));
                    }
                }
                confirmations.push("VIX > 30 + oil > $90 + equities falling".to_string());
                invalidations.push("VIX drops below 20, equities stabilize".to_string());
            }
            "stagflation" => {
                let gold_up = gold_dir.as_deref() == Some("rising");
                let eq_down = eq_dir.as_deref() == Some("falling");
                if let Some(v) = vix {
                    if v > 25.0 {
                        prob += 0.08;
                    }
                }
                if let Some(o) = oil {
                    if o > 80.0 {
                        prob += 0.08;
                        drivers.push(format!("Oil at ${:.0} — inflationary pressure", o));
                    }
                }
                if gold_up && eq_down {
                    prob += 0.12;
                    drivers
                        .push("Gold rising + equities falling — stagflation signal".to_string());
                }
                confirmations
                    .push("VIX > 25, oil > $80, gold up, equities down together".to_string());
                invalidations.push("Oil drops, equities recover, CPI falls".to_string());
            }
            "risk-on" => {
                if let Some(v) = vix {
                    if v < 18.0 {
                        prob += 0.08;
                        drivers.push(format!("VIX low at {:.1}", v));
                    }
                }
                if eq_dir.as_deref() == Some("rising") {
                    prob += 0.06;
                    drivers.push("Equities rising".to_string());
                }
                confirmations.push("VIX < 18, equities up, DXY stable/falling".to_string());
                invalidations
                    .push("VIX spikes above 25, major geopolitical shock".to_string());
            }
            "risk-off" => {
                if let Some(v) = vix {
                    if v > 22.0 {
                        prob += 0.06;
                        drivers.push(format!("VIX at {:.1} — stress building", v));
                    }
                }
                if gold_dir.as_deref() == Some("rising") {
                    prob += 0.05;
                    drivers.push("Gold rising — safe-haven demand".to_string());
                }
                confirmations
                    .push("VIX > 25, gold up, equities down, DXY strengthening".to_string());
                invalidations.push("VIX drops, equities rally, risk appetite returns".to_string());
            }
            "neutral" | "transition" => {
                // Neutral/transition is more likely when signals are mixed
                let balance = (momentum.risk_on_momentum as i8 - momentum.risk_off_momentum as i8)
                    .unsigned_abs();
                if balance <= 1 {
                    prob += 0.10;
                    drivers.push("Signals evenly split — indecisive market".to_string());
                }
                confirmations.push("Conflicting signals persist, no clear direction".to_string());
                invalidations.push("Clear momentum emerges in either direction".to_string());
            }
            "lean risk-on" => {
                if momentum_toward_risk_on && momentum.risk_on_momentum >= 3 {
                    prob += 0.06;
                }
                if eq_dir.as_deref() == Some("rising") {
                    drivers.push("Equities trending up".to_string());
                }
                confirmations
                    .push("3+ signals risk-on, VIX below 20, equities up".to_string());
                invalidations.push("Momentum reverses, VIX rises above 22".to_string());
            }
            "lean risk-off" => {
                if momentum_toward_risk_off && momentum.risk_off_momentum >= 3 {
                    prob += 0.06;
                }
                if gold_dir.as_deref() == Some("rising") {
                    drivers.push("Gold bid — defensive positioning".to_string());
                }
                confirmations
                    .push("3+ signals risk-off, gold up, equities soft".to_string());
                invalidations.push("Risk appetite returns, VIX drops below 18".to_string());
            }
            _ => {}
        }

        // Add generic confirmations/invalidations if empty
        if confirmations.is_empty() {
            confirmations.push(format!(
                "Sustained momentum toward {}",
                target
            ));
        }
        if invalidations.is_empty() {
            invalidations.push(format!(
                "Momentum reverses away from {}",
                target
            ));
        }

        // Clamp probability
        prob = prob.clamp(0.0, 0.95);

        // Only include if non-trivial
        if prob >= 0.02 {
            candidates.push(TransitionCandidate {
                target_regime: target.to_string(),
                probability: (prob * 100.0).round() / 100.0,
                probability_label: probability_label(prob),
                drivers,
                confirmations,
                invalidations,
            });
        }
    }

    // Sort by probability descending
    candidates.sort_by(|a, b| {
        b.probability
            .partial_cmp(&a.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    candidates
}

/// Compute historical transition context from regime snapshots.
fn compute_historical(
    snapshots: &[regime_snapshots::RegimeSnapshot],
) -> HistoricalContext {
    if snapshots.is_empty() {
        return HistoricalContext {
            total_snapshots: 0,
            total_transitions: 0,
            avg_days_between_transitions: None,
            most_common_transition: None,
        };
    }

    // Count transitions
    let mut transitions: Vec<(String, String)> = Vec::new();
    let mut transition_dates: Vec<&str> = Vec::new();

    // snapshots are ordered desc by recorded_at, so reverse for chronological
    let chronological: Vec<_> = snapshots.iter().rev().collect();

    for i in 1..chronological.len() {
        if chronological[i].regime != chronological[i - 1].regime {
            transitions.push((
                chronological[i - 1].regime.clone(),
                chronological[i].regime.clone(),
            ));
            transition_dates.push(&chronological[i].recorded_at);
        }
    }

    // Average days between transitions
    let avg_days = if transition_dates.len() >= 2 {
        let mut total_days: f64 = 0.0;
        let mut count = 0;
        for window in transition_dates.windows(2) {
            if let (Some(d1), Some(d2)) = (
                parse_date_str(window[0]),
                parse_date_str(window[1]),
            ) {
                let diff = (d2 - d1).num_days().unsigned_abs() as f64;
                total_days += diff;
                count += 1;
            }
        }
        if count > 0 {
            Some(total_days / count as f64)
        } else {
            None
        }
    } else {
        None
    };

    // Most common transition
    let most_common = if !transitions.is_empty() {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (from, to) in &transitions {
            let key = format!("{} → {}", from, to);
            *counts.entry(key).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(k, _)| k)
    } else {
        None
    };

    HistoricalContext {
        total_snapshots: snapshots.len(),
        total_transitions: transitions.len(),
        avg_days_between_transitions: avg_days.map(|d| (d * 10.0).round() / 10.0),
        most_common_transition: most_common,
    }
}

/// Parse a date string (YYYY-MM-DD...) into chrono NaiveDate.
fn parse_date_str(s: &str) -> Option<chrono::NaiveDate> {
    let date_part = if s.len() >= 10 { &s[..10] } else { s };
    chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

/// Compute days since a date string.
fn days_since(s: &str) -> i64 {
    let today = chrono::Utc::now().date_naive();
    match parse_date_str(s) {
        Some(d) => (today - d).num_days(),
        None => 0,
    }
}

/// Compute regime stability: higher = more stable (less likely to transition).
fn compute_stability(days_in_regime: i64, confidence: f64, momentum: &SignalMomentum) -> f64 {
    let mut stability: f64 = 0.0;

    // Duration factor: longer in regime = more stable (up to a point)
    stability += match days_in_regime {
        0..=2 => 0.10,
        3..=7 => 0.20,
        8..=14 => 0.30,
        15..=30 => 0.40,
        _ => 0.45,
    };

    // Confidence factor
    stability += confidence * 0.30;

    // Momentum balance: balanced signals = less stable
    let balance =
        (momentum.risk_on_momentum as i8 - momentum.risk_off_momentum as i8).unsigned_abs();
    stability += match balance {
        0 => 0.0,  // perfectly balanced = unstable
        1 => 0.05,
        2 => 0.10,
        3 => 0.15,
        _ => 0.20,
    };

    stability.clamp(0.0, 1.0)
}

/// Build the full transition probability report.
pub fn build_report(backend: &BackendConnection) -> Result<TransitionReport> {
    // Get current regime
    let current = regime_snapshots::get_current_backend(backend)?;

    let (current_regime, current_confidence, recorded_at) = match &current {
        Some(c) => (
            c.regime.clone(),
            c.confidence.unwrap_or(0.0),
            c.recorded_at.as_str(),
        ),
        None => ("transition".to_string(), 0.0, ""),
    };

    let days_in = if recorded_at.is_empty() {
        0
    } else {
        days_since(recorded_at)
    };

    // Signal momentum
    let momentum = compute_momentum(backend);

    // Stability
    let stability = compute_stability(days_in, current_confidence, &momentum);

    // Market data for special regime checks
    let vix = price_f64(backend, "^VIX");
    let oil = price_f64(backend, "CL=F");

    // Build candidates
    let transitions = build_candidates(
        &current_regime,
        current_confidence,
        &momentum,
        vix,
        oil,
        &momentum.gold_direction.clone(),
        &momentum.equity_direction.clone(),
    );

    // Historical context
    let history = regime_snapshots::get_history_backend(backend, None)?;
    let historical_context = compute_historical(&history);

    Ok(TransitionReport {
        current_regime: current_regime.to_string(),
        current_confidence,
        days_in_regime: days_in,
        stability,
        signal_momentum: momentum,
        transitions,
        historical_context,
    })
}

/// Run the regime transitions CLI command.
pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let report = build_report(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // Terminal output
    println!("═══ Regime Transition Probability ═══");
    println!();
    println!(
        "Current: {} (confidence: {:.0}%, stability: {:.0}%)",
        report.current_regime.to_uppercase(),
        report.current_confidence * 100.0,
        report.stability * 100.0,
    );
    println!("Days in regime: {}", report.days_in_regime);
    println!();

    // Signal momentum
    println!("── Signal Momentum ──");
    let m = &report.signal_momentum;
    let signals: Vec<(&str, &Option<String>)> = vec![
        ("VIX", &m.vix_direction),
        ("DXY", &m.dxy_direction),
        ("10Y Yield", &m.yield_direction),
        ("Equities", &m.equity_direction),
        ("Gold", &m.gold_direction),
        ("Oil", &m.oil_direction),
    ];
    for (name, dir) in &signals {
        let arrow = match dir.as_deref() {
            Some("rising") => "↑",
            Some("falling") => "↓",
            Some("flat") => "→",
            _ => "—",
        };
        println!("  {} {}", arrow, name);
    }
    println!(
        "  Risk-on momentum: {} | Risk-off momentum: {}",
        m.risk_on_momentum, m.risk_off_momentum
    );
    println!();

    // Transitions
    if report.transitions.is_empty() {
        println!("No significant transition probabilities detected.");
    } else {
        println!("── Transition Candidates ──");
        for t in &report.transitions {
            let icon = match t.probability_label.as_str() {
                "high" => "🔴",
                "medium" => "🟡",
                "low" => "🟢",
                _ => "⚪",
            };
            println!(
                "{} {} → {:.0}% ({})",
                icon,
                t.target_regime.to_uppercase(),
                t.probability * 100.0,
                t.probability_label,
            );
            for d in &t.drivers {
                println!("    ▸ {}", d);
            }
            if !t.confirmations.is_empty() {
                println!("    ✓ Confirms: {}", t.confirmations[0]);
            }
            if !t.invalidations.is_empty() {
                println!("    ✗ Invalidates: {}", t.invalidations[0]);
            }
            println!();
        }
    }

    // Historical context
    let h = &report.historical_context;
    if h.total_snapshots > 0 {
        println!("── Historical Context ──");
        println!(
            "  {} snapshots, {} transitions",
            h.total_snapshots, h.total_transitions
        );
        if let Some(avg) = h.avg_days_between_transitions {
            println!("  Avg days between transitions: {:.1}", avg);
        }
        if let Some(ref common) = h.most_common_transition {
            println!("  Most common transition: {}", common);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probability_label_thresholds() {
        assert_eq!(probability_label(0.7), "high");
        assert_eq!(probability_label(0.6), "high");
        assert_eq!(probability_label(0.45), "medium");
        assert_eq!(probability_label(0.3), "medium");
        assert_eq!(probability_label(0.15), "low");
        assert_eq!(probability_label(0.1), "low");
        assert_eq!(probability_label(0.05), "minimal");
        assert_eq!(probability_label(0.0), "minimal");
    }

    #[test]
    fn regime_distance_same() {
        assert_eq!(regime_distance("risk-on", "risk-on"), 0);
    }

    #[test]
    fn regime_distance_adjacent() {
        assert_eq!(regime_distance("risk-on", "lean risk-on"), 1);
        assert_eq!(regime_distance("lean risk-off", "risk-off"), 1);
    }

    #[test]
    fn regime_distance_far() {
        assert_eq!(regime_distance("risk-on", "risk-off"), 4);
    }

    #[test]
    fn regime_order_values() {
        assert!(regime_order("risk-on") > regime_order("neutral"));
        assert!(regime_order("neutral") > regime_order("risk-off"));
        assert!(regime_order("risk-off") > regime_order("crisis"));
    }

    #[test]
    fn normalize_regime_variants() {
        assert_eq!(normalize_regime("risk-on"), "risk-on");
        assert_eq!(normalize_regime("Risk-On"), "risk-on");
        assert_eq!(normalize_regime("lean risk-off"), "lean risk-off");
        assert_eq!(normalize_regime("something-weird"), "transition");
    }

    #[test]
    fn stability_increases_with_days() {
        let m = SignalMomentum {
            vix_direction: None,
            dxy_direction: None,
            yield_direction: None,
            equity_direction: None,
            gold_direction: None,
            oil_direction: None,
            risk_on_momentum: 3,
            risk_off_momentum: 3,
        };
        let s1 = compute_stability(1, 0.5, &m);
        let s2 = compute_stability(20, 0.5, &m);
        assert!(s2 > s1, "stability should increase with duration: {} vs {}", s1, s2);
    }

    #[test]
    fn stability_increases_with_confidence() {
        let m = SignalMomentum {
            vix_direction: None,
            dxy_direction: None,
            yield_direction: None,
            equity_direction: None,
            gold_direction: None,
            oil_direction: None,
            risk_on_momentum: 3,
            risk_off_momentum: 3,
        };
        let s1 = compute_stability(10, 0.3, &m);
        let s2 = compute_stability(10, 0.9, &m);
        assert!(s2 > s1, "stability should increase with confidence: {} vs {}", s1, s2);
    }

    #[test]
    fn stability_clamped_to_unit() {
        let m = SignalMomentum {
            vix_direction: None,
            dxy_direction: None,
            yield_direction: None,
            equity_direction: None,
            gold_direction: None,
            oil_direction: None,
            risk_on_momentum: 6,
            risk_off_momentum: 0,
        };
        let s = compute_stability(100, 1.0, &m);
        assert!(s <= 1.0);
        assert!(s >= 0.0);
    }

    #[test]
    fn candidates_exclude_current_regime() {
        let m = SignalMomentum {
            vix_direction: Some("falling".to_string()),
            dxy_direction: Some("falling".to_string()),
            yield_direction: Some("rising".to_string()),
            equity_direction: Some("rising".to_string()),
            gold_direction: Some("falling".to_string()),
            oil_direction: Some("falling".to_string()),
            risk_on_momentum: 6,
            risk_off_momentum: 0,
        };
        let candidates = build_candidates("risk-on", 0.8, &m, Some(15.0), Some(60.0), &m.gold_direction.clone(), &m.equity_direction.clone());
        assert!(
            !candidates.iter().any(|c| c.target_regime == "risk-on"),
            "should not include current regime as candidate"
        );
    }

    #[test]
    fn candidates_sorted_by_probability() {
        let m = SignalMomentum {
            vix_direction: Some("rising".to_string()),
            dxy_direction: Some("rising".to_string()),
            yield_direction: Some("falling".to_string()),
            equity_direction: Some("falling".to_string()),
            gold_direction: Some("rising".to_string()),
            oil_direction: Some("rising".to_string()),
            risk_on_momentum: 0,
            risk_off_momentum: 6,
        };
        let candidates = build_candidates("neutral", 0.4, &m, Some(28.0), Some(88.0), &m.gold_direction.clone(), &m.equity_direction.clone());
        for w in candidates.windows(2) {
            assert!(
                w[0].probability >= w[1].probability,
                "candidates should be sorted descending: {} >= {}",
                w[0].probability,
                w[1].probability
            );
        }
    }

    #[test]
    fn crisis_probability_elevated_with_high_vix() {
        let m = SignalMomentum {
            vix_direction: Some("rising".to_string()),
            dxy_direction: None,
            yield_direction: None,
            equity_direction: Some("falling".to_string()),
            gold_direction: Some("rising".to_string()),
            oil_direction: Some("rising".to_string()),
            risk_on_momentum: 0,
            risk_off_momentum: 4,
        };
        let candidates = build_candidates("risk-off", 0.6, &m, Some(32.0), Some(95.0), &m.gold_direction.clone(), &m.equity_direction.clone());
        let crisis = candidates.iter().find(|c| c.target_regime == "crisis");
        assert!(crisis.is_some(), "crisis should be a candidate");
        assert!(
            crisis.unwrap().probability >= 0.20,
            "crisis probability should be elevated with VIX>30 and oil>90: {}",
            crisis.unwrap().probability
        );
    }

    #[test]
    fn historical_context_empty() {
        let ctx = compute_historical(&[]);
        assert_eq!(ctx.total_snapshots, 0);
        assert_eq!(ctx.total_transitions, 0);
        assert!(ctx.avg_days_between_transitions.is_none());
        assert!(ctx.most_common_transition.is_none());
    }

    #[test]
    fn historical_context_with_transitions() {
        let snapshots = vec![
            regime_snapshots::RegimeSnapshot {
                id: 3,
                regime: "risk-off".to_string(),
                confidence: Some(0.7),
                drivers: None,
                vix: None,
                dxy: None,
                yield_10y: None,
                oil: None,
                gold: None,
                btc: None,
                recorded_at: "2026-03-28".to_string(),
            },
            regime_snapshots::RegimeSnapshot {
                id: 2,
                regime: "transition".to_string(),
                confidence: Some(0.5),
                drivers: None,
                vix: None,
                dxy: None,
                yield_10y: None,
                oil: None,
                gold: None,
                btc: None,
                recorded_at: "2026-03-25".to_string(),
            },
            regime_snapshots::RegimeSnapshot {
                id: 1,
                regime: "risk-on".to_string(),
                confidence: Some(0.8),
                drivers: None,
                vix: None,
                dxy: None,
                yield_10y: None,
                oil: None,
                gold: None,
                btc: None,
                recorded_at: "2026-03-20".to_string(),
            },
        ];
        let ctx = compute_historical(&snapshots);
        assert_eq!(ctx.total_snapshots, 3);
        assert_eq!(ctx.total_transitions, 2);
    }

    #[test]
    fn parse_date_str_valid() {
        let d = parse_date_str("2026-03-28T14:00:00");
        assert!(d.is_some());
        assert_eq!(d.unwrap().to_string(), "2026-03-28");
    }

    #[test]
    fn parse_date_str_short() {
        let d = parse_date_str("2026-03-28");
        assert!(d.is_some());
    }

    #[test]
    fn parse_date_str_invalid() {
        let d = parse_date_str("invalid");
        assert!(d.is_none());
    }

    #[test]
    fn transition_report_serializes() {
        let report = TransitionReport {
            current_regime: "risk-off".to_string(),
            current_confidence: 0.7,
            days_in_regime: 3,
            stability: 0.55,
            signal_momentum: SignalMomentum {
                vix_direction: Some("rising".to_string()),
                dxy_direction: Some("rising".to_string()),
                yield_direction: Some("falling".to_string()),
                equity_direction: Some("falling".to_string()),
                gold_direction: Some("rising".to_string()),
                oil_direction: Some("rising".to_string()),
                risk_on_momentum: 0,
                risk_off_momentum: 6,
            },
            transitions: vec![TransitionCandidate {
                target_regime: "crisis".to_string(),
                probability: 0.35,
                probability_label: "medium".to_string(),
                drivers: vec!["VIX elevated".to_string()],
                confirmations: vec!["VIX > 30".to_string()],
                invalidations: vec!["VIX drops".to_string()],
            }],
            historical_context: HistoricalContext {
                total_snapshots: 10,
                total_transitions: 3,
                avg_days_between_transitions: Some(5.0),
                most_common_transition: Some("risk-off → transition".to_string()),
            },
        };
        let json = serde_json::to_string(&report);
        assert!(json.is_ok());
        let text = json.unwrap();
        assert!(text.contains("crisis"));
        assert!(text.contains("0.35"));
    }
}
