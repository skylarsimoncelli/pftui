mod alerts;
mod app;
mod cli;
mod commands;
mod config;
mod data;
mod db;
mod indicators;
mod models;
mod price;
mod regime;
mod tui;

use anyhow::{bail, Result};
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::config::load_config;
use crate::db::{default_db_path, open_db};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config()?;
    let db_path = default_db_path();
    let conn = open_db(&db_path)?;

    match cli.command {
        None => {
            // Auto-detect: run setup if no portfolio data
            if !commands::setup::has_portfolio_data(&conn) {
                commands::setup::run(&conn, &config, false)?;
                // Reload config (setup may have changed portfolio_mode)
                let config = load_config()?;
                drop(conn);
                let mut app = app::App::new(&config, db_path);
                app.init();
                let result = tui::run(&mut app);
                app.shutdown();
                return result;
            }

            // Launch TUI
            drop(conn);
            let mut app = app::App::new(&config, db_path);
            app.init();
            let result = tui::run(&mut app);
            app.shutdown();
            result
        }

        Some(Command::Setup) => {
            commands::setup::run(&conn, &config, true)
        }

        Some(Command::Summary { group_by, period, what_if, technicals }) => commands::summary::run(&conn, &config, group_by.as_ref(), period.as_ref(), what_if.as_deref(), technicals),
        Some(Command::Export { format, output }) => commands::export::run(&conn, &format, &config, output.as_deref()),

        Some(Command::ListTx { notes }) => {
            if config.is_percentage_mode() {
                bail!("list-tx is not available in percentage mode (no transactions).\nRun `pftui setup` to switch to full mode.");
            }
            commands::list_tx::run(&conn, notes)
        }

        Some(Command::AddTx {
            symbol,
            category,
            tx_type,
            quantity,
            price,
            currency,
            date,
            notes,
        }) => {
            if config.is_percentage_mode() {
                bail!("add-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
            }
            commands::add_tx::run(
                &conn, symbol, category, tx_type, quantity, price, currency, date, notes,
            )
        }

        Some(Command::RemoveTx { id }) => {
            if config.is_percentage_mode() {
                bail!("remove-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
            }
            commands::remove_tx::run(&conn, id)
        }

        Some(Command::Watch { symbol, category, bulk, target, direction }) => {
            use crate::models::asset_names::infer_category;

            // Collect symbols: either --bulk or single positional
            let symbols: Vec<String> = if let Some(bulk_str) = bulk {
                bulk_str
                    .split(',')
                    .map(|s| s.trim().to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else if let Some(sym) = symbol {
                vec![sym.to_uppercase()]
            } else {
                bail!("Provide a symbol or use --bulk SYMBOL1,SYMBOL2,...");
            };

            if symbols.is_empty() {
                bail!("No valid symbols provided");
            }

            // Validate target if provided
            if let Some(ref t) = target {
                let cleaned = t.replace(['$', ','], "");
                if rust_decimal::Decimal::from_str_exact(&cleaned).is_err() {
                    bail!("Invalid target price: '{}'. Use a number (e.g. 300, 55000.50)", t);
                }
                if direction != "above" && direction != "below" {
                    bail!("Invalid direction: '{}'. Use 'above' or 'below'", direction);
                }
                if symbols.len() > 1 {
                    bail!("--target can only be set for a single symbol, not with --bulk");
                }
            }

            let mut added = 0;
            for upper in &symbols {
                let cat = match &category {
                    Some(c) => c.parse().unwrap_or_else(|_| infer_category(upper)),
                    None => infer_category(upper),
                };
                db::watchlist::add_to_watchlist(&conn, upper, cat)?;
                let name = crate::models::asset_names::resolve_name(upper);
                let display = if name.is_empty() { upper.clone() } else { name };
                println!("Added {} ({}) to watchlist as {}", upper, display, cat);

                // Set target if provided
                if let Some(ref t) = target {
                    let cleaned = t.replace(['$', ','], "");
                    db::watchlist::set_watchlist_target(&conn, upper, Some(&cleaned), Some(&direction))?;
                    println!("  Target: {} {} {}", upper, direction, cleaned);

                    // Auto-create an alert rule for this target
                    let rule_text = format!("{} {} {}", upper, direction, cleaned);
                    db::alerts::add_alert(&conn, "price", upper, &direction, &cleaned, &rule_text)?;
                    println!("  Alert created: {}", rule_text);
                }

                added += 1;
            }

            if added > 1 {
                println!("\n{} symbols added to watchlist.", added);
            }
            Ok(())
        }

        Some(Command::Unwatch { symbol }) => {
            let upper = symbol.to_uppercase();
            if db::watchlist::remove_from_watchlist(&conn, &upper)? {
                println!("Removed {} from watchlist", upper);
            } else {
                println!("{} was not in the watchlist", upper);
            }
            Ok(())
        }

        Some(Command::Refresh) => commands::refresh::run(&conn, &config),
        Some(Command::Value) => commands::value::run(&conn, &config),
        Some(Command::Brief { technicals }) => commands::brief::run(&conn, &config, technicals),
        Some(Command::Watchlist { approaching }) => commands::watchlist_cli::run(&conn, &config, approaching.as_deref()),

        Some(Command::SetCash { symbol, amount }) => {
            if config.is_percentage_mode() {
                bail!("set-cash is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
            }
            commands::set_cash::run(&conn, &symbol, &amount)
        }

        Some(Command::Demo) => {
            commands::demo::run(&config)
        }

        Some(Command::Snapshot { width, height, plain }) => {
            commands::snapshot::run(&config, Some(width), Some(height), plain)
        }

        Some(Command::Import { path, mode }) => {
            let import_mode = match mode {
                cli::ImportModeArg::Replace => commands::import::ImportMode::Replace,
                cli::ImportModeArg::Merge => commands::import::ImportMode::Merge,
            };
            commands::import::run(&conn, &config, &path, import_mode)
        }

        Some(Command::History { date, group_by }) => {
            commands::history::run(&conn, &config, &date, group_by.as_ref())
        }

        Some(Command::Macro { json }) => commands::macro_cmd::run(&conn, &config, json),

        Some(Command::Performance { since, period, vs, json }) => {
            commands::performance::run(&conn, &config, since.as_deref(), period.as_deref(), vs.as_deref(), json)
        }

        Some(Command::Movers { threshold, json }) => {
            commands::movers::run(&conn, &config, Some(&threshold), json)
        }

        Some(Command::Alerts { action, value, json, status }) => {
            // Parse value as either a rule string (for add) or an ID (for remove/ack/rearm)
            let id = value.as_deref().and_then(|v| v.parse::<i64>().ok());
            let rule = if id.is_none() { value.clone() } else { None };
            let args = commands::alerts::AlertsArgs {
                rule,
                id,
                json,
                status_filter: status,
            };
            commands::alerts::run(&conn, &action, &args)
        }

        Some(Command::Target { action, symbol, target, band, json }) => {
            match action.as_str() {
                "set" => {
                    let sym = symbol.as_ref().ok_or_else(|| anyhow::anyhow!("--symbol required for 'set'"))?;
                    let tgt = target.as_ref().ok_or_else(|| anyhow::anyhow!("--target required for 'set'"))?;
                    commands::target::run(&db_path, sym, tgt, band.as_deref())
                }
                "list" => commands::target::list(&db_path, json),
                "remove" => {
                    let sym = symbol.as_ref().ok_or_else(|| anyhow::anyhow!("--symbol required for 'remove'"))?;
                    commands::target::remove(&db_path, sym)
                }
                _ => Err(anyhow::anyhow!("Invalid action. Use: set, list, remove"))
            }
        }
    }
}
