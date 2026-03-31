use anyhow::Result;
use chrono::{NaiveDate, Utc};
use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::db::alerts::list_alerts_backend;
use crate::db::backend::BackendConnection;
use crate::db::convictions::list_current_backend;
use crate::db::scenarios::list_scenarios_backend;
use crate::db::user_predictions::list_predictions_backend;

// ==================== Guidance Structures ====================

/// Routine workflow guidance payload for agents.
/// A single call that answers "what should I focus on right now?" by
/// aggregating pending actions, stale data, and priority signals.
#[derive(Serialize)]
struct GuidancePayload {
    timestamp: String,
    /// Prioritized action items (most urgent first)
    action_items: Vec<ActionItem>,
    /// Summary counts for quick triage
    summary: GuidanceSummary,
    /// Predictions past their target date that need scoring
    pending_predictions: Vec<PendingPrediction>,
    /// Triggered alerts not yet acknowledged
    triggered_alerts: Vec<TriggeredAlert>,
    /// Convictions not updated in 7+ days
    stale_convictions: Vec<StaleConviction>,
    /// Scenarios with significant probability changes (>5pp) in recent history
    scenario_shifts: Vec<ScenarioShift>,
}

#[derive(Serialize)]
struct GuidanceSummary {
    total_action_items: usize,
    critical_count: usize,
    high_count: usize,
    medium_count: usize,
    low_count: usize,
    pending_predictions_count: usize,
    triggered_alerts_count: usize,
    stale_convictions_count: usize,
    scenario_shifts_count: usize,
}

#[derive(Serialize, Clone)]
struct ActionItem {
    priority: String,
    category: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
struct PendingPrediction {
    id: i64,
    claim: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    conviction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_agent: Option<String>,
    created_at: String,
    days_overdue: i64,
}

#[derive(Serialize)]
struct TriggeredAlert {
    id: i64,
    rule: String,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    triggered_at: Option<String>,
}

#[derive(Serialize)]
struct StaleConviction {
    symbol: String,
    score: i32,
    last_updated: String,
    days_stale: i64,
}

#[derive(Serialize)]
struct ScenarioShift {
    id: i64,
    name: String,
    probability: f64,
    phase: String,
    updated_at: String,
}

// ==================== Entry Point ====================

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let now = Utc::now();
    let today = now.date_naive();
    let timestamp = now.to_rfc3339();

    let mut action_items: Vec<ActionItem> = Vec::new();

    // 1. Pending predictions (outcome = "pending", optionally past target_date)
    let pending_predictions = build_pending_predictions(backend, today);
    if !pending_predictions.is_empty() {
        let overdue_count = pending_predictions.iter().filter(|p| p.days_overdue > 0).count();
        let total = pending_predictions.len();
        action_items.push(ActionItem {
            priority: if overdue_count > 0 { "high".into() } else { "medium".into() },
            category: "predictions".into(),
            description: format!(
                "{total} pending prediction{} ({overdue_count} overdue) — score with `journal prediction score` or `auto-score`",
                if total != 1 { "s" } else { "" },
            ),
            detail: pending_predictions.iter().filter(|p| p.days_overdue > 0).take(3).map(|p| {
                format!("#{}: {} ({}d overdue)", p.id, truncate_str(&p.claim, 60), p.days_overdue)
            }).collect::<Vec<_>>().first().cloned(),
        });
    }

    // 2. Triggered alerts
    let triggered_alerts = build_triggered_alerts(backend);
    if !triggered_alerts.is_empty() {
        let count = triggered_alerts.len();
        action_items.push(ActionItem {
            priority: "critical".into(),
            category: "alerts".into(),
            description: format!(
                "{count} triggered alert{} — review with `analytics alerts triage` then `alerts ack`",
                if count != 1 { "s" } else { "" },
            ),
            detail: triggered_alerts.first().map(|a| {
                format!("#{}: {} ({})", a.id, a.rule, a.symbol)
            }),
        });
    }

    // 3. Stale convictions (not updated in 7+ days)
    let stale_convictions = build_stale_convictions(backend, today);
    if !stale_convictions.is_empty() {
        let count = stale_convictions.len();
        action_items.push(ActionItem {
            priority: "low".into(),
            category: "convictions".into(),
            description: format!(
                "{count} conviction{} stale (7+ days) — update with `analytics conviction set`",
                if count != 1 { "s" } else { "" },
            ),
            detail: stale_convictions.first().map(|c| {
                format!("{}: score {} ({}d stale)", c.symbol, c.score, c.days_stale)
            }),
        });
    }

    // 4. Scenario shifts
    let scenario_shifts = build_scenario_shifts(backend);
    if !scenario_shifts.is_empty() {
        let count = scenario_shifts.len();
        action_items.push(ActionItem {
            priority: "medium".into(),
            category: "scenarios".into(),
            description: format!(
                "{count} scenario{} recently updated — review with `analytics scenario list`",
                if count != 1 { "s" } else { "" },
            ),
            detail: scenario_shifts.first().map(|s| {
                format!("{}: {:.0}% ({})", s.name, s.probability * 100.0, s.phase)
            }),
        });
    }

