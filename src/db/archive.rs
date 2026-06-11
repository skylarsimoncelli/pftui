//! Archival utilities for database backups and table exports (R3).
//!
//! Everything written here lands OUTSIDE the repo, in `~/pftui-archives/`
//! by default (override with `PFTUI_ARCHIVE_DIR` — used by tests). The live
//! database contains real personal financial data; archives must never be
//! committed or printed.
//!
//! Two consumers:
//!   - `pftui system archive-db` — operator-visible full-DB backup
//!     (`VACUUM INTO`) and per-table JSON export.
//!   - `archive_and_drop_dead_tables` — the R3 cull migration. SAFEST
//!     design chosen: the export runs inside the migration path itself,
//!     immediately before the drop, so a drop can never outrun its archive.
//!     If the export fails for a non-empty table, the drop is SKIPPED and a
//!     warning is printed — the table survives until the next run.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::types::ValueRef;
use rusqlite::Connection;

/// Tables dropped by the R3 cull. Each is archived (if non-empty) before
/// the drop. All four lost their CREATE TABLE statements in the same
/// change, so fresh databases never see them and this list is a no-op.
///
///   prediction_cache        — 0 rows, superseded by predictions_cache
///   conviction_durability   — no code writer/reader (agent raw SQL)
///   thesis_citations        — no code writer/reader (agent raw SQL)
///   narrative_money_history — write-only ingestion, no reader; the live
///                             narrative-divergence report computes from
///                             news_cache + predictions directly
pub const DEAD_TABLES: &[&str] = &[
    "prediction_cache",
    "conviction_durability",
    "thesis_citations",
    "narrative_money_history",
];

/// Archive directory: `$PFTUI_ARCHIVE_DIR` if set (tests), else
/// `~/pftui-archives`. Always outside the repo.
pub fn archive_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PFTUI_ARCHIVE_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pftui-archives")
}

fn valid_ident(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

/// Export every row of `table` as a JSON document at `path`:
/// `{ "table": ..., "exported_at": ..., "row_count": N, "rows": [...] }`.
/// Returns the number of rows exported. The parent directory is created.
pub fn export_table_json(conn: &Connection, table: &str, path: &Path) -> Result<usize> {
    if !valid_ident(table) {
        bail!("invalid table name: {table:?}");
    }
    if !table_exists(conn, table)? {
        bail!("table {table} does not exist");
    }
    let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\""))?;
    let column_names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let mut rows_json: Vec<serde_json::Value> = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut obj = serde_json::Map::with_capacity(column_names.len());
        for (i, name) in column_names.iter().enumerate() {
            let value = match row.get_ref(i)? {
                ValueRef::Null => serde_json::Value::Null,
                ValueRef::Integer(v) => serde_json::Value::from(v),
                ValueRef::Real(v) => serde_json::Value::from(v),
                ValueRef::Text(t) => {
                    serde_json::Value::from(String::from_utf8_lossy(t).into_owned())
                }
                ValueRef::Blob(b) => {
                    // No personal table stores blobs today; hex keeps it lossless.
                    serde_json::Value::from(
                        b.iter().map(|x| format!("{x:02x}")).collect::<String>(),
                    )
                }
            };
            obj.insert(name.clone(), value);
        }
        rows_json.push(serde_json::Value::Object(obj));
    }
    let doc = serde_json::json!({
        "table": table,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "row_count": rows_json.len(),
        "rows": rows_json,
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating archive dir {}", parent.display()))?;
    }
    let count = rows_json.len();
    std::fs::write(path, serde_json::to_string_pretty(&doc)?)
        .with_context(|| format!("writing table archive {}", path.display()))?;
    Ok(count)
}

/// Full-database backup via `VACUUM INTO` (atomic, consistent, compacts).
/// Fails if `dest` already exists (SQLite refuses to overwrite).
pub fn backup_database(conn: &Connection, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating archive dir {}", parent.display()))?;
    }
    let dest_str = dest
        .to_str()
        .with_context(|| format!("non-UTF8 backup path {}", dest.display()))?;
    conn.execute("VACUUM INTO ?1", [dest_str])
        .with_context(|| format!("VACUUM INTO {}", dest.display()))?;
    Ok(())
}

