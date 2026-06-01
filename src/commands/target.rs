use anyhow::{bail, Result};
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db::allocation_targets;
use crate::db::backend::BackendConnection;

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    floor_pct: Option<&str>,
    ceiling_pct: Option<&str>,
    target_pct: Option<&str>,
    drift_band_pct: Option<&str>,
) -> Result<()> {
    match (floor_pct, ceiling_pct, target_pct) {
        (Some(floor_str), Some(ceiling_str), None) => {
            if drift_band_pct.is_some() {
                bail!("Use either --floor/--ceiling or legacy --target/--band, not both");
            }
            let floor = parse_pct("floor", floor_str)?;
            let ceiling = parse_pct("ceiling", ceiling_str)?;
            validate_range(floor, ceiling)?;
            allocation_targets::set_target_range_backend(backend, symbol, floor, ceiling)?;
            let midpoint = (floor + ceiling) / Decimal::from(2);
            println!(
                "Set target range for {} to {}%-{}% (midpoint: {}%)",
                symbol, floor, ceiling, midpoint
            );
        }
        (None, None, Some(target_str)) => {
            let target = parse_pct("target", target_str)?;
            if target < Decimal::ZERO || target > Decimal::from(100) {
                bail!("Target percentage must be between 0 and 100");
            }

            let drift_band = if let Some(band_str) = drift_band_pct {
                parse_pct("drift band", band_str)?
            } else {
                Decimal::from(2) // default 2%
            };

            if drift_band < Decimal::ZERO || drift_band > Decimal::from(50) {
                bail!("Drift band must be between 0 and 50");
            }

            let floor = target - drift_band;
            let ceiling = target + drift_band;
            allocation_targets::set_target_backend(backend, symbol, target, drift_band)?;
            println!(
                "Set target range for {} to {}%-{}% (legacy target: {}% ±{}%)",
                symbol, floor, ceiling, target, drift_band
            );
        }
        (Some(_), None, _) | (None, Some(_), _) => {
            bail!("Both --floor and --ceiling are required for range targets");
        }
        (Some(_), Some(_), Some(_)) => {
            bail!("Use either --floor/--ceiling or legacy --target/--band, not both");
        }
        (None, None, None) => {
            bail!(
                "--floor and --ceiling are required for 'set' (legacy --target is still accepted)"
            );
        }
    }

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

        println!(
            "{:<15} {:>10} {:>10} {:>10} {:>10}",
            "Symbol", "Floor %", "Ceiling %", "Midpoint %", "Band %"
        );
        println!("{}", "-".repeat(62));
        for target in targets {
            println!(
                "{:<15} {:>10} {:>10} {:>10} {:>10}",
                target.symbol,
                target.target_floor_pct,
                target.target_ceiling_pct,
                target.target_pct,
                target.drift_band_pct
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

fn parse_pct(label: &str, value: &str) -> Result<Decimal> {
    Decimal::from_str(value.trim_end_matches('%'))
        .map_err(|_| anyhow::anyhow!("Invalid {} percentage: {}", label, value))
}

fn validate_range(floor: Decimal, ceiling: Decimal) -> Result<()> {
    if floor < Decimal::ZERO || floor > Decimal::from(100) {
        bail!("Floor percentage must be between 0 and 100");
    }
    if ceiling < Decimal::ZERO || ceiling > Decimal::from(100) {
        bail!("Ceiling percentage must be between 0 and 100");
    }
    if floor > ceiling {
        bail!("Floor percentage must be less than or equal to ceiling percentage");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    fn backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: open_in_memory(),
        }
    }

    #[test]
    fn range_set_stores_floor_and_ceiling() {
        let backend = backend();
        run(&backend, "GC=F", Some("22"), Some("30"), None, None).unwrap();

        let target = allocation_targets::get_target_backend(&backend, "GC=F")
            .unwrap()
            .unwrap();
        assert_eq!(target.target_floor_pct, dec!(22));
        assert_eq!(target.target_ceiling_pct, dec!(30));
        assert_eq!(target.target_pct, dec!(26));
        assert_eq!(target.drift_band_pct, dec!(4));
    }

    #[test]
    fn legacy_set_translates_to_equivalent_range() {
        let backend = backend();
        run(&backend, "GC=F", None, None, Some("25"), Some("3")).unwrap();

        let target = allocation_targets::get_target_backend(&backend, "GC=F")
            .unwrap()
            .unwrap();
        assert_eq!(target.target_floor_pct, dec!(22));
        assert_eq!(target.target_ceiling_pct, dec!(28));
        assert_eq!(target.target_pct, dec!(25));
        assert_eq!(target.drift_band_pct, dec!(3));
    }

    #[test]
    fn range_set_rejects_inverted_bounds() {
        let backend = backend();
        let err = run(&backend, "GC=F", Some("30"), Some("22"), None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Floor percentage"));
    }

    #[test]
    fn cash_symbols_accepted_with_wide_band() {
        // The target setter has no special-case for cash. Operators can set
        // wide bands on USD/GBP/EUR to model cash optionality.
        let backend = backend();
        run(&backend, "USD", Some("30"), Some("60"), None, None).unwrap();
        run(&backend, "GBP", Some("5"), Some("15"), None, None).unwrap();
        run(&backend, "EUR", Some("0"), Some("10"), None, None).unwrap();

        let usd = allocation_targets::get_target_backend(&backend, "USD")
            .unwrap()
            .unwrap();
        assert_eq!(usd.target_floor_pct, dec!(30));
        assert_eq!(usd.target_ceiling_pct, dec!(60));

        let gbp = allocation_targets::get_target_backend(&backend, "GBP")
            .unwrap()
            .unwrap();
        assert_eq!(gbp.target_floor_pct, dec!(5));
        assert_eq!(gbp.target_ceiling_pct, dec!(15));

        let eur = allocation_targets::get_target_backend(&backend, "EUR")
            .unwrap()
            .unwrap();
        assert_eq!(eur.target_floor_pct, dec!(0));
        assert_eq!(eur.target_ceiling_pct, dec!(10));
    }
}
