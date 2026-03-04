use anyhow::Result;
use rusqlite::{params, Connection};

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
