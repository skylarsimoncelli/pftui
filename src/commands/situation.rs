use anyhow::{bail, Result};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::scenario_engine;
use crate::db::scenarios;

/// Promote a scenario from hypothesis to active situation.
pub fn promote(backend: &BackendConnection, name: &str, json_output: bool) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", name))?;

    let phase = scenario_engine::get_scenario_phase_backend(backend, scenario.id)?;
    if phase == "active" {
        bail!("Scenario '{}' is already an active situation", name);
    }
    if phase == "resolved" {
        bail!("Scenario '{}' is resolved — use demote first to revert", name);
    }

    let branch_count = scenario_engine::count_branches_backend(backend, scenario.id)?;

    let old_phase = scenario_engine::set_scenario_phase_backend(backend, scenario.id, "active", None)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "promote",
                "scenario": name,
                "old_phase": old_phase,
                "new_phase": "active",
                "branch_count": branch_count,
                "warning": if branch_count == 0 { Some("No branches defined yet — consider adding branches") } else { None },
            }))?
        );
    } else {
        println!("✓ Promoted '{}' to active situation", name);
        if branch_count == 0 {
            println!("  ⚠ No branches defined — consider adding branches with:");
            println!("    pftui analytics situation branch add --situation \"{}\" --branch \"NAME\" --probability N", name);
        }
    }
    Ok(())
}

/// Default situation dashboard (no subcommand) — list active situations with summaries.
pub fn dashboard(backend: &BackendConnection, json_output: bool) -> Result<()> {
    // First run the existing situation snapshot for backward compatibility
    let snapshot = crate::analytics::situation::build_snapshot_backend(backend)?;

    // Also load F53 engine data
    let active_scenarios = scenarios::list_scenarios_backend(backend, Some("active"))?;
    let _hypothesis_scenarios = scenarios::list_scenarios_backend(backend, Some("active"))
        .ok()
        .unwrap_or_default();

    // Build situation summaries
    let mut situation_summaries = Vec::new();
    for scenario in &active_scenarios {
        let phase = scenario_engine::get_scenario_phase_backend(backend, scenario.id)
            .unwrap_or_else(|_| "hypothesis".to_string());
        if phase != "active" {
            continue;
        }
        let branches = scenario_engine::list_branches_backend(backend, scenario.id).unwrap_or_default();
        let indicators = scenario_engine::list_indicators_backend(backend, scenario.id, None).unwrap_or_default();
        let triggered = indicators.iter().filter(|i| i.status == "triggered").count();
        let updates = scenario_engine::list_updates_backend(backend, scenario.id, Some(1)).unwrap_or_default();
        let latest_update = updates.first().map(|u| u.headline.clone());

        situation_summaries.push(json!({
            "name": scenario.name,
            "probability": scenario.probability,
            "phase": phase,
            "branches": branches.len(),
            "indicators_total": indicators.len(),
            "indicators_triggered": triggered,
            "latest_update": latest_update,
        }));
    }

    if json_output {
        let output = json!({
            "legacy_snapshot": snapshot,
            "active_situations": situation_summaries,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Print legacy snapshot
        println!("Situation Room");
        println!("{}", serde_json::to_string_pretty(&snapshot)?);

        if !situation_summaries.is_empty() {
            println!("\n═══ Active Situations ═══");
            for s in &situation_summaries {
                let name = s["name"].as_str().unwrap_or("?");
                let prob = s["probability"].as_f64().unwrap_or(0.0);
                let branches = s["branches"].as_u64().unwrap_or(0);
                let ind_total = s["indicators_total"].as_u64().unwrap_or(0);
                let ind_triggered = s["indicators_triggered"].as_u64().unwrap_or(0);
                println!(
                    "  {} ({:.0}%) — {} branches, {}/{} indicators triggered",
                    name, prob, branches, ind_triggered, ind_total
                );
                if let Some(latest) = s["latest_update"].as_str() {
                    println!("    Latest: {}", latest);
                }
            }
        }

        // Count hypothesis scenarios too
        let all_scenarios = scenarios::list_scenarios_backend(backend, None).unwrap_or_default();
        let hyp_count = all_scenarios.iter()
            .filter(|s| scenario_engine::get_scenario_phase_backend(backend, s.id)
                .unwrap_or_else(|_| "hypothesis".to_string()) == "hypothesis")
            .count();
        if hyp_count > 0 {
            println!("\n  {} hypotheses not yet promoted", hyp_count);
        }
    }
    Ok(())
}

/// List all active situations with branch/indicator summaries.
pub fn list(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let all_scenarios = scenarios::list_scenarios_backend(backend, None)?;

    let mut situations = Vec::new();
    for scenario in &all_scenarios {
        let phase = scenario_engine::get_scenario_phase_backend(backend, scenario.id)
            .unwrap_or_else(|_| "hypothesis".to_string());
        if phase != "active" {
            continue;
        }
        let branches = scenario_engine::list_branches_backend(backend, scenario.id).unwrap_or_default();
        let indicators = scenario_engine::list_indicators_backend(backend, scenario.id, None).unwrap_or_default();
        let triggered = indicators.iter().filter(|i| i.status == "triggered").count();
        let watching = indicators.iter().filter(|i| i.status == "watching").count();
        let fading = indicators.iter().filter(|i| i.status == "fading").count();

        let branch_summaries: Vec<_> = branches.iter().map(|b| json!({
            "name": b.name,
            "probability": b.probability,
            "status": b.status,
        })).collect();

        situations.push(json!({
            "name": scenario.name,
            "probability": scenario.probability,
            "description": scenario.description,
            "branches": branch_summaries,
            "indicators": {
                "total": indicators.len(),
                "triggered": triggered,
                "watching": watching,
                "fading": fading,
            },
        }));
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({ "situations": situations }))?);
    } else {
        if situations.is_empty() {
            println!("No active situations. Promote a scenario with:");
            println!("  pftui journal scenario promote \"SCENARIO_NAME\"");
            return Ok(());
        }
        for s in &situations {
            let name = s["name"].as_str().unwrap_or("?");
            let prob = s["probability"].as_f64().unwrap_or(0.0);
            println!("■ {} ({:.0}%)", name, prob);
            if let Some(desc) = s["description"].as_str() {
                println!("  {}", desc);
            }
            if let Some(branches) = s["branches"].as_array() {
                for b in branches {
                    let bname = b["name"].as_str().unwrap_or("?");
                    let bprob = b["probability"].as_f64().unwrap_or(0.0);
                    let bstatus = b["status"].as_str().unwrap_or("?");
                    println!("  ├── {} ({:.0}%) [{}]", bname, bprob, bstatus);
                }
            }
            let ind = &s["indicators"];
            println!(
                "  └── Indicators: {} total, {} triggered, {} watching, {} fading",
                ind["total"], ind["triggered"], ind["watching"], ind["fading"]
            );
        }
    }
    Ok(())
}

