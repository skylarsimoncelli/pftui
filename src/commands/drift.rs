use anyhow::Result;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::db;
use crate::db::allocation_targets::{AllocationTarget, BandPosition};
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::models::position::{compute_positions, Position};

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let targets = db::allocation_targets::list_targets_backend(backend)?;

    if targets.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No allocation targets set. Use `pftui portfolio target set <symbol> --floor <pct> --ceiling <pct>` to set targets.");
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

    let mut drift_data = compute_drift_rows(&positions, &targets);

    // Sort by absolute drift descending (biggest drifts first)
    drift_data.sort_by_key(|b| std::cmp::Reverse(b.drift.abs()));

    if json {
        // Format percentages to 2 decimal places for JSON output.
        let formatted_data: Vec<DriftRowJson> = drift_data
            .iter()
            .map(|row| DriftRowJson {
                symbol: row.symbol.clone(),
                target_pct: round_decimal_2(row.target_pct),
                target_floor_pct: round_decimal_2(row.target_floor_pct),
                target_ceiling_pct: round_decimal_2(row.target_ceiling_pct),
                actual_pct: round_decimal_2(row.actual_pct),
                drift: round_decimal_2(row.drift),
                drift_band: round_decimal_2(row.drift_band),
                band_position: row.band_position.as_str().to_string(),
                over_band: row.over_band,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&formatted_data)?);
    } else {
        println!(
            "{:<10} {:>10} {:>10} {:>10} {:>10} {:>12}  Status",
            "Symbol", "Floor %", "Ceiling %", "Actual %", "Drift %", "Band"
        );
        println!("{}", "-".repeat(84));

        for row in &drift_data {
            let status = if row.over_band {
                "⚠️  OUT OF BAND"
            } else {
                "✓  In range"
            };

            // Format to 2 decimal places for display
            println!(
                "{:<10} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>12}  {}",
                row.symbol,
                row.target_floor_pct,
                row.target_ceiling_pct,
                row.actual_pct,
                row.drift,
                row.band_position.as_str(),
                status
            );
        }

        let out_of_band_count = drift_data.iter().filter(|r| r.over_band).count();
        if out_of_band_count > 0 {
            println!(
                "\n⚠️  {} position(s) outside target range",
                out_of_band_count
            );
            println!("Run `pftui rebalance` to see suggested trades.");
        }
    }

    Ok(())
}

/// Compute drift rows for every position with a matching allocation target.
///
/// Symbol-agnostic — cash symbols (USD, GBP, EUR, ...) are treated exactly
/// like any other symbol. The drift report includes a cash row whenever the
/// operator has set a target on that cash symbol, and is silent otherwise.
fn compute_drift_rows(positions: &[Position], targets: &[AllocationTarget]) -> Vec<DriftRow> {
    let target_map: HashMap<&str, &AllocationTarget> = targets
        .iter()
        .map(|t| (t.symbol.as_str(), t))
        .collect();

    let mut rows = Vec::new();
    for pos in positions {
        if let Some(target) = target_map.get(pos.symbol.as_str()) {
            let actual_pct = pos.allocation_pct.unwrap_or_default();
            let drift = target.drift_from_actual(actual_pct);
            let band_position = target.band_position(actual_pct);
            let over_band = band_position != BandPosition::InBand;

            rows.push(DriftRow {
                symbol: pos.symbol.clone(),
                target_pct: target.target_pct,
                target_floor_pct: target.target_floor_pct,
                target_ceiling_pct: target.target_ceiling_pct,
                actual_pct,
                drift,
                drift_band: target.drift_band_pct,
                band_position,
                over_band,
            });
        }
    }
    rows
}

#[derive(serde::Serialize)]
struct DriftRow {
    symbol: String,
    target_pct: Decimal,
    target_floor_pct: Decimal,
    target_ceiling_pct: Decimal,
    actual_pct: Decimal,
    drift: Decimal,
    drift_band: Decimal,
    band_position: BandPosition,
    over_band: bool,
}

#[derive(serde::Serialize)]
struct DriftRowJson {
    symbol: String,
    target_pct: f64,
    target_floor_pct: f64,
    target_ceiling_pct: f64,
    actual_pct: f64,
    drift: f64,
    drift_band: f64,
    band_position: String,
    over_band: bool,
}

