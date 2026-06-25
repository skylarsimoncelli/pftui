//! report_archive — L3 ledger of every generated report (the Reporting Loop).
//!
//! Each `/pftui-report` build writes its full rendered markdown here, mode-tagged
//! (`public`/`private`). The NEXT build reads the most recent prior report so it can
//! compare price action over the period since and hold the desk accountable to what it
//! previously said. This is the first time the reporting line of operation feeds back
//! into the mechanical/DB layer — see docs/DATA-ARCHITECTURE.md (L3 "Reporting Loop").
//!
//! Append-only, with ONE sanctioned exception: a same-day rebuild for the same
//! `(report_date, mode)` deletes the prior row before inserting so re-runs don't
//! duplicate. No historical row is ever rewritten.

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct ReportArchiveRecord {
    pub report_date: String,
    pub mode: String,
    pub title: Option<String>,
    pub content: String,
    pub stance_json: Option<String>,
    pub created_at: String,
}

impl ReportArchiveRecord {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            report_date: row.get(0)?,
            mode: row.get(1)?,
            title: row.get(2)?,
            content: row.get(3)?,
            stance_json: row.get(4)?,
            created_at: row.get(5)?,
        })
    }
}

const SELECT_COLS: &str = "report_date, mode, title, content, stance_json, created_at";

