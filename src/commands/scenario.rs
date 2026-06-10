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
    _driver: Option<&str>,
    _notes: Option<&str>,
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
            let normalized = scenarios::compute_normalized_set_backend(backend)?;

            if json_output {
                // Augment each scenario with the deviation from its base
                // rate (probability − base_rate) when a base rate is set.
                let annotated: Vec<serde_json::Value> = scenarios_list
                    .iter()
                    .map(|s| {
                        let mut value = serde_json::to_value(s)?;
                        if let (Some(obj), Some(base)) = (value.as_object_mut(), s.base_rate) {
                            obj.insert(
                                "base_rate_deviation".to_string(),
                                json!(((s.probability - base) * 10.0).round() / 10.0),
                            );
                        }
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>>>()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "scenarios": annotated,
                        "normalized_set": {
                            "modeled_sum": normalized.modeled_sum,
                            "residual_probability": normalized.residual_probability,
                            "residual_materialized": normalized.residual_materialized,
                            "residual_scenario_name": scenarios::RESIDUAL_SCENARIO_NAME,
                            "overfill_state": normalized.overfill_state.as_str(),
                        }
                    }))?
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
                    let base_info = s
                        .base_rate
                        .map(|base| {
                            let dev = s.probability - base;
                            let sign = if dev >= 0.0 { "+" } else { "" };
                            format!("  [base {:.1}%, dev {}{:.1}pp]", base, sign, dev)
                        })
                        .unwrap_or_default();
                    println!(
                        "  {:25} {:5.1}%   {}{}{}",
                        s.name, s.probability, s.status, base_info, desc_preview
                    );
                }
                println!(
                    "  modeled sum: {:.1}%  residual ({}): {:.1}%  state: {}",
                    normalized.modeled_sum,
                    scenarios::RESIDUAL_SCENARIO_NAME,
                    normalized.residual_probability,
                    normalized.overfill_state.as_str(),
                );
                if matches!(
                    normalized.overfill_state,
                    scenarios::OverfillState::Overfilled
                ) {
                    println!(
                        "  data-quality warning: modeled scenarios sum to {:.1}% (>100%); rebalance the set (see docs/ANALYTICS-SPEC.md)",
                        normalized.modeled_sum,
                    );
                }
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

        _ => bail!("unknown action '{}'. Valid: add, list, remove, promote, signal-add, signal-list, signal-update, signal-remove, history, timeline (update and set-base-rate have dedicated handlers)", action),
    }

    Ok(())
}

/// Ledger-discipline options for a guarded scenario update (epistemics R4).
#[derive(Debug, Default, Clone, Copy)]
pub struct UpdateGuardOpts<'a> {
    /// Which layer proposes this update. Defaults to `synthesis`.
    pub proposer: Option<&'a str>,
    /// Evidence behind the probability move — REQUIRED for probability updates.
    pub evidence: Option<&'a str>,
    /// Hard data print justifying a daily-delta-cap bypass.
    pub hard_print: Option<&'a str>,
    /// Acknowledge a same-day update by a different proposer.
    pub override_conflict: bool,
}

