//! SQLite cache for Fear & Greed sentiment indices.
//!
//! Stores both crypto (Alternative.me) and traditional (derived) indices
//! with 1-hour TTL for current values, historical snapshots for trends.

use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A cached sentiment index reading.
#[derive(Debug, Clone)]
pub struct SentimentReading {
    pub index_type: String, // "crypto" or "traditional"
    pub value: u8,          // 0-100
    pub classification: String,
    pub timestamp: i64,
    pub fetched_at: String,
}

/// Upsert a sentiment reading into the cache.
///
/// Uses index_type as the primary key for the latest reading.
/// Also appends to sentiment_history for trend tracking.
pub fn upsert_reading(conn: &Connection, reading: &SentimentReading) -> Result<()> {
    // Update the current cache (latest value per index_type)
    conn.execute(
        "INSERT INTO sentiment_cache (index_type, value, classification, timestamp, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(index_type) DO UPDATE SET
           value = excluded.value,
           classification = excluded.classification,
           timestamp = excluded.timestamp,
           fetched_at = excluded.fetched_at",
        params![
            reading.index_type,
            reading.value,
            reading.classification,
            reading.timestamp,
            reading.fetched_at,
        ],
    )?;

    // Also append to history for trend tracking
    conn.execute(
        "INSERT OR IGNORE INTO sentiment_history (index_type, date, value, classification)
         VALUES (?1, date(?2, 'unixepoch'), ?3, ?4)",
        params![
            reading.index_type,
            reading.timestamp,
            reading.value,
            reading.classification,
        ],
    )?;

    Ok(())
}

pub fn upsert_reading_backend(backend: &BackendConnection, reading: &SentimentReading) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_reading(conn, reading),
        |pool| upsert_reading_postgres(pool, reading),
    )
}

/// Get the latest sentiment reading for a given index type.
///
/// Returns None if not cached or if stale (>1 hour old).
pub fn get_latest(conn: &Connection, index_type: &str) -> Result<Option<SentimentReading>> {
    let one_hour_ago = chrono::Utc::now() - chrono::Duration::hours(1);
    let cutoff = one_hour_ago.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut stmt = conn.prepare(
        "SELECT index_type, value, classification, timestamp, fetched_at 
         FROM sentiment_cache
         WHERE index_type = ?1 AND fetched_at > ?2",
    )?;

    let mut rows = stmt.query_map(params![index_type, cutoff], |row| {
        Ok(SentimentReading {
            index_type: row.get(0)?,
            value: row.get(1)?,
            classification: row.get(2)?,
            timestamp: row.get(3)?,
            fetched_at: row.get(4)?,
        })
    })?;

    match rows.next() {
        Some(Ok(reading)) => Ok(Some(reading)),
        _ => Ok(None),
    }
}

pub fn get_latest_backend(
    backend: &BackendConnection,
    index_type: &str,
) -> Result<Option<SentimentReading>> {
    query::dispatch(
        backend,
        |conn| get_latest(conn, index_type),
        |pool| get_latest_postgres(pool, index_type),
    )
}

/// Get historical sentiment readings for a given index type.
///
/// Returns up to `days` of daily readings (one per day).
pub fn get_history(conn: &Connection, index_type: &str, days: u32) -> Result<Vec<(String, u8)>> {
    let mut stmt = conn.prepare(
        "SELECT date, value FROM sentiment_history
         WHERE index_type = ?1
         ORDER BY date DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![index_type, days], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u8>(1)?))
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

#[allow(dead_code)]
pub fn get_history_backend(
    backend: &BackendConnection,
    index_type: &str,
    days: u32,
) -> Result<Vec<(String, u8)>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, index_type, days),
        |pool| get_history_postgres(pool, index_type, days),
    )
}

/// Delete sentiment readings older than `days`.
///
/// Prunes both current cache and history to prevent unbounded growth.
pub fn prune_old(conn: &Connection, days: u32) -> Result<usize> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days.into());
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

    let pruned = conn.execute(
        "DELETE FROM sentiment_history WHERE date < ?1",
        params![cutoff_str],
    )?;

    Ok(pruned)
}

#[allow(dead_code)]
pub fn prune_old_backend(backend: &BackendConnection, days: u32) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| prune_old(conn, days),
        |pool| prune_old_postgres(pool, days),
    )
}

