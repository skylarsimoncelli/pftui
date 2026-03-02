use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::cli::SummaryGroupBy;
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::price_cache::get_all_cached_prices;
use crate::db::transactions::list_transactions;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

pub fn run(conn: &Connection, config: &Config, group_by: Option<&SummaryGroupBy>) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, &prices, group_by),
        PortfolioMode::Percentage => run_percentage(conn, &prices, group_by),
    }
}

fn run_full(
    conn: &Connection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    group_by: Option<&SummaryGroupBy>,
) -> Result<()> {
    let txs = list_transactions(conn)?;
    if txs.is_empty() {
        println!("No transactions found. Add one with: pftui add-tx");
        return Ok(());
    }

    let positions = compute_positions(&txs, prices);
    if positions.is_empty() {
        println!("No open positions.");
        return Ok(());
    }

    match group_by {
        Some(SummaryGroupBy::Category) => print_grouped_by_category(&positions, config),
        None => print_full_table(&positions, config),
    }
}

fn run_percentage(
    conn: &Connection,
    prices: &HashMap<String, Decimal>,
    group_by: Option<&SummaryGroupBy>,
) -> Result<()> {
    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        println!("No allocations found. Run: pftui setup");
        return Ok(());
    }

    let positions = compute_positions_from_allocations(&allocs, prices);

    match group_by {
        Some(SummaryGroupBy::Category) => print_grouped_by_category_pct(&positions),
        None => print_percentage_table(&positions),
    }
}

fn print_full_table(positions: &[Position], config: &Config) -> Result<()> {
    println!(
        "{:<8} {:<10} {:>8} {:>10} {:>10} {:>8} {:>8}",
        "Symbol", "Category", "Qty", "Cost", "Price", "Gain%", "Alloc%"
    );
    println!("{}", "-".repeat(70));

    let mut total_value = dec!(0);
    let mut total_cost = dec!(0);

    for pos in positions {
        let price_str = pos.current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());
        let gain_str = pos.gain_pct
            .map(|g| format!("{:+.1}%", g))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos.allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>8} {:>10.2} {:>10} {:>8} {:>8}",
            pos.symbol,
            pos.category,
            pos.quantity,
            pos.avg_cost,
            price_str,
            gain_str,
            alloc_str,
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

    if positions.iter().all(|p| p.current_price.is_none() || p.category == AssetCategory::Cash) {
        println!("\nNote: No cached prices. Run `pftui refresh` to fetch live prices.");
    }

    Ok(())
}

fn print_percentage_table(positions: &[Position]) -> Result<()> {
    println!(
        "{:<8} {:<10} {:>10} {:>8}",
        "Symbol", "Category", "Price", "Alloc%"
    );
    println!("{}", "-".repeat(40));

    for pos in positions {
        let price_str = pos.current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos.allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>10} {:>8}",
            pos.symbol, pos.category, price_str, alloc_str,
        );
    }

    Ok(())
}

