use anyhow::{bail, Result};
use chrono::{Duration, FixedOffset, Local, NaiveDate, Offset, Utc};
use regex::Regex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::prediction_falsification_rules::{self, PredictionFalsificationRule};
use crate::db::prediction_lessons;
use crate::db::price_cache::get_cached_price_backend;
use crate::db::price_history::get_price_at_date_backend;
use crate::db::user_predictions;

const LOW_PREDICTION_CAP_PER_HOUR: usize = 5;

#[derive(Debug, Clone, Serialize)]
struct ScorecardLessonCoverageRow {
    id: i64,
    claim: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeframe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scored_at: Option<String>,
    has_lesson: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lesson_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lesson_command: Option<String>,
}

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
        "low" | "medium" | "high" | "macro" | "macro-checkpoint" => Ok(()),
        _ => bail!(
            "invalid timeframe '{}'. Valid: low, medium, high, macro, macro-checkpoint (aliases: short=low, long=high). Use --timeframe <value> or positional shorthand after the claim.",
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

fn parse_lessons_applied_arg(value: Option<&str>) -> Result<Vec<i64>> {
    let Some(raw) = value else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    for token in raw
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let token = token.trim_start_matches('#');
        let id: i64 = token.parse().map_err(|_| {
            anyhow::anyhow!(
                "invalid lesson id '{}'. Use comma-separated numeric IDs, e.g. --lessons 218,240",
                token
            )
        })?;
        if id <= 0 {
            bail!("invalid lesson id '{}'. Lesson IDs must be positive", token);
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn parse_date_filter(value: Option<&str>) -> Result<Option<NaiveDate>> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let normalized = raw.trim().to_lowercase();
    let today = Local::now().date_naive();
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

fn should_apply_low_prediction_cap(timeframe: Option<&str>, source_agent: Option<&str>) -> bool {
    matches!(timeframe, Some("low"))
        && source_agent
            .map(user_predictions::is_low_analyst_agent)
            .unwrap_or(false)
}

fn enforce_low_prediction_cap(
    backend: &BackendConnection,
    timeframe: Option<&str>,
    source_agent: Option<&str>,
    override_cap: bool,
) -> Result<()> {
    if override_cap || !should_apply_low_prediction_cap(timeframe, source_agent) {
        return Ok(());
    }

    let recent = user_predictions::count_recent_low_analyst_predictions_backend(backend)?;
    if recent >= LOW_PREDICTION_CAP_PER_HOUR {
        bail!(
            "LOW prediction cap reached: low analyst has already written {} predictions in the last hour (cap {}). Use --override-cap only for an exceptional high-mechanism call.",
            recent,
            LOW_PREDICTION_CAP_PER_HOUR
        );
    }

    Ok(())
}

/// Confidence ceiling applied to predictions without a machine-parseable
/// falsification rule. An unfalsifiable claim cannot be mechanically scored,
/// so its stated confidence is capped to keep the calibration loop honest.
pub const UNFALSIFIABLE_CONFIDENCE_CAP: f64 = 0.3;

/// Margin added to the trailing calibration hit rate when clamping
/// over-confident predictions at write time.
pub const CALIBRATION_CAP_MARGIN: f64 = 0.15;

/// Minimum scored sample size before the calibration matrix is allowed to
/// clamp stated confidence.
pub const CALIBRATION_CAP_MIN_N: i64 = 8;

/// Confidence ceiling while the predicting layer has an ACTIVE forecast
/// misalignment on the prediction's symbol (`forecast_misalignments`): a
/// layer currently on a ≥5 wrong-sign streak does not get to assert high
/// confidence on the same asset. Composes with the other caps — the most
/// restrictive wins.
pub const MISALIGNMENT_CONFIDENCE_CAP: f64 = 0.25;

/// Structured result of parsing a `--falsify` rule string.
///
/// Grammar (deterministic, no LLM):
///   `<SYMBOL> <verb> <comparator> <value> [<value2>] by <YYYY-MM-DD>`
/// with verb ∈ {close, closes, stays, prints} and comparator ∈
/// {above, below, between, in-range, in-band}.
///
/// `--falsify` encodes THE CLAIM'S SUCCESS CONDITION: the condition that,
/// if met, scores the prediction CORRECT.
/// - `close-*` / `prints-*`: at least ONE daily close beyond the threshold
///   inside the evaluation window scores CORRECT (prints-* rules are
///   evaluated against daily closes because intraday data is unavailable).
/// - `stays-*`: EVERY daily close inside the window must satisfy the
///   condition; the rule scores CORRECT only after the window expires clean,
///   and WRONG immediately on the first violating close.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedFalsifyRule {
    pub rule_type: String,
    pub asset: String,
    pub threshold_value: Option<f64>,
    pub threshold_low: Option<f64>,
    pub threshold_high: Option<f64>,
    pub eval_date_end: String,
}

fn parse_falsify_number(token: &str) -> Option<f64> {
    let cleaned = token.replace(',', "");
    let value: f64 = cleaned.parse().ok()?;
    if value.is_finite() {
        Some(value)
    } else {
        None
    }
}

/// Deterministic parser for `--falsify` rule strings. See
/// [`ParsedFalsifyRule`] for the grammar.
pub fn parse_falsify_rule(raw: &str) -> Result<ParsedFalsifyRule> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.len() < 5 {
        bail!(
            "falsify rule too short: expected '<SYMBOL> <verb> <comparator> <value> [<value2>] by <YYYY-MM-DD>', got '{}'",
            raw
        );
    }

    let asset = tokens[0].to_string();
    if parse_falsify_number(&asset).is_some() {
        bail!(
            "falsify rule must start with an asset symbol, got numeric token '{}'",
            asset
        );
    }

    let verb = match tokens[1].to_ascii_lowercase().as_str() {
        "close" | "closes" => "close",
        "stays" => "stays",
        "prints" => "prints",
        other => bail!(
            "unknown falsify verb '{}'. Expected close, closes, stays, or prints",
            other
        ),
    };

    let comparator = tokens[2].to_ascii_lowercase();
    let (needs_two, suffix) = match comparator.as_str() {
        "above" => (false, "above"),
        "below" => (false, "below"),
        "between" | "in-range" | "in-band" => (true, "range"),
        other => bail!(
            "unknown falsify comparator '{}'. Expected above, below, between, in-range, or in-band",
            other
        ),
    };

    let rule_type = match (verb, suffix) {
        ("close", "above") => "close-above",
        ("close", "below") => "close-below",
        ("close", "range") => "close-between",
        ("stays", "above") => "stays-above",
        ("stays", "below") => "stays-below",
        ("stays", "range") => "stays-in-range",
        ("prints", "above") => "prints-above",
        ("prints", "below") => "prints-below",
        ("prints", "range") => "prints-in-band",
        _ => unreachable!("verb and comparator are validated above"),
    };

    let value_count = if needs_two { 2 } else { 1 };
    let expected_len = 3 + value_count + 2; // tokens + 'by' + date
    if tokens.len() != expected_len {
        bail!(
            "falsify rule '{}' has {} tokens; expected {} for '{} {}' form",
            raw,
            tokens.len(),
            expected_len,
            verb,
            comparator
        );
    }

    let first = parse_falsify_number(tokens[3])
        .ok_or_else(|| anyhow::anyhow!("falsify threshold '{}' is not a number", tokens[3]))?;
    let (threshold_value, threshold_low, threshold_high) = if needs_two {
        let second = parse_falsify_number(tokens[4])
            .ok_or_else(|| anyhow::anyhow!("falsify threshold '{}' is not a number", tokens[4]))?;
        let (low, high) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        (None, Some(low), Some(high))
    } else {
        (Some(first), None, None)
    };

    let by_idx = 3 + value_count;
    if !tokens[by_idx].eq_ignore_ascii_case("by") {
        bail!(
            "falsify rule must end with 'by <YYYY-MM-DD>', found '{}' instead of 'by'",
            tokens[by_idx]
        );
    }
    let end = NaiveDate::parse_from_str(tokens[by_idx + 1], "%Y-%m-%d").map_err(|_| {
        anyhow::anyhow!(
            "falsify rule deadline '{}' is not a valid YYYY-MM-DD date",
            tokens[by_idx + 1]
        )
    })?;

    Ok(ParsedFalsifyRule {
        rule_type: rule_type.to_string(),
        asset,
        threshold_value,
        threshold_low,
        threshold_high,
        eval_date_end: end.format("%Y-%m-%d").to_string(),
    })
}

/// Bucket a conviction value into a calibration band. Accepts both the
/// textual conviction used by `user_predictions` (low/medium/high) and the
/// numeric -5..=+5 conviction used by analyst views (|c| <= 1 → low,
/// 2-3 → medium, 4-5 → high).
pub fn conviction_band(conviction: Option<&str>) -> &'static str {
    let raw = match conviction {
        Some(v) => v.trim(),
        None => return "medium",
    };
    if let Ok(value) = raw.parse::<f64>() {
        let magnitude = value.abs();
        return if magnitude <= 1.0 {
            "low"
        } else if magnitude <= 3.0 {
            "medium"
        } else {
            "high"
        };
    }
    match raw.to_ascii_lowercase().as_str() {
        "low" | "weak" => "low",
        "high" | "strong" => "high",
        _ => "medium",
    }
}

/// Trailing calibration evidence for one (layer, topic, conviction band)
/// cell of `calibration_matrix`.
#[derive(Debug, Clone, Copy)]
pub struct CalibrationCapEvidence {
    pub n_scored: i64,
    pub hit_rate: f64,
}

/// Tolerant read of the calibration matrix cell for (layer, topic, band).
/// Handles both the current shape (`conviction_band`, `n`, `hit_rate`) and
/// legacy/alternate column names (`conviction`, `n_scored`,
/// `partial_credit_rate`). Returns Ok(None) when the table or cell is
/// missing so write paths never fail because calibration data is absent.
fn calibration_matrix_cell(
    conn: &rusqlite::Connection,
    layer: &str,
    topic: &str,
    band: &str,
) -> Result<Option<CalibrationCapEvidence>> {
    let mut stmt = match conn.prepare("PRAGMA table_info('calibration_matrix')") {
        Ok(stmt) => stmt,
        Err(_) => return Ok(None),
    };
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|row| row.ok())
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    let pick = |candidates: &[&str]| -> Option<String> {
        candidates
            .iter()
            .find(|c| names.iter().any(|n| n == **c))
            .map(|c| (*c).to_string())
    };
    let (Some(layer_col), Some(band_col), Some(n_col), Some(rate_col)) = (
        pick(&["layer", "timeframe"]),
        pick(&["conviction_band", "conviction"]),
        pick(&["n", "n_scored", "n_total"]),
        pick(&["hit_rate", "partial_credit_rate"]),
    ) else {
        return Ok(None);
    };
    let topic_col = pick(&["topic"]);

    let mut sql = format!(
        "SELECT {n_col}, {rate_col} FROM calibration_matrix
         WHERE {layer_col} = ?1 AND {band_col} = ?2"
    );
    if topic_col.is_some() {
        sql.push_str(" AND topic = ?3");
    }
    sql.push_str(" LIMIT 1");

    let mut stmt = conn.prepare(&sql)?;
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(CalibrationCapEvidence {
            n_scored: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            hit_rate: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
        })
    };
    let cell = if topic_col.is_some() {
        stmt.query_row(rusqlite::params![layer, band, topic], mapper)
    } else {
        stmt.query_row(rusqlite::params![layer, band], mapper)
    };
    match cell {
        Ok(evidence) => Ok(Some(evidence)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn extract_date(raw: &str) -> Option<NaiveDate> {
    timestamp_date_in_timezone(raw, local_fixed_offset())
}

fn local_fixed_offset() -> FixedOffset {
    Local::now().offset().fix()
}

fn timestamp_date_in_timezone(raw: &str, offset: FixedOffset) -> Option<NaiveDate> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&offset).date_naive());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z") {
        return Some(dt.with_timezone(&offset).date_naive());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z") {
        return Some(dt.with_timezone(&offset).date_naive());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
                .with_timezone(&offset)
                .date_naive(),
        );
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Some(
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
                .with_timezone(&offset)
                .date_naive(),
        );
    }
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
    lessons_applied: Option<&str>,
    outcome: Option<&str>,
    notes: Option<&str>,
    lesson: Option<&str>,
    filter: Option<&str>,
    date: Option<&str>,
    limit: Option<usize>,
    lesson_coverage: bool,
    topic: Option<&str>,
    source_article_id: Option<i64>,
    override_cap: bool,
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
            enforce_low_prediction_cap(
                backend,
                resolved_timeframe.as_deref(),
                source_agent,
                override_cap,
            )?;
            let lessons_applied = parse_lessons_applied_arg(lessons_applied)?;
            let new_id = user_predictions::add_prediction_backend_with_details(
                backend,
                claim,
                symbol,
                conviction,
                resolved_timeframe.as_deref(),
                confidence,
                source_agent,
                target_date,
                resolution_criteria,
                &lessons_applied,
                topic,
                source_article_id,
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
            let resolved_timeframe = match timeframe {
                Some(tf) => Some(normalize_timeframe(tf)?),
                None => None,
            };
            let stats = user_predictions::get_stats_filtered_backend(
                backend,
                resolved_timeframe.as_deref(),
                source_agent,
            )?;
            if json_output {
                // Add filter metadata to JSON output
                let mut val = serde_json::to_value(&stats)?;
                if let serde_json::Value::Object(ref mut map) = val {
                    if let Some(ref tf) = resolved_timeframe {
                        map.insert(
                            "filter_timeframe".to_string(),
                            serde_json::Value::String(tf.clone()),
                        );
                    }
                    if let Some(agent) = source_agent {
                        map.insert(
                            "filter_agent".to_string(),
                            serde_json::Value::String(agent.to_string()),
                        );
                    }
                }
                println!("{}", serde_json::to_string_pretty(&val)?);
            } else {
                let mut header = String::from("Prediction stats");
                if resolved_timeframe.is_some() || source_agent.is_some() {
                    header.push_str(" (filtered:");
                    if let Some(ref tf) = resolved_timeframe {
                        header.push_str(&format!(" timeframe={}", tf));
                    }
                    if let Some(agent) = source_agent {
                        header.push_str(&format!(" agent={}", agent));
                    }
                    header.push(')');
                }
                println!("{}:", header);
                println!("  Total: {}", stats.total);
                println!("  Scored: {}", stats.scored);
                println!("  Pending: {}", stats.pending);
                println!("  Correct: {}", stats.correct);
                println!("  Partial: {}", stats.partial);
                println!("  Wrong: {}", stats.wrong);
                println!("  Hit rate: {:.1}%", stats.hit_rate_pct);

                if !stats.by_timeframe.is_empty() {
                    println!("\n  By timeframe:");
                    let mut tf_entries: Vec<_> = stats.by_timeframe.iter().collect();
                    tf_entries.sort_by_key(|(k, _)| match k.as_str() {
                        "low" => 0,
                        "medium" => 1,
                        "high" => 2,
                        "macro-checkpoint" => 3,
                        "macro" => 4,
                        _ => 5,
                    });
                    for (tf, s) in &tf_entries {
                        println!(
                            "    {:<16} — {}/{} scored, {:.1}% hit rate ({} correct, {} partial, {} wrong)",
                            tf, s.scored, s.total, s.hit_rate_pct, s.correct, s.partial, s.wrong
                        );
                    }
                }

                if !stats.by_source_agent.is_empty() {
                    println!("\n  By agent:");
                    let mut agent_entries: Vec<_> = stats.by_source_agent.iter().collect();
                    agent_entries.sort_by_key(|(a, _)| *a);
                    for (agent, s) in &agent_entries {
                        println!(
                            "    {:<20} — {}/{} scored, {:.1}% hit rate ({} correct, {} partial, {} wrong)",
                            agent, s.scored, s.total, s.hit_rate_pct, s.correct, s.partial, s.wrong
                        );
                    }
                }

                if !stats.by_conviction.is_empty() {
                    println!("\n  By conviction:");
                    let mut conv_entries: Vec<_> = stats.by_conviction.iter().collect();
                    conv_entries.sort_by_key(|(k, _)| match k.as_str() {
                        "low" => 0,
                        "medium" => 1,
                        "high" => 2,
                        _ => 3,
                    });
                    for (conv, s) in &conv_entries {
                        println!(
                            "    {:<8} — {}/{} scored, {:.1}% hit rate ({} correct, {} partial, {} wrong)",
                            conv, s.scored, s.total, s.hit_rate_pct, s.correct, s.partial, s.wrong
                        );
                    }
                }

                if !stats.by_symbol.is_empty() {
                    println!("\n  By symbol (top 10):");
                    let mut sym_entries: Vec<_> = stats.by_symbol.iter().collect();
                    sym_entries.sort_by_key(|(_, b)| std::cmp::Reverse(b.total));
                    for (sym, s) in sym_entries.iter().take(10) {
                        println!(
                            "    {:<10} — {}/{} scored, {:.1}% hit rate",
                            sym, s.scored, s.total, s.hit_rate_pct
                        );
                    }
                }
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
            let lesson_coverage_rows = if lesson_coverage {
                Some(build_scorecard_lesson_coverage(backend, &rows)?)
            } else {
                None
            };

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
                "wrong_with_lesson": wrong.saturating_sub(wrong_without_lesson),
                "wrong_without_lesson": wrong_without_lesson,
                "lesson_coverage_pct": if wrong > 0 {
                    ((wrong.saturating_sub(wrong_without_lesson)) as f64 / wrong as f64) * 100.0
                } else {
                    0.0
                },
                "lesson_coverage": lesson_coverage,
                "wrong_predictions": lesson_coverage_rows,
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
                let wrong_with_lesson = wrong.saturating_sub(wrong_without_lesson);
                let lesson_coverage_pct = if wrong > 0 {
                    wrong_with_lesson as f64 / wrong as f64 * 100.0
                } else {
                    0.0
                };
                println!(
                    "  Lesson coverage: {}/{} wrong calls ({:.1}%)",
                    wrong_with_lesson, wrong, lesson_coverage_pct
                );
                println!("  Wrong calls missing lesson: {}", wrong_without_lesson);
                if let Some(rows) = lesson_coverage_rows.as_ref() {
                    let missing = rows.iter().filter(|row| !row.has_lesson).count();
                    if !rows.is_empty() {
                        println!("  Lesson coverage for wrong calls:");
                        for row in rows.iter().take(10) {
                            let symbol = row.symbol.as_deref().unwrap_or("—");
                            let status = if row.has_lesson {
                                format!(
                                    "[lesson:{}]",
                                    row.lesson_type.as_deref().unwrap_or("present")
                                )
                            } else {
                                "[no lesson]".to_string()
                            };
                            println!(
                                "    #{} [{}] {} {}",
                                row.id,
                                symbol,
                                truncate_claim(&row.claim, 55),
                                status
                            );
                            if let Some(command) = row.lesson_command.as_deref() {
                                println!("      add: {}", command);
                            }
                        }
                        if rows.len() > 10 {
                            println!("    ... and {} more", rows.len() - 10);
                        }
                        println!("  Wrong calls still missing lesson: {}", missing);
                    }
                }
            }
        }

        _ => bail!(
            "unknown predict action '{}'. Valid: add, list, score, score-batch, stats, scorecard",
            action
        ),
    }

    Ok(())
}

