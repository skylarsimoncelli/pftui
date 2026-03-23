//! `pftui portfolio allocation` — quick allocation snapshot.
//!
//! Shows each position's allocation percentage, optionally grouped by category.
//! Lighter than `portfolio summary` — no technicals, no gains, no what-if.

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use crate::cli::SummaryGroupBy;
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::compute_positions;

pub fn run(
    backend: &BackendConnection,
    group_by: Option<&SummaryGroupBy>,
    json: bool,
) -> Result<()> {
    let txs = db::transactions::list_transactions_backend(backend)?;
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    // Ensure cash assets price at 1.0
    for tx in &txs {
        if tx.category == AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions(&txs, &prices, &fx_rates);

    let use_category = matches!(group_by, Some(SummaryGroupBy::Category));

    if use_category {
        run_by_category(&positions, json)
    } else {
        run_by_position(&positions, json)
    }
}

/// Per-position allocation view (default).
fn run_by_position(positions: &[crate::models::position::Position], json: bool) -> Result<()> {
    let mut rows: Vec<AllocRow> = positions
        .iter()
        .filter(|p| p.allocation_pct.unwrap_or_default() > dec!(0))
        .map(|p| AllocRow {
            symbol: p.symbol.clone(),
            category: p.category.to_string(),
            allocation_pct: p.allocation_pct.unwrap_or_default(),
        })
        .collect();

    // Sort by allocation descending
    rows.sort_by(|a, b| b.allocation_pct.cmp(&a.allocation_pct));

    if json {
        let json_rows: Vec<AllocRowJson> = rows.iter().map(|r| r.to_json()).collect();
        println!("{}", serde_json::to_string_pretty(&json_rows)?);
    } else {
        println!("{:<10} {:<12} {:>10}", "Symbol", "Category", "Alloc %");
        println!("{}", "-".repeat(34));
        for row in &rows {
            println!(
                "{:<10} {:<12} {:>9.1}%",
                row.symbol, row.category, row.allocation_pct
            );
        }
    }

    Ok(())
}

/// Category-grouped allocation view.
fn run_by_category(positions: &[crate::models::position::Position], json: bool) -> Result<()> {
    let mut category_totals: HashMap<String, Decimal> = HashMap::new();
    let mut category_positions: HashMap<String, Vec<(String, Decimal)>> = HashMap::new();

    for p in positions {
        let alloc = p.allocation_pct.unwrap_or_default();
        if alloc <= dec!(0) {
            continue;
        }
        let cat = p.category.to_string();
        *category_totals.entry(cat.clone()).or_insert(dec!(0)) += alloc;
        category_positions
            .entry(cat)
            .or_default()
            .push((p.symbol.clone(), alloc));
    }

    let mut cats: Vec<(String, Decimal)> = category_totals.into_iter().collect();
    cats.sort_by(|a, b| b.1.cmp(&a.1));

    if json {
        let json_cats: Vec<serde_json::Value> = cats
            .iter()
            .map(|(cat, total)| {
                let members = category_positions.get(cat).cloned().unwrap_or_default();
                let mut sorted_members = members;
                sorted_members.sort_by(|a, b| b.1.cmp(&a.1));
                serde_json::json!({
                    "category": cat,
                    "allocation_pct": round_decimal_2(*total),
                    "positions": sorted_members.iter().map(|(sym, pct)| {
                        serde_json::json!({
                            "symbol": sym,
                            "allocation_pct": round_decimal_2(*pct),
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_cats)?);
    } else {
        for (cat, total) in &cats {
            println!("{:<14} {:>9.1}%", cat, total);
            let mut members = category_positions.get(cat).cloned().unwrap_or_default();
            members.sort_by(|a, b| b.1.cmp(&a.1));
            for (sym, pct) in &members {
                println!("  {:<12} {:>9.1}%", sym, pct);
            }
        }
    }

    Ok(())
}

struct AllocRow {
    symbol: String,
    category: String,
    allocation_pct: Decimal,
}

#[derive(serde::Serialize)]
struct AllocRowJson {
    symbol: String,
    category: String,
    allocation_pct: f64,
}

impl AllocRow {
    fn to_json(&self) -> AllocRowJson {
        AllocRowJson {
            symbol: self.symbol.clone(),
            category: self.category.clone(),
            allocation_pct: round_decimal_2(self.allocation_pct),
        }
    }
}

fn round_decimal_2(value: Decimal) -> f64 {
    let rounded = value.round_dp(2);
    rounded.to_string().parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_decimal_2() {
        assert_eq!(round_decimal_2(dec!(51.4567)), 51.46);
        assert_eq!(round_decimal_2(dec!(0.1)), 0.1);
        assert_eq!(round_decimal_2(dec!(100.0)), 100.0);
        assert_eq!(round_decimal_2(dec!(23.09)), 23.09);
    }
}
