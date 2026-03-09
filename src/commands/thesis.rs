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

pub fn run_history(
    backend: &BackendConnection,
    section: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let entries = thesis::get_thesis_history_backend(backend, section, limit)?;

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

pub fn run_remove(backend: &BackendConnection, section: &str, json_output: bool) -> Result<()> {
    thesis::remove_thesis_backend(backend, section)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({ "removed": section }))?);
    } else {
        println!("Removed thesis section: {}", section);
    }

    Ok(())
}
