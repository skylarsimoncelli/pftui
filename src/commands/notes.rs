use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::daily_notes;

fn validate_section(section: &str) -> Result<()> {
    match section {
        "market" | "decisions" | "system" | "analysis" | "events" | "general" => Ok(()),
        _ => bail!(
            "invalid section '{}'. Valid: market, decisions, system, analysis, events, general",
            section
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    date: Option<&str>,
    section: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let content = value.ok_or_else(|| anyhow::anyhow!("note content required"))?;
            let note_date = date
                .map(|d| d.to_string())
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
            let sec = section.unwrap_or("general");
            validate_section(sec)?;

            let new_id = daily_notes::add_note_backend(backend, &note_date, sec, content)?;
            if json_output {
                let rows = daily_notes::list_notes_backend(backend, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                }
            } else {
                println!("Added note #{} ({}/{})", new_id, note_date, sec);
            }
        }

        "list" => {
            if let Some(s) = section {
                validate_section(s)?;
            }
            let rows = daily_notes::list_notes_backend(backend, date, section, limit)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "notes": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No notes found.");
            } else {
                println!("Daily notes ({}):", rows.len());
                for row in rows {
                    println!("  #{} [{}:{}] {}", row.id, row.date, row.section, row.content);
                }
            }
        }

        "search" => {
            let query = value.ok_or_else(|| anyhow::anyhow!("search query required"))?;
            let rows = daily_notes::search_notes_backend(backend, query, since, limit)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "notes": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No notes matched '{}'.", query);
            } else {
                println!("Search results for '{}' ({}):", query, rows.len());
                for row in rows {
                    println!("  #{} [{}:{}] {}", row.id, row.date, row.section, row.content);
                }
            }
        }

        "remove" => {
            let note_id = id.ok_or_else(|| anyhow::anyhow!("--id required for remove"))?;
            daily_notes::remove_note_backend(backend, note_id)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "removed": note_id }))?);
            } else {
                println!("Removed note #{}", note_id);
            }
        }

        _ => bail!("unknown notes action '{}'. Valid: add, list, search, remove", action),
    }

    Ok(())
}
