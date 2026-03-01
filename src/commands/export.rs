use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::Connection;

use crate::cli::ExportFormat;
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::price_cache::get_all_cached_prices;
use crate::db::transactions::list_transactions;
use crate::models::position::{compute_positions, compute_positions_from_allocations};

pub fn run(conn: &Connection, format: &ExportFormat, config: &Config) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions(conn)?;
            compute_positions(&txs, &prices)
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations(conn)?;
            compute_positions_from_allocations(&allocs, &prices)
        }
    };

    match config.portfolio_mode {
        PortfolioMode::Full => export_full(&positions, format),
        PortfolioMode::Percentage => export_percentage(&positions, format),
    }
}

fn export_full(
    positions: &[crate::models::position::Position],
    format: &ExportFormat,
) -> Result<()> {
    match format {
        ExportFormat::Json => {
            let json = serde_json::to_string_pretty(&positions)?;
            println!("{}", json);
        }
        ExportFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            wtr.write_record([
                "symbol", "category", "quantity", "avg_cost", "total_cost",
                "current_price", "current_value", "gain", "gain_pct", "allocation_pct",
            ])?;
            for pos in positions {
                wtr.write_record([
                    &pos.symbol,
                    &pos.category.to_string(),
                    &pos.quantity.to_string(),
                    &pos.avg_cost.to_string(),
                    &pos.total_cost.to_string(),
                    &pos.current_price.map(|p| p.to_string()).unwrap_or_default(),
                    &pos.current_value.map(|v| v.to_string()).unwrap_or_default(),
                    &pos.gain.map(|g| g.to_string()).unwrap_or_default(),
                    &pos.gain_pct.map(|g| g.to_string()).unwrap_or_default(),
                    &pos.allocation_pct.map(|a| a.to_string()).unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;
        }
    }
    Ok(())
}

fn export_percentage(
    positions: &[crate::models::position::Position],
    format: &ExportFormat,
) -> Result<()> {
    match format {
        ExportFormat::Json => {
            // Export reduced fields for percentage mode
            let reduced: Vec<serde_json::Value> = positions
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "symbol": p.symbol,
                        "category": p.category.to_string(),
                        "current_price": p.current_price.map(|v| v.to_string()),
                        "allocation_pct": p.allocation_pct.map(|v| v.to_string()),
                    })
                })
                .collect();
            let json = serde_json::to_string_pretty(&reduced)?;
            println!("{}", json);
        }
        ExportFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            wtr.write_record(["symbol", "category", "current_price", "allocation_pct"])?;
            for pos in positions {
                wtr.write_record([
                    &pos.symbol,
                    &pos.category.to_string(),
                    &pos.current_price.map(|p| p.to_string()).unwrap_or_default(),
                    &pos.allocation_pct.map(|a| a.to_string()).unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;
        }
    }
    Ok(())
}
