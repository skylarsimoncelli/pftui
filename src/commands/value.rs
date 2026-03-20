use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
#[cfg(test)]
use rusqlite::Connection;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations};

/// Format a decimal value with commas as thousands separators.
fn format_with_commas(value: Decimal, dp: u32) -> String {
    let rounded = value.round_dp(dp);
    let s = format!("{:.prec$}", rounded, prec = dp as usize);

    let (integer_part, decimal_part) = if let Some(dot_pos) = s.find('.') {
        (&s[..dot_pos], Some(&s[dot_pos..]))
    } else {
        (s.as_str(), None)
    };

    // Handle negative numbers
    let (sign, digits) = if let Some(stripped) = integer_part.strip_prefix('-') {
        ("-", stripped)
    } else {
        ("", integer_part)
    };

    // Insert commas
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
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(backend, config, &prices, json),
        PortfolioMode::Percentage => run_percentage(backend, config, &prices, json),
    }
}

fn load_fx_rates(backend: &BackendConnection) -> HashMap<String, Decimal> {
    crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default()
}

fn run_full(
    backend: &BackendConnection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    json: bool,
) -> Result<()> {
    let txs = list_transactions_backend(backend)?;
    if txs.is_empty() {
        if json {
            println!("{{\"error\": \"No positions\"}}");
        } else {
            println!("No positions. Add one with: pftui add-tx");
        }
        return Ok(());
    }

    let fx_rates = load_fx_rates(backend);
    let positions = compute_positions(&txs, prices, &fx_rates);
    if positions.is_empty() {
        if json {
            println!("{{\"error\": \"No open positions\"}}");
        } else {
            println!("No open positions.");
        }
        return Ok(());
    }

    let total_value: Decimal = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum();

    let total_cost: Decimal = positions
        .iter()
        .map(|p| p.total_cost)
        .sum();

    let total_gain = total_value - total_cost;
    let total_gain_pct = if total_cost > dec!(0) {
        (total_gain / total_cost) * dec!(100)
    } else {
        dec!(0)
    };

    let priced_count = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .count();
    let total_count = positions.len();
    
    if json {
        let value_f64: f64 = total_value.to_string().parse().unwrap_or(0.0);
        let change_abs_f64: f64 = total_gain.to_string().parse().unwrap_or(0.0);
        let change_pct_f64: f64 = total_gain_pct.to_string().parse().unwrap_or(0.0);
        println!("{{\"value\": {:.2}, \"change_pct\": {:.2}, \"change_abs\": {:.2}}}", 
            value_f64, change_pct_f64, change_abs_f64);
        return Ok(());
    }

    // Single compact output line
    let csym = config.currency_symbol();
    let sign = if total_gain >= dec!(0) { "+" } else { "" };
    println!(
        "Portfolio: {}{} ({}{}{} / {}{}%)",
        csym,
        format_with_commas(total_value, 2),
        sign,
        csym,
        format_with_commas(total_gain, 2),
        sign,
        total_gain_pct.round_dp(1),
    );

    // Category breakdown
    let mut categories: HashMap<AssetCategory, Decimal> = HashMap::new();
    for pos in &positions {
        if let Some(val) = pos.current_value {
            *categories.entry(pos.category).or_insert(dec!(0)) += val;
        }
    }
    if !categories.is_empty() && total_value > dec!(0) {
        let mut sorted: Vec<_> = categories.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let parts: Vec<String> = sorted
            .iter()
            .map(|(cat, val)| {
                let pct = (val / total_value * dec!(100)).round_dp(0);
                format!("{} {}%", format_category(cat), pct)
            })
            .collect();
        println!("  {}", parts.join(", "));
    }

    // Warn if prices are missing
    if priced_count < total_count {
        let missing = total_count - priced_count;
        println!(
            "\n{}/{} positions missing prices. Run `pftui refresh` first.",
            missing, total_count
        );
    }

    Ok(())
}

fn run_percentage(
    backend: &BackendConnection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    json: bool,
) -> Result<()> {
    let allocs = list_allocations_backend(backend)?;
    if allocs.is_empty() {
        if json {
            println!("{{\"error\": \"No allocations\"}}");
        } else {
            println!("No allocations. Run: pftui setup");
        }
        return Ok(());
    }

    let fx_rates = load_fx_rates(backend);
    let positions = compute_positions_from_allocations(&allocs, prices, &fx_rates);

    let priced: Vec<_> = positions
        .iter()
        .filter(|p| p.current_price.is_some())
        .collect();

    if priced.is_empty() {
        if json {
            println!("{{\"error\": \"No prices cached\"}}");
        } else {
            println!("No prices cached. Run `pftui refresh` first.");
        }
        return Ok(());
    }
    
    if json {
        // In percentage mode, we don't have absolute value, just allocations
        let alloc_data: Vec<_> = positions.iter().map(|p| {
            serde_json::json!({
                "symbol": p.symbol,
                "allocation_pct": p.allocation_pct.map(|a| a.to_string().parse::<f64>().unwrap_or(0.0)),
                "current_price": p.current_price.map(|p| p.to_string().parse::<f64>().unwrap_or(0.0)),
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&alloc_data)?);
        return Ok(());
    }

    // In percentage mode, show allocation breakdown with prices
    let csym = config.currency_symbol();
    println!("Portfolio allocations:");
    for pos in &positions {
        let price_str = pos
            .current_price
            .map(|p| format!("{}{}", csym, format_with_commas(p, 2)))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "?%".to_string());
        println!("  {} {} ({})", pos.symbol, price_str, alloc_str);
    }

    let missing = positions.len() - priced.len();
    if missing > 0 {
        println!(
            "\n{}/{} positions missing prices. Run `pftui refresh` first.",
            missing,
            positions.len()
        );
    }

    Ok(())
}

fn format_category(cat: &AssetCategory) -> &'static str {
    match cat {
        AssetCategory::Equity => "Equity",
        AssetCategory::Crypto => "Crypto",
        AssetCategory::Forex => "Forex",
        AssetCategory::Cash => "Cash",
        AssetCategory::Commodity => "Commodity",
        AssetCategory::Fund => "Fund",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_backend(conn: Connection) -> crate::db::backend::BackendConnection {
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn format_with_commas_basic() {
        assert_eq!(format_with_commas(dec!(1234567.89), 2), "1,234,567.89");
    }

    #[test]
    fn format_with_commas_small() {
        assert_eq!(format_with_commas(dec!(42.50), 2), "42.50");
    }

    #[test]
    fn format_with_commas_large() {
        assert_eq!(format_with_commas(dec!(1000000), 2), "1,000,000.00");
    }

    #[test]
    fn format_with_commas_negative() {
        assert_eq!(format_with_commas(dec!(-1234.56), 2), "-1,234.56");
    }

    #[test]
    fn format_with_commas_zero() {
        assert_eq!(format_with_commas(dec!(0), 2), "0.00");
    }

    #[test]
    fn format_with_commas_no_decimals() {
        assert_eq!(format_with_commas(dec!(12345), 0), "12,345");
    }

    #[test]
    fn value_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        // Should not panic, just prints "No positions"
        let backend = to_backend(conn);
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn value_with_positions_no_prices() {
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
    fn value_with_positions_and_prices() {
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
                open: None,
        },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn value_percentage_mode_no_prices() {
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
}
