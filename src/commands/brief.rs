use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations;
use crate::db::price_cache::get_all_cached_prices;
use crate::db::price_history::{get_history, get_prices_at_date};
use crate::db::transactions::list_transactions;
use crate::indicators::macd::{compute_macd, MacdResult};
use crate::indicators::rsi::compute_rsi;
use crate::indicators::sma::compute_sma;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

/// Format a decimal with commas as thousands separators.
fn fmt_commas(value: Decimal, dp: u32) -> String {
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

/// Format a currency value with symbol prefix.
fn fmt_currency(value: Decimal, dp: u32, base: &str) -> String {
    let sym = crate::config::currency_symbol(base);
    format!("{}{}", sym, fmt_commas(value, dp))
}

/// Compute percent change between two values.
fn pct_change(current: Decimal, previous: Decimal) -> Option<Decimal> {
    if previous > dec!(0) {
        Some(((current - previous) / previous) * dec!(100))
    } else {
        None
    }
}

pub fn run(conn: &Connection, config: &Config, technicals: bool) -> Result<()> {
    let cached = get_all_cached_prices(conn)?;
    let prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();

    // Get 1-day historical prices for top movers
    let today = Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let symbols: Vec<String> = prices.keys().cloned().collect();
    let hist_1d = get_prices_at_date(conn, &symbols, &yesterday_str).unwrap_or_default();

    // Load price history for technicals if requested
    let technicals_data = if technicals {
        compute_technicals_for_symbols(conn, &symbols)
    } else {
        HashMap::new()
    };

    match config.portfolio_mode {
        PortfolioMode::Full => run_full(conn, config, &prices, &hist_1d, &technicals_data),
        PortfolioMode::Percentage => run_percentage(conn, config, &prices, &hist_1d, &technicals_data),
    }
}

fn run_full(
    conn: &Connection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<()> {
    let txs = list_transactions(conn)?;
    if txs.is_empty() {
        println!("# Portfolio Brief\n\nNo positions. Add one with: `pftui add-tx`");
        return Ok(());
    }

    let positions = compute_positions(&txs, prices);
    if positions.is_empty() {
        println!("# Portfolio Brief\n\nNo open positions.");
        return Ok(());
    }

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = positions.iter().map(|p| p.total_cost).sum();
    let total_gain = total_value - total_cost;
    let total_gain_pct = pct_change(total_value, total_cost).unwrap_or(dec!(0));
    let base = &config.base_currency;

    let priced_count = positions.iter().filter(|p| p.current_price.is_some()).count();
    let total_count = positions.len();

    // Compute daily P&L
    let mut daily_pnl = dec!(0);
    let mut has_daily = false;
    for pos in &positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(p) => p,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(p) => *p,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        daily_pnl += (current - prev) * pos.quantity;
        has_daily = true;
    }

    // Date header
    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);

    // Total value line
    let sign = if total_gain >= dec!(0) { "+" } else { "" };
    println!(
        "**{}** ({}{} / {}{}%)",
        fmt_currency(total_value, 2, base),
        sign,
        fmt_commas(total_gain, 2),
        sign,
        total_gain_pct.round_dp(1),
    );

    // Daily P&L line
    if has_daily {
        let day_sign = if daily_pnl >= dec!(0) { "+" } else { "" };
        let day_pct = if total_value > dec!(0) {
            (daily_pnl / (total_value - daily_pnl)) * dec!(100)
        } else {
            dec!(0)
        };
        println!(
            "**1D:** {}{} ({}{}%)",
            day_sign,
            fmt_currency(daily_pnl.abs(), 2, base),
            day_sign,
            day_pct.round_dp(2),
        );
    }
    println!();

    // Category allocation
    print_category_allocation(&positions, total_value);

    // Top movers (by daily change %)
    print_top_movers(&positions, hist_1d, base);

    // Position table
    print_position_table_full(&positions, base, hist_1d);

    // Technicals section
    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }

    // Warnings
    if priced_count < total_count {
        let missing = total_count - priced_count;
        println!(
            "\n> ⚠️ {}/{} positions missing prices. Run `pftui refresh`.",
            missing, total_count
        );
    }

    Ok(())
}

