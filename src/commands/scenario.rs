use crate::db::backend::BackendConnection;
use crate::db::scenarios;
use anyhow::{bail, Result};
use serde_json::json;

fn resolve_scenario_for_update(
    backend: &BackendConnection,
    id: Option<i64>,
    name: Option<&str>,
) -> Result<scenarios::Scenario> {
    if let Some(id) = id {
        let scenarios_list = scenarios::list_scenarios_backend(backend, None)?;
        return scenarios_list
            .into_iter()
            .find(|scenario| scenario.id == id)
            .ok_or_else(|| anyhow::anyhow!("scenario id {} not found", id));
    }

    let name = name.ok_or_else(|| anyhow::anyhow!("scenario name or --id required"))?;
    if let Some(scenario) = scenarios::get_scenario_by_name_backend(backend, name)? {
        return Ok(scenario);
    }

    let needle = name.trim().to_lowercase();
    let scenarios_list = scenarios::list_scenarios_backend(backend, None)?;

    let exact_ci: Vec<_> = scenarios_list
        .iter()
        .filter(|scenario| scenario.name.to_lowercase() == needle)
        .cloned()
        .collect();
    if let [scenario] = exact_ci.as_slice() {
        return Ok(scenario.clone());
    }
    if exact_ci.len() > 1 {
        let matches = exact_ci
            .iter()
            .map(|scenario| format!("#{} {}", scenario.id, scenario.name))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("multiple scenarios match '{}': {}", name, matches);
    }

    let fuzzy: Vec<_> = scenarios_list
        .into_iter()
        .filter(|scenario| scenario.name.to_lowercase().contains(&needle))
        .collect();
    if let [scenario] = fuzzy.as_slice() {
        return Ok(scenario.clone());
    }
    if !fuzzy.is_empty() {
        let matches = fuzzy
            .iter()
            .map(|scenario| format!("#{} {}", scenario.id, scenario.name))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("scenario '{}' not found exactly. candidates: {}", name, matches);
    }

    bail!("scenario '{}' not found", name)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
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
    notes: Option<&str>,
    evidence: Option<&str>,
    source: Option<&str>,
    scenario_name: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("scenario name required"))?;
            let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;

            let id = scenarios::add_scenario_backend(
                backend,
                name,
                prob,
                description,
                impact,
                triggers,
                precedent,
            )?;

            if json_output {
                let scenario = scenarios::get_scenario_by_name_backend(backend, name)?.unwrap();
                println!("{}", serde_json::to_string_pretty(&scenario)?);
            } else {
                println!("Added scenario #{}: {}", id, name);
            }
        }

        "list" => {
            let scenarios_list = scenarios::list_scenarios_backend(backend, status)?;

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
            let scenario = resolve_scenario_for_update(backend, id, value)?;
            let history_note = driver.or(notes);

            // If probability is being updated, use special handler
            if let Some(prob) = probability {
                scenarios::update_scenario_probability_backend(backend, scenario.id, prob, history_note)?;
                if !json_output {
                    println!("Updated probability for '{}' to {:.1}%", scenario.name, prob);
                }
            }

            // Update other fields if provided
            if description.is_some() || impact.is_some() || triggers.is_some() || status.is_some()
            {
                scenarios::update_scenario_backend(
                    backend,
                    scenario.id,
                    description,
                    impact,
                    triggers,
                    status,
                )?;
                if !json_output && probability.is_none() {
                    println!("Updated scenario '{}'", scenario.name);
                }
            }

            if json_output {
                let updated =
                    scenarios::get_scenario_by_name_backend(backend, &scenario.name)?.unwrap();
                println!("{}", serde_json::to_string_pretty(&updated)?);
            }
        }

        "remove" => {
            let scenario_id = if let Some(i) = id {
                i
            } else if let Some(name) = value {
                scenarios::get_scenario_by_name_backend(backend, name)?
                    .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", name))?
                    .id
            } else {
                bail!("scenario name or --id required");
            };

            scenarios::remove_scenario_backend(backend, scenario_id)?;
            if !json_output {
                println!("Removed scenario #{}", scenario_id);
            }
        }

        "signal-add" => {
            let scenario_name =
                scenario_name.ok_or_else(|| anyhow::anyhow!("--scenario required"))?;
            let signal_text = value.ok_or_else(|| anyhow::anyhow!("signal text required"))?;

            let scenario = scenarios::get_scenario_by_name_backend(backend, scenario_name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", scenario_name))?;

            let signal_id =
                scenarios::add_signal_backend(backend, scenario.id, signal_text, status, evidence, source)?;

            if json_output {
                let signals = scenarios::list_signals_backend(backend, scenario.id, None)?;
                let inserted = signals.iter().find(|s| s.id == signal_id).unwrap();
                println!("{}", serde_json::to_string_pretty(inserted)?);
            } else {
                println!("Added signal #{} to scenario '{}'", signal_id, scenario_name);
            }
        }

        "signal-list" => {
            let scenario_name =
                scenario_name.ok_or_else(|| anyhow::anyhow!("--scenario required"))?;

            let scenario = scenarios::get_scenario_by_name_backend(backend, scenario_name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", scenario_name))?;

            let signals = scenarios::list_signals_backend(backend, scenario.id, status)?;

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

            scenarios::update_signal_backend(backend, sig_id, status, evidence)?;

            if json_output {
                println!("{}", json!({"updated": sig_id}));
            } else {
                println!("Updated signal #{}", sig_id);
            }
        }

        "signal-remove" => {
            let sig_id = signal_id.ok_or_else(|| anyhow::anyhow!("--signal-id required"))?;

            scenarios::remove_signal_backend(backend, sig_id)?;

            if json_output {
                println!("{}", json!({"removed": sig_id}));
            } else {
                println!("Removed signal #{}", sig_id);
            }
        }

        "history" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("scenario name required"))?;

            let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", name))?;

            let history = scenarios::get_history_backend(backend, scenario.id, limit)?;

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

        "promote" => {
            let name = value.ok_or_else(|| anyhow::anyhow!("scenario name required"))?;
            let scenario = scenarios::get_scenario_by_name_backend(backend, name)?
                .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found", name))?;

            if scenario.phase == "active" {
                if json_output {
                    println!("{}", json!({"error": "already active", "scenario": name}));
                } else {
                    println!("Scenario '{}' is already an active situation.", name);
                }
                return Ok(());
            }
            if scenario.phase == "resolved" {
                bail!("Cannot promote resolved scenario '{}'. Create a new one.", name);
            }

            scenarios::promote_scenario_backend(backend, scenario.id)?;

            if json_output {
                let updated = scenarios::get_scenario_by_name_backend(backend, name)?.unwrap();
                println!("{}", serde_json::to_string_pretty(&updated)?);
            } else {
                println!("Promoted '{}' to active situation.", name);
                println!("Manage with: pftui analytics situation view --situation \"{}\"", name);
            }
        }

        "timeline" => {
            let timelines = scenarios::get_all_timelines_backend(backend, limit.map(|l| l as u32))?;

            if json_output {
                // Compute period bounds
                let mut min_date: Option<String> = None;
                let mut max_date: Option<String> = None;
                for t in &timelines {
                    for pt in &t.data_points {
                        if min_date.as_ref().is_none_or(|d| pt.date < *d) {
                            min_date = Some(pt.date.clone());
                        }
                        if max_date.as_ref().is_none_or(|d| pt.date > *d) {
                            max_date = Some(pt.date.clone());
                        }
                    }
                }
                let period = json!({
                    "from": min_date,
                    "to": max_date,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "timeline": {
                            "scenarios": timelines,
                            "period": period,
                        }
                    }))?
                );
            } else if timelines.is_empty() {
                println!("No active scenarios with history.");
            } else {
                println!("Scenario Probability Timeline");
                println!("{}", "=".repeat(60));
                for t in &timelines {
                    let change_str = match t.change {
                        Some(c) if c > 0.0 => format!(" (+{:.1}pp)", c),
                        Some(c) if c < 0.0 => format!(" ({:.1}pp)", c),
                        _ => String::new(),
                    };
                    println!(
                        "\n  {} — current: {:.1}%{}",
                        t.name, t.current_probability, change_str
                    );
                    if t.data_points.is_empty() {
                        println!("    (no history)");
                    } else {
                        for pt in &t.data_points {
                            println!("    {}  {:.1}%", pt.date, pt.probability);
                        }
                    }
                }
            }
        }

        _ => bail!("unknown action '{}'. Valid: add, list, update, remove, promote, signal-add, signal-list, signal-update, signal-remove, history, timeline", action),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        }
    }

    #[test]
    fn resolve_scenario_update_accepts_case_insensitive_name() {
        let backend = test_backend();
        scenarios::add_scenario_backend(
            &backend,
            "Iran-US War Escalation",
            45.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let scenario =
            resolve_scenario_for_update(&backend, None, Some("iran-us war escalation")).unwrap();
        assert_eq!(scenario.name, "Iran-US War Escalation");
    }

    #[test]
    fn resolve_scenario_update_accepts_unique_partial_name() {
        let backend = test_backend();
        scenarios::add_scenario_backend(
            &backend,
            "Iran-US War Escalation",
            45.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let scenario = resolve_scenario_for_update(&backend, None, Some("War Escal")).unwrap();
        assert_eq!(scenario.name, "Iran-US War Escalation");
    }

    #[test]
    fn resolve_scenario_update_returns_candidates_for_ambiguous_partial() {
        let backend = test_backend();
        scenarios::add_scenario_backend(&backend, "US Recession 2026", 40.0, None, None, None, None)
            .unwrap();
        scenarios::add_scenario_backend(&backend, "EU Recession 2026", 35.0, None, None, None, None)
            .unwrap();

        let err = resolve_scenario_for_update(&backend, None, Some("Recession")).unwrap_err();
        assert!(err.to_string().contains("candidates"));
        assert!(err.to_string().contains("US Recession 2026"));
    }

    #[test]
    fn resolve_scenario_update_accepts_id() {
        let backend = test_backend();
        let id = scenarios::add_scenario_backend(
            &backend,
            "Hard Landing",
            55.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let scenario = resolve_scenario_for_update(&backend, Some(id), None).unwrap();
        assert_eq!(scenario.id, id);
    }
}
