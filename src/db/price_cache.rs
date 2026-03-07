use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};

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
