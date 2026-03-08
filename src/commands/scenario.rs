use crate::db::{self, scenarios};
use anyhow::{bail, Result};
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run(
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    signal_id: Option<i64>,
    probability: Option<f64>,
    description: Option<&str>,
    impact: Option<&str>,
    triggers: Option<&str>,
    precedent: Option<&str>,
    status: Option<&str>,
    driver: Option<&str>,
    evidence: Option<&str>,
    source: Option<&str>,
    scenario_name: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    match action {
        "add" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("scenario name required"))?;
            let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;

            let id = scenarios::add_scenario(
                &conn,
                name,
                prob,
                description,
                impact,
                triggers,
                precedent,
            )?;

            if json_output {
                let scenario = scenarios::get_scenario_by_name(&conn, name)?.unwrap();
                println!("{}", serde_json::to_string_pretty(&scenario)?);
            } else {
                println!("Added scenario #{}: {}", id, name);
            }
        }

        "list" => {
            let scenarios_list = scenarios::list_scenarios(&conn, status)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "scenarios": scenarios_list }))?
                );
            } else if scenarios_list.is_empty() {
                println!("No scenarios found");
            } else {
                let status_label = status.unwrap_or("all");
                println!("Scenarios ({} {}):", scenarios_list.len(), status_label);
                for s in scenarios_list {
                    let desc_preview = s
                        .description
                        .as_ref()
                        .map(|d| {
                            let truncated = if d.len() > 40 {
                                format!("{}...", &d[..37])
                            } else {
                                d.clone()
                            };
                            format!(" — {}", truncated)
                        })
                        .unwrap_or_default();
                    println!(
                        "  {:25} {:5.1}%   {}{}",
                        s.name, s.probability, s.status, desc_preview
                    );
                }
            }
        }

        "update" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("scenario name required"))?;
            let scenario = scenarios::get_scenario_by_name(&conn, name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", name))?;

            // If probability is being updated, use special handler
            if let Some(prob) = probability {
                scenarios::update_scenario_probability(&conn, scenario.id, prob, driver)?;
                if !json_output {
                    println!("Updated probability for '{}' to {:.1}%", name, prob);
                }
            }

            // Update other fields if provided
            if description.is_some() || impact.is_some() || triggers.is_some() || status.is_some()
            {
                scenarios::update_scenario(
                    &conn,
                    scenario.id,
                    description,
                    impact,
                    triggers,
                    status,
                )?;
                if !json_output && probability.is_none() {
                    println!("Updated scenario '{}'", name);
                }
            }

            if json_output {
                let updated = scenarios::get_scenario_by_name(&conn, name)?.unwrap();
                println!("{}", serde_json::to_string_pretty(&updated)?);
            }
        }

        "remove" => {
            let scenario_id = if let Some(i) = id {
                i
            } else if let Some(name) = value {
                scenarios::get_scenario_by_name(&conn, name)?
                    .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", name))?
                    .id
            } else {
                bail!("scenario name or --id required");
            };

            scenarios::remove_scenario(&conn, scenario_id)?;
            if !json_output {
                println!("Removed scenario #{}", scenario_id);
            }
        }

        "signal-add" => {
            let scenario_name =
                scenario_name.ok_or_else(|| anyhow::anyhow!("--scenario required"))?;
            let signal_text = value.ok_or_else(|| anyhow::anyhow!("signal text required"))?;

            let scenario = scenarios::get_scenario_by_name(&conn, scenario_name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", scenario_name))?;

            let signal_id =
                scenarios::add_signal(&conn, scenario.id, signal_text, status, evidence, source)?;

            if json_output {
                let signals = scenarios::list_signals(&conn, scenario.id, None)?;
                let inserted = signals.iter().find(|s| s.id == signal_id).unwrap();
                println!("{}", serde_json::to_string_pretty(inserted)?);
            } else {
                println!("Added signal #{} to scenario '{}'", signal_id, scenario_name);
            }
        }

        "signal-list" => {
            let scenario_name =
                scenario_name.ok_or_else(|| anyhow::anyhow!("--scenario required"))?;

            let scenario = scenarios::get_scenario_by_name(&conn, scenario_name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", scenario_name))?;

            let signals = scenarios::list_signals(&conn, scenario.id, status)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "signals": signals }))?
                );
            } else if signals.is_empty() {
                println!("No signals for scenario '{}'", scenario_name);
            } else {
                println!("Signals for '{}' ({}):", scenario_name, signals.len());
                for sig in signals {
                    let evidence_preview = sig
                        .evidence
                        .as_ref()
                        .map(|e| {
                            let truncated = if e.len() > 30 {
                                format!("{}...", &e[..27])
                            } else {
                                e.clone()
                            };
                            format!(" — {}", truncated)
                        })
                        .unwrap_or_default();
                    let source_text = sig.source.as_ref().map(|s| format!("({})", s)).unwrap_or_default();
                    println!("  [{}] {}  {}{}", sig.status, sig.signal, source_text, evidence_preview);
                }
            }
        }

        "signal-update" => {
            let sig_id = signal_id.ok_or_else(|| anyhow::anyhow!("--signal-id required"))?;

            scenarios::update_signal(&conn, sig_id, status, evidence)?;

            if json_output {
                println!("{}", json!({"updated": sig_id}));
            } else {
                println!("Updated signal #{}", sig_id);
            }
        }

        "signal-remove" => {
            let sig_id = signal_id.ok_or_else(|| anyhow::anyhow!("--signal-id required"))?;

            scenarios::remove_signal(&conn, sig_id)?;

            if json_output {
                println!("{}", json!({"removed": sig_id}));
            } else {
                println!("Removed signal #{}", sig_id);
            }
        }

        "history" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("scenario name required"))?;

            let scenario = scenarios::get_scenario_by_name(&conn, name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", name))?;

            let history = scenarios::get_history(&conn, scenario.id, limit)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "history": history }))?
                );
            } else if history.is_empty() {
                println!("No history for scenario '{}'", name);
            } else {
                println!("Probability history for '{}' ({} entries):", name, history.len());
                for entry in history {
                    let driver_text = entry
                        .driver
                        .as_ref()
                        .map(|d| format!(" — {}", d))
                        .unwrap_or_default();
                    println!(
                        "  {:.1}%  {}{}",
                        entry.probability, entry.recorded_at, driver_text
                    );
                }
            }
        }

        _ => bail!("unknown action '{}'. Valid: add, list, update, remove, signal-add, signal-list, signal-update, signal-remove, history", action),
    }

    Ok(())
}
