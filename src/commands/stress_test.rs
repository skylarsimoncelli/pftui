use std::collections::HashMap;

use anyhow::{anyhow, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::analytics::scenarios::{apply_preset, list_presets, parse_preset};
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::scenarios::list_scenarios_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::position::{compute_positions, compute_positions_from_allocations};

pub fn run(backend: &BackendConnection, config: &Config, scenario: &str, json: bool) -> Result<()> {
    let preset = parse_preset(scenario).ok_or_else(|| {
        anyhow!(
            "Unknown scenario '{}'. Try: Oil $100, BTC 40k, Gold $6000, 2008 GFC, 1973 Oil Crisis",
            scenario
        )
    })?;

    let prices: HashMap<String, Decimal> = get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();
    if prices.is_empty() {
        anyhow::bail!("No cached prices. Run `pftui refresh` first.");
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let base_positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions_backend(backend)?;
            compute_positions(&txs, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations_backend(backend)?;
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };

    let overrides = apply_preset(preset, &prices);
    let mut stressed_prices = prices.clone();
    for (sym, px) in &overrides {
        stressed_prices.insert(sym.clone(), *px);
    }

    let stressed_positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions_backend(backend)?;
            compute_positions(&txs, &stressed_prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations_backend(backend)?;
            compute_positions_from_allocations(&allocs, &stressed_prices, &fx_rates)
        }
    };

    let base_total: Decimal = base_positions.iter().filter_map(|p| p.current_value).sum();
    let stressed_total: Decimal = stressed_positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum();
    let delta = stressed_total - base_total;
    let delta_pct = if base_total > dec!(0) {
        (delta / base_total) * dec!(100)
    } else {
        dec!(0)
    };

    if json {
        let payload = serde_json::json!({
            "scenario": scenario,
            "base_total": base_total.to_string(),
            "stressed_total": stressed_total.to_string(),
            "delta": delta.to_string(),
            "delta_pct": delta_pct.round_dp(2).to_string(),
            "overrides": overrides.into_iter().map(|(k,v)| (k, v.to_string())).collect::<HashMap<_,_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("Stress Test: {}\n", scenario);
    println!("Base Total:     {:.2} {}", base_total, config.base_currency);
    println!(
        "Stressed Total: {:.2} {}",
        stressed_total, config.base_currency
    );
    println!(
        "Delta:          {:+.2} {} ({:+.2}%)",
        delta, config.base_currency, delta_pct
    );
    println!();
    println!("Overrides:");
    for (sym, px) in overrides {
        println!("  {} -> {:.2}", sym, px);
    }
    Ok(())
}

/// List all available stress-test scenarios: built-in presets + active user scenarios.
pub fn run_list(backend: &BackendConnection, json: bool) -> Result<()> {
    let presets = list_presets();
    let active_scenarios = list_scenarios_backend(backend, Some("active")).unwrap_or_default();

    if json {
        let preset_json: Vec<serde_json::Value> = presets
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "type": "preset",
                    "aliases": p.aliases,
                    "description": p.description,
                })
            })
            .collect();

        let scenario_json: Vec<serde_json::Value> = active_scenarios
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "type": "scenario",
                    "probability": s.probability,
                    "description": s.description,
                    "phase": s.phase,
                })
            })
            .collect();

        let payload = serde_json::json!({
            "presets": preset_json,
            "scenarios": scenario_json,
            "total": presets.len() + active_scenarios.len(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("Built-in Presets");
    println!("{}", "─".repeat(60));
    for p in &presets {
        println!("  {:20} {}", p.name, p.description);
        println!(
            "  {:20} aliases: {}",
            "",
            p.aliases.join(", ")
        );
    }

    if !active_scenarios.is_empty() {
        println!();
        println!("Active Scenarios (from analytics scenario list)");
        println!("{}", "─".repeat(60));
        for s in &active_scenarios {
            let desc = s.description.as_deref().unwrap_or("—");
            println!(
                "  {:20} prob: {:>5.1}%  {}",
                s.name, s.probability, desc
            );
        }
    }

    println!();
    println!(
        "Total: {} presets + {} active scenarios",
        presets.len(),
        active_scenarios.len()
    );
    Ok(())
}
