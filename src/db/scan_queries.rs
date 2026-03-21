use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct ScanQueryRow {
    pub name: String,
    pub filter_expr: String,
    pub updated_at: String,
}

pub fn upsert_scan_query(conn: &Connection, name: &str, filter_expr: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO scan_queries (name, filter_expr, updated_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(name) DO UPDATE
         SET filter_expr = excluded.filter_expr,
             updated_at = datetime('now')",
        params![name, filter_expr],
    )?;
    Ok(())
}

pub fn upsert_scan_query_backend(
    backend: &BackendConnection,
    name: &str,
    filter_expr: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_scan_query(conn, name, filter_expr),
        |pool| upsert_scan_query_postgres(pool, name, filter_expr),
    )
}

pub fn get_scan_query(conn: &Connection, name: &str) -> Result<Option<ScanQueryRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, filter_expr, updated_at
         FROM scan_queries
         WHERE name = ?1",
    )?;
    let mut rows = stmt.query(params![name])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(ScanQueryRow {
            name: row.get(0)?,
            filter_expr: row.get(1)?,
            updated_at: row.get(2)?,
        }));
    }
    Ok(None)
}

pub fn get_scan_query_backend(
    backend: &BackendConnection,
    name: &str,
) -> Result<Option<ScanQueryRow>> {
    query::dispatch(
        backend,
        |conn| get_scan_query(conn, name),
        |pool| get_scan_query_postgres(pool, name),
    )
}

pub fn list_scan_queries(conn: &Connection) -> Result<Vec<ScanQueryRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, filter_expr, updated_at
         FROM scan_queries
         ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ScanQueryRow {
            name: row.get(0)?,
            filter_expr: row.get(1)?,
            updated_at: row.get(2)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn list_scan_queries_backend(backend: &BackendConnection) -> Result<Vec<ScanQueryRow>> {
    query::dispatch(backend, list_scan_queries, list_scan_queries_postgres)
}

fn upsert_scan_query_postgres(pool: &PgPool, name: &str, filter_expr: &str) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO scan_queries (name, filter_expr, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT(name) DO UPDATE
             SET filter_expr = EXCLUDED.filter_expr,
                 updated_at = NOW()",
        )
        .bind(name)
        .bind(filter_expr)
        .execute(pool)
        .await
    })?;
    Ok(())
}

fn get_scan_query_postgres(pool: &PgPool, name: &str) -> Result<Option<ScanQueryRow>> {
    let row = crate::db::pg_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT name, filter_expr, TO_CHAR(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             FROM scan_queries
             WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|(name, filter_expr, updated_at)| ScanQueryRow {
        name,
        filter_expr,
        updated_at,
    }))
}

fn list_scan_queries_postgres(pool: &PgPool) -> Result<Vec<ScanQueryRow>> {
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT name, filter_expr, TO_CHAR(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             FROM scan_queries
             ORDER BY name ASC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|(name, filter_expr, updated_at)| ScanQueryRow {
            name,
            filter_expr,
            updated_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_get_and_list() {
        let conn = crate::db::open_in_memory();
        upsert_scan_query(&conn, "risk", "allocation_pct > 10").unwrap();
        upsert_scan_query(&conn, "risk", "allocation_pct > 12").unwrap();

        let row = get_scan_query(&conn, "risk").unwrap().unwrap();
        assert_eq!(row.filter_expr, "allocation_pct > 12");

        let all = list_scan_queries(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "risk");
    }
}
