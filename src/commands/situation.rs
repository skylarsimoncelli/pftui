use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::cli::SituationCommand;
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::scenarios;

#[derive(Debug, Serialize)]
struct SituationListEntry {
    id: i64,
    name: String,
    probability: f64,
    phase: String,
    status: String,
    branch_count: usize,
    impact_count: usize,
    indicator_count: usize,
    indicators_triggered: usize,
    update_count: usize,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct SituationView {
    scenario: scenarios::Scenario,
    branches: Vec<scenarios::ScenarioBranch>,
    impacts: Vec<scenarios::ScenarioImpact>,
    indicators: Vec<scenarios::ScenarioIndicator>,
    updates: Vec<scenarios::ScenarioUpdate>,
}

#[derive(Debug, Serialize)]
struct ExposureEntry {
    scenario_id: i64,
    scenario_name: String,
    direction: String,
    tier: String,
    mechanism: Option<String>,
}

pub fn run(backend: &BackendConnection, command: SituationCommand) -> Result<()> {
    match command {
        SituationCommand::Dashboard { .. } => {
            // Dashboard is handled in main.rs via the existing analytics situation path
            Ok(())
        }
        SituationCommand::Matrix { symbol, json } => {
            run_matrix(backend, symbol.as_deref(), json)
        }
        SituationCommand::List { phase, json } => run_list(backend, phase.as_deref(), json),
        SituationCommand::View { situation, json } => run_view(backend, &situation, json),
        SituationCommand::Demote { situation, json } => run_demote(backend, &situation, json),
        SituationCommand::Resolve {
            situation,
            resolution,
            json,
        } => run_resolve(backend, &situation, resolution.as_deref(), json),
        SituationCommand::Branch { command } => run_branch(backend, command),
        SituationCommand::Impact { command } => run_impact(backend, command),
        SituationCommand::Indicator { command } => run_indicator(backend, command),
        SituationCommand::Update { command } => run_update(backend, command),
        SituationCommand::Exposure { symbol, json } => run_exposure(backend, &symbol, json),
        SituationCommand::Populate { json } => run_populate(backend, json),
    }
}

fn find_scenario(backend: &BackendConnection, name: &str) -> Result<scenarios::Scenario> {
    match scenarios::get_scenario_by_name_backend(backend, name)? {
        Some(s) => Ok(s),
        None => bail!("Scenario not found: {}", name),
    }
}

fn find_active_situation(backend: &BackendConnection, name: &str) -> Result<scenarios::Scenario> {
    let s = find_scenario(backend, name)?;
    if s.phase != "active" {
        bail!(
            "Scenario '{}' is in phase '{}', not 'active'. Use `journal scenario promote` first.",
            name,
            s.phase
        );
    }
    Ok(s)
}

fn find_branch_id(backend: &BackendConnection, scenario_id: i64, branch_name: &str) -> Result<i64> {
    let branches = scenarios::list_branches_backend(backend, scenario_id)?;
    for b in branches {
        if b.name.eq_ignore_ascii_case(branch_name) {
            return Ok(b.id);
        }
    }
    bail!("Branch not found: {}", branch_name)
}

fn run_list(backend: &BackendConnection, phase: Option<&str>, json_output: bool) -> Result<()> {
    let phase = phase.unwrap_or("active");
    let scenarios_list = scenarios::list_scenarios_by_phase_backend(backend, phase)?;

    let mut entries = Vec::new();
    for s in &scenarios_list {
        let branches = scenarios::list_branches_backend(backend, s.id)?;
        let impacts = scenarios::list_impacts_backend(backend, s.id)?;
        let indicators = scenarios::list_indicators_backend(backend, s.id)?;
        let updates = scenarios::list_updates_backend(backend, s.id, None)?;
        let triggered = indicators
            .iter()
            .filter(|i| i.status == "triggered")
            .count();

        entries.push(SituationListEntry {
            id: s.id,
            name: s.name.clone(),
            probability: s.probability,
            phase: s.phase.clone(),
            status: s.status.clone(),
            branch_count: branches.len(),
            impact_count: impacts.len(),
            indicator_count: indicators.len(),
            indicators_triggered: triggered,
            update_count: updates.len(),
            updated_at: s.updated_at.clone(),
        });
    }

    if json_output {
        if entries.is_empty() {
            let empty_result = serde_json::json!({
                "situations": [],
                "count": 0,
                "phase": phase,
                "hint": format!(
                    "No {} situations found. Scenarios must be promoted to active situations before they appear here. Use: pftui journal scenario promote --json \"Scenario Name\"",
                    phase
                )
            });
            println!("{}", serde_json::to_string_pretty(&empty_result)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
    } else {
        if entries.is_empty() {
            println!("No {} situations found.", phase);
            println!();
            println!("Promote a scenario: pftui journal scenario promote --json \"Scenario Name\"");
            return Ok(());
        }
        println!("Active Situations");
        println!("════════════════════════════════════════════════════════════════");
        for entry in &entries {
            let indicator_status = if entry.indicator_count > 0 {
                format!(
                    " | indicators: {}/{} triggered",
                    entry.indicators_triggered, entry.indicator_count
                )
            } else {
                String::new()
            };
            println!("\n[{}] {} ({}%)", entry.id, entry.name, entry.probability);
            println!(
                "  {} branches | {} impacts | {} updates{}",
                entry.branch_count, entry.impact_count, entry.update_count, indicator_status
            );
            println!("  Updated: {}", entry.updated_at);
        }
        println!();
    }
    Ok(())
}

fn run_view(backend: &BackendConnection, name: &str, json_output: bool) -> Result<()> {
    let scenario = find_active_situation(backend, name)?;
    let branches = scenarios::list_branches_backend(backend, scenario.id)?;
    let impacts = scenarios::list_impacts_backend(backend, scenario.id)?;
    let indicators = scenarios::list_indicators_backend(backend, scenario.id)?;
    let updates = scenarios::list_updates_backend(backend, scenario.id, Some(20))?;

    let view = SituationView {
        scenario: scenario.clone(),
        branches: branches.clone(),
        impacts: impacts.clone(),
        indicators: indicators.clone(),
        updates: updates.clone(),
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        println!("Situation: {}", scenario.name);
        println!("════════════════════════════════════════════════════════════════");
        println!(
            "Probability: {}%  |  Phase: {}  |  Status: {}",
            scenario.probability, scenario.phase, scenario.status
        );
        if let Some(desc) = &scenario.description {
            println!("Description: {}", desc);
        }
        println!();

        // Branches
        if !branches.is_empty() {
            println!("BRANCHES");
            for b in &branches {
                let status_badge = if b.status != "active" {
                    format!(" [{}]", b.status)
                } else {
                    String::new()
                };
                println!(
                    "  {}. {} — {}%{}",
                    b.id, b.name, b.probability, status_badge
                );
                if let Some(desc) = &b.description {
                    println!("     {}", desc);
                }
            }
            println!();
        }

        // Impact chains
        if !impacts.is_empty() {
            println!("IMPACT CHAINS");
            // Group by tier for display
            for tier in &["primary", "secondary", "tertiary"] {
                let tier_impacts: Vec<_> = impacts.iter().filter(|i| i.tier == *tier).collect();
                if !tier_impacts.is_empty() {
                    for imp in tier_impacts {
                        let parent_note = imp
                            .parent_id
                            .map(|p| format!(" (parent: #{})", p))
                            .unwrap_or_default();
                        let mech = imp
                            .mechanism
                            .as_deref()
                            .map(|m| format!(" — {}", m))
                            .unwrap_or_default();
                        println!(
                            "  [{}] #{} {} {} {}{}{}",
                            tier, imp.id, imp.symbol, imp.direction, tier, mech, parent_note,
                        );
                    }
                }
            }
            println!();
        }

        // Indicators
        if !indicators.is_empty() {
            let triggered = indicators
                .iter()
                .filter(|i| i.status == "triggered")
                .count();
            println!("INDICATORS ({}/{}  triggered)", triggered, indicators.len());
            for ind in &indicators {
                let val = ind
                    .last_value
                    .as_deref()
                    .map(|v| format!(" [current: {}]", v))
                    .unwrap_or_default();
                println!(
                    "  [{}] {} {} {} {} — {}{}",
                    ind.status, ind.symbol, ind.metric, ind.operator, ind.threshold, ind.label, val
                );
            }
            println!();
        }

        // Recent updates
        if !updates.is_empty() {
            println!("RECENT UPDATES");
            for u in updates.iter().take(10) {
                let severity_badge = if u.severity != "normal" {
                    format!("[{}] ", u.severity.to_uppercase())
                } else {
                    String::new()
                };
                println!("  {} {}{}", u.created_at, severity_badge, u.headline);
                if let Some(detail) = &u.detail {
                    println!("     {}", detail);
                }
                if let Some(nd) = &u.next_decision {
                    let at = u
                        .next_decision_at
                        .as_deref()
                        .map(|a| format!(" by {}", a))
                        .unwrap_or_default();
                    println!("     → Next: {}{}", nd, at);
                }
            }
            println!();
        }
    }
    Ok(())
}

fn run_demote(backend: &BackendConnection, name: &str, json_output: bool) -> Result<()> {
    let scenario = find_active_situation(backend, name)?;
    scenarios::demote_scenario_backend(backend, scenario.id)?;

    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "action": "demote",
                "scenario": name,
                "new_phase": "hypothesis",
            })
        );
    } else {
        println!("Demoted '{}' back to hypothesis.", name);
    }
    Ok(())
}

