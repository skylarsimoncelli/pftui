use std::collections::HashSet;
use std::io::{self, Write};

use anyhow::{bail, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::config::{save_config, Config, DatabaseBackend, PortfolioMode, SUPPORTED_CURRENCIES};
use crate::db::backend::{open_from_config, BackendConnection};
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

pub fn run(config: &Config, is_explicit: bool) -> Result<()> {
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
    let (database_backend, database_url) = prompt_database_backend(config)?;
    new_config.database_backend = database_backend;
    new_config.database_url = database_url;

    let db_path = crate::db::default_db_path();
    let selected_backend = open_from_config(&new_config, &db_path)?;

    // If explicit setup with existing data, warn on the selected backend.
    if is_explicit {
        let tx_count = transactions::count_transactions_backend(&selected_backend)?;
        let alloc_count = allocations::count_allocations_backend(&selected_backend)?;
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
            reset_setup_tables(&selected_backend)?;
            println!("  Data cleared.\n");
        }
    }

    match mode {
        PortfolioMode::Full => full_mode_setup(&selected_backend, &new_config)?,
        PortfolioMode::Percentage => percentage_mode_setup(&selected_backend)?,
    }

    // Optional Brave API key for richer research/news/economic data.
    println!();
    println!("  Optional: Brave Search API key");
    println!("  For richer news, economic data, and market intelligence, add a Brave Search API key (free tier: $5/month credits).");
    println!("  Get one at https://brave.com/search/api/");
    let brave_key = prompt("  Enter key (or press Enter to skip): ")?;
    new_config.brave_api_key = if brave_key.trim().is_empty() {
        None
    } else {
        Some(brave_key)
    };

    // Save config with chosen mode and currency
    save_config(&new_config)?;

    println!("  \x1b[32m✓\x1b[0m Setup complete!");
    println!();

    Ok(())
}

fn reset_setup_tables(backend: &BackendConnection) -> Result<()> {
    crate::db::query::dispatch(
        backend,
        |conn| {
            conn.execute_batch(
                "DELETE FROM transactions;
                 DELETE FROM portfolio_allocations;
                 DELETE FROM price_cache;
                 DELETE FROM price_history;",
            )?;
            Ok(())
        },
        |pool| {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                sqlx::query("DELETE FROM transactions").execute(pool).await?;
                sqlx::query("DELETE FROM portfolio_allocations")
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM price_cache").execute(pool).await?;
                sqlx::query("DELETE FROM price_history").execute(pool).await?;
                Ok::<(), sqlx::Error>(())
            })?;
            Ok(())
        },
    )
}

fn prompt_database_backend(config: &Config) -> Result<(DatabaseBackend, Option<String>)> {
    println!("  Database backend:");
    println!("    \x1b[1m[1]\x1b[0m SQLite \x1b[90m— local file (default)\x1b[0m");
    println!("    \x1b[1m[2]\x1b[0m PostgreSQL \x1b[90m— external database URL\x1b[0m");
    println!();

    let default_choice = match config.database_backend {
        DatabaseBackend::Sqlite => "1",
        DatabaseBackend::Postgres => "2",
    };

    let backend = loop {
        let choice = prompt(&format!("  Select [1/2] (default: {}): ", default_choice))?;
        let normalized = if choice.trim().is_empty() {
            default_choice.to_string()
        } else {
            choice
        };
        match normalized.trim() {
            "1" => break DatabaseBackend::Sqlite,
            "2" => break DatabaseBackend::Postgres,
            _ => println!("  Please enter 1 or 2."),
        }
    };

    let url = if backend == DatabaseBackend::Postgres {
        loop {
            let raw = prompt("  PostgreSQL URL (postgres://...): ")?;
            let trimmed = raw.trim();
            if trimmed.starts_with("postgres://") || trimmed.starts_with("postgresql://") {
                break Some(trimmed.to_string());
            }
            println!("  Enter a valid PostgreSQL URL starting with postgres:// or postgresql://");
        }
    } else {
        None
    };

    println!();
    Ok((backend, url))
}

