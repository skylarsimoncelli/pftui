use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::analytics::impact;
use crate::analytics::narrative::{self, ConvictionShift};
use crate::db::backend::BackendConnection;
use crate::db::user_predictions;

use crate::commands::analytics::{
    build_alignment_rows, build_resolution_entry, DivergenceRow, ResolutionEntry,
};

use super::brief_common::{
    self, include_section, parse_sections, AlertsSummary, CorrelationBreakJson, ScenarioSummary,
    SentimentCategoryJson,
};

// ==================== Evening Brief Structures ====================

/// A consolidated evening analysis payload for agents.
/// Extends morning-brief with deep analysis: narrative, cross-timeframe
/// resolution, conviction changes, prediction stats, and opportunities.
/// Designed for the evening analyst who previously needed 20+ separate
/// analytics commands to assemble a full picture.
///
/// When `--section` is used, only requested sections are computed and
/// populated; others are null/empty. The `sections_requested` field
/// documents which sections were included.
#[derive(Serialize)]
struct EveningBrief {
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
    // ---- Evening-specific deep analysis ----
    /// Structured analytical narrative: recap, key themes, analytical memory
    narrative: serde_json::Value,
    /// Identified opportunities: undervalued positions, scenario plays, entry points
    opportunities: serde_json::Value,
    /// Conviction changes in the last 7 days
    conviction_changes: Vec<ConvictionShift>,
    /// Prediction accuracy stats (overall)
    prediction_stats: serde_json::Value,
    /// Cross-timeframe resolution: divergent assets with resolution guidance
    cross_timeframe_resolution: CrossTimeframeResolution,
}

#[derive(Serialize)]
struct CrossTimeframeResolution {
    /// Divergent assets with resolution analysis
    resolutions: Vec<ResolutionEntry>,
    /// Count of divergent assets
    divergent_count: usize,
    /// Total tracked assets
    total_assets: usize,
    /// Regime read used for resolution ("clean", "mixed", "conflicted")
    regime_read: String,
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

    // ---- Shared morning-brief sections ----

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

    // ---- Evening-specific deep analysis ----

    let narrative_val = if include_section(&sections, "narrative") {
        match narrative::build_report_backend(backend, false) {
            Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        }
    } else {
        serde_json::Value::Null
    };

    let opportunities_val = if include_section(&sections, "opportunities") {
        match impact::build_opportunities_report_backend(backend) {
            Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        }
    } else {
        serde_json::Value::Null
    };

    let conviction_changes = if include_section(&sections, "conviction_changes") {
        narrative::conviction_changes_backend(backend, 7)
    } else {
        Vec::new()
    };

    let prediction_stats = if include_section(&sections, "prediction_stats") {
        match user_predictions::get_stats_backend(backend) {
            Ok(stats) => serde_json::to_value(&stats).unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        }
    } else {
        serde_json::Value::Null
    };

    let cross_timeframe_resolution =
        if include_section(&sections, "cross_timeframe_resolution") {
            build_cross_timeframe_resolution(backend)
        } else {
            CrossTimeframeResolution {
                resolutions: Vec::new(),
                divergent_count: 0,
                total_assets: 0,
                regime_read: "skipped".to_string(),
            }
        };

    let brief = EveningBrief {
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
        narrative: narrative_val,
        opportunities: opportunities_val,
        conviction_changes,
        prediction_stats,
        cross_timeframe_resolution,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&brief)?);
    } else {
        print_terminal(&brief);
    }

    Ok(())
}

// ==================== Cross-Timeframe Resolution Builder ====================

