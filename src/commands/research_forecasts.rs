//! `pftui research forecasts ...` — retroactive forecast scoring over the
//! analyst judgment stream. Engine: `crate::research::forecast_scoring`
//! (horizon conventions live there and ONLY there).

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::research::forecast_scoring::{
    build_report, current_streaks, horizon_kind_for_days, load_rows, reissue_drifted,
    score_all, verify_scores, ForecastReport, HorizonKind, ReissueSummary, ReportRow,
    ScorePassSummary, StreakRow, VerifyReport, LAYER_ORDER,
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
struct VerifyPayload {
    report: VerifyReport,
    reissue: Option<ReissueSummary>,
    journal_note_id: Option<i64>,
}

/// Cap on per-row drift lines in text output (JSON carries everything).
const VERIFY_TEXT_ROW_CAP: usize = 50;

/// `pftui research forecasts verify [--threshold-pp 0.5] [--reissue] [--json]`
///
/// Recomputes every SCORED row's realized return against TODAY'S price
/// series WITHOUT mutating the append-only ledger. With `--reissue`,
/// drifted rows are marked `superseded`, corrected rows are inserted, and
/// the action is journaled (daily_notes, author `system`).
pub fn verify_cmd(
    backend: &BackendConnection,
    threshold_pp: f64,
    reissue: bool,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let report = verify_scores(conn, threshold_pp)?;

    let mut reissue_summary: Option<ReissueSummary> = None;
    let mut journal_note_id: Option<i64> = None;
    if reissue && report.drifted > 0 {
        let summary = reissue_drifted(conn, threshold_pp)?;
        if summary.superseded > 0 {
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let content = format!(
                "forecast_scores reissue: {} drifted row(s) (|recomputed − stored| > {}pp vs today's repaired price series) marked status='superseded'; {} corrected row(s) inserted. Assets: {}. Action: pftui research forecasts verify --reissue. Ledger doctrine: superseded rows retained, never deleted.",
                summary.superseded,
                threshold_pp,
                summary.inserted,
                summary.assets.join(", "),
            );
            journal_note_id = Some(crate::db::daily_notes::add_note(
                conn,
                &today,
                "data-integrity",
                &content,
                "system",
            )?);
        }
        reissue_summary = Some(summary);
    }

    if json {
        return print_json(&VerifyPayload {
            report,
            reissue: reissue_summary,
            journal_note_id,
        });
    }

    println!(
        "FORECAST SCORE VERIFICATION — recompute vs today's price series (threshold {}pp{})",
        report.threshold_pp,
        if reissue { "; --reissue" } else { "; read-only" }
    );
    println!(
        "  scored rows checked: {} — clean: {}, drifted: {}, unresolvable: {}, series-changed: {}, hit-flips: {}",
        report.scored_rows,
        report.clean,
        report.drifted,
        report.unresolvable,
        report.series_changed,
        report.hit_flips
    );
    if report.drift_rows.is_empty() {
        println!("  ✓ ledger verified clean — every scored row reproduces from the current series");
        return Ok(());
    }
    println!();
    println!(
        "  {:<11} {:<9} {:>6} {:<10} {:>9} → {:<9} {:>8}  series",
        "LAYER", "ASSET", "HZN", "VIEW DATE", "stored", "recomputed", "drift"
    );
    for d in report.drift_rows.iter().take(VERIFY_TEXT_ROW_CAP) {
        println!(
            "  {:<11} {:<9} {:>6} {:<10} {:>8.2}% → {:<9} {:>7}  {}{}",
            d.analyst,
            d.asset,
            fmt_horizon(d.horizon_days),
            d.view_date,
            d.stored_realized_pct,
            d.recomputed_realized_pct
                .map(|v| format!("{v:.2}%"))
                .unwrap_or_else(|| "—".to_string()),
            d.drift_pp
                .map(|v| format!("{v:.2}pp"))
                .unwrap_or_else(|| "n/a".to_string()),
            d.stored_series.as_deref().unwrap_or("?"),
            if d.recomputed_series.is_some()
                && d.recomputed_series != d.stored_series
            {
                format!(" → {}", d.recomputed_series.as_deref().unwrap_or("?"))
            } else if d.recomputed_series.is_none() {
                " (no longer resolvable)".to_string()
            } else {
                String::new()
            }
        );
    }
    if report.drift_rows.len() > VERIFY_TEXT_ROW_CAP {
        println!(
            "  … {} more row(s) — use --json for the full list",
            report.drift_rows.len() - VERIFY_TEXT_ROW_CAP
        );
    }
    println!();
    println!("  PER ASSET / SERIES (groups with drift or unresolvable rows):");
    for g in report
        .per_series
        .iter()
        .filter(|g| g.rows_drifted > 0 || g.rows_unresolvable > 0)
    {
        println!(
            "    {:<9} via {:<11} checked={:<4} drifted={:<4} unresolvable={:<4} max={:.2}pp mean={:.2}pp",
            g.asset,
            g.series_used,
            g.rows_checked,
            g.rows_drifted,
            g.rows_unresolvable,
            g.max_drift_pp,
            g.mean_drift_pp
        );
    }
    match &reissue_summary {
        Some(s) => {
            println!();
            println!(
                "  REISSUED: {} row(s) superseded, {} corrected row(s) inserted (assets: {}){}",
                s.superseded,
                s.inserted,
                s.assets.join(", "),
                journal_note_id
                    .map(|id| format!(" — journaled as daily note #{id}"))
                    .unwrap_or_default()
            );
        }
        None if report.drifted > 0 => {
            println!();
            println!(
                "  Remediation (append-only — drifted rows are never edited in place):"
            );
            println!("    pftui research forecasts verify --reissue");
        }
        None => {}
    }
    Ok(())
}

/// Refresh-tail misalignment detection (runs right after the retro-score).
/// Skips silently on Postgres, same contract as the scoring passes.
pub fn detect_misalignments_for_refresh(
    backend: &BackendConnection,
) -> Result<crate::db::forecast_misalignments::DetectionSummary> {
    let Some(conn) = backend.sqlite_native() else {
        return Ok(Default::default());
    };
    crate::db::forecast_misalignments::detect_and_update(conn)
}

/// `pftui research misalignments [--all] [--json]`
pub fn misalignments_cmd(backend: &BackendConnection, all: bool, json: bool) -> Result<()> {
    use crate::db::forecast_misalignments as fm;
    let conn = require_sqlite(backend)?;
    let rows = if all {
        fm::list_all(conn)?
    } else {
        fm::active_misalignments(conn)?
    };
    if json {
        return print_json(&serde_json::json!({
            "scope": if all { "all" } else { "active" },
            "threshold": fm::MISALIGNMENT_STREAK_THRESHOLD,
            "misalignments": rows,
        }));
    }
    if rows.is_empty() {
        if all {
            println!("No misalignment episodes recorded yet.");
        } else {
            println!(
                "No active forecast misalignments (streak threshold {}).",
                fm::MISALIGNMENT_STREAK_THRESHOLD
            );
        }
        return Ok(());
    }
    println!(
        "FORECAST MISALIGNMENTS ({}; wrong-sign streak ≥ {})",
        if all { "full ledger" } else { "active" },
        fm::MISALIGNMENT_STREAK_THRESHOLD
    );
    println!(
        "  {:<4} {:<8} {:<9} {:>6} {:<5} {:<23} {:>9}  {:<12} recovered_at",
        "id", "layer", "asset", "streak", "call", "span", "against", "status"
    );
    for m in &rows {
        println!(
            "  {:<4} {:<8} {:<9} {:>6} {:<5} {:<23} {:>8.1}%  {:<12} {}",
            m.id,
            m.layer,
            m.asset,
            m.streak_len,
            m.call,
            format!("{} → {}", m.span_start, m.span_end),
            m.cum_realized_against_pct,
            m.status,
            m.recovered_at.as_deref().unwrap_or("—"),
        );
    }
    let active_count = rows.iter().filter(|m| m.status == "active").count();
    if active_count > 0 {
        println!(
            "\n{} active — probation in force: these (layer, asset) views are excluded from convergence voting and prediction confidence is capped at 0.25.",
            active_count
        );
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
