use anyhow::{bail, Result};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};

use crate::alerts::AlertStatus;
use crate::db::backend::BackendConnection;
use crate::db::query;
use crate::db::{
    agent_messages, alerts, convictions, correlation_snapshots, price_cache, regime_snapshots,
    research_questions, scenarios, structural, technical_levels, technical_snapshots, thesis,
    timeframe_signals, transactions, trends, user_predictions, watchlist,
};
use crate::models::asset::AssetCategory;

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    value2: Option<&str>,
    value3: Option<&str>,
    symbol: Option<&str>,
    countries: &[String],
    metric: Option<&str>,
    score: Option<f64>,
    rank: Option<i32>,
    trend: Option<&str>,
    probability: Option<f64>,
    phase: Option<&str>,
    evidence: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
    driver: Option<&str>,
    impact: Option<&str>,
    outcome: Option<&str>,
    decade: Option<i32>,
    composite: bool,
    file: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    from: Option<&str>,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "technicals" => run_technicals(
            backend,
            symbol,
            value.unwrap_or("1d"),
            limit,
            json_output,
        ),
        "levels" => run_levels(backend, symbol, signal_type, limit, json_output),
        "signals" => run_signals(backend, symbol, signal_type, severity, limit, json_output),
        "summary" => run_summary(backend, json_output),
        "low" => run_low(backend, json_output),
        "medium" => run_medium(backend, json_output),
        "high" => run_high(backend, json_output),
        "macro" => run_macro(
            backend,
            value,
            value2,
            value3,
            countries,
            metric,
            score,
            rank,
            trend,
            probability,
            phase,
            evidence,
            notes,
            source,
            driver,
            impact,
            outcome,
            decade,
            composite,
            file,
            from,
            date,
            limit,
            json_output,
        ),
        "alignment" => run_alignment(backend, symbol, json_output),
        "divergence" => run_divergence(backend, symbol, json_output),
        "digest" => run_digest(backend, from, limit, json_output),
        "recap" => run_recap(backend, date, limit, json_output),
        "gaps" => run_gaps(backend, json_output),
        _ => bail!(
            "unknown analytics action '{}'. Valid: technicals, levels, signals, summary, low, medium, high, macro, alignment, divergence, digest, recap, gaps",
            action
        ),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct RecapEvent {
    at: String,
    event_type: String,
    source: String,
    summary: String,
}

fn parse_day_filter(value: Option<&str>) -> Result<Option<NaiveDate>> {
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

fn ts_matches_day(ts: &str, day: Option<NaiveDate>) -> bool {
    let Some(target) = day else {
        return true;
    };
    if ts.len() < 10 {
        return false;
    }
    NaiveDate::parse_from_str(&ts[..10], "%Y-%m-%d")
        .map(|d| d == target)
        .unwrap_or(false)
}

fn run_digest(
    backend: &BackendConnection,
    from: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let role = from.unwrap_or("evening-analyst");
    let lim = limit.unwrap_or(10);

    let regime = regime_snapshots::get_current_backend(backend)
        .ok()
        .flatten();
    let top_signals =
        timeframe_signals::list_signals_backend(backend, None, None, Some(lim)).unwrap_or_default();
    let divergences = build_alignment_rows(backend, None)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .take(lim)
        .collect::<Vec<_>>();
    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let pending_predictions =
        user_predictions::list_predictions_backend(backend, Some("pending"), None, None, Some(lim))
            .unwrap_or_default();
    let scorecard = user_predictions::get_stats_backend(backend).ok();
    let recent_messages = agent_messages::list_messages_backend(
        backend,
        None,
        Some(role),
        None,
        true,
        None,
        None,
        Some(lim),
    )
    .unwrap_or_default();

    let payload = match role {
        "low-agent" | "low-timeframe-analyst" => json!({
            "from": role,
            "regime": regime,
            "signals": top_signals,
            "divergences": divergences,
            "pending_predictions": pending_predictions,
            "unacked_messages": recent_messages,
        }),
        "medium-agent" | "medium-timeframe-analyst" => json!({
            "from": role,
            "scenarios": scenarios_list,
            "convictions": conviction_rows,
            "pending_predictions": pending_predictions,
            "signals": top_signals,
            "unacked_messages": recent_messages,
        }),
        _ => json!({
            "from": role,
            "regime": regime,
            "scenarios": scenarios_list,
            "signals": top_signals,
            "divergences": divergences,
            "prediction_stats": scorecard,
            "pending_predictions": pending_predictions,
            "unacked_messages": recent_messages,
        }),
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("Analytics Digest ({})", role);
        println!("  Signals: {}", top_signals.len());
        println!("  Divergences: {}", divergences.len());
        println!("  Pending predictions: {}", pending_predictions.len());
        println!("  Unacked messages: {}", recent_messages.len());
    }

    Ok(())
}

fn run_recap(
    backend: &BackendConnection,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let day = parse_day_filter(date)?;
    let mut events: Vec<RecapEvent> = Vec::new();

    let preds = user_predictions::list_predictions_backend(backend, None, None, None, None)
        .unwrap_or_default();
    for p in preds {
        if ts_matches_day(&p.created_at, day) {
            events.push(RecapEvent {
                at: p.created_at.clone(),
                event_type: "prediction_added".to_string(),
                source: p
                    .source_agent
                    .clone()
                    .unwrap_or_else(|| "predict".to_string()),
                summary: format!("#{} {}", p.id, p.claim),
            });
        }
        if let Some(scored_at) = p.scored_at.as_ref() {
            if ts_matches_day(scored_at, day) {
                events.push(RecapEvent {
                    at: scored_at.clone(),
                    event_type: "prediction_scored".to_string(),
                    source: p
                        .source_agent
                        .clone()
                        .unwrap_or_else(|| "predict".to_string()),
                    summary: format!("#{} -> {}", p.id, p.outcome),
                });
            }
        }
    }

    let scenario_rows = scenarios::list_scenarios_backend(backend, None).unwrap_or_default();
    for s in scenario_rows {
        if ts_matches_day(&s.updated_at, day) {
            events.push(RecapEvent {
                at: s.updated_at.clone(),
                event_type: "scenario_updated".to_string(),
                source: "scenario".to_string(),
                summary: format!("{} {:.1}% ({})", s.name, s.probability, s.status),
            });
        }
    }

    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    for c in conviction_rows {
        if ts_matches_day(&c.recorded_at, day) {
            events.push(RecapEvent {
                at: c.recorded_at.clone(),
                event_type: "conviction_set".to_string(),
                source: "conviction".to_string(),
                summary: format!("{} -> {}", c.symbol, c.score),
            });
        }
    }

    let signal_rows =
        timeframe_signals::list_signals_backend(backend, None, None, None).unwrap_or_default();
    for s in signal_rows {
        if ts_matches_day(&s.detected_at, day) {
            events.push(RecapEvent {
                at: s.detected_at.clone(),
                event_type: "timeframe_signal".to_string(),
                source: "analytics".to_string(),
                summary: format!("[{}] {}", s.severity, s.description),
            });
        }
    }

    let regime_rows = regime_snapshots::get_history_backend(backend, Some(limit.unwrap_or(50)))
        .unwrap_or_default();
    for r in regime_rows {
        if ts_matches_day(&r.recorded_at, day) {
            events.push(RecapEvent {
                at: r.recorded_at.clone(),
                event_type: "regime_snapshot".to_string(),
                source: "regime".to_string(),
                summary: format!("{} ({:.2})", r.regime, r.confidence.unwrap_or(0.0)),
            });
        }
    }

    let msg_rows =
        agent_messages::list_messages_backend(backend, None, None, None, false, None, None, None)
            .unwrap_or_default();
    for m in msg_rows {
        if ts_matches_day(&m.created_at, day) {
            events.push(RecapEvent {
                at: m.created_at.clone(),
                event_type: "agent_message".to_string(),
                source: m.from_agent.clone(),
                summary: m.content.chars().take(120).collect(),
            });
        }
    }

    events.sort_by(|a, b| b.at.cmp(&a.at));
    if let Some(n) = limit {
        events.truncate(n);
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "date": day.map(|d| d.to_string()),
                "events": events,
                "count": events.len(),
            }))?
        );
    } else if events.is_empty() {
        println!("No recap events found.");
    } else {
        println!("Analytics Recap");
        for e in events {
            println!("  {} [{}:{}] {}", e.at, e.source, e.event_type, e.summary);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct GapRow {
    layer: &'static str,
    table: &'static str,
    count: i64,
    status: &'static str,
    last_update: Option<String>,
    age_hours: Option<f64>,
}

fn parse_dt(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(dt, Utc));
    }
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
}