fn run_resolve(
    backend: &BackendConnection,
    name: &str,
    resolution: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let scenario = find_scenario(backend, name)?;
    scenarios::resolve_scenario_backend(backend, scenario.id, resolution)?;

    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "action": "resolve",
                "scenario": name,
                "new_phase": "resolved",
                "resolution_notes": resolution,
            })
        );
    } else {
        println!("Resolved '{}'.", name);
        if let Some(notes) = resolution {
            println!("Resolution: {}", notes);
        }
    }
    Ok(())
}

fn run_branch(
    backend: &BackendConnection,
    command: crate::cli::SituationBranchCommand,
) -> Result<()> {
    use crate::cli::SituationBranchCommand;
    match command {
        SituationBranchCommand::Add {
            situation,
            value,
            probability,
            description,
            json,
        } => {
            let scenario = find_active_situation(backend, &situation)?;
            let id = scenarios::add_branch_backend(
                backend,
                scenario.id,
                &value,
                probability.unwrap_or(0.0),
                description.as_deref(),
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "branch_added",
                        "id": id,
                        "scenario": situation,
                        "branch": value,
                    })
                );
            } else {
                println!("Added branch '{}' (id: {}) to '{}'.", value, id, situation);
            }
        }
        SituationBranchCommand::List { situation, json } => {
            let scenario = find_active_situation(backend, &situation)?;
            let branches = scenarios::list_branches_backend(backend, scenario.id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&branches)?);
            } else {
                if branches.is_empty() {
                    println!("No branches for '{}'.", situation);
                    return Ok(());
                }
                println!("Branches for '{}':", situation);
                for b in &branches {
                    let status_badge = if b.status != "active" {
                        format!(" [{}]", b.status)
                    } else {
                        String::new()
                    };
                    println!(
                        "  #{} {} — {}%{}",
                        b.id, b.name, b.probability, status_badge
                    );
                    if let Some(desc) = &b.description {
                        println!("     {}", desc);
                    }
                }
            }
        }
        SituationBranchCommand::Update {
            id,
            probability,
            status,
            description,
            json,
        } => {
            scenarios::update_branch_backend(
                backend,
                id,
                probability,
                status.as_deref(),
                description.as_deref(),
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "branch_updated",
                        "id": id,
                    })
                );
            } else {
                println!("Updated branch #{}.", id);
            }
        }
    }
    Ok(())
}

fn run_impact(
    backend: &BackendConnection,
    command: crate::cli::SituationImpactCommand,
) -> Result<()> {
    use crate::cli::SituationImpactCommand;
    match command {
        SituationImpactCommand::Add {
            situation,
            symbol,
            direction,
            tier,
            mechanism,
            parent,
            branch,
            json,
        } => {
            let scenario = find_active_situation(backend, &situation)?;
            // Validate direction
            if !["bullish", "bearish", "volatile", "neutral"].contains(&direction.as_str()) {
                bail!(
                    "Invalid direction: {}. Use bullish, bearish, volatile, or neutral.",
                    direction
                );
            }
            if !["primary", "secondary", "tertiary"].contains(&tier.as_str()) {
                bail!(
                    "Invalid tier: {}. Use primary, secondary, or tertiary.",
                    tier
                );
            }
            let branch_id = if let Some(ref bn) = branch {
                Some(find_branch_id(backend, scenario.id, bn)?)
            } else {
                None
            };
            let id = scenarios::add_impact_backend(
                backend,
                scenario.id,
                branch_id,
                &symbol,
                &direction,
                &tier,
                mechanism.as_deref(),
                parent,
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "impact_added",
                        "id": id,
                        "scenario": situation,
                        "symbol": symbol,
                        "direction": direction,
                        "tier": tier,
                    })
                );
            } else {
                println!(
                    "Added impact #{}: {} {} ({}) to '{}'.",
                    id, symbol, direction, tier, situation
                );
            }
        }
        SituationImpactCommand::List {
            situation,
            tree,
            json,
        } => {
            let scenario = find_active_situation(backend, &situation)?;
            let impacts = scenarios::list_impacts_backend(backend, scenario.id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&impacts)?);
            } else if impacts.is_empty() {
                println!("No impacts for '{}'.", situation);
            } else {
                println!("Impact chains for '{}':", situation);
                if tree {
                    // Tree display: show roots first, then children indented
                    let roots: Vec<_> = impacts.iter().filter(|i| i.parent_id.is_none()).collect();
                    for root in roots {
                        print_impact_tree(root, &impacts, 0);
                    }
                } else {
                    for imp in &impacts {
                        let mech = imp
                            .mechanism
                            .as_deref()
                            .map(|m| format!(" — {}", m))
                            .unwrap_or_default();
                        println!(
                            "  #{} [{}] {} {} {}",
                            imp.id, imp.tier, imp.symbol, imp.direction, mech
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn print_impact_tree(
    node: &scenarios::ScenarioImpact,
    all: &[scenarios::ScenarioImpact],
    depth: usize,
) {
    let indent = "  ".repeat(depth + 1);
    let prefix = if depth == 0 { "→" } else { "└→" };
    let mech = node
        .mechanism
        .as_deref()
        .map(|m| format!(" — {}", m))
        .unwrap_or_default();
    println!(
        "{}{} #{} {} {} [{}]{}",
        indent, prefix, node.id, node.symbol, node.direction, node.tier, mech
    );
    let children: Vec<_> = all
        .iter()
        .filter(|i| i.parent_id == Some(node.id))
        .collect();
    for child in children {
        print_impact_tree(child, all, depth + 1);
    }
}

fn run_indicator(
    backend: &BackendConnection,
    command: crate::cli::SituationIndicatorCommand,
) -> Result<()> {
    use crate::cli::SituationIndicatorCommand;
    match command {
        SituationIndicatorCommand::Add {
            situation,
            symbol,
            operator,
            threshold,
            label,
            branch,
            impact,
            metric,
            json,
        } => {
            let scenario = find_active_situation(backend, &situation)?;
            // Validate operator
            if ![
                "<",
                "<=",
                ">",
                ">=",
                "above_sma",
                "below_sma",
                "rsi_above",
                "rsi_below",
            ]
            .contains(&operator.as_str())
            {
                bail!(
                    "Invalid operator: {}. Use >, >=, <, <=, above_sma, below_sma, rsi_above, rsi_below.",
                    operator
                );
            }
            let branch_id = if let Some(ref bn) = branch {
                Some(find_branch_id(backend, scenario.id, bn)?)
            } else {
                None
            };
            let id = scenarios::add_indicator_backend(
                backend,
                scenario.id,
                branch_id,
                impact,
                &symbol,
                &metric,
                &operator,
                &threshold,
                &label,
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "indicator_added",
                        "id": id,
                        "scenario": situation,
                        "symbol": symbol,
                        "operator": operator,
                        "threshold": threshold,
                    })
                );
            } else {
                println!(
                    "Added indicator #{}: {} {} {} {} — {}",
                    id, symbol, metric, operator, threshold, label
                );
            }
        }
        SituationIndicatorCommand::List { situation, json } => {
            let scenario = find_active_situation(backend, &situation)?;
            let indicators = scenarios::list_indicators_backend(backend, scenario.id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&indicators)?);
            } else if indicators.is_empty() {
                println!("No indicators for '{}'.", situation);
            } else {
                let triggered = indicators
                    .iter()
                    .filter(|i| i.status == "triggered")
                    .count();
                println!(
                    "Indicators for '{}' ({}/{} triggered):",
                    situation,
                    triggered,
                    indicators.len()
                );
                for ind in &indicators {
                    let val = ind
                        .last_value
                        .as_deref()
                        .map(|v| format!(" [current: {}]", v))
                        .unwrap_or_default();
                    println!(
                        "  #{} [{}] {} {} {} {} — {}{}",
                        ind.id,
                        ind.status,
                        ind.symbol,
                        ind.metric,
                        ind.operator,
                        ind.threshold,
                        ind.label,
                        val
                    );
                }
            }
        }
    }
    Ok(())
}