/// Insert a report, replacing any existing row for the same (report_date, mode) so a
/// same-day rebuild is idempotent. Returns the new rowid.
pub fn insert_report(
    conn: &Connection,
    report_date: &str,
    mode: &str,
    title: Option<&str>,
    content: &str,
    stance_json: Option<&str>,
    created_at: &str,
) -> Result<i64> {
    conn.execute(
        "DELETE FROM report_archive WHERE report_date = ?1 AND mode = ?2",
        params![report_date, mode],
    )?;
    conn.execute(
        "INSERT INTO report_archive (report_date, mode, title, content, stance_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![report_date, mode, title, content, stance_json, created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The most recent archived report for `mode` strictly BEFORE `before_date` — the prior
/// report the new run reflects against. `mode = None` matches any mode.
pub fn latest_before(
    conn: &Connection,
    mode: Option<&str>,
    before_date: &str,
) -> Result<Option<ReportArchiveRecord>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM report_archive
         WHERE report_date < ?1 {mode_clause}
         ORDER BY report_date DESC, created_at DESC LIMIT 1",
        mode_clause = if mode.is_some() { "AND mode = ?2" } else { "" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = match mode {
        Some(m) => stmt.query_map(params![before_date, m], ReportArchiveRecord::from_row)?,
        None => stmt.query_map(params![before_date], ReportArchiveRecord::from_row)?,
    };
    Ok(rows.next().transpose()?)
}

/// The most recent `limit` archived reports for `mode` (newest first). `mode = None`
/// matches any mode.
pub fn latest_reports(
    conn: &Connection,
    mode: Option<&str>,
    limit: i64,
) -> Result<Vec<ReportArchiveRecord>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM report_archive
         {where_clause}
         ORDER BY report_date DESC, created_at DESC LIMIT ?{n}",
        where_clause = if mode.is_some() { "WHERE mode = ?1" } else { "" },
        n = if mode.is_some() { 2 } else { 1 }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = match mode {
        Some(m) => stmt.query_map(params![m, limit], ReportArchiveRecord::from_row)?,
        None => stmt.query_map(params![limit], ReportArchiveRecord::from_row)?,
    };
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn insert_report_backend(
    backend: &BackendConnection,
    report_date: &str,
    mode: &str,
    title: Option<&str>,
    content: &str,
    stance_json: Option<&str>,
    created_at: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| insert_report(conn, report_date, mode, title, content, stance_json, created_at),
        |pool| {
            insert_report_postgres(
                pool,
                report_date,
                mode,
                title,
                content,
                stance_json,
                created_at,
            )
        },
    )
}

#[allow(dead_code)] // backend-dispatch variant; the report loader uses the raw-conn path
pub fn latest_before_backend(
    backend: &BackendConnection,
    mode: Option<&str>,
    before_date: &str,
) -> Result<Option<ReportArchiveRecord>> {
    query::dispatch(
        backend,
        |conn| latest_before(conn, mode, before_date),
        |pool| latest_before_postgres(pool, mode, before_date),
    )
}

#[allow(dead_code)]
pub fn latest_reports_backend(
    backend: &BackendConnection,
    mode: Option<&str>,
    limit: i64,
) -> Result<Vec<ReportArchiveRecord>> {
    query::dispatch(
        backend,
        |conn| latest_reports(conn, mode, limit),
        |pool| latest_reports_postgres(pool, mode, limit),
    )
}

type ArchiveRow = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
);

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS report_archive (
                id BIGSERIAL PRIMARY KEY,
                report_date TEXT NOT NULL,
                mode TEXT NOT NULL,
                title TEXT,
                content TEXT NOT NULL,
                stance_json TEXT,
                created_at TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_report_archive_date
             ON report_archive(report_date DESC, mode)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_report_postgres(
    pool: &PgPool,
    report_date: &str,
    mode: &str,
    title: Option<&str>,
    content: &str,
    stance_json: Option<&str>,
    created_at: &str,
) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM report_archive WHERE report_date = $1 AND mode = $2")
            .bind(report_date)
            .bind(mode)
            .execute(pool)
            .await?;
        sqlx::query_scalar(
            "INSERT INTO report_archive (report_date, mode, title, content, stance_json, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(report_date)
        .bind(mode)
        .bind(title)
        .bind(content)
        .bind(stance_json)
        .bind(created_at)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn map_archive_row(row: ArchiveRow) -> ReportArchiveRecord {
    ReportArchiveRecord {
        report_date: row.0,
        mode: row.1,
        title: row.2,
        content: row.3,
        stance_json: row.4,
        created_at: row.5,
    }
}

#[allow(dead_code)] // reached only via latest_before_backend (Postgres path)
fn latest_before_postgres(
    pool: &PgPool,
    mode: Option<&str>,
    before_date: &str,
) -> Result<Option<ReportArchiveRecord>> {
    ensure_table_postgres(pool)?;
    let row: Option<ArchiveRow> = crate::db::pg_runtime::block_on(async {
        match mode {
            Some(m) => {
                sqlx::query_as(
                    "SELECT report_date, mode, title, content, stance_json, created_at
                     FROM report_archive WHERE report_date < $1 AND mode = $2
                     ORDER BY report_date DESC, created_at DESC LIMIT 1",
                )
                .bind(before_date)
                .bind(m)
                .fetch_optional(pool)
                .await
            }
            None => {
                sqlx::query_as(
                    "SELECT report_date, mode, title, content, stance_json, created_at
                     FROM report_archive WHERE report_date < $1
                     ORDER BY report_date DESC, created_at DESC LIMIT 1",
                )
                .bind(before_date)
                .fetch_optional(pool)
                .await
            }
        }
    })?;
    Ok(row.map(map_archive_row))
}

fn latest_reports_postgres(
    pool: &PgPool,
    mode: Option<&str>,
    limit: i64,
) -> Result<Vec<ReportArchiveRecord>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<ArchiveRow> = crate::db::pg_runtime::block_on(async {
        match mode {
            Some(m) => {
                sqlx::query_as(
                    "SELECT report_date, mode, title, content, stance_json, created_at
                     FROM report_archive WHERE mode = $1
                     ORDER BY report_date DESC, created_at DESC LIMIT $2",
                )
                .bind(m)
                .bind(limit)
                .fetch_all(pool)
                .await
            }
            None => {
                sqlx::query_as(
                    "SELECT report_date, mode, title, content, stance_json, created_at
                     FROM report_archive
                     ORDER BY report_date DESC, created_at DESC LIMIT $1",
                )
                .bind(limit)
                .fetch_all(pool)
                .await
            }
        }
    })?;
    Ok(rows.into_iter().map(map_archive_row).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(conn: &Connection) {
        insert_report(conn, "2026-06-10", "private", Some("A"), "body-a", None, "2026-06-10T10:00:00Z").unwrap();
        insert_report(conn, "2026-06-18", "private", Some("B"), "body-b", Some("{\"BTC\":1}"), "2026-06-18T10:00:00Z").unwrap();
        insert_report(conn, "2026-06-18", "public", Some("Bpub"), "body-bpub", None, "2026-06-18T10:05:00Z").unwrap();
    }

    #[test]
    fn latest_before_respects_mode_and_date() {
        let conn = crate::db::open_in_memory();
        seed(&conn);
        let prior = latest_before(&conn, Some("private"), "2026-06-25").unwrap().unwrap();
        assert_eq!(prior.report_date, "2026-06-18");
        assert_eq!(prior.mode, "private");
        assert_eq!(prior.stance_json.as_deref(), Some("{\"BTC\":1}"));

        let prior2 = latest_before(&conn, Some("private"), "2026-06-18").unwrap().unwrap();
        assert_eq!(prior2.report_date, "2026-06-10");
    }

    #[test]
    fn same_day_rebuild_is_idempotent() {
        let conn = crate::db::open_in_memory();
        seed(&conn);
        // Rebuild the same (date, mode) — should replace, not duplicate.
        insert_report(&conn, "2026-06-18", "private", Some("B2"), "body-b2", None, "2026-06-18T12:00:00Z").unwrap();
        let rows = latest_reports(&conn, Some("private"), 10).unwrap();
        let on_18: Vec<_> = rows.iter().filter(|r| r.report_date == "2026-06-18").collect();
        assert_eq!(on_18.len(), 1);
        assert_eq!(on_18[0].content, "body-b2");
    }

    #[test]
    fn latest_reports_filters_mode() {
        let conn = crate::db::open_in_memory();
        seed(&conn);
        assert_eq!(latest_reports(&conn, Some("public"), 10).unwrap().len(), 1);
        assert_eq!(latest_reports(&conn, None, 10).unwrap().len(), 3);
    }
}
