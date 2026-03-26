use anyhow::Result;
use chrono::Utc;
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

use crate::commands::correlations::compute_breaks_backend;
use crate::commands::news_sentiment;

// ==================== Morning Brief Structures ====================

/// A consolidated morning intelligence payload for agents.
/// Combines situation room, deltas, portfolio brief, correlation breaks,
/// scenario probabilities, triggered alerts, and news sentiment into a
/// single JSON blob so agents make one CLI call instead of 5-6.
#[derive(Serialize)]
struct MorningBrief {
    timestamp: String,
    /// Situation Room: headline, watch-now items, portfolio impacts, risk matrix
    situation: serde_json::Value,
    /// 24h change radar: what moved since last refresh
    deltas: serde_json::Value,
    /// Cross-timeframe alignment, divergence, constraints
    synthesis: serde_json::Value,
    /// Active scenarios with probabilities (sorted by probability desc)
    scenarios: Vec<ScenarioSummary>,
    /// Correlation breaks between tracked pairs (threshold 0.30)
    correlation_breaks: Vec<CorrelationBreakJson>,
    /// Upcoming catalysts (this week)
    catalysts: serde_json::Value,
    /// Portfolio impact analysis: which holdings are most affected by current conditions
    impact: serde_json::Value,
    /// Alerts: triggered (unacked) + armed counts
    alerts: AlertsSummary,
    /// News sentiment aggregation by category (last 24h)
    news_sentiment: Vec<SentimentCategoryJson>,
}

#[derive(Serialize)]
struct ScenarioSummary {
    id: i64,
    name: String,
    probability: f64,
    phase: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    updated_at: String,
}

#[derive(Serialize)]
struct CorrelationBreakJson {
    pair: String,
    corr_7d: Option<f64>,
    corr_90d: Option<f64>,
    break_delta: f64,
}

#[derive(Serialize)]
struct AlertsSummary {
    armed_count: usize,
    triggered_count: usize,
    triggered: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct SentimentCategoryJson {
    category: String,
    count: usize,
    avg_score: f64,
    bullish: usize,
    bearish: usize,
    neutral: usize,
    label: String,
}

// ==================== Entry Point ====================

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339();