fn build_scorecard_lesson_coverage(
    backend: &BackendConnection,
    rows: &[user_predictions::UserPrediction],
) -> Result<Vec<ScorecardLessonCoverageRow>> {
    let lesson_views = prediction_lessons::list_lesson_views_backend(backend, None, None)?;
    let lesson_map = lesson_views
        .into_iter()
        .filter_map(|view| view.lesson.map(|lesson| (view.prediction_id, lesson)))
        .collect::<std::collections::HashMap<_, _>>();

    let mut items: Vec<_> = rows
        .iter()
        .filter(|row| row.outcome == "wrong")
        .map(|row| {
            let lesson = lesson_map.get(&row.id);
            ScorecardLessonCoverageRow {
                id: row.id,
                claim: row.claim.clone(),
                symbol: row.symbol.clone(),
                timeframe: row.timeframe.clone(),
                source_agent: row.source_agent.clone(),
                scored_at: row.scored_at.clone(),
                has_lesson: lesson.is_some(),
                lesson_type: lesson.map(|lesson| lesson.miss_type.clone()),
                lesson_command: lesson.is_none().then(|| lesson_add_command(row.id)),
            }
        })
        .collect();

    items.sort_by(|a, b| {
        let a_rank = if a.has_lesson { 1 } else { 0 };
        let b_rank = if b.has_lesson { 1 } else { 0 };
        a_rank.cmp(&b_rank).then_with(|| {
            b.scored_at
                .as_deref()
                .unwrap_or("")
                .cmp(a.scored_at.as_deref().unwrap_or(""))
        })
    });

    Ok(items)
}

fn lesson_add_command(prediction_id: i64) -> String {
    format!(
        "pftui journal prediction lessons add --prediction-id {} --miss-type timing --what-happened \"...\" --why-wrong \"...\"",
        prediction_id
    )
}

fn truncate_claim(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        value.to_string()
    } else {
        format!("{}...", &value[..max_len])
    }
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

/// Direction parsed from a prediction claim.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PriceDirection {
    Above,
    Below,
}

/// A parsed price-direction prediction extracted from the claim text.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ParsedPricePrediction {
    symbol: String,
    direction: PriceDirection,
    target_price: Decimal,
}

/// Known symbol aliases for matching prediction claims to price_history/price_cache symbols.
#[allow(dead_code)]
fn resolve_symbol_alias(token: &str) -> Option<&'static str> {
    match token.to_uppercase().as_str() {
        "BTC" | "BITCOIN" => Some("BTC-USD"),
        "ETH" | "ETHEREUM" => Some("ETH-USD"),
        "SOL" | "SOLANA" => Some("SOL-USD"),
        "GOLD" | "XAUUSD" => Some("GC=F"),
        "SILVER" | "XAGUSD" => Some("SI=F"),
        "DXY" | "DOLLAR" => Some("DX-Y.NYB"),
        "SPY" | "S&P" | "SP500" | "S&P500" => Some("SPY"),
        "OIL" | "CRUDE" | "WTI" => Some("CL=F"),
        "VIX" => Some("^VIX"),
        "NASDAQ" | "QQQ" => Some("QQQ"),
        _ => None,
    }
}

/// Attempt to parse a price-direction prediction from the claim text.
///
/// Matches patterns like:
///   "BTC above $70K by ..."
///   "Gold below 2000 by ..."
///   "TSLA above 250.50 by ..."
///   "BTC > $100,000 by ..."
///   "ETH >= 4000"
#[allow(dead_code)]
fn parse_price_prediction(
    claim: &str,
    prediction_symbol: Option<&str>,
) -> Option<ParsedPricePrediction> {
    let claim_lower = claim.to_lowercase();

    // Pattern: <SYMBOL> (above|below|>|<|>=|<=) $?<PRICE>[K|k|M|m]
    // The suffix [KkMmBb] must immediately follow the number (no whitespace).
    let re = Regex::new(
        r"(?i)\b([A-Za-z][A-Za-z0-9&]{0,10})\s+(?:(above|over|>|>=)\s*\$?\s*([\d,]+(?:\.\d+)?)([kmb])?(?:\s|$|[^a-z0-9])|(below|under|<|<=)\s*\$?\s*([\d,]+(?:\.\d+)?)([kmb])?(?:\s|$|[^a-z0-9]))"
    ).ok()?;

    if let Some(caps) = re.captures(claim) {
        let symbol_token = caps.get(1)?.as_str();

        // Determine the actual ticker symbol
        let resolved_symbol = if let Some(alias) = resolve_symbol_alias(symbol_token) {
            alias.to_string()
        } else if prediction_symbol.is_some() {
            prediction_symbol?.to_string()
        } else {
            // Use the raw token as-is (could be a ticker like TSLA)
            symbol_token.to_uppercase()
        };

        // Parse direction and price
        let (direction, price_str, suffix) = if caps.get(2).is_some() {
            (
                PriceDirection::Above,
                caps.get(3)?.as_str(),
                caps.get(4).map(|m| m.as_str()),
            )
        } else {
            (
                PriceDirection::Below,
                caps.get(6)?.as_str(),
                caps.get(7).map(|m| m.as_str()),
            )
        };

        let clean_price = price_str.replace(',', "");
        let mut price: Decimal = clean_price.parse().ok()?;

        // Apply suffix multiplier
        match suffix.map(|s| s.to_lowercase()).as_deref() {
            Some("k") => price *= Decimal::from(1_000),
            Some("m") => price *= Decimal::from(1_000_000),
            Some("b") => price *= Decimal::from(1_000_000_000),
            _ => {}
        }

        return Some(ParsedPricePrediction {
            symbol: resolved_symbol,
            direction,
            target_price: price,
        });
    }

    // Also try pattern with claim_lower for "X reaches/hits Y" → treat as "above"
    let re2 = Regex::new(
        r"(?i)\b([A-Za-z][A-Za-z0-9&]{0,10})\s+(?:reaches?|hits?|to|at)\s+\$?\s*([\d,]+(?:\.\d+)?)([kmb])?(?:\s|$|[^a-z0-9])"
    ).ok()?;

    if let Some(caps) = re2.captures(claim) {
        let symbol_token = caps.get(1)?.as_str();
        let resolved_symbol = if let Some(alias) = resolve_symbol_alias(symbol_token) {
            alias.to_string()
        } else if prediction_symbol.is_some() {
            prediction_symbol?.to_string()
        } else {
            symbol_token.to_uppercase()
        };

        let clean_price = caps.get(2)?.as_str().replace(',', "");
        let mut price: Decimal = clean_price.parse().ok()?;
        match caps.get(3).map(|m| m.as_str().to_lowercase()).as_deref() {
            Some("k") => price *= Decimal::from(1_000),
            Some("m") => price *= Decimal::from(1_000_000),
            Some("b") => price *= Decimal::from(1_000_000_000),
            _ => {}
        }

        // "reaches/hits/to" implies price should be AT or ABOVE
        return Some(ParsedPricePrediction {
            symbol: resolved_symbol,
            direction: PriceDirection::Above,
            target_price: price,
        });
    }

    // If no pattern matched in claim but we have a symbol and the claim mentions a dollar amount
    if let Some(sym) = prediction_symbol {
        let price_re = Regex::new(r"\$\s*([\d,]+(?:\.\d+)?)([kmb])?(?:\s|$|[^a-z0-9])").ok()?;
        if let Some(caps) = price_re.captures(claim) {
            let clean_price = caps.get(1)?.as_str().replace(',', "");
            let mut price: Decimal = clean_price.parse().ok()?;
            match caps.get(2).map(|m| m.as_str().to_lowercase()).as_deref() {
                Some("k") => price *= Decimal::from(1_000),
                Some("m") => price *= Decimal::from(1_000_000),
                Some("b") => price *= Decimal::from(1_000_000_000),
                _ => {}
            }
            let direction = if claim_lower.contains("above")
                || claim_lower.contains("over")
                || claim_lower.contains("reach")
                || claim_lower.contains("hit")
                || claim_lower.contains("bull")
            {
                PriceDirection::Above
            } else if claim_lower.contains("below")
                || claim_lower.contains("under")
                || claim_lower.contains("bear")
                || claim_lower.contains("drop")
                || claim_lower.contains("fall")
            {
                PriceDirection::Below
            } else {
                return None; // Ambiguous direction
            };

            return Some(ParsedPricePrediction {
                symbol: resolve_symbol_alias(sym)
                    .map(String::from)
                    .unwrap_or_else(|| sym.to_string()),
                direction,
                target_price: price,
            });
        }
    }

    None
}

