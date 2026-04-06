use anyhow::{bail, Result};
use chrono::{Duration, FixedOffset, Local, NaiveDate, Offset, Utc};
use regex::Regex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_cached_price_backend;
use crate::db::price_history::get_price_at_date_backend;
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
                        "macro" => 3,
                        _ => 4,
                    });
                    for (tf, s) in &tf_entries {
                        println!(
                            "    {:<8} — {}/{} scored, {:.1}% hit rate ({} correct, {} partial, {} wrong)",
                            tf, s.scored, s.total, s.hit_rate_pct, s.correct, s.partial, s.wrong
                        );
                    }
                }

                if !stats.by_source_agent.is_empty() {
                    println!("\n  By agent:");
                    let mut agent_entries: Vec<_> = stats.by_source_agent.iter().collect();
                    agent_entries.sort_by(|(a, _), (b, _)| a.cmp(b));
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
                    sym_entries.sort_by(|(_, a), (_, b)| b.total.cmp(&a.total));
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

/// Direction parsed from a prediction claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PriceDirection {
    Above,
    Below,
}

/// A parsed price-direction prediction extracted from the claim text.
#[derive(Debug, Clone)]
struct ParsedPricePrediction {
    symbol: String,
    direction: PriceDirection,
    target_price: Decimal,
}

/// Known symbol aliases for matching prediction claims to price_history/price_cache symbols.
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

/// Result of auto-scoring a single prediction.
#[derive(Debug, Clone, serde::Serialize)]
struct AutoScoreResult {
    id: i64,
    claim: String,
    symbol: Option<String>,
    parsed_symbol: String,
    direction: String,
    target_price: String,
    actual_price: String,
    outcome: String,
    note: String,
}

