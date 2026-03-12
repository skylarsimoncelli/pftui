use anyhow::{bail, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};

use crate::alerts::AlertStatus;
use crate::db::backend::BackendConnection;
use crate::db::{
    alerts, price_cache,
    convictions, correlation_snapshots, regime_snapshots, research_questions, scenarios, structural,
    thesis, timeframe_signals, transactions, trends, user_predictions, watchlist,
};
use crate::db::query;
use crate::models::asset::AssetCategory;

pub fn run(
    backend: &BackendConnection,
    action: &str,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "signals" => run_signals(backend, symbol, signal_type, severity, limit, json_output),
        "summary" => run_summary(backend, json_output),
        "low" => run_low(backend, json_output),
        "medium" => run_medium(backend, json_output),
        "high" => run_high(backend, json_output),
        "macro" => run_macro(backend, json_output),
        "alignment" => run_alignment(backend, symbol, json_output),
        "gaps" => run_gaps(backend, json_output),
        _ => bail!(
            "unknown analytics action '{}'. Valid: signals, summary, low, medium, high, macro, alignment, gaps",
            action
        ),
    }
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
        format!("SELECT COUNT(*), CAST(MAX({}) AS TEXT) FROM {}", ts_col, table)
    } else {
        format!("SELECT COUNT(*), MAX({}) FROM {}", ts_col, table)
    };
    let pg_sql = if epoch_seconds {
        format!("SELECT COUNT(*)::BIGINT, MAX({})::text FROM {}", ts_col, table)
    } else {
        format!("SELECT COUNT(*)::BIGINT, MAX({})::text FROM {}", ts_col, table)
    };

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
    let specs: [(&str, &str, &str, i64, bool); 13] = [
        ("low", "price_cache", "fetched_at", 1, false),
        ("low", "price_history", "date", 48, false),
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
        println!("{:<7} {:<20} {:>8} {:<8} {:>8}", "Layer", "Table", "Records", "Status", "Age(h)");
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

fn run_signals(
    backend: &BackendConnection,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let mut rows =
        timeframe_signals::list_signals_backend(backend, signal_type, severity, limit.or(Some(25)))?;
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
                sig.severity, sig.signal_type, sig.description, sig.assets, sig.layers, sig.detected_at
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
    let trends_list = trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
    let top_trend = trends_list.first().cloned();
    let cycles = structural::list_cycles_backend(backend).unwrap_or_default();
    let top_cycle = cycles.first().cloned();
    let signal = timeframe_signals::latest_signal_backend(backend)?;
    let signal_count =
        timeframe_signals::list_signals_backend(backend, None, None, None).unwrap_or_default().len();
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
            println!("LOW: {} ({:.2})", r.regime.to_uppercase(), r.confidence.unwrap_or(0.0));
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
    let corr = correlation_snapshots::list_current_backend(backend, Some("30d")).unwrap_or_default();
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
            println!("  Regime: {} ({:.2})", r.regime, r.confidence.unwrap_or(0.0));
        }
        println!("  Correlations tracked: {}", corr.len());
        println!("  Signals: {}", signals.len());
    }

    Ok(())
}

fn run_medium(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let scenarios_list = scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let thesis_sections = thesis::list_thesis_backend(backend).unwrap_or_default();
    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let questions = research_questions::list_questions_backend(backend, Some("open")).unwrap_or_default();
    let predictions = user_predictions::list_predictions_backend(backend, Some("pending"), None, Some(20))
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
    let trends_list = trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default();
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

fn run_macro(backend: &BackendConnection, json_output: bool) -> Result<()> {
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

fn discover_alignment_symbols(backend: &BackendConnection, filter_symbol: Option<&str>) -> Vec<String> {
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
    let low_bias = regime_snapshots::get_current_backend(backend)?
        .map(|r| regime_to_bias(&r.regime).to_string())
        .unwrap_or_else(|| "neutral".to_string());

    let conviction_map: HashMap<String, String> = convictions::list_current_backend(backend)
        .unwrap_or_default()
        .into_iter()
        .map(|c| (c.symbol.to_uppercase(), bias_from_score(c.score)))
        .collect();

    let macro_outcomes = structural::list_outcomes_backend(backend).unwrap_or_default();
    let mut rows = Vec::new();
    for sym in symbols {
        let medium = conviction_map
            .get(&sym)
            .cloned()
            .unwrap_or_else(|| "neutral".to_string());

        let high_impacts = trends::get_impacts_for_symbol_backend(backend, &sym).unwrap_or_default();
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

        let mut macro_bias = "neutral".to_string();
        let needle = sym.to_lowercase();
        for o in &macro_outcomes {
            if let Some(ai) = o.asset_implications.as_ref() {
                let lower = ai.to_lowercase();
                if lower.contains(&needle) {
                    if lower.contains("bull") && !lower.contains("bear") {
                        macro_bias = "bull".to_string();
                    } else if lower.contains("bear") && !lower.contains("bull") {
                        macro_bias = "bear".to_string();
                    }
                    break;
                }
            }
        }

        let layers = [low_bias.clone(), medium.clone(), high.clone(), macro_bias.clone()];
        let bull = layers.iter().filter(|v| v.as_str() == "bull").count();
        let bear = layers.iter().filter(|v| v.as_str() == "bear").count();
        let consensus = consensus_from_counts(bull, bear);
        let score_pct = (bull.max(bear) as f64 / 4.0) * 100.0;

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
