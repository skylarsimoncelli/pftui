use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Record of a prediction market probability at a specific date.
#[derive(Debug, Clone)]
pub struct PredictionHistoryRecord {
    #[allow(dead_code)] // Used in Markets tab sparkline rendering
    pub id: String,
    #[allow(dead_code)] // Used in Markets tab sparkline rendering
    pub date: String,         // YYYY-MM-DD
    pub probability: f64,
}

/// Insert a daily probability snapshot for a prediction market.
/// Uses INSERT OR REPLACE to handle duplicate date snapshots.
#[allow(dead_code)] // Used by refresh integration (F17.3+) and batch_insert_history
pub fn insert_history(
    conn: &Connection,
    id: &str,
    date: &str,
    probability: f64,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO predictions_history (id, date, probability)
         VALUES (?, ?, ?)",
        params![id, date, probability],
    )?;
    Ok(())
}

/// Get historical probability records for a prediction market, ordered by date ascending.
pub fn get_history(
    conn: &Connection,
    id: &str,
    days: usize,
) -> Result<Vec<PredictionHistoryRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, probability
         FROM predictions_history
         WHERE id = ?
         ORDER BY date DESC
         LIMIT ?",
    )?;

    let records = stmt
        .query_map(params![id, days], |row| {
            Ok(PredictionHistoryRecord {
                id: row.get(0)?,
                date: row.get(1)?,
                probability: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(records)
}

/// Batch insert daily snapshots for multiple prediction markets.
pub fn batch_insert_history(
    conn: &Connection,
    records: &[(String, String, f64)], // (id, date, probability)
) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO predictions_history (id, date, probability)
         VALUES (?, ?, ?)",
    )?;

    for (id, date, probability) in records {
        stmt.execute(params![id, date, probability])?;
    }

    Ok(())
}

#[allow(dead_code)]
pub fn insert_history_backend(
    backend: &BackendConnection,
    id: &str,
    date: &str,
    probability: f64,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| insert_history(conn, id, date, probability),
        |pool| insert_history_postgres(pool, id, date, probability),
    )
}

pub fn get_history_backend(
    backend: &BackendConnection,
    id: &str,
    days: usize,
) -> Result<Vec<PredictionHistoryRecord>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, id, days),
        |pool| get_history_postgres(pool, id, days),
    )
}

#[allow(dead_code)]
pub fn batch_insert_history_backend(
    backend: &BackendConnection,
    records: &[(String, String, f64)],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| batch_insert_history(conn, records),
        |pool| batch_insert_history_postgres(pool, records),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS predictions_history (
                id TEXT NOT NULL,
                date TEXT NOT NULL,
                probability DOUBLE PRECISION NOT NULL,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (id, date)
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_history_postgres(pool: &PgPool, id: &str, date: &str, probability: f64) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO predictions_history (id, date, probability)
             VALUES ($1, $2, $3)
             ON CONFLICT (id, date) DO UPDATE SET probability = EXCLUDED.probability",
        )
        .bind(id)
        .bind(date)
        .bind(probability)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_history_postgres(pool: &PgPool, id: &str, days: usize) -> Result<Vec<PredictionHistoryRecord>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<(String, String, f64)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, date, probability
             FROM predictions_history
             WHERE id = $1
             ORDER BY date DESC
             LIMIT $2",
        )
        .bind(id)
        .bind(days as i64)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|(id, date, probability)| PredictionHistoryRecord {
            id,
            date,
            probability,
        })
        .collect())
}

fn batch_insert_history_postgres(pool: &PgPool, records: &[(String, String, f64)]) -> Result<()> {
    for (id, date, probability) in records {
        insert_history_postgres(pool, id, date, *probability)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predictions_history_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE predictions_history (
                id TEXT NOT NULL,
                date TEXT NOT NULL,
                probability REAL NOT NULL,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (id, date)
            )",
        )
        .unwrap();

        insert_history(&conn, "market1", "2026-03-01", 0.45).unwrap();
        insert_history(&conn, "market1", "2026-03-02", 0.48).unwrap();
        insert_history(&conn, "market1", "2026-03-03", 0.52).unwrap();

        let history = get_history(&conn, "market1", 10).unwrap();
        assert_eq!(history.len(), 3);
        // Should be ordered DESC by date
        assert_eq!(history[0].date, "2026-03-03");
        assert_eq!(history[0].probability, 0.52);
        assert_eq!(history[2].date, "2026-03-01");
    }

    #[test]
    fn test_batch_insert() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE predictions_history (
                id TEXT NOT NULL,
                date TEXT NOT NULL,
                probability REAL NOT NULL,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (id, date)
            )",
        )
        .unwrap();

        let records = vec![
            ("market1".to_string(), "2026-03-01".to_string(), 0.30),
            ("market1".to_string(), "2026-03-02".to_string(), 0.35),
            ("market2".to_string(), "2026-03-01".to_string(), 0.60),
        ];

        batch_insert_history(&conn, &records).unwrap();

        let history1 = get_history(&conn, "market1", 10).unwrap();
        assert_eq!(history1.len(), 2);

        let history2 = get_history(&conn, "market2", 10).unwrap();
        assert_eq!(history2.len(), 1);
        assert_eq!(history2[0].probability, 0.60);
    }

    #[test]
    fn test_replace_on_duplicate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE predictions_history (
                id TEXT NOT NULL,
                date TEXT NOT NULL,
                probability REAL NOT NULL,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (id, date)
            )",
        )
        .unwrap();

        insert_history(&conn, "market1", "2026-03-01", 0.40).unwrap();
        insert_history(&conn, "market1", "2026-03-01", 0.50).unwrap(); // Update

        let history = get_history(&conn, "market1", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].probability, 0.50); // Should use the updated value
    }
}
