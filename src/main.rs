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
mod report;
mod research;
mod text_util;
mod tui;
mod web;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Command};
use crate::config::{load_config_with_first_run_prompt, DatabaseBackend};
use crate::db::backend::open_from_config;
use crate::db::default_db_path;

/// Parse a `--since` duration string into a positive day count.
///
/// Accepts `Nh` (hours, rounded up to one day minimum), `Nd` (days), `Nw`
/// (weeks), or `Nm` (months ≈ 30 days). Used by the analytics rebuild
/// commands which operate at daily resolution.
fn parse_since_to_days(value: &str) -> Result<i64> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--since must not be empty");
    }
    let last = value
        .chars()
        .last()
        .ok_or_else(|| anyhow::anyhow!("--since must not be empty"))?;
    let stripped = &value[..value.len() - last.len_utf8()];
    let amount: i64 = stripped
        .parse()
        .map_err(|err| anyhow::anyhow!("invalid --since amount '{}': {}", stripped, err))?;
    if amount <= 0 {
        bail!("--since must be a positive duration");
    }
    let days = match last {
        'h' | 'H' => std::cmp::max(1, amount / 24),
        'd' | 'D' => amount,
        'w' | 'W' => amount * 7,
        'm' | 'M' => amount * 30,
        other => bail!(
            "could not parse --since '{}': expected Nh/Nd/Nw/Nm (got suffix '{}')",
            value,
            other
        ),
    };
    Ok(days)
}

/// Prepend the market snapshot line (`pftui data snapshot-line`) to journal
/// content when `--stamp` is passed. Best-effort: a missing line (empty price
/// history, non-SQLite backend) warns to stderr and writes unstamped — a
/// journal write must never fail because the stamp could not be built.
fn apply_stamp(
    backend: &crate::db::backend::BackendConnection,
    content: String,
    stamp: bool,
) -> String {
    if !stamp {
        return content;
    }
    match commands::snapshot_line::stamp_prefix(backend) {
        Some(line) => format!("{line}\n{content}"),
        None => {
            eprintln!("warning: --stamp requested but no snapshot line available (no cached closes); writing unstamped");
            content
        }
    }
}

