use anyhow::Result;

use crate::data::fred;
use crate::db::backend::BackendConnection;
use crate::db::economic_data;
use crate::db::macro_events;

pub fn run(backend: &BackendConnection, indicator: Option<&str>, json: bool) -> Result<()> {
    let mut rows = economic_data::get_all_backend(backend)?;
    let macro_events = macro_events::list_recent_backend(backend, 10)?;
    if let Some(ind) = indicator {
        let needle = ind.to_lowercase();
        rows.retain(|r| r.indicator.to_lowercase() == needle);
    }

    if json {
        let indicators: Vec<_> = rows
            .iter()
            .map(|r| {
                let (unit, display_name) = indicator_metadata(&r.indicator);
                serde_json::json!({
                    "indicator": r.indicator,
                    "display_name": display_name,
                    "value": r.value.to_string(),
                    "unit": unit,
                    "previous": r.previous.map(|v| v.to_string()),
                    "change": r.change.map(|v| v.to_string()),
                    "source_url": r.source_url,
                    "fetched_at": r.fetched_at,
                })
            })
            .collect();
        let surprises: Vec<_> = macro_events
            .iter()
            .map(|event| {
                let name = fred::series_by_id(&event.series_id)
                    .map(|series| series.name)
                    .unwrap_or(event.series_id.as_str());
                serde_json::json!({
                    "series_id": event.series_id,
                    "series_name": name,
                    "event_date": event.event_date,
                    "expected": event.expected.to_string(),
                    "actual": event.actual.to_string(),
                    "surprise_pct": event.surprise_pct.to_string(),
                    "created_at": event.created_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "indicators": indicators,
                "macro_events": surprises,
            }))?
        );
        return Ok(());
    }

    if rows.is_empty() {
        println!("No economy data available. Run `pftui refresh` first.");
        return Ok(());
    }

    println!(
        "{:<24} {:>12} {:>12} {:>12}  Source",
        "Indicator", "Value", "Previous", "Change"
    );
    println!("{}", "─".repeat(84));

    for r in &rows {
        let previous = r
            .previous
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());
        let change = r
            .change
            .map(|v| format!("{:+.2}", v))
            .unwrap_or_else(|| "—".to_string());
        println!(
            "{:<24} {:>12} {:>12} {:>12}  {}",
            display_name(&r.indicator),
            format!("{:.2}", r.value),
            previous,
            change,
            truncate_url(&r.source_url, 18),
        );
    }

    if !macro_events.is_empty() {
        println!();
        println!("Recent macro surprises:");
        for event in macro_events.iter().take(5) {
            let name = fred::series_by_id(&event.series_id)
                .map(|series| series.name)
                .unwrap_or(event.series_id.as_str());
            println!(
                "  {} ({}) expected {} actual {} surprise {:+}%",
                name, event.event_date, event.expected, event.actual, event.surprise_pct
            );
        }
    }

    Ok(())
}

fn display_name(indicator: &str) -> &str {
    match indicator {
        "cpi" => "CPI",
        "unemployment_rate" => "Unemployment",
        "nfp" => "Nonfarm Payrolls",
        "pmi_manufacturing" => "PMI Manufacturing",
        "pmi_services" => "PMI Services",
        "fed_funds_rate" => "Fed Funds Rate",
        "initial_jobless_claims" => "Initial Jobless Claims",
        "ppi" => "PPI",
        _ => indicator,
    }
}

/// Return (unit, display_name) for an economy indicator.
/// Units help agents and users interpret raw values correctly.
fn indicator_metadata(indicator: &str) -> (&str, &str) {
    match indicator {
        "cpi" => ("% YoY", "CPI (YoY Inflation)"),
        "unemployment_rate" => ("%", "Unemployment Rate"),
        "nfp" => ("thousands", "Nonfarm Payrolls"),
        "pmi_manufacturing" => ("index (0-100)", "ISM Manufacturing PMI"),
        "pmi_services" => ("index (0-100)", "ISM Services PMI"),
        "fed_funds_rate" => ("%", "Federal Funds Rate"),
        "initial_jobless_claims" => ("claims", "Initial Jobless Claims"),
        "ppi" => ("% YoY", "PPI (Producer Prices)"),
        _ => ("", indicator),
    }
}

fn truncate_url(url: &str, max: usize) -> String {
    if url.len() <= max {
        return url.to_string();
    }
    format!("{}...", &url[..max.saturating_sub(3)])
}
