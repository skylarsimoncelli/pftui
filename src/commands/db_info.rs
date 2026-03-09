use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::db::backend::BackendConnection;

#[derive(Debug, Serialize)]
struct TableInfo {
    name: String,
    rows: i64,
}

#[derive(Debug, Serialize)]
struct DbInfo {
    backend: String,
    target: String,
    table_count: usize,
    total_rows: i64,
    tables: Vec<TableInfo>,
}

pub fn run(
    backend: &BackendConnection,
    sqlite_path: &Path,
    database_url: Option<&str>,
    json: bool,
) -> Result<()> {
    let info = match backend {
        BackendConnection::Sqlite { conn } => {
            let tables = sqlite_tables_with_counts(conn)?;
            DbInfo {
                backend: "sqlite".to_string(),
                target: sqlite_path.display().to_string(),
                table_count: tables.len(),
                total_rows: tables.iter().map(|t| t.rows).sum(),
                tables,
            }
        }
        BackendConnection::Postgres { pool } => {
            let tables = postgres_tables_with_counts(pool)?;
            DbInfo {
                backend: "postgres".to_string(),
                target: database_url.unwrap_or("<from config>").to_string(),
                table_count: tables.len(),
                total_rows: tables.iter().map(|t| t.rows).sum(),
                tables,
            }
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }

    println!("Backend: {}", info.backend);
    println!("Target: {}", info.target);
    println!("Tables: {}", info.table_count);
    println!("Total rows: {}", info.total_rows);
    println!();
    println!("{:<40} {:>12}", "Table", "Rows");
    println!("{:-<40} {:-<12}", "", "");
    for t in &info.tables {
        println!("{:<40} {:>12}", t.name, t.rows);
    }

    Ok(())
}

fn sqlite_tables_with_counts(conn: &rusqlite::Connection) -> Result<Vec<TableInfo>> {
    let mut stmt = conn.prepare(
        "SELECT name
         FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
         ORDER BY name ASC",
    )?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let quoted = quote_ident(&name);
        let sql = format!("SELECT COUNT(*) FROM {quoted}");
        let rows: i64 = conn.query_row(&sql, [], |r| r.get(0)).unwrap_or(0);
        tables.push(TableInfo { name, rows });
    }
    Ok(tables)
}

fn postgres_tables_with_counts(pool: &sqlx::PgPool) -> Result<Vec<TableInfo>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let names: Vec<String> = runtime.block_on(async {
        sqlx::query_scalar(
            "SELECT table_name
             FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_type = 'BASE TABLE'
             ORDER BY table_name ASC",
        )
        .fetch_all(pool)
        .await
    })?;

    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let quoted = quote_ident(&name);
        let sql = format!("SELECT COUNT(*)::BIGINT FROM {quoted}");
        let rows: i64 = runtime.block_on(async {
            sqlx::query_scalar(&sql).fetch_one(pool).await
        })?;
        tables.push(TableInfo { name, rows });
    }
    Ok(tables)
}

fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
