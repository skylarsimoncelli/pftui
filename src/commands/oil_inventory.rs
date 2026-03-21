//! `pftui data oil-inventory` command — EIA crude oil inventory & SPR data.
//!
//! Displays weekly petroleum status report data from the U.S. Energy Information
//! Administration: commercial crude inventories, SPR levels, total stocks,
//! weekly changes, and deviation from historical averages.

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::data::eia::{self, EIA_SERIES};

#[derive(Debug, Serialize)]
struct InventoryRow {
    series: String,
    name: String,
    period: String,
    value_kb: f64,
    value_mb: f64,
    weekly_change_kb: Option<f64>,
    weekly_change_mb: Option<f64>,
    five_year_avg_kb: Option<f64>,
    deviation_from_avg_kb: Option<f64>,
    deviation_from_avg_pct: Option<f64>,
    unit: String,
}

pub fn run(config: &Config, weeks: usize, json: bool) -> Result<()> {
    let api_key = config
        .eia_api_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No EIA API key configured. Register (free) at https://www.eia.gov/opendata/register.php \
                 then run: pftui system config set eia_api_key YOUR_KEY"
            )
        })?;

    let limit = weeks.max(2); // need at least 2 for weekly change

    let mut rows = Vec::new();

    for series_meta in EIA_SERIES {
        match fetch_row(api_key, series_meta, limit) {
            Ok(row) => rows.push(row),
            Err(e) => {
                if !json {
                    eprintln!("Warning: Failed to fetch {}: {}", series_meta.name, e);
                }
            }
        }
    }

    if json {
        let payload: serde_json::Map<String, serde_json::Value> = rows
            .iter()
            .map(|row| {
                let key = normalize_key(&row.name);
                let val = serde_json::json!({
                    "series_id": row.series,
                    "period": row.period,
                    "value_thousand_barrels": row.value_kb,
                    "value_million_barrels": row.value_mb,
                    "weekly_change_thousand_barrels": row.weekly_change_kb,
                    "weekly_change_million_barrels": row.weekly_change_mb,
                    "five_year_avg_thousand_barrels": row.five_year_avg_kb,
                    "deviation_from_avg_thousand_barrels": row.deviation_from_avg_kb,
                    "deviation_from_avg_pct": row.deviation_from_avg_pct,
                    "unit": row.unit,
                });
                (key, val)
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if rows.is_empty() {
        println!("No EIA inventory data available. Check your API key and network.");
    } else {
        print_table(&rows);
    }

    Ok(())
}

fn fetch_row(api_key: &str, series_meta: &eia::EiaSeries, limit: usize) -> Result<InventoryRow> {
    let observations = eia::fetch_series(api_key, series_meta.series_id, limit)?;

    let latest = observations
        .first()
        .ok_or_else(|| anyhow::anyhow!("No data for {}", series_meta.series_id))?;

    let change = eia::weekly_change(&observations);
    let avg = eia::five_year_average(&observations);

    let dev_kb = avg.map(|a| eia::deviation_from_avg(latest.value, a));
    let dev_pct = avg.map(|a| eia::deviation_pct(latest.value, a));

    Ok(InventoryRow {
        series: series_meta.series_id.to_string(),
        name: series_meta.name.to_string(),
        period: latest.period.clone(),
        value_kb: round2(latest.value),
        value_mb: round2(latest.value / 1_000.0),
        weekly_change_kb: change.map(round2),
        weekly_change_mb: change.map(|c| round2(c / 1_000.0)),
        five_year_avg_kb: avg.map(round2),
        deviation_from_avg_kb: dev_kb.map(round2),
        deviation_from_avg_pct: dev_pct.map(round1),
        unit: series_meta.unit.to_string(),
    })
}

fn print_table(rows: &[InventoryRow]) {
    println!(
        "\n\
        EIA Weekly Petroleum Status Report\n\
        ══════════════════════════════════════════════════════════════════════\n"
    );

    for row in rows {
        println!("  {} ({})", row.name, row.series);
        println!("    Report date:   {}", row.period);
        println!(
            "    Level:         {} kb  ({:.1} million barrels)",
            format_with_commas(row.value_kb),
            row.value_mb
        );

        if let Some(change) = row.weekly_change_kb {
            println!(
                "    Weekly change: {} kb  ({} mb)",
                format_signed(change),
                format_signed(change / 1_000.0)
            );
        }

        if let Some(avg) = row.five_year_avg_kb {
            println!("    5-year avg:    {} kb", format_with_commas(avg));
        }

        if let (Some(dev), Some(pct)) = (row.deviation_from_avg_kb, row.deviation_from_avg_pct) {
            println!(
                "    vs 5Y avg:     {} kb  ({}{:.1}%)",
                format_signed(dev),
                if pct >= 0.0 { "+" } else { "" },
                pct
            );
        }

        println!();
    }
}

fn normalize_key(name: &str) -> String {
    name.to_lowercase().replace([' ', '-'], "_")
}

fn format_with_commas(n: f64) -> String {
    let n_str = format!("{:.0}", n);
    let negative = n_str.starts_with('-');
    let digits: String = n_str.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut result = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

fn format_signed(value: f64) -> String {
    if value >= 0.0 {
        format!("+{}", format_with_commas(value))
    } else {
        format_with_commas(value)
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_key_produces_snake_case() {
        assert_eq!(
            normalize_key("Commercial Crude Inventories"),
            "commercial_crude_inventories"
        );
        assert_eq!(
            normalize_key("Strategic Petroleum Reserve"),
            "strategic_petroleum_reserve"
        );
    }

    #[test]
    fn format_with_commas_works() {
        assert_eq!(format_with_commas(440123.0), "440,123");
        assert_eq!(format_with_commas(1234.0), "1,234");
        assert_eq!(format_with_commas(999.0), "999");
    }

    #[test]
    fn format_signed_works() {
        assert_eq!(format_signed(2000.0), "+2,000");
        assert_eq!(format_signed(-1500.0), "-1,500");
    }
}
