//! forecast_misalignments — misalignment tripwires with mechanical teeth (R2).
//!
//! An L3 ledger over the scored forecast corpus (`forecast_scores`): when a
//! canonical layer's CURRENT consecutive wrong-sign streak on one asset
//! reaches `MISALIGNMENT_STREAK_THRESHOLD`, an ACTIVE misalignment row is
//! recorded for that (layer, asset). While active:
//!
//!   - the layer's views on that asset are EXCLUDED from convergence
//!     voting/averaging (probation — `src/db/analyst_views.rs`), rendered
//!     visibly, never hidden;
//!   - `journal prediction add` caps that layer's stated confidence on the
//!     symbol at `MISALIGNMENT_CONFIDENCE_CAP` (`src/commands/predict.rs`);
//!   - `analytics epistemics record` counts it into
//!     `run_health.active_misalignments`.
//!
//! Detection runs in the `data refresh` tail (after forecast retro-scoring)
//! and is idempotent. Status transitions:
//!
//!   active ──(a scored HIT lands after the streak span)──► recovered
//!   active ──(operator/agent acknowledgement — reserved, no writer yet)──► acknowledged
//!
//! Ledger contract: rows are never deleted. The only mutations are outcome
//! fills — streak growth on the open row (streak_len / span_end /
//! cum_realized_against_pct extend while the streak keeps missing) and the
//! recovery transition. A streak that re-forms after recovery creates a NEW
//! row, so the ledger preserves every distinct misalignment episode.
//!
//! Only the four canonical voting layers are tracked: measurement layers
//! (blind, antithesis) never vote, so probation has nothing to revoke, and
//! their multi-horizon scoring would multiply-count one judgment stream.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::db::analyst_views::is_canonical_analyst;
use crate::research::forecast_scoring::{self, tail_streak, ScoredRow};

/// Current wrong-sign streak length at which a misalignment trips.
pub const MISALIGNMENT_STREAK_THRESHOLD: usize = 5;

/// One misalignment episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisalignmentRow {
    pub id: i64,
    pub layer: String,
    pub asset: String,
    pub detected_at: String,
    pub streak_len: i64,
    /// `bull` or `bear` — the sign the layer kept calling wrong.
    pub call: String,
    pub span_start: String,
    pub span_end: String,
    /// Sum of the streak rows' realized horizon returns — the cumulative
    /// move against the calls (overlapping windows; indicative).
    pub cum_realized_against_pct: f64,
    /// `active` | `acknowledged` | `recovered`.
    pub status: String,
    pub recovered_at: Option<String>,
}

/// Summary of one detection pass (refresh-tail output).
#[derive(Debug, Clone, Default, Serialize)]
pub struct DetectionSummary {
    /// Newly tripped this pass: "layer/ASSET (len)".
    pub newly_detected: Vec<String>,
    /// Recovered this pass: "layer/ASSET".
    pub newly_recovered: Vec<String>,
    /// Every misalignment active after the pass.
    pub active: Vec<MisalignmentRow>,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS forecast_misalignments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            layer TEXT NOT NULL,
            asset TEXT NOT NULL,
            detected_at TEXT NOT NULL,
            streak_len INTEGER NOT NULL,
            call TEXT NOT NULL,
            span_start TEXT NOT NULL,
            span_end TEXT NOT NULL,
            cum_realized_against_pct REAL NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            recovered_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_forecast_misalignments_status
            ON forecast_misalignments(status);
        CREATE INDEX IF NOT EXISTS idx_forecast_misalignments_layer_asset
            ON forecast_misalignments(layer, asset);",
    )?;
    Ok(())
}

fn row_to_misalignment(row: &rusqlite::Row) -> Result<MisalignmentRow, rusqlite::Error> {
    Ok(MisalignmentRow {
        id: row.get(0)?,
        layer: row.get(1)?,
        asset: row.get(2)?,
        detected_at: row.get(3)?,
        streak_len: row.get(4)?,
        call: row.get(5)?,
        span_start: row.get(6)?,
        span_end: row.get(7)?,
        cum_realized_against_pct: row.get(8)?,
        status: row.get(9)?,
        recovered_at: row.get(10)?,
    })
}

const COLUMNS: &str = "id, layer, asset, detected_at, streak_len, call, span_start, span_end, \
     cum_realized_against_pct, status, recovered_at";

