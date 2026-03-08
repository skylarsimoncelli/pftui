use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThesisEntry {
    pub id: i64,
    pub section: String,
    pub content: String,
    pub conviction: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThesisHistoryEntry {
    pub id: i64,
    pub section: String,
    pub content: String,
    pub conviction: String,
    pub recorded_at: String,
}

impl ThesisEntry {
    fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            section: row.get(1)?,
            content: row.get(2)?,
            conviction: row.get(3)?,
            updated_at: row.get(4)?,
        })
    }
}

impl ThesisHistoryEntry {
    fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            section: row.get(1)?,
            content: row.get(2)?,
            conviction: row.get(3)?,
            recorded_at: row.get(4)?,
        })
    }
}

pub fn upsert_thesis(
    conn: &Connection,
    section: &str,
    content: &str,
    conviction: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO thesis_history (section, content, conviction)
         SELECT section, content, conviction FROM thesis WHERE section = ?1",
        params![section],
    )?;

    conn.execute(
        "INSERT INTO thesis (section, content, conviction, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(section) DO UPDATE SET
             content = excluded.content,
             conviction = excluded.conviction,
             updated_at = datetime('now')",
        params![section, content, conviction],
    )?;
    Ok(())
}

pub fn list_thesis(conn: &Connection) -> Result<Vec<ThesisEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, section, content, conviction, updated_at
         FROM thesis
         ORDER BY section ASC",
    )?;
    let rows = stmt.query_map([], ThesisEntry::from_row)?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

pub fn get_thesis_section(conn: &Connection, section: &str) -> Result<Option<ThesisEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, section, content, conviction, updated_at
         FROM thesis
         WHERE section = ?1",
    )?;
    let mut rows = stmt.query_map(params![section], ThesisEntry::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn get_thesis_history(
    conn: &Connection,
    section: &str,
    limit: Option<usize>,
) -> Result<Vec<ThesisHistoryEntry>> {
    let mut history = Vec::new();
    if let Some(limit) = limit {
        let mut stmt = conn.prepare(
            "SELECT id, section, content, conviction, recorded_at
             FROM thesis_history
             WHERE section = ?1
             ORDER BY recorded_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![section, limit as i64], ThesisHistoryEntry::from_row)?;
        for row in rows {
            history.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, section, content, conviction, recorded_at
             FROM thesis_history
             WHERE section = ?1
             ORDER BY recorded_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![section], ThesisHistoryEntry::from_row)?;
        for row in rows {
            history.push(row?);
        }
    }
    Ok(history)
}

pub fn remove_thesis(conn: &Connection, section: &str) -> Result<()> {
    conn.execute("DELETE FROM thesis WHERE section = ?1", params![section])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE thesis (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                section TEXT NOT NULL UNIQUE,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL DEFAULT 'medium',
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE thesis_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                section TEXT NOT NULL,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX idx_thesis_history_section ON thesis_history(section);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn upsert_creates_then_updates_with_history() {
        let conn = setup();
        upsert_thesis(&conn, "regime", "risk off", "high").unwrap();
        let first = get_thesis_section(&conn, "regime").unwrap().unwrap();
        assert_eq!(first.content, "risk off");
        assert_eq!(first.conviction, "high");
        assert_eq!(get_thesis_history(&conn, "regime", None).unwrap().len(), 0);

        upsert_thesis(&conn, "regime", "risk on", "low").unwrap();
        let second = get_thesis_section(&conn, "regime").unwrap().unwrap();
        assert_eq!(second.content, "risk on");
        assert_eq!(second.conviction, "low");
        let history = get_thesis_history(&conn, "regime", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "risk off");
    }

    #[test]
    fn remove_deletes_live_entry_only() {
        let conn = setup();
        upsert_thesis(&conn, "btc", "bullish", "medium").unwrap();
        remove_thesis(&conn, "btc").unwrap();
        assert!(get_thesis_section(&conn, "btc").unwrap().is_none());
    }
}
