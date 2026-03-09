use crate::db::{self, thesis};
use anyhow::Result;
use serde_json::json;

pub fn run_update(
    section: &str,
    content: &str,
    conviction: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    thesis::upsert_thesis(&conn, section, content, conviction)?;

    if json_output {
        let updated = thesis::get_thesis_section(&conn, section)?.unwrap();
        println!("{}", serde_json::to_string_pretty(&updated)?);
    } else {
        println!("Updated thesis section: {}", section);
    }

    Ok(())
}

pub fn run_list(json_output: bool) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    let entries = thesis::list_thesis(&conn)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({ "sections": entries }))?);
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

pub fn run_history(section: &str, limit: Option<usize>, json_output: bool) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    let entries = thesis::get_thesis_history(&conn, section, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({ "history": entries }))?);
    } else {
        if entries.is_empty() {
            println!("No history found for section: {}", section);
            return Ok(());
        }

        println!("Thesis History — {}", section);
        println!("{}", "─".repeat(80));
        println!();

        for entry in entries {
            println!("Recorded: {} | Conviction: {}", entry.recorded_at, entry.conviction);
            println!("{}", entry.content);
            println!("{}", "─".repeat(80));
        }
    }

    Ok(())
}

pub fn run_remove(section: &str, json_output: bool) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    thesis::remove_thesis(&conn, section)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({ "removed": section }))?);
    } else {
        println!("Removed thesis section: {}", section);
    }

    Ok(())
}
