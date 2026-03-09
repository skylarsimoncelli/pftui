use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;
use crate::models::asset::AssetCategory;

/// A single entry in the watchlist table.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WatchlistEntry {
    pub id: i64,
    pub symbol: String,
    pub category: String,
    pub group_id: i64,
    pub added_at: String,
    pub target_price: Option<String>,
    pub target_direction: Option<String>,
}

/// Add a symbol to the watchlist. Uses ON CONFLICT to upsert.
pub fn add_to_watchlist(
    conn: &Connection,
    symbol: &str,
    category: AssetCategory,
) -> Result<i64> {
    add_to_watchlist_in_group(conn, symbol, category, 1)
}

/// Add a symbol to a specific watchlist group (1-3).
pub fn add_to_watchlist_in_group(
    conn: &Connection,
    symbol: &str,
    category: AssetCategory,
    group_id: i64,
) -> Result<i64> {
    let gid = if (1..=3).contains(&group_id) { group_id } else { 1 };
    conn.execute(
        "INSERT INTO watchlist (symbol, category, group_id)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(symbol) DO UPDATE SET
           category = excluded.category,
           group_id = excluded.group_id",
        params![symbol.to_uppercase(), category.to_string(), gid],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Set or clear a target price on a watchlist entry.
/// direction should be "above" or "below". Pass None to clear.
pub fn set_watchlist_target(
    conn: &Connection,
    symbol: &str,
    target_price: Option<&str>,
    target_direction: Option<&str>,
) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE watchlist SET target_price = ?1, target_direction = ?2
         WHERE UPPER(symbol) = UPPER(?3)",
        params![target_price, target_direction, symbol],
    )?;
    Ok(rows > 0)
}

/// Remove a symbol from the watchlist.
pub fn remove_from_watchlist(conn: &Connection, symbol: &str) -> Result<bool> {
    let rows = conn.execute(
        "DELETE FROM watchlist WHERE UPPER(symbol) = UPPER(?1)",
        params![symbol],
    )?;
    Ok(rows > 0)
}

/// List all watchlist entries, ordered by added_at descending.
pub fn list_watchlist(conn: &Connection) -> Result<Vec<WatchlistEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, category, group_id, added_at, target_price, target_direction
         FROM watchlist ORDER BY added_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WatchlistEntry {
            id: row.get(0)?,
            symbol: row.get(1)?,
            category: row.get(2)?,
            group_id: row.get(3)?,
            added_at: row.get(4)?,
            target_price: row.get(5)?,
            target_direction: row.get(6)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// List watchlist entries for a specific group.
pub fn list_watchlist_by_group(conn: &Connection, group_id: i64) -> Result<Vec<WatchlistEntry>> {
    let gid = if (1..=3).contains(&group_id) { group_id } else { 1 };
    let mut stmt = conn.prepare(
        "SELECT id, symbol, category, group_id, added_at, target_price, target_direction
         FROM watchlist WHERE group_id = ?1 ORDER BY added_at DESC",
    )?;
    let rows = stmt.query_map(params![gid], |row| {
        Ok(WatchlistEntry {
            id: row.get(0)?,
            symbol: row.get(1)?,
            category: row.get(2)?,
            group_id: row.get(3)?,
            added_at: row.get(4)?,
            target_price: row.get(5)?,
            target_direction: row.get(6)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Get unique symbols and their categories from the watchlist.
#[allow(dead_code)]
pub fn get_watchlist_symbols(conn: &Connection) -> Result<Vec<(String, AssetCategory)>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, category FROM watchlist ORDER BY symbol",
    )?;
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

/// Check if a symbol exists in the watchlist.
#[allow(dead_code)]
pub fn is_watched(conn: &Connection, symbol: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM watchlist WHERE UPPER(symbol) = UPPER(?1)",
        params![symbol],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

pub fn add_to_watchlist_backend(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_to_watchlist(conn, symbol, category),
        |pool| add_to_watchlist_postgres(pool, symbol, category, 1),
    )
}

pub fn add_to_watchlist_in_group_backend(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
    group_id: i64,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_to_watchlist_in_group(conn, symbol, category, group_id),
        |pool| add_to_watchlist_postgres(pool, symbol, category, group_id),
    )
}

pub fn set_watchlist_target_backend(
    backend: &BackendConnection,
    symbol: &str,
    target_price: Option<&str>,
    target_direction: Option<&str>,
) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| set_watchlist_target(conn, symbol, target_price, target_direction),
        |pool| set_watchlist_target_postgres(pool, symbol, target_price, target_direction),
    )
}

pub fn remove_from_watchlist_backend(backend: &BackendConnection, symbol: &str) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| remove_from_watchlist(conn, symbol),
        |pool| remove_from_watchlist_postgres(pool, symbol),
    )
}