/// R3 cull migration: archive-then-drop each `DEAD_TABLES` entry.
///
/// Safety properties:
///   - Fresh DBs: tables were removed from the schema, so nothing exists
///     and this is a no-op.
///   - Non-empty table: exported to `<archive_dir>/<table>-pre-drop-<date>.json`
///     BEFORE the drop. If the export fails, the drop is skipped (warning
///     on stderr) and retried on the next startup.
///   - Empty table: dropped directly, nothing to lose.
///   - Idempotent: after the drop the table no longer exists.
pub fn archive_and_drop_dead_tables(conn: &Connection) -> Result<()> {
    for table in DEAD_TABLES {
        if !table_exists(conn, table)? {
            continue;
        }
        let rows: i64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |r| {
                r.get(0)
            })?;
        if rows > 0 {
            let date = chrono::Utc::now().format("%Y%m%d");
            let path = archive_dir().join(format!("{table}-pre-drop-{date}.json"));
            match export_table_json(conn, table, &path) {
                Ok(n) => {
                    eprintln!(
                        "pftui: archived {n} rows of dead table {table} to {} before drop",
                        path.display()
                    );
                }
                Err(err) => {
                    eprintln!(
                        "pftui: WARNING: could not archive dead table {table} ({err}); \
                         leaving it in place — will retry on next startup"
                    );
                    continue;
                }
            }
        }
        conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\""))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pftui-archive-test-{tag}-{}-{}",
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
    fn export_table_json_roundtrips_values() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sample (id INTEGER PRIMARY KEY, name TEXT, score REAL, blob_col BLOB);
             INSERT INTO sample (name, score, blob_col) VALUES ('alpha', 1.5, X'00FF');
             INSERT INTO sample (name, score, blob_col) VALUES (NULL, NULL, NULL);",
        )
        .unwrap();
        let dir = temp_dir("export");
        let path = dir.join("sample.json");
        let n = export_table_json(&conn, "sample", &path).unwrap();
        assert_eq!(n, 2);
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(doc["table"], "sample");
        assert_eq!(doc["row_count"], 2);
        assert_eq!(doc["rows"][0]["name"], "alpha");
        assert_eq!(doc["rows"][0]["score"], 1.5);
        assert_eq!(doc["rows"][0]["blob_col"], "00ff");
        assert!(doc["rows"][1]["name"].is_null());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_rejects_bad_identifiers_and_missing_tables() {
        let conn = Connection::open_in_memory().unwrap();
        let dir = temp_dir("badident");
        assert!(export_table_json(&conn, "no; drop", &dir.join("x.json")).is_err());
        assert!(export_table_json(&conn, "missing_table", &dir.join("y.json")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Single test covering both cull paths (fresh DB no-op + legacy tables
    /// archived-then-dropped) so the PFTUI_ARCHIVE_DIR env var is only
    /// mutated in one place.
    #[test]
    fn cull_is_safe_on_fresh_db_and_archives_legacy_tables() {
        let dir = temp_dir("cull");
        std::env::set_var("PFTUI_ARCHIVE_DIR", &dir);

        // Fresh DB: none of the dead tables exist — must be a clean no-op.
        let fresh = Connection::open_in_memory().unwrap();
        archive_and_drop_dead_tables(&fresh).unwrap();

        // Legacy DB: recreate the old table shapes with synthetic rows.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE prediction_cache (market_id TEXT PRIMARY KEY, question TEXT NOT NULL);
             CREATE TABLE conviction_durability (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 prediction_id INTEGER NOT NULL,
                 window_days INTEGER NOT NULL,
                 conviction_drift REAL NOT NULL DEFAULT 0.0,
                 note TEXT,
                 recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE thesis_citations (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 thesis_id INTEGER NOT NULL,
                 source_type TEXT NOT NULL,
                 source_id INTEGER,
                 citation_text TEXT,
                 created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO conviction_durability (prediction_id, window_days, note)
                 VALUES (1, 30, 'synthetic');
             INSERT INTO thesis_citations (thesis_id, source_type, citation_text)
                 VALUES (1, 'lesson', 'synthetic citation');",
        )
        .unwrap();
        archive_and_drop_dead_tables(&conn).unwrap();

        // All dead tables gone (empty prediction_cache dropped without export).
        for t in DEAD_TABLES {
            assert!(!table_exists(&conn, t).unwrap(), "{t} should be dropped");
        }
        // Non-empty tables archived first.
        let date = chrono::Utc::now().format("%Y%m%d");
        for t in ["conviction_durability", "thesis_citations"] {
            let path = dir.join(format!("{t}-pre-drop-{date}.json"));
            assert!(path.exists(), "archive for {t} missing");
            let doc: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(doc["row_count"], 1);
        }
        // Empty table dropped without an archive file.
        assert!(!dir
            .join(format!("prediction_cache-pre-drop-{date}.json"))
            .exists());

        // Idempotent on re-run.
        archive_and_drop_dead_tables(&conn).unwrap();

        std::env::remove_var("PFTUI_ARCHIVE_DIR");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backup_database_produces_openable_copy() {
        let dir = temp_dir("backup");
        let src_path = dir.join("src.db");
        let conn = Connection::open(&src_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (42);",
        )
        .unwrap();
        let dest = dir.join("backup.db");
        backup_database(&conn, &dest).unwrap();
        let copy = Connection::open(&dest).unwrap();
        let x: i64 = copy.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 42);
        std::fs::remove_dir_all(&dir).ok();
    }
}
