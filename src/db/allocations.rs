use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};

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
            allocation_pct: row
                .get::<_, String>(3)?
                .parse()
                .unwrap_or(Decimal::ZERO),
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
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM portfolio_allocations",
        [],
        |r| r.get(0),
    )?;
    Ok(count)
}

pub fn get_unique_allocation_symbols(
    conn: &Connection,
) -> Result<Vec<(String, AssetCategory)>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, category FROM portfolio_allocations ORDER BY symbol",
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
