use std::collections::HashSet;
use std::io::{self, Write};

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rusqlite::Connection;

use crate::config::{save_config, Config, PortfolioMode, SUPPORTED_CURRENCIES};
use crate::db::{allocations, transactions};
use crate::models::asset::AssetCategory;
use crate::models::asset_names;
use crate::models::transaction::{NewTransaction, TxType};
use crate::price::{coingecko, yahoo};

struct SetupEntry {
    symbol: String,
    name: String,
    category: AssetCategory,
    value: Option<Decimal>, // full mode only
    pct: Decimal,
}

fn prompt(label: &str) -> Result<String> {
    print!("{}", label);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn confirm(msg: &str) -> Result<bool> {
    let answer = prompt(&format!("{} [y/N]: ", msg))?;
    Ok(answer.to_lowercase() == "y")
}

pub fn run(conn: &Connection, config: &Config, is_explicit: bool) -> Result<()> {
    // If explicit setup with existing data, warn
    if is_explicit {
        let tx_count = transactions::count_transactions(conn)?;
        let alloc_count = allocations::count_allocations(conn)?;
        if tx_count > 0 || alloc_count > 0 {
            println!();
            println!("  \x1b[33m! Warning: You have existing portfolio data.\x1b[0m");
            if tx_count > 0 {
                println!("    {} transactions", tx_count);
            }
            if alloc_count > 0 {
                println!("    {} allocations", alloc_count);
            }
            println!("    Setup will \x1b[1mdelete all existing data\x1b[0m.");
            println!();
            if !confirm("  Continue?")? {
                println!("  Cancelled.");
                return Ok(());
            }
            // Reset all data
            conn.execute_batch(
                "DELETE FROM transactions;
                 DELETE FROM portfolio_allocations;
                 DELETE FROM price_cache;
                 DELETE FROM price_history;",
            )?;
            println!("  Data cleared.\n");
        }
    }

    // Welcome
    println!();
    println!("  \x1b[1mWelcome to pftui setup!\x1b[0m");
    println!();

    // Currency selection
    println!("  \x1b[1mBase currency:\x1b[0m \x1b[90m(all values displayed in this currency)\x1b[0m");
    println!();
    let cols = 2;
    let per_col = SUPPORTED_CURRENCIES.len().div_ceil(cols);
    for row in 0..per_col {
        let mut line = String::from("    ");
        for col in 0..cols {
            let idx = row + col * per_col;
            if let Some((code, label)) = SUPPORTED_CURRENCIES.get(idx) {
                line.push_str(&format!(
                    "\x1b[1m[{:<3}]\x1b[0m {:<28}",
                    code, label
                ));
            }
        }
        println!("{}", line);
    }
    println!();
    let chosen_currency = loop {
        let input = prompt("  Currency code [USD]: ")?;
        let code = if input.is_empty() {
            "USD".to_string()
        } else {
            input.to_uppercase()
        };
        // Accept any code from our list, or any 3-letter code
        if SUPPORTED_CURRENCIES.iter().any(|(c, _)| *c == code.as_str()) {
            break code;
        }
        if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) {
            println!("    \x1b[90mCustom currency: {}. Prices will still be fetched in USD.\x1b[0m", code);
            break code;
        }
        println!("    \x1b[33mEnter a valid 3-letter currency code (e.g. USD, EUR, GBP).\x1b[0m");
    };
    println!("    → {}", chosen_currency);
    println!();

    // Mode selection
    println!("  Select portfolio mode:");
    println!("    \x1b[1m[1]\x1b[0m Full \x1b[90m— track values, quantities, and transactions\x1b[0m");
    println!("    \x1b[1m[2]\x1b[0m Percentage \x1b[90m— allocation percentages only (privacy)\x1b[0m");
    println!();
    println!("  \x1b[90mTip: Full mode lets you press 'p' anytime to hide values for privacy.\x1b[0m");
    println!("  \x1b[90m     Percentage mode cannot be switched to full mode later.\x1b[0m");
    println!();

    let mode = loop {
        let choice = prompt("  Select [1/2]: ")?;
        match choice.as_str() {
            "1" => break PortfolioMode::Full,
            "2" => break PortfolioMode::Percentage,
            _ => println!("  Please enter 1 or 2."),
        }
    };

    println!();

    // Build new config with chosen currency for use during setup
    let mut new_config = config.clone();
    new_config.base_currency = chosen_currency;
    new_config.portfolio_mode = mode;

    match mode {
        PortfolioMode::Full => full_mode_setup(conn, &new_config)?,
        PortfolioMode::Percentage => percentage_mode_setup(conn)?,
    }

    // Save config with chosen mode and currency
    save_config(&new_config)?;

    println!("  \x1b[32m✓\x1b[0m Setup complete!");
    println!();

    Ok(())
}

