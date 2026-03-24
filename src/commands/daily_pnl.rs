use std::collections::HashMap;

use anyhow::Result;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::config::Config;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_prices_at_date_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::compute_positions;

#[derive(Debug, Serialize)]
struct PositionPnl {
    symbol: String,
    name: String,
    category: String,
    quantity: Decimal,
    prev_price: Option<Decimal>,
    current_price: Option<Decimal>,
    price_change: Option<Decimal>,
    price_change_pct: Option<Decimal>,
    prev_value: Option<Decimal>,
    current_value: Option<Decimal>,
    daily_pnl: Option<Decimal>,
}

#[derive(Debug, Serialize)]
struct DailyPnlOutput {
    date: String,
    prev_date: String,
    currency: String,
    positions: Vec<PositionPnl>,
    total_current_value: Decimal,
    total_prev_value: Decimal,
    total_daily_pnl: Decimal,
    total_daily_pnl_pct: Option<Decimal>,
}

fn return_pct(start: Decimal, end: Decimal) -> Option<Decimal> {
    if start == dec!(0) {
        return None;
    }
    Some(((end - start) / start) * dec!(100))
}

pub fn run(backend: &BackendConnection, config: &Config, json: bool) -> Result<()> {
    let today = Utc::now().date_naive();
    let yesterday = today - Duration::days(1);
    let today_str = today.format("%Y-%m-%d").to_string();
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();

    // Get current prices and transactions
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
            println!(
                r#"{{"error":"no_positions","message":"No open positions found."}}"#
            );
        } else {
            println!("No open positions found.");
        }
        return Ok(());
    }

    // Get previous day's prices
    let symbols: Vec<String> = positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .map(|p| p.symbol.clone())
        .collect();
    let prev_prices = get_prices_at_date_backend(backend, &symbols, &yesterday_str)?;

    // Build per-position P&L
    let mut pnl_rows: Vec<PositionPnl> = Vec::new();
    let mut total_current = dec!(0);
    let mut total_prev = dec!(0);
    let mut total_pnl = dec!(0);

    for pos in &positions {
        if pos.category == AssetCategory::Cash {
            // Cash doesn't have daily P&L from price changes
            let val = pos.current_value.unwrap_or(dec!(0));
            total_current += val;
            total_prev += val;
            continue;
        }

        let prev_price = prev_prices.get(&pos.symbol).copied();
        let cur_price = pos.current_price;

        let (price_change, price_change_pct) = match (prev_price, cur_price) {
            (Some(pp), Some(cp)) => {
                let change = cp - pp;
                let pct = return_pct(pp, cp);
                (Some(change), pct)
            }
            _ => (None, None),
        };

        // Compute values considering FX
        let fx = if pos.currency != "USD" {
            fx_rates.get(&pos.currency).copied().unwrap_or(dec!(1))
        } else {
            dec!(1)
        };

        let prev_value = prev_price.map(|pp| pp * pos.quantity * fx);
        let cur_value = pos.current_value;
        let daily_pnl = match (cur_value, prev_value) {
            (Some(cv), Some(pv)) => Some(cv - pv),
            _ => None,
        };

        if let Some(cv) = cur_value {
            total_current += cv;
        }
        if let Some(pv) = prev_value {
            total_prev += pv;
        }
        if let Some(dpnl) = daily_pnl {
            total_pnl += dpnl;
        }

        pnl_rows.push(PositionPnl {
            symbol: pos.symbol.clone(),
            name: pos.name.clone(),
            category: format!("{:?}", pos.category),
            quantity: pos.quantity,
            prev_price,
            current_price: cur_price,
            price_change,
            price_change_pct,
            prev_value,
            current_value: cur_value,
            daily_pnl,
        });
    }

    // Sort by absolute daily P&L descending (biggest movers first)
    pnl_rows.sort_by(|a, b| {
        let a_abs = a.daily_pnl.unwrap_or(dec!(0)).abs();
        let b_abs = b.daily_pnl.unwrap_or(dec!(0)).abs();
        b_abs.cmp(&a_abs)
    });

    let total_pnl_pct = return_pct(total_prev, total_current);

    if json {
        let output = DailyPnlOutput {
            date: today_str,
            prev_date: yesterday_str,
            currency: config.base_currency.clone(),
            positions: pnl_rows,
            total_current_value: total_current,
            total_prev_value: total_prev,
            total_daily_pnl: total_pnl,
            total_daily_pnl_pct: total_pnl_pct,
        };
        println!("{}", serde_json::to_string(&output)?);
    } else {
        print_text(
            &pnl_rows,
            &config.base_currency,
            &today_str,
            &yesterday_str,
            total_current,
            total_prev,
            total_pnl,
            total_pnl_pct,
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_text(
    rows: &[PositionPnl],
    currency: &str,
    today: &str,
    prev: &str,
    total_current: Decimal,
    total_prev: Decimal,
    total_pnl: Decimal,
    total_pnl_pct: Option<Decimal>,
) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                   📊  DAILY P&L REPORT                         ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!(
        "  Period: {} → {}    Currency: {}",
        prev, today, currency
    );
    println!();

    println!(
        "  {:<10} {:>12} {:>12} {:>10} {:>14}",
        "Symbol", "Prev Price", "Cur Price", "Chg %", "Daily P&L"
    );
    println!("  {}", "─".repeat(62));

    for row in rows {
        let prev_p = row
            .prev_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".into());
        let cur_p = row
            .current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".into());
        let chg_pct = row
            .price_change_pct
            .map(|p| format!("{:+.2}%", p))
            .unwrap_or_else(|| "N/A".into());
        let pnl = row
            .daily_pnl
            .map(|p| format!("{:+.2}", p))
            .unwrap_or_else(|| "N/A".into());

        println!(
            "  {:<10} {:>12} {:>12} {:>10} {:>14}",
            row.symbol, prev_p, cur_p, chg_pct, pnl
        );
    }

    println!("  {}", "─".repeat(62));

    let pnl_pct_str = total_pnl_pct
        .map(|p| format!("{:+.2}%", p))
        .unwrap_or_else(|| "N/A".into());

    println!(
        "  {:<10} {:>12.2} {:>12.2} {:>10} {:>14}",
        "TOTAL",
        total_prev,
        total_current,
        pnl_pct_str,
        format!("{:+.2}", total_pnl)
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::price_cache::upsert_price;
    use crate::db::price_history::upsert_history;
    use crate::db::transactions::insert_transaction;
    use crate::models::asset::AssetCategory;
    use crate::models::price::{HistoryRecord, PriceQuote};
    use crate::models::transaction::{NewTransaction, TxType};

    fn make_backend() -> crate::db::backend::BackendConnection {
        let conn = crate::db::open_in_memory();
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_return_pct() {
        assert_eq!(return_pct(dec!(100), dec!(110)), Some(dec!(10)));
        assert_eq!(return_pct(dec!(100), dec!(90)), Some(dec!(-10)));
        assert_eq!(return_pct(dec!(0), dec!(100)), None);
    }

    #[test]
    fn test_daily_pnl_no_transactions() {
        let backend = make_backend();
        let config = Config::default();
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_daily_pnl_json_no_transactions() {
        let backend = make_backend();
        let config = Config::default();
        let result = run(&backend, &config, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_daily_pnl_with_positions() {
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
                price: dec!(155),
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

        let yesterday = (Utc::now().date_naive() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        upsert_history(
            &conn,
            "AAPL",
            "test",
            &[HistoryRecord {
                date: yesterday,
                close: dec!(152),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_daily_pnl_json_with_positions() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        let tx = NewTransaction {
            symbol: "BTC".into(),
            category: AssetCategory::Crypto,
            tx_type: TxType::Buy,
            quantity: dec!(1),
            price_per: dec!(50000),
            currency: "USD".into(),
            date: "2026-01-01".into(),
            notes: None,
        };
        insert_transaction(&conn, &tx).unwrap();

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

        let yesterday = (Utc::now().date_naive() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        upsert_history(
            &conn,
            "BTC",
            "test",
            &[HistoryRecord {
                date: yesterday,
                close: dec!(85000),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        let result = run(&backend, &config, true);
        assert!(result.is_ok());
    }
}