/// Group positions by asset category and display allocation summary.
fn print_grouped_by_category(positions: &[Position], config: &Config) -> Result<()> {
    let groups = group_by_category(positions);

    let total_value: Decimal = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum();

    let total_cost: Decimal = positions
        .iter()
        .map(|p| p.total_cost)
        .sum();

    // Sort groups by value descending
    let mut sorted_groups: Vec<_> = groups.into_iter().collect();
    sorted_groups.sort_by(|a, b| b.1.value.cmp(&a.1.value));

    println!(
        "{:<12} {:>12} {:>10} {:>8} {:>8}",
        "Category", "Value", "Cost", "Gain%", "Alloc%"
    );
    println!("{}", "─".repeat(54));

    for (category, group) in &sorted_groups {
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

    let priced = positions.iter().filter(|p| p.current_price.is_some()).count();
    let total = positions.len();
    if priced < total {
        println!(
            "\nNote: {}/{} positions have prices. Run `pftui refresh` for live data.",
            priced, total
        );
    }

    Ok(())
}

/// Group percentage-mode positions by category.
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

fn group_by_category(positions: &[Position]) -> HashMap<AssetCategory, CategoryGroup> {
    let mut groups: HashMap<AssetCategory, CategoryGroup> = HashMap::new();

    for pos in positions {
        let group = groups.entry(pos.category).or_insert_with(|| CategoryGroup {
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

    groups
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
    use crate::models::asset::AssetCategory;

    fn make_position(
        symbol: &str,
        category: AssetCategory,
        qty: Decimal,
        avg_cost: Decimal,
        current_price: Option<Decimal>,
    ) -> Position {
        let total_cost = qty * avg_cost;
        let current_value = current_price.map(|p| p * qty);
        let gain = current_value.map(|v| v - total_cost);
        let gain_pct = if total_cost > dec!(0) {
            gain.map(|g| (g / total_cost) * dec!(100))
        } else {
            None
        };
        Position {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            category,
            quantity: qty,
            avg_cost,
            total_cost,
            currency: "USD".to_string(),
            current_price,
            current_value,
            gain,
            gain_pct,
            allocation_pct: None,
        }
    }

    #[test]
    fn test_group_by_category_single() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200))),
        ];
        let groups = group_by_category(&positions);
        assert_eq!(groups.len(), 1);
        let eq = groups.get(&AssetCategory::Equity).unwrap();
        assert_eq!(eq.value, dec!(2000));
        assert_eq!(eq.cost, dec!(1500));
        assert_eq!(eq.symbols, vec!["AAPL"]);
    }

    #[test]
    fn test_group_by_category_multiple_same() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200))),
            make_position("GOOG", AssetCategory::Equity, dec!(5), dec!(100), Some(dec!(120))),
        ];
        let groups = group_by_category(&positions);
        assert_eq!(groups.len(), 1);
        let eq = groups.get(&AssetCategory::Equity).unwrap();
        assert_eq!(eq.value, dec!(2600)); // 2000 + 600
        assert_eq!(eq.cost, dec!(2000)); // 1500 + 500
        assert_eq!(eq.symbols.len(), 2);
    }

    #[test]
    fn test_group_by_category_mixed() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(100), Some(dec!(150))),
            make_position("BTC", AssetCategory::Crypto, dec!(1), dec!(30000), Some(dec!(85000))),
            make_position("USD", AssetCategory::Cash, dec!(50000), dec!(1), Some(dec!(1))),
        ];
        let groups = group_by_category(&positions);
        assert_eq!(groups.len(), 3);

        let equity = groups.get(&AssetCategory::Equity).unwrap();
        assert_eq!(equity.value, dec!(1500));

        let crypto = groups.get(&AssetCategory::Crypto).unwrap();
        assert_eq!(crypto.value, dec!(85000));

        let cash = groups.get(&AssetCategory::Cash).unwrap();
        assert_eq!(cash.value, dec!(50000));
    }

    #[test]
    fn test_group_by_category_no_price() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(100), None),
        ];
        let groups = group_by_category(&positions);
        let eq = groups.get(&AssetCategory::Equity).unwrap();
        assert_eq!(eq.value, dec!(0)); // no price = no value added
        assert_eq!(eq.cost, dec!(1000));
    }

    #[test]
    fn test_format_category() {
        assert_eq!(format_category(&AssetCategory::Equity), "Equity");
        assert_eq!(format_category(&AssetCategory::Crypto), "Crypto");
        assert_eq!(format_category(&AssetCategory::Forex), "Forex");
        assert_eq!(format_category(&AssetCategory::Cash), "Cash");
        assert_eq!(format_category(&AssetCategory::Commodity), "Commodity");
        assert_eq!(format_category(&AssetCategory::Fund), "Fund");
    }

    #[test]
    fn test_group_by_category_empty() {
        let positions: Vec<Position> = vec![];
        let groups = group_by_category(&positions);
        assert!(groups.is_empty());
    }
}
