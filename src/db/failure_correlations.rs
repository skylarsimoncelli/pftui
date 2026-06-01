//! `failure_correlations` — pairwise co-failure rates between lesson clusters.
//! Mirrored from the live-DB enrichment session (June 1 2026).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureCorrelation {
    pub cluster_a: String,
    pub cluster_b: String,
    pub co_wrong_count: i64,
    pub a_total_wrong: i64,
    pub b_total_wrong: i64,
    pub co_wrong_share: f64,
    pub window_days: i64,
    pub computed_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS failure_correlations (
            cluster_a TEXT NOT NULL,
            cluster_b TEXT NOT NULL,
            co_wrong_count INTEGER NOT NULL,
            a_total_wrong INTEGER NOT NULL,
            b_total_wrong INTEGER NOT NULL,
            co_wrong_share REAL NOT NULL,
            window_days INTEGER NOT NULL DEFAULT 7,
            computed_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(cluster_a, cluster_b, window_days)
        );
        CREATE INDEX IF NOT EXISTS idx_failure_correlations_cluster_a
            ON failure_correlations(cluster_a);
        CREATE INDEX IF NOT EXISTS idx_failure_correlations_cluster_b
            ON failure_correlations(cluster_b);",
    )?;
    Ok(())
}

pub fn list(
    conn: &Connection,
    cluster: Option<&str>,
    min_share: Option<f64>,
) -> Result<Vec<FailureCorrelation>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT cluster_a, cluster_b, co_wrong_count, a_total_wrong, b_total_wrong,
                co_wrong_share, window_days, computed_at
         FROM failure_correlations WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(c) = cluster {
        sql.push_str(" AND (cluster_a = ? OR cluster_b = ?)");
        args.push(Box::new(c.to_string()));
        args.push(Box::new(c.to_string()));
    }
    if let Some(m) = min_share {
        sql.push_str(" AND co_wrong_share >= ?");
        args.push(Box::new(m));
    }
    sql.push_str(" ORDER BY co_wrong_share DESC, cluster_a ASC, cluster_b ASC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), |row| {
            Ok(FailureCorrelation {
                cluster_a: row.get(0)?,
                cluster_b: row.get(1)?,
                co_wrong_count: row.get(2)?,
                a_total_wrong: row.get(3)?,
                b_total_wrong: row.get(4)?,
                co_wrong_share: row.get(5)?,
                window_days: row.get(6)?,
                computed_at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn upsert(
    conn: &Connection,
    cluster_a: &str,
    cluster_b: &str,
    co_wrong_count: i64,
    a_total_wrong: i64,
    b_total_wrong: i64,
    co_wrong_share: f64,
    window_days: i64,
) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO failure_correlations
            (cluster_a, cluster_b, co_wrong_count, a_total_wrong, b_total_wrong,
             co_wrong_share, window_days)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(cluster_a, cluster_b, window_days) DO UPDATE SET
            co_wrong_count = excluded.co_wrong_count,
            a_total_wrong = excluded.a_total_wrong,
            b_total_wrong = excluded.b_total_wrong,
            co_wrong_share = excluded.co_wrong_share,
            computed_at = datetime('now')",
        params![
            cluster_a,
            cluster_b,
            co_wrong_count,
            a_total_wrong,
            b_total_wrong,
            co_wrong_share,
            window_days,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn list_filters_by_cluster_and_min_share() {
        let conn = fresh_conn();
        upsert(&conn, "gold-cluster", "btc-cluster", 4, 10, 12, 0.4, 7).unwrap();
        upsert(&conn, "gold-cluster", "spy-cluster", 6, 10, 8, 0.6, 7).unwrap();
        upsert(&conn, "oil-cluster", "btc-cluster", 2, 5, 12, 0.4, 7).unwrap();

        let gold = list(&conn, Some("gold-cluster"), None).unwrap();
        assert_eq!(gold.len(), 2);
        let high = list(&conn, None, Some(0.5)).unwrap();
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].cluster_b, "spy-cluster");
        // Ordering by share desc.
        let all = list(&conn, None, None).unwrap();
        assert!(all[0].co_wrong_share >= all[1].co_wrong_share);
    }
}