fn run_agent_journal(
    backend: &crate::db::backend::BackendConnection,
    command: Option<cli::JournalCommand>,
) -> Result<()> {
    match command {
        None => commands::journal::run_list(backend, Some(20), None, None, None, None, None, false),
        Some(cli::JournalCommand::Entry { command }) => match command {
            cli::JournalEntryCommand::Add {
                value,
                content,
                date,
                tag,
                tags,
                symbol,
                conviction,
                author,
                stamp,
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
                let resolved = apply_stamp(backend, resolved, stamp);
                let normalized_tags = commands::journal::normalize_tags(&tag, tags.as_deref());
                commands::journal::run_add(
                    backend,
                    &resolved,
                    date.as_deref(),
                    normalized_tags.as_deref(),
                    symbol.as_deref(),
                    conviction.as_deref(),
                    author.as_deref(),
                    json,
                )
            }
            cli::JournalEntryCommand::List {
                limit,
                since,
                tag,
                symbol,
                filter_status,
                author,
                json,
            } => commands::journal::run_list(
                backend,
                limit,
                since.as_deref(),
                tag.as_deref(),
                symbol.as_deref(),
                filter_status.as_deref(),
                author.as_deref(),
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
                topic,
                source_article_id,
                target_date,
                resolution_criteria,
                lessons,
                override_cap,
                skip_preflight,
                accept_preflight,
                inline,
                preflight_threshold,
                layer,
                with_adversary,
                falsify,
                override_confidence_cap,
                cap_rationale,
                json,
            } => {
                let text = claim.or(value).ok_or_else(|| {
                    anyhow::anyhow!(
                        "No prediction text provided. Use --claim \"your prediction\" or pass it as the first positional argument.\n\
                         Examples:\n  pftui journal prediction add --claim \"BTC above 70k\" --timeframe low\n  \
                         pftui journal prediction add \"BTC above 70k\" --timeframe low"
                    )
                })?;
                let effective_timeframe = timeframe.clone().or_else(|| timeframe_pos.clone());
                let effective_layer = layer.clone().or_else(|| effective_timeframe.clone());
                commands::predict::run_add_with_preflight(
                    backend,
                    &text,
                    symbol.as_deref(),
                    conviction.as_deref(),
                    effective_timeframe.as_deref(),
                    confidence.or(confidence_pos),
                    source_agent.as_deref(),
                    target_date.as_deref(),
                    resolution_criteria.as_deref(),
                    lessons.as_deref(),
                    topic.as_deref(),
                    source_article_id,
                    override_cap,
                    effective_layer.as_deref(),
                    skip_preflight,
                    accept_preflight,
                    inline,
                    preflight_threshold,
                    with_adversary,
                    falsify.as_deref(),
                    override_confidence_cap,
                    cap_rationale.as_deref(),
                    json,
                )
            }
            cli::JournalPredictionCommand::Adversary {
                claim,
                symbol,
                timeframe,
                conviction,
                layer,
                json,
            } => {
                let effective_layer = layer.clone().or_else(|| timeframe.clone());
                commands::predict::run_adversary(
                    backend,
                    &claim,
                    symbol.as_deref(),
                    timeframe.as_deref(),
                    conviction.as_deref(),
                    effective_layer.as_deref(),
                    json,
                )
            }
            cli::JournalPredictionCommand::Preflight {
                claim,
                symbol,
                timeframe,
                conviction,
                layer,
                topic,
                inline,
                json,
            } => {
                let effective_layer = layer.clone().or_else(|| timeframe.clone());
                commands::predict::run_preflight(
                    backend,
                    &claim,
                    symbol.as_deref(),
                    timeframe.as_deref(),
                    conviction.as_deref(),
                    effective_layer.as_deref(),
                    topic.as_deref(),
                    inline,
                    json,
                )
            }
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
                None,
                filter.as_deref(),
                None,
                limit,
                false,
                None,
                None,
                false,
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
                    None,
                    merged_outcome.as_deref(),
                    merged_notes.as_deref(),
                    lesson.as_deref(),
                    None,
                    None,
                    None,
                    false,
                    None,
                    None,
                    false,
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
                None,                 // value
                None,                 // id
                None,                 // symbol
                None,                 // conviction
                timeframe.as_deref(), // timeframe
                None,                 // confidence
                agent.as_deref(),     // source_agent
                None,                 // target_date
                None,                 // resolution_criteria
                None,                 // lessons_applied
                None,                 // outcome
                None,                 // notes
                None,                 // lesson
                None,                 // filter
                None,                 // date
                None,                 // limit
                false,                // lesson_coverage
                None,                 // topic
                None,                 // source_article_id
                false,                // override_cap
                json,
            ),
            cli::JournalPredictionCommand::Scorecard {
                date,
                limit,
                lesson_coverage,
                json,
            } => commands::predict::run(
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
                None,
                date.as_deref(),
                limit,
                lesson_coverage,
                None,
                None,
                false,
                json,
            ),
            cli::JournalPredictionCommand::AutoScore {
                since,
                dry_run,
                confidence_floor,
                force,
                json,
            } => {
                let floor = match confidence_floor {
                    cli::PredictionConfidenceFloorArg::Medium => "medium",
                    cli::PredictionConfidenceFloorArg::High => "high",
                };
                commands::predict::run_auto_score(
                    backend,
                    since.as_deref(),
                    dry_run,
                    floor,
                    force,
                    json,
                )
            }
            cli::JournalPredictionCommand::RescoreAudit {
                apply_high_confidence,
                json,
            } => commands::rescore_audit::run_rescore_audit(backend, apply_high_confidence, json),
            cli::JournalPredictionCommand::Lessons {
                command,
                miss_type,
                unresolved,
                limit,
                include_retired,
                json,
            } => match command {
                None => commands::predict::run_lessons(
                    backend,
                    miss_type.as_deref(),
                    unresolved,
                    limit,
                    include_retired,
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
                Some(cli::JournalPredictionLessonsCommand::Bulk {
                    input,
                    auto_stub,
                    unresolved: unresolved_only,
                    dry_run,
                    json: json_flag,
                }) => commands::predict::run_bulk_lessons(
                    backend,
                    input.as_deref(),
                    auto_stub,
                    unresolved_only,
                    dry_run,
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
                author,
                stamp,
                json,
            } => commands::notes::run(
                backend,
                "add",
                Some(&apply_stamp(backend, value, stamp)),
                None,
                date.as_deref(),
                section.as_deref(),
                None,
                None,
                author.as_deref(),
                json,
            ),
            cli::JournalNotesCommand::List {
                since,
                limit,
                author,
                json,
            } => commands::notes::run(
                backend,
                "list",
                None,
                None,
                None,
                None,
                since.as_deref(),
                limit,
                author.as_deref(),
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
                None,
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
                None,
                json,
            ),
            cli::JournalNotesCommand::Repetition { author, days, json } => {
                commands::notes::run_repetition(backend, author.as_deref(), days, json)
            }
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
                id,
                note_pos,
                probability,
                description,
                impact,
                triggers,
                precedent: _,
                status,
                driver,
                notes,
                evidence,
                proposer,
                hard_print,
                override_conflict,
                json,
            } => {
                let merged_notes = driver.or(notes).or(note_pos);
                commands::scenario::update(
                    backend,
                    value.as_deref(),
                    id,
                    probability,
                    description.as_deref(),
                    impact.as_deref(),
                    triggers.as_deref(),
                    status.as_deref(),
                    merged_notes.as_deref(),
                    commands::scenario::UpdateGuardOpts {
                        proposer: proposer.as_deref(),
                        evidence: evidence.as_deref(),
                        hard_print: hard_print.as_deref(),
                        override_conflict,
                    },
                    json,
                )
            }
            cli::JournalScenarioCommand::SetBaseRate {
                value,
                rate,
                reference,
                json,
            } => commands::scenario::set_base_rate(backend, &value, rate, &reference, json),
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
            cli::JournalScenarioCommand::Timeline { days, json } => commands::scenario::run(
                backend,
                "timeline",
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
                days.map(|d| d as usize),
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
        Some(cli::JournalCommand::Replies { command }) => match command {
            cli::JournalRepliesCommand::List {
                report_date,
                asset,
                decision_type,
                json,
            } => commands::analytics_enrichment::replies_list(
                backend,
                report_date.as_deref(),
                asset.as_deref(),
                decision_type.as_deref(),
                json,
            ),
            cli::JournalRepliesCommand::Add {
                report_date,
                reply_date,
                asset,
                decision_type,
                response_class,
                conviction_implied,
                horizon,
                reasoning,
                raw_content,
                journal_id,
                json,
            } => commands::analytics_enrichment::replies_add(
                backend,
                &report_date,
                reply_date.as_deref(),
                asset.as_deref(),
                &decision_type,
                &response_class,
                conviction_implied.as_deref(),
                horizon.as_deref(),
                reasoning.as_deref(),
                &raw_content,
                journal_id,
                json,
            ),
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
    geo: bool,
    limit: usize,
    json: bool,
) -> Result<()> {
    match command {
        None | Some(cli::DataPredictionsCommand::Markets { .. }) => {
            // Merge top-level flags with subcommand flags (subcommand wins if present)
            let (cat, srch, geo, lim, js) = match command {
                Some(cli::DataPredictionsCommand::Markets {
                    category: sc,
                    search: ss,
                    geo: sg,
                    limit: sl,
                    json: sj,
                }) => (
                    sc.or(category),
                    ss.or(search),
                    sg || geo,
                    sl, // subcommand limit takes precedence (has its own default)
                    sj || json,
                ),
                _ => (category, search, geo, limit, json),
            };
            commands::predictions::run(backend, cat.as_deref(), srch.as_deref(), geo, lim, js)
        }
        Some(cli::DataPredictionsCommand::Stats {
            timeframe,
            agent,
            json: j,
        }) => {
            commands::predict::run(
                backend,
                "stats",
                None,                 // value
                None,                 // id
                None,                 // symbol
                None,                 // conviction
                timeframe.as_deref(), // timeframe
                None,                 // confidence
                agent.as_deref(),     // source_agent
                None,                 // target_date
                None,                 // resolution_criteria
                None,                 // lessons_applied
                None,                 // outcome
                None,                 // notes
                None,                 // lesson
                None,                 // filter
                None,                 // date
                None,                 // limit
                false,                // lesson_coverage
                None,                 // topic
                None,                 // source_article_id
                false,                // override_cap
                j || json,
            )
        }
        Some(cli::DataPredictionsCommand::Scorecard {
            date,
            limit: lim,
            lesson_coverage,
            json: j,
        }) => commands::predict::run(
            backend,
            "scorecard",
            None,            // value
            None,            // id
            None,            // symbol
            None,            // conviction
            None,            // timeframe
            None,            // confidence
            None,            // source_agent
            None,            // target_date
            None,            // resolution_criteria
            None,            // lessons_applied
            None,            // outcome
            None,            // notes
            None,            // lesson
            None,            // filter
            date.as_deref(), // date
            lim,             // limit
            lesson_coverage,
            None,  // topic
            None,  // source_article_id
            false, // override_cap
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
            None,                 // value
            None,                 // id
            symbol.as_deref(),    // symbol
            None,                 // conviction
            timeframe.as_deref(), // timeframe
            None,                 // confidence
            None,                 // source_agent
            None,                 // target_date
            None,                 // resolution_criteria
            None,                 // lessons_applied
            None,                 // outcome
            None,                 // notes
            None,                 // lesson
            Some("pending"),      // filter = pending
            None,                 // date
            lim,                  // limit
            false,                // lesson_coverage
            None,                 // topic
            None,                 // source_article_id
            false,                // override_cap
            j || json,
        ),
        Some(cli::DataPredictionsCommand::Map {
            scenario,
            search,
            contract,
            list,
            auto_suggest,
            json: j,
        }) => commands::predictions_map::run_map(
            backend,
            scenario.as_deref(),
            search.as_deref(),
            contract.as_deref(),
            list,
            auto_suggest,
            j || json,
        ),
        Some(cli::DataPredictionsCommand::SuggestMappings {
            scenario,
            limit: lim,
            json: j,
        }) => commands::predictions_map::run_suggest_mappings(
            backend,
            scenario.as_deref(),
            lim,
            j || json,
        ),
        Some(cli::DataPredictionsCommand::Unmap {
            scenario,
            contract,
            json: j,
        }) => {
            commands::predictions_map::run_unmap(backend, &scenario, contract.as_deref(), j || json)
        }
        Some(cli::DataPredictionsCommand::Add {
            claim,
            symbol,
            conviction,
            timeframe,
            confidence,
            source_agent,
            topic,
            source_article_id,
            target_date,
            resolution_criteria,
            lessons,
            override_cap,
            skip_preflight,
            accept_preflight,
            inline,
            preflight_threshold,
            layer,
            with_adversary,
            falsify,
            override_confidence_cap,
            cap_rationale,
            json: j,
        }) => {
            // Route through the same disciplined path as `journal prediction
            // add`: falsification-rule parsing, the 0.3 unfalsifiable
            // confidence cap, the calibration-derived clamp (with
            // --override-confidence-cap/--cap-rationale), and auto-preflight.
            let effective_layer = layer.clone().or_else(|| timeframe.clone());
            commands::predict::run_add_with_preflight(
                backend,
                &claim,
                symbol.as_deref(),
                conviction.as_deref(),
                timeframe.as_deref(),
                confidence,
                source_agent.as_deref(),
                target_date.as_deref(),
                resolution_criteria.as_deref(),
                lessons.as_deref(),
                topic.as_deref(),
                source_article_id,
                override_cap,
                effective_layer.as_deref(),
                skip_preflight,
                accept_preflight,
                inline,
                preflight_threshold,
                with_adversary,
                falsify.as_deref(),
                override_confidence_cap,
                cap_rationale.as_deref(),
                j || json,
            )
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let timing = cli.timing;
    let start = if timing {
        Some(std::time::Instant::now())
    } else {
        None
    };

    let result = run_cli(cli);

    if let Some(start) = start {
        let elapsed = start.elapsed();
        eprintln!("[timing] elapsed_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
    }

    result
}

fn run_cli(cli: Cli) -> Result<()> {
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

    // JSON-sourced report charts do not need the portfolio database.
    if let Some(Command::Report {
        command:
            cli::ReportCommand::Chart {
                chart_name,
                from_db,
                from_json,
                out,
                format,
                json,
            },
    }) = &cli.command
    {
        if from_db.is_some() && from_json.is_some() {
            bail!("use either --from-db or --from-json, not both");
        }
        if from_json.is_some() || from_db.is_none() {
            return commands::report::run_chart_without_db(commands::report::ReportChartOptions {
                chart_name,
                from_db: from_db.as_deref(),
                from_json: from_json.as_deref(),
                out: out.as_deref(),
                format: *format,
                json_output: *json,
            });
        }
    }

    let config = load_config_with_first_run_prompt()?;
    let db_path = default_db_path();

    if let Some(Command::System {
        command: cli::SystemCommand::Mirror { ref command },
    }) = cli.command
    {
        return commands::mirror::run(&config, &db_path, command);
    }

    // archive-db runs BEFORE backend init: a backup tool must never mutate
    // the database first (startup migrations would otherwise run, and the
    // R3 cull migration drops tables — the backup must capture pre-drop
    // state when invoked ahead of an upgrade).
    if let Some(Command::System {
        command:
            cli::SystemCommand::ArchiveDb {
                ref out,
                ref table,
                json,
            },
    }) = cli.command
    {
        if config.database_backend != DatabaseBackend::Sqlite {
            bail!("`pftui system archive-db` backs up the local SQLite database; use pg_dump for the postgres backend");
        }
        let conn = rusqlite::Connection::open(&db_path)
            .with_context(|| format!("opening {} for backup", db_path.display()))?;
        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        return commands::archive_db::run(&backend, out.as_deref(), table.as_deref(), json);
    }

    if let Some(Command::System {
        command: cli::SystemCommand::Schema { ref command },
    }) = cli.command
    {
        if config.database_backend != DatabaseBackend::Sqlite {
            bail!("`pftui system schema` currently supports the SQLite backend only");
        }
        return match command {
            cli::SchemaCommand::Verify { json } => {
                commands::schema::run_verify_sqlite(&db_path, *json)
            }
            cli::SchemaCommand::Repair {
                dry_run,
                confirm,
                json,
            } => commands::schema::run_repair_sqlite(&db_path, *dry_run, *confirm, *json),
        };
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

        Some(Command::Prediction { command }) => {
            run_agent_journal(&backend, Some(cli::JournalCommand::Prediction { command }))
        }

        Some(Command::Data { command }) => match command {
            cli::DataCommand::Refresh {
                notify,
                json,
                timeout,
                only,
                skip,
                stale,
                accept_outlier,
            } => {
                if cached_only {
                    if json {
                        println!("{{\"error\": \"cached-only mode enabled\"}}");
                    } else {
                        println!("Cached-only mode enabled; skipping refresh network calls.");
                    }
                    Ok(())
                } else {
                    let plan = if stale {
                        let stale_sources =
                            commands::status::stale_refresh_sources_backend(&backend)?;
                        if stale_sources.is_empty() {
                            if json {
                                println!(
                                    "{{\"refreshed_sources\":[],\"message\":\"No stale or empty status-tracked feeds detected.\"}}"
                                );
                            } else {
                                println!("No stale or empty status-tracked feeds detected.");
                            }
                            return Ok(());
                        }
                        commands::refresh::RefreshPlan::from_only(&stale_sources)?
                    } else if !only.is_empty() {
                        commands::refresh::RefreshPlan::from_only(&only)?
                    } else if !skip.is_empty() {
                        commands::refresh::RefreshPlan::from_skip(&skip)?
                    } else {
                        commands::refresh::RefreshPlan::full()
                    };
                    let plan = plan.with_accept_outliers(accept_outlier);
                    if json {
                        commands::refresh::run_json_with_plan(
                            &backend,
                            &config,
                            notify,
                            &plan,
                            timeout,
                        )
                    } else {
                        commands::refresh::run_with_plan(
                            &backend,
                            &config,
                            notify,
                            &plan,
                            timeout,
                        )
                    }
                }
            }
            cli::DataCommand::Status { json, .. } => commands::status::run_backend(&backend, json),
            cli::DataCommand::SnapshotLine { json } => {
                commands::snapshot_line::run(&backend, json)
            }
            cli::DataCommand::Series { command } => match command {
                cli::DataSeriesCommand::Status { json } => {
                    commands::series_status::run(&backend, json)
                }
            },
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
                command,
                source,
                search,
                hours,
                breaking,
                filter_independence,
                limit,
                with_sentiment,
                json,
            } => match command {
                Some(cli::DataNewsCommand::Feeds { command }) => match command {
                    cli::DataNewsFeedsCommand::List { json } => {
                        commands::news::run_feeds_list(&backend, &config, json)
                    }
                    cli::DataNewsFeedsCommand::Reset { feed_id, json } => {
                        commands::news::run_feeds_reset(&backend, &feed_id, json)
                    }
                },
                Some(cli::DataNewsCommand::Sources { command }) => match command {
                    cli::DataNewsSourcesCommand::List { json } => {
                        commands::news::run_sources_list(&backend, json)
                    }
                    cli::DataNewsSourcesCommand::Unclassified {
                        since,
                        min_articles,
                        json,
                    } => commands::news::run_sources_unclassified(
                        &backend,
                        &since,
                        min_articles,
                        json,
                    ),
                    cli::DataNewsSourcesCommand::Stats { since, json } => {
                        commands::news::run_sources_stats(&backend, &since, json)
                    }
                    cli::DataNewsSourcesCommand::Set {
                        domain,
                        tier,
                        notes,
                        json,
                    } => commands::news::run_sources_set(
                        &backend,
                        &domain,
                        tier,
                        notes.as_deref(),
                        json,
                    ),
                    cli::DataNewsSourcesCommand::Remove { domain, json } => {
                        commands::news::run_sources_remove(&backend, &domain, json)
                    }
                },
                Some(cli::DataNewsCommand::Topics { command }) => match command {
                    cli::DataNewsTopicsCommand::List { json } => {
                        commands::news::run_topics_list(&backend, json)
                    }
                    cli::DataNewsTopicsCommand::Set {
                        topic,
                        primary_market_id,
                        secondary_market_id,
                        notes,
                        json,
                    } => commands::news::run_topics_set(
                        &backend,
                        &topic,
                        &primary_market_id,
                        secondary_market_id.as_deref(),
                        notes.as_deref(),
                        json,
                    ),
                    cli::DataNewsTopicsCommand::Remove { topic, json } => {
                        commands::news::run_topics_remove(&backend, &topic, json)
                    }
                },
                None => commands::news::run(
                    &backend,
                    &config,
                    source.as_deref(),
                    search.as_deref(),
                    hours,
                    breaking,
                    filter_independence.as_deref(),
                    limit,
                    with_sentiment,
                    json,
                ),
            },
            cli::DataCommand::Sentiment {
                symbol,
                history,
                json,
            } => commands::sentiment::run(&backend, symbol.as_deref(), history, json),
            cli::DataCommand::FearGreed { history, json } => {
                commands::fear_greed::run(&backend, history, json)
            }
            cli::DataCommand::Calendar {
                command,
                days,
                impact,
                event_type,
                json,
            } => commands::calendar::dispatch(
                &backend, command, days, impact, event_type, json,
            ),
            cli::DataCommand::Cot {
                symbol,
                force_refresh,
                json,
            } => {
                commands::cot::run(&backend, symbol.as_deref(), force_refresh, json)
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
                geo,
                limit,
                json,
            } => dispatch_predictions(&backend, command, category, search, geo, limit, json),
            cli::DataCommand::Options { command } => match command {
                cli::DataOptionsCommand::Refresh {
                    symbol,
                    all,
                    json,
                } => commands::options::run_refresh(&backend, symbol.as_deref(), all, json),
                cli::DataOptionsCommand::Show {
                    symbol,
                    limit,
                    json,
                } => commands::options::run_show(&backend, &symbol, limit, json),
                cli::DataOptionsCommand::View {
                    symbol,
                    expiry,
                    limit,
                    json,
                } => commands::options::run_view(&symbol, expiry.as_deref(), limit, json),
            },
            cli::DataCommand::EtfFlows { days, fund, json } => {
                commands::etf_flows::run(days, fund, json)
            }
            cli::DataCommand::Supply { symbol, json } => {
                commands::supply::run(&backend, symbol, json)
            }
            cli::DataCommand::Sovereign { json } => commands::sovereign::run(&backend, json),
            cli::DataCommand::Prices {
                command,
                market,
                json,
                auto_refresh,
            } => match command {
                Some(cli::DataPricesCommand::Audit { symbol, json }) => {
                    commands::prices::run_audit(&backend, symbol.as_deref(), json)
                }
                None => commands::prices::run(&backend, &config, market, json, auto_refresh),
            },
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
                    Some(cli::DataAlertsRedirect::Check { today, newly_triggered, kind, condition, symbol, status, urgency, json }) => (
                        "check",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json,
                            status_filter: status,
                            today,
                            kind,
                            symbol,
                            from_level: None,
                            condition,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                            newly_triggered_only: newly_triggered,
                            urgency_filter: urgency,
                            all_triggered: false,
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
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered: false,
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
            cli::DataCommand::RealYields { command } => match command {
                cli::DataRealYieldsCommand::Refresh { days, json } => {
                    commands::real_yields::refresh(&backend, &config, days, json)
                }
                cli::DataRealYieldsCommand::Show {
                    series,
                    since,
                    json,
                } => commands::real_yields::show(
                    &backend,
                    series.as_deref(),
                    Some(since.as_str()),
                    json,
                ),
            },
            cli::DataCommand::Audit { table, json } => {
                commands::data_audit::run(&backend, table.as_deref(), json)
            }
            cli::DataCommand::Decontaminate {
                symbol,
                before,
                dry_run: _,
                confirm,
                json,
            } => commands::decontaminate::run(&backend, &symbol, before.as_deref(), confirm, json),
            cli::DataCommand::Flows { command } => match command {
                cli::DataFlowsCommand::Refresh { asset, json } => {
                    commands::flows::refresh(&backend, asset, json)
                }
                cli::DataFlowsCommand::Show {
                    asset,
                    since,
                    json,
                } => commands::flows::show(&backend, asset, since, json),
            },
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
            cli::SystemCommand::ArchiveDb { .. } => {
                unreachable!("archive-db is handled before backend initialization")
            }
            cli::SystemCommand::Schema { .. } => {
                unreachable!("schema commands are handled before backend initialization")
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
                view,
                subtab,
                demo,
            } => commands::snapshot::run(
                &config,
                Some(width),
                Some(height),
                plain,
                view.as_deref(),
                subtab,
                demo,
            ),
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
            cli::SystemCommand::DataCoverage { json } => {
                commands::data_coverage::run(&backend, json)
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
            Some(cli::PortfolioCommand::Status { json }) => {
                commands::portfolio_status::run(&backend, &config, json)
            }
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
                    floor,
                    ceiling,
                    target,
                    band,
                } => {
                    let sym = symbol
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("--symbol required for 'set'"))?;
                    commands::target::run(
                        &backend,
                        sym,
                        floor.as_deref(),
                        ceiling.as_deref(),
                        target.as_deref(),
                        band.as_deref(),
                    )
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
            Some(cli::PortfolioCommand::Drawdown { json }) => {
                commands::drawdown::run(&backend, &config, json)
            }
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
            Some(cli::PortfolioCommand::SetCash {
                symbol,
                amount,
                confirm,
                dry_run,
                json,
            }) => commands::set_cash::run(
                &backend,
                &symbol,
                &amount,
                commands::set_cash::SetCashOptions {
                    confirm,
                    dry_run,
                    json,
                },
            ),
            Some(cli::PortfolioCommand::Transaction { command }) => match command {
                cli::PortfolioTransactionCommand::Add {
                    symbol,
                    category,
                    tx_type,
                    quantity,
                    price,
                    currency,
                    cash_currency,
                    no_auto_cash,
                    dry_run,
                    json,
                    date,
                    notes,
                } => {
                    if config.is_percentage_mode() {
                        bail!("add-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::add_tx::run(
                        &backend,
                        symbol,
                        category,
                        tx_type,
                        quantity,
                        price,
                        currency,
                        cash_currency,
                        no_auto_cash,
                        dry_run,
                        json,
                        date,
                        notes,
                    )
                }
                cli::PortfolioTransactionCommand::Remove {
                    id,
                    unpaired,
                    dry_run,
                    json,
                } => {
                    if config.is_percentage_mode() {
                        bail!("remove-tx is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::remove_tx::run(&backend, id, unpaired, dry_run, json)
                }
                cli::PortfolioTransactionCommand::List {
                    notes,
                    paired,
                    json,
                } => {
                    if config.is_percentage_mode() {
                        bail!("list-tx is not available in percentage mode (no transactions).\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::list_tx::run(&backend, notes, paired, json)
                }
                cli::PortfolioTransactionCommand::RepairPairs {
                    dry_run,
                    confirm,
                    skip,
                    max_days,
                    max_notional_pct,
                    json,
                } => {
                    if config.is_percentage_mode() {
                        bail!("repair-pairs is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
                    }
                    commands::repair_pairs::run(
                        &backend,
                        commands::repair_pairs::Options {
                            dry_run,
                            confirm,
                            skip,
                            max_days,
                            max_notional_pct,
                            json,
                        },
                    )
                }
                cli::PortfolioTransactionCommand::ImportDelta {
                    csv,
                    dry_run,
                    apply,
                    json,
                } => {
                    if config.is_percentage_mode() {
                        bail!("import-delta is not available in percentage mode.\nRun `pftui setup` to switch to full mode.");
                    }
                    if dry_run && apply {
                        bail!("--dry-run and --apply are mutually exclusive");
                    }
                    commands::import_delta::run(
                        &backend,
                        commands::import_delta::Options {
                            csv_path: csv,
                            apply,
                            json,
                            backup: true,
                        },
                    )
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

        Some(Command::Report {
            command:
                cli::ReportCommand::Chart {
                    chart_name,
                    from_db,
                    from_json,
                    out,
                    format,
                    json,
                },
        }) => commands::report::run_chart(
            &backend,
            &config,
            commands::report::ReportChartOptions {
                chart_name: &chart_name,
                from_db: from_db.as_deref(),
                from_json: from_json.as_deref(),
                out: out.as_deref(),
                format,
                json_output: json,
            },
        ),

        Some(Command::Report {
            command:
                cli::ReportCommand::Build {
                    command:
                        cli::ReportBuildCommand::Daily {
                            mode,
                            date,
                            out_dir,
                            dry_run,
                            json,
                        },
                },
        }) => commands::report::run_build_daily(
            &backend,
            commands::report::BuildDailyOptions {
                mode,
                date: date.as_deref(),
                out_dir: out_dir.as_deref(),
                dry_run,
                json,
            },
        ),

        Some(Command::Report {
            command:
                cli::ReportCommand::Archive {
                    command:
                        cli::ReportArchiveCommand::Import {
                            file,
                            mode,
                            date,
                            title,
                            json,
                        },
                },
        }) => commands::report::run_archive_import(
            &backend,
            &file,
            mode,
            &date,
            title.as_deref(),
            json,
        ),

        Some(Command::Report {
            command:
                cli::ReportCommand::Archive {
                    command: cli::ReportArchiveCommand::List { mode, limit, json },
                },
        }) => commands::report::run_archive_list(&backend, mode, limit, json),

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
            cli::AnalyticsCommand::Gex { symbol, json } => {
                commands::options::run_analytics_gex(&backend, &symbol, json)
            }
            cli::AnalyticsCommand::Technicals {
                command,
                symbol,
                timeframe,
                limit,
                include,
                json,
            } => match command {
                Some(cli::AnalyticsTechnicalsCommand::Indicators { symbol, json }) => {
                    commands::technicals_indicators::run(&backend, &symbol, json)
                }
                Some(cli::AnalyticsTechnicalsCommand::Structure {
                    symbol,
                    timeframe,
                    json,
                }) => commands::technicals_structure::run(&backend, &symbol, &timeframe, json),
                Some(cli::AnalyticsTechnicalsCommand::Cyber {
                    symbol,
                    timeframe,
                    lookback_signals,
                    json,
                }) => commands::technicals_cyber::run(
                    &backend,
                    &symbol,
                    &timeframe,
                    lookback_signals,
                    json,
                ),
                None => commands::analytics::run_technicals_cmd(
                    &backend,
                    symbol.as_deref(),
                    &timeframe,
                    limit,
                    include.as_deref(),
                    json,
                ),
            },
            cli::AnalyticsCommand::Models { command } => match command {
                cli::AnalyticsModelsCommand::List { json } => commands::cli_json::or_json_error(
                    "analytics models list",
                    json,
                    commands::models_cmd::run_list(json),
                ),
                cli::AnalyticsModelsCommand::Show { name, json } => commands::cli_json::or_json_error(
                    "analytics models show",
                    json,
                    commands::models_cmd::run_show(&name, json),
                ),
                cli::AnalyticsModelsCommand::Backtest {
                    name,
                    from,
                    to,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics models backtest",
                    json,
                    commands::models_cmd::run_backtest(
                        &backend,
                        &name,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                ),
                cli::AnalyticsModelsCommand::Compare {
                    names,
                    from,
                    to,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics models compare",
                    json,
                    commands::models_cmd::run_compare(
                        &backend,
                        &names,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                ),
                cli::AnalyticsModelsCommand::Optimize {
                    name,
                    from,
                    to,
                    param,
                    folds,
                    objective,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics models optimize",
                    json,
                    commands::models_cmd::run_optimize(
                        &backend,
                        &name,
                        from.as_deref(),
                        to.as_deref(),
                        &param,
                        folds,
                        &objective,
                        json,
                    ),
                ),
            },
            cli::AnalyticsCommand::Cycles { command } => match command {
                // Wrap in or_json_error so a failure emits the same
                // {"error":{command,message}} envelope on stdout (nonzero exit)
                // as hurst/avwap/regime-break — uniform agent contract.
                cli::AnalyticsCyclesCommand::Clock { asset, json } => {
                    commands::cli_json::or_json_error(
                        "analytics cycles clock",
                        json,
                        commands::cycle_clock_cmd::run(&backend, asset.as_deref(), json),
                    )
                }
                cli::AnalyticsCyclesCommand::Analyze {
                    symbol,
                    asset,
                    degree,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics cycles analyze",
                    json,
                    match symbol.or(asset) {
                        Some(sym) => commands::cycle_engine_cmd::run_analyze(
                            &backend,
                            &sym,
                            degree.as_deref(),
                            json,
                        ),
                        None => Err(anyhow::anyhow!(
                            "provide a symbol (positional) or --asset, e.g. `cycles analyze BTC`"
                        )),
                    },
                ),
                cli::AnalyticsCyclesCommand::BottomSignals {
                    symbol,
                    asset,
                    timeframe,
                    json,
                    sub,
                } => match sub {
                    Some(cli::BottomSignalsCommand::Backtest {
                        symbol: bt_symbol,
                        asset: bt_asset,
                        timeframe: bt_tf,
                        window,
                        expectancy,
                        detrend,
                        json: bt_json,
                    }) => commands::cli_json::or_json_error(
                        "analytics cycles bottom-signals backtest",
                        bt_json,
                        match bt_symbol.or(bt_asset).or(symbol).or(asset) {
                            Some(sym) => commands::cycle_signals_cmd::run_backtest(
                                &backend, &sym, &bt_tf, window, expectancy, detrend, bt_json,
                            ),
                            None => Err(anyhow::anyhow!(
                                "provide a symbol (positional) or --asset, e.g. `cycles bottom-signals backtest --asset BTC`"
                            )),
                        },
                    ),
                    Some(cli::BottomSignalsCommand::TriggerBacktest {
                        symbol: bt_symbol,
                        asset: bt_asset,
                        triggers,
                        mode,
                        horizons,
                        timeframe: bt_tf,
                        window,
                        json: bt_json,
                    }) => commands::cli_json::or_json_error(
                        "analytics cycles bottom-signals trigger-backtest",
                        bt_json,
                        match bt_symbol.or(bt_asset).or(symbol).or(asset) {
                            Some(sym) => commands::cycle_signals_cmd::run_trigger_backtest(
                                commands::cycle_signals_cmd::TriggerBacktestCommandOptions {
                                    backend: &backend,
                                    symbol: &sym,
                                    side: crate::analytics::cycle_signal_backtest::TriggerSide::Bottom,
                                    timeframe: &bt_tf,
                                    triggers: &triggers,
                                    mode: &mode,
                                    horizons: &horizons,
                                    window,
                                    json_output: bt_json,
                                },
                            ),
                            None => Err(anyhow::anyhow!(
                                "provide a symbol (positional) or --asset, e.g. `cycles bottom-signals trigger-backtest --asset BTC --trigger rsi_ma_cross_above_rsi`"
                            )),
                        },
                    ),
                    None => commands::cli_json::or_json_error(
                        "analytics cycles bottom-signals",
                        json,
                        match symbol.or(asset) {
                            Some(sym) => commands::cycle_signals_cmd::run(
                                &backend,
                                &sym,
                                &timeframe,
                                json,
                            ),
                            None => Err(anyhow::anyhow!(
                                "provide a symbol (positional) or --asset, e.g. `cycles bottom-signals --asset BTC`"
                            )),
                        },
                    ),
                },
                cli::AnalyticsCyclesCommand::TopSignals {
                    symbol,
                    asset,
                    timeframe,
                    json,
                    sub,
                } => match sub {
                    Some(cli::TopSignalsCommand::Backtest {
                        symbol: bt_symbol,
                        asset: bt_asset,
                        timeframe: bt_tf,
                        window,
                        expectancy,
                        detrend,
                        json: bt_json,
                    }) => commands::cli_json::or_json_error(
                        "analytics cycles top-signals backtest",
                        bt_json,
                        match bt_symbol.or(bt_asset).or(symbol).or(asset) {
                            Some(sym) => commands::cycle_signals_cmd::run_top_backtest(
                                &backend, &sym, &bt_tf, window, expectancy, detrend, bt_json,
                            ),
                            None => Err(anyhow::anyhow!(
                                "provide a symbol (positional) or --asset, e.g. `cycles top-signals backtest --asset BTC`"
                            )),
                        },
                    ),
                    Some(cli::TopSignalsCommand::TriggerBacktest {
                        symbol: bt_symbol,
                        asset: bt_asset,
                        triggers,
                        mode,
                        horizons,
                        timeframe: bt_tf,
                        window,
                        json: bt_json,
                    }) => commands::cli_json::or_json_error(
                        "analytics cycles top-signals trigger-backtest",
                        bt_json,
                        match bt_symbol.or(bt_asset).or(symbol).or(asset) {
                            Some(sym) => commands::cycle_signals_cmd::run_trigger_backtest(
                                commands::cycle_signals_cmd::TriggerBacktestCommandOptions {
                                    backend: &backend,
                                    symbol: &sym,
                                    side: crate::analytics::cycle_signal_backtest::TriggerSide::Top,
                                    timeframe: &bt_tf,
                                    triggers: &triggers,
                                    mode: &mode,
                                    horizons: &horizons,
                                    window,
                                    json_output: bt_json,
                                },
                            ),
                            None => Err(anyhow::anyhow!(
                                "provide a symbol (positional) or --asset, e.g. `cycles top-signals trigger-backtest --asset BTC --trigger rsi_ma_cross_below_rsi`"
                            )),
                        },
                    ),
                    None => commands::cli_json::or_json_error(
                        "analytics cycles top-signals",
                        json,
                        match symbol.or(asset) {
                            Some(sym) => commands::cycle_signals_cmd::run_top(
                                &backend, &sym, &timeframe, json,
                            ),
                            None => Err(anyhow::anyhow!(
                                "provide a symbol (positional) or --asset, e.g. `cycles top-signals --asset BTC`"
                            )),
                        },
                    ),
                },
                cli::AnalyticsCyclesCommand::Tracked {
                    asset,
                    polarity,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics cycles tracked",
                    json,
                    commands::cycle_tracked_cmd::run(
                        &backend,
                        asset.as_deref(),
                        polarity.as_deref(),
                        json,
                    ),
                ),
                cli::AnalyticsCyclesCommand::Ledger {
                    symbol,
                    asset,
                    degree,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics cycles ledger",
                    json,
                    match symbol.or(asset) {
                        Some(sym) => {
                            commands::cycle_engine_cmd::run_ledger(&backend, &sym, &degree, json)
                        }
                        None => Err(anyhow::anyhow!(
                            "provide a symbol (positional) or --asset, e.g. `cycles ledger BTC --degree 4-year`"
                        )),
                    },
                ),
            },
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
                direction,
                source,
                limit,
                json,
            } => commands::analytics::run_signals_combined(
                &backend,
                symbol.as_deref(),
                signal_type.as_deref(),
                severity.as_deref(),
                direction.as_deref(),
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
            cli::AnalyticsCommand::Calibration {
                threshold,
                window_days,
                by_layer,
                json,
            } => {
                commands::calibration::run(&backend, threshold, window_days, by_layer, json)
            }
            cli::AnalyticsCommand::RiskFactors { command } => match command {
                cli::AnalyticsRiskFactorsCommand::Add {
                    symbol,
                    factor,
                    direction,
                    exposure,
                    notes,
                    json,
                } => {
                    let id = crate::db::risk_factor_mappings::upsert_backend(
                        &backend,
                        &symbol,
                        &factor,
                        &direction,
                        exposure,
                        notes.as_deref(),
                    )?;
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({ "action": "upsert", "id": id, "symbol": symbol, "factor": factor })
                        );
                    } else {
                        println!(
                            "risk_factor_mappings upserted (id {id}): {symbol}/{factor} {direction} x{exposure:.2}"
                        );
                    }
                    Ok(())
                }
                cli::AnalyticsRiskFactorsCommand::List { symbol, json } => {
                    let rows = crate::db::risk_factor_mappings::list_backend(
                        &backend,
                        symbol.as_deref(),
                    )?;
                    if json {
                        let json_rows: Vec<serde_json::Value> = rows
                            .iter()
                            .map(|r| {
                                serde_json::json!({
                                    "id": r.id,
                                    "symbol": r.symbol,
                                    "factor": r.factor,
                                    "direction": r.direction,
                                    "exposure_multiplier": r.exposure_multiplier,
                                    "notes": r.notes,
                                    "created_at": r.created_at,
                                })
                            })
                            .collect();
                        println!("{}", serde_json::to_string_pretty(&json_rows)?);
                    } else {
                        for r in &rows {
                            println!(
                                "{:<8} {:<24} {:<6}  x{:<5.2}  {}",
                                r.symbol,
                                r.factor,
                                r.direction,
                                r.exposure_multiplier,
                                r.notes.as_deref().unwrap_or("")
                            );
                        }
                    }
                    Ok(())
                }
                cli::AnalyticsRiskFactorsCommand::Delete {
                    symbol,
                    factor,
                    json,
                } => {
                    let n = backend
                        .sqlite_native()
                        .and_then(|conn| {
                            crate::db::risk_factor_mappings::delete(conn, &symbol, &factor).ok()
                        })
                        .unwrap_or(0);
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({ "deleted": n, "symbol": symbol, "factor": factor })
                        );
                    } else {
                        println!("deleted {n} mapping(s) for {symbol}/{factor}");
                    }
                    Ok(())
                }
            },
            cli::AnalyticsCommand::CalibrationMatrix { command } => match command {
                cli::AnalyticsCalibrationMatrixCommand::Rebuild { since, json } => {
                    let result =
                        crate::analytics::calibration_scorer::rebuild_calibration_matrix_backend(
                            &backend, since,
                        )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    } else {
                        println!(
                            "calibration_matrix rebuilt: {} rows deleted, {} rows inserted",
                            result.rows_deleted, result.rows_inserted
                        );
                    }
                    Ok(())
                }
                cli::AnalyticsCalibrationMatrixCommand::List { layer, json } => {
                    let rows = crate::analytics::calibration_scorer::list_calibration_matrix_backend(
                        &backend,
                        layer.as_deref(),
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&rows)?);
                    } else {
                        for row in &rows {
                            println!(
                                "{:<10} {:<14} {:<8}  n={:<4}  hit_rate={:.2}  stated_conf={}",
                                row.layer,
                                row.topic,
                                row.conviction_band,
                                row.n,
                                row.hit_rate,
                                row.stated_confidence
                                    .map(|v| format!("{v:.2}"))
                                    .unwrap_or_else(|| "—".to_string())
                            );
                        }
                    }
                    Ok(())
                }
            },
            cli::AnalyticsCommand::NarrativeDivergence {
                hours,
                threshold,
                json,
            } => commands::narrative_divergence::run(&backend, hours, threshold, json),
            cli::AnalyticsCommand::NewsSilence {
                command,
                window_days,
                json,
            } => match command {
                None => commands::news_silence::run(&backend, window_days, json),
                Some(cli::AnalyticsNewsSilenceCommand::RebuildBaselines { since, json }) => {
                    let days = parse_since_to_days(&since)?;
                    commands::news_silence::rebuild_baselines(&backend, days, json)
                }
            },
            cli::AnalyticsCommand::Lessons { command } => match command {
                cli::AnalyticsLessonsCommand::Applied { since, json } => {
                    commands::lessons_applied::run(&backend, &since, json)
                }
                cli::AnalyticsLessonsCommand::Curate {
                    dry_run,
                    retire_after_days,
                    json,
                } => commands::lessons_curate::run_curate(
                    &backend,
                    dry_run,
                    retire_after_days,
                    json,
                ),
                cli::AnalyticsLessonsCommand::Revive { id, json } => {
                    commands::lessons_curate::run_revive(&backend, id, json)
                }
                cli::AnalyticsLessonsCommand::Health { json } => {
                    commands::lessons_curate::run_health(&backend, json)
                }
                cli::AnalyticsLessonsCommand::Rules { command } => match command {
                    cli::AnalyticsLessonsRulesCommand::Add {
                        rule,
                        rationale,
                        sources,
                        enforcement,
                        json,
                    } => commands::standing_rules::run_add(
                        &backend,
                        &rule,
                        rationale.as_deref(),
                        sources.as_deref(),
                        &enforcement,
                        json,
                    ),
                    cli::AnalyticsLessonsRulesCommand::List { all, json } => {
                        commands::standing_rules::run_list(&backend, all, json)
                    }
                    cli::AnalyticsLessonsRulesCommand::Retire { id, json } => {
                        commands::standing_rules::run_retire(&backend, id, json)
                    }
                    cli::AnalyticsLessonsRulesCommand::Cite { id, json } => {
                        commands::standing_rules::run_cite(&backend, id, json)
                    }
                },
            },
            cli::AnalyticsCommand::Thesis { command } => match command {
                cli::AnalyticsThesisCommand::SetReview {
                    section,
                    date,
                    json,
                } => commands::thesis::run_set_review(&backend, &section, &date, json),
                cli::AnalyticsThesisCommand::ReviewDue { json } => {
                    commands::thesis::run_review_due(&backend, json)
                }
            },
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
            cli::AnalyticsCommand::NewsSources { command } => match command {
                cli::AnalyticsNewsSourcesCommand::Accuracy {
                    domain,
                    topic,
                    window_days,
                    include_pre_deployment,
                    json,
                } => commands::analytics::run_news_source_accuracy(
                    &backend,
                    domain.as_deref(),
                    topic.as_deref(),
                    window_days,
                    include_pre_deployment,
                    json,
                ),
                cli::AnalyticsNewsSourcesCommand::Rank {
                    topic,
                    window_days,
                    limit,
                    json,
                } => commands::analytics::run_news_source_rank(
                    &backend,
                    topic.as_deref(),
                    window_days,
                    limit,
                    json,
                ),
                cli::AnalyticsNewsSourcesCommand::RebuildAccuracy {
                    since,
                    dry_run,
                    json,
                } => {
                    let days = since
                        .as_deref()
                        .map(parse_since_to_days)
                        .transpose()?;
                    commands::analytics::run_news_source_rebuild_accuracy(
                        &backend, days, dry_run, json,
                    )
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
                    allocation_bias,
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
                    allocation_bias.as_deref(),
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
                    layer,
                    limit,
                    json,
                } => commands::analyst_views::divergence(
                    &backend,
                    min_spread,
                    asset.as_deref(),
                    layer.as_deref(),
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
                cli::AnalyticsViewsCommand::Convergence {
                    asset,
                    since,
                    json,
                } => commands::analyst_views::convergence(&backend, &asset, &since, json),
                cli::AnalyticsViewsCommand::ConvergenceAll { since, json } => {
                    commands::analyst_views::convergence_all(&backend, &since, json)
                }
                cli::AnalyticsViewsCommand::Delete {
                    analyst,
                    asset,
                    json,
                } => commands::analyst_views::delete(&backend, &analyst, &asset, json),
                cli::AnalyticsViewsCommand::Stale {
                    days,
                    move_pct,
                    json,
                } => commands::views_stale::run(&backend, days, move_pct, json),
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
            cli::AnalyticsCommand::RealRates { command } => match command {
                cli::AnalyticsRealRatesCommand::Differentials { since, json } => {
                    commands::real_yields::differentials(&backend, Some(since.as_str()), json)
                }
            },
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
                Some(cli::AnalyticsMacroCommand::Log { command, limit, json }) => match command {
                    Some(cli::AnalyticsMacroLogCommand::Add {
                        value,
                        development,
                        date,
                        cycle_impact,
                        outcome_shift,
                        json: command_json,
                    }) => {
                        let development = development.or(value);
                        let date =
                            date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
                        commands::analytics::run(
                            &backend,
                            "macro",
                            Some("log"),
                            Some("add"),
                            development.as_deref(),
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
                            cycle_impact.as_deref(),
                            outcome_shift.as_deref(),
                            None,
                            false,
                            None,
                            None,
                            None,
                            None,
                            Some(&date),
                            None,
                            command_json || json,
                        )
                    }
                    None => commands::analytics::run(
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
                },
                Some(cli::AnalyticsMacroCommand::Regime { command }) => match command {
                    cli::AnalyticsMacroRegimeCommand::Current { json } => {
                        commands::regime::run(
                            &backend, "current", None, None, None, json,
                        )
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
                    cli::AnalyticsMacroRegimeCommand::History {
                        limit,
                        from,
                        to,
                        json,
                    } => commands::regime::run(
                        &backend,
                        "history",
                        limit,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                    cli::AnalyticsMacroRegimeCommand::Transitions {
                        limit,
                        from,
                        to,
                        json,
                    } => commands::regime::run(
                        &backend,
                        "transitions",
                        limit,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                    cli::AnalyticsMacroRegimeCommand::Summary { from, to, json } => {
                        commands::regime::run(
                            &backend,
                            "summary",
                            None,
                            from.as_deref(),
                            to.as_deref(),
                            json,
                        )
                    }
                    cli::AnalyticsMacroRegimeCommand::ConfidenceTrend {
                        limit,
                        window,
                        from,
                        to,
                        json,
                    } => commands::regime::run_confidence_trend(
                        &backend,
                        limit,
                        window,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                    cli::AnalyticsMacroRegimeCommand::Override {
                        regime,
                        reason,
                        expires,
                        clear,
                        json,
                    } => commands::regime::run_override(
                        &backend,
                        regime.as_deref(),
                        reason.as_deref(),
                        &expires,
                        clear,
                        json,
                    ),
                },
            },
            cli::AnalyticsCommand::Alignment {
                command,
                symbol,
                summary,
                json,
            } => match command {
                Some(cli::AnalyticsAlignmentCommand::Current { json: cmd_json }) => {
                    commands::alignment_score::run_current(&backend, &config, cmd_json || json)
                }
                Some(cli::AnalyticsAlignmentCommand::History {
                    since,
                    json: cmd_json,
                }) => commands::alignment_score::run_history(&backend, &since, cmd_json || json),
                Some(cli::AnalyticsAlignmentCommand::Compute {
                    date,
                    store,
                    json: cmd_json,
                }) => commands::alignment_score::run_compute(
                    &backend,
                    &config,
                    date.as_deref(),
                    store,
                    cmd_json || json,
                ),
                None => commands::analytics::run(
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
            },
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
            cli::AnalyticsCommand::Digest {
                from,
                agent_filter,
                limit,
                json,
            } => commands::analytics::run(
                &backend,
                "digest",
                agent_filter.as_deref(),
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
            cli::AnalyticsCommand::MarketSnapshot { json, auto_refresh } => {
                commands::market_snapshot::run(&backend, &config, json, auto_refresh)
            }
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
                    severity,
                    seed_alerts,
                    cooldown,
                    verbose,
                    history_depth,
                    json,
                }) => commands::correlations::run_breaks(
                    &backend,
                    threshold,
                    limit,
                    severity.as_deref(),
                    seed_alerts,
                    cooldown,
                    verbose,
                    history_depth,
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
                    false,
                    json,
                ),
                cli::AnalyticsTrendsCommand::List {
                    timeframe,
                    direction,
                    conviction,
                    category,
                    status,
                    limit,
                    verbose,
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
                    verbose,
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
                    false,
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
                    false,
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
                            false,
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
                            false,
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
                        false,
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
                        false,
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
                        json,
                    } => (
                        "add",
                        commands::alerts::AlertsArgs {
                            rule,
                            id: None,
                            ids: vec![],
                            json,
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
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered: false,
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
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered: false,
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
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered: false,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::Check { today, newly_triggered, kind, condition, symbol, status, urgency, json } => (
                        "check",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids: vec![],
                            json,
                            status_filter: status,
                            today,
                            kind,
                            symbol,
                            from_level: None,
                            condition,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                            newly_triggered_only: newly_triggered,
                            urgency_filter: urgency,
                            all_triggered: false,
                        },
                    ),
                    cli::AnalyticsAlertsCommand::Ack {
                        ids,
                        all_triggered,
                        condition,
                        kind,
                        symbol,
                        json,
                    } => (
                        "ack",
                        commands::alerts::AlertsArgs {
                            rule: None,
                            id: None,
                            ids,
                            json,
                            status_filter: None,
                            today: false,
                            kind,
                            symbol,
                            from_level: None,
                            condition,
                            label: None,
                            triggered: false,
                            since_hours: None,
                            recurring: false,
                            cooldown_minutes: 0,
                            recent: false,
                            recent_hours: 24,
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered,
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
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered: false,
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
                            newly_triggered_only: false,
                            urgency_filter: None,
                            all_triggered: false,
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
                    precedent: _,
                    status,
                    driver,
                    notes,
                    evidence,
                    proposer,
                    hard_print,
                    override_conflict,
                    json,
                } => {
                    let merged_notes = driver.or(notes).or(note_pos);
                    commands::scenario::update(
                        &backend,
                        Some(&value),
                        None,
                        probability,
                        description.as_deref(),
                        impact.as_deref(),
                        triggers.as_deref(),
                        status.as_deref(),
                        merged_notes.as_deref(),
                        commands::scenario::UpdateGuardOpts {
                            proposer: proposer.as_deref(),
                            evidence: evidence.as_deref(),
                            hard_print: hard_print.as_deref(),
                            override_conflict,
                        },
                        json,
                    )
                }
                cli::AnalyticsScenarioCommand::SetBaseRate {
                    value,
                    rate,
                    reference,
                    json,
                } => commands::scenario::set_base_rate(&backend, &value, rate, &reference, json),
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
                cli::AnalyticsScenarioCommand::Detect { hours, limit, json } => {
                    commands::scenario_detect::run(&backend, hours, limit, json)
                }
                cli::AnalyticsScenarioCommand::ImpactMatrix { json } => {
                    commands::impact_matrix::run(&backend, &config, json)
                }
                cli::AnalyticsScenarioCommand::Timeline { days, json } => {
                    commands::scenario::run(
                        &backend,
                        "timeline",
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
                        days.map(|d| d as usize),
                        json,
                    )
                }
            },
            cli::AnalyticsCommand::Epistemics { command } => match command {
                cli::AnalyticsEpistemicsCommand::Record {
                    date,
                    agreement,
                    blind_divergence,
                    panel_dispersion,
                    novelty,
                    fallback_warnings,
                    scenario_delta_total,
                    audit_pass_rate,
                    agents,
                    notes,
                    conviction_price_corr,
                    forecast_hit_rate,
                    active_misalignments,
                    json,
                } => commands::epistemics::record(
                    &backend,
                    &date,
                    agreement,
                    blind_divergence,
                    panel_dispersion,
                    novelty,
                    fallback_warnings,
                    scenario_delta_total,
                    audit_pass_rate,
                    agents,
                    notes.as_deref(),
                    conviction_price_corr,
                    forecast_hit_rate,
                    active_misalignments,
                    json,
                ),
                cli::AnalyticsEpistemicsCommand::Show { date, json } => {
                    commands::epistemics::show(&backend, date.as_deref(), json)
                }
                cli::AnalyticsEpistemicsCommand::History { limit, json } => {
                    commands::epistemics::history(&backend, limit, json)
                }
                cli::AnalyticsEpistemicsCommand::Rivalry { json } => {
                    commands::epistemics::rivalry(&backend, json)
                }
                cli::AnalyticsEpistemicsCommand::ConvictionPrice { days, asset, json } => {
                    commands::epistemics::conviction_price(&backend, days, asset.as_deref(), json)
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
                geo,
                limit,
                json,
            } => dispatch_predictions(&backend, command, category, search, geo, limit, json),
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
                cli::AnalyticsPowerFlowCommand::Conflicts { days, json } => {
                    commands::power_flow_conflicts::run(&backend, days, json)
                }
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
            cli::AnalyticsCommand::MorningBrief { json, section } => {
                commands::morning_brief::run(&backend, json, &section)
            }
            cli::AnalyticsCommand::EveningBrief { json, section } => {
                commands::evening_brief::run(&backend, json, &section)
            }
            cli::AnalyticsCommand::Guidance { json } => {
                commands::guidance::run(&backend, json)
            }
            cli::AnalyticsCommand::RegimeFlows { json } => {
                commands::regime_flows::run(&backend, json)
            }
            cli::AnalyticsCommand::PowerSignals { days, json } => {
                commands::power_signals::run(&backend, days, json)
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
                cli::AnalyticsBacktestCommand::Report { json } => {
                    commands::backtest::run_report(&backend, json)
                }
                cli::AnalyticsBacktestCommand::Agent { agent, json } => {
                    commands::backtest::run_agent(&backend, &agent, json)
                }
                cli::AnalyticsBacktestCommand::Diagnostics { agent, json } => {
                    commands::backtest::run_diagnostics(&backend, agent.as_deref(), json)
                }
                cli::AnalyticsBacktestCommand::Scenario {
                    regime,
                    inflation_min,
                    inflation_max,
                    recession_min,
                    recession_max,
                    iran_min,
                    iran_max,
                    risk_on_min,
                    risk_on_max,
                    layer,
                    topic,
                    conviction,
                    json,
                } => {
                    let args = commands::backtest_scenario::ScenarioBacktestArgs {
                        regime: regime.as_deref(),
                        inflation_min,
                        inflation_max,
                        recession_min,
                        recession_max,
                        iran_min,
                        iran_max,
                        risk_on_min,
                        risk_on_max,
                        layer: layer.as_deref(),
                        topic: topic.as_deref(),
                        conviction: conviction.as_deref(),
                    };
                    commands::backtest_scenario::run_scenario(&backend, args, json)
                }
                cli::AnalyticsBacktestCommand::LayerBias {
                    regime,
                    inflation_min,
                    inflation_max,
                    recession_min,
                    recession_max,
                    iran_min,
                    iran_max,
                    risk_on_min,
                    risk_on_max,
                    json,
                } => {
                    let args = commands::backtest_scenario::ScenarioBacktestArgs {
                        regime: regime.as_deref(),
                        inflation_min,
                        inflation_max,
                        recession_min,
                        recession_max,
                        iran_min,
                        iran_max,
                        risk_on_min,
                        risk_on_max,
                        layer: None,
                        topic: None,
                        conviction: None,
                    };
                    commands::backtest_scenario::run_layer_bias(&backend, args, json)
                }
            },
            cli::AnalyticsCommand::Environment { command } => match command {
                cli::AnalyticsEnvironmentCommand::Current { json } => {
                    commands::environment_cmd::run_current(&backend, json)
                }
            },
            cli::AnalyticsCommand::Analog {
                asset,
                horizon,
                k,
                exclude_days,
                json,
            } => commands::environment_cmd::run_analog(
                &backend,
                &asset,
                horizon,
                k,
                exclude_days,
                json,
            ),
            cli::AnalyticsCommand::Positioning {
                asset,
                horizon,
                k,
                json,
            } => commands::environment_cmd::run_positioning(&backend, &asset, horizon, k, json),
            cli::AnalyticsCommand::TailRisk {
                asset,
                lookback,
                threshold,
                json,
            } => commands::cli_json::or_json_error(
                "analytics tail-risk",
                json,
                commands::tail_risk::run(&backend, &asset, lookback, threshold, json),
            ),
            cli::AnalyticsCommand::TailDependence {
                asset,
                vs,
                q,
                json,
            } => commands::cli_json::or_json_error(
                "analytics tail-dependence",
                json,
                commands::tail_dependence::run(&backend, &asset, &vs, q, json),
            ),
            cli::AnalyticsCommand::Avwap {
                asset,
                anchor,
                anchor_date,
                json,
            } => commands::cli_json::or_json_error(
                "analytics avwap",
                json,
                commands::avwap::run(&backend, &asset, &anchor, anchor_date.as_deref(), json),
            ),
            cli::AnalyticsCommand::Hurst {
                asset,
                lookback,
                json,
            } => commands::cli_json::or_json_error(
                "analytics hurst",
                json,
                commands::hurst::run(&backend, &asset, lookback, json),
            ),
            cli::AnalyticsCommand::RegimeBreak {
                asset,
                lookback,
                k,
                h,
                json,
            } => commands::cli_json::or_json_error(
                "analytics regime-break",
                json,
                commands::regime_break::run(&backend, &asset, lookback, k, h, json),
            ),
            cli::AnalyticsCommand::RiskDashboard { asset, vs, json } => {
                commands::cli_json::or_json_error(
                    "analytics risk-dashboard",
                    json,
                    commands::risk_dashboard::run(&backend, &asset, vs.as_deref(), json),
                )
            }
            cli::AnalyticsCommand::Basket { command } => match command {
                cli::AnalyticsBasketCommand::Weights {
                    assets,
                    method,
                    lookback,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics basket weights",
                    json,
                    commands::basket::run(&backend, &assets, &method, lookback, json),
                ),
            },
            cli::AnalyticsCommand::Survival {
                asset,
                budget,
                confidence,
                lookback,
                json,
            } => commands::cli_json::or_json_error(
                "analytics survival",
                json,
                commands::survival::run(&backend, &asset, budget, confidence, lookback, json),
            ),
            cli::AnalyticsCommand::Strategy { command } => match command {
                cli::AnalyticsStrategyCommand::Backtest {
                    asset,
                    entry,
                    exit,
                    stop_loss,
                    take_profit,
                    trailing_stop,
                    commission,
                    slippage,
                    next_bar_fill,
                    vol_target,
                    vol_window,
                    max_leverage,
                    from,
                    to,
                    limit,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics strategy backtest",
                    json,
                    commands::strategy::run_backtest(
                        &backend,
                        &asset,
                        &entry,
                        exit.as_deref(),
                        stop_loss,
                        take_profit,
                        trailing_stop,
                        commission,
                        slippage,
                        next_bar_fill,
                        vol_target,
                        vol_window,
                        max_leverage,
                        from.as_deref(),
                        to.as_deref(),
                        limit,
                        json,
                    ),
                ),
                cli::AnalyticsStrategyCommand::Segment {
                    asset,
                    when,
                    from,
                    to,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics strategy segment",
                    json,
                    commands::strategy::run_segment(
                        &backend,
                        &asset,
                        &when,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                ),
                cli::AnalyticsStrategyCommand::Compare {
                    asset,
                    when,
                    when_label,
                    vs,
                    vs_label,
                    from,
                    to,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics strategy compare",
                    json,
                    commands::strategy::run_compare(
                        &backend,
                        &asset,
                        &when,
                        &when_label,
                        &vs,
                        &vs_label,
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                ),
                cli::AnalyticsStrategyCommand::Explain { asset, entry, json } => {
                    commands::cli_json::or_json_error(
                        "analytics strategy explain",
                        json,
                        commands::strategy::run_explain(&backend, &asset, &entry, json),
                    )
                }
                cli::AnalyticsStrategyCommand::Sweep {
                    asset,
                    entry,
                    values,
                    exit,
                    from,
                    to,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics strategy sweep",
                    json,
                    commands::strategy::run_sweep(
                        &backend,
                        &asset,
                        &entry,
                        &values,
                        exit.as_deref(),
                        from.as_deref(),
                        to.as_deref(),
                        json,
                    ),
                ),
                cli::AnalyticsStrategyCommand::Walkforward {
                    asset,
                    entry,
                    values,
                    exit,
                    folds,
                    json,
                } => commands::cli_json::or_json_error(
                    "analytics strategy walkforward",
                    json,
                    commands::strategy::run_walkforward(
                        &backend,
                        &asset,
                        &entry,
                        &values,
                        exit.as_deref(),
                        folds,
                        json,
                    ),
                ),
            },
            cli::AnalyticsCommand::Sources { command } => match command {
                cli::AnalyticsSourcesCommand::List { source_type, json } => {
                    commands::analytics_enrichment::sources_list(
                        &backend,
                        source_type.as_deref(),
                        json,
                    )
                }
                cli::AnalyticsSourcesCommand::Set {
                    canonical_id,
                    display_name,
                    source_type,
                    aliases,
                    topics,
                    accuracy_rating,
                    framework_summary,
                    json,
                } => commands::analytics_enrichment::sources_set(
                    &backend,
                    &canonical_id,
                    &display_name,
                    &source_type,
                    aliases.as_deref(),
                    topics.as_deref(),
                    accuracy_rating.as_deref(),
                    framework_summary.as_deref(),
                    json,
                ),
                cli::AnalyticsSourcesCommand::Remove { canonical_id, json } => {
                    commands::analytics_enrichment::sources_remove(&backend, &canonical_id, json)
                }
            },
            cli::AnalyticsCommand::Events { command } => match command {
                cli::AnalyticsEventsCommand::List {
                    category,
                    since,
                    asset,
                    json,
                } => commands::analytics_enrichment::events_list(
                    &backend,
                    category.as_deref(),
                    since.as_deref(),
                    asset.as_deref(),
                    json,
                ),
                cli::AnalyticsEventsCommand::Add {
                    event_date,
                    event_time,
                    category,
                    headline,
                    detail,
                    source,
                    magnitude,
                    persistence,
                    asset_impact,
                    related_scenario,
                    related_prediction,
                    notes,
                    json,
                } => commands::analytics_enrichment::events_add(
                    &backend,
                    &event_date,
                    event_time.as_deref(),
                    &category,
                    &headline,
                    detail.as_deref(),
                    source.as_deref(),
                    magnitude,
                    persistence.as_deref(),
                    asset_impact.as_deref(),
                    related_scenario.as_deref(),
                    related_prediction.as_deref(),
                    notes.as_deref(),
                    json,
                ),
            },
            cli::AnalyticsCommand::Fragments { command } => match command {
                cli::AnalyticsFragmentsCommand::List {
                    fragment_type,
                    topic,
                    cluster,
                    for_claim,
                    json,
                } => commands::analytics_enrichment::fragments_list(
                    &backend,
                    fragment_type.as_deref(),
                    topic.as_deref(),
                    cluster.as_deref(),
                    for_claim.as_deref(),
                    json,
                ),
                cli::AnalyticsFragmentsCommand::Show { canonical_id, json } => {
                    commands::analytics_enrichment::fragments_show(&backend, &canonical_id, json)
                }
            },
            cli::AnalyticsCommand::CalibrationAdjustments {
                layer,
                topic,
                conviction,
                json,
            } => commands::analytics_enrichment::calibration_adjustments_list(
                &backend,
                layer.as_deref(),
                topic.as_deref(),
                conviction.as_deref(),
                json,
            ),
            cli::AnalyticsCommand::Failures { command } => match command {
                cli::AnalyticsFailuresCommand::Correlations {
                    cluster,
                    min_share,
                    json,
                } => commands::analytics_enrichment::failures_correlations(
                    &backend,
                    cluster.as_deref(),
                    min_share,
                    json,
                ),
            },
            cli::AnalyticsCommand::Clusters { command } => match command {
                cli::AnalyticsClustersCommand::List { json } => {
                    commands::analytics_enrichment::clusters_list(&backend, json)
                }
                cli::AnalyticsClustersCommand::Stats { json } => {
                    commands::analytics_enrichment::clusters_stats(&backend, json)
                }
            },
            cli::AnalyticsCommand::ThesisChains { command } => match command {
                cli::AnalyticsThesisChainsCommand::List { state, node, json } => {
                    commands::analytics_enrichment::thesis_chains_list(
                        &backend,
                        state.as_deref(),
                        node.as_deref(),
                        json,
                    )
                }
                cli::AnalyticsThesisChainsCommand::Show { id, json } => {
                    commands::analytics_enrichment::thesis_chains_show(&backend, id, json)
                }
                cli::AnalyticsThesisChainsCommand::Validate { id, as_of, json } => {
                    commands::analytics_enrichment::thesis_chains_validate(
                        &backend,
                        id,
                        as_of.as_deref(),
                        json,
                    )
                }
                cli::AnalyticsThesisChainsCommand::Extract {
                    from_thesis,
                    from_lessons,
                    from_messages,
                    since,
                    dry_run,
                    apply,
                    json,
                } => commands::analytics_enrichment::thesis_chains_extract(
                    &backend,
                    from_thesis,
                    from_lessons,
                    from_messages,
                    &since,
                    dry_run,
                    apply,
                    json,
                ),
                cli::AnalyticsThesisChainsCommand::Add {
                    antecedent,
                    consequent,
                    relation,
                    antecedent_id,
                    consequent_id,
                    conviction,
                    evidence_count,
                    source_lesson_ids,
                    source_thesis_sections,
                    json,
                } => commands::analytics_enrichment::thesis_chains_add(
                    &backend,
                    &antecedent,
                    &consequent,
                    &relation,
                    antecedent_id.as_deref(),
                    consequent_id.as_deref(),
                    conviction.as_deref(),
                    evidence_count,
                    source_lesson_ids.as_deref(),
                    source_thesis_sections.as_deref(),
                    json,
                ),
            },
            cli::AnalyticsCommand::Falsifications {
                rule_type,
                auto_eligible,
                for_prediction,
                json,
            } => commands::analytics_enrichment::falsifications_list(
                &backend,
                rule_type.as_deref(),
                auto_eligible,
                for_prediction,
                json,
            ),
            cli::AnalyticsCommand::Recommendations { command } => match command {
                cli::AnalyticsRecommendationsCommand::Record {
                    symbol,
                    action,
                    rationale,
                    date,
                    source,
                    json,
                } => commands::recommendations::record_cmd(
                    &backend,
                    &symbol,
                    &action,
                    rationale.as_deref(),
                    date.as_deref(),
                    &source,
                    json,
                ),
                cli::AnalyticsRecommendationsCommand::List {
                    date,
                    asset,
                    symbol,
                    recommendation_type,
                    since,
                    limit,
                    json,
                } => commands::recommendations::list_cmd(
                    &backend,
                    date.as_deref(),
                    asset.or(symbol).as_deref(),
                    recommendation_type.as_deref(),
                    since.as_deref(),
                    limit,
                    json,
                ),
                cli::AnalyticsRecommendationsCommand::Scoreboard { symbol, json } => {
                    commands::recommendations::scoreboard_cmd(&backend, symbol.as_deref(), json)
                }
                cli::AnalyticsRecommendationsCommand::Score {
                    all,
                    id,
                    horizon,
                    since,
                    json,
                } => commands::recommendations::score_cmd(
                    &backend,
                    all,
                    id,
                    horizon,
                    since.as_deref(),
                    json,
                ),
                cli::AnalyticsRecommendationsCommand::Accuracy {
                    recommendation_type,
                    asset,
                    since,
                    threshold,
                    by_asset,
                    json,
                } => commands::recommendations::accuracy_cmd(
                    &backend,
                    recommendation_type.as_deref(),
                    asset.as_deref(),
                    &since,
                    threshold,
                    by_asset,
                    json,
                ),
                cli::AnalyticsRecommendationsCommand::Link {
                    id,
                    reply_id,
                    transaction_id,
                    action_status,
                    json,
                } => commands::recommendations::link_cmd(
                    &backend,
                    id,
                    reply_id,
                    transaction_id,
                    action_status.as_deref(),
                    json,
                ),
                cli::AnalyticsRecommendationsCommand::RelinkHistorical { window, json } => {
                    commands::recommendations::relink_historical_cmd(&backend, window, json)
                }
            },
            cli::AnalyticsCommand::Adversary { command } => match command {
                cli::AnalyticsAdversaryCommand::Synthesis { command } => match command {
                    cli::AnalyticsAdversarySynthesisCommand::Add {
                        asset,
                        convergence,
                        counter,
                        evidence,
                        falsification,
                        fragility,
                        recorded_at,
                        json,
                    } => commands::adversary_synthesis::synthesis_add(
                        &backend,
                        &asset,
                        &convergence,
                        &counter,
                        &evidence,
                        &falsification,
                        fragility,
                        recorded_at.as_deref(),
                        json,
                    ),
                    cli::AnalyticsAdversarySynthesisCommand::Show {
                        asset,
                        since,
                        json,
                    } => commands::adversary_synthesis::synthesis_show(
                        &backend,
                        asset.as_deref(),
                        since.as_deref(),
                        json,
                    ),
                    cli::AnalyticsAdversarySynthesisCommand::FragilityRank { since, json } => {
                        commands::adversary_synthesis::synthesis_fragility_rank(
                            &backend,
                            since.as_deref(),
                            json,
                        )
                    }
                },
            },
            cli::AnalyticsCommand::Flows { command } => match command {
                cli::AnalyticsFlowsCommand::Summary { since, json } => {
                    commands::flows::summary(&backend, since, json)
                }
            },
        },

        // Research harness (R1a): signal registry + event-study engine.
        Some(Command::Research { command }) => match command {
            cli::ResearchCommand::Forecasts { command } => match command {
                cli::ResearchForecastsCommand::Score { json } => {
                    commands::research_forecasts::score_cmd(&backend, json)
                }
                cli::ResearchForecastsCommand::Report {
                    layer,
                    asset,
                    window_days,
                    json,
                } => commands::research_forecasts::report_cmd(
                    &backend,
                    layer.as_deref(),
                    asset.as_deref(),
                    window_days,
                    json,
                ),
                cli::ResearchForecastsCommand::Streaks { threshold, json } => {
                    commands::research_forecasts::streaks_cmd(&backend, threshold, json)
                }
                cli::ResearchForecastsCommand::Verify {
                    threshold_pp,
                    reissue,
                    json,
                } => commands::research_forecasts::verify_cmd(
                    &backend,
                    threshold_pp,
                    reissue,
                    json,
                ),
            },
            cli::ResearchCommand::Misalignments { all, json } => {
                commands::research_forecasts::misalignments_cmd(&backend, all, json)
            }
            cli::ResearchCommand::Dossier { domain, asset, json } => {
                commands::research_dossier::run(&backend, &domain, asset.as_deref(), json)
            }
            cli::ResearchCommand::Signals { command } => match command {
                cli::ResearchSignalsCommand::List { json } => {
                    commands::research_harness::run_signals_list(json)
                }
            },
            cli::ResearchCommand::Backtest {
                signal,
                asset,
                as_of,
                json,
            } => commands::research_harness::run_backtest(
                &backend,
                signal.as_deref(),
                asset.as_deref(),
                as_of.as_deref(),
                json,
            ),
            cli::ResearchCommand::Expectancy {
                signal,
                asset,
                json,
            } => commands::research_harness::run_expectancy(
                &backend,
                signal.as_deref(),
                asset.as_deref(),
                json,
            ),
            cli::ResearchCommand::Events {
                signal,
                asset,
                limit,
                json,
            } => commands::research_harness::run_events(&backend, &signal, &asset, limit, json),
            cli::ResearchCommand::Shadowbook { json } => {
                commands::shadow_book::run(&backend, json)
            }
            cli::ResearchCommand::VerifyThesis { section, json } => {
                commands::research_thesis_verify::run(&backend, section.as_deref(), json)
            }
        },
    };

    match (result, backend.flush()) {
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
        (Ok(v), Ok(_)) => Ok(v),
    }
}
