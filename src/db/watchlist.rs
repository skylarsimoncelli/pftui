use anyhow::Result;
use rusqlite::{params, Connection};

use crate::models::asset::AssetCategory;

/// A single entry in the watchlist table.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WatchlistEntry {
    pub id: i64,
    pub symbol: String,
    pub category: String,
    pub added_at: String,
}

/// Add a symbol to the watchlist. Uses ON CONFLICT to upsert.
pub fn add_to_watchlist(
    conn: &Connection,
    symbol: &str,
    category: AssetCategory,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO watchlist (symbol, category)
         VALUES (?1, ?2)
         ON CONFLICT(symbol) DO UPDATE SET
           category = excluded.category",
        params![symbol.to_uppercase(), category.to_string()],
    )?;
    Ok(conn.last_insert_rowid())
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
        "SELECT id, symbol, category, added_at
         FROM watchlist ORDER BY added_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WatchlistEntry {
            id: row.get(0)?,
            symbol: row.get(1)?,
            category: row.get(2)?,
            added_at: row.get(3)?,
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
        // Remove with different case works
        assert!(remove_from_watchlist(&conn, "Aapl").unwrap());
        assert!(list_watchlist(&conn).unwrap().is_empty());
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
}
