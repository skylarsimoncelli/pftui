use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};

use crate::data::polymarket::PredictionMarket;

/// Upsert prediction market into cache.
pub fn upsert_prediction(conn: &Connection, market: &PredictionMarket) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO prediction_cache (market_id, question, outcome_yes_price, outcome_no_price, volume, category, end_date, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(market_id) DO UPDATE SET
           question = excluded.question,
           outcome_yes_price = excluded.outcome_yes_price,
           outcome_no_price = excluded.outcome_no_price,
           volume = excluded.volume,
           category = excluded.category,
           end_date = excluded.end_date,
           fetched_at = excluded.fetched_at",
        params![
            market.market_id,
            market.question,
            market.outcome_yes_price,
            market.outcome_no_price,
            market.volume,
            market.category,
            market.end_date,
            now,
        ],
    )?;
    Ok(())
}

/// Retrieve all cached prediction markets.
pub fn get_all_predictions(conn: &Connection) -> Result<Vec<PredictionMarket>> {
    let mut stmt = conn.prepare(
        "SELECT market_id, question, outcome_yes_price, outcome_no_price, volume, category, end_date
         FROM prediction_cache
         ORDER BY CAST(volume AS REAL) DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(PredictionMarket {
            market_id: row.get(0)?,
            question: row.get(1)?,
            outcome_yes_price: row.get(2)?,
            outcome_no_price: row.get(3)?,
            volume: row.get(4)?,
            category: row.get(5)?,
            end_date: row.get(6)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Retrieve predictions filtered by category.
pub fn get_predictions_by_category(conn: &Connection, category: &str) -> Result<Vec<PredictionMarket>> {
    let mut stmt = conn.prepare(
        "SELECT market_id, question, outcome_yes_price, outcome_no_price, volume, category, end_date
         FROM prediction_cache
         WHERE category = ?1
         ORDER BY CAST(volume AS REAL) DESC",
    )?;

    let rows = stmt.query_map(params![category], |row| {
        Ok(PredictionMarket {
            market_id: row.get(0)?,
            question: row.get(1)?,
            outcome_yes_price: row.get(2)?,
            outcome_no_price: row.get(3)?,
            volume: row.get(4)?,
            category: row.get(5)?,
            end_date: row.get(6)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Delete all cached prediction markets.
#[allow(dead_code)]
pub fn clear_predictions(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM prediction_cache", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_upsert_and_get() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();

        let market = PredictionMarket {
            market_id: "12345".to_string(),
            question: "Will BTC hit $100k?".to_string(),
            outcome_yes_price: "0.65".to_string(),
            outcome_no_price: "0.35".to_string(),
            volume: "50000".to_string(),
            category: "Crypto".to_string(),
            end_date: "2026-12-31T00:00:00Z".to_string(),
        };

        upsert_prediction(&conn, &market).unwrap();

        let all = get_all_predictions(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].question, "Will BTC hit $100k?");
    }

    #[test]
    fn test_get_by_category() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();

        let m1 = PredictionMarket {
            market_id: "1".to_string(),
            question: "Crypto Q1".to_string(),
            outcome_yes_price: "0.5".to_string(),
            outcome_no_price: "0.5".to_string(),
            volume: "1000".to_string(),
            category: "Crypto".to_string(),
            end_date: "2026-06-01T00:00:00Z".to_string(),
        };
        let m2 = PredictionMarket {
            market_id: "2".to_string(),
            question: "Politics Q2".to_string(),
            outcome_yes_price: "0.4".to_string(),
            outcome_no_price: "0.6".to_string(),
            volume: "2000".to_string(),
            category: "Politics".to_string(),
            end_date: "2026-07-01T00:00:00Z".to_string(),
        };

        upsert_prediction(&conn, &m1).unwrap();
        upsert_prediction(&conn, &m2).unwrap();

        let crypto = get_predictions_by_category(&conn, "Crypto").unwrap();
        assert_eq!(crypto.len(), 1);
        assert_eq!(crypto[0].question, "Crypto Q1");
    }
}
