use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::models::position::compute_positions;

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let targets = db::allocation_targets::list_targets_backend(backend)?;
    
    if targets.is_empty() {
        if json {
            println!("{{\"error\": \"No allocation targets set\"}}");
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
    
    let fx_rates = backend
        .sqlite_native()
        .map(|conn| crate::db::fx_cache::get_all_fx_rates(conn).unwrap_or_default())
        .unwrap_or_default();
    let positions = compute_positions(&txs, &prices, &fx_rates);

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    
    if total_value <= Decimal::ZERO {
        if json {
            println!("{{\"error\": \"Portfolio value is zero\"}}");
        } else {
            println!("Portfolio value is zero. Cannot compute rebalance.");
        }
        return Ok(());
    }

    let target_map: HashMap<String, (Decimal, Decimal)> = targets
        .into_iter()
        .map(|t| (t.symbol.clone(), (t.target_pct, t.drift_band_pct)))
        .collect();

    let mut rebalance_actions: Vec<RebalanceAction> = Vec::new();

    for pos in positions {
        if let Some((target_pct, drift_band)) = target_map.get(&pos.symbol) {
            let actual_pct = pos.allocation_pct.unwrap_or_default();
            let drift = actual_pct - target_pct;
            let abs_drift = drift.abs();

            if abs_drift > *drift_band {
                // Outside drift band — compute suggested trade
                let target_value = total_value * target_pct / dec!(100);
                let current_value = pos.current_value.unwrap_or_default();
                let diff_value = target_value - current_value;
                let action_type = if diff_value > Decimal::ZERO {
                    "BUY"
                } else {
                    "SELL"
                };

                rebalance_actions.push(RebalanceAction {
                    symbol: pos.symbol.clone(),
                    current_value,
                    target_value,
                    diff_value: diff_value.abs(),
                    action: action_type.to_string(),
                    target_pct: *target_pct,
                    actual_pct,
                    drift,
                });
            }
        }
    }

    // Sort by absolute diff_value descending (biggest trades first)
    rebalance_actions.sort_by(|a, b| b.diff_value.cmp(&a.diff_value));

    if json {
        println!("{}", serde_json::to_string_pretty(&rebalance_actions)?);
    } else {
        if rebalance_actions.is_empty() {
            println!("✓ Portfolio is balanced. All positions within drift bands.");
            return Ok(());
        }

        println!(
            "{:<10} {:>10} {:>12} {:>12} {:>12} {:>10}",
            "Symbol", "Action", "Current $", "Target $", "Diff $", "Drift %"
        );
        println!("{}", "-".repeat(78));

        for action in &rebalance_actions {
            println!(
                "{:<10} {:>10} {:>12.2} {:>12.2} {:>12.2} {:>10.2}",
                action.symbol,
                action.action,
                action.current_value,
                action.target_value,
                action.diff_value,
                action.drift
            );
        }

        let total_to_move: Decimal = rebalance_actions.iter().map(|a| a.diff_value).sum();
        println!("\nTotal capital to rebalance: ${:.2}", total_to_move / dec!(2)); // Divide by 2 because buy+sell are double-counted
        println!("Portfolio value: ${:.2}", total_value);
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct RebalanceAction {
    symbol: String,
    current_value: Decimal,
    target_value: Decimal,
    diff_value: Decimal,
    action: String,
    target_pct: Decimal,
    actual_pct: Decimal,
    drift: Decimal,
}
