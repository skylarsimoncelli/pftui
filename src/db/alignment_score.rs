//! Operator-vs-analyst daily alignment score.
//!
//! Aggregates the per-asset gap between Skylar's stated views (journal entries
//! authored 'skylar', the optional `operator_replies` table, recent
//! transactions) and the analyst convergence per asset
//! (`db::analyst_views::convergence_report_backend`). Each held asset above 1%
//! allocation contributes its allocation-weighted class score; the daily total
//! collapses to a 0-100 number with a regime label.
//!
//! Storage: `alignment_score_history(date, total_alignment_score, components,
//! divergent_assets, regime_state, computed_at)`. `components` and
//! `divergent_assets` are JSON.

use anyhow::Result;
use chrono::{Duration, NaiveDate, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::analyst_views::{convergence_report_backend, ConvergenceReport};
use crate::db::backend::BackendConnection;
use crate::db::query;

/// One asset's contribution to a daily alignment score.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignmentComponent {
    pub symbol: String,
    pub allocation_weight: f64,
    pub operator_view: Option<OperatorView>,
    pub analyst_summary: String,
    pub analyst_avg_conviction: f64,
    pub alignment_class: String,
    pub class_score: f64,
}

/// Operator-side direction + magnitude derived from journal/operator_replies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperatorView {
    pub direction: String,
    pub conviction_magnitude: i64,
    pub source: String,
    pub recorded_at: String,
}

/// One row of `alignment_score_history`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentScoreRow {
    pub date: String,
    pub total_alignment_score: f64,
    pub components: Vec<AlignmentComponent>,
    pub divergent_assets: Vec<String>,
    pub regime_state: String,
    pub computed_at: String,
}

/// Input shape: one held asset above 1% allocation.
#[derive(Debug, Clone)]
pub struct HeldAsset {
    pub symbol: String,
    pub allocation_pct: f64,
}

// ---------------------------------------------------------------------------
// Direction + classification helpers (pure functions — easy to unit-test)
// ---------------------------------------------------------------------------

/// Map the convergence-summary classification onto a coarse direction.
/// `None` means the summary is not actionable (insufficient or pure divergence).
pub fn analyst_direction_from_summary(summary: &str) -> Option<&'static str> {
    match summary {
        "strong-convergent-bull" | "convergent-bull" => Some("bull"),
        "strong-convergent-bear" | "convergent-bear" => Some("bear"),
        "convergent-neutral" => Some("neutral"),
        // divergent / neutral-with-divergence / insufficient-views → no clean direction
        _ => None,
    }
}

/// Round-toward-int magnitude for the analyst's avg conviction.
pub fn analyst_magnitude_from_avg(avg: f64) -> i64 {
    avg.round().abs() as i64
}

/// Classify the per-asset alignment from (operator view, analyst view).
///
/// Returns one of:
///   `aligned`              — same direction, similar magnitude (|Δ| ≤ 2)
///   `divergent-magnitude`  — same direction, very different magnitude (|Δ| > 2)
///   `divergent-direction`  — opposite directions
///   `insufficient-views`   — either side missing or analyst-direction unactionable
pub fn classify_alignment(
    operator_dir: Option<&str>,
    operator_mag: i64,
    analyst_dir: Option<&str>,
    analyst_mag: i64,
) -> &'static str {
    let (op, an) = match (operator_dir, analyst_dir) {
        (Some(o), Some(a)) => (o, a),
        _ => return "insufficient-views",
    };
    if op == "neutral" && an == "neutral" {
        return "aligned";
    }
    // Opposite directions (bull vs bear) → divergent-direction.
    let opposite = matches!(
        (op, an),
        ("bull", "bear") | ("bear", "bull")
    );
    if opposite {
        return "divergent-direction";
    }
    // One side neutral, the other a direction → treat as divergent-direction
    // (the operator and analyst disagree on whether action is warranted).
    if (op == "neutral") ^ (an == "neutral") {
        return "divergent-direction";
    }
    // Same direction — compare magnitudes.
    let diff = (operator_mag - analyst_mag).abs();
    if diff > 2 {
        "divergent-magnitude"
    } else {
        "aligned"
    }
}

