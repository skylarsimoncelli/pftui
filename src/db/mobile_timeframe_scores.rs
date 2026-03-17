use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MobileTimeframeScore {
    pub timeframe: String,
    pub score: f64,
    pub summary: Option<String>,
    pub updated_at: String,
}

impl MobileTimeframeScore {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            timeframe: row.get(0)?,
            score: row.get(1)?,
            summary: row.get(2)?,
            updated_at: row.get(3)?,
        })
    }
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mobile_timeframe_scores (
            timeframe TEXT PRIMARY KEY
                CHECK(timeframe IN ('low', 'medium', 'high', 'macro')),
            score REAL NOT NULL
                CHECK(score >= -100 AND score <= 100),
            summary TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    Ok(())
}

pub fn list_scores(conn: &Connection) -> Result<Vec<MobileTimeframeScore>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT timeframe, score, summary, updated_at
         FROM mobile_timeframe_scores
         ORDER BY CASE timeframe
            WHEN 'low' THEN 1
            WHEN 'medium' THEN 2
            WHEN 'high' THEN 3
            WHEN 'macro' THEN 4
            ELSE 5
         END",
    )?;
    let rows = stmt.query_map([], MobileTimeframeScore::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn upsert_score(
    conn: &Connection,
    timeframe: &str,
    score: f64,
    summary: Option<&str>,
) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO mobile_timeframe_scores (timeframe, score, summary, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(timeframe) DO UPDATE SET
            score = excluded.score,
            summary = excluded.summary,
            updated_at = datetime('now')",
        params![timeframe, score, summary],
    )?;
    Ok(())
}

pub fn list_scores_backend(backend: &BackendConnection) -> Result<Vec<MobileTimeframeScore>> {
    query::dispatch(backend, list_scores, list_scores_postgres)
}

#[allow(dead_code)]
pub fn upsert_score_backend(
    backend: &BackendConnection,
    timeframe: &str,
    score: f64,
    summary: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_score(conn, timeframe, score, summary),
        |pool| upsert_score_postgres(pool, timeframe, score, summary),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS mobile_timeframe_scores (
                timeframe TEXT PRIMARY KEY
                    CHECK(timeframe IN ('low', 'medium', 'high', 'macro')),
                score DOUBLE PRECISION NOT NULL
                    CHECK(score >= -100 AND score <= 100),
                summary TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type ScoreRow = (String, f64, Option<String>, String);

fn to_score(row: ScoreRow) -> MobileTimeframeScore {
    MobileTimeframeScore {
        timeframe: row.0,
        score: row.1,
        summary: row.2,
        updated_at: row.3,
    }
}

fn list_scores_postgres(pool: &PgPool) -> Result<Vec<MobileTimeframeScore>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<ScoreRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT timeframe, score, summary, updated_at::text
             FROM mobile_timeframe_scores
             ORDER BY CASE timeframe
                WHEN 'low' THEN 1
                WHEN 'medium' THEN 2
                WHEN 'high' THEN 3
                WHEN 'macro' THEN 4
                ELSE 5
             END",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(to_score).collect())
}

fn upsert_score_postgres(
    pool: &PgPool,
    timeframe: &str,
    score: f64,
    summary: Option<&str>,
) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO mobile_timeframe_scores (timeframe, score, summary, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT(timeframe) DO UPDATE SET
                score = EXCLUDED.score,
                summary = EXCLUDED.summary,
                updated_at = NOW()",
        )
        .bind(timeframe)
        .bind(score)
        .bind(summary)
        .execute(pool)
        .await
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upserts_and_lists_scores() {
        let conn = crate::db::open_in_memory();
        upsert_score(&conn, "low", 35.0, Some("Near-term risk improving")).unwrap();
        upsert_score(&conn, "macro", -20.0, Some("Liquidity still tight")).unwrap();

        let rows = list_scores(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].timeframe, "low");
        assert_eq!(rows[0].score, 35.0);
        assert_eq!(rows[1].timeframe, "macro");
        assert_eq!(rows[1].score, -20.0);
    }
}
