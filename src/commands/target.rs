use anyhow::{bail, Result};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db::backend::BackendConnection;
use crate::db::allocation_targets;

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    target_pct: &str,
    drift_band_pct: Option<&str>,
) -> Result<()> {
    let target = Decimal::from_str(target_pct.trim_end_matches('%'))
        .map_err(|_| anyhow::anyhow!("Invalid target percentage: {}", target_pct))?;

    if target < Decimal::ZERO || target > Decimal::from(100) {
        bail!("Target percentage must be between 0 and 100");
    }

    let drift_band = if let Some(band_str) = drift_band_pct {
        Decimal::from_str(band_str.trim_end_matches('%'))
            .map_err(|_| anyhow::anyhow!("Invalid drift band percentage: {}", band_str))?
    } else {
        Decimal::from(2) // default 2%
    };

    if drift_band < Decimal::ZERO || drift_band > Decimal::from(50) {
        bail!("Drift band must be between 0 and 50");
    }

    allocation_targets::set_target_backend(backend, symbol, target, drift_band)?;

    println!(
        "Set target for {} to {}% (drift band: ±{}%)",
        symbol, target, drift_band
    );

    Ok(())
}

pub fn list(backend: &BackendConnection, json: bool) -> Result<()> {
    let targets = allocation_targets::list_targets_backend(backend)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&targets)?);
    } else {
        if targets.is_empty() {
            println!("No allocation targets set.");
            return Ok(());
        }

        println!("{:<15} {:>10} {:>12}", "Symbol", "Target %", "Drift Band %");
        println!("{}", "-".repeat(40));
        for target in targets {
            println!(
                "{:<15} {:>10} {:>12}",
                target.symbol, target.target_pct, target.drift_band_pct
            );
        }
    }

    Ok(())
}

pub fn remove(backend: &BackendConnection, symbol: &str) -> Result<()> {
    allocation_targets::remove_target_backend(backend, symbol)?;
    println!("Removed target for {}", symbol);
    Ok(())
}