/// Score contribution for an alignment class.
pub fn class_score(class: &str) -> Option<f64> {
    match class {
        "aligned" => Some(100.0),
        "divergent-magnitude" => Some(50.0),
        "divergent-direction" => Some(0.0),
        _ => None,
    }
}

/// Map a 0-100 total alignment score onto a regime label.
pub fn regime_state(score: f64) -> &'static str {
    if score >= 80.0 {
        "high-alignment"
    } else if score >= 50.0 {
        "mixed"
    } else {
        "divergent"
    }
}

/// Parse a coarse direction + conviction magnitude from a journal entry body.
///
/// This is a heuristic — the journal is free-form prose. We anchor on the
/// most explicit keywords first and fall back to direction-only when the
/// magnitude is ambiguous. Returns `None` if no direction keyword fires.
pub fn parse_journal_direction(content: &str) -> Option<(&'static str, i64)> {
    let lower = content.to_lowercase();
    // Strong negative phrases beat strong positive when both appear (rare).
    let bull_kw = ["bullish", "long ", "long\n", "long.", "buying", "accumulating", "added to ", "rip ", "moon"];
    let bear_kw = ["bearish", "short ", "short\n", "short.", "selling", "trimming", "trimmed", "dumping", "puked"];
    let strong_kw = [
        "strongly", "high conviction", "max conviction", "all-in", "pounding the table",
        "very bullish", "very bearish",
    ];
    let light_kw = [
        "lightly", "low conviction", "small ", "small\n", "skeptical", "leaning", "tentative",
    ];

    let bull_hit = bull_kw.iter().any(|k| lower.contains(k));
    let bear_hit = bear_kw.iter().any(|k| lower.contains(k));

    let direction = match (bull_hit, bear_hit) {
        (true, false) => "bull",
        (false, true) => "bear",
        (true, true) => {
            // Take the keyword that appears LAST as the conclusion.
            let bull_idx = bull_kw.iter().filter_map(|k| lower.rfind(k)).max();
            let bear_idx = bear_kw.iter().filter_map(|k| lower.rfind(k)).max();
            match (bull_idx, bear_idx) {
                (Some(b), Some(s)) if b >= s => "bull",
                (Some(_), Some(_)) => "bear",
                _ => return None,
            }
        }
        (false, false) => {
            // Allow an explicit "neutral" / "hold" / "no change" entry.
            if lower.contains("neutral")
                || lower.contains("flat")
                || lower.contains("no change")
                || lower.contains("holding")
                || lower.contains("on hold")
            {
                return Some(("neutral", 0));
            }
            return None;
        }
    };

    let magnitude = if strong_kw.iter().any(|k| lower.contains(k)) {
        4
    } else if light_kw.iter().any(|k| lower.contains(k)) {
        1
    } else {
        2
    };

    Some((direction, magnitude))
}

// ---------------------------------------------------------------------------
// Operator-view loading
// ---------------------------------------------------------------------------

