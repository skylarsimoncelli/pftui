use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;
use crate::models::price::HistoryRecord;

pub fn upsert_history(conn: &Connection, symbol: &str, source: &str, records: &[HistoryRecord]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO price_history (symbol, date, close, source, volume, open, high, low)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(symbol, date) DO UPDATE SET
           close = excluded.close,
           source = excluded.source,
           volume = COALESCE(excluded.volume, price_history.volume),
           open = COALESCE(excluded.open, price_history.open),
           high = COALESCE(excluded.high, price_history.high),
           low = COALESCE(excluded.low, price_history.low)",
    )?;
    for rec in records {
        let volume_str = rec.volume.map(|v| v.to_string());
        let open_str = rec.open.map(|v| v.to_string());
        let high_str = rec.high.map(|v| v.to_string());
        let low_str = rec.low.map(|v| v.to_string());
        stmt.execute(params![symbol, rec.date, rec.close.to_string(), source, volume_str, open_str, high_str, low_str])?;
    }
    Ok(())
}

pub fn get_history(conn: &Connection, symbol: &str, limit: u32) -> Result<Vec<HistoryRecord>> {
    let mut stmt = conn.prepare(
        "SELECT date, close, volume, open, high, low FROM price_history
         WHERE symbol = ?1
         ORDER BY date DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![symbol, limit], |row| {
        let volume_str: Option<String> = row.get(2)?;
        let volume = volume_str.and_then(|s| s.parse::<u64>().ok());
        let open_str: Option<String> = row.get(3)?;
        let high_str: Option<String> = row.get(4)?;
        let low_str: Option<String> = row.get(5)?;
        Ok(HistoryRecord {
            date: row.get(0)?,
            close: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            volume,
            open: open_str.and_then(|s| s.parse().ok()),
            high: high_str.and_then(|s| s.parse().ok()),
            low: low_str.and_then(|s| s.parse().ok()),
        })
    })?;
    let mut result: Vec<HistoryRecord> = rows.filter_map(|r| r.ok()).collect();
    result.reverse(); // chronological order (oldest first)
    Ok(result)
}

/// Get the closest price on or before the given date for a symbol.
/// Returns None if no history exists at or before that date.
pub fn get_price_at_date(conn: &Connection, symbol: &str, date: &str) -> Result<Option<Decimal>> {
    let mut stmt = conn.prepare(
        "SELECT close FROM price_history
         WHERE symbol = ?1 AND date <= ?2
         ORDER BY date DESC
         LIMIT 1",
    )?;
    let result = stmt
        .query_row(params![symbol, date], |row| {
            let close_str: String = row.get(0)?;
            Ok(close_str.parse::<Decimal>().unwrap_or(Decimal::ZERO))
        })
        .ok();
    Ok(result)
}

/// Get the closest prices on or before the given date for multiple symbols.
/// Returns a map of symbol -> price for symbols that have history.
pub fn get_prices_at_date(
    conn: &Connection,
    symbols: &[String],
    date: &str,
) -> Result<HashMap<String, Decimal>> {
    let mut result = HashMap::new();
    for symbol in symbols {
        if let Some(price) = get_price_at_date(conn, symbol, date)? {
            result.insert(symbol.clone(), price);
        }
    }
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

#[allow(dead_code)]
pub fn upsert_history_backend(
    backend: &BackendConnection,
    symbol: &str,
    source: &str,
    records: &[HistoryRecord],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_history(conn, symbol, source, records),
        |pool| upsert_history_postgres(pool, symbol, source, records),
    )
}

#[allow(dead_code)]
pub fn get_history_backend(
    backend: &BackendConnection,
    symbol: &str,
    limit: u32,
) -> Result<Vec<HistoryRecord>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, symbol, limit),
        |pool| get_history_postgres(pool, symbol, limit),
    )
}

