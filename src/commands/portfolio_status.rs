//! `pftui portfolio status` — consolidated portfolio snapshot for agent consumption.
//!
//! Combines allocation, value, daily P&L, and unrealized gain/loss into a single
//! command and JSON payload. Agents get everything they need in one call instead
//! of running summary + allocation + daily-pnl + unrealized separately.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_prices_at_date_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations};

#[derive(Debug, Serialize)]
struct PositionStatus {
    symbol: String,
    category: String,
    allocation_pct: f64,
    current_price: Option<f64>,
    current_value: Option<f64>,
    quantity: f64,
    avg_cost: f64,
    unrealized_gain: f64,
    unrealized_gain_pct: f64,
    daily_change: Option<f64>,
    daily_change_pct: Option<f64>,
}

#[derive(Debug, Serialize)]
struct CategorySummary {
    category: String,
    allocation_pct: f64,
    value: f64,
    daily_change: Option<f64>,
    positions: usize,
}

#[derive(Debug, Serialize)]
struct PortfolioStatusOutput {
    total_value: f64,
    total_unrealized_gain: f64,
    total_unrealized_gain_pct: f64,
    total_daily_pnl: f64,
    total_daily_pnl_pct: Option<f64>,
    date: String,
    currency: String,
    position_count: usize,
    categories: Vec<CategorySummary>,
    positions: Vec<PositionStatus>,
}

fn dec_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

fn dec_to_f64_2(d: Decimal) -> f64 {
    let rounded = d.round_dp(2);
    rounded.to_string().parse::<f64>().unwrap_or(0.0)
}

/// Format a decimal value with commas as thousands separators.
fn format_with_commas(value: Decimal, dp: u32) -> String {
    let rounded = value.round_dp(dp);
    let s = format!("{:.prec$}", rounded, prec = dp as usize);

    let (integer_part, decimal_part) = if let Some(dot_pos) = s.find('.') {
        (&s[..dot_pos], Some(&s[dot_pos..]))
    } else {
        (s.as_str(), None)
    };

    let (sign, digits) = if let Some(stripped) = integer_part.strip_prefix('-') {
        ("-", stripped)
    } else {
        ("", integer_part)
    };

    let mut result = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    let formatted_int: String = result.chars().rev().collect();

    match decimal_part {
        Some(dec) => format!("{}{}{}", sign, formatted_int, dec),
        None => format!("{}{}", sign, formatted_int),
    }
}

pub fn run(backend: &BackendConnection, config: &Config, json: bool) -> Result<()> {
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(backend, config, &mut prices, json),
        PortfolioMode::Percentage => run_percentage(backend, &prices, json),
    }
}