/// Load the operator's most-recent stated view for `symbol`.
///
/// Priority order:
///   1. `operator_replies` table (if it exists in the DB) — explicit reply tied to the asset
///   2. journal entries authored 'skylar' for the asset in the last 14 days
fn load_operator_view_sqlite(conn: &Connection, symbol: &str) -> Result<Option<OperatorView>> {
    // (1) operator_replies — the table is optional. Check existence first.
    let has_replies: bool = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='operator_replies'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;

    if has_replies {
        // We don't own the schema, so be liberal: look for an `asset` (or `symbol`)
        // column and a free-form `content` (or `reply`) column with a timestamp.
        let row: Option<(Option<String>, String, String)> = conn
            .prepare(
                "SELECT
                   COALESCE(direction, ''),
                   COALESCE(content, reply, ''),
                   COALESCE(created_at, recorded_at, '')
                 FROM operator_replies
                 WHERE UPPER(COALESCE(asset, symbol, '')) = UPPER(?1)
                 ORDER BY COALESCE(created_at, recorded_at, '') DESC
                 LIMIT 1",
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_row(params![symbol], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .ok()
            });
        if let Some((dir_col, content, recorded_at)) = row {
            // Prefer the structured direction column if set; otherwise parse content.
            let parsed = dir_col
                .as_deref()
                .filter(|s| !s.is_empty())
                .and_then(|d| {
                    let lower = d.to_lowercase();
                    if lower.contains("bull") || lower == "long" {
                        Some(("bull", 2))
                    } else if lower.contains("bear") || lower == "short" {
                        Some(("bear", 2))
                    } else if lower.contains("neutral") || lower == "hold" {
                        Some(("neutral", 0))
                    } else {
                        None
                    }
                })
                .or_else(|| parse_journal_direction(&content));
            if let Some((direction, mag)) = parsed {
                return Ok(Some(OperatorView {
                    direction: direction.to_string(),
                    conviction_magnitude: mag,
                    source: "operator_replies".to_string(),
                    recorded_at,
                }));
            }
        }
    }

    // (2) journal entries authored 'skylar' last 14 days, matching this asset.
    let since = (Utc::now() - Duration::days(14)).to_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT content, timestamp
           FROM journal
          WHERE author = 'skylar'
            AND timestamp >= ?1
            AND (UPPER(COALESCE(symbol, '')) = UPPER(?2)
                 OR INSTR(UPPER(content), UPPER(?2)) > 0)
          ORDER BY timestamp DESC
          LIMIT 25",
    )?;
    let rows = stmt.query_map(params![since, symbol], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (content, ts) = row?;
        if let Some((direction, mag)) = parse_journal_direction(&content) {
            return Ok(Some(OperatorView {
                direction: direction.to_string(),
                conviction_magnitude: mag,
                source: "journal".to_string(),
                recorded_at: ts,
            }));
        }
    }

    Ok(None)
}

