use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyNote {
    pub id: i64,
    pub date: String,
    pub section: String,
    pub content: String,
    pub created_at: String,
}

impl DailyNote {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            date: row.get(1)?,
            section: row.get(2)?,
            content: row.get(3)?,
            created_at: row.get(4)?,
        })
    }
}

pub fn add_note(conn: &Connection, date: &str, section: &str, content: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO daily_notes (date, section, content)
         VALUES (?, ?, ?)",
        params![date, section, content],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_notes(
    conn: &Connection,
    date: Option<&str>,
    section: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<DailyNote>> {
    let mut query = String::from(
        "SELECT id, date, section, content, created_at
         FROM daily_notes",
    );

    let mut where_parts = Vec::new();
    if let Some(d) = date {
        where_parts.push(format!("date = '{}'", d.replace('"', "''")));
    }
    if let Some(s) = section {
        where_parts.push(format!("section = '{}'", s.replace('"', "''")));
    }
    if !where_parts.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&where_parts.join(" AND "));
    }

    query.push_str(" ORDER BY date DESC, created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], DailyNote::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn search_notes(
    conn: &Connection,
    query: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<DailyNote>> {
    let mut sql = String::from(
        "SELECT id, date, section, content, created_at
         FROM daily_notes
         WHERE content LIKE ?",
    );

    if let Some(s) = since {
        sql.push_str(&format!(" AND date >= '{}'", s.replace('"', "''")));
    }

    sql.push_str(" ORDER BY date DESC, created_at DESC");
    if let Some(n) = limit {
        sql.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&sql)?;
    let pattern = format!("%{}%", query);
    let rows = stmt.query_map([pattern], DailyNote::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn remove_note(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM daily_notes WHERE id = ?", [id])?;
    Ok(())
}