fn table_stats(
    backend: &BackendConnection,
    table: &str,
    ts_col: &str,
    epoch_seconds: bool,
) -> Result<(i64, Option<String>)> {
    let sqlite_sql = if epoch_seconds {
        format!(
            "SELECT COUNT(*), CAST(MAX({}) AS TEXT) FROM {}",
            ts_col, table
        )
    } else {
        format!("SELECT COUNT(*), MAX({}) FROM {}", ts_col, table)
    };
    let pg_sql = format!(
        "SELECT COUNT(*)::BIGINT, MAX({})::text FROM {}",
        ts_col, table
    );

    query::dispatch(
        backend,
        |conn| {
            let (count, ts): (i64, Option<String>) =
                conn.query_row(&sqlite_sql, [], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok((count, ts))
        },
        |pool| {
            let row: (i64, Option<String>) = crate::db::pg_runtime::block_on(async {
                sqlx::query_as(&pg_sql).fetch_one(pool).await
            })?;
            Ok(row)
        },
    )
}

fn classify_gap(
    layer: &'static str,
    table: &'static str,
    count: i64,
    raw_ts: Option<String>,
    max_age_hours: i64,
    epoch_seconds: bool,
) -> GapRow {
    if count <= 0 {
        return GapRow {
            layer,
            table,
            count,
            status: "missing",
            last_update: None,
            age_hours: None,
        };
    }

    let parsed = raw_ts.as_ref().and_then(|raw| {
        if epoch_seconds {
            raw.parse::<i64>()
                .ok()
                .and_then(|secs| DateTime::from_timestamp(secs, 0).map(|d| d.with_timezone(&Utc)))
        } else {
            parse_dt(raw)
        }
    });

    let now = Utc::now();
    let age_hours = parsed.map(|dt| (now.signed_duration_since(dt).num_seconds() as f64) / 3600.0);
    let status = if let Some(age) = age_hours {
        if age > max_age_hours as f64 {
            "stale"
        } else {
            "fresh"
        }
    } else {
        "stale"
    };

    GapRow {
        layer,
        table,
        count,
        status,
        last_update: parsed.map(|dt| dt.to_rfc3339()),
        age_hours,
    }
}

fn run_gaps(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let specs: [(&str, &str, &str, i64, bool); 14] = [
        ("low", "price_cache", "fetched_at", 1, false),
        ("low", "price_history", "date", 48, false),
        ("low", "technical_snapshots", "computed_at", 24, false),
        ("low", "regime_snapshots", "recorded_at", 24, false),
        ("low", "timeframe_signals", "detected_at", 24, false),
        ("medium", "scenarios", "updated_at", 24 * 7, false),
        ("medium", "thesis", "updated_at", 24 * 14, false),
        ("medium", "convictions", "recorded_at", 24 * 14, false),
        ("high", "trend_tracker", "updated_at", 24 * 14, false),
        ("macro", "structural_outcomes", "updated_at", 24 * 30, false),
        ("macro", "news_cache", "fetched_at", 12, false),
        ("macro", "predictions_cache", "updated_at", 24, true),
        ("macro", "cot_cache", "fetched_at", 24 * 8, false),
        ("macro", "onchain_cache", "fetched_at", 24 * 2, false),
    ];

    let mut rows = Vec::new();
    for (layer, table, ts_col, max_age_hours, epoch_seconds) in specs {
        if let Ok((count, last_update)) = table_stats(backend, table, ts_col, epoch_seconds) {
            rows.push(classify_gap(
                layer,
                table,
                count,
                last_update,
                max_age_hours,
                epoch_seconds,
            ));
        }
    }

    let missing = rows.iter().filter(|r| r.status == "missing").count();
    let stale = rows.iter().filter(|r| r.status == "stale").count();
    let fresh = rows.iter().filter(|r| r.status == "fresh").count();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "gaps": rows,
                "summary": {
                    "missing": missing,
                    "stale": stale,
                    "fresh": fresh,
                    "checked": rows.len()
                }
            }))?
        );
    } else {
        println!("Analytics Data Gaps");
        println!(
            "{:<7} {:<20} {:>8} {:<8} {:>8}",
            "Layer", "Table", "Records", "Status", "Age(h)"
        );
        println!("{}", "─".repeat(62));
        for row in &rows {
            let age = row
                .age_hours
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:<7} {:<20} {:>8} {:<8} {:>8}",
                row.layer, row.table, row.count, row.status, age
            );
        }
        println!(
            "\nChecked {} tables: {} fresh, {} stale, {} missing",
            rows.len(),
            fresh,
            stale,
            missing
        );
    }

    Ok(())
}

fn run_technicals(
    backend: &BackendConnection,
    symbol: Option<&str>,
    timeframe: &str,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows = if let Some(sym) = symbol {
        technical_snapshots::get_latest_snapshot_backend(backend, &sym.to_uppercase(), timeframe)?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        technical_snapshots::list_latest_snapshots_backend(backend, timeframe, limit)?
    };
    if let Some(limit) = limit {
        rows.truncate(limit);
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "timeframe": timeframe,
                "technicals": rows,
                "count": rows.len(),
            }))?
        );
    } else if rows.is_empty() {
        println!(
            "No technical snapshots found for timeframe '{}'.",
            timeframe
        );
    } else {
        println!("Technical Snapshots ({})", timeframe);
        println!(
            "{:<10} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:<8}",
            "Symbol", "RSI", "MACD", "Hist", "SMA20", "SMA50", "SMA200", "52W%", "Volume"
        );
        println!("{}", "─".repeat(90));
        for row in rows.drain(..) {
            println!(
                "{:<10} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:<8}",
                row.symbol,
                fmt_opt(row.rsi_14, 1),
                fmt_opt(row.macd, 2),
                fmt_opt(row.macd_histogram, 2),
                fmt_opt(row.sma_20, 2),
                fmt_opt(row.sma_50, 2),
                fmt_opt(row.sma_200, 2),
                fmt_opt(row.range_52w_position, 0),
                row.volume_regime.unwrap_or_else(|| "n/a".to_string()),
            );
        }
    }

    Ok(())
}

