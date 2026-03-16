use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsensusCall {
    pub id: i64,
    pub source: String,
    pub topic: String,
    pub call_text: String,
    pub call_date: String,
    pub created_at: String,
}

impl ConsensusCall {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            source: row.get(1)?,
            topic: row.get(2)?,
            call_text: row.get(3)?,
            call_date: row.get(4)?,
            created_at: row.get(5)?,
        })
    }
}

pub fn add_call(
    conn: &Connection,
    source: &str,
    topic: &str,
    call_text: &str,
    call_date: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO consensus_tracker (source, topic, call_text, call_date)
         VALUES (?1, ?2, ?3, ?4)",
        params![source, topic, call_text, call_date],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_calls(
    conn: &Connection,
    topic: Option<&str>,
    source: Option<&str>,
    limit: usize,
) -> Result<Vec<ConsensusCall>> {
    let mut sql = String::from(
        "SELECT id, source, topic, call_text, call_date, created_at
         FROM consensus_tracker
         WHERE 1=1",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(topic) = topic {
        sql.push_str(" AND topic = ?");
        params_vec.push(Box::new(topic.to_string()));
    }
    if let Some(source) = source {
        sql.push_str(" AND source = ?");
        params_vec.push(Box::new(source.to_string()));
    }

    sql.push_str(" ORDER BY call_date DESC, created_at DESC LIMIT ?");
    params_vec.push(Box::new(limit as i64));

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), ConsensusCall::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn add_call_backend(
    backend: &BackendConnection,
    source: &str,
    topic: &str,
    call_text: &str,
    call_date: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_call(conn, source, topic, call_text, call_date),
        |pool| add_call_postgres(pool, source, topic, call_text, call_date),
    )
}

pub fn list_calls_backend(
    backend: &BackendConnection,
    topic: Option<&str>,
    source: Option<&str>,
    limit: usize,
) -> Result<Vec<ConsensusCall>> {
    query::dispatch(
        backend,
        |conn| list_calls(conn, topic, source, limit),
        |pool| list_calls_postgres(pool, topic, source, limit),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS consensus_tracker (
                id BIGSERIAL PRIMARY KEY,
                source TEXT NOT NULL,
                topic TEXT NOT NULL,
                call_text TEXT NOT NULL,
                call_date TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_consensus_tracker_topic ON consensus_tracker(topic)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_consensus_tracker_date ON consensus_tracker(call_date)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_call_postgres(
    pool: &PgPool,
    source: &str,
    topic: &str,
    call_text: &str,
    call_date: &str,
) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO consensus_tracker (source, topic, call_text, call_date)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(source)
        .bind(topic)
        .bind(call_text)
        .bind(call_date)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_calls_postgres(
    pool: &PgPool,
    topic: Option<&str>,
    source: Option<&str>,
    limit: usize,
) -> Result<Vec<ConsensusCall>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<(i64, String, String, String, String, String)> =
        crate::db::pg_runtime::block_on(async {
            let mut builder = sqlx::QueryBuilder::new(
                "SELECT id, source, topic, call_text, call_date, created_at::text
                 FROM consensus_tracker
                 WHERE 1=1",
            );

            if let Some(topic) = topic {
                builder.push(" AND topic = ").push_bind(topic);
            }
            if let Some(source) = source {
                builder.push(" AND source = ").push_bind(source);
            }

            builder
                .push(" ORDER BY call_date DESC, created_at DESC LIMIT ")
                .push_bind(limit as i64);

            builder.build_query_as().fetch_all(pool).await
        })?;

    Ok(rows
        .into_iter()
        .map(
            |(id, source, topic, call_text, call_date, created_at)| ConsensusCall {
                id,
                source,
                topic,
                call_text,
                call_date,
                created_at,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_and_filters_consensus_calls() {
        let conn = crate::db::open_in_memory();
        add_call(
            &conn,
            "Goldman Sachs",
            "rate_cuts",
            "50bp cuts in Sep+Dec 2026",
            "2026-03-12",
        )
        .unwrap();
        add_call(
            &conn,
            "JP Morgan",
            "gold_target",
            "$6,300 by year-end 2026",
            "2026-02-25",
        )
        .unwrap();

        let rate_cuts = list_calls(&conn, Some("rate_cuts"), None, 10).unwrap();
        assert_eq!(rate_cuts.len(), 1);
        assert_eq!(rate_cuts[0].source, "Goldman Sachs");
        assert_eq!(rate_cuts[0].call_text, "50bp cuts in Sep+Dec 2026");
    }
}
