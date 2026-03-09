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
use crate::db::backend::open_from_config;
use crate::db::default_db_path;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config_with_first_run_prompt()?;
    let db_path = default_db_path();

    if let Some(Command::Config {
        action,
        field,
        value,
        json,
    }) = &cli.command
    {
        return commands::config_cmd::run(action, field.as_deref(), value.as_deref(), *json);
    }

    let backend = open_from_config(&config, &db_path)?;

    let result = match cli.command {
        None => {
            let Some(_conn) = backend.sqlite_native() else {
                bail!(
                    "`tui` is not yet available with database_backend=postgres.\nUse CLI/Web commands with postgres backend for now."
                );
            };
            // Auto-detect: run setup if no portfolio data
            if !commands::setup::has_portfolio_data(&backend) {
                commands::setup::run(&backend, &config, false)?;
                // Reload config (setup may have changed portfolio_mode)
                let config = load_config_with_first_run_prompt()?;
                let mut app = app::App::new(&config, db_path);
                app.init();
                let result = tui::run(&mut app);
                app.shutdown();
                result
            } else {
                // Launch TUI
                let mut app = app::App::new(&config, db_path);
                app.init();
                let result = tui::run(&mut app);
                app.shutdown();
                result
            }
        }

        Some(Command::Setup) => {
            commands::setup::run(&backend, &config, true)
        }

        Some(Command::Summary { group_by, period, what_if, json }) => {
            commands::summary::run(
                &backend,
                &config,
                group_by.as_ref(),
                period.as_ref(),
                what_if.as_deref(),
                true,
                json,
            )
        }
        Some(Command::Export { format, output }) => {
            commands::export::run(&backend, &format, &config, output.as_deref())
        }

        Some(Command::ListTx { notes, json }) => {
            if config.is_percentage_mode() {
                bail!("list-tx is not available in percentage mode (no transactions).\nRun `pftui setup` to switch to full mode.");
            }
            commands::list_tx::run(&backend, notes, json)
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
                &backend, symbol, category, tx_type, quantity, price, currency, date, notes,
            )
        }

        Some(Command::RemoveTx { id }) => {
            if config.is_percentage_mode() {
                bail!("remove-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
            }
            commands::remove_tx::run(&backend, id)
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
                        db::watchlist::add_to_watchlist_backend(&backend, upper, cat)?;
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
                    db::watchlist::set_watchlist_target_backend(&backend, upper, Some(&cleaned), Some(&direction))?;
                    println!("  Target: {} {} {}", upper, direction, cleaned);

                    // Auto-create an alert rule for this target
                    let rule_text = format!("{} {} {}", upper, direction, cleaned);
                    db::alerts::add_alert_backend(&backend, "price", upper, &direction, &cleaned, &rule_text)?;
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
            if db::watchlist::remove_from_watchlist_backend(&backend, &upper)? {
                println!("Removed {} from watchlist", upper);
            } else {
                println!("{} was not in the watchlist", upper);
            }
            Ok(())
        }

        Some(Command::Refresh { notify }) => {
            commands::refresh::run(&backend, &config, notify)
        }
        Some(Command::Status { json, .. }) => {
            commands::status::run_backend(&backend, json)
        }
        Some(Command::Config { .. }) => unreachable!(),
        Some(Command::Value { json }) => {
            commands::value::run(&backend, &config, json)
        }
        Some(Command::Brief { json }) => {
            if let Some(conn) = backend.sqlite_native() {
                commands::brief::run(conn, &config, true, json)
            } else {
                commands::summary::run(&backend, &config, None, None, None, true, json)
            }
        }
        Some(Command::Watchlist { approaching, json }) => {
            commands::watchlist_cli::run(&backend, &config, approaching.as_deref(), json)
        }

        Some(Command::SetCash { symbol, amount }) => {
            if config.is_percentage_mode() {
                bail!("set-cash is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
            }
            commands::set_cash::run(&backend, &symbol, &amount)
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
            commands::import::run(&backend, &config, &path, import_mode)
        }

        Some(Command::History { date, group_by }) => {
            commands::history::run(&backend, &config, &date, group_by.as_ref())
        }

        Some(Command::Macro { json }) => commands::macro_cmd::run(&backend, &config, json),
        Some(Command::Oil { json }) => commands::oil::run(&backend, json),
        Some(Command::Crisis { json }) => commands::crisis::run(&backend, json),
        Some(Command::Regime { action, limit, json }) => {
            let Some(conn) = backend.sqlite_native() else {
                bail!("regime is currently available only with database_backend=sqlite");
            };
            commands::regime::run(conn, &action, limit, json)
        }
        Some(Command::Fedwatch { json }) => commands::fedwatch::run(json),
        Some(Command::Sovereign { json }) => commands::sovereign::run(json),
        Some(Command::Economy { indicator, json }) => {
            commands::economy::run(&backend, indicator.as_deref(), json)
        }

        Some(Command::Eod { json }) => {
            commands::eod::run(&backend, &config, json)
        }

        Some(Command::Global { country, indicator, json }) => {
            commands::global::run(&backend, country.as_deref(), indicator.as_deref(), json)
        }

        Some(Command::Performance { since, period, vs, json }) => {
            commands::performance::run(&backend, &config, since.as_deref(), period.as_deref(), vs.as_deref(), json)
        }

        Some(Command::EtfFlows { days, fund, json }) => {
            commands::etf_flows::run(days, fund, json)
        }

        Some(Command::Movers { threshold, json }) => {
            commands::movers::run(&backend, &config, Some(&threshold), json)
        }
        Some(Command::Scan {
            filter,
            save,
            load,
            list,
            json,
        }) => {
            commands::scan::run(
                &backend,
                &config,
                filter.as_deref(),
                save.as_deref(),
                load.as_deref(),
                list,
                json,
            )
        }

        Some(Command::Scenario {
            action,
            value,
            id,
            signal_id,
            probability,
            description,
            impact,
            triggers,
            precedent,
            status,
            driver,
            evidence,
            source,
            scenario,
            limit,
            json,
        }) => {
            commands::scenario::run(
                &backend,
                &action,
                value.as_deref(),
                id,
                signal_id,
                probability,
                description.as_deref(),
                impact.as_deref(),
                triggers.as_deref(),
                precedent.as_deref(),
                status.as_deref(),
                driver.as_deref(),
                evidence.as_deref(),
                source.as_deref(),
                scenario.as_deref(),
                limit,
                json,
            )
        }
        Some(Command::Question {
            action,
            value,
            id,
            tilt,
            evidence,
            signal,
            resolution,
            status,
            json,
        }) => commands::question::run(
            &action,
            value.as_deref(),
            id,
            tilt.as_deref(),
            evidence.as_deref(),
            signal.as_deref(),
            resolution.as_deref(),
            status.as_deref(),
            json,
        ),
        Some(Command::AgentMsg {
            action,
            value,
            id,
            from,
            to,
            priority,
            category,
            layer,
            unacked,
            since,
            days,
            limit,
            json,
        }) => commands::agent_msg::run(
            &action,
            value.as_deref(),
            id,
            from.as_deref(),
            to.as_deref(),
            priority.as_deref(),
            category.as_deref(),
            layer.as_deref(),
            unacked,
            since.as_deref(),
            days,
            limit,
            json,
        ),
        Some(Command::Analytics {
            action,
            symbol,
            signal_type,
            severity,
            limit,
            json,
        }) => commands::analytics::run(
            &backend,
            &action,
            symbol.as_deref(),
            signal_type.as_deref(),
            severity.as_deref(),
            limit,
            json,
        ),
        Some(Command::Thesis {
            action,
            value,
            content,
            conviction,
            limit,
            json,
        }) => match action.as_str() {
            "list" => commands::thesis::run_list(&backend, json),
            "update" => {
                let section = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing section name. Usage: pftui thesis update <section> --content \"...\"")
                })?;
                let content_text = content.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing content. Usage: pftui thesis update <section> --content \"...\"")
                })?;
                commands::thesis::run_update(&backend, section, content_text, conviction.as_deref(), json)
            }
            "history" => {
                let section = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing section name. Usage: pftui thesis history <section>")
                })?;
                commands::thesis::run_history(&backend, section, limit, json)
            }
            "remove" => {
                let section = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing section name. Usage: pftui thesis remove <section>")
                })?;
                commands::thesis::run_remove(&backend, section, json)
            }
            _ => Err(anyhow::anyhow!(
                "Unknown action '{}'. Available: list, update, history, remove",
                action
            )),
        },

        Some(Command::Predictions { category, search, limit, json }) => {
            commands::predictions::run(&backend, category.as_deref(), search.as_deref(), limit, json)
        }
        Some(Command::Predict {
            action,
            value,
            id,
            symbol,
            conviction,
            target_date,
            outcome,
            notes,
            filter,
            limit,
            json,
        }) => commands::predict::run(
            &action,
            value.as_deref(),
            id,
            symbol.as_deref(),
            conviction.as_deref(),
            target_date.as_deref(),
            outcome.as_deref(),
            notes.as_deref(),
            filter.as_deref(),
            limit,
            json,
        ),
        Some(Command::Correlations {
            action,
            value,
            value2,
            window,
            period,
            store,
            limit,
            json,
        }) => commands::correlations::run(
            &backend,
            action.as_deref(),
            value.as_deref(),
            value2.as_deref(),
            window,
            period.as_deref(),
            store,
            limit,
            json,
        ),

        Some(Command::News { source, search, hours, limit, json }) => {
            commands::news::run(&backend, source.as_deref(), search.as_deref(), hours, limit, json)
        }

        Some(Command::Sentiment { symbol, history, json }) => {
            commands::sentiment::run(symbol.as_deref(), history, json)
        }

        Some(Command::Supply { symbol, json }) => {
            commands::supply::run(&backend, symbol, json)
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
            commands::alerts::run(&backend, &action, &args)
        }

        Some(Command::Target { action, symbol, target, band, json }) => {
            match action.as_str() {
                "set" => {
                    let sym = symbol.as_ref().ok_or_else(|| anyhow::anyhow!("--symbol required for 'set'"))?;
                    let tgt = target.as_ref().ok_or_else(|| anyhow::anyhow!("--target required for 'set'"))?;
                    commands::target::run(&backend, sym, tgt, band.as_deref())
                }
                "list" => commands::target::list(&backend, json),
                "remove" => {
                    let sym = symbol.as_ref().ok_or_else(|| anyhow::anyhow!("--symbol required for 'remove'"))?;
                    commands::target::remove(&backend, sym)
                }
                _ => Err(anyhow::anyhow!("Invalid action. Use: set, list, remove"))
            }
        }
        Some(Command::Drift { json }) => commands::drift::run(&backend, json),
        Some(Command::Rebalance { json }) => commands::rebalance::run(&backend, json),
        Some(Command::Conviction {
            action,
            value,
            score,
            notes,
            limit,
            json,
        }) => match action.as_str() {
            "set" => {
                let symbol = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing symbol. Usage: pftui conviction set SYMBOL --score N")
                })?;
                let score_val = score.ok_or_else(|| {
                    anyhow::anyhow!("Missing score. Usage: pftui conviction set SYMBOL --score N")
                })?;
                commands::conviction::run_set(&backend, symbol, score_val, notes.as_deref(), json)
            }
            "list" => commands::conviction::run_list(&backend, json),
            "history" => {
                let symbol = value.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("Missing symbol. Usage: pftui conviction history SYMBOL")
                })?;
                commands::conviction::run_history(&backend, symbol, limit, json)
            }
            "changes" => {
                let days = if let Some(val) = value.as_deref() {
                    val.parse::<usize>().unwrap_or(7)
                } else {
                    7
                };
                commands::conviction::run_changes(&backend, days, json)
            }
            _ => Err(anyhow::anyhow!(
                "Invalid action. Use: set, list, history, changes"
            )),
        },
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
                    &backend,
                    content_text,
                    date.as_deref(),
                    tag.as_deref(),
                    symbol.as_deref(),
                    conviction.as_deref(),
                    json,
                )
            }
            "list" => commands::journal::run_list(
                &backend,
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
                commands::journal::run_search(&backend, query, since.as_deref(), limit, json)
            }
            "update" => {
                let entry_id = id.ok_or_else(|| {
                    anyhow::anyhow!("Missing entry ID. Usage: pftui journal update --id N [--content \"...\"] [--status ...]")
                })?;
                commands::journal::run_update(
                    &backend,
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
                commands::journal::run_remove(&backend, entry_id, json)
            }
            "tags" => commands::journal::run_tags(&backend, json),
            "stats" => commands::journal::run_stats(&backend, json),
            _ => Err(anyhow::anyhow!(
                "Unknown journal action '{}'. Valid actions: add, list, search, update, remove, tags, stats",
                action
            )),
        },
        Some(Command::Notes {
            action,
            value,
            id,
            date,
            section,
            since,
            limit,
            json,
        }) => commands::notes::run(
            &action,
            value.as_deref(),
            id,
            date.as_deref(),
            section.as_deref(),
            since.as_deref(),
            limit,
            json,
        ),
        Some(Command::Opportunity {
            action,
            value,
            date,
            asset,
            missed_gain_pct,
            missed_gain_usd,
            avoided_loss_pct,
            avoided_loss_usd,
            rational,
            notes,
            since,
            limit,
            json,
        }) => commands::opportunity::run(
            &action,
            value.as_deref(),
            date.as_deref(),
            asset.as_deref(),
            missed_gain_pct,
            missed_gain_usd,
            avoided_loss_pct,
            avoided_loss_usd,
            rational,
            notes.as_deref(),
            since.as_deref(),
            limit,
            json,
        ),
        Some(Command::Dividends {
            action,
            value,
            amount,
            pay_date,
            ex_date,
            currency,
            notes,
            json,
        }) => {
            let args = commands::dividends::DividendsArgs {
                value,
                amount,
                pay_date,
                ex_date,
                currency,
                notes,
                json,
            };
            commands::dividends::run(&backend, &action, args)
        }
        Some(Command::Annotate {
            symbol,
            thesis,
            invalidation,
            review_date,
            target,
            show,
            list,
            remove,
            json,
        }) => {
            let args = commands::annotate::AnnotateArgs {
                symbol: symbol.as_deref(),
                thesis: thesis.as_deref(),
                invalidation: invalidation.as_deref(),
                review_date: review_date.as_deref(),
                target: target.as_deref(),
                show,
                list,
                remove,
                json,
            };
            commands::annotate::run(&backend, args)
        }
        Some(Command::Group {
            action,
            name,
            symbols,
            json,
        }) => {
            commands::group::run(
                &backend,
                &config,
                &action,
                name.as_deref(),
                symbols.as_deref(),
                json,
            )
        }
        Some(Command::MigrateJournal {
            path,
            dry_run,
            default_tag,
            default_status,
            json,
        }) => {
            commands::migrate_journal::run(
                &backend,
                &path,
                dry_run,
                default_tag.as_deref(),
                &default_status,
                json,
            )
        }

        Some(Command::Web { port, bind, no_auth }) => {
            // Web server runs in async context
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                web::run_server(db_path.to_string_lossy().to_string(), config, &bind, port, !no_auth).await
            })
        }

        Some(Command::Sector { json }) => commands::sector::run(&backend, &config, json),
        Some(Command::Heatmap { json }) => commands::heatmap::run(&backend, json),
        Some(Command::Options {
            symbol,
            expiry,
            limit,
            json,
        }) => commands::options::run(&symbol, expiry.as_deref(), limit, json),
        Some(Command::Portfolio { action, name, json }) => {
            commands::portfolio::run(&action, name.as_deref(), json)
        }
        Some(Command::StressTest { scenario, json }) => {
            commands::stress_test::run(&backend, &config, &scenario, json)
        }
        Some(Command::Research {
            query,
            news,
            freshness,
            count,
            json,
            fed,
            earnings,
            geopolitics,
            cot,
            etf,
            opec,
        }) => {
            let freshness_checked = match freshness.as_deref() {
                Some(v) => Some(commands::research::validate_freshness(v)?),
                None => None,
            };
            let preset = commands::research::ResearchPresetArgs {
                fed,
                earnings,
                geopolitics,
                cot,
                etf,
                opec,
            };
            commands::research::run(
                query.as_deref(),
                news,
                freshness_checked.as_deref(),
                count,
                json,
                preset,
            )
        }
        Some(Command::Structural {
            action,
            value,
            country,
            metric,
            score,
            rank,
            trend,
            stage,
            entered,
            probability,
            horizon,
            description,
            parallel,
            impact,
            driver,
            period,
            event,
            parallel_to,
            similarity,
            outcome,
            evidence,
            signals,
            notes,
            source,
            date,
            since,
            limit,
            json,
        }) => {
            let Some(conn) = backend.sqlite_native() else {
                bail!("structural is currently available only with database_backend=sqlite");
            };
            commands::structural::run(
                conn,
                &action,
                value.as_deref(),
                country.as_deref(),
                metric.as_deref(),
                score,
                rank,
                trend.as_deref(),
                stage.as_deref(),
                entered.as_deref(),
                probability,
                horizon.as_deref(),
                description.as_deref(),
                parallel.as_deref(),
                impact.as_deref(),
                driver.as_deref(),
                period.as_deref(),
                event.as_deref(),
                parallel_to.as_deref(),
                similarity,
                outcome.as_deref(),
                evidence.as_deref(),
                signals.as_deref(),
                notes.as_deref(),
                source.as_deref(),
                date.as_deref(),
                since.as_deref(),
                limit,
                json,
            )
        }
        Some(Command::Trends {
            action,
            value,
            trend,
            category,
            direction,
            conviction,
            description,
            signal,
            status,
            date,
            impact,
            source,
            symbol,
            mechanism,
            timeframe,
            limit,
            json,
        }) => {
            let Some(conn) = backend.sqlite_native() else {
                bail!("trends is currently available only with database_backend=sqlite");
            };
            commands::trends::run(
                conn,
                &action,
                value.as_deref(),
                trend.as_deref(),
                category.as_deref(),
                direction.as_deref(),
                conviction.as_deref(),
                description.as_deref(),
                signal.as_deref(),
                status.as_deref(),
                date.as_deref(),
                impact.as_deref(),
                source.as_deref(),
                symbol.as_deref(),
                mechanism.as_deref(),
                timeframe.as_deref(),
                limit,
                json,
            )
        }
    };

    match (result, backend.flush()) {
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
        (Ok(v), Ok(_)) => Ok(v),
    }
}
