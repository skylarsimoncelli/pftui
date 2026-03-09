use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::db::backend::BackendConnection;

#[derive(Debug, Serialize)]
struct TableInfo {
    name: String,
    rows: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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
        if t.error.is_some() {
            println!("{:<40} {:>12}", t.name, "ERR");
        } else {
            println!("{:<40} {:>12}", t.name, t.rows);
        }
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
        match conn.query_row(&sql, [], |r| r.get(0)) {
            Ok(rows) => tables.push(TableInfo {
                name,
                rows,
                error: None,
            }),
            Err(e) => tables.push(TableInfo {
                name,
                rows: 0,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(tables)
}

fn postgres_tables_with_counts(pool: &sqlx::PgPool) -> Result<Vec<TableInfo>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let mut tables = runtime.block_on(async {
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT table_name
             FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_type = 'BASE TABLE'
             ORDER BY table_name ASC",
        )
        .fetch_all(pool)
        .await?;

        let mut join_set = tokio::task::JoinSet::new();
        for name in names {
            let pool = pool.clone();
            join_set.spawn(async move {
                let quoted = quote_ident(&name);
                let sql = format!("SELECT COUNT(*)::BIGINT FROM {quoted}");
                match sqlx::query_scalar::<_, i64>(&sql).fetch_one(&pool).await {
                    Ok(rows) => TableInfo {
                        name,
                        rows,
                        error: None,
                    },
                    Err(e) => TableInfo {
                        name,
                        rows: 0,
                        error: Some(e.to_string()),
                    },
                }
            });
        }

        let mut tables = Vec::new();
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(info) => tables.push(info),
                Err(e) => tables.push(TableInfo {
                    name: "<join_error>".to_string(),
                    rows: 0,
                    error: Some(e.to_string()),
                }),
            }
        }
        Ok::<Vec<TableInfo>, sqlx::Error>(tables)
    })?;
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tables)
}

fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}
