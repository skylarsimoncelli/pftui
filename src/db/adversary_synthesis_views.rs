//! `adversary_synthesis_views` — synthesis-time adversary records.
//!
//! Each row captures the per-asset, per-run "case against the convergence"
//! written by the `analyst-adversary` pseudo-layer AFTER the four
//! timeframe analysts have published their `analyst_views` for a run, but
//! BEFORE the synthesis (evening / morning) agent reads them. The intent
//! is structural counter-pressure: the four analysts share priors
//! (same bundles, same lesson book, same thesis context), so their
//! agreement may be confirmation of shared assumptions rather than
//! independent corroboration. The adversary uses ONLY the data those
//! analysts already saw, names the strongest opposing case, and
//! enumerates falsification triggers under which the dominant
//! convergence would be invalidated.
//!
//! Sister table to the write-time `adversary_views` (per-prediction case
//! against an individual claim composed deterministically from the
//! substrate at write time). The two tables are intentionally distinct:
//! the write-time row is keyed by `prediction_id`, this synthesis-time
//! row is keyed by `(asset, recorded_at)`.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Persisted synthesis-time adversary view for a single asset on a single run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdversarySynthesisView {
    pub id: i64,
    pub asset: String,
    pub current_convergence_summary: String,
    pub counter_case_summary: String,
    /// JSON-encoded `Vec<String>`.
    pub counter_case_evidence_points: String,
    /// JSON-encoded `Vec<String>`.
    pub falsification_triggers: String,
    /// 1..=5; >= 3 triggers the synthesis-gating contract documented in
    /// AGENTS.md and `agents/routines/adversary-analyst.md`.
    pub fragility_score: i64,
    pub recorded_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS adversary_synthesis_views (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            asset TEXT NOT NULL,
            current_convergence_summary TEXT NOT NULL,
            counter_case_summary TEXT NOT NULL,
            counter_case_evidence_points TEXT NOT NULL,
            falsification_triggers TEXT NOT NULL,
            fragility_score INTEGER NOT NULL CHECK(fragility_score BETWEEN 1 AND 5),
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_adversary_synthesis_views_asset
            ON adversary_synthesis_views(asset);
        CREATE INDEX IF NOT EXISTS idx_adversary_synthesis_views_recorded_at
            ON adversary_synthesis_views(recorded_at);",
    )?;
    Ok(())
}

/// Insert a new synthesis-time adversary view.
///
/// `counter_case_evidence_points` and `falsification_triggers` are
/// caller-encoded JSON arrays so the storage layer stays agnostic of the
/// composition layer's exact shape (typically `Vec<String>`).
#[allow(clippy::too_many_arguments)]
pub fn insert(
    conn: &Connection,
    asset: &str,
    current_convergence_summary: &str,
    counter_case_summary: &str,
    counter_case_evidence_points_json: &str,
    falsification_triggers_json: &str,
    fragility_score: i64,
    recorded_at: Option<&str>,
) -> Result<i64> {
    ensure_table(conn)?;
    if !(1..=5).contains(&fragility_score) {
        anyhow::bail!(
            "fragility_score must be in 1..=5, got {}",
            fragility_score
        );
    }
    match recorded_at {
        Some(ts) => {
            conn.execute(
                "INSERT INTO adversary_synthesis_views
                    (asset, current_convergence_summary, counter_case_summary,
                     counter_case_evidence_points, falsification_triggers,
                     fragility_score, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    asset,
                    current_convergence_summary,
                    counter_case_summary,
                    counter_case_evidence_points_json,
                    falsification_triggers_json,
                    fragility_score,
                    ts,
                ],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO adversary_synthesis_views
                    (asset, current_convergence_summary, counter_case_summary,
                     counter_case_evidence_points, falsification_triggers,
                     fragility_score)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    asset,
                    current_convergence_summary,
                    counter_case_summary,
                    counter_case_evidence_points_json,
                    falsification_triggers_json,
                    fragility_score,
                ],
            )?;
        }
    }
    Ok(conn.last_insert_rowid())
}

