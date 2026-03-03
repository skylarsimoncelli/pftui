use std::collections::HashMap;

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::cli::SummaryGroupBy;
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::price_history::get_prices_at_date;
use crate::db::transactions::list_transactions;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

/// Validate a date string is YYYY-MM-DD format and represents a real date.
fn validate_date(date: &str) -> Result<()> {
    if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        bail!("Invalid date '{}': expected YYYY-MM-DD format (e.g. 2026-02-28)", date);
    }
    Ok(())
}

pub fn run(
    conn: &Connection,
    config: &Config,
    date: &str,
    group_by: Option<&SummaryGroupBy>,
) -> Result<()> {
    validate_date(date)?;

    // Check the date isn't in the future
    let today = chrono::Utc::now().date_naive();
    let target = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
    if target > today {
        bail!("Date '{}' is in the future. Historical data is only available for past dates.", date);
    }

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, date, group_by),
        PortfolioMode::Percentage => run_percentage(conn, config, date, group_by),
    }
}

fn run_full(
    conn: &Connection,
    config: &Config,
    date: &str,
    group_by: Option<&SummaryGroupBy>,
) -> Result<()> {
    let txs = list_transactions(conn)?;
    if txs.is_empty() {
        println!("No transactions found. Add one with: pftui add-tx");
        return Ok(());
    }

    // Filter transactions to only those on or before the target date
    let txs_at_date: Vec<_> = txs
        .into_iter()
        .filter(|tx| tx.date.as_str() <= date)
        .collect();

    if txs_at_date.is_empty() {
        println!("No transactions found on or before {}.", date);
        return Ok(());
    }

    // Get historical prices at the target date
    let symbols: Vec<String> = txs_at_date
        .iter()
        .map(|tx| tx.symbol.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut prices = get_prices_at_date(conn, &symbols, date)?;

    // Cash always prices at 1.0
    for tx in &txs_at_date {
        if tx.category == AssetCategory::Cash {
            prices.entry(tx.symbol.clone()).or_insert(dec!(1));
        }
    }

    let positions = compute_positions(&txs_at_date, &prices);
    if positions.is_empty() {
        println!("No open positions as of {}.", date);
        return Ok(());
    }

    // Print banner
    print_date_banner(date);

    match group_by {
        Some(SummaryGroupBy::Category) => print_grouped_by_category(&positions, config),
        None => print_full_table(&positions, config, date),
    }
}

fn run_percentage(
    conn: &Connection,
    config: &Config,
    date: &str,
    group_by: Option<&SummaryGroupBy>,
) -> Result<()> {
    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        println!("No allocations found. Run: pftui setup");
        return Ok(());
    }

    let symbols: Vec<String> = allocs.iter().map(|a| a.symbol.clone()).collect();
    let mut prices = get_prices_at_date(conn, &symbols, date)?;

    // Cash always prices at 1.0
    for alloc in &allocs {
        if alloc.category == AssetCategory::Cash {
            prices.entry(alloc.symbol.clone()).or_insert(dec!(1));
        }
    }

    let positions = compute_positions_from_allocations(&allocs, &prices);

    print_date_banner(date);

    match group_by {
        Some(SummaryGroupBy::Category) => print_grouped_by_category_pct(&positions),
        None => print_percentage_table(&positions, config, date),
    }
}

fn print_date_banner(date: &str) {
    println!("╔══════════════════════════════════════════╗");
    println!("║       📅  PORTFOLIO AS OF  📅           ║");
    println!("║             {}               ║", date);
    println!("╚══════════════════════════════════════════╝");
    println!();
}

