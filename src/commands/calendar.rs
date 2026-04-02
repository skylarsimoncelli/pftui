use anyhow::{bail, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde_json::json;

use crate::cli::CalendarCommand;
use crate::data::calendar::{fetch_events, Event};
use crate::db::backend::BackendConnection;
use crate::db::calendar_cache;

/// Dispatch calendar subcommands.
///
/// When no subcommand is given (`pftui data calendar --json`), defaults to `list`
/// using the top-level flags. When `list` is explicit, subcommand flags take
/// precedence over top-level flags.
pub fn dispatch(
    backend: &BackendConnection,
    command: Option<CalendarCommand>,
    top_days: i64,
    top_impact: Option<String>,
    top_event_type: Option<String>,
    top_json: bool,
) -> Result<()> {
    match command {
        None => run_list(
            backend,
            top_days,
            top_impact.as_deref(),
            top_event_type.as_deref(),
            top_json,
        ),
        Some(CalendarCommand::List {
            days,
            impact,
            event_type,
            json,
        }) => run_list(
            backend,
            days,
            impact.or(top_impact).as_deref(),
            event_type.or(top_event_type).as_deref(),
            json || top_json,
        ),
        Some(CalendarCommand::Add {
            date,
            name,
            impact,
            event_type,
            symbol,
            json,
        }) => run_add(
            backend,
            &date,
            &name,
            &impact,
            &event_type,
            symbol.as_deref(),
            json || top_json,
        ),
        Some(CalendarCommand::Remove { date, name, json }) => {
            run_remove(backend, &date, &name, json || top_json)
        }
    }
}

/// List upcoming calendar events (preserves original `pftui data calendar` behavior).
fn run_list(
    backend: &BackendConnection,
    days: i64,
    impact_filter: Option<&str>,
    type_filter: Option<&str>,
    json: bool,
) -> Result<()> {
    // Fetch from external sources (original behavior)
    let mut events = fetch_events(days)?;

    // Also pull any manually-added events from the DB that may not be in the fetch
    let today = Utc::now().date_naive();
    let cutoff = today + Duration::days(days);
    let today_str = today.format("%Y-%m-%d").to_string();

    let db_events =
        calendar_cache::get_upcoming_events_backend(backend, &today_str, 200).unwrap_or_default();

    // Merge DB events that aren't already in the fetched list
    for db_event in db_events {
        let db_date = NaiveDate::parse_from_str(&db_event.date, "%Y-%m-%d").ok();
        let in_range = db_date
            .map(|d| d >= today && d <= cutoff)
            .unwrap_or(false);
        if !in_range {
            continue;
        }
        let already_present = events
            .iter()
            .any(|e| e.date == db_event.date && e.name == db_event.name);
        if !already_present {
            events.push(Event {
                date: db_event.date,
                name: db_event.name,
                impact: db_event.impact,
                previous: db_event.previous,
                forecast: db_event.forecast,
                event_type: db_event.event_type,
                symbol: db_event.symbol,
            });
        }
    }

    // Sort by date
    events.sort_by(|a, b| a.date.cmp(&b.date));

    // Filter by impact level if specified
    if let Some(impact) = impact_filter {
        let impact_lower = impact.to_lowercase();
        events.retain(|e| e.impact.to_lowercase() == impact_lower);
    }

    // Filter by event type if specified
    if let Some(etype) = type_filter {
        let etype_lower = etype.to_lowercase();
        events.retain(|e| e.event_type.to_lowercase() == etype_lower);
    }

    if events.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No calendar events found for the next {} days.", days);
        }
        return Ok(());
    }

    if json {
        print_json(&events)?;
    } else {
        print_table(&events);
    }

    Ok(())
}

/// Add a custom calendar event (geopolitical deadline, trade event, etc.).
fn run_add(
    backend: &BackendConnection,
    date: &str,
    name: &str,
    impact: &str,
    event_type: &str,
    symbol: Option<&str>,
    json: bool,
) -> Result<()> {
    // Validate date format
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("invalid date format '{}'. Use YYYY-MM-DD", date))?;

    // Validate impact level
    let impact_lower = impact.to_lowercase();
    if !["high", "medium", "low"].contains(&impact_lower.as_str()) {
        bail!(
            "invalid impact level '{}'. Use high, medium, or low",
            impact
        );
    }

    // Validate event type
    let type_lower = event_type.to_lowercase();
    if !["economic", "earnings", "geopolitical"].contains(&type_lower.as_str()) {
        bail!(
            "invalid event type '{}'. Use economic, earnings, or geopolitical",
            event_type
        );
    }

    calendar_cache::upsert_event_backend(
        backend,
        date,
        name,
        &impact_lower,
        None,
        None,
        &type_lower,
        symbol,
    )?;

    if json {
        let result = json!({
            "status": "added",
            "date": date,
            "name": name,
            "impact": impact_lower,
            "event_type": type_lower,
            "symbol": symbol,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "✓ Added {} event: {} ({}) on {}",
            type_lower, name, impact_lower, date
        );
        if let Some(sym) = symbol {
            println!("  Symbol: {}", sym);
        }
        println!("\nThis event will appear in `analytics catalysts` ranking.");
    }

    Ok(())
}

