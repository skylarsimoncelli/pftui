//! `lesson_citations` table — records each time a `prediction_lessons` row is
//! referenced from another table (most commonly `user_predictions` via the
//! `lessons_applied` JSON column or a future direct foreign-key). The table
//! was originally created during a live-DB enrichment session; this module
//! adds an empty schema-side definition so any fresh DB created by `pftui`
//! has the table available for the lesson half-life curation routine.
//!
//! Schema mirrors the live-DB shape:
//!   (lesson_id INTEGER NOT NULL,
//!    cited_in_table TEXT NOT NULL,
//!    cited_in_id INTEGER NOT NULL,
//!    cited_at TEXT NOT NULL DEFAULT (datetime('now')),
//!    citation_count INTEGER NOT NULL DEFAULT 1,
//!    PRIMARY KEY (lesson_id, cited_in_table, cited_in_id))

use anyhow::Result;
use rusqlite::{params, Connection};

/// Create the lesson_citations table if it does not exist. Safe to call
/// on every startup.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS lesson_citations (
            lesson_id INTEGER NOT NULL,
            cited_in_table TEXT NOT NULL,
            cited_in_id INTEGER NOT NULL,
            cited_at TEXT NOT NULL DEFAULT (datetime('now')),
            citation_count INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (lesson_id, cited_in_table, cited_in_id)
        );
        CREATE INDEX IF NOT EXISTS idx_lesson_citations_lesson
            ON lesson_citations(lesson_id);
        CREATE INDEX IF NOT EXISTS idx_lesson_citations_cited_at
            ON lesson_citations(cited_at);",
    )?;
    Ok(())
}

/// Record (or bump) a citation of `lesson_id` from `cited_in_table`/`cited_in_id`.
/// Increments `citation_count` on conflict; updates `cited_at` to now.
#[allow(dead_code)]
pub fn record_citation(
    conn: &Connection,
    lesson_id: i64,
    cited_in_table: &str,
    cited_in_id: i64,
) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO lesson_citations (lesson_id, cited_in_table, cited_in_id)
         VALUES (?, ?, ?)
         ON CONFLICT(lesson_id, cited_in_table, cited_in_id) DO UPDATE SET
             citation_count = citation_count + 1,
             cited_at = datetime('now')",
        params![lesson_id, cited_in_table, cited_in_id],
    )?;
    Ok(())
}

/// Return (lesson_id, last_cited_at) for every lesson that has at least one
/// citation. Used to repopulate the denormalized `prediction_lessons.last_cited_at`
/// column.
pub fn latest_citation_per_lesson(conn: &Connection) -> Result<Vec<(i64, String)>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT lesson_id, MAX(cited_at) FROM lesson_citations GROUP BY lesson_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn ensure_table_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        ensure_table(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='lesson_citations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn record_citation_bumps_count_and_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        record_citation(&conn, 42, "user_predictions", 100).unwrap();
        record_citation(&conn, 42, "user_predictions", 100).unwrap();
        let (count,): (i64,) = conn
            .query_row(
                "SELECT citation_count FROM lesson_citations WHERE lesson_id = 42",
                [],
                |row| Ok((row.get(0)?,)),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn latest_citation_per_lesson_returns_max() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn.execute(
            "INSERT INTO lesson_citations (lesson_id, cited_in_table, cited_in_id, cited_at)
             VALUES (1, 'user_predictions', 10, '2026-01-01'),
                    (1, 'user_predictions', 11, '2026-03-01'),
                    (2, 'user_predictions', 12, '2026-02-01')",
            [],
        )
        .unwrap();
        let rows = latest_citation_per_lesson(&conn).unwrap();
        let map: std::collections::HashMap<i64, String> = rows.into_iter().collect();
        assert_eq!(map.get(&1).map(String::as_str), Some("2026-03-01"));
        assert_eq!(map.get(&2).map(String::as_str), Some("2026-02-01"));
    }
}