fn print_full_table(positions: &[Position], config: &Config, date: &str) -> Result<()> {
    println!(
        "{:<8} {:<10} {:>8} {:>10} {:>10} {:>8} {:>8}",
        "Symbol", "Category", "Qty", "Cost", "Price", "Gain%", "Alloc%"
    );
    println!("{}", "-".repeat(70));

    let mut total_value = dec!(0);
    let mut total_cost = dec!(0);
    let mut missing_prices = 0;

    for pos in positions {
        let price_str = pos
            .current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| {
                missing_prices += 1;
                "N/A".to_string()
            });
        let gain_str = pos
            .gain_pct
            .map(|g| format!("{:+.1}%", g))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>8} {:>10.2} {:>10} {:>8} {:>8}",
            pos.symbol, pos.category, pos.quantity, pos.avg_cost, price_str, gain_str, alloc_str,
        );

        if let Some(v) = pos.current_value {
            total_value += v;
        }
        total_cost += pos.total_cost;
    }

    println!("{}", "-".repeat(70));
    let total_gain = total_value - total_cost;
    let total_gain_pct = if total_cost > dec!(0) {
        (total_gain / total_cost) * dec!(100)
    } else {
        dec!(0)
    };

    println!(
        "Total Value: {:.2} {}  |  Cost: {:.2}  |  Gain: {:+.2} ({:+.1}%)",
        total_value, config.base_currency, total_cost, total_gain, total_gain_pct
    );

    if missing_prices > 0 {
        println!(
            "\nNote: {} position(s) have no price history for {}.",
            missing_prices, date
        );
        println!("Run `pftui refresh` to build price history, then try again.");
    }

    Ok(())
}

fn print_grouped_by_category(positions: &[Position], config: &Config) -> Result<()> {
    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();

    let mut groups: HashMap<AssetCategory, CategoryGroup> = HashMap::new();
    for pos in positions {
        let group = groups
            .entry(pos.category)
            .or_insert_with(|| CategoryGroup {
                value: dec!(0),
                cost: dec!(0),
                symbols: Vec::new(),
            });
        if let Some(v) = pos.current_value {
            group.value += v;
        }
        group.cost += pos.total_cost;
        group.symbols.push(pos.symbol.clone());
    }

    let mut sorted: Vec<_> = groups.into_iter().collect();
    sorted.sort_by(|a, b| b.1.value.cmp(&a.1.value));

    println!(
        "{:<12} {:>12} {:>10} {:>8} {:>8}",
        "Category", "Value", "Cost", "Gain%", "Alloc%"
    );
    println!("{}", "─".repeat(54));

    for (category, group) in &sorted {
        let alloc_pct = if total_value > dec!(0) {
            (group.value / total_value) * dec!(100)
        } else {
            dec!(0)
        };
        let gain_pct = if group.cost > dec!(0) {
            ((group.value - group.cost) / group.cost) * dec!(100)
        } else {
            dec!(0)
        };
        let symbols_str = group.symbols.join(", ");

        println!(
            "{:<12} {:>12.2} {:>10.2} {:>+7.1}% {:>6.1}%",
            format_category(category),
            group.value,
            group.cost,
            gain_pct,
            alloc_pct,
        );
        println!("  {}", symbols_str);
    }

    println!("{}", "─".repeat(54));

    let total_gain = total_value - total_cost;
    let total_gain_pct = if total_cost > dec!(0) {
        (total_gain / total_cost) * dec!(100)
    } else {
        dec!(0)
    };

    println!(
        "Total: {:.2} {}  |  Cost: {:.2}  |  Gain: {:+.2} ({:+.1}%)",
        total_value, config.base_currency, total_cost, total_gain, total_gain_pct
    );

    Ok(())
}

fn print_percentage_table(positions: &[Position], config: &Config, date: &str) -> Result<()> {
    println!(
        "{:<8} {:<10} {:>10} {:>8}",
        "Symbol", "Category", "Price", "Alloc%"
    );
    println!("{}", "-".repeat(40));

    let mut missing = 0;
    for pos in positions {
        let price_str = pos
            .current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| {
                missing += 1;
                "N/A".to_string()
            });
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>10} {:>8}",
            pos.symbol, pos.category, price_str, alloc_str,
        );
    }

    if missing > 0 {
        println!(
            "\nNote: {} position(s) have no price history for {}.",
            missing, date
        );
    }

    let _ = config; // used for consistency with full mode signature
    Ok(())
}

