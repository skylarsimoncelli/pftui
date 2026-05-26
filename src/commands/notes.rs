use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::daily_notes;

fn validate_section(section: &str) -> Result<()> {
    match section {
        "market" | "decisions" | "system" | "analysis" | "events" | "general" | "alert" => Ok(()),
        _ => bail!(
            "invalid section '{}'. Valid: market, decisions, system, analysis, events, general, alert",
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
    author: Option<&str>,
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
            let author_value = author.unwrap_or("system");

            let new_id =
                daily_notes::add_note_backend(backend, &note_date, sec, content, author_value)?;
            if json_output {
                let rows = daily_notes::list_notes_backend(backend, None, None, None, None)?;
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
            let rows = daily_notes::list_notes_backend(backend, date, section, limit, author)?;
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
                    println!(
                        "  #{} [{}:{}] {}",
                        row.id, row.date, row.section, row.content
                    );
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
                    println!(
                        "  #{} [{}:{}] {}",
                        row.id, row.date, row.section, row.content
                    );
                }
            }
        }

        "remove" => {
            let note_id = id.ok_or_else(|| anyhow::anyhow!("--id required for remove"))?;
            daily_notes::remove_note_backend(backend, note_id)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "removed": note_id }))?
                );
            } else {
                println!("Removed note #{}", note_id);
            }
        }

        _ => bail!(
            "unknown notes action '{}'. Valid: add, list, search, remove",
            action
        ),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_section_accepts_all_valid() {
        for section in &[
            "market",
            "decisions",
            "system",
            "analysis",
            "events",
            "general",
            "alert",
        ] {
            assert!(
                validate_section(section).is_ok(),
                "section '{}' should be valid",
                section
            );
        }
    }

    #[test]
    fn test_validate_section_rejects_invalid() {
        for section in &["alerts", "foo", "trading", ""] {
            assert!(
                validate_section(section).is_err(),
                "section '{}' should be invalid",
                section
            );
        }
    }

    fn setup_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn notes_add_persists_author_flag() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("LOW: pre-market scan"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            Some("analyst-low"),
            false,
        )
        .unwrap();
        let rows = daily_notes::list_notes_backend(&backend, None, None, None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].author, "analyst-low");
        assert_eq!(rows[0].content, "LOW: pre-market scan");
    }

    #[test]
    fn notes_list_filters_by_author() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("low note"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            Some("analyst-low"),
            false,
        )
        .unwrap();
        run(
            &backend,
            "add",
            Some("medium note"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            Some("analyst-medium"),
            false,
        )
        .unwrap();
        let lows = daily_notes::list_notes_backend(&backend, None, None, None, Some("analyst-low"))
            .unwrap();
        assert_eq!(lows.len(), 1);
        assert_eq!(lows[0].content, "low note");
    }

    #[test]
    fn notes_add_defaults_author_to_system() {
        let backend = setup_backend();
        run(
            &backend,
            "add",
            Some("no author"),
            None,
            Some("2026-03-04"),
            Some("analysis"),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let rows = daily_notes::list_notes_backend(&backend, None, None, None, None).unwrap();
        assert_eq!(rows[0].author, "system");
    }
}
