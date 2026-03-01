use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};

use crate::models::price::HistoryRecord;

pub fn upsert_history(conn: &Connection, symbol: &str, source: &str, records: &[HistoryRecord]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO price_history (symbol, date, close, source, volume)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(symbol, date) DO UPDATE SET
           close = excluded.close,
           source = excluded.source,
           volume = COALESCE(excluded.volume, price_history.volume)",
    )?;
    for rec in records {
        let volume_str = rec.volume.map(|v| v.to_string());
        stmt.execute(params![symbol, rec.date, rec.close.to_string(), source, volume_str])?;
    }
    Ok(())
}

pub fn get_history(conn: &Connection, symbol: &str, limit: u32) -> Result<Vec<HistoryRecord>> {
    let mut stmt = conn.prepare(
        "SELECT date, close, volume FROM price_history
         WHERE symbol = ?1
         ORDER BY date DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![symbol, limit], |row| {
        let volume_str: Option<String> = row.get(2)?;
        let volume = volume_str.and_then(|s| s.parse::<u64>().ok());
        Ok(HistoryRecord {
            date: row.get(0)?,
            close: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            volume,
        })
    })?;
    let mut result: Vec<HistoryRecord> = rows.filter_map(|r| r.ok()).collect();
    result.reverse(); // chronological order (oldest first)
    Ok(result)
}

pub fn get_all_symbols_history(conn: &Connection, limit: u32) -> Result<Vec<(String, Vec<HistoryRecord>)>> {
    let mut sym_stmt = conn.prepare(
        "SELECT DISTINCT symbol FROM price_history",
    )?;
    let symbols: Vec<String> = sym_stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::new();
    for sym in symbols {
        let records = get_history(conn, &sym, limit)?;
        if !records.is_empty() {
            result.push((sym, records));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    #[test]
    fn test_upsert_and_get() {
        let conn = open_in_memory();
        let records = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(1_000_000) },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(105), volume: Some(1_500_000) },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(103), volume: None },
        ];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched.len(), 3);
        assert_eq!(fetched[0].date, "2025-01-01");
        assert_eq!(fetched[0].volume, Some(1_000_000));
        assert_eq!(fetched[1].volume, Some(1_500_000));
        assert_eq!(fetched[2].close, dec!(103));
        assert_eq!(fetched[2].volume, None);
    }

    #[test]
    fn test_upsert_overwrites() {
        let conn = open_in_memory();
        let r1 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(500_000) }];
        upsert_history(&conn, "AAPL", "yahoo", &r1).unwrap();

        let r2 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(200), volume: Some(750_000) }];
        upsert_history(&conn, "AAPL", "yahoo", &r2).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].close, dec!(200));
        assert_eq!(fetched[0].volume, Some(750_000));
    }

    #[test]
    fn test_upsert_preserves_volume_when_null() {
        let conn = open_in_memory();
        // First insert with volume
        let r1 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(500_000) }];
        upsert_history(&conn, "AAPL", "yahoo", &r1).unwrap();

        // Second insert without volume — should preserve existing
        let r2 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(105), volume: None }];
        upsert_history(&conn, "AAPL", "yahoo", &r2).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched[0].close, dec!(105));
        assert_eq!(fetched[0].volume, Some(500_000));
    }
}