pub fn get_price_at_date_backend(
    backend: &BackendConnection,
    symbol: &str,
    date: &str,
) -> Result<Option<Decimal>> {
    query::dispatch(
        backend,
        |conn| get_price_at_date(conn, symbol, date),
        |pool| get_price_at_date_postgres(pool, symbol, date),
    )
}

#[allow(dead_code)]
pub fn get_prices_at_date_backend(
    backend: &BackendConnection,
    symbols: &[String],
    date: &str,
) -> Result<HashMap<String, Decimal>> {
    query::dispatch(
        backend,
        |conn| get_prices_at_date(conn, symbols, date),
        |pool| get_prices_at_date_postgres(pool, symbols, date),
    )
}

#[allow(dead_code)]
pub fn get_all_symbols_history_backend(
    backend: &BackendConnection,
    limit: u32,
) -> Result<Vec<(String, Vec<HistoryRecord>)>> {
    query::dispatch(
        backend,
        |conn| get_all_symbols_history(conn, limit),
        |pool| get_all_symbols_history_postgres(pool, limit),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                source TEXT NOT NULL,
                volume TEXT,
                open TEXT,
                high TEXT,
                low TEXT,
                PRIMARY KEY (symbol, date)
            )",
        )
        .execute(pool)
        .await?;

        // Migration: add OHLC columns if missing (F48)
        sqlx::query(
            "DO $$ BEGIN
                ALTER TABLE price_history ADD COLUMN IF NOT EXISTS open TEXT;
                ALTER TABLE price_history ADD COLUMN IF NOT EXISTS high TEXT;
                ALTER TABLE price_history ADD COLUMN IF NOT EXISTS low TEXT;
             END $$",
        )
        .execute(pool)
        .await?;

        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_history_postgres(
    pool: &PgPool,
    symbol: &str,
    source: &str,
    records: &[HistoryRecord],
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        for rec in records {
            let volume = rec.volume.map(|v| v.to_string());
            let open = rec.open.map(|v| v.to_string());
            let high = rec.high.map(|v| v.to_string());
            let low = rec.low.map(|v| v.to_string());
            sqlx::query(
                "INSERT INTO price_history (symbol, date, close, source, volume, open, high, low)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (symbol, date)
                 DO UPDATE SET
                    close = EXCLUDED.close,
                    source = EXCLUDED.source,
                    volume = COALESCE(EXCLUDED.volume, price_history.volume),
                    open = COALESCE(EXCLUDED.open, price_history.open),
                    high = COALESCE(EXCLUDED.high, price_history.high),
                    low = COALESCE(EXCLUDED.low, price_history.low)",
            )
            .bind(symbol)
            .bind(&rec.date)
            .bind(rec.close.to_string())
            .bind(source)
            .bind(volume)
            .bind(open)
            .bind(high)
            .bind(low)
            .execute(pool)
            .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type HistoryRow = (String, String, Option<String>, Option<String>, Option<String>, Option<String>);

fn history_record_from_row(row: HistoryRow) -> HistoryRecord {
    let volume = row.2.and_then(|v| v.parse::<u64>().ok());
    HistoryRecord {
        date: row.0,
        close: row.1.parse().unwrap_or(Decimal::ZERO),
        volume,
        open: row.3.and_then(|s| s.parse().ok()),
        high: row.4.and_then(|s| s.parse().ok()),
        low: row.5.and_then(|s| s.parse().ok()),
    }
}

fn get_history_postgres(pool: &PgPool, symbol: &str, limit: u32) -> Result<Vec<HistoryRecord>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<HistoryRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT date, close, volume, open, high, low
             FROM price_history
             WHERE symbol = $1
             ORDER BY date DESC
             LIMIT $2",
        )
        .bind(symbol)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    })?;
    let mut out: Vec<HistoryRecord> = rows.into_iter().map(history_record_from_row).collect();
    out.reverse();
    Ok(out)
}

