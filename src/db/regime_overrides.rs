//! Manual regime override storage with auto-expiry.
//!
//! Stores a temporary override for the macro regime classification
//! that takes precedence over the automated signal-based assessment.
//! Overrides expire after a configurable duration and are checked
//! before the standard regime classification is applied.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A stored regime override entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeOverride {
    pub id: i64,
    pub regime: String,
    pub reason: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// ─── SQLite helpers ────────────────────────────────────────────────────────────

fn ensure_table_sqlite(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS regime_overrides (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            regime      TEXT NOT NULL,
            reason      TEXT NOT NULL DEFAULT '',
            expires_at  INTEGER NOT NULL,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;
    Ok(())
}

fn set_sqlite(conn: &Connection, regime: &str, reason: &str, expires_ts: i64) -> Result<()> {
    ensure_table_sqlite(conn)?;
    conn.execute("DELETE FROM regime_overrides", [])?;
    conn.execute(
        "INSERT INTO regime_overrides (regime, reason, expires_at) VALUES (?1, ?2, ?3)",
        params![regime, reason, expires_ts],
    )?;
    Ok(())
}

fn clear_sqlite(conn: &Connection) -> Result<()> {
    ensure_table_sqlite(conn)?;
    conn.execute("DELETE FROM regime_overrides", [])?;
    Ok(())
}

fn get_active_sqlite(conn: &Connection) -> Result<Option<RegimeOverride>> {
    ensure_table_sqlite(conn)?;
    let now_ts = Utc::now().timestamp();
    let result = conn.query_row(
        "SELECT id, regime, reason, expires_at, created_at FROM regime_overrides LIMIT 1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    );
    match result {
        Ok((id, regime, reason, expires_ts, created_ts)) => {
            if expires_ts <= now_ts {
                return Ok(None);
            }
            Ok(Some(RegimeOverride {
                id,
                regime,
                reason,
                expires_at: DateTime::from_timestamp(expires_ts, 0)
                    .unwrap_or_else(Utc::now),
                created_at: DateTime::from_timestamp(created_ts, 0)
                    .unwrap_or_else(Utc::now),
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// ─── Postgres helpers ─────────────────────────────────────────────────────────

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS regime_overrides (
                id          BIGSERIAL PRIMARY KEY,
                regime      TEXT NOT NULL,
                reason      TEXT NOT NULL DEFAULT '',
                expires_at  BIGINT NOT NULL,
                created_at  BIGINT NOT NULL
                    DEFAULT extract(epoch from now())::bigint
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn set_postgres(pool: &PgPool, regime: &str, reason: &str, expires_ts: i64) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM regime_overrides")
            .execute(pool)
            .await?;
        sqlx::query(
            "INSERT INTO regime_overrides (regime, reason, expires_at) VALUES ($1, $2, $3)",
        )
        .bind(regime)
        .bind(reason)
        .bind(expires_ts)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn clear_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        // Table may not exist yet; create it silently.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS regime_overrides (
                id BIGSERIAL PRIMARY KEY, regime TEXT NOT NULL,
                reason TEXT NOT NULL DEFAULT '', expires_at BIGINT NOT NULL,
                created_at BIGINT NOT NULL DEFAULT extract(epoch from now())::bigint
            )",
        )
        .execute(pool)
        .await
        .ok();
        sqlx::query("DELETE FROM regime_overrides")
            .execute(pool)
            .await
            .ok();
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_active_postgres(pool: &PgPool) -> Result<Option<RegimeOverride>> {
    ensure_table_postgres(pool)?;
    let now_ts = Utc::now().timestamp();
    let row = crate::db::pg_runtime::block_on(async {
        let row = sqlx::query_as::<_, (i64, String, String, i64, i64)>(
            "SELECT id, regime, reason, expires_at, created_at FROM regime_overrides LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        Ok::<_, sqlx::Error>(row)
    })?;
    match row {
        Some((id, regime, reason, expires_ts, created_ts)) => {
            if expires_ts <= now_ts {
                return Ok(None);
            }
            Ok(Some(RegimeOverride {
                id,
                regime,
                reason,
                expires_at: DateTime::from_timestamp(expires_ts, 0).unwrap_or_else(Utc::now),
                created_at: DateTime::from_timestamp(created_ts, 0).unwrap_or_else(Utc::now),
            }))
        }
        None => Ok(None),
    }
}

// ─── Public backend-dispatching API ───────────────────────────────────────────

/// Upsert a regime override (replaces any existing one).
pub fn set_override_backend(
    backend: &BackendConnection,
    regime: &str,
    reason: &str,
    expires_at: &DateTime<Utc>,
) -> Result<()> {
    let expires_ts = expires_at.timestamp();
    query::dispatch(
        backend,
        |conn| set_sqlite(conn, regime, reason, expires_ts),
        |pool| set_postgres(pool, regime, reason, expires_ts),
    )
}

/// Clear any active regime override.
pub fn clear_override_backend(backend: &BackendConnection) -> Result<()> {
    query::dispatch(backend, clear_sqlite, |pool| clear_postgres(pool))
}

/// Get the active (non-expired) regime override, if any.
pub fn get_active_override_backend(backend: &BackendConnection) -> Result<Option<RegimeOverride>> {
    query::dispatch(backend, get_active_sqlite, |pool| get_active_postgres(pool))
}
