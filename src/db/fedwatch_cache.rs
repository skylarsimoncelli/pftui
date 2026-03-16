use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::data::fedwatch::FedWatchSnapshot;
use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct FedWatchCacheEntry {
    pub id: i64,
    pub source_label: String,
    pub source_url: String,
    pub no_change_pct: f64,
    pub verified: bool,
    pub warning: Option<String>,
    pub fetched_at: String,
    pub snapshot: FedWatchSnapshot,
}

impl FedWatchCacheEntry {
    pub fn from_snapshot(
        snapshot: FedWatchSnapshot,
        source_label: String,
        verified: bool,
        warning: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            source_url: snapshot.source_url.clone(),
            no_change_pct: snapshot.summary.no_change_pct,
            fetched_at: snapshot.fetched_at.clone(),
            source_label,
            verified,
            warning,
            snapshot,
        }
    }
}

pub fn insert_snapshot(conn: &Connection, entry: &FedWatchCacheEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO fedwatch_cache
         (source_label, source_url, no_change_pct, verified, warning, snapshot_json, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            entry.source_label,
            entry.source_url,
            entry.no_change_pct,
            if entry.verified { 1 } else { 0 },
            entry.warning,
            serde_json::to_string(&entry.snapshot)?,
            entry.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn insert_snapshot_backend(
    backend: &BackendConnection,
    entry: &FedWatchCacheEntry,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| insert_snapshot(conn, entry),
        |pool| insert_snapshot_postgres(pool, entry),
    )
}

pub fn get_latest_snapshot(conn: &Connection) -> Result<Option<FedWatchCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_label, source_url, no_change_pct, verified, warning, snapshot_json, fetched_at
         FROM fedwatch_cache
         ORDER BY fetched_at DESC, id DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query([])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    Ok(Some(FedWatchCacheEntry {
        id: row.get(0)?,
        source_label: row.get(1)?,
        source_url: row.get(2)?,
        no_change_pct: row.get(3)?,
        verified: row.get::<_, i64>(4)? != 0,
        warning: row.get(5)?,
        snapshot: serde_json::from_str(&row.get::<_, String>(6)?)?,
        fetched_at: row.get(7)?,
    }))
}

pub fn get_latest_snapshot_backend(
    backend: &BackendConnection,
) -> Result<Option<FedWatchCacheEntry>> {
    query::dispatch(backend, get_latest_snapshot, get_latest_snapshot_postgres)
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS fedwatch_cache (
                id BIGSERIAL PRIMARY KEY,
                source_label TEXT NOT NULL,
                source_url TEXT NOT NULL,
                no_change_pct DOUBLE PRECISION NOT NULL,
                verified BOOLEAN NOT NULL DEFAULT TRUE,
                warning TEXT,
                snapshot_json TEXT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_fedwatch_cache_fetched_at ON fedwatch_cache(fetched_at DESC)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_snapshot_postgres(pool: &PgPool, entry: &FedWatchCacheEntry) -> Result<()> {
    ensure_table_postgres(pool)?;
    let snapshot_json = serde_json::to_string(&entry.snapshot)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO fedwatch_cache
             (source_label, source_url, no_change_pct, verified, warning, snapshot_json, fetched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz)",
        )
        .bind(&entry.source_label)
        .bind(&entry.source_url)
        .bind(entry.no_change_pct)
        .bind(entry.verified)
        .bind(&entry.warning)
        .bind(snapshot_json)
        .bind(&entry.fetched_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_latest_snapshot_postgres(pool: &PgPool) -> Result<Option<FedWatchCacheEntry>> {
    ensure_table_postgres(pool)?;
    let row = crate::db::pg_runtime::block_on(async {
        sqlx::query_as::<_, (i64, String, String, f64, bool, Option<String>, String, String)>(
            "SELECT id, source_label, source_url, no_change_pct, verified, warning, snapshot_json, fetched_at::text
             FROM fedwatch_cache
             ORDER BY fetched_at DESC, id DESC
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
    })?;

    row.map(
        |(
            id,
            source_label,
            source_url,
            no_change_pct,
            verified,
            warning,
            snapshot_json,
            fetched_at,
        )| {
            Ok(FedWatchCacheEntry {
                id,
                source_label,
                source_url,
                no_change_pct,
                verified,
                warning,
                snapshot: serde_json::from_str(&snapshot_json)?,
                fetched_at,
            })
        },
    )
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::fedwatch::{MeetingInfo, SummaryProbabilities};

    #[test]
    fn inserts_and_reads_latest_snapshot() {
        let conn = crate::db::open_in_memory();
        let entry = FedWatchCacheEntry::from_snapshot(
            FedWatchSnapshot {
                source_url: "https://example.com".to_string(),
                fetched_at: "2026-03-16T12:00:00Z".to_string(),
                meetings: vec!["18 Mar26".to_string()],
                meeting_info: MeetingInfo {
                    meeting_date: "18 Mar 2026".to_string(),
                    contract: "ZQH6".to_string(),
                    expires: "31 Mar 2026".to_string(),
                    mid_price: 96.25,
                    prior_volume: 100,
                    prior_open_interest: 200,
                },
                summary: SummaryProbabilities {
                    ease_pct: 1.0,
                    no_change_pct: 94.0,
                    hike_pct: 5.0,
                },
                target_probabilities: vec![],
            },
            "CME FedWatch".to_string(),
            true,
            None,
        );

        insert_snapshot(&conn, &entry).unwrap();
        let loaded = get_latest_snapshot(&conn)
            .unwrap()
            .expect("entry should exist");
        assert_eq!(loaded.source_label, "CME FedWatch");
        assert!(loaded.verified);
        assert_eq!(loaded.snapshot.summary.no_change_pct, 94.0);
    }
}