fn get_price_at_date_postgres(pool: &PgPool, symbol: &str, date: &str) -> Result<Option<Decimal>> {
    ensure_tables_postgres(pool)?;
    let close: Option<String> = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "SELECT close
             FROM price_history
             WHERE symbol = $1 AND date <= $2
             ORDER BY date DESC
             LIMIT 1",
        )
        .bind(symbol)
        .bind(date)
        .fetch_optional(pool)
        .await
    })?;
    Ok(close.map(|c| c.parse().unwrap_or(Decimal::ZERO)))
}

fn get_prices_at_date_postgres(
    pool: &PgPool,
    symbols: &[String],
    date: &str,
) -> Result<HashMap<String, Decimal>> {
    let mut result = HashMap::new();
    for symbol in symbols {
        if let Some(price) = get_price_at_date_postgres(pool, symbol, date)? {
            result.insert(symbol.clone(), price);
        }
    }
    Ok(result)
}

fn get_all_symbols_history_postgres(
    pool: &PgPool,
    limit: u32,
) -> Result<Vec<(String, Vec<HistoryRecord>)>> {
    ensure_tables_postgres(pool)?;
    let symbols: Vec<String> = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT DISTINCT symbol FROM price_history")
            .fetch_all(pool)
            .await
    })?;

    let mut result = Vec::new();
    for symbol in symbols {
        let records = get_history_postgres(pool, &symbol, limit)?;
        if !records.is_empty() {
            result.push((symbol, records));
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
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(1_000_000), open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(105), volume: Some(1_500_000), open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(103), volume: None, open: None, high: None, low: None },
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
        let r1 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(500_000), open: None, high: None, low: None }];
        upsert_history(&conn, "AAPL", "yahoo", &r1).unwrap();

        let r2 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(200), volume: Some(750_000), open: None, high: None, low: None }];
        upsert_history(&conn, "AAPL", "yahoo", &r2).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].close, dec!(200));
        assert_eq!(fetched[0].volume, Some(750_000));
    }

    #[test]
    fn test_upsert_preserves_volume_when_null() {
        let conn = open_in_memory();
        let r1 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: Some(500_000), open: None, high: None, low: None }];
        upsert_history(&conn, "AAPL", "yahoo", &r1).unwrap();

        let r2 = vec![HistoryRecord { date: "2025-01-01".into(), close: dec!(105), volume: None, open: None, high: None, low: None }];
        upsert_history(&conn, "AAPL", "yahoo", &r2).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched[0].close, dec!(105));
        assert_eq!(fetched[0].volume, Some(500_000));
    }

    #[test]
    fn test_get_price_at_date_exact() {
        let conn = open_in_memory();
        let records = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-02".into(), close: dec!(105), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
        ];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        let price = get_price_at_date(&conn, "AAPL", "2025-01-02").unwrap();
        assert_eq!(price, Some(dec!(105)));
    }

    #[test]
    fn test_get_price_at_date_falls_back() {
        let conn = open_in_memory();
        let records = vec![
            HistoryRecord { date: "2025-01-01".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
            HistoryRecord { date: "2025-01-03".into(), close: dec!(110), volume: None, open: None, high: None, low: None },
        ];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        // No data for Jan 2, should fall back to Jan 1
        let price = get_price_at_date(&conn, "AAPL", "2025-01-02").unwrap();
        assert_eq!(price, Some(dec!(100)));
    }

    #[test]
    fn test_get_price_at_date_no_data() {
        let conn = open_in_memory();
        let records = vec![
            HistoryRecord { date: "2025-01-05".into(), close: dec!(100), volume: None, open: None, high: None, low: None },
        ];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        // All data is after Jan 2 — no result
        let price = get_price_at_date(&conn, "AAPL", "2025-01-02").unwrap();
        assert_eq!(price, None);
    }

    #[test]
    fn test_get_prices_at_date_multiple() {
        let conn = open_in_memory();
        upsert_history(&conn, "AAPL", "yahoo", &[
            HistoryRecord { date: "2025-01-01".into(), close: dec!(150), volume: None, open: None, high: None, low: None },
        ]).unwrap();
        upsert_history(&conn, "BTC", "coingecko", &[
            HistoryRecord { date: "2025-01-01".into(), close: dec!(42000), volume: None, open: None, high: None, low: None },
        ]).unwrap();

        let symbols = vec!["AAPL".to_string(), "BTC".to_string(), "MISSING".to_string()];
        let prices = get_prices_at_date(&conn, &symbols, "2025-01-01").unwrap();
        assert_eq!(prices.len(), 2);
        assert_eq!(prices["AAPL"], dec!(150));
        assert_eq!(prices["BTC"], dec!(42000));
        assert!(!prices.contains_key("MISSING"));
    }

    #[test]
    fn test_ohlcv_round_trip() {
        let conn = open_in_memory();
        let records = vec![
            HistoryRecord {
                date: "2025-01-01".into(),
                close: dec!(105),
                volume: Some(2_000_000),
                open: Some(dec!(100)),
                high: Some(dec!(108)),
                low: Some(dec!(98)),
            },
            HistoryRecord {
                date: "2025-01-02".into(),
                close: dec!(110),
                volume: Some(3_000_000),
                open: Some(dec!(106)),
                high: Some(dec!(112)),
                low: Some(dec!(104)),
            },
        ];
        upsert_history(&conn, "AAPL", "yahoo", &records).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched.len(), 2);
        // First record (oldest)
        assert_eq!(fetched[0].open, Some(dec!(100)));
        assert_eq!(fetched[0].high, Some(dec!(108)));
        assert_eq!(fetched[0].low, Some(dec!(98)));
        assert_eq!(fetched[0].close, dec!(105));
        assert_eq!(fetched[0].volume, Some(2_000_000));
        // Second record
        assert_eq!(fetched[1].open, Some(dec!(106)));
        assert_eq!(fetched[1].high, Some(dec!(112)));
        assert_eq!(fetched[1].low, Some(dec!(104)));
        assert_eq!(fetched[1].close, dec!(110));
    }

    #[test]
    fn test_ohlcv_partial_preserves_existing() {
        let conn = open_in_memory();
        // First upsert with full OHLCV
        let r1 = vec![HistoryRecord {
            date: "2025-01-01".into(),
            close: dec!(105),
            volume: Some(2_000_000),
            open: Some(dec!(100)),
            high: Some(dec!(108)),
            low: Some(dec!(98)),
        }];
        upsert_history(&conn, "AAPL", "yahoo", &r1).unwrap();

        // Second upsert with only close (no OHLC) — should preserve existing OHLC
        let r2 = vec![HistoryRecord {
            date: "2025-01-01".into(),
            close: dec!(107),
            volume: None,
            open: None,
            high: None,
            low: None,
        }];
        upsert_history(&conn, "AAPL", "yahoo", &r2).unwrap();

        let fetched = get_history(&conn, "AAPL", 90).unwrap();
        assert_eq!(fetched[0].close, dec!(107)); // close updated
        assert_eq!(fetched[0].open, Some(dec!(100))); // preserved
        assert_eq!(fetched[0].high, Some(dec!(108))); // preserved
        assert_eq!(fetched[0].low, Some(dec!(98))); // preserved
        assert_eq!(fetched[0].volume, Some(2_000_000)); // preserved
    }

    #[test]
    fn test_ohlcv_none_when_not_available() {
        let conn = open_in_memory();
        // CoinGecko-style: close+volume only, no OHLC
        let records = vec![HistoryRecord {
            date: "2025-01-01".into(),
            close: dec!(42000),
            volume: Some(50_000_000_000),
            open: None,
            high: None,
            low: None,
        }];
        upsert_history(&conn, "BTC", "coingecko", &records).unwrap();

        let fetched = get_history(&conn, "BTC", 90).unwrap();
        assert_eq!(fetched[0].close, dec!(42000));
        assert_eq!(fetched[0].open, None);
        assert_eq!(fetched[0].high, None);
        assert_eq!(fetched[0].low, None);
    }
}
