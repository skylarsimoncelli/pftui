use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::db::news_cache::{get_latest_news, NewsEntry};

/// Run the `pftui news` command.
pub fn run(
    conn: &Connection,
    source: Option<&str>,
    search: Option<&str>,
    hours: Option<i64>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let entries = get_latest_news(conn, limit, source, None, search, hours)?;

    if entries.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No cached news entries. Run `pftui refresh` first.");
        }
        return Ok(());
    }

    if json {
        print_json(&entries)?;
    } else {
        print_table(&entries);
    }

    Ok(())
}

/// Print news entries as a formatted table.
fn print_table(entries: &[NewsEntry]) {
    if entries.is_empty() {
        println!("No matching news entries found.");
        return;
    }

    // Calculate column widths
    let title_width = 80;
    let source_width = 20;
    let time_width = 16;

    // Print header
    println!(
        "{:<title$}  {:<source$}  {:<time$}",
        "Title",
        "Source",
        "Time",
        title = title_width,
        source = source_width,
        time = time_width,
    );
    println!("{}", "─".repeat(title_width + source_width + time_width + 4));

    // Print rows
    for entry in entries {
        let title = if entry.title.len() > title_width {
            format!("{}...", &entry.title[..title_width - 3])
        } else {
            entry.title.clone()
        };

        let time_str = format_timestamp(entry.published_at);

        println!(
            "{:<title$}  {:<source$}  {:<time$}",
            title,
            entry.source,
            time_str,
            title = title_width,
            source = source_width,
            time = time_width,
        );
    }

    println!("\nTotal: {} articles", entries.len());
}

/// Format Unix timestamp as relative time or date string.
fn format_timestamp(ts: i64) -> String {
    let dt = chrono::DateTime::from_timestamp(ts, 0)
        .unwrap_or_else(chrono::Utc::now);
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(dt);

    if diff.num_minutes() < 60 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_days() < 7 {
        format!("{}d ago", diff.num_days())
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

/// Print news entries as JSON array.
fn print_json(entries: &[NewsEntry]) -> Result<()> {
    let json_entries: Vec<_> = entries
        .iter()
        .map(|entry| {
            json!({
                "id": entry.id,
                "title": entry.title,
                "url": entry.url,
                "source": entry.source,
                "category": entry.category,
                "published_at": entry.published_at,
                "fetched_at": entry.fetched_at,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&json_entries)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp() {
        let now = chrono::Utc::now().timestamp();
        let five_min_ago = now - 300;
        let two_hours_ago = now - 7200;
        let yesterday = now - 86400;

        assert!(format_timestamp(five_min_ago).contains("m ago"));
        assert!(format_timestamp(two_hours_ago).contains("h ago"));
        assert!(format_timestamp(yesterday).contains("d ago"));
    }
}
