use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::str::FromStr;
use rusqlite::Connection;

use crate::cli::{SummaryGroupBy, SummaryPeriod};
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::price_cache::get_all_cached_prices;
use crate::db::price_history::get_prices_at_date;
use crate::db::transactions::list_transactions;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

/// Parse a what-if override string like "GC=F:5500,BTC:55000" into symbol→price pairs.
fn parse_what_if(input: &str) -> Result<HashMap<String, Decimal>> {
    let mut overrides = HashMap::new();
    for pair in input.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        // Split on last ':' to handle symbols containing '=' etc.
        let colon_pos = pair.rfind(':').ok_or_else(|| {
            anyhow::anyhow!("Invalid --what-if pair '{}': expected SYMBOL:PRICE", pair)
        })?;
        let symbol = pair[..colon_pos].trim().to_uppercase();
        let price_str = pair[colon_pos + 1..].trim();
        let price = Decimal::from_str(price_str).map_err(|_| {
            anyhow::anyhow!("Invalid price '{}' for symbol '{}' in --what-if", price_str, symbol)
        })?;
        if price < dec!(0) {
            anyhow::bail!("Negative price '{}' for symbol '{}' in --what-if", price, symbol);
        }
        overrides.insert(symbol, price);
    }
    if overrides.is_empty() {
        anyhow::bail!("--what-if requires at least one SYMBOL:PRICE pair");
    }
    Ok(overrides)
}

pub fn run(
    conn: &Connection,
    config: &Config,
    group_by: Option<&SummaryGroupBy>,
    period: Option<&SummaryPeriod>,
    what_if: Option<&str>,
) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    // Apply hypothetical price overrides
    let overrides = match what_if {
        Some(input) => {
            let ov = parse_what_if(input)?;
            for (sym, price) in &ov {
                prices.insert(sym.clone(), *price);
            }
            Some(ov)
        }
        None => None,
    };

    // Compute the start date for period-based P&L
    let period_start = period.map(|p| {
        let today = Utc::now().date_naive();
        let start = today - chrono::Duration::days(p.days_back());
        start.format("%Y-%m-%d").to_string()
    });

    // Look up historical prices if period is requested
    let historical_prices = match (&period_start, period) {
        (Some(date), Some(_)) => {
            let symbols: Vec<String> = prices.keys().cloned().collect();
            Some(get_prices_at_date(conn, &symbols, date)?)
        }
        _ => None,
    };

    if let Some(ref ov) = overrides {
        print_what_if_banner(ov);
    }

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, &prices, group_by, period, &historical_prices),
        PortfolioMode::Percentage => run_percentage(conn, &prices, group_by, period, &historical_prices),
    }
}

/// Print a banner showing the hypothetical price overrides being applied.
fn print_what_if_banner(overrides: &HashMap<String, Decimal>) {
    println!("╔══════════════════════════════════════════╗");
    println!("║         ⚠  WHAT-IF SCENARIO  ⚠          ║");
    println!("╠══════════════════════════════════════════╣");
    let mut sorted: Vec<_> = overrides.iter().collect();
    sorted.sort_by_key(|(sym, _)| (*sym).clone());
    for (symbol, price) in &sorted {
        println!("║  {:<12} → {:>24.2}  ║", symbol, price);
    }
    println!("╚══════════════════════════════════════════╝");
    println!();
}

fn run_full(
    conn: &Connection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    group_by: Option<&SummaryGroupBy>,
    period: Option<&SummaryPeriod>,
    historical_prices: &Option<HashMap<String, Decimal>>,
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

    match (group_by, period) {
        (Some(SummaryGroupBy::Category), Some(p)) => {
            print_grouped_by_category_with_period(&positions, config, p, historical_prices)
        }
        (Some(SummaryGroupBy::Category), None) => print_grouped_by_category(&positions, config),
        (None, Some(p)) => print_full_table_with_period(&positions, config, p, historical_prices),
        (None, None) => print_full_table(&positions, config),
    }
}

