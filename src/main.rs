mod alerts;
mod analytics;
mod app;
mod cli;
mod commands;
mod config;
mod data;
mod db;
mod indicators;
mod models;
mod notify;
mod price;
mod regime;
mod tui;
mod web;

use anyhow::{bail, Result};
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::config::load_config_with_first_run_prompt;
use crate::db::{default_db_path, open_db};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config_with_first_run_prompt()?;
    let db_path = default_db_path();
    let conn = open_db(&db_path)?;

    match cli.command {
        None => {
            // Auto-detect: run setup if no portfolio data
            if !commands::setup::has_portfolio_data(&conn) {
                commands::setup::run(&conn, &config, false)?;
                // Reload config (setup may have changed portfolio_mode)
                let config = load_config_with_first_run_prompt()?;
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

        Some(Command::Summary { group_by, period, what_if, json }) => commands::summary::run(&conn, &config, group_by.as_ref(), period.as_ref(), what_if.as_deref(), true, json),
        Some(Command::Export { format, output }) => commands::export::run(&conn, &format, &config, output.as_deref()),

        Some(Command::ListTx { notes, json }) => {
            if config.is_percentage_mode() {
                bail!("list-tx is not available in percentage mode (no transactions).\nRun `pftui setup` to switch to full mode.");
            }
            commands::list_tx::run(&conn, notes, json)
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
            let runtime = tokio::runtime::Runtime::new()?;
            for upper in &symbols {
                let cat = match &category {
                    Some(c) => c.parse().unwrap_or_else(|_| infer_category(upper)),
                    None => infer_category(upper),
                };

                // Validate symbol by attempting price fetch before persisting it.
                let yahoo_sym = match cat {
                    crate::models::asset::AssetCategory::Crypto => {
                        if upper.ends_with("-USD") {
                            upper.clone()
                        } else {
                            format!("{}-USD", upper)
                        }
                    }
                    _ => upper.clone(),
                };
                
                match runtime.block_on(price::yahoo::fetch_price(&yahoo_sym)) {
                    Ok(_) => {
                        db::watchlist::add_to_watchlist(&conn, upper, cat)?;
                        let name = crate::models::asset_names::resolve_name(upper);
                        let display = if name.is_empty() { upper.clone() } else { name };
                        println!("Added {} ({}) to watchlist as {}", upper, display, cat);
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: skipping {} — price lookup failed ({}). Symbol may be invalid.",
                            upper, e
                        );
                        continue;
                    }
                }

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

        Some(Command::Refresh { notify }) => commands::refresh::run(&conn, &config, notify),
        Some(Command::Status) => commands::status::run(&conn),
        Some(Command::Config { action, field, value }) => {
            commands::config_cmd::run(&action, field.as_deref(), value.as_deref())
        }
        Some(Command::Value { json }) => commands::value::run(&conn, &config, json),
        Some(Command::Brief { json }) => commands::brief::run(&conn, &config, true, json),
        Some(Command::Watchlist { approaching, json }) => commands::watchlist_cli::run(&conn, &config, approaching.as_deref(), json),

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
        Some(Command::Economy { indicator, json }) => {
            commands::economy::run(&conn, indicator.as_deref(), json)
        }

        Some(Command::Eod { json }) => commands::eod::run(&conn, &config, json),

        Some(Command::Global { country, indicator, json }) => {
            commands::global::run(&conn, country.as_deref(), indicator.as_deref(), json)
        }

        Some(Command::Performance { since, period, vs, json }) => {
            commands::performance::run(&conn, &config, since.as_deref(), period.as_deref(), vs.as_deref(), json)
        }

        Some(Command::EtfFlows { days, fund, json }) => {
            commands::etf_flows::run(days, fund, json)
        }

        Some(Command::Movers { threshold, json }) => {
            commands::movers::run(&conn, &config, Some(&threshold), json)
        }

        Some(Command::Predictions { category, search, limit, json }) => {
            commands::predictions::run(&conn, category.as_deref(), search.as_deref(), limit, json)
        }

        Some(Command::News { source, search, hours, limit, json }) => {
            commands::news::run(&conn, source.as_deref(), search.as_deref(), hours, limit, json)
        }

        Some(Command::Sentiment { symbol, history, json }) => {
            commands::sentiment::run(symbol.as_deref(), history, json)
        }

        Some(Command::Supply { symbol, json }) => {
            commands::supply::run(symbol, json)
        }

        Some(Command::Calendar { days, impact, json }) => {
            commands::calendar::run(days, impact.as_deref(), json)
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
        Some(Command::Drift { json }) => commands::drift::run(&db_path, json),
        Some(Command::Rebalance { json }) => commands::rebalance::run(&db_path, json),
        Some(Command::Journal {
            action,
            value,
            id,
            date,
            tag,
            symbol,
            conviction,
            status,
            filter_status,
            content,
            since,
            limit,
            json,
        }) => match action.as_str() {
            "add" => {
                let content_text = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing content text. Usage: pftui journal add \"your entry text\"")
                })?;
                commands::journal::run_add(
                    content_text,
                    date.as_deref(),
                    tag.as_deref(),
                    symbol.as_deref(),
                    conviction.as_deref(),
                    json,
                )
            }
            "list" => commands::journal::run_list(
                limit,
                since.as_deref(),
                tag.as_deref(),
                symbol.as_deref(),
                filter_status.as_deref(),
                json,
            ),
            "search" => {
                let query = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing search query. Usage: pftui journal search \"query\"")
                })?;
                commands::journal::run_search(query, since.as_deref(), limit, json)
            }
            "update" => {
                let entry_id = id.ok_or_else(|| {
                    anyhow::anyhow!("Missing entry ID. Usage: pftui journal update --id N [--content \"...\"] [--status ...]")
                })?;
                commands::journal::run_update(
                    entry_id,
                    content.as_deref(),
                    status.as_deref(),
                    json,
                )
            }
            "remove" => {
                let entry_id = id.ok_or_else(|| {
                    anyhow::anyhow!("Missing entry ID. Usage: pftui journal remove --id N")
                })?;
                commands::journal::run_remove(entry_id, json)
            }
            "tags" => commands::journal::run_tags(json),
            "stats" => commands::journal::run_stats(json),
            _ => Err(anyhow::anyhow!(
                "Unknown journal action '{}'. Valid actions: add, list, search, update, remove, tags, stats",
                action
            )),
        },
        Some(Command::MigrateJournal {
            path,
            dry_run,
            default_tag,
            default_status,
            json,
        }) => commands::migrate_journal::run(
            &conn,
            &path,
            dry_run,
            default_tag.as_deref(),
            &default_status,
            json,
        ),

        Some(Command::Web { port, bind, no_auth }) => {
            // Web server runs in async context
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                web::run_server(db_path.to_string_lossy().to_string(), config, &bind, port, !no_auth).await
            })
        }

        Some(Command::Sector { json }) => commands::sector::run(&conn, &config, json),
        Some(Command::Research { query, news, freshness, count, json }) => {
            let freshness_checked = match freshness.as_deref() {
                Some(v) => Some(commands::research::validate_freshness(v)?),
                None => None,
            };
            commands::research::run(
                &conn,
                &query,
                news,
                freshness_checked.as_deref(),
                count,
                json,
            )
        }
    }
}