/// Remove a calendar event by date and name.
fn run_remove(backend: &BackendConnection, date: &str, name: &str, json: bool) -> Result<()> {
    // Validate date format
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("invalid date format '{}'. Use YYYY-MM-DD", date))?;

    let deleted = calendar_cache::delete_event_by_name_backend(backend, date, name)?;

    if json {
        let result = json!({
            "status": if deleted > 0 { "removed" } else { "not_found" },
            "date": date,
            "name": name,
            "deleted": deleted,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if deleted > 0 {
        println!("✓ Removed event: {} on {}", name, date);
    } else {
        println!("No event found matching '{}' on {}", name, date);
    }

    Ok(())
}

/// Print calendar events as a formatted table.
fn print_table(events: &[Event]) {
    if events.is_empty() {
        println!("No calendar events found.");
        return;
    }

    // Calculate column widths
    let date_width = 10;
    let impact_width = 8;
    let type_width = 14;
    let name_width = 52;

    // Print header
    println!(
        "{:<date$}  {:<impact$}  {:<type_w$}  {:<name$}",
        "Date",
        "Impact",
        "Type",
        "Event",
        date = date_width,
        impact = impact_width,
        type_w = type_width,
        name = name_width,
    );
    println!(
        "{}",
        "─".repeat(date_width + impact_width + type_width + name_width + 6)
    );

    // Print rows
    for event in events {
        let name = if event.name.len() > name_width {
            format!("{}...", &event.name[..name_width - 3])
        } else {
            event.name.clone()
        };

        // Color-code impact (terminal colors)
        let impact_display = match event.impact.to_lowercase().as_str() {
            "high" => format!("\x1b[31m{:^width$}\x1b[0m", "HIGH", width = impact_width),
            "medium" => format!("\x1b[33m{:^width$}\x1b[0m", "MED", width = impact_width),
            "low" => format!("\x1b[32m{:^width$}\x1b[0m", "LOW", width = impact_width),
            _ => format!("{:^width$}", event.impact, width = impact_width),
        };

        let type_display = match event.event_type.to_lowercase().as_str() {
            "geopolitical" => format!(
                "\x1b[35m{:<width$}\x1b[0m",
                "geopolitical",
                width = type_width
            ),
            "earnings" => format!(
                "\x1b[36m{:<width$}\x1b[0m",
                "earnings",
                width = type_width
            ),
            _ => format!("{:<width$}", event.event_type, width = type_width),
        };

        println!(
            "{:<date$}  {}  {}  {:<name$}",
            event.date,
            impact_display,
            type_display,
            name,
            date = date_width,
            name = name_width,
        );
    }

    println!("\nTotal: {} events", events.len());
}

/// Print calendar events as JSON array.
fn print_json(events: &[Event]) -> Result<()> {
    let json_events: Vec<_> = events
        .iter()
        .map(|event| {
            json!({
                "date": event.date,
                "name": event.name,
                "impact": event.impact,
                "previous": event.previous,
                "forecast": event.forecast,
                "event_type": event.event_type,
                "symbol": event.symbol,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&json_events)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_date_format() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add(&backend, "not-a-date", "Test", "high", "geopolitical", None, false);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("invalid date"),
            "should reject bad date format"
        );
    }

    #[test]
    fn validates_impact_level() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add(
            &backend,
            "2026-04-06",
            "Test",
            "extreme",
            "geopolitical",
            None,
            false,
        );
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("invalid impact"),
            "should reject bad impact level"
        );
    }

    #[test]
    fn validates_event_type() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add(
            &backend,
            "2026-04-06",
            "Test",
            "high",
            "unknown",
            None,
            false,
        );
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("invalid event type"),
            "should reject bad event type"
        );
    }

    #[test]
    fn add_and_remove_roundtrip() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        // Add event
        run_add(
            &backend,
            "2026-04-06",
            "Iran Hormuz Deadline",
            "high",
            "geopolitical",
            None,
            false,
        )
        .unwrap();

        // Verify it exists
        let events =
            calendar_cache::get_upcoming_events_backend(&backend, "2026-04-01", 100).unwrap();
        assert!(
            events.iter().any(|e| e.name == "Iran Hormuz Deadline"),
            "event should be in DB after add"
        );

        // Remove it
        run_remove(&backend, "2026-04-06", "Iran Hormuz Deadline", false).unwrap();

        // Verify it's gone
        let events =
            calendar_cache::get_upcoming_events_backend(&backend, "2026-04-01", 100).unwrap();
        assert!(
            !events.iter().any(|e| e.name == "Iran Hormuz Deadline"),
            "event should be removed"
        );
    }

    #[test]
    fn add_with_symbol() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        run_add(
            &backend,
            "2026-04-15",
            "AAPL Earnings",
            "high",
            "earnings",
            Some("AAPL"),
            false,
        )
        .unwrap();

        let events =
            calendar_cache::get_upcoming_events_backend(&backend, "2026-04-01", 100).unwrap();
        let aapl = events.iter().find(|e| e.name == "AAPL Earnings");
        assert!(aapl.is_some(), "earnings event should be in DB");
        assert_eq!(aapl.unwrap().symbol.as_deref(), Some("AAPL"));
        assert_eq!(aapl.unwrap().event_type, "earnings");
    }

    #[test]
    fn remove_nonexistent_returns_zero() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        // Should not error, just report 0 deleted
        let result = run_remove(&backend, "2026-12-31", "Does Not Exist", false);
        assert!(result.is_ok());
    }
}
