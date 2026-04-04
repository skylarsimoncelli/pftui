//! Shared types and helpers for morning-brief and evening-brief commands.
//!
//! Both brief commands use identical struct definitions for scenarios,
//! correlation breaks, alerts, and sentiment. This module extracts them
//! to avoid duplication and provides the section-filtering infrastructure.

use std::collections::HashSet;

use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::analytics::catalysts::{self, CatalystWindow};
use crate::analytics::deltas::{self, DeltaWindow};
use crate::analytics::impact;
use crate::analytics::situation;
use crate::analytics::synthesis;
use crate::db::alerts::list_alerts_backend;
use crate::db::backend::BackendConnection;
use crate::db::news_cache::get_latest_news_backend;
use crate::db::scenarios::list_scenarios_backend;

use crate::commands::correlations::{compute_breaks_backend, interpret_break};
use crate::commands::news_sentiment;

// ==================== Shared Brief Structures ====================

#[derive(Serialize)]
pub struct ScenarioSummary {
    pub id: i64,
    pub name: String,
    pub probability: f64,
    pub phase: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct CorrelationBreakJson {
    pub pair: String,
    pub corr_7d: Option<f64>,
    pub corr_90d: Option<f64>,
    pub break_delta: f64,
    pub severity: String,
    pub interpretation: String,
    pub signal: String,
}

#[derive(Serialize)]
pub struct AlertsSummary {
    pub armed_count: usize,
    pub triggered_count: usize,
    pub triggered: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct SentimentCategoryJson {
    pub category: String,
    pub count: usize,
    pub avg_score: f64,
    pub bullish: usize,
    pub bearish: usize,
    pub neutral: usize,
    pub label: String,
}

// ==================== Section Filter ====================

/// All known section names for morning-brief.
/// Used by tests and documentation; agents can reference these in --section flags.
#[allow(dead_code)]
pub const MORNING_SECTIONS: &[&str] = &[
    "situation",
    "deltas",
    "synthesis",
    "scenarios",
    "correlation_breaks",
    "catalysts",
    "impact",
    "alerts",
    "news_sentiment",
];

/// Additional section names for evening-brief (on top of morning sections).
/// Used by tests and documentation; agents can reference these in --section flags.
#[allow(dead_code)]
pub const EVENING_EXTRA_SECTIONS: &[&str] = &[
    "narrative",
    "opportunities",
    "conviction_changes",
    "prediction_stats",
    "cross_timeframe_resolution",
];

/// Parse a comma-separated section filter string into a HashSet.
/// Returns None if no filter was provided (meaning "all sections").
/// Unrecognized section names are silently ignored.
pub fn parse_sections(filter: &Option<String>) -> Option<HashSet<String>> {
    filter.as_ref().map(|s| {
        s.split(',')
            .map(|part| part.trim().to_lowercase())
            .filter(|part| !part.is_empty())
            .collect()
    })
}

/// Check whether a section should be included given the filter.
/// If sections is None, all sections are included.
pub fn include_section(sections: &Option<HashSet<String>>, name: &str) -> bool {
    match sections {
        None => true,
        Some(set) => set.contains(name),
    }
}

// ==================== Shared Data Builders ====================

/// Build the situation room section.
pub fn build_situation(backend: &BackendConnection) -> serde_json::Value {
    match situation::build_snapshot_backend(backend) {
        Ok(snap) => serde_json::to_value(&snap).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

/// Build the 24h deltas section.
pub fn build_deltas(backend: &BackendConnection) -> serde_json::Value {
    match deltas::build_report_backend(backend, DeltaWindow::Hours24, false) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

/// Build the cross-timeframe synthesis section.
pub fn build_synthesis(backend: &BackendConnection) -> serde_json::Value {
    match synthesis::build_report_backend(backend) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

/// Build active scenarios sorted by probability desc.
pub fn build_scenarios(backend: &BackendConnection) -> Vec<ScenarioSummary> {
    match list_scenarios_backend(backend, Some("active")) {
        Ok(mut rows) => {
            rows.sort_by(|a, b| {
                b.probability
                    .partial_cmp(&a.probability)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            rows.into_iter()
                .map(|s| ScenarioSummary {
                    id: s.id,
                    name: s.name,
                    probability: s.probability,
                    phase: s.phase,
                    status: s.status,
                    description: s.description,
                    updated_at: s.updated_at,
                })
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

/// Build correlation breaks (threshold 0.30, top 20).
pub fn build_correlation_breaks(backend: &BackendConnection) -> Vec<CorrelationBreakJson> {
    match compute_breaks_backend(backend, 0.30, 20) {
        Ok(breaks) => breaks
            .into_iter()
            .map(|b| {
                let interp = interpret_break(&b);
                CorrelationBreakJson {
                    pair: format!("{}/{}", b.symbol_a, b.symbol_b),
                    corr_7d: b.corr_7d,
                    corr_90d: b.corr_90d,
                    break_delta: b.break_delta,
                    severity: interp.severity,
                    interpretation: interp.interpretation,
                    signal: interp.signal,
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Build catalysts (this week).
pub fn build_catalysts(backend: &BackendConnection) -> serde_json::Value {
    match catalysts::build_report_backend(backend, CatalystWindow::Week) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

/// Build portfolio impact.
pub fn build_impact(backend: &BackendConnection) -> serde_json::Value {
    match impact::build_impact_report_backend(backend) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    }
}

/// Build alerts summary.
pub fn build_alerts(backend: &BackendConnection) -> AlertsSummary {
    match list_alerts_backend(backend) {
        Ok(rules) => {
            let armed_count = rules
                .iter()
                .filter(|r| r.status == AlertStatus::Armed)
                .count();
            let triggered: Vec<&crate::alerts::AlertRule> = rules
                .iter()
                .filter(|r| r.status == AlertStatus::Triggered)
                .collect();
            let triggered_json: Vec<serde_json::Value> = triggered
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "rule": r.rule_text,
                        "symbol": r.symbol,
                        "triggered_at": r.triggered_at,
                    })
                })
                .collect();
            AlertsSummary {
                armed_count,
                triggered_count: triggered_json.len(),
                triggered: triggered_json,
            }
        }
        Err(_) => AlertsSummary {
            armed_count: 0,
            triggered_count: 0,
            triggered: Vec::new(),
        },
    }
}

/// Build news sentiment (last 24h, by category).
pub fn build_news_sentiment(backend: &BackendConnection) -> Vec<SentimentCategoryJson> {
    match get_latest_news_backend(backend, 100, None, None, None, Some(24)) {
        Ok(entries) => {
            let scored = news_sentiment::score_all(&entries);
            let aggs = news_sentiment::aggregate_by_category(&scored);
            aggs.into_iter()
                .map(|a| SentimentCategoryJson {
                    category: a.group,
                    count: a.count,
                    avg_score: a.avg_score,
                    bullish: a.bullish_count,
                    bearish: a.bearish_count,
                    neutral: a.neutral_count,
                    label: a.label.as_str().to_string(),
                })
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

// ==================== Terminal Helpers ====================

/// Print situation section to terminal.
pub fn print_situation(situation: &serde_json::Value) {
    if let Some(headline) = situation.get("headline").and_then(|v| v.as_str()) {
        println!();
        println!("SITUATION: {headline}");
    }
    if let Some(subtitle) = situation.get("subtitle").and_then(|v| v.as_str()) {
        println!("  {subtitle}");
    }

    if let Some(items) = situation.get("watch_now").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            println!();
            println!("WATCH NOW:");
            for item in items.iter().take(5) {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let detail = item.get("detail").and_then(|v| v.as_str()).unwrap_or("");
                let severity = item
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                println!("  [{severity}] {title} — {detail}");
            }
        }
    }
}

/// Print deltas section to terminal.
pub fn print_deltas(deltas: &serde_json::Value) {
    if let Some(radar) = deltas.get("change_radar").and_then(|v| v.as_array()) {
        if !radar.is_empty() {
            println!();
            println!("24H CHANGES:");
            for item in radar.iter().take(10) {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let detail = item.get("detail").and_then(|v| v.as_str()).unwrap_or("");
                let severity = item
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                println!("  [{severity}] {title} — {detail}");
            }
        }
    }
}

/// Print scenarios section to terminal.
pub fn print_scenarios(scenarios: &[ScenarioSummary]) {
    if !scenarios.is_empty() {
        println!();
        println!("SCENARIOS:");
        for s in scenarios {
            println!("  {:.0}% — {} [{}]", s.probability * 100.0, s.name, s.phase);
        }
    }
}

/// Print correlation breaks section to terminal.
pub fn print_correlation_breaks(breaks: &[CorrelationBreakJson]) {
    if !breaks.is_empty() {
        println!();
        println!("CORRELATION BREAKS:");
        for cb in breaks {
            let severity_icon = match cb.severity.as_str() {
                "severe" => "🔴",
                "moderate" => "🟡",
                _ => "🟢",
            };
            println!(
                "\n  {} {} (Δ{:+.2}) — {}",
                severity_icon, cb.pair, cb.break_delta, cb.severity,
            );
            println!(
                "    7d: {:.2}  90d: {:.2}",
                cb.corr_7d.unwrap_or(0.0),
                cb.corr_90d.unwrap_or(0.0),
            );
            println!("    {}", cb.interpretation);
            println!("    → {}", cb.signal);
        }
    }
}

/// Print alerts section to terminal.
pub fn print_alerts(alerts: &AlertsSummary) {
    if alerts.triggered_count > 0 {
        println!();
        println!(
            "ALERTS: {} triggered, {} armed",
            alerts.triggered_count, alerts.armed_count
        );
        for a in &alerts.triggered {
            let rule = a.get("rule").and_then(|v| v.as_str()).unwrap_or("?");
            println!("  🔴 {rule}");
        }
    } else {
        println!();
        println!("ALERTS: 0 triggered, {} armed", alerts.armed_count);
    }
}

/// Print news sentiment section to terminal.
pub fn print_news_sentiment(sentiment: &[SentimentCategoryJson]) {
    if !sentiment.is_empty() {
        println!();
        println!("NEWS SENTIMENT:");
        for ns in sentiment {
            println!(
                "  {} — {} ({} articles, avg {:.1})",
                ns.category, ns.label, ns.count, ns.avg_score
            );
        }
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sections_none_returns_none() {
        assert!(parse_sections(&None).is_none());
    }

    #[test]
    fn parse_sections_empty_returns_empty_set() {
        let result = parse_sections(&Some(String::new()));
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn parse_sections_single() {
        let result = parse_sections(&Some("alerts".to_string())).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains("alerts"));
    }

    #[test]
    fn parse_sections_multiple() {
        let result =
            parse_sections(&Some("alerts,scenarios,situation".to_string())).unwrap();
        assert_eq!(result.len(), 3);
        assert!(result.contains("alerts"));
        assert!(result.contains("scenarios"));
        assert!(result.contains("situation"));
    }

    #[test]
    fn parse_sections_trims_whitespace() {
        let result =
            parse_sections(&Some(" alerts , scenarios ".to_string())).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains("alerts"));
        assert!(result.contains("scenarios"));
    }

    #[test]
    fn parse_sections_lowercases() {
        let result = parse_sections(&Some("ALERTS,Scenarios".to_string())).unwrap();
        assert!(result.contains("alerts"));
        assert!(result.contains("scenarios"));
    }

    #[test]
    fn include_section_none_filter_includes_all() {
        assert!(include_section(&None, "alerts"));
        assert!(include_section(&None, "anything"));
    }

    #[test]
    fn include_section_with_filter() {
        let sections = parse_sections(&Some("alerts,scenarios".to_string()));
        assert!(include_section(&sections, "alerts"));
        assert!(include_section(&sections, "scenarios"));
        assert!(!include_section(&sections, "deltas"));
    }

    #[test]
    fn scenario_summary_serialize() {
        let s = ScenarioSummary {
            id: 1,
            name: "Test Scenario".to_string(),
            probability: 0.35,
            phase: "developing".to_string(),
            status: "active".to_string(),
            description: Some("Test description".to_string()),
            updated_at: "2026-03-26".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("Test Scenario"));
        assert!(json.contains("0.35"));
    }

    #[test]
    fn correlation_break_json_serialize() {
        let cb = CorrelationBreakJson {
            pair: "GC=F/DXY".to_string(),
            corr_7d: Some(-0.45),
            corr_90d: Some(-0.82),
            break_delta: 0.37,
            severity: "minor".to_string(),
            interpretation: "Gold and dollar correlation shifted".to_string(),
            signal: "Monitor DXY direction".to_string(),
        };
        let json = serde_json::to_string(&cb).unwrap();
        assert!(json.contains("GC=F/DXY"));
        assert!(json.contains("0.37"));
        assert!(json.contains("severity"));
        assert!(json.contains("interpretation"));
        assert!(json.contains("signal"));
    }

    #[test]
    fn alerts_summary_serialize() {
        let a = AlertsSummary {
            armed_count: 5,
            triggered_count: 1,
            triggered: vec![serde_json::json!({
                "id": 1,
                "rule": "BTC above 100000",
                "symbol": "BTC-USD",
                "triggered_at": "2026-03-26T10:00:00Z",
            })],
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("armed_count"));
        assert!(json.contains("BTC above 100000"));
    }

    #[test]
    fn sentiment_category_serialize() {
        let sc = SentimentCategoryJson {
            category: "crypto".to_string(),
            count: 15,
            avg_score: 2.3,
            bullish: 10,
            bearish: 3,
            neutral: 2,
            label: "bullish".to_string(),
        };
        let json = serde_json::to_string(&sc).unwrap();
        assert!(json.contains("crypto"));
        assert!(json.contains("bullish"));
    }

    #[test]
    fn morning_sections_list_complete() {
        assert_eq!(MORNING_SECTIONS.len(), 9);
        assert!(MORNING_SECTIONS.contains(&"situation"));
        assert!(MORNING_SECTIONS.contains(&"alerts"));
        assert!(MORNING_SECTIONS.contains(&"news_sentiment"));
    }

    #[test]
    fn evening_extra_sections_list_complete() {
        assert_eq!(EVENING_EXTRA_SECTIONS.len(), 5);
        assert!(EVENING_EXTRA_SECTIONS.contains(&"narrative"));
        assert!(EVENING_EXTRA_SECTIONS.contains(&"cross_timeframe_resolution"));
    }
}
