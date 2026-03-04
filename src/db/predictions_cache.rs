use anyhow::Result;
use rusqlite::{params, Connection};

use crate::data::predictions::{MarketCategory, PredictionMarket};

/// Ensure the predictions_cache table exists.
#[allow(dead_code)] // Used by schema migrations (already in schema.rs)
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS predictions_cache (
            id TEXT PRIMARY KEY,
            question TEXT NOT NULL,
            probability REAL NOT NULL,
            volume_24h REAL NOT NULL,
            category TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// Insert or replace cached prediction markets.
#[allow(dead_code)] // Infrastructure for F17.3+ (refresh --predictions, predictions CLI)
pub fn upsert_predictions(conn: &Connection, markets: &[PredictionMarket]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO predictions_cache
         (id, question, probability, volume_24h, category, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )?;

    for market in markets {
        let category_str = match market.category {
            MarketCategory::Crypto => "crypto",
            MarketCategory::Economics => "economics",
            MarketCategory::Geopolitics => "geopolitics",
            MarketCategory::AI => "ai",
            MarketCategory::Other => "other",
        };

        stmt.execute(params![
            market.id,
            market.question,
            market.probability,
            market.volume_24h,
            category_str,
            market.updated_at,
        ])?;
    }

    Ok(())
}

/// Get cached predictions, ordered by volume descending.
pub fn get_cached_predictions(conn: &Connection, limit: usize) -> Result<Vec<PredictionMarket>> {
    let mut stmt = conn.prepare(
        "SELECT id, question, probability, volume_24h, category, updated_at
         FROM predictions_cache
         ORDER BY volume_24h DESC
         LIMIT ?",
    )?;

    let markets = stmt
        .query_map(params![limit], |row| {
            let category_str: String = row.get(4)?;
            let category = match category_str.as_str() {
                "crypto" => MarketCategory::Crypto,
                "economics" => MarketCategory::Economics,
                "geopolitics" => MarketCategory::Geopolitics,
                "ai" => MarketCategory::AI,
                _ => MarketCategory::Other,
            };

            Ok(PredictionMarket {
                id: row.get(0)?,
                question: row.get(1)?,
                probability: row.get(2)?,
                volume_24h: row.get(3)?,
                category,
                updated_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(markets)
}

/// Get the most recent update timestamp in the cache.
#[allow(dead_code)] // Infrastructure for F17.3+ (refresh --predictions staleness check)
pub fn get_last_update(conn: &Connection) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT MAX(updated_at) FROM predictions_cache")?;
    let ts: Option<i64> = stmt.query_row([], |row| row.get(0)).ok().flatten();
    Ok(ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predictions_cache_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();

        let markets = vec![
            PredictionMarket {
                id: "test1".into(),
                question: "Will BTC reach $100k?".into(),
                probability: 0.45,
                volume_24h: 50000.0,
                category: MarketCategory::Crypto,
                updated_at: 1000000,
            },
            PredictionMarket {
                id: "test2".into(),
                question: "US recession 2026?".into(),
                probability: 0.22,
                volume_24h: 30000.0,
                category: MarketCategory::Economics,
                updated_at: 1000000,
            },
        ];

        upsert_predictions(&conn, &markets).unwrap();

        let cached = get_cached_predictions(&conn, 10).unwrap();
        assert_eq!(cached.len(), 2);
        // Should be ordered by volume desc
        assert_eq!(cached[0].id, "test1");
        assert_eq!(cached[1].id, "test2");

        let last_update = get_last_update(&conn).unwrap();
        assert_eq!(last_update, Some(1000000));
    }
}
