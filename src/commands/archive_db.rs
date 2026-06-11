//! `pftui system archive-db` — back up the whole database (VACUUM INTO) or
//! export one table as JSON, into ~/pftui-archives/ by default (R3).
//!
//! Archives always land OUTSIDE the repo. Prints destination path + size;
//! never prints row contents.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::db::archive;
use crate::db::backend::BackendConnection;

pub fn run(
    backend: &BackendConnection,
    out: Option<&str>,
    table: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = match backend {
        BackendConnection::Sqlite { conn } => conn,
        BackendConnection::Postgres { .. } => {
            anyhow::bail!(
                "system archive-db backs up the local SQLite database; \
                 use pg_dump for the postgres backend"
            );
        }
    };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");

    let (mode, dest, rows) = match table {
        Some(table) => {
            let dest = out
                .map(PathBuf::from)
                .unwrap_or_else(|| archive::archive_dir().join(format!("{table}-{stamp}.json")));
            let rows = archive::export_table_json(conn, table, &dest)?;
            ("table-json".to_string(), dest, Some(rows))
        }
        None => {
            let dest = out
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    archive::archive_dir().join(format!("pftui-backup-{stamp}.db"))
                });
            archive::backup_database(conn, &dest)?;
            ("full-db".to_string(), dest, None)
        }
    };

    let size_bytes = std::fs::metadata(&dest)
        .with_context(|| format!("statting archive {}", dest.display()))?
        .len();

    if json_output {
        let doc = serde_json::json!({
            "mode": mode,
            "path": dest.display().to_string(),
            "size_bytes": size_bytes,
            "rows_exported": rows,
        });
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        match rows {
            Some(n) => println!(
                "Archived {n} rows to {} ({})",
                dest.display(),
                human_size(size_bytes)
            ),
            None => println!(
                "Database backed up to {} ({})",
                dest.display(),
                human_size(size_bytes)
            ),
        }
    }
    Ok(())
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pftui-archive-cmd-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn full_backup_and_table_export_to_explicit_out_paths() {
        let dir = temp_dir("run");
        let db_path = dir.join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, label TEXT);
             INSERT INTO demo (label) VALUES ('synthetic');",
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };

        let db_out = dir.join("backup.db");
        run(&backend, Some(db_out.to_str().unwrap()), None, true).unwrap();
        assert!(db_out.exists());
        let copy = Connection::open(&db_out).unwrap();
        let n: i64 = copy
            .query_row("SELECT COUNT(*) FROM demo", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);

        let json_out = dir.join("demo.json");
        run(
            &backend,
            Some(json_out.to_str().unwrap()),
            Some("demo"),
            true,
        )
        .unwrap();
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
        assert_eq!(doc["row_count"], 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
