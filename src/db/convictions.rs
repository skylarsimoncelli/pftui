use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionEntry {
    pub id: i64,
    pub symbol: String,
    pub score: i32,
    pub notes: Option<String>,
    pub recorded_at: String,
}

impl ConvictionEntry {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol: row.get(1)?,
            score: row.get(2)?,
            notes: row.get(3)?,
            recorded_at: row.get(4)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionChange {
    pub symbol: String,
    pub old_score: i32,
    pub new_score: i32,
    pub old_date: String,
    pub new_date: String,
    pub change_delta: i32,
}

pub fn set_conviction(
    conn: &Connection,
    symbol: &str,
    score: i32,
    notes: Option<&str>,
) -> Result<i64> {
    if !(-5..=5).contains(&score) {
        anyhow::bail!("Score must be between -5 and +5, got {}", score);
    }

    conn.execute(
        "INSERT INTO convictions (symbol, score, notes)
         VALUES (?, ?, ?)",
        params![symbol, score, notes],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_current(conn: &Connection) -> Result<Vec<ConvictionEntry>> {
    let mut stmt = conn.prepare(
        "WITH latest AS (
             SELECT symbol, MAX(id) as max_id
             FROM convictions
             GROUP BY symbol
         )
         SELECT c.id, c.symbol, c.score, c.notes, c.recorded_at
         FROM convictions c
         INNER JOIN latest l ON c.symbol = l.symbol AND c.id = l.max_id
         ORDER BY ABS(c.score) DESC, c.symbol ASC",
    )?;

    let rows = stmt.query_map([], ConvictionEntry::from_row)?;
    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn get_history(
    conn: &Connection,
    symbol: &str,
    limit: Option<usize>,
) -> Result<Vec<ConvictionEntry>> {
    let query = if let Some(lim) = limit {
        format!(
            "SELECT id, symbol, score, notes, recorded_at
             FROM convictions
             WHERE symbol = ?
             ORDER BY id DESC
             LIMIT {}",
            lim
        )
    } else {
        "SELECT id, symbol, score, notes, recorded_at
         FROM convictions
         WHERE symbol = ?
         ORDER BY id DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params![symbol], ConvictionEntry::from_row)?;
    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn get_changes(conn: &Connection, days: usize) -> Result<Vec<ConvictionChange>> {
    let query = format!(
        "WITH recent AS (
             SELECT id, symbol, score, recorded_at
             FROM convictions
             WHERE recorded_at >= datetime('now', '-{} days')
         ),
         latest_per_symbol AS (
             SELECT symbol, MAX(id) as max_id
             FROM recent
             GROUP BY symbol
         ),
         current_scores AS (
             SELECT r.symbol, r.score as new_score, r.recorded_at as new_date, r.id as current_id
             FROM recent r
             INNER JOIN latest_per_symbol l ON r.symbol = l.symbol AND r.id = l.max_id
         ),
         prior_scores AS (
             SELECT c.symbol, c.score as old_score, c.recorded_at as old_date
             FROM convictions c
             INNER JOIN current_scores cs ON c.symbol = cs.symbol
             WHERE c.id < cs.current_id
             AND c.id = (
                 SELECT MAX(id)
                 FROM convictions
                 WHERE symbol = c.symbol AND id < cs.current_id
             )
         )
         SELECT cs.symbol, COALESCE(ps.old_score, 0), cs.new_score, 
                COALESCE(ps.old_date, ''), cs.new_date,
                cs.new_score - COALESCE(ps.old_score, 0) as delta
         FROM current_scores cs
         LEFT JOIN prior_scores ps ON cs.symbol = ps.symbol
         WHERE cs.new_score != COALESCE(ps.old_score, 0)
         ORDER BY ABS(cs.new_score - COALESCE(ps.old_score, 0)) DESC",
        days
    );

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok(ConvictionChange {
            symbol: row.get(0)?,
            old_score: row.get(1)?,
            new_score: row.get(2)?,
            old_date: row.get(3)?,
            new_date: row.get(4)?,
            change_delta: row.get(5)?,
        })
    })?;

    let mut changes = Vec::new();
    for change in rows {
        changes.push(change?);
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_set_and_list_current() {
        let conn = setup_test_db();

        set_conviction(&conn, "BTC", 4, Some("Strong bullish thesis")).unwrap();
        set_conviction(&conn, "GC=F", -2, Some("Bearish short-term")).unwrap();
        set_conviction(&conn, "BTC", 5, Some("Updated to max conviction")).unwrap();

        let current = list_current(&conn).unwrap();
        assert_eq!(current.len(), 2);

        let btc = current.iter().find(|e| e.symbol == "BTC").unwrap();
        assert_eq!(btc.score, 5);
        assert_eq!(btc.notes.as_deref(), Some("Updated to max conviction"));

        let gold = current.iter().find(|e| e.symbol == "GC=F").unwrap();
        assert_eq!(gold.score, -2);
    }

    #[test]
    fn test_get_history() {
        let conn = setup_test_db();

        set_conviction(&conn, "SPY", 3, Some("Bull run")).unwrap();
        set_conviction(&conn, "SPY", -1, Some("Market turned")).unwrap();
        set_conviction(&conn, "SPY", 0, Some("Neutral now")).unwrap();

        let history = get_history(&conn, "SPY", None).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].score, 0); // Most recent first

        let limited = get_history(&conn, "SPY", Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_score_validation() {
        let conn = setup_test_db();

        let result = set_conviction(&conn, "TEST", 6, None);
        assert!(result.is_err());

        let result = set_conviction(&conn, "TEST", -6, None);
        assert!(result.is_err());

        let result = set_conviction(&conn, "TEST", 5, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_changes() {
        let conn = setup_test_db();

        // Simulate conviction changes
        set_conviction(&conn, "BTC", 2, Some("Initial")).unwrap();
        set_conviction(&conn, "BTC", 5, Some("Upgraded")).unwrap();
        set_conviction(&conn, "ETH", -1, Some("Bearish")).unwrap();

        let changes = get_changes(&conn, 7).unwrap();
        assert!(!changes.is_empty());

        let btc_change = changes.iter().find(|c| c.symbol == "BTC").unwrap();
        assert_eq!(btc_change.old_score, 2);
        assert_eq!(btc_change.new_score, 5);
        assert_eq!(btc_change.change_delta, 3);
    }
}