fn full_mode_setup(conn: &Connection, config: &Config) -> Result<()> {
    let total_str = prompt(&format!(
        "  Total portfolio value ({}): ",
        config.base_currency
    ))?;
    let total: Decimal = total_str
        .replace([',', '$'], "")
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid number: {}", total_str))?;

    if total <= dec!(0) {
        bail!("Portfolio value must be positive");
    }

    println!();
    println!("  \x1b[90mAdd positions (type 'done' when finished)\x1b[0m");
    println!();

    let mut entries: Vec<SetupEntry> = Vec::new();
    let mut seen_symbols: HashSet<String> = HashSet::new();
    let mut position_num = 1;

    loop {
        let input = prompt(&format!("  Position {}: ", position_num))?;
        if input.to_lowercase() == "done" || input.is_empty() {
            if entries.is_empty() {
                println!("  \x1b[33mNo positions added. Add at least one.\x1b[0m");
                continue;
            }
            break;
        }

        let (symbol, name, category) = match resolve_symbol(&input)? {
            Some(r) => r,
            None => continue,
        };

        if seen_symbols.contains(&symbol) {
            println!("    \x1b[33mAlready added {}. Skipping.\x1b[0m", symbol);
            continue;
        }

        println!(
            "    → {} ({}) \x1b[90m[{}]\x1b[0m",
            name_or_symbol(&name, &symbol),
            symbol,
            category
        );

        let val_input = prompt("    Value or percentage: ")?;
        let (value, pct) = parse_value_or_pct(&val_input, total)?;

        let csym = crate::config::currency_symbol(&config.base_currency);
        println!(
            "    → {}{:.2} ({:.1}% of portfolio)",
            csym, value, pct
        );
        println!();

        seen_symbols.insert(symbol.clone());
        entries.push(SetupEntry {
            symbol,
            name,
            category,
            value: Some(value),
            pct,
        });
        position_num += 1;
    }

    // Summary
    let csym = crate::config::currency_symbol(&config.base_currency);
    println!();
    println!("  \x1b[1mPortfolio Summary:\x1b[0m");
    let mut allocated = dec!(0);
    for entry in &entries {
        let val = entry.value.unwrap_or(dec!(0));
        println!(
            "    {:<20} {:>1}{:>12.2}  {:>5.1}%",
            name_or_symbol(&entry.name, &entry.symbol),
            csym,
            val,
            entry.pct
        );
        allocated += val;
    }
    let remaining = total - allocated;
    let remaining_pct = if total > dec!(0) {
        (remaining / total) * dec!(100)
    } else {
        dec!(0)
    };
    println!("    {}", "─".repeat(42));
    println!(
        "    {:<20} {:>1}{:>12.2}  {:>5.1}%",
        "Allocated", csym, allocated, dec!(100) - remaining_pct
    );
    if remaining > dec!(0) {
        println!(
            "    {:<20} {:>1}{:>12.2}  {:>5.1}%",
            "Remaining", csym, remaining, remaining_pct
        );
    }
    println!();

    if !confirm("  Save and continue?")? {
        println!("  Cancelled.");
        return Ok(());
    }

    // Fetch prices and create transactions
    println!();
    println!("  Fetching current prices...");
    let rt = tokio::runtime::Runtime::new()?;

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    for entry in &entries {
        let value = entry.value.unwrap_or(dec!(0));

        let (price, qty) = if entry.category == AssetCategory::Cash {
            (dec!(1), value)
        } else {
            let fetched = rt.block_on(fetch_price_for_symbol(
                &entry.symbol,
                entry.category,
            ));
            match fetched {
                Ok(p) if p > dec!(0) => {
                    let q = value / p;
                    println!(
                        "    {}: {}{:.2} → qty {:.4}",
                        entry.symbol, csym, p, q
                    );
                    (p, q)
                }
                _ => {
                    println!(
                        "    \x1b[33m{}: Could not fetch price\x1b[0m",
                        entry.symbol
                    );
                    let manual = prompt("    Enter current price: ")?;
                    let p: Decimal = manual
                        .replace([',', '$'], "")
                        .parse()
                        .unwrap_or(dec!(1));
                    let q = if p > dec!(0) { value / p } else { value };
                    (p, q)
                }
            }
        };

        let tx = NewTransaction {
            symbol: entry.symbol.clone(),
            category: entry.category,
            tx_type: TxType::Buy,
            quantity: qty,
            price_per: price,
            currency: config.base_currency.clone(),
            date: today.clone(),
            notes: Some("setup".to_string()),
        };
        transactions::insert_transaction(conn, &tx)?;
    }

    println!();
    Ok(())
}

