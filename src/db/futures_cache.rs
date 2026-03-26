//! futures_cache — SQLite + Postgres cache for overnight futures quotes.
//!
//! Stores the latest snapshot so agents can read pre-market positioning without
//! re-fetching from Yahoo Finance every time.

use anyhow::Result;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::data::futures::FuturesQuote;
use crate::db::backend::BackendConnection;
use crate::db::query;

/// Create the futures_cache table if it doesn't exist (SQLite).
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS futures_cache (
            symbol          TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            last_price      TEXT NOT NULL,
            previous_close  TEXT,
            change          TEXT,
            change_pct      TEXT,
            volume          INTEGER,
            fetched_at      TEXT NOT NULL
        );",
    )?;
    Ok(())
}

/// Create the futures_cache table if it doesn't exist (Postgres).
pub fn ensure_table_pg(pool: &PgPool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS futures_cache (
                symbol          TEXT PRIMARY KEY,
                name            TEXT NOT NULL,
                last_price      TEXT NOT NULL,
                previous_close  TEXT,
                change          TEXT,
                change_pct      TEXT,
                volume          BIGINT,
                fetched_at      TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

/// Upsert a single futures quote into the cache.
pub fn upsert(conn: &Connection, q: &FuturesQuote) -> Result<()> {
    conn.execute(
        "INSERT INTO futures_cache (symbol, name, last_price, previous_close, change, change_pct, volume, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(symbol) DO UPDATE SET
           name = excluded.name,
           last_price = excluded.last_price,
           previous_close = excluded.previous_close,
           change = excluded.change,
           change_pct = excluded.change_pct,
           volume = excluded.volume,
           fetched_at = excluded.fetched_at",
        params![
            q.symbol,
            q.name,
            q.last_price.to_string(),
            q.previous_close.map(|d| d.to_string()),
            q.change.map(|d| d.to_string()),
            q.change_pct.map(|d| d.to_string()),
            q.volume.map(|v| v as i64),
            q.fetched_at,
        ],
    )?;
    Ok(())
}

/// Upsert a single futures quote into Postgres cache.
pub fn upsert_pg(pool: &PgPool, q: &FuturesQuote) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        sqlx::query(
            "INSERT INTO futures_cache (symbol, name, last_price, previous_close, change, change_pct, volume, fetched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT(symbol) DO UPDATE SET
               name = EXCLUDED.name,
               last_price = EXCLUDED.last_price,
               previous_close = EXCLUDED.previous_close,
               change = EXCLUDED.change,
               change_pct = EXCLUDED.change_pct,
               volume = EXCLUDED.volume,
               fetched_at = EXCLUDED.fetched_at",
        )
        .bind(&q.symbol)
        .bind(&q.name)
        .bind(q.last_price.to_string())
        .bind(q.previous_close.map(|d| d.to_string()))
        .bind(q.change.map(|d| d.to_string()))
        .bind(q.change_pct.map(|d| d.to_string()))
        .bind(q.volume.map(|v| v as i64))
        .bind(&q.fetched_at)
        .execute(pool)
        .await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

/// Upsert via backend dispatch.
pub fn upsert_backend(backend: &BackendConnection, q: &FuturesQuote) -> Result<()> {
    query::dispatch(
        backend,
        |conn| {
            ensure_table(conn)?;
            upsert(conn, q)
        },
        |pool| {
            ensure_table_pg(pool)?;
            upsert_pg(pool, q)
        },
    )
}

/// Read all cached futures quotes.
pub fn get_all(conn: &Connection) -> Result<Vec<FuturesQuote>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT symbol, name, last_price, previous_close, change, change_pct, volume, fetched_at
         FROM futures_cache ORDER BY symbol",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(FuturesQuote {
            symbol: row.get(0)?,
            name: row.get(1)?,
            last_price: row
                .get::<_, String>(2)?
                .parse()
                .unwrap_or(Decimal::ZERO),
            previous_close: row.get::<_, Option<String>>(3)?.and_then(|s| s.parse().ok()),
            change: row.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
            change_pct: row.get::<_, Option<String>>(5)?.and_then(|s| s.parse().ok()),
            volume: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
            fetched_at: row.get(7)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
}

/// Read all cached futures quotes from Postgres.
pub fn get_all_pg(pool: &PgPool) -> Result<Vec<FuturesQuote>> {
    ensure_table_pg(pool)?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<String>, Option<i64>, String)>(
            "SELECT symbol, name, last_price, previous_close, change, change_pct, volume, fetched_at
             FROM futures_cache ORDER BY symbol",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(symbol, name, last_price, prev, chg, chg_pct, vol, fetched)| FuturesQuote {
                symbol,
                name,
                last_price: last_price.parse().unwrap_or(Decimal::ZERO),
                previous_close: prev.and_then(|s| s.parse().ok()),
                change: chg.and_then(|s| s.parse().ok()),
                change_pct: chg_pct.and_then(|s| s.parse().ok()),
                volume: vol.map(|v| v as u64),
                fetched_at: fetched,
            })
            .collect())
    })
}

/// Read all cached futures quotes via backend dispatch.
pub fn get_all_backend(backend: &BackendConnection) -> Result<Vec<FuturesQuote>> {
    query::dispatch(backend, get_all, get_all_pg)
}
