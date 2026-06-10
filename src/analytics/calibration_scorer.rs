//! Rebuild `calibration_matrix` rows from scored `user_predictions`.
//!
//! The Self-Retrospective Calibration section of the report (and the
//! private_open_predictions_calibration loader inside `BuildContext::load`)
//! both read from `calibration_matrix`. Nothing in the rest of the
//! codebase populates it — without this scorer the table stays empty
//! and both surfaces render "No 90-day calibration rows are attached".
//!
//! Aggregation key: `(timeframe, topic, conviction_band)` where
//! `conviction_band` is one of `low` / `medium` / `high`. Bucketing rules
//! mirror the existing routine prompts: low ∈ {0.0..0.4}, medium ∈
//! {0.4..0.7}, high ∈ {0.7..1.0}; predictions without a confidence value
//! land in `medium` so the matrix has coverage even on legacy rows.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::db::backend::BackendConnection;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationMatrixRow {
    pub layer: String,
    pub topic: String,
    pub conviction_band: String,
    pub n: i64,
    pub hit_rate: f64,
    pub stated_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RebuildResult {
    pub rebuilt_at: String,
    pub rows_deleted: i64,
    pub rows_inserted: usize,
    pub rows: Vec<CalibrationMatrixRow>,
}

/// Rebuild the calibration_matrix table from user_predictions outcomes
/// over the trailing `since_days` window. Returns the new rows along
/// with delete / insert counts so the caller can log a one-line summary.
pub fn rebuild_calibration_matrix_backend(
    backend: &BackendConnection,
    since_days: i64,
) -> Result<RebuildResult> {
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => anyhow::bail!("calibration scorer requires the sqlite backend"),
    };

    crate::db::schema::run_migrations(conn)?;

    let rows = compute_rows(conn, since_days)?;

    let deleted: i64 = conn
        .execute("DELETE FROM calibration_matrix", [])
        .unwrap_or(0) as i64;

    let mut inserted = 0usize;
    for row in &rows {
        conn.execute(
            "INSERT INTO calibration_matrix
                (layer, topic, conviction_band, n, hit_rate, stated_confidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                row.layer,
                row.topic,
                row.conviction_band,
                row.n,
                row.hit_rate,
                row.stated_confidence,
            ],
        )?;
        inserted += 1;
    }

    Ok(RebuildResult {
        rebuilt_at: chrono::Utc::now().to_rfc3339(),
        rows_deleted: deleted,
        rows_inserted: inserted,
        rows,
    })
}

