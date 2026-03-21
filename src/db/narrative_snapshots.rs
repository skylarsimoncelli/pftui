use anyhow::Result;
use rusqlite::{params, Connection, Row};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct NarrativeSnapshotRecord {
    pub report_json: String,
}

#[allow(dead_code)]
impl NarrativeSnapshotRecord {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            report_json: row.get(0)?,
        })
    }
}

pub fn insert_snapshot(conn: &Connection, recorded_at: &str, report_json: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO narrative_snapshots (recorded_at, report_json)
         VALUES (?1, ?2)",
        params![recorded_at, report_json],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(dead_code)]
pub fn latest_snapshot(conn: &Connection) -> Result<Option<NarrativeSnapshotRecord>> {
    let mut stmt = conn.prepare(
        "SELECT report_json
         FROM narrative_snapshots
         ORDER BY recorded_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], NarrativeSnapshotRecord::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn insert_snapshot_backend(
    backend: &BackendConnection,
    recorded_at: &str,
    report_json: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| insert_snapshot(conn, recorded_at, report_json),
        |pool| insert_snapshot_postgres(pool, recorded_at, report_json),
    )
}

#[allow(dead_code)]
pub fn latest_snapshot_backend(
    backend: &BackendConnection,
) -> Result<Option<NarrativeSnapshotRecord>> {
    query::dispatch(backend, latest_snapshot, latest_snapshot_postgres)
}

#[allow(dead_code)]
type SnapshotRow = (String,);

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS narrative_snapshots (
                id BIGSERIAL PRIMARY KEY,
                recorded_at TIMESTAMPTZ NOT NULL,
                report_json TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_narrative_snapshots_recorded_at
             ON narrative_snapshots(recorded_at DESC)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_snapshot_postgres(pool: &PgPool, recorded_at: &str, report_json: &str) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO narrative_snapshots (recorded_at, report_json)
             VALUES ($1::timestamptz, $2)
             RETURNING id",
        )
        .bind(recorded_at)
        .bind(report_json)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

#[allow(dead_code)]
fn latest_snapshot_postgres(pool: &PgPool) -> Result<Option<NarrativeSnapshotRecord>> {
    ensure_table_postgres(pool)?;
    let row: Option<SnapshotRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT report_json
             FROM narrative_snapshots
             ORDER BY recorded_at DESC
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|row| NarrativeSnapshotRecord { report_json: row.0 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_latest_snapshot() {
        let conn = crate::db::open_in_memory();
        insert_snapshot(&conn, "2026-03-21T10:00:00Z", "{\"a\":1}").unwrap();
        insert_snapshot(&conn, "2026-03-21T11:00:00Z", "{\"b\":2}").unwrap();

        let latest = latest_snapshot(&conn).unwrap().unwrap();
        assert_eq!(latest.report_json, "{\"b\":2}");
    }
}
