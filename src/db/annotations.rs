use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct Annotation {
    pub symbol: String,
    pub thesis: String,
    pub invalidation: Option<String>,
    pub review_date: Option<String>,
    pub target_price: Option<String>,
    pub updated_at: String,
}

pub fn get_annotation(conn: &Connection, symbol: &str) -> Result<Option<Annotation>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, thesis, invalidation, review_date, target_price, updated_at
         FROM annotations
         WHERE symbol = ?1",
    )?;
    let item = stmt
        .query_row(params![symbol], |row| {
            Ok(Annotation {
                symbol: row.get(0)?,
                thesis: row.get(1)?,
                invalidation: row.get(2)?,
                review_date: row.get(3)?,
                target_price: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .ok();
    Ok(item)
}

pub fn get_annotation_backend(backend: &BackendConnection, symbol: &str) -> Result<Option<Annotation>> {
    query::dispatch(
        backend,
        |conn| get_annotation(conn, symbol),
        |pool| get_annotation_postgres(pool, symbol),
    )
}

pub fn list_annotations(conn: &Connection) -> Result<Vec<Annotation>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, thesis, invalidation, review_date, target_price, updated_at
         FROM annotations
         ORDER BY COALESCE(review_date, '9999-12-31') ASC, symbol ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Annotation {
            symbol: row.get(0)?,
            thesis: row.get(1)?,
            invalidation: row.get(2)?,
            review_date: row.get(3)?,
            target_price: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn list_annotations_backend(backend: &BackendConnection) -> Result<Vec<Annotation>> {
    query::dispatch(backend, list_annotations, list_annotations_postgres)
}

pub fn upsert_annotation(conn: &Connection, ann: &Annotation) -> Result<()> {
    conn.execute(
        "INSERT INTO annotations (symbol, thesis, invalidation, review_date, target_price, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(symbol) DO UPDATE SET
            thesis = excluded.thesis,
            invalidation = excluded.invalidation,
            review_date = excluded.review_date,
            target_price = excluded.target_price,
            updated_at = datetime('now')",
        params![
            ann.symbol,
            ann.thesis,
            ann.invalidation,
            ann.review_date,
            ann.target_price
        ],
    )?;
    Ok(())
}

pub fn upsert_annotation_backend(backend: &BackendConnection, ann: &Annotation) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_annotation(conn, ann),
        |pool| upsert_annotation_postgres(pool, ann),
    )
}

pub fn remove_annotation(conn: &Connection, symbol: &str) -> Result<bool> {
    let changed = conn.execute("DELETE FROM annotations WHERE symbol = ?1", params![symbol])?;
    Ok(changed > 0)
}

pub fn remove_annotation_backend(backend: &BackendConnection, symbol: &str) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| remove_annotation(conn, symbol),
        |pool| remove_annotation_postgres(pool, symbol),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS annotations (
                symbol TEXT PRIMARY KEY,
                thesis TEXT NOT NULL DEFAULT '',
                invalidation TEXT,
                review_date TEXT,
                target_price TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type AnnRow = (String, String, Option<String>, Option<String>, Option<String>, String);

fn to_annotation(r: AnnRow) -> Annotation {
    Annotation {
        symbol: r.0,
        thesis: r.1,
        invalidation: r.2,
        review_date: r.3,
        target_price: r.4,
        updated_at: r.5,
    }
}

fn get_annotation_postgres(pool: &PgPool, symbol: &str) -> Result<Option<Annotation>> {
    ensure_tables_postgres(pool)?;
        let row: Option<AnnRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT symbol, thesis, invalidation, review_date, target_price, updated_at::text
             FROM annotations
             WHERE symbol = $1",
        )
        .bind(symbol)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_annotation))
}

fn list_annotations_postgres(pool: &PgPool) -> Result<Vec<Annotation>> {
    ensure_tables_postgres(pool)?;
        let rows: Vec<AnnRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT symbol, thesis, invalidation, review_date, target_price, updated_at::text
             FROM annotations
             ORDER BY COALESCE(review_date, '9999-12-31') ASC, symbol ASC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(to_annotation).collect())
}

fn upsert_annotation_postgres(pool: &PgPool, ann: &Annotation) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO annotations (symbol, thesis, invalidation, review_date, target_price, updated_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             ON CONFLICT(symbol) DO UPDATE SET
               thesis = EXCLUDED.thesis,
               invalidation = EXCLUDED.invalidation,
               review_date = EXCLUDED.review_date,
               target_price = EXCLUDED.target_price,
               updated_at = NOW()",
        )
        .bind(&ann.symbol)
        .bind(&ann.thesis)
        .bind(&ann.invalidation)
        .bind(&ann.review_date)
        .bind(&ann.target_price)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn remove_annotation_postgres(pool: &PgPool, symbol: &str) -> Result<bool> {
    ensure_tables_postgres(pool)?;
        let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM annotations WHERE symbol = $1")
            .bind(symbol)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_get_remove_roundtrip() {
        let conn = crate::db::open_in_memory();
        let ann = Annotation {
            symbol: "GC=F".to_string(),
            thesis: "Long-term inflation hedge".to_string(),
            invalidation: Some("Real rates break higher".to_string()),
            review_date: Some("2026-06-30".to_string()),
            target_price: Some("5500".to_string()),
            updated_at: String::new(),
        };
        upsert_annotation(&conn, &ann).unwrap();

        let fetched = get_annotation(&conn, "GC=F").unwrap().unwrap();
        assert_eq!(fetched.symbol, "GC=F");
        assert_eq!(fetched.thesis, "Long-term inflation hedge");
        assert_eq!(fetched.review_date.as_deref(), Some("2026-06-30"));

        let removed = remove_annotation(&conn, "GC=F").unwrap();
        assert!(removed);
        assert!(get_annotation(&conn, "GC=F").unwrap().is_none());
    }
}
