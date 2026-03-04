use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub timestamp: String,
    pub content: String,
    pub tag: Option<String>,
    pub symbol: Option<String>,
    pub conviction: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewJournalEntry {
    pub timestamp: String,
    pub content: String,
    pub tag: Option<String>,
    pub symbol: Option<String>,
    pub conviction: Option<String>,
    pub status: String,
}

impl JournalEntry {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            content: row.get(2)?,
            tag: row.get(3)?,
            symbol: row.get(4)?,
            conviction: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
        })
    }
}

pub fn add_entry(conn: &Connection, entry: &NewJournalEntry) -> Result<i64> {
    conn.execute(
        "INSERT INTO journal (timestamp, content, tag, symbol, conviction, status)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            &entry.timestamp,
            &entry.content,
            &entry.tag,
            &entry.symbol,
            &entry.conviction,
            &entry.status,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_entry(conn: &Connection, id: i64) -> Result<Option<JournalEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at
         FROM journal WHERE id = ?",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(JournalEntry::from_row(row)?))
    } else {
        Ok(None)
    }
}

pub fn list_entries(
    conn: &Connection,
    limit: Option<usize>,
    since: Option<&str>,
    tag: Option<&str>,
    symbol: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<JournalEntry>> {
    let mut query = String::from(
        "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at
         FROM journal WHERE 1=1",
    );

    if let Some(since_date) = since {
        query.push_str(&format!(" AND timestamp >= '{}'", since_date));
    }
    if let Some(tag_filter) = tag {
        let tags: Vec<&str> = tag_filter.split(',').collect();
        let placeholders = tags.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        query.push_str(&format!(" AND tag IN ({})", placeholders));
    }
    if let Some(sym) = symbol {
        query.push_str(&format!(" AND symbol = '{}'", sym));
    }
    if let Some(st) = status {
        query.push_str(&format!(" AND status = '{}'", st));
    }

    query.push_str(" ORDER BY timestamp DESC");

    if let Some(lim) = limit {
        query.push_str(&format!(" LIMIT {}", lim));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = if let Some(tag_filter) = tag {
        let tags: Vec<&str> = tag_filter.split(',').collect();
        let params: Vec<&dyn rusqlite::ToSql> =
            tags.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
        stmt.query_map(&params[..], JournalEntry::from_row)?
    } else {
        stmt.query_map([], JournalEntry::from_row)?
    };

    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn search_entries(
    conn: &Connection,
    query: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<JournalEntry>> {
    let mut sql = String::from(
        "SELECT id, timestamp, content, tag, symbol, conviction, status, created_at
         FROM journal WHERE content LIKE ?",
    );

    if let Some(since_date) = since {
        sql.push_str(&format!(" AND timestamp >= '{}'", since_date));
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }

    let search_pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![search_pattern], JournalEntry::from_row)?;

    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn update_entry(
    conn: &Connection,
    id: i64,
    content: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    if let Some(c) = content {
        conn.execute("UPDATE journal SET content = ? WHERE id = ?", params![c, id])?;
    }
    if let Some(s) = status {
        conn.execute("UPDATE journal SET status = ? WHERE id = ?", params![s, id])?;
    }
    Ok(())
}

pub fn remove_entry(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM journal WHERE id = ?", params![id])?;
    Ok(())
}

pub fn get_all_tags(conn: &Connection) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn.prepare("SELECT tag, COUNT(*) as count FROM journal WHERE tag IS NOT NULL GROUP BY tag ORDER BY count DESC")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

    let mut tags = Vec::new();
    for tag in rows {
        tags.push(tag?);
    }
    Ok(tags)
}

#[derive(Debug, Serialize)]
pub struct JournalStats {
    pub total_entries: usize,
    pub entries_by_tag: Vec<(String, usize)>,
    pub entries_by_month: Vec<(String, usize)>,
}

pub fn get_stats(conn: &Connection) -> Result<JournalStats> {
    let total: usize = conn.query_row("SELECT COUNT(*) FROM journal", [], |row| row.get(0))?;

    let tags = get_all_tags(conn)?;

    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m', timestamp) as month, COUNT(*) as count
         FROM journal GROUP BY month ORDER BY month DESC",
    )?;
    let months = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut entries_by_month = Vec::new();
    for month in months {
        entries_by_month.push(month?);
    }

    Ok(JournalStats {
        total_entries: total,
        entries_by_tag: tags,
        entries_by_month,
    })
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
    fn test_add_and_get_entry() {
        let conn = setup_test_db();
        let entry = NewJournalEntry {
            timestamp: "2026-03-04T20:00:00Z".to_string(),
            content: "Test entry".to_string(),
            tag: Some("test".to_string()),
            symbol: Some("GC=F".to_string()),
            conviction: Some("high".to_string()),
            status: "open".to_string(),
        };

        let id = add_entry(&conn, &entry).unwrap();
        let retrieved = get_entry(&conn, id).unwrap().unwrap();

        assert_eq!(retrieved.content, "Test entry");
        assert_eq!(retrieved.tag, Some("test".to_string()));
        assert_eq!(retrieved.symbol, Some("GC=F".to_string()));
        assert_eq!(retrieved.conviction, Some("high".to_string()));
        assert_eq!(retrieved.status, "open");
    }

    #[test]
    fn test_list_entries() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = list_entries(&conn, None, None, None, None, None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "Entry 1"); // Most recent first
    }

    #[test]
    fn test_list_entries_with_tag_filter() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Trade entry".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Thesis entry".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = list_entries(&conn, None, None, Some("trade"), None, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Trade entry");
    }

    #[test]
    fn test_search_entries() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Gold thesis confirmed".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Bitcoin pump".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let entries = search_entries(&conn, "gold", None, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Gold thesis confirmed");
    }

    #[test]
    fn test_update_entry() {
        let conn = setup_test_db();
        let id = add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Original".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        update_entry(&conn, id, Some("Updated"), Some("validated")).unwrap();
        let entry = get_entry(&conn, id).unwrap().unwrap();
        assert_eq!(entry.content, "Updated");
        assert_eq!(entry.status, "validated");
    }

    #[test]
    fn test_remove_entry() {
        let conn = setup_test_db();
        let id = add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "To be deleted".to_string(),
                tag: None,
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        remove_entry(&conn, id).unwrap();
        let entry = get_entry(&conn, id).unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn test_get_all_tags() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-03T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-02T20:00:00Z".to_string(),
                content: "Entry 3".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let tags = get_all_tags(&conn).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], ("trade".to_string(), 2));
        assert_eq!(tags[1], ("thesis".to_string(), 1));
    }

    #[test]
    fn test_get_stats() {
        let conn = setup_test_db();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-03-04T20:00:00Z".to_string(),
                content: "Entry 1".to_string(),
                tag: Some("trade".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();
        add_entry(
            &conn,
            &NewJournalEntry {
                timestamp: "2026-02-15T20:00:00Z".to_string(),
                content: "Entry 2".to_string(),
                tag: Some("thesis".to_string()),
                symbol: None,
                conviction: None,
                status: "open".to_string(),
            },
        )
        .unwrap();

        let stats = get_stats(&conn).unwrap();
        assert_eq!(stats.total_entries, 2);
        assert_eq!(stats.entries_by_tag.len(), 2);
        assert_eq!(stats.entries_by_month.len(), 2);
    }
}
