use anyhow::{bail, Result};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::prediction_contracts;
use crate::db::scenario_contract_mappings;
use crate::db::scenarios;

/// Run `data predictions map` — link a contract to a scenario, or list existing mappings.
pub fn run_map(
    backend: &BackendConnection,
    scenario: Option<&str>,
    search: Option<&str>,
    contract_id: Option<&str>,
    list: bool,
    json: bool,
) -> Result<()> {
    if list {
        return list_mappings(backend, json);
    }

    // Must provide a scenario name for creating a mapping
    let scenario_name = match scenario {
        Some(name) => name,
        None => bail!("--scenario is required. Use --list to see existing mappings."),
    };

    // Must provide either --search or --contract
    if search.is_none() && contract_id.is_none() {
        bail!("Provide --search to find a contract by keyword, or --contract with a specific contract_id.");
    }

    // Resolve scenario
    let scenario_row = scenarios::get_scenario_by_name_backend(backend, scenario_name)?;
    let scenario_row = match scenario_row {
        Some(s) => s,
        None => bail!(
            "Scenario '{}' not found. Use `analytics scenario list` to see active scenarios.",
            scenario_name
        ),
    };

    // Resolve contract
    let resolved_contract_id = if let Some(cid) = contract_id {
        cid.to_string()
    } else {
        // Search for contracts matching the query
        let contracts =
            prediction_contracts::get_contracts_backend(backend, None, search, 10)?;
        if contracts.is_empty() {
            bail!(
                "No contracts found matching '{}'. Run `pftui data refresh` to fetch latest contracts, or try a different search term.",
                search.unwrap_or("")
            );
        }
        if contracts.len() == 1 {
            contracts[0].contract_id.clone()
        } else {
            // Show candidates and ask user to pick with --contract
            if json {
                let candidates: Vec<_> = contracts
                    .iter()
                    .map(|c| {
                        json!({
                            "contract_id": c.contract_id,
                            "question": c.question,
                            "probability_pct": (c.last_price * 100.0),
                            "category": c.category,
                            "volume_24h": c.volume_24h,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "multiple_matches",
                        "message": "Multiple contracts match. Provide --contract with a specific contract_id.",
                        "candidates": candidates,
                    }))?
                );
            } else {
                println!(
                    "Multiple contracts match '{}'. Pick one with --contract:\n",
                    search.unwrap_or("")
                );
                for c in &contracts {
                    println!(
                        "  {:<20}  {:>5.1}%  {}",
                        &c.contract_id[..c.contract_id.len().min(20)],
                        c.last_price * 100.0,
                        truncate_question(&c.question, 60),
                    );
                }
                println!(
                    "\nExample: pftui data predictions map --scenario \"{}\" --contract \"{}\"",
                    scenario_name, contracts[0].contract_id,
                );
            }
            return Ok(());
        }
    };

    // Create the mapping
    scenario_contract_mappings::add_mapping_backend(
        backend,
        scenario_row.id,
        &resolved_contract_id,
    )?;

    // Get contract info for confirmation
    let contracts = prediction_contracts::get_contracts_backend(
        backend,
        None,
        Some(&resolved_contract_id),
        1,
    )
    .unwrap_or_default();

    // Try exact contract_id match from the full list if search didn't find it
    let contract_info = if contracts.is_empty() {
        prediction_contracts::get_contracts_backend(backend, None, None, 500)
            .unwrap_or_default()
            .into_iter()
            .find(|c| c.contract_id == resolved_contract_id)
    } else {
        contracts.into_iter().next()
    };

    if json {
        let output = json!({
            "status": "mapped",
            "scenario_id": scenario_row.id,
            "scenario_name": scenario_row.name,
            "scenario_probability": scenario_row.probability,
            "contract_id": resolved_contract_id,
            "contract_question": contract_info.as_ref().map(|c| c.question.as_str()).unwrap_or("unknown"),
            "contract_probability": contract_info.as_ref().map(|c| c.last_price).unwrap_or(0.0),
            "contract_probability_pct": contract_info.as_ref().map(|c| c.last_price * 100.0).unwrap_or(0.0),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("✓ Mapped scenario → contract:");
        println!(
            "  Scenario: {} ({:.1}%)",
            scenario_row.name,
            scenario_row.probability * 100.0
        );
        if let Some(c) = &contract_info {
            println!(
                "  Contract: {} ({:.1}%)",
                truncate_question(&c.question, 60),
                c.last_price * 100.0,
            );
            let divergence = (scenario_row.probability - c.last_price) * 100.0;
            let sign = if divergence >= 0.0 { "+" } else { "" };
            println!("  Divergence: {}pp{:.1}", sign, divergence);
        } else {
            println!("  Contract: {} (probability unavailable)", resolved_contract_id);
        }
        println!("\nOn next `pftui data refresh`, the contract probability will auto-log to scenario history.");
    }

    Ok(())
}

/// Run `data predictions unmap` — remove a scenario-contract mapping.
pub fn run_unmap(
    backend: &BackendConnection,
    scenario_name: &str,
    contract_id: Option<&str>,
    json: bool,
) -> Result<()> {
    let scenario_row = scenarios::get_scenario_by_name_backend(backend, scenario_name)?;
    let scenario_row = match scenario_row {
        Some(s) => s,
        None => bail!(
            "Scenario '{}' not found. Use `analytics scenario list` to see scenarios.",
            scenario_name
        ),
    };

    if let Some(cid) = contract_id {
        // Remove specific mapping
        let removed =
            scenario_contract_mappings::remove_mapping_backend(backend, scenario_row.id, cid)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": if removed { "removed" } else { "not_found" },
                    "scenario": scenario_name,
                    "contract_id": cid,
                }))?
            );
        } else if removed {
            println!(
                "✓ Removed mapping: {} ↛ {}",
                scenario_name, cid
            );
        } else {
            println!("No mapping found for scenario '{}' → contract '{}'.", scenario_name, cid);
        }
    } else {
        // Remove all mappings for scenario
        let count = scenario_contract_mappings::remove_all_for_scenario_backend(
            backend,
            scenario_row.id,
        )?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "removed_all",
                    "scenario": scenario_name,
                    "count": count,
                }))?
            );
        } else if count > 0 {
            println!(
                "✓ Removed {} mapping(s) for scenario '{}'.",
                count, scenario_name
            );
        } else {
            println!("No mappings found for scenario '{}'.", scenario_name);
        }
    }

    Ok(())
}