fn fmt_opt(value: Option<f64>, precision: usize) -> String {
    value
        .map(|v| format!("{:.*}", precision, v))
        .unwrap_or_else(|| "N/A".to_string())
}

fn run_levels(
    backend: &BackendConnection,
    symbol: Option<&str>,
    level_type: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows = if let Some(sym) = symbol {
        technical_levels::get_levels_for_symbol_backend(backend, &sym.to_uppercase())?
    } else {
        technical_levels::list_all_levels_backend(backend, None)?
    };

    // Filter by level_type if provided
    if let Some(lt) = level_type {
        let lt_lower = lt.to_lowercase();
        rows.retain(|r| r.level_type.to_lowercase() == lt_lower);
    }

    if let Some(limit) = limit {
        rows.truncate(limit);
    }

    if json_output {
        // Enrich with nearest-level context when filtering by symbol
        let output = if let Some(sym) = symbol {
            let spot = price_cache::get_cached_price_backend(backend, &sym.to_uppercase(), "USD")
                .ok()
                .flatten()
                .map(|q| q.price.to_string().parse::<f64>().unwrap_or(0.0));

            let nearest = spot.and_then(|price| {
                nearest_levels(&rows, price)
            });

            json!({
                "symbol": sym.to_uppercase(),
                "spot_price": spot,
                "nearest_support": nearest.as_ref().map(|n| &n.0),
                "nearest_resistance": nearest.as_ref().map(|n| &n.1),
                "levels": rows,
                "count": rows.len(),
            })
        } else {
            json!({
                "levels": rows,
                "count": rows.len(),
            })
        };

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if rows.is_empty() {
        println!(
            "No market structure levels found{}.",
            symbol
                .map(|s| format!(" for {}", s.to_uppercase()))
                .unwrap_or_default()
        );
        println!("Run `pftui data refresh` to compute levels.");
    } else {
        if let Some(sym) = symbol {
            println!("Market Structure Levels — {}", sym.to_uppercase());
        } else {
            println!("Market Structure Levels — All Symbols");
        }
        println!(
            "{:<10} {:<14} {:>12} {:>8} {:<16} Notes",
            "Symbol", "Type", "Price", "Str", "Method"
        );
        println!("{}", "─".repeat(80));
        for row in &rows {
            println!(
                "{:<10} {:<14} {:>12} {:>8} {:<16} {}",
                row.symbol,
                row.level_type,
                format_level_price(row.price),
                format!("{:.0}%", row.strength * 100.0),
                row.source_method,
                row.notes.as_deref().unwrap_or(""),
            );
        }
    }

    Ok(())
}

/// Find the nearest support (below price) and resistance (above price).
fn nearest_levels(
    levels: &[crate::db::technical_levels::TechnicalLevelRecord],
    price: f64,
) -> Option<(serde_json::Value, serde_json::Value)> {
    let support_types = ["support", "swing_low", "bb_lower", "range_52w_low"];
    let resist_types = ["resistance", "swing_high", "bb_upper", "range_52w_high"];

    let nearest_support = levels
        .iter()
        .filter(|l| {
            l.price < price
                && (support_types.contains(&l.level_type.as_str())
                    || l.level_type.starts_with("sma_"))
        })
        .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal))
        .map(|l| {
            json!({
                "price": l.price,
                "type": l.level_type,
                "strength": l.strength,
                "distance_pct": ((price - l.price) / price) * 100.0,
                "notes": l.notes,
            })
        })
        .unwrap_or(serde_json::Value::Null);

    let nearest_resistance = levels
        .iter()
        .filter(|l| {
            l.price > price
                && (resist_types.contains(&l.level_type.as_str())
                    || l.level_type.starts_with("sma_"))
        })
        .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal))
        .map(|l| {
            json!({
                "price": l.price,
                "type": l.level_type,
                "strength": l.strength,
                "distance_pct": ((l.price - price) / price) * 100.0,
                "notes": l.notes,
            })
        })
        .unwrap_or(serde_json::Value::Null);

    Some((nearest_support, nearest_resistance))
}

fn format_level_price(price: f64) -> String {
    if price >= 10000.0 {
        format!("{:.0}", price)
    } else if price >= 1.0 {
        format!("{:.2}", price)
    } else {
        format!("{:.4}", price)
    }
}

fn run_signals(
    backend: &BackendConnection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows = timeframe_signals::list_signals_backend(
        backend,
        signal_type,
        severity,
        limit.or(Some(25)),
    )?;
    if let Some(sym) = symbol {
        let needle = format!("\"{}\"", sym.to_uppercase());
        rows.retain(|r| r.assets.to_uppercase().contains(&needle));
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "signals": rows,
                "count": rows.len()
            }))?
        );
    } else if rows.is_empty() {
        println!("No cross-timeframe signals found.");
    } else {
        println!("Cross-timeframe signals ({}):", rows.len());
        for sig in rows {
            println!(
                "  [{}|{}] {}\n    assets={} layers={} at={}",
                sig.severity,
                sig.signal_type,
                sig.description,
                sig.assets,
                sig.layers,
                sig.detected_at
            );
        }
    }

    Ok(())
}

fn run_summary(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let regime = regime_snapshots::get_current_backend(backend)?;
    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let top_scenario = scenarios_list.first().cloned();
    let trends_list =
        trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
    let top_trend = trends_list.first().cloned();
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    let top_cycle = cycles.first().cloned();
    let signal = timeframe_signals::latest_signal_backend(backend)?;
    let signal_count = timeframe_signals::list_signals_backend(backend, None, None, None)
        .unwrap_or_default()
        .len();
    let price_count = price_cache::get_all_cached_prices_backend(backend)
        .unwrap_or_default()
        .len();
    let all_alerts = alerts::list_alerts_backend(backend).unwrap_or_default();
    let alert_count = all_alerts.len();
    let triggered_alert_count = all_alerts
        .iter()
        .filter(|a| a.status == AlertStatus::Triggered)
        .count();
    let alignments = build_alignment_rows(backend, None)?;
    let alignment_score = if alignments.is_empty() {
        0.0
    } else {
        alignments.iter().map(|a| a.score_pct).sum::<f64>() / alignments.len() as f64
    };
    let divergence_notes: Vec<String> = alignments
        .iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .take(5)
        .map(|a| format!("{} {}B/{}S", a.symbol, a.bull_layers, a.bear_layers))
        .collect();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "regime": regime,
                "top_scenario": top_scenario,
                "top_trend": top_trend,
                "top_cycle": top_cycle,
                "top_signal": signal,
                "prices_tracked": price_count,
                "alerts_total": alert_count,
                "alerts_triggered": triggered_alert_count,
                "signal_count": signal_count,
                "alignment_score_pct": alignment_score,
                "alignment_assets": alignments.len(),
                "divergence_notes": divergence_notes,
            }))?
        );
    } else {
        println!("Analytics Engine — Multi-Timeframe Intelligence");
        println!("════════════════════════════════════════════════════════════════");
        println!(
            "PRICES: {}  ALERTS: {} (triggered {})  SIGNALS: {}",
            price_count, alert_count, triggered_alert_count, signal_count
        );
        println!(
            "ALIGNMENT SCORE: {} {:.0}% ({} assets)",
            score_bar(alignment_score),
            alignment_score,
            alignments.len()
        );
        if !divergence_notes.is_empty() {
            println!("DIVERGENCES: {}", divergence_notes.join(" | "));
        }
        if let Some(r) = regime {
            println!(
                "LOW: {} ({:.2})",
                r.regime.to_uppercase(),
                r.confidence.unwrap_or(0.0)
            );
        } else {
            println!("LOW: no regime snapshot");
        }
        if let Some(s) = top_scenario {
            println!("MEDIUM: {} ({:.1}%)", s.name, s.probability);
        } else {
            println!("MEDIUM: no active scenario");
        }
        if let Some(t) = top_trend {
            println!("HIGH: {} [{}]", t.name, t.direction);
        } else {
            println!("HIGH: no active trend");
        }
        if let Some(c) = top_cycle {
            println!("MACRO: {} -> {}", c.cycle_name, c.current_stage);
        } else {
            println!("MACRO: no structural cycle");
        }
        if let Some(sig) = signal {
            println!("ALIGNMENT SIGNAL: [{}] {}", sig.severity, sig.description);
        }
    }
    Ok(())
}