#[allow(dead_code)]
pub fn get(conn: &Connection, id: i64) -> Result<Option<AdversarySynthesisView>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, asset, current_convergence_summary, counter_case_summary,
                counter_case_evidence_points, falsification_triggers,
                fragility_score, recorded_at
         FROM adversary_synthesis_views WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            Ok(AdversarySynthesisView {
                id: r.get(0)?,
                asset: r.get(1)?,
                current_convergence_summary: r.get(2)?,
                counter_case_summary: r.get(3)?,
                counter_case_evidence_points: r.get(4)?,
                falsification_triggers: r.get(5)?,
                fragility_score: r.get(6)?,
                recorded_at: r.get(7)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// List rows, newest first. Filters are AND-combined. `since` is an
/// ISO-8601 timestamp string; rows with `recorded_at >= since` are kept.
pub fn list(
    conn: &Connection,
    asset: Option<&str>,
    since: Option<&str>,
) -> Result<Vec<AdversarySynthesisView>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT id, asset, current_convergence_summary, counter_case_summary,
                counter_case_evidence_points, falsification_triggers,
                fragility_score, recorded_at
         FROM adversary_synthesis_views WHERE 1=1",
    );
    let mut bound: Vec<String> = Vec::new();
    if let Some(a) = asset {
        sql.push_str(" AND asset = ?");
        bound.push(a.to_string());
    }
    if let Some(s) = since {
        sql.push_str(" AND recorded_at >= ?");
        bound.push(s.to_string());
    }
    sql.push_str(" ORDER BY recorded_at DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let params_dyn: Vec<&dyn rusqlite::ToSql> =
        bound.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params_dyn.as_slice(), |r| {
            Ok(AdversarySynthesisView {
                id: r.get(0)?,
                asset: r.get(1)?,
                current_convergence_summary: r.get(2)?,
                counter_case_summary: r.get(3)?,
                counter_case_evidence_points: r.get(4)?,
                falsification_triggers: r.get(5)?,
                fragility_score: r.get(6)?,
                recorded_at: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Latest row for a given asset, optionally constrained to `recorded_at >= since`.
#[allow(dead_code)]
pub fn latest_for_asset(
    conn: &Connection,
    asset: &str,
    since: Option<&str>,
) -> Result<Option<AdversarySynthesisView>> {
    let rows = list(conn, Some(asset), since)?;
    Ok(rows.into_iter().next())
}

/// Per-asset fragility ranking: for each asset whose latest row falls
/// within the optional `since` cutoff, return `(asset, max_fragility, latest_recorded_at)`.
/// Sorted by `max_fragility` DESC, then `asset` ASC for deterministic
/// output across runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FragilityRankRow {
    pub asset: String,
    pub max_fragility_score: i64,
    pub latest_recorded_at: String,
}

pub fn fragility_rank(
    conn: &Connection,
    since: Option<&str>,
) -> Result<Vec<FragilityRankRow>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT asset, MAX(fragility_score) AS max_score, MAX(recorded_at) AS latest_at
         FROM adversary_synthesis_views WHERE 1=1",
    );
    let mut bound: Vec<String> = Vec::new();
    if let Some(s) = since {
        sql.push_str(" AND recorded_at >= ?");
        bound.push(s.to_string());
    }
    sql.push_str(" GROUP BY asset ORDER BY max_score DESC, asset ASC");
    let mut stmt = conn.prepare(&sql)?;
    let params_dyn: Vec<&dyn rusqlite::ToSql> =
        bound.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(params_dyn.as_slice(), |r| {
            Ok(FragilityRankRow {
                asset: r.get(0)?,
                max_fragility_score: r.get(1)?,
                latest_recorded_at: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        ensure_table(&conn).expect("ensure_table");
        conn
    }

    #[test]
    fn insert_and_get_roundtrip() {
        let conn = fresh_conn();
        let id = insert(
            &conn,
            "BTC",
            "All four layers say BTC structural support firms above $75k.",
            "Cycle top is closer than convergence claims.",
            "[\"realized cap stalling\",\"ETF flow tail risk\"]",
            "[\"BTC closes < 65k for 5 sessions\",\"GLD/BTC ratio < 0.05\"]",
            4,
            Some("2026-06-02T18:00:00Z"),
        )
        .unwrap();
        let got = get(&conn, id).unwrap().unwrap();
        assert_eq!(got.asset, "BTC");
        assert_eq!(got.fragility_score, 4);
        assert!(got.counter_case_summary.contains("Cycle top"));
        assert!(got.counter_case_evidence_points.contains("realized cap"));
        assert!(got.falsification_triggers.contains("65k"));
        assert_eq!(got.recorded_at, "2026-06-02T18:00:00Z");
    }

    #[test]
    fn insert_rejects_out_of_range_score() {
        let conn = fresh_conn();
        let err = insert(
            &conn,
            "BTC",
            "x",
            "y",
            "[]",
            "[]",
            6,
            None,
        );
        assert!(err.is_err());
        let err2 = insert(&conn, "BTC", "x", "y", "[]", "[]", 0, None);
        assert!(err2.is_err());
    }

    #[test]
    fn list_filters_by_asset_and_since_and_orders_desc() {
        let conn = fresh_conn();
        let a = insert(
            &conn, "BTC", "c", "k", "[]", "[]", 3,
            Some("2026-06-01T00:00:00Z"),
        )
        .unwrap();
        let b = insert(
            &conn, "BTC", "c", "k", "[]", "[]", 4,
            Some("2026-06-02T00:00:00Z"),
        )
        .unwrap();
        let _gld = insert(
            &conn, "GLD", "c", "k", "[]", "[]", 5,
            Some("2026-06-02T01:00:00Z"),
        )
        .unwrap();

        let all_btc = list(&conn, Some("BTC"), None).unwrap();
        assert_eq!(all_btc.len(), 2);
        // newest first
        assert_eq!(all_btc[0].id, b);
        assert_eq!(all_btc[1].id, a);

        let recent_btc = list(&conn, Some("BTC"), Some("2026-06-02T00:00:00Z")).unwrap();
        assert_eq!(recent_btc.len(), 1);
        assert_eq!(recent_btc[0].id, b);
    }

    #[test]
    fn latest_for_asset_returns_top_of_list() {
        let conn = fresh_conn();
        insert(
            &conn, "BTC", "c", "k1", "[]", "[]", 2,
            Some("2026-06-01T00:00:00Z"),
        )
        .unwrap();
        let b = insert(
            &conn, "BTC", "c", "k2", "[]", "[]", 4,
            Some("2026-06-02T00:00:00Z"),
        )
        .unwrap();
        let got = latest_for_asset(&conn, "BTC", None).unwrap().unwrap();
        assert_eq!(got.id, b);
    }

    #[test]
    fn fragility_rank_orders_by_max_score_desc_then_asset_asc() {
        let conn = fresh_conn();
        // BTC: max 4
        insert(&conn, "BTC", "c", "k", "[]", "[]", 2, Some("2026-06-01T00:00:00Z")).unwrap();
        insert(&conn, "BTC", "c", "k", "[]", "[]", 4, Some("2026-06-02T00:00:00Z")).unwrap();
        // GLD: max 5
        insert(&conn, "GLD", "c", "k", "[]", "[]", 5, Some("2026-06-02T00:00:00Z")).unwrap();
        // AAA: max 4 (ties BTC)
        insert(&conn, "AAA", "c", "k", "[]", "[]", 4, Some("2026-06-02T00:00:00Z")).unwrap();

        let rank = fragility_rank(&conn, None).unwrap();
        assert_eq!(rank.len(), 3);
        assert_eq!(rank[0].asset, "GLD");
        assert_eq!(rank[0].max_fragility_score, 5);
        // AAA < BTC lexicographically; both at 4
        assert_eq!(rank[1].asset, "AAA");
        assert_eq!(rank[1].max_fragility_score, 4);
        assert_eq!(rank[2].asset, "BTC");
        assert_eq!(rank[2].max_fragility_score, 4);
    }

    #[test]
    fn fragility_rank_respects_since_filter() {
        let conn = fresh_conn();
        insert(&conn, "BTC", "c", "k", "[]", "[]", 5, Some("2026-05-01T00:00:00Z")).unwrap();
        insert(&conn, "GLD", "c", "k", "[]", "[]", 3, Some("2026-06-02T00:00:00Z")).unwrap();
        let rank = fragility_rank(&conn, Some("2026-06-01T00:00:00Z")).unwrap();
        assert_eq!(rank.len(), 1);
        assert_eq!(rank[0].asset, "GLD");
    }
}
