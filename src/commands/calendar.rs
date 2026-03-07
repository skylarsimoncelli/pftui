use anyhow::Result;
use serde_json::json;

use crate::data::calendar::{fetch_events, Event};

/// Run the `pftui calendar` command.
pub fn run(days: i64, impact_filter: Option<&str>, json: bool) -> Result<()> {
    let mut events = fetch_events(days)?;

    // Filter by impact level if specified
    if let Some(impact) = impact_filter {
        let impact_lower = impact.to_lowercase();
        events.retain(|e| e.impact.to_lowercase() == impact_lower);
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

/// Print calendar events as a formatted table.
fn print_table(events: &[Event]) {
    if events.is_empty() {
        println!("No calendar events found.");
        return;
    }

    // Calculate column widths
    let date_width = 10;
    let impact_width = 8;
    let name_width = 60;

    // Print header
    println!(
        "{:<date$}  {:<impact$}  {:<name$}",
        "Date",
        "Impact",
        "Event",
        date = date_width,
        impact = impact_width,
        name = name_width,
    );
    println!("{}", "─".repeat(date_width + impact_width + name_width + 4));

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

        println!(
            "{:<date$}  {}  {:<name$}",
            event.date,
            impact_display,
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
