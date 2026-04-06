//! `pftui supply` command — COMEX warehouse inventory data.
//!
//! Displays registered and eligible stocks for gold and silver from CME Group.
//! Data refreshes daily after market close (~5pm ET).

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::data::comex::{fetch_inventory, ComexInventory, COMEX_METALS};
use crate::db::backend::BackendConnection;
use crate::db::comex_cache::{
    get_latest_inventory_backend, upsert_inventory_backend, ComexCacheEntry,
};

#[derive(Serialize)]
struct SupplyOutput {
    symbol: String,
    metal: String,
    date: String,
    registered: f64,
    eligible: f64,
    total: f64,
    reg_ratio: f64,
    unit: String,
}

/// Run `pftui supply` command.
///
/// Fetches or displays cached COMEX inventory data.
/// Symbols: GC=F (gold), SI=F (silver).
pub fn run(backend: &BackendConnection, symbol: Option<String>, json: bool) -> Result<()> {
    if let Some(sym) = symbol {
        // Fetch or display single metal
        display_metal(backend, &sym, json)?;
    } else {
        // Fetch or display all metals
        display_all(backend, json)?;
    }

    Ok(())
}

/// Display inventory for all tracked metals.
fn display_all(backend: &BackendConnection, json: bool) -> Result<()> {
    let mut outputs = Vec::new();

    for metal_meta in COMEX_METALS {
        let (output, warning) = get_or_fetch_inventory(backend, metal_meta.symbol)?;
        if let Some(warning) = warning {
            eprintln!("Warning: {warning}");
        }
        match output {
            Some(data) => outputs.push(data),
            None => {
                if !json {
                    eprintln!("Warning: No data available for {}", metal_meta.metal);
                }
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&outputs)?);
    } else {
        if outputs.is_empty() {
            println!("No COMEX inventory data available.");
            return Ok(());
        }

        println!("╔════════════════════════════════════════════════════════════════╗");
        println!("║           COMEX Warehouse Inventory — CME Group            ║");
        println!("╚════════════════════════════════════════════════════════════════╝\n");

        for output in outputs {
            print_inventory(&output);
            println!();
        }
    }

    Ok(())
}

/// Display inventory for a single metal.
fn display_metal(backend: &BackendConnection, symbol: &str, json: bool) -> Result<()> {
    // Validate symbol
    if !COMEX_METALS.iter().any(|m| m.symbol == symbol) {
        let valid: Vec<_> = COMEX_METALS.iter().map(|m| m.symbol).collect();
        anyhow::bail!(
            "Invalid symbol '{}'. Supported: {}",
            symbol,
            valid.join(", ")
        );
    }

    let (output, warning) = get_or_fetch_inventory(backend, symbol)?;
    if let Some(warning) = warning {
        eprintln!("Warning: {warning}");
    }

    if let Some(data) = output {
        if json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            println!("╔════════════════════════════════════════════════════════════════╗");
            println!("║           COMEX Warehouse Inventory — CME Group            ║");
            println!("╚════════════════════════════════════════════════════════════════╝\n");
            print_inventory(&data);
        }
    } else if json {
        println!("null");
    } else {
        println!("No COMEX data available for {}", symbol);
    }

    Ok(())
}

/// Get cached inventory or fetch fresh if stale.
///
/// Cache policy: refresh if data is >24 hours old.
/// Parse a timestamp string flexibly (RFC3339, Postgres-style, naive).
fn parse_timestamp_flexible(raw: &str) -> Option<chrono::DateTime<Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z")
        .or_else(|_| chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z"))
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, Utc))
        })
}

