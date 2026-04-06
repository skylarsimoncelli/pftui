use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::db::backend::BackendConnection;
use crate::db::prediction_contracts;
use crate::db::scenario_contract_mappings;
use crate::db::scenarios;

#[derive(Debug, Serialize)]
struct MappingSuggestionReport {
    scenarios_scanned: usize,
    unmapped_contracts_scanned: usize,
    suggestions: Vec<ScenarioMappingSuggestion>,
}

#[derive(Debug, Serialize)]
struct ScenarioMappingSuggestion {
    scenario_id: i64,
    scenario_name: String,
    scenario_probability_pct: f64,
    keywords: Vec<String>,
    candidates: Vec<ContractMappingCandidate>,
}

#[derive(Debug, Serialize)]
struct ContractMappingCandidate {
    rank: usize,
    contract_id: String,
    question: String,
    event_title: String,
    category: String,
    probability_pct: f64,
    volume_24h: f64,
    liquidity: f64,
    relevance_score: u32,
    matched_keywords: Vec<String>,
    map_command: String,
}

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

pub fn run_suggest_mappings(
    backend: &BackendConnection,
    scenario_name: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let report = build_suggestion_report(backend, scenario_name, limit)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_suggestions(&report);
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

fn build_suggestion_report(
    backend: &BackendConnection,
    scenario_name: Option<&str>,
    limit: usize,
) -> Result<MappingSuggestionReport> {
    let scenarios = scenarios::list_scenarios_backend(backend, Some("active"))?;
    let scenarios: Vec<_> = if let Some(name) = scenario_name {
        scenarios
            .into_iter()
            .filter(|scenario| scenario.name.eq_ignore_ascii_case(name))
            .collect()
    } else {
        scenarios
    };

    if scenario_name.is_some() && scenarios.is_empty() {
        bail!("Scenario '{}' not found among active scenarios.", scenario_name.unwrap_or(""));
    }

    let mapped_contract_ids: HashSet<String> = scenario_contract_mappings::list_enriched_backend(backend)?
        .into_iter()
        .map(|mapping| mapping.contract_id)
        .collect();
    let contracts = prediction_contracts::get_contracts_backend(backend, None, None, 5000)?;
    let unmapped_contracts: Vec<_> = contracts
        .into_iter()
        .filter(|contract| !mapped_contract_ids.contains(&contract.contract_id))
        .collect();

    let suggestions = scenarios
        .iter()
        .filter_map(|scenario| build_scenario_suggestion(scenario, &unmapped_contracts, limit))
        .collect();

    Ok(MappingSuggestionReport {
        scenarios_scanned: scenarios.len(),
        unmapped_contracts_scanned: unmapped_contracts.len(),
        suggestions,
    })
}

fn build_scenario_suggestion(
    scenario: &scenarios::Scenario,
    contracts: &[prediction_contracts::PredictionContract],
    limit: usize,
) -> Option<ScenarioMappingSuggestion> {
    let keywords = scenario_keywords(scenario);
    if keywords.is_empty() {
        return None;
    }

    let mut candidates: Vec<_> = contracts
        .iter()
        .filter_map(|contract| score_contract_for_scenario(scenario, &keywords, contract))
        .collect();

    candidates.sort_by(|a, b| {
        b.relevance_score
            .cmp(&a.relevance_score)
            .then_with(|| b.liquidity.partial_cmp(&a.liquidity).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| b.volume_24h.partial_cmp(&a.volume_24h).unwrap_or(std::cmp::Ordering::Equal))
    });
    candidates.truncate(limit);

    if candidates.is_empty() {
        return None;
    }

    for (idx, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = idx + 1;
    }

    Some(ScenarioMappingSuggestion {
        scenario_id: scenario.id,
        scenario_name: scenario.name.clone(),
        scenario_probability_pct: scenario.probability,
        keywords,
        candidates,
    })
}

fn score_contract_for_scenario(
    scenario: &scenarios::Scenario,
    keywords: &[String],
    contract: &prediction_contracts::PredictionContract,
) -> Option<ContractMappingCandidate> {
    let search_text = format!(
        "{} {}",
        contract.question.to_lowercase(),
        contract.event_title.to_lowercase()
    );
    let matched_keywords: Vec<String> = keywords
        .iter()
        .filter(|keyword| search_text.contains(keyword.as_str()))
        .cloned()
        .collect();
    if matched_keywords.is_empty() {
        return None;
    }

    let inferred_category = infer_scenario_category(scenario);
    let mut relevance_score = (matched_keywords.len() as u32) * 25;
    if inferred_category
        .as_deref()
        .map(|category| category == contract.category)
        .unwrap_or(false)
    {
        relevance_score += 15;
    }
    if search_text.contains(&scenario.name.to_lowercase()) {
        relevance_score += 20;
    }
    if contract.liquidity >= 50_000.0 {
        relevance_score += 10;
    } else if contract.liquidity >= 10_000.0 {
        relevance_score += 5;
    }

    Some(ContractMappingCandidate {
        rank: 0,
        contract_id: contract.contract_id.clone(),
        question: contract.question.clone(),
        event_title: contract.event_title.clone(),
        category: contract.category.clone(),
        probability_pct: round1(contract.last_price * 100.0),
        volume_24h: contract.volume_24h,
        liquidity: contract.liquidity,
        relevance_score,
        matched_keywords,
        map_command: format!(
            "pftui data predictions map --scenario \"{}\" --contract \"{}\"",
            scenario.name, contract.contract_id
        ),
    })
}

fn scenario_keywords(scenario: &scenarios::Scenario) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut keywords = Vec::new();
    for text in [
        Some(scenario.name.as_str()),
        scenario.description.as_deref(),
        scenario.triggers.as_deref(),
        scenario.historical_precedent.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        for token in tokenize(text) {
            if seen.insert(token.clone()) {
                keywords.push(token);
            }
        }
    }
    keywords
}

