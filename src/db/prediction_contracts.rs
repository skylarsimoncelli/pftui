use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A prediction market contract from an external exchange (Polymarket, Kalshi, etc.).
/// Richer than predictions_cache: includes exchange, event grouping, liquidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionContract {
    pub contract_id: String,
    pub exchange: String,
    pub event_id: String,
    pub event_title: String,
    pub question: String,
    pub category: String,
    pub last_price: f64,     // 0.0-1.0 implied probability
    pub volume_24h: f64,
    pub liquidity: f64,
    pub end_date: Option<String>, // ISO8601 when contract resolves
    pub updated_at: i64,
}

/// Insert or update prediction market contracts.
pub fn upsert_contracts(conn: &Connection, contracts: &[PredictionContract]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO prediction_market_contracts
         (contract_id, exchange, event_id, event_title, question, category,
          last_price, volume_24h, liquidity, end_date, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )?;

    for c in contracts {
        stmt.execute(params![
            c.contract_id,
            c.exchange,
            c.event_id,
            c.event_title,
            c.question,
            c.category,
            c.last_price,
            c.volume_24h,
            c.liquidity,
            c.end_date,
            c.updated_at,
        ])?;
    }

    Ok(())
}

pub fn upsert_contracts_backend(
    backend: &BackendConnection,
    contracts: &[PredictionContract],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_contracts(conn, contracts),
        |pool| upsert_contracts_postgres(pool, contracts),
    )
}

/// Get prediction market contracts, ordered by volume descending.
/// Optional category and search filters.
pub fn get_contracts(
    conn: &Connection,
    category: Option<&str>,
    search: Option<&str>,
    limit: usize,
) -> Result<Vec<PredictionContract>> {
    // Build query dynamically based on filters
    let mut sql = String::from(
        "SELECT contract_id, exchange, event_id, event_title, question, category,
                last_price, volume_24h, liquidity, end_date, updated_at
         FROM prediction_market_contracts WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(cat) = category {
        sql.push_str(" AND category = ?");
        param_values.push(Box::new(cat.to_string()));
    }
    if let Some(q) = search {
        sql.push_str(" AND (question LIKE ? OR event_title LIKE ?)");
        let pattern = format!("%{}%", q);
        param_values.push(Box::new(pattern.clone()));
        param_values.push(Box::new(pattern));
    }

    sql.push_str(" ORDER BY volume_24h DESC LIMIT ?");
    param_values.push(Box::new(limit as i64));

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let contracts = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(PredictionContract {
                contract_id: row.get(0)?,
                exchange: row.get(1)?,
                event_id: row.get(2)?,
                event_title: row.get(3)?,
                question: row.get(4)?,
                category: row.get(5)?,
                last_price: row.get(6)?,
                volume_24h: row.get(7)?,
                liquidity: row.get(8)?,
                end_date: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(contracts)
}

pub fn get_contracts_backend(
    backend: &BackendConnection,
    category: Option<&str>,
    search: Option<&str>,
    limit: usize,
) -> Result<Vec<PredictionContract>> {
    // Clone filter strings so they can move into the closures
    let cat_owned = category.map(|s| s.to_string());
    let search_owned = search.map(|s| s.to_string());
    let cat2 = cat_owned.clone();
    let search2 = search_owned.clone();
    query::dispatch(
        backend,
        move |conn| get_contracts(conn, cat_owned.as_deref(), search_owned.as_deref(), limit),
        move |pool| get_contracts_postgres(pool, cat2.as_deref(), search2.as_deref(), limit),
    )
}

/// Count total contracts in the table.
#[allow(dead_code)] // Infrastructure for F55.5+ (calibration, analytics integration)
pub fn count_contracts(conn: &Connection) -> Result<usize> {
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM prediction_market_contracts", [], |r| {
            r.get(0)
        })?;
    Ok(count as usize)
}

/// Get the most recent update timestamp.
pub fn get_last_update(conn: &Connection) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT MAX(updated_at) FROM prediction_market_contracts")?;
    let ts: Option<i64> = stmt.query_row([], |row| row.get(0)).ok().flatten();
    Ok(ts)
}

pub fn get_last_update_backend(backend: &BackendConnection) -> Result<Option<i64>> {
    query::dispatch(backend, get_last_update, get_last_update_postgres)
}

