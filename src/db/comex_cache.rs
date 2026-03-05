//! SQLite cache for COMEX warehouse inventory data.
//!
//! Stores daily registered/eligible inventory snapshots.
//! Daily refresh — data updates after market close (~5pm ET).

use anyhow::Result;
use rusqlite::{params, Connection};

/// A cached COMEX inventory record.
#[derive(Debug, Clone)]
pub struct ComexCacheEntry {
    pub symbol: String,       // GC=F or SI=F
    pub date: String,          // YYYY-MM-DD
    pub registered: f64,       // Registered stocks (troy oz)
    pub eligible: f64,         // Eligible stocks (troy oz)
    pub total: f64,            // Total (registered + eligible)
    pub reg_ratio: f64,        // Registered / Total (%)
    pub fetched_at: String,    // ISO 8601 timestamp
}

/// Upsert a COMEX inventory record into the cache.
///
/// Uses (symbol, date) as the primary key.
pub fn upsert_inventory(conn: &Connection, entry: &ComexCacheEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO comex_cache (
            symbol, date, registered, eligible, total, reg_ratio, fetched_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(symbol, date) DO UPDATE SET
            registered = excluded.registered,
            eligible = excluded.eligible,
            total = excluded.total,
            reg_ratio = excluded.reg_ratio,
            fetched_at = excluded.fetched_at",
        params![
            entry.symbol,
            entry.date,
            entry.registered,
            entry.eligible,
            entry.total,
            entry.reg_ratio,
            entry.fetched_at,
        ],
    )?;
    Ok(())
}

/// Batch upsert multiple COMEX inventory records.
pub fn upsert_inventories(conn: &Connection, entries: &[ComexCacheEntry]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for entry in entries {
        upsert_inventory(&tx, entry)?;
    }
    tx.commit()?;
    Ok(())
}

/// Get the most recent COMEX inventory for a symbol.
pub fn get_latest_inventory(conn: &Connection, symbol: &str) -> Result<Option<ComexCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, date, registered, eligible, total, reg_ratio, fetched_at
         FROM comex_cache
         WHERE symbol = ?1
         ORDER BY date DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query(params![symbol])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ComexCacheEntry {
            symbol: row.get(0)?,
            date: row.get(1)?,
            registered: row.get(2)?,
            eligible: row.get(3)?,
            total: row.get(4)?,
            reg_ratio: row.get(5)?,
            fetched_at: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

/// Get COMEX inventory history for a symbol (last N days).
pub fn get_inventory_history(
    conn: &Connection,
    symbol: &str,
    days: usize,
) -> Result<Vec<ComexCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, date, registered, eligible, total, reg_ratio, fetched_at
         FROM comex_cache
         WHERE symbol = ?1
         ORDER BY date DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![symbol, days], |row| {
        Ok(ComexCacheEntry {
            symbol: row.get(0)?,
            date: row.get(1)?,
            registered: row.get(2)?,
            eligible: row.get(3)?,
            total: row.get(4)?,
            reg_ratio: row.get(5)?,
            fetched_at: row.get(6)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

/// Get previous day's inventory for trend comparison.
pub fn get_previous_inventory(
    conn: &Connection,
    symbol: &str,
    current_date: &str,
) -> Result<Option<ComexCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, date, registered, eligible, total, reg_ratio, fetched_at
         FROM comex_cache
         WHERE symbol = ?1 AND date < ?2
         ORDER BY date DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query(params![symbol, current_date])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ComexCacheEntry {
            symbol: row.get(0)?,
            date: row.get(1)?,
            registered: row.get(2)?,
            eligible: row.get(3)?,
            total: row.get(4)?,
            reg_ratio: row.get(5)?,
            fetched_at: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

/// Check if we have fresh data (today's date).
pub fn has_fresh_data(conn: &Connection, symbol: &str) -> Result<bool> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare("SELECT 1 FROM comex_cache WHERE symbol = ?1 AND date = ?2")?;
    let exists = stmt.exists(params![symbol, today])?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE comex_cache (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                registered REAL NOT NULL,
                eligible REAL NOT NULL,
                total REAL NOT NULL,
                reg_ratio REAL NOT NULL,
                fetched_at TEXT NOT NULL,
                PRIMARY KEY (symbol, date)
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get_latest() {
        let conn = setup_test_db();
        let entry = ComexCacheEntry {
            symbol: "GC=F".to_string(),
            date: "2026-03-05".to_string(),
            registered: 10_000_000.0,
            eligible: 20_000_000.0,
            total: 30_000_000.0,
            reg_ratio: 33.33,
            fetched_at: "2026-03-05T10:00:00Z".to_string(),
        };

        upsert_inventory(&conn, &entry).unwrap();
        let latest = get_latest_inventory(&conn, "GC=F").unwrap().unwrap();
        assert_eq!(latest.registered, 10_000_000.0);
        assert_eq!(latest.reg_ratio, 33.33);
    }

    #[test]
    fn test_get_previous_inventory() {
        let conn = setup_test_db();
        let old = ComexCacheEntry {
            symbol: "GC=F".to_string(),
            date: "2026-03-04".to_string(),
            registered: 9_500_000.0,
            eligible: 20_000_000.0,
            total: 29_500_000.0,
            reg_ratio: 32.20,
            fetched_at: "2026-03-04T10:00:00Z".to_string(),
        };
        let new = ComexCacheEntry {
            symbol: "GC=F".to_string(),
            date: "2026-03-05".to_string(),
            registered: 10_000_000.0,
            eligible: 20_000_000.0,
            total: 30_000_000.0,
            reg_ratio: 33.33,
            fetched_at: "2026-03-05T10:00:00Z".to_string(),
        };

        upsert_inventory(&conn, &old).unwrap();
        upsert_inventory(&conn, &new).unwrap();

        let prev = get_previous_inventory(&conn, "GC=F", "2026-03-05")
            .unwrap()
            .unwrap();
        assert_eq!(prev.date, "2026-03-04");
        assert_eq!(prev.registered, 9_500_000.0);
    }

    #[test]
    fn test_has_fresh_data() {
        let conn = setup_test_db();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let entry = ComexCacheEntry {
            symbol: "GC=F".to_string(),
            date: today.clone(),
            registered: 10_000_000.0,
            eligible: 20_000_000.0,
            total: 30_000_000.0,
            reg_ratio: 33.33,
            fetched_at: chrono::Utc::now().to_rfc3339(),
        };

        assert!(!has_fresh_data(&conn, "GC=F").unwrap());
        upsert_inventory(&conn, &entry).unwrap();
        assert!(has_fresh_data(&conn, "GC=F").unwrap());
    }
}
