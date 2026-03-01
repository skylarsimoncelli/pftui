use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::price_cache::get_all_cached_prices;
use crate::db::transactions::list_transactions;
use crate::models::position::{compute_positions, compute_positions_from_allocations};

pub fn run(conn: &Connection, config: &Config) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, &prices),
        PortfolioMode::Percentage => run_percentage(conn, &prices),
    }
}

fn run_full(conn: &Connection, config: &Config, prices: &HashMap<String, Decimal>) -> Result<()> {
    let txs = list_transactions(conn)?;
    if txs.is_empty() {
        println!("No transactions found. Add one with: pftui add-tx");
        return Ok(());
    }

    let positions = compute_positions(&txs, prices);
    if positions.is_empty() {
        println!("No open positions.");
        return Ok(());
    }

    println!(
        "{:<8} {:<10} {:>8} {:>10} {:>10} {:>8} {:>8}",
        "Symbol", "Category", "Qty", "Cost", "Price", "Gain%", "Alloc%"
    );
    println!("{}", "-".repeat(70));

    let mut total_value = dec!(0);
    let mut total_cost = dec!(0);

    for pos in &positions {
        let price_str = pos.current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());
        let gain_str = pos.gain_pct
            .map(|g| format!("{:+.1}%", g))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos.allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>8} {:>10.2} {:>10} {:>8} {:>8}",
            pos.symbol,
            pos.category,
            pos.quantity,
            pos.avg_cost,
            price_str,
            gain_str,
            alloc_str,
        );

        if let Some(v) = pos.current_value {
            total_value += v;
        }
        total_cost += pos.total_cost;
    }

    println!("{}", "-".repeat(70));
    let total_gain = total_value - total_cost;
    let total_gain_pct = if total_cost > dec!(0) {
        (total_gain / total_cost) * dec!(100)
    } else {
        dec!(0)
    };

    println!(
        "Total Value: {:.2} {}  |  Cost: {:.2}  |  Gain: {:+.2} ({:+.1}%)",
        total_value, config.base_currency, total_cost, total_gain, total_gain_pct
    );

    if prices.is_empty() {
        println!("\nNote: No cached prices. Run `pftui` (TUI) to fetch live prices.");
    }

    Ok(())
}

fn run_percentage(conn: &Connection, prices: &HashMap<String, Decimal>) -> Result<()> {
    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        println!("No allocations found. Run: pftui setup");
        return Ok(());
    }

    let positions = compute_positions_from_allocations(&allocs, prices);

    println!(
        "{:<8} {:<10} {:>10} {:>8}",
        "Symbol", "Category", "Price", "Alloc%"
    );
    println!("{}", "-".repeat(40));

    for pos in &positions {
        let price_str = pos.current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos.allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>10} {:>8}",
            pos.symbol, pos.category, price_str, alloc_str,
        );
    }

    Ok(())
}