fn run_percentage(
    conn: &Connection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    hist_1d: &HashMap<String, Decimal>,
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) -> Result<()> {
    let allocs = list_allocations(conn)?;
    if allocs.is_empty() {
        println!("# Portfolio Brief\n\nNo allocations. Run: `pftui setup`");
        return Ok(());
    }

    let positions = compute_positions_from_allocations(&allocs, prices);
    let base = &config.base_currency;

    let priced: Vec<_> = positions.iter().filter(|p| p.current_price.is_some()).collect();
    if priced.is_empty() {
        println!("# Portfolio Brief\n\nNo prices cached. Run `pftui refresh` first.");
        return Ok(());
    }

    let date_str = Utc::now().format("%Y-%m-%d").to_string();
    println!("# Portfolio Brief — {}\n", date_str);
    println!("*Percentage mode (allocation-based)*\n");

    // Category allocation (use raw pct since no total value)
    print_category_allocation_pct(&positions);

    // Top movers
    print_top_movers(&positions, hist_1d, base);

    // Position table for percentage mode
    println!("## Positions\n");
    println!("| Symbol | Category | Price | 1D | Alloc |");
    println!("|--------|----------|------:|---:|------:|");
    for pos in &positions {
        let price_str = pos
            .current_price
            .map(|p| fmt_currency(p, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "—".to_string());
        let name = resolve_name(&pos.symbol);
        let symbol_display = if name.is_empty() {
            pos.symbol.clone()
        } else {
            format!("{} ({})", pos.symbol, name)
        };
        let day_str = if pos.category == AssetCategory::Cash {
            "—".to_string()
        } else {
            match (pos.current_price, hist_1d.get(&pos.symbol)) {
                (Some(current), Some(prev)) if *prev > dec!(0) => {
                    let pct = ((current - prev) / prev) * dec!(100);
                    format!("{:+.1}%", pct)
                }
                _ => "—".to_string(),
            }
        };
        println!("| {} | {} | {} | {} | {} |", symbol_display, pos.category, price_str, day_str, alloc_str);
    }

    // Technicals section
    if !technicals_data.is_empty() {
        print_technicals_section(&positions, technicals_data);
    }

    let missing = positions.len() - priced.len();
    if missing > 0 {
        println!(
            "\n> ⚠️ {}/{} positions missing prices. Run `pftui refresh`.",
            missing,
            positions.len()
        );
    }

    Ok(())
}

// ──────────────────────────────────────────────────────────────
// Technicals
// ──────────────────────────────────────────────────────────────

/// Snapshot of technical indicator values for a single symbol.
#[derive(Debug)]
struct TechnicalSnapshot {
    rsi_14: Option<f64>,
    macd: Option<MacdResult>,
    sma_50: Option<f64>,
    sma_200: Option<f64>,
}

/// Label the RSI value for quick reading.
fn rsi_label(rsi: f64) -> &'static str {
    if rsi >= 70.0 {
        "overbought"
    } else if rsi <= 30.0 {
        "oversold"
    } else {
        "neutral"
    }
}

/// Label the MACD signal.
fn macd_label(m: &MacdResult) -> &'static str {
    if m.histogram > 0.0 {
        "bullish"
    } else if m.histogram < 0.0 {
        "bearish"
    } else {
        "neutral"
    }
}

/// Compute technical indicators for a list of symbols from cached price history.
fn compute_technicals_for_symbols(
    conn: &Connection,
    symbols: &[String],
) -> HashMap<String, TechnicalSnapshot> {
    let mut result = HashMap::new();

    for symbol in symbols {
        // Need at least 200 days for SMA-200; fetch 250 to be safe
        let history = match get_history(conn, symbol, 250) {
            Ok(h) if h.len() >= 14 => h,
            _ => continue,
        };

        let closes: Vec<f64> = history
            .iter()
            .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
            .collect();

        let rsi_values = compute_rsi(&closes, 14);
        let rsi_14 = rsi_values.iter().rev().find_map(|v| *v);

        let macd_values = compute_macd(&closes, 12, 26, 9);
        let macd = macd_values.iter().rev().find_map(|v| *v);

        let sma_50_values = compute_sma(&closes, 50);
        let sma_50 = sma_50_values.iter().rev().find_map(|v| *v);

        let sma_200_values = compute_sma(&closes, 200);
        let sma_200 = sma_200_values.iter().rev().find_map(|v| *v);

        result.insert(
            symbol.clone(),
            TechnicalSnapshot {
                rsi_14,
                macd,
                sma_50,
                sma_200,
            },
        );
    }

    result
}