fn tokenize(text: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "from", "that", "this", "into", "will", "have", "has",
        "are", "2026", "2025", "2024", "market", "scenario", "probability", "risk", "than",
    ];

    text.split(|c: char| !c.is_ascii_alphanumeric())
        .map(|token| token.trim().to_lowercase())
        .filter(|token| token.len() >= 3 || matches!(token.as_str(), "fed" | "btc" | "cpi"))
        .filter(|token| !STOPWORDS.contains(&token.as_str()))
        .collect()
}

fn infer_scenario_category(scenario: &scenarios::Scenario) -> Option<String> {
    let joined = format!(
        "{} {} {} {}",
        scenario.name,
        scenario.description.as_deref().unwrap_or(""),
        scenario.triggers.as_deref().unwrap_or(""),
        scenario.historical_precedent.as_deref().unwrap_or(""),
    )
    .to_lowercase();

    let category_keywords: HashMap<&str, &[&str]> = HashMap::from([
        ("economics", &["fed", "rate", "recession", "inflation", "cpi", "jobs", "economy"][..]),
        ("geopolitics", &["war", "iran", "china", "tariff", "election", "conflict", "ceasefire"][..]),
        ("crypto", &["btc", "bitcoin", "eth", "ethereum", "crypto", "solana"][..]),
    ]);

    category_keywords.into_iter().find_map(|(category, hints)| {
        hints
            .iter()
            .any(|hint| joined.contains(hint))
            .then(|| category.to_string())
    })
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn print_suggestions(report: &MappingSuggestionReport) {
    if report.suggestions.is_empty() {
        println!("No unmapped high-relevance contracts found for active scenarios.");
        println!("Try `pftui data refresh` first, or widen the scenario descriptions/triggers.");
        return;
    }

    println!(
        "Scenario → Contract Mapping Suggestions\n\nScanned {} active scenario(s) and {} unmapped contract(s).\n",
        report.scenarios_scanned, report.unmapped_contracts_scanned
    );

    for suggestion in &report.suggestions {
        println!(
            "{} ({:.1}%)",
            suggestion.scenario_name, suggestion.scenario_probability_pct
        );
        println!("  Keywords: {}", suggestion.keywords.join(", "));
        for candidate in &suggestion.candidates {
            println!(
                "  {}. {:>5.1}%  score {:>3}  {}",
                candidate.rank,
                candidate.probability_pct,
                candidate.relevance_score,
                truncate_question(&candidate.question, 72),
            );
            println!(
                "     event={}  category={}  vol24h={:.0}  liq={:.0}",
                truncate_question(&candidate.event_title, 36),
                candidate.category,
                candidate.volume_24h,
                candidate.liquidity,
            );
            println!("     matched={} ", candidate.matched_keywords.join(", "));
            println!("     {}", candidate.map_command);
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup_test_db() -> BackendConnection {
        let conn = db::open_in_memory();
        BackendConnection::Sqlite { conn }
    }

    fn insert_scenario(
        backend: &BackendConnection,
        name: &str,
        probability: f64,
        description: Option<&str>,
        triggers: Option<&str>,
    ) -> i64 {
        let conn = backend.sqlite();
        let id = scenarios::add_scenario(conn, name, probability, description, None, triggers, None)
            .unwrap();
        scenarios::update_scenario(conn, id, None, None, None, Some("active")).unwrap();
        id
    }

    fn insert_contract(
        backend: &BackendConnection,
        contract_id: &str,
        question: &str,
        event_title: &str,
        category: &str,
        volume_24h: f64,
        liquidity: f64,
    ) {
        prediction_contracts::upsert_contracts_backend(
            backend,
            &[prediction_contracts::PredictionContract {
                contract_id: contract_id.to_string(),
                exchange: "polymarket".to_string(),
                event_id: "evt".to_string(),
                event_title: event_title.to_string(),
                question: question.to_string(),
                category: category.to_string(),
                last_price: 0.42,
                volume_24h,
                liquidity,
                end_date: None,
                updated_at: 1_711_670_000,
            }],
        )
        .unwrap();
    }

    #[test]
    fn tokenize_removes_stopwords_and_keeps_signal_words() {
        let tokens = tokenize("Will the Fed trigger a recession in 2026?");
        assert!(tokens.contains(&"fed".to_string()));
        assert!(tokens.contains(&"recession".to_string()));
        assert!(!tokens.contains(&"will".to_string()));
        assert!(!tokens.contains(&"2026".to_string()));
    }

    #[test]
    fn suggest_mappings_prefers_high_overlap_and_unmapped_contracts() {
        let backend = setup_test_db();
        let scenario_id = insert_scenario(
            &backend,
            "US Recession 2026",
            40.0,
            Some("Hard landing and recession risk"),
            Some("Fed cuts and labor weakness"),
        );
        insert_contract(
            &backend,
            "c-recession",
            "Will the US enter a recession in 2026?",
            "US recession market",
            "economics",
            25_000.0,
            100_000.0,
        );
        insert_contract(
            &backend,
            "c-inflation",
            "Will CPI exceed 4% by December?",
            "Inflation market",
            "economics",
            50_000.0,
            80_000.0,
        );
        scenario_contract_mappings::add_mapping_backend(&backend, scenario_id, "c-inflation")
            .unwrap();

        let report = build_suggestion_report(&backend, None, 5).unwrap();
        assert_eq!(report.suggestions.len(), 1);
        let candidates = &report.suggestions[0].candidates;
        assert_eq!(candidates[0].contract_id, "c-recession");
        assert!(!candidates.iter().any(|candidate| candidate.contract_id == "c-inflation"));
    }

    #[test]
    fn suggest_mappings_respects_scenario_filter() {
        let backend = setup_test_db();
        insert_scenario(&backend, "Iran escalation", 35.0, Some("Conflict risk"), None);
        insert_contract(
            &backend,
            "c-iran",
            "Will Iran and the US enter direct conflict this year?",
            "Iran risk",
            "geopolitics",
            12_000.0,
            30_000.0,
        );

        let report = build_suggestion_report(&backend, Some("Iran escalation"), 3).unwrap();
        assert_eq!(report.suggestions.len(), 1);
        assert_eq!(report.suggestions[0].scenario_name, "Iran escalation");
    }
}
