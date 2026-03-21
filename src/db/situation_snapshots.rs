use anyhow::Result;
use rusqlite::{params, Connection, Row};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct SituationSnapshotRecord {
    pub snapshot_json: String,
}

impl SituationSnapshotRecord {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            snapshot_json: row.get(0)?,
        })
    }
}

pub fn insert_snapshot(conn: &Connection, recorded_at: &str, snapshot_json: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO situation_snapshots (recorded_at, snapshot_json)
         VALUES (?1, ?2)",
        params![recorded_at, snapshot_json],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn latest_snapshot(conn: &Connection) -> Result<Option<SituationSnapshotRecord>> {
    let mut stmt = conn.prepare(
        "SELECT snapshot_json
         FROM situation_snapshots
         ORDER BY recorded_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], SituationSnapshotRecord::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn latest_snapshot_before(
    conn: &Connection,
    cutoff: &str,
) -> Result<Option<SituationSnapshotRecord>> {
    let mut stmt = conn.prepare(
        "SELECT snapshot_json
         FROM situation_snapshots
         WHERE recorded_at <= ?1
         ORDER BY recorded_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![cutoff], SituationSnapshotRecord::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn insert_snapshot_backend(
    backend: &BackendConnection,
    recorded_at: &str,
    snapshot_json: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| insert_snapshot(conn, recorded_at, snapshot_json),
        |pool| insert_snapshot_postgres(pool, recorded_at, snapshot_json),
    )
}

pub fn latest_snapshot_backend(
    backend: &BackendConnection,
) -> Result<Option<SituationSnapshotRecord>> {
    query::dispatch(backend, latest_snapshot, latest_snapshot_postgres)
}

pub fn latest_snapshot_before_backend(
    backend: &BackendConnection,
    cutoff: &str,
) -> Result<Option<SituationSnapshotRecord>> {
    query::dispatch(
        backend,
        |conn| latest_snapshot_before(conn, cutoff),
        |pool| latest_snapshot_before_postgres(pool, cutoff),
    )
}

type SnapshotRow = (String,);

fn to_snapshot(row: SnapshotRow) -> SituationSnapshotRecord {
    SituationSnapshotRecord {
        snapshot_json: row.0,
    }
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS situation_snapshots (
                id BIGSERIAL PRIMARY KEY,
                recorded_at TIMESTAMPTZ NOT NULL,
                snapshot_json TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_situation_snapshots_recorded_at
             ON situation_snapshots(recorded_at DESC)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_snapshot_postgres(pool: &PgPool, recorded_at: &str, snapshot_json: &str) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO situation_snapshots (recorded_at, snapshot_json)
             VALUES ($1::timestamptz, $2)
             RETURNING id",
        )
        .bind(recorded_at)
        .bind(snapshot_json)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn latest_snapshot_postgres(pool: &PgPool) -> Result<Option<SituationSnapshotRecord>> {
    ensure_table_postgres(pool)?;
    let row: Option<SnapshotRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT snapshot_json
             FROM situation_snapshots
             ORDER BY recorded_at DESC
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_snapshot))
}

fn latest_snapshot_before_postgres(
    pool: &PgPool,
    cutoff: &str,
) -> Result<Option<SituationSnapshotRecord>> {
    ensure_table_postgres(pool)?;
    let row: Option<SnapshotRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT snapshot_json
             FROM situation_snapshots
             WHERE recorded_at <= $1::timestamptz
             ORDER BY recorded_at DESC
             LIMIT 1",
        )
        .bind(cutoff)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_latest_snapshot() {
        let conn = crate::db::open_in_memory();
        insert_snapshot(&conn, "2026-03-20T10:00:00Z", "{\"a\":1}").unwrap();
        insert_snapshot(&conn, "2026-03-20T11:00:00Z", "{\"b\":2}").unwrap();

        let latest = latest_snapshot(&conn).unwrap().unwrap();
        assert_eq!(latest.snapshot_json, "{\"b\":2}");
    }

    #[test]
    fn reads_latest_snapshot_before_cutoff() {
        let conn = crate::db::open_in_memory();
        insert_snapshot(&conn, "2026-03-20T10:00:00Z", "{\"a\":1}").unwrap();
        insert_snapshot(&conn, "2026-03-20T12:00:00Z", "{\"b\":2}").unwrap();

        let before = latest_snapshot_before(&conn, "2026-03-20T11:00:00Z")
            .unwrap()
            .unwrap();
        assert_eq!(before.snapshot_json, "{\"a\":1}");
    }
}