fn full_mode_setup(backend: &BackendConnection, config: &Config) -> Result<()> {
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
        let result = interactive_symbol_search(&format!("  Position {}: ", position_num))?;
        let (symbol, name, category) = match result {
            Some(r) => r,
            None => {
                if entries.is_empty() {
                    println!("  \x1b[33mNo positions added. Add at least one.\x1b[0m");
                    continue;
                }
                break;
            }
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
        transactions::insert_transaction_backend(backend, &tx)?;
    }

    println!();
    Ok(())
}

fn percentage_mode_setup(backend: &BackendConnection) -> Result<()> {
    println!("  \x1b[90mAdd positions (type 'done' when finished)\x1b[0m");
    println!();

    let mut entries: Vec<SetupEntry> = Vec::new();
    let mut seen_symbols: HashSet<String> = HashSet::new();
    let mut position_num = 1;

    loop {
        let result = interactive_symbol_search(&format!("  Position {}: ", position_num))?;
        let (symbol, name, category) = match result {
            Some(r) => r,
            None => {
                if entries.is_empty() {
                    println!("  \x1b[33mNo positions added. Add at least one.\x1b[0m");
                    continue;
                }
                break;
            }
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
        allocations::insert_allocation_backend(backend, &entry.symbol, entry.category, entry.pct)?;
    }

    println!();
    Ok(())
}

/// Maximum number of inline suggestions to display.
const MAX_SUGGESTIONS: usize = 8;

/// Interactive symbol search with inline suggestions.
///
/// Uses crossterm raw mode to capture keystrokes character-by-character.
/// Shows ranked fuzzy matches from `search_names` below the input prompt,
/// updating live as the user types.
///
/// Controls:
///   - Type characters to search
///   - Backspace to delete
///   - Up/Down arrows to highlight a suggestion
///   - Enter to accept highlighted suggestion (or raw input if none highlighted)
///   - 1-9 to quick-select a numbered suggestion
///   - Esc to cancel
///   - Ctrl-C to cancel
fn interactive_symbol_search(label: &str) -> Result<Option<(String, String, AssetCategory)>> {
    let mut input = String::new();
    let mut highlight: usize = 0; // 0 = none, 1-based index into suggestions
    let mut last_match_count: usize = 0;

    // Print the initial prompt
    print!("{}", label);
    io::stdout().flush()?;

    terminal::enable_raw_mode()?;

    let result = (|| -> Result<Option<(String, String, AssetCategory)>> {
        loop {
            if !event::poll(std::time::Duration::from_millis(100))? {
                continue;
            }

            let ev = event::read()?;
            let Event::Key(key) = ev else {
                continue;
            };

            // Only handle Press events (ignore Release/Repeat on some platforms)
            if key.kind != event::KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    clear_suggestions(last_match_count);
                    print!("\r\n");
                    return Ok(None);
                }
                KeyCode::Esc => {
                    clear_suggestions(last_match_count);
                    print!("\r\n");
                    return Ok(None);
                }
                KeyCode::Enter => {
                    let matches = asset_names::search_names(&input);
                    let display: Vec<_> = matches.iter().take(MAX_SUGGESTIONS).collect();

                    clear_suggestions(last_match_count);
                    print!("\r\n");

                    if highlight >= 1 && highlight <= display.len() {
                        let (ticker, name) = display[highlight - 1];
                        let cat = asset_names::infer_category(ticker);
                        return Ok(Some((ticker.to_string(), name.to_string(), cat)));
                    }

                    // No highlight — treat raw input like the old resolve_symbol
                    let trimmed = input.trim().to_string();
                    if trimmed.is_empty() || trimmed.to_lowercase() == "done" {
                        return Ok(None);
                    }

                    // Check for exact match first
                    let upper = trimmed.to_uppercase();
                    if let Some(&(ticker, name)) =
                        matches.iter().find(|(t, _)| t.to_uppercase() == upper)
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

                    if !matches.is_empty() {
                        // Multiple matches but none highlighted — take first
                        let (ticker, name) = matches[0];
                        let cat = asset_names::infer_category(ticker);
                        return Ok(Some((ticker.to_string(), name.to_string(), cat)));
                    }

                    // No matches at all — accept as custom symbol
                    let symbol = upper;
                    let category = asset_names::infer_category(&symbol);
                    return Ok(Some((symbol, String::new(), category)));
                }
                KeyCode::Backspace => {
                    input.pop();
                    highlight = 0;
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Quick-select: digit 1-9 selects that suggestion if matches exist
                    if c.is_ascii_digit() && c != '0' && !input.is_empty() {
                        let idx = (c as u8 - b'0') as usize;
                        let matches = asset_names::search_names(&input);
                        let display_len = matches.len().min(MAX_SUGGESTIONS);
                        if idx <= display_len {
                            clear_suggestions(last_match_count);
                            print!("\r\n");
                            let (ticker, name) = matches[idx - 1];
                            let cat = asset_names::infer_category(ticker);
                            return Ok(Some((ticker.to_string(), name.to_string(), cat)));
                        }
                    }
                    input.push(c);
                    highlight = 0;
                }
                KeyCode::Up => {
                    highlight = highlight.saturating_sub(1);
                }
                KeyCode::Down => {
                    let matches = asset_names::search_names(&input);
                    let max = matches.len().min(MAX_SUGGESTIONS);
                    if highlight < max {
                        highlight += 1;
                    }
                }
                _ => {}
            }

            // Redraw prompt line and suggestions
            let matches = asset_names::search_names(&input);
            let display: Vec<_> = matches.iter().take(MAX_SUGGESTIONS).collect();

            // Clear old suggestions first
            clear_suggestions(last_match_count);

            // Rewrite the prompt line (carriage return, clear line, print prompt + input)
            print!("\r\x1b[2K{}{}", label, input);

            // Show suggestion count hint on the prompt line
            if !input.is_empty() && !matches.is_empty() {
                let shown = display.len();
                let total = matches.len();
                if total > shown {
                    print!("  \x1b[90m({} of {} matches)\x1b[0m", shown, total);
                } else {
                    print!("  \x1b[90m({} match{})\x1b[0m", total, if total == 1 { "" } else { "es" });
                }
            }

            // Print suggestions below
            if !input.is_empty() && !display.is_empty() {
                for (i, (ticker, name)) in display.iter().enumerate() {
                    let cat = asset_names::infer_category(ticker);
                    let idx = i + 1;
                    if highlight == idx {
                        // Highlighted row
                        print!(
                            "\r\n    \x1b[7m [{idx}] {name} ({ticker}) [{cat}] \x1b[0m",
                        );
                    } else {
                        print!(
                            "\r\n    \x1b[1m[{idx}]\x1b[0m {name} ({ticker}) \x1b[90m[{cat}]\x1b[0m",
                        );
                    }
                }
                // Move cursor back up to the prompt line
                let count = display.len();
                if count > 0 {
                    print!("\x1b[{}A", count);
                }
                // Position cursor at end of input on the prompt line
                let col = label.len() + input.len() + 1;
                print!("\r\x1b[{}C", col - 1);
            }

            last_match_count = display.len();
            io::stdout().flush()?;
        }
    })();

    terminal::disable_raw_mode()?;
    result
}

/// Clear N suggestion lines below the cursor.
fn clear_suggestions(count: usize) {
    if count > 0 {
        // Save cursor, move down and clear each line, restore cursor
        for i in 1..=count {
            print!("\x1b[s\x1b[{}B\r\x1b[2K\x1b[u", i);
        }
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
pub fn has_portfolio_data(backend: &BackendConnection) -> bool {
    let tx_count = transactions::count_transactions_backend(backend).unwrap_or(0);
    let alloc_count = allocations::count_allocations_backend(backend).unwrap_or(0);
    tx_count > 0 || alloc_count > 0
}