fn percentage_mode_setup(conn: &Connection) -> Result<()> {
    println!("  \x1b[90mAdd positions (type 'done' when finished)\x1b[0m");
    println!();

    let mut entries: Vec<SetupEntry> = Vec::new();
    let mut seen_symbols: HashSet<String> = HashSet::new();
    let mut position_num = 1;

    loop {
        let input = prompt(&format!("  Position {}: ", position_num))?;
        if input.to_lowercase() == "done" || input.is_empty() {
            if entries.is_empty() {
                println!("  \x1b[33mNo positions added. Add at least one.\x1b[0m");
                continue;
            }
            break;
        }

        let (symbol, name, category) = match resolve_symbol(&input)? {
            Some(r) => r,
            None => continue,
        };

        if seen_symbols.contains(&symbol) {
            println!("    \x1b[33mAlready added {}. Skipping.\x1b[0m", symbol);
            continue;
        }

        println!(
            "    → {} ({}) \x1b[90m[{}]\x1b[0m",
            name_or_symbol(&name, &symbol),
            symbol,
            category
        );

        let pct_input = prompt("    Allocation %: ")?;
        let pct: Decimal = pct_input
            .replace('%', "")
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid percentage: {}", pct_input))?;

        if pct <= dec!(0) {
            println!("    \x1b[33mPercentage must be positive. Skipping.\x1b[0m");
            continue;
        }

        println!();

        seen_symbols.insert(symbol.clone());
        entries.push(SetupEntry {
            symbol,
            name,
            category,
            value: None,
            pct,
        });
        position_num += 1;
    }

    // Summary
    let total_pct: Decimal = entries.iter().map(|e| e.pct).sum();
    println!();
    println!("  \x1b[1mAllocation Summary:\x1b[0m");
    for entry in &entries {
        println!(
            "    {:<24} {:>5.1}%",
            name_or_symbol(&entry.name, &entry.symbol),
            entry.pct
        );
    }
    println!("    {}", "─".repeat(32));
    println!("    {:<24} {:>5.1}%", "Total", total_pct);
    if total_pct < dec!(100) {
        let rem = dec!(100) - total_pct;
        println!(
            "    {:<24} {:>5.1}%",
            "Remaining", rem
        );
    }
    if total_pct > dec!(100) {
        println!("    \x1b[33m! Allocations exceed 100%\x1b[0m");
    }
    println!();

    if !confirm("  Save and continue?")? {
        println!("  Cancelled.");
        return Ok(());
    }

    // Insert allocations
    for entry in &entries {
        allocations::insert_allocation(conn, &entry.symbol, entry.category, entry.pct)?;
    }

    println!();
    Ok(())
}

