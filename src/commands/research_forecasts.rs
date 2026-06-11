//! `pftui research forecasts ...` — retroactive forecast scoring over the
//! analyst judgment stream. Engine: `crate::research::forecast_scoring`
//! (horizon conventions live there and ONLY there).

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::research::forecast_scoring::{
    build_report, current_streaks, horizon_kind_for_days, load_rows, score_all,
    ForecastReport, HorizonKind, ReportRow, ScorePassSummary, StreakRow, LAYER_ORDER,
};

fn require_sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow!("research forecasts commands require the SQLite backend"))
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn validate_layer(layer: &str) -> Result<()> {
    if LAYER_ORDER.contains(&layer) {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid layer '{}'. Valid: {}",
            layer,
            LAYER_ORDER.join(", ")
        ))
    }
}

/// Refresh-tail entry point: skip silently on Postgres (same contract as the
/// prediction/recommendation auto-scores).
pub fn auto_score_for_refresh(backend: &BackendConnection) -> Result<ScorePassSummary> {
    let Some(conn) = backend.sqlite_native() else {
        return Ok(ScorePassSummary::default());
    };
    score_all(conn)
}

/// `pftui research forecasts score [--json]`
pub fn score_cmd(backend: &BackendConnection, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let summary = score_all(conn)?;
    if json {
        return print_json(&summary);
    }
    println!(
        "Forecast scoring: {} cell(s) examined — {} newly scored ({} neutral), {} pending, {} unscorable{}",
        summary.examined,
        summary.newly_scored,
        summary.neutral_scored,
        summary.pending,
        summary.unscorable,
        if summary.skipped_unknown_layer > 0 {
            format!(", {} skipped (unknown layer)", summary.skipped_unknown_layer)
        } else {
            String::new()
        }
    );
    println!(
        "Corpus: {} scored / {} total rows in forecast_scores",
        summary.corpus_scored_total, summary.corpus_total
    );
    Ok(())
}

fn fmt_opt_pct(value: Option<f64>) -> String {
    value.map(|v| format!("{v:+.1}%")).unwrap_or_else(|| "—".to_string())
}

fn fmt_horizon(days: i64) -> String {
    match horizon_kind_for_days(days) {
        HorizonKind::Trading => format!("{days}td"),
        HorizonKind::Calendar => format!("{days}d"),
    }
}

fn render_report_row(row: &ReportRow) {
    let hit = row
        .hit_rate_pct
        .map(|v| format!("{v:.0}%"))
        .unwrap_or_else(|| "—".to_string());
    let mw = row
        .mean_weighted_score
        .map(|v| format!("{v:+.2}"))
        .unwrap_or_else(|| "—".to_string());
    let streak = if row.current_miss_streak > 0 {
        format!(
            "{} {} miss(es)",
            row.current_miss_streak,
            row.streak_call.as_deref().unwrap_or("?")
        )
    } else {
        "—".to_string()
    };
    println!(
        "  {:<11} {:<9} {:>5}  n={:<3} neut={:<3} pend={:<3} hit={:<4} w={:<6} bull→{:<8} bear→{:<8} streak: {}",
        row.layer,
        row.asset,
        fmt_horizon(row.horizon_days),
        row.n_scored,
        row.n_neutral,
        row.n_pending,
        hit,
        mw,
        fmt_opt_pct(row.mean_realized_bull_pct),
        fmt_opt_pct(row.mean_realized_bear_pct),
        streak
    );
}

#[derive(Serialize)]
struct ReportPayload<'a> {
    layer: Option<&'a str>,
    asset: Option<&'a str>,
    window_days: Option<i64>,
    report: ForecastReport,
}

/// `pftui research forecasts report [--layer X] [--asset Y] [--window-days N] [--json]`
pub fn report_cmd(
    backend: &BackendConnection,
    layer: Option<&str>,
    asset: Option<&str>,
    window_days: Option<i64>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    if let Some(layer) = layer {
        validate_layer(layer)?;
    }
    let rows = load_rows(conn, layer, asset, window_days)?;
    let report = build_report(&rows);
    if json {
        return print_json(&ReportPayload {
            layer,
            asset,
            window_days,
            report,
        });
    }
    if report.rows.is_empty() {
        println!("No forecast scores match. Run `pftui research forecasts score` first.");
        return Ok(());
    }
    println!(
        "FORECAST SCORE REPORT (layer × asset × horizon; hit rate over non-neutral scored views)"
    );
    let mut current_layer = String::new();
    for row in &report.rows {
        if row.layer != current_layer {
            current_layer = row.layer.clone();
            println!("{}", current_layer.to_uppercase());
        }
        render_report_row(row);
    }
    println!("TOTALS (per layer × horizon)");
    for row in &report.totals {
        render_report_row(row);
    }
    Ok(())
}

#[derive(Serialize)]
struct StreaksPayload {
    threshold: usize,
    streaks: Vec<StreakRow>,
}

/// `pftui research forecasts streaks [--threshold N] [--json]`
pub fn streaks_cmd(backend: &BackendConnection, threshold: usize, json: bool) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = load_rows(conn, None, None, None)?;
    let streaks = current_streaks(&rows, threshold);
    if json {
        return print_json(&StreaksPayload { threshold, streaks });
    }
    if streaks.is_empty() {
        println!("No (layer, asset) with a current wrong-sign streak ≥ {threshold}.");
        return Ok(());
    }
    println!(
        "CURRENT WRONG-SIGN STREAKS ≥ {threshold} (consecutive same-sign misses, most recent calls)"
    );
    for s in &streaks {
        println!(
            "  {:<11} {:<9} {:>5}  {} consecutive {} misses  {} → {}  cumulative move against calls: {:+.1}%",
            s.layer,
            s.asset,
            fmt_horizon(s.horizon_days),
            s.streak_len,
            s.call,
            s.first_view_date,
            s.last_view_date,
            s.cumulative_realized_pct
        );
    }
    Ok(())
}