/// `journal scenario update` / `analytics scenario update` — guarded.
///
/// Probability updates require `--evidence`, are recorded in the
/// `scenario_updates` ledger with proposer + evidence, are capped at
/// 5pp cumulative |Δ| per scenario per day (bypass with `--hard-print`),
/// and same-day updates by a second proposer require `--override-conflict`.
#[allow(clippy::too_many_arguments)]
pub fn update(
    backend: &BackendConnection,
    value: Option<&str>,
    id: Option<i64>,
    probability: Option<f64>,
    description: Option<&str>,
    impact: Option<&str>,
    triggers: Option<&str>,
    status: Option<&str>,
    history_note: Option<&str>,
    guard: UpdateGuardOpts<'_>,
    json_output: bool,
) -> Result<()> {
    let scenario = resolve_scenario_for_update(backend, id, value)?;

    // If probability is being updated, enforce ledger discipline.
    if let Some(prob) = probability {
        let Some(evidence) = guard.evidence.map(str::trim).filter(|e| !e.is_empty()) else {
            bail!(
                "--evidence is required for probability updates: every probability move must cite the data that moved it.\nExample:\n  pftui journal scenario update \"{}\" --probability {:.0} \\\n    --evidence \"CPI 2026-06-10 printed 2.4% vs 2.6% expected\" --proposer <layer>\nNon-probability field updates (--description/--status/...) do not need evidence.",
                scenario.name,
                prob,
            );
        };
        let proposer = guard
            .proposer
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or(scenarios::DEFAULT_SCENARIO_PROPOSER);
        scenarios::guarded_update_scenario_probability_backend(
            backend,
            scenario.id,
            prob,
            history_note,
            proposer,
            evidence,
            guard.hard_print,
            guard.override_conflict,
        )?;
        if !json_output {
            println!(
                "Updated probability for '{}' to {:.1}% (proposer: {})",
                scenario.name, prob, proposer
            );
            if let Some(hp) = guard.hard_print {
                println!("  hard-print bypass logged: {}", hp);
            }
        }
    }

    // Update other fields if provided
    if description.is_some() || impact.is_some() || triggers.is_some() || status.is_some() {
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
        let updated = scenarios::get_scenario_by_name_backend(backend, &scenario.name)?
            .ok_or_else(|| anyhow::anyhow!("scenario '{}' vanished mid-update", scenario.name))?;
        println!("{}", serde_json::to_string_pretty(&updated)?);
    }

    Ok(())
}

