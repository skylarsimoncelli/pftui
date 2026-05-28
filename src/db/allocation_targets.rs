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
    pub target_floor_pct: Decimal,
    pub target_ceiling_pct: Decimal,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BandPosition {
    BelowFloor,
    InBand,
    AboveCeiling,
}

impl BandPosition {
    pub fn as_str(self) -> &'static str {
        match self {
            BandPosition::BelowFloor => "below-floor",
            BandPosition::InBand => "in-band",
            BandPosition::AboveCeiling => "above-ceiling",
        }
    }
}

impl AllocationTarget {
    pub fn band_position(&self, actual_pct: Decimal) -> BandPosition {
        if actual_pct < self.target_floor_pct {
            BandPosition::BelowFloor
        } else if actual_pct > self.target_ceiling_pct {
            BandPosition::AboveCeiling
        } else {
            BandPosition::InBand
        }
    }

    pub fn drift_from_actual(&self, actual_pct: Decimal) -> Decimal {
        match self.band_position(actual_pct) {
            BandPosition::BelowFloor => actual_pct - self.target_floor_pct,
            BandPosition::InBand => Decimal::ZERO,
            BandPosition::AboveCeiling => actual_pct - self.target_ceiling_pct,
        }
    }

    pub fn rebalance_pct_for_actual(&self, actual_pct: Decimal) -> Option<Decimal> {
        match self.band_position(actual_pct) {
            BandPosition::BelowFloor => Some(self.target_floor_pct),
            BandPosition::InBand => None,
            BandPosition::AboveCeiling => Some(self.target_ceiling_pct),
        }
    }
}

pub fn set_target(
    conn: &Connection,
    symbol: &str,
    target_pct: Decimal,
    drift_band_pct: Decimal,
) -> Result<()> {
    set_target_range(
        conn,
        symbol,
        target_pct - drift_band_pct,
        target_pct + drift_band_pct,
    )
}

pub fn set_target_range(
    conn: &Connection,
    symbol: &str,
    target_floor_pct: Decimal,
    target_ceiling_pct: Decimal,
) -> Result<()> {
    let (target_pct, drift_band_pct) = midpoint_and_band(target_floor_pct, target_ceiling_pct);
    conn.execute(
        "INSERT OR REPLACE INTO allocation_targets
            (symbol, target_pct, drift_band_pct, target_floor_pct, target_ceiling_pct, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        params![
            symbol,
            target_pct.to_string(),
            drift_band_pct.to_string(),
            target_floor_pct.to_string(),
            target_ceiling_pct.to_string()
        ],
    )?;
    Ok(())
}

#[allow(dead_code)] // Used in tests, will be used by TUI in F6.4
pub fn get_target(conn: &Connection, symbol: &str) -> Result<Option<AllocationTarget>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, target_pct, drift_band_pct, target_floor_pct, target_ceiling_pct, updated_at
         FROM allocation_targets
         WHERE symbol = ?1",
    )?;

    let result = stmt.query_row(params![symbol], |row| {
        allocation_target_from_sqlite_row(row)
    });

    match result {
        Ok(target) => Ok(Some(target)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn list_targets(conn: &Connection) -> Result<Vec<AllocationTarget>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, target_pct, drift_band_pct, target_floor_pct, target_ceiling_pct, updated_at
         FROM allocation_targets
         ORDER BY symbol",
    )?;

    let targets = stmt
        .query_map([], allocation_target_from_sqlite_row)?
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

pub fn set_target_range_backend(
    backend: &BackendConnection,
    symbol: &str,
    target_floor_pct: Decimal,
    target_ceiling_pct: Decimal,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| set_target_range(conn, symbol, target_floor_pct, target_ceiling_pct),
        |pool| set_target_range_postgres(pool, symbol, target_floor_pct, target_ceiling_pct),
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
                target_floor_pct NUMERIC NOT NULL,
                target_ceiling_pct NUMERIC NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        ensure_range_columns_postgres(pool).await?;
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
    set_target_range_postgres(
        pool,
        symbol,
        target_pct - drift_band_pct,
        target_pct + drift_band_pct,
    )
}

fn set_target_range_postgres(
    pool: &PgPool,
    symbol: &str,
    target_floor_pct: Decimal,
    target_ceiling_pct: Decimal,
) -> Result<()> {
    let (target_pct, drift_band_pct) = midpoint_and_band(target_floor_pct, target_ceiling_pct);
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO allocation_targets
                (symbol, target_pct, drift_band_pct, target_floor_pct, target_ceiling_pct, updated_at)
             VALUES ($1, $2::NUMERIC, $3::NUMERIC, $4::NUMERIC, $5::NUMERIC, NOW())
             ON CONFLICT(symbol) DO UPDATE SET
                target_pct = EXCLUDED.target_pct,
                drift_band_pct = EXCLUDED.drift_band_pct,
                target_floor_pct = EXCLUDED.target_floor_pct,
                target_ceiling_pct = EXCLUDED.target_ceiling_pct,
                updated_at = NOW()",
        )
        .bind(symbol)
        .bind(target_pct.to_string())
        .bind(drift_band_pct.to_string())
        .bind(target_floor_pct.to_string())
        .bind(target_ceiling_pct.to_string())
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(dead_code)]
fn get_target_postgres(pool: &PgPool, symbol: &str) -> Result<Option<AllocationTarget>> {
    ensure_tables_postgres(pool)?;
    let row: Option<(String, String, String, String, String, String)> =
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT symbol, target_pct::TEXT, drift_band_pct::TEXT,
                    target_floor_pct::TEXT, target_ceiling_pct::TEXT, updated_at::text
             FROM allocation_targets
             WHERE symbol = $1",
            )
            .bind(symbol)
            .fetch_optional(pool)
            .await
        })?;
    Ok(row.map(allocation_target_from_postgres_row))
}

