use anyhow::{bail, Result};
use serde::Serialize;

use crate::cli::SituationCommand;
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
        println!("{}", serde_json::to_string_pretty(&entries)?);
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
}