pub fn load_operator_view_backend(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<Option<OperatorView>> {
    query::dispatch(
        backend,
        |conn| load_operator_view_sqlite(conn, symbol),
        // Postgres backend — keep parity by returning None (the alignment
        // score is a SQLite-first feature; postgres support can extend later).
        |_pool| Ok(None),
    )
}

// ---------------------------------------------------------------------------
// Per-asset alignment + daily aggregation
// ---------------------------------------------------------------------------

/// Compute one asset's alignment component given an analyst convergence report
/// and the operator's stated view (if any).
pub fn build_component(
    symbol: &str,
    allocation_pct: f64,
    operator: Option<OperatorView>,
    analyst: &ConvergenceReport,
) -> AlignmentComponent {
    let analyst_dir = analyst_direction_from_summary(&analyst.summary);
    let analyst_mag = analyst_magnitude_from_avg(analyst.stats.avg_conviction);
    let operator_dir = operator.as_ref().map(|o| o.direction.as_str());
    let operator_mag = operator.as_ref().map(|o| o.conviction_magnitude).unwrap_or(0);

    let class = classify_alignment(operator_dir, operator_mag, analyst_dir, analyst_mag);
    let score = class_score(class).unwrap_or(0.0);
    AlignmentComponent {
        symbol: symbol.to_string(),
        allocation_weight: allocation_pct,
        operator_view: operator,
        analyst_summary: analyst.summary.clone(),
        analyst_avg_conviction: analyst.stats.avg_conviction,
        alignment_class: class.to_string(),
        class_score: score,
    }
}

/// Compute the daily alignment score for a set of held assets.
///
/// For each held asset above 1% allocation:
///   - load its analyst convergence report
///   - load the operator's most-recent view
///   - classify alignment, multiply class score by allocation weight
///   - the daily total = sum(weighted scores) / sum(weights)
///
/// Components with `insufficient-views` carry no weight in the total.
pub fn compute_for_date(
    backend: &BackendConnection,
    held: &[HeldAsset],
    date: NaiveDate,
    convergence_window: Option<&str>,
) -> Result<AlignmentScoreRow> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let mut components = Vec::new();
    let mut weighted_sum = 0.0;
    let mut weight_total = 0.0;
    let mut divergent = Vec::new();

    for asset in held {
        if asset.allocation_pct < 1.0 {
            continue;
        }
        let analyst = match convergence_report_backend(backend, &asset.symbol, convergence_window) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let operator = load_operator_view_backend(backend, &asset.symbol).unwrap_or(None);
        let comp = build_component(&asset.symbol, asset.allocation_pct, operator, &analyst);
        if comp.alignment_class != "insufficient-views" {
            weighted_sum += comp.allocation_weight * comp.class_score;
            weight_total += comp.allocation_weight;
            if comp.alignment_class.starts_with("divergent") {
                divergent.push(comp.symbol.clone());
            }
        }
        components.push(comp);
    }

    let total = if weight_total > 0.0 {
        (weighted_sum / weight_total * 10.0).round() / 10.0
    } else {
        // No actionable signals → call it the neutral mid-point so the regime
        // does not falsely trigger "divergent" on a day with no operator views.
        50.0
    };
    let regime = regime_state(total).to_string();
    Ok(AlignmentScoreRow {
        date: date_str.clone(),
        total_alignment_score: total,
        components,
        divergent_assets: divergent,
        regime_state: regime,
        computed_at: Utc::now().to_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Persist (or replace) one row in `alignment_score_history`.
pub fn upsert_row_sqlite(conn: &Connection, row: &AlignmentScoreRow) -> Result<()> {
    let components_json = serde_json::to_string(&row.components)?;
    let divergent_json = serde_json::to_string(&row.divergent_assets)?;
    conn.execute(
        "INSERT INTO alignment_score_history (date, total_alignment_score, components,
            divergent_assets, regime_state, computed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(date) DO UPDATE SET
             total_alignment_score = excluded.total_alignment_score,
             components = excluded.components,
             divergent_assets = excluded.divergent_assets,
             regime_state = excluded.regime_state,
             computed_at = excluded.computed_at",
        params![
            row.date,
            row.total_alignment_score,
            components_json,
            divergent_json,
            row.regime_state,
            row.computed_at,
        ],
    )?;
    Ok(())
}

pub fn upsert_row_backend(backend: &BackendConnection, row: &AlignmentScoreRow) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_row_sqlite(conn, row),
        |_pool| {
            anyhow::bail!("alignment_score_history is SQLite-only for now");
        },
    )
}

fn row_from_db_sqlite(conn: &Connection, date: &str) -> Result<Option<AlignmentScoreRow>> {
    let mut stmt = conn.prepare(
        "SELECT date, total_alignment_score, components, divergent_assets,
                regime_state, computed_at
           FROM alignment_score_history
          WHERE date = ?1",
    )?;
    let mut rows = stmt.query(params![date])?;
    if let Some(row) = rows.next()? {
        let date: String = row.get(0)?;
        let total: f64 = row.get(1)?;
        let components_json: String = row.get(2)?;
        let divergent_json: String = row.get(3)?;
        let regime: Option<String> = row.get(4)?;
        let computed_at: String = row.get(5)?;
        let components: Vec<AlignmentComponent> =
            serde_json::from_str(&components_json).unwrap_or_default();
        let divergent_assets: Vec<String> =
            serde_json::from_str(&divergent_json).unwrap_or_default();
        Ok(Some(AlignmentScoreRow {
            date,
            total_alignment_score: total,
            components,
            divergent_assets,
            regime_state: regime.unwrap_or_default(),
            computed_at,
        }))
    } else {
        Ok(None)
    }
}

pub fn get_row_backend(
    backend: &BackendConnection,
    date: &str,
) -> Result<Option<AlignmentScoreRow>> {
    query::dispatch(
        backend,
        |conn| row_from_db_sqlite(conn, date),
        |_pool| Ok(None),
    )
}

fn history_sqlite(conn: &Connection, since: Option<&str>) -> Result<Vec<AlignmentScoreRow>> {
    let mut sql = String::from(
        "SELECT date, total_alignment_score, components, divergent_assets,
                regime_state, computed_at
           FROM alignment_score_history",
    );
    if let Some(s) = since {
        sql.push_str(&format!(" WHERE date >= '{}'", s.replace('\'', "")));
    }
    sql.push_str(" ORDER BY date ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (date, total, comp_json, div_json, regime, computed_at) = r?;
        let components: Vec<AlignmentComponent> =
            serde_json::from_str(&comp_json).unwrap_or_default();
        let divergent_assets: Vec<String> = serde_json::from_str(&div_json).unwrap_or_default();
        out.push(AlignmentScoreRow {
            date,
            total_alignment_score: total,
            components,
            divergent_assets,
            regime_state: regime.unwrap_or_default(),
            computed_at,
        });
    }
    Ok(out)
}

pub fn history_backend(
    backend: &BackendConnection,
    since: Option<&str>,
) -> Result<Vec<AlignmentScoreRow>> {
    query::dispatch(
        backend,
        |conn| history_sqlite(conn, since),
        |_pool| Ok(Vec::new()),
    )
}

/// Parse a `--since` token (Nd, Nw, Nm, or a YYYY-MM-DD date) into an absolute
/// YYYY-MM-DD anchor used by the history query.
pub fn parse_since_token(value: &str) -> Result<String> {
    let today = Utc::now().date_naive();
    if let Some(stripped) = value.strip_suffix('d') {
        let n: i64 = stripped.parse()?;
        return Ok((today - Duration::days(n)).format("%Y-%m-%d").to_string());
    }
    if let Some(stripped) = value.strip_suffix('w') {
        let n: i64 = stripped.parse()?;
        return Ok((today - Duration::weeks(n)).format("%Y-%m-%d").to_string());
    }
    if let Some(stripped) = value.strip_suffix('m') {
        let n: i64 = stripped.parse()?;
        return Ok((today - Duration::days(n * 30))
            .format("%Y-%m-%d")
            .to_string());
    }
    if let Ok(d) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Ok(d.format("%Y-%m-%d").to_string());
    }
    anyhow::bail!(
        "could not parse --since '{}': expected Nd, Nw, Nm, or YYYY-MM-DD",
        value
    )
}