fn list_targets_postgres(pool: &PgPool) -> Result<Vec<AllocationTarget>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<(String, String, String, String, String, String)> =
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT symbol, target_pct::TEXT, drift_band_pct::TEXT,
                    target_floor_pct::TEXT, target_ceiling_pct::TEXT, updated_at::text
             FROM allocation_targets
             ORDER BY symbol",
            )
            .fetch_all(pool)
            .await
        })?;
    Ok(rows
        .into_iter()
        .map(allocation_target_from_postgres_row)
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

async fn ensure_range_columns_postgres(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("ALTER TABLE allocation_targets ADD COLUMN IF NOT EXISTS target_floor_pct NUMERIC")
        .execute(pool)
        .await?;
    sqlx::query(
        "ALTER TABLE allocation_targets ADD COLUMN IF NOT EXISTS target_ceiling_pct NUMERIC",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE allocation_targets
         SET target_floor_pct = target_pct - drift_band_pct
         WHERE target_floor_pct IS NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE allocation_targets
         SET target_ceiling_pct = target_pct + drift_band_pct
         WHERE target_ceiling_pct IS NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE allocation_targets ALTER COLUMN target_floor_pct SET NOT NULL")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE allocation_targets ALTER COLUMN target_ceiling_pct SET NOT NULL")
        .execute(pool)
        .await?;
    Ok(())
}

fn allocation_target_from_sqlite_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AllocationTarget> {
    Ok(AllocationTarget {
        symbol: row.get(0)?,
        target_pct: parse_decimal(&row.get::<_, String>(1)?),
        drift_band_pct: parse_decimal(&row.get::<_, String>(2)?),
        target_floor_pct: parse_decimal(&row.get::<_, String>(3)?),
        target_ceiling_pct: parse_decimal(&row.get::<_, String>(4)?),
        updated_at: row.get(5)?,
    })
}

fn allocation_target_from_postgres_row(
    row: (String, String, String, String, String, String),
) -> AllocationTarget {
    AllocationTarget {
        symbol: row.0,
        target_pct: parse_decimal(&row.1),
        drift_band_pct: parse_decimal(&row.2),
        target_floor_pct: parse_decimal(&row.3),
        target_ceiling_pct: parse_decimal(&row.4),
        updated_at: row.5,
    }
}

fn midpoint_and_band(target_floor_pct: Decimal, target_ceiling_pct: Decimal) -> (Decimal, Decimal) {
    let two = Decimal::from(2);
    (
        (target_floor_pct + target_ceiling_pct) / two,
        (target_ceiling_pct - target_floor_pct) / two,
    )
}

fn parse_decimal(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap_or_default()
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
        assert_eq!(target.target_floor_pct, dec!(22));
        assert_eq!(target.target_ceiling_pct, dec!(28));
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
        assert_eq!(target.target_floor_pct, dec!(10));
        assert_eq!(target.target_ceiling_pct, dec!(20));
        Ok(())
    }

    #[test]
    fn test_set_target_range_computes_legacy_midpoint() -> Result<()> {
        let conn = test_db()?;
        set_target_range(&conn, "GC=F", dec!(22), dec!(30))?;
        let target = get_target(&conn, "GC=F")?.unwrap();
        assert_eq!(target.target_floor_pct, dec!(22));
        assert_eq!(target.target_ceiling_pct, dec!(30));
        assert_eq!(target.target_pct, dec!(26));
        assert_eq!(target.drift_band_pct, dec!(4));
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

    #[test]
    fn test_range_drift_is_zero_inside_band() -> Result<()> {
        let conn = test_db()?;
        set_target_range(&conn, "GC=F", dec!(22), dec!(30))?;
        let target = get_target(&conn, "GC=F")?.unwrap();
        assert_eq!(target.drift_from_actual(dec!(23)), Decimal::ZERO);
        assert_eq!(target.band_position(dec!(23)), BandPosition::InBand);
        Ok(())
    }

    #[test]
    fn test_range_drift_uses_floor_edge_below_band() -> Result<()> {
        let conn = test_db()?;
        set_target_range(&conn, "GC=F", dec!(22), dec!(30))?;
        let target = get_target(&conn, "GC=F")?.unwrap();
        assert_eq!(target.drift_from_actual(dec!(20)), dec!(-2));
        assert_eq!(target.band_position(dec!(20)), BandPosition::BelowFloor);
        assert_eq!(target.rebalance_pct_for_actual(dec!(20)), Some(dec!(22)));
        Ok(())
    }

    #[test]
    fn test_range_drift_uses_ceiling_edge_above_band() -> Result<()> {
        let conn = test_db()?;
        set_target_range(&conn, "GC=F", dec!(22), dec!(30))?;
        let target = get_target(&conn, "GC=F")?.unwrap();
        assert_eq!(target.drift_from_actual(dec!(33)), dec!(3));
        assert_eq!(target.band_position(dec!(33)), BandPosition::AboveCeiling);
        assert_eq!(target.rebalance_pct_for_actual(dec!(33)), Some(dec!(30)));
        Ok(())
    }
}
