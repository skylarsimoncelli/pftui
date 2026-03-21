use anyhow::Result;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::str::FromStr;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, serde::Serialize)]
pub struct AllocationTarget {
    pub symbol: String,
    pub target_pct: Decimal,
    pub drift_band_pct: Decimal,
    pub updated_at: String,
}

pub fn set_target(
    conn: &Connection,
    symbol: &str,
    target_pct: Decimal,
    drift_band_pct: Decimal,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO allocation_targets (symbol, target_pct, drift_band_pct, updated_at)
         VALUES (?1, ?2, ?3, datetime('now'))",
        params![symbol, target_pct.to_string(), drift_band_pct.to_string()],
    )?;
    Ok(())
}

#[allow(dead_code)] // Used in tests, will be used by TUI in F6.4
pub fn get_target(conn: &Connection, symbol: &str) -> Result<Option<AllocationTarget>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, target_pct, drift_band_pct, updated_at
         FROM allocation_targets
         WHERE symbol = ?1",
    )?;

    let result = stmt.query_row(params![symbol], |row| {
        Ok(AllocationTarget {
            symbol: row.get(0)?,
            target_pct: Decimal::from_str(&row.get::<_, String>(1)?).unwrap_or_default(),
            drift_band_pct: Decimal::from_str(&row.get::<_, String>(2)?).unwrap_or_default(),
            updated_at: row.get(3)?,
        })
    });

    match result {
        Ok(target) => Ok(Some(target)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn list_targets(conn: &Connection) -> Result<Vec<AllocationTarget>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, target_pct, drift_band_pct, updated_at
         FROM allocation_targets
         ORDER BY symbol",
    )?;

    let targets = stmt
        .query_map([], |row| {
            Ok(AllocationTarget {
                symbol: row.get(0)?,
                target_pct: Decimal::from_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                drift_band_pct: Decimal::from_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                updated_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(targets)
}

pub fn remove_target(conn: &Connection, symbol: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM allocation_targets WHERE symbol = ?1",
        params![symbol],
    )?;
    Ok(())
}

pub fn set_target_backend(
    backend: &BackendConnection,
    symbol: &str,
    target_pct: Decimal,
    drift_band_pct: Decimal,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| set_target(conn, symbol, target_pct, drift_band_pct),
        |pool| set_target_postgres(pool, symbol, target_pct, drift_band_pct),
    )
}

#[allow(dead_code)]
pub fn get_target_backend(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<Option<AllocationTarget>> {
    query::dispatch(
        backend,
        |conn| get_target(conn, symbol),
        |pool| get_target_postgres(pool, symbol),
    )
}

pub fn list_targets_backend(backend: &BackendConnection) -> Result<Vec<AllocationTarget>> {
    query::dispatch(backend, list_targets, list_targets_postgres)
}

pub fn remove_target_backend(backend: &BackendConnection, symbol: &str) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_target(conn, symbol),
        |pool| remove_target_postgres(pool, symbol),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS allocation_targets (
                symbol TEXT PRIMARY KEY,
                target_pct NUMERIC NOT NULL,
                drift_band_pct NUMERIC NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn set_target_postgres(
    pool: &PgPool,
    symbol: &str,
    target_pct: Decimal,
    drift_band_pct: Decimal,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO allocation_targets (symbol, target_pct, drift_band_pct, updated_at)
             VALUES ($1, $2::NUMERIC, $3::NUMERIC, NOW())
             ON CONFLICT(symbol) DO UPDATE SET
                target_pct = EXCLUDED.target_pct,
                drift_band_pct = EXCLUDED.drift_band_pct,
                updated_at = NOW()",
        )
        .bind(symbol)
        .bind(target_pct.to_string())
        .bind(drift_band_pct.to_string())
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(dead_code)]
fn get_target_postgres(pool: &PgPool, symbol: &str) -> Result<Option<AllocationTarget>> {
    ensure_tables_postgres(pool)?;
    let row: Option<(String, String, String, String)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT symbol, target_pct::TEXT, drift_band_pct::TEXT, updated_at::text
             FROM allocation_targets
             WHERE symbol = $1",
        )
        .bind(symbol)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|r| AllocationTarget {
        symbol: r.0,
        target_pct: Decimal::from_str(&r.1).unwrap_or_default(),
        drift_band_pct: Decimal::from_str(&r.2).unwrap_or_default(),
        updated_at: r.3,
    }))
}

fn list_targets_postgres(pool: &PgPool) -> Result<Vec<AllocationTarget>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<(String, String, String, String)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT symbol, target_pct::TEXT, drift_band_pct::TEXT, updated_at::text
             FROM allocation_targets
             ORDER BY symbol",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| AllocationTarget {
            symbol: r.0,
            target_pct: Decimal::from_str(&r.1).unwrap_or_default(),
            drift_band_pct: Decimal::from_str(&r.2).unwrap_or_default(),
            updated_at: r.3,
        })
        .collect())
}

fn remove_target_postgres(pool: &PgPool, symbol: &str) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM allocation_targets WHERE symbol = $1")
            .bind(symbol)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use rust_decimal_macros::dec;

    fn test_db() -> Result<Connection> {
        let conn = Connection::open_in_memory()?;
        crate::db::schema::run_migrations(&conn)?;
        Ok(conn)
    }

    #[test]
    fn test_set_and_get_target() -> Result<()> {
        let conn = test_db()?;
        set_target(&conn, "GC=F", dec!(25), dec!(3))?;
        let target = get_target(&conn, "GC=F")?.unwrap();
        assert_eq!(target.symbol, "GC=F");
        assert_eq!(target.target_pct, dec!(25));
        assert_eq!(target.drift_band_pct, dec!(3));
        Ok(())
    }

    #[test]
    fn test_update_target() -> Result<()> {
        let conn = test_db()?;
        set_target(&conn, "BTC-USD", dec!(10), dec!(2))?;
        set_target(&conn, "BTC-USD", dec!(15), dec!(5))?;
        let target = get_target(&conn, "BTC-USD")?.unwrap();
        assert_eq!(target.target_pct, dec!(15));
        assert_eq!(target.drift_band_pct, dec!(5));
        Ok(())
    }

    #[test]
    fn test_list_targets() -> Result<()> {
        let conn = test_db()?;
        set_target(&conn, "GC=F", dec!(25), dec!(3))?;
        set_target(&conn, "BTC-USD", dec!(10), dec!(2))?;
        let targets = list_targets(&conn)?;
        assert_eq!(targets.len(), 2);
        Ok(())
    }

    #[test]
    fn test_remove_target() -> Result<()> {
        let conn = test_db()?;
        set_target(&conn, "GC=F", dec!(25), dec!(3))?;
        remove_target(&conn, "GC=F")?;
        assert!(get_target(&conn, "GC=F")?.is_none());
        Ok(())
    }
}