fn run_update(
    backend: &BackendConnection,
    command: crate::cli::SituationUpdateCommand,
) -> Result<()> {
    use crate::cli::SituationUpdateCommand;
    match command {
        SituationUpdateCommand::Log {
            situation,
            headline,
            detail,
            severity,
            source,
            source_agent,
            next_decision,
            next_decision_at,
            branch,
            json,
        } => {
            let scenario = find_active_situation(backend, &situation)?;
            if !["low", "normal", "elevated", "critical"].contains(&severity.as_str()) {
                bail!(
                    "Invalid severity: {}. Use low, normal, elevated, or critical.",
                    severity
                );
            }
            let branch_id = if let Some(ref bn) = branch {
                Some(find_branch_id(backend, scenario.id, bn)?)
            } else {
                None
            };
            let id = scenarios::add_update_backend(
                backend,
                scenario.id,
                branch_id,
                &headline,
                detail.as_deref(),
                &severity,
                source.as_deref(),
                source_agent.as_deref(),
                next_decision.as_deref(),
                next_decision_at.as_deref(),
            )?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "update_logged",
                        "id": id,
                        "scenario": situation,
                        "headline": headline,
                        "severity": severity,
                    })
                );
            } else {
                println!("Logged update #{} for '{}'.", id, situation);
            }
        }
        SituationUpdateCommand::List {
            situation,
            limit,
            json,
        } => {
            let scenario = find_active_situation(backend, &situation)?;
            let updates = scenarios::list_updates_backend(backend, scenario.id, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&updates)?);
            } else if updates.is_empty() {
                println!("No updates for '{}'.", situation);
            } else {
                println!("Updates for '{}':", situation);
                for u in &updates {
                    let sev = if u.severity != "normal" {
                        format!("[{}] ", u.severity.to_uppercase())
                    } else {
                        String::new()
                    };
                    println!("  {} {}{}", u.created_at, sev, u.headline);
                    if let Some(detail) = &u.detail {
                        println!("     {}", detail);
                    }
                }
            }
        }
    }
    Ok(())
}

fn run_exposure(backend: &BackendConnection, symbol: &str, json_output: bool) -> Result<()> {
    let impacts = scenarios::list_impacts_by_symbol_backend(backend, symbol)?;
    if impacts.is_empty() {
        if json_output {
            println!("[]");
        } else {
            println!("No active situation impacts for {}.", symbol);
        }
        return Ok(());
    }

    let mut entries = Vec::new();
    for imp in &impacts {
        let scenario = scenarios::list_scenarios_by_phase_backend(backend, "active")?
            .into_iter()
            .find(|s| s.id == imp.scenario_id);
        let scenario_name = scenario
            .map(|s| s.name)
            .unwrap_or_else(|| format!("#{}", imp.scenario_id));
        entries.push(ExposureEntry {
            scenario_id: imp.scenario_id,
            scenario_name,
            direction: imp.direction.clone(),
            tier: imp.tier.clone(),
            mechanism: imp.mechanism.clone(),
        });
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        println!("Situation exposure for {}:", symbol);
        for e in &entries {
            let mech = e
                .mechanism
                .as_deref()
                .map(|m| format!(" — {}", m))
                .unwrap_or_default();
            println!(
                "  {} [{}] {} ({}){}",
                e.scenario_name, e.tier, e.direction, e.scenario_id, mech
            );
        }
    }
    Ok(())
}

// ── Cross-situation matrix ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MatrixReport {
    situations: Vec<MatrixSituation>,
    symbol_overlap: Vec<SymbolOverlap>,
    indicator_summary: IndicatorSummary,
}

#[derive(Debug, Serialize)]
struct MatrixSituation {
    id: i64,
    name: String,
    probability: f64,
    phase: String,
    branches: Vec<MatrixBranch>,
    impacted_symbols: Vec<MatrixImpactedSymbol>,
    indicators: MatrixIndicators,
    latest_update: Option<MatrixUpdate>,
}

#[derive(Debug, Serialize)]
struct MatrixBranch {
    name: String,
    probability: f64,
    status: String,
}

#[derive(Debug, Serialize)]
struct MatrixImpactedSymbol {
    symbol: String,
    direction: String,
    tier: String,
}

