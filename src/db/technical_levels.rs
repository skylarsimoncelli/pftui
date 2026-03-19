use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Row as PgRow};

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A single stored market structure level (support, resistance, MA, swing, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TechnicalLevelRecord {
    pub id: Option<i64>,
    pub symbol: String,
    pub level_type: String,    // support, resistance, sma_20, sma_50, sma_200, swing_high, swing_low, range_52w_high, range_52w_low, bb_upper, bb_lower, gap_fill
    pub price: f64,
    pub strength: f64,         // 0.0-1.0 confidence
    pub source_method: String, // pivot, moving_average, swing, range, bollinger, gap
    pub timeframe: String,     // 1d
    pub notes: Option<String>,
    pub computed_at: String,
}

impl TechnicalLevelRecord {
    fn from_sqlite_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol: row.get(1)?,
            level_type: row.get(2)?,
            price: row.get(3)?,
            strength: row.get(4)?,
            source_method: row.get(5)?,
            timeframe: row.get(6)?,
            notes: row.get(7)?,
            computed_at: row.get(8)?,
        })
    }
}

const SELECT_COLUMNS: &str =
    "id, symbol, level_type, price, strength, source_method, timeframe, notes, computed_at";

const SELECT_COLUMNS_PG: &str =
    "id, symbol, level_type, price, strength, source_method, timeframe, notes, computed_at::TEXT";

// ---------------------------------------------------------------------------
// SQLite
// ---------------------------------------------------------------------------

pub fn upsert_levels(conn: &Connection, symbol: &str, levels: &[TechnicalLevelRecord]) -> Result<()> {
    // Delete previous levels for this symbol then insert fresh set
    conn.execute(
        "DELETE FROM technical_levels WHERE symbol = ?1",
        params![symbol],
    )?;
    let mut stmt = conn.prepare(
        "INSERT INTO technical_levels (symbol, level_type, price, strength, source_method, timeframe, notes, computed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for level in levels {
        stmt.execute(params![
            level.symbol,
            level.level_type,
            level.price,
            level.strength,
            level.source_method,
            level.timeframe,
            level.notes,
            level.computed_at,
        ])?;
    }
    Ok(())
}

pub fn get_levels_for_symbol(
    conn: &Connection,
    symbol: &str,
) -> Result<Vec<TechnicalLevelRecord>> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM technical_levels WHERE symbol = ?1 ORDER BY price ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![symbol], TechnicalLevelRecord::from_sqlite_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn list_all_levels(
    conn: &Connection,
    limit: Option<usize>,
) -> Result<Vec<TechnicalLevelRecord>> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM technical_levels ORDER BY symbol ASC, price ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], TechnicalLevelRecord::from_sqlite_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    if let Some(limit) = limit {
        out.truncate(limit);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Backend dispatch
// ---------------------------------------------------------------------------

pub fn upsert_levels_backend(
    backend: &BackendConnection,
    symbol: &str,
    levels: &[TechnicalLevelRecord],
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_levels(conn, symbol, levels),
        |pool| upsert_levels_postgres(pool, symbol, levels),
    )
}

pub fn get_levels_for_symbol_backend(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<Vec<TechnicalLevelRecord>> {
    query::dispatch(
        backend,
        |conn| get_levels_for_symbol(conn, symbol),
        |pool| get_levels_for_symbol_postgres(pool, symbol),
    )
}

pub fn list_all_levels_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
) -> Result<Vec<TechnicalLevelRecord>> {
    query::dispatch(
        backend,
        |conn| list_all_levels(conn, limit),
        |pool| list_all_levels_postgres(pool, limit),
    )
}

// ---------------------------------------------------------------------------
// PostgreSQL
// ---------------------------------------------------------------------------

fn row_to_record_pg(row: &sqlx::postgres::PgRow) -> TechnicalLevelRecord {
    TechnicalLevelRecord {
        id: row.get(0),
        symbol: row.get(1),
        level_type: row.get(2),
        price: row.get(3),
        strength: row.get(4),
        source_method: row.get(5),
        timeframe: row.get(6),
        notes: row.get(7),
        computed_at: row.get(8),
    }
}

