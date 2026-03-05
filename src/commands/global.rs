//! `pftui global` — Terminal-friendly global macro dashboard.
//!
//! Displays World Bank structural macro data for major economies.
//! Supports filtering by country and indicator, plus `--json` output.

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;

use crate::data::worldbank::{
    COUNTRY_BRAZIL, COUNTRY_CHINA, COUNTRY_EU, COUNTRY_INDIA, COUNTRY_RUSSIA,
    COUNTRY_SOUTH_AFRICA, COUNTRY_UK, COUNTRY_US, INDICATOR_CURRENT_ACCOUNT, INDICATOR_DEBT_GDP,
    INDICATOR_GDP_GROWTH, INDICATOR_RESERVES,
};
use crate::db::worldbank_cache;

/// Run the global macro dashboard command.
pub fn run(
    conn: &Connection,
    country_filter: Option<&str>,
    indicator_filter: Option<&str>,
    json: bool,
) -> Result<()> {
    // Load all cached World Bank data
    let all_data = worldbank_cache::get_latest_indicators(conn)?;

    // Apply filters
    let filtered_data: Vec<_> = all_data
        .into_iter()
        .filter(|d| {
            if let Some(c) = country_filter {
                d.country_code == c.to_uppercase()
            } else {
                true
            }
        })
        .filter(|d| {
            if let Some(i) = indicator_filter {
                let normalized = i.to_lowercase();
                match normalized.as_str() {
                    "gdp" => d.indicator_code == INDICATOR_GDP_GROWTH,
                    "debt" => d.indicator_code == INDICATOR_DEBT_GDP,
                    "current-account" | "currentaccount" => {
                        d.indicator_code == INDICATOR_CURRENT_ACCOUNT
                    }
                    "reserves" => d.indicator_code == INDICATOR_RESERVES,
                    code => d.indicator_code == code,
                }
            } else {
                true
            }
        })
        .collect();

    if json {
        print_json(&filtered_data)?;
    } else {
        print_terminal(&filtered_data)?;
    }

    Ok(())
}

// ─── JSON output ────────────────────────────────────────────────────────────

fn print_json(data: &[crate::data::worldbank::WorldBankDataPoint]) -> Result<()> {
    use serde_json::{json, Map, Value};

    // Group by country_code → indicator_code → value
    let mut countries_map: HashMap<String, Map<String, Value>> = HashMap::new();

    for point in data {
        let country_entry = countries_map
            .entry(point.country_code.clone())
            .or_default();

        let value_json = if let Some(val) = point.value {
            json!({
                "value": val.to_string(),
                "year": point.year,
                "indicator_name": point.indicator_name,
            })
        } else {
            json!({
                "value": null,
                "year": point.year,
                "indicator_name": point.indicator_name,
            })
        };

        country_entry.insert(point.indicator_code.clone(), value_json);
    }

    let output = json!({
        "global_macro": countries_map,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// ─── Terminal output ────────────────────────────────────────────────────────

fn print_terminal(data: &[crate::data::worldbank::WorldBankDataPoint]) -> Result<()> {
    if data.is_empty() {
        println!("No global macro data found. Run `pftui refresh` to populate cache.");
        return Ok(());
    }

    // Group by country
    let mut by_country: HashMap<String, Vec<&crate::data::worldbank::WorldBankDataPoint>> =
        HashMap::new();
    for point in data {
        by_country
            .entry(point.country_code.clone())
            .or_default()
            .push(point);
    }

    // Sort countries by display order
    let country_order = vec![
        COUNTRY_US,
        COUNTRY_EU,
        COUNTRY_UK,
        COUNTRY_CHINA,
        COUNTRY_INDIA,
        COUNTRY_RUSSIA,
        COUNTRY_BRAZIL,
        COUNTRY_SOUTH_AFRICA,
    ];

    println!("╭─ Global Macro (World Bank) ─────────────────────────────────────╮");
    println!("│");

    for country_code in country_order {
        if let Some(points) = by_country.get(country_code) {
            if points.is_empty() {
                continue;
            }

            let country_name = &points[0].country_name;
            println!("│ ┌─ {} ({}) ─────────────────", country_name, country_code);

            // Sort indicators by type
            let mut gdp_growth = None;
            let mut debt_gdp = None;
            let mut current_account = None;
            let mut reserves = None;

            for point in points {
                match point.indicator_code.as_str() {
                    INDICATOR_GDP_GROWTH => gdp_growth = Some(point),
                    INDICATOR_DEBT_GDP => debt_gdp = Some(point),
                    INDICATOR_CURRENT_ACCOUNT => current_account = Some(point),
                    INDICATOR_RESERVES => reserves = Some(point),
                    _ => {}
                }
            }

            if let Some(p) = gdp_growth {
                if let Some(val) = p.value {
                    println!(
                        "│ │  GDP Growth:        {:>8.2}%  ({})",
                        val, p.year
                    );
                }
            }

            if let Some(p) = debt_gdp {
                if let Some(val) = p.value {
                    println!(
                        "│ │  Debt/GDP:          {:>8.2}%  ({})",
                        val, p.year
                    );
                }
            }

            if let Some(p) = current_account {
                if let Some(val) = p.value {
                    let sign = if val.is_sign_positive() { "+" } else { "" };
                    println!(
                        "│ │  Current Account:   {:>8}{:.2}%  ({})",
                        sign, val, p.year
                    );
                }
            }

            if let Some(p) = reserves {
                if let Some(val) = p.value {
                    // Convert to trillions for readability
                    let trillions = val / rust_decimal::Decimal::new(1_000_000_000_000, 0);
                    println!(
                        "│ │  Reserves:          ${:>7.2}T  ({})",
                        trillions, p.year
                    );
                }
            }

            println!("│ │");
        }
    }

    println!("│");
    println!("╰──────────────────────────────────────────────────────────────────╯");

    Ok(())
}
