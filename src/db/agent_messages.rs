use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: i64,
    pub from_agent: String,
    pub to_agent: Option<String>,
    pub package_id: Option<String>,
    pub package_title: Option<String>,
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
            package_id: row.get(3)?,
            package_title: row.get(4)?,
            priority: row.get(5)?,
            content: row.get(6)?,
            category: row.get(7)?,
            layer: row.get(8)?,
            acknowledged: row.get(9)?,
            created_at: row.get(10)?,
            acknowledged_at: row.get(11)?,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn send_message(
    conn: &Connection,
    from: &str,
    to: Option<&str>,
    priority: Option<&str>,
    content: &str,
    category: Option<&str>,
    layer: Option<&str>,
    package_id: Option<&str>,
    package_title: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO agent_messages (from_agent, to_agent, package_id, package_title, priority, content, category, layer)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            from,
            to,
            package_id,
            package_title,
            priority.unwrap_or("normal"),
            content,
            category,
            layer
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
pub fn list_messages(
    conn: &Connection,
    from: Option<&str>,
    to: Option<&str>,
    layer: Option<&str>,
    unacked_only: bool,
    since: Option<&str>,
    package_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AgentMessage>> {
    let mut query = String::from(
        "SELECT id, from_agent, to_agent, package_id, package_title, priority, content, category, layer, acknowledged, created_at, acknowledged_at
         FROM agent_messages",
    );

    let mut where_parts = Vec::new();
    if let Some(f) = from {
        where_parts.push(format!("from_agent = '{}'", f.replace('"', "''")));
    }
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
    if let Some(pid) = package_id {
        where_parts.push(format!("package_id = '{}'", pid.replace('"', "''")));
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

pub fn get_message_by_id(conn: &Connection, id: i64) -> Result<Option<AgentMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, from_agent, to_agent, package_id, package_title, priority, content, category, layer, acknowledged, created_at, acknowledged_at
         FROM agent_messages
         WHERE id = ?",
    )?;
    let mut rows = stmt.query_map([id], AgentMessage::from_row)?;
    Ok(rows.next().transpose()?)
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

#[allow(clippy::too_many_arguments)]
pub fn send_message_backend(
    backend: &BackendConnection,
    from: &str,
    to: Option<&str>,
    priority: Option<&str>,
    content: &str,
    category: Option<&str>,
    layer: Option<&str>,
    package_id: Option<&str>,
    package_title: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            send_message(
                conn,
                from,
                to,
                priority,
                content,
                category,
                layer,
                package_id,
                package_title,
            )
        },
        |pool| {
            send_message_postgres(
                pool,
                from,
                to,
                priority,
                content,
                category,
                layer,
                package_id,
                package_title,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub fn list_messages_backend(
    backend: &BackendConnection,
    from: Option<&str>,
    to: Option<&str>,
    layer: Option<&str>,
    unacked_only: bool,
    since: Option<&str>,
    package_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AgentMessage>> {
    query::dispatch(
        backend,
        |conn| {
            list_messages(
                conn,
                from,
                to,
                layer,
                unacked_only,
                since,
                package_id,
                limit,
            )
        },
        |pool| {
            list_messages_postgres(
                pool,
                from,
                to,
                layer,
                unacked_only,
                since,
                package_id,
                limit,
            )
        },
    )
}

pub fn get_message_by_id_backend(
    backend: &BackendConnection,
    id: i64,
) -> Result<Option<AgentMessage>> {
    query::dispatch(
        backend,
        |conn| get_message_by_id(conn, id),
        |pool| get_message_by_id_postgres(pool, id),
    )
}

pub fn acknowledge_backend(backend: &BackendConnection, id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| acknowledge(conn, id),
        |pool| acknowledge_postgres(pool, id),
    )
}

pub fn acknowledge_all_backend(backend: &BackendConnection, to: &str) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| acknowledge_all(conn, to),
        |pool| acknowledge_all_postgres(pool, to),
    )
}

pub fn purge_old_backend(backend: &BackendConnection, days: usize) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| purge_old(conn, days),
        |pool| purge_old_postgres(pool, days),
    )
}

type AgentMsgRow = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
    Option<String>,
);