fn build_cross_timeframe_resolution(backend: &BackendConnection) -> CrossTimeframeResolution {
    let alignments = build_alignment_rows(backend, None).unwrap_or_default();
    let total_assets = alignments.len();

    // Build divergences: assets where layers disagree
    let mut divergences: Vec<DivergenceRow> = alignments
        .iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .map(|a| {
            let dominant_side = if a.bull_layers > a.bear_layers {
                "bull"
            } else if a.bear_layers > a.bull_layers {
                "bear"
            } else {
                "split"
            }
            .to_string();
            let disagreement_pct = (a.bull_layers.min(a.bear_layers) as f64 / 4.0) * 100.0;
            DivergenceRow {
                symbol: a.symbol.clone(),
                low: a.low.clone(),
                medium: a.medium.clone(),
                high: a.high.clone(),
                macro_bias: a.macro_bias.clone(),
                bull_layers: a.bull_layers,
                bear_layers: a.bear_layers,
                disagreement_pct,
                dominant_side,
            }
        })
        .collect();
    divergences.sort_by(|a, b| {
        b.disagreement_pct
            .partial_cmp(&a.disagreement_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let divergent_count = divergences.len();

    // Compute regime read for resolution context
    let regime_read = if divergent_count == 0 {
        "clean"
    } else if divergent_count as f64 / total_assets.max(1) as f64 > 0.5 {
        "conflicted"
    } else {
        "mixed"
    }
    .to_string();

    // Build resolutions for each divergent asset
    let resolutions: Vec<ResolutionEntry> = divergences
        .iter()
        .map(|div| build_resolution_entry(div, &regime_read))
        .collect();

    CrossTimeframeResolution {
        resolutions,
        divergent_count,
        total_assets,
        regime_read,
    }
}

// ==================== Terminal Output ====================

fn print_terminal(brief: &EveningBrief) {
    println!("EVENING ANALYSIS — {}", &brief.timestamp[..10]);
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

    // ---- Evening-specific sections ----

    // Narrative
    if let Some(recap) = brief.narrative.get("recap") {
        if let Some(events) = recap.get("events").and_then(|v| v.as_array()) {
            if !events.is_empty() {
                println!();
                println!("NARRATIVE RECAP:");
                for event in events.iter().take(8) {
                    let title = event.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    let kind = event.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    println!("  [{kind}] {title}");
                }
            }
        }
    }

    // Conviction changes
    if !brief.conviction_changes.is_empty() {
        println!();
        println!("CONVICTION CHANGES (7d):");
        for c in &brief.conviction_changes {
            println!(
                "  {} — {} → {} ({:+})",
                c.symbol, c.old_score, c.new_score, c.delta
            );
        }
    }

    // Cross-timeframe resolution
    let ctr = &brief.cross_timeframe_resolution;
    if !ctr.resolutions.is_empty() {
        println!();
        println!(
            "CROSS-TIMEFRAME RESOLUTION ({} divergent / {} tracked, regime: {}):",
            ctr.divergent_count, ctr.total_assets, ctr.regime_read
        );
        for r in &ctr.resolutions {
            let stance_icon = match r.stance.as_str() {
                "lean-bull" => "🟢",
                "lean-bear" => "🔴",
                _ => "🟡",
            };
            println!(
                "  {} {} — {} (conf {:.0}%, {})",
                stance_icon,
                r.symbol,
                r.stance,
                r.confidence * 100.0,
                r.severity
            );
            println!("    {}", r.disagreement);
        }
    }

    // Prediction stats
    if let Some(total) = brief.prediction_stats.get("total").and_then(|v| v.as_u64()) {
        let scored = brief
            .prediction_stats
            .get("scored")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let hit_rate = brief
            .prediction_stats
            .get("hit_rate_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        println!();
        println!("PREDICTION STATS: {total} total, {scored} scored, {hit_rate:.1}% hit rate");
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
    fn evening_brief_json_output() {
        let backend = test_backend();
        // Should not panic on empty database
        let result = run(&backend, true, &None);
        assert!(result.is_ok());
    }

    #[test]
    fn evening_brief_terminal_output() {
        let backend = test_backend();
        let result = run(&backend, false, &None);
        assert!(result.is_ok());
    }

    #[test]
    fn evening_brief_full_struct_serialize() {
        let brief = EveningBrief {
            timestamp: "2026-03-28T22:00:00Z".to_string(),
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
            narrative: serde_json::json!(null),
            opportunities: serde_json::json!(null),
            conviction_changes: vec![],
            prediction_stats: serde_json::json!(null),
            cross_timeframe_resolution: CrossTimeframeResolution {
                resolutions: vec![],
                divergent_count: 0,
                total_assets: 0,
                regime_read: "clean".to_string(),
            },
        };
        let json = serde_json::to_string_pretty(&brief).unwrap();
        assert!(json.contains("timestamp"));
        assert!(json.contains("situation"));
        assert!(json.contains("narrative"));
        assert!(json.contains("opportunities"));
        assert!(json.contains("conviction_changes"));
        assert!(json.contains("prediction_stats"));
        assert!(json.contains("cross_timeframe_resolution"));
        // sections_requested should be omitted when None
        assert!(!json.contains("sections_requested"));
    }

    #[test]
    fn evening_brief_section_filter() {
        let backend = test_backend();
        let result = run(
            &backend,
            true,
            &Some("alerts,narrative,scenarios".to_string()),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn evening_brief_sections_requested_in_json() {
        let brief = EveningBrief {
            timestamp: "2026-04-04T22:00:00Z".to_string(),
            sections_requested: Some(vec![
                "alerts".to_string(),
                "narrative".to_string(),
                "scenarios".to_string(),
            ]),
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
            narrative: serde_json::Value::Null,
            opportunities: serde_json::Value::Null,
            conviction_changes: vec![],
            prediction_stats: serde_json::Value::Null,
            cross_timeframe_resolution: CrossTimeframeResolution {
                resolutions: vec![],
                divergent_count: 0,
                total_assets: 0,
                regime_read: "skipped".to_string(),
            },
        };
        let json = serde_json::to_string_pretty(&brief).unwrap();
        assert!(json.contains("sections_requested"));
        assert!(json.contains("narrative"));
    }

    #[test]
    fn evening_brief_single_section() {
        let backend = test_backend();
        let result = run(&backend, true, &Some("narrative".to_string()));
        assert!(result.is_ok());
    }

    #[test]
    fn cross_timeframe_resolution_empty_on_empty_db() {
        let backend = test_backend();
        let ctr = build_cross_timeframe_resolution(&backend);
        assert_eq!(ctr.divergent_count, 0);
        assert_eq!(ctr.total_assets, 0);
        assert_eq!(ctr.regime_read, "clean");
        assert!(ctr.resolutions.is_empty());
    }

    #[test]
    fn cross_timeframe_resolution_regime_read_clean_when_no_divergence() {
        let backend = test_backend();
        let ctr = build_cross_timeframe_resolution(&backend);
        assert_eq!(ctr.regime_read, "clean");
    }
}
