use anyhow::{bail, Result};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db;

pub fn run(
    db_path: &std::path::Path,
    symbol: &str,
    target_pct: &str,
    drift_band_pct: Option<&str>,
) -> Result<()> {
    let conn = db::open_db(db_path)?;

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

    db::allocation_targets::set_target(&conn, symbol, target, drift_band)?;

    println!(
        "Set target for {} to {}% (drift band: ±{}%)",
        symbol, target, drift_band
    );

    Ok(())
}

pub fn list(db_path: &std::path::Path, json: bool) -> Result<()> {
    let conn = db::open_db(db_path)?;
    let targets = db::allocation_targets::list_targets(&conn)?;

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

pub fn remove(db_path: &std::path::Path, symbol: &str) -> Result<()> {
    let conn = db::open_db(db_path)?;
    db::allocation_targets::remove_target(&conn, symbol)?;
    println!("Removed target for {}", symbol);
    Ok(())
}
