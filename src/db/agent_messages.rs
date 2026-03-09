use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: i64,
    pub from_agent: String,
    pub to_agent: Option<String>,
    pub priority: String,
    pub content: String,
    pub category: Option<String>,
    pub layer: Option<String>,
    pub acknowledged: i64,
    pub created_at: String,
    pub acknowledged_at: Option<String>,
}

impl AgentMessage {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            from_agent: row.get(1)?,
            to_agent: row.get(2)?,
            priority: row.get(3)?,
            content: row.get(4)?,
            category: row.get(5)?,
            layer: row.get(6)?,
            acknowledged: row.get(7)?,
            created_at: row.get(8)?,
            acknowledged_at: row.get(9)?,
        })
    }
}

pub fn send_message(
    conn: &Connection,
    from: &str,
    to: Option<&str>,
    priority: Option<&str>,
    content: &str,
    category: Option<&str>,
    layer: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO agent_messages (from_agent, to_agent, priority, content, category, layer)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![from, to, priority.unwrap_or("normal"), content, category, layer],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_messages(
    conn: &Connection,
    to: Option<&str>,
    layer: Option<&str>,
    unacked_only: bool,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AgentMessage>> {
    let mut query = String::from(
        "SELECT id, from_agent, to_agent, priority, content, category, layer, acknowledged, created_at, acknowledged_at
         FROM agent_messages",
    );

    let mut where_parts = Vec::new();
    if let Some(t) = to {
        where_parts.push(format!(
            "(to_agent IS NULL OR to_agent = '{}')",
            t.replace('"', "''")
        ));
    }
    if let Some(l) = layer {
        where_parts.push(format!("layer = '{}'", l.replace('"', "''")));
    }
    if unacked_only {
        where_parts.push("acknowledged = 0".to_string());
    }
    if let Some(s) = since {
        where_parts.push(format!("created_at >= '{}'", s.replace('"', "''")));
    }

    if !where_parts.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&where_parts.join(" AND "));
    }

    query.push_str(" ORDER BY created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], AgentMessage::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn acknowledge(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE agent_messages
         SET acknowledged = 1, acknowledged_at = datetime('now')
         WHERE id = ?",
        [id],
    )?;
    Ok(())
}

pub fn acknowledge_all(conn: &Connection, to: &str) -> Result<usize> {
    let n = conn.execute(
        "UPDATE agent_messages
         SET acknowledged = 1, acknowledged_at = datetime('now')
         WHERE acknowledged = 0 AND (to_agent = ? OR to_agent IS NULL)",
        [to],
    )?;
    Ok(n)
}

pub fn purge_old(conn: &Connection, days: usize) -> Result<usize> {
    let n = conn.execute(
        "DELETE FROM agent_messages
         WHERE acknowledged = 1
           AND created_at < datetime('now', ?)",
        [format!("-{} days", days)],
    )?;
    Ok(n)
}