/// The open (non-recovered) misalignment for one (layer, asset), if any.
/// `acknowledged` rows count as open for detection purposes (no duplicate
/// episode is created, recovery still applies) but NOT for probation.
fn get_open(conn: &Connection, layer: &str, asset: &str) -> Result<Option<MisalignmentRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM forecast_misalignments
         WHERE layer = ?1 AND UPPER(asset) = UPPER(?2) AND status IN ('active','acknowledged')
         ORDER BY id DESC LIMIT 1"
    ))?;
    let mut rows = stmt.query_map(params![layer, asset], row_to_misalignment)?;
    match rows.next() {
        Some(Ok(r)) => Ok(Some(r)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

/// Every ACTIVE misalignment, longest streak first.
pub fn active_misalignments(conn: &Connection) -> Result<Vec<MisalignmentRow>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM forecast_misalignments
         WHERE status = 'active'
         ORDER BY streak_len DESC, layer ASC, asset ASC"
    ))?;
    let rows = stmt.query_map([], row_to_misalignment)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Full ledger, newest first.
pub fn list_all(conn: &Connection) -> Result<Vec<MisalignmentRow>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM forecast_misalignments ORDER BY id DESC"
    ))?;
    let rows = stmt.query_map([], row_to_misalignment)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Count of ACTIVE misalignments (run_health derivation).
pub fn count_active(conn: &Connection) -> Result<i64> {
    ensure_table(conn)?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM forecast_misalignments WHERE status = 'active'",
        [],
        |row| row.get(0),
    )?;
    Ok(n)
}

/// Probation map for the convergence layer: (layer, ASSET-uppercase) →
/// streak_len for every ACTIVE misalignment.
pub fn active_probation_map(conn: &Connection) -> Result<HashMap<(String, String), i64>> {
    Ok(active_misalignments(conn)?
        .into_iter()
        .map(|m| ((m.layer, m.asset.to_uppercase()), m.streak_len))
        .collect())
}

/// Strip a `-USD` suffix for symbol comparison (views are written under held
/// aliases like `BTC`; predictions often use the deep series `BTC-USD`).
fn base_symbol(sym: &str) -> String {
    let upper = sym.to_uppercase();
    upper
        .strip_suffix("-USD")
        .map(str::to_string)
        .unwrap_or(upper)
}

/// The ACTIVE misalignment for (layer, symbol), tolerant of the `SYM` ↔
/// `SYM-USD` alias split. Used by the prediction confidence clamp.
pub fn active_for_symbol(
    conn: &Connection,
    layer: &str,
    symbol: &str,
) -> Result<Option<MisalignmentRow>> {
    let target = base_symbol(symbol);
    Ok(active_misalignments(conn)?
        .into_iter()
        .find(|m| m.layer == layer && base_symbol(&m.asset) == target))
}

/// One detection pass over the scored forecast corpus. Idempotent: with no
/// new scored rows, a re-run changes nothing.
///
///   1. RECOVERY — an open misalignment whose group has a scored HIT dated
///      on/after its span_end is marked `recovered` (the streak broke).
///   2. DETECTION — a (canonical layer, asset) with a current wrong-sign
///      streak ≥ threshold and no open misalignment gets a new ACTIVE row.
///   3. EXTENSION — an open row whose streak kept growing has its
///      streak_len / span_end / cum_realized_against_pct refreshed
///      (outcome fill; detected_at is preserved).
pub fn detect_and_update(conn: &Connection) -> Result<DetectionSummary> {
    detect_and_update_with_threshold(conn, MISALIGNMENT_STREAK_THRESHOLD)
}