#[derive(Debug, Serialize)]
struct MatrixIndicators {
    total: usize,
    watching: usize,
    triggered: usize,
    triggered_labels: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MatrixUpdate {
    headline: String,
    severity: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct SymbolOverlap {
    symbol: String,
    situations: Vec<OverlapEntry>,
}

#[derive(Debug, Serialize)]
struct OverlapEntry {
    situation: String,
    direction: String,
    tier: String,
}

#[derive(Debug, Serialize)]
struct IndicatorSummary {
    total_watching: usize,
    total_triggered: usize,
    recently_triggered: Vec<RecentlyTriggered>,
}

#[derive(Debug, Serialize)]
struct RecentlyTriggered {
    situation: String,
    label: String,
    symbol: String,
    metric: String,
    last_value: Option<String>,
    triggered_at: Option<String>,
}

fn run_matrix(
    backend: &BackendConnection,
    symbol_filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let all_active =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    // Filter to only promoted situations (phase=active), not hypotheses
    let active: Vec<_> = all_active
        .into_iter()
        .filter(|s| s.phase == "active")
        .collect();

    if active.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "situations": [],
                    "symbol_overlap": [],
                    "indicator_summary": {
                        "total_watching": 0,
                        "total_triggered": 0,
                        "recently_triggered": []
                    }
                }))?
            );
        } else {
            println!("No active situations. Promote scenarios with `journal scenario promote`.");
        }
        return Ok(());
    }

    // Collect per-situation data
    let mut matrix_situations = Vec::new();
    // symbol → list of (situation_name, direction, tier)
    let mut symbol_map: BTreeMap<String, Vec<OverlapEntry>> = BTreeMap::new();
    let mut all_watching = 0usize;
    let mut all_triggered = 0usize;
    let mut recently_triggered_list: Vec<RecentlyTriggered> = Vec::new();

    for scenario in &active {
        let branches = scenarios::list_branches_backend(backend, scenario.id)
            .unwrap_or_default();
        let impacts = scenarios::list_impacts_backend(backend, scenario.id)
            .unwrap_or_default();
        let indicators = scenarios::list_indicators_backend(backend, scenario.id)
            .unwrap_or_default();
        let updates = scenarios::list_updates_backend(backend, scenario.id, Some(1))
            .unwrap_or_default();

        // Build impacted symbols (deduplicate by symbol)
        let mut seen_symbols = BTreeSet::new();
        let mut impacted_symbols = Vec::new();
        for impact in &impacts {
            if seen_symbols.insert(impact.symbol.clone()) {
                impacted_symbols.push(MatrixImpactedSymbol {
                    symbol: impact.symbol.clone(),
                    direction: impact.direction.clone(),
                    tier: impact.tier.clone(),
                });
                symbol_map
                    .entry(impact.symbol.clone())
                    .or_default()
                    .push(OverlapEntry {
                        situation: scenario.name.clone(),
                        direction: impact.direction.clone(),
                        tier: impact.tier.clone(),
                    });
            }
        }

        // Indicator stats
        let watching = indicators.iter().filter(|i| i.status == "watching").count();
        let triggered = indicators.iter().filter(|i| i.status == "triggered").count();
        all_watching += watching;
        all_triggered += triggered;

        let triggered_labels: Vec<String> = indicators
            .iter()
            .filter(|i| i.status == "triggered")
            .map(|i| i.label.clone())
            .collect();

        for ind in indicators.iter().filter(|i| i.status == "triggered") {
            recently_triggered_list.push(RecentlyTriggered {
                situation: scenario.name.clone(),
                label: ind.label.clone(),
                symbol: ind.symbol.clone(),
                metric: ind.metric.clone(),
                last_value: ind.last_value.clone(),
                triggered_at: ind.triggered_at.clone(),
            });
        }

        let latest_update = updates.first().map(|u| MatrixUpdate {
            headline: u.headline.clone(),
            severity: u.severity.clone(),
            created_at: u.created_at.clone(),
        });

        matrix_situations.push(MatrixSituation {
            id: scenario.id,
            name: scenario.name.clone(),
            probability: scenario.probability,
            phase: scenario.phase.clone(),
            branches: branches
                .iter()
                .map(|b| MatrixBranch {
                    name: b.name.clone(),
                    probability: b.probability,
                    status: b.status.clone(),
                })
                .collect(),
            impacted_symbols,
            indicators: MatrixIndicators {
                total: indicators.len(),
                watching,
                triggered,
                triggered_labels,
            },
            latest_update,
        });
    }

    // Sort recently triggered by triggered_at descending
    recently_triggered_list.sort_by(|a, b| b.triggered_at.cmp(&a.triggered_at));
    recently_triggered_list.truncate(10);

    // Apply symbol filter if specified
    if let Some(filter) = symbol_filter {
        let filter_upper = filter.to_uppercase();
        matrix_situations.retain(|s| {
            s.impacted_symbols
                .iter()
                .any(|sym| sym.symbol.to_uppercase().contains(&filter_upper))
        });
    }

    // Build symbol overlap
    // When filtering by symbol, show all matching symbols; otherwise only those in 2+ situations
    let symbol_overlap: Vec<SymbolOverlap> = symbol_map
        .into_iter()
        .filter(|(symbol, entries)| {
            if let Some(filter) = symbol_filter {
                symbol.to_uppercase().contains(&filter.to_uppercase())
            } else {
                entries.len() >= 2
            }
        })
        .map(|(symbol, situations)| SymbolOverlap {
            symbol,
            situations,
        })
        .collect();

    let report = MatrixReport {
        situations: matrix_situations,
        symbol_overlap,
        indicator_summary: IndicatorSummary {
            total_watching: all_watching,
            total_triggered: all_triggered,
            recently_triggered: recently_triggered_list,
        },
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_matrix_text(&report);
    }

    Ok(())
}

fn print_matrix_text(report: &MatrixReport) {
    println!("Situation Matrix — Cross-Situation View");
    println!("════════════════════════════════════════════════════════════════");

    if report.situations.is_empty() {
        println!("No active situations match the filter.");
        return;
    }

    for (i, sit) in report.situations.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!(
            "▸ {} ({:.1}%) [{}]",
            sit.name, sit.probability, sit.phase
        );

        if !sit.branches.is_empty() {
            println!("  Branches:");
            for b in &sit.branches {
                println!(
                    "    {} — {:.1}% [{}]",
                    b.name, b.probability, b.status
                );
            }
        }

        if !sit.impacted_symbols.is_empty() {
            let symbols: Vec<String> = sit
                .impacted_symbols
                .iter()
                .map(|s| format!("{} {} ({})", s.symbol, s.direction, s.tier))
                .collect();
            println!("  Impacts: {}", symbols.join(", "));
        }

        println!(
            "  Indicators: {} total, {} watching, {} triggered",
            sit.indicators.total, sit.indicators.watching, sit.indicators.triggered
        );
        if !sit.indicators.triggered_labels.is_empty() {
            println!(
                "    Triggered: {}",
                sit.indicators.triggered_labels.join(", ")
            );
        }

        if let Some(update) = &sit.latest_update {
            println!(
                "  Latest: [{}] {} ({})",
                update.severity, update.headline, update.created_at
            );
        }
    }

    if !report.symbol_overlap.is_empty() {
        println!();
        println!("SYMBOL OVERLAP");
        println!("────────────────────────────────────────────────────────────────");
        for overlap in &report.symbol_overlap {
            let entries: Vec<String> = overlap
                .situations
                .iter()
                .map(|e| format!("{} {} ({})", e.situation, e.direction, e.tier))
                .collect();
            println!("  {} — {}", overlap.symbol, entries.join(" | "));
        }
    }

    let summary = &report.indicator_summary;
    println!();
    println!(
        "INDICATORS: {} watching, {} triggered",
        summary.total_watching, summary.total_triggered
    );
    if !summary.recently_triggered.is_empty() {
        println!("Recently triggered:");
        for t in &summary.recently_triggered {
            let val = t.last_value.as_deref().unwrap_or("—");
            let at = t.triggered_at.as_deref().unwrap_or("—");
            println!(
                "  {} — {} {} = {} [{}] ({})",
                t.situation, t.symbol, t.metric, val, t.label, at
            );
        }
    }
}

#[derive(Debug, Serialize)]
struct PopulateResult {
    populated: Vec<PopulatedScore>,
    sources: PopulateSources,
}