/// View full situation composite.
pub fn view(backend: &BackendConnection, name: &str, json_output: bool) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", name))?;

    let phase = scenario_engine::get_scenario_phase_backend(backend, scenario.id)?;
    let branches = scenario_engine::list_branches_backend(backend, scenario.id)?;
    let impacts = scenario_engine::list_impacts_backend(backend, scenario.id)?;
    let indicators = scenario_engine::list_indicators_backend(backend, scenario.id, None)?;
    let updates = scenario_engine::list_updates_backend(backend, scenario.id, None)?;
    let history = scenarios::get_history_backend(backend, scenario.id, Some(20))?;
    let signals = scenarios::list_signals_backend(backend, scenario.id, None)?;

    let composite = json!({
        "scenario": {
            "id": scenario.id,
            "name": scenario.name,
            "probability": scenario.probability,
            "description": scenario.description,
            "asset_impact": scenario.asset_impact,
            "triggers": scenario.triggers,
            "historical_precedent": scenario.historical_precedent,
            "status": scenario.status,
            "phase": phase,
            "created_at": scenario.created_at,
            "updated_at": scenario.updated_at,
        },
        "branches": branches,
        "impacts": impacts,
        "indicators": indicators,
        "updates": updates,
        "history": history,
        "signals": signals,
    });

    if json_output {
        println!("{}", serde_json::to_string_pretty(&composite)?);
    } else {
        println!("═══ {} ═══", scenario.name);
        println!("Phase: {}  |  Probability: {:.0}%  |  Status: {}", phase, scenario.probability, scenario.status);
        if let Some(desc) = &scenario.description {
            println!("Description: {}", desc);
        }

        if !branches.is_empty() {
            println!("\n── Branches ──");
            for b in &branches {
                println!("  {} ({:.0}%) [{}]", b.name, b.probability, b.status);
                if let Some(d) = &b.description {
                    println!("    {}", d);
                }
            }
        }

        if !impacts.is_empty() {
            println!("\n── Impacts ──");
            for i in &impacts {
                let parent_tag = i.parent_id.map(|p| format!(" (child of #{})", p)).unwrap_or_default();
                println!("  #{} {} → {} [{}]{}", i.id, i.symbol, i.direction, i.tier, parent_tag);
                if let Some(m) = &i.mechanism {
                    println!("    Mechanism: {}", m);
                }
            }
        }

        if !indicators.is_empty() {
            println!("\n── Indicators ──");
            for ind in &indicators {
                let value_str = ind.last_value.as_deref().unwrap_or("—");
                println!("  [{}] {} {} {} {} (last: {})", ind.status, ind.label, ind.symbol, ind.operator, ind.threshold, value_str);
            }
        }

        if !updates.is_empty() {
            println!("\n── Updates ──");
            for u in updates.iter().take(10) {
                let sev_marker = match u.severity.as_str() {
                    "critical" => "🔴",
                    "elevated" => "🟡",
                    "low" => "⚪",
                    _ => "🔵",
                };
                println!("  {} {} — {}", sev_marker, u.created_at, u.headline);
            }
        }

        if !history.is_empty() {
            println!("\n── History (last {}) ──", history.len());
            for h in history.iter().take(5) {
                let driver_str = h.driver.as_deref().unwrap_or("");
                println!("  {:.0}% @ {} — {}", h.probability, h.recorded_at, driver_str);
            }
        }
    }
    Ok(())
}

