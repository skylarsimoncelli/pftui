use crate::db::backend::BackendConnection;
use crate::db::scenarios;
use anyhow::Result;
use serde::Serialize;

/// Automated scenario probability suggestion based on signal evidence.
///
/// Analyzes each active scenario's signals (triggered/watching/invalidated)
/// and recent probability trend to suggest whether probability should
/// increase, decrease, or hold. Designed for agent consumption —
/// agents can use this to inform their probability update decisions.

#[derive(Debug, Serialize)]
pub struct ScenarioSuggestion {
    pub scenario_id: i64,
    pub scenario_name: String,
    pub current_probability: f64,
    pub signal_summary: SignalSummary,
    pub trend: ProbabilityTrend,
    pub suggestion: SuggestionAction,
    pub reasoning: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SignalSummary {
    pub total: usize,
    pub triggered: usize,
    pub watching: usize,
    pub invalidated: usize,
    pub other: usize,
    /// Ratio of triggered signals to total (0.0 - 1.0)
    pub trigger_ratio: f64,
    /// Signals that are triggered (for reference)
    pub triggered_signals: Vec<String>,
    /// Signals still being watched
    pub watching_signals: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProbabilityTrend {
    /// Direction of recent probability changes
    pub direction: String,
    /// Number of history entries analyzed
    pub history_count: usize,
    /// Most recent probability change (current - previous)
    pub last_change: Option<f64>,
    /// Average probability over recent history
    pub average: Option<f64>,
    /// Driver of most recent change
    pub last_driver: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SuggestionAction {
    /// "increase", "decrease", "hold"
    pub action: String,
    /// Suggested magnitude: "minor" (1-5%), "moderate" (5-15%), "major" (15%+)
    pub magnitude: String,
    /// Confidence in this suggestion: "low", "medium", "high"
    pub confidence: String,
    /// Suggested new probability (if action is not "hold")
    pub suggested_probability: Option<f64>,
}

#[derive(Debug, Serialize)]
struct SuggestOutput {
    suggestions: Vec<ScenarioSuggestion>,
    summary: SuggestSummary,
}

#[derive(Debug, Serialize)]
struct SuggestSummary {
    total_scenarios: usize,
    needing_increase: usize,
    needing_decrease: usize,
    holding_steady: usize,
}

fn classify_signal_status(status: &str) -> &str {
    let lower = status.to_lowercase();
    if lower.contains("trigger") || lower.contains("confirmed") || lower.contains("fired") {
        "triggered"
    } else if lower.contains("watch") || lower.contains("pending") || lower.contains("monitoring")
    {
        "watching"
    } else if lower.contains("invalid")
        || lower.contains("dismiss")
        || lower.contains("resolved")
        || lower.contains("false")
    {
        "invalidated"
    } else {
        "other"
    }
}

fn compute_signal_summary(signals: &[scenarios::ScenarioSignal]) -> SignalSummary {
    let mut triggered = 0usize;
    let mut watching = 0usize;
    let mut invalidated = 0usize;
    let mut other = 0usize;
    let mut triggered_signals = Vec::new();
    let mut watching_signals = Vec::new();

    for signal in signals {
        match classify_signal_status(&signal.status) {
            "triggered" => {
                triggered += 1;
                triggered_signals.push(signal.signal.clone());
            }
            "watching" => {
                watching += 1;
                watching_signals.push(signal.signal.clone());
            }
            "invalidated" => {
                invalidated += 1;
            }
            _ => {
                other += 1;
            }
        }
    }

    let total = signals.len();
    let active_total = triggered + watching;
    let trigger_ratio = if active_total > 0 {
        triggered as f64 / active_total as f64
    } else {
        0.0
    };

    SignalSummary {
        total,
        triggered,
        watching,
        invalidated,
        other,
        trigger_ratio,
        triggered_signals,
        watching_signals,
    }
}

fn compute_trend(history: &[scenarios::ScenarioHistoryEntry]) -> ProbabilityTrend {
    if history.is_empty() {
        return ProbabilityTrend {
            direction: "unknown".to_string(),
            history_count: 0,
            last_change: None,
            average: None,
            last_driver: None,
        };
    }

    let history_count = history.len();

    // History is typically ordered newest-first
    let last_change = if history.len() >= 2 {
        Some(history[0].probability - history[1].probability)
    } else {
        None
    };

    let average = if !history.is_empty() {
        let sum: f64 = history.iter().map(|h| h.probability).sum();
        Some(sum / history.len() as f64)
    } else {
        None
    };

    let direction = match last_change {
        Some(change) if change > 2.0 => "rising",
        Some(change) if change < -2.0 => "falling",
        Some(_) => "stable",
        None => "unknown",
    }
    .to_string();

    let last_driver = history.first().and_then(|h| h.driver.clone());

    ProbabilityTrend {
        direction,
        history_count,
        last_change,
        average,
        last_driver,
    }
}

fn compute_suggestion(
    scenario: &scenarios::Scenario,
    signal_summary: &SignalSummary,
    trend: &ProbabilityTrend,
) -> (SuggestionAction, Vec<String>) {
    let mut reasoning = Vec::new();
    let current = scenario.probability;

    // Score starts at 0. Positive = increase, negative = decrease.
    let mut score: f64 = 0.0;
    let mut confidence_level = "medium";

    // Factor 1: Signal trigger ratio
    if signal_summary.total > 0 {
        let ratio = signal_summary.trigger_ratio;
        if ratio > 0.7 {
            score += 15.0;
            reasoning.push(format!(
                "Strong signal evidence: {}/{} active signals triggered ({:.0}%)",
                signal_summary.triggered,
                signal_summary.triggered + signal_summary.watching,
                ratio * 100.0
            ));
            confidence_level = "high";
        } else if ratio > 0.5 {
            score += 8.0;
            reasoning.push(format!(
                "Moderate signal evidence: {}/{} active signals triggered ({:.0}%)",
                signal_summary.triggered,
                signal_summary.triggered + signal_summary.watching,
                ratio * 100.0
            ));
        } else if ratio > 0.3 {
            score += 3.0;
            reasoning.push(format!(
                "Some signals triggering: {}/{} active signals ({:.0}%)",
                signal_summary.triggered,
                signal_summary.triggered + signal_summary.watching,
                ratio * 100.0
            ));
        } else if ratio < 0.1 && signal_summary.triggered == 0 && signal_summary.watching > 0 {
            score -= 5.0;
            reasoning.push(format!(
                "No signals triggered yet: 0/{} watching",
                signal_summary.watching
            ));
        }

        // Invalidated signals reduce probability
        if signal_summary.invalidated > 0 {
            let invalidation_weight =
                signal_summary.invalidated as f64 / signal_summary.total as f64;
            let penalty = invalidation_weight * -15.0;
            score += penalty;
            reasoning.push(format!(
                "{} signal(s) invalidated out of {} total",
                signal_summary.invalidated, signal_summary.total
            ));
        }
    } else {
        reasoning.push("No signals tracked for this scenario".to_string());
        confidence_level = "low";
    }

    // Factor 2: Probability trend momentum
    if let Some(change) = trend.last_change {
        if change.abs() > 5.0 {
            // Recent large change — don't pile on, suggest holding
            score *= 0.5;
            reasoning.push(format!(
                "Large recent probability change ({:+.1}%), moderating suggestion",
                change
            ));
        }
    }

    // Factor 3: Boundary effects
    if current >= 90.0 && score > 0.0 {
        score *= 0.3;
        reasoning.push("Already near ceiling (>=90%), limited upside".to_string());
        confidence_level = "low";
    } else if current <= 10.0 && score < 0.0 {
        score *= 0.3;
        reasoning.push("Already near floor (<=10%), limited downside".to_string());
        confidence_level = "low";
    }

    // Determine action
    let (action, magnitude) = if score.abs() < 2.0 {
        ("hold".to_string(), "none".to_string())
    } else if score.abs() < 5.0 {
        let dir = if score > 0.0 { "increase" } else { "decrease" };
        (dir.to_string(), "minor".to_string())
    } else if score.abs() < 15.0 {
        let dir = if score > 0.0 { "increase" } else { "decrease" };
        (dir.to_string(), "moderate".to_string())
    } else {
        let dir = if score > 0.0 { "increase" } else { "decrease" };
        (dir.to_string(), "major".to_string())
    };

    // Compute suggested probability
    let suggested_probability = if action == "hold" {
        None
    } else {
        let delta = match magnitude.as_str() {
            "minor" => {
                if score > 0.0 {
                    3.0
                } else {
                    -3.0
                }
            }
            "moderate" => {
                if score > 0.0 {
                    8.0
                } else {
                    -8.0
                }
            }
            "major" => {
                if score > 0.0 {
                    15.0
                } else {
                    -15.0
                }
            }
            _ => 0.0,
        };
        let new_prob = (current + delta).clamp(0.0, 100.0);
        Some(new_prob)
    };

    if action == "hold" {
        reasoning.push("Evidence does not warrant a probability change at this time".to_string());
    }

    (
        SuggestionAction {
            action,
            magnitude,
            confidence: confidence_level.to_string(),
            suggested_probability,
        },
        reasoning,
    )
}

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let scenarios_list = scenarios::list_scenarios_backend(backend, Some("active"))?;

    if scenarios_list.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "suggestions": [],
                    "summary": {
                        "total_scenarios": 0,
                        "needing_increase": 0,
                        "needing_decrease": 0,
                        "holding_steady": 0
                    }
                }))?
            );
        } else {
            println!("No active scenarios found.");
        }
        return Ok(());
    }

    let mut suggestions = Vec::new();

    for scenario in &scenarios_list {
        let signals = scenarios::list_signals_backend(backend, scenario.id, None)?;
        let history = scenarios::get_history_backend(backend, scenario.id, Some(10))?;

        let signal_summary = compute_signal_summary(&signals);
        let trend = compute_trend(&history);
        let (suggestion, reasoning) = compute_suggestion(scenario, &signal_summary, &trend);

        suggestions.push(ScenarioSuggestion {
            scenario_id: scenario.id,
            scenario_name: scenario.name.clone(),
            current_probability: scenario.probability,
            signal_summary,
            trend,
            suggestion,
            reasoning,
        });
    }

    let mut needing_increase = 0;
    let mut needing_decrease = 0;
    let mut holding_steady = 0;

    for s in &suggestions {
        match s.suggestion.action.as_str() {
            "increase" => needing_increase += 1,
            "decrease" => needing_decrease += 1,
            _ => holding_steady += 1,
        }
    }

    if json_output {
        let output = SuggestOutput {
            suggestions,
            summary: SuggestSummary {
                total_scenarios: scenarios_list.len(),
                needing_increase,
                needing_decrease,
                holding_steady,
            },
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Scenario Probability Suggestions ({} active):",
            scenarios_list.len()
        );
        println!();

        for s in &suggestions {
            let action_symbol = match s.suggestion.action.as_str() {
                "increase" => "▲",
                "decrease" => "▼",
                _ => "—",
            };

            let action_detail = if let Some(suggested) = s.suggestion.suggested_probability {
                format!(
                    "{} {} {:.1}% → {:.1}% ({} change, {} confidence)",
                    action_symbol,
                    s.suggestion.action.to_uppercase(),
                    s.current_probability,
                    suggested,
                    s.suggestion.magnitude,
                    s.suggestion.confidence
                )
            } else {
                format!(
                    "{} HOLD at {:.1}% ({} confidence)",
                    action_symbol, s.current_probability, s.suggestion.confidence
                )
            };

            println!("  {:30} {}", s.scenario_name, action_detail);

            let signals_line = format!(
                "    Signals: {} triggered, {} watching, {} invalidated (of {} total)",
                s.signal_summary.triggered,
                s.signal_summary.watching,
                s.signal_summary.invalidated,
                s.signal_summary.total
            );
            println!("{}", signals_line);

            for reason in &s.reasoning {
                println!("    • {}", reason);
            }
            println!();
        }

        println!(
            "Summary: {} increase, {} decrease, {} hold",
            needing_increase, needing_decrease, holding_steady
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_signal_status() {
        assert_eq!(classify_signal_status("triggered"), "triggered");
        assert_eq!(classify_signal_status("Triggered"), "triggered");
        assert_eq!(classify_signal_status("confirmed"), "triggered");
        assert_eq!(classify_signal_status("watching"), "watching");
        assert_eq!(classify_signal_status("Watching"), "watching");
        assert_eq!(classify_signal_status("pending"), "watching");
        assert_eq!(classify_signal_status("monitoring"), "watching");
        assert_eq!(classify_signal_status("invalidated"), "invalidated");
        assert_eq!(classify_signal_status("dismissed"), "invalidated");
        assert_eq!(classify_signal_status("resolved"), "invalidated");
        assert_eq!(classify_signal_status("unknown_status"), "other");
    }

    #[test]
    fn test_signal_summary_empty() {
        let summary = compute_signal_summary(&[]);
        assert_eq!(summary.total, 0);
        assert_eq!(summary.triggered, 0);
        assert_eq!(summary.watching, 0);
        assert_eq!(summary.invalidated, 0);
        assert_eq!(summary.trigger_ratio, 0.0);
    }

    #[test]
    fn test_signal_summary_mixed() {
        let signals = vec![
            scenarios::ScenarioSignal {
                id: 1,
                scenario_id: 1,
                signal: "Oil above $100".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 2,
                scenario_id: 1,
                signal: "VIX above 30".to_string(),
                status: "watching".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 3,
                scenario_id: 1,
                signal: "GDP below 0".to_string(),
                status: "invalidated".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
        ];

        let summary = compute_signal_summary(&signals);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.triggered, 1);
        assert_eq!(summary.watching, 1);
        assert_eq!(summary.invalidated, 1);
        assert!((summary.trigger_ratio - 0.5).abs() < 0.01);
        assert_eq!(summary.triggered_signals, vec!["Oil above $100"]);
        assert_eq!(summary.watching_signals, vec!["VIX above 30"]);
    }

    #[test]
    fn test_signal_summary_all_triggered() {
        let signals = vec![
            scenarios::ScenarioSignal {
                id: 1,
                scenario_id: 1,
                signal: "Signal A".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 2,
                scenario_id: 1,
                signal: "Signal B".to_string(),
                status: "confirmed".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
        ];

        let summary = compute_signal_summary(&signals);
        assert_eq!(summary.triggered, 2);
        assert_eq!(summary.watching, 0);
        assert!((summary.trigger_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_trend_empty_history() {
        let trend = compute_trend(&[]);
        assert_eq!(trend.direction, "unknown");
        assert_eq!(trend.history_count, 0);
        assert!(trend.last_change.is_none());
        assert!(trend.average.is_none());
    }

    #[test]
    fn test_trend_rising() {
        let history = vec![
            scenarios::ScenarioHistoryEntry {
                id: 2,
                scenario_id: 1,
                probability: 50.0,
                driver: Some("Oil spike".to_string()),
                recorded_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioHistoryEntry {
                id: 1,
                scenario_id: 1,
                probability: 40.0,
                driver: None,
                recorded_at: "2026-03-25".to_string(),
            },
        ];

        let trend = compute_trend(&history);
        assert_eq!(trend.direction, "rising");
        assert_eq!(trend.last_change, Some(10.0));
        assert_eq!(trend.average, Some(45.0));
        assert_eq!(trend.last_driver.as_deref(), Some("Oil spike"));
    }

    #[test]
    fn test_trend_stable() {
        let history = vec![
            scenarios::ScenarioHistoryEntry {
                id: 2,
                scenario_id: 1,
                probability: 41.0,
                driver: None,
                recorded_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioHistoryEntry {
                id: 1,
                scenario_id: 1,
                probability: 40.0,
                driver: None,
                recorded_at: "2026-03-25".to_string(),
            },
        ];

        let trend = compute_trend(&history);
        assert_eq!(trend.direction, "stable");
    }

    #[test]
    fn test_suggestion_hold_no_signals() {
        let scenario = scenarios::Scenario {
            id: 1,
            name: "Test".to_string(),
            probability: 50.0,
            description: None,
            asset_impact: None,
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "hypothesis".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };

        let signal_summary = compute_signal_summary(&[]);
        let trend = compute_trend(&[]);
        let (suggestion, reasoning) = compute_suggestion(&scenario, &signal_summary, &trend);

        assert_eq!(suggestion.action, "hold");
        assert!(reasoning
            .iter()
            .any(|r| r.contains("No signals tracked")));
    }

    #[test]
    fn test_suggestion_increase_strong_signals() {
        let scenario = scenarios::Scenario {
            id: 1,
            name: "War Escalation".to_string(),
            probability: 40.0,
            description: None,
            asset_impact: None,
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "hypothesis".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };

        let signals = vec![
            scenarios::ScenarioSignal {
                id: 1,
                scenario_id: 1,
                signal: "Oil above $100".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 2,
                scenario_id: 1,
                signal: "Carrier groups deployed".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 3,
                scenario_id: 1,
                signal: "Hormuz closed".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
        ];

        let signal_summary = compute_signal_summary(&signals);
        let trend = compute_trend(&[]);
        let (suggestion, _reasoning) = compute_suggestion(&scenario, &signal_summary, &trend);

        assert_eq!(suggestion.action, "increase");
        assert!(suggestion.suggested_probability.is_some());
        let suggested = suggestion.suggested_probability.unwrap();
        assert!(suggested > 40.0, "Should suggest higher than current 40%");
    }

    #[test]
    fn test_suggestion_decrease_invalidated() {
        let scenario = scenarios::Scenario {
            id: 1,
            name: "Soft Landing".to_string(),
            probability: 30.0,
            description: None,
            asset_impact: None,
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "hypothesis".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };

        let signals = vec![
            scenarios::ScenarioSignal {
                id: 1,
                scenario_id: 1,
                signal: "Unemployment below 4%".to_string(),
                status: "invalidated".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 2,
                scenario_id: 1,
                signal: "GDP growth above 2%".to_string(),
                status: "invalidated".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 3,
                scenario_id: 1,
                signal: "Fed rate cuts".to_string(),
                status: "watching".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
        ];

        let signal_summary = compute_signal_summary(&signals);
        let trend = compute_trend(&[]);
        let (suggestion, _reasoning) = compute_suggestion(&scenario, &signal_summary, &trend);

        assert_eq!(suggestion.action, "decrease");
        assert!(suggestion.suggested_probability.is_some());
        let suggested = suggestion.suggested_probability.unwrap();
        assert!(suggested < 30.0, "Should suggest lower than current 30%");
    }

    #[test]
    fn test_suggestion_ceiling_dampening() {
        let scenario = scenarios::Scenario {
            id: 1,
            name: "Inflation".to_string(),
            probability: 95.0,
            description: None,
            asset_impact: None,
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "hypothesis".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };

        let signals = vec![scenarios::ScenarioSignal {
            id: 1,
            scenario_id: 1,
            signal: "CPI above 4%".to_string(),
            status: "triggered".to_string(),
            evidence: None,
            source: None,
            updated_at: "2026-03-26".to_string(),
        }];

        let signal_summary = compute_signal_summary(&signals);
        let trend = compute_trend(&[]);
        let (_suggestion, reasoning) = compute_suggestion(&scenario, &signal_summary, &trend);

        // Should dampen the suggestion due to ceiling
        assert!(reasoning.iter().any(|r| r.contains("ceiling")));
    }

    #[test]
    fn test_suggested_probability_clamped() {
        let scenario = scenarios::Scenario {
            id: 1,
            name: "Test".to_string(),
            probability: 98.0,
            description: None,
            asset_impact: None,
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "hypothesis".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };

        let signals = vec![
            scenarios::ScenarioSignal {
                id: 1,
                scenario_id: 1,
                signal: "A".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
            scenarios::ScenarioSignal {
                id: 2,
                scenario_id: 1,
                signal: "B".to_string(),
                status: "triggered".to_string(),
                evidence: None,
                source: None,
                updated_at: "2026-03-26".to_string(),
            },
        ];

        let signal_summary = compute_signal_summary(&signals);
        let trend = compute_trend(&[]);
        let (suggestion, _) = compute_suggestion(&scenario, &signal_summary, &trend);

        if let Some(prob) = suggestion.suggested_probability {
            assert!(prob <= 100.0, "Probability should be clamped to 100");
            assert!(prob >= 0.0, "Probability should be clamped to 0");
        }
    }
}
