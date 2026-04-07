use anyhow::Result;
use chrono::{NaiveDate, Utc};
use serde::Serialize;

use crate::alerts::AlertStatus;
use crate::commands::status::{source_statuses_backend, DataSourceStatus, SourceStatus};
use crate::db::alerts::list_alerts_backend;
use crate::db::analyst_views;
use crate::db::backend::BackendConnection;
use crate::db::convictions::list_current_backend;
use crate::db::scenarios::list_scenarios_backend;
use crate::db::user_predictions::list_predictions_backend;
use crate::models::asset::AssetCategory;

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
    /// Analyst views that are missing or stale (7+ days) for portfolio assets
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stale_views: Vec<StaleView>,
    /// Portfolio-matrix view coverage stats
    #[serde(skip_serializing_if = "Option::is_none")]
    view_coverage: Option<ViewCoverage>,
    /// Status-based stale/empty feed summary from `data status`
    #[serde(skip_serializing_if = "Option::is_none")]
    data_health: Option<DataHealthSummary>,
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
    stale_views_count: usize,
    view_coverage_pct: u64,
    degraded_data_sources_count: usize,
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

/// An analyst view that is either missing or stale for a portfolio asset.
#[derive(Serialize, Clone)]
struct StaleView {
    asset: String,
    analyst: String,
    status: String, // "missing" or "stale"
    #[serde(skip_serializing_if = "Option::is_none")]
    last_updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days_stale: Option<i64>,
}

/// View coverage statistics for the portfolio matrix.
#[derive(Serialize, Clone)]
struct ViewCoverage {
    total_assets: usize,
    total_cells: usize,
    filled_cells: usize,
    coverage_pct: u64,
    missing_count: usize,
    stale_count: usize,
}

#[derive(Serialize, Clone)]
struct DataHealthSource {
    name: String,
    status: String,
    records: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_fetch: Option<String>,
}

#[derive(Serialize, Clone)]
struct DataHealthSummary {
    total_sources: usize,
    fresh_count: usize,
    stale_count: usize,
    empty_count: usize,
    degraded_count: usize,
    degraded_sources: Vec<DataHealthSource>,
}