/// List the current calibration_matrix rows, optionally filtered to one
/// layer. Returns rows sorted by (layer, topic, conviction_band).
pub fn list_calibration_matrix_backend(
    backend: &BackendConnection,
    layer_filter: Option<&str>,
) -> Result<Vec<CalibrationMatrixRow>> {
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => anyhow::bail!("calibration list requires the sqlite backend"),
    };
    crate::db::schema::run_migrations(conn)?;
    let sql = match layer_filter {
        Some(_) => {
            "SELECT layer, topic, conviction_band, n, hit_rate, stated_confidence
             FROM calibration_matrix WHERE layer = ?
             ORDER BY layer, topic, conviction_band"
        }
        None => {
            "SELECT layer, topic, conviction_band, n, hit_rate, stated_confidence
             FROM calibration_matrix
             ORDER BY layer, topic, conviction_band"
        }
    };
    let mut stmt = conn.prepare(sql)?;
    let mapped = |row: &rusqlite::Row<'_>| {
        Ok(CalibrationMatrixRow {
            layer: row.get(0)?,
            topic: row.get(1)?,
            conviction_band: row.get(2)?,
            n: row.get(3)?,
            hit_rate: row.get(4)?,
            stated_confidence: row.get(5).ok(),
        })
    };
    let rows = match layer_filter {
        Some(l) => stmt
            .query_map(rusqlite::params![l], mapped)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt
            .query_map([], mapped)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

fn compute_rows(conn: &rusqlite::Connection, since_days: i64) -> Result<Vec<CalibrationMatrixRow>> {
    let cutoff = format!("date('now','-{} days')", since_days);
    // Bucketed query: bucket conviction text into low/medium/high,
    // bucket numeric confidence onto the same banding so the join is
    // clean. Predictions without conviction land in `medium`.
    let sql = format!(
        "SELECT
           COALESCE(NULLIF(timeframe,''),'unspecified') AS layer,
           COALESCE(NULLIF(topic,''),'other') AS topic,
           CASE
             WHEN LOWER(COALESCE(conviction,'')) IN ('low','weak') THEN 'low'
             WHEN LOWER(COALESCE(conviction,'')) IN ('high','strong') THEN 'high'
             ELSE 'medium'
           END AS conviction_band,
           SUM(CASE WHEN outcome='correct' THEN 1 ELSE 0 END) AS correct_count,
           SUM(CASE WHEN outcome='partial' THEN 1 ELSE 0 END) AS partial_count,
           SUM(CASE WHEN outcome='wrong' THEN 1 ELSE 0 END) AS wrong_count,
           COUNT(*) AS total_count,
           AVG(confidence) AS avg_confidence
         FROM user_predictions
         WHERE outcome IN ('correct','partial','wrong')
           AND scored_at >= {cutoff}
         GROUP BY layer, topic, conviction_band
         ORDER BY layer, topic, conviction_band"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |row| {
            let layer: String = row.get(0)?;
            let topic: String = row.get(1)?;
            let band: String = row.get(2)?;
            let correct: i64 = row.get(3)?;
            let partial: i64 = row.get(4)?;
            let _wrong: i64 = row.get(5)?;
            let total: i64 = row.get(6)?;
            let avg_conf: Option<f64> = row.get(7).ok();
            // Partial credit: +0.5 each.
            let hit_rate = if total > 0 {
                (correct as f64 + 0.5 * partial as f64) / total as f64
            } else {
                0.0
            };
            Ok(CalibrationMatrixRow {
                layer,
                topic,
                conviction_band: band,
                n: total,
                hit_rate,
                stated_confidence: avg_conf,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use rusqlite::Connection;

    fn fresh_backend_with_predictions() -> BackendConnection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        // Seed 5 predictions: 3 correct, 1 wrong, 1 partial across two
        // (layer, topic, conviction_band) buckets.
        for (timeframe, topic, conviction, confidence, outcome) in [
            ("low", "crypto", "high", 0.8, "correct"),
            ("low", "crypto", "high", 0.7, "correct"),
            ("low", "crypto", "high", 0.6, "wrong"),
            ("medium", "fed", "medium", 0.5, "partial"),
            ("medium", "fed", "medium", 0.55, "correct"),
        ] {
            conn.execute(
                "INSERT INTO user_predictions
                    (claim, symbol, conviction, timeframe, topic, confidence,
                     outcome, scored_at)
                 VALUES ('synthetic', 'BTC', ?1, ?2, ?3, ?4, ?5, datetime('now'))",
                rusqlite::params![conviction, timeframe, topic, confidence, outcome],
            )
            .unwrap();
        }
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn rebuild_aggregates_outcomes_into_calibration_rows() {
        let backend = fresh_backend_with_predictions();
        let result = rebuild_calibration_matrix_backend(&backend, 365).unwrap();
        assert_eq!(result.rows_inserted, 2);
        let crypto_high = result
            .rows
            .iter()
            .find(|r| r.layer == "low" && r.topic == "crypto" && r.conviction_band == "high")
            .expect("crypto high band present");
        assert_eq!(crypto_high.n, 3);
        // 2 correct / 3 = 0.667 (partial credit doesn't apply here).
        assert!((crypto_high.hit_rate - (2.0 / 3.0)).abs() < 1e-9);

        let fed_medium = result
            .rows
            .iter()
            .find(|r| r.layer == "medium" && r.topic == "fed")
            .expect("fed medium band present");
        // 1 correct + 0.5 partial = 1.5 / 2 = 0.75.
        assert!((fed_medium.hit_rate - 0.75).abs() < 1e-9);
    }

    #[test]
    fn list_returns_rebuilt_rows() {
        let backend = fresh_backend_with_predictions();
        let _ = rebuild_calibration_matrix_backend(&backend, 365).unwrap();
        let rows = list_calibration_matrix_backend(&backend, None).unwrap();
        assert_eq!(rows.len(), 2);
        let only_low = list_calibration_matrix_backend(&backend, Some("low")).unwrap();
        assert_eq!(only_low.len(), 1);
        assert_eq!(only_low[0].layer, "low");
    }

    /// Regression: legacy DBs created before `conviction_band` (and the other
    /// analytic columns) were added to the `calibration_matrix` CREATE have the
    /// old shape on disk. `run_migrations` must self-heal the table so a rebuild
    /// INSERT no longer fails with
    /// "table calibration_matrix has no column named conviction_band".
    #[test]
    fn migration_adds_missing_columns_to_legacy_calibration_matrix() {
        let conn = Connection::open_in_memory().unwrap();
        // Simulate the legacy on-disk shape: table exists but only has the
        // earliest columns. CREATE TABLE IF NOT EXISTS in run_migrations would
        // otherwise leave this untouched.
        conn.execute_batch(
            "CREATE TABLE calibration_matrix (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                layer TEXT
            );",
        )
        .unwrap();

        crate::db::schema::run_migrations(&conn).unwrap();

        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('calibration_matrix')")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for expected in [
            "layer",
            "topic",
            "conviction_band",
            "n",
            "hit_rate",
            "stated_confidence",
            "recorded_at",
        ] {
            assert!(
                cols.iter().any(|c| c == expected),
                "expected column `{expected}` after migration, got {cols:?}"
            );
        }

        // A rebuild INSERT path must now succeed against the healed table.
        conn.execute(
            "INSERT INTO user_predictions
                (claim, symbol, conviction, timeframe, topic, confidence,
                 outcome, scored_at)
             VALUES ('synthetic', 'BTC', 'high', 'low', 'crypto', 0.8, 'correct', datetime('now'))",
            [],
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };
        let result = rebuild_calibration_matrix_backend(&backend, 365).unwrap();
        assert_eq!(result.rows_inserted, 1);
    }

    /// Regression: a live DB carried BOTH the legacy scorer shape
    /// (`PRIMARY KEY(layer, topic, conviction, window_days)` with
    /// `conviction TEXT NOT NULL`) AND the appended canonical columns.
    /// The rebuild INSERT populates only the canonical columns, so it died
    /// with "NOT NULL constraint failed: calibration_matrix.conviction".
    /// `run_migrations` must rebuild the table to the canonical shape
    /// (preserving rows) so the rebuild succeeds.
    #[test]
    fn migration_rebuilds_drifted_hybrid_calibration_matrix() {
        let conn = Connection::open_in_memory().unwrap();
        // Exact drifted shape observed on the live DB (synthetic data only).
        conn.execute_batch(
            "CREATE TABLE calibration_matrix (
                layer TEXT NOT NULL, topic TEXT NOT NULL, conviction TEXT NOT NULL,
                n_scored INTEGER NOT NULL DEFAULT 0, n_correct INTEGER NOT NULL DEFAULT 0,
                n_partial INTEGER NOT NULL DEFAULT 0, n_wrong INTEGER NOT NULL DEFAULT 0,
                strict_hit_rate REAL NOT NULL DEFAULT 0.0, partial_credit_rate REAL NOT NULL DEFAULT 0.0,
                avg_confidence REAL, overconfidence_pp REAL, sample_se REAL,
                computed_at TEXT NOT NULL DEFAULT (datetime('now')), window_days INTEGER NOT NULL DEFAULT 180,
                conviction_band TEXT, n INTEGER NOT NULL DEFAULT 0, hit_rate REAL NOT NULL DEFAULT 0.0,
                stated_confidence REAL, recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY(layer, topic, conviction, window_days)
            );
            INSERT INTO calibration_matrix
                (layer, topic, conviction, n_scored, strict_hit_rate, avg_confidence, computed_at)
            VALUES ('low', 'crypto', 'high', 4, 0.75, 0.8, '2026-05-01T00:00:00Z');",
        )
        .unwrap();

        crate::db::schema::run_migrations(&conn).unwrap();

        // The legacy column set must be gone; the canonical shape (with `id`)
        // must be present.
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('calibration_matrix')")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!cols.iter().any(|c| c == "conviction"), "legacy `conviction` column should be dropped, got {cols:?}");
        assert!(cols.iter().any(|c| c == "id"), "canonical `id` column missing, got {cols:?}");

        // The pre-existing row must be preserved with the legacy→canonical
        // mapping (conviction → conviction_band, n_scored → n, etc.).
        let (band, n, hit_rate, conf, recorded): (String, i64, f64, f64, String) = conn
            .query_row(
                "SELECT conviction_band, n, hit_rate, stated_confidence, recorded_at FROM calibration_matrix",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(band, "high");
        assert_eq!(n, 4);
        assert!((hit_rate - 0.75).abs() < 1e-9);
        assert!((conf - 0.8).abs() < 1e-9);
        // recorded_at existed (with a datetime('now') default) on the hybrid
        // shape, so it wins over the legacy computed_at; just require NOT NULL.
        assert!(!recorded.is_empty());

        // Seed a scored prediction for the end-to-end rebuild below.
        conn.execute(
            "INSERT INTO user_predictions
                (claim, symbol, conviction, timeframe, topic, confidence,
                 outcome, scored_at)
             VALUES ('synthetic', 'BTC', 'high', 'low', 'crypto', 0.8, 'correct', datetime('now'))",
            [],
        )
        .unwrap();
        // Idempotence: a second migration pass must not duplicate or drop rows.
        crate::db::schema::run_migrations(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM calibration_matrix", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);

        // And the rebuild command must now succeed end-to-end.
        let backend = BackendConnection::Sqlite { conn };
        let result = rebuild_calibration_matrix_backend(&backend, 365).unwrap();
        assert_eq!(result.rows_inserted, 1);
    }
}