/// Get unique categories with contract counts.
#[allow(dead_code)] // Infrastructure for F55.5+ (calibration, analytics integration)
pub fn get_category_counts(conn: &Connection) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn.prepare(
        "SELECT category, COUNT(*) FROM prediction_market_contracts GROUP BY category ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// --- Postgres implementations ---

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS prediction_market_contracts (
                contract_id TEXT PRIMARY KEY,
                exchange TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_title TEXT NOT NULL,
                question TEXT NOT NULL,
                category TEXT NOT NULL,
                last_price DOUBLE PRECISION NOT NULL,
                volume_24h DOUBLE PRECISION NOT NULL,
                liquidity DOUBLE PRECISION NOT NULL,
                end_date TEXT,
                updated_at BIGINT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_contracts_postgres(pool: &PgPool, contracts: &[PredictionContract]) -> Result<()> {
    ensure_table_postgres(pool)?;
    if contracts.is_empty() {
        return Ok(());
    }
    crate::db::pg_runtime::block_on(async {
        let mut tx = pool.begin().await?;
        for c in contracts {
            sqlx::query(
                "INSERT INTO prediction_market_contracts
                 (contract_id, exchange, event_id, event_title, question, category,
                  last_price, volume_24h, liquidity, end_date, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT(contract_id) DO UPDATE SET
                   exchange = EXCLUDED.exchange,
                   event_id = EXCLUDED.event_id,
                   event_title = EXCLUDED.event_title,
                   question = EXCLUDED.question,
                   category = EXCLUDED.category,
                   last_price = EXCLUDED.last_price,
                   volume_24h = EXCLUDED.volume_24h,
                   liquidity = EXCLUDED.liquidity,
                   end_date = EXCLUDED.end_date,
                   updated_at = EXCLUDED.updated_at",
            )
            .bind(&c.contract_id)
            .bind(&c.exchange)
            .bind(&c.event_id)
            .bind(&c.event_title)
            .bind(&c.question)
            .bind(&c.category)
            .bind(c.last_price)
            .bind(c.volume_24h)
            .bind(c.liquidity)
            .bind(&c.end_date)
            .bind(c.updated_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_contracts_postgres(
    pool: &PgPool,
    category: Option<&str>,
    search: Option<&str>,
    limit: usize,
) -> Result<Vec<PredictionContract>> {
    ensure_table_postgres(pool)?;
    type Row = (String, String, String, String, String, String, f64, f64, f64, Option<String>, i64);
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        // Use a simple query for the common case (no filters)
        if category.is_none() && search.is_none() {
            return sqlx::query_as(
                "SELECT contract_id, exchange, event_id, event_title, question, category,
                        last_price, volume_24h, liquidity, end_date, updated_at
                 FROM prediction_market_contracts
                 ORDER BY volume_24h DESC LIMIT $1",
            )
            .bind(limit as i64)
            .fetch_all(pool)
            .await;
        }

        // Build dynamic query for filtered cases
        let mut sql = String::from(
            "SELECT contract_id, exchange, event_id, event_title, question, category,
                    last_price, volume_24h, liquidity, end_date, updated_at
             FROM prediction_market_contracts WHERE 1=1",
        );
        let mut param_idx = 1i32;

        if category.is_some() {
            sql.push_str(&format!(" AND category = ${}", param_idx));
            param_idx += 1;
        }
        if search.is_some() {
            sql.push_str(&format!(
                " AND (question ILIKE ${p} OR event_title ILIKE ${p})",
                p = param_idx
            ));
            param_idx += 1;
        }
        sql.push_str(&format!(" ORDER BY volume_24h DESC LIMIT ${}", param_idx));

        let mut q = sqlx::query_as::<_, Row>(&sql);
        if let Some(cat) = category {
            q = q.bind(cat.to_string());
        }
        if let Some(s) = search {
            q = q.bind(format!("%{}%", s));
        }
        q = q.bind(limit as i64);
        q.fetch_all(pool).await
    })?;

    Ok(rows
        .into_iter()
        .map(|r| PredictionContract {
            contract_id: r.0,
            exchange: r.1,
            event_id: r.2,
            event_title: r.3,
            question: r.4,
            category: r.5,
            last_price: r.6,
            volume_24h: r.7,
            liquidity: r.8,
            end_date: r.9,
            updated_at: r.10,
        })
        .collect())
}

fn get_last_update_postgres(pool: &PgPool) -> Result<Option<i64>> {
    ensure_table_postgres(pool)?;
    let ts: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(updated_at), 0) FROM prediction_market_contracts",
        )
        .fetch_one(pool)
        .await
    })?;
    Ok(if ts == 0 { None } else { Some(ts) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS prediction_market_contracts (
                contract_id TEXT PRIMARY KEY,
                exchange TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_title TEXT NOT NULL,
                question TEXT NOT NULL,
                category TEXT NOT NULL,
                last_price REAL NOT NULL,
                volume_24h REAL NOT NULL,
                liquidity REAL NOT NULL,
                end_date TEXT,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pmc_category ON prediction_market_contracts(category);
            CREATE INDEX IF NOT EXISTS idx_pmc_volume ON prediction_market_contracts(volume_24h);",
        )
        .unwrap();
        conn
    }

    fn sample_contracts() -> Vec<PredictionContract> {
        vec![
            PredictionContract {
                contract_id: "0xabc123".into(),
                exchange: "polymarket".into(),
                event_id: "evt-fed-april".into(),
                event_title: "Fed decision in April?".into(),
                question: "Will the Fed cut rates by 25 bps in April?".into(),
                category: "economics".into(),
                last_price: 0.12,
                volume_24h: 324449.0,
                liquidity: 1130775.0,
                end_date: Some("2026-05-01T00:00:00Z".into()),
                updated_at: 1711670000,
            },
            PredictionContract {
                contract_id: "0xdef456".into(),
                exchange: "polymarket".into(),
                event_id: "evt-recession".into(),
                event_title: "US Recession in 2026?".into(),
                question: "Will the US enter a recession in 2026?".into(),
                category: "economics".into(),
                last_price: 0.58,
                volume_24h: 892000.0,
                liquidity: 2500000.0,
                end_date: Some("2026-12-31T00:00:00Z".into()),
                updated_at: 1711670000,
            },
            PredictionContract {
                contract_id: "0xghi789".into(),
                exchange: "polymarket".into(),
                event_id: "evt-iran".into(),
                event_title: "Iran strike by 2026?".into(),
                question: "Will the US or Israel strike Iran by end of 2026?".into(),
                category: "geopolitics".into(),
                last_price: 0.35,
                volume_24h: 450000.0,
                liquidity: 1800000.0,
                end_date: Some("2026-12-31T00:00:00Z".into()),
                updated_at: 1711670000,
            },
        ]
    }

    #[test]
    fn upsert_and_query_roundtrip() {
        let conn = setup();
        let contracts = sample_contracts();
        upsert_contracts(&conn, &contracts).unwrap();

        let result = get_contracts(&conn, None, None, 10).unwrap();
        assert_eq!(result.len(), 3);
        // Should be ordered by volume desc
        assert_eq!(result[0].contract_id, "0xdef456"); // highest vol
        assert_eq!(result[1].contract_id, "0xghi789");
        assert_eq!(result[2].contract_id, "0xabc123"); // lowest vol
    }

    #[test]
    fn filter_by_category() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();

        let result = get_contracts(&conn, Some("geopolitics"), None, 10).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].contract_id, "0xghi789");
    }

    #[test]
    fn filter_by_search() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();

        let result = get_contracts(&conn, None, Some("recession"), 10).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].contract_id, "0xdef456");
    }

    #[test]
    fn filter_by_category_and_search() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();

        let result = get_contracts(&conn, Some("economics"), Some("Fed"), 10).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].contract_id, "0xabc123");
    }

    #[test]
    fn upsert_updates_existing() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();

        // Update the first contract's price
        let mut updated = sample_contracts();
        updated[0].last_price = 0.25;
        updated[0].volume_24h = 500000.0;
        upsert_contracts(&conn, &updated[..1]).unwrap();

        let result = get_contracts(&conn, None, Some("Fed"), 10).unwrap();
        assert_eq!(result.len(), 1);
        assert!((result[0].last_price - 0.25).abs() < 0.001);
        assert!((result[0].volume_24h - 500000.0).abs() < 1.0);
    }

    #[test]
    fn last_update_empty_table() {
        let conn = setup();
        assert_eq!(get_last_update(&conn).unwrap(), None);
    }

    #[test]
    fn last_update_with_data() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();
        assert_eq!(get_last_update(&conn).unwrap(), Some(1711670000));
    }

    #[test]
    fn count_contracts_empty() {
        let conn = setup();
        assert_eq!(count_contracts(&conn).unwrap(), 0);
    }

    #[test]
    fn count_contracts_with_data() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();
        assert_eq!(count_contracts(&conn).unwrap(), 3);
    }

    #[test]
    fn category_counts() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();
        let counts = get_category_counts(&conn).unwrap();
        assert_eq!(counts.len(), 2);
        // economics has 2, geopolitics has 1
        assert_eq!(counts[0], ("economics".to_string(), 2));
        assert_eq!(counts[1], ("geopolitics".to_string(), 1));
    }

    #[test]
    fn limit_respected() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();
        let result = get_contracts(&conn, None, None, 2).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn contract_serializes_to_json() {
        let c = &sample_contracts()[0];
        let json = serde_json::to_value(c).unwrap();
        assert_eq!(json["exchange"], "polymarket");
        assert_eq!(json["last_price"], 0.12);
        assert_eq!(json["category"], "economics");
    }

    #[test]
    fn search_matches_event_title() {
        let conn = setup();
        upsert_contracts(&conn, &sample_contracts()).unwrap();
        // Search for "Iran" which appears in event_title, not question
        let result = get_contracts(&conn, None, Some("Iran"), 10).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].contract_id, "0xghi789");
    }
}