fn print_grouped_by_category_pct(positions: &[Position]) -> Result<()> {
    let mut category_alloc: HashMap<AssetCategory, (Decimal, Vec<String>)> = HashMap::new();

    for pos in positions {
        let entry = category_alloc
            .entry(pos.category)
            .or_insert_with(|| (dec!(0), Vec::new()));
        if let Some(alloc) = pos.allocation_pct {
            entry.0 += alloc;
        }
        entry.1.push(pos.symbol.clone());
    }

    let mut sorted: Vec<_> = category_alloc.into_iter().collect();
    sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));

    println!("{:<12} {:>8}", "Category", "Alloc%");
    println!("{}", "─".repeat(22));

    for (category, (alloc, symbols)) in &sorted {
        let symbols_str = symbols.join(", ");
        println!("{:<12} {:>6.1}%", format_category(category), alloc);
        println!("  {}", symbols_str);
    }

    Ok(())
}

struct CategoryGroup {
    value: Decimal,
    cost: Decimal,
    symbols: Vec<String>,
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

    #[test]
    fn validate_date_valid() {
        assert!(validate_date("2026-02-28").is_ok());
        assert!(validate_date("2025-01-01").is_ok());
        assert!(validate_date("2026-12-31").is_ok());
    }

    #[test]
    fn validate_date_invalid_format() {
        assert!(validate_date("02-28-2026").is_err());
        assert!(validate_date("2026/02/28").is_err());
        assert!(validate_date("not-a-date").is_err());
        assert!(validate_date("").is_err());
    }

    #[test]
    fn validate_date_invalid_day() {
        // Feb 30 doesn't exist
        assert!(validate_date("2026-02-30").is_err());
    }

    #[test]
    fn history_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let result = run(&conn, &config, "2026-01-15", None);
        assert!(result.is_ok());
    }

    #[test]
    fn history_no_txs_before_date() {
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
                date: "2026-06-01".to_string(),
                notes: None,
            },
        )
        .unwrap();

        // Looking at a date before the only transaction
        let result = run(&conn, &config, "2026-01-01", None);
        assert!(result.is_ok());
    }

    #[test]
    fn history_with_price_data() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::price_history::upsert_history;
        use crate::db::transactions::insert_transaction;
        use crate::models::price::HistoryRecord;
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

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2025-06-01".into(),
                    close: dec!(180),
                    volume: None,
                },
                HistoryRecord {
                    date: "2025-06-15".into(),
                    close: dec!(190),
                    volume: None,
                },
            ],
        )
        .unwrap();

        let result = run(&conn, &config, "2025-06-10", None);
        assert!(result.is_ok());
    }

    #[test]
    fn history_with_group_by() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::price_history::upsert_history;
        use crate::db::transactions::insert_transaction;
        use crate::models::price::HistoryRecord;
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

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(30000),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
            },
        )
        .unwrap();

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2025-06-01".into(),
                close: dec!(200),
                volume: None,
            }],
        )
        .unwrap();

        upsert_history(
            &conn,
            "BTC",
            "coingecko",
            &[HistoryRecord {
                date: "2025-06-01".into(),
                close: dec!(85000),
                volume: None,
            }],
        )
        .unwrap();

        let result = run(&conn, &config, "2025-06-01", Some(&SummaryGroupBy::Category));
        assert!(result.is_ok());
    }

    #[test]
    fn history_cash_always_priced() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
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

        // Cash should always show value even without price history
        let result = run(&conn, &config, "2025-06-01", None);
        assert!(result.is_ok());
    }

    #[test]
    fn history_percentage_mode() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };

        use crate::db::allocations::insert_allocation;
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(60)).unwrap();
        insert_allocation(&conn, "GC=F", AssetCategory::Commodity, dec!(40)).unwrap();

        upsert_history(
            &conn,
            "BTC",
            "coingecko",
            &[HistoryRecord {
                date: "2025-06-01".into(),
                close: dec!(85000),
                volume: None,
            }],
        )
        .unwrap();

        upsert_history(
            &conn,
            "GC=F",
            "yahoo",
            &[HistoryRecord {
                date: "2025-06-01".into(),
                close: dec!(2800),
                volume: None,
            }],
        )
        .unwrap();

        let result = run(&conn, &config, "2025-06-01", None);
        assert!(result.is_ok());
    }
}
