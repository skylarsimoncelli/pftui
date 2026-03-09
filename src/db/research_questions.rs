use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchQuestion {
    pub id: i64,
    pub question: String,
    pub evidence_tilt: String,
    pub key_signal: Option<String>,
    pub evidence: Option<String>,
    pub first_raised: String,
    pub last_updated: String,
    pub status: String,
    pub resolution: Option<String>,
}

impl ResearchQuestion {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            question: row.get(1)?,
            evidence_tilt: row.get(2)?,
            key_signal: row.get(3)?,
            evidence: row.get(4)?,
            first_raised: row.get(5)?,
            last_updated: row.get(6)?,
            status: row.get(7)?,
            resolution: row.get(8)?,
        })
    }
}

pub fn add_question(conn: &Connection, question: &str, key_signal: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO research_questions (question, key_signal)
         VALUES (?, ?)",
        params![question, key_signal],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_questions(conn: &Connection, status_filter: Option<&str>) -> Result<Vec<ResearchQuestion>> {
    let query = if let Some(status) = status_filter {
        format!(
            "SELECT id, question, evidence_tilt, key_signal, evidence, first_raised, last_updated, status, resolution
             FROM research_questions
             WHERE status = '{}'
             ORDER BY last_updated DESC",
            status.replace('"', "''")
        )
    } else {
        "SELECT id, question, evidence_tilt, key_signal, evidence, first_raised, last_updated, status, resolution
         FROM research_questions
         ORDER BY last_updated DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], ResearchQuestion::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn update_question(
    conn: &Connection,
    id: i64,
    tilt: Option<&str>,
    evidence: Option<&str>,
    key_signal: Option<&str>,
) -> Result<()> {
    if tilt.is_none() && evidence.is_none() && key_signal.is_none() {
        return Ok(());
    }

    let mut update_parts = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(t) = tilt {
        update_parts.push("evidence_tilt = ?");
        params.push(Box::new(t.to_string()));
    }

    if let Some(ev) = evidence {
        update_parts.push(
            "evidence = CASE
                WHEN evidence IS NULL OR evidence = '' THEN ?
                ELSE evidence || char(10) || ?
             END",
        );
        params.push(Box::new(ev.to_string()));
        params.push(Box::new(ev.to_string()));
    }

    if let Some(sig) = key_signal {
        update_parts.push("key_signal = ?");
        params.push(Box::new(sig.to_string()));
    }

    update_parts.push("last_updated = datetime('now')");

    let sql = format!(
        "UPDATE research_questions SET {} WHERE id = ?",
        update_parts.join(", ")
    );
    params.push(Box::new(id));

    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

pub fn resolve_question(conn: &Connection, id: i64, resolution: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE research_questions
         SET status = ?, resolution = ?, last_updated = datetime('now')
         WHERE id = ?",
        params![status, resolution, id],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub fn add_question_backend(
    backend: &BackendConnection,
    question: &str,
    key_signal: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_question(conn, question, key_signal),
        |pool| add_question_postgres(pool, question, key_signal),
    )
}

pub fn list_questions_backend(
    backend: &BackendConnection,
    status_filter: Option<&str>,
) -> Result<Vec<ResearchQuestion>> {
    query::dispatch(
        backend,
        |conn| list_questions(conn, status_filter),
        |pool| list_questions_postgres(pool, status_filter),
    )
}

#[allow(dead_code)]
pub fn update_question_backend(
    backend: &BackendConnection,
    id: i64,
    tilt: Option<&str>,
    evidence: Option<&str>,
    key_signal: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_question(conn, id, tilt, evidence, key_signal),
        |pool| update_question_postgres(pool, id, tilt, evidence, key_signal),
    )
}

#[allow(dead_code)]
pub fn resolve_question_backend(
    backend: &BackendConnection,
    id: i64,
    resolution: &str,
    status: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| resolve_question(conn, id, resolution, status),
        |pool| resolve_question_postgres(pool, id, resolution, status),
    )
}

type QuestionRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
);

fn from_pg_row(r: QuestionRow) -> ResearchQuestion {
    ResearchQuestion {
        id: r.0,
        question: r.1,
        evidence_tilt: r.2,
        key_signal: r.3,
        evidence: r.4,
        first_raised: r.5,
        last_updated: r.6,
        status: r.7,
        resolution: r.8,
    }
}

#[allow(dead_code)]
fn add_question_postgres(pool: &PgPool, question: &str, key_signal: Option<&str>) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO research_questions (question, key_signal)
             VALUES ($1, $2)
             RETURNING id",
        )
        .bind(question)
        .bind(key_signal)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_questions_postgres(pool: &PgPool, status_filter: Option<&str>) -> Result<Vec<ResearchQuestion>> {
    let rows: Vec<QuestionRow> = if let Some(status) = status_filter {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, question, evidence_tilt, key_signal, evidence, first_raised, last_updated::text, status, resolution
                 FROM research_questions
                 WHERE status = $1
                 ORDER BY last_updated DESC",
            )
            .bind(status)
            .fetch_all(pool)
            .await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, question, evidence_tilt, key_signal, evidence, first_raised, last_updated::text, status, resolution
                 FROM research_questions
                 ORDER BY last_updated DESC",
            )
            .fetch_all(pool)
            .await
        })?
    };
    Ok(rows.into_iter().map(from_pg_row).collect())
}

#[allow(dead_code)]
fn update_question_postgres(
    pool: &PgPool,
    id: i64,
    tilt: Option<&str>,
    evidence: Option<&str>,
    key_signal: Option<&str>,
) -> Result<()> {
    if tilt.is_none() && evidence.is_none() && key_signal.is_none() {
        return Ok(());
    }
    crate::db::pg_runtime::block_on(async {
        if let Some(v) = tilt {
            sqlx::query(
                "UPDATE research_questions SET evidence_tilt = $1, last_updated = NOW() WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(pool)
            .await?;
        }
        if let Some(v) = evidence {
            sqlx::query(
                "UPDATE research_questions
                 SET evidence = CASE
                     WHEN evidence IS NULL OR evidence = '' THEN $1
                     ELSE evidence || E'\\n' || $1
                 END,
                 last_updated = NOW()
                 WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(pool)
            .await?;
        }
        if let Some(v) = key_signal {
            sqlx::query(
                "UPDATE research_questions SET key_signal = $1, last_updated = NOW() WHERE id = $2",
            )
            .bind(v)
            .bind(id)
            .execute(pool)
            .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(dead_code)]
fn resolve_question_postgres(pool: &PgPool, id: i64, resolution: &str, status: &str) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE research_questions
             SET status = $1, resolution = $2, last_updated = NOW()
             WHERE id = $3",
        )
        .bind(status)
        .bind(resolution)
        .bind(id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
