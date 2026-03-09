use anyhow::Result;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::models::position::compute_positions;

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let targets = db::allocation_targets::list_targets_backend(backend)?;
    
    if targets.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No allocation targets set. Use `pftui target set <symbol> --target <pct>` to set targets.");
        }
        return Ok(());
    }

    let txs = db::transactions::list_transactions_backend(backend)?;
    
    let cached = get_all_cached_prices_backend(backend)?;
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
    
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions(&txs, &prices, &fx_rates);

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
        // Format percentages to 2 decimal places for JSON output.
        let formatted_data: Vec<DriftRowJson> = drift_data.iter().map(|row| {
            DriftRowJson {
                symbol: row.symbol.clone(),
                target_pct: round_decimal_2(row.target_pct),
                actual_pct: round_decimal_2(row.actual_pct),
                drift: round_decimal_2(row.drift),
                drift_band: round_decimal_2(row.drift_band),
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
    target_pct: f64,
    actual_pct: f64,
    drift: f64,
    drift_band: f64,
    over_band: bool,
}

fn round_decimal_2(value: Decimal) -> f64 {
    let rounded = value.round_dp(2);
    rounded.to_string().parse::<f64>().unwrap_or(0.0)
}
