#![allow(dead_code)]

use crate::db::backend::BackendConnection;
use crate::db::thesis;
use anyhow::Result;
use serde_json::json;

pub fn run_update(
    backend: &BackendConnection,
    section: &str,
    content: &str,
    conviction: Option<&str>,
    json_output: bool,
) -> Result<()> {
    thesis::upsert_thesis_backend(backend, section, content, conviction)?;

    if json_output {
        let updated = thesis::get_thesis_section_backend(backend, section)?.unwrap();
        println!("{}", serde_json::to_string_pretty(&updated)?);
    } else {
        println!("Updated thesis section: {}", section);
    }

    Ok(())
}

pub fn run_list(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let entries = thesis::list_thesis_backend(backend)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "sections": entries }))?
        );
    } else {
        if entries.is_empty() {
            println!("No thesis sections found.");
            return Ok(());
        }

        println!(
            "{:<20} {:<12} {:<50} {:<20}",
            "Section", "Conviction", "Content (preview)", "Updated"
        );
        println!("{}", "─".repeat(105));

        for entry in entries {
            let truncated_content = if entry.content.len() > 47 {
                format!("{}...", &entry.content[..47])
            } else {
                entry.content.clone()
            };

            println!(
                "{:<20} {:<12} {:<50} {:<20}",
                entry.section,
                entry.conviction,
                truncated_content,
                &entry.updated_at[..16], // Show YYYY-MM-DD HH:MM
            );
        }
    }

    Ok(())
}

pub fn run_history(
    backend: &BackendConnection,
    section: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let entries = thesis::get_thesis_history_backend(backend, section, limit)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "history": entries }))?
        );
    } else {
        if entries.is_empty() {
            println!("No history found for section: {}", section);
            return Ok(());
        }

        println!("Thesis History — {}", section);
        println!("{}", "─".repeat(80));
        println!();

        for entry in entries {
            println!(
                "Recorded: {} | Conviction: {}",
                entry.recorded_at, entry.conviction
            );
            println!("{}", entry.content);
            println!("{}", "─".repeat(80));
        }
    }

    Ok(())
}

pub fn run_remove(backend: &BackendConnection, section: &str, json_output: bool) -> Result<()> {
    thesis::remove_thesis_backend(backend, section)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "removed": section }))?
        );
    } else {
        println!("Removed thesis section: {}", section);
    }

    Ok(())
}

/// `pftui analytics thesis set-review <section> --date YYYY-MM-DD`
pub fn run_set_review(
    backend: &BackendConnection,
    section: &str,
    date: &str,
    json_output: bool,
) -> Result<()> {
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        anyhow::bail!("invalid --date '{}': expected YYYY-MM-DD", date);
    }
    let updated = thesis::set_review_by_backend(backend, section, Some(date))?;
    if !updated {
        anyhow::bail!(
            "thesis section '{}' not found (see existing sections with `analytics thesis review-due --json`)",
            section
        );
    }
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "section": section, "review_by": date }))?
        );
    } else {
        println!("Thesis section '{}' scheduled for review by {}", section, date);
    }
    Ok(())
}

/// `pftui analytics thesis review-due [--json]` — sections whose review_by
/// has passed, plus sections with no review date at all ("unscheduled").
pub fn run_review_due(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let entries = thesis::list_thesis_backend(backend)?;

    let mut due: Vec<&crate::db::thesis::ThesisEntry> = Vec::new();
    let mut unscheduled: Vec<&crate::db::thesis::ThesisEntry> = Vec::new();
    for entry in &entries {
        match entry.review_by.as_deref() {
            Some(date) if date <= today.as_str() => due.push(entry),
            Some(_) => {}
            None => unscheduled.push(entry),
        }
    }
    due.sort_by(|a, b| a.review_by.cmp(&b.review_by));

    if json_output {
        let due_json: Vec<serde_json::Value> = due
            .iter()
            .map(|e| {
                json!({
                    "section": e.section,
                    "review_by": e.review_by,
                    "conviction": e.conviction,
                    "updated_at": e.updated_at,
                })
            })
            .collect();
        let unscheduled_json: Vec<serde_json::Value> = unscheduled
            .iter()
            .map(|e| {
                json!({
                    "section": e.section,
                    "conviction": e.conviction,
                    "updated_at": e.updated_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "as_of": today,
                "due": due_json,
                "unscheduled": unscheduled_json,
                "due_count": due.len(),
                "unscheduled_count": unscheduled.len(),
            }))?
        );
    } else {
        if due.is_empty() && unscheduled.is_empty() {
            println!("No thesis sections due for review and none unscheduled.");
            return Ok(());
        }
        if !due.is_empty() {
            println!("Thesis sections due for review (as of {}):", today);
            for e in &due {
                println!(
                    "  {} — review_by {} (conviction {}, last updated {})",
                    e.section,
                    e.review_by.as_deref().unwrap_or("?"),
                    e.conviction,
                    &e.updated_at[..e.updated_at.len().min(10)]
                );
            }
        } else {
            println!("No thesis sections past their review date.");
        }
        if !unscheduled.is_empty() {
            println!("\nUnscheduled (no review_by set — schedule with `analytics thesis set-review <section> --date YYYY-MM-DD`):");
            for e in &unscheduled {
                println!("  {} (conviction {})", e.section, e.conviction);
            }
        }
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
    fn set_review_and_review_due() {
        let backend = setup_backend();
        crate::db::thesis::upsert_thesis_backend(
            &backend,
            "demo-cycle",
            "Synthetic demo thesis content.",
            Some("medium"),
        )
        .unwrap();
        crate::db::thesis::upsert_thesis_backend(
            &backend,
            "demo-rotation",
            "Another synthetic demo thesis.",
            None,
        )
        .unwrap();

        // Past date → due.
        run_set_review(&backend, "demo-cycle", "2020-01-01", false).unwrap();
        let entries = crate::db::thesis::list_thesis_backend(&backend).unwrap();
        let cycle = entries.iter().find(|e| e.section == "demo-cycle").unwrap();
        assert_eq!(cycle.review_by.as_deref(), Some("2020-01-01"));
        let rotation = entries.iter().find(|e| e.section == "demo-rotation").unwrap();
        assert!(rotation.review_by.is_none());

        // review_by survives a content upsert (REPLACE path).
        crate::db::thesis::upsert_thesis_backend(
            &backend,
            "demo-cycle",
            "Updated synthetic demo content.",
            None,
        )
        .unwrap();
        let entries = crate::db::thesis::list_thesis_backend(&backend).unwrap();
        let cycle = entries.iter().find(|e| e.section == "demo-cycle").unwrap();
        assert_eq!(cycle.review_by.as_deref(), Some("2020-01-01"));

        run_review_due(&backend, false).unwrap();
        run_review_due(&backend, true).unwrap();
    }

    #[test]
    fn set_review_validates_inputs() {
        let backend = setup_backend();
        assert!(run_set_review(&backend, "missing-section", "2026-01-01", false).is_err());
        crate::db::thesis::upsert_thesis_backend(&backend, "demo", "x", None).unwrap();
        assert!(run_set_review(&backend, "demo", "not-a-date", false).is_err());
    }
}
