//! `adversary_views` — write-time adversary records for `user_predictions`.
//!
//! Each row captures the "case against" a single draft prediction as it was
//! computed at write time. Composed deterministically from existing substrate
//! (anti-pattern `reasoning_fragments`, top-3 lessons from the highest
//! co-failing cluster per `failure_correlations`, derived falsification
//! triggers). No LLM call is required to materialise these — the write-time
//! adversary is a structured "case against" assembled from data the substrate
//! already holds.
//!
//! Sister table to the synthesis-time `adversary_views` concept tracked under
//! the separate P3 "Adversary pseudo-analyst layer" TODO. The two will be
//! reconciled when that item lands; for now we own the table outright so the
//! write-time adversary has somewhere to persist.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Persisted write-time adversary view for a single prediction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdversaryView {
    pub id: i64,
    /// FK into `user_predictions.id`. Optional so an adversary can be
    /// computed without persisting against a saved prediction (rare —
    /// almost every caller will have a prediction_id).
    pub prediction_id: Option<i64>,
    pub cluster_key: String,
    /// JSON-encoded `Vec<AdversaryArgument>`.
    pub anti_pattern_arguments: String,
    /// JSON-encoded `Vec<CofailureWarning>`.
    pub cofailure_warnings: String,
    /// JSON-encoded `Vec<String>`.
    pub falsification_triggers: String,
    pub generated_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS adversary_views (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER,
            cluster_key TEXT NOT NULL,
            anti_pattern_arguments TEXT NOT NULL,
            cofailure_warnings TEXT NOT NULL,
            falsification_triggers TEXT NOT NULL,
            generated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY(prediction_id) REFERENCES user_predictions(id)
        );
        CREATE INDEX IF NOT EXISTS idx_adversary_views_prediction_id
            ON adversary_views(prediction_id);
        CREATE INDEX IF NOT EXISTS idx_adversary_views_cluster_key
            ON adversary_views(cluster_key);",
    )?;
    Ok(())
}

/// Insert a new adversary view row.
///
/// `anti_pattern_arguments`, `cofailure_warnings`, and `falsification_triggers`
/// are caller-encoded JSON arrays so the storage layer stays agnostic of the
/// exact shape the composition layer chose.
pub fn insert(
    conn: &Connection,
    prediction_id: Option<i64>,
    cluster_key: &str,
    anti_pattern_arguments_json: &str,
    cofailure_warnings_json: &str,
    falsification_triggers_json: &str,
) -> Result<i64> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO adversary_views
            (prediction_id, cluster_key, anti_pattern_arguments,
             cofailure_warnings, falsification_triggers)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            prediction_id,
            cluster_key,
            anti_pattern_arguments_json,
            cofailure_warnings_json,
            falsification_triggers_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(dead_code)]
pub fn get(conn: &Connection, id: i64) -> Result<Option<AdversaryView>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, prediction_id, cluster_key, anti_pattern_arguments,
                cofailure_warnings, falsification_triggers, generated_at
         FROM adversary_views WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], |r| {
            Ok(AdversaryView {
                id: r.get(0)?,
                prediction_id: r.get(1)?,
                cluster_key: r.get(2)?,
                anti_pattern_arguments: r.get(3)?,
                cofailure_warnings: r.get(4)?,
                falsification_triggers: r.get(5)?,
                generated_at: r.get(6)?,
            })
        })
        .optional()?;
    Ok(row)
}

#[allow(dead_code)]
pub fn list_for_prediction(conn: &Connection, prediction_id: i64) -> Result<Vec<AdversaryView>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, prediction_id, cluster_key, anti_pattern_arguments,
                cofailure_warnings, falsification_triggers, generated_at
         FROM adversary_views
         WHERE prediction_id = ?1
         ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![prediction_id], |r| {
            Ok(AdversaryView {
                id: r.get(0)?,
                prediction_id: r.get(1)?,
                cluster_key: r.get(2)?,
                anti_pattern_arguments: r.get(3)?,
                cofailure_warnings: r.get(4)?,
                falsification_triggers: r.get(5)?,
                generated_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // The FK on `adversary_views.prediction_id` references
        // `user_predictions(id)`. When SQLite was compiled with
        // `SQLITE_DEFAULT_FOREIGN_KEYS=1` the in-memory connection enforces
        // FKs at INSERT time, so we materialise a minimal stub of the
        // parent table to exercise the storage layer in isolation.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_predictions (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );",
        )
        .unwrap();
        for stub_id in 1..=42 {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO user_predictions (id) VALUES (?1)",
                params![stub_id],
            );
        }
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_then_get_roundtrips_arrays() {
        let conn = fresh_conn();
        let id = insert(
            &conn,
            Some(42),
            "realrates_dominates_gold",
            "[{\"fragment_id\":\"realrates-dominates-gold\",\"summary\":\"x\"}]",
            "[{\"cluster_key\":\"btc_correlation_regime\",\"lesson_id\":7}]",
            "[\"real yields > 2.5% breaks the call\"]",
        )
        .unwrap();
        let got = get(&conn, id).unwrap().unwrap();
        assert_eq!(got.prediction_id, Some(42));
        assert_eq!(got.cluster_key, "realrates_dominates_gold");
        assert!(got.anti_pattern_arguments.contains("realrates-dominates-gold"));
        assert!(got.cofailure_warnings.contains("btc_correlation_regime"));
        assert!(got.falsification_triggers.contains("real yields"));
    }

    #[test]
    fn list_for_prediction_orders_by_id() {
        let conn = fresh_conn();
        let a = insert(&conn, Some(1), "c", "[]", "[]", "[]").unwrap();
        let b = insert(&conn, Some(1), "c", "[]", "[]", "[]").unwrap();
        let c = insert(&conn, Some(2), "c", "[]", "[]", "[]").unwrap();
        let rows = list_for_prediction(&conn, 1).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, a);
        assert_eq!(rows[1].id, b);
        let only_two = list_for_prediction(&conn, 2).unwrap();
        assert_eq!(only_two.len(), 1);
        assert_eq!(only_two[0].id, c);
    }

    #[test]
    fn null_prediction_id_is_supported() {
        let conn = fresh_conn();
        let id = insert(&conn, None, "k", "[]", "[]", "[]").unwrap();
        let got = get(&conn, id).unwrap().unwrap();
        assert!(got.prediction_id.is_none());
    }
}