fn from_pg_row(r: AgentMsgRow) -> AgentMessage {
    AgentMessage {
        id: r.0,
        from_agent: r.1,
        to_agent: r.2,
        package_id: r.3,
        package_title: r.4,
        priority: r.5,
        content: r.6,
        category: r.7,
        layer: r.8,
        acknowledged: r.9,
        created_at: r.10,
        acknowledged_at: r.11,
    }
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS agent_messages (
                id BIGSERIAL PRIMARY KEY,
                from_agent TEXT NOT NULL,
                to_agent TEXT,
                package_id TEXT,
                package_title TEXT,
                priority TEXT NOT NULL DEFAULT 'normal',
                content TEXT NOT NULL,
                category TEXT,
                layer TEXT,
                acknowledged BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                acknowledged_at TIMESTAMPTZ
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("ALTER TABLE agent_messages ADD COLUMN IF NOT EXISTS package_id TEXT")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE agent_messages ADD COLUMN IF NOT EXISTS package_title TEXT")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_agent_messages_to ON agent_messages(to_agent)")
            .execute(pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_agent_messages_ack ON agent_messages(acknowledged)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_agent_messages_package ON agent_messages(package_id)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn send_message_postgres(
    pool: &PgPool,
    from: &str,
    to: Option<&str>,
    priority: Option<&str>,
    content: &str,
    category: Option<&str>,
    layer: Option<&str>,
    package_id: Option<&str>,
    package_title: Option<&str>,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO agent_messages (from_agent, to_agent, package_id, package_title, priority, content, category, layer)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
        )
        .bind(from)
        .bind(to)
        .bind(package_id)
        .bind(package_title)
        .bind(priority.unwrap_or("normal"))
        .bind(content)
        .bind(category)
        .bind(layer)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn list_messages_postgres(
    pool: &PgPool,
    from: Option<&str>,
    to: Option<&str>,
    layer: Option<&str>,
    unacked_only: bool,
    since: Option<&str>,
    package_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AgentMessage>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<AgentMsgRow> = match (from, to, layer, unacked_only, since, package_id, limit) {
        (Some(f), Some(t), Some(l), true, Some(s), None, Some(n)) => {
            crate::db::pg_runtime::block_on(async {
                sqlx::query_as(
                "SELECT id, from_agent, to_agent, package_id, package_title, priority, content, category, layer, acknowledged, created_at::text, acknowledged_at::text
                 FROM agent_messages
                 WHERE from_agent = $1 AND (to_agent IS NULL OR to_agent = $2) AND layer = $3 AND acknowledged = 0 AND created_at::text >= $4
                 ORDER BY created_at DESC
                 LIMIT $5",
            )
            .bind(f)
            .bind(t)
            .bind(l)
            .bind(s)
            .bind(n as i64)
            .fetch_all(pool)
            .await
            })?
        }
        _ => {
            // Simpler incremental filtering for maintainability.
            let mut rows: Vec<AgentMsgRow> = crate::db::pg_runtime::block_on(async {
                sqlx::query_as(
                    "SELECT id, from_agent, to_agent, package_id, package_title, priority, content, category, layer, acknowledged, created_at::text, acknowledged_at::text
                     FROM agent_messages
                     ORDER BY created_at DESC",
                )
                .fetch_all(pool)
                .await
            })?;
            if let Some(f) = from {
                rows.retain(|r| r.1 == f);
            }
            if let Some(t) = to {
                rows.retain(|r| r.2.as_deref().is_none_or(|v| v == t));
            }
            if let Some(l) = layer {
                rows.retain(|r| r.8.as_deref().is_some_and(|v| v == l));
            }
            if unacked_only {
                rows.retain(|r| r.9 == 0);
            }
            if let Some(s) = since {
                rows.retain(|r| r.10.as_str() >= s);
            }
            if let Some(pid) = package_id {
                rows.retain(|r| r.3.as_deref().is_some_and(|v| v == pid));
            }
            if let Some(n) = limit {
                rows.truncate(n);
            }
            rows
        }
    };
    Ok(rows.into_iter().map(from_pg_row).collect())
}

fn get_message_by_id_postgres(pool: &PgPool, id: i64) -> Result<Option<AgentMessage>> {
    ensure_tables_postgres(pool)?;
    let row: Option<AgentMsgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, from_agent, to_agent, package_id, package_title, priority, content, category, layer, acknowledged, created_at::text, acknowledged_at::text
             FROM agent_messages
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(from_pg_row))
}

fn acknowledge_postgres(pool: &PgPool, id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE agent_messages
             SET acknowledged = 1, acknowledged_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn acknowledge_all_postgres(pool: &PgPool, to: &str) -> Result<usize> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE agent_messages
             SET acknowledged = 1, acknowledged_at = NOW()
             WHERE acknowledged = 0 AND (to_agent = $1 OR to_agent IS NULL)",
        )
        .bind(to)
        .execute(pool)
        .await
    })?;
    Ok(rows.rows_affected() as usize)
}

fn purge_old_postgres(pool: &PgPool, days: usize) -> Result<usize> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "DELETE FROM agent_messages
             WHERE acknowledged = 1
               AND created_at < NOW() - (($1::TEXT || ' days')::INTERVAL)",
        )
        .bind(days as i64)
        .execute(pool)
        .await
    })?;
    Ok(rows.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn setup_test_db() -> Connection {
        open_in_memory()
    }

    #[test]
    fn send_and_filter_message_packages() {
        let conn = setup_test_db();
        let id1 = send_message(
            &conn,
            "macro-agent",
            Some("risk-agent"),
            Some("high"),
            "Rates impulse turning",
            Some("handoff"),
            Some("macro"),
            Some("pkg-1"),
            Some("Fed package"),
        )
        .unwrap();
        let id2 = send_message(
            &conn,
            "macro-agent",
            Some("risk-agent"),
            Some("high"),
            "Watch DXY and front-end yields",
            Some("handoff"),
            Some("macro"),
            Some("pkg-1"),
            Some("Fed package"),
        )
        .unwrap();

        let packaged = list_messages(
            &conn,
            Some("macro-agent"),
            Some("risk-agent"),
            Some("macro"),
            false,
            None,
            Some("pkg-1"),
            None,
        )
        .unwrap();

        assert_eq!(packaged.len(), 2);
        assert_eq!(packaged[0].package_id.as_deref(), Some("pkg-1"));
        assert_eq!(packaged[0].package_title.as_deref(), Some("Fed package"));

        let inserted = get_message_by_id(&conn, id1).unwrap().unwrap();
        assert_eq!(inserted.package_id.as_deref(), Some("pkg-1"));
        let inserted2 = get_message_by_id(&conn, id2).unwrap().unwrap();
        assert_eq!(inserted2.package_title.as_deref(), Some("Fed package"));
    }
}