#[derive(Debug, Serialize)]
struct PopulatedScore {
    timeframe: String,
    score: f64,
    summary: String,
}

#[derive(Debug, Serialize)]
struct PopulateSources {
    regime: Option<String>,
    regime_confidence: Option<f64>,
    active_scenarios: usize,
    active_trends: usize,
    structural_cycles: usize,
    convictions: usize,
    technical_signals: usize,
}

/// Derive a LOW timeframe score from regime snapshot data.
///
/// Maps regime classification → base direction, scaled by confidence.
/// Also incorporates technical signal density as a volatility modifier.
fn derive_low_score(backend: &BackendConnection) -> (f64, String) {
    let regime = db::regime_snapshots::get_current_backend(backend)
        .unwrap_or(None);
    let tech_signals = db::technical_signals::list_signals_backend(backend, None, None, Some(200))
        .unwrap_or_default();

    let (base, regime_label) = match regime.as_ref().map(|r| r.regime.as_str()) {
        Some("risk-on") => (40.0, "risk-on"),
        Some("risk-off") => (-40.0, "risk-off"),
        Some("crisis") => (-60.0, "crisis"),
        Some("stagflation") => (-50.0, "stagflation"),
        Some("cautious") => (-15.0, "cautious"),
        Some("neutral") | Some("mixed") => (0.0, "neutral"),
        Some("euphoric") => (60.0, "euphoric"),
        Some(other) => (0.0, other),
        None => return (0.0, "No regime data available".to_string()),
    };

    let confidence = regime
        .as_ref()
        .and_then(|r| r.confidence)
        .unwrap_or(0.5);
    let scaled = base * confidence;

    // Technical signal density modifier: more signals = more conviction in the direction
    let signal_modifier = (tech_signals.len() as f64 / 50.0).min(1.0) * 10.0;
    let final_score = (scaled + if base >= 0.0 { signal_modifier } else { -signal_modifier })
        .clamp(-100.0, 100.0);

    let summary = format!(
        "Regime {} (conf {:.0}%), {} tech signals",
        regime_label,
        confidence * 100.0,
        tech_signals.len()
    );
    (final_score, summary)
}

/// Derive a MEDIUM timeframe score from scenario probabilities and convictions.
///
/// Positive-outlook scenarios (probability weighted) push score up;
/// negative-outlook scenarios push it down. Convictions add directional tilt.
fn derive_medium_score(backend: &BackendConnection) -> (f64, String) {
    let scenarios_list = scenarios::list_scenarios_backend(backend, Some("active"))
        .unwrap_or_default();
    let convictions = db::convictions::list_current_backend(backend)
        .unwrap_or_default();

    if scenarios_list.is_empty() && convictions.is_empty() {
        return (0.0, "No scenario or conviction data available".to_string());
    }

    // Scenario-weighted score: classify each scenario by its impact text
    let mut scenario_score = 0.0;
    let mut scenario_weight = 0.0;
    for s in &scenarios_list {
        let direction = classify_scenario_direction(s);
        scenario_score += direction * (s.probability / 100.0);
        scenario_weight += s.probability / 100.0;
    }
    if scenario_weight > 0.0 {
        scenario_score /= scenario_weight;
    }
    scenario_score *= 50.0; // Scale to -50..+50 range

    // Conviction average: score is -5 to +5, scale to -20..+20
    let conviction_score = if convictions.is_empty() {
        0.0
    } else {
        let avg = convictions.iter().map(|c| c.score as f64).sum::<f64>()
            / convictions.len() as f64;
        avg * 4.0 // -5*4=-20 to 5*4=+20
    };

    let final_score = (scenario_score + conviction_score).clamp(-100.0, 100.0);
    let summary = format!(
        "{} active scenarios, {} convictions",
        scenarios_list.len(),
        convictions.len()
    );
    (final_score, summary)
}

/// Derive a HIGH timeframe score from active trend directions and convictions.
///
/// Bullish trends push score up, bearish down, weighted by conviction level.
fn derive_high_score(backend: &BackendConnection) -> (f64, String) {
    let trends = db::trends::list_trends_backend(backend, Some("active"), None)
        .unwrap_or_default();

    if trends.is_empty() {
        return (0.0, "No active trend data available".to_string());
    }

    let mut score = 0.0;
    for t in &trends {
        let direction_mult = match t.direction.to_lowercase().as_str() {
            "up" | "bullish" => 1.0,
            "down" | "bearish" => -1.0,
            _ => 0.0,
        };
        let conviction_mult = match t.conviction.to_lowercase().as_str() {
            "high" => 1.0,
            "medium" => 0.6,
            "low" => 0.3,
            _ => 0.5,
        };
        score += direction_mult * conviction_mult * 20.0;
    }

    let final_score = (score / trends.len() as f64).clamp(-100.0, 100.0);
    let bull_count = trends.iter().filter(|t| {
        matches!(t.direction.to_lowercase().as_str(), "up" | "bullish")
    }).count();
    let bear_count = trends.iter().filter(|t| {
        matches!(t.direction.to_lowercase().as_str(), "down" | "bearish")
    }).count();

    let summary = format!(
        "{} trends ({} bull, {} bear)",
        trends.len(),
        bull_count,
        bear_count
    );
    (final_score, summary)
}

/// Derive a MACRO timeframe score from structural cycles.
///
/// Cycle stages map to a score: expansion/boom = positive, contraction/bust = negative.
fn derive_macro_score(backend: &BackendConnection) -> (f64, String) {
    let cycles = db::structural::list_cycles_backend(backend).unwrap_or_default();

    if cycles.is_empty() {
        return (0.0, "No structural cycle data available".to_string());
    }

    let mut score = 0.0;
    for c in &cycles {
        let stage_score = match c.current_stage.to_lowercase().as_str() {
            "expansion" | "boom" | "growth" | "recovery" => 30.0,
            "peak" | "topping" | "late-cycle" => 10.0,
            "contraction" | "bust" | "recession" | "decline" => -30.0,
            "trough" | "bottoming" | "accumulation" => -10.0,
            "transition" | "mixed" => 0.0,
            _ => 0.0,
        };
        score += stage_score;
    }

    let final_score = (score / cycles.len() as f64).clamp(-100.0, 100.0);
    let stages: Vec<String> = cycles.iter()
        .map(|c| format!("{}: {}", c.cycle_name, c.current_stage))
        .collect();

    let summary = if stages.len() <= 3 {
        stages.join(", ")
    } else {
        format!("{} cycles tracked", cycles.len())
    };
    (final_score, summary)
}

/// Classify a scenario's directional impact from its description and impact text.
/// Returns +1.0 (bullish), -1.0 (bearish), or 0.0 (neutral/ambiguous).
fn classify_scenario_direction(scenario: &scenarios::Scenario) -> f64 {
    let text = [
        scenario.description.as_deref().unwrap_or(""),
        scenario.asset_impact.as_deref().unwrap_or(""),
        &scenario.name,
    ]
    .join(" ")
    .to_lowercase();

    let bull_keywords = [
        "bull", "rally", "growth", "expansion", "boom", "recovery",
        "upside", "breakout", "risk-on", "easing",
    ];
    let bear_keywords = [
        "bear", "crash", "recession", "contraction", "crisis", "decline",
        "downside", "breakdown", "risk-off", "tightening", "stagflation",
    ];

    let bull_hits = bull_keywords.iter().filter(|kw| text.contains(**kw)).count();
    let bear_hits = bear_keywords.iter().filter(|kw| text.contains(**kw)).count();

    if bull_hits > bear_hits {
        1.0
    } else if bear_hits > bull_hits {
        -1.0
    } else {
        0.0
    }
}

