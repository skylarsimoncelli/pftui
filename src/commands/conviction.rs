use crate::db::backend::BackendConnection;
use crate::db::convictions;
use anyhow::Result;
use chrono::Local;
use serde_json::json;

pub fn run_set(
    backend: &BackendConnection,
    symbol: &str,
    score: i32,
    notes: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let id = convictions::set_conviction_backend(backend, symbol, score, notes)?;

    if json_output {
        let all_current = convictions::list_current_backend(backend)?;
        let entry = all_current.iter().find(|e| e.id == id);
        if let Some(e) = entry {
            println!("{}", serde_json::to_string_pretty(e)?);
        } else {
            // Fallback if not in current list (shouldn't happen)
            let history = convictions::get_history_backend(backend, symbol, Some(1))?;
            if let Some(e) = history.first() {
                println!("{}", serde_json::to_string_pretty(e)?);
            }
        }
    } else {
        println!(
            "Set conviction for {} to {} ({})",
            symbol,
            format_score(score),
            notes.unwrap_or("no notes")
        );
    }

    Ok(())
}

pub fn run_list(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let entries = convictions::list_current_backend(backend)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "convictions": entries }))?
        );
    } else {
        if entries.is_empty() {
            println!("No convictions recorded.");
            return Ok(());
        }

        println!("Current Convictions:");
        println!();
        for entry in &entries {
            let time_ago = format_time_ago(&entry.recorded_at);
            println!(
                "  {:<10} {:>3}   {}   {}",
                entry.symbol,
                format_score(entry.score),
                entry.notes.as_deref().unwrap_or("—"),
                time_ago
            );
        }
    }

    Ok(())
}

pub fn run_history(
    backend: &BackendConnection,
    symbol: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let entries = convictions::get_history_backend(backend, symbol, limit)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "history": entries }))?
        );
    } else {
        if entries.is_empty() {
            println!("No conviction history for {}", symbol);
            return Ok(());
        }

        println!("Conviction history for {}:", symbol);
        println!();
        println!("{:<5} {:<20} {:>6} Notes", "ID", "Date", "Score");
        println!("{}", "─".repeat(80));

        for entry in &entries {
            let date = &entry.recorded_at[..16]; // YYYY-MM-DD HH:MM
            let notes_truncated = entry
                .notes
                .as_deref()
                .map(|n| {
                    if n.len() > 50 {
                        format!("{}...", &n[..47])
                    } else {
                        n.to_string()
                    }
                })
                .unwrap_or_else(|| "—".to_string());

            println!(
                "{:<5} {:<20} {:>6} {}",
                entry.id,
                date,
                format_score(entry.score),
                notes_truncated
            );
        }
    }

    Ok(())
}

pub fn run_changes(backend: &BackendConnection, days: usize, json_output: bool) -> Result<()> {
    let changes = convictions::get_changes_backend(backend, days)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "changes": changes }))?
        );
    } else {
        if changes.is_empty() {
            println!("No conviction changes in the last {} days", days);
            return Ok(());
        }

        println!("Conviction changes (last {} days):", days);
        println!();
        println!(
            "{:<10} {:>6} → {:>6}  Δ{:>4}  Date",
            "Symbol", "Old", "New", ""
        );
        println!("{}", "─".repeat(60));

        for change in &changes {
            let new_date = &change.new_date[..16];
            let delta_str = format_delta(change.change_delta);
            println!(
                "{:<10} {:>6} → {:>6}  {:>5}  {}",
                change.symbol,
                format_score(change.old_score),
                format_score(change.new_score),
                delta_str,
                new_date
            );
        }
    }

    Ok(())
}

fn format_score(score: i32) -> String {
    if score > 0 {
        format!("+{}", score)
    } else {
        score.to_string()
    }
}

fn format_delta(delta: i32) -> String {
    if delta > 0 {
        format!("+{}", delta)
    } else {
        delta.to_string()
    }
}

fn format_time_ago(timestamp: &str) -> String {
    let now = Local::now();
    let parsed = chrono::DateTime::parse_from_rfc3339(timestamp);

    if let Ok(dt) = parsed {
        let local_dt = dt.with_timezone(&Local);
        let duration = now.signed_duration_since(local_dt);

        if duration.num_seconds() < 60 {
            "just now".to_string()
        } else if duration.num_minutes() < 60 {
            let m = duration.num_minutes();
            format!("{}m ago", m)
        } else if duration.num_hours() < 24 {
            let h = duration.num_hours();
            format!("{}h ago", h)
        } else if duration.num_days() < 7 {
            let d = duration.num_days();
            format!("{}d ago", d)
        } else if duration.num_weeks() < 4 {
            let w = duration.num_weeks();
            format!("{}w ago", w)
        } else {
            let m = duration.num_days() / 30;
            format!("{}mo ago", m)
        }
    } else {
        // Fallback if parse fails
        timestamp[..10].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_score() {
        assert_eq!(format_score(5), "+5");
        assert_eq!(format_score(0), "0");
        assert_eq!(format_score(-3), "-3");
    }

    #[test]
    fn test_format_delta() {
        assert_eq!(format_delta(3), "+3");
        assert_eq!(format_delta(0), "0");
        assert_eq!(format_delta(-2), "-2");
    }
}