// ---------------------------------------------------------------------------
// Threshold alert: emit an agent_messages row when alignment stays below 50
// for 2+ consecutive days.
// ---------------------------------------------------------------------------

/// Returns the number of consecutive most-recent rows with score < 50.
pub fn consecutive_low_streak(history: &[AlignmentScoreRow]) -> usize {
    // history is ordered ASC by date — walk back from the end.
    let mut count = 0;
    for row in history.iter().rev() {
        if row.total_alignment_score < 50.0 {
            count += 1;
        } else {
            break;
        }
    }
    count
}

/// Emit an `agent_messages` row to `synthesis` when the streak is ≥ 2 and we
/// have not already alerted for the most recent date.
///
/// Returns `Some(message_id)` if a row was inserted, or `None` if no alert
/// was warranted or one already exists for today.
pub fn maybe_emit_drift_alert_backend(
    backend: &BackendConnection,
    history: &[AlignmentScoreRow],
) -> Result<Option<i64>> {
    let streak = consecutive_low_streak(history);
    if streak < 2 {
        return Ok(None);
    }
    let last = match history.last() {
        Some(r) => r,
        None => return Ok(None),
    };
    let today_str = last.date.clone();
    // Idempotency: don't double-emit on the same date.
    let already: bool = query::dispatch(
        backend,
        |conn| {
            let count: i64 = conn
                .prepare(
                    "SELECT COUNT(*) FROM agent_messages
                      WHERE from_agent = 'alignment-score'
                        AND to_agent = 'synthesis'
                        AND category = 'signal'
                        AND content LIKE ?1",
                )?
                .query_row(params![format!("%{}%", today_str)], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
            Ok(count > 0)
        },
        |_pool| Ok(false),
    )?;
    if already {
        return Ok(None);
    }
    let content = format!(
        "Operator/analyst regime drift: alignment score has been below 50 for {streak} consecutive days (latest {today_str} = {score:.1}). Divergent assets: {divergent:?}.",
        streak = streak,
        today_str = today_str,
        score = last.total_alignment_score,
        divergent = last.divergent_assets,
    );
    let id = crate::db::agent_messages::send_message_backend(
        backend,
        "alignment-score",
        Some("synthesis"),
        Some("normal"),
        &content,
        Some("signal"),
        None,
        None,
        None,
    )?;
    Ok(Some(id))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::analyst_views::ConvergenceStats;

    fn report_with(summary: &str, avg: f64) -> ConvergenceReport {
        ConvergenceReport {
            asset: "BTC".to_string(),
            as_of: "2026-06-01T00:00:00Z".to_string(),
            views: vec![],
            stats: ConvergenceStats {
                n_views: 4,
                avg_conviction: avg,
                min_conviction: 0,
                max_conviction: 0,
                max_divergence: 0,
                alloc_bias_counts: Default::default(),
            },
            summary: summary.to_string(),
        }
    }

    #[test]
    fn direction_map_covers_summaries() {
        assert_eq!(analyst_direction_from_summary("strong-convergent-bull"), Some("bull"));
        assert_eq!(analyst_direction_from_summary("convergent-bear"), Some("bear"));
        assert_eq!(analyst_direction_from_summary("convergent-neutral"), Some("neutral"));
        assert_eq!(analyst_direction_from_summary("divergent"), None);
        assert_eq!(analyst_direction_from_summary("insufficient-views"), None);
    }

    #[test]
    fn classify_aligned_same_direction_close_magnitude() {
        let class = classify_alignment(Some("bull"), 3, Some("bull"), 2);
        assert_eq!(class, "aligned");
    }

    #[test]
    fn classify_divergent_magnitude_same_direction_far_magnitude() {
        let class = classify_alignment(Some("bull"), 4, Some("bull"), 1);
        assert_eq!(class, "divergent-magnitude");
    }

    #[test]
    fn classify_divergent_direction_opposite() {
        let class = classify_alignment(Some("bull"), 3, Some("bear"), 3);
        assert_eq!(class, "divergent-direction");
    }

    #[test]
    fn classify_divergent_when_one_side_neutral() {
        let class = classify_alignment(Some("neutral"), 0, Some("bull"), 3);
        assert_eq!(class, "divergent-direction");
    }

    #[test]
    fn classify_insufficient_when_missing() {
        let class = classify_alignment(None, 0, Some("bull"), 3);
        assert_eq!(class, "insufficient-views");
        let class = classify_alignment(Some("bull"), 3, None, 0);
        assert_eq!(class, "insufficient-views");
    }

    #[test]
    fn regime_thresholds() {
        assert_eq!(regime_state(85.0), "high-alignment");
        assert_eq!(regime_state(80.0), "high-alignment");
        assert_eq!(regime_state(60.0), "mixed");
        assert_eq!(regime_state(50.0), "mixed");
        assert_eq!(regime_state(49.9), "divergent");
        assert_eq!(regime_state(0.0), "divergent");
    }

    #[test]
    fn class_scores_map_to_expected_numbers() {
        assert_eq!(class_score("aligned"), Some(100.0));
        assert_eq!(class_score("divergent-magnitude"), Some(50.0));
        assert_eq!(class_score("divergent-direction"), Some(0.0));
        assert_eq!(class_score("insufficient-views"), None);
    }

    #[test]
    fn parse_journal_picks_bullish() {
        let parsed = parse_journal_direction("Feeling strongly bullish on gold here.");
        assert_eq!(parsed, Some(("bull", 4)));
    }

    #[test]
    fn parse_journal_picks_bearish_low_conviction() {
        let parsed = parse_journal_direction("Lightly trimming BTC, leaning bearish for the week.");
        assert_eq!(parsed, Some(("bear", 1)));
    }

    #[test]
    fn parse_journal_neutral_phrase() {
        let parsed = parse_journal_direction("Holding gold here, no change in stance.");
        assert_eq!(parsed, Some(("neutral", 0)));
    }

    #[test]
    fn parse_journal_no_signal() {
        let parsed = parse_journal_direction("Bought groceries.");
        assert!(parsed.is_none());
    }

    #[test]
    fn build_component_aligned_carries_weight() {
        let analyst = report_with("convergent-bull", 2.0);
        let operator = Some(OperatorView {
            direction: "bull".to_string(),
            conviction_magnitude: 3,
            source: "journal".to_string(),
            recorded_at: "2026-05-31T12:00:00Z".to_string(),
        });
        let comp = build_component("BTC", 30.0, operator, &analyst);
        assert_eq!(comp.alignment_class, "aligned");
        assert_eq!(comp.class_score, 100.0);
        assert_eq!(comp.allocation_weight, 30.0);
    }

    #[test]
    fn build_component_divergent_direction() {
        let analyst = report_with("convergent-bear", -2.0);
        let operator = Some(OperatorView {
            direction: "bull".to_string(),
            conviction_magnitude: 3,
            source: "journal".to_string(),
            recorded_at: "2026-05-31T12:00:00Z".to_string(),
        });
        let comp = build_component("BTC", 20.0, operator, &analyst);
        assert_eq!(comp.alignment_class, "divergent-direction");
        assert_eq!(comp.class_score, 0.0);
    }

    #[test]
    fn build_component_insufficient_when_analyst_divergent() {
        let analyst = report_with("divergent", 0.0);
        let operator = Some(OperatorView {
            direction: "bull".to_string(),
            conviction_magnitude: 3,
            source: "journal".to_string(),
            recorded_at: "2026-05-31T12:00:00Z".to_string(),
        });
        let comp = build_component("BTC", 20.0, operator, &analyst);
        assert_eq!(comp.alignment_class, "insufficient-views");
    }

    #[test]
    fn weighted_total_excludes_insufficient_components() {
        // Manually verify the weighting formula used by `compute_for_date`.
        let comps: Vec<(&str, f64, f64)> = vec![
            ("aligned", 50.0, 100.0),
            ("divergent-direction", 20.0, 0.0),
            ("insufficient-views", 30.0, 0.0), // excluded
        ];
        let mut weighted: f64 = 0.0;
        let mut total: f64 = 0.0;
        for (class, w, s) in &comps {
            if *class != "insufficient-views" {
                weighted += w * s;
                total += w;
            }
        }
        let score: f64 = weighted / total;
        // 50*100 / (50+20) = 5000/70 = 71.43...
        assert!((score - 71.428571_f64).abs() < 0.001);
    }

    #[test]
    fn consecutive_low_streak_counts_only_recent_trailing() {
        let mut rows = Vec::new();
        for (date, score) in [
            ("2026-05-25", 80.0),
            ("2026-05-26", 30.0), // break in the streak — older
            ("2026-05-27", 70.0), // back above 50
            ("2026-05-28", 40.0),
            ("2026-05-29", 35.0),
        ] {
            rows.push(AlignmentScoreRow {
                date: date.to_string(),
                total_alignment_score: score,
                components: vec![],
                divergent_assets: vec![],
                regime_state: regime_state(score).to_string(),
                computed_at: "x".to_string(),
            });
        }
        assert_eq!(consecutive_low_streak(&rows), 2);
    }

    #[test]
    fn consecutive_low_streak_zero_when_last_high() {
        let rows = vec![AlignmentScoreRow {
            date: "2026-05-28".to_string(),
            total_alignment_score: 85.0,
            components: vec![],
            divergent_assets: vec![],
            regime_state: "high-alignment".to_string(),
            computed_at: "x".to_string(),
        }];
        assert_eq!(consecutive_low_streak(&rows), 0);
    }

    #[test]
    fn parse_since_token_supports_days_weeks_months() {
        let today = Utc::now().date_naive();
        let d = parse_since_token("7d").unwrap();
        let expected = (today - Duration::days(7)).format("%Y-%m-%d").to_string();
        assert_eq!(d, expected);
        let w = parse_since_token("2w").unwrap();
        let expected = (today - Duration::weeks(2)).format("%Y-%m-%d").to_string();
        assert_eq!(w, expected);
        let m = parse_since_token("3m").unwrap();
        let expected = (today - Duration::days(90)).format("%Y-%m-%d").to_string();
        assert_eq!(m, expected);
        let abs = parse_since_token("2026-01-01").unwrap();
        assert_eq!(abs, "2026-01-01");
    }

    #[test]
    fn parse_since_token_rejects_garbage() {
        assert!(parse_since_token("forever").is_err());
    }

    #[test]
    fn upsert_and_read_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        let row = AlignmentScoreRow {
            date: "2026-06-01".to_string(),
            total_alignment_score: 72.5,
            components: vec![AlignmentComponent {
                symbol: "BTC".to_string(),
                allocation_weight: 25.0,
                operator_view: Some(OperatorView {
                    direction: "bull".to_string(),
                    conviction_magnitude: 3,
                    source: "journal".to_string(),
                    recorded_at: "2026-05-31T12:00:00Z".to_string(),
                }),
                analyst_summary: "convergent-bull".to_string(),
                analyst_avg_conviction: 2.0,
                alignment_class: "aligned".to_string(),
                class_score: 100.0,
            }],
            divergent_assets: vec![],
            regime_state: "mixed".to_string(),
            computed_at: "2026-06-01T00:00:00Z".to_string(),
        };
        upsert_row_sqlite(&conn, &row).unwrap();
        let got = row_from_db_sqlite(&conn, "2026-06-01").unwrap().unwrap();
        assert_eq!(got.total_alignment_score, 72.5);
        assert_eq!(got.components.len(), 1);
        assert_eq!(got.components[0].symbol, "BTC");
        assert_eq!(got.regime_state, "mixed");
    }

    #[test]
    fn upsert_replaces_same_date() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        let mut row = AlignmentScoreRow {
            date: "2026-06-01".to_string(),
            total_alignment_score: 60.0,
            components: vec![],
            divergent_assets: vec![],
            regime_state: "mixed".to_string(),
            computed_at: "2026-06-01T00:00:00Z".to_string(),
        };
        upsert_row_sqlite(&conn, &row).unwrap();
        row.total_alignment_score = 85.0;
        row.regime_state = "high-alignment".to_string();
        upsert_row_sqlite(&conn, &row).unwrap();
        let got = row_from_db_sqlite(&conn, "2026-06-01").unwrap().unwrap();
        assert_eq!(got.total_alignment_score, 85.0);
        assert_eq!(got.regime_state, "high-alignment");
    }

    #[test]
    fn history_returns_rows_in_ascending_date_order() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        for d in ["2026-06-03", "2026-06-01", "2026-06-02"] {
            let row = AlignmentScoreRow {
                date: d.to_string(),
                total_alignment_score: 70.0,
                components: vec![],
                divergent_assets: vec![],
                regime_state: "mixed".to_string(),
                computed_at: "x".to_string(),
            };
            upsert_row_sqlite(&conn, &row).unwrap();
        }
        let got = history_sqlite(&conn, None).unwrap();
        let dates: Vec<String> = got.iter().map(|r| r.date.clone()).collect();
        assert_eq!(dates, vec!["2026-06-01", "2026-06-02", "2026-06-03"]);
    }
}