fn run_percentage(
    conn: &Connection,
    prices: &HashMap<String, Decimal>,
    group_by: Option<&SummaryGroupBy>,
    period: Option<&SummaryPeriod>,
    historical_prices: &Option<HashMap<String, Decimal>>,
) -> Result<()> {
    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        println!("No allocations found. Run: pftui setup");
        return Ok(());
    }

    let positions = compute_positions_from_allocations(&allocs, prices);

    match (group_by, period) {
        (Some(SummaryGroupBy::Category), Some(p)) => {
            print_grouped_by_category_pct_with_period(&positions, p, historical_prices)
        }
        (Some(SummaryGroupBy::Category), None) => print_grouped_by_category_pct(&positions),
        (None, Some(p)) => print_percentage_table_with_period(&positions, p, historical_prices),
        (None, None) => print_percentage_table(&positions),
    }
}

// ──────────────────────────────────────────────────────────────
// Full mode: default table
// ──────────────────────────────────────────────────────────────

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

/// Full mode table with period-based P&L instead of cost-basis gain.
fn print_full_table_with_period(
    positions: &[Position],
    config: &Config,
    period: &SummaryPeriod,
    historical_prices: &Option<HashMap<String, Decimal>>,
) -> Result<()> {
    let label = period.label();
    println!(
        "{:<8} {:<10} {:>8} {:>10} {:>10} {:>10} {:>8}",
        "Symbol", "Category", "Qty", "Price", format!("{} ago", label), format!("Chg {}", label), "Alloc%"
    );
    println!("{}", "-".repeat(74));

    let hist = historical_prices.as_ref();
    let mut total_value = dec!(0);
    let mut total_prev_value = dec!(0);
    let mut has_period_data = false;

    for pos in positions {
        let price_str = pos.current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());

        let prev_price = if pos.category == AssetCategory::Cash {
            Some(dec!(1))
        } else {
            hist.and_then(|h| h.get(&pos.symbol).copied())
        };

        let prev_str = prev_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());

        let change_str = match (pos.current_price, prev_price) {
            (Some(cur), Some(prev)) if prev > dec!(0) => {
                has_period_data = true;
                let pct = ((cur - prev) / prev) * dec!(100);
                format!("{:+.1}%", pct)
            }
            _ => "N/A".to_string(),
        };

        let alloc_str = pos.allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>8} {:>10} {:>10} {:>10} {:>8}",
            pos.symbol,
            pos.category,
            pos.quantity,
            price_str,
            prev_str,
            change_str,
            alloc_str,
        );

        if let Some(v) = pos.current_value {
            total_value += v;
        }
        if let Some(prev) = prev_price {
            total_prev_value += prev * pos.quantity;
        }
    }

    println!("{}", "-".repeat(74));

    let period_change = total_value - total_prev_value;
    let period_pct = if total_prev_value > dec!(0) {
        (period_change / total_prev_value) * dec!(100)
    } else {
        dec!(0)
    };

    println!(
        "Total Value: {:.2} {}  |  {} P&L: {:+.2} ({:+.1}%)",
        total_value, config.base_currency, label, period_change, period_pct
    );

    if !has_period_data {
        println!(
            "\nNote: No price history for {} period. Run `pftui refresh` and try again later.",
            label
        );
    }

    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Full mode: grouped by category
// ──────────────────────────────────────────────────────────────

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

