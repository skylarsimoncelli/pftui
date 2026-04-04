use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::db::backend::BackendConnection;

use super::brief_common::{
    self, include_section, parse_sections, AlertsSummary, CorrelationBreakJson, ScenarioSummary,
    SentimentCategoryJson,
};

// ==================== Morning Brief Structures ====================

/// A consolidated morning intelligence payload for agents.
/// Combines situation room, deltas, portfolio brief, correlation breaks,
/// scenario probabilities, triggered alerts, and news sentiment into a
/// single JSON blob so agents make one CLI call instead of 5-6.
///
/// When `--section` is used, only requested sections are computed and
/// populated; others are null/empty. The `sections_requested` field
/// documents which sections were included.
#[derive(Serialize)]
struct MorningBrief {
    timestamp: String,
    /// Which sections were requested (null = all sections included)
    #[serde(skip_serializing_if = "Option::is_none")]
    sections_requested: Option<Vec<String>>,
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

// ==================== Entry Point ====================

pub fn run(
    backend: &BackendConnection,
    json_output: bool,
    section_filter: &Option<String>,
) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339();
    let sections = parse_sections(section_filter);

    // Build sections_requested metadata
    let sections_requested = sections.as_ref().map(|set| {
        let mut v: Vec<String> = set.iter().cloned().collect();
        v.sort();
        v
    });

    // Only compute sections that are requested
    let situation_val = if include_section(&sections, "situation") {
        brief_common::build_situation(backend)
    } else {
        serde_json::Value::Null
    };

    let deltas_val = if include_section(&sections, "deltas") {
        brief_common::build_deltas(backend)
    } else {
        serde_json::Value::Null
    };

    let synthesis_val = if include_section(&sections, "synthesis") {
        brief_common::build_synthesis(backend)
    } else {
        serde_json::Value::Null
    };

    let scenarios = if include_section(&sections, "scenarios") {
        brief_common::build_scenarios(backend)
    } else {
        Vec::new()
    };

    let correlation_breaks = if include_section(&sections, "correlation_breaks") {
        brief_common::build_correlation_breaks(backend)
    } else {
        Vec::new()
    };

    let catalysts_val = if include_section(&sections, "catalysts") {
        brief_common::build_catalysts(backend)
    } else {
        serde_json::Value::Null
    };

    let impact_val = if include_section(&sections, "impact") {
        brief_common::build_impact(backend)
    } else {
        serde_json::Value::Null
    };

    let alerts = if include_section(&sections, "alerts") {
        brief_common::build_alerts(backend)
    } else {
        AlertsSummary {
            armed_count: 0,
            triggered_count: 0,
            triggered: Vec::new(),
        }
    };

    let news_sentiment = if include_section(&sections, "news_sentiment") {
        brief_common::build_news_sentiment(backend)
    } else {
        Vec::new()
    };

    let brief = MorningBrief {
        timestamp,
        sections_requested,
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

    if let Some(ref requested) = brief.sections_requested {
        println!("(sections: {})", requested.join(", "));
    }

    brief_common::print_situation(&brief.situation);
    brief_common::print_deltas(&brief.deltas);
    brief_common::print_scenarios(&brief.scenarios);
    brief_common::print_correlation_breaks(&brief.correlation_breaks);
    brief_common::print_alerts(&brief.alerts);
    brief_common::print_news_sentiment(&brief.news_sentiment);

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
        let result = run(&backend, true, &None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_morning_brief_terminal_output() {
        let backend = test_backend();
        let result = run(&backend, false, &None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_morning_brief_full_struct() {
        let brief = MorningBrief {
            timestamp: "2026-03-26T06:00:00Z".to_string(),
            sections_requested: None,
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
        // sections_requested should be omitted when None
        assert!(!json.contains("sections_requested"));
    }

    #[test]
    fn test_morning_brief_section_filter() {
        let backend = test_backend();
        let result = run(&backend, true, &Some("alerts,scenarios".to_string()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_morning_brief_sections_requested_in_json() {
        let brief = MorningBrief {
            timestamp: "2026-04-04T14:00:00Z".to_string(),
            sections_requested: Some(vec!["alerts".to_string(), "scenarios".to_string()]),
            situation: serde_json::Value::Null,
            deltas: serde_json::Value::Null,
            synthesis: serde_json::Value::Null,
            scenarios: vec![],
            correlation_breaks: vec![],
            catalysts: serde_json::Value::Null,
            impact: serde_json::Value::Null,
            alerts: AlertsSummary {
                armed_count: 0,
                triggered_count: 0,
                triggered: vec![],
            },
            news_sentiment: vec![],
        };
        let json = serde_json::to_string_pretty(&brief).unwrap();
        assert!(json.contains("sections_requested"));
        assert!(json.contains("alerts"));
        assert!(json.contains("scenarios"));
    }

    #[test]
    fn test_morning_brief_single_section() {
        let backend = test_backend();
        let result = run(&backend, true, &Some("alerts".to_string()));
        assert!(result.is_ok());
    }
}