fn upsert_levels_postgres(pool: &PgPool, symbol: &str, levels: &[TechnicalLevelRecord]) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM technical_levels WHERE symbol = $1")
            .bind(symbol)
            .execute(pool)
            .await?;

        for level in levels {
            sqlx::query(
                "INSERT INTO technical_levels (symbol, level_type, price, strength, source_method, timeframe, notes, computed_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8::TIMESTAMPTZ)",
            )
            .bind(&level.symbol)
            .bind(&level.level_type)
            .bind(level.price)
            .bind(level.strength)
            .bind(&level.source_method)
            .bind(&level.timeframe)
            .bind(&level.notes)
            .bind(&level.computed_at)
            .execute(pool)
            .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_levels_for_symbol_postgres(
    pool: &PgPool,
    symbol: &str,
) -> Result<Vec<TechnicalLevelRecord>> {
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS_PG} FROM technical_levels WHERE symbol = $1 ORDER BY price ASC"
        ))
        .bind(symbol)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.iter().map(row_to_record_pg).collect())
}

fn list_all_levels_postgres(
    pool: &PgPool,
    limit: Option<usize>,
) -> Result<Vec<TechnicalLevelRecord>> {
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {SELECT_COLUMNS_PG} FROM technical_levels ORDER BY symbol ASC, price ASC"
    ));
    if let Some(limit) = limit {
        qb.push(" LIMIT ").push_bind(limit as i64);
    }
    let rows = crate::db::pg_runtime::block_on(async { qb.build().fetch_all(pool).await })?;
    Ok(rows.iter().map(row_to_record_pg).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn sample_level(symbol: &str, level_type: &str, price: f64, strength: f64) -> TechnicalLevelRecord {
        TechnicalLevelRecord {
            id: None,
            symbol: symbol.to_string(),
            level_type: level_type.to_string(),
            price,
            strength,
            source_method: "pivot".to_string(),
            timeframe: "1d".to_string(),
            notes: None,
            computed_at: "2026-03-18T16:00:00Z".to_string(),
        }
    }

    #[test]
    fn upsert_replaces_previous_levels() {
        let conn = open_in_memory();
        let levels_v1 = vec![
            sample_level("AAPL", "support", 150.0, 0.8),
            sample_level("AAPL", "resistance", 180.0, 0.7),
        ];
        upsert_levels(&conn, "AAPL", &levels_v1).unwrap();
        assert_eq!(get_levels_for_symbol(&conn, "AAPL").unwrap().len(), 2);

        let levels_v2 = vec![sample_level("AAPL", "support", 155.0, 0.9)];
        upsert_levels(&conn, "AAPL", &levels_v2).unwrap();
        let result = get_levels_for_symbol(&conn, "AAPL").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].price, 155.0);
    }

    #[test]
    fn upsert_does_not_affect_other_symbols() {
        let conn = open_in_memory();
        upsert_levels(&conn, "AAPL", &[sample_level("AAPL", "support", 150.0, 0.8)]).unwrap();
        upsert_levels(&conn, "BTC", &[sample_level("BTC", "resistance", 100000.0, 0.9)]).unwrap();

        // Replace AAPL levels
        upsert_levels(&conn, "AAPL", &[sample_level("AAPL", "sma_200", 160.0, 1.0)]).unwrap();

        assert_eq!(get_levels_for_symbol(&conn, "AAPL").unwrap().len(), 1);
        assert_eq!(get_levels_for_symbol(&conn, "BTC").unwrap().len(), 1);
    }

    #[test]
    fn list_all_returns_sorted_by_symbol_then_price() {
        let conn = open_in_memory();
        upsert_levels(
            &conn,
            "BTC",
            &[
                sample_level("BTC", "support", 80000.0, 0.7),
                sample_level("BTC", "resistance", 100000.0, 0.8),
            ],
        )
        .unwrap();
        upsert_levels(
            &conn,
            "AAPL",
            &[sample_level("AAPL", "sma_50", 170.0, 1.0)],
        )
        .unwrap();

        let all = list_all_levels(&conn, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].symbol, "AAPL");
        assert_eq!(all[1].symbol, "BTC");
        assert!(all[1].price < all[2].price);
    }

    #[test]
    fn list_all_respects_limit() {
        let conn = open_in_memory();
        upsert_levels(
            &conn,
            "AAPL",
            &[
                sample_level("AAPL", "support", 150.0, 0.8),
                sample_level("AAPL", "resistance", 180.0, 0.7),
                sample_level("AAPL", "sma_50", 165.0, 1.0),
            ],
        )
        .unwrap();
        assert_eq!(list_all_levels(&conn, Some(2)).unwrap().len(), 2);
    }
}
