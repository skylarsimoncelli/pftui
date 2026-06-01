//! `clusters` — read-side helpers for the `cluster_key` taxonomy that
//! `prediction_lessons` and `user_predictions.lessons_applied` link into.
//! Also exposes a keyword-based claim classifier used by
//! `pftui analytics fragments --for-claim`.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterEntry {
    pub cluster_key: String,
    pub lesson_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClusterStats {
    pub cluster_key: String,
    pub lesson_count: i64,
    pub predictions_applying: i64,
}

/// List distinct cluster_keys present on `prediction_lessons`, with the
/// count of lessons mapped to each.
pub fn list_clusters(conn: &Connection) -> Result<Vec<ClusterEntry>> {
    // The cluster_key column is added by the schema migration in
    // `db::schema::run_migrations`; gracefully return [] if the table itself
    // is missing.
    let table_exists: i64 = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='prediction_lessons'")?
        .query_row([], |row| row.get(0))
        .unwrap_or(0);
    if table_exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT cluster_key, COUNT(*) AS n
         FROM prediction_lessons
         WHERE cluster_key IS NOT NULL AND cluster_key != ''
         GROUP BY cluster_key
         ORDER BY n DESC, cluster_key ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ClusterEntry {
                cluster_key: row.get(0)?,
                lesson_count: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Per-cluster lesson count + the number of `user_predictions` whose
/// `lessons_applied` JSON array references at least one lesson in that
/// cluster.
pub fn cluster_stats(conn: &Connection) -> Result<Vec<ClusterStats>> {
    let clusters = list_clusters(conn)?;
    let mut out = Vec::with_capacity(clusters.len());
    // user_predictions may not exist yet on fresh DBs.
    let predictions_exists: i64 = conn
        .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='user_predictions'")?
        .query_row([], |row| row.get(0))
        .unwrap_or(0);
    for c in clusters {
        let predictions_applying: i64 = if predictions_exists > 0 {
            // Count distinct user_predictions whose lessons_applied list
            // contains at least one lesson id belonging to this cluster.
            let mut stmt = conn.prepare(
                "SELECT COUNT(DISTINCT up.id)
                 FROM user_predictions up, json_each(up.lessons_applied) je
                 JOIN prediction_lessons pl
                   ON pl.id = je.value
                 WHERE pl.cluster_key = ?1",
            )?;
            stmt.query_row(rusqlite::params![&c.cluster_key], |row| row.get(0))
                .unwrap_or(0)
        } else {
            0
        };
        out.push(ClusterStats {
            cluster_key: c.cluster_key,
            lesson_count: c.lesson_count,
            predictions_applying,
        });
    }
    Ok(out)
}

/// Lightweight keyword-based cluster classifier. Maps a free-form claim to a
/// canonical cluster_key (matching those used in the live DB). Returns None
/// when no rule matches.
pub fn classify_claim(claim: &str) -> Option<&'static str> {
    let lower = claim.to_lowercase();
    // Rules are ordered by specificity. First match wins.
    const RULES: &[(&str, &[&str])] = &[
        (
            "iran_oil_managed_theater",
            &["iran", "oil", "strait", "hormuz", "tanker", "opec"],
        ),
        (
            "iran_gold_war_fatigue",
            &["iran", "gold", "war fatigue", "ceasefire"],
        ),
        (
            "tight_threshold_close_miss",
            &["close to", "just below", "just above", "barely", "round number", "threshold"],
        ),
        (
            "dxy_two_driver",
            &["dxy", "dollar", "rate differential", "safe haven"],
        ),
        (
            "options_gamma_pinning",
            &["gamma", "max pain", "pin", "expiry", "0dte"],
        ),
        (
            "realrates_dominates_gold",
            &["gold", "real yield", "tips", "breakeven"],
        ),
        (
            "fed_dot_repricing",
            &["fed", "fomc", "rate cut", "rate hike", "dot plot", "powell"],
        ),
        (
            "fourth_turning_crisis",
            &["fourth turning", "regime change", "crisis era"],
        ),
    ];
    for (cluster, keywords) in RULES {
        if keywords.iter().any(|kw| lower.contains(kw)) {
            return Some(*cluster);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognises_iran_oil_claim() {
        assert_eq!(
            classify_claim("Iran tensions in Strait of Hormuz keep oil bid"),
            Some("iran_oil_managed_theater")
        );
    }

    #[test]
    fn classify_recognises_gamma_claim() {
        assert_eq!(
            classify_claim("Gamma pin into the 0dte expiry"),
            Some("options_gamma_pinning")
        );
    }

    #[test]
    fn classify_recognises_dxy_claim() {
        assert_eq!(
            classify_claim("DXY break from rate differential"),
            Some("dxy_two_driver")
        );
    }

    #[test]
    fn classify_returns_none_for_unmatched_claim() {
        assert_eq!(classify_claim("XYZ random words zzz"), None);
    }

    #[test]
    fn list_clusters_returns_empty_when_no_data() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        let result = list_clusters(&conn).unwrap();
        assert!(result.is_empty());
    }
}