/// Grouped by category with period-based P&L.
fn print_grouped_by_category_with_period(
    positions: &[Position],
    config: &Config,
    period: &SummaryPeriod,
    historical_prices: &Option<HashMap<String, Decimal>>,
) -> Result<()> {
    let hist = historical_prices.as_ref();
    let label = period.label();

    let total_value: Decimal = positions
        .iter()
        .filter_map(|p| p.current_value)
        .sum();

    // Build category groups with period data
    let mut groups: HashMap<AssetCategory, CategoryPeriodGroup> = HashMap::new();

    for pos in positions {
        let group = groups.entry(pos.category).or_insert_with(|| CategoryPeriodGroup {
            value: dec!(0),
            prev_value: dec!(0),
            symbols: Vec::new(),
        });

        if let Some(v) = pos.current_value {
            group.value += v;
        }

        let prev_price = if pos.category == AssetCategory::Cash {
            Some(dec!(1))
        } else {
            hist.and_then(|h| h.get(&pos.symbol).copied())
        };
        if let Some(prev) = prev_price {
            group.prev_value += prev * pos.quantity;
        }

        group.symbols.push(pos.symbol.clone());
    }

    let mut sorted: Vec<_> = groups.into_iter().collect();
    sorted.sort_by(|a, b| b.1.value.cmp(&a.1.value));

    println!(
        "{:<12} {:>12} {:>12} {:>10} {:>8}",
        "Category", "Value", format!("{} ago", label), format!("Chg {}", label), "Alloc%"
    );
    println!("{}", "─".repeat(58));

    for (category, group) in &sorted {
        let alloc_pct = if total_value > dec!(0) {
            (group.value / total_value) * dec!(100)
        } else {
            dec!(0)
        };

        let change_pct = if group.prev_value > dec!(0) {
            ((group.value - group.prev_value) / group.prev_value) * dec!(100)
        } else {
            dec!(0)
        };

        let symbols_str = group.symbols.join(", ");

        println!(
            "{:<12} {:>12.2} {:>12.2} {:>+9.1}% {:>6.1}%",
            format_category(category),
            group.value,
            group.prev_value,
            change_pct,
            alloc_pct,
        );
        println!("  {}", symbols_str);
    }

    println!("{}", "─".repeat(58));

    let total_prev: Decimal = sorted.iter().map(|(_, g)| g.prev_value).sum();
    let period_change = total_value - total_prev;
    let period_pct = if total_prev > dec!(0) {
        (period_change / total_prev) * dec!(100)
    } else {
        dec!(0)
    };

    println!(
        "Total: {:.2} {}  |  {} P&L: {:+.2} ({:+.1}%)",
        total_value, config.base_currency, label, period_change, period_pct
    );

    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Percentage mode
// ──────────────────────────────────────────────────────────────

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

/// Percentage mode with period-based price changes.
fn print_percentage_table_with_period(
    positions: &[Position],
    period: &SummaryPeriod,
    historical_prices: &Option<HashMap<String, Decimal>>,
) -> Result<()> {
    let hist = historical_prices.as_ref();
    let label = period.label();

    println!(
        "{:<8} {:<10} {:>10} {:>10} {:>10} {:>8}",
        "Symbol", "Category", "Price", format!("{} ago", label), format!("Chg {}", label), "Alloc%"
    );
    println!("{}", "-".repeat(60));

    for pos in positions {
        let price_str = pos.current_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());

        let prev_price = if pos.category == AssetCategory::Cash {
            Some(dec!(1))
        } else {
            hist.and_then(|h| h.get(&pos.symbol).copied())
        };

        let prev_str = prev_price
            .map(|p| format!("{:.2}", p))
            .unwrap_or_else(|| "N/A".to_string());

        let change_str = match (pos.current_price, prev_price) {
            (Some(cur), Some(prev)) if prev > dec!(0) => {
                let pct = ((cur - prev) / prev) * dec!(100);
                format!("{:+.1}%", pct)
            }
            _ => "N/A".to_string(),
        };

        let alloc_str = pos.allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "{:<8} {:<10} {:>10} {:>10} {:>10} {:>8}",
            pos.symbol, pos.category, price_str, prev_str, change_str, alloc_str,
        );
    }

    Ok(())
}

/// Percentage mode grouped by category.
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

