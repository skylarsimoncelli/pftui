use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyNote {
    pub id: i64,
    pub date: String,
    pub section: String,
    pub content: String,
    pub author: String,
    pub created_at: String,
}

impl DailyNote {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            date: row.get(1)?,
            section: row.get(2)?,
            content: row.get(3)?,
            author: row.get(4)?,
            created_at: row.get(5)?,
        })
    }
}

pub fn add_note(
    conn: &Connection,
    date: &str,
    section: &str,
    content: &str,
    author: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO daily_notes (date, section, content, author)
         VALUES (?, ?, ?, ?)",
        params![date, section, content, author],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_notes(
    conn: &Connection,
    date: Option<&str>,
    section: Option<&str>,
    limit: Option<usize>,
    author: Option<&str>,
) -> Result<Vec<DailyNote>> {
    let mut query = String::from(
        "SELECT id, date, section, content, author, created_at
         FROM daily_notes",
    );

    let mut where_parts = Vec::new();
    if let Some(d) = date {
        where_parts.push(format!("date = '{}'", d.replace('"', "''")));
    }
    if let Some(s) = section {
        where_parts.push(format!("section = '{}'", s.replace('"', "''")));
    }
    if let Some(a) = author {
        where_parts.push(format!("author = '{}'", a.replace('\'', "''")));
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
        "SELECT id, date, section, content, author, created_at
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

pub fn add_note_backend(
    backend: &BackendConnection,
    date: &str,
    section: &str,
    content: &str,
    author: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_note(conn, date, section, content, author),
        |pool| add_note_postgres(pool, date, section, content, author),
    )
}

pub fn list_notes_backend(
    backend: &BackendConnection,
    date: Option<&str>,
    section: Option<&str>,
    limit: Option<usize>,
    author: Option<&str>,
) -> Result<Vec<DailyNote>> {
    query::dispatch(
        backend,
        |conn| list_notes(conn, date, section, limit, author),
        |pool| list_notes_postgres(pool, date, section, limit, author),
    )
}

pub fn search_notes_backend(
    backend: &BackendConnection,
    query_text: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<DailyNote>> {
    query::dispatch(
        backend,
        |conn| search_notes(conn, query_text, since, limit),
        |pool| search_notes_postgres(pool, query_text, since, limit),
    )
}

pub fn remove_note_backend(backend: &BackendConnection, id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_note(conn, id),
        |pool| remove_note_postgres(pool, id),
    )
}

type DailyNoteRow = (i64, String, String, String, String, String);

fn from_pg_row(r: DailyNoteRow) -> DailyNote {
    DailyNote {
        id: r.0,
        date: r.1,
        section: r.2,
        content: r.3,
        author: r.4,
        created_at: r.5,
    }
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS daily_notes (
                id BIGSERIAL PRIMARY KEY,
                date TEXT NOT NULL,
                section TEXT NOT NULL,
                content TEXT NOT NULL,
                author TEXT NOT NULL DEFAULT 'system',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE daily_notes ADD COLUMN IF NOT EXISTS author TEXT NOT NULL DEFAULT 'system'",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_daily_notes_date ON daily_notes(date)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_daily_notes_section ON daily_notes(section)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_daily_notes_author ON daily_notes(author)")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_note_postgres(
    pool: &PgPool,
    date: &str,
    section: &str,
    content: &str,
    author: &str,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO daily_notes (date, section, content, author)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(date)
        .bind(section)
        .bind(content)
        .bind(author)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_notes_postgres(
    pool: &PgPool,
    date: Option<&str>,
    section: Option<&str>,
    limit: Option<usize>,
    author: Option<&str>,
) -> Result<Vec<DailyNote>> {
    ensure_tables_postgres(pool)?;
    let mut rows: Vec<DailyNoteRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, date, section, content, author, created_at::text
             FROM daily_notes
             ORDER BY date DESC, created_at DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    if let Some(d) = date {
        rows.retain(|r| r.1 == d);
    }
    if let Some(s) = section {
        rows.retain(|r| r.2 == s);
    }
    if let Some(a) = author {
        rows.retain(|r| r.4 == a);
    }
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Ok(rows.into_iter().map(from_pg_row).collect())
}

fn search_notes_postgres(
    pool: &PgPool,
    query_text: &str,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<DailyNote>> {
    ensure_tables_postgres(pool)?;
    let mut rows: Vec<DailyNoteRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, date, section, content, author, created_at::text
             FROM daily_notes
             WHERE content ILIKE $1
             ORDER BY date DESC, created_at DESC",
        )
        .bind(format!("%{}%", query_text))
        .fetch_all(pool)
        .await
    })?;
    if let Some(s) = since {
        rows.retain(|r| r.1.as_str() >= s);
    }
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Ok(rows.into_iter().map(from_pg_row).collect())
}

fn remove_note_postgres(pool: &PgPool, id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM daily_notes WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