/// Resolve a situation.
pub fn resolve(backend: &BackendConnection, name: &str, resolution: &str, winning_branch: Option<&str>, json_output: bool) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", name))?;

    // If a winning branch is specified, mark it as resolved
    if let Some(branch_name) = winning_branch {
        if let Some(branch) = scenario_engine::get_branch_by_name_backend(backend, scenario.id, branch_name)? {
            scenario_engine::update_branch_backend(backend, branch.id, None, None, Some("resolved"))?;
        }
    }

    let old_phase = scenario_engine::set_scenario_phase_backend(backend, scenario.id, "resolved", Some(resolution))?;

    // Log a history entry about resolution
    scenarios::update_scenario_probability_backend(
        backend,
        scenario.id,
        scenario.probability,
        Some(&format!("Resolved: {}", resolution)),
    )?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "action": "resolve",
            "scenario": name,
            "old_phase": old_phase,
            "resolution": resolution,
            "winning_branch": winning_branch,
        }))?);
    } else {
        println!("✓ Resolved situation '{}'", name);
        println!("  Resolution: {}", resolution);
        if let Some(b) = winning_branch {
            println!("  Winning branch: {}", b);
        }
    }
    Ok(())
}

/// Demote a situation back to hypothesis.
pub fn demote(backend: &BackendConnection, name: &str, json_output: bool) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", name))?;

    let old_phase = scenario_engine::set_scenario_phase_backend(backend, scenario.id, "hypothesis", None)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "action": "demote",
            "scenario": name,
            "old_phase": old_phase,
            "new_phase": "hypothesis",
        }))?);
    } else {
        println!("✓ Demoted '{}' from {} to hypothesis", name, old_phase);
    }
    Ok(())
}

// ── Branch commands ────────────────────────────────────────────────────

pub fn branch_add(
    backend: &BackendConnection,
    situation: &str,
    branch: &str,
    probability: f64,
    description: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let id = scenario_engine::add_branch_backend(backend, scenario.id, branch, probability, description, None)?;

    // Log to history
    scenarios::update_scenario_probability_backend(
        backend,
        scenario.id,
        scenario.probability,
        Some(&format!("Added branch '{}' ({:.0}%)", branch, probability)),
    )?;

    if json_output {
        let b = scenario_engine::get_branch_by_name_backend(backend, scenario.id, branch)?;
        println!("{}", serde_json::to_string_pretty(&b)?);
    } else {
        println!("✓ Added branch #{} '{}' ({:.0}%) to '{}'", id, branch, probability, situation);
    }
    Ok(())
}