fn run_low(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let regime = regime_snapshots::get_current_backend(backend)?;
    let corr =
        correlation_snapshots::list_current_backend(backend, Some("30d")).unwrap_or_default();
    let signals =
        timeframe_signals::list_signals_backend(backend, None, None, Some(10)).unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "regime": regime,
                "correlations": corr,
                "signals": signals,
            }))?
        );
    } else {
        println!("LOW Layer");
        if let Some(r) = regime {
            println!(
                "  Regime: {} ({:.2})",
                r.regime,
                r.confidence.unwrap_or(0.0)
            );
        }
        println!("  Correlations tracked: {}", corr.len());
        println!("  Signals: {}", signals.len());
    }

    Ok(())
}

fn run_medium(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let thesis_sections = thesis::list_thesis_backend(backend).unwrap_or_default();
    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let questions =
        research_questions::list_questions_backend(backend, Some("open")).unwrap_or_default();
    let predictions =
        user_predictions::list_predictions_backend(backend, Some("pending"), None, None, Some(20))
            .unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "scenarios": scenarios_list,
                "thesis": thesis_sections,
                "convictions": conviction_rows,
                "research_questions": questions,
                "predictions": predictions,
            }))?
        );
    } else {
        println!("MEDIUM Layer");
        println!("  Scenarios: {}", scenarios_list.len());
        println!("  Thesis sections: {}", thesis_sections.len());
        println!("  Convictions: {}", conviction_rows.len());
        println!("  Open questions: {}", questions.len());
        println!("  Pending predictions: {}", predictions.len());
    }

    Ok(())
}