/// Print a technicals section for all positions that have indicator data.
fn print_technicals_section(
    positions: &[Position],
    technicals_data: &HashMap<String, TechnicalSnapshot>,
) {
    // Only show positions that have technicals (skip cash)
    let relevant: Vec<&Position> = positions
        .iter()
        .filter(|p| {
            p.category != AssetCategory::Cash && technicals_data.contains_key(&p.symbol)
        })
        .collect();

    if relevant.is_empty() {
        return;
    }

    println!("## Technicals\n");
    println!("| Symbol | RSI(14) | Signal | MACD | Hist | SMA(50) | SMA(200) |");
    println!("|--------|--------:|--------|-----:|-----:|--------:|---------:|");

    for pos in &relevant {
        let snap = match technicals_data.get(&pos.symbol) {
            Some(s) => s,
            None => continue,
        };

        let rsi_str = snap
            .rsi_14
            .map(|v| format!("{:.1}", v))
            .unwrap_or_else(|| "—".to_string());

        let rsi_sig = snap
            .rsi_14
            .map(|v| rsi_label(v).to_string())
            .unwrap_or_else(|| "—".to_string());

        let macd_str = snap
            .macd
            .map(|m| format!("{:.2}", m.macd))
            .unwrap_or_else(|| "—".to_string());

        let hist_str = snap
            .macd
            .map(|m| {
                let sign = if m.histogram >= 0.0 { "+" } else { "" };
                format!("{}{:.2} ({})", sign, m.histogram, macd_label(&m))
            })
            .unwrap_or_else(|| "—".to_string());

        let sma50_str = snap
            .sma_50
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());

        let sma200_str = snap
            .sma_200
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());

        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            pos.symbol, rsi_str, rsi_sig, macd_str, hist_str, sma50_str, sma200_str,
        );
    }
    println!();
}

// ──────────────────────────────────────────────────────────────
// Shared markdown sections
// ──────────────────────────────────────────────────────────────

fn print_category_allocation(positions: &[Position], total_value: Decimal) {
    let mut categories: HashMap<AssetCategory, Decimal> = HashMap::new();

    for pos in positions {
        if let Some(val) = pos.current_value {
            *categories.entry(pos.category).or_insert(dec!(0)) += val;
        }
    }

    if categories.is_empty() || total_value <= dec!(0) {
        return;
    }

    let mut sorted: Vec<_> = categories.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("## Allocation\n");

    let parts: Vec<String> = sorted
        .iter()
        .map(|(cat, val)| {
            let pct = (val / total_value * dec!(100)).round_dp(0);
            format!("**{}** {}%", format_category(cat), pct)
        })
        .collect();

    println!("{}\n", parts.join(" · "));
}

fn print_category_allocation_pct(positions: &[Position]) {
    let mut categories: HashMap<AssetCategory, Decimal> = HashMap::new();

    for pos in positions {
        if let Some(alloc) = pos.allocation_pct {
            *categories.entry(pos.category).or_insert(dec!(0)) += alloc;
        }
    }

    if categories.is_empty() {
        return;
    }

    let mut sorted: Vec<_> = categories.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("## Allocation\n");

    let parts: Vec<String> = sorted
        .iter()
        .map(|(cat, pct)| {
            format!("**{}** {}%", format_category(cat), pct.round_dp(0))
        })
        .collect();

    println!("{}\n", parts.join(" · "));
}

fn print_top_movers(
    positions: &[Position],
    hist_1d: &HashMap<String, Decimal>,
    base: &str,
) {
    let mut movers: Vec<(&str, Decimal, Decimal)> = Vec::new(); // (symbol, current, pct_change)

    for pos in positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current = match pos.current_price {
            Some(p) => p,
            None => continue,
        };
        let prev = match hist_1d.get(&pos.symbol) {
            Some(p) => *p,
            None => continue,
        };
        if prev <= dec!(0) {
            continue;
        }
        let pct = ((current - prev) / prev) * dec!(100);
        movers.push((&pos.symbol, current, pct));
    }

    if movers.is_empty() {
        return;
    }

    // Sort by absolute change descending
    movers.sort_by(|a, b| b.2.abs().partial_cmp(&a.2.abs()).unwrap_or(std::cmp::Ordering::Equal));

    println!("## Top Movers (1D)\n");

    let count = movers.len().min(5);
    for (symbol, current, pct) in &movers[..count] {
        let direction = if *pct >= dec!(0) { "📈" } else { "📉" };
        let name = resolve_name(symbol);
        let label = if name.is_empty() {
            symbol.to_string()
        } else {
            format!("{} ({})", symbol, name)
        };
        println!(
            "- {} **{}** {} ({:+.1}%)",
            direction,
            label,
            fmt_currency(*current, 2, base),
            pct,
        );
    }
    println!();
}