#[allow(dead_code)]
pub fn list_watchlist_backend(backend: &BackendConnection) -> Result<Vec<WatchlistEntry>> {
    query::dispatch(backend, list_watchlist, list_watchlist_postgres)
}

#[allow(dead_code)]
pub fn list_watchlist_by_group_backend(
    backend: &BackendConnection,
    group_id: i64,
) -> Result<Vec<WatchlistEntry>> {
    query::dispatch(
        backend,
        |conn| list_watchlist_by_group(conn, group_id),
        |pool| list_watchlist_by_group_postgres(pool, group_id),
    )
}

#[allow(dead_code)]
pub fn get_watchlist_symbols_backend(
    backend: &BackendConnection,
) -> Result<Vec<(String, AssetCategory)>> {
    query::dispatch(backend, get_watchlist_symbols, get_watchlist_symbols_postgres)
}

#[allow(dead_code)]
pub fn is_watched_backend(backend: &BackendConnection, symbol: &str) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| is_watched(conn, symbol),
        |pool| is_watched_postgres(pool, symbol),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS watchlist (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL,
                group_id BIGINT NOT NULL DEFAULT 1,
                added_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                target_price TEXT,
                target_direction TEXT
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_to_watchlist_postgres(
    pool: &PgPool,
    symbol: &str,
    category: AssetCategory,
    group_id: i64,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let gid = if (1..=3).contains(&group_id) { group_id } else { 1 };
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO watchlist (symbol, category, group_id)
             VALUES ($1, $2, $3)
             ON CONFLICT(symbol) DO UPDATE SET
                category = EXCLUDED.category,
                group_id = EXCLUDED.group_id
             RETURNING id",
        )
        .bind(symbol.to_uppercase())
        .bind(category.to_string())
        .bind(gid)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn set_watchlist_target_postgres(
    pool: &PgPool,
    symbol: &str,
    target_price: Option<&str>,
    target_direction: Option<&str>,
) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query(
            "UPDATE watchlist
             SET target_price = $1, target_direction = $2
             WHERE UPPER(symbol) = UPPER($3)",
        )
        .bind(target_price)
        .bind(target_direction)
        .bind(symbol)
        .execute(pool)
        .await
    })?;
    Ok(rows.rows_affected() > 0)
}

fn remove_from_watchlist_postgres(pool: &PgPool, symbol: &str) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query("DELETE FROM watchlist WHERE UPPER(symbol) = UPPER($1)")
            .bind(symbol)
            .execute(pool)
            .await
    })?;
    Ok(rows.rows_affected() > 0)
}

#[allow(dead_code)]
type WatchlistRow = (i64, String, String, i64, String, Option<String>, Option<String>);

#[allow(dead_code)]
fn watchlist_entry_from_row(row: WatchlistRow) -> WatchlistEntry {
    WatchlistEntry {
        id: row.0,
        symbol: row.1,
        category: row.2,
        group_id: row.3,
        added_at: row.4,
        target_price: row.5,
        target_direction: row.6,
    }
}

#[allow(dead_code)]
fn list_watchlist_postgres(pool: &PgPool) -> Result<Vec<WatchlistEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<WatchlistRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT id, symbol, category, group_id, added_at::text, target_price, target_direction
             FROM watchlist
             ORDER BY added_at DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(watchlist_entry_from_row).collect())
}

