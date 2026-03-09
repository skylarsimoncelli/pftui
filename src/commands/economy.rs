use anyhow::Result;

use crate::db::backend::BackendConnection;
use crate::db::economic_data;

pub fn run(backend: &BackendConnection, indicator: Option<&str>, json: bool) -> Result<()> {
    let mut rows = economic_data::get_all_backend(backend)?;
    if let Some(ind) = indicator {
        let needle = ind.to_lowercase();
        rows.retain(|r| r.indicator.to_lowercase() == needle);
    }

    if json {
        let payload: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "indicator": r.indicator,
                    "value": r.value.to_string(),
                    "previous": r.previous.map(|v| v.to_string()),
                    "change": r.change.map(|v| v.to_string()),
                    "source_url": r.source_url,
                    "fetched_at": r.fetched_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
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

fn truncate_url(url: &str, max: usize) -> String {
    if url.len() <= max {
        return url.to_string();
    }
    format!("{}...", &url[..max.saturating_sub(3)])
}