    // 1. Situation room
    let situation_val = match situation::build_snapshot_backend(backend) {
        Ok(snap) => serde_json::to_value(&snap).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 2. Deltas (24h window)
    let deltas_val = match deltas::build_report_backend(backend, DeltaWindow::Hours24, false) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 3. Synthesis (cross-timeframe)
    let synthesis_val = match synthesis::build_report_backend(backend) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 4. Active scenarios (sorted by probability desc)
    let scenarios = match list_scenarios_backend(backend, Some("active")) {
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
    };

    // 5. Correlation breaks (threshold 0.30, top 20)
    let correlation_breaks = match compute_breaks_backend(backend, 0.30, 20) {
        Ok(breaks) => breaks
            .into_iter()
            .map(|b| CorrelationBreakJson {
                pair: format!("{}/{}", b.symbol_a, b.symbol_b),
                corr_7d: b.corr_7d,
                corr_90d: b.corr_90d,
                break_delta: b.break_delta,
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    // 6. Catalysts (this week)
    let catalysts_val =
        match catalysts::build_report_backend(backend, CatalystWindow::Week) {
            Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        };

    // 7. Portfolio impact
    let impact_val = match impact::build_impact_report_backend(backend) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 8. Alerts summary
    let alerts = match list_alerts_backend(backend) {
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
    };

    // 9. News sentiment (last 24h, by category)
    let news_sentiment = match get_latest_news_backend(backend, 100, None, None, None, Some(24)) {
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
    };

    let brief = MorningBrief {
        timestamp,
        situation: situation_val,
        deltas: deltas_val,
        synthesis: synthesis_val,
        scenarios,
        correlation_breaks,
        catalysts: catalysts_val,
        impact: impact_val,
        alerts,
        news_sentiment,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&brief)?);
    } else {
        print_terminal(&brief);
    }

    Ok(())
}

fn print_terminal(brief: &MorningBrief) {
    println!("MORNING BRIEF — {}", &brief.timestamp[..10]);
    println!("════════════════════════════════════════════════════════════════");

    // Situation headline
    if let Some(headline) = brief.situation.get("headline").and_then(|v| v.as_str()) {
        println!();
        println!("SITUATION: {headline}");
    }
    if let Some(subtitle) = brief.situation.get("subtitle").and_then(|v| v.as_str()) {
        println!("  {subtitle}");
    }

    // Watch now
    if let Some(items) = brief.situation.get("watch_now").and_then(|v| v.as_array()) {
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

    // Deltas
    if let Some(radar) = brief.deltas.get("change_radar").and_then(|v| v.as_array()) {
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

    // Scenarios
    if !brief.scenarios.is_empty() {
        println!();
        println!("SCENARIOS:");
        for s in &brief.scenarios {
            println!("  {:.0}% — {} [{}]", s.probability * 100.0, s.name, s.phase);
        }
    }

    // Correlation breaks
    if !brief.correlation_breaks.is_empty() {
        println!();
        println!("CORRELATION BREAKS:");
        for cb in &brief.correlation_breaks {
            println!(
                "  {} — 7d: {:.2}, 90d: {:.2}, Δ: {:.2}",
                cb.pair,
                cb.corr_7d.unwrap_or(0.0),
                cb.corr_90d.unwrap_or(0.0),
                cb.break_delta,
            );
        }
    }

    // Alerts
    if brief.alerts.triggered_count > 0 {
        println!();
        println!(
            "ALERTS: {} triggered, {} armed",
            brief.alerts.triggered_count, brief.alerts.armed_count
        );
        for a in &brief.alerts.triggered {
            let rule = a.get("rule").and_then(|v| v.as_str()).unwrap_or("?");
            println!("  🔴 {rule}");
        }
    } else {
        println!();
        println!("ALERTS: 0 triggered, {} armed", brief.alerts.armed_count);
    }

    // News sentiment
    if !brief.news_sentiment.is_empty() {
        println!();
        println!("NEWS SENTIMENT:");
        for ns in &brief.news_sentiment {
            println!(
                "  {} — {} ({} articles, avg {:.1})",
                ns.category, ns.label, ns.count, ns.avg_score
            );
        }
    }

    println!();
    println!("Use --json for full machine-readable output.");
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use rusqlite::Connection;

    fn test_backend() -> BackendConnection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_morning_brief_json_output() {
        let backend = test_backend();
        // Should not panic on empty database
        let result = run(&backend, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_morning_brief_terminal_output() {
        let backend = test_backend();
        let result = run(&backend, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_scenario_summary_serialize() {
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
    fn test_correlation_break_json_serialize() {
        let cb = CorrelationBreakJson {
            pair: "GC=F/DXY".to_string(),
            corr_7d: Some(-0.45),
            corr_90d: Some(-0.82),
            break_delta: 0.37,
        };
        let json = serde_json::to_string(&cb).unwrap();
        assert!(json.contains("GC=F/DXY"));
        assert!(json.contains("0.37"));
    }

    #[test]
    fn test_alerts_summary_serialize() {
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
    fn test_sentiment_category_serialize() {
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
    fn test_morning_brief_full_struct() {
        let brief = MorningBrief {
            timestamp: "2026-03-26T06:00:00Z".to_string(),
            situation: serde_json::json!({"headline": "test"}),
            deltas: serde_json::json!({"change_radar": []}),
            synthesis: serde_json::json!(null),
            scenarios: vec![],
            correlation_breaks: vec![],
            catalysts: serde_json::json!(null),
            impact: serde_json::json!(null),
            alerts: AlertsSummary {
                armed_count: 0,
                triggered_count: 0,
                triggered: vec![],
            },
            news_sentiment: vec![],
        };
        let json = serde_json::to_string_pretty(&brief).unwrap();
        assert!(json.contains("timestamp"));
        assert!(json.contains("situation"));
        assert!(json.contains("scenarios"));
        assert!(json.contains("correlation_breaks"));
    }
}