/// Auto-score pending predictions whose target_date has passed.
/// Only scores unambiguous price-direction predictions.
pub fn run_auto_score(backend: &BackendConnection, dry_run: bool, json_output: bool) -> Result<()> {
    let today = Utc::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();

    // Fetch all pending predictions
    let pending =
        user_predictions::list_predictions_backend(backend, Some("pending"), None, None, None)?;

    let mut scoreable: Vec<AutoScoreResult> = Vec::new();
    let mut skipped: Vec<serde_json::Value> = Vec::new();

    for pred in &pending {
        // Check if target_date has passed
        let target_date = match &pred.target_date {
            Some(td) => {
                let parsed = NaiveDate::parse_from_str(td, "%Y-%m-%d");
                match parsed {
                    Ok(d) if d <= today => d,
                    Ok(_) => {
                        // Target date is in the future — skip
                        continue;
                    }
                    Err(_) => {
                        // Can't parse target_date — skip
                        skipped.push(json!({
                            "id": pred.id,
                            "reason": "unparseable target_date",
                            "target_date": td,
                        }));
                        continue;
                    }
                }
            }
            None => {
                // No target_date — skip
                continue;
            }
        };

        // Try to parse a price-direction prediction from the claim
        let parsed = match parse_price_prediction(&pred.claim, pred.symbol.as_deref()) {
            Some(p) => p,
            None => {
                skipped.push(json!({
                    "id": pred.id,
                    "reason": "not a parseable price-direction prediction",
                    "claim": pred.claim,
                }));
                continue;
            }
        };

        // Get the price at the target date (or closest before it)
        let price_at_date = get_price_at_date_backend(
            backend,
            &parsed.symbol,
            &target_date.format("%Y-%m-%d").to_string(),
        )
        .ok()
        .flatten();

        // Also get current price as fallback for very recent dates
        let current_price = get_current_price(backend, &parsed.symbol);

        let actual_price = match price_at_date.or(current_price) {
            Some(p) if p > Decimal::ZERO => p,
            _ => {
                skipped.push(json!({
                    "id": pred.id,
                    "reason": "no price data available",
                    "symbol": parsed.symbol,
                    "target_date": target_date.to_string(),
                }));
                continue;
            }
        };

        // Determine outcome
        let outcome = match parsed.direction {
            PriceDirection::Above => {
                if actual_price >= parsed.target_price {
                    "correct"
                } else {
                    "wrong"
                }
            }
            PriceDirection::Below => {
                if actual_price <= parsed.target_price {
                    "correct"
                } else {
                    "wrong"
                }
            }
        };

        let direction_str = match parsed.direction {
            PriceDirection::Above => "above",
            PriceDirection::Below => "below",
        };

        let note = format!(
            "Auto-scored from market data: {} was {} at {} (target: {} {} by {})",
            parsed.symbol,
            actual_price,
            today_str,
            direction_str,
            parsed.target_price,
            target_date,
        );

        scoreable.push(AutoScoreResult {
            id: pred.id,
            claim: pred.claim.clone(),
            symbol: pred.symbol.clone(),
            parsed_symbol: parsed.symbol,
            direction: direction_str.to_string(),
            target_price: parsed.target_price.to_string(),
            actual_price: actual_price.to_string(),
            outcome: outcome.to_string(),
            note,
        });
    }

    // Apply scores (unless dry_run)
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

    if json_output {
        let payload = json!({
            "dry_run": dry_run,
            "scored": scoreable,
            "scored_count": scoreable.len(),
            "skipped": skipped,
            "skipped_count": skipped.len(),
            "total_pending": pending.len(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if scoreable.is_empty() {
        println!("No predictions eligible for auto-scoring.");
        if !skipped.is_empty() {
            println!("  {} predictions skipped (no target_date, not parseable, or no price data)", skipped.len());
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
                "  #{} [{}] {} → {} (actual: {}, target: {} {})",
                r.id,
                r.parsed_symbol,
                if r.claim.len() > 50 {
                    format!("{}...", &r.claim[..47])
                } else {
                    r.claim.clone()
                },
                r.outcome,
                r.actual_price,
                r.direction,
                r.target_price,
            );
        }
        if !skipped.is_empty() {
            println!(
                "  {} predictions skipped (not auto-scoreable)",
                skipped.len()
            );
        }
    }

    Ok(())
}

/// List wrong predictions with structured lessons (or mark which lack lessons).
pub fn run_lessons(
    backend: &BackendConnection,
    miss_type: Option<&str>,
    unresolved_only: bool,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    use crate::db::prediction_lessons;

    // Validate miss_type if provided
    if let Some(mt) = miss_type {
        prediction_lessons::validate_miss_type_str(mt)?;
    }

    let backend_limit = if unresolved_only { None } else { limit };
    let mut views = prediction_lessons::list_lesson_views_backend(backend, miss_type, backend_limit)?;
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
            println!("Showing only unresolved backlog ({} without lessons)\n", unresolved_count);
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
    }
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

pub fn run_bulk_lessons(
    backend: &BackendConnection,
    input_path: &str,
    unresolved_only: bool,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    use crate::db::prediction_lessons;
    use crate::db::user_predictions;

    let raw = std::fs::read_to_string(input_path)
        .map_err(|err| anyhow::anyhow!("failed to read '{}': {}", input_path, err))?;
    let entries = parse_bulk_lessons_input(&raw)?;
    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    let lesson_views = prediction_lessons::list_lesson_views_backend(backend, None, None)?;

    let prediction_map: std::collections::HashMap<i64, _> =
        predictions.into_iter().map(|prediction| (prediction.id, prediction)).collect();
    let existing_lessons: std::collections::HashSet<i64> = lesson_views
        .into_iter()
        .filter(|view| view.lesson.is_some())
        .map(|view| view.prediction_id)
        .collect();

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
                reason: Some(format!("prediction outcome is '{}', not 'wrong'", prediction.outcome)),
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

    let added = results.iter().filter(|result| result.status == "added").count();
    let skipped = results.iter().filter(|result| result.status == "skipped").count();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "input_path": input_path,
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
    let predictions =
        user_predictions::list_predictions_backend(backend, None, None, None, None)?;
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
        assert_eq!(
            p.target_price,
            Decimal::from_str_exact("250.50").unwrap()
        );
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
        let result = parse_price_prediction(
            "Price will drop below $50,000",
            Some("BTC-USD"),
        );
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.symbol, "BTC-USD");
        assert_eq!(p.direction, PriceDirection::Below);
        assert_eq!(p.target_price, Decimal::from(50_000));
    }

    #[test]
    fn parse_non_price_prediction_returns_none() {
        let result = parse_price_prediction(
            "Fed will cut rates in Q2 2026",
            None,
        );
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
        assert_eq!(
            date,
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap()
        );
    }

    #[test]
    fn extract_date_converts_rfc3339_timestamp_to_local_calendar_day() {
        let offset = FixedOffset::west_opt(6 * 3600).unwrap();
        let date = timestamp_date_in_timezone("2026-04-06T00:30:00Z", offset).unwrap();
        assert_eq!(
            date,
            NaiveDate::from_ymd_opt(2026, 4, 5).unwrap()
        );
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
                }),
            },
        ];
        filter_lesson_views(&mut views, true);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].prediction_id, 1);
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

        let result = run_bulk_lessons(&backend, path.to_str().unwrap(), true, true, true);
        std::fs::remove_file(&path).ok();
        assert!(result.is_ok());
    }
}