/// Threshold-parameterized pass (tests exercise small synthetic corpora).
pub fn detect_and_update_with_threshold(
    conn: &Connection,
    threshold: usize,
) -> Result<DetectionSummary> {
    ensure_table(conn)?;
    let mut summary = DetectionSummary::default();
    let rows = forecast_scoring::load_rows(conn, None, None, None)?;

    // Group canonical-layer rows by (layer, ASSET). Canonical layers score
    // at exactly one horizon, so this matches the streak feed's grouping.
    let mut groups: BTreeMap<(String, String), Vec<&ScoredRow>> = BTreeMap::new();
    for row in &rows {
        if !is_canonical_analyst(&row.analyst) {
            continue;
        }
        groups
            .entry((row.analyst.clone(), row.asset.to_uppercase()))
            .or_default()
            .push(row);
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    for ((layer, asset), group) in &groups {
        let mut open = get_open(conn, layer, asset)?;

        // 1. Recovery: a scored non-neutral HIT on/after the recorded span
        // end means the streak broke.
        if let Some(open_row) = &open {
            let hit_landed = group.iter().any(|r| {
                r.status == "scored"
                    && r.direction_hit == Some(true)
                    && r.view_date.as_str() >= open_row.span_end.as_str()
            });
            if hit_landed {
                conn.execute(
                    "UPDATE forecast_misalignments
                     SET status = 'recovered', recovered_at = ?1
                     WHERE id = ?2",
                    params![now, open_row.id],
                )?;
                summary
                    .newly_recovered
                    .push(format!("{}/{}", layer, asset));
                open = None;
            }
        }

        let streak = tail_streak(group);
        match (open, streak) {
            // 2. New episode.
            (None, Some(s)) if s.len >= threshold => {
                conn.execute(
                    "INSERT INTO forecast_misalignments
                        (layer, asset, detected_at, streak_len, call, span_start,
                         span_end, cum_realized_against_pct, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active')",
                    params![
                        layer,
                        asset,
                        now,
                        s.len as i64,
                        s.call,
                        s.first_view_date,
                        s.last_view_date,
                        s.cumulative_realized_pct,
                    ],
                )?;
                summary
                    .newly_detected
                    .push(format!("{}/{} ({})", layer, asset, s.len));
            }
            // 3. Extension of an open episode (outcome fill).
            (Some(open_row), Some(s)) if s.len as i64 != open_row.streak_len => {
                conn.execute(
                    "UPDATE forecast_misalignments
                     SET streak_len = ?1, span_end = ?2, cum_realized_against_pct = ?3
                     WHERE id = ?4",
                    params![
                        s.len as i64,
                        s.last_view_date,
                        s.cumulative_realized_pct,
                        open_row.id,
                    ],
                )?;
            }
            _ => {}
        }
    }

    summary.active = active_misalignments(conn)?;
    Ok(summary)
}

/// "layer/ASSET (len)" list for one-line summaries.
pub fn format_active_brief(active: &[MisalignmentRow]) -> String {
    active
        .iter()
        .map(|m| format!("{}/{} ({})", m.layer, m.asset, m.streak_len))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal schema: the scored corpus this module reads plus the price /
    /// view tables `forecast_scoring::score_all` needs.
    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'test',
                PRIMARY KEY (symbol, date)
            );
            CREATE TABLE analyst_view_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL DEFAULT '',
                recorded_at TEXT NOT NULL
            );",
        )
        .expect("schema");
        forecast_scoring::ensure_table(&conn).expect("forecast_scores table");
        ensure_table(&conn).expect("misalignment table");
        conn
    }

    /// Insert a pre-scored forecast row directly (the detection input).
    #[allow(clippy::too_many_arguments)]
    fn insert_scored(
        conn: &Connection,
        view_id: i64,
        layer: &str,
        asset: &str,
        conviction: i64,
        view_date: &str,
        hit: bool,
        realized: f64,
    ) {
        let direction = if conviction >= 0 { "bull" } else { "bear" };
        conn.execute(
            "INSERT INTO forecast_scores
                (view_history_id, analyst, asset, direction, conviction, horizon_days,
                 view_date, realized_pct, series_used, direction_hit, weighted_score,
                 status, scored_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 45, ?6, ?7, ?3, ?8, 0.5, 'scored', ?6)",
            params![view_id, layer, asset, direction, conviction, view_date, realized, hit as i64],
        )
        .expect("insert scored row");
    }

    fn seed_streak(conn: &Connection, layer: &str, asset: &str, misses: usize, start_id: i64) {
        for i in 0..misses {
            insert_scored(
                conn,
                start_id + i as i64,
                layer,
                asset,
                3,
                &format!("2026-04-{:02}", i + 1),
                false,
                -2.5,
            );
        }
    }

    #[test]
    fn streak_at_threshold_creates_active_misalignment() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "GC=F", 5, 1);
        let summary = detect_and_update(&conn).unwrap();
        assert_eq!(summary.newly_detected, vec!["medium/GC=F (5)"]);
        assert_eq!(summary.active.len(), 1);
        let m = &summary.active[0];
        assert_eq!(m.layer, "medium");
        assert_eq!(m.asset, "GC=F");
        assert_eq!(m.streak_len, 5);
        assert_eq!(m.call, "bull");
        assert_eq!(m.span_start, "2026-04-01");
        assert_eq!(m.span_end, "2026-04-05");
        assert!((m.cum_realized_against_pct + 12.5).abs() < 1e-9);
        assert_eq!(m.status, "active");
        assert!(m.recovered_at.is_none());
    }

    #[test]
    fn streak_below_threshold_does_not_trip() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "GC=F", 4, 1);
        let summary = detect_and_update(&conn).unwrap();
        assert!(summary.newly_detected.is_empty());
        assert!(summary.active.is_empty());
    }

    #[test]
    fn measurement_layers_never_trip() {
        let conn = test_conn();
        seed_streak(&conn, "blind", "GC=F", 8, 1);
        seed_streak(&conn, "antithesis", "GC=F", 8, 100);
        let summary = detect_and_update(&conn).unwrap();
        assert!(summary.active.is_empty());
    }

    #[test]
    fn rerun_is_idempotent_and_extension_fills_outcome() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "GC=F", 5, 1);
        detect_and_update(&conn).unwrap();
        let first = active_misalignments(&conn).unwrap();
        assert_eq!(first.len(), 1);
        let detected_at = first[0].detected_at.clone();

        // Re-run with no new data: nothing changes, no duplicate episode.
        let summary = detect_and_update(&conn).unwrap();
        assert!(summary.newly_detected.is_empty());
        assert!(summary.newly_recovered.is_empty());
        assert_eq!(summary.active.len(), 1);
        assert_eq!(list_all(&conn).unwrap().len(), 1);

        // Two more misses: the open episode extends in place.
        insert_scored(&conn, 6, "medium", "GC=F", 4, "2026-04-06", false, -3.0);
        insert_scored(&conn, 7, "medium", "GC=F", 4, "2026-04-07", false, -3.0);
        let summary = detect_and_update(&conn).unwrap();
        assert!(summary.newly_detected.is_empty(), "no new episode");
        assert_eq!(summary.active.len(), 1);
        let m = &summary.active[0];
        assert_eq!(m.streak_len, 7);
        assert_eq!(m.span_end, "2026-04-07");
        assert!((m.cum_realized_against_pct + 18.5).abs() < 1e-9);
        assert_eq!(m.detected_at, detected_at, "detected_at preserved");
        assert_eq!(list_all(&conn).unwrap().len(), 1);
    }

    #[test]
    fn hit_after_span_recovers_the_misalignment() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "GC=F", 6, 1);
        detect_and_update(&conn).unwrap();
        assert_eq!(count_active(&conn).unwrap(), 1);

        // A hit lands after the span.
        insert_scored(&conn, 7, "medium", "GC=F", 3, "2026-04-10", true, 2.0);
        let summary = detect_and_update(&conn).unwrap();
        assert_eq!(summary.newly_recovered, vec!["medium/GC=F"]);
        assert_eq!(count_active(&conn).unwrap(), 0);
        let all = list_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, "recovered");
        assert!(all[0].recovered_at.is_some());

        // Idempotent after recovery too.
        let summary = detect_and_update(&conn).unwrap();
        assert!(summary.newly_recovered.is_empty());
        assert!(summary.newly_detected.is_empty());
    }

    #[test]
    fn new_streak_after_recovery_creates_a_new_episode() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "GC=F", 5, 1);
        detect_and_update(&conn).unwrap();
        insert_scored(&conn, 6, "medium", "GC=F", 3, "2026-04-10", true, 2.0);
        detect_and_update(&conn).unwrap();

        // Five fresh misses after the recovery hit.
        for i in 0..5 {
            insert_scored(
                &conn,
                7 + i,
                "medium",
                "GC=F",
                2,
                &format!("2026-05-{:02}", i + 1),
                false,
                -1.0,
            );
        }
        let summary = detect_and_update(&conn).unwrap();
        assert_eq!(summary.newly_detected, vec!["medium/GC=F (5)"]);
        let all = list_all(&conn).unwrap();
        assert_eq!(all.len(), 2, "ledger preserves both episodes");
        assert_eq!(count_active(&conn).unwrap(), 1);
        let active = &active_misalignments(&conn).unwrap()[0];
        assert_eq!(active.span_start, "2026-05-01");
    }

    #[test]
    fn probation_map_and_symbol_lookup() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "BTC", 5, 1);
        detect_and_update(&conn).unwrap();
        let map = active_probation_map(&conn).unwrap();
        assert_eq!(
            map.get(&("medium".to_string(), "BTC".to_string())),
            Some(&5)
        );
        // SYM ↔ SYM-USD alias tolerance for the prediction clamp.
        assert!(active_for_symbol(&conn, "medium", "BTC-USD")
            .unwrap()
            .is_some());
        assert!(active_for_symbol(&conn, "medium", "btc").unwrap().is_some());
        assert!(active_for_symbol(&conn, "low", "BTC").unwrap().is_none());
        assert!(active_for_symbol(&conn, "medium", "GC=F")
            .unwrap()
            .is_none());
    }

    #[test]
    fn format_brief_matches_refresh_line_shape() {
        let conn = test_conn();
        seed_streak(&conn, "medium", "GC=F", 7, 1);
        seed_streak(&conn, "medium", "SPY", 7, 100);
        detect_and_update(&conn).unwrap();
        let active = active_misalignments(&conn).unwrap();
        assert_eq!(
            format_active_brief(&active),
            "medium/GC=F (7), medium/SPY (7)"
        );
    }
}
