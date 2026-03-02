mod app;
mod cli;
mod commands;
mod config;
mod db;
mod models;
mod price;
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

        Some(Command::Summary { group_by, period }) => commands::summary::run(&conn, &config, group_by.as_ref(), period.as_ref()),
        Some(Command::Export { format }) => commands::export::run(&conn, &format, &config),

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

        Some(Command::Watch { symbol, category }) => {
            use crate::models::asset_names::infer_category;
            let cat = match category {
                Some(c) => c.parse().unwrap_or_else(|_| infer_category(&symbol)),
                None => infer_category(&symbol),
            };
            let upper = symbol.to_uppercase();
            db::watchlist::add_to_watchlist(&conn, &upper, cat)?;
            let name = crate::models::asset_names::resolve_name(&upper);
            let display = if name.is_empty() { upper.clone() } else { name };
            println!("Added {} ({}) to watchlist as {}", upper, display, cat);
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
    }
}