/// Get the current price for a symbol. Tries price_cache first, then latest price_history.
#[allow(dead_code)]
fn get_current_price(backend: &BackendConnection, symbol: &str) -> Option<Decimal> {
    // Try price cache first (most recent)
    if let Ok(Some(quote)) = get_cached_price_backend(backend, symbol, "USD") {
        if quote.price > Decimal::ZERO {
            return Some(quote.price);
        }
    }

    // Fallback: latest price_history entry
    let today = Utc::now().format("%Y-%m-%d").to_string();
    if let Ok(Some(price)) = get_price_at_date_backend(backend, symbol, &today) {
        if price > Decimal::ZERO {
            return Some(price);
        }
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoScoreConfidenceFloor {
    Medium,
    High,
}

impl AutoScoreConfidenceFloor {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => bail!(
                "invalid confidence floor '{}'. Expected medium or high",
                other
            ),
        }
    }

    fn allows(self, confidence: &str) -> bool {
        let rank = match confidence.trim().to_ascii_lowercase().as_str() {
            "high" => 2,
            "medium" => 1,
            _ => 0,
        };
        match self {
            Self::Medium => rank >= 1,
            Self::High => rank >= 2,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AutoScoreResult {
    id: i64,
    rule_id: i64,
    claim: String,
    symbol: Option<String>,
    series: String,
    rule_type: String,
    eval_date_start: Option<String>,
    eval_date_end: String,
    threshold: String,
    observed: String,
    outcome: String,
    note: String,
}

#[derive(Debug, Clone, Serialize)]
struct AutoScoreFailure {
    id: i64,
    rule_id: i64,
    rule_type: String,
    reason: String,
    detail: String,
}

#[derive(Debug, Clone)]
struct RuleDecision {
    outcome: &'static str,
    observed: Decimal,
    threshold: String,
    series: String,
    evidence: String,
}

/// Counts returned to non-CLI callers (e.g. the `data refresh` tail step).
#[derive(Debug, Clone, Default, Serialize)]
pub struct AutoScoreSummary {
    pub scored: usize,
    pub correct: usize,
    pub wrong: usize,
    pub skipped: usize,
    pub failures: usize,
}

struct AutoScoreRun {
    scoreable: Vec<AutoScoreResult>,
    skipped: Vec<serde_json::Value>,
    failures: Vec<AutoScoreFailure>,
    total_rules: usize,
}

impl AutoScoreRun {
    fn summary(&self) -> AutoScoreSummary {
        AutoScoreSummary {
            scored: self.scoreable.len(),
            correct: self
                .scoreable
                .iter()
                .filter(|r| r.outcome == "correct")
                .count(),
            wrong: self
                .scoreable
                .iter()
                .filter(|r| r.outcome == "wrong")
                .count(),
            skipped: self.skipped.len(),
            failures: self.failures.len(),
        }
    }
}

/// Evaluate falsification rules against daily closes and (unless dry_run)
/// write decided outcomes. Rules whose window is still open and undecided
/// are skipped without error. Already-scored predictions are never
/// overwritten unless `force` is set.
fn compute_auto_score(
    backend: &BackendConnection,
    since: Option<&str>,
    dry_run: bool,
    confidence_floor: &str,
    force: bool,
) -> Result<AutoScoreRun> {
    let today = Utc::now().date_naive();
    if let Some(since) = since {
        NaiveDate::parse_from_str(since, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("--since must be YYYY-MM-DD"))?;
    }
    let floor = AutoScoreConfidenceFloor::parse(confidence_floor)?;

    prediction_falsification_rules::ensure_table_backend(backend)?;
    let rules =
        prediction_falsification_rules::list_active_auto_score_rules_backend(backend, since)?;

    let mut scoreable: Vec<AutoScoreResult> = Vec::new();
    let mut skipped: Vec<serde_json::Value> = Vec::new();
    let mut failures: Vec<AutoScoreFailure> = Vec::new();

    for rule in &rules {
        if !floor.allows(&rule.parse_confidence) {
            skipped.push(json!({
                "id": rule.prediction_id,
                "rule_id": rule.id,
                "reason": "below_confidence_floor",
                "parse_confidence": rule.parse_confidence,
                "confidence_floor": confidence_floor,
            }));
            continue;
        }
        if rule.current_outcome != "pending" && !force {
            skipped.push(json!({
                "id": rule.prediction_id,
                "rule_id": rule.id,
                "reason": "already_scored",
                "outcome": rule.current_outcome,
            }));
            continue;
        }

        match evaluate_falsification_rule(backend, rule, today) {
            Ok(Some(decision)) => {
                let note = format!(
                    "auto-scored: {} — {} [series {}]",
                    restate_rule(rule, &decision.threshold),
                    decision.evidence,
                    decision.series,
                );
                scoreable.push(AutoScoreResult {
                    id: rule.prediction_id,
                    rule_id: rule.id,
                    claim: rule.claim.clone(),
                    symbol: rule
                        .symbol
                        .clone()
                        .or_else(|| rule.prediction_symbol.clone()),
                    series: decision.series,
                    rule_type: rule.rule_type.clone(),
                    eval_date_start: rule.eval_date_start.clone(),
                    eval_date_end: rule.eval_date_end.clone(),
                    threshold: decision.threshold,
                    observed: decision.observed.to_string(),
                    outcome: decision.outcome.to_string(),
                    note,
                });
            }
            Ok(None) => {
                skipped.push(json!({
                    "id": rule.prediction_id,
                    "rule_id": rule.id,
                    "reason": "window_open_undecided",
                    "eval_date_end": rule.eval_date_end,
                }));
            }
            Err(err) => {
                failures.push(AutoScoreFailure {
                    id: rule.prediction_id,
                    rule_id: rule.id,
                    rule_type: rule.rule_type.clone(),
                    reason: "evaluation_failed".to_string(),
                    detail: err.to_string(),
                });
            }
        }
    }

    if !dry_run {
        for result in &scoreable {
            user_predictions::score_prediction_backend(
                backend,
                result.id,
                &result.outcome,
                Some(&result.note),
                None,
            )?;
        }
    }

    Ok(AutoScoreRun {
        scoreable,
        skipped,
        failures,
        total_rules: rules.len(),
    })
}

/// Auto-score entry point for `pftui data refresh`: scores due rules with
/// default settings and returns counts for a one-line summary.
pub fn auto_score_for_refresh(backend: &BackendConnection) -> Result<AutoScoreSummary> {
    Ok(compute_auto_score(backend, None, false, "medium", false)?.summary())
}

/// Auto-score pending predictions from their falsification rules.
/// `close-*`/`prints-*` rules score CORRECT on the first qualifying daily
/// close inside the window and WRONG once the window expires without one;
/// `stays-*` rules score WRONG on the first violating close and CORRECT
/// only after the window expires clean.
pub fn run_auto_score(
    backend: &BackendConnection,
    since: Option<&str>,
    dry_run: bool,
    confidence_floor: &str,
    force: bool,
    json_output: bool,
) -> Result<()> {
    let run = compute_auto_score(backend, since, dry_run, confidence_floor, force)?;
    let summary = run.summary();
    let AutoScoreRun {
        scoreable,
        skipped,
        failures,
        total_rules,
    } = run;

    if json_output {
        let payload = json!({
            "dry_run": dry_run,
            "confidence_floor": confidence_floor,
            "force": force,
            "scored": scoreable,
            "scored_count": scoreable.len(),
            "correct_count": summary.correct,
            "wrong_count": summary.wrong,
            "skipped": skipped,
            "skipped_count": skipped.len(),
            "failures": failures,
            "failure_count": failures.len(),
            "total_rules": total_rules,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if scoreable.is_empty() {
        println!("No predictions eligible for auto-scoring.");
        if !skipped.is_empty() {
            println!("  {} rule(s) skipped.", skipped.len());
        }
        if !failures.is_empty() {
            println!("  {} rule(s) failed evaluation.", failures.len());
        }
    } else {
        let action = if dry_run {
            "Would auto-score"
        } else {
            "Auto-scored"
        };
        println!("{} {} prediction(s):", action, scoreable.len());
        for r in &scoreable {
            println!(
                "  #{} [{}] {} -> {} (observed: {}, threshold: {}, series: {})",
                r.id,
                r.rule_type,
                if r.claim.len() > 50 {
                    format!("{}...", &r.claim[..47])
                } else {
                    r.claim.clone()
                },
                r.outcome,
                r.observed,
                r.threshold,
                r.series,
            );
        }
        println!(
            "  Summary: {} scored ({} correct / {} wrong).",
            summary.scored, summary.correct, summary.wrong
        );
        if !skipped.is_empty() {
            println!("  {} rule(s) skipped.", skipped.len());
        }
        if !failures.is_empty() {
            println!("  {} rule(s) failed evaluation.", failures.len());
        }
    }

    Ok(())
}

/// Human-readable restatement of the success condition a rule encodes.
fn restate_rule(rule: &PredictionFalsificationRule, threshold: &str) -> String {
    let symbol = rule
        .symbol
        .as_deref()
        .or(rule.prediction_symbol.as_deref())
        .unwrap_or("?");
    let window = match rule.eval_date_start.as_deref() {
        Some(start) => format!("{}..{}", start, rule.eval_date_end),
        None => format!("by {}", rule.eval_date_end),
    };
    format!(
        "{} {} {} within {}",
        rule.rule_type, symbol, threshold, window
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleCondition {
    Above,
    Below,
    InRange,
}

fn evaluate_falsification_rule(
    backend: &BackendConnection,
    rule: &PredictionFalsificationRule,
    today: NaiveDate,
) -> Result<Option<RuleDecision>> {
    // prints-* rules are evaluated against daily closes: intraday data is
    // not available, so a "print" is approximated by a close.
    match rule.rule_type.as_str() {
        "close-above" | "prints-above" => {
            evaluate_breach_rule(backend, rule, today, RuleCondition::Above)
        }
        "close-below" | "prints-below" => {
            evaluate_breach_rule(backend, rule, today, RuleCondition::Below)
        }
        "close-between" | "prints-in-band" => {
            evaluate_breach_rule(backend, rule, today, RuleCondition::InRange)
        }
        "stays-above" => evaluate_stays_rule(backend, rule, today, RuleCondition::Above),
        "stays-below" => evaluate_stays_rule(backend, rule, today, RuleCondition::Below),
        "stays-in-range" => evaluate_stays_rule(backend, rule, today, RuleCondition::InRange),
        "correlation-above" | "correlation-below" => {
            bail!("unsupported_data_source: correlation rule evaluation is not implemented")
        }
        other => bail!("unsupported_rule_type: {}", other),
    }
}

struct RuleWindow<'a> {
    start: &'a str,
    end_capped: String,
    expired: bool,
}

fn rule_window(rule: &PredictionFalsificationRule, today: NaiveDate) -> RuleWindow<'_> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let expired = today_str.as_str() > rule.eval_date_end.as_str();
    let end_capped = if expired {
        rule.eval_date_end.clone()
    } else {
        today_str
    };
    RuleWindow {
        start: rule
            .eval_date_start
            .as_deref()
            .unwrap_or(&rule.eval_date_end),
        end_capped,
        expired,
    }
}

fn condition_bounds(
    rule: &PredictionFalsificationRule,
    condition: RuleCondition,
) -> Result<(Decimal, Option<Decimal>, String)> {
    match condition {
        RuleCondition::Above | RuleCondition::Below => {
            let threshold = decimal_threshold(rule)?;
            let symbol = if condition == RuleCondition::Above {
                ">"
            } else {
                "<"
            };
            Ok((threshold, None, format!("{} {}", symbol, threshold)))
        }
        RuleCondition::InRange => {
            let low = decimal_from_f64(rule.threshold_low, "threshold_low")?;
            let high = decimal_from_f64(rule.threshold_high, "threshold_high")?;
            Ok((low, Some(high), format!("{}..{}", low, high)))
        }
    }
}

fn close_satisfies(
    condition: RuleCondition,
    close: Decimal,
    low: Decimal,
    high: Option<Decimal>,
) -> bool {
    match condition {
        RuleCondition::Above => close > low,
        RuleCondition::Below => close < low,
        RuleCondition::InRange => {
            let high = high.unwrap_or(low);
            close >= low && close <= high
        }
    }
}

/// `close-*` / `prints-*` semantics: the prediction is CORRECT as soon as at
/// least one daily close inside the evaluation window satisfies the success
/// condition; WRONG once the window has expired without one; undecided
/// (None) while the window is still open.
fn evaluate_breach_rule(
    backend: &BackendConnection,
    rule: &PredictionFalsificationRule,
    today: NaiveDate,
    condition: RuleCondition,
) -> Result<Option<RuleDecision>> {
    let symbol = rule_symbol(rule)?;
    let (low, high, threshold_desc) = condition_bounds(rule, condition)?;
    let window = rule_window(rule, today);
    let Some((series, rows)) =
        load_series_window(backend, &symbol, window.start, &window.end_capped)?
    else {
        if window.expired {
            bail!(
                "missing_price_history: {} between {} and {}",
                symbol,
                window.start,
                window.end_capped
            );
        }
        return Ok(None);
    };

    if let Some((date, close)) = rows
        .iter()
        .find(|(_, close)| close_satisfies(condition, *close, low, high))
    {
        return Ok(Some(RuleDecision {
            outcome: "correct",
            observed: *close,
            threshold: threshold_desc,
            series,
            evidence: format!("{} close {} met the success condition", date, close),
        }));
    }

    if window.expired {
        // Decided wrong: report the close that came nearest to qualifying.
        let (date, close) = match condition {
            RuleCondition::Below => rows
                .iter()
                .min_by(|a, b| a.1.cmp(&b.1))
                .cloned()
                .unwrap_or_default(),
            _ => rows
                .iter()
                .max_by(|a, b| a.1.cmp(&b.1))
                .cloned()
                .unwrap_or_default(),
        };
        return Ok(Some(RuleDecision {
            outcome: "wrong",
            observed: close,
            threshold: threshold_desc,
            series,
            evidence: format!(
                "window expired {}; nearest close {} on {} never met the success condition",
                rule.eval_date_end, close, date
            ),
        }));
    }

    Ok(None)
}

/// `stays-*` semantics: every daily close inside the window must satisfy
/// the condition. WRONG immediately on the first violating close; CORRECT
/// only once the window has expired clean; undecided (None) while open.
fn evaluate_stays_rule(
    backend: &BackendConnection,
    rule: &PredictionFalsificationRule,
    today: NaiveDate,
    condition: RuleCondition,
) -> Result<Option<RuleDecision>> {
    let symbol = rule_symbol(rule)?;
    let (low, high, threshold_desc) = condition_bounds(rule, condition)?;
    let window = rule_window(rule, today);
    let Some((series, rows)) =
        load_series_window(backend, &symbol, window.start, &window.end_capped)?
    else {
        if window.expired {
            bail!(
                "missing_price_history: {} between {} and {}",
                symbol,
                window.start,
                window.end_capped
            );
        }
        return Ok(None);
    };

    if let Some((date, close)) = rows
        .iter()
        .find(|(_, close)| !close_satisfies(condition, *close, low, high))
    {
        return Ok(Some(RuleDecision {
            outcome: "wrong",
            observed: *close,
            threshold: threshold_desc,
            series,
            evidence: format!("{} close {} violated the stays condition", date, close),
        }));
    }

    if window.expired {
        // Window closed with every close satisfying the condition.
        let (date, close) = match condition {
            RuleCondition::Above => rows
                .iter()
                .min_by(|a, b| a.1.cmp(&b.1))
                .cloned()
                .unwrap_or_default(),
            _ => rows
                .iter()
                .max_by(|a, b| a.1.cmp(&b.1))
                .cloned()
                .unwrap_or_default(),
        };
        return Ok(Some(RuleDecision {
            outcome: "correct",
            observed: close,
            threshold: threshold_desc,
            series,
            evidence: format!(
                "window expired {} with every close satisfying the condition (tightest close {} on {})",
                rule.eval_date_end, close, date
            ),
        }));
    }

    Ok(None)
}

fn rule_symbol(rule: &PredictionFalsificationRule) -> Result<String> {
    rule.symbol
        .as_deref()
        .or(rule.prediction_symbol.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing_symbol"))
}

fn decimal_threshold(rule: &PredictionFalsificationRule) -> Result<Decimal> {
    decimal_from_f64(
        rule.threshold_value
            .or(rule.threshold_high)
            .or(rule.threshold_low),
        "threshold_value",
    )
}

fn decimal_from_f64(value: Option<f64>, field: &str) -> Result<Decimal> {
    let value = value.ok_or_else(|| anyhow::anyhow!("missing_{}", field))?;
    Decimal::from_f64_retain(value).ok_or_else(|| anyhow::anyhow!("invalid_{}", field))
}

/// Candidate price series for a rule symbol, in preference order. The rule's
/// own symbol is canonical; the `-USD` suffixed (or de-suffixed) alias is a
/// fallback for crypto series stored under either convention.
fn series_candidates(symbol: &str) -> Vec<String> {
    let mut candidates = vec![symbol.to_string()];
    if let Some(base) = symbol.strip_suffix("-USD") {
        candidates.push(base.to_string());
    } else {
        candidates.push(format!("{symbol}-USD"));
    }
    candidates
}

/// (resolved series symbol, ordered (date, close) rows) for a rule window.
type SeriesWindowRows = (String, Vec<(String, Decimal)>);

/// Load (date, close) rows for the first candidate series with coverage in
/// the window. Returns None when no candidate has any rows.
fn load_series_window(
    backend: &BackendConnection,
    symbol: &str,
    start: &str,
    end: &str,
) -> Result<Option<SeriesWindowRows>> {
    for candidate in series_candidates(symbol) {
        let rows = price_history_rows_backend(backend, &candidate, start, end)?;
        if !rows.is_empty() {
            return Ok(Some((candidate, rows)));
        }
    }
    Ok(None)
}

fn price_history_rows_backend(
    backend: &BackendConnection,
    symbol: &str,
    start: &str,
    end: &str,
) -> Result<Vec<(String, Decimal)>> {
    crate::db::query::dispatch(
        backend,
        |conn| {
            let mut stmt = conn.prepare(
                "SELECT date, close FROM price_history
                 WHERE symbol = ?1 AND date >= ?2 AND date <= ?3
                 ORDER BY date ASC",
            )?;
            let rows = stmt.query_map(rusqlite::params![symbol, start, end], |row| {
                let date: String = row.get(0)?;
                let close: String = row.get(1)?;
                Ok((date, close.parse::<Decimal>().unwrap_or(Decimal::ZERO)))
            })?;
            Ok(rows.filter_map(|row| row.ok()).collect())
        },
        |pool| {
            let rows: Vec<(String, String)> = crate::db::pg_runtime::block_on(async {
                sqlx::query_as(
                    "SELECT date, close FROM price_history
                     WHERE symbol = $1 AND date >= $2 AND date <= $3
                     ORDER BY date ASC",
                )
                .bind(symbol)
                .bind(start)
                .bind(end)
                .fetch_all(pool)
                .await
            })?;
            Ok(rows
                .into_iter()
                .map(|(date, close)| (date, close.parse::<Decimal>().unwrap_or(Decimal::ZERO)))
                .collect())
        },
    )
}

/// List wrong predictions with structured lessons (or mark which lack lessons).
pub fn run_lessons(
    backend: &BackendConnection,
    miss_type: Option<&str>,
    unresolved_only: bool,
    limit: Option<usize>,
    include_retired: bool,
    json_output: bool,
) -> Result<()> {
    use crate::db::prediction_lessons;

    // Validate miss_type if provided
    if let Some(mt) = miss_type {
        prediction_lessons::validate_miss_type_str(mt)?;
    }

    // Pull a larger backend window when filtering; truncate to the
    // requested limit after the active/unresolved filter is applied so the
    // half-life retired rows do not consume the cap.
    let backend_limit = if unresolved_only || !include_retired {
        None
    } else {
        limit
    };
    let mut views =
        prediction_lessons::list_lesson_views_backend(backend, miss_type, backend_limit)?;
    if !include_retired {
        // Keep rows that either have no lesson (so unresolved can still
        // surface them) OR have an active lesson.
        views.retain(|v| match v.lesson.as_ref() {
            Some(lesson) => lesson.status == prediction_lessons::STATUS_ACTIVE,
            None => true,
        });
    }
    filter_lesson_views(&mut views, unresolved_only);
    if let Some(limit) = limit {
        views.truncate(limit);
    }
    let (total_wrong, with_lessons) = prediction_lessons::lesson_coverage_backend(backend)?;
    let unresolved_count = total_wrong.saturating_sub(with_lessons);

    if json_output {
        let json_views: Vec<serde_json::Value> = views
            .iter()
            .map(|v| {
                let mut obj = serde_json::json!({
                    "prediction_id": v.prediction_id,
                    "claim": v.claim,
                    "symbol": v.symbol,
                    "conviction": v.conviction,
                    "timeframe": v.timeframe,
                    "confidence": v.confidence,
                    "source_agent": v.source_agent,
                    "target_date": v.target_date,
                    "outcome": v.outcome,
                    "score_notes": v.score_notes,
                    "created_at": v.created_at,
                    "scored_at": v.scored_at,
                    "has_lesson": v.lesson.is_some(),
                });
                if let Some(ref lesson) = v.lesson {
                    obj["lesson"] = serde_json::json!({
                        "id": lesson.id,
                        "miss_type": lesson.miss_type,
                        "what_predicted": lesson.what_predicted,
                        "what_happened": lesson.what_happened,
                        "why_wrong": lesson.why_wrong,
                        "signal_misread": lesson.signal_misread,
                        "created_at": lesson.created_at,
                    });
                }
                obj
            })
            .collect();

        let output = serde_json::json!({
            "total_wrong": total_wrong,
            "with_lessons": with_lessons,
            "without_lessons": unresolved_count,
            "unresolved_only": unresolved_only,
            "coverage_pct": if total_wrong > 0 {
                (with_lessons as f64 / total_wrong as f64 * 100.0).round()
            } else {
                0.0
            },
            "predictions": json_views,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Prediction Lessons — {}/{} wrong predictions have lessons ({:.0}% coverage)\n",
            with_lessons,
            total_wrong,
            if total_wrong > 0 {
                with_lessons as f64 / total_wrong as f64 * 100.0
            } else {
                0.0
            }
        );
        if unresolved_only {
            println!(
                "Showing only unresolved backlog ({} without lessons)\n",
                unresolved_count
            );
        }

        if views.is_empty() {
            if unresolved_only {
                println!("No wrong predictions without lessons found.");
            } else {
                println!("No wrong predictions found.");
            }
            return Ok(());
        }

        for v in &views {
            let symbol_str = v
                .symbol
                .as_deref()
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default();
            let agent_str = v
                .source_agent
                .as_deref()
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();

            println!(
                "#{}{}{} — {} ({})",
                v.prediction_id, symbol_str, agent_str, v.claim, v.conviction
            );

            if let Some(ref lesson) = v.lesson {
                println!("  Miss type:      {}", lesson.miss_type);
                println!("  What happened:  {}", lesson.what_happened);
                println!("  Why wrong:      {}", lesson.why_wrong);
                if let Some(ref signal) = lesson.signal_misread {
                    println!("  Signal misread: {}", signal);
                }
            } else {
                println!("  ⚠ No lesson extracted yet");
            }
            println!();
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BulkLessonInput {
    prediction_id: i64,
    miss_type: String,
    what_happened: String,
    why_wrong: String,
    signal_misread: Option<String>,
}

#[derive(Debug, Serialize)]
struct BulkLessonStub {
    prediction_id: i64,
    claim: String,
    symbol: Option<String>,
    source_agent: Option<String>,
    scored_at: Option<String>,
    age_days: i64,
    stub: BulkLessonInput,
    root_cause: String,
    going_forward: String,
}

#[derive(Debug, Serialize)]
struct BulkLessonResult {
    prediction_id: i64,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    lesson_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

fn filter_lesson_views(
    views: &mut Vec<crate::db::prediction_lessons::PredictionLessonView>,
    unresolved_only: bool,
) {
    if unresolved_only {
        views.retain(|view| view.lesson.is_none());
        views.sort_by(|a, b| lesson_backlog_sort_key(a).cmp(&lesson_backlog_sort_key(b)));
    }
}

fn lesson_backlog_sort_key(
    view: &crate::db::prediction_lessons::PredictionLessonView,
) -> (&str, i64) {
    (
        view.scored_at.as_deref().unwrap_or(&view.created_at),
        view.prediction_id,
    )
}

fn parse_bulk_lessons_input(raw: &str) -> Result<Vec<BulkLessonInput>> {
    let parsed: Vec<BulkLessonInput> = serde_json::from_str(raw).map_err(|err| {
        anyhow::anyhow!(
            "invalid bulk lessons JSON: {}. Expected an array of {{prediction_id, miss_type, what_happened, why_wrong, signal_misread?}} objects",
            err
        )
    })?;
    if parsed.is_empty() {
        bail!("bulk lessons input is empty");
    }
    Ok(parsed)
}

fn score_timestamp(prediction: &user_predictions::UserPrediction) -> &str {
    prediction
        .scored_at
        .as_deref()
        .unwrap_or(&prediction.created_at)
}

fn stub_miss_type(prediction: &user_predictions::UserPrediction) -> &'static str {
    if prediction.target_date.is_some() {
        "timing"
    } else {
        "directional"
    }
}

fn stub_what_happened(prediction: &user_predictions::UserPrediction) -> String {
    let mut parts = vec![format!(
        "Prediction was scored wrong on {}.",
        score_timestamp(prediction)
    )];
    if let Some(symbol) = prediction.symbol.as_deref() {
        parts.push(format!("Symbol: {}.", symbol));
    }
    if let Some(notes) = prediction
        .score_notes
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("Outcome notes: {}.", notes.trim()));
    } else {
        parts.push("Replace with the actual market outcome that invalidated the call.".to_string());
    }
    parts.join(" ")
}

fn stub_why_wrong() -> String {
    "Root cause: <fill in why the call failed>. Going forward: <fill in what changes next time>."
        .to_string()
}

fn stub_signal_misread(prediction: &user_predictions::UserPrediction) -> Option<String> {
    prediction
        .resolution_criteria
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("Re-check resolution criteria: {}", value.trim()))
}

fn prediction_age_days(prediction: &user_predictions::UserPrediction) -> i64 {
    let timestamp = score_timestamp(prediction);
    timestamp_date_in_timezone(timestamp, local_fixed_offset())
        .map(|date| {
            Local::now()
                .date_naive()
                .signed_duration_since(date)
                .num_days()
        })
        .unwrap_or(0)
}

fn build_bulk_lesson_stubs(
    predictions: Vec<user_predictions::UserPrediction>,
) -> Vec<BulkLessonStub> {
    let mut items: Vec<_> = predictions
        .into_iter()
        .map(|prediction| BulkLessonStub {
            prediction_id: prediction.id,
            claim: prediction.claim.clone(),
            symbol: prediction.symbol.clone(),
            source_agent: prediction.source_agent.clone(),
            scored_at: prediction.scored_at.clone(),
            age_days: prediction_age_days(&prediction),
            stub: BulkLessonInput {
                prediction_id: prediction.id,
                miss_type: stub_miss_type(&prediction).to_string(),
                what_happened: stub_what_happened(&prediction),
                why_wrong: stub_why_wrong(),
                signal_misread: stub_signal_misread(&prediction),
            },
            root_cause: "<fill in why the call failed>".to_string(),
            going_forward: "<fill in what changes next time>".to_string(),
        })
        .collect();
    items.sort_by(|a, b| {
        a.scored_at
            .as_deref()
            .unwrap_or("")
            .cmp(b.scored_at.as_deref().unwrap_or(""))
            .then_with(|| a.prediction_id.cmp(&b.prediction_id))
    });
    items
}

pub fn run_bulk_lessons(
    backend: &BackendConnection,
    input_path: Option<&str>,
    auto_stub: bool,
    unresolved_only: bool,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    use crate::db::prediction_lessons;
    use crate::db::user_predictions;

    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    let lesson_views = prediction_lessons::list_lesson_views_backend(backend, None, None)?;

    let prediction_map: std::collections::HashMap<i64, _> = predictions
        .into_iter()
        .map(|prediction| (prediction.id, prediction))
        .collect();
    let existing_lessons: std::collections::HashSet<i64> = lesson_views
        .into_iter()
        .filter(|view| view.lesson.is_some())
        .map(|view| view.prediction_id)
        .collect();

    if input_path.is_none() {
        let mut backlog: Vec<_> = prediction_map
            .values()
            .filter(|prediction| prediction.outcome == "wrong")
            .filter(|prediction| !existing_lessons.contains(&prediction.id))
            .cloned()
            .collect();
        backlog.sort_by(|a, b| {
            score_timestamp(a)
                .cmp(score_timestamp(b))
                .then_with(|| a.id.cmp(&b.id))
        });

        let stubs = auto_stub.then(|| build_bulk_lesson_stubs(backlog.clone()));

        if json_output {
            let payload = if let Some(stubs) = stubs {
                json!({
                    "mode": "auto_stub",
                    "total": backlog.len(),
                    "predictions": stubs,
                })
            } else {
                json!({
                    "mode": "backlog",
                    "total": backlog.len(),
                    "predictions": backlog.iter().map(|prediction| json!({
                        "prediction_id": prediction.id,
                        "claim": prediction.claim,
                        "symbol": prediction.symbol,
                        "source_agent": prediction.source_agent,
                        "scored_at": prediction.scored_at,
                        "created_at": prediction.created_at,
                        "age_days": prediction_age_days(prediction),
                    })).collect::<Vec<_>>(),
                })
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else if let Some(stubs) = stubs {
            println!(
                "Prediction lesson backlog: {} wrong predictions without lessons (oldest first)\n",
                stubs.len()
            );
            for stub in &stubs {
                let symbol = stub.symbol.as_deref().unwrap_or("—");
                let agent = stub.source_agent.as_deref().unwrap_or("unknown");
                let scored_at = stub.scored_at.as_deref().unwrap_or("unknown");
                println!(
                    "#{} [{}] {} | agent={} | scored={} | age={}d",
                    stub.prediction_id, symbol, stub.claim, agent, scored_at, stub.age_days
                );
                println!("  stub:");
                println!("    prediction_id: {}", stub.stub.prediction_id);
                println!("    miss_type: {}", stub.stub.miss_type);
                println!("    what_happened: {}", stub.stub.what_happened);
                println!("    why_wrong: {}", stub.stub.why_wrong);
                if let Some(signal_misread) = stub.stub.signal_misread.as_deref() {
                    println!("    signal_misread: {}", signal_misread);
                }
                println!("  fill root_cause: {}", stub.root_cause);
                println!("  fill going_forward: {}", stub.going_forward);
                println!();
            }
        } else {
            println!(
                "Prediction lesson backlog: {} wrong predictions without lessons (oldest first)\n",
                backlog.len()
            );
            for prediction in &backlog {
                let symbol = prediction.symbol.as_deref().unwrap_or("—");
                let agent = prediction.source_agent.as_deref().unwrap_or("unknown");
                let scored_at = prediction
                    .scored_at
                    .as_deref()
                    .unwrap_or(&prediction.created_at);
                println!(
                    "#{} [{}] {} | agent={} | scored={} | age={}d",
                    prediction.id,
                    symbol,
                    prediction.claim,
                    agent,
                    scored_at,
                    prediction_age_days(prediction)
                );
            }
        }
        return Ok(());
    }

    let input_path = input_path.expect("checked above");
    let raw = std::fs::read_to_string(input_path)
        .map_err(|err| anyhow::anyhow!("failed to read '{}': {}", input_path, err))?;
    let entries = parse_bulk_lessons_input(&raw)?;
    let mut results = Vec::new();

    for entry in entries {
        prediction_lessons::validate_miss_type_str(&entry.miss_type)?;

        let Some(prediction) = prediction_map.get(&entry.prediction_id) else {
            results.push(BulkLessonResult {
                prediction_id: entry.prediction_id,
                status: "skipped".to_string(),
                lesson_id: None,
                reason: Some("prediction not found".to_string()),
            });
            continue;
        };

        if prediction.outcome != "wrong" {
            results.push(BulkLessonResult {
                prediction_id: entry.prediction_id,
                status: "skipped".to_string(),
                lesson_id: None,
                reason: Some(format!(
                    "prediction outcome is '{}', not 'wrong'",
                    prediction.outcome
                )),
            });
            continue;
        }

        if unresolved_only && existing_lessons.contains(&entry.prediction_id) {
            results.push(BulkLessonResult {
                prediction_id: entry.prediction_id,
                status: "skipped".to_string(),
                lesson_id: None,
                reason: Some("prediction already has a lesson".to_string()),
            });
            continue;
        }

        if dry_run {
            results.push(BulkLessonResult {
                prediction_id: entry.prediction_id,
                status: "dry_run".to_string(),
                lesson_id: None,
                reason: None,
            });
            continue;
        }

        let lesson_id = prediction_lessons::add_lesson_backend(
            backend,
            entry.prediction_id,
            &entry.miss_type,
            &prediction.claim,
            &entry.what_happened,
            &entry.why_wrong,
            entry.signal_misread.as_deref(),
        )?;
        results.push(BulkLessonResult {
            prediction_id: entry.prediction_id,
            status: "added".to_string(),
            lesson_id: Some(lesson_id),
            reason: None,
        });
    }

    let added = results
        .iter()
        .filter(|result| result.status == "added")
        .count();
    let skipped = results
        .iter()
        .filter(|result| result.status == "skipped")
        .count();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "input_path": input_path,
                "auto_stub": auto_stub,
                "dry_run": dry_run,
                "unresolved_only": unresolved_only,
                "added": added,
                "skipped": skipped,
                "results": results,
            }))?
        );
    } else {
        println!(
            "Bulk prediction lessons: {} added, {} skipped{}",
            added,
            skipped,
            if dry_run { " (dry run)" } else { "" }
        );
        for result in &results {
            match result.status.as_str() {
                "added" => println!(
                    "  added   #{} -> lesson #{}",
                    result.prediction_id,
                    result.lesson_id.unwrap_or_default()
                ),
                "dry_run" => println!("  dry-run #{}", result.prediction_id),
                _ => println!(
                    "  skipped #{} ({})",
                    result.prediction_id,
                    result.reason.as_deref().unwrap_or("unknown reason")
                ),
            }
        }
    }

    Ok(())
}

/// Add a structured lesson for a wrong prediction.
pub fn run_add_lesson(
    backend: &BackendConnection,
    prediction_id: i64,
    miss_type: &str,
    what_happened: &str,
    why_wrong: &str,
    signal_misread: Option<&str>,
    json_output: bool,
) -> Result<()> {
    use crate::db::prediction_lessons;
    use crate::db::user_predictions;

    // Validate miss_type
    prediction_lessons::validate_miss_type_str(miss_type)?;

    // Look up the prediction to get what_predicted (the claim) and verify it's wrong
    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    let prediction = predictions
        .iter()
        .find(|p| p.id == prediction_id)
        .ok_or_else(|| anyhow::anyhow!("Prediction #{} not found", prediction_id))?;

    if prediction.outcome != "wrong" {
        bail!(
            "Prediction #{} has outcome '{}', not 'wrong'. Lessons can only be added to wrong predictions.",
            prediction_id,
            prediction.outcome
        );
    }

    let what_predicted = &prediction.claim;

    let id = prediction_lessons::add_lesson_backend(
        backend,
        prediction_id,
        miss_type,
        what_predicted,
        what_happened,
        why_wrong,
        signal_misread,
    )?;

    if json_output {
        let output = serde_json::json!({
            "id": id,
            "prediction_id": prediction_id,
            "miss_type": miss_type,
            "what_predicted": what_predicted,
            "what_happened": what_happened,
            "why_wrong": why_wrong,
            "signal_misread": signal_misread,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Added lesson for prediction #{} (miss type: {})",
            prediction_id, miss_type
        );
    }

    Ok(())
}

/// Run the pre-flight check without persisting a prediction. Implements the
/// `pftui journal prediction preflight` CLI surface.
#[allow(clippy::too_many_arguments)]
pub fn run_preflight(
    backend: &BackendConnection,
    claim: &str,
    symbol: Option<&str>,
    timeframe: Option<&str>,
    conviction: Option<&str>,
    layer: Option<&str>,
    topic: Option<&str>,
    inline: bool,
    json_output: bool,
) -> Result<()> {
    let conn = backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("prediction preflight requires the SQLite backend"))?;
    let resolved_timeframe = match timeframe {
        Some(tf) => Some(normalize_timeframe(tf)?),
        None => None,
    };
    if let Some(c) = conviction {
        validate_conviction(c)?;
    }
    let resolved_layer = layer.map(|s| s.to_string()).or_else(|| resolved_timeframe.clone());
    let draft = crate::db::preflight::PreflightDraft {
        claim: claim.to_string(),
        symbol: symbol.map(|s| s.to_string()),
        timeframe: resolved_timeframe,
        conviction: conviction.map(|s| s.to_string()),
        layer: resolved_layer,
        topic: topic.map(|s| s.to_string()),
    };
    let findings = crate::db::preflight::compute_preflight(conn, &draft)?;
    if inline && !json_output {
        println!("{}", findings.inline_summary());
        return Ok(());
    }
    if json_output {
        println!("{}", serde_json::to_string_pretty(&findings)?);
        return Ok(());
    }
    render_preflight_pretty(&findings);
    Ok(())
}

fn render_preflight_pretty(findings: &crate::db::preflight::PreflightFindings) {
    println!("Preflight check");
    println!("  claim: {}", findings.draft.claim);
    if let Some(c) = &findings.cluster_key {
        println!("  cluster_key: {}", c);
    } else {
        println!("  cluster_key: <unclassified>");
    }
    println!(
        "  preflight_score: {}/100 (higher = riskier)",
        findings.preflight_score
    );
    if !findings.risk_factors.is_empty() {
        println!("  risk_factors:");
        for f in &findings.risk_factors {
            println!("    - {}", f);
        }
    }
    if let Some(adj) = &findings.calibration_adjustment {
        println!(
            "  calibration: layer={} topic={} conviction={} direction={} adjustment={:+.0}pp ({}/{} scored, raw_hit_rate={:.0}%)",
            adj.layer,
            adj.topic,
            adj.conviction,
            adj.adjustment_direction,
            adj.adjustment_pp,
            adj.n_scored,
            adj.n_scored,
            adj.raw_hit_rate * 100.0,
        );
        if !adj.apply_note.is_empty() {
            println!("    note: {}", adj.apply_note);
        }
    }
    if let Some(stats) = &findings.cluster_hit_stats {
        println!(
            "  cluster_hit_rate: {:.0}% ({}/{} scored, n_total={})",
            stats.hit_rate_pct, stats.n_correct, stats.n_scored, stats.n_total
        );
    }
    if !findings.reasoning_fragments.is_empty() {
        println!("  reasoning_fragments:");
        for f in &findings.reasoning_fragments {
            println!(
                "    - {} [{}] topic={} conf={}",
                f.canonical_id, f.fragment_type, f.topic, f.confidence
            );
        }
    }
    if let Some(co) = &findings.top_co_failing_cluster {
        println!(
            "  top_co_failing_cluster: {} <-> {} share={:.0}% (window={}d, n={})",
            co.cluster_a,
            co.cluster_b,
            co.co_wrong_share * 100.0,
            co.window_days,
            co.co_wrong_count,
        );
    }
    if !findings.similar_predictions.is_empty() {
        println!("  similar_predictions (top {}):", findings.similar_predictions.len());
        for p in &findings.similar_predictions {
            println!(
                "    - #{} [{}|{}] {}",
                p.id,
                p.symbol.clone().unwrap_or_else(|| "—".into()),
                p.outcome,
                p.claim
            );
        }
    }
    if !findings.scenario_link_distribution.is_empty() {
        println!("  scenario_link_distribution:");
        for s in &findings.scenario_link_distribution {
            let label = s
                .scenario_name
                .clone()
                .unwrap_or_else(|| format!("scenario#{}", s.scenario_id));
            println!("    - {}: {} links", label, s.link_count);
        }
    }
    if let Some(rule) = &findings.similar_falsification_rule {
        println!(
            "  most_similar_falsification_rule: id={} prediction={} type={} similarity={:.2}",
            rule.id, rule.prediction_id, rule.rule_type, rule.similarity_score
        );
        println!("    excerpt: {}", rule.claim_excerpt);
    }
    if !findings.thesis_chains.is_empty() {
        println!("  thesis_chains (touching this symbol):");
        for c in &findings.thesis_chains {
            println!(
                "    - #{} [{}] {} --{}--> {}",
                c.id, c.current_state, c.antecedent_text, c.relation, c.consequent_text,
            );
        }
    }
}

/// Add a prediction with the auto-preflight gate. Wraps the legacy add path
/// in `run` so existing callers without preflight semantics continue to work.
#[allow(clippy::too_many_arguments)]
pub fn run_add_with_preflight(
    backend: &BackendConnection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
    lessons_applied: Option<&str>,
    topic: Option<&str>,
    source_article_id: Option<i64>,
    override_cap: bool,
    layer: Option<&str>,
    skip_preflight: bool,
    accept_preflight: bool,
    inline: bool,
    preflight_threshold: Option<u32>,
    with_adversary: bool,
    falsify: Option<&str>,
    override_confidence_cap: bool,
    cap_rationale: Option<&str>,
    json_output: bool,
) -> Result<()> {
    // Validate inputs early so callers see the same errors regardless of
    // preflight pathing.
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
    if override_confidence_cap && cap_rationale.map(|r| r.trim().is_empty()).unwrap_or(true) {
        bail!("--override-confidence-cap requires --cap-rationale \"<text>\" explaining why the calibration clamp does not apply");
    }

    // ── Falsification rule parse (deterministic grammar, no LLM) ──────
    let today_str = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let falsify_parse: Option<std::result::Result<ParsedFalsifyRule, String>> =
        falsify.map(|raw| parse_falsify_rule(raw).map_err(|e| e.to_string()));
    let falsifiable = matches!(falsify_parse, Some(Ok(_)));

    // ── Write-time confidence discipline ──────────────────────────────
    let mut effective_confidence = confidence;
    let mut confidence_cap_notes: Vec<String> = Vec::new();

    if !falsifiable {
        let reason = match &falsify_parse {
            Some(Err(err)) => format!("--falsify did not parse ({err})"),
            _ => "no --falsify rule supplied".to_string(),
        };
        if let Some(conf) = effective_confidence {
            if conf > UNFALSIFIABLE_CONFIDENCE_CAP {
                effective_confidence = Some(UNFALSIFIABLE_CONFIDENCE_CAP);
                confidence_cap_notes.push(format!(
                    "unfalsifiable prediction — confidence capped at {:.2} (stated {:.2}; {})",
                    UNFALSIFIABLE_CONFIDENCE_CAP, conf, reason
                ));
            }
        }
        if !json_output {
            eprintln!(
                "Warning: unfalsifiable prediction — confidence capped at {:.2} ({}). Supply --falsify \"<SYMBOL> <close|closes|stays|prints> <above|below|between|in-range|in-band> <value> [<value2>] by <YYYY-MM-DD>\" to lift the cap.",
                UNFALSIFIABLE_CONFIDENCE_CAP, reason
            );
        }
    }

    // Calibration-derived clamp: if the trailing scored record for this
    // (layer, topic, conviction band) shows the stated confidence is more
    // than CALIBRATION_CAP_MARGIN above the realized hit rate, clamp it.
    let mut cap_override_note: Option<String> = None;
    if let (Some(conf), Some(conn)) = (effective_confidence, backend.sqlite_native()) {
        let cap_layer = resolved_timeframe.as_deref().unwrap_or("medium");
        let cap_topic = crate::db::news_source_accuracy::normalize_topic(topic)?;
        let band = conviction_band(conviction);
        if let Some(evidence) = calibration_matrix_cell(conn, cap_layer, &cap_topic, band)? {
            let ceiling = evidence.hit_rate + CALIBRATION_CAP_MARGIN;
            if evidence.n_scored >= CALIBRATION_CAP_MIN_N && conf > ceiling {
                if override_confidence_cap {
                    let rationale = cap_rationale.unwrap_or_default().trim().to_string();
                    cap_override_note = Some(format!("[cap-override: {rationale}]"));
                    if !json_output {
                        println!(
                            "Calibration cap overridden: trailing hit rate for ({}, {}, {}) is {:.0}% over {} scored calls (cap would be {:.2}); rationale recorded.",
                            cap_layer,
                            cap_topic,
                            band,
                            evidence.hit_rate * 100.0,
                            evidence.n_scored,
                            ceiling
                        );
                    }
                } else {
                    effective_confidence = Some(ceiling.clamp(0.0, 1.0));
                    confidence_cap_notes.push(format!(
                        "calibration cap: ({}, {}, {}) trailing hit rate {:.0}% over {} scored calls — confidence clamped from {:.2} to {:.2}",
                        cap_layer,
                        cap_topic,
                        band,
                        evidence.hit_rate * 100.0,
                        evidence.n_scored,
                        conf,
                        ceiling.clamp(0.0, 1.0)
                    ));
                    if !json_output {
                        println!(
                            "Calibration cap: trailing hit rate for ({}, {}, {}) is {:.0}% over {} scored calls — confidence clamped from {:.2} to {:.2}. Pass --override-confidence-cap --cap-rationale \"...\" to bypass.",
                            cap_layer,
                            cap_topic,
                            band,
                            evidence.hit_rate * 100.0,
                            evidence.n_scored,
                            conf,
                            ceiling.clamp(0.0, 1.0)
                        );
                    }
                }
            }
        }
    }

    // Misalignment clamp (score-reactive): while the predicting layer has an
    // ACTIVE forecast misalignment on this symbol, confidence is capped at
    // MISALIGNMENT_CONFIDENCE_CAP. Applied after the other caps on the same
    // running `effective_confidence`, so the most restrictive cap wins.
    if let (Some(conf), Some(conn), Some(sym)) =
        (effective_confidence, backend.sqlite_native(), symbol)
    {
        let cap_layer = layer
            .or(resolved_timeframe.as_deref())
            .unwrap_or("medium");
        if let Some(mis) =
            crate::db::forecast_misalignments::active_for_symbol(conn, cap_layer, sym)?
        {
            if conf > MISALIGNMENT_CONFIDENCE_CAP {
                if override_confidence_cap {
                    let rationale = cap_rationale.unwrap_or_default().trim().to_string();
                    let note = format!("[misalignment-cap-override: {rationale}]");
                    cap_override_note = Some(match cap_override_note {
                        Some(existing) => format!("{existing} {note}"),
                        None => note,
                    });
                    if !json_output {
                        println!(
                            "Misalignment cap overridden: {} has {} consecutive wrong-sign {} forecasts ({} → {}, {:+.1}% cumulative against); rationale recorded.",
                            mis.layer,
                            mis.streak_len,
                            mis.asset,
                            mis.span_start,
                            mis.span_end,
                            mis.cum_realized_against_pct
                        );
                    }
                } else {
                    effective_confidence = Some(MISALIGNMENT_CONFIDENCE_CAP);
                    confidence_cap_notes.push(format!(
                        "misalignment cap: {} has {} consecutive wrong-sign {} forecasts ({} → {}, {:+.1}% cumulative against) — confidence capped from {:.2} to {:.2}",
                        mis.layer,
                        mis.streak_len,
                        mis.asset,
                        mis.span_start,
                        mis.span_end,
                        mis.cum_realized_against_pct,
                        conf,
                        MISALIGNMENT_CONFIDENCE_CAP
                    ));
                    if !json_output {
                        println!(
                            "Misalignment cap: {} has {} consecutive wrong-sign {} forecasts ({:+.1}% cumulative against the calls) — confidence capped at {:.2} (stated {:.2}). Pass --override-confidence-cap --cap-rationale \"...\" to bypass.",
                            mis.layer,
                            mis.streak_len,
                            mis.asset,
                            mis.cum_realized_against_pct,
                            MISALIGNMENT_CONFIDENCE_CAP,
                            conf
                        );
                    }
                }
            }
        }
    }
    let confidence = effective_confidence;

    let threshold = preflight_threshold
        .unwrap_or(crate::db::preflight::DEFAULT_PREFLIGHT_ABORT_THRESHOLD);

    let mut inline_block: Option<String> = None;
    if !skip_preflight {
        let conn = backend
            .sqlite_native()
            .ok_or_else(|| anyhow::anyhow!("prediction preflight requires the SQLite backend"))?;
        let resolved_layer = layer.map(|s| s.to_string()).or_else(|| resolved_timeframe.clone());
        let draft = crate::db::preflight::PreflightDraft {
            claim: claim.to_string(),
            symbol: symbol.map(|s| s.to_string()),
            timeframe: resolved_timeframe.clone(),
            conviction: conviction.map(|s| s.to_string()),
            layer: resolved_layer,
            topic: topic.map(|s| s.to_string()),
        };
        let findings = crate::db::preflight::compute_preflight(conn, &draft)?;
        let blocking = findings.is_blocking(threshold);

        if blocking && !accept_preflight {
            if json_output {
                let payload = serde_json::json!({
                    "aborted": true,
                    "preflight_threshold": threshold,
                    "findings": findings,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                eprintln!(
                    "Prediction aborted by preflight (score {} >= threshold {}).",
                    findings.preflight_score, threshold
                );
                eprintln!("Re-run with --accept-preflight to commit, or --skip-preflight to bypass entirely.");
                eprintln!();
                render_preflight_pretty(&findings);
            }
            bail!(
                "preflight blocked save (score {} >= threshold {})",
                findings.preflight_score,
                threshold
            );
        }
        if inline {
            inline_block = Some(findings.inline_summary());
        }
        if !json_output {
            println!(
                "Preflight: cluster={} score={}/100{}",
                findings
                    .cluster_key
                    .clone()
                    .unwrap_or_else(|| "<unclassified>".into()),
                findings.preflight_score,
                if blocking {
                    " (override via --accept-preflight)"
                } else {
                    ""
                }
            );
        }
    }

    // Optional write-time adversary composition. Persisted AFTER the
    // prediction is saved so the `adversary_views.prediction_id` FK lands
    // on a real row. We compute the adversary view here (pre-save) so its
    // short summary can be appended to `resolution_criteria` alongside the
    // preflight inline block, keeping both views part of the prediction's
    // permanent record.
    let mut adversary_view_for_persist: Option<crate::db::adversary::AdversaryView> = None;
    let mut adversary_inline_summary: Option<String> = None;
    if with_adversary {
        let conn = backend
            .sqlite_native()
            .ok_or_else(|| anyhow::anyhow!("--with-adversary requires the SQLite backend"))?;
        let draft = crate::db::adversary::AdversaryDraft {
            claim: claim.to_string(),
            symbol: symbol.map(|s| s.to_string()),
            timeframe: resolved_timeframe.clone(),
            conviction: conviction.map(|s| s.to_string()),
            layer: layer.map(|s| s.to_string()).or_else(|| resolved_timeframe.clone()),
        };
        let view = crate::db::adversary::compose(conn, &draft)?;
        adversary_inline_summary = Some(view.inline_summary());
        adversary_view_for_persist = Some(view);
    }

    let mut resolution_with_inline: Option<String> = match (resolution_criteria, inline_block.as_ref())
    {
        (Some(existing), Some(block)) => Some(format!("{}\n{}", existing, block)),
        (Some(existing), None) => Some(existing.to_string()),
        (None, Some(block)) => Some(block.clone()),
        (None, None) => None,
    };
    if let Some(adv_summary) = adversary_inline_summary.as_ref() {
        resolution_with_inline = Some(match resolution_with_inline {
            Some(existing) => format!("{}\n{}", existing, adv_summary),
            None => adv_summary.clone(),
        });
    }
    if let Some(note) = cap_override_note.as_ref() {
        resolution_with_inline = Some(match resolution_with_inline {
            Some(existing) => format!("{} {}", existing, note),
            None => note.clone(),
        });
    }

    enforce_low_prediction_cap(
        backend,
        resolved_timeframe.as_deref(),
        source_agent,
        override_cap,
    )?;
    let lessons_applied_ids = parse_lessons_applied_arg(lessons_applied)?;
    let new_id = user_predictions::add_prediction_backend_with_details(
        backend,
        claim,
        symbol,
        conviction,
        resolved_timeframe.as_deref(),
        confidence,
        source_agent,
        target_date,
        resolution_with_inline.as_deref(),
        &lessons_applied_ids,
        topic,
        source_article_id,
    )?;

    // ── Persist the falsification rule (or its unstructured fallback) ──
    let mut persisted_falsification: Option<serde_json::Value> = None;
    if let Some(parse_result) = falsify_parse.as_ref() {
        let new_rule = match parse_result {
            Ok(parsed) => crate::db::prediction_falsification_rules::NewFalsificationRule {
                prediction_id: new_id,
                rule_type: parsed.rule_type.clone(),
                symbol: Some(parsed.asset.clone()),
                threshold_value: parsed.threshold_value,
                threshold_low: parsed.threshold_low,
                threshold_high: parsed.threshold_high,
                threshold_text: None,
                eval_date_start: today_str.clone(),
                eval_date_end: parsed.eval_date_end.clone(),
                auto_score_eligible: true,
                parse_confidence: "high".to_string(),
            },
            Err(_) => crate::db::prediction_falsification_rules::NewFalsificationRule {
                prediction_id: new_id,
                rule_type: "unstructured".to_string(),
                symbol: symbol.map(|s| s.to_string()),
                threshold_value: None,
                threshold_low: None,
                threshold_high: None,
                threshold_text: falsify.map(|s| s.to_string()),
                eval_date_start: today_str.clone(),
                eval_date_end: target_date
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| today_str.clone()),
                auto_score_eligible: false,
                parse_confidence: "low".to_string(),
            },
        };
        let rule_id =
            crate::db::prediction_falsification_rules::insert_rule_backend(backend, &new_rule)?;
        persisted_falsification = Some(serde_json::json!({
            "rule_id": rule_id,
            "rule": new_rule,
            "parse_error": parse_result.as_ref().err(),
        }));
        if !json_output {
            match parse_result {
                Ok(parsed) => println!(
                    "  + falsification rule #{} recorded: {} {} (window {}..{}, auto-score eligible)",
                    rule_id, parsed.rule_type, parsed.asset, today_str, parsed.eval_date_end
                ),
                Err(err) => println!(
                    "  + unstructured falsification rule #{} recorded (not auto-scoreable): {}",
                    rule_id, err
                ),
            }
        }
    }

    let mut persisted_adversary_view_id: Option<i64> = None;
    if let Some(view) = adversary_view_for_persist.as_ref() {
        let conn = backend
            .sqlite_native()
            .ok_or_else(|| anyhow::anyhow!("--with-adversary requires the SQLite backend"))?;
        let cluster_key = view
            .cluster_key
            .clone()
            .unwrap_or_else(|| "unclassified".to_string());
        let anti_pattern_args_json = serde_json::to_string(&view.anti_pattern_arguments)?;
        let cofailure_warnings_json = serde_json::to_string(&view.cofailure_warnings)?;
        let falsification_triggers_json = serde_json::to_string(&view.falsification_triggers)?;
        let adversary_id = crate::db::adversary_views::insert(
            conn,
            Some(new_id),
            &cluster_key,
            &anti_pattern_args_json,
            &cofailure_warnings_json,
            &falsification_triggers_json,
        )?;
        persisted_adversary_view_id = Some(adversary_id);
    }

    if json_output {
        let rows = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
        if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
            let payload = serde_json::json!({
                "prediction": row,
                "adversary_view_id": persisted_adversary_view_id,
                "adversary_view": adversary_view_for_persist,
                "falsification": persisted_falsification,
                "confidence_caps_applied": confidence_cap_notes,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    } else {
        println!("Added prediction #{}", new_id);
        for note in &confidence_cap_notes {
            println!("  ! {}", note);
        }
        if let Some(adv_id) = persisted_adversary_view_id {
            println!("  + adversary_view #{} persisted", adv_id);
        }
    }
    Ok(())
}

/// Compose and print the write-time adversary view for the supplied draft.
/// Does not persist — callers that want persistence should use
/// `prediction add --with-adversary`.
#[allow(clippy::too_many_arguments)]
pub fn run_adversary(
    backend: &BackendConnection,
    claim: &str,
    symbol: Option<&str>,
    timeframe: Option<&str>,
    conviction: Option<&str>,
    layer: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("prediction adversary requires the SQLite backend"))?;
    let resolved_timeframe = match timeframe {
        Some(tf) => Some(normalize_timeframe(tf)?),
        None => None,
    };
    if let Some(c) = conviction {
        validate_conviction(c)?;
    }
    let resolved_layer = layer.map(|s| s.to_string()).or_else(|| resolved_timeframe.clone());
    let draft = crate::db::adversary::AdversaryDraft {
        claim: claim.to_string(),
        symbol: symbol.map(|s| s.to_string()),
        timeframe: resolved_timeframe,
        conviction: conviction.map(|s| s.to_string()),
        layer: resolved_layer,
    };
    let view = crate::db::adversary::compose(conn, &draft)?;

    if json_output {
        // Spec: output a single JSON object with the three fields.
        let payload = serde_json::json!({
            "cluster_key": view.cluster_key,
            "anti_pattern_arguments": view.anti_pattern_arguments,
            "cofailure_warnings": view.cofailure_warnings,
            "falsification_triggers": view.falsification_triggers,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    for line in view.pretty_lines() {
        println!("{}", line);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

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
    fn low_prediction_cap_blocks_sixth_low_agent_prediction() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        for i in 0..LOW_PREDICTION_CAP_PER_HOUR {
            user_predictions::add_prediction_backend(
                &backend,
                &format!("LOW setup prediction {}", i + 1),
                None,
                Some("medium"),
                Some("low"),
                Some(0.55),
                Some("low-agent"),
                None,
                None,
                &[],
            )
            .unwrap();
        }

        let blocked = run(
            &backend,
            "add",
            Some("sixth low prediction"),
            None,
            None,
            Some("medium"),
            Some("low"),
            Some(0.6),
            Some("low-agent"),
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
            false,
            false,
        );
        let err = blocked.unwrap_err();
        assert!(format!("{err:#}").contains("LOW prediction cap reached"));

        let override_result = run(
            &backend,
            "add",
            Some("override low prediction"),
            None,
            None,
            Some("medium"),
            Some("low"),
            Some(0.6),
            Some("low-agent"),
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
            true,
            false,
        );
        assert!(override_result.is_ok());
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

    #[test]
    fn parse_btc_above_70k() {
        let result = parse_price_prediction("BTC above $70K by Mar 28", None);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "BTC-USD");
        assert_eq!(p.direction, PriceDirection::Above);
        assert_eq!(p.target_price, Decimal::from(70_000));
    }

    #[test]
    fn parse_gold_below_2000() {
        let result = parse_price_prediction("Gold below 2000 by end of March", None);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "GC=F");
        assert_eq!(p.direction, PriceDirection::Below);
        assert_eq!(p.target_price, Decimal::from(2_000));
    }

    #[test]
    fn parse_tsla_above_250() {
        let result = parse_price_prediction("TSLA above 250.50 by Q2", None);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "TSLA");
        assert_eq!(p.direction, PriceDirection::Above);
        assert_eq!(p.target_price, Decimal::from_str_exact("250.50").unwrap());
    }

    #[test]
    fn parse_btc_reaches_100k() {
        let result = parse_price_prediction("BTC reaches $100K by year end", None);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "BTC-USD");
        assert_eq!(p.direction, PriceDirection::Above);
        assert_eq!(p.target_price, Decimal::from(100_000));
    }

    #[test]
    fn parse_with_symbol_and_dollar_amount() {
        let result = parse_price_prediction("Price will drop below $50,000", Some("BTC-USD"));
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "BTC-USD");
        assert_eq!(p.direction, PriceDirection::Below);
        assert_eq!(p.target_price, Decimal::from(50_000));
    }

    #[test]
    fn parse_non_price_prediction_returns_none() {
        let result = parse_price_prediction("Fed will cut rates in Q2 2026", None);
        assert!(result.is_none());
    }

    #[test]
    fn parse_eth_over_4000() {
        let result = parse_price_prediction("ETH over $4,000 by April", None);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "ETH-USD");
        assert_eq!(p.direction, PriceDirection::Above);
        assert_eq!(p.target_price, Decimal::from(4_000));
    }

    #[test]
    fn parse_silver_under_30() {
        let result = parse_price_prediction("Silver under $30 by June", None);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "SI=F");
        assert_eq!(p.direction, PriceDirection::Below);
        assert_eq!(p.target_price, Decimal::from(30));
    }

    #[test]
    fn resolve_symbol_alias_btc() {
        assert_eq!(resolve_symbol_alias("BTC"), Some("BTC-USD"));
        assert_eq!(resolve_symbol_alias("Bitcoin"), Some("BTC-USD"));
    }

    #[test]
    fn resolve_symbol_alias_unknown() {
        assert_eq!(resolve_symbol_alias("XYZZY"), None);
    }

    #[test]
    fn extract_date_converts_utc_naive_timestamp_to_local_calendar_day() {
        let offset = FixedOffset::west_opt(6 * 3600).unwrap();
        let date = timestamp_date_in_timezone("2026-04-06 00:30:00", offset).unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    }

    #[test]
    fn extract_date_converts_rfc3339_timestamp_to_local_calendar_day() {
        let offset = FixedOffset::west_opt(6 * 3600).unwrap();
        let date = timestamp_date_in_timezone("2026-04-06T00:30:00Z", offset).unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 4, 5).unwrap());
    }

    #[test]
    fn parse_bulk_lessons_input_accepts_array() {
        let items = parse_bulk_lessons_input(
            r#"[{"prediction_id":42,"miss_type":"timing","what_happened":"late","why_wrong":"timing drift"}]"#,
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].prediction_id, 42);
        assert_eq!(items[0].miss_type, "timing");
    }

    #[test]
    fn parse_bulk_lessons_input_rejects_empty() {
        let err = parse_bulk_lessons_input("[]").unwrap_err().to_string();
        assert!(err.contains("empty"));
    }

    #[test]
    fn filter_lesson_views_keeps_only_unresolved_when_requested() {
        let mut views = vec![
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 1,
                claim: "one".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-06T00:00:00Z".to_string(),
                scored_at: None,
                lesson: None,
            },
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 2,
                claim: "two".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-06T00:00:00Z".to_string(),
                scored_at: None,
                lesson: Some(crate::db::prediction_lessons::PredictionLesson {
                    id: 9,
                    prediction_id: 2,
                    miss_type: "timing".to_string(),
                    what_predicted: "two".to_string(),
                    what_happened: "other".to_string(),
                    why_wrong: "wrong".to_string(),
                    signal_misread: None,
                    created_at: "2026-04-06T00:00:00Z".to_string(),
                    status: "active".to_string(),
                    last_cited_at: None,
                }),
            },
        ];
        filter_lesson_views(&mut views, true);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].prediction_id, 1);
    }

    #[test]
    fn scorecard_lesson_coverage_marks_missing_and_present_lessons() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        crate::db::user_predictions::add_prediction_backend(
            &backend,
            "Wrong without lesson",
            Some("BTC-USD"),
            Some("medium"),
            Some("low"),
            None,
            Some("low-agent"),
            None,
            None,
            &[],
        )
        .unwrap();
        crate::db::user_predictions::score_prediction_backend(&backend, 1, "wrong", None, None)
            .unwrap();

        crate::db::user_predictions::add_prediction_backend(
            &backend,
            "Wrong with lesson",
            Some("GC=F"),
            Some("high"),
            Some("high"),
            None,
            Some("high-agent"),
            None,
            None,
            &[],
        )
        .unwrap();
        crate::db::user_predictions::score_prediction_backend(&backend, 2, "wrong", None, None)
            .unwrap();
        crate::db::prediction_lessons::add_lesson_backend(
            &backend,
            2,
            "timing",
            "Wrong with lesson",
            "Gold stalled",
            "Timing was off",
            None,
        )
        .unwrap();

        let rows = crate::db::user_predictions::list_predictions_backend(
            &backend,
            Some("wrong"),
            None,
            None,
            None,
        )
        .unwrap();
        let coverage = build_scorecard_lesson_coverage(&backend, &rows).unwrap();

        assert_eq!(coverage.len(), 2);
        assert_eq!(coverage[0].id, 1);
        assert!(!coverage[0].has_lesson);
        assert_eq!(
            coverage[0].lesson_command.as_deref(),
            Some(
                "pftui journal prediction lessons add --prediction-id 1 --miss-type timing --what-happened \"...\" --why-wrong \"...\""
            )
        );
        assert_eq!(coverage[1].id, 2);
        assert!(coverage[1].has_lesson);
        assert_eq!(coverage[1].lesson_type.as_deref(), Some("timing"));
        assert!(coverage[1].lesson_command.is_none());
    }

    #[test]
    fn lesson_add_command_formats_prediction_id() {
        assert_eq!(
            lesson_add_command(42),
            "pftui journal prediction lessons add --prediction-id 42 --miss-type timing --what-happened \"...\" --why-wrong \"...\""
        );
    }

    #[test]
    fn unresolved_limit_applies_after_filtering() {
        let mut views = vec![
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 1,
                claim: "resolved".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-06T00:00:00Z".to_string(),
                scored_at: None,
                lesson: Some(crate::db::prediction_lessons::PredictionLesson {
                    id: 1,
                    prediction_id: 1,
                    miss_type: "timing".to_string(),
                    what_predicted: "resolved".to_string(),
                    what_happened: "resolved".to_string(),
                    why_wrong: "resolved".to_string(),
                    signal_misread: None,
                    created_at: "2026-04-06T00:00:00Z".to_string(),
                    status: "active".to_string(),
                    last_cited_at: None,
                }),
            },
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 2,
                claim: "unresolved-a".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-06T00:00:00Z".to_string(),
                scored_at: None,
                lesson: None,
            },
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 3,
                claim: "unresolved-b".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-06T00:00:00Z".to_string(),
                scored_at: None,
                lesson: None,
            },
        ];

        filter_lesson_views(&mut views, true);
        views.truncate(1);

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].prediction_id, 2);
    }

    #[test]
    fn unresolved_filter_sorts_oldest_scored_predictions_first() {
        let mut views = vec![
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 2,
                claim: "newer".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-10T00:00:00Z".to_string(),
                scored_at: Some("2026-04-12T00:00:00Z".to_string()),
                lesson: None,
            },
            crate::db::prediction_lessons::PredictionLessonView {
                prediction_id: 1,
                claim: "older".to_string(),
                symbol: None,
                conviction: "medium".to_string(),
                timeframe: None,
                confidence: None,
                source_agent: None,
                target_date: None,
                outcome: "wrong".to_string(),
                score_notes: None,
                created_at: "2026-04-01T00:00:00Z".to_string(),
                scored_at: Some("2026-04-02T00:00:00Z".to_string()),
                lesson: None,
            },
        ];

        filter_lesson_views(&mut views, true);

        assert_eq!(
            views
                .iter()
                .map(|view| view.prediction_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn autoscore_close_above_scores_correct_on_qualifying_close() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes above 100 by end of window",
            "BTC-USD",
            "close-above",
            Some(100.0),
            None,
            None,
            "high",
            "2026-05-30",
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '101', 'test'),
                    ('BTC-USD', '2026-05-30', '95', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let scored = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        // One qualifying close inside the window is enough even though the
        // final close fell back below the threshold.
        assert_eq!(scored.outcome, "correct");
        let notes = scored.score_notes.as_deref().unwrap();
        assert!(notes.contains("auto-scored: close-above"), "notes: {notes}");
        assert!(notes.contains("2026-05-15 close 101"), "notes: {notes}");
        assert!(notes.contains("[series BTC-USD]"), "notes: {notes}");
    }

    #[test]
    fn autoscore_close_above_scores_wrong_after_window_expires() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes above 100 by end of window",
            "BTC-USD",
            "close-above",
            Some(100.0),
            None,
            None,
            "high",
            "2026-05-30",
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '98', 'test'),
                    ('BTC-USD', '2026-05-30', '92', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let scored = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        assert_eq!(scored.outcome, "wrong");
        let notes = scored.score_notes.as_deref().unwrap();
        assert!(notes.contains("window expired 2026-05-30"), "notes: {notes}");
        assert!(notes.contains("98"), "notes: {notes}");
    }

    #[test]
    fn autoscore_open_window_without_qualifying_close_stays_pending() {
        let conn = db::open_in_memory();
        let far_future = (Utc::now().date_naive() + Duration::days(60))
            .format("%Y-%m-%d")
            .to_string();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes above 100 eventually",
            "BTC-USD",
            "close-above",
            Some(100.0),
            None,
            None,
            "high",
            &far_future,
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '95', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let prediction = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        // No qualifying close yet and the window is still open → undecided.
        assert_eq!(prediction.outcome, "pending");
        assert!(prediction.score_notes.is_none());
    }

    #[test]
    fn autoscore_stays_above_scores_wrong_on_first_violation_even_in_open_window() {
        let conn = db::open_in_memory();
        let far_future = (Utc::now().date_naive() + Duration::days(60))
            .format("%Y-%m-%d")
            .to_string();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC stays above 100 for the window",
            "BTC-USD",
            "stays-above",
            Some(100.0),
            None,
            None,
            "high",
            &far_future,
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '105', 'test'),
                    ('BTC-USD', '2026-05-16', '99', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let scored = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        assert_eq!(scored.outcome, "wrong");
        let notes = scored.score_notes.as_deref().unwrap();
        assert!(
            notes.contains("2026-05-16 close 99 violated"),
            "notes: {notes}"
        );
    }

    #[test]
    fn autoscore_stays_in_range_scores_correct_only_after_window_expires_clean() {
        let conn = db::open_in_memory();

        // Open window, all closes inside the band → still pending.
        let far_future = (Utc::now().date_naive() + Duration::days(60))
            .format("%Y-%m-%d")
            .to_string();
        let open_id = seed_autoscore_prediction(
            &conn,
            "BTC stays in range for the open window",
            "BTC-USD",
            "stays-in-range",
            None,
            Some(90.0),
            Some(110.0),
            "high",
            &far_future,
        );
        // Expired window, all closes inside the band → correct.
        let expired_id = seed_autoscore_prediction(
            &conn,
            "BTC stayed in range for the closed window",
            "BTC-USD",
            "stays-in-range",
            None,
            Some(90.0),
            Some(110.0),
            "high",
            "2026-05-30",
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '95', 'test'),
                    ('BTC-USD', '2026-05-20', '108', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let open = rows.iter().find(|row| row.id == open_id).unwrap();
        let expired = rows.iter().find(|row| row.id == expired_id).unwrap();

        assert_eq!(open.outcome, "pending");
        assert_eq!(expired.outcome, "correct");
        assert!(expired
            .score_notes
            .as_deref()
            .unwrap()
            .contains("window expired 2026-05-30 with every close satisfying"));
    }

    #[test]
    fn autoscore_never_overwrites_already_scored_prediction() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes above 100 by end of window",
            "BTC-USD",
            "close-above",
            Some(100.0),
            None,
            None,
            "high",
            "2026-05-30",
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '101', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };
        crate::db::user_predictions::score_prediction_backend(
            &backend,
            prediction_id,
            "wrong",
            Some("manually scored by operator"),
            None,
        )
        .unwrap();

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let prediction = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        assert_eq!(prediction.outcome, "wrong");
        assert_eq!(
            prediction.score_notes.as_deref(),
            Some("manually scored by operator")
        );
    }

    #[test]
    fn autoscore_prints_rule_uses_daily_closes() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "DXY prints below 96.5 before deadline",
            "DX-Y.NYB",
            "prints-below",
            Some(96.5),
            None,
            None,
            "high",
            "2026-05-30",
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('DX-Y.NYB', '2026-05-12', '96.1', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let scored = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();
        assert_eq!(scored.outcome, "correct");
    }

    #[test]
    fn autoscore_falls_back_to_usd_suffixed_series_and_notes_it() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes above 100 by end of window",
            "BTC",
            "close-above",
            Some(100.0),
            None,
            None,
            "high",
            "2026-05-30",
        );
        // No 'BTC' series rows in the window — only 'BTC-USD'.
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-15', '102', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let scored = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();
        assert_eq!(scored.outcome, "correct");
        assert!(scored
            .score_notes
            .as_deref()
            .unwrap()
            .contains("[series BTC-USD]"));
    }

    #[test]
    fn autoscore_missing_price_history_fails_without_scoring() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes above 100 by end of window",
            "BTC-USD",
            "close-above",
            Some(100.0),
            None,
            None,
            "high",
            "2026-05-30",
        );
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), false, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let prediction = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        assert_eq!(prediction.outcome, "pending");
        let rules =
            crate::db::prediction_falsification_rules::list_active_auto_score_rules_backend(
                &backend,
                Some("2026-05-01"),
            )
            .unwrap();
        let err = evaluate_falsification_rule(
            &backend,
            &rules[0],
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("missing_price_history"));
    }

    #[test]
    fn autoscore_dry_run_does_not_mutate_prediction() {
        let conn = db::open_in_memory();
        let prediction_id = seed_autoscore_prediction(
            &conn,
            "BTC closes below 100 by end of window",
            "BTC-USD",
            "close-below",
            Some(100.0),
            None,
            None,
            "medium",
            "2026-05-30",
        );
        conn.execute(
            "INSERT INTO price_history (symbol, date, close, source)
             VALUES ('BTC-USD', '2026-05-30', '90', 'test')",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        run_auto_score(&backend, Some("2026-05-01"), true, "medium", false, true).unwrap();
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let prediction = rows
            .into_iter()
            .find(|row| row.id == prediction_id)
            .unwrap();

        assert_eq!(prediction.outcome, "pending");
        assert!(prediction.score_notes.is_none());
    }

    #[allow(clippy::too_many_arguments)]
    fn seed_autoscore_prediction(
        conn: &rusqlite::Connection,
        claim: &str,
        symbol: &str,
        rule_type: &str,
        threshold_value: Option<f64>,
        threshold_low: Option<f64>,
        threshold_high: Option<f64>,
        confidence: &str,
        eval_date_end: &str,
    ) -> i64 {
        let id = crate::db::user_predictions::add_prediction(
            conn,
            claim,
            Some(symbol),
            Some("medium"),
            Some("medium"),
            Some(0.7),
            Some("test-agent"),
            Some(eval_date_end),
            None,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO prediction_falsification_rules
                (prediction_id, rule_type, symbol, threshold_value, threshold_low, threshold_high,
                 eval_date_start, eval_date_end, parse_confidence, auto_score_eligible)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '2026-05-01', ?7, ?8, 1)",
            rusqlite::params![
                id,
                rule_type,
                symbol,
                threshold_value,
                threshold_low,
                threshold_high,
                eval_date_end,
                confidence
            ],
        )
        .unwrap();
        id
    }

    // ── --falsify grammar parser ───────────────────────────────────────

    #[test]
    fn falsify_parser_close_below() {
        let rule = parse_falsify_rule("BTC close below 50000 by 2026-09-30").unwrap();
        assert_eq!(rule.rule_type, "close-below");
        assert_eq!(rule.asset, "BTC");
        assert_eq!(rule.threshold_value, Some(50000.0));
        assert_eq!(rule.threshold_low, None);
        assert_eq!(rule.threshold_high, None);
        assert_eq!(rule.eval_date_end, "2026-09-30");
    }

    #[test]
    fn falsify_parser_closes_above() {
        let rule = parse_falsify_rule("GC=F closes above 4670 by 2026-08-01").unwrap();
        assert_eq!(rule.rule_type, "close-above");
        assert_eq!(rule.asset, "GC=F");
        assert_eq!(rule.threshold_value, Some(4670.0));
        assert_eq!(rule.eval_date_end, "2026-08-01");
    }

    #[test]
    fn falsify_parser_stays_in_range_with_two_thresholds() {
        let rule = parse_falsify_rule("BTC stays in-range 45000 85000 by 2026-12-31").unwrap();
        assert_eq!(rule.rule_type, "stays-in-range");
        assert_eq!(rule.asset, "BTC");
        assert_eq!(rule.threshold_value, None);
        assert_eq!(rule.threshold_low, Some(45000.0));
        assert_eq!(rule.threshold_high, Some(85000.0));
        assert_eq!(rule.eval_date_end, "2026-12-31");
    }

    #[test]
    fn falsify_parser_prints_below() {
        let rule = parse_falsify_rule("DX-Y.NYB prints below 96.5 by 2027-01-31").unwrap();
        assert_eq!(rule.rule_type, "prints-below");
        assert_eq!(rule.asset, "DX-Y.NYB");
        assert_eq!(rule.threshold_value, Some(96.5));
        assert_eq!(rule.eval_date_end, "2027-01-31");
    }

    #[test]
    fn falsify_parser_close_between_and_band_aliases() {
        let between = parse_falsify_rule("SPY close between 600 700 by 2026-07-31").unwrap();
        assert_eq!(between.rule_type, "close-between");
        assert_eq!(between.threshold_low, Some(600.0));
        assert_eq!(between.threshold_high, Some(700.0));

        let band = parse_falsify_rule("SPY prints in-band 600 700 by 2026-07-31").unwrap();
        assert_eq!(band.rule_type, "prints-in-band");

        let stays_below = parse_falsify_rule("GLD stays below 5000 by 2026-10-01").unwrap();
        assert_eq!(stays_below.rule_type, "stays-below");

        let stays_above = parse_falsify_rule("GLD stays above 4000 by 2026-10-01").unwrap();
        assert_eq!(stays_above.rule_type, "stays-above");
    }

    #[test]
    fn falsify_parser_normalizes_swapped_range_bounds_and_commas() {
        let rule = parse_falsify_rule("BTC stays between 85,000 45,000 by 2026-12-31").unwrap();
        assert_eq!(rule.threshold_low, Some(45000.0));
        assert_eq!(rule.threshold_high, Some(85000.0));
    }

    #[test]
    fn falsify_parser_rejects_malformed_strings() {
        for malformed in [
            "BTC will probably go up a lot soon",
            "close below 50000 by 2026-09-30", // missing symbol → verb slot wrong
            "BTC drifts below 50000 by 2026-09-30", // unknown verb
            "BTC close near 50000 by 2026-09-30", // unknown comparator
            "BTC close below fifty-thousand by 2026-09-30", // non-numeric threshold
            "BTC close below 50000 by September", // bad date
            "BTC close below 50000",           // missing deadline
            "BTC close between 1 by 2026-09-30", // range form with one value
            "50000 close below BTC by 2026-09-30", // numeric symbol slot
        ] {
            assert!(
                parse_falsify_rule(malformed).is_err(),
                "expected parse failure for: {malformed}"
            );
        }
    }

    // ── conviction banding ─────────────────────────────────────────────

    #[test]
    fn conviction_band_handles_text_and_numeric_forms() {
        assert_eq!(conviction_band(None), "medium");
        assert_eq!(conviction_band(Some("low")), "low");
        assert_eq!(conviction_band(Some("HIGH")), "high");
        assert_eq!(conviction_band(Some("medium")), "medium");
        assert_eq!(conviction_band(Some("1")), "low");
        assert_eq!(conviction_band(Some("-1")), "low");
        assert_eq!(conviction_band(Some("3")), "medium");
        assert_eq!(conviction_band(Some("-2")), "medium");
        assert_eq!(conviction_band(Some("4")), "high");
        assert_eq!(conviction_band(Some("-5")), "high");
    }

    // ── write-time confidence discipline ───────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn add_prediction_via_cli_path(
        backend: &BackendConnection,
        claim: &str,
        confidence: Option<f64>,
        conviction: Option<&str>,
        falsify: Option<&str>,
        override_confidence_cap: bool,
        cap_rationale: Option<&str>,
    ) -> Result<()> {
        run_add_with_preflight(
            backend,
            claim,
            Some("BTC-USD"),
            conviction,
            Some("medium"),
            confidence,
            Some("test-agent"),
            Some("2026-12-31"),
            None,
            None,
            Some("crypto"),
            None,
            false,
            None,
            true, // skip_preflight — preflight substrate not under test here
            false,
            false,
            None,
            false,
            falsify,
            override_confidence_cap,
            cap_rationale,
            true, // json output keeps test stdout structured
        )
    }

    fn latest_prediction(
        backend: &BackendConnection,
    ) -> crate::db::user_predictions::UserPrediction {
        crate::db::user_predictions::list_predictions_backend(backend, None, None, None, None)
            .unwrap()
            .into_iter()
            .max_by_key(|p| p.id)
            .unwrap()
    }

    #[test]
    fn add_without_falsify_caps_confidence_at_unfalsifiable_ceiling() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        add_prediction_via_cli_path(
            &backend,
            "BTC structurally repriced higher",
            Some(0.85),
            Some("high"),
            None,
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(UNFALSIFIABLE_CONFIDENCE_CAP));
    }

    /// The `data predictions add` / `analytics predictions add` alias now
    /// dispatches to `run_add_with_preflight` with the exact argument shape
    /// below (preflight NOT skipped — identical to `journal prediction add`).
    /// These two tests pin the alias contract: no --falsify → 0.3 cap;
    /// --falsify → parsed rule, confidence untouched.
    fn add_prediction_via_alias_dispatch(
        backend: &BackendConnection,
        claim: &str,
        confidence: Option<f64>,
        falsify: Option<&str>,
    ) -> Result<()> {
        // Mirrors main.rs::run_data_predictions DataPredictionsCommand::Add.
        run_add_with_preflight(
            backend,
            claim,
            Some("BTC-USD"),
            Some("medium"),
            Some("medium"),
            confidence,
            Some("medium-agent"),
            Some("2026-12-31"),
            None,  // resolution_criteria
            None,  // lessons
            Some("crypto"),
            None,  // source_article_id
            false, // override_cap
            Some("medium"), // effective_layer = layer.or(timeframe)
            false, // skip_preflight — alias defaults match journal add
            false, // accept_preflight
            false, // inline
            None,  // preflight_threshold
            false, // with_adversary
            falsify,
            false, // override_confidence_cap
            None,  // cap_rationale
            true,  // json output keeps test stdout structured
        )
    }

    #[test]
    fn alias_add_without_falsify_gets_unfalsifiable_cap() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        add_prediction_via_alias_dispatch(
            &backend,
            "BTC structurally repriced higher via the alias",
            Some(0.9),
            None,
        )
        .unwrap();
        let row = latest_prediction(&backend);
        assert_eq!(
            row.confidence,
            Some(UNFALSIFIABLE_CONFIDENCE_CAP),
            "alias without --falsify must hit the 0.3 unfalsifiable cap"
        );
    }

    #[test]
    fn alias_add_with_falsify_parses_rule_and_keeps_confidence() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        add_prediction_via_alias_dispatch(
            &backend,
            "BTC reclaims six figures via the alias",
            Some(0.7),
            Some("BTC-USD close above 100000 by 2026-12-31"),
        )
        .unwrap();
        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(0.7), "valid --falsify must not cap");

        let conn = backend.sqlite_native().unwrap();
        let (rule_type, eligible): (String, i64) = conn
            .query_row(
                "SELECT rule_type, auto_score_eligible
                 FROM prediction_falsification_rules
                 WHERE prediction_id = ?1",
                rusqlite::params![row.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(rule_type, "close-above");
        assert_eq!(eligible, 1);
    }

    #[test]
    fn add_with_valid_falsify_keeps_confidence_and_records_rule() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        add_prediction_via_cli_path(
            &backend,
            "BTC capitulates to the low cluster",
            Some(0.7),
            Some("medium"),
            Some("BTC close below 50000 by 2026-09-30"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(0.7), "valid --falsify must not cap");

        let conn = backend.sqlite_native().unwrap();
        let (rule_type, symbol, threshold, end, eligible, parse_conf): (
            String,
            String,
            f64,
            String,
            i64,
            String,
        ) = conn
            .query_row(
                "SELECT rule_type, symbol, threshold_value, eval_date_end,
                        auto_score_eligible, parse_confidence
                 FROM prediction_falsification_rules
                 WHERE prediction_id = ?1",
                rusqlite::params![row.id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(rule_type, "close-below");
        assert_eq!(symbol, "BTC");
        assert_eq!(threshold, 50000.0);
        assert_eq!(end, "2026-09-30");
        assert_eq!(eligible, 1);
        assert_eq!(parse_conf, "high");
    }

    #[test]
    fn add_with_malformed_falsify_records_unstructured_rule_and_caps_confidence() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        add_prediction_via_cli_path(
            &backend,
            "BTC goes parabolic",
            Some(0.9),
            Some("high"),
            Some("BTC moons hard sometime soon"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(UNFALSIFIABLE_CONFIDENCE_CAP));

        let conn = backend.sqlite_native().unwrap();
        let (rule_type, threshold_text, eligible, parse_conf): (String, String, i64, String) =
            conn.query_row(
                "SELECT rule_type, threshold_text, auto_score_eligible, parse_confidence
                 FROM prediction_falsification_rules
                 WHERE prediction_id = ?1",
                rusqlite::params![row.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(rule_type, "unstructured");
        assert_eq!(threshold_text, "BTC moons hard sometime soon");
        assert_eq!(eligible, 0);
        assert_eq!(parse_conf, "low");
    }

    fn seed_calibration_cell(
        backend: &BackendConnection,
        layer: &str,
        topic: &str,
        band: &str,
        n: i64,
        hit_rate: f64,
    ) {
        let conn = backend.sqlite_native().unwrap();
        conn.execute(
            "INSERT INTO calibration_matrix (layer, topic, conviction_band, n, hit_rate)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![layer, topic, band, n, hit_rate],
        )
        .unwrap();
    }

    #[test]
    fn calibration_cap_clamps_overconfident_prediction() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        // Trailing record: 40% hit rate over 10 scored calls → ceiling 0.55.
        seed_calibration_cell(&backend, "medium", "crypto", "medium", 10, 0.40);

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.9),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        let conf = row.confidence.unwrap();
        assert!(
            (conf - (0.40 + CALIBRATION_CAP_MARGIN)).abs() < 1e-9,
            "expected clamp to 0.55, got {conf}"
        );
    }

    #[test]
    fn calibration_cap_skips_small_samples() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        // Only 7 scored calls — below CALIBRATION_CAP_MIN_N.
        seed_calibration_cell(&backend, "medium", "crypto", "medium", 7, 0.40);

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.9),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(0.9));
    }

    #[test]
    fn calibration_cap_override_keeps_confidence_and_records_rationale() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_calibration_cell(&backend, "medium", "crypto", "medium", 12, 0.35);

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.9),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            true,
            Some("regime changed: post-halving supply shock not in trailing sample"),
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(0.9));
        let criteria = row.resolution_criteria.unwrap_or_default();
        assert!(
            criteria.contains("[cap-override: regime changed"),
            "resolution_criteria must carry the rationale, got: {criteria}"
        );
    }

    #[test]
    fn calibration_cap_override_requires_rationale() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let err = add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.9),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            true,
            None,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("--cap-rationale"));
    }

    fn seed_active_misalignment(
        backend: &BackendConnection,
        layer: &str,
        asset: &str,
        streak: i64,
    ) {
        let conn = backend.sqlite_native().unwrap();
        crate::db::forecast_misalignments::ensure_table(conn).unwrap();
        conn.execute(
            "INSERT INTO forecast_misalignments
                (layer, asset, detected_at, streak_len, call, span_start, span_end,
                 cum_realized_against_pct, status)
             VALUES (?1, ?2, '2026-06-01 00:00:00', ?3, 'bull',
                     '2026-04-01', '2026-04-22', -40.5, 'active')",
            rusqlite::params![layer, asset, streak],
        )
        .unwrap();
    }

    #[test]
    fn misalignment_cap_clamps_confidence_on_the_misaligned_symbol() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        // Views are written under the held alias GC=F; the prediction's
        // medium layer is on an active 7-miss streak there. The test helper
        // predicts on BTC-USD, so seed BTC to also exercise the -USD twin.
        seed_active_misalignment(&backend, "medium", "BTC", 7);

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.7),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(
            row.confidence,
            Some(MISALIGNMENT_CONFIDENCE_CAP),
            "active misalignment on (medium, BTC) must cap a BTC-USD prediction"
        );
    }

    #[test]
    fn misalignment_cap_composes_with_calibration_cap_most_restrictive_wins() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        // Calibration alone would clamp 0.9 → 0.55; the misalignment cap is
        // tighter and must win.
        seed_calibration_cell(&backend, "medium", "crypto", "medium", 10, 0.40);
        seed_active_misalignment(&backend, "medium", "BTC-USD", 5);

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.9),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(MISALIGNMENT_CONFIDENCE_CAP));
    }

    #[test]
    fn misalignment_cap_does_not_fire_for_other_layers_or_symbols() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_active_misalignment(&backend, "low", "BTC", 6); // wrong layer
        seed_active_misalignment(&backend, "medium", "GC=F", 6); // wrong symbol

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.7),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            false,
            None,
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(0.7));
    }

    #[test]
    fn misalignment_cap_override_keeps_confidence_and_records_rationale() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_active_misalignment(&backend, "medium", "BTC", 7);

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.7),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            true,
            Some("streak driven by a single resolved macro shock"),
        )
        .unwrap();

        let row = latest_prediction(&backend);
        assert_eq!(row.confidence, Some(0.7));
        let criteria = row.resolution_criteria.unwrap_or_default();
        assert!(
            criteria.contains("[misalignment-cap-override: streak driven"),
            "resolution_criteria must carry the rationale, got: {criteria}"
        );
    }

    #[test]
    fn recovered_misalignment_does_not_clamp() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_active_misalignment(&backend, "medium", "BTC", 7);
        backend
            .sqlite_native()
            .unwrap()
            .execute(
                "UPDATE forecast_misalignments SET status = 'recovered',
                 recovered_at = '2026-06-05 00:00:00'",
                [],
            )
            .unwrap();

        add_prediction_via_cli_path(
            &backend,
            "BTC reclaims the range high",
            Some(0.7),
            Some("medium"),
            Some("BTC close above 90000 by 2026-12-31"),
            false,
            None,
        )
        .unwrap();

        assert_eq!(latest_prediction(&backend).confidence, Some(0.7));
    }

    #[test]
    fn calibration_matrix_cell_tolerates_legacy_column_shape() {
        // Legacy shape: `conviction` TEXT instead of `conviction_band`, and
        // alternate count/rate column names. Built raw (no migrations) so the
        // self-healing migration cannot rewrite it first.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE calibration_matrix (
                layer TEXT,
                topic TEXT,
                conviction TEXT,
                n_scored INTEGER,
                partial_credit_rate REAL
            );
            INSERT INTO calibration_matrix VALUES ('low', 'crypto', 'high', 9, 0.42);",
        )
        .unwrap();

        let cell = calibration_matrix_cell(&conn, "low", "crypto", "high")
            .unwrap()
            .expect("legacy cell must be readable");
        assert_eq!(cell.n_scored, 9);
        assert!((cell.hit_rate - 0.42).abs() < 1e-9);

        // Missing cell → None, not an error.
        assert!(calibration_matrix_cell(&conn, "macro", "fed", "low")
            .unwrap()
            .is_none());
    }

    #[test]
    fn build_bulk_lesson_stubs_prefills_root_cause_prompt() {
        let predictions = vec![crate::db::user_predictions::UserPrediction {
            id: 7,
            claim: "BTC breaks 100k by summer".to_string(),
            symbol: Some("BTC-USD".to_string()),
            conviction: "high".to_string(),
            timeframe: Some("high".to_string()),
            topic: "other".to_string(),
            confidence: Some(0.8),
            source_agent: Some("high-agent".to_string()),
            source_article_id: None,
            target_date: Some("2026-08-01".to_string()),
            resolution_criteria: Some("Close above 100k".to_string()),
            outcome: "wrong".to_string(),
            score_notes: Some("BTC stayed rangebound".to_string()),
            lesson: None,
            lessons_applied: Vec::new(),
            created_at: "2026-04-01T00:00:00Z".to_string(),
            scored_at: Some("2026-04-03T00:00:00Z".to_string()),
        }];

        let stubs = build_bulk_lesson_stubs(predictions);

        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].prediction_id, 7);
        assert_eq!(stubs[0].stub.miss_type, "timing");
        assert!(stubs[0]
            .stub
            .what_happened
            .contains("BTC stayed rangebound"));
        assert!(stubs[0].stub.why_wrong.contains("Root cause"));
        assert_eq!(
            stubs[0].stub.signal_misread.as_deref(),
            Some("Re-check resolution criteria: Close above 100k")
        );
        assert!(stubs[0].age_days >= 0);
        assert_eq!(stubs[0].root_cause, "<fill in why the call failed>");
        assert_eq!(stubs[0].going_forward, "<fill in what changes next time>");
    }

    #[test]
    fn prediction_age_days_accepts_sqlite_timestamp_format() {
        let scored_at = (Local::now() - Duration::days(3))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let prediction = crate::db::user_predictions::UserPrediction {
            id: 9,
            claim: "age test".to_string(),
            symbol: None,
            conviction: "medium".to_string(),
            timeframe: None,
            topic: "other".to_string(),
            confidence: None,
            source_agent: None,
            source_article_id: None,
            target_date: None,
            resolution_criteria: None,
            outcome: "wrong".to_string(),
            score_notes: None,
            lesson: None,
            lessons_applied: Vec::new(),
            created_at: scored_at.clone(),
            scored_at: Some(scored_at),
        };

        assert!(prediction_age_days(&prediction) >= 2);
    }

    #[test]
    fn run_bulk_lessons_dry_run_skips_non_wrong_and_existing() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        crate::db::user_predictions::add_prediction_backend(
            &backend,
            "Wrong call",
            None,
            Some("medium"),
            Some("low"),
            None,
            None,
            None,
            None,
            &[],
        )
        .unwrap();
        crate::db::user_predictions::score_prediction_backend(&backend, 1, "wrong", None, None)
            .unwrap();
        crate::db::user_predictions::add_prediction_backend(
            &backend,
            "Correct call",
            None,
            Some("medium"),
            Some("low"),
            None,
            None,
            None,
            None,
            &[],
        )
        .unwrap();
        crate::db::user_predictions::score_prediction_backend(&backend, 2, "correct", None, None)
            .unwrap();
        crate::db::prediction_lessons::add_lesson_backend(
            &backend,
            1,
            "timing",
            "Wrong call",
            "moved later",
            "late",
            None,
        )
        .unwrap();

        let path = std::env::temp_dir().join("pftui-bulk-lessons-test.json");
        std::fs::write(
            &path,
            r#"[{"prediction_id":1,"miss_type":"timing","what_happened":"later","why_wrong":"late"},
                {"prediction_id":2,"miss_type":"directional","what_happened":"up","why_wrong":"not wrong"}]"#,
        )
        .unwrap();

        let result = run_bulk_lessons(
            &backend,
            Some(path.to_str().unwrap()),
            false,
            true,
            true,
            true,
        );
        std::fs::remove_file(&path).ok();
        assert!(result.is_ok());
    }

    #[test]
    fn run_bulk_lessons_allows_backlog_preview_without_input() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        crate::db::user_predictions::add_prediction_backend(
            &backend,
            "Older wrong call",
            Some("BTC-USD"),
            Some("medium"),
            Some("low"),
            None,
            Some("low-agent"),
            None,
            None,
            &[],
        )
        .unwrap();
        crate::db::user_predictions::score_prediction_backend(&backend, 1, "wrong", None, None)
            .unwrap();
        crate::db::user_predictions::add_prediction_backend(
            &backend,
            "Newer wrong call",
            Some("GC=F"),
            Some("medium"),
            Some("high"),
            None,
            Some("high-agent"),
            None,
            None,
            &[],
        )
        .unwrap();
        crate::db::user_predictions::score_prediction_backend(&backend, 2, "wrong", None, None)
            .unwrap();

        let result = run_bulk_lessons(&backend, None, true, true, true, true);
        assert!(result.is_ok());
    }

    fn seed_high_risk_substrate(conn: &rusqlite::Connection) {
        // Make the gold cluster look maximally risky so the preflight score
        // for a gold/real-yield claim crosses the 50pt abort threshold.
        crate::db::reasoning_fragments::upsert_fragment(
            conn,
            "realrates-dominates-gold",
            "Real yields dominate gold direction",
            "anti-pattern",
            "gold",
            "high",
            None,
            true,
        )
        .unwrap();
        // Calibration row: layer=low / topic=commodities / conviction=high
        // with a 17pp discount (drives the +25 risk).
        crate::db::calibration_adjustments::upsert(
            conn,
            "low",
            "commodities",
            "high",
            12,
            0.55,
            0.72,
            -17.0,
            "discount",
            "Discount confidence by 17pp",
        )
        .unwrap();
        // Failure correlation with 75% co-fail share triggers +20.
        crate::db::failure_correlations::upsert(
            conn,
            "realrates_dominates_gold",
            "btc_correlation_regime",
            6,
            8,
            10,
            0.75,
            7,
        )
        .unwrap();
        // Lesson under the cluster + edge so the fragment is reachable.
        conn.execute(
            "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, topic, outcome, lessons_applied) VALUES ('stub', 'GLD', 'high', 'medium', 'commodities', 'pending', '[]')",
            [],
        ).unwrap();
        let pid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO prediction_lessons
                (prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, cluster_key)
             VALUES (?1, 'directional', 'a', 'b', 'c', NULL, 'realrates_dominates_gold')",
            rusqlite::params![pid],
        )
        .unwrap();
        let lid: i64 = conn.last_insert_rowid();
        crate::db::reasoning_fragments::upsert_edge(conn, lid, "realrates-dominates-gold", "primary")
            .unwrap();
    }

    fn realrates_gold_claim() -> &'static str {
        "Real yield breakdown drives tips and breakeven repricing"
    }

    #[test]
    fn preflight_add_blocks_high_score_without_accept_flag() {
        let conn = db::open_in_memory();
        seed_high_risk_substrate(&conn);
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add_with_preflight(
            &backend,
            realrates_gold_claim(),
            Some("GLD"),
            Some("high"),
            Some("medium"),
            Some(0.7),
            Some("medium-agent"),
            None,
            None,
            None,
            Some("commodities"),
            None,
            false,
            Some("low"),
            false, // skip_preflight
            false, // accept_preflight
            false, // inline
            None,  // threshold (default 50)
            false, // with_adversary
            None,  // falsify
            false, // override_confidence_cap
            None,  // cap_rationale
            false,
        );
        let err = result.unwrap_err();
        assert!(format!("{err:#}").contains("preflight blocked save"));
    }

    #[test]
    fn preflight_add_skip_flag_bypasses_substrate() {
        let conn = db::open_in_memory();
        seed_high_risk_substrate(&conn);
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add_with_preflight(
            &backend,
            realrates_gold_claim(),
            Some("GLD"),
            Some("high"),
            Some("medium"),
            Some(0.7),
            Some("medium-agent"),
            None,
            None,
            None,
            Some("commodities"),
            None,
            false,
            Some("low"),
            true,  // skip_preflight
            false, // accept_preflight
            false, // inline
            None,
            false, // with_adversary
            None,  // falsify
            false, // override_confidence_cap
            None,  // cap_rationale
            true,  // json (suppresses pretty)
        );
        assert!(result.is_ok(), "skip-preflight must bypass abort: {result:?}");
    }

    #[test]
    fn preflight_add_accept_flag_commits_blocking_score() {
        let conn = db::open_in_memory();
        seed_high_risk_substrate(&conn);
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add_with_preflight(
            &backend,
            realrates_gold_claim(),
            Some("GLD"),
            Some("high"),
            Some("medium"),
            Some(0.7),
            Some("medium-agent"),
            None,
            None,
            None,
            Some("commodities"),
            None,
            false,
            Some("low"),
            false, // skip_preflight
            true,  // accept_preflight
            true,  // inline -> resolution_criteria appended
            None,
            false, // with_adversary
            None,  // falsify
            false, // override_confidence_cap
            None,  // cap_rationale
            true,
        );
        assert!(result.is_ok(), "accept-preflight must commit: {result:?}");
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let saved = rows.iter().find(|r| r.claim == realrates_gold_claim()).unwrap();
        let crit = saved.resolution_criteria.clone().unwrap_or_default();
        assert!(crit.contains("[preflight]"), "expected inline preflight block in resolution_criteria, got: {crit}");
    }

    #[test]
    fn preflight_add_low_score_commits_without_flag() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let result = run_add_with_preflight(
            &backend,
            "Totally uncategorized text zzz",
            None,
            Some("medium"),
            Some("medium"),
            Some(0.5),
            Some("medium-agent"),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
            false,
            false,
            None,
            false, // with_adversary
            None,  // falsify
            false, // override_confidence_cap
            None,  // cap_rationale
            true,
        );
        assert!(result.is_ok(), "low-score claim must commit: {result:?}");
    }

    #[test]
    fn with_adversary_flag_persists_adversary_view_and_appends_inline_summary() {
        // Seed substrate so the adversary view has anti-pattern + co-failure
        // content to surface.
        let conn = db::open_in_memory();
        crate::db::reasoning_fragments::upsert_fragment(
            &conn,
            "options-gamma-pinning",
            "Round-number strikes pin price intraday when OI > 50k",
            "anti-pattern",
            "options",
            "high",
            Some("OI at round strike > 50_000"),
            false,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, topic, outcome, lessons_applied) VALUES ('stub', 'SPY', 'medium', 'low', 'equities', 'pending', '[]')",
            [],
        )
        .unwrap();
        let pid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO prediction_lessons (prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, cluster_key)
             VALUES (?1, 'directional', 'a', 'b', 'pinned at strike', NULL, 'options_gamma_pinning')",
            rusqlite::params![pid],
        )
        .unwrap();
        let lid: i64 = conn.last_insert_rowid();
        crate::db::reasoning_fragments::upsert_edge(
            &conn,
            lid,
            "options-gamma-pinning",
            "primary",
        )
        .unwrap();
        // Co-failure: tight_threshold_close_miss with a lesson on that side.
        crate::db::failure_correlations::upsert(
            &conn,
            "options_gamma_pinning",
            "tight_threshold_close_miss",
            4,
            6,
            8,
            0.66,
            7,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, topic, outcome, lessons_applied) VALUES ('stub2', 'SPY', 'medium', 'low', 'equities', 'pending', '[]')",
            [],
        )
        .unwrap();
        let pid2: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO prediction_lessons (prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, cluster_key)
             VALUES (?1, 'directional', 'a', 'b', 'close pinned within 0.5xATR', NULL, 'tight_threshold_close_miss')",
            rusqlite::params![pid2],
        )
        .unwrap();

        let backend = BackendConnection::Sqlite { conn };
        let result = run_add_with_preflight(
            &backend,
            "SPY gamma pin at 700 by 0dte expiry",
            Some("SPY"),
            Some("medium"),
            Some("low"),
            Some(0.6),
            Some("low-agent"),
            None,
            None,
            None,
            Some("equities"),
            None,
            true, // override_cap so the cap path doesn't get in the way
            Some("low"),
            true,  // skip_preflight so the adversary path is exercised in isolation
            false, // accept_preflight
            false, // inline (preflight)
            None,
            true,  // with_adversary
            None,  // falsify
            false, // override_confidence_cap
            None,  // cap_rationale
            false, // pretty output
        );
        assert!(result.is_ok(), "with-adversary must commit: {result:?}");

        // Find the newly inserted prediction.
        let rows =
            crate::db::user_predictions::list_predictions_backend(&backend, None, None, None, None)
                .unwrap();
        let saved = rows
            .iter()
            .find(|r| r.claim.contains("SPY gamma pin at 700"))
            .expect("expected prediction row");
        let crit = saved.resolution_criteria.clone().unwrap_or_default();
        assert!(
            crit.contains("[adversary]"),
            "expected inline adversary block in resolution_criteria, got: {crit}"
        );

        // adversary_views row persisted and linked to the new prediction id.
        let conn = backend.sqlite_native().unwrap();
        let views = crate::db::adversary_views::list_for_prediction(conn, saved.id).unwrap();
        assert_eq!(views.len(), 1, "expected one adversary_views row");
        let view = &views[0];
        assert_eq!(view.cluster_key, "options_gamma_pinning");
        assert!(
            view.anti_pattern_arguments.contains("options-gamma-pinning"),
            "anti_pattern_arguments missing fragment id: {}",
            view.anti_pattern_arguments
        );
        assert!(
            view.cofailure_warnings.contains("tight_threshold_close_miss"),
            "cofailure_warnings missing cluster: {}",
            view.cofailure_warnings
        );
        assert!(
            !view.falsification_triggers.is_empty()
                && view.falsification_triggers != "[]",
            "expected non-empty falsification_triggers, got: {}",
            view.falsification_triggers
        );
    }
}
