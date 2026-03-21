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

/// Resolve intuitive timeframe aliases to canonical names.
/// `short` → `low`, `long` → `high`. `medium` and `macro` are already canonical.
fn resolve_timeframe_alias(value: &str) -> &str {
    match value {
        "short" => "low",
        "long" => "high",
        _ => value,
    }
}

fn validate_timeframe(value: &str) -> Result<()> {
    match value {
        "low" | "medium" | "high" | "macro" => Ok(()),
        _ => bail!(
            "invalid timeframe '{}'. Valid: low, medium, high, macro (aliases: short=low, long=high). Use --timeframe <value> or positional shorthand after the claim.",
            value
        ),
    }
}

/// Resolve aliases and validate a timeframe value. Returns the canonical name.
fn normalize_timeframe(value: &str) -> Result<String> {
    let canonical = resolve_timeframe_alias(value);
    validate_timeframe(canonical)?;
    Ok(canonical.to_string())
}

fn validate_outcome(value: &str) -> Result<()> {
    match value {
        "pending" | "correct" | "partial" | "wrong" => Ok(()),
        _ => bail!(
            "invalid outcome '{}'. Valid: pending, correct, partial, wrong",
            value
        ),
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
    let parsed = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        anyhow::anyhow!(
            "invalid date '{}'. Use YYYY-MM-DD, today, or yesterday",
            raw
        )
    })?;
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
            let resolved_timeframe = match timeframe {
                Some(tf) => Some(normalize_timeframe(tf)?),
                None => None,
            };
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
                resolved_timeframe.as_deref(),
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
            let resolved_timeframe = match timeframe {
                Some(tf) => Some(normalize_timeframe(tf)?),
                None => None,
            };
            let rows = user_predictions::list_predictions_backend(
                backend,
                filter,
                symbol,
                resolved_timeframe.as_deref(),
                limit,
            )?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({ "predictions": rows, "count": rows.len() })
                    )?
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
            let pid = id.ok_or_else(|| {
                anyhow::anyhow!(
                    "prediction id required. Usage: pftui journal prediction score <ID> <OUTCOME> [NOTES] [--lesson ...] or --id/--outcome flags"
                )
            })?;
            let out = outcome.ok_or_else(|| {
                anyhow::anyhow!(
                    "prediction outcome required. Usage: pftui journal prediction score <ID> <OUTCOME> [NOTES] [--lesson ...] or --id/--outcome flags"
                )
            })?;
            validate_outcome(out)?;
            user_predictions::score_prediction_backend(backend, pid, out, notes, lesson)?;

            if json_output {
                let rows =
                    user_predictions::list_predictions_backend(backend, None, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == pid) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "scored": pid }))?
                    );
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
            let resolved_timeframe = match timeframe {
                Some(tf) => Some(normalize_timeframe(tf)?),
                None => None,
            };
            let target_date = parse_date_filter(date)?;
            let mut rows = user_predictions::list_predictions_backend(
                backend,
                filter,
                symbol,
                resolved_timeframe.as_deref(),
                limit,
            )?;
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
                "timeframe": resolved_timeframe,
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
                if let Some(ref tf) = resolved_timeframe {
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

        _ => bail!(
            "unknown predict action '{}'. Valid: add, list, score, score-batch, stats, scorecard",
            action
        ),
    }

    Ok(())
}

