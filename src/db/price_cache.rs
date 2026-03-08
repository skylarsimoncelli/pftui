use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;
use crate::models::price::PriceQuote;

#[allow(dead_code)]
pub fn get_cached_price(conn: &Connection, symbol: &str, currency: &str) -> Result<Option<PriceQuote>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, price, currency, fetched_at, source FROM price_cache
         WHERE symbol = ?1 AND currency = ?2",
    )?;
    let mut rows = stmt.query_map(params![symbol, currency], |row| {
        Ok(PriceQuote {
            symbol: row.get(0)?,
            price: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            currency: row.get(2)?,
            fetched_at: row.get(3)?,
            source: row.get(4)?,
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn upsert_price(conn: &Connection, quote: &PriceQuote) -> Result<()> {
    conn.execute(
        "INSERT INTO price_cache (symbol, price, currency, fetched_at, source)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(symbol, currency) DO UPDATE SET
           price = excluded.price,
           fetched_at = excluded.fetched_at,
           source = excluded.source",
        params![
            quote.symbol,
            quote.price.to_string(),
            quote.currency,
            quote.fetched_at,
            quote.source,
        ],
    )?;
    Ok(())
}

pub fn get_all_cached_prices(conn: &Connection) -> Result<Vec<PriceQuote>> {
    let mut stmt = conn.prepare(
        "SELECT symbol, price, currency, fetched_at, source FROM price_cache",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PriceQuote {
            symbol: row.get(0)?,
            price: row.get::<_, String>(1)?.parse().unwrap_or(Decimal::ZERO),
            currency: row.get(2)?,
            fetched_at: row.get(3)?,
            source: row.get(4)?,
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[allow(dead_code)]
pub fn get_cached_price_backend(
    backend: &BackendConnection,
    symbol: &str,
    currency: &str,
) -> Result<Option<PriceQuote>> {
    query::dispatch(
        backend,
        |conn| get_cached_price(conn, symbol, currency),
        |pool| get_cached_price_postgres(pool, symbol, currency),
    )
}

pub fn upsert_price_backend(backend: &BackendConnection, quote: &PriceQuote) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_price(conn, quote),
        |pool| upsert_price_postgres(pool, quote),
    )
}

pub fn get_all_cached_prices_backend(backend: &BackendConnection) -> Result<Vec<PriceQuote>> {
    query::dispatch(backend, get_all_cached_prices, get_all_cached_prices_postgres)
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS price_cache (
                symbol TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                fetched_at TEXT NOT NULL,
                source TEXT NOT NULL,
                PRIMARY KEY (symbol, currency)
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type PriceCacheRow = (String, String, String, String, String);

fn price_quote_from_row(row: PriceCacheRow) -> PriceQuote {
    PriceQuote {
        symbol: row.0,
        price: row.1.parse().unwrap_or(Decimal::ZERO),
        currency: row.2,
        fetched_at: row.3,
        source: row.4,
        pre_market_price: None,
        post_market_price: None,
        post_market_change_percent: None,
    }
}

fn get_cached_price_postgres(pool: &PgPool, symbol: &str, currency: &str) -> Result<Option<PriceQuote>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let row: Option<PriceCacheRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT symbol, price, currency, fetched_at, source
             FROM price_cache
             WHERE symbol = $1 AND currency = $2",
        )
        .bind(symbol)
        .bind(currency)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(price_quote_from_row))
}

fn upsert_price_postgres(pool: &PgPool, quote: &PriceQuote) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "INSERT INTO price_cache (symbol, price, currency, fetched_at, source)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (symbol, currency)
             DO UPDATE SET
                price = EXCLUDED.price,
                fetched_at = EXCLUDED.fetched_at,
                source = EXCLUDED.source",
        )
        .bind(&quote.symbol)
        .bind(quote.price.to_string())
        .bind(&quote.currency)
        .bind(&quote.fetched_at)
        .bind(&quote.source)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_all_cached_prices_postgres(pool: &PgPool) -> Result<Vec<PriceQuote>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<PriceCacheRow> = runtime.block_on(async {
        sqlx::query_as("SELECT symbol, price, currency, fetched_at, source FROM price_cache")
            .fetch_all(pool)
            .await
    })?;
    Ok(rows.into_iter().map(price_quote_from_row).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rust_decimal_macros::dec;

    #[test]
    fn test_upsert_and_get() {
        let conn = open_in_memory();
        let quote = PriceQuote {
            symbol: "AAPL".to_string(),
            price: dec!(189.50),
            currency: "USD".to_string(),
            fetched_at: "2025-01-15T12:00:00Z".to_string(),
            source: "yahoo".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
        };
        upsert_price(&conn, &quote).unwrap();

        let cached = get_cached_price(&conn, "AAPL", "USD").unwrap().unwrap();
        assert_eq!(cached.price, dec!(189.50));

        // Upsert with new price
        let quote2 = PriceQuote {
            price: dec!(195.00),
            fetched_at: "2025-01-15T13:00:00Z".to_string(),
            ..quote
        };
        upsert_price(&conn, &quote2).unwrap();

        let cached = get_cached_price(&conn, "AAPL", "USD").unwrap().unwrap();
        assert_eq!(cached.price, dec!(195.00));
    }
}