fn run_high(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let trends_list =
        trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
    let mut evidence = Vec::new();
    let mut impacts = Vec::new();
    for t in &trends_list {
        let ev = trends::list_evidence_backend(backend, t.id, Some(3)).unwrap_or_default();
        evidence.push(json!({ "trend": t.name, "items": ev }));
        let imp = trends::list_asset_impacts_backend(backend, t.id).unwrap_or_default();
        impacts.push(json!({ "trend": t.name, "items": imp }));
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "trends": trends_list,
                "evidence": evidence,
                "impacts": impacts,
            }))?
        );
    } else {
        println!("HIGH Layer");
        println!("  Active trends: {}", trends_list.len());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_macro(
    backend: &BackendConnection,
    subaction: Option<&str>,
    arg1: Option<&str>,
    arg2: Option<&str>,
    countries: &[String],
    metric: Option<&str>,
    score: Option<f64>,
    rank: Option<i32>,
    trend: Option<&str>,
    probability: Option<f64>,
    phase: Option<&str>,
    evidence: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
    driver: Option<&str>,
    impact: Option<&str>,
    outcome: Option<&str>,
    decade: Option<i32>,
    composite: bool,
    file: Option<&str>,
    _from: Option<&str>,
    date: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let country = countries.first().map(|s| s.as_str());
    match subaction.unwrap_or("dashboard") {
        "dashboard" => {}
        "metrics" => {
            if arg1 == Some("set") {
                let c = arg2
                    .or(country)
                    .ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro metrics set <country> --metric <name> [--score N] [--rank N] [--trend rising|stable|declining]"))?;
                let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
                let tr = trend.unwrap_or("stable");
                let id =
                    structural::set_metric_backend(backend, c, m, score, rank, tr, notes, source)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &json!({"id": id, "country": c, "metric": m})
                        )?
                    );
                } else {
                    println!("Set macro metric: {} {} = {:?}", c, m, score);
                }
                return Ok(());
            }
            return run_macro_metrics(backend, arg1.or(country), json_output);
        }
        "metric-set" => {
            let c = country
                .or(arg1)
                .ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro metric-set <country> --metric <name> [--score N] [--rank N] [--trend rising|stable|declining]"))?;
            let m = metric.ok_or_else(|| anyhow::anyhow!("--metric required"))?;
            let tr = trend.unwrap_or("stable");
            let id = structural::set_metric_backend(backend, c, m, score, rank, tr, notes, source)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"id": id, "country": c, "metric": m}))?
                );
            } else {
                println!("Set macro metric: {} {} = {:?}", c, m, score);
            }
            return Ok(());
        }
        "compare" => {
            let left = arg1.ok_or_else(|| {
                anyhow::anyhow!("usage: pftui analytics macro compare <country-a> <country-b>")
            })?;
            let right = arg2.ok_or_else(|| {
                anyhow::anyhow!("usage: pftui analytics macro compare <country-a> <country-b>")
            })?;
            return run_macro_compare(backend, left, right, json_output);
        }
        "cycles" => {
            if arg1 == Some("history") {
                if arg2 == Some("add") {
                    let c = country.ok_or_else(|| anyhow::anyhow!("--country required"))?;
                    let m = metric.ok_or_else(|| anyhow::anyhow!("--determinant required"))?;
                    let d = decade.ok_or_else(|| anyhow::anyhow!("--year required"))?;
                    let sc = score.ok_or_else(|| anyhow::anyhow!("--score required"))?;
                    return run_macro_cycles_history_add(
                        backend,
                        c,
                        m,
                        d,
                        sc,
                        notes,
                        source,
                        json_output,
                    );
                }
                if arg2 == Some("add-batch") {
                    let path =
                        file.ok_or_else(|| anyhow::anyhow!("--file required for add-batch"))?;
                    let mut rdr = csv::Reader::from_path(path)?;
                    let mut inserted = 0usize;
                    for rec in rdr.records() {
                        let rec = rec?;
                        let c = rec.get(0).unwrap_or("").trim();
                        let m = rec.get(1).unwrap_or("").trim();
                        let d: i32 = rec.get(2).unwrap_or("").trim().parse()?;
                        let sc: f64 = rec.get(3).unwrap_or("").trim().parse()?;
                        let n = rec.get(4).map(str::trim).filter(|v| !v.is_empty());
                        let s = rec.get(5).map(str::trim).filter(|v| !v.is_empty());
                        if c.is_empty() || m.is_empty() {
                            continue;
                        }
                        structural::add_metric_history_backend(backend, c, m, d, sc, n, s)?;
                        inserted += 1;
                    }
                    if json_output {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(
                                &json!({"inserted": inserted, "file": path})
                            )?
                        );
                    } else {
                        println!("Imported {} history rows from {}", inserted, path);
                    }
                    return Ok(());
                }
                return run_macro_cycles_history(
                    backend,
                    countries,
                    metric,
                    decade,
                    composite,
                    json_output,
                );
            }
            if arg1 == Some("update") {
                let name = arg2.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro cycles update <name> --phase <phase> [--evidence text]"))?;
                let stage = phase.ok_or_else(|| anyhow::anyhow!("--phase required"))?;
                structural::set_cycle_backend(backend, name, stage, None, notes, evidence)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"name": name, "phase": stage}))?
                    );
                } else {
                    println!("Updated cycle: {} -> {}", name, stage);
                }
                return Ok(());
            }
            return run_macro_cycles(backend, json_output);
        }
        "cycle-update" => {
            let name = arg1.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro cycle-update <name> --phase <phase> [--evidence text]"))?;
            let stage = phase.ok_or_else(|| anyhow::anyhow!("--phase required"))?;
            structural::set_cycle_backend(backend, name, stage, None, notes, evidence)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"name": name, "phase": stage}))?
                );
            } else {
                println!("Updated cycle: {} -> {}", name, stage);
            }
            return Ok(());
        }
        "outcomes" => {
            if arg1 == Some("update") {
                let name = arg2.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro outcomes update <name> --probability <N> [--driver text]"))?;
                let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
                structural::update_outcome_probability_backend(backend, name, prob, driver)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &json!({"name": name, "probability": prob, "driver": driver})
                        )?
                    );
                } else {
                    println!("Updated outcome: {} -> {:.1}%", name, prob);
                }
                return Ok(());
            }
            return run_macro_outcomes(backend, json_output);
        }
        "outcome-update" => {
            let name = arg1.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro outcome-update <name> --probability <N> [--driver text]"))?;
            let prob = probability.ok_or_else(|| anyhow::anyhow!("--probability required"))?;
            structural::update_outcome_probability_backend(backend, name, prob, driver)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"name": name, "probability": prob, "driver": driver})
                    )?
                );
            } else {
                println!("Updated outcome: {} -> {:.1}%", name, prob);
            }
            return Ok(());
        }
        "parallels" => return run_macro_parallels(backend, json_output),
        "log" => {
            if arg1 == Some("add") {
                let development = arg2.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro log add <development> --date YYYY-MM-DD [--impact text] [--outcome text]"))?;
                let d = date.ok_or_else(|| anyhow::anyhow!("--date required"))?;
                let id = structural::add_log_backend(backend, d, development, impact, outcome)?;
                if json_output {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({"id": id, "date": d}))?
                    );
                } else {
                    println!("Added macro log entry for {}", d);
                }
                return Ok(());
            }
            return run_macro_log(backend, limit, json_output);
        }
        "log-add" => {
            let development = arg1.ok_or_else(|| anyhow::anyhow!("usage: pftui analytics macro log-add <development> --date YYYY-MM-DD [--impact text] [--outcome text]"))?;
            let d = date.ok_or_else(|| anyhow::anyhow!("--date required"))?;
            let id = structural::add_log_backend(backend, d, development, impact, outcome)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({"id": id, "date": d}))?
                );
            } else {
                println!("Added macro log entry for {}", d);
            }
            return Ok(());
        }
        other => {
            bail!(
                "unknown analytics macro subcommand '{}'. Valid: dashboard, metrics, metric-set, compare, cycles, cycle-update, outcomes, outcome-update, parallels, log, log-add",
                other
            )
        }
    }

    let metrics = structural::list_metrics_backend(backend, None, None).unwrap_or_default();
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    let outcomes = structural::list_outcomes_backend(backend).unwrap_or_default();
    let parallels = structural::list_parallels_backend(backend, None).unwrap_or_default();
    let log_rows = structural::list_log_backend(backend, None, Some(10)).unwrap_or_default();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "power_metrics": metrics,
                "structural_cycles": cycles,
                "structural_outcomes": outcomes,
                "historical_parallels": parallels,
                "structural_log": log_rows,
            }))?
        );
    } else {
        println!("MACRO Layer");
        println!("  Metrics: {}", metrics.len());
        println!("  Cycles: {}", cycles.len());
        println!("  Outcomes: {}", outcomes.len());
        println!("  Parallels: {}", parallels.len());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_macro_cycles_history_add(
    backend: &BackendConnection,
    country: &str,
    determinant: &str,
    year: i32,
    score: f64,
    notes: Option<&str>,
    source: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let id = structural::add_metric_history_backend(
        backend,
        country,
        determinant,
        year,
        score,
        notes,
        source,
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": id,
                "country": country,
                "determinant": determinant,
                "year": year,
                "score": score,
                "notes": notes,
                "source": source,
            }))?
        );
    } else {
        println!(
            "Added history row: {} {} {} = {:.2}",
            country, determinant, year, score
        );
    }

    Ok(())
}

fn run_macro_metrics(
    backend: &BackendConnection,
    country: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let metrics = structural::list_metrics_backend(backend, country, None).unwrap_or_default();
    if metrics.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "country": country,
                    "metrics": [],
                    "count": 0,
                }))?
            );
        } else {
            println!("No macro power metrics found.");
        }
        return Ok(());
    }

    let grouped = build_country_metric_views(&metrics);
    if json_output {
        let out = if let Some(c) = country {
            if let Some(view) = grouped.get(c) {
                json!({
                    "country": c,
                    "metrics": view.metrics,
                    "composite": view.composite,
                    "composite_prev": view.composite_prev,
                    "composite_delta": view.composite_delta,
                })
            } else {
                json!({
                    "country": c,
                    "metrics": [],
                    "composite": null,
                    "composite_prev": null,
                    "composite_delta": null,
                })
            }
        } else {
            json!({ "countries": grouped })
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if let Some(c) = country {
        if let Some(view) = grouped.get(c) {
            println!("Macro Metrics ({})", c);
            for m in &view.metrics {
                println!(
                    "  {:<20} score={} rank={} trend={} {}",
                    m.metric,
                    m.score
                        .map(|v| format!("{:.2}", v))
                        .unwrap_or_else(|| "—".to_string()),
                    m.rank
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "—".to_string()),
                    m.trend,
                    trend_arrow(&m.trend),
                );
            }
            println!(
                "\n  Composite (0-10): {}{}",
                view.composite
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "—".to_string()),
                view.composite_delta
                    .map(|d| format!(" ({:+.2} vs prev)", d))
                    .unwrap_or_default()
            );
        } else {
            println!("No macro power metrics found for {}.", c);
        }
    } else {
        println!("Macro Metrics (all countries)");
        for (c, view) in grouped {
            println!(
                "  {:<10} composite={}{} ({} metrics)",
                c,
                view.composite
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "—".to_string()),
                view.composite_delta
                    .map(|d| format!(" {:+.2}", d))
                    .unwrap_or_default(),
                view.metrics.len()
            );
        }
    }
    Ok(())
}

