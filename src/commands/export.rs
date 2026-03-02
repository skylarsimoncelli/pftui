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

/// Round a Decimal to 2 decimal places for human-readable CSV output.
fn round2(d: Decimal) -> String {
    d.round_dp(2).to_string()
}

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
                    &pos.avg_cost.round_dp(2).to_string(),
                    &pos.total_cost.round_dp(2).to_string(),
                    &pos.current_price.map(round2).unwrap_or_default(),
                    &pos.current_value.map(round2).unwrap_or_default(),
                    &pos.gain.map(round2).unwrap_or_default(),
                    &pos.gain_pct.map(round2).unwrap_or_default(),
                    &pos.allocation_pct.map(round2).unwrap_or_default(),
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
                    &pos.current_price.map(round2).unwrap_or_default(),
                    &pos.allocation_pct.map(round2).unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn round2_basic() {
        assert_eq!(round2(dec!(33.333333333333333333333333333)), "33.33");
    }

    #[test]
    fn round2_rounds_up() {
        assert_eq!(round2(dec!(49.999)), "50.00");
    }

    #[test]
    fn round2_whole_number() {
        assert_eq!(round2(dec!(100)), "100");
    }

    #[test]
    fn round2_small() {
        assert_eq!(round2(dec!(0.006)), "0.01");
    }

    #[test]
    fn round2_negative() {
        assert_eq!(round2(dec!(-12.3456)), "-12.35");
    }
}
