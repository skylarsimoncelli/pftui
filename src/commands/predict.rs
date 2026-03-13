use anyhow::{bail, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::user_predictions;

fn validate_conviction(value: &str) -> Result<()> {
    match value {
        "high" | "medium" | "low" => Ok(()),
        _ => bail!("invalid conviction '{}'. Valid: high, medium, low", value),
    }
}

fn validate_timeframe(value: &str) -> Result<()> {
    match value {
        "low" | "medium" | "high" | "macro" => Ok(()),
        _ => bail!("invalid timeframe '{}'. Valid: low, medium, high, macro", value),
    }
}

fn validate_outcome(value: &str) -> Result<()> {
    match value {
        "pending" | "correct" | "partial" | "wrong" => Ok(()),
        _ => bail!("invalid outcome '{}'. Valid: pending, correct, partial, wrong", value),
    }
}

fn parse_date_filter(value: Option<&str>) -> Result<Option<NaiveDate>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let normalized = raw.trim().to_lowercase();
    let today = Utc::now().date_naive();
    if normalized == "today" {
        return Ok(Some(today));
    }
    if normalized == "yesterday" {
        return Ok(Some(today - Duration::days(1)));
    }
    let parsed = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("invalid date '{}'. Use YYYY-MM-DD, today, or yesterday", raw))?;
    Ok(Some(parsed))
}

