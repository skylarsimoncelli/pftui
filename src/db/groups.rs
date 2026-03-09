use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct GroupRow {
    pub name: String,
    pub created_at: String,
}

pub fn create_group(conn: &Connection, name: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO groups (name) VALUES (?1)
         ON CONFLICT(name) DO NOTHING",
        params![name],
    )?;
    Ok(())
}

pub fn create_group_backend(backend: &BackendConnection, name: &str) -> Result<()> {
    query::dispatch(
        backend,
        |conn| create_group(conn, name),
        |pool| create_group_postgres(pool, name),
    )
}

pub fn remove_group(conn: &Connection, name: &str) -> Result<bool> {
    let changed = conn.execute("DELETE FROM groups WHERE name = ?1", params![name])?;
    Ok(changed > 0)
}

pub fn remove_group_backend(backend: &BackendConnection, name: &str) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| remove_group(conn, name),
        |pool| remove_group_postgres(pool, name),
    )
}

pub fn list_groups(conn: &Connection) -> Result<Vec<GroupRow>> {
    let mut stmt = conn.prepare("SELECT name, created_at FROM groups ORDER BY name ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(GroupRow {
            name: row.get(0)?,
            created_at: row.get(1)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn list_groups_backend(backend: &BackendConnection) -> Result<Vec<GroupRow>> {
    query::dispatch(backend, list_groups, list_groups_postgres)
}

pub fn set_group_members(conn: &Connection, group_name: &str, symbols: &[String]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO groups (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
        params![group_name],
    )?;
    tx.execute("DELETE FROM group_members WHERE group_name = ?1", params![group_name])?;
    for sym in symbols {
        tx.execute(
            "INSERT INTO group_members (group_name, symbol) VALUES (?1, ?2)",
            params![group_name, sym.to_uppercase()],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn set_group_members_backend(
    backend: &BackendConnection,
    group_name: &str,
    symbols: &[String],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| set_group_members(conn, group_name, symbols),
        |pool| set_group_members_postgres(pool, group_name, symbols),
    )
}

pub fn get_group_members(conn: &Connection, group_name: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT symbol FROM group_members WHERE group_name = ?1 ORDER BY symbol ASC",
    )?;
    let rows = stmt.query_map(params![group_name], |row| row.get(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_group_members_backend(backend: &BackendConnection, group_name: &str) -> Result<Vec<String>> {
    query::dispatch(
        backend,
        |conn| get_group_members(conn, group_name),
        |pool| get_group_members_postgres(pool, group_name),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS groups (
                name TEXT PRIMARY KEY,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS group_members (
                group_name TEXT NOT NULL REFERENCES groups(name) ON DELETE CASCADE,
                symbol TEXT NOT NULL,
                PRIMARY KEY (group_name, symbol)
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn create_group_postgres(pool: &PgPool, name: &str) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "INSERT INTO groups (name) VALUES ($1)
             ON CONFLICT(name) DO NOTHING",
        )
        .bind(name)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn remove_group_postgres(pool: &PgPool, name: &str) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(async {
        sqlx::query("DELETE FROM groups WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected() > 0)
}

fn list_groups_postgres(pool: &PgPool) -> Result<Vec<GroupRow>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(String, String)> = runtime.block_on(async {
        sqlx::query_as("SELECT name, created_at::text FROM groups ORDER BY name ASC")
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|(name, created_at)| GroupRow { name, created_at })
        .collect())
}

fn set_group_members_postgres(pool: &PgPool, group_name: &str, symbols: &[String]) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO groups (name) VALUES ($1)
             ON CONFLICT(name) DO NOTHING",
        )
        .bind(group_name)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM group_members WHERE group_name = $1")
            .bind(group_name)
            .execute(&mut *tx)
            .await?;
        for sym in symbols {
            sqlx::query("INSERT INTO group_members (group_name, symbol) VALUES ($1, $2)")
                .bind(group_name)
                .bind(sym.to_uppercase())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_group_members_postgres(pool: &PgPool, group_name: &str) -> Result<Vec<String>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query_scalar::<_, String>(
            "SELECT symbol
             FROM group_members
             WHERE group_name = $1
             ORDER BY symbol ASC",
        )
        .bind(group_name)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_set_list_remove_group() {
        let conn = crate::db::open_in_memory();
        create_group(&conn, "hard-assets").unwrap();
        set_group_members(
            &conn,
            "hard-assets",
            &["GC=F".to_string(), "SI=F".to_string(), "BTC".to_string()],
        )
        .unwrap();

        let members = get_group_members(&conn, "hard-assets").unwrap();
        assert_eq!(members.len(), 3);
        assert!(members.contains(&"GC=F".to_string()));

        let groups = list_groups(&conn).unwrap();
        assert!(groups.iter().any(|g| g.name == "hard-assets"));

        assert!(remove_group(&conn, "hard-assets").unwrap());
        assert!(get_group_members(&conn, "hard-assets").unwrap().is_empty());
    }
}