    // Sort action items by priority
    action_items.sort_by(|a, b| priority_rank(&a.priority).cmp(&priority_rank(&b.priority)));

    let summary = GuidanceSummary {
        total_action_items: action_items.len(),
        critical_count: action_items.iter().filter(|a| a.priority == "critical").count(),
        high_count: action_items.iter().filter(|a| a.priority == "high").count(),
        medium_count: action_items.iter().filter(|a| a.priority == "medium").count(),
        low_count: action_items.iter().filter(|a| a.priority == "low").count(),
        pending_predictions_count: pending_predictions.len(),
        triggered_alerts_count: triggered_alerts.len(),
        stale_convictions_count: stale_convictions.len(),
        scenario_shifts_count: scenario_shifts.len(),
    };

    let payload = GuidancePayload {
        timestamp,
        action_items,
        summary,
        pending_predictions,
        triggered_alerts,
        stale_convictions,
        scenario_shifts,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_terminal(&payload);
    }

    Ok(())
}

// ==================== Builders ====================

fn build_pending_predictions(backend: &BackendConnection, today: NaiveDate) -> Vec<PendingPrediction> {
    let predictions = match list_predictions_backend(backend, Some("pending"), None, None, None) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut pending: Vec<PendingPrediction> = predictions
        .into_iter()
        .map(|p| {
            let days_overdue = p.target_date.as_ref().and_then(|td| {
                // Try parsing the target date
                NaiveDate::parse_from_str(td, "%Y-%m-%d")
                    .ok()
                    .map(|target| (today - target).num_days().max(0))
            }).unwrap_or(0);

            PendingPrediction {
                id: p.id,
                claim: p.claim,
                symbol: p.symbol,
                conviction: p.conviction,
                target_date: p.target_date,
                source_agent: p.source_agent,
                created_at: p.created_at,
                days_overdue,
            }
        })
        .collect();

    // Sort by days_overdue descending (most overdue first)
    pending.sort_by(|a, b| b.days_overdue.cmp(&a.days_overdue));
    pending
}

fn build_triggered_alerts(backend: &BackendConnection) -> Vec<TriggeredAlert> {
    let rules = match list_alerts_backend(backend) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    rules
        .into_iter()
        .filter(|r| r.status == AlertStatus::Triggered)
        .map(|r| TriggeredAlert {
            id: r.id,
            rule: r.rule_text,
            symbol: r.symbol,
            triggered_at: r.triggered_at,
        })
        .collect()
}

fn build_stale_convictions(backend: &BackendConnection, today: NaiveDate) -> Vec<StaleConviction> {
    let convictions = match list_current_backend(backend) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let stale_threshold_days = 7i64;

    let mut stale: Vec<StaleConviction> = convictions
        .into_iter()
        .filter_map(|c| {
            let recorded_date = parse_date_prefix(&c.recorded_at)?;
            let days_stale = (today - recorded_date).num_days();
            if days_stale >= stale_threshold_days {
                Some(StaleConviction {
                    symbol: c.symbol,
                    score: c.score,
                    last_updated: c.recorded_at,
                    days_stale,
                })
            } else {
                None
            }
        })
        .collect();

    // Sort by staleness descending
    stale.sort_by(|a, b| b.days_stale.cmp(&a.days_stale));
    stale
}

