use anyhow::Result;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::db;
use crate::db::price_cache::get_all_cached_prices;
use crate::models::position::compute_positions;

pub fn run(db_path: &std::path::Path, json: bool) -> Result<()> {
    let conn = db::open_db(db_path)?;
    let targets = db::allocation_targets::list_targets(&conn)?;
    
    if targets.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No allocation targets set. Use `pftui target set <symbol> --target <pct>` to set targets.");
        }
        return Ok(());
    }

    let txs = db::transactions::list_transactions(&conn)?;
    
    let cached = get_all_cached_prices(&conn)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect();
    
    // Ensure cash assets price at 1.0
    for tx in &txs {
        if tx.category == crate::models::asset::AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }
    
    let positions = compute_positions(&txs, &prices);

    let target_map: HashMap<String, (Decimal, Decimal)> = targets
        .into_iter()
        .map(|t| (t.symbol.clone(), (t.target_pct, t.drift_band_pct)))
        .collect();

    let mut drift_data: Vec<DriftRow> = Vec::new();

    for pos in positions {
        if let Some((target_pct, drift_band)) = target_map.get(&pos.symbol) {
            let actual_pct = pos.allocation_pct.unwrap_or_default();
            let drift = actual_pct - target_pct;
            let abs_drift = drift.abs();
            let over_band = abs_drift > *drift_band;

            drift_data.push(DriftRow {
                symbol: pos.symbol.clone(),
                target_pct: *target_pct,
                actual_pct,
                drift,
                drift_band: *drift_band,
                over_band,
            });
        }
    }

    // Sort by absolute drift descending (biggest drifts first)
    drift_data.sort_by(|a, b| b.drift.abs().cmp(&a.drift.abs()));

    if json {
        // Format decimals to 4 decimal places for JSON output
        let formatted_data: Vec<DriftRowJson> = drift_data.iter().map(|row| {
            DriftRowJson {
                symbol: row.symbol.clone(),
                target_pct: format!("{:.4}", row.target_pct),
                actual_pct: format!("{:.4}", row.actual_pct),
                drift: format!("{:.4}", row.drift),
                drift_band: format!("{:.4}", row.drift_band),
                over_band: row.over_band,
            }
        }).collect();
        println!("{}", serde_json::to_string_pretty(&formatted_data)?);
    } else {
        println!(
            "{:<10} {:>10} {:>10} {:>10} {:>10}  Status",
            "Symbol", "Target %", "Actual %", "Drift %", "Band %"
        );
        println!("{}", "-".repeat(72));

        for row in &drift_data {
            let status = if row.over_band {
                "⚠️  OUT OF BAND"
            } else {
                "✓  In range"
            };

            // Format to 2 decimal places for display
            println!(
                "{:<10} {:>10.2} {:>10.2} {:>10.2} {:>10.2}  {}",
                row.symbol,
                row.target_pct,
                row.actual_pct,
                row.drift,
                row.drift_band,
                status
            );
        }

        let out_of_band_count = drift_data.iter().filter(|r| r.over_band).count();
        if out_of_band_count > 0 {
            println!("\n⚠️  {} position(s) outside drift band", out_of_band_count);
            println!("Run `pftui rebalance` to see suggested trades.");
        }
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct DriftRow {
    symbol: String,
    target_pct: Decimal,
    actual_pct: Decimal,
    drift: Decimal,
    drift_band: Decimal,
    over_band: bool,
}

#[derive(serde::Serialize)]
struct DriftRowJson {
    symbol: String,
    target_pct: String,
    actual_pct: String,
    drift: String,
    drift_band: String,
    over_band: bool,
}