fn upsert_reading_postgres(pool: &PgPool, reading: &SentimentReading) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "INSERT INTO sentiment_cache (index_type, value, classification, timestamp, fetched_at)
             VALUES ($1, $2, $3, $4, $5::timestamptz)
             ON CONFLICT (index_type) DO UPDATE SET
               value = EXCLUDED.value,
               classification = EXCLUDED.classification,
               timestamp = EXCLUDED.timestamp,
               fetched_at = EXCLUDED.fetched_at",
        )
        .bind(&reading.index_type)
        .bind(reading.value as i64)
        .bind(&reading.classification)
        .bind(reading.timestamp)
        .bind(&reading.fetched_at)
        .execute(pool)
        .await?;

        sqlx::query(
            "INSERT INTO sentiment_history (index_type, date, value, classification)
             VALUES ($1, TO_CHAR(TO_TIMESTAMP($2), 'YYYY-MM-DD'), $3, $4)
             ON CONFLICT (index_type, date) DO NOTHING",
        )
        .bind(&reading.index_type)
        .bind(reading.timestamp as f64)
        .bind(reading.value as i64)
        .bind(&reading.classification)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_latest_postgres(pool: &PgPool, index_type: &str) -> Result<Option<SentimentReading>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let row: Option<(String, i64, String, i64, String)> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT index_type, value, classification, timestamp, fetched_at::text
             FROM sentiment_cache
             WHERE index_type = $1
               AND fetched_at > NOW() - INTERVAL '1 hour'",
        )
        .bind(index_type)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|r| SentimentReading {
        index_type: r.0,
        value: r.1 as u8,
        classification: r.2,
        timestamp: r.3,
        fetched_at: r.4,
    }))
}

fn get_history_postgres(pool: &PgPool, index_type: &str, days: u32) -> Result<Vec<(String, u8)>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(String, i64)> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT date, value
             FROM sentiment_history
             WHERE index_type = $1
             ORDER BY date DESC
             LIMIT $2",
        )
        .bind(index_type)
        .bind(days as i64)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(|(d, v)| (d, v as u8)).collect())
}

fn prune_old_postgres(pool: &PgPool, days: u32) -> Result<usize> {
    let runtime = tokio::runtime::Runtime::new()?;
    let pruned = runtime.block_on(async {
        sqlx::query(
            "DELETE FROM sentiment_history
             WHERE date < TO_CHAR(NOW() - ($1 * INTERVAL '1 day'), 'YYYY-MM-DD')",
        )
        .bind(days as i64)
        .execute(pool)
        .await
    })?;
    Ok(pruned.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get_latest() {
        let conn = setup_test_db();
        let now = chrono::Utc::now();

        let reading = SentimentReading {
            index_type: "crypto".to_string(),
            value: 22,
            classification: "Extreme Fear".to_string(),
            timestamp: now.timestamp(),
            fetched_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        upsert_reading(&conn, &reading).unwrap();

        let latest = get_latest(&conn, "crypto").unwrap();
        assert!(latest.is_some());
        let latest = latest.unwrap();
        assert_eq!(latest.value, 22);
        assert_eq!(latest.classification, "Extreme Fear");
    }

    #[test]
    fn test_stale_cache() {
        let conn = setup_test_db();
        let two_hours_ago = chrono::Utc::now() - chrono::Duration::hours(2);

        let reading = SentimentReading {
            index_type: "crypto".to_string(),
            value: 50,
            classification: "Neutral".to_string(),
            timestamp: two_hours_ago.timestamp(),
            fetched_at: two_hours_ago.format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        upsert_reading(&conn, &reading).unwrap();

        // Should return None because it's older than 1 hour
        let latest = get_latest(&conn, "crypto").unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_get_history() {
        let conn = setup_test_db();
        
        // Insert readings for 3 consecutive days
        for i in 0..3 {
            let day = chrono::Utc::now() - chrono::Duration::days(i);
            let reading = SentimentReading {
                index_type: "crypto".to_string(),
                value: 20 + (i as u8 * 10),
                classification: "Fear".to_string(),
                timestamp: day.timestamp(),
                fetched_at: day.format("%Y-%m-%d %H:%M:%S").to_string(),
            };
            upsert_reading(&conn, &reading).unwrap();
        }

        let history = get_history(&conn, "crypto", 3).unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_prune_old() {
        let conn = setup_test_db();
        
        // Insert old reading (40 days ago)
        let old = chrono::Utc::now() - chrono::Duration::days(40);
        let reading = SentimentReading {
            index_type: "crypto".to_string(),
            value: 80,
            classification: "Extreme Greed".to_string(),
            timestamp: old.timestamp(),
            fetched_at: old.format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        upsert_reading(&conn, &reading).unwrap();

        // Prune anything older than 30 days
        let pruned = prune_old(&conn, 30).unwrap();
        assert_eq!(pruned, 1);

        // Verify it's gone
        let history = get_history(&conn, "crypto", 100).unwrap();
        assert_eq!(history.len(), 0);
    }
}
