use crate::db::backend::BackendConnection;
use crate::db::journal;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::json;

pub fn normalize_tags(repeated_tags: &[String], csv_tags: Option<&str>) -> Option<String> {
    let mut normalized = Vec::new();

    for raw in repeated_tags {
        for tag in raw.split(',') {
            let trimmed = tag.trim();
            if !trimmed.is_empty() && !normalized.iter().any(|existing| existing == trimmed) {
                normalized.push(trimmed.to_string());
            }
        }
    }

    if let Some(csv) = csv_tags {
        for tag in csv.split(',') {
            let trimmed = tag.trim();
            if !trimmed.is_empty() && !normalized.iter().any(|existing| existing == trimmed) {
                normalized.push(trimmed.to_string());
            }
        }
    }

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.join(","))
    }
}

pub fn run_add(
    backend: &BackendConnection,
    content: &str,
    date: Option<&str>,
    tag: Option<&str>,
    symbol: Option<&str>,
    conviction: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let timestamp = if let Some(d) = date {
        // Parse user-provided date — try ISO 8601 first, then fallback to date-only
        if let Ok(dt) = DateTime::parse_from_rfc3339(d) {
            dt.to_rfc3339()
        } else if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M") {
            DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).to_rfc3339()
        } else {
            // Try date-only, default to midnight UTC
            let naive_date = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")?;
            let naive_dt = naive_date.and_hms_opt(0, 0, 0).unwrap();
            DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc).to_rfc3339()
        }
    } else {
        Utc::now().to_rfc3339()
    };

    let entry = journal::NewJournalEntry {
        timestamp,
        content: content.to_string(),
        tag: tag.map(|s| s.to_string()),
        symbol: symbol.map(|s| s.to_string()),
        conviction: conviction.map(|s| s.to_string()),
        status: "open".to_string(),
    };

    let id = journal::add_entry_backend(backend, &entry)?;

    if json_output {
        let inserted = journal::get_entry_backend(backend, id)?.unwrap();
        println!("{}", serde_json::to_string_pretty(&inserted)?);
    } else {
        println!("Added journal entry #{}", id);
    }

    Ok(())
}

pub fn run_list(
    backend: &BackendConnection,
    limit: Option<usize>,
    since: Option<&str>,
    tag: Option<&str>,
    symbol: Option<&str>,
    status: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let since_timestamp = if let Some(s) = since {
        Some(parse_since(s)?)
    } else {
        None
    };

    let entries = journal::list_entries_backend(
        backend,
        limit,
        since_timestamp.as_deref(),
        tag,
        symbol,
        status,
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "entries": entries }))?
        );
    } else {
        if entries.is_empty() {
            println!("No journal entries found.");
            return Ok(());
        }

        println!(
            "{:<5} {:<20} {:<50} {:<12} {:<10} {:<10}",
            "ID", "Timestamp", "Content", "Tag", "Symbol", "Status"
        );
        println!("{}", "─".repeat(110));

        for entry in entries {
            let truncated_content = if entry.content.len() > 47 {
                format!("{}...", &entry.content[..47])
            } else {
                entry.content.clone()
            };

            println!(
                "{:<5} {:<20} {:<50} {:<12} {:<10} {:<10}",
                entry.id,
                &entry.timestamp[..16], // Show YYYY-MM-DD HH:MM
                truncated_content,
                entry.tag.as_deref().unwrap_or("—"),
                entry.symbol.as_deref().unwrap_or("—"),
                entry.status
            );
        }
    }

    Ok(())
}

pub fn run_search(
    backend: &BackendConnection,
    query: &str,
    since: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let since_timestamp = if let Some(s) = since {
        Some(parse_since(s)?)
    } else {
        None
    };

    let entries =
        journal::search_entries_backend(backend, query, since_timestamp.as_deref(), limit)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "entries": entries }))?
        );
    } else {
        if entries.is_empty() {
            println!("No journal entries found matching '{}'.", query);
            return Ok(());
        }

        println!(
            "{:<5} {:<20} {:<50} {:<12}",
            "ID", "Timestamp", "Content", "Tag"
        );
        println!("{}", "─".repeat(90));

        for entry in entries {
            let truncated_content = if entry.content.len() > 47 {
                format!("{}...", &entry.content[..47])
            } else {
                entry.content.clone()
            };

            println!(
                "{:<5} {:<20} {:<50} {:<12}",
                entry.id,
                &entry.timestamp[..16],
                truncated_content,
                entry.tag.as_deref().unwrap_or("—")
            );
        }
    }

    Ok(())
}

pub fn run_update(
    backend: &BackendConnection,
    id: i64,
    content: Option<&str>,
    status: Option<&str>,
    json_output: bool,
) -> Result<()> {
    journal::update_entry_backend(backend, id, content, status)?;

    if json_output {
        let updated = journal::get_entry_backend(backend, id)?.unwrap();
        println!("{}", serde_json::to_string_pretty(&updated)?);
    } else {
        println!("Updated journal entry #{}", id);
    }

    Ok(())
}

pub fn run_remove(backend: &BackendConnection, id: i64, json_output: bool) -> Result<()> {
    journal::remove_entry_backend(backend, id)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "removed": id }))?
        );
    } else {
        println!("Removed journal entry #{}", id);
    }

    Ok(())
}

pub fn run_tags(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let tags = journal::get_all_tags_backend(backend)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "tags": tags }))?
        );
    } else {
        if tags.is_empty() {
            println!("No tags found.");
            return Ok(());
        }

        println!("{:<20} {:<10}", "Tag", "Count");
        println!("{}", "─".repeat(30));

        for (tag, count) in tags {
            println!("{:<20} {:<10}", tag, count);
        }
    }

    Ok(())
}

pub fn run_stats(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let stats = journal::get_stats_backend(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        println!("Total entries: {}", stats.total_entries);

        if !stats.entries_by_tag.is_empty() {
            println!("\nEntries by tag:");
            for (tag, count) in stats.entries_by_tag {
                println!("  {}: {}", tag, count);
            }
        }

        if !stats.entries_by_month.is_empty() {
            println!("\nEntries by month:");
            for (month, count) in stats.entries_by_month {
                println!("  {}: {}", month, count);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_tags;

    #[test]
    fn normalize_tags_merges_repeat_and_csv_inputs() {
        let normalized = normalize_tags(
            &["macro".to_string(), "oil,geopolitical".to_string()],
            Some("oil, rates "),
        );
        assert_eq!(normalized.as_deref(), Some("macro,oil,geopolitical,rates"));
    }
}

fn parse_since(since: &str) -> Result<String> {
    // Handle relative dates like "7d", "30d", "1w"
    if let Some(stripped) = since.strip_suffix('d') {
        let days: i64 = stripped.parse()?;
        let date = Utc::now() - chrono::Duration::days(days);
        Ok(date.to_rfc3339())
    } else if let Some(stripped) = since.strip_suffix('w') {
        let weeks: i64 = stripped.parse()?;
        let date = Utc::now() - chrono::Duration::weeks(weeks);
        Ok(date.to_rfc3339())
    } else if let Some(stripped) = since.strip_suffix('m') {
        let months: i64 = stripped.parse()?;
        let date = Utc::now() - chrono::Duration::days(months * 30);
        Ok(date.to_rfc3339())
    } else {
        // Try parsing as absolute date
        if let Ok(dt) = DateTime::parse_from_rfc3339(since) {
            Ok(dt.to_rfc3339())
        } else {
            let naive_date = chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d")?;
            let naive_dt = naive_date.and_hms_opt(0, 0, 0).unwrap();
            Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc).to_rfc3339())
        }
    }
}