pub fn branch_list(backend: &BackendConnection, situation: &str, json_output: bool) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let branches = scenario_engine::list_branches_backend(backend, scenario.id)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&branches)?);
    } else {
        if branches.is_empty() {
            println!("No branches for '{}'", situation);
            return Ok(());
        }
        for b in &branches {
            println!("  {} ({:.0}%) [{}]", b.name, b.probability, b.status);
            if let Some(d) = &b.description {
                println!("    {}", d);
            }
        }
    }
    Ok(())
}

pub fn branch_update(
    backend: &BackendConnection,
    situation: &str,
    branch_name: &str,
    probability: Option<f64>,
    description: Option<&str>,
    status: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let branch = scenario_engine::get_branch_by_name_backend(backend, scenario.id, branch_name)?
        .ok_or_else(|| anyhow::anyhow!("Branch not found: {}", branch_name))?;

    scenario_engine::update_branch_backend(backend, branch.id, probability, description, status)?;

    // Log to history
    let mut changes = Vec::new();
    if let Some(p) = probability {
        changes.push(format!("probability→{:.0}%", p));
    }
    if let Some(s) = status {
        changes.push(format!("status→{}", s));
    }
    if !changes.is_empty() {
        scenarios::update_scenario_probability_backend(
            backend,
            scenario.id,
            scenario.probability,
            Some(&format!("Branch '{}': {}", branch_name, changes.join(", "))),
        )?;
    }

    if json_output {
        let updated = scenario_engine::get_branch_by_name_backend(backend, scenario.id, branch_name)?;
        println!("{}", serde_json::to_string_pretty(&updated)?);
    } else {
        println!("✓ Updated branch '{}' on '{}'", branch_name, situation);
    }
    Ok(())
}

// ── Impact commands ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn impact_add(
    backend: &BackendConnection,
    situation: &str,
    symbol: &str,
    direction: &str,
    tier: &str,
    branch_name: Option<&str>,
    mechanism: Option<&str>,
    parent_id: Option<i64>,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let branch_id = if let Some(bname) = branch_name {
        let branch = scenario_engine::get_branch_by_name_backend(backend, scenario.id, bname)?
            .ok_or_else(|| anyhow::anyhow!("Branch not found: {}", bname))?;
        Some(branch.id)
    } else {
        None
    };

    let id = scenario_engine::add_impact_backend(
        backend, scenario.id, branch_id, symbol, direction, tier, mechanism, parent_id,
    )?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "id": id,
            "scenario": situation,
            "symbol": symbol,
            "direction": direction,
            "tier": tier,
        }))?);
    } else {
        println!("✓ Added impact #{}: {} → {} [{}] on '{}'", id, symbol, direction, tier, situation);
    }
    Ok(())
}

pub fn impact_list(backend: &BackendConnection, situation: &str, tree: bool, json_output: bool) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let impacts = scenario_engine::list_impacts_backend(backend, scenario.id)?;

    if json_output {
        if tree {
            let tree_json = build_impact_tree(&impacts);
            println!("{}", serde_json::to_string_pretty(&tree_json)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&impacts)?);
        }
    } else {
        if impacts.is_empty() {
            println!("No impacts for '{}'", situation);
            return Ok(());
        }
        if tree {
            print_impact_tree(&impacts, None, 0);
        } else {
            for i in &impacts {
                let parent_tag = i.parent_id.map(|p| format!(" (child of #{})", p)).unwrap_or_default();
                println!("  #{} {} → {} [{}]{}", i.id, i.symbol, i.direction, i.tier, parent_tag);
                if let Some(m) = &i.mechanism {
                    println!("    {}", m);
                }
            }
        }
    }
    Ok(())
}