fn run_macro_compare(
    backend: &BackendConnection,
    country_a: &str,
    country_b: &str,
    json_output: bool,
) -> Result<()> {
    let a_rows =
        structural::list_metrics_backend(backend, Some(country_a), None).unwrap_or_default();
    let b_rows =
        structural::list_metrics_backend(backend, Some(country_b), None).unwrap_or_default();
    let a_view = build_country_metric_view(&a_rows);
    let b_view = build_country_metric_view(&b_rows);

    let mut keys: BTreeSet<String> = BTreeSet::new();
    for row in &a_view.metrics {
        keys.insert(row.metric.clone());
    }
    for row in &b_view.metrics {
        keys.insert(row.metric.clone());
    }
    let map_a: HashMap<String, MetricCurrentRow> = a_view
        .metrics
        .iter()
        .map(|m| (m.metric.clone(), m.clone()))
        .collect();
    let map_b: HashMap<String, MetricCurrentRow> = b_view
        .metrics
        .iter()
        .map(|m| (m.metric.clone(), m.clone()))
        .collect();

    let rows: Vec<serde_json::Value> = keys
        .into_iter()
        .map(|metric| {
            let left = map_a.get(&metric);
            let right = map_b.get(&metric);
            let left_score = left.and_then(|m| m.score);
            let right_score = right.and_then(|m| m.score);
            let gap = match (left_score, right_score) {
                (Some(l), Some(r)) => Some(l - r),
                _ => None,
            };
            let left_prev = a_view.prev_scores.get(&metric).copied().flatten();
            let right_prev = b_view.prev_scores.get(&metric).copied().flatten();
            let prev_gap = match (left_prev, right_prev) {
                (Some(l), Some(r)) => Some(l - r),
                _ => None,
            };
            json!({
                "metric": metric,
                "left_score": left_score,
                "left_trend": left.map(|m| m.trend.clone()),
                "right_score": right_score,
                "right_trend": right.map(|m| m.trend.clone()),
                "gap": gap,
                "trend": gap_trend(gap, prev_gap),
            })
        })
        .collect();

    let composite_gap = match (a_view.composite, b_view.composite) {
        (Some(l), Some(r)) => Some(l - r),
        _ => None,
    };
    let composite_prev_gap = match (a_view.composite_prev, b_view.composite_prev) {
        (Some(l), Some(r)) => Some(l - r),
        _ => None,
    };
    let composite_trend = gap_trend(composite_gap, composite_prev_gap);

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "left_country": country_a,
                "right_country": country_b,
                "rows": rows,
                "composite": {
                    "left": a_view.composite,
                    "right": b_view.composite,
                    "gap": composite_gap,
                    "trend": composite_trend,
                }
            }))?
        );
    } else {
        println!("Macro Compare: {} vs {}", country_a, country_b);
        println!(
            "{:<16} {:>10} {:>10} {:>8} {:>10}",
            "Determinant", country_a, country_b, "Gap", "Trend"
        );
        println!("{}", "─".repeat(62));
        for row in &rows {
            let metric = row["metric"].as_str().unwrap_or("metric");
            let left = row["left_score"]
                .as_f64()
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string());
            let right = row["right_score"]
                .as_f64()
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string());
            let gap = row["gap"]
                .as_f64()
                .map(|v| format!("{:+.2}", v))
                .unwrap_or_else(|| "—".to_string());
            let trend = row["trend"].as_str().unwrap_or("Unknown");
            println!(
                "{:<16} {:>10} {:>10} {:>8} {:>10}",
                metric, left, right, gap, trend
            );
        }
        println!("{}", "─".repeat(62));
        println!(
            "{:<16} {:>10} {:>10} {:>8} {:>10}",
            "Composite",
            a_view
                .composite
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string()),
            b_view
                .composite
                .map(|v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string()),
            composite_gap
                .map(|v| format!("{:+.2}", v))
                .unwrap_or_else(|| "—".to_string()),
            composite_trend
        );
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct MetricCurrentRow {
    metric: String,
    score: Option<f64>,
    rank: Option<i32>,
    trend: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CountryMetricView {
    metrics: Vec<MetricCurrentRow>,
    composite: Option<f64>,
    composite_prev: Option<f64>,
    composite_delta: Option<f64>,
    #[serde(skip_serializing)]
    prev_scores: HashMap<String, Option<f64>>,
}

fn build_country_metric_views(
    rows: &[structural::PowerMetric],
) -> HashMap<String, CountryMetricView> {
    let mut grouped: HashMap<String, Vec<structural::PowerMetric>> = HashMap::new();
    for row in rows {
        grouped
            .entry(row.country.clone())
            .or_default()
            .push(row.clone());
    }
    grouped
        .into_iter()
        .map(|(country, list)| (country, build_country_metric_view(&list)))
        .collect()
}

fn build_country_metric_view(rows: &[structural::PowerMetric]) -> CountryMetricView {
    let mut latest: HashMap<String, MetricCurrentRow> = HashMap::new();
    let mut prev: HashMap<String, Option<f64>> = HashMap::new();

    for row in rows {
        if !latest.contains_key(&row.metric) {
            latest.insert(
                row.metric.clone(),
                MetricCurrentRow {
                    metric: row.metric.clone(),
                    score: row.score,
                    rank: row.rank,
                    trend: row.trend.clone(),
                },
            );
        } else if !prev.contains_key(&row.metric) {
            prev.insert(row.metric.clone(), row.score);
        }
    }

    let mut metrics: Vec<MetricCurrentRow> = latest.into_values().collect();
    metrics.sort_by(|a, b| a.metric.cmp(&b.metric));

    let mut current_components: Vec<f64> = Vec::new();
    let mut prev_components: Vec<f64> = Vec::new();
    for m in &metrics {
        if dalio_metric_key(&m.metric).is_some() {
            if let Some(score) = m.score {
                current_components.push(score);
            }
            if let Some(Some(prev_score)) = prev.get(&m.metric) {
                prev_components.push(*prev_score);
            }
        }
    }

    let composite = avg(&current_components);
    let composite_prev = avg(&prev_components);
    let composite_delta = match (composite, composite_prev) {
        (Some(c), Some(p)) => Some(c - p),
        _ => None,
    };

    CountryMetricView {
        metrics,
        composite,
        composite_prev,
        composite_delta,
        prev_scores: prev,
    }
}

fn avg(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn dalio_metric_key(metric: &str) -> Option<&'static str> {
    let m = metric.to_ascii_lowercase();
    if m.contains("education") {
        Some("education")
    } else if m.contains("innovation") {
        Some("innovation")
    } else if m.contains("competit") {
        Some("competitiveness")
    } else if m.contains("military") {
        Some("military")
    } else if m.contains("trade") {
        Some("trade")
    } else if m.contains("economic") || m.contains("gdp") || m.contains("output") {
        Some("economic_output")
    } else if m.contains("financial") {
        Some("financial_center")
    } else if m.contains("reserve") || m.contains("currency") {
        Some("reserve_currency")
    } else {
        None
    }
}

fn trend_arrow(trend: &str) -> &'static str {
    match trend.to_ascii_lowercase().as_str() {
        "up" | "rising" | "improving" | "bullish" => "↑",
        "down" | "falling" | "weakening" | "bearish" => "↓",
        _ => "→",
    }
}

fn gap_trend(gap: Option<f64>, prev_gap: Option<f64>) -> &'static str {
    match (gap, prev_gap) {
        (Some(g), Some(p)) if g.abs() < p.abs() => "Closing",
        (Some(g), Some(p)) if g.abs() > p.abs() => "Widening",
        (Some(_), Some(_)) => "Stable",
        _ => "Unknown",
    }
}

