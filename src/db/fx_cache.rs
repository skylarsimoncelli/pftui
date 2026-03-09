use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rusqlite::Connection;
use sqlx::PgPool;
use std::str::FromStr;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Insert or update an FX rate in the cache.
pub fn upsert_fx_rate(conn: &Connection, currency: &str, rate: Decimal) -> Result<()> {
    let fetched_at = Utc::now().to_rfc3339();
    let rate_str = rate.to_string();

    conn.execute(
        "INSERT INTO fx_cache (currency, rate, fetched_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(currency) DO UPDATE SET
             rate = excluded.rate,
             fetched_at = excluded.fetched_at",
        [currency, &rate_str, &fetched_at],
    )?;
    Ok(())
}

pub fn upsert_fx_rate_backend(
    backend: &BackendConnection,
    currency: &str,
    rate: Decimal,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_fx_rate(conn, currency, rate),
        |pool| upsert_fx_rate_postgres(pool, currency, rate),
    )
}

/// Retrieve a cached FX rate if it's fresh (less than 15 minutes old).
#[allow(dead_code)]
pub fn get_fx_rate(conn: &Connection, currency: &str) -> Result<Option<Decimal>> {
    let result = conn.query_row(
        "SELECT rate, fetched_at FROM fx_cache WHERE currency = ?1",
        [currency],
        |row| {
            let rate_str: String = row.get(0)?;
            let fetched_at_str: String = row.get(1)?;
            Ok((rate_str, fetched_at_str))
        },
    );

    match result {
        Ok((rate_str, fetched_at_str)) => {
            // Check freshness: 15 minutes = 900 seconds
            if let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&fetched_at_str) {
                let age = Utc::now().signed_duration_since(fetched_at.with_timezone(&Utc));
                if age.num_seconds() < 900 {
                    // Fresh data
                    let rate = Decimal::from_str(&rate_str)?;
                    return Ok(Some(rate));
                }
            }
            Ok(None) // Stale
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all cached FX rates that are fresh (less than 15 minutes old).
pub fn get_all_fx_rates(conn: &Connection) -> Result<std::collections::HashMap<String, Decimal>> {
    let mut stmt = conn.prepare("SELECT currency, rate, fetched_at FROM fx_cache")?;
    let rows = stmt.query_map([], |row| {
        let currency: String = row.get(0)?;
        let rate_str: String = row.get(1)?;
        let fetched_at_str: String = row.get(2)?;
        Ok((currency, rate_str, fetched_at_str))
    })?;

    let mut rates = std::collections::HashMap::new();
    let now = Utc::now();

    for row in rows {
        let (currency, rate_str, fetched_at_str) = row?;
        
        // Check freshness
        if let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&fetched_at_str) {
            let age = now.signed_duration_since(fetched_at.with_timezone(&Utc));
            if age.num_seconds() < 900 {
                // Fresh data
                if let Ok(rate) = Decimal::from_str(&rate_str) {
                    rates.insert(currency, rate);
                }
            }
        }
    }

    Ok(rates)
}

pub fn get_all_fx_rates_backend(
    backend: &BackendConnection,
) -> Result<std::collections::HashMap<String, Decimal>> {
    query::dispatch(backend, get_all_fx_rates, get_all_fx_rates_postgres)
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS fx_cache (
                currency TEXT PRIMARY KEY,
                rate TEXT NOT NULL,
                fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_fx_rate_postgres(pool: &PgPool, currency: &str, rate: Decimal) -> Result<()> {
    ensure_table_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "INSERT INTO fx_cache (currency, rate, fetched_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (currency) DO UPDATE SET
               rate = EXCLUDED.rate,
               fetched_at = EXCLUDED.fetched_at",
        )
        .bind(currency)
        .bind(rate.to_string())
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_all_fx_rates_postgres(pool: &PgPool) -> Result<std::collections::HashMap<String, Decimal>> {
    ensure_table_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(String, String, i64)> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT currency, rate, EXTRACT(EPOCH FROM fetched_at)::BIGINT
             FROM fx_cache",
        )
        .fetch_all(pool)
        .await
    })?;

    let mut rates = std::collections::HashMap::new();
    let now = Utc::now().timestamp();
    for (currency, rate_str, fetched_epoch) in rows {
        if now - fetched_epoch < 900 {
            if let Ok(rate) = Decimal::from_str(&rate_str) {
                rates.insert(currency, rate);
            }
        }
    }
    Ok(rates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE fx_cache (
                currency TEXT PRIMARY KEY,
                rate TEXT NOT NULL,
                fetched_at TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get_fx_rate() {
        let conn = setup_test_db();
        
        // Insert GBP rate
        upsert_fx_rate(&conn, "GBP", dec!(1.27)).unwrap();
        
        // Retrieve it
        let rate = get_fx_rate(&conn, "GBP").unwrap();
        assert_eq!(rate, Some(dec!(1.27)));
        
        // Update it
        upsert_fx_rate(&conn, "GBP", dec!(1.28)).unwrap();
        let rate = get_fx_rate(&conn, "GBP").unwrap();
        assert_eq!(rate, Some(dec!(1.28)));
    }

    #[test]
    fn test_get_nonexistent_rate() {
        let conn = setup_test_db();
        let rate = get_fx_rate(&conn, "JPY").unwrap();
        assert_eq!(rate, None);
    }

    #[test]
    fn test_get_all_fx_rates() {
        let conn = setup_test_db();
        
        upsert_fx_rate(&conn, "GBP", dec!(1.27)).unwrap();
        upsert_fx_rate(&conn, "EUR", dec!(1.08)).unwrap();
        upsert_fx_rate(&conn, "CAD", dec!(0.72)).unwrap();
        
        let rates = get_all_fx_rates(&conn).unwrap();
        assert_eq!(rates.len(), 3);
        assert_eq!(rates.get("GBP"), Some(&dec!(1.27)));
        assert_eq!(rates.get("EUR"), Some(&dec!(1.08)));
        assert_eq!(rates.get("CAD"), Some(&dec!(0.72)));
    }
}