fn print_position_table_full(
    positions: &[Position],
    base: &str,
    hist_1d: &HashMap<String, Decimal>,
) {
    println!("## Positions\n");
    println!("| Symbol | Category | Qty | Price | Value | Gain | 1D | Alloc |");
    println!("|--------|----------|----:|------:|------:|-----:|---:|------:|");

    for pos in positions {
        let name = resolve_name(&pos.symbol);
        let symbol_display = if name.is_empty() {
            pos.symbol.clone()
        } else {
            format!("{} ({})", pos.symbol, name)
        };
        let price_str = pos
            .current_price
            .map(|p| fmt_currency(p, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let value_str = pos
            .current_value
            .map(|v| fmt_currency(v, 2, base))
            .unwrap_or_else(|| "N/A".to_string());
        let gain_str = pos
            .gain_pct
            .map(|g| format!("{:+.1}%", g))
            .unwrap_or_else(|| "—".to_string());
        let alloc_str = pos
            .allocation_pct
            .map(|a| format!("{:.1}%", a))
            .unwrap_or_else(|| "—".to_string());

        // 1D change
        let day_str = if pos.category == AssetCategory::Cash {
            "—".to_string()
        } else {
            match (pos.current_price, hist_1d.get(&pos.symbol)) {
                (Some(current), Some(prev)) if *prev > dec!(0) => {
                    let pct = ((current - prev) / prev) * dec!(100);
                    format!("{:+.1}%", pct)
                }
                _ => "—".to_string(),
            }
        };

        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            symbol_display, pos.category, pos.quantity, price_str, value_str, gain_str, day_str,
            alloc_str,
        );
    }
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

    #[test]
    fn fmt_commas_basic() {
        assert_eq!(fmt_commas(dec!(1234567.89), 2), "1,234,567.89");
    }

    #[test]
    fn fmt_commas_small() {
        assert_eq!(fmt_commas(dec!(42.50), 2), "42.50");
    }

    #[test]
    fn fmt_commas_negative() {
        assert_eq!(fmt_commas(dec!(-1234.56), 2), "-1,234.56");
    }

    #[test]
    fn fmt_commas_zero() {
        assert_eq!(fmt_commas(dec!(0), 2), "0.00");
    }

    #[test]
    fn fmt_currency_usd() {
        assert_eq!(fmt_currency(dec!(1234.56), 2, "USD"), "$1,234.56");
    }

    #[test]
    fn fmt_currency_gbp() {
        assert_eq!(fmt_currency(dec!(1234.56), 2, "GBP"), "£1,234.56");
    }

    #[test]
    fn fmt_currency_eur() {
        assert_eq!(fmt_currency(dec!(500.00), 2, "EUR"), "€500.00");
    }

    #[test]
    fn fmt_currency_unknown() {
        // Unknown currencies use the code as prefix
        assert_eq!(fmt_currency(dec!(100.00), 2, "XYZ"), "XYZ100.00");
    }

    #[test]
    fn pct_change_positive() {
        let result = pct_change(dec!(110), dec!(100));
        assert_eq!(result, Some(dec!(10)));
    }

    #[test]
    fn pct_change_negative() {
        let result = pct_change(dec!(90), dec!(100));
        assert_eq!(result, Some(dec!(-10)));
    }

    #[test]
    fn pct_change_zero_base() {
        let result = pct_change(dec!(100), dec!(0));
        assert_eq!(result, None);
    }

    #[test]
    fn brief_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();
        let result = run(&conn, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_with_positions_no_prices() {
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

        let result = run(&conn, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_with_positions_and_prices() {
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

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(200),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(85000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let result = run(&conn, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_percentage_mode() {
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
                price: dec!(85000),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".to_string(),
                price: dec!(2500),
                currency: "USD".to_string(),
                source: "test".to_string(),
                fetched_at: "2025-06-15T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let result = run(&conn, &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn brief_percentage_mode_no_prices() {
        let conn = crate::db::open_in_memory();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Default::default()
        };

        use crate::db::allocations::insert_allocation;
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(50)).unwrap();

        let result = run(&conn, &config, false);
        assert!(result.is_ok());
    }

    fn make_position(
        symbol: &str,
        category: AssetCategory,
        qty: Decimal,
        avg_cost: Decimal,
        current_price: Option<Decimal>,
        total_value_for_alloc: Option<Decimal>,
    ) -> Position {
        let total_cost = qty * avg_cost;
        let current_value = current_price.map(|p| p * qty);
        let gain = current_value.map(|v| v - total_cost);
        let gain_pct = if total_cost > dec!(0) {
            gain.map(|g| (g / total_cost) * dec!(100))
        } else {
            None
        };
        let allocation_pct = match (current_value, total_value_for_alloc) {
            (Some(v), Some(tv)) if tv > dec!(0) => Some((v / tv) * dec!(100)),
            _ => None,
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
            allocation_pct,
        }
    }

    #[test]
    fn top_movers_sorts_by_absolute_change() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200)), Some(dec!(100000))),
            make_position("GOOG", AssetCategory::Equity, dec!(5), dec!(100), Some(dec!(90)), Some(dec!(100000))),
            make_position("BTC", AssetCategory::Crypto, dec!(1), dec!(30000), Some(dec!(85000)), Some(dec!(100000))),
        ];

        let mut hist_1d: HashMap<String, Decimal> = HashMap::new();
        hist_1d.insert("AAPL".to_string(), dec!(195));
        hist_1d.insert("GOOG".to_string(), dec!(100));
        hist_1d.insert("BTC".to_string(), dec!(83000));

        // Verify it doesn't panic — output goes to stdout
        print_top_movers(&positions, &hist_1d, "USD");
    }

    #[test]
    fn category_allocation_groups_correctly() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(100), Some(dec!(150)), Some(dec!(2600))),
            make_position("GOOG", AssetCategory::Equity, dec!(5), dec!(100), Some(dec!(120)), Some(dec!(2600))),
            make_position("BTC", AssetCategory::Crypto, dec!(1), dec!(500), Some(dec!(1000)), Some(dec!(2600))),
        ];

        // Verify it doesn't panic — output goes to stdout
        print_category_allocation(&positions, dec!(2600));
    }

    #[test]
    fn technicals_section_skips_cash_positions() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200)), Some(dec!(100000))),
            make_position("USD", AssetCategory::Cash, dec!(50000), dec!(1), Some(dec!(1)), Some(dec!(100000))),
        ];

        let mut technicals = HashMap::new();
        technicals.insert(
            "AAPL".to_string(),
            TechnicalSnapshot {
                rsi_14: Some(55.0),
                macd: Some(MacdResult { macd: 1.5, signal: 1.0, histogram: 0.5 }),
                sma_50: Some(190.0),
                sma_200: Some(175.0),
            },
        );

        // Should not panic and should skip USD
        print_technicals_section(&positions, &technicals);
    }

    #[test]
    fn technicals_section_empty_data_produces_no_output() {
        let positions = vec![
            make_position("AAPL", AssetCategory::Equity, dec!(10), dec!(150), Some(dec!(200)), Some(dec!(100000))),
        ];

        let technicals: HashMap<String, TechnicalSnapshot> = HashMap::new();

        // Should not produce output when no technicals data
        print_technicals_section(&positions, &technicals);
    }

    #[test]
    fn rsi_label_categories() {
        assert_eq!(rsi_label(75.0), "overbought");
        assert_eq!(rsi_label(70.0), "overbought");
        assert_eq!(rsi_label(25.0), "oversold");
        assert_eq!(rsi_label(30.0), "oversold");
        assert_eq!(rsi_label(50.0), "neutral");
    }

    #[test]
    fn macd_label_categories() {
        let bullish = MacdResult { macd: 1.0, signal: 0.5, histogram: 0.5 };
        assert_eq!(macd_label(&bullish), "bullish");
        let bearish = MacdResult { macd: -1.0, signal: -0.5, histogram: -0.5 };
        assert_eq!(macd_label(&bearish), "bearish");
        let neutral = MacdResult { macd: 0.0, signal: 0.0, histogram: 0.0 };
        assert_eq!(macd_label(&neutral), "neutral");
    }

    #[test]
    fn brief_with_technicals_flag() {
        let conn = crate::db::open_in_memory();
        let config = Config::default();

        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::{NewTransaction, TxType};
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

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
                fetched_at: "2025-06-15T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        // With technicals=true, should succeed (no history means no indicators displayed)
        let result = run(&conn, &config, true);
        assert!(result.is_ok());
    }
}
