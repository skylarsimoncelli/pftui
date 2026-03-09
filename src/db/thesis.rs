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

pub fn upsert_thesis_backend(
    backend: &BackendConnection,
    section: &str,
    content: &str,
    conviction: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_thesis(conn, section, content, conviction),
        |pool| upsert_thesis_postgres(pool, section, content, conviction),
    )
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

pub fn list_thesis_backend(backend: &BackendConnection) -> Result<Vec<ThesisEntry>> {
    query::dispatch(backend, list_thesis, list_thesis_postgres)
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

pub fn remove_thesis(conn: &Connection, section: &str) -> Result<()> {
    conn.execute("DELETE FROM thesis WHERE section = ?", params![section])?;
    Ok(())
}

pub fn remove_thesis_backend(backend: &BackendConnection, section: &str) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_thesis(conn, section),
        |pool| remove_thesis_postgres(pool, section),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
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
                conviction TEXT NOT NULL DEFAULT 'medium',
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type ThesisRow = (i64, String, String, String, String);
type ThesisHistRow = (i64, String, String, String, String);

fn to_thesis_entry(row: ThesisRow) -> ThesisEntry {
    ThesisEntry {
        id: row.0,
        section: row.1,
        content: row.2,
        conviction: row.3,
        updated_at: row.4,
    }
}

fn to_thesis_history_entry(row: ThesisHistRow) -> ThesisHistoryEntry {
    ThesisHistoryEntry {
        id: row.0,
        section: row.1,
        content: row.2,
        conviction: row.3,
        recorded_at: row.4,
    }
}

fn upsert_thesis_postgres(
    pool: &PgPool,
    section: &str,
    content: &str,
    conviction: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        let existing: Option<(String, String)> = sqlx::query_as(
            "SELECT content, conviction
             FROM thesis
             WHERE section = $1",
        )
        .bind(section)
        .fetch_optional(pool)
        .await?;

        if let Some((existing_content, existing_conviction)) = &existing {
            sqlx::query(
                "INSERT INTO thesis_history (section, content, conviction)
                 VALUES ($1, $2, $3)",
            )
            .bind(section)
            .bind(existing_content)
            .bind(existing_conviction)
            .execute(pool)
            .await?;
        }

        let final_conviction = conviction
            .map(ToOwned::to_owned)
            .or_else(|| existing.as_ref().map(|(_, c)| c.clone()))
            .unwrap_or_else(|| "medium".to_string());

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
        .bind(final_conviction)
        .execute(pool)
        .await?;

        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_thesis_postgres(pool: &PgPool) -> Result<Vec<ThesisEntry>> {
    ensure_tables_postgres(pool)?;
        let rows: Vec<ThesisRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, section, content, conviction, updated_at::text
             FROM thesis
             ORDER BY section ASC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(to_thesis_entry).collect())
}

fn get_thesis_section_postgres(pool: &PgPool, section: &str) -> Result<Option<ThesisEntry>> {
    ensure_tables_postgres(pool)?;
        let row: Option<ThesisRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, section, content, conviction, updated_at::text
             FROM thesis
             WHERE section = $1",
        )
        .bind(section)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_thesis_entry))
}

fn get_thesis_history_postgres(
    pool: &PgPool,
    section: &str,
    limit: Option<usize>,
) -> Result<Vec<ThesisHistoryEntry>> {
    ensure_tables_postgres(pool)?;
        let rows: Vec<ThesisHistRow> = crate::db::pg_runtime::block_on(async {
        if let Some(limit) = limit {
            sqlx::query_as(
                "SELECT id, section, content, conviction, recorded_at::text
                 FROM thesis_history
                 WHERE section = $1
                 ORDER BY recorded_at DESC
                 LIMIT $2",
            )
            .bind(section)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT id, section, content, conviction, recorded_at::text
                 FROM thesis_history
                 WHERE section = $1
                 ORDER BY recorded_at DESC",
            )
            .bind(section)
            .fetch_all(pool)
            .await
        }
    })?;
    Ok(rows.into_iter().map(to_thesis_history_entry).collect())
}

fn remove_thesis_postgres(pool: &PgPool, section: &str) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM thesis WHERE section = $1")
            .bind(section)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
