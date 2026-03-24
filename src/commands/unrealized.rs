use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::config::Config;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::position::compute_positions;

#[derive(Debug, Serialize)]
struct PositionUnrealized {
    symbol: String,
    name: String,
    category: String,
    quantity: Decimal,
    avg_cost: Decimal,
    total_cost: Decimal,
    current_price: Option<Decimal>,
    current_value: Option<Decimal>,
    unrealized_gain: Option<Decimal>,
    unrealized_gain_pct: Option<Decimal>,
    allocation_pct: Option<Decimal>,
}

#[derive(Debug, Serialize)]
struct CategorySummary {
    category: String,
    total_cost: Decimal,
    current_value: Decimal,
    unrealized_gain: Decimal,
    unrealized_gain_pct: Option<Decimal>,
    allocation_pct: Option<Decimal>,
}

#[derive(Debug, Serialize)]
struct UnrealizedOutput {
    as_of: String,
    currency: String,
    positions: Vec<PositionUnrealized>,
    by_category: Vec<CategorySummary>,
    total_cost: Decimal,
    total_current_value: Decimal,
    total_unrealized_gain: Decimal,
    total_unrealized_gain_pct: Option<Decimal>,
}

fn gain_pct(cost: Decimal, value: Decimal) -> Option<Decimal> {
    if cost == dec!(0) {
        return None;
    }
    Some(((value - cost) / cost) * dec!(100))
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    group_by_category: bool,
    json: bool,
) -> Result<()> {
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = HashMap::new();
    for quote in &cached {
        prices.insert(quote.symbol.clone(), quote.price);
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();

    let txs = list_transactions_backend(backend)?;
    if txs.is_empty() {
        if json {
            println!(r#"{{"error":"no_transactions","message":"No transactions found. Add one with: pftui portfolio transaction add"}}"#);
        } else {
            println!("No transactions found. Add one with: pftui portfolio transaction add");
        }
        return Ok(());
    }

    let positions = compute_positions(&txs, &prices, &fx_rates);
    if positions.is_empty() {
        if json {
            println!(r#"{{"error":"no_positions","message":"No open positions found."}}"#);
        } else {
            println!("No open positions found.");
        }
        return Ok(());
    }

    // Build per-position unrealized rows
    let mut rows: Vec<PositionUnrealized> = Vec::new();
    let mut total_cost = dec!(0);
    let mut total_value = dec!(0);

    for pos in &positions {
        let cur_val = pos.current_value.unwrap_or(dec!(0));
        total_cost += pos.total_cost;
        total_value += cur_val;

        rows.push(PositionUnrealized {
            symbol: pos.symbol.clone(),
            name: pos.name.clone(),
            category: format!("{:?}", pos.category),
            quantity: pos.quantity,
            avg_cost: pos.avg_cost,
            total_cost: pos.total_cost,
            current_price: pos.current_price,
            current_value: pos.current_value,
            unrealized_gain: pos.gain,
            unrealized_gain_pct: pos.gain_pct,
            allocation_pct: pos.allocation_pct,
        });
    }

    // Sort by absolute unrealized gain descending (biggest impact first)
    rows.sort_by(|a, b| {
        let a_abs = a.unrealized_gain.unwrap_or(dec!(0)).abs();
        let b_abs = b.unrealized_gain.unwrap_or(dec!(0)).abs();
        b_abs.cmp(&a_abs)
    });

    let total_gain = total_value - total_cost;
    let total_gain_pct = gain_pct(total_cost, total_value);

    // Build category summaries
    let mut cat_map: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    for row in &rows {
        let entry = cat_map.entry(row.category.clone()).or_insert((dec!(0), dec!(0)));
        entry.0 += row.total_cost;
        entry.1 += row.current_value.unwrap_or(dec!(0));
    }

    let mut by_category: Vec<CategorySummary> = cat_map
        .into_iter()
        .map(|(cat, (cost, value))| {
            let gain = value - cost;
            let alloc = if total_value > dec!(0) {
                Some((value / total_value) * dec!(100))
            } else {
                None
            };
            CategorySummary {
                category: cat,
                total_cost: cost,
                current_value: value,
                unrealized_gain: gain,
                unrealized_gain_pct: gain_pct(cost, value),
                allocation_pct: alloc,
            }
        })
        .collect();

    // Sort categories by absolute gain descending
    by_category.sort_by(|a, b| b.unrealized_gain.abs().cmp(&a.unrealized_gain.abs()));

    let output = UnrealizedOutput {
        as_of: today.clone(),
        currency: config.base_currency.clone(),
        positions: rows,
        by_category,
        total_cost,
        total_current_value: total_value,
        total_unrealized_gain: total_gain,
        total_unrealized_gain_pct: total_gain_pct,
    };

    if json {
        println!("{}", serde_json::to_string(&output)?);
    } else if group_by_category {
        print_text_grouped(&output);
    } else {
        print_text(&output);
    }

    Ok(())
}

fn print_text(output: &UnrealizedOutput) {
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                 📊  UNREALIZED GAIN/LOSS REPORT                        ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!(
        "  As of: {}    Currency: {}",
        output.as_of, output.currency
    );
    println!();

    println!(
        "  {:<10} {:>10} {:>12} {:>12} {:>12} {:>10} {:>8}",
        "Symbol", "Qty", "Avg Cost", "Cost Basis", "Cur Value", "Gain", "Gain %"
    );
    println!("  {}", "─".repeat(78));

    for row in &output.positions {
        let cur_val = row
            .current_value
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "N/A".into());
        let gain = row
            .unrealized_gain
            .map(|g| format!("{:+.2}", g))
            .unwrap_or_else(|| "N/A".into());
        let gain_pct = row
            .unrealized_gain_pct
            .map(|p| format!("{:+.2}%", p))
            .unwrap_or_else(|| "N/A".into());

        println!(
            "  {:<10} {:>10} {:>12.2} {:>12.2} {:>12} {:>10} {:>8}",
            row.symbol,
            format_qty(row.quantity),
            row.avg_cost,
            row.total_cost,
            cur_val,
            gain,
            gain_pct,
        );
    }

    println!("  {}", "─".repeat(78));

    let total_gain_str = format!("{:+.2}", output.total_unrealized_gain);
    let total_gain_pct_str = output
        .total_unrealized_gain_pct
        .map(|p| format!("{:+.2}%", p))
        .unwrap_or_else(|| "N/A".into());

    println!(
        "  {:<10} {:>10} {:>12} {:>12.2} {:>12.2} {:>10} {:>8}",
        "TOTAL", "", "", output.total_cost, output.total_current_value, total_gain_str, total_gain_pct_str,
    );
    println!();

    // Category breakdown
    println!("  By Category:");
    println!(
        "  {:<12} {:>12} {:>12} {:>12} {:>10} {:>8}",
        "Category", "Cost Basis", "Cur Value", "Gain", "Gain %", "Alloc %"
    );
    println!("  {}", "─".repeat(70));

    for cat in &output.by_category {
        let gain_pct = cat
            .unrealized_gain_pct
            .map(|p| format!("{:+.2}%", p))
            .unwrap_or_else(|| "N/A".into());
        let alloc = cat
            .allocation_pct
            .map(|p| format!("{:.1}%", p))
            .unwrap_or_else(|| "N/A".into());

        println!(
            "  {:<12} {:>12.2} {:>12.2} {:>12} {:>10} {:>8}",
            cat.category,
            cat.total_cost,
            cat.current_value,
            format!("{:+.2}", cat.unrealized_gain),
            gain_pct,
            alloc,
        );
    }
    println!();
}

fn print_text_grouped(output: &UnrealizedOutput) {
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                 📊  UNREALIZED GAIN/LOSS REPORT                        ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!(
        "  As of: {}    Currency: {}",
        output.as_of, output.currency
    );
    println!();

    // Group positions by category
    let mut grouped: HashMap<String, Vec<&PositionUnrealized>> = HashMap::new();
    for row in &output.positions {
        grouped.entry(row.category.clone()).or_default().push(row);
    }

    // Sort categories by absolute gain descending (matching by_category order)
    let mut cats: Vec<String> = grouped.keys().cloned().collect();
    cats.sort_by(|a, b| {
        let a_gain: Decimal = grouped[a]
            .iter()
            .filter_map(|r| r.unrealized_gain)
            .sum();
        let b_gain: Decimal = grouped[b]
            .iter()
            .filter_map(|r| r.unrealized_gain)
            .sum();
        b_gain.abs().cmp(&a_gain.abs())
    });

    for cat in &cats {
        let rows = &grouped[cat];
        let cat_summary = output.by_category.iter().find(|c| &c.category == cat);

        println!("  ┌─ {} ─┐", cat);
        println!(
            "  {:<10} {:>10} {:>12} {:>12} {:>12} {:>10} {:>8}",
            "Symbol", "Qty", "Avg Cost", "Cost Basis", "Cur Value", "Gain", "Gain %"
        );
        println!("  {}", "─".repeat(78));

        for row in rows {
            let cur_val = row
                .current_value
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "N/A".into());
            let gain = row
                .unrealized_gain
                .map(|g| format!("{:+.2}", g))
                .unwrap_or_else(|| "N/A".into());
            let gain_pct_str = row
                .unrealized_gain_pct
                .map(|p| format!("{:+.2}%", p))
                .unwrap_or_else(|| "N/A".into());

            println!(
                "  {:<10} {:>10} {:>12.2} {:>12.2} {:>12} {:>10} {:>8}",
                row.symbol,
                format_qty(row.quantity),
                row.avg_cost,
                row.total_cost,
                cur_val,
                gain,
                gain_pct_str,
            );
        }

        if let Some(cs) = cat_summary {
            let gain_pct_str = cs
                .unrealized_gain_pct
                .map(|p| format!("{:+.2}%", p))
                .unwrap_or_else(|| "N/A".into());
            println!("  {}", "─".repeat(78));
            println!(
                "  {:<10} {:>10} {:>12} {:>12.2} {:>12.2} {:>10} {:>8}",
                "Subtotal",
                "",
                "",
                cs.total_cost,
                cs.current_value,
                format!("{:+.2}", cs.unrealized_gain),
                gain_pct_str,
            );
        }
        println!();
    }

    // Grand total
    let total_gain_str = format!("{:+.2}", output.total_unrealized_gain);
    let total_gain_pct_str = output
        .total_unrealized_gain_pct
        .map(|p| format!("{:+.2}%", p))
        .unwrap_or_else(|| "N/A".into());

    println!("  {}", "═".repeat(78));
    println!(
        "  {:<10} {:>10} {:>12} {:>12.2} {:>12.2} {:>10} {:>8}",
        "TOTAL", "", "", output.total_cost, output.total_current_value, total_gain_str, total_gain_pct_str,
    );
    println!();
}

fn format_qty(qty: Decimal) -> String {
    // Show fewer decimals for large quantities, more for fractional
    if qty >= dec!(100) {
        format!("{:.1}", qty)
    } else if qty >= dec!(1) {
        format!("{:.2}", qty)
    } else {
        format!("{:.4}", qty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::price_cache::upsert_price;
    use crate::db::transactions::insert_transaction;
    use crate::models::asset::AssetCategory;
    use crate::models::price::PriceQuote;
    use crate::models::transaction::{NewTransaction, TxType};

    fn make_backend() -> crate::db::backend::BackendConnection {
        let conn = crate::db::open_in_memory();
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_unrealized_no_transactions() {
        let backend = make_backend();
        let config = Config::default();
        let result = run(&backend, &config, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unrealized_json_no_transactions() {
        let backend = make_backend();
        let config = Config::default();
        let result = run(&backend, &config, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unrealized_with_positions() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        let tx = NewTransaction {
            symbol: "AAPL".into(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".into(),
            date: "2026-01-01".into(),
            notes: None,
        };
        insert_transaction(&conn, &tx).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".into(),
                price: dec!(200),
                currency: "USD".into(),
                source: "test".into(),
                fetched_at: "2026-03-24T12:00:00Z".into(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        let result = run(&backend, &config, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unrealized_json_with_positions() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        let tx1 = NewTransaction {
            symbol: "BTC".into(),
            category: AssetCategory::Crypto,
            tx_type: TxType::Buy,
            quantity: dec!(1),
            price_per: dec!(50000),
            currency: "USD".into(),
            date: "2026-01-01".into(),
            notes: None,
        };
        insert_transaction(&conn, &tx1).unwrap();

        let tx2 = NewTransaction {
            symbol: "GC=F".into(),
            category: AssetCategory::Commodity,
            tx_type: TxType::Buy,
            quantity: dec!(5),
            price_per: dec!(2000),
            currency: "USD".into(),
            date: "2026-01-01".into(),
            notes: None,
        };
        insert_transaction(&conn, &tx2).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".into(),
                price: dec!(87000),
                currency: "USD".into(),
                source: "test".into(),
                fetched_at: "2026-03-24T12:00:00Z".into(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".into(),
                price: dec!(3100),
                currency: "USD".into(),
                source: "test".into(),
                fetched_at: "2026-03-24T12:00:00Z".into(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        let result = run(&backend, &config, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unrealized_grouped() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        let tx1 = NewTransaction {
            symbol: "BTC".into(),
            category: AssetCategory::Crypto,
            tx_type: TxType::Buy,
            quantity: dec!(1),
            price_per: dec!(50000),
            currency: "USD".into(),
            date: "2026-01-01".into(),
            notes: None,
        };
        insert_transaction(&conn, &tx1).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".into(),
                price: dec!(87000),
                currency: "USD".into(),
                source: "test".into(),
                fetched_at: "2026-03-24T12:00:00Z".into(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        let result = run(&backend, &config, true, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_gain_pct_helper() {
        assert_eq!(gain_pct(dec!(100), dec!(110)), Some(dec!(10)));
        assert_eq!(gain_pct(dec!(100), dec!(90)), Some(dec!(-10)));
        assert_eq!(gain_pct(dec!(0), dec!(100)), None);
    }
}
