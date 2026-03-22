use anyhow::Result;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A snapshot of the total portfolio value at a point in time.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Infrastructure for F10.2+ (performance CLI, TUI panel)
pub struct PortfolioSnapshot {
    pub date: String,
    pub total_value: Decimal,
    pub cash_value: Decimal,
    pub invested_value: Decimal,
    pub snapshot_at: String,
}

/// A snapshot of a single position's value at a point in time.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Infrastructure for F10.2+ (performance CLI, TUI panel)
pub struct PositionSnapshot {
    pub date: String,
    pub symbol: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub value: Decimal,
}

/// Store (or update) a daily portfolio snapshot. One row per date.
pub fn upsert_portfolio_snapshot(
    conn: &Connection,
    date: &str,
    total_value: Decimal,
    cash_value: Decimal,
    invested_value: Decimal,
) -> Result<()> {
    conn.execute(
        "INSERT INTO portfolio_snapshots (date, total_value, cash_value, invested_value)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date) DO UPDATE SET
           total_value = excluded.total_value,
           cash_value = excluded.cash_value,
           invested_value = excluded.invested_value,
           snapshot_at = datetime('now')",
        params![
            date,
            total_value.to_string(),
            cash_value.to_string(),
            invested_value.to_string(),
        ],
    )?;
    Ok(())
}

pub fn upsert_portfolio_snapshot_backend(
    backend: &BackendConnection,
    date: &str,
    total_value: Decimal,
    cash_value: Decimal,
    invested_value: Decimal,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_portfolio_snapshot(conn, date, total_value, cash_value, invested_value),
        |pool| {
            upsert_portfolio_snapshot_postgres(pool, date, total_value, cash_value, invested_value)
        },
    )
}

/// Store (or update) a daily position snapshot. One row per (date, symbol).
pub fn upsert_position_snapshot(
    conn: &Connection,
    date: &str,
    symbol: &str,
    quantity: Decimal,
    price: Decimal,
    value: Decimal,
) -> Result<()> {
    conn.execute(
        "INSERT INTO position_snapshots (date, symbol, quantity, price, value)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, symbol) DO UPDATE SET
           quantity = excluded.quantity,
           price = excluded.price,
           value = excluded.value",
        params![
            date,
            symbol,
            quantity.to_string(),
            price.to_string(),
            value.to_string(),
        ],
    )?;
    Ok(())
}

pub fn upsert_position_snapshot_backend(
    backend: &BackendConnection,
    date: &str,
    symbol: &str,
    quantity: Decimal,
    price: Decimal,
    value: Decimal,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_position_snapshot(conn, date, symbol, quantity, price, value),
        |pool| upsert_position_snapshot_postgres(pool, date, symbol, quantity, price, value),
    )
}

/// Get portfolio snapshots for the last N days, ordered by date ascending.
#[allow(dead_code)] // Infrastructure for F10.2+ (performance CLI, TUI panel)
pub fn get_portfolio_snapshots(conn: &Connection, limit: usize) -> Result<Vec<PortfolioSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT date, total_value, cash_value, invested_value, snapshot_at
         FROM portfolio_snapshots
         ORDER BY date DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(PortfolioSnapshot {
            date: row.get(0)?,
            total_value: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            cash_value: row.get::<_, String>(2)?.parse().unwrap_or(Decimal::ZERO),
            invested_value: row.get::<_, String>(3)?.parse().unwrap_or(Decimal::ZERO),
            snapshot_at: row.get(4)?,
        })
    })?;
    let mut result: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    result.reverse(); // ascending order
    Ok(result)
}