/// `journal scenario set-base-rate` — record the reference-class base rate a
/// scenario's probability should be anchored against.
pub fn set_base_rate(
    backend: &BackendConnection,
    name: &str,
    rate: f64,
    reference: &str,
    json_output: bool,
) -> Result<()> {
    let scenario = resolve_scenario_for_update(backend, None, Some(name))?;
    scenarios::set_base_rate_backend(backend, scenario.id, rate, reference)?;

    if json_output {
        let updated = scenarios::get_scenario_by_name_backend(backend, &scenario.name)?
            .ok_or_else(|| anyhow::anyhow!("scenario '{}' not found after update", scenario.name))?;
        println!("{}", serde_json::to_string_pretty(&updated)?);
    } else {
        let deviation = scenario.probability - rate;
        let sign = if deviation >= 0.0 { "+" } else { "" };
        println!(
            "Set base rate for '{}': {:.1}% ({})",
            scenario.name, rate, reference
        );
        println!(
            "  current probability {:.1}% deviates {}{:.1}pp from base",
            scenario.probability, sign, deviation
        );
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

    fn guard(
        proposer: Option<&'static str>,
        evidence: Option<&'static str>,
        hard_print: Option<&'static str>,
        override_conflict: bool,
    ) -> UpdateGuardOpts<'static> {
        UpdateGuardOpts {
            proposer,
            evidence,
            hard_print,
            override_conflict,
        }
    }

    fn add_test_scenario(backend: &BackendConnection, name: &str, prob: f64) {
        scenarios::add_scenario_backend(backend, name, prob, None, None, None, None).unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn try_update(
        backend: &BackendConnection,
        name: &str,
        prob: f64,
        opts: UpdateGuardOpts<'_>,
    ) -> Result<()> {
        update(
            backend,
            Some(name),
            None,
            Some(prob),
            None,
            None,
            None,
            None,
            None,
            opts,
            true,
        )
    }

    #[test]
    fn probability_update_requires_evidence() {
        let backend = test_backend();
        add_test_scenario(&backend, "Inflation Resurgence", 32.0);

        let err = try_update(
            &backend,
            "Inflation Resurgence",
            30.0,
            guard(None, None, None, false),
        )
        .unwrap_err();
        assert!(err.to_string().contains("--evidence is required"));

        // With evidence the same move succeeds.
        try_update(
            &backend,
            "Inflation Resurgence",
            30.0,
            guard(Some("analyst-medium"), Some("CPI printed soft"), None, false),
        )
        .unwrap();
    }

    #[test]
    fn non_probability_update_does_not_need_evidence() {
        let backend = test_backend();
        add_test_scenario(&backend, "Hard Landing", 20.0);
        update(
            &backend,
            Some("Hard Landing"),
            None,
            None,
            Some("updated description"),
            None,
            None,
            None,
            None,
            guard(None, None, None, false),
            true,
        )
        .unwrap();
    }

    #[test]
    fn daily_delta_cap_rejects_six_pp() {
        let backend = test_backend();
        add_test_scenario(&backend, "Inflation Resurgence", 32.0);

        let err = try_update(
            &backend,
            "Inflation Resurgence",
            26.0, // 6pp in one move
            guard(Some("synthesis"), Some("vibes"), None, false),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("daily delta cap"), "got: {msg}");
        assert!(msg.contains("--hard-print"), "got: {msg}");
    }

    #[test]
    fn daily_delta_cap_accumulates_across_updates() {
        let backend = test_backend();
        add_test_scenario(&backend, "Inflation Resurgence", 32.0);

        // 3pp — fine.
        try_update(
            &backend,
            "Inflation Resurgence",
            29.0,
            guard(Some("synthesis"), Some("breakevens fading"), None, false),
        )
        .unwrap();
        // Another 3pp by the same proposer → cumulative 6pp → rejected,
        // and the error shows the day's ledger.
        let err = try_update(
            &backend,
            "Inflation Resurgence",
            26.0,
            guard(Some("synthesis"), Some("more fading"), None, false),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("daily delta cap"), "got: {msg}");
        assert!(msg.contains("32.0% → 29.0%"), "ledger missing: {msg}");
    }

    #[test]
    fn hard_print_bypasses_daily_cap_and_is_logged() {
        let backend = test_backend();
        add_test_scenario(&backend, "Inflation Resurgence", 32.0);

        try_update(
            &backend,
            "Inflation Resurgence",
            40.0, // 8pp — over the cap
            guard(
                Some("analyst-medium"),
                Some("CPI 3.4% vs 2.8% expected"),
                Some("CPI 2026-06-10 print"),
                false,
            ),
        )
        .unwrap();
        let updated = scenarios::get_scenario_by_name_backend(&backend, "Inflation Resurgence")
            .unwrap()
            .unwrap();
        assert!((updated.probability - 40.0).abs() < 1e-9);
    }

    #[test]
    fn same_day_conflict_requires_override() {
        let backend = test_backend();
        add_test_scenario(&backend, "Inflation Resurgence", 32.0);

        try_update(
            &backend,
            "Inflation Resurgence",
            30.0,
            guard(Some("analyst-medium"), Some("soft CPI"), None, false),
        )
        .unwrap();

        // A different proposer the same day is rejected...
        let err = try_update(
            &backend,
            "Inflation Resurgence",
            28.0,
            guard(Some("analyst-macro"), Some("long-cycle read"), None, false),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("same-day conflict"), "got: {msg}");
        assert!(msg.contains("analyst-medium"), "got: {msg}");

        // ...unless --override-conflict is passed.
        try_update(
            &backend,
            "Inflation Resurgence",
            28.0,
            guard(Some("analyst-macro"), Some("long-cycle read"), None, true),
        )
        .unwrap();
        let updated = scenarios::get_scenario_by_name_backend(&backend, "Inflation Resurgence")
            .unwrap()
            .unwrap();
        assert!((updated.probability - 28.0).abs() < 1e-9);
    }

    #[test]
    fn set_base_rate_roundtrips_and_validates() {
        let backend = test_backend();
        add_test_scenario(&backend, "US Equities Up Year", 75.0);

        set_base_rate(
            &backend,
            "US Equities Up Year",
            70.0,
            "US equities up-years frequency 1950-2025 ~70%",
            true,
        )
        .unwrap();

        let s = scenarios::get_scenario_by_name_backend(&backend, "US Equities Up Year")
            .unwrap()
            .unwrap();
        assert_eq!(s.base_rate, Some(70.0));
        assert_eq!(
            s.base_rate_reference.as_deref(),
            Some("US equities up-years frequency 1950-2025 ~70%")
        );

        // Out-of-range rate is rejected.
        let err =
            set_base_rate(&backend, "US Equities Up Year", 130.0, "nonsense", true).unwrap_err();
        assert!(err.to_string().contains("0..=100"));
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
