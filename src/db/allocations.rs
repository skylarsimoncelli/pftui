use anyhow::Result;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;
use crate::models::allocation::Allocation;
use crate::models::asset::AssetCategory;

pub fn insert_allocation(
    conn: &Connection,
    symbol: &str,
    category: AssetCategory,
    pct: Decimal,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO portfolio_allocations (symbol, category, allocation_pct)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(symbol) DO UPDATE SET
           category = excluded.category,
           allocation_pct = excluded.allocation_pct",
        params![symbol, category.to_string(), pct.to_string()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_allocation_backend(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
    pct: Decimal,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| insert_allocation(conn, symbol, category, pct),
        |pool| insert_allocation_postgres(pool, symbol, category, pct),
    )
}

pub fn list_allocations(conn: &Connection) -> Result<Vec<Allocation>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, category, allocation_pct, created_at
         FROM portfolio_allocations ORDER BY allocation_pct DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Allocation {
            id: row.get(0)?,
            symbol: row.get(1)?,
            category: row
                .get::<_, String>(2)?
                .parse()
                .unwrap_or(AssetCategory::Equity),
            allocation_pct: row.get::<_, String>(3)?.parse().unwrap_or(Decimal::ZERO),
            created_at: row.get(4)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[allow(dead_code)]
pub fn delete_all_allocations(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM portfolio_allocations", [])?;
    Ok(())
}

pub fn count_allocations(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM portfolio_allocations", [], |r| {
        r.get(0)
    })?;
    Ok(count)
}

pub fn get_unique_allocation_symbols(conn: &Connection) -> Result<Vec<(String, AssetCategory)>> {
    let mut stmt =
        conn.prepare("SELECT symbol, category FROM portfolio_allocations ORDER BY symbol")?;
    let rows = stmt.query_map([], |row| {
        let symbol: String = row.get(0)?;
        let cat: String = row.get(1)?;
        Ok((symbol, cat.parse().unwrap_or(AssetCategory::Equity)))
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[allow(dead_code)]
pub fn list_allocations_backend(backend: &BackendConnection) -> Result<Vec<Allocation>> {
    query::dispatch(backend, list_allocations, list_allocations_postgres)
}

#[allow(dead_code)]
pub fn count_allocations_backend(backend: &BackendConnection) -> Result<i64> {
    query::dispatch(backend, count_allocations, count_allocations_postgres)
}

pub fn get_unique_allocation_symbols_backend(
    backend: &BackendConnection,
) -> Result<Vec<(String, AssetCategory)>> {
    query::dispatch(
        backend,
        get_unique_allocation_symbols,
        get_unique_allocation_symbols_postgres,
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS portfolio_allocations (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                allocation_pct NUMERIC NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(dead_code)]
type AllocationRow = (i64, String, String, String, String);

#[allow(dead_code)]
fn allocation_from_row(row: AllocationRow) -> Allocation {
    Allocation {
        id: row.0,
        symbol: row.1,
        category: row.2.parse().unwrap_or(AssetCategory::Equity),
        allocation_pct: row.3.parse().unwrap_or(Decimal::ZERO),
        created_at: row.4,
    }
}

#[allow(dead_code)]
fn list_allocations_postgres(pool: &PgPool) -> Result<Vec<Allocation>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<AllocationRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, symbol, category, allocation_pct::TEXT, created_at::text
             FROM portfolio_allocations
             ORDER BY allocation_pct DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(allocation_from_row).collect())
}

fn insert_allocation_postgres(
    pool: &PgPool,
    symbol: &str,
    category: AssetCategory,
    pct: Decimal,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO portfolio_allocations (symbol, category, allocation_pct)
             VALUES ($1, $2, $3::NUMERIC)
             ON CONFLICT(symbol) DO UPDATE SET
               category = EXCLUDED.category,
               allocation_pct = EXCLUDED.allocation_pct
             RETURNING id",
        )
        .bind(symbol)
        .bind(category.to_string())
        .bind(pct.to_string())
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn count_allocations_postgres(pool: &PgPool) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM portfolio_allocations")
            .fetch_one(pool)
            .await
    })?;
    Ok(count)
}

fn get_unique_allocation_symbols_postgres(pool: &PgPool) -> Result<Vec<(String, AssetCategory)>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<(String, String)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT symbol, category
             FROM portfolio_allocations
             ORDER BY symbol",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|(symbol, category)| (symbol, category.parse().unwrap_or(AssetCategory::Equity)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    #[test]
    fn test_insert_and_list() {
        let conn = open_in_memory();
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(25)).unwrap();
        insert_allocation(&conn, "GC=F", AssetCategory::Commodity, dec!(30)).unwrap();

        let allocs = list_allocations(&conn).unwrap();
        assert_eq!(allocs.len(), 2);
        assert_eq!(allocs[0].allocation_pct, dec!(30)); // ordered desc
        assert_eq!(allocs[1].symbol, "BTC");
    }

    #[test]
    fn test_upsert() {
        let conn = open_in_memory();
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(25)).unwrap();
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(40)).unwrap();

        let allocs = list_allocations(&conn).unwrap();
        assert_eq!(allocs.len(), 1);
        assert_eq!(allocs[0].allocation_pct, dec!(40));
    }

    #[test]
    fn test_delete_all() {
        let conn = open_in_memory();
        insert_allocation(&conn, "BTC", AssetCategory::Crypto, dec!(25)).unwrap();
        delete_all_allocations(&conn).unwrap();
        assert_eq!(count_allocations(&conn).unwrap(), 0);
    }
}