fn get_or_fetch_inventory(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<(Option<SupplyOutput>, Option<String>)> {
    get_or_fetch_inventory_with(backend, symbol, fetch_inventory)
}

fn get_or_fetch_inventory_with<F>(
    backend: &BackendConnection,
    symbol: &str,
    fetcher: F,
) -> Result<(Option<SupplyOutput>, Option<String>)>
where
    F: Fn(&str) -> Result<ComexInventory>,
{
    // Try cache first
    let cached = get_latest_inventory_backend(backend, symbol)?;
    if let Some(ref cached_entry) = cached {
        // Parse fetched_at timestamp
        let fetched_at = parse_timestamp_flexible(&cached_entry.fetched_at)
            .unwrap_or_else(Utc::now);

        let age = Utc::now().signed_duration_since(fetched_at);

        // Use cache if <24 hours old
        if age < chrono::Duration::hours(24) {
            return Ok((Some(supply_output_from_cache(cached_entry)), None));
        }
    }

    // Cache miss or stale — fetch fresh data
    match fetcher(symbol) {
        Ok(inv) => {
            // Cache the fresh data
            let entry = ComexCacheEntry {
                symbol: inv.symbol.clone(),
                date: inv.date.clone(),
                registered: inv.registered,
                eligible: inv.eligible,
                total: inv.total,
                reg_ratio: inv.reg_ratio,
                fetched_at: Utc::now().to_rfc3339(),
            };

            upsert_inventory_backend(backend, &entry)?;

            Ok((Some(SupplyOutput {
                symbol: inv.symbol,
                metal: metal_metadata(symbol).metal.to_string(),
                date: inv.date,
                registered: inv.registered,
                eligible: inv.eligible,
                total: inv.total,
                reg_ratio: inv.reg_ratio,
                unit: metal_metadata(symbol).unit.to_string(),
            }), None))
        }
        Err(e) => {
            if let Some(cached_entry) = cached {
                Ok((
                    Some(supply_output_from_cache(&cached_entry)),
                    Some(format!(
                        "Failed to fetch {} inventory live ({}). Returning stale cached data from {}.",
                        symbol, e, cached_entry.date
                    )),
                ))
            } else {
                Ok((
                    None,
                    Some(format!("Failed to fetch {} inventory live: {}", symbol, e)),
                ))
            }
        }
    }
}

fn metal_metadata(symbol: &str) -> &'static crate::data::comex::ComexMetal {
    COMEX_METALS
        .iter()
        .find(|m| m.symbol == symbol)
        .unwrap_or(&COMEX_METALS[0])
}

fn supply_output_from_cache(cached: &ComexCacheEntry) -> SupplyOutput {
    SupplyOutput {
        symbol: cached.symbol.clone(),
        metal: metal_metadata(&cached.symbol).metal.to_string(),
        date: cached.date.clone(),
        registered: cached.registered,
        eligible: cached.eligible,
        total: cached.total,
        reg_ratio: cached.reg_ratio,
        unit: metal_metadata(&cached.symbol).unit.to_string(),
    }
}

/// Print a single inventory record (non-JSON).
fn print_inventory(inv: &SupplyOutput) {
    println!("{}  ({})", inv.metal, inv.symbol);
    println!("  Date:           {}", inv.date);
    println!(
        "  Registered:     {} {}",
        format_with_commas(inv.registered),
        inv.unit
    );
    println!(
        "  Eligible:       {} {}",
        format_with_commas(inv.eligible),
        inv.unit
    );
    println!(
        "  Total:          {} {}",
        format_with_commas(inv.total),
        inv.unit
    );
    println!("  Reg Ratio:      {:.1}%", inv.reg_ratio);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_cached_inventory_is_returned_when_live_fetch_fails() {
        let backend = BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };

        upsert_inventory_backend(
            &backend,
            &ComexCacheEntry {
                symbol: "GC=F".to_string(),
                date: "2026-04-01".to_string(),
                registered: 100.0,
                eligible: 200.0,
                total: 300.0,
                reg_ratio: 33.3,
                fetched_at: "2026-04-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let (output, warning) =
            get_or_fetch_inventory_with(&backend, "GC=F", |_| anyhow::bail!("network down"))
                .unwrap();

        let output = output.expect("expected stale cached output");
        assert_eq!(output.symbol, "GC=F");
        assert_eq!(output.date, "2026-04-01");
        assert!(warning.unwrap().contains("Returning stale cached data"));
    }
}

/// Format a number with thousands separators.
fn format_with_commas(n: f64) -> String {
    let n_str = format!("{:.0}", n);
    let mut result = String::new();
    let chars: Vec<char> = n_str.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*ch);
    }
    result
}