fn build_impact_tree(impacts: &[scenario_engine::ScenarioImpact]) -> serde_json::Value {
    fn build_children(impacts: &[scenario_engine::ScenarioImpact], parent_id: Option<i64>) -> Vec<serde_json::Value> {
        impacts
            .iter()
            .filter(|i| i.parent_id == parent_id)
            .map(|i| {
                let children = build_children(impacts, Some(i.id));
                let mut node = json!({
                    "id": i.id,
                    "symbol": i.symbol,
                    "direction": i.direction,
                    "tier": i.tier,
                    "mechanism": i.mechanism,
                });
                if !children.is_empty() {
                    node.as_object_mut().unwrap().insert("children".to_string(), json!(children));
                }
                node
            })
            .collect()
    }
    json!(build_children(impacts, None))
}

fn print_impact_tree(impacts: &[scenario_engine::ScenarioImpact], parent_id: Option<i64>, depth: usize) {
    let indent = "  ".repeat(depth);
    let children: Vec<_> = impacts.iter().filter(|i| i.parent_id == parent_id).collect();
    for (idx, imp) in children.iter().enumerate() {
        let connector = if idx == children.len() - 1 { "└──" } else { "├──" };
        println!("{}{}#{} {} → {} [{}]", indent, connector, imp.id, imp.symbol, imp.direction, imp.tier);
        if let Some(m) = &imp.mechanism {
            println!("{}    {}", indent, m);
        }
        print_impact_tree(impacts, Some(imp.id), depth + 1);
    }
}

// ── Indicator commands ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn indicator_add(
    backend: &BackendConnection,
    situation: &str,
    symbol: &str,
    operator: &str,
    threshold: &str,
    label: &str,
    branch_name: Option<&str>,
    impact_id: Option<i64>,
    metric: &str,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let branch_id = if let Some(bname) = branch_name {
        let branch = scenario_engine::get_branch_by_name_backend(backend, scenario.id, bname)?
            .ok_or_else(|| anyhow::anyhow!("Branch not found: {}", bname))?;
        Some(branch.id)
    } else {
        None
    };

    let id = scenario_engine::add_indicator_backend(
        backend, scenario.id, branch_id, impact_id, symbol, metric, operator, threshold, label,
    )?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "id": id,
            "scenario": situation,
            "symbol": symbol,
            "operator": operator,
            "threshold": threshold,
            "label": label,
            "metric": metric,
        }))?);
    } else {
        println!("✓ Added indicator #{}: {} {} {} {} on '{}'", id, label, symbol, operator, threshold, situation);
    }
    Ok(())
}

pub fn indicator_list(
    backend: &BackendConnection,
    situation: &str,
    status_filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let indicators = scenario_engine::list_indicators_backend(backend, scenario.id, status_filter)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&indicators)?);
    } else {
        if indicators.is_empty() {
            println!("No indicators for '{}'{}", situation,
                status_filter.map(|s| format!(" with status '{}'", s)).unwrap_or_default());
            return Ok(());
        }
        for ind in &indicators {
            let value_str = ind.last_value.as_deref().unwrap_or("—");
            let checked_str = ind.last_checked.as_deref().unwrap_or("never");
            let status_marker = match ind.status.as_str() {
                "triggered" => "🔴",
                "fading" => "🟡",
                "watching" => "⚪",
                "expired" => "⚫",
                _ => "?",
            };
            println!("  {} [{}] {} {} {} {} (last: {}, checked: {})",
                status_marker, ind.status, ind.label, ind.symbol, ind.operator, ind.threshold,
                value_str, checked_str);
        }
    }
    Ok(())
}

// ── Update commands ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn update_log(
    backend: &BackendConnection,
    situation: &str,
    headline: &str,
    detail: Option<&str>,
    severity: &str,
    branch_name: Option<&str>,
    source: Option<&str>,
    source_agent: Option<&str>,
    next_decision: Option<&str>,
    next_decision_at: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let branch_id = if let Some(bname) = branch_name {
        let branch = scenario_engine::get_branch_by_name_backend(backend, scenario.id, bname)?
            .ok_or_else(|| anyhow::anyhow!("Branch not found: {}", bname))?;
        Some(branch.id)
    } else {
        None
    };

    let id = scenario_engine::add_update_backend(
        backend, scenario.id, branch_id, headline, detail, severity,
        source, source_agent, next_decision, next_decision_at,
    )?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "id": id,
            "scenario": situation,
            "headline": headline,
            "severity": severity,
        }))?);
    } else {
        let sev_marker = match severity {
            "critical" => "🔴",
            "elevated" => "🟡",
            "low" => "⚪",
            _ => "🔵",
        };
        println!("{} Logged update #{} on '{}': {}", sev_marker, id, situation, headline);
    }
    Ok(())
}

