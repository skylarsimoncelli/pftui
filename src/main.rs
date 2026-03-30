mod alerts;
mod analytics;
mod app;
mod broker;
mod cli;
mod commands;
mod config;
mod data;
mod db;
mod indicators;
mod mobile;
mod models;
mod notify;
mod price;
mod regime;
mod tui;
mod web;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Command};
use crate::config::load_config_with_first_run_prompt;
use crate::db::backend::open_from_config;
use crate::db::default_db_path;

fn run_agent_journal(
    backend: &crate::db::backend::BackendConnection,
    command: Option<cli::JournalCommand>,
) -> Result<()> {
    match command {
        None => commands::journal::run_list(backend, Some(20), None, None, None, None, false),
        Some(cli::JournalCommand::Entry { command }) => match command {
            cli::JournalEntryCommand::Add {
                value,
                content,
                date,
                tag,
                symbol,
                conviction,
                json,
            } => {
                let resolved = content.or(value).ok_or_else(|| {
                    anyhow::anyhow!(
                        "journal entry text required.\n\n\
                         Usage:\n  \
                         pftui journal entry add \"your entry text\"\n  \
                         pftui journal entry add --content \"your entry text\"\n\n\
                         Run 'pftui journal entry add --help' for all options."
                    )
                })?;
                commands::journal::run_add(
                    backend,
                    &resolved,
                    date.as_deref(),
                    tag.as_deref(),
                    symbol.as_deref(),
                    conviction.as_deref(),
                    json,
                )
            }
            cli::JournalEntryCommand::List {
                limit,
                since,
                tag,
                symbol,
                filter_status,
                json,
            } => commands::journal::run_list(
                backend,
                limit,
                since.as_deref(),
                tag.as_deref(),
                symbol.as_deref(),
                filter_status.as_deref(),
                json,
            ),
            cli::JournalEntryCommand::Search {
                query,
                since,
                limit,
                json,
            } => commands::journal::run_search(backend, &query, since.as_deref(), limit, json),
            cli::JournalEntryCommand::Update {
                id,
                content,
                status,
                json,
            } => commands::journal::run_update(
                backend,
                id,
                content.as_deref(),
                status.as_deref(),
                json,
            ),
            cli::JournalEntryCommand::Remove { id, json } => {
                commands::journal::run_remove(backend, id, json)
            }
            cli::JournalEntryCommand::Tags { json } => commands::journal::run_tags(backend, json),
            cli::JournalEntryCommand::Stats { json } => commands::journal::run_stats(backend, json),
        },
        Some(cli::JournalCommand::Prediction { command }) => match command {
            cli::JournalPredictionCommand::Add {
                value,
                claim,
                timeframe_pos,
                confidence_pos,
                symbol,
                conviction,
                timeframe,
                confidence,
                source_agent,
                target_date,
                resolution_criteria,
                json,
            } => {
                let text = claim.or(value).ok_or_else(|| {
                    anyhow::anyhow!(
                        "No prediction text provided. Use --claim \"your prediction\" or pass it as the first positional argument.\n\
                         Examples:\n  pftui journal prediction add --claim \"BTC above 70k\" --timeframe low\n  \
                         pftui journal prediction add \"BTC above 70k\" --timeframe low"
                    )
                })?;
                commands::predict::run(
                backend,
                "add",
                Some(&text),
                None,
                symbol.as_deref(),
                conviction.as_deref(),
                timeframe.as_deref().or(timeframe_pos.as_deref()),
                confidence.or(confidence_pos),
                source_agent.as_deref(),
                target_date.as_deref(),
                resolution_criteria.as_deref(),
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            )},
            cli::JournalPredictionCommand::List {
                filter,
                timeframe,
                symbol,
                limit,
                json,
            } => commands::predict::run(
                backend,
                "list",
                None,
                None,
                symbol.as_deref(),
                None,
                timeframe.as_deref(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                filter.as_deref(),
                None,
                limit,
                json,
            ),
            cli::JournalPredictionCommand::Score {
                id,
                id_pos,
                outcome,
                outcome_pos,
                notes,
                notes_pos,
                lesson,
                json,
            } => {
                let merged_outcome = outcome.or(outcome_pos);
                let merged_notes = notes.or(notes_pos);
                commands::predict::run(
                    backend,
                    "score",
                    None,
                    id.or(id_pos),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    merged_outcome.as_deref(),
                    merged_notes.as_deref(),
                    lesson.as_deref(),
                    None,
                    None,
                    None,
                    json,
                )
            }
            cli::JournalPredictionCommand::ScoreBatch { entries, json } => {
                commands::predict::run_score_batch(backend, &entries, json)
            }
            cli::JournalPredictionCommand::Stats {
                timeframe,
                agent,
                json,
            } => commands::predict::run(
                backend,
                "stats",
                None,                          // value
                None,                          // id
                None,                          // symbol
                None,                          // conviction
                timeframe.as_deref(),          // timeframe
                None,                          // confidence
                agent.as_deref(),              // source_agent
                None,                          // target_date
                None,                          // resolution_criteria
                None,                          // outcome
                None,                          // notes
                None,                          // lesson
                None,                          // filter
                None,                          // date
                None,                          // limit
                json,
            ),
            cli::JournalPredictionCommand::Scorecard { date, limit, json } => {
                commands::predict::run(
                    backend,
                    "scorecard",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    date.as_deref(),
                    limit,
                    json,
                )
            }
            cli::JournalPredictionCommand::AutoScore { dry_run, json } => {
                commands::predict::run_auto_score(backend, dry_run, json)
            }
            cli::JournalPredictionCommand::Lessons {
                command,
                miss_type,
                limit,
                json,
            } => match command {
                None => commands::predict::run_lessons(
                    backend,
                    miss_type.as_deref(),
                    limit,
                    json,
                ),
                Some(cli::JournalPredictionLessonsCommand::Add {
                    prediction_id,
                    miss_type: mt,
                    what_happened,
                    why_wrong,
                    signal_misread,
                    json: json_flag,
                }) => commands::predict::run_add_lesson(
                    backend,
                    prediction_id,
                    &mt,
                    &what_happened,
                    &why_wrong,
                    signal_misread.as_deref(),
                    json_flag,
                ),
            },
        },
        Some(cli::JournalCommand::Conviction { command }) => match command {
            cli::JournalConvictionCommand::Set {
                symbol,
                score_pos,
                score,
                notes,
                notes_pos,
                json,
            } => {
                let score_val = score.or(score_pos).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing score. Usage: pftui journal conviction set SYMBOL <SCORE> [NOTES] or --score N [--notes ...]"
                    )
                })?;
                let merged_notes = notes.or(notes_pos);
                commands::conviction::run_set(
                    backend,
                    &symbol,
                    score_val,
                    merged_notes.as_deref(),
                    json,
                )
            }
            cli::JournalConvictionCommand::List { json } => {
                commands::conviction::run_list(backend, json)
            }
            cli::JournalConvictionCommand::History {
                symbol,
                limit,
                json,
            } => commands::conviction::run_history(backend, &symbol, limit, json),
            cli::JournalConvictionCommand::Changes { days, json } => {
                let d = days
                    .as_deref()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(7);
                commands::conviction::run_changes(backend, d, json)
            }
        },
        Some(cli::JournalCommand::Notes { command }) => match command {
            cli::JournalNotesCommand::Add {
                value,
                date,
                section,
                json,
            } => commands::notes::run(
                backend,
                "add",
                Some(&value),
                None,
                date.as_deref(),
                section.as_deref(),
                None,
                None,
                json,
            ),
            cli::JournalNotesCommand::List { since, limit, json } => commands::notes::run(
                backend,
                "list",
                None,
                None,
                None,
                None,
                since.as_deref(),
                limit,
                json,
            ),
            cli::JournalNotesCommand::Search {
                query,
                since,
                limit,
                json,
            } => commands::notes::run(
                backend,
                "search",
                Some(&query),
                None,
                None,
                None,
                since.as_deref(),
                limit,
                json,
            ),
            cli::JournalNotesCommand::Remove { id, json } => commands::notes::run(
                backend,
                "remove",
                None,
                Some(id),
                None,
                None,
                None,
                None,
                json,
            ),
        },
        Some(cli::JournalCommand::Scenario { command }) => match command {
            cli::JournalScenarioCommand::Add {
                value,
                probability,
                description,
                impact,
                triggers,
                precedent,
                status,
                json,
            } => commands::scenario::run(
                backend,
                "add",
                Some(&value),
                None,
                None,
                probability,
                description.as_deref(),
                impact.as_deref(),
                triggers.as_deref(),
                precedent.as_deref(),
                status.as_deref(),
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::JournalScenarioCommand::List {
                status,
                limit,
                json,
            } => commands::scenario::run(
                backend,
                "list",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                status.as_deref(),
                None,
                None,
                None,
                None,
                None,
                limit,
                json,
            ),
            cli::JournalScenarioCommand::Update {
                value,
                note_pos,
                probability,
                description,
                impact,
                triggers,
                precedent,
                status,
                driver,
                notes,
                json,
            } => {
                let merged_notes = driver.or(notes).or(note_pos);
                commands::scenario::run(
                    backend,
                    "update",
                    Some(&value),
                    None,
                    None,
                    probability,
                    description.as_deref(),
                    impact.as_deref(),
                    triggers.as_deref(),
                    precedent.as_deref(),
                    status.as_deref(),
                    merged_notes.as_deref(),
                    merged_notes.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    json,
                )
            }
            cli::JournalScenarioCommand::Remove { value, json } => commands::scenario::run(
                backend,
                "remove",
                Some(&value),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::JournalScenarioCommand::History { value, limit, json } => commands::scenario::run(
                backend,
                "history",
                Some(&value),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                limit,
                json,
            ),
            cli::JournalScenarioCommand::Promote { value, json } => commands::scenario::run(
                backend,
                "promote",
                Some(&value),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::JournalScenarioCommand::Signal { command } => match command {
                cli::JournalScenarioSignalCommand::Add {
                    value,
                    scenario,
                    source,
                    status,
                    json,
                } => commands::scenario::run(
                    backend,
                    "signal-add",
                    Some(&value),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    status.as_deref(),
                    None,
                    None,
                    None,
                    source.as_deref(),
                    scenario.as_deref(),
                    None,
                    json,
                ),
                cli::JournalScenarioSignalCommand::List {
                    scenario,
                    status,
                    limit,
                    json,
                } => commands::scenario::run(
                    backend,
                    "signal-list",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    status.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    scenario.as_deref(),
                    limit,
                    json,
                ),
                cli::JournalScenarioSignalCommand::Update {
                    signal_id,
                    evidence,
                    status,
                    json,
                } => commands::scenario::run(
                    backend,
                    "signal-update",
                    None,
                    None,
                    Some(signal_id),
                    None,
                    None,
                    None,
                    None,
                    None,
                    status.as_deref(),
                    None,
                    None,
                    evidence.as_deref(),
                    None,
                    None,
                    None,
                    json,
                ),
                cli::JournalScenarioSignalCommand::Remove { signal_id, json } => {
                    commands::scenario::run(
                        backend,
                        "signal-remove",
                        None,
                        None,
                        Some(signal_id),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        json,
                    )
                }
            },
        },
    }
}

/// Dispatch `data predictions` / `analytics predictions` subcommands.
/// When no subcommand is given, falls back to showing prediction market odds.
fn dispatch_predictions(
    backend: &db::backend::BackendConnection,
    command: Option<cli::DataPredictionsCommand>,
    category: Option<String>,
    search: Option<String>,
    limit: usize,
    json: bool,
) -> Result<()> {
    match command {
        None | Some(cli::DataPredictionsCommand::Markets { .. }) => {
            // Merge top-level flags with subcommand flags (subcommand wins if present)
            let (cat, srch, lim, js) = match command {
                Some(cli::DataPredictionsCommand::Markets {
                    category: sc,
                    search: ss,
                    limit: sl,
                    json: sj,
                }) => (
                    sc.or(category),
                    ss.or(search),
                    sl, // subcommand limit takes precedence (has its own default)
                    sj || json,
                ),
                _ => (category, search, limit, json),
            };
            commands::predictions::run(backend, cat.as_deref(), srch.as_deref(), lim, js)
        }
        Some(cli::DataPredictionsCommand::Stats {
            timeframe,
            agent,
            json: j,
        }) => {
            commands::predict::run(
                backend,
                "stats",
                None,                          // value
                None,                          // id
                None,                          // symbol
                None,                          // conviction
                timeframe.as_deref(),          // timeframe
                None,                          // confidence
                agent.as_deref(),              // source_agent
                None,                          // target_date
                None,                          // resolution_criteria
                None,                          // outcome
                None,                          // notes
                None,                          // lesson
                None,                          // filter
                None,                          // date
                None,                          // limit
                j || json,
            )
        }
        Some(cli::DataPredictionsCommand::Scorecard {
            date,
            limit: lim,
            json: j,
        }) => commands::predict::run(
            backend,
            "scorecard",
            None,               // value
            None,               // id
            None,               // symbol
            None,               // conviction
            None,               // timeframe
            None,               // confidence
            None,               // source_agent
            None,               // target_date
            None,               // resolution_criteria
            None,               // outcome
            None,               // notes
            None,               // lesson
            None,               // filter
            date.as_deref(),    // date
            lim,                // limit
            j || json,
        ),
        Some(cli::DataPredictionsCommand::Unanswered {
            timeframe,
            symbol,
            limit: lim,
            json: j,
        }) => commands::predict::run(
            backend,
            "list",
            None,                          // value
            None,                          // id
            symbol.as_deref(),             // symbol
            None,                          // conviction
            timeframe.as_deref(),          // timeframe
            None,                          // confidence
            None,                          // source_agent
            None,                          // target_date
            None,                          // resolution_criteria
            None,                          // outcome
            None,                          // notes
            None,                          // lesson
            Some("pending"),               // filter = pending
            None,                          // date
            lim,                           // limit
            j || json,
        ),
        Some(cli::DataPredictionsCommand::Map {
            scenario,
            search,
            contract,
            list,
            json: j,
        }) => commands::predictions_map::run_map(
            backend,
            scenario.as_deref(),
            search.as_deref(),
            contract.as_deref(),
            list,
            j || json,
        ),
        Some(cli::DataPredictionsCommand::Unmap {
            scenario,
            contract,
            json: j,
        }) => commands::predictions_map::run_unmap(
            backend,
            &scenario,
            contract.as_deref(),
            j || json,
        ),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cached_only = cli.cached_only;

    if matches!(cli.command, Some(Command::Console)) {
        return commands::console::run(cached_only);
    }

    // Search doesn't need database — intercept early
    if let Some(Command::System {
        command: cli::SystemCommand::Search { ref query, json },
    }) = cli.command
    {
        let query_str = query.join(" ");
        let cli_cmd = Cli::command();
        return commands::search::run(cli_cmd, &query_str, json);
    }

    // Market hours doesn't need database — intercept early
    if let Some(Command::System {
        command: cli::SystemCommand::MarketHours { json },
    }) = cli.command
    {
        return commands::market_hours::run(json);
    }

    let config = load_config_with_first_run_prompt()?;
    let db_path = default_db_path();

    if let Some(Command::System {
        command: cli::SystemCommand::Mirror { ref command },
    }) = cli.command
    {
        return commands::mirror::run(&config, &db_path, command);
    }

    let should_sync_mirror_on_startup = cli.command.is_none()
        || matches!(
            cli.command,
            Some(Command::System {
                command: cli::SystemCommand::Web { .. }
            })
        )
        || matches!(
            cli.command,
            Some(Command::System {
                command: cli::SystemCommand::Mobile {
                    command: cli::MobileCommand::Serve,
                }
            })
        );
    if should_sync_mirror_on_startup {
        commands::mirror::spawn_startup_sync_if_needed(&config, &db_path);
    }

    let backend = open_from_config(&config, &db_path)?;

    let result = match cli.command {
        None => {
            // Auto-detect: run setup if no portfolio data
            if !commands::setup::has_portfolio_data(&backend) {
                commands::setup::run(&config, false)?;
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

        Some(Command::Console) => unreachable!("console handled before backend initialization"),

        Some(Command::Journal { command }) => run_agent_journal(&backend, command),

        Some(Command::Data { command }) => match command {
            cli::DataCommand::Refresh { notify, json } => {
                if cached_only {
                    if json {
                        println!("{{\"error\": \"cached-only mode enabled\"}}");
                    } else {
                        println!("Cached-only mode enabled; skipping refresh network calls.");
                    }
                    Ok(())
                } else if json {
                    commands::refresh::run_json(&backend, &config, notify)
                } else {
                    commands::refresh::run(&backend, &config, notify)
                }
            }
            cli::DataCommand::Status { json, .. } => commands::status::run_backend(&backend, json),
            cli::DataCommand::Dashboard { command } => match command {
                cli::DashboardCommand::Macro { json } => {
                    commands::macro_cmd::run(&backend, &config, json, cached_only)
                }
                cli::DashboardCommand::Oil { json } => {
                    commands::oil::run(&backend, json, cached_only)
                }
                cli::DashboardCommand::Crisis { json } => {
                    commands::crisis::run(&backend, json, cached_only)
                }
                cli::DashboardCommand::Sector { json } => {
                    commands::sector::run(&backend, &config, json)
                }
                cli::DashboardCommand::Heatmap { json } => commands::heatmap::run(&backend, json),
                cli::DashboardCommand::Global {
                    country,
                    indicator,
                    json,
                } => {
                    commands::global::run(&backend, country.as_deref(), indicator.as_deref(), json)
                }
            },
            cli::DataCommand::News {
                source,
                search,
                hours,
                limit,
                with_sentiment,
                json,
            } => commands::news::run(
                &backend,
                source.as_deref(),
                search.as_deref(),
                hours,
                limit,
                with_sentiment,
                json,
            ),
            cli::DataCommand::Sentiment {
                symbol,
                history,
                json,
            } => commands::sentiment::run(&backend, symbol.as_deref(), history, json),
            cli::DataCommand::Calendar { days, impact, json } => {
                commands::calendar::run(days, impact.as_deref(), json)
            }
            cli::DataCommand::Cot { symbol, json } => {
                commands::cot::run(&backend, symbol.as_deref(), json)
            }
            cli::DataCommand::Fedwatch { json } => commands::fedwatch::run(&backend, &config, json),
            cli::DataCommand::Onchain { json } => commands::onchain::run(&backend, json),
            cli::DataCommand::Economy { indicator, json } => {
                commands::economy::run(&backend, indicator.as_deref(), json)
            }
            cli::DataCommand::Consensus { command } => match command {
                cli::ConsensusCommand::Add {
                    source,
                    topic,
                    call_text,
                    date,
                    json,
                } => commands::consensus::run(
                    &backend,
                    "add",
                    Some(&source),
                    Some(&topic),
                    Some(&call_text),
                    Some(&date),
                    20,
                    json,
                ),
                cli::ConsensusCommand::List {
                    topic,
                    source,
                    limit,
                    json,
                } => commands::consensus::run(
                    &backend,
                    "list",
                    source.as_deref(),
                    topic.as_deref(),
                    None,
                    None,
                    limit,
                    json,
                ),
            },
            cli::DataCommand::Predictions {
                command,
                category,
                search,
                limit,
                json,
            } => dispatch_predictions(&backend, command, category, search, limit, json),
            cli::DataCommand::Options {
                symbol,
                expiry,
                limit,
                json,
            } => commands::options::run(&symbol, expiry.as_deref(), limit, json),
            cli::DataCommand::EtfFlows { days, fund, json } => {
                commands::etf_flows::run(days, fund, json)
            }
            cli::DataCommand::Supply { symbol, json } => {
                commands::supply::run(&backend, symbol, json)
            }
            cli::DataCommand::Sovereign { json } => commands::sovereign::run(&backend, json),
            cli::DataCommand::Prices { market, json } => {
                commands::prices::run(&backend, market, json)
            }
            cli::DataCommand::OilInventory { weeks, json } => {
                commands::oil_inventory::run(&config, weeks, json)
            }
            cli::DataCommand::Futures { json } => {
                commands::futures::run(&backend, json, cached_only)
            }
            cli::DataCommand::OilPremium { json } => commands::oil_premium::run(&backend, json),
            cli::DataCommand::Backfill { json } => commands::backfill::run(&backend, json),
            cli::DataCommand::Alerts { command } => {
                let (action, args) = match command {
                    Some(cli::DataAlertsRedirect::Check { today, json }) => (
                        "check",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json,
                            status_filter: None,
                            today,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    Some(cli::DataAlertsRedirect::List {
                        status,
                        triggered,
                        since,
                        today,
                        recent,
                        recent_hours,
                        json,
                    }) => (
                        "list",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json,
                            status_filter: status,
                            today,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered,
                            since_hours: since,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent,
                            recent_hours,
                        },
                    ),
                    None => {
                        println!("Alert management is under `analytics alerts`.");
                        println!();
                        println!("Common commands:");
                        println!("  pftui analytics alerts check       Check alerts against current data");
                        println!("  pftui analytics alerts list        List alert rules");
                        println!("  pftui analytics alerts add         Add an alert rule");
                        println!("  pftui analytics alerts ack         Acknowledge triggered alerts");
                        println!("  pftui analytics alerts seed-defaults  Seed smart-alert defaults");
                        println!();
                        println!("Run `pftui analytics alerts --help` for full details.");
                        return Ok(());
                    }
                };
                commands::alerts::run(&backend, action, &args)
            }
        },
        Some(Command::System { command }) => match command {
            cli::SystemCommand::Daemon { command } => match command {
                cli::DaemonCommand::Start { interval, json } => {
                    commands::daemon::run(&config, &db_path, interval, json)
                }
                cli::DaemonCommand::Status { json } => commands::daemon::run_status(json),
            },
            cli::SystemCommand::Config {
                action,
                field,
                value,
                json,
            } => commands::config_cmd::run(&action, field.as_deref(), value.as_deref(), json),
            cli::SystemCommand::DbInfo { json } => {
                commands::db_info::run(&backend, &db_path, config.database_url.as_deref(), json)
            }
            cli::SystemCommand::Doctor { json } => {
                let runtime = tokio::runtime::Runtime::new()?;
                runtime.block_on(async { commands::doctor::run(json).await })
            }
            cli::SystemCommand::Export { format, output } => {
                commands::export::run(&backend, &format, &config, output.as_deref())
            }
            cli::SystemCommand::Import { path, mode } => {
                let import_mode = match mode {
                    cli::ImportModeArg::Replace => commands::import::ImportMode::Replace,
                    cli::ImportModeArg::Merge => commands::import::ImportMode::Merge,
                };
                commands::import::run(&backend, &config, &path, import_mode)
            }
            cli::SystemCommand::Mirror { .. } => {
                unreachable!("mirror handled before backend initialization")
            }
            cli::SystemCommand::Snapshot {
                width,
                height,
                plain,
            } => commands::snapshot::run(&config, Some(width), Some(height), plain),
            cli::SystemCommand::Setup => commands::setup::run(&config, true),
            cli::SystemCommand::Demo => commands::demo::run(&config),
            cli::SystemCommand::Web {
                port,
                bind,
                no_auth,
            } => {
                let runtime = tokio::runtime::Runtime::new()?;
                runtime.block_on(async {
                    web::run_server(
                        db_path.to_string_lossy().to_string(),
                        config,
                        &bind,
                        port,
                        !no_auth,
                    )
                    .await
                })
            }
            cli::SystemCommand::Mobile { command } => match command {
                cli::MobileCommand::Enable { bind, port } => {
                    mobile::commands::enable(&config, &bind, port)
                }
                cli::MobileCommand::Disable => mobile::commands::disable(&config),
                cli::MobileCommand::Status { json } => mobile::commands::status(&config, json),
                cli::MobileCommand::Token { command } => match command {
                    cli::MobileTokenCommand::Generate { name, permission } => {
                        mobile::commands::generate_token(&config, &name, permission)
                    }
                    cli::MobileTokenCommand::List { json } => {
                        mobile::commands::list_tokens(&config, json)
                    }
                    cli::MobileTokenCommand::Revoke { prefix, json } => {
                        mobile::commands::revoke_token(&config, &prefix, json)
                    }
                },
                cli::MobileCommand::Serve => {
                    let server_backend = backend.clone_for_server()?;
                    crate::db::pg_runtime::block_on(mobile::server::run_server(
                        server_backend,
                        config,
                    ))
                }
            },
            cli::SystemCommand::Universe { command } => match command {
                cli::UniverseCommand::List { json } => commands::universe::list(json),
                cli::UniverseCommand::Add {
                    symbol,
                    group,
                    json,
                } => commands::universe::add(&symbol, &group, json),
                cli::UniverseCommand::Remove {
                    symbol,
                    group,
                    json,
                } => commands::universe::remove(&symbol, &group, json),
            },
            cli::SystemCommand::Search { query, json } => {
                let query_str = query.join(" ");
                let cli_cmd = cli::Cli::command();
                commands::search::run(cli_cmd, &query_str, json)
            }
            cli::SystemCommand::MarketHours { json } => {
                commands::market_hours::run(json)
            }
            cli::SystemCommand::MigrateJournal {
                path,
                dry_run,
                default_tag,
                default_status,
                json,
            } => commands::migrate_journal::run(
                &backend,
                &path,
                dry_run,
                default_tag.as_deref(),
                &default_status,
                json,
            ),
        },
        Some(Command::Portfolio { command }) => match command {
            None => commands::summary::run(&backend, &config, None, None, None, true, false),
            Some(cli::PortfolioCommand::Summary {
                group_by,
                period,
                what_if,
                json,
            }) => commands::summary::run(
                &backend,
                &config,
                group_by.as_ref(),
                period.as_ref(),
                what_if.as_deref(),
                true,
                json,
            ),
            Some(cli::PortfolioCommand::Value { json }) => {
                commands::value::run(&backend, &config, json)
            }
            Some(cli::PortfolioCommand::Brief { json }) => {
                commands::brief::run_backend(&backend, &config, true, json, cached_only)
            }
            Some(cli::PortfolioCommand::Eod { json }) => {
                commands::eod::run(&backend, &config, json)
            }
            Some(cli::PortfolioCommand::DailyPnl { json }) => {
                commands::daily_pnl::run(&backend, &config, json)
            }
            Some(cli::PortfolioCommand::Unrealized { group_by, json }) => {
                commands::unrealized::run(
                    &backend,
                    &config,
                    group_by.is_some(),
                    json,
                )
            }
            Some(cli::PortfolioCommand::Performance {
                since,
                period,
                vs,
                json,
            }) => commands::performance::run(
                &backend,
                &config,
                since.as_deref(),
                period.as_deref(),
                vs.as_deref(),
                json,
            ),
            Some(cli::PortfolioCommand::History { date, group_by }) => {
                commands::history::run(&backend, &config, &date, group_by.as_ref())
            }
            Some(cli::PortfolioCommand::Target { command }) => match command {
                cli::PortfolioTargetCommand::Set {
                    symbol,
                    target,
                    band,
                } => {
                    let sym = symbol
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--symbol required for 'set'"))?;
                    let tgt = target
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--target required for 'set'"))?;
                    commands::target::run(&backend, sym, tgt, band.as_deref())
                }
                cli::PortfolioTargetCommand::List { json } => {
                    commands::target::list(&backend, json)
                }
                cli::PortfolioTargetCommand::Remove { symbol } => {
                    let sym = symbol
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--symbol required for 'remove'"))?;
                    commands::target::remove(&backend, sym)
                }
            },
            Some(cli::PortfolioCommand::Allocation { group_by, json }) => {
                commands::allocation::run(&backend, group_by.as_ref(), json)
            }
            Some(cli::PortfolioCommand::Drift { json }) => commands::drift::run(&backend, json),
            Some(cli::PortfolioCommand::Rebalance { json }) => {
                commands::rebalance::run(&backend, json)
            }
            Some(cli::PortfolioCommand::StressTest {
                scenario,
                list_scenarios,
                json,
            }) => {
                if list_scenarios {
                    commands::stress_test::run_list(&backend, json)
                } else {
                    let scenario = scenario.ok_or_else(|| {
                        anyhow::anyhow!(
                            "Scenario name required. Use --list-scenarios to see available options."
                        )
                    })?;
                    commands::stress_test::run(&backend, &config, &scenario, json)
                }
            }
            Some(cli::PortfolioCommand::Dividends {
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
            Some(cli::PortfolioCommand::Annotate {
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
            Some(cli::PortfolioCommand::Group {
                action,
                name,
                symbols,
                json,
            }) => commands::group::run(
                &backend,
                &config,
                &action,
                name.as_deref(),
                symbols.as_deref(),
                json,
            ),
            Some(cli::PortfolioCommand::Opportunity { command }) => match command {
                cli::PortfolioOpportunityCommand::Add {
                    value,
                    date,
                    asset,
                    missed_gain_pct,
                    missed_gain_usd,
                    avoided_loss_pct,
                    avoided_loss_usd,
                    rational,
                    notes,
                    json,
                } => commands::opportunity::run(
                    &backend,
                    "add",
                    value.as_deref(),
                    date.as_deref(),
                    asset.as_deref(),
                    missed_gain_pct,
                    missed_gain_usd,
                    avoided_loss_pct,
                    avoided_loss_usd,
                    rational,
                    notes.as_deref(),
                    None,
                    None,
                    json,
                ),
                cli::PortfolioOpportunityCommand::List {
                    since,
                    asset,
                    limit,
                    json,
                } => commands::opportunity::run(
                    &backend,
                    "list",
                    None,
                    None,
                    asset.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    since.as_deref(),
                    limit,
                    json,
                ),
                cli::PortfolioOpportunityCommand::Stats { since, json } => {
                    commands::opportunity::run(
                        &backend,
                        "stats",
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        since.as_deref(),
                        None,
                        json,
                    )
                }
            },
            Some(cli::PortfolioCommand::Profiles { command }) => match command {
                cli::PortfolioProfilesCommand::List { json } => {
                    commands::portfolio::run("list", None, json)
                }
                cli::PortfolioProfilesCommand::Current { json } => {
                    commands::portfolio::run("current", None, json)
                }
                cli::PortfolioProfilesCommand::Create { name, json } => {
                    commands::portfolio::run("create", name.as_deref(), json)
                }
                cli::PortfolioProfilesCommand::Switch { name, json } => {
                    commands::portfolio::run("switch", name.as_deref(), json)
                }
                cli::PortfolioProfilesCommand::Remove { name, json } => {
                    commands::portfolio::run("remove", name.as_deref(), json)
                }
            },
            Some(cli::PortfolioCommand::Watchlist {
                action,
                approaching,
                json,
            }) => match action {
                Some(cli::WatchlistCommand::Add {
                    symbol,
                    category,
                    bulk,
                    target,
                    direction,
                }) => {
                    use crate::models::asset_names::infer_category;

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
                    if let Some(ref t) = target {
                        let cleaned = t.replace(['$', ','], "");
                        if rust_decimal::Decimal::from_str_exact(&cleaned).is_err() {
                            bail!(
                                "Invalid target price: '{}'. Use a number (e.g. 300, 55000.50)",
                                t
                            );
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
                        if let Some(ref t) = target {
                            let cleaned = t.replace(['$', ','], "");
                            db::watchlist::set_watchlist_target_backend(
                                &backend,
                                upper,
                                Some(&cleaned),
                                Some(&direction),
                            )?;
                            println!("  Target: {} {} {}", upper, direction, cleaned);
                            let rule_text = format!("{} {} {}", upper, direction, cleaned);
                            db::alerts::add_alert_backend(
                                &backend,
                                db::alerts::NewAlert {
                                    kind: "price",
                                    symbol: upper,
                                    direction: &direction,
                                    condition: None,
                                    threshold: &cleaned,
                                    rule_text: &rule_text,
                                    recurring: false,
                                    cooldown_minutes: 0,
                                },
                            )?;
                            println!("  Alert created: {}", rule_text);
                        }
                        added += 1;
                    }
                    if added > 1 {
                        println!("\n{} symbols added to watchlist.", added);
                    }
                    Ok(())
                }
                Some(cli::WatchlistCommand::Remove { symbol }) => {
                    let upper = symbol.to_uppercase();
                    if db::watchlist::remove_from_watchlist_backend(&backend, &upper)? {
                        println!("Removed {} from watchlist", upper);
                    } else {
                        println!("{} was not in the watchlist", upper);
                    }
                    Ok(())
                }
                Some(cli::WatchlistCommand::List { approaching, json }) => {
                    commands::watchlist_cli::run(
                        &backend,
                        &config,
                        approaching.as_deref(),
                        json,
                        cached_only,
                    )
                }
                None => commands::watchlist_cli::run(
                    &backend,
                    &config,
                    approaching.as_deref(),
                    json,
                    cached_only,
                ),
            },
            Some(cli::PortfolioCommand::SetCash { symbol, amount }) => {
                commands::set_cash::run(&backend, &symbol, &amount)
            }
            Some(cli::PortfolioCommand::Transaction { command }) => match command {
                cli::PortfolioTransactionCommand::Add {
                    symbol,
                    category,
                    tx_type,
                    quantity,
                    price,
                    currency,
                    date,
                    notes,
                } => {
                    if config.is_percentage_mode() {
                        bail!("add-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::add_tx::run(
                        &backend, symbol, category, tx_type, quantity, price, currency, date, notes,
                    )
                }
                cli::PortfolioTransactionCommand::Remove { id } => {
                    if config.is_percentage_mode() {
                        bail!("remove-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::remove_tx::run(&backend, id)
                }
                cli::PortfolioTransactionCommand::List { notes, json } => {
                    if config.is_percentage_mode() {
                        bail!("list-tx is not available in percentage mode (no transactions).\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::list_tx::run(&backend, notes, json)
                }
            },
            Some(cli::PortfolioCommand::Broker { command }) => match command {
                cli::PortfolioBrokerCommand::Add {
                    broker,
                    api_key,
                    secret,
                    label,
                    json,
                } => commands::broker::run_add(
                    &backend,
                    broker,
                    api_key.as_deref(),
                    secret.as_deref(),
                    label.as_deref(),
                    json,
                ),
                cli::PortfolioBrokerCommand::Sync {
                    broker,
                    dry_run,
                    json,
                } => commands::broker::run_sync(&backend, broker, dry_run, json),
                cli::PortfolioBrokerCommand::Remove { broker, json } => {
                    commands::broker::run_remove(&backend, broker, json)
                }
                cli::PortfolioBrokerCommand::List { json } => {
                    commands::broker::run_list(&backend, json)
                }
            },
        },

        Some(Command::Agent { command }) => match command {
            crate::cli::AgentCommand::Message { command } => match command {
                cli::AgentMessageCommand::Send {
                    value,
                    batch,
                    package_id,
                    package_title,
                    from,
                    to,
                    priority,
                    category,
                    layer,
                    json,
                } => commands::agent_msg::run(
                    &backend,
                    "send",
                    value.as_deref(),
                    &batch,
                    None,
                    &[],
                    package_id.as_deref(),
                    package_title.as_deref(),
                    from.as_deref(),
                    to.as_deref(),
                    priority.as_deref(),
                    category.as_deref(),
                    layer.as_deref(),
                    false,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AgentMessageCommand::List {
                    from,
                    to,
                    layer,
                    unacked,
                    since,
                    package_id,
                    limit,
                    json,
                } => commands::agent_msg::run(
                    &backend,
                    "list",
                    None,
                    &[],
                    None,
                    &[],
                    package_id.as_deref(),
                    None,
                    from.as_deref(),
                    to.as_deref(),
                    None,
                    None,
                    layer.as_deref(),
                    unacked,
                    since.as_deref(),
                    None,
                    limit,
                    json,
                ),
                cli::AgentMessageCommand::Reply {
                    value,
                    id,
                    from,
                    priority,
                    category,
                    layer,
                    json,
                } => commands::agent_msg::run(
                    &backend,
                    "reply",
                    value.as_deref(),
                    &[],
                    id,
                    &[],
                    None,
                    None,
                    from.as_deref(),
                    None,
                    priority.as_deref(),
                    category.as_deref(),
                    layer.as_deref(),
                    false,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AgentMessageCommand::Flag {
                    value,
                    id,
                    quality,
                    from,
                    priority,
                    category,
                    layer,
                    json,
                } => commands::agent_msg::run(
                    &backend,
                    "flag",
                    value.as_deref().or(if quality {
                        Some("Data quality issue detected")
                    } else {
                        None
                    }),
                    &[],
                    id,
                    &[],
                    None,
                    None,
                    from.as_deref(),
                    None,
                    priority.as_deref(),
                    category.as_deref(),
                    layer.as_deref(),
                    false,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AgentMessageCommand::Ack {
                    id,
                    all,
                    to,
                    json,
                } => {
                    if all {
                        // --all flag: behave like ack-all
                        commands::agent_msg::run(
                            &backend,
                            "ack-all",
                            None,
                            &[],
                            None,
                            &[],
                            None,
                            None,
                            None,
                            to.as_deref(),
                            None,
                            None,
                            None,
                            false,
                            None,
                            None,
                            None,
                            json,
                        )
                    } else {
                        commands::agent_msg::run(
                            &backend,
                            "ack",
                            None,
                            &[],
                            None,
                            &id,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            false,
                            None,
                            None,
                            None,
                            json,
                        )
                    }
                }
                cli::AgentMessageCommand::AckAll { to, json } => commands::agent_msg::run(
                    &backend,
                    "ack-all",
                    None,
                    &[],
                    None,
                    &[],
                    None,
                    None,
                    None,
                    to.as_deref(),
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AgentMessageCommand::Purge { days, json } => commands::agent_msg::run(
                    &backend,
                    "purge",
                    None,
                    &[],
                    None,
                    &[],
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    days,
                    None,
                    json,
                ),
            },
            crate::cli::AgentCommand::Debate { command } => match command {
                cli::AgentDebateCommand::Start {
                    topic,
                    rounds,
                    json,
                } => commands::debate::start(&backend, &topic, rounds, json),
                cli::AgentDebateCommand::AddRound {
                    debate_id,
                    round,
                    position,
                    argument,
                    agent_source,
                    evidence,
                    json,
                } => commands::debate::add_round(
                    &backend,
                    &commands::debate::AddRoundParams {
                        debate_id,
                        round_num: round,
                        position: &position,
                        agent_source: agent_source.as_deref(),
                        argument: &argument,
                        evidence: evidence.as_deref(),
                        json_output: json,
                    },
                ),
                cli::AgentDebateCommand::Resolve {
                    debate_id,
                    summary,
                    json,
                } => commands::debate::resolve(&backend, debate_id, summary.as_deref(), json),
                cli::AgentDebateCommand::History {
                    status,
                    topic,
                    limit,
                    json,
                } => commands::debate::history(
                    &backend,
                    status.as_deref(),
                    topic.as_deref(),
                    limit,
                    json,
                ),
                cli::AgentDebateCommand::Summary { debate_id, json } => {
                    commands::debate::summary(&backend, debate_id, json)
                }
            },
        },
        Some(Command::Analytics { command }) => match command {
            cli::AnalyticsCommand::Asset { symbol, json } => {
                commands::analytics::run_asset_intelligence(&backend, &symbol, json)
            }
            cli::AnalyticsCommand::Technicals {
                symbol,
                timeframe,
                limit,
                json,
            } => commands::analytics::run(
                &backend,
                "technicals",
                Some(&timeframe),
                None,
                None,
                symbol.as_deref(),
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                limit,
                json,
            ),
            cli::AnalyticsCommand::Levels {
                symbol,
                level_type,
                limit,
                json,
            } => commands::analytics::run(
                &backend,
                "levels",
                None,
                None,
                None,
                symbol.as_deref(),
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                level_type.as_deref(),
                None,
                None,
                None,
                limit,
                json,
            ),
            cli::AnalyticsCommand::Signals {
                symbol,
                signal_type,
                severity,
                source,
                limit,
                json,
            } => commands::analytics::run_signals_combined(
                &backend,
                symbol.as_deref(),
                signal_type.as_deref(),
                severity.as_deref(),
                &source,
                limit,
                json,
            ),
            cli::AnalyticsCommand::Summary { json } => commands::analytics::run(
                &backend,
                "summary",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Situation { command, json } => match command {
                None | Some(cli::SituationCommand::Dashboard { .. }) => {
                    // Use top-level --json OR dashboard subcommand --json
                    let use_json = json
                        || matches!(
                            command,
                            Some(cli::SituationCommand::Dashboard { json: true })
                        );
                    commands::analytics::run(
                        &backend,
                        "situation",
                        None,
                        None,
                        None,
                        None,
                        &[],
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        use_json,
                    )
                }
                Some(cmd) => commands::situation::run(&backend, cmd),
            },
            cli::AnalyticsCommand::Deltas { since, json } => commands::analytics::run(
                &backend,
                "deltas",
                Some(&since),
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Catalysts { window, json } => commands::analytics::run(
                &backend,
                "catalysts",
                Some(&window),
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Impact { json } => commands::analytics::run(
                &backend,
                "impact",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::ImpactEstimate { json } => {
                commands::impact_estimate::run(&backend, json)
            }
            cli::AnalyticsCommand::Calibration { threshold, json } => {
                commands::calibration::run(&backend, threshold, json)
            }
            cli::AnalyticsCommand::DebateScore { command } => match command {
                cli::AnalyticsDebateScoreCommand::Add {
                    debate_id,
                    winner,
                    margin,
                    outcome,
                    assessment,
                    scored_by,
                    json,
                } => commands::debate_score::add(
                    &backend,
                    &commands::debate_score::ScoreParams {
                        debate_id,
                        winner: &winner,
                        margin: &margin,
                        actual_outcome: &outcome,
                        argument_assessment: assessment.as_deref(),
                        scored_by: scored_by.as_deref(),
                        json_output: json,
                    },
                ),
                cli::AnalyticsDebateScoreCommand::List {
                    topic,
                    winner,
                    limit,
                    json,
                } => commands::debate_score::list(
                    &backend,
                    topic.as_deref(),
                    winner.as_deref(),
                    limit,
                    json,
                ),
                cli::AnalyticsDebateScoreCommand::Accuracy { topic, json } => {
                    commands::debate_score::accuracy(&backend, topic.as_deref(), json)
                }
                cli::AnalyticsDebateScoreCommand::Unscored { limit, json } => {
                    commands::debate_score::unscored(&backend, limit, json)
                }
            },
            cli::AnalyticsCommand::Views { command } => match command {
                cli::AnalyticsViewsCommand::Set {
                    analyst,
                    asset,
                    direction,
                    conviction,
                    reasoning,
                    evidence,
                    blind_spots,
                    json,
                } => commands::analyst_views::set(
                    &backend,
                    &analyst,
                    &asset,
                    &direction,
                    conviction,
                    &reasoning,
                    evidence.as_deref(),
                    blind_spots.as_deref(),
                    json,
                ),
                cli::AnalyticsViewsCommand::List {
                    analyst,
                    asset,
                    limit,
                    json,
                } => commands::analyst_views::list(
                    &backend,
                    analyst.as_deref(),
                    asset.as_deref(),
                    limit,
                    json,
                ),
                cli::AnalyticsViewsCommand::Matrix { json } => {
                    commands::analyst_views::matrix(&backend, json)
                }
                cli::AnalyticsViewsCommand::PortfolioMatrix { json } => {
                    commands::analyst_views::portfolio_matrix(&backend, json)
                }
                cli::AnalyticsViewsCommand::History {
                    asset,
                    analyst,
                    limit,
                    json,
                } => commands::analyst_views::history(
                    &backend,
                    &asset,
                    analyst.as_deref(),
                    limit,
                    json,
                ),
                cli::AnalyticsViewsCommand::Divergence {
                    min_spread,
                    asset,
                    limit,
                    json,
                } => commands::analyst_views::divergence(
                    &backend,
                    min_spread,
                    asset.as_deref(),
                    limit,
                    json,
                ),
                cli::AnalyticsViewsCommand::Accuracy {
                    analyst,
                    asset,
                    json,
                } => commands::analyst_views::accuracy(
                    &backend,
                    analyst.as_deref(),
                    asset.as_deref(),
                    json,
                ),
                cli::AnalyticsViewsCommand::Delete {
                    analyst,
                    asset,
                    json,
                } => commands::analyst_views::delete(&backend, &analyst, &asset, json),
            },
            cli::AnalyticsCommand::Opportunities { json } => commands::analytics::run(
                &backend,
                "opportunities",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Narrative { json } => commands::analytics::run(
                &backend,
                "narrative",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Synthesis { json } => commands::analytics::run(
                &backend,
                "synthesis",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Low { json } => commands::analytics::run(
                &backend,
                "low",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Medium { json } => commands::analytics::run(
                &backend,
                "medium",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::High { json } => commands::analytics::run(
                &backend,
                "high",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Macro { command, json } => match command {
                None => commands::analytics::run(
                    &backend,
                    "macro",
                    None,
                    None,
                    None,
                    None,
                    &[],
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                Some(cli::AnalyticsMacroCommand::Metrics { country, json }) => {
                    commands::analytics::run(
                        &backend,
                        "macro",
                        Some("metrics"),
                        country.as_deref(),
                        None,
                        None,
                        &[],
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        json,
                    )
                }
                Some(cli::AnalyticsMacroCommand::Compare { left, right, json }) => {
                    commands::analytics::run(
                        &backend,
                        "macro",
                        Some("compare"),
                        left.as_deref(),
                        right.as_deref(),
                        None,
                        &[],
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        json,
                    )
                }
                Some(cli::AnalyticsMacroCommand::Cycles { command, json }) => match command {
                    None => commands::analytics::run(
                        &backend,
                        "macro",
                        Some("cycles"),
                        None,
                        None,
                        None,
                        &[],
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        json,
                    ),
                    Some(cli::AnalyticsMacroCyclesCommand::Current { country, json }) => {
                        let countries: Vec<String> =
                            country.into_iter().collect();
                        commands::analytics::run(
                            &backend,
                            "macro",
                            Some("cycles"),
                            Some("current"),
                            None,
                            None,
                            &countries,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            false,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            json,
                        )
                    }
                    Some(cli::AnalyticsMacroCyclesCommand::History { command }) => match command {
                        cli::AnalyticsMacroCyclesHistoryCommand::Add {
                            country,
                            determinant,
                            year,
                            score,
                            notes,
                            source,
                            json,
                        } => commands::analytics::run(
                            &backend,
                            "macro",
                            Some("cycles"),
                            Some("history"),
                            Some("add"),
                            None,
                            &[country],
                            Some(&determinant),
                            Some(score),
                            None,
                            None,
                            None,
                            None,
                            None,
                            notes.as_deref(),
                            source.as_deref(),
                            None,
                            None,
                            None,
                            Some(year),
                            false,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            json,
                        ),
                        cli::AnalyticsMacroCyclesHistoryCommand::List {
                            countries,
                            determinant,
                            year,
                            composite,
                            json,
                        } => commands::analytics::run(
                            &backend,
                            "macro",
                            Some("cycles"),
                            Some("history"),
                            Some("list"),
                            None,
                            &countries,
                            determinant.as_deref(),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            year,
                            composite,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            json,
                        ),
                    },
                    Some(cli::AnalyticsMacroCyclesCommand::Update {
                        name,
                        phase,
                        notes,
                        evidence,
                        json,
                    }) => commands::analytics::run(
                        &backend,
                        "macro",
                        Some("cycles"),
                        Some("update"),
                        Some(&name),
                        None,
                        &[],
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some(&phase),
                        evidence.as_deref(),
                        notes.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        json,
                    ),
                },
                Some(cli::AnalyticsMacroCommand::Outcomes { json }) => commands::analytics::run(
                    &backend,
                    "macro",
                    Some("outcomes"),
                    None,
                    None,
                    None,
                    &[],
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                Some(cli::AnalyticsMacroCommand::Parallels { json }) => commands::analytics::run(
                    &backend,
                    "macro",
                    Some("parallels"),
                    None,
                    None,
                    None,
                    &[],
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                Some(cli::AnalyticsMacroCommand::Log { limit, json }) => commands::analytics::run(
                    &backend,
                    "macro",
                    Some("log"),
                    None,
                    None,
                    None,
                    &[],
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    None,
                    None,
                    None,
                    limit,
                    json,
                ),
                Some(cli::AnalyticsMacroCommand::Regime { command }) => match command {
                    cli::AnalyticsMacroRegimeCommand::Current { json } => {
                        commands::regime::run(&backend, "current", None, json)
                    }
                    cli::AnalyticsMacroRegimeCommand::Set {
                        regime,
                        confidence,
                        drivers,
                        json,
                    } => commands::regime::run_set(
                        &backend,
                        &regime,
                        confidence,
                        drivers.as_deref(),
                        json,
                    ),
                    cli::AnalyticsMacroRegimeCommand::History { limit, json } => {
                        commands::regime::run(&backend, "history", limit, json)
                    }
                    cli::AnalyticsMacroRegimeCommand::Transitions { limit, json } => {
                        commands::regime::run(&backend, "transitions", limit, json)
                    }
                },
            },
            cli::AnalyticsCommand::Alignment {
                symbol,
                summary,
                json,
            } => commands::analytics::run(
                &backend,
                if summary {
                    "alignment-summary"
                } else {
                    "alignment"
                },
                None,
                None,
                None,
                symbol.as_deref(),
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Divergence { symbol, json } => commands::analytics::run(
                &backend,
                "divergence",
                None,
                None,
                None,
                symbol.as_deref(),
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::CrossTimeframe {
                symbol,
                threshold,
                limit,
                resolve,
                json,
            } => commands::analytics::run_cross_timeframe(
                &backend,
                symbol.as_deref(),
                threshold,
                limit,
                resolve,
                json,
            ),
            cli::AnalyticsCommand::Digest { from, limit, json } => commands::analytics::run(
                &backend,
                "digest",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                from.as_deref(),
                None,
                limit,
                json,
            ),
            cli::AnalyticsCommand::Recap { date, limit, json } => commands::analytics::run(
                &backend,
                "recap",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                date.as_deref(),
                limit,
                json,
            ),
            cli::AnalyticsCommand::WeeklyReview { days, json } => commands::analytics::run(
                &backend,
                "weekly-review",
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                Some(days),
                json,
            ),
            cli::AnalyticsCommand::Gaps { symbol, json } => commands::analytics::run(
                &backend,
                "gaps",
                None,
                None,
                None,
                symbol.as_deref(),
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                json,
            ),
            cli::AnalyticsCommand::Movers {
                command,
                threshold,
                overnight,
                json,
            } => match command {
                Some(cli::AnalyticsMoversCommand::Themes {
                    threshold: t,
                    min_symbols,
                    json: j,
                }) => commands::movers::run_themes(&backend, &config, &t, min_symbols, j),
                None => {
                    commands::movers::run(&backend, &config, Some(&threshold), overnight, json)
                }
            },
            cli::AnalyticsCommand::Correlations { command, json } => match command {
                None => commands::correlations::run(
                    &backend,
                    Some("compute"),
                    None,
                    None,
                    30,
                    None,
                    false,
                    15,
                    json,
                ),
                Some(cli::AnalyticsCorrelationsCommand::Compute {
                    window,
                    period,
                    store,
                    limit,
                    json,
                }) => commands::correlations::run(
                    &backend,
                    Some("compute"),
                    None,
                    None,
                    window,
                    period.as_deref(),
                    store,
                    limit,
                    json,
                ),
                Some(cli::AnalyticsCorrelationsCommand::History {
                    symbol_a,
                    symbol_b,
                    window,
                    period,
                    limit,
                    json,
                }) => commands::correlations::run(
                    &backend,
                    Some("history"),
                    Some(&symbol_a),
                    Some(&symbol_b),
                    window,
                    period.as_deref(),
                    false,
                    limit,
                    json,
                ),
                Some(cli::AnalyticsCorrelationsCommand::Latest {
                    period,
                    limit,
                    with_impact,
                    json,
                }) => {
                    if with_impact && json {
                        commands::correlations::run_latest_with_impact(
                            &backend,
                            period.as_deref(),
                            limit,
                        )
                    } else {
                        commands::correlations::run(
                            &backend,
                            Some("latest"),
                            None,
                            None,
                            30,
                            period.as_deref(),
                            false,
                            limit,
                            json,
                        )
                    }
                }
                Some(cli::AnalyticsCorrelationsCommand::List {
                    period,
                    limit,
                    with_impact,
                    json,
                }) => {
                    if with_impact && json {
                        commands::correlations::run_latest_with_impact(
                            &backend,
                            period.as_deref(),
                            limit,
                        )
                    } else {
                        commands::correlations::run(
                            &backend,
                            Some("latest"),
                            None,
                            None,
                            30,
                            period.as_deref(),
                            false,
                            limit,
                            json,
                        )
                    }
                }
                Some(cli::AnalyticsCorrelationsCommand::Breaks {
                    threshold,
                    limit,
                    seed_alerts,
                    cooldown,
                    json,
                }) => commands::correlations::run_breaks(
                    &backend,
                    threshold,
                    limit,
                    seed_alerts,
                    cooldown,
                    json,
                ),
            },
            cli::AnalyticsCommand::Scan {
                filter,
                save,
                load,
                list,
                news_keyword,
                trackline_breaches,
                json,
            } => commands::scan::run(
                &backend,
                &config,
                filter.as_deref(),
                save.as_deref(),
                load.as_deref(),
                list,
                news_keyword.as_deref(),
                trackline_breaches,
                json,
            ),
            cli::AnalyticsCommand::Research {
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
            } => {
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
            cli::AnalyticsCommand::Trends { command } => match command {
                cli::AnalyticsTrendsCommand::Add {
                    value,
                    timeframe,
                    direction,
                    conviction,
                    category,
                    description,
                    asset_impact,
                    key_signal,
                    status,
                    json,
                } => commands::trends::run(
                    &backend,
                    "add",
                    value.as_deref(),
                    None,
                    timeframe.as_deref(),
                    direction.as_deref(),
                    conviction.as_deref(),
                    category.as_deref(),
                    description.as_deref(),
                    asset_impact.as_deref(),
                    key_signal.as_deref(),
                    status.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AnalyticsTrendsCommand::List {
                    timeframe,
                    direction,
                    conviction,
                    category,
                    status,
                    limit,
                    json,
                } => commands::trends::run(
                    &backend,
                    "list",
                    None,
                    None,
                    timeframe.as_deref(),
                    direction.as_deref(),
                    conviction.as_deref(),
                    category.as_deref(),
                    None,
                    None,
                    None,
                    status.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    limit,
                    json,
                ),
                cli::AnalyticsTrendsCommand::Update {
                    value,
                    id,
                    timeframe,
                    direction,
                    conviction,
                    category,
                    description,
                    asset_impact,
                    key_signal,
                    status,
                    json,
                } => commands::trends::run(
                    &backend,
                    "update",
                    value.as_deref(),
                    id,
                    timeframe.as_deref(),
                    direction.as_deref(),
                    conviction.as_deref(),
                    category.as_deref(),
                    description.as_deref(),
                    asset_impact.as_deref(),
                    key_signal.as_deref(),
                    status.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AnalyticsTrendsCommand::Dashboard { json } => commands::trends::run(
                    &backend,
                    "dashboard",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AnalyticsTrendsCommand::Evidence { command } => match command {
                    cli::AnalyticsTrendsEvidenceCommand::Add {
                        id,
                        evidence,
                        value,
                        date,
                        direction_impact,
                        source,
                        json,
                    } => {
                        let evidence_text = evidence.or(value);
                        commands::trends::run(
                            &backend,
                            "evidence-add",
                            None,
                            id,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            date.as_deref(),
                            evidence_text.as_deref(),
                            direction_impact.as_deref(),
                            source.as_deref(),
                            None,
                            None,
                            None,
                            None,
                            None,
                            json,
                        )
                    }
                    cli::AnalyticsTrendsEvidenceCommand::List { id, limit, json } => {
                        commands::trends::run(
                            &backend,
                            "evidence-list",
                            None,
                            id,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            limit,
                            json,
                        )
                    }
                },
                cli::AnalyticsTrendsCommand::Impact { command } => match command {
                    cli::AnalyticsTrendsImpactCommand::Add {
                        id,
                        symbol,
                        impact,
                        mechanism,
                        impact_timeframe,
                        date,
                        json,
                    } => commands::trends::run(
                        &backend,
                        "impact-add",
                        None,
                        id,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        date.as_deref(),
                        None,
                        None,
                        None,
                        symbol.as_deref(),
                        impact.as_deref(),
                        mechanism.as_deref(),
                        impact_timeframe.as_deref(),
                        None,
                        json,
                    ),
                    cli::AnalyticsTrendsImpactCommand::List {
                        id,
                        symbol,
                        limit,
                        json,
                    } => commands::trends::run(
                        &backend,
                        "impact-list",
                        None,
                        id,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        symbol.as_deref(),
                        None,
                        None,
                        None,
                        limit,
                        json,
                    ),
                },
            },
            cli::AnalyticsCommand::Alerts { command } => {
                // Handle triage separately — it has its own function signature
                if let cli::AnalyticsAlertsCommand::Triage { json } = command {
                    return commands::alerts::run_triage(&backend, json);
                }
                let (action, args) = match command {
                    cli::AnalyticsAlertsCommand::Add {
                        rule,
                        kind,
                        symbol,
                        from_level,
                        condition,
                        label,
                        recurring,
                        cooldown_minutes,
                    } => (
                        "add",
                        commands::alerts::AlertsArgs {
                            rule,
                            id: None,
                            ids: vec![],
                            json: false,
                            status_filter: None,
                            today: false,
                            kind,
                            symbol,
                            from_level,
                            condition,
                            label,
                            triggered: false,
                            since_hours: None,
                            recurring,
                            cooldown_minutes,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::List {
                        status,
                        triggered,
                        since,
                        today,
                        recent,
                        recent_hours,
                        json,
                    } => (
                        "list",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json,
                            status_filter: status,
                            today,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered,
                            since_hours: since,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent,
                            recent_hours,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::Remove { id } => (
                        "remove",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: Some(id),
                            ids: vec![],
                            json: false,
                            status_filter: None,
                            today: false,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::Check { today, json } => (
                        "check",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json,
                            status_filter: None,
                            today,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::Ack { ids } => (
                        "ack",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids,
                            json: false,
                            status_filter: None,
                            today: false,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::Rearm { id } => (
                        "rearm",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: Some(id),
                            ids: vec![],
                            json: false,
                            status_filter: None,
                            today: false,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::SeedDefaults => (
                        "seed-defaults",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json: false,
                            status_filter: None,
                            today: false,
                            kind: None,
                            symbol: None,
                            from_level: None,
                            condition: None,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                        },
                    ),
                    // Triage is handled above via early return
                    cli::AnalyticsAlertsCommand::Triage { .. } => unreachable!(),
                };
                commands::alerts::run(&backend, action, &args)
            }
            cli::AnalyticsCommand::Scenario { command } => match command {
                cli::AnalyticsScenarioCommand::Add {
                    value,
                    probability,
                    description,
                    impact,
                    triggers,
                    precedent,
                    status,
                    json,
                } => commands::scenario::run(
                    &backend,
                    "add",
                    Some(&value),
                    None,
                    None,
                    probability,
                    description.as_deref(),
                    impact.as_deref(),
                    triggers.as_deref(),
                    precedent.as_deref(),
                    status.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AnalyticsScenarioCommand::List {
                    status,
                    limit,
                    json,
                } => commands::scenario::run(
                    &backend,
                    "list",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    status.as_deref(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    limit,
                    json,
                ),
                cli::AnalyticsScenarioCommand::Update {
                    value,
                    note_pos,
                    probability,
                    description,
                    impact,
                    triggers,
                    precedent,
                    status,
                    driver,
                    notes,
                    json,
                } => {
                    let merged_notes = driver.or(notes).or(note_pos);
                    commands::scenario::run(
                        &backend,
                        "update",
                        Some(&value),
                        None,
                        None,
                        probability,
                        description.as_deref(),
                        impact.as_deref(),
                        triggers.as_deref(),
                        precedent.as_deref(),
                        status.as_deref(),
                        merged_notes.as_deref(),
                        merged_notes.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        json,
                    )
                }
                cli::AnalyticsScenarioCommand::Remove { value, json } => commands::scenario::run(
                    &backend,
                    "remove",
                    Some(&value),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    json,
                ),
                cli::AnalyticsScenarioCommand::History { value, limit, json } => {
                    commands::scenario::run(
                        &backend,
                        "history",
                        Some(&value),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        limit,
                        json,
                    )
                }
                cli::AnalyticsScenarioCommand::Signal { command } => match command {
                    cli::AnalyticsScenarioSignalCommand::Add {
                        value,
                        scenario,
                        source,
                        status,
                        json,
                    } => commands::scenario::run(
                        &backend,
                        "signal-add",
                        Some(&value),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        status.as_deref(),
                        None,
                        None,
                        None,
                        source.as_deref(),
                        scenario.as_deref(),
                        None,
                        json,
                    ),
                    cli::AnalyticsScenarioSignalCommand::List {
                        scenario,
                        status,
                        limit,
                        json,
                    } => commands::scenario::run(
                        &backend,
                        "signal-list",
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        status.as_deref(),
                        None,
                        None,
                        None,
                        None,
                        scenario.as_deref(),
                        limit,
                        json,
                    ),
                    cli::AnalyticsScenarioSignalCommand::Update {
                        signal_id,
                        evidence,
                        status,
                        json,
                    } => commands::scenario::run(
                        &backend,
                        "signal-update",
                        None,
                        None,
                        Some(signal_id),
                        None,
                        None,
                        None,
                        None,
                        None,
                        status.as_deref(),
                        None,
                        None,
                        evidence.as_deref(),
                        None,
                        None,
                        None,
                        json,
                    ),
                    cli::AnalyticsScenarioSignalCommand::Remove { signal_id, json } => {
                        commands::scenario::run(
                            &backend,
                            "signal-remove",
                            None,
                            None,
                            Some(signal_id),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            json,
                        )
                    }
                },
                cli::AnalyticsScenarioCommand::Suggest { json } => {
                    commands::scenario_suggest::run(&backend, json)
                }
                cli::AnalyticsScenarioCommand::ImpactMatrix { json } => {
                    commands::impact_matrix::run(&backend, &config, json)
                }
            },
            cli::AnalyticsCommand::Conviction { command } => match command {
                cli::AnalyticsConvictionCommand::Set {
                    symbol,
                    score_pos,
                    score,
                    notes,
                    notes_pos,
                    json,
                } => {
                    let score_val = score.or(score_pos).ok_or_else(|| {
                        anyhow::anyhow!(
                            "Missing score. Usage: pftui analytics conviction set SYMBOL <SCORE> [NOTES] or --score N [--notes ...]"
                        )
                    })?;
                    let merged_notes = notes.or(notes_pos);
                    commands::conviction::run_set(
                        &backend,
                        &symbol,
                        score_val,
                        merged_notes.as_deref(),
                        json,
                    )
                }
                cli::AnalyticsConvictionCommand::List { json } => {
                    commands::conviction::run_list(&backend, json)
                }
                cli::AnalyticsConvictionCommand::History {
                    symbol,
                    limit,
                    json,
                } => commands::conviction::run_history(&backend, &symbol, limit, json),
                cli::AnalyticsConvictionCommand::Changes { days, json } => {
                    let d = days
                        .as_deref()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(7);
                    commands::conviction::run_changes(&backend, d, json)
                }
            },
            cli::AnalyticsCommand::Predictions {
                command,
                category,
                search,
                limit,
                json,
            } => dispatch_predictions(&backend, command, category, search, limit, json),
            cli::AnalyticsCommand::PowerFlow { command } => match command {
                cli::AnalyticsPowerFlowCommand::Add {
                    event,
                    source,
                    direction,
                    target,
                    evidence,
                    magnitude,
                    agent_source,
                    date,
                    json,
                } => commands::power_flow::run_add(
                    &backend,
                    &event,
                    &source,
                    &direction,
                    target.as_deref(),
                    &evidence,
                    magnitude,
                    agent_source.as_deref(),
                    date.as_deref(),
                    json,
                ),
                cli::AnalyticsPowerFlowCommand::List {
                    complex,
                    direction,
                    days,
                    json,
                } => commands::power_flow::run_list(
                    &backend,
                    complex.as_deref(),
                    direction.as_deref(),
                    days,
                    json,
                ),
                cli::AnalyticsPowerFlowCommand::Balance { days, json } => {
                    commands::power_flow::run_balance(&backend, days, json)
                }
                cli::AnalyticsPowerFlowCommand::Assess {
                    days,
                    complex,
                    json,
                } => commands::power_flow::run_assess(
                    &backend,
                    days,
                    complex.as_deref(),
                    json,
                ),
            },
            cli::AnalyticsCommand::NewsSentiment {
                category,
                hours,
                limit,
                detail,
                json,
            } => commands::news_sentiment::run(
                &backend,
                category.as_deref(),
                hours,
                limit,
                detail,
                json,
            ),
            cli::AnalyticsCommand::MorningBrief { json } => {
                commands::morning_brief::run(&backend, json)
            }
            cli::AnalyticsCommand::EveningBrief { json } => {
                commands::evening_brief::run(&backend, json)
            }
            cli::AnalyticsCommand::RegimeFlows { json } => {
                commands::regime_flows::run(&backend, json)
            }
            cli::AnalyticsCommand::RegimeTransitions { json } => {
                commands::regime_transitions::run(&backend, json)
            }
            cli::AnalyticsCommand::Backtest { command } => match command {
                cli::AnalyticsBacktestCommand::Predictions {
                    symbol,
                    agent,
                    timeframe,
                    conviction,
                    limit,
                    json,
                } => commands::backtest::run_predictions(
                    &backend,
                    symbol.as_deref(),
                    agent.as_deref(),
                    timeframe.as_deref(),
                    conviction.as_deref(),
                    limit,
                    json,
                ),
            },
        },
    };

    match (result, backend.flush()) {
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
        (Ok(v), Ok(_)) => Ok(v),
    }
}
