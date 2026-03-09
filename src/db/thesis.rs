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
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
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
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
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
    conviction: Option<&str>,
) -> Result<()> {
    // First, snapshot the existing entry to history (if it exists)
    if let Some(existing) = get_thesis_section(conn, section)? {
        conn.execute(
            "INSERT INTO thesis_history (section, content, conviction)
             VALUES (?, ?, ?)",
            params![existing.section, existing.content, existing.conviction],
        )?;
    }

    // Determine conviction: use provided, or inherit from existing, or default to "medium"
    let final_conviction = if let Some(conv) = conviction {
        conv.to_string()
    } else if let Some(existing) = get_thesis_section(conn, section)? {
        existing.conviction
    } else {
        "medium".to_string()
    };

    // Upsert using INSERT OR REPLACE
    conn.execute(
        "INSERT OR REPLACE INTO thesis (section, content, conviction, updated_at)
         VALUES (?, ?, ?, datetime('now'))",
        params![section, content, final_conviction],
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
         WHERE section = ?",
    )?;

    let mut rows = stmt.query(params![section])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ThesisEntry::from_row(row)?))
    } else {
        Ok(None)
    }
}

pub fn get_thesis_history(
    conn: &Connection,
    section: &str,
    limit: Option<usize>,
) -> Result<Vec<ThesisHistoryEntry>> {
    let query = if let Some(lim) = limit {
        format!(
            "SELECT id, section, content, conviction, recorded_at
             FROM thesis_history
             WHERE section = ?
             ORDER BY recorded_at DESC
             LIMIT {}",
            lim
        )
    } else {
        "SELECT id, section, content, conviction, recorded_at
         FROM thesis_history
         WHERE section = ?
         ORDER BY recorded_at DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params![section], ThesisHistoryEntry::from_row)?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

pub fn remove_thesis(conn: &Connection, section: &str) -> Result<()> {
    conn.execute("DELETE FROM thesis WHERE section = ?", params![section])?;
    Ok(())
}