/// Percentage mode grouped by category with period-based price changes.
fn print_grouped_by_category_pct_with_period(
    positions: &[Position],
    period: &SummaryPeriod,
    historical_prices: &Option<HashMap<String, Decimal>>,
) -> Result<()> {
    let hist = historical_prices.as_ref();
    let label = period.label();

    let mut category_data: HashMap<AssetCategory, CategoryPctPeriodGroup> = HashMap::new();

    for pos in positions {
        let entry = category_data
            .entry(pos.category)
            .or_insert_with(|| CategoryPctPeriodGroup { alloc: dec!(0), symbols: Vec::new() });
        if let Some(alloc) = pos.allocation_pct {
            entry.alloc += alloc;
        }

        let prev_price = if pos.category == AssetCategory::Cash {
            Some(dec!(1))
        } else {
            hist.and_then(|h| h.get(&pos.symbol).copied())
        };

        entry.symbols.push(SymbolPriceData {
            symbol: pos.symbol.clone(),
            current_price: pos.current_price,
            prev_price,
        });
    }

    let mut sorted: Vec<_> = category_data.into_iter().collect();
    sorted.sort_by(|a, b| b.1.alloc.cmp(&a.1.alloc));

    println!("{:<12} {:>8} {:>10}", "Category", "Alloc%", format!("Chg {}", label));
    println!("{}", "─".repeat(34));

    for (category, group) in &sorted {
        // Compute average change for the category (simple mean of % changes)
        let changes: Vec<Decimal> = group.symbols
            .iter()
            .filter_map(|spd| {
                match (spd.current_price, spd.prev_price) {
                    (Some(c), Some(p)) if p > dec!(0) => Some(((c - p) / p) * dec!(100)),
                    _ => None,
                }
            })
            .collect();

        let avg_change = if changes.is_empty() {
            "N/A".to_string()
        } else {
            let sum: Decimal = changes.iter().sum();
            let avg = sum / Decimal::from(changes.len() as i64);
            format!("{:+.1}%", avg)
        };

        let symbol_names: Vec<String> = group.symbols.iter().map(|spd| spd.symbol.clone()).collect();
        let symbols_str = symbol_names.join(", ");

        println!("{:<12} {:>6.1}% {:>10}", format_category(category), group.alloc, avg_change);
        println!("  {}", symbols_str);
    }

    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────

struct CategoryGroup {
    value: Decimal,
    cost: Decimal,
    symbols: Vec<String>,
}

struct CategoryPeriodGroup {
    value: Decimal,
    prev_value: Decimal,
    symbols: Vec<String>,
}

struct SymbolPriceData {
    symbol: String,
    current_price: Option<Decimal>,
    prev_price: Option<Decimal>,
}

struct CategoryPctPeriodGroup {
    alloc: Decimal,
    symbols: Vec<SymbolPriceData>,
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
        assert_eq!(eq.value, dec!(2600));
        assert_eq!(eq.cost, dec!(2000));
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
        assert_eq!(eq.value, dec!(0));
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

    #[test]
    fn test_period_days_back() {
        assert_eq!(SummaryPeriod::Today.days_back(), 1);
        assert_eq!(SummaryPeriod::OneWeek.days_back(), 7);
        assert_eq!(SummaryPeriod::OneMonth.days_back(), 30);
        assert_eq!(SummaryPeriod::ThreeMonths.days_back(), 90);
        assert_eq!(SummaryPeriod::OneYear.days_back(), 365);
    }

    #[test]
    fn test_period_label() {
        assert_eq!(SummaryPeriod::Today.label(), "today");
        assert_eq!(SummaryPeriod::OneWeek.label(), "1W");
        assert_eq!(SummaryPeriod::OneMonth.label(), "1M");
        assert_eq!(SummaryPeriod::ThreeMonths.label(), "3M");
        assert_eq!(SummaryPeriod::OneYear.label(), "1Y");
    }

    #[test]
    fn test_summary_with_period_no_history() {
        // When no historical prices exist, period output should still work (showing N/A)
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        insert_transaction(&conn, &NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        }).unwrap();

        upsert_price(&conn, &PriceQuote {
            symbol: "AAPL".to_string(),
            price: dec!(200),
            currency: "USD".to_string(),
            source: "test".to_string(),
            fetched_at: "2025-01-15T00:00:00Z".to_string(),
        }).unwrap();

        // Should succeed even with no history data
        let result = run(&conn, &config, None, Some(&SummaryPeriod::OneMonth), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_summary_with_period_and_history() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::db::price_history::upsert_history;
        use crate::models::price::{HistoryRecord, PriceQuote};

        insert_transaction(&conn, &NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        }).unwrap();

        upsert_price(&conn, &PriceQuote {
            symbol: "AAPL".to_string(),
            price: dec!(200),
            currency: "USD".to_string(),
            source: "test".to_string(),
            fetched_at: "2025-06-15T00:00:00Z".to_string(),
        }).unwrap();

        upsert_history(&conn, "AAPL", "yahoo", &[
            HistoryRecord { date: "2025-05-15".into(), close: dec!(180), volume: None },
            HistoryRecord { date: "2025-06-01".into(), close: dec!(190), volume: None },
        ]).unwrap();

        // Should succeed with historical data available
        let result = run(&conn, &config, None, Some(&SummaryPeriod::OneMonth), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_summary_with_period_and_group_by() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        insert_transaction(&conn, &NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        }).unwrap();

        upsert_price(&conn, &PriceQuote {
            symbol: "AAPL".to_string(),
            price: dec!(200),
            currency: "USD".to_string(),
            source: "test".to_string(),
            fetched_at: "2025-01-15T00:00:00Z".to_string(),
        }).unwrap();

        // Both --group-by category and --period together
        let result = run(&conn, &config, Some(&SummaryGroupBy::Category), Some(&SummaryPeriod::OneWeek), None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_what_if_single() {
        let overrides = parse_what_if("BTC:55000").unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides.get("BTC"), Some(&dec!(55000)));
    }

    #[test]
    fn test_parse_what_if_multiple() {
        let overrides = parse_what_if("GC=F:5500,BTC:55000,AAPL:250.50").unwrap();
        assert_eq!(overrides.len(), 3);
        assert_eq!(overrides.get("GC=F"), Some(&dec!(5500)));
        assert_eq!(overrides.get("BTC"), Some(&dec!(55000)));
        assert_eq!(overrides.get("AAPL"), Some(&Decimal::new(25050, 2)));
    }

    #[test]
    fn test_parse_what_if_case_insensitive() {
        let overrides = parse_what_if("btc:50000").unwrap();
        assert_eq!(overrides.get("BTC"), Some(&dec!(50000)));
    }

    #[test]
    fn test_parse_what_if_with_spaces() {
        let overrides = parse_what_if(" BTC : 55000 , AAPL : 200 ").unwrap();
        assert_eq!(overrides.len(), 2);
        assert_eq!(overrides.get("BTC"), Some(&dec!(55000)));
        assert_eq!(overrides.get("AAPL"), Some(&dec!(200)));
    }

    #[test]
    fn test_parse_what_if_empty_fails() {
        assert!(parse_what_if("").is_err());
    }

    #[test]
    fn test_parse_what_if_no_colon_fails() {
        assert!(parse_what_if("BTC55000").is_err());
    }

    #[test]
    fn test_parse_what_if_bad_price_fails() {
        assert!(parse_what_if("BTC:notanumber").is_err());
    }

    #[test]
    fn test_parse_what_if_negative_price_fails() {
        assert!(parse_what_if("BTC:-100").is_err());
    }

    #[test]
    fn test_parse_what_if_zero_price() {
        let overrides = parse_what_if("BTC:0").unwrap();
        assert_eq!(overrides.get("BTC"), Some(&dec!(0)));
    }

    #[test]
    fn test_what_if_overrides_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        insert_transaction(&conn, &NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(10),
            price_per: dec!(150),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        }).unwrap();

        upsert_price(&conn, &PriceQuote {
            symbol: "AAPL".to_string(),
            price: dec!(200),
            currency: "USD".to_string(),
            source: "test".to_string(),
            fetched_at: "2025-01-15T00:00:00Z".to_string(),
        }).unwrap();

        // With what-if override, should succeed and use hypothetical price
        let result = run(&conn, &config, None, None, Some("AAPL:300"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_what_if_with_group_by() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        insert_transaction(&conn, &NewTransaction {
            symbol: "BTC".to_string(),
            category: AssetCategory::Crypto,
            tx_type: TxType::Buy,
            quantity: dec!(1),
            price_per: dec!(30000),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
        }).unwrap();

        upsert_price(&conn, &PriceQuote {
            symbol: "BTC".to_string(),
            price: dec!(85000),
            currency: "USD".to_string(),
            source: "test".to_string(),
            fetched_at: "2025-01-15T00:00:00Z".to_string(),
        }).unwrap();

        // What-if + group-by should work together
        let result = run(&conn, &config, Some(&SummaryGroupBy::Category), None, Some("BTC:100000"));
        assert!(result.is_ok());
    }
}
