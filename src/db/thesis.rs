use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

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

pub fn upsert_thesis_backend(
    backend: &BackendConnection,
    section: &str,
    content: &str,
    conviction: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_thesis(conn, section, content, conviction),
        |pool| upsert_thesis_postgres(pool, section, content, conviction),
    )
}

pub fn list_thesis_backend(backend: &BackendConnection) -> Result<Vec<ThesisEntry>> {
    query::dispatch(backend, list_thesis, list_thesis_postgres)
}

pub fn get_thesis_section_backend(
    backend: &BackendConnection,
    section: &str,
) -> Result<Option<ThesisEntry>> {
    query::dispatch(
        backend,
        |conn| get_thesis_section(conn, section),
        |pool| get_thesis_section_postgres(pool, section),
    )
}

pub fn get_thesis_history_backend(
    backend: &BackendConnection,
    section: &str,
    limit: Option<usize>,
) -> Result<Vec<ThesisHistoryEntry>> {
    query::dispatch(
        backend,
        |conn| get_thesis_history(conn, section, limit),
        |pool| get_thesis_history_postgres(pool, section, limit),
    )
}

pub fn remove_thesis_backend(backend: &BackendConnection, section: &str) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_thesis(conn, section),
        |pool| remove_thesis_postgres(pool, section),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS thesis (
                id BIGSERIAL PRIMARY KEY,
                section TEXT NOT NULL UNIQUE,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL DEFAULT 'medium',
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS thesis_history (
                id BIGSERIAL PRIMARY KEY,
                section TEXT NOT NULL,
                content TEXT NOT NULL,
                conviction TEXT NOT NULL,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_thesis_history_section ON thesis_history(section)")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_thesis_postgres(pool: &PgPool, section: &str, content: &str, conviction: &str) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "INSERT INTO thesis_history (section, content, conviction)
             SELECT section, content, conviction FROM thesis WHERE section = $1",
        )
        .bind(section)
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO thesis (section, content, conviction, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT(section) DO UPDATE SET
                content = EXCLUDED.content,
                conviction = EXCLUDED.conviction,
                updated_at = NOW()",
        )
        .bind(section)
        .bind(content)
        .bind(conviction)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_thesis_postgres(pool: &PgPool) -> Result<Vec<ThesisEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(i64, String, String, String, String)> = runtime.block_on(async {
        sqlx::query_as::<_, (i64, String, String, String, String)>(
            "SELECT id, section, content, conviction, updated_at::text
             FROM thesis
             ORDER BY section ASC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ThesisEntry {
            id: r.0,
            section: r.1,
            content: r.2,
            conviction: r.3,
            updated_at: r.4,
        })
        .collect())
}

fn get_thesis_section_postgres(pool: &PgPool, section: &str) -> Result<Option<ThesisEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let row: Option<(i64, String, String, String, String)> = runtime.block_on(async {
        sqlx::query_as::<_, (i64, String, String, String, String)>(
            "SELECT id, section, content, conviction, updated_at::text
             FROM thesis
             WHERE section = $1",
        )
        .bind(section)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|r| ThesisEntry {
        id: r.0,
        section: r.1,
        content: r.2,
        conviction: r.3,
        updated_at: r.4,
    }))
}

fn get_thesis_history_postgres(
    pool: &PgPool,
    section: &str,
    limit: Option<usize>,
) -> Result<Vec<ThesisHistoryEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(i64, String, String, String, String)> = if let Some(limit) = limit {
        runtime.block_on(async {
            sqlx::query_as::<_, (i64, String, String, String, String)>(
                "SELECT id, section, content, conviction, recorded_at::text
                 FROM thesis_history
                 WHERE section = $1
                 ORDER BY recorded_at DESC, id DESC
                 LIMIT $2",
            )
            .bind(section)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        })?
    } else {
        runtime.block_on(async {
            sqlx::query_as::<_, (i64, String, String, String, String)>(
                "SELECT id, section, content, conviction, recorded_at::text
                 FROM thesis_history
                 WHERE section = $1
                 ORDER BY recorded_at DESC, id DESC",
            )
            .bind(section)
            .fetch_all(pool)
            .await
        })?
    };
    Ok(rows
        .into_iter()
        .map(|r| ThesisHistoryEntry {
            id: r.0,
            section: r.1,
            content: r.2,
            conviction: r.3,
            recorded_at: r.4,
        })
        .collect())
}

fn remove_thesis_postgres(pool: &PgPool, section: &str) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query("DELETE FROM thesis WHERE section = $1")
            .bind(section)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
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