// ==================== Entry Point ====================

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let now = Utc::now();
    let today = now.date_naive();
    let timestamp = now.to_rfc3339();

    let mut action_items: Vec<ActionItem> = Vec::new();
    let data_health = build_data_health(backend);

    if let Some(health) = &data_health {
        if health.degraded_count > 0 {
            let priority = if health.empty_count > 0 {
                "critical"
            } else if health.stale_count >= 3 {
                "high"
            } else {
                "medium"
            };
            let detail = health
                .degraded_sources
                .iter()
                .take(3)
                .map(|source| format!("{} ({})", source.name, source.status))
                .collect::<Vec<_>>()
                .join(", ");
            action_items.push(ActionItem {
                priority: priority.into(),
                category: "data_health".into(),
                description: format!(
                    "{} degraded data source{} detected — review with `data status` or refresh with `data refresh --stale`",
                    health.degraded_count,
                    if health.degraded_count != 1 { "s" } else { "" },
                ),
                detail: if detail.is_empty() { None } else { Some(detail) },
            });
        }
    }

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

    // 5. Stale/missing analyst views
    let (stale_views, view_coverage) = build_stale_views(backend, today);
    if !stale_views.is_empty() {
        let missing_count = stale_views.iter().filter(|v| v.status == "missing").count();
        let stale_count = stale_views.iter().filter(|v| v.status == "stale").count();
        let coverage_pct = view_coverage.as_ref().map(|c| c.coverage_pct).unwrap_or(0);
        let mut parts = Vec::new();
        if missing_count > 0 {
            parts.push(format!("{missing_count} missing"));
        }
        if stale_count > 0 {
            parts.push(format!("{stale_count} stale"));
        }
        action_items.push(ActionItem {
            priority: if coverage_pct < 25 { "medium".into() } else { "low".into() },
            category: "views".into(),
            description: format!(
                "Analyst views: {} ({coverage_pct}% coverage) — update with `analytics views set`",
                parts.join(", "),
            ),
            detail: stale_views.first().map(|v| {
                if v.status == "missing" {
                    format!("{}/{}: no view set", v.asset, v.analyst)
                } else {
                    format!(
                        "{}/{}: {}d stale",
                        v.asset,
                        v.analyst,
                        v.days_stale.unwrap_or(0)
                    )
                }
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
        stale_views_count: stale_views.len(),
        view_coverage_pct: view_coverage.as_ref().map(|c| c.coverage_pct).unwrap_or(0),
        degraded_data_sources_count: data_health
            .as_ref()
            .map(|health| health.degraded_count)
            .unwrap_or(0),
    };

    let payload = GuidancePayload {
        timestamp,
        action_items,
        summary,
        pending_predictions,
        triggered_alerts,
        stale_convictions,
        scenario_shifts,
        stale_views,
        view_coverage,
        data_health,
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

fn build_stale_views(
    backend: &BackendConnection,
    today: NaiveDate,
) -> (Vec<StaleView>, Option<ViewCoverage>) {
    use std::collections::BTreeSet;

    // Collect portfolio symbols (held + watchlist, excluding cash)
    let mut symbols = BTreeSet::new();

    if let Ok(rows) = crate::db::transactions::get_unique_symbols_backend(backend) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                symbols.insert(sym.to_uppercase());
            }
        }
    }
    if let Ok(rows) = crate::db::allocations::get_unique_allocation_symbols_backend(backend) {
        for (sym, cat) in rows {
            if cat != AssetCategory::Cash {
                symbols.insert(sym.to_uppercase());
            }
        }
    }
    if let Ok(rows) = crate::db::watchlist::get_watchlist_symbols_backend(backend) {
        for (sym, _cat) in rows {
            symbols.insert(sym.to_uppercase());
        }
    }

    if symbols.is_empty() {
        return (Vec::new(), None);
    }

    let portfolio_symbols: Vec<String> = symbols.into_iter().collect();

    // Get the full view matrix
    let matrix = match analyst_views::get_portfolio_view_matrix_backend(backend, &portfolio_symbols)
    {
        Ok(m) => m,
        Err(_) => return (Vec::new(), None),
    };

    let analysts = ["low", "medium", "high", "macro"];
    let stale_threshold_days = 7i64;
    let total_assets = matrix.len();
    let total_cells = total_assets * analysts.len();
    let mut filled_cells = 0usize;
    let mut stale_views = Vec::new();
    let mut missing_count = 0usize;
    let mut stale_count = 0usize;

    for row in &matrix {
        for analyst in &analysts {
            if let Some(v) = row.views.iter().find(|v| v.analyst == *analyst) {
                filled_cells += 1;
                // Check if stale
                if let Some(updated_date) = parse_date_prefix(&v.updated_at) {
                    let days = (today - updated_date).num_days();
                    if days >= stale_threshold_days {
                        stale_views.push(StaleView {
                            asset: row.asset.clone(),
                            analyst: analyst.to_string(),
                            status: "stale".into(),
                            last_updated: Some(v.updated_at.clone()),
                            days_stale: Some(days),
                        });
                        stale_count += 1;
                    }
                }
            } else {
                // Missing view
                stale_views.push(StaleView {
                    asset: row.asset.clone(),
                    analyst: analyst.to_string(),
                    status: "missing".into(),
                    last_updated: None,
                    days_stale: None,
                });
                missing_count += 1;
            }
        }
    }

    // Sort: missing first (more actionable), then stale by days descending
    stale_views.sort_by(|a, b| {
        let a_rank = if a.status == "missing" { 0 } else { 1 };
        let b_rank = if b.status == "missing" { 0 } else { 1 };
        a_rank.cmp(&b_rank).then_with(|| {
            b.days_stale
                .unwrap_or(0)
                .cmp(&a.days_stale.unwrap_or(0))
        })
    });

    let coverage_pct = if total_cells > 0 {
        (filled_cells as f64 / total_cells as f64 * 100.0).round() as u64
    } else {
        0
    };

    let coverage = ViewCoverage {
        total_assets,
        total_cells,
        filled_cells,
        coverage_pct,
        missing_count,
        stale_count,
    };

    (stale_views, Some(coverage))
}

fn build_data_health(backend: &BackendConnection) -> Option<DataHealthSummary> {
    let sources = source_statuses_backend(backend).ok()?;
    Some(data_health_summary_from_sources(&sources))
}

fn data_health_summary_from_sources(sources: &[DataSourceStatus]) -> DataHealthSummary {
    let fresh_count = sources
        .iter()
        .filter(|source| source.status == SourceStatus::Fresh)
        .count();
    let stale_count = sources
        .iter()
        .filter(|source| source.status == SourceStatus::Stale)
        .count();
    let empty_count = sources
        .iter()
        .filter(|source| source.status == SourceStatus::Empty)
        .count();

    let mut degraded_sources: Vec<DataHealthSource> = sources
        .iter()
        .filter(|source| source.status != SourceStatus::Fresh)
        .map(|source| DataHealthSource {
            name: source.name.to_string(),
            status: source.status.as_lowercase_str().to_string(),
            records: source.records,
            last_fetch: source.last_fetch.clone(),
        })
        .collect();

    degraded_sources.sort_by(|a, b| {
        data_health_status_rank(&a.status)
            .cmp(&data_health_status_rank(&b.status))
            .then_with(|| a.name.cmp(&b.name))
    });

    DataHealthSummary {
        total_sources: sources.len(),
        fresh_count,
        stale_count,
        empty_count,
        degraded_count: stale_count + empty_count,
        degraded_sources,
    }
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

fn data_health_status_rank(status: &str) -> u8 {
    match status {
        "empty" => 0,
        "stale" => 1,
        _ => 2,
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

    if let Some(health) = &payload.data_health {
        if health.degraded_count > 0 {
            println!();
            println!(
                "DATA HEALTH: {} degraded of {} tracked sources ({} stale, {} empty)",
                health.degraded_count,
                health.total_sources,
                health.stale_count,
                health.empty_count
            );
            println!(
                "  {}",
                health
                    .degraded_sources
                    .iter()
                    .take(8)
                    .map(|source| format!("{} [{}]", source.name, source.status))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

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

    // Stale/missing analyst views
    if !payload.stale_views.is_empty() {
        if let Some(cov) = &payload.view_coverage {
            println!();
            println!(
                "ANALYST VIEW GAPS ({}/{} cells, {}% coverage):",
                cov.filled_cells, cov.total_cells, cov.coverage_pct
            );
        } else {
            println!();
            println!("ANALYST VIEW GAPS:");
        }
        let missing: Vec<_> = payload
            .stale_views
            .iter()
            .filter(|v| v.status == "missing")
            .collect();
        let stale: Vec<_> = payload
            .stale_views
            .iter()
            .filter(|v| v.status == "stale")
            .collect();
        if !missing.is_empty() {
            let count = missing.len();
            println!(
                "  Missing ({count}): {}",
                missing
                    .iter()
                    .take(8)
                    .map(|v| format!("{}/{}", v.asset, v.analyst))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if count > 8 {
                println!("    ... and {} more", count - 8);
            }
        }
        if !stale.is_empty() {
            let count = stale.len();
            println!(
                "  Stale ({count}): {}",
                stale
                    .iter()
                    .take(8)
                    .map(|v| {
                        format!(
                            "{}/{} ({}d)",
                            v.asset,
                            v.analyst,
                            v.days_stale.unwrap_or(0)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if count > 8 {
                println!("    ... and {} more", count - 8);
            }
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
            stale_views_count: 0,
            view_coverage_pct: 0,
            degraded_data_sources_count: 0,
        };
        assert_eq!(s.total_action_items, 0);
        assert_eq!(s.stale_views_count, 0);
        assert_eq!(s.view_coverage_pct, 0);
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

    #[test]
    fn test_stale_view_serialization_missing() {
        let sv = StaleView {
            asset: "BTC".into(),
            analyst: "high".into(),
            status: "missing".into(),
            last_updated: None,
            days_stale: None,
        };
        let json = serde_json::to_value(&sv).unwrap();
        assert_eq!(json["asset"], "BTC");
        assert_eq!(json["analyst"], "high");
        assert_eq!(json["status"], "missing");
        // Optional fields should be absent
        assert!(json.get("last_updated").is_none());
        assert!(json.get("days_stale").is_none());
    }

    #[test]
    fn test_stale_view_serialization_stale() {
        let sv = StaleView {
            asset: "GC=F".into(),
            analyst: "low".into(),
            status: "stale".into(),
            last_updated: Some("2026-03-20 14:00:00+00".into()),
            days_stale: Some(14),
        };
        let json = serde_json::to_value(&sv).unwrap();
        assert_eq!(json["asset"], "GC=F");
        assert_eq!(json["status"], "stale");
        assert_eq!(json["days_stale"], 14);
        assert!(json["last_updated"].as_str().unwrap().starts_with("2026-03-20"));
    }

    #[test]
    fn test_view_coverage_serialization() {
        let cov = ViewCoverage {
            total_assets: 10,
            total_cells: 40,
            filled_cells: 4,
            coverage_pct: 10,
            missing_count: 36,
            stale_count: 0,
        };
        let json = serde_json::to_value(&cov).unwrap();
        assert_eq!(json["total_assets"], 10);
        assert_eq!(json["total_cells"], 40);
        assert_eq!(json["filled_cells"], 4);
        assert_eq!(json["coverage_pct"], 10);
        assert_eq!(json["missing_count"], 36);
        assert_eq!(json["stale_count"], 0);
    }

    #[test]
    fn test_stale_view_sorting() {
        let mut views = [
            StaleView {
                asset: "A".into(),
                analyst: "low".into(),
                status: "stale".into(),
                last_updated: Some("2026-03-01".into()),
                days_stale: Some(30),
            },
            StaleView {
                asset: "B".into(),
                analyst: "high".into(),
                status: "missing".into(),
                last_updated: None,
                days_stale: None,
            },
            StaleView {
                asset: "C".into(),
                analyst: "macro".into(),
                status: "stale".into(),
                last_updated: Some("2026-03-25".into()),
                days_stale: Some(9),
            },
        ];

        // Apply same sort as builder
        views.sort_by(|a, b| {
            let a_rank = if a.status == "missing" { 0 } else { 1 };
            let b_rank = if b.status == "missing" { 0 } else { 1 };
            a_rank.cmp(&b_rank).then_with(|| {
                b.days_stale
                    .unwrap_or(0)
                    .cmp(&a.days_stale.unwrap_or(0))
            })
        });

        // Missing should come first, then stale sorted by days descending
        assert_eq!(views[0].status, "missing");
        assert_eq!(views[0].asset, "B");
        assert_eq!(views[1].status, "stale");
        assert_eq!(views[1].days_stale, Some(30));
        assert_eq!(views[2].status, "stale");
        assert_eq!(views[2].days_stale, Some(9));
    }

    #[test]
    fn test_view_coverage_priority_low_when_above_25pct() {
        // Coverage >= 25% should produce "low" priority
        let coverage_pct: u64 = 30;
        let priority = if coverage_pct < 25 {
            "medium"
        } else {
            "low"
        };
        assert_eq!(priority, "low");
    }

    #[test]
    fn test_view_coverage_priority_medium_when_below_25pct() {
        // Coverage < 25% should produce "medium" priority
        let coverage_pct: u64 = 4;
        let priority = if coverage_pct < 25 {
            "medium"
        } else {
            "low"
        };
        assert_eq!(priority, "medium");
    }

    #[test]
    fn test_guidance_payload_stale_views_skip_when_empty() {
        let payload = GuidancePayload {
            timestamp: "2026-04-03T20:00:00Z".into(),
            action_items: vec![],
            summary: GuidanceSummary {
                total_action_items: 0,
                critical_count: 0,
                high_count: 0,
                medium_count: 0,
                low_count: 0,
                pending_predictions_count: 0,
                triggered_alerts_count: 0,
                stale_convictions_count: 0,
                scenario_shifts_count: 0,
                stale_views_count: 0,
                view_coverage_pct: 100,
                degraded_data_sources_count: 0,
            },
            pending_predictions: vec![],
            triggered_alerts: vec![],
            stale_convictions: vec![],
            scenario_shifts: vec![],
            stale_views: vec![],
            view_coverage: None,
            data_health: None,
        };
        let json = serde_json::to_value(&payload).unwrap();
        // stale_views should be absent when empty (skip_serializing_if)
        assert!(json.get("stale_views").is_none());
        // view_coverage should be absent when None
        assert!(json.get("view_coverage").is_none());
        assert!(json.get("data_health").is_none());
    }

    #[test]
    fn test_guidance_summary_includes_view_fields() {
        let s = GuidanceSummary {
            total_action_items: 1,
            critical_count: 0,
            high_count: 0,
            medium_count: 1,
            low_count: 0,
            pending_predictions_count: 0,
            triggered_alerts_count: 0,
            stale_convictions_count: 0,
            scenario_shifts_count: 0,
            stale_views_count: 199,
            view_coverage_pct: 4,
            degraded_data_sources_count: 3,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["stale_views_count"], 199);
        assert_eq!(json["view_coverage_pct"], 4);
        assert_eq!(json["degraded_data_sources_count"], 3);
    }

    #[test]
    fn test_data_health_summary_orders_empty_before_stale() {
        let summary = data_health_summary_from_sources(&[
            DataSourceStatus {
                name: "Prices",
                last_fetch: Some("2026-04-06T10:00:00Z".into()),
                records: 10,
                status: SourceStatus::Stale,
            },
            DataSourceStatus {
                name: "News",
                last_fetch: None,
                records: 0,
                status: SourceStatus::Empty,
            },
            DataSourceStatus {
                name: "Calendar",
                last_fetch: Some("2026-04-06T10:05:00Z".into()),
                records: 3,
                status: SourceStatus::Fresh,
            },
        ]);

        assert_eq!(summary.total_sources, 3);
        assert_eq!(summary.fresh_count, 1);
        assert_eq!(summary.stale_count, 1);
        assert_eq!(summary.empty_count, 1);
        assert_eq!(summary.degraded_count, 2);
        assert_eq!(summary.degraded_sources[0].name, "News");
        assert_eq!(summary.degraded_sources[0].status, "empty");
        assert_eq!(summary.degraded_sources[1].name, "Prices");
        assert_eq!(summary.degraded_sources[1].status, "stale");
    }

    #[test]
    fn test_data_health_status_rank_prefers_empty_then_stale() {
        assert!(data_health_status_rank("empty") < data_health_status_rank("stale"));
        assert!(data_health_status_rank("stale") < data_health_status_rank("fresh"));
    }
}