fn run_macro_cycles(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&cycles)?);
    } else {
        for c in cycles {
            println!(
                "{}: {} (since {})",
                c.cycle_name,
                c.current_stage,
                c.stage_entered.unwrap_or_else(|| "?".to_string())
            );
        }
    }
    Ok(())
}

fn run_macro_cycles_history(
    backend: &BackendConnection,
    countries: &[String],
    metric: Option<&str>,
    decade: Option<i32>,
    composite: bool,
    json_output: bool,
) -> Result<()> {
    let rows = structural::list_metric_history_backend(backend, countries, metric, decade)
        .unwrap_or_default();
    let show_composite = composite || metric.is_none();

    if show_composite {
        let mut country_decade_scores: HashMap<String, HashMap<i32, Vec<f64>>> = HashMap::new();
        for r in &rows {
            country_decade_scores
                .entry(r.country.clone())
                .or_default()
                .entry(r.decade)
                .or_default()
                .push(r.score);
        }

        let live_metrics =
            structural::list_metrics_backend(backend, None, None).unwrap_or_default();
        let live_views = build_country_metric_views(&live_metrics);
        let mut decades: BTreeSet<i32> = BTreeSet::new();
        for dmap in country_decade_scores.values() {
            for d in dmap.keys() {
                decades.insert(*d);
            }
        }
        let mut decades_vec: Vec<i32> = decades.into_iter().collect();
        decades_vec.sort_unstable();

        if json_output {
            let countries_json = country_decade_scores
                .iter()
                .map(|(country, dmap)| {
                    let mut points = serde_json::Map::new();
                    for d in &decades_vec {
                        let val = dmap
                            .get(d)
                            .and_then(|vals| avg(vals))
                            .map(|v| (v * 100.0).round() / 100.0);
                        points.insert(d.to_string(), json!(val));
                    }
                    let live = live_views
                        .get(country)
                        .and_then(|v| v.composite)
                        .map(|v| (v * 100.0).round() / 100.0);
                    points.insert("2026".to_string(), json!(live));
                    json!({"country": country, "series": points})
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "mode": "composite",
                    "decades": decades_vec,
                    "countries": countries_json,
                    "rows": rows,
                }))?
            );
        } else {
            let mut header = String::from("        ");
            for d in &decades_vec {
                header.push_str(&format!("{:>6}", d));
            }
            header.push_str(&format!("{:>6}", 2026));
            println!("{}", header);
            let mut countries_sorted = country_decade_scores.keys().cloned().collect::<Vec<_>>();
            countries_sorted.sort();
            for c in countries_sorted {
                let mut line = format!("{:<8}", c);
                let dmap = country_decade_scores.get(&c).expect("country exists");
                for d in &decades_vec {
                    let val = dmap.get(d).and_then(|vals| avg(vals));
                    match val {
                        Some(v) => line.push_str(&format!("{:>6.1}", v)),
                        None => line.push_str(&format!("{:>6}", "—")),
                    }
                }
                let live = live_views.get(&c).and_then(|v| v.composite);
                match live {
                    Some(v) => line.push_str(&format!("{:>6.1}", v)),
                    None => line.push_str(&format!("{:>6}", "—")),
                }
                println!("{}", line);
            }
        }
        return Ok(());
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"rows": rows, "count": rows.len()}))?
        );
    } else if rows.is_empty() {
        println!("No power metric history rows found.");
    } else {
        for r in rows {
            println!("{} {} {} = {:.2}", r.country, r.metric, r.decade, r.score);
        }
    }
    Ok(())
}

fn run_macro_outcomes(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let outcomes = structural::list_outcomes_backend(backend).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&outcomes)?);
    } else {
        for o in outcomes {
            println!("{}: {:.0}%", o.name, o.probability);
        }
    }
    Ok(())
}

fn run_macro_parallels(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let parallels = structural::list_parallels_backend(backend, None).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&parallels)?);
    } else {
        for p in parallels {
            println!("{} | {} → {}", p.period, p.event, p.parallel_to);
        }
    }
    Ok(())
}

fn run_macro_log(
    backend: &BackendConnection,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let rows =
        structural::list_log_backend(backend, None, Some(limit.unwrap_or(20))).unwrap_or_default();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for l in rows {
            println!("{} | {}", l.date, l.development);
        }
    }
    Ok(())
}

fn regime_to_bias(regime: &str) -> &'static str {
    match regime {
        "risk-on" => "bull",
        "risk-off" | "crisis" | "stagflation" => "bear",
        _ => "neutral",
    }
}

