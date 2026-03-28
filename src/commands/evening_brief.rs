use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::analytics::catalysts::{self, CatalystWindow};
use crate::analytics::deltas::{self, DeltaWindow};
use crate::analytics::impact;
use crate::analytics::narrative::{self, ConvictionShift};
use crate::analytics::situation;
use crate::analytics::synthesis;
use crate::db::alerts::list_alerts_backend;
use crate::db::backend::BackendConnection;
use crate::db::news_cache::get_latest_news_backend;
use crate::db::scenarios::list_scenarios_backend;
use crate::db::user_predictions;

use crate::commands::analytics::{
    build_alignment_rows, build_resolution_entry, DivergenceRow, ResolutionEntry,
};
use crate::commands::correlations::{compute_breaks_backend, interpret_break};
use crate::commands::news_sentiment;

// ==================== Evening Brief Structures ====================

/// A consolidated evening analysis payload for agents.
/// Extends morning-brief with deep analysis: narrative, cross-timeframe
/// resolution, conviction changes, prediction stats, and opportunities.
/// Designed for the evening analyst who previously needed 20+ separate
/// analytics commands to assemble a full picture.
#[derive(Serialize)]
struct EveningBrief {
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
    severity: String,
    interpretation: String,
    signal: String,
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
    };

    // 6. Catalysts (this week)
    let catalysts_val = match catalysts::build_report_backend(backend, CatalystWindow::Week) {
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

    // ---- Evening-specific deep analysis ----

    // 10. Narrative report (recap, key themes, analytical memory)
    let narrative_val = match narrative::build_report_backend(backend, false) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 11. Opportunities (identified entry points, scenario plays)
    let opportunities_val = match impact::build_opportunities_report_backend(backend) {
        Ok(report) => serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 12. Conviction changes (last 7 days)
    let conviction_changes = narrative::conviction_changes_backend(backend, 7);

    // 13. Prediction stats (overall accuracy)
    let prediction_stats = match user_predictions::get_stats_backend(backend) {
        Ok(stats) => serde_json::to_value(&stats).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };

    // 14. Cross-timeframe resolution (divergent assets with resolution guidance)
    let cross_timeframe_resolution = build_cross_timeframe_resolution(backend);

    let brief = EveningBrief {
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
    println!(
        "EVENING ANALYSIS — {}",
        &brief.timestamp[..10]
    );
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
        println!(
            "PREDICTION STATS: {total} total, {scored} scored, {hit_rate:.1}% hit rate"
        );
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
        let result = run(&backend, true);
        assert!(result.is_ok());
    }

    #[test]
    fn evening_brief_terminal_output() {
        let backend = test_backend();
        let result = run(&backend, false);
        assert!(result.is_ok());
    }

    #[test]
    fn evening_brief_scenario_summary_serialize() {
        let s = ScenarioSummary {
            id: 1,
            name: "Test Scenario".to_string(),
            probability: 0.55,
            phase: "developing".to_string(),
            status: "active".to_string(),
            description: Some("A test scenario".to_string()),
            updated_at: "2026-03-28".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("Test Scenario"));
        assert!(json.contains("0.55"));
    }

    #[test]
    fn evening_brief_alerts_summary_serialize() {
        let a = AlertsSummary {
            armed_count: 10,
            triggered_count: 2,
            triggered: vec![serde_json::json!({
                "id": 1,
                "rule": "Gold above 3100",
                "symbol": "GC=F",
                "triggered_at": "2026-03-28T18:00:00Z",
            })],
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("armed_count"));
        assert!(json.contains("Gold above 3100"));
    }

    #[test]
    fn evening_brief_cross_timeframe_resolution_serialize() {
        let ctr = CrossTimeframeResolution {
            resolutions: vec![],
            divergent_count: 0,
            total_assets: 5,
            regime_read: "clean".to_string(),
        };
        let json = serde_json::to_string(&ctr).unwrap();
        assert!(json.contains("divergent_count"));
        assert!(json.contains("total_assets"));
        assert!(json.contains("regime_read"));
    }

    #[test]
    fn evening_brief_full_struct_serialize() {
        let brief = EveningBrief {
            timestamp: "2026-03-28T22:00:00Z".to_string(),
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
    }

    #[test]
    fn evening_brief_sentiment_serialize() {
        let sc = SentimentCategoryJson {
            category: "macro".to_string(),
            count: 20,
            avg_score: -1.5,
            bullish: 5,
            bearish: 12,
            neutral: 3,
            label: "bearish".to_string(),
        };
        let json = serde_json::to_string(&sc).unwrap();
        assert!(json.contains("macro"));
        assert!(json.contains("bearish"));
    }

    #[test]
    fn evening_brief_correlation_break_serialize() {
        let cb = CorrelationBreakJson {
            pair: "BTC-USD/^GSPC".to_string(),
            corr_7d: Some(0.85),
            corr_90d: Some(0.42),
            break_delta: 0.43,
            severity: "moderate".to_string(),
            interpretation: "BTC tracking equities more closely".to_string(),
            signal: "Risk-on trade active".to_string(),
        };
        let json = serde_json::to_string(&cb).unwrap();
        assert!(json.contains("BTC-USD/^GSPC"));
        assert!(json.contains("moderate"));
        assert!(json.contains("interpretation"));
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
