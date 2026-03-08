use crate::db::thesis;
use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::json;

pub fn run(
    conn: &Connection,
    action: &str,
    value: Option<&str>,
    content: Option<&str>,
    conviction: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "list" => {
            let entries = thesis::list_thesis(conn)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "thesis": entries }))?
                );
                return Ok(());
            }

            if entries.is_empty() {
                println!("No thesis sections found.");
                return Ok(());
            }

            println!(
                "{:<18} {:<10} {:<52} {:<20}",
                "Section", "Conviction", "Content", "Updated"
            );
            println!("{}", "─".repeat(102));
            for entry in entries {
                let preview = if entry.content.len() > 49 {
                    format!("{}...", &entry.content[..49])
                } else {
                    entry.content.clone()
                };
                println!(
                    "{:<18} {:<10} {:<52} {:<20}",
                    entry.section,
                    entry.conviction,
                    preview,
                    &entry.updated_at[..entry.updated_at.len().min(19)]
                );
            }
        }
        "update" => {
            let section = value.ok_or_else(|| anyhow::anyhow!("section name required"))?;
            let content = content.ok_or_else(|| anyhow::anyhow!("--content required"))?;

            let conviction = if let Some(c) = conviction {
                validate_conviction(c)?
            } else if let Some(existing) = thesis::get_thesis_section(conn, section)? {
                existing.conviction
            } else {
                "medium".to_string()
            };

            thesis::upsert_thesis(conn, section, content, &conviction)?;

            if json_output {
                let updated = thesis::get_thesis_section(conn, section)?.unwrap();
                println!("{}", serde_json::to_string_pretty(&updated)?);
            } else {
                println!("Updated thesis section '{}'", section);
            }
        }
        "history" => {
            let section = value.ok_or_else(|| anyhow::anyhow!("section name required"))?;
            let entries = thesis::get_thesis_history(conn, section, limit)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "section": section, "history": entries }))?
                );
                return Ok(());
            }

            if entries.is_empty() {
                println!("No thesis history for section '{}'.", section);
                return Ok(());
            }

            println!("History for '{}':", section);
            for entry in entries {
                let preview = if entry.content.len() > 64 {
                    format!("{}...", &entry.content[..64])
                } else {
                    entry.content
                };
                println!(
                    "  {}  [{}] {}",
                    &entry.recorded_at[..entry.recorded_at.len().min(19)],
                    entry.conviction,
                    preview
                );
            }
        }
        "remove" => {
            let section = value.ok_or_else(|| anyhow::anyhow!("section name required"))?;
            thesis::remove_thesis(conn, section)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "removed": section }))?
                );
            } else {
                println!("Removed thesis section '{}'", section);
            }
        }
        _ => bail!("unknown action '{}'. Valid: list, update, history, remove", action),
    }

    Ok(())
}

fn validate_conviction(conviction: &str) -> Result<String> {
    let lower = conviction.to_lowercase();
    match lower.as_str() {
        "high" | "medium" | "low" => Ok(lower),
        _ => bail!("invalid conviction '{}'. Use: high, medium, low", conviction),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_conviction_values() {
        assert_eq!(validate_conviction("high").unwrap(), "high");
        assert!(validate_conviction("invalid").is_err());
    }
}