fn run_alignment(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let alignments = build_alignment_rows(backend, symbol)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "alignments": alignments,
                "count": alignments.len(),
            }))?
        );
    } else {
        if alignments.is_empty() {
            println!("No assets available for alignment.");
            return Ok(());
        }
        println!("Alignment Matrix");
        println!(
            "{:<10} {:<7} {:<7} {:<7} {:<7} {:<13} {:>6}",
            "Symbol", "Low", "Medium", "High", "Macro", "Consensus", "Score"
        );
        println!("{}", "─".repeat(72));
        for a in alignments {
            println!(
                "{:<10} {:<7} {:<7} {:<7} {:<7} {:<13} {:>5.0}%",
                a.symbol, a.low, a.medium, a.high, a.macro_bias, a.consensus, a.score_pct
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct DivergenceRow {
    symbol: String,
    low: String,
    medium: String,
    high: String,
    macro_bias: String,
    bull_layers: usize,
    bear_layers: usize,
    disagreement_pct: f64,
    dominant_side: String,
}

fn run_divergence(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let mut rows: Vec<DivergenceRow> = build_alignment_rows(backend, symbol)?
        .into_iter()
        .filter(|a| a.bull_layers > 0 && a.bear_layers > 0)
        .map(|a| {
            let dominant_side = if a.bull_layers > a.bear_layers {
                "bull"
            } else if a.bear_layers > a.bull_layers {
                "bear"
            } else {
                "split"
            }
            .to_string();
            let disagreement_pct = (a.bull_layers.min(a.bear_layers) as f64 / 4.0) * 100.0;
            DivergenceRow {
                symbol: a.symbol,
                low: a.low,
                medium: a.medium,
                high: a.high,
                macro_bias: a.macro_bias,
                bull_layers: a.bull_layers,
                bear_layers: a.bear_layers,
                disagreement_pct,
                dominant_side,
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        b.disagreement_pct
            .partial_cmp(&a.disagreement_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "divergences": rows,
                "count": rows.len(),
            }))?
        );
    } else if rows.is_empty() {
        println!("No cross-layer divergences detected.");
    } else {
        println!("Cross-Layer Divergence");
        println!(
            "{:<10} {:<7} {:<7} {:<7} {:<7} {:>6} {:>6} {:>8}",
            "Symbol", "Low", "Medium", "High", "Macro", "Bull", "Bear", "Split%"
        );
        println!("{}", "─".repeat(72));
        for row in rows {
            println!(
                "{:<10} {:<7} {:<7} {:<7} {:<7} {:>6} {:>6} {:>7.0}%",
                row.symbol,
                row.low,
                row.medium,
                row.high,
                row.macro_bias,
                row.bull_layers,
                row.bear_layers,
                row.disagreement_pct
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct AlignmentRow {
    symbol: String,
    low: String,
    medium: String,
    high: String,
    macro_bias: String,
    consensus: String,
    score_pct: f64,
    bull_layers: usize,
    bear_layers: usize,
}

fn score_bar(score_pct: f64) -> String {
    let filled = (score_pct.clamp(0.0, 100.0) / 10.0).round() as usize;
    let mut out = String::new();
    for i in 0..10 {
        out.push(if i < filled { '█' } else { '░' });
    }
    out
}

fn bias_from_score(score: i32) -> String {
    if score > 0 {
        "bull".to_string()
    } else if score < 0 {
        "bear".to_string()
    } else {
        "neutral".to_string()
    }
}

fn consensus_from_counts(bull: usize, bear: usize) -> String {
    if bull == 4 {
        "STRONG BUY".to_string()
    } else if bear == 4 {
        "STRONG AVOID".to_string()
    } else if bull >= 3 {
        "BULLISH".to_string()
    } else if bear >= 3 {
        "BEARISH".to_string()
    } else {
        "MIXED".to_string()
    }
}

fn discover_alignment_symbols(
    backend: &BackendConnection,
    filter_symbol: Option<&str>,
) -> Vec<String> {
    if let Some(sym) = filter_symbol {
        return vec![sym.to_uppercase()];
    }

    let mut symbols: BTreeSet<String> = BTreeSet::new();
    if let Ok(rows) = transactions::get_unique_symbols_backend(backend) {
        for (symbol, category) in rows {
            if category != AssetCategory::Cash {
                symbols.insert(symbol.to_uppercase());
            }
        }
    }
    if let Ok(rows) = watchlist::list_watchlist_backend(backend) {
        for row in rows {
            if !row.category.eq_ignore_ascii_case("cash") {
                symbols.insert(row.symbol.to_uppercase());
            }
        }
    }
    symbols.into_iter().collect()
}

fn build_alignment_rows(
    backend: &BackendConnection,
    filter_symbol: Option<&str>,
) -> Result<Vec<AlignmentRow>> {
    let symbols = discover_alignment_symbols(backend, filter_symbol);
    let low_regime = regime_snapshots::get_current_backend(backend)?;
    let low_bias = low_regime
        .as_ref()
        .map(|r| regime_to_bias(&r.regime).to_string())
        .unwrap_or_else(|| "neutral".to_string());
    let low_conf = low_regime
        .as_ref()
        .and_then(|r| r.confidence)
        .map(normalize_confidence)
        .unwrap_or(0.5);

    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let conviction_bias_map: HashMap<String, String> = conviction_rows
        .iter()
        .map(|c| (c.symbol.to_uppercase(), bias_from_score(c.score)))
        .collect();
    let conviction_score_map: HashMap<String, f64> = conviction_rows
        .iter()
        .map(|c| {
            (
                c.symbol.to_uppercase(),
                (c.score as f64 / 5.0).clamp(-1.0, 1.0),
            )
        })
        .collect();

    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let impact_rows = if let Some(sym) = filter_symbol {
        trends::get_impacts_for_symbol_backend(backend, &sym.to_uppercase()).unwrap_or_default()
    } else {
        trends::list_all_impacts_backend(backend).unwrap_or_default()
    };
    let mut impacts_by_symbol: HashMap<String, Vec<(trends::Trend, trends::TrendAssetImpact)>> =
        HashMap::new();
    for (trend, impact) in impact_rows {
        impacts_by_symbol
            .entry(impact.symbol.to_uppercase())
            .or_default()
            .push((trend, impact));
    }

    let mut rows = Vec::new();
    for sym in symbols {
        let medium = conviction_bias_map
            .get(&sym)
            .cloned()
            .unwrap_or_else(|| "neutral".to_string());
        let medium_signal = conviction_score_map.get(&sym).copied().unwrap_or(0.0);

        let high_impacts = impacts_by_symbol.get(&sym).cloned().unwrap_or_default();
        let bull_high = high_impacts
            .iter()
            .filter(|(_, i)| i.impact.eq_ignore_ascii_case("bullish"))
            .count();
        let bear_high = high_impacts
            .iter()
            .filter(|(_, i)| i.impact.eq_ignore_ascii_case("bearish"))
            .count();
        let high = if bull_high > bear_high {
            "bull"
        } else if bear_high > bull_high {
            "bear"
        } else {
            "neutral"
        }
        .to_string();
        let high_total = bull_high + bear_high;
        let high_signal = if high_total == 0 {
            0.0
        } else {
            (bull_high as f64 - bear_high as f64) / high_total as f64
        };

        let mut macro_bias = "neutral".to_string();
        let mut macro_signal = 0.0;
        let needle = sym.to_lowercase();
        for s in &scenarios_list {
            if let Some(ai) = s.asset_impact.as_ref() {
                let lower = ai.to_lowercase();
                if lower.contains(&needle) {
                    let direction = if lower.contains("bull") && !lower.contains("bear") {
                        1.0
                    } else if lower.contains("bear") && !lower.contains("bull") {
                        -1.0
                    } else {
                        0.0
                    };
                    macro_signal += direction * (s.probability / 100.0);
                }
            }
        }
        macro_signal = macro_signal.clamp(-1.0, 1.0);
        if macro_signal > 0.05 {
            macro_bias = "bull".to_string();
        } else if macro_signal < -0.05 {
            macro_bias = "bear".to_string();
        }

        let low_signal = bias_to_signal(&low_bias) * low_conf;
        let bull = [low_signal, medium_signal, high_signal, macro_signal]
            .iter()
            .filter(|v| **v > 0.05)
            .count();
        let bear = [low_signal, medium_signal, high_signal, macro_signal]
            .iter()
            .filter(|v| **v < -0.05)
            .count();
        let consensus = consensus_from_counts(bull, bear);
        let weighted = (0.20 * low_signal)
            + (0.30 * medium_signal)
            + (0.25 * high_signal)
            + (0.25 * macro_signal);
        let score_pct = (weighted.abs() * 100.0).clamp(0.0, 100.0);

        rows.push(AlignmentRow {
            symbol: sym,
            low: low_bias.clone(),
            medium,
            high,
            macro_bias,
            consensus,
            score_pct,
            bull_layers: bull,
            bear_layers: bear,
        });
    }
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(rows)
}

fn normalize_confidence(raw: f64) -> f64 {
    if raw > 1.0 {
        (raw / 100.0).clamp(0.0, 1.0)
    } else {
        raw.clamp(0.0, 1.0)
    }
}

fn bias_to_signal(bias: &str) -> f64 {
    match bias {
        "bull" => 1.0,
        "bear" => -1.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn macro_cycles_history_add_persists_row() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        run_macro_cycles_history_add(
            &backend,
            "US",
            "education",
            1950,
            9.0,
            Some("GI Bill expansion"),
            Some("test-source"),
            true,
        )
        .unwrap();

        let rows =
            structural::list_metric_history_backend(&backend, &["US".to_string()], None, None)
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].country, "US");
        assert_eq!(rows[0].metric, "education");
        assert_eq!(rows[0].decade, 1950);
        assert_eq!(rows[0].score, 9.0);
        assert_eq!(rows[0].notes.as_deref(), Some("GI Bill expansion"));
        assert_eq!(rows[0].source.as_deref(), Some("test-source"));
    }
}
