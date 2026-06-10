//! `pftui analytics lessons rules` — standing operational rules consolidated
//! from the prediction_lessons library. Rendered compactly: the active rule
//! list is injected verbatim into analyst prompts.

use anyhow::Result;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::standing_rules;

pub fn run_add(
    backend: &BackendConnection,
    rule: &str,
    rationale: Option<&str>,
    sources: Option<&str>,
    enforcement: &str,
    json_output: bool,
) -> Result<()> {
    // Validate the --sources list is comma-separated integers before write.
    if let Some(raw) = sources {
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() || part.parse::<i64>().is_err() {
                anyhow::bail!(
                    "invalid --sources '{}': expected comma-separated prediction_lessons ids (e.g. \"12,40,77\")",
                    raw
                );
            }
        }
    }
    let id = standing_rules::add_rule_backend(backend, rule, rationale, sources, enforcement)?;
    if json_output {
        let row = standing_rules::get_rule_backend(backend, id)?;
        println!("{}", serde_json::to_string_pretty(&json!({ "added": row }))?);
    } else {
        println!("Added standing rule #{} ({})", id, enforcement);
    }
    Ok(())
}

pub fn run_list(backend: &BackendConnection, all: bool, json_output: bool) -> Result<()> {
    let rules = standing_rules::list_rules_backend(backend, all)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "rules": rules, "count": rules.len() }))?
        );
    } else if rules.is_empty() {
        println!(
            "No {}standing rules. Add one with `analytics lessons rules add --rule \"...\"`.",
            if all { "" } else { "active " }
        );
    } else {
        // Compact render — this block gets injected into prompts.
        println!("Standing rules ({}):", rules.len());
        for r in &rules {
            let retired = if r.status == "retired" { " [retired]" } else { "" };
            let violations = if r.violation_count > 0 {
                format!(" (violated ×{})", r.violation_count)
            } else {
                String::new()
            };
            println!("  #{} [{}]{}{} {}", r.id, r.enforcement, retired, violations, r.rule);
            if let Some(rationale) = r.rationale.as_deref() {
                println!("      why: {}", rationale);
            }
            if let Some(srcs) = r.source_lesson_ids.as_deref() {
                println!("      lessons: {}", srcs);
            }
        }
    }
    Ok(())
}

pub fn run_retire(backend: &BackendConnection, id: i64, json_output: bool) -> Result<()> {
    let updated = standing_rules::retire_rule_backend(backend, id)?;
    if !updated {
        anyhow::bail!("standing rule #{} not found", id);
    }
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "retired": id }))?
        );
    } else {
        println!("Retired standing rule #{}", id);
    }
    Ok(())
}

pub fn run_cite(backend: &BackendConnection, id: i64, json_output: bool) -> Result<()> {
    let count = standing_rules::cite_rule_backend(backend, id)?;
    let Some(count) = count else {
        anyhow::bail!("standing rule #{} not found", id);
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "cited": id, "violation_count": count }))?
        );
    } else {
        println!(
            "Recorded violation of standing rule #{} (violation_count now {})",
            id, count
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn add_list_retire_cite_workflow() {
        let backend = setup_backend();
        run_add(
            &backend,
            "Cap magnitude forecasts at 1.5x trailing realized vol.",
            Some("Magnitude-overshoot is the dominant repeated failure pattern."),
            Some("12,40,77"),
            "advisory",
            false,
        )
        .unwrap();
        run_list(&backend, false, false).unwrap();
        run_list(&backend, true, true).unwrap();
        run_cite(&backend, 1, false).unwrap();
        run_retire(&backend, 1, false).unwrap();
        let active = crate::db::standing_rules::list_rules_backend(&backend, false).unwrap();
        assert!(active.is_empty());
        let all = crate::db::standing_rules::list_rules_backend(&backend, true).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].violation_count, 1);
        assert_eq!(all[0].status, "retired");
    }

    #[test]
    fn add_rejects_malformed_sources() {
        let backend = setup_backend();
        let err = run_add(&backend, "rule", None, Some("12,abc"), "advisory", false);
        assert!(err.is_err());
    }

    #[test]
    fn retire_and_cite_missing_id_error() {
        let backend = setup_backend();
        assert!(run_retire(&backend, 42, false).is_err());
        assert!(run_cite(&backend, 42, false).is_err());
    }
}