/// Get position snapshots for a specific date.
#[allow(dead_code)]
pub fn get_position_snapshots_for_date(
    conn: &Connection,
    date: &str,
) -> Result<Vec<PositionSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT date, symbol, quantity, price, value
         FROM position_snapshots
         WHERE date = ?1
         ORDER BY symbol",
    )?;
    let rows = stmt.query_map(params![date], |row| {
        Ok(PositionSnapshot {
            date: row.get(0)?,
            symbol: row.get(1)?,
            quantity: row.get::<_, String>(2)?.parse().unwrap_or(Decimal::ZERO),
            price: row.get::<_, String>(3)?.parse().unwrap_or(Decimal::ZERO),
            value: row.get::<_, String>(4)?.parse().unwrap_or(Decimal::ZERO),
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Get the most recent snapshot date, if any.
#[allow(dead_code)]
pub fn latest_snapshot_date(conn: &Connection) -> Result<Option<String>> {
    let mut stmt =
        conn.prepare("SELECT date FROM portfolio_snapshots ORDER BY date DESC LIMIT 1")?;
    let mut rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Get portfolio snapshots since a given date (inclusive), ordered ascending.
#[allow(dead_code)] // Used in tests; available for F10.3 (TUI performance panel)
pub fn get_portfolio_snapshots_since(
    conn: &Connection,
    since_date: &str,
) -> Result<Vec<PortfolioSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT date, total_value, cash_value, invested_value, snapshot_at
         FROM portfolio_snapshots
         WHERE date >= ?1
         ORDER BY date ASC",
    )?;
    let rows = stmt.query_map(params![since_date], |row| {
        Ok(PortfolioSnapshot {
            date: row.get(0)?,
            total_value: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            cash_value: row.get::<_, String>(2)?.parse().unwrap_or(Decimal::ZERO),
            invested_value: row.get::<_, String>(3)?.parse().unwrap_or(Decimal::ZERO),
            snapshot_at: row.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Get all portfolio snapshots, ordered ascending.
pub fn get_all_portfolio_snapshots(conn: &Connection) -> Result<Vec<PortfolioSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT date, total_value, cash_value, invested_value, snapshot_at
         FROM portfolio_snapshots
         ORDER BY date ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PortfolioSnapshot {
            date: row.get(0)?,
            total_value: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            cash_value: row.get::<_, String>(2)?.parse().unwrap_or(Decimal::ZERO),
            invested_value: row.get::<_, String>(3)?.parse().unwrap_or(Decimal::ZERO),
            snapshot_at: row.get(4)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_all_portfolio_snapshots_backend(
    backend: &BackendConnection,
) -> Result<Vec<PortfolioSnapshot>> {
    query::dispatch(
        backend,
        get_all_portfolio_snapshots,
        get_all_portfolio_snapshots_postgres,
    )
}

fn get_all_portfolio_snapshots_postgres(pool: &PgPool) -> Result<Vec<PortfolioSnapshot>> {
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT date, total_value, cash_value, invested_value, snapshot_at::text
             FROM portfolio_snapshots
             ORDER BY date ASC",
        )
        .fetch_all(pool)
        .await
    })?;

    Ok(rows
        .into_iter()
        .map(
            |(date, total_value, cash_value, invested_value, snapshot_at)| PortfolioSnapshot {
                date,
                total_value: total_value.parse().unwrap_or(Decimal::ZERO),
                cash_value: cash_value.parse().unwrap_or(Decimal::ZERO),
                invested_value: invested_value.parse().unwrap_or(Decimal::ZERO),
                snapshot_at,
            },
        )
        .collect())
}

fn upsert_portfolio_snapshot_postgres(
    pool: &PgPool,
    date: &str,
    total_value: Decimal,
    cash_value: Decimal,
    invested_value: Decimal,
) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO portfolio_snapshots (date, total_value, cash_value, invested_value)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (date) DO UPDATE SET
               total_value = EXCLUDED.total_value,
               cash_value = EXCLUDED.cash_value,
               invested_value = EXCLUDED.invested_value,
               snapshot_at = NOW()",
        )
        .bind(date)
        .bind(total_value.to_string())
        .bind(cash_value.to_string())
        .bind(invested_value.to_string())
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_position_snapshot_postgres(
    pool: &PgPool,
    date: &str,
    symbol: &str,
    quantity: Decimal,
    price: Decimal,
    value: Decimal,
) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO position_snapshots (date, symbol, quantity, price, value)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (date, symbol) DO UPDATE SET
               quantity = EXCLUDED.quantity,
               price = EXCLUDED.price,
               value = EXCLUDED.value",
        )
        .bind(date)
        .bind(symbol)
        .bind(quantity.to_string())
        .bind(price.to_string())
        .bind(value.to_string())
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

/// Count total portfolio snapshots.
#[allow(dead_code)]
pub fn snapshot_count(conn: &Connection) -> Result<i64> {
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM portfolio_snapshots", [], |r| r.get(0))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    #[test]
    fn test_upsert_and_get_portfolio_snapshot() {
        let conn = open_in_memory();
        upsert_portfolio_snapshot(&conn, "2026-03-04", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();

        let snaps = get_portfolio_snapshots(&conn, 10).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].date, "2026-03-04");
        assert_eq!(snaps[0].total_value, dec!(100000));
        assert_eq!(snaps[0].cash_value, dec!(20000));
        assert_eq!(snaps[0].invested_value, dec!(80000));
    }

    #[test]
    fn test_upsert_overwrites_same_date() {
        let conn = open_in_memory();
        upsert_portfolio_snapshot(&conn, "2026-03-04", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-04", dec!(105000), dec!(20000), dec!(85000))
            .unwrap();

        let snaps = get_portfolio_snapshots(&conn, 10).unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].total_value, dec!(105000));
    }

    #[test]
    fn test_multiple_dates_ordered() {
        let conn = open_in_memory();
        upsert_portfolio_snapshot(&conn, "2026-03-02", dec!(98000), dec!(20000), dec!(78000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-04", dec!(102000), dec!(20000), dec!(82000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-03", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();

        let snaps = get_portfolio_snapshots(&conn, 10).unwrap();
        assert_eq!(snaps.len(), 3);
        assert_eq!(snaps[0].date, "2026-03-02");
        assert_eq!(snaps[1].date, "2026-03-03");
        assert_eq!(snaps[2].date, "2026-03-04");
    }

    #[test]
    fn test_limit_respected() {
        let conn = open_in_memory();
        for i in 1..=5 {
            upsert_portfolio_snapshot(
                &conn,
                &format!("2026-03-0{}", i),
                dec!(100000) + Decimal::from(i * 1000),
                dec!(20000),
                dec!(80000) + Decimal::from(i * 1000),
            )
            .unwrap();
        }

        let snaps = get_portfolio_snapshots(&conn, 3).unwrap();
        assert_eq!(snaps.len(), 3);
        // Should be the 3 most recent, in ascending order
        assert_eq!(snaps[0].date, "2026-03-03");
        assert_eq!(snaps[2].date, "2026-03-05");
    }

    #[test]
    fn test_position_snapshot_upsert_and_get() {
        let conn = open_in_memory();
        upsert_position_snapshot(&conn, "2026-03-04", "AAPL", dec!(10), dec!(195), dec!(1950))
            .unwrap();
        upsert_position_snapshot(
            &conn,
            "2026-03-04",
            "BTC",
            dec!(1.5),
            dec!(84000),
            dec!(126000),
        )
        .unwrap();

        let snaps = get_position_snapshots_for_date(&conn, "2026-03-04").unwrap();
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].symbol, "AAPL");
        assert_eq!(snaps[0].value, dec!(1950));
        assert_eq!(snaps[1].symbol, "BTC");
        assert_eq!(snaps[1].value, dec!(126000));
    }

    #[test]
    fn test_position_snapshot_overwrites() {
        let conn = open_in_memory();
        upsert_position_snapshot(&conn, "2026-03-04", "AAPL", dec!(10), dec!(195), dec!(1950))
            .unwrap();
        upsert_position_snapshot(&conn, "2026-03-04", "AAPL", dec!(10), dec!(200), dec!(2000))
            .unwrap();

        let snaps = get_position_snapshots_for_date(&conn, "2026-03-04").unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].price, dec!(200));
        assert_eq!(snaps[0].value, dec!(2000));
    }

    #[test]
    fn test_latest_snapshot_date() {
        let conn = open_in_memory();
        assert!(latest_snapshot_date(&conn).unwrap().is_none());

        upsert_portfolio_snapshot(&conn, "2026-03-02", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-04", dec!(102000), dec!(20000), dec!(82000))
            .unwrap();

        assert_eq!(latest_snapshot_date(&conn).unwrap().unwrap(), "2026-03-04");
    }

    #[test]
    fn test_snapshot_count() {
        let conn = open_in_memory();
        assert_eq!(snapshot_count(&conn).unwrap(), 0);

        upsert_portfolio_snapshot(&conn, "2026-03-02", dec!(100000), dec!(20000), dec!(80000))
            .unwrap();
        upsert_portfolio_snapshot(&conn, "2026-03-03", dec!(101000), dec!(20000), dec!(81000))
            .unwrap();

        assert_eq!(snapshot_count(&conn).unwrap(), 2);
    }

    #[test]
    fn test_empty_snapshots() {
        let conn = open_in_memory();
        let snaps = get_portfolio_snapshots(&conn, 10).unwrap();
        assert!(snaps.is_empty());
    }
}