fn extract_date(raw: &str) -> Option<NaiveDate> {
    if let Ok(d) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Some(d);
    }
    if raw.len() >= 10 {
        let prefix = &raw[..10];
        if let Ok(d) = NaiveDate::parse_from_str(prefix, "%Y-%m-%d") {
            return Some(d);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
    outcome: Option<&str>,
    notes: Option<&str>,
    lesson: Option<&str>,
    filter: Option<&str>,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let claim = value.ok_or_else(|| anyhow::anyhow!("prediction claim required"))?;
            if let Some(c) = conviction {
                validate_conviction(c)?;
            }
            if let Some(tf) = timeframe {
                validate_timeframe(tf)?;
            }
            if let Some(conf) = confidence {
                if !(0.0..=1.0).contains(&conf) {
                    bail!("invalid confidence '{}'. Valid range: 0.0..=1.0", conf);
                }
            }
            let new_id = user_predictions::add_prediction_backend(
                backend,
                claim,
                symbol,
                conviction,
                timeframe,
                confidence,
                source_agent,
                target_date,
                resolution_criteria,
            )?;

            if json_output {
                let rows =
                    user_predictions::list_predictions_backend(backend, None, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                }
            } else {
                println!("Added prediction #{}", new_id);
            }
        }

        "list" => {
            if let Some(f) = filter {
                validate_outcome(f)?;
            }
            if let Some(tf) = timeframe {
                validate_timeframe(tf)?;
            }
            let rows = user_predictions::list_predictions_backend(
                backend,
                filter,
                symbol,
                timeframe,
                limit,
            )?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "predictions": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No predictions found.");
            } else {
                println!("Predictions ({}):", rows.len());
                for row in rows {
                    let sym = row.symbol.unwrap_or_else(|| "—".to_string());
                    println!(
                        "  #{} [{}|{}|{}] {}",
                        row.id, sym, row.conviction, row.outcome, row.claim
                    );
                }
            }
        }

        "score" => {
            let pid = id.ok_or_else(|| anyhow::anyhow!("--id required"))?;
            let out = outcome.ok_or_else(|| anyhow::anyhow!("--outcome required"))?;
            validate_outcome(out)?;
            user_predictions::score_prediction_backend(backend, pid, out, notes, lesson)?;

            if json_output {
                let rows =
                    user_predictions::list_predictions_backend(backend, None, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == pid) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&json!({ "scored": pid }))?);
                }
            } else {
                println!("Scored prediction #{} as {}", pid, out);
            }
        }

        "stats" => {
            let stats = user_predictions::get_stats_backend(backend)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Prediction stats:");
                println!("  Total: {}", stats.total);
                println!("  Scored: {}", stats.scored);
                println!("  Pending: {}", stats.pending);
                println!("  Correct: {}", stats.correct);
                println!("  Partial: {}", stats.partial);
                println!("  Wrong: {}", stats.wrong);
                println!("  Hit rate: {:.1}%", stats.hit_rate_pct);
                println!("  Timeframes tracked: {}", stats.by_timeframe.len());
                println!("  Source agents tracked: {}", stats.by_source_agent.len());
            }
        }

        "scorecard" => {
            if let Some(tf) = timeframe {
                validate_timeframe(tf)?;
            }
            let target_date = parse_date_filter(date)?;
            let mut rows =
                user_predictions::list_predictions_backend(backend, filter, symbol, timeframe, limit)?;
            if let Some(d) = target_date {
                rows.retain(|row| {
                    let event_date = row
                        .scored_at
                        .as_deref()
                        .and_then(extract_date)
                        .or_else(|| extract_date(&row.created_at));
                    event_date == Some(d)
                });
            }

            let total = rows.len();
            let mut scored = 0usize;
            let mut pending = 0usize;
            let mut correct = 0usize;
            let mut partial = 0usize;
            let mut wrong = 0usize;
            for row in &rows {
                match row.outcome.as_str() {
                    "pending" => pending += 1,
                    "correct" => {
                        correct += 1;
                        scored += 1;
                    }
                    "partial" => {
                        partial += 1;
                        scored += 1;
                    }
                    "wrong" => {
                        wrong += 1;
                        scored += 1;
                    }
                    _ => {}
                }
            }
            let hit_rate_pct = if scored > 0 {
                ((correct as f64) + 0.5 * (partial as f64)) / (scored as f64) * 100.0
            } else {
                0.0
            };

            let wrong_without_lesson = rows
                .iter()
                .filter(|r| r.outcome == "wrong")
                .filter(|r| r.lesson.as_deref().is_none_or(|v| v.trim().is_empty()))
                .count();

            // Current streak of consecutive "correct" outcomes among scored predictions.
            let mut scored_rows: Vec<_> = rows
                .iter()
                .filter(|r| r.scored_at.is_some() || r.outcome != "pending")
                .collect();
            scored_rows.sort_by(|a, b| {
                let ka = a.scored_at.as_deref().unwrap_or(&a.created_at);
                let kb = b.scored_at.as_deref().unwrap_or(&b.created_at);
                kb.cmp(ka)
            });
            let mut current_correct_streak = 0usize;
            for row in scored_rows {
                if row.outcome == "correct" {
                    current_correct_streak += 1;
                } else {
                    break;
                }
            }

            let payload = json!({
                "date": target_date.map(|d| d.to_string()),
                "timeframe": timeframe,
                "symbol": symbol,
                "total": total,
                "scored": scored,
                "pending": pending,
                "correct": correct,
                "partial": partial,
                "wrong": wrong,
                "hit_rate_pct": hit_rate_pct,
                "current_correct_streak": current_correct_streak,
                "wrong_without_lesson": wrong_without_lesson,
            });

            if json_output {
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                println!("Prediction scorecard:");
                if let Some(d) = target_date {
                    println!("  Date: {}", d);
                }
                if let Some(tf) = timeframe {
                    println!("  Timeframe: {}", tf);
                }
                println!("  Total: {}", total);
                println!("  Scored: {}", scored);
                println!("  Pending: {}", pending);
                println!("  Correct: {}", correct);
                println!("  Partial: {}", partial);
                println!("  Wrong: {}", wrong);
                println!("  Hit rate: {:.1}%", hit_rate_pct);
                println!("  Current correct streak: {}", current_correct_streak);
                println!("  Wrong calls missing lesson: {}", wrong_without_lesson);
            }
        }

        _ => bail!("unknown predict action '{}'. Valid: add, list, score, stats, scorecard", action),
    }

    Ok(())
}