#[allow(dead_code)]
fn list_watchlist_by_group_postgres(pool: &PgPool, group_id: i64) -> Result<Vec<WatchlistEntry>> {
    ensure_tables_postgres(pool)?;
    let gid = if (1..=3).contains(&group_id) { group_id } else { 1 };
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<WatchlistRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT id, symbol, category, group_id, added_at::text, target_price, target_direction
             FROM watchlist
             WHERE group_id = $1
             ORDER BY added_at DESC",
        )
        .bind(gid)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(watchlist_entry_from_row).collect())
}

#[allow(dead_code)]
fn get_watchlist_symbols_postgres(pool: &PgPool) -> Result<Vec<(String, AssetCategory)>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(String, String)> = runtime.block_on(async {
        sqlx::query_as("SELECT symbol, category FROM watchlist ORDER BY symbol")
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|(symbol, category)| (symbol, category.parse().unwrap_or(AssetCategory::Equity)))
        .collect())
}

fn is_watched_postgres(pool: &PgPool, symbol: &str) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let count: i64 = runtime.block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM watchlist WHERE UPPER(symbol) = UPPER($1)")
            .bind(symbol)
            .fetch_one(pool)
            .await
    })?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn test_add_and_list() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();

        let entries = list_watchlist(&conn).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_upsert_same_symbol() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Fund).unwrap();

        let entries = list_watchlist(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].category, "fund");
        assert_eq!(entries[0].group_id, 1);
    }

    #[test]
    fn test_remove() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        assert!(remove_from_watchlist(&conn, "AAPL").unwrap());

        let entries = list_watchlist(&conn).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let conn = open_in_memory();
        assert!(!remove_from_watchlist(&conn, "XYZ").unwrap());
    }

    #[test]
    fn test_is_watched() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        assert!(is_watched(&conn, "AAPL").unwrap());
        assert!(!is_watched(&conn, "MSFT").unwrap());
    }

    #[test]
    fn test_case_insensitive_operations() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "aapl", AssetCategory::Equity).unwrap();
        // Stored as uppercase
        let entries = list_watchlist(&conn).unwrap();
        assert_eq!(entries[0].symbol, "AAPL");
        assert_eq!(entries[0].group_id, 1);
        // Remove with different case works
        assert!(remove_from_watchlist(&conn, "Aapl").unwrap());
        assert!(list_watchlist(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_set_watchlist_target() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "TSLA", AssetCategory::Equity).unwrap();

        // Set target
        assert!(set_watchlist_target(&conn, "TSLA", Some("300"), Some("below")).unwrap());
        let entries = list_watchlist(&conn).unwrap();
        assert_eq!(entries[0].target_price.as_deref(), Some("300"));
        assert_eq!(entries[0].target_direction.as_deref(), Some("below"));

        // Clear target
        assert!(set_watchlist_target(&conn, "TSLA", None, None).unwrap());
        let entries = list_watchlist(&conn).unwrap();
        assert!(entries[0].target_price.is_none());
        assert!(entries[0].target_direction.is_none());
    }

    #[test]
    fn test_set_target_nonexistent_symbol() {
        let conn = open_in_memory();
        assert!(!set_watchlist_target(&conn, "NOPE", Some("100"), Some("above")).unwrap());
    }

    #[test]
    fn test_get_watchlist_symbols() {
        let conn = open_in_memory();
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();

        let syms = get_watchlist_symbols(&conn).unwrap();
        assert_eq!(syms.len(), 2);
        // Ordered alphabetically
        assert_eq!(syms[0].0, "AAPL");
        assert_eq!(syms[1].0, "BTC");
    }

    #[test]
    fn test_list_watchlist_by_group() {
        let conn = open_in_memory();
        add_to_watchlist_in_group(&conn, "BTC", AssetCategory::Crypto, 1).unwrap();
        add_to_watchlist_in_group(&conn, "SOL", AssetCategory::Crypto, 2).unwrap();

        let g1 = list_watchlist_by_group(&conn, 1).unwrap();
        let g2 = list_watchlist_by_group(&conn, 2).unwrap();
        assert_eq!(g1.len(), 1);
        assert_eq!(g2.len(), 1);
        assert_eq!(g1[0].symbol, "BTC");
        assert_eq!(g2[0].symbol, "SOL");
    }
}