fn round_decimal_2(value: Decimal) -> f64 {
    let rounded = value.round_dp(2);
    rounded.to_string().parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::asset::AssetCategory;
    use rust_decimal_macros::dec;

    fn target() -> AllocationTarget {
        AllocationTarget {
            symbol: "GC=F".to_string(),
            target_pct: dec!(26),
            drift_band_pct: dec!(4),
            target_floor_pct: dec!(22),
            target_ceiling_pct: dec!(30),
            updated_at: "test".to_string(),
        }
    }

    fn cash_target(symbol: &str, floor: Decimal, ceiling: Decimal) -> AllocationTarget {
        AllocationTarget {
            symbol: symbol.to_string(),
            target_pct: (floor + ceiling) / Decimal::from(2),
            drift_band_pct: (ceiling - floor) / Decimal::from(2),
            target_floor_pct: floor,
            target_ceiling_pct: ceiling,
            updated_at: "test".to_string(),
        }
    }

    fn position(symbol: &str, category: AssetCategory, allocation_pct: Decimal) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: dec!(0),
            avg_cost: dec!(0),
            total_cost: dec!(0),
            currency: "USD".to_string(),
            current_price: None,
            current_value: None,
            gain: None,
            gain_pct: None,
            allocation_pct: Some(allocation_pct),
            native_currency: None,
            fx_rate: None,
        }
    }

    #[test]
    fn in_band_actual_has_zero_drift() {
        let target = target();
        assert_eq!(target.drift_from_actual(dec!(27)), Decimal::ZERO);
        assert_eq!(target.band_position(dec!(27)), BandPosition::InBand);
    }

    #[test]
    fn below_floor_actual_has_negative_edge_drift() {
        let target = target();
        assert_eq!(target.drift_from_actual(dec!(20)), dec!(-2));
        assert_eq!(target.band_position(dec!(20)), BandPosition::BelowFloor);
    }

    #[test]
    fn above_ceiling_actual_has_positive_edge_drift() {
        let target = target();
        assert_eq!(target.drift_from_actual(dec!(33)), dec!(3));
        assert_eq!(target.band_position(dec!(33)), BandPosition::AboveCeiling);
    }

    #[test]
    fn drift_rows_include_cash_position_when_target_exists() {
        // USD has a wide 30-60 band and sits at 25% — below floor.
        // GC=F has a 22-30 band and sits at 28% — in band, so no row.
        // BTC has no target — never appears.
        let positions = vec![
            position("USD", AssetCategory::Cash, dec!(25)),
            position("GC=F", AssetCategory::Commodity, dec!(28)),
            position("BTC", AssetCategory::Crypto, dec!(45)),
        ];
        let targets = vec![
            cash_target("USD", dec!(30), dec!(60)),
            cash_target("GC=F", dec!(22), dec!(30)),
        ];

        let rows = compute_drift_rows(&positions, &targets);
        let symbols: Vec<&str> = rows.iter().map(|r| r.symbol.as_str()).collect();
        assert!(symbols.contains(&"USD"), "cash row missing: {symbols:?}");
        assert!(symbols.contains(&"GC=F"));
        assert!(!symbols.contains(&"BTC"));

        let usd_row = rows.iter().find(|r| r.symbol == "USD").unwrap();
        assert_eq!(usd_row.actual_pct, dec!(25));
        assert_eq!(usd_row.target_floor_pct, dec!(30));
        assert_eq!(usd_row.target_ceiling_pct, dec!(60));
        assert_eq!(usd_row.drift, dec!(-5));
        assert_eq!(usd_row.band_position, BandPosition::BelowFloor);
        assert!(usd_row.over_band);
    }

    #[test]
    fn drift_rows_silent_for_cash_when_no_target_exists() {
        // Cash without a target stays silent — no auto-seeded default.
        let positions = vec![
            position("USD", AssetCategory::Cash, dec!(45)),
            position("GC=F", AssetCategory::Commodity, dec!(35)),
        ];
        let targets = vec![cash_target("GC=F", dec!(22), dec!(30))];

        let rows = compute_drift_rows(&positions, &targets);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "GC=F");
    }

    #[test]
    fn drift_rows_in_band_cash_emits_zero_drift_row() {
        // Cash at 45% with a 30-60 band is in-band: row is emitted with
        // drift == 0 and over_band == false (consistent with non-cash assets).
        let positions = vec![position("USD", AssetCategory::Cash, dec!(45))];
        let targets = vec![cash_target("USD", dec!(30), dec!(60))];

        let rows = compute_drift_rows(&positions, &targets);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "USD");
        assert_eq!(rows[0].drift, Decimal::ZERO);
        assert_eq!(rows[0].band_position, BandPosition::InBand);
        assert!(!rows[0].over_band);
    }
}
