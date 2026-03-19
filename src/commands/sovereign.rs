use anyhow::Result;

use crate::data::sovereign::{self, CachedComexSilver};
use crate::db::backend::BackendConnection;
use crate::db::comex_cache::get_latest_inventory_backend;

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    // Load cached COMEX silver data as fallback for when live fetch fails
    let cached_silver = get_latest_inventory_backend(backend, "SI=F")
        .ok()
        .flatten()
        .map(|entry| CachedComexSilver {
            date: entry.date,
            registered_oz: entry.registered,
            eligible_oz: entry.eligible,
            total_oz: entry.total,
            registered_ratio_pct: entry.reg_ratio,
        });

    let snapshot = sovereign::fetch_snapshot(cached_silver)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(());
    }

    // Print warnings first if any
    for warning in &snapshot.warnings {
        eprintln!("  warning: {}", warning);
    }
    if !snapshot.warnings.is_empty() {
        eprintln!();
    }

    println!("\nSovereign Holdings Tracker\n");
    println!("  Fetched: {}", snapshot.fetched_at);
    if let Some(as_of) = &snapshot.cb_gold_as_of {
        println!("  WGC gold as-of: {}", as_of);
    }
    println!();

    if snapshot.cb_gold_tonnes.is_empty() {
        println!("  Central-bank gold: unavailable\n");
    } else {
        println!("  Central-bank gold (WGC, tonnes):");
        for row in snapshot.cb_gold_tonnes.iter().take(12) {
            println!("    {:<28} {:>10.2}", row.country, row.tonnes);
        }
        println!();
    }

    if snapshot.government_btc_holdings.is_empty() {
        println!("  Government BTC holdings: unavailable\n");
    } else {
        println!("  Government BTC holdings:");
        for row in snapshot.government_btc_holdings.iter().take(12) {
            println!("    {:<28} {:>10.0} BTC", row.name, row.btc);
        }
        println!();
    }

    println!("  COMEX silver (SI=F):");
    if snapshot.comex_silver.date == "unavailable" {
        println!("    No data available");
    } else {
        let registered = format_with_commas(snapshot.comex_silver.registered_oz);
        let eligible = format_with_commas(snapshot.comex_silver.eligible_oz);
        println!(
            "    Date {} | Registered {} oz | Eligible {} oz | Reg ratio {:.1}%",
            snapshot.comex_silver.date,
            registered,
            eligible,
            snapshot.comex_silver.registered_ratio_pct
        );
    }
    println!();

    println!("  Sources:");
    println!("    WGC API: {}", snapshot.source_urls.wgc_cbd_api);
    println!(
        "    Government BTC: {}",
        snapshot.source_urls.btc_governments_page
    );
    println!(
        "    COMEX silver: {}",
        snapshot.source_urls.comex_silver_xls
    );
    println!();

    Ok(())
}

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