/// Score multiple predictions at once. Each entry is `id:outcome` (e.g. `3:correct 7:wrong 12:partial`).
pub fn run_score_batch(
    backend: &BackendConnection,
    entries: &[String],
    json_output: bool,
) -> Result<()> {
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for entry in entries {
        let parts: Vec<&str> = entry.splitn(2, ':').collect();
        if parts.len() != 2 {
            let err_msg = format!(
                "invalid entry '{}'. Expected format: id:outcome (e.g. 3:correct)",
                entry
            );
            if json_output {
                errors.push(serde_json::json!({
                    "entry": entry,
                    "error": err_msg,
                }));
            } else {
                eprintln!("Error: {}", err_msg);
            }
            continue;
        }

        let id_str = parts[0].trim();
        let outcome = parts[1].trim();

        let id = match id_str.parse::<i64>() {
            Ok(v) => v,
            Err(_) => {
                let err_msg = format!("invalid prediction id '{}' in entry '{}'", id_str, entry);
                if json_output {
                    errors.push(serde_json::json!({
                        "entry": entry,
                        "error": err_msg,
                    }));
                } else {
                    eprintln!("Error: {}", err_msg);
                }
                continue;
            }
        };

        if let Err(e) = validate_outcome(outcome) {
            if json_output {
                errors.push(serde_json::json!({
                    "entry": entry,
                    "error": e.to_string(),
                }));
            } else {
                eprintln!("Error scoring #{}: {}", id, e);
            }
            continue;
        }

        match user_predictions::score_prediction_backend(backend, id, outcome, None, None) {
            Ok(()) => {
                if json_output {
                    results.push(serde_json::json!({
                        "id": id,
                        "outcome": outcome,
                        "status": "scored",
                    }));
                } else {
                    println!("Scored prediction #{} as {}", id, outcome);
                }
            }
            Err(e) => {
                if json_output {
                    errors.push(serde_json::json!({
                        "entry": entry,
                        "id": id,
                        "error": e.to_string(),
                    }));
                } else {
                    eprintln!("Error scoring #{}: {}", id, e);
                }
            }
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "scored": results,
                "errors": errors,
                "total_scored": results.len(),
                "total_errors": errors.len(),
            }))?
        );
    } else if results.is_empty() && errors.is_empty() {
        println!("No entries to score.");
    } else {
        println!(
            "\nBatch complete: {} scored, {} errors",
            results.len(),
            errors.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_alias_short_to_low() {
        assert_eq!(resolve_timeframe_alias("short"), "low");
    }

    #[test]
    fn resolve_alias_long_to_high() {
        assert_eq!(resolve_timeframe_alias("long"), "high");
    }

    #[test]
    fn resolve_alias_medium_passthrough() {
        assert_eq!(resolve_timeframe_alias("medium"), "medium");
    }

    #[test]
    fn resolve_alias_macro_passthrough() {
        assert_eq!(resolve_timeframe_alias("macro"), "macro");
    }

    #[test]
    fn resolve_alias_canonical_low_passthrough() {
        assert_eq!(resolve_timeframe_alias("low"), "low");
    }

    #[test]
    fn resolve_alias_canonical_high_passthrough() {
        assert_eq!(resolve_timeframe_alias("high"), "high");
    }

    #[test]
    fn normalize_short_resolves_to_low() {
        let result = normalize_timeframe("short").unwrap();
        assert_eq!(result, "low");
    }

    #[test]
    fn normalize_long_resolves_to_high() {
        let result = normalize_timeframe("long").unwrap();
        assert_eq!(result, "high");
    }

    #[test]
    fn normalize_canonical_values_pass() {
        for tf in &["low", "medium", "high", "macro"] {
            let result = normalize_timeframe(tf).unwrap();
            assert_eq!(result, *tf);
        }
    }

    #[test]
    fn normalize_invalid_timeframe_errors() {
        let result = normalize_timeframe("weekly");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid timeframe"));
        assert!(msg.contains("aliases: short=low, long=high"));
    }

    #[test]
    fn validate_timeframe_rejects_aliases_directly() {
        // validate_timeframe only accepts canonical; aliases go through normalize_timeframe
        assert!(validate_timeframe("short").is_err());
        assert!(validate_timeframe("long").is_err());
    }

    #[test]
    fn validate_timeframe_accepts_canonical() {
        assert!(validate_timeframe("low").is_ok());
        assert!(validate_timeframe("medium").is_ok());
        assert!(validate_timeframe("high").is_ok());
        assert!(validate_timeframe("macro").is_ok());
    }
}