/// Resolve user input to (symbol, name, category) via autocomplete.
fn resolve_symbol(input: &str) -> Result<Option<(String, String, AssetCategory)>> {
    let query = input.trim();
    if query.is_empty() {
        return Ok(None);
    }

    let matches = asset_names::search_names(query);

    if matches.is_empty() {
        // No match — accept as custom symbol
        let symbol = query.to_uppercase();
        let category = asset_names::infer_category(&symbol);
        println!(
            "    \x1b[90mUnknown symbol. Category inferred: {}\x1b[0m",
            category
        );
        let cat_input = prompt("    Category (or Enter to accept): ")?;
        let final_cat = if cat_input.is_empty() {
            category
        } else {
            cat_input.parse().unwrap_or(category)
        };
        return Ok(Some((symbol, String::new(), final_cat)));
    }

    // Check for exact ticker match
    let upper = query.to_uppercase();
    if let Some(&(ticker, name)) = matches
        .iter()
        .find(|(t, _)| t.to_uppercase() == upper)
    {
        let cat = asset_names::infer_category(ticker);
        return Ok(Some((ticker.to_string(), name.to_string(), cat)));
    }

    // Single match
    if matches.len() == 1 {
        let (ticker, name) = matches[0];
        let cat = asset_names::infer_category(ticker);
        return Ok(Some((ticker.to_string(), name.to_string(), cat)));
    }

    // Multiple matches — show numbered list (max 10)
    let display: Vec<_> = matches.iter().take(10).collect();
    println!("    \x1b[90mMultiple matches:\x1b[0m");
    for (i, (ticker, name)) in display.iter().enumerate() {
        let cat = asset_names::infer_category(ticker);
        println!(
            "      \x1b[1m[{}]\x1b[0m {} ({}) \x1b[90m[{}]\x1b[0m",
            i + 1,
            name,
            ticker,
            cat
        );
    }
    if matches.len() > 10 {
        println!("      \x1b[90m... and {} more\x1b[0m", matches.len() - 10);
    }

    let choice = prompt("    Select number: ")?;
    let idx: usize = choice.parse().unwrap_or(0);
    if idx >= 1 && idx <= display.len() {
        let (ticker, name) = display[idx - 1];
        let cat = asset_names::infer_category(ticker);
        Ok(Some((ticker.to_string(), name.to_string(), cat)))
    } else {
        println!("    \x1b[33mInvalid choice. Try again.\x1b[0m");
        Ok(None)
    }
}

fn parse_value_or_pct(input: &str, total: Decimal) -> Result<(Decimal, Decimal)> {
    let trimmed = input.replace([',', '$'], "");
    let trimmed = trimmed.trim();

    if trimmed.ends_with('%') {
        // Percentage input
        let pct_str = trimmed.trim_end_matches('%').trim();
        let pct: Decimal = pct_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid percentage: {}", input))?;
        let value = (pct / dec!(100)) * total;
        Ok((value, pct))
    } else {
        // Value input
        let value: Decimal = trimmed
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid value: {}", input))?;
        let pct = if total > dec!(0) {
            (value / total) * dec!(100)
        } else {
            dec!(0)
        };
        Ok((value, pct))
    }
}

async fn fetch_price_for_symbol(
    symbol: &str,
    category: AssetCategory,
) -> Result<Decimal> {
    match category {
        AssetCategory::Crypto => {
            let quotes = coingecko::fetch_prices(&[symbol.to_string()]).await?;
            quotes
                .first()
                .map(|q| q.price)
                .ok_or_else(|| anyhow::anyhow!("No price returned"))
        }
        AssetCategory::Cash => Ok(dec!(1)),
        _ => {
            let quote = yahoo::fetch_price(symbol).await?;
            Ok(quote.price)
        }
    }
}

fn name_or_symbol(name: &str, symbol: &str) -> String {
    if name.is_empty() {
        symbol.to_string()
    } else {
        format!("{} {}", name, symbol)
    }
}

/// Check if the database has any portfolio data.
pub fn has_portfolio_data(conn: &Connection) -> bool {
    let tx_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap_or(0);
    let alloc_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM portfolio_allocations", [], |r| r.get(0))
        .unwrap_or(0);
    tx_count > 0 || alloc_count > 0
}