fn run_populate(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let (low_score, low_summary) = derive_low_score(backend);
    let (medium_score, medium_summary) = derive_medium_score(backend);
    let (high_score, high_summary) = derive_high_score(backend);
    let (macro_score, macro_summary) = derive_macro_score(backend);

    // Upsert all four timeframe scores
    db::mobile_timeframe_scores::upsert_score_backend(
        backend, "low", low_score, Some(&low_summary),
    )?;
    db::mobile_timeframe_scores::upsert_score_backend(
        backend, "medium", medium_score, Some(&medium_summary),
    )?;
    db::mobile_timeframe_scores::upsert_score_backend(
        backend, "high", high_score, Some(&high_summary),
    )?;
    db::mobile_timeframe_scores::upsert_score_backend(
        backend, "macro", macro_score, Some(&macro_summary),
    )?;

    // Collect source stats for reporting
    let regime = db::regime_snapshots::get_current_backend(backend).unwrap_or(None);
    let scenarios_list = scenarios::list_scenarios_backend(backend, Some("active"))
        .unwrap_or_default();
    let trends = db::trends::list_trends_backend(backend, Some("active"), None)
        .unwrap_or_default();
    let cycles = db::structural::list_cycles_backend(backend).unwrap_or_default();
    let convictions = db::convictions::list_current_backend(backend).unwrap_or_default();
    let tech_signals = db::technical_signals::list_signals_backend(backend, None, None, Some(200))
        .unwrap_or_default();

    let result = PopulateResult {
        populated: vec![
            PopulatedScore {
                timeframe: "low".to_string(),
                score: (low_score * 10.0).round() / 10.0,
                summary: low_summary.clone(),
            },
            PopulatedScore {
                timeframe: "medium".to_string(),
                score: (medium_score * 10.0).round() / 10.0,
                summary: medium_summary.clone(),
            },
            PopulatedScore {
                timeframe: "high".to_string(),
                score: (high_score * 10.0).round() / 10.0,
                summary: high_summary.clone(),
            },
            PopulatedScore {
                timeframe: "macro".to_string(),
                score: (macro_score * 10.0).round() / 10.0,
                summary: macro_summary.clone(),
            },
        ],
        sources: PopulateSources {
            regime: regime.as_ref().map(|r| r.regime.clone()),
            regime_confidence: regime.as_ref().and_then(|r| r.confidence),
            active_scenarios: scenarios_list.len(),
            active_trends: trends.len(),
            structural_cycles: cycles.len(),
            convictions: convictions.len(),
            technical_signals: tech_signals.len(),
        },
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Situation Engine — Auto-Populate");
        println!("════════════════════════════════════════════════════════════════");
        println!();
        println!("Sources:");
        if let Some(r) = &regime {
            println!(
                "  Regime: {} (confidence {:.0}%)",
                r.regime,
                r.confidence.unwrap_or(0.0) * 100.0
            );
        } else {
            println!("  Regime: none");
        }
        println!("  Scenarios: {}", scenarios_list.len());
        println!("  Trends: {}", trends.len());
        println!("  Cycles: {}", cycles.len());
        println!("  Convictions: {}", convictions.len());
        println!("  Tech signals: {}", tech_signals.len());
        println!();
        println!("Derived Timeframe Scores:");
        for score in &result.populated {
            println!(
                "  {:>6}: {:+6.1}  {}",
                score.timeframe.to_uppercase(),
                score.score,
                score.summary
            );
        }
        println!();
        println!("✓ Scores written to mobile_timeframe_scores table.");
        println!("  Run `pftui analytics situation --json` to verify.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn setup() -> BackendConnection {
        let conn = Connection::open_in_memory().unwrap();
        db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn populate_empty_db_produces_zero_scores() {
        let backend = setup();
        run_populate(&backend, false).unwrap();

        let scores = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();
        assert_eq!(scores.len(), 4);
        for score in &scores {
            assert_eq!(score.score, 0.0, "empty DB should produce zero for {}", score.timeframe);
        }
    }

    #[test]
    fn populate_with_regime_sets_low_score() {
        let backend = setup();
        db::regime_snapshots::store_regime_backend(
            &backend,
            "risk-on",
            Some(0.8),
            Some("VIX low, DXY weak"),
            Some(14.0),
            Some(100.0),
            Some(4.2),
            Some(70.0),
            Some(2500.0),
            Some(90000.0),
        )
        .unwrap();

        run_populate(&backend, false).unwrap();

        let scores = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();
        let low = scores.iter().find(|s| s.timeframe == "low").unwrap();
        assert!(low.score > 20.0, "risk-on with 0.8 confidence should be positive, got {}", low.score);
        assert!(low.summary.as_deref().unwrap_or("").contains("risk-on"));
    }

    #[test]
    fn populate_with_scenarios_sets_medium_score() {
        let backend = setup();
        // Add a bearish scenario with high probability
        scenarios::add_scenario_backend(
            &backend,
            "Recession",
            70.0,
            Some("Economic contraction accelerating"),
            Some("bearish equities, bearish crypto"),
            None,
            None,
        )
        .unwrap();

        run_populate(&backend, false).unwrap();

        let scores = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();
        let medium = scores.iter().find(|s| s.timeframe == "medium").unwrap();
        assert!(medium.score < 0.0, "bearish scenario should produce negative medium, got {}", medium.score);
    }

    #[test]
    fn populate_with_trends_sets_high_score() {
        let backend = setup();
        db::trends::add_trend_backend(
            &backend,
            "AI capex cycle",
            "high",
            "up",
            "high",
            Some("tech"),
            Some("AI demand increasing"),
            Some("bullish NVDA"),
            Some("earnings"),
        )
        .unwrap();

        run_populate(&backend, false).unwrap();

        let scores = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();
        let high = scores.iter().find(|s| s.timeframe == "high").unwrap();
        assert!(high.score > 0.0, "bullish trend should produce positive high, got {}", high.score);
        assert!(high.summary.as_deref().unwrap_or("").contains("1 trends"));
    }

    #[test]
    fn populate_with_cycles_sets_macro_score() {
        let backend = setup();
        db::structural::set_cycle_backend(
            &backend,
            "US Debt Cycle",
            "contraction",
            Some("2025-01-01"),
            Some("Fiscal tightening, debt ceiling"),
            Some("Rising yields, budget cuts"),
        )
        .unwrap();

        run_populate(&backend, false).unwrap();

        let scores = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();
        let macro_s = scores.iter().find(|s| s.timeframe == "macro").unwrap();
        assert!(macro_s.score < 0.0, "contraction stage should produce negative macro, got {}", macro_s.score);
    }

    #[test]
    fn populate_is_idempotent() {
        let backend = setup();
        db::regime_snapshots::store_regime_backend(
            &backend,
            "risk-off",
            Some(0.7),
            Some("VIX elevated"),
            Some(28.0),
            Some(106.0),
            Some(4.5),
            Some(80.0),
            Some(2600.0),
            Some(78000.0),
        )
        .unwrap();

        run_populate(&backend, false).unwrap();
        let first = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();

        run_populate(&backend, false).unwrap();
        let second = db::mobile_timeframe_scores::list_scores_backend(&backend).unwrap();

        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.timeframe, b.timeframe);
            assert_eq!(a.score, b.score);
        }
    }

    #[test]
    fn populate_json_output_is_valid() {
        let backend = setup();
        // Just verify it doesn't panic with json=true
        run_populate(&backend, true).unwrap();
    }

    #[test]
    fn classify_scenario_direction_works() {
        let bull = scenarios::Scenario {
            id: 1,
            name: "Bull market".to_string(),
            probability: 50.0,
            description: Some("Growth expansion ahead".to_string()),
            asset_impact: Some("bullish equities".to_string()),
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "active".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };
        assert_eq!(classify_scenario_direction(&bull), 1.0);

        let bear = scenarios::Scenario {
            id: 2,
            name: "Recession scenario".to_string(),
            probability: 30.0,
            description: Some("Economic contraction".to_string()),
            asset_impact: Some("bearish risk assets, crash risk".to_string()),
            triggers: None,
            historical_precedent: None,
            status: "active".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            phase: "active".to_string(),
            resolved_at: None,
            resolution_notes: None,
        };
        assert_eq!(classify_scenario_direction(&bear), -1.0);
    }

    #[test]
    fn test_promote_and_list_active() {
        let backend = setup();
        // Add a scenario
        let id = scenarios::add_scenario_backend(
            &backend,
            "Test Conflict",
            45.0,
            Some("A test scenario"),
            None,
            None,
            None,
        )
        .unwrap();

        // Should be in hypothesis phase
        let scenarios_list =
            scenarios::list_scenarios_by_phase_backend(&backend, "hypothesis").unwrap();
        assert_eq!(scenarios_list.len(), 1);
        assert_eq!(scenarios_list[0].phase, "hypothesis");

        // No active situations
        let active = scenarios::list_scenarios_by_phase_backend(&backend, "active").unwrap();
        assert!(active.is_empty());

        // Promote
        scenarios::promote_scenario_backend(&backend, id).unwrap();

        // Now active
        let active = scenarios::list_scenarios_by_phase_backend(&backend, "active").unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].phase, "active");
        assert_eq!(active[0].name, "Test Conflict");
    }

    #[test]
    fn test_branch_crud() {
        let backend = setup();
        let scenario_id = scenarios::add_scenario_backend(
            &backend,
            "Test Scenario",
            50.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, scenario_id).unwrap();

        let b1 = scenarios::add_branch_backend(
            &backend,
            scenario_id,
            "Escalation",
            60.0,
            Some("Full escalation"),
        )
        .unwrap();
        let _b2 = scenarios::add_branch_backend(&backend, scenario_id, "Containment", 40.0, None)
            .unwrap();

        let branches = scenarios::list_branches_backend(&backend, scenario_id).unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].name, "Escalation");
        assert_eq!(branches[1].name, "Containment");

        scenarios::update_branch_backend(&backend, b1, Some(70.0), None, None).unwrap();
        let branches = scenarios::list_branches_backend(&backend, scenario_id).unwrap();
        assert!((branches[0].probability - 70.0).abs() < 0.01);
    }

    #[test]
    fn test_impact_crud() {
        let backend = setup();
        let scenario_id =
            scenarios::add_scenario_backend(&backend, "Oil Shock", 55.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, scenario_id).unwrap();

        let imp1 = scenarios::add_impact_backend(
            &backend,
            scenario_id,
            None,
            "CL=F",
            "bullish",
            "primary",
            Some("Supply route closure"),
            None,
        )
        .unwrap();

        let _imp2 = scenarios::add_impact_backend(
            &backend,
            scenario_id,
            None,
            "GC=F",
            "bullish",
            "secondary",
            Some("Inflation expectations"),
            Some(imp1),
        )
        .unwrap();

        let impacts = scenarios::list_impacts_backend(&backend, scenario_id).unwrap();
        assert_eq!(impacts.len(), 2);
        assert_eq!(impacts[0].symbol, "CL=F");
        assert_eq!(impacts[0].tier, "primary");
        assert!(impacts[0].parent_id.is_none());
        assert_eq!(impacts[1].parent_id, Some(imp1));

        // Exposure query
        let exposure = scenarios::list_impacts_by_symbol_backend(&backend, "GC=F").unwrap();
        assert_eq!(exposure.len(), 1);
        assert_eq!(exposure[0].direction, "bullish");
    }

    #[test]
    fn test_indicator_crud() {
        let backend = setup();
        let scenario_id =
            scenarios::add_scenario_backend(&backend, "Rate Cut", 40.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, scenario_id).unwrap();

        let _ind_id = scenarios::add_indicator_backend(
            &backend,
            scenario_id,
            None,
            None,
            "GC=F",
            "close",
            ">",
            "3000.00",
            "Gold above $3000 confirms inflation fear",
        )
        .unwrap();

        let indicators = scenarios::list_indicators_backend(&backend, scenario_id).unwrap();
        assert_eq!(indicators.len(), 1);
        assert_eq!(indicators[0].status, "watching");
        assert_eq!(indicators[0].operator, ">");
        assert_eq!(indicators[0].threshold, "3000.00");
    }

    #[test]
    fn test_update_log_crud() {
        let backend = setup();
        let scenario_id =
            scenarios::add_scenario_backend(&backend, "Trade War", 30.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, scenario_id).unwrap();

        scenarios::add_update_backend(
            &backend,
            scenario_id,
            None,
            "Tariffs announced",
            Some("25% tariffs on tech imports"),
            "elevated",
            Some("Reuters"),
            Some("low-agent"),
            Some("Retaliation response expected"),
            Some("2026-03-25"),
        )
        .unwrap();

        let updates = scenarios::list_updates_backend(&backend, scenario_id, None).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].headline, "Tariffs announced");
        assert_eq!(updates[0].severity, "elevated");
    }

    #[test]
    fn test_demote_and_resolve() {
        let backend = setup();
        let id =
            scenarios::add_scenario_backend(&backend, "False Alarm", 20.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, id).unwrap();

        // Demote
        scenarios::demote_scenario_backend(&backend, id).unwrap();
        let s = scenarios::get_scenario_by_name_backend(&backend, "False Alarm")
            .unwrap()
            .unwrap();
        assert_eq!(s.phase, "hypothesis");

        // Promote again and resolve
        scenarios::promote_scenario_backend(&backend, id).unwrap();
        scenarios::resolve_scenario_backend(&backend, id, Some("Was nothing")).unwrap();
        let s = scenarios::get_scenario_by_name_backend(&backend, "False Alarm")
            .unwrap()
            .unwrap();
        assert_eq!(s.phase, "resolved");
        assert_eq!(s.resolution_notes.as_deref(), Some("Was nothing"));
    }

    #[test]
    fn test_list_all_watching_indicators() {
        let backend = setup();

        // Create two scenarios with indicators
        let s1 =
            scenarios::add_scenario_backend(&backend, "Scenario A", 50.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, s1).unwrap();
        scenarios::add_indicator_backend(
            &backend, s1, None, None, "BTC-USD", "close", ">", "100000", "BTC 100k",
        )
        .unwrap();

        let s2 =
            scenarios::add_scenario_backend(&backend, "Scenario B", 30.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, s2).unwrap();
        scenarios::add_indicator_backend(
            &backend, s2, None, None, "GC=F", "close", "<", "2000", "Gold collapse",
        )
        .unwrap();

        let watching = scenarios::list_all_watching_indicators_backend(&backend).unwrap();
        assert_eq!(watching.len(), 2);
        assert!(watching.iter().all(|i| i.status == "watching"));
    }

    #[test]
    fn test_update_indicator_evaluation_no_trigger() {
        let backend = setup();
        let s1 =
            scenarios::add_scenario_backend(&backend, "Test Eval", 50.0, None, None, None, None)
                .unwrap();
        scenarios::promote_scenario_backend(&backend, s1).unwrap();
        let ind_id = scenarios::add_indicator_backend(
            &backend, s1, None, None, "BTC-USD", "close", ">", "100000", "BTC 100k",
        )
        .unwrap();

        // Update without triggering
        scenarios::update_indicator_evaluation_backend(&backend, ind_id, "87500.00", false)
            .unwrap();

        let indicators = scenarios::list_indicators_backend(&backend, s1).unwrap();
        assert_eq!(indicators[0].status, "watching");
        assert_eq!(indicators[0].last_value.as_deref(), Some("87500.00"));
        assert!(indicators[0].last_checked.is_some());
        assert!(indicators[0].triggered_at.is_none());
    }

    #[test]
    fn test_update_indicator_evaluation_trigger() {
        let backend = setup();
        let s1 = scenarios::add_scenario_backend(
            &backend,
            "Test Trigger",
            50.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, s1).unwrap();
        let ind_id = scenarios::add_indicator_backend(
            &backend, s1, None, None, "GC=F", "close", ">", "3000", "Gold 3k",
        )
        .unwrap();

        // Trigger the indicator
        scenarios::update_indicator_evaluation_backend(&backend, ind_id, "3150.00", true).unwrap();

        let indicators = scenarios::list_indicators_backend(&backend, s1).unwrap();
        assert_eq!(indicators[0].status, "triggered");
        assert_eq!(indicators[0].last_value.as_deref(), Some("3150.00"));
        assert!(indicators[0].triggered_at.is_some());

        // Triggered indicators should NOT appear in watching list
        let watching = scenarios::list_all_watching_indicators_backend(&backend).unwrap();
        assert!(watching.is_empty());
    }

    #[test]
    fn test_indicator_evaluation_operators() {
        let backend = setup();
        let s1 = scenarios::add_scenario_backend(
            &backend,
            "Operator Test",
            50.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, s1).unwrap();

        // Greater than: 3150 > 3000 = true
        let ind_gt = scenarios::add_indicator_backend(
            &backend, s1, None, None, "GC=F", "close", ">", "3000", "GT test",
        )
        .unwrap();
        scenarios::update_indicator_evaluation_backend(&backend, ind_gt, "3150", true).unwrap();

        // Less than: 85000 < 100000 = true
        let ind_lt = scenarios::add_indicator_backend(
            &backend, s1, None, None, "BTC-USD", "close", "<", "100000", "LT test",
        )
        .unwrap();
        scenarios::update_indicator_evaluation_backend(&backend, ind_lt, "85000", true).unwrap();

        let indicators = scenarios::list_indicators_backend(&backend, s1).unwrap();
        let triggered_count = indicators.iter().filter(|i| i.status == "triggered").count();
        assert_eq!(triggered_count, 2);
    }

    #[test]
    fn test_matrix_empty_when_no_situations() {
        let backend = setup();
        // No scenarios at all — matrix should produce empty results
        let active =
            scenarios::list_scenarios_backend(&backend, Some("active")).unwrap_or_default();
        assert!(active.is_empty());
    }

    #[test]
    fn test_matrix_filters_by_active_phase() {
        let backend = setup();
        // Add a scenario (default phase=hypothesis, status=active)
        let id = scenarios::add_scenario_backend(
            &backend,
            "Hypothesis Only",
            30.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        // status=active scenarios exist, but only those with phase=active are true situations
        let all_active =
            scenarios::list_scenarios_backend(&backend, Some("active")).unwrap_or_default();
        assert_eq!(all_active.len(), 1);
        assert_eq!(all_active[0].phase, "hypothesis");

        // After promoting, phase becomes active
        scenarios::promote_scenario_backend(&backend, id).unwrap();
        let promoted =
            scenarios::list_scenarios_backend(&backend, Some("active")).unwrap_or_default();
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].phase, "active");
    }

    #[test]
    fn test_matrix_shows_branches_and_impacts() {
        let backend = setup();
        let s1 = scenarios::add_scenario_backend(
            &backend,
            "Trade War",
            65.0,
            Some("US-China trade war escalation"),
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, s1).unwrap();

        // Add branches
        scenarios::add_branch_backend(&backend, s1, "Full Decoupling", 40.0, Some("Complete tech decoupling")).unwrap();
        scenarios::add_branch_backend(&backend, s1, "Negotiated Settlement", 60.0, Some("Deal reached")).unwrap();

        // Add impacts
        scenarios::add_impact_backend(&backend, s1, None, "BTC-USD", "bullish", "primary", Some("Safe haven bid"), None).unwrap();
        scenarios::add_impact_backend(&backend, s1, None, "GC=F", "bullish", "primary", Some("Flight to safety"), None).unwrap();

        let s2 = scenarios::add_scenario_backend(
            &backend,
            "Rate Cut Cycle",
            55.0,
            Some("Fed cuts rates aggressively"),
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, s2).unwrap();

        // Add overlapping impact
        scenarios::add_impact_backend(&backend, s2, None, "BTC-USD", "bullish", "secondary", Some("Liquidity expansion"), None).unwrap();
        scenarios::add_impact_backend(&backend, s2, None, "SPY", "bullish", "primary", Some("Lower discount rate"), None).unwrap();

        // Verify we can load all data needed for matrix
        let active = scenarios::list_scenarios_backend(&backend, Some("active")).unwrap();
        assert_eq!(active.len(), 2);

        let branches = scenarios::list_branches_backend(&backend, s1).unwrap();
        assert_eq!(branches.len(), 2);

        let impacts_s1 = scenarios::list_impacts_backend(&backend, s1).unwrap();
        assert_eq!(impacts_s1.len(), 2);

        let impacts_s2 = scenarios::list_impacts_backend(&backend, s2).unwrap();
        assert_eq!(impacts_s2.len(), 2);

        // BTC-USD appears in both situations
        let btc_impacts = scenarios::list_impacts_by_symbol_backend(&backend, "BTC-USD").unwrap();
        assert_eq!(btc_impacts.len(), 2);
    }

    #[test]
    fn test_matrix_indicator_aggregation() {
        let backend = setup();
        let s1 = scenarios::add_scenario_backend(
            &backend,
            "Matrix Indicator Sit",
            50.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        scenarios::promote_scenario_backend(&backend, s1).unwrap();

        // Add 3 indicators: 1 triggered, 2 watching
        let ind1 = scenarios::add_indicator_backend(
            &backend, s1, None, None, "BTC-USD", "close", ">", "100000", "BTC 100k",
        )
        .unwrap();
        scenarios::update_indicator_evaluation_backend(&backend, ind1, "105000", true).unwrap();

        scenarios::add_indicator_backend(
            &backend, s1, None, None, "GC=F", "close", ">", "3000", "Gold 3k",
        )
        .unwrap();
        scenarios::add_indicator_backend(
            &backend, s1, None, None, "DX-Y.NYB", "close", "<", "100", "DXY sub-100",
        )
        .unwrap();

        let indicators = scenarios::list_indicators_backend(&backend, s1).unwrap();
        assert_eq!(indicators.len(), 3);
        let watching = indicators.iter().filter(|i| i.status == "watching").count();
        let triggered = indicators.iter().filter(|i| i.status == "triggered").count();
        assert_eq!(watching, 2);
        assert_eq!(triggered, 1);
        assert_eq!(indicators.iter().find(|i| i.status == "triggered").unwrap().label, "BTC 100k");
    }
}