fn run_full(
    backend: &BackendConnection,
    config: &Config,
    prices: &mut HashMap<String, Decimal>,
    json: bool,
) -> Result<()> {
    let txs = list_transactions_backend(backend)?;
    if txs.is_empty() {
        if json {
            println!(r#"{{"error":"no_positions","message":"No positions. Add with: pftui portfolio transaction add"}}"#);
        } else {
            println!("No positions. Add one with: pftui portfolio transaction add");
        }
        return Ok(());
    }

    // Ensure cash assets price at 1.0
    for tx in &txs {
        if tx.category == AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions(&txs, prices, &fx_rates);
    if positions.is_empty() {
        if json {
            println!(r#"{{"error":"no_open_positions","message":"No open positions."}}"#);
        } else {
            println!("No open positions.");
        }
        return Ok(());
    }

    // Get yesterday's prices for daily P&L
    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let today_str = today.format("%Y-%m-%d").to_string();

    let symbols: Vec<String> = positions.iter().map(|p| p.symbol.clone()).collect();
    let prev_prices = get_prices_at_date_backend(backend, &symbols, &yesterday_str)
        .unwrap_or_default();

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_unrealized = total_value - total_cost;
    let total_unrealized_pct = if total_cost > dec!(0) {
        (total_unrealized / total_cost) * dec!(100)
    } else {
        dec!(0)
    };

    // Build position-level status
    let mut pos_statuses: Vec<PositionStatus> = Vec::new();
    let mut total_daily_pnl = dec!(0);
    let mut total_prev_value = dec!(0);

    for p in &positions {
        let alloc = p.allocation_pct.unwrap_or_default();
        let cur_price = p.current_price.unwrap_or_default();
        let cur_value = p.current_value.unwrap_or_default();

        let unrealized = cur_value - p.total_cost;
        let unrealized_pct = if p.total_cost > dec!(0) {
            (unrealized / p.total_cost) * dec!(100)
        } else {
            dec!(0)
        };

        let prev_price = prev_prices.get(&p.symbol).copied();
        let (daily_change, daily_change_pct) = if let Some(pp) = prev_price {
            let prev_val = pp * p.quantity;
            total_prev_value += prev_val;
            let change = cur_value - prev_val;
            total_daily_pnl += change;
            let pct = if prev_val > dec!(0) {
                Some((change / prev_val) * dec!(100))
            } else {
                None
            };
            (Some(change), pct)
        } else {
            (None, None)
        };

        pos_statuses.push(PositionStatus {
            symbol: p.symbol.clone(),
            category: p.category.to_string(),
            allocation_pct: dec_to_f64_2(alloc),
            current_price: Some(dec_to_f64(cur_price)),
            current_value: Some(dec_to_f64_2(cur_value)),
            quantity: dec_to_f64(p.quantity),
            avg_cost: dec_to_f64_2(p.avg_cost),
            unrealized_gain: dec_to_f64_2(unrealized),
            unrealized_gain_pct: dec_to_f64_2(unrealized_pct),
            daily_change: daily_change.map(dec_to_f64_2),
            daily_change_pct: daily_change_pct.map(dec_to_f64_2),
        });
    }

    // Sort by allocation descending
    pos_statuses.sort_by(|a, b| {
        b.allocation_pct
            .partial_cmp(&a.allocation_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_daily_pnl_pct = if total_prev_value > dec!(0) {
        Some((total_daily_pnl / total_prev_value) * dec!(100))
    } else {
        None
    };

    // Build category summaries
    let mut cat_map: HashMap<String, (Decimal, Decimal, Option<Decimal>, usize)> = HashMap::new();
    for ps in &pos_statuses {
        let entry = cat_map
            .entry(ps.category.clone())
            .or_insert((dec!(0), dec!(0), Some(dec!(0)), 0));
        entry.0 += Decimal::try_from(ps.allocation_pct).unwrap_or_default();
        entry.1 += Decimal::try_from(ps.current_value.unwrap_or(0.0)).unwrap_or_default();
        if let Some(dc) = ps.daily_change {
            if let Some(ref mut total_dc) = entry.2 {
                *total_dc += Decimal::try_from(dc).unwrap_or_default();
            }
        } else {
            entry.2 = None;
        }
        entry.3 += 1;
    }

    let mut categories: Vec<CategorySummary> = cat_map
        .into_iter()
        .map(|(cat, (alloc, val, dc, count))| CategorySummary {
            category: cat,
            allocation_pct: dec_to_f64_2(alloc),
            value: dec_to_f64_2(val),
            daily_change: dc.map(dec_to_f64_2),
            positions: count,
        })
        .collect();
    categories.sort_by(|a, b| {
        b.allocation_pct
            .partial_cmp(&a.allocation_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if json {
        let output = PortfolioStatusOutput {
            total_value: dec_to_f64_2(total_value),
            total_unrealized_gain: dec_to_f64_2(total_unrealized),
            total_unrealized_gain_pct: dec_to_f64_2(total_unrealized_pct),
            total_daily_pnl: dec_to_f64_2(total_daily_pnl),
            total_daily_pnl_pct: total_daily_pnl_pct.map(dec_to_f64_2),
            date: today_str,
            currency: config.base_currency.clone(),
            position_count: pos_statuses.len(),
            categories,
            positions: pos_statuses,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let csym = config.currency_symbol();
        let sign = if total_unrealized >= dec!(0) { "+" } else { "" };
        println!(
            "Portfolio Status — {}",
            today_str
        );
        println!();
        println!(
            "  Value:      {}{}",
            csym,
            format_with_commas(total_value, 2)
        );
        println!(
            "  Unrealized: {}{}{} ({}{}%)",
            sign,
            csym,
            format_with_commas(total_unrealized, 2),
            sign,
            total_unrealized_pct.round_dp(2),
        );
        let pnl_sign = if total_daily_pnl >= dec!(0) { "+" } else { "" };
        println!(
            "  Daily P&L:  {}{}{} ({}{}%)",
            pnl_sign,
            csym,
            format_with_commas(total_daily_pnl, 2),
            pnl_sign,
            total_daily_pnl_pct.map_or("N/A".to_string(), |p| format!("{}", p.round_dp(2))),
        );
        println!();

        // Category breakdown
        println!("  {:14} {:>10} {:>12} {:>12}", "Category", "Alloc %", "Value", "Daily Chg");
        println!("  {}", "-".repeat(50));
        for cat in &categories {
            let dc_str = cat
                .daily_change
                .map(|d| {
                    let s = if d >= 0.0 { "+" } else { "" };
                    format!(
                        "{}{}{}",
                        s,
                        csym,
                        format_with_commas(
                            Decimal::try_from(d).unwrap_or_default(),
                            2
                        )
                    )
                })
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "  {:14} {:>9.1}% {:>11}{} {:>12}",
                cat.category,
                cat.allocation_pct,
                csym,
                format_with_commas(
                    Decimal::try_from(cat.value).unwrap_or_default(),
                    2
                ),
                dc_str
            );
        }
        println!();

        // Position details
        println!(
            "  {:10} {:>10} {:>12} {:>10} {:>10}",
            "Symbol", "Alloc %", "Value", "Unreal%", "Daily%"
        );
        println!("  {}", "-".repeat(56));
        for ps in &pos_statuses {
            let val_str = ps
                .current_value
                .map(|v| {
                    format!(
                        "{}{}",
                        csym,
                        format_with_commas(
                            Decimal::try_from(v).unwrap_or_default(),
                            2
                        )
                    )
                })
                .unwrap_or_else(|| "N/A".to_string());
            let unreal_str = if ps.unrealized_gain_pct.abs() < 0.005 {
                "0.00%".to_string()
            } else {
                let s = if ps.unrealized_gain_pct >= 0.0 { "+" } else { "" };
                format!("{}{:.2}%", s, ps.unrealized_gain_pct)
            };
            let daily_str = ps
                .daily_change_pct
                .map(|d| {
                    let s = if d >= 0.0 { "+" } else { "" };
                    format!("{}{:.2}%", s, d)
                })
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "  {:10} {:>9.1}% {:>12} {:>10} {:>10}",
                ps.symbol, ps.allocation_pct, val_str, unreal_str, daily_str,
            );
        }
    }

    Ok(())
}

fn run_percentage(
    backend: &BackendConnection,
    prices: &HashMap<String, Decimal>,
    json: bool,
) -> Result<()> {
    let allocs = list_allocations_backend(backend)?;
    if allocs.is_empty() {
        if json {
            println!(r#"{{"error":"no_allocations","message":"No allocations. Run: pftui setup"}}"#);
        } else {
            println!("No allocations. Run: pftui setup");
        }
        return Ok(());
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions_from_allocations(&allocs, prices, &fx_rates);

    let priced: Vec<_> = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .collect();

    if priced.is_empty() {
        if json {
            println!(r#"{{"error":"no_prices","message":"No prices cached. Run pftui data refresh first."}}"#);
        } else {
            println!("No prices cached. Run `pftui data refresh` first.");
        }
        return Ok(());
    }

    if json {
        let alloc_data: Vec<serde_json::Value> = positions
            .iter()
            .map(|p| {
                serde_json::json!({
                    "symbol": p.symbol,
                    "category": p.category.to_string(),
                    "allocation_pct": p.allocation_pct.map(dec_to_f64_2),
                    "current_price": p.current_price.map(dec_to_f64),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&alloc_data)?);
    } else {
        println!("Portfolio Status (percentage mode):");
        for p in &positions {
            let price_str = p
                .current_price
                .map(|pr| format!("${}", format_with_commas(pr, 2)))
                .unwrap_or_else(|| "N/A".to_string());
            let alloc_str = p
                .allocation_pct
                .map(|a| format!("{:.1}%", a))
                .unwrap_or_else(|| "?%".to_string());
            println!("  {} {} ({})", p.symbol, price_str, alloc_str);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn to_backend(conn: Connection) -> crate::db::backend::BackendConnection {
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn status_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let backend = to_backend(conn);
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn status_empty_db_json() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let backend = to_backend(conn);
        let result = run(&backend, &config, true);
        assert!(result.is_ok());
    }

    #[test]
    fn status_with_positions_no_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn status_with_positions_and_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::price_cache::upsert_price;
        use crate::db::transactions::insert_transaction;
        use crate::models::price::PriceQuote;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(200),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-01-15T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, true);
        assert!(result.is_ok());
    }

    #[test]
    fn status_with_cash_position() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::price_cache::upsert_price;
        use crate::db::transactions::insert_transaction;
        use crate::models::price::PriceQuote;
        use crate::models::transaction::{NewTransaction, TxType};

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "USD".to_string(),
                category: AssetCategory::Cash,
                tx_type: TxType::Buy,
                quantity: dec!(50000),
                price_per: dec!(1),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(67000),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(68000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-01-15T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, true);
        assert!(result.is_ok());
    }

    #[test]
    fn status_percentage_mode() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };

        use crate::db::allocations::insert_allocation;
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(50)).unwrap();
        insert_allocation(&conn, "GC=F", AssetCategory::Commodity, dec!(50)).unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn status_percentage_mode_json() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };

        use crate::db::allocations::insert_allocation;
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(50)).unwrap();
        insert_allocation(&conn, "GC=F", AssetCategory::Commodity, dec!(50)).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(68000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-01-15T00:00:00Z".to_string(),
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
                symbol: "GC=F".to_string(),
                price: dec!(5000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-01-15T00:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, true);
        assert!(result.is_ok());
    }

    #[test]
    fn dec_to_f64_2_rounds() {
        assert_eq!(dec_to_f64_2(dec!(51.4567)), 51.46);
        assert_eq!(dec_to_f64_2(dec!(0.1)), 0.1);
        assert_eq!(dec_to_f64_2(dec!(100.0)), 100.0);
    }

    #[test]
    fn format_with_commas_basic() {
        assert_eq!(format_with_commas(dec!(1234567.89), 2), "1,234,567.89");
    }

    #[test]
    fn format_with_commas_negative() {
        assert_eq!(format_with_commas(dec!(-42.50), 2), "-42.50");
    }
}