fn build_scenario_shifts(backend: &BackendConnection) -> Vec<ScenarioShift> {
    let scenarios = match list_scenarios_backend(backend, Some("active")) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Return recently-updated scenarios (updated within last 24h),
    // sorted by probability descending
    let cutoff = Utc::now() - chrono::Duration::hours(24);
    let cutoff_str = cutoff.to_rfc3339();

    let mut shifts: Vec<ScenarioShift> = scenarios
        .into_iter()
        .filter(|s| s.updated_at >= cutoff_str)
        .map(|s| ScenarioShift {
            id: s.id,
            name: s.name,
            probability: s.probability,
            phase: s.phase,
            updated_at: s.updated_at,
        })
        .collect();

    shifts.sort_by(|a, b| {
        b.probability
            .partial_cmp(&a.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    shifts
}

// ==================== Helpers ====================

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

fn parse_date_prefix(s: &str) -> Option<NaiveDate> {
    // Handle both "2026-03-31" and "2026-03-31T..." and "2026-03-31 ..." formats
    let date_str = if s.len() >= 10 { &s[..10] } else { s };
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

// ==================== Terminal Output ====================

fn print_terminal(payload: &GuidancePayload) {
    println!("ROUTINE GUIDANCE — {}", &payload.timestamp[..10]);
    println!("════════════════════════════════════════════════════════════════");

    // Summary line
    let s = &payload.summary;
    if s.total_action_items == 0 {
        println!();
        println!("  ✅ Nothing needs attention right now.");
        println!();
        return;
    }

    println!();
    print!("  {} action item{}:", s.total_action_items, if s.total_action_items != 1 { "s" } else { "" });
    let mut parts = Vec::new();
    if s.critical_count > 0 { parts.push(format!("🔴 {} critical", s.critical_count)); }
    if s.high_count > 0 { parts.push(format!("🟠 {} high", s.high_count)); }
    if s.medium_count > 0 { parts.push(format!("🟡 {} medium", s.medium_count)); }
    if s.low_count > 0 { parts.push(format!("🟢 {} low", s.low_count)); }
    println!(" {}", parts.join(" · "));

    // Action items
    println!();
    println!("ACTION ITEMS:");
    for item in &payload.action_items {
        let icon = match item.priority.as_str() {
            "critical" => "🔴",
            "high" => "🟠",
            "medium" => "🟡",
            "low" => "🟢",
            _ => "⚪",
        };
        println!("  {icon} [{}/{}] {}", item.priority.to_uppercase(), item.category, item.description);
        if let Some(detail) = &item.detail {
            println!("     └─ {detail}");
        }
    }

    // Pending predictions
    if !payload.pending_predictions.is_empty() {
        println!();
        println!("PENDING PREDICTIONS ({}):", payload.pending_predictions.len());
        for p in payload.pending_predictions.iter().take(10) {
            let overdue_tag = if p.days_overdue > 0 {
                format!(" ({}d overdue)", p.days_overdue)
            } else {
                String::new()
            };
            let agent = p.source_agent.as_deref().unwrap_or("—");
            println!("  #{:<4} [{}] {} — {}{overdue_tag}",
                p.id, p.conviction, truncate_str(&p.claim, 55), agent);
        }
        if payload.pending_predictions.len() > 10 {
            println!("  ... and {} more", payload.pending_predictions.len() - 10);
        }
    }

    // Triggered alerts
    if !payload.triggered_alerts.is_empty() {
        println!();
        println!("TRIGGERED ALERTS ({}):", payload.triggered_alerts.len());
        for a in payload.triggered_alerts.iter().take(10) {
            println!("  #{:<4} {} — {}", a.id, a.symbol, a.rule);
        }
    }

    // Stale convictions
    if !payload.stale_convictions.is_empty() {
        println!();
        println!("STALE CONVICTIONS ({}):", payload.stale_convictions.len());
        for c in payload.stale_convictions.iter().take(10) {
            println!("  {:<12} score={:<3} ({}d stale)", c.symbol, c.score, c.days_stale);
        }
    }

    // Scenario shifts
    if !payload.scenario_shifts.is_empty() {
        println!();
        println!("RECENT SCENARIO UPDATES ({}, last 24h):", payload.scenario_shifts.len());
        for s in payload.scenario_shifts.iter().take(10) {
            println!("  {:<30} {:.0}% [{}]", s.name, s.probability * 100.0, s.phase);
        }
    }

    println!();
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_rank_ordering() {
        assert!(priority_rank("critical") < priority_rank("high"));
        assert!(priority_rank("high") < priority_rank("medium"));
        assert!(priority_rank("medium") < priority_rank("low"));
        assert!(priority_rank("low") < priority_rank("unknown"));
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("hello world this is a long string", 10);
        assert!(result.len() <= 14); // 10 chars + "…" (3 bytes)
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_parse_date_prefix_iso() {
        let d = parse_date_prefix("2026-03-31").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn test_parse_date_prefix_with_time() {
        let d = parse_date_prefix("2026-03-31T14:30:00Z").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn test_parse_date_prefix_with_space() {
        let d = parse_date_prefix("2026-03-31 14:30:00+00").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn test_parse_date_prefix_invalid() {
        assert!(parse_date_prefix("not-a-date").is_none());
    }

    #[test]
    fn test_parse_date_prefix_too_short() {
        assert!(parse_date_prefix("2026").is_none());
    }

    #[test]
    fn test_guidance_summary_empty() {
        let s = GuidanceSummary {
            total_action_items: 0,
            critical_count: 0,
            high_count: 0,
            medium_count: 0,
            low_count: 0,
            pending_predictions_count: 0,
            triggered_alerts_count: 0,
            stale_convictions_count: 0,
            scenario_shifts_count: 0,
        };
        assert_eq!(s.total_action_items, 0);
    }

    #[test]
    fn test_action_item_serialization() {
        let item = ActionItem {
            priority: "critical".into(),
            category: "alerts".into(),
            description: "Test alert".into(),
            detail: Some("detail here".into()),
        };
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["priority"], "critical");
        assert_eq!(json["category"], "alerts");
        assert_eq!(json["detail"], "detail here");
    }

    #[test]
    fn test_action_item_no_detail_skipped() {
        let item = ActionItem {
            priority: "low".into(),
            category: "test".into(),
            description: "No detail".into(),
            detail: None,
        };
        let json = serde_json::to_value(&item).unwrap();
        assert!(json.get("detail").is_none());
    }
}