pub fn update_list(
    backend: &BackendConnection,
    situation: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let scenario = scenarios::get_scenario_by_name_backend(backend, situation)?
        .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", situation))?;

    let updates = scenario_engine::list_updates_backend(backend, scenario.id, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&updates)?);
    } else {
        if updates.is_empty() {
            println!("No updates for '{}'", situation);
            return Ok(());
        }
        for u in &updates {
            let sev_marker = match u.severity.as_str() {
                "critical" => "🔴",
                "elevated" => "🟡",
                "low" => "⚪",
                _ => "🔵",
            };
            println!("{} {} — {}", sev_marker, u.created_at, u.headline);
            if let Some(d) = &u.detail {
                println!("    {}", d);
            }
            if let Some(nd) = &u.next_decision {
                let at_str = u.next_decision_at.as_deref().unwrap_or("");
                println!("    Next decision: {} {}", nd, at_str);
            }
        }
    }
    Ok(())
}

// ── Exposure command ───────────────────────────────────────────────────

pub fn exposure(backend: &BackendConnection, symbol: &str, json_output: bool) -> Result<()> {
    let impacts = scenario_engine::list_impacts_for_symbol_backend(backend, symbol)?;

    if impacts.is_empty() {
        if json_output {
            println!("{}", serde_json::to_string_pretty(&json!({
                "symbol": symbol,
                "situations": [],
            }))?);
        } else {
            println!("No situation exposure for {}", symbol);
        }
        return Ok(());
    }

    // Group by scenario
    let mut by_scenario: std::collections::BTreeMap<i64, Vec<&scenario_engine::ScenarioImpact>> = std::collections::BTreeMap::new();
    for imp in &impacts {
        by_scenario.entry(imp.scenario_id).or_default().push(imp);
    }

    let mut entries = Vec::new();
    for (scenario_id, imps) in &by_scenario {
        // Look up scenario name
        let all = scenarios::list_scenarios_backend(backend, None)?;
        let scenario_name = all.iter().find(|s| s.id == *scenario_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| format!("scenario#{}", scenario_id));
        let scenario_prob = all.iter().find(|s| s.id == *scenario_id)
            .map(|s| s.probability)
            .unwrap_or(0.0);

        let impact_details: Vec<_> = imps.iter().map(|i| json!({
            "direction": i.direction,
            "tier": i.tier,
            "mechanism": i.mechanism,
            "branch_id": i.branch_id,
        })).collect();

        entries.push(json!({
            "scenario": scenario_name,
            "probability": scenario_prob,
            "impacts": impact_details,
        }));
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "symbol": symbol,
            "situations": entries,
        }))?);
    } else {
        println!("Exposure for {}:", symbol);
        for e in &entries {
            let name = e["scenario"].as_str().unwrap_or("?");
            let prob = e["probability"].as_f64().unwrap_or(0.0);
            println!("  {} ({:.0}%)", name, prob);
            if let Some(impacts_arr) = e["impacts"].as_array() {
                for i in impacts_arr {
                    let dir = i["direction"].as_str().unwrap_or("?");
                    let tier = i["tier"].as_str().unwrap_or("?");
                    let mech = i["mechanism"].as_str().unwrap_or("");
                    println!("    → {} [{}] {}", dir, tier, mech);
                }
            }
        }
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// INDICATOR EVALUATION ENGINE (Phase 2)
// ══════════════════════════════════════════════════════════════════════

use crate::db::technical_snapshots;
use crate::db::price_cache;

/// Result of evaluating a single indicator.
#[derive(Debug, serde::Serialize)]
pub struct EvaluationResult {
    pub indicator_id: i64,
    pub label: String,
    pub symbol: String,
    pub old_status: String,
    pub new_status: String,
    pub value: String,
    pub changed: bool,
}

/// Evaluate all non-expired indicators against current market data.
pub fn evaluate_indicators(
    backend: &BackendConnection,
    situation_filter: Option<&str>,
) -> Result<Vec<EvaluationResult>> {
    let indicators = if let Some(name) = situation_filter {
        let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
            .ok_or_else(|| anyhow::anyhow!("Scenario not found: {}", name))?;
        scenario_engine::list_indicators_backend(backend, scenario.id, None)?
            .into_iter()
            .filter(|i| i.status != "expired")
            .collect()
    } else {
        scenario_engine::list_all_active_indicators_backend(backend)?
    };

    let mut results = Vec::new();

    for ind in &indicators {
        let eval = evaluate_single(backend, ind)?;
        if let Some(result) = eval {
            // Update the indicator in the database
            if result.changed {
                scenario_engine::update_indicator_status_backend(
                    backend, result.indicator_id, &result.new_status, &result.value,
                )?;
                // Log status change to scenario_history
                let driver = format!(
                    "Indicator '{}': {} → {} ({} = {})",
                    result.label, result.old_status, result.new_status, ind.symbol, result.value
                );
                let current_prob = scenarios::get_scenario_by_name_backend(backend, &get_scenario_name(backend, ind.scenario_id)?)
                    .ok()
                    .flatten()
                    .map(|s| s.probability)
                    .unwrap_or(0.0);
                scenarios::update_scenario_probability_backend(
                    backend,
                    ind.scenario_id,
                    current_prob,
                    Some(&driver),
                ).ok(); // best-effort
            } else {
                // Just update last_value and last_checked
                scenario_engine::update_indicator_checked_backend(
                    backend, result.indicator_id, &result.value,
                )?;
            }
            results.push(result);
        }
    }

    Ok(results)
}

/// Helper to get scenario name from id.
fn get_scenario_name(backend: &BackendConnection, scenario_id: i64) -> Result<String> {
    let all = scenarios::list_scenarios_backend(backend, None)?;
    all.iter()
        .find(|s| s.id == scenario_id)
        .map(|s| s.name.clone())
        .ok_or_else(|| anyhow::anyhow!("Scenario #{} not found", scenario_id))
}

/// Evaluate a single indicator against current data.
fn evaluate_single(
    backend: &BackendConnection,
    indicator: &scenario_engine::ScenarioIndicator,
) -> Result<Option<EvaluationResult>> {
    let symbol = &indicator.symbol;
    let operator = &indicator.operator;
    let threshold_str = &indicator.threshold;

    // Get current value based on metric
    let current_value = get_metric_value(backend, symbol, &indicator.metric)?;
    let current_value = match current_value {
        Some(v) => v,
        None => return Ok(None), // No data available, skip
    };

    let condition_met = evaluate_condition(operator, current_value, threshold_str, backend, symbol)?;

    // Determine new status based on current status and condition
    let old_status = &indicator.status;
    let new_status = match (old_status.as_str(), condition_met) {
        ("watching", true) => "triggered",
        ("watching", false) => "watching",
        ("triggered", true) => "triggered",
        ("triggered", false) => "fading",
        ("fading", true) => "triggered",
        ("fading", false) => "fading",
        (_, _) => old_status.as_str(),
    };

    let changed = new_status != old_status;
    let value_str = format!("{:.4}", current_value);

    Ok(Some(EvaluationResult {
        indicator_id: indicator.id,
        label: indicator.label.clone(),
        symbol: symbol.clone(),
        old_status: old_status.clone(),
        new_status: new_status.to_string(),
        value: value_str,
        changed,
    }))
}

/// Get a metric value for a symbol from the database.
fn get_metric_value(backend: &BackendConnection, symbol: &str, metric: &str) -> Result<Option<f64>> {
    match metric {
        "close" | "price" => {
            // Try price_cache first
            let quote = price_cache::get_cached_price_backend(backend, symbol, "USD")?;
            if let Some(q) = quote {
                use rust_decimal::prelude::ToPrimitive;
                return Ok(Some(q.price.to_f64().unwrap_or(0.0)));
            }
            Ok(None)
        }
        "rsi_14" | "rsi" => {
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            Ok(snap.and_then(|s| s.rsi_14))
        }
        "sma_20" => {
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            Ok(snap.and_then(|s| s.sma_20))
        }
        "sma_50" => {
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            Ok(snap.and_then(|s| s.sma_50))
        }
        "sma_200" => {
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            Ok(snap.and_then(|s| s.sma_200))
        }
        "macd" => {
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            Ok(snap.and_then(|s| s.macd))
        }
        "atr_14" | "atr" => {
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            Ok(snap.and_then(|s| s.atr_14))
        }
        "volume" => {
            // Try to get volume from price_cache OHLCV if available
            // For now, not directly available — skip
            Ok(None)
        }
        _ => {
            // Unknown metric — try price
            let quote = price_cache::get_cached_price_backend(backend, symbol, "USD")?;
            if let Some(q) = quote {
                use rust_decimal::prelude::ToPrimitive;
                return Ok(Some(q.price.to_f64().unwrap_or(0.0)));
            }
            Ok(None)
        }
    }
}

/// Evaluate a condition: does `current_value <operator> threshold` hold?
fn evaluate_condition(
    operator: &str,
    current_value: f64,
    threshold_str: &str,
    backend: &BackendConnection,
    symbol: &str,
) -> Result<bool> {
    match operator {
        ">" | ">=" | "<" | "<=" => {
            let threshold: f64 = threshold_str.parse()
                .map_err(|_| anyhow::anyhow!("Invalid threshold '{}' — must be numeric", threshold_str))?;
            Ok(match operator {
                ">" => current_value > threshold,
                ">=" => current_value >= threshold,
                "<" => current_value < threshold,
                "<=" => current_value <= threshold,
                _ => unreachable!(),
            })
        }
        "above_sma" | "below_sma" => {
            // Threshold is the SMA period (20, 50, 200)
            let period: u32 = threshold_str.parse()
                .map_err(|_| anyhow::anyhow!("Invalid SMA period '{}' — must be 20, 50, or 200", threshold_str))?;
            let snap = technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")?;
            let sma_value = snap.and_then(|s| match period {
                20 => s.sma_20,
                50 => s.sma_50,
                200 => s.sma_200,
                _ => None,
            });
            match sma_value {
                Some(sma) => {
                    let close = get_metric_value(backend, symbol, "close")?.unwrap_or(0.0);
                    Ok(if operator == "above_sma" { close > sma } else { close < sma })
                }
                None => Ok(false), // No SMA data available
            }
        }
        "rsi_above" | "rsi_below" => {
            let threshold: f64 = threshold_str.parse()
                .map_err(|_| anyhow::anyhow!("Invalid RSI threshold '{}' — must be numeric", threshold_str))?;
            let rsi = get_metric_value(backend, symbol, "rsi_14")?;
            match rsi {
                Some(rsi_val) => {
                    Ok(if operator == "rsi_above" { rsi_val > threshold } else { rsi_val < threshold })
                }
                None => Ok(false),
            }
        }
        _ => bail!("Unknown operator: {}", operator),
    }
}

/// CLI command: evaluate indicators.
pub fn indicator_evaluate(
    backend: &BackendConnection,
    situation: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let results = evaluate_indicators(backend, situation)?;

    let changed: Vec<_> = results.iter().filter(|r| r.changed).collect();
    let total = results.len();

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({
            "evaluated": total,
            "changed": changed.len(),
            "results": results,
        }))?);
    } else {
        if total == 0 {
            println!("No indicators to evaluate");
            return Ok(());
        }
        println!("Evaluated {} indicators", total);
        if !changed.is_empty() {
            println!("Changes:");
            for r in &changed {
                let marker = if r.new_status == "triggered" { "🔴" } else { "🟡" };
                println!("  {} {} {} → {} (value: {})", marker, r.label, r.old_status, r.new_status, r.value);
            }
        } else {
            println!("No status changes");
        }
    }
    Ok(())
}

/// Called from data refresh to evaluate indicators and return summary.
pub fn evaluate_on_refresh(backend: &BackendConnection) -> Result<(usize, usize)> {
    let active = scenario_engine::count_active_situations_backend(backend)?;
    if active == 0 {
        return Ok((0, 0));
    }

    let _results = evaluate_indicators(backend, None)?;
    let triggered = scenario_engine::count_triggered_indicators_backend(backend)?;

    Ok((active, triggered))
}