/// List all scenario-contract mappings with enriched details.
fn list_mappings(backend: &BackendConnection, json: bool) -> Result<()> {
    let mappings = scenario_contract_mappings::list_enriched_backend(backend)?;

    if mappings.is_empty() {
        if json {
            println!("[]");
        } else {
            println!(
                "No scenario-contract mappings. Use `data predictions map` to create one."
            );
        }
        return Ok(());
    }

    if json {
        let output: Vec<_> = mappings
            .iter()
            .map(|m| {
                json!({
                    "mapping_id": m.mapping_id,
                    "scenario_id": m.scenario_id,
                    "scenario_name": m.scenario_name,
                    "scenario_probability": m.scenario_probability,
                    "scenario_probability_pct": m.scenario_probability * 100.0,
                    "contract_id": m.contract_id,
                    "contract_question": m.contract_question,
                    "contract_probability": m.contract_probability,
                    "contract_probability_pct": m.contract_probability * 100.0,
                    "contract_category": m.contract_category,
                    "divergence_pp": m.divergence_pp,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Scenario → Contract Mappings\n");
        let max_scenario = 25;
        let max_question = 45;

        println!(
            "{:<sw$}  {:>8}  {:<qw$}  {:>8}  {:>8}",
            "Scenario",
            "Scn%",
            "Contract",
            "Mkt%",
            "Δpp",
            sw = max_scenario,
            qw = max_question,
        );
        println!("{}", "─".repeat(max_scenario + max_question + 8 + 8 + 8 + 8));

        for m in &mappings {
            let scenario = truncate_question(&m.scenario_name, max_scenario);
            let question = truncate_question(&m.contract_question, max_question);
            let sign = if m.divergence_pp >= 0.0 { "+" } else { "" };

            println!(
                "{:<sw$}  {:>7.1}%  {:<qw$}  {:>7.1}%  {:>7}",
                scenario,
                m.scenario_probability * 100.0,
                question,
                m.contract_probability * 100.0,
                format!("{}pp{:.1}", sign, m.divergence_pp),
                sw = max_scenario,
                qw = max_question,
            );
        }
    }

    Ok(())
}

fn truncate_question(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
