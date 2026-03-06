use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rusqlite::Connection;
use std::str::FromStr;

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

/// Retrieve a cached FX rate if it's fresh (less than 15 minutes old).
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
