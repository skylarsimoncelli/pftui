use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Row as PgRow};
use std::collections::HashMap;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TechnicalSnapshotRecord {
    pub symbol: String,
    pub timeframe: String,
    pub rsi_14: Option<f64>,
    pub macd: Option<f64>,
    pub macd_signal: Option<f64>,
    pub macd_histogram: Option<f64>,
    pub sma_20: Option<f64>,
    pub sma_50: Option<f64>,
    pub sma_200: Option<f64>,
    pub bollinger_upper: Option<f64>,
    pub bollinger_middle: Option<f64>,
    pub bollinger_lower: Option<f64>,
    pub range_52w_low: Option<f64>,
    pub range_52w_high: Option<f64>,
    pub range_52w_position: Option<f64>,
    pub volume_avg_20: Option<f64>,
    pub volume_ratio_20: Option<f64>,
    pub volume_regime: Option<String>,
    pub above_sma_20: Option<bool>,
    pub above_sma_50: Option<bool>,
    pub above_sma_200: Option<bool>,
    /// Average True Range (14-period), computed from OHLCV when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_14: Option<f64>,
    /// ATR as percentage of current price (ATR / close * 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_ratio: Option<f64>,
    /// Whether current ATR exceeds 1.5x its 20-period average (volatility expansion).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range_expansion: Option<bool>,
    /// Day's range relative to ATR: (high - low) / ATR. >1.5 = wide bar, <0.5 = inside bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_range_ratio: Option<f64>,
    pub computed_at: String,
}

impl TechnicalSnapshotRecord {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            symbol: row.get(0)?,
            timeframe: row.get(1)?,
            rsi_14: row.get(2)?,
            macd: row.get(3)?,
            macd_signal: row.get(4)?,
            macd_histogram: row.get(5)?,
            sma_20: row.get(6)?,
            sma_50: row.get(7)?,
            sma_200: row.get(8)?,
            bollinger_upper: row.get(9)?,
            bollinger_middle: row.get(10)?,
            bollinger_lower: row.get(11)?,
            range_52w_low: row.get(12)?,
            range_52w_high: row.get(13)?,
            range_52w_position: row.get(14)?,
            volume_avg_20: row.get(15)?,
            volume_ratio_20: row.get(16)?,
            volume_regime: row.get(17)?,
            above_sma_20: row.get(18)?,
            above_sma_50: row.get(19)?,
            above_sma_200: row.get(20)?,
            atr_14: row.get(21)?,
            atr_ratio: row.get(22)?,
            range_expansion: row.get(23)?,
            day_range_ratio: row.get(24)?,
            computed_at: row.get(25)?,
        })
    }
}

const SELECT_COLUMNS: &str = "symbol, timeframe, rsi_14, macd, macd_signal, macd_histogram, \
    sma_20, sma_50, sma_200, bollinger_upper, bollinger_middle, bollinger_lower, \
    range_52w_low, range_52w_high, range_52w_position, volume_avg_20, volume_ratio_20, \
    volume_regime, above_sma_20, above_sma_50, above_sma_200, \
    atr_14, atr_ratio, range_expansion, day_range_ratio, computed_at";

const SELECT_COLUMNS_PG: &str = "symbol, timeframe, rsi_14, macd, macd_signal, macd_histogram, \
    sma_20, sma_50, sma_200, bollinger_upper, bollinger_middle, bollinger_lower, \
    range_52w_low, range_52w_high, range_52w_position, volume_avg_20, volume_ratio_20, \
    volume_regime, above_sma_20, above_sma_50, above_sma_200, \
    atr_14, atr_ratio, range_expansion, day_range_ratio, computed_at::TEXT";

pub fn insert_snapshot(conn: &Connection, row: &TechnicalSnapshotRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO technical_snapshots (
            symbol, timeframe, rsi_14, macd, macd_signal, macd_histogram,
            sma_20, sma_50, sma_200, bollinger_upper, bollinger_middle, bollinger_lower,
            range_52w_low, range_52w_high, range_52w_position,
            volume_avg_20, volume_ratio_20, volume_regime,
            above_sma_20, above_sma_50, above_sma_200,
            atr_14, atr_ratio, range_expansion, day_range_ratio, computed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
        params![
            row.symbol,
            row.timeframe,
            row.rsi_14,
            row.macd,
            row.macd_signal,
            row.macd_histogram,
            row.sma_20,
            row.sma_50,
            row.sma_200,
            row.bollinger_upper,
            row.bollinger_middle,
            row.bollinger_lower,
            row.range_52w_low,
            row.range_52w_high,
            row.range_52w_position,
            row.volume_avg_20,
            row.volume_ratio_20,
            row.volume_regime,
            row.above_sma_20,
            row.above_sma_50,
            row.above_sma_200,
            row.atr_14,
            row.atr_ratio,
            row.range_expansion,
            row.day_range_ratio,
            row.computed_at,
        ],
    )?;
    Ok(())
}

pub fn insert_snapshot_backend(
    backend: &BackendConnection,
    row: &TechnicalSnapshotRecord,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| insert_snapshot(conn, row),
        |pool| insert_snapshot_postgres(pool, row),
    )
}

pub fn get_latest_snapshot(
    conn: &Connection,
    symbol: &str,
    timeframe: &str,
) -> Result<Option<TechnicalSnapshotRecord>> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS}
         FROM technical_snapshots
         WHERE symbol = ?1 AND timeframe = ?2
         ORDER BY computed_at DESC
         LIMIT 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(
        params![symbol, timeframe],
        TechnicalSnapshotRecord::from_row,
    )?;
    Ok(rows.next().transpose()?)
}

pub fn get_latest_snapshot_backend(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
) -> Result<Option<TechnicalSnapshotRecord>> {
    query::dispatch(
        backend,
        |conn| get_latest_snapshot(conn, symbol, timeframe),
        |pool| get_latest_snapshot_postgres(pool, symbol, timeframe),
    )
}

pub fn list_latest_snapshots(
    conn: &Connection,
    timeframe: &str,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSnapshotRecord>> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS}
         FROM technical_snapshots t
         WHERE timeframe = ?1
           AND computed_at = (
             SELECT MAX(ts.computed_at)
             FROM technical_snapshots ts
             WHERE ts.symbol = t.symbol AND ts.timeframe = t.timeframe
           )
         ORDER BY symbol ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![timeframe], TechnicalSnapshotRecord::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    if let Some(limit) = limit {
        out.truncate(limit);
    }
    Ok(out)
}

pub fn list_latest_snapshots_backend(
    backend: &BackendConnection,
    timeframe: &str,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSnapshotRecord>> {
    query::dispatch(
        backend,
        |conn| list_latest_snapshots(conn, timeframe, limit),
        |pool| list_latest_snapshots_postgres(pool, timeframe, limit),
    )
}

/// Batch-fetch the latest snapshot per symbol for a set of symbols (SQLite).
///
/// Returns a map of symbol → latest snapshot. Symbols not found in the table
/// are omitted from the result. Uses `WHERE symbol IN (...)` to fetch all
/// symbols in one query instead of N individual queries.
pub fn get_latest_snapshots_batch(
    conn: &Connection,
    symbols: &[String],
    timeframe: &str,
) -> Result<HashMap<String, TechnicalSnapshotRecord>> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }
    // Build placeholder string: ?2, ?3, ?4, ... (timeframe is ?1)
    let placeholders: Vec<String> = (2..=symbols.len() + 1).map(|i| format!("?{}", i)).collect();
    let in_clause = placeholders.join(", ");
    let sql = format!(
        "SELECT {SELECT_COLUMNS}
         FROM technical_snapshots t
         WHERE t.timeframe = ?1
           AND t.symbol IN ({in_clause})
           AND t.computed_at = (
             SELECT MAX(ts.computed_at)
             FROM technical_snapshots ts
             WHERE ts.symbol = t.symbol AND ts.timeframe = t.timeframe
           )
         ORDER BY t.symbol ASC"
    );
    let mut stmt = conn.prepare(&sql)?;

    let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(timeframe.to_string())];
    for s in symbols {
        params_vec.push(Box::new(s.clone()));
    }
    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(params_refs.as_slice(), TechnicalSnapshotRecord::from_row)?;
    let mut result = HashMap::new();
    for row in rows {
        let record = row?;
        result.insert(record.symbol.clone(), record);
    }
    Ok(result)
}

fn get_latest_snapshots_batch_postgres(
    pool: &PgPool,
    symbols: &[String],
    timeframe: &str,
) -> Result<HashMap<String, TechnicalSnapshotRecord>> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<sqlx::postgres::PgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS_PG}
             FROM technical_snapshots t
             WHERE t.timeframe = $1
               AND t.symbol = ANY($2)
               AND t.computed_at = (
                 SELECT MAX(ts.computed_at)
                 FROM technical_snapshots ts
                 WHERE ts.symbol = t.symbol AND ts.timeframe = t.timeframe
               )
             ORDER BY t.symbol ASC"
        ))
        .bind(timeframe)
        .bind(symbols)
        .fetch_all(pool)
        .await
    })?;
    let mut result = HashMap::new();
    for row in &rows {
        let record = row_to_record_pg(row);
        result.insert(record.symbol.clone(), record);
    }
    Ok(result)
}

/// Batch-fetch the latest snapshot per symbol for a set of symbols.
///
/// Returns a map of symbol → latest snapshot. Symbols not found are omitted.
/// Uses a single query with `IN`/`ANY` instead of N individual queries.
pub fn get_latest_snapshots_batch_backend(
    backend: &BackendConnection,
    symbols: &[String],
    timeframe: &str,
) -> Result<HashMap<String, TechnicalSnapshotRecord>> {
    query::dispatch(
        backend,
        |conn| get_latest_snapshots_batch(conn, symbols, timeframe),
        |pool| get_latest_snapshots_batch_postgres(pool, symbols, timeframe),
    )
}

fn row_to_record_pg(row: &sqlx::postgres::PgRow) -> TechnicalSnapshotRecord {
    TechnicalSnapshotRecord {
        symbol: row.get(0),
        timeframe: row.get(1),
        rsi_14: row.get(2),
        macd: row.get(3),
        macd_signal: row.get(4),
        macd_histogram: row.get(5),
        sma_20: row.get(6),
        sma_50: row.get(7),
        sma_200: row.get(8),
        bollinger_upper: row.get(9),
        bollinger_middle: row.get(10),
        bollinger_lower: row.get(11),
        range_52w_low: row.get(12),
        range_52w_high: row.get(13),
        range_52w_position: row.get(14),
        volume_avg_20: row.get(15),
        volume_ratio_20: row.get(16),
        volume_regime: row.get(17),
        above_sma_20: row.get(18),
        above_sma_50: row.get(19),
        above_sma_200: row.get(20),
        atr_14: row.get(21),
        atr_ratio: row.get(22),
        range_expansion: row.get(23),
        day_range_ratio: row.get(24),
        computed_at: row.get(25),
    }
}

fn insert_snapshot_postgres(pool: &PgPool, row: &TechnicalSnapshotRecord) -> Result<()> {
    let query = "INSERT INTO technical_snapshots (
            symbol, timeframe, rsi_14, macd, macd_signal, macd_histogram,
            sma_20, sma_50, sma_200, bollinger_upper, bollinger_middle, bollinger_lower,
            range_52w_low, range_52w_high, range_52w_position,
            volume_avg_20, volume_ratio_20, volume_regime,
            above_sma_20, above_sma_50, above_sma_200,
            atr_14, atr_ratio, range_expansion, day_range_ratio, computed_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
            $13, $14, $15, $16, $17, $18, $19, $20, $21,
            $22, $23, $24, $25, $26::TIMESTAMPTZ
        )";
    crate::db::pg_runtime::block_on(async {
        sqlx::query(query)
            .bind(&row.symbol)
            .bind(&row.timeframe)
            .bind(row.rsi_14)
            .bind(row.macd)
            .bind(row.macd_signal)
            .bind(row.macd_histogram)
            .bind(row.sma_20)
            .bind(row.sma_50)
            .bind(row.sma_200)
            .bind(row.bollinger_upper)
            .bind(row.bollinger_middle)
            .bind(row.bollinger_lower)
            .bind(row.range_52w_low)
            .bind(row.range_52w_high)
            .bind(row.range_52w_position)
            .bind(row.volume_avg_20)
            .bind(row.volume_ratio_20)
            .bind(&row.volume_regime)
            .bind(row.above_sma_20)
            .bind(row.above_sma_50)
            .bind(row.above_sma_200)
            .bind(row.atr_14)
            .bind(row.atr_ratio)
            .bind(row.range_expansion)
            .bind(row.day_range_ratio)
            .bind(&row.computed_at)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_latest_snapshot_postgres(
    pool: &PgPool,
    symbol: &str,
    timeframe: &str,
) -> Result<Option<TechnicalSnapshotRecord>> {
    let row = crate::db::pg_runtime::block_on(async {
        sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS_PG}
             FROM technical_snapshots
             WHERE symbol = $1 AND timeframe = $2
             ORDER BY computed_at DESC
             LIMIT 1"
        ))
        .bind(symbol)
        .bind(timeframe)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|row| row_to_record_pg(&row)))
}

fn list_latest_snapshots_postgres(
    pool: &PgPool,
    timeframe: &str,
    limit: Option<usize>,
) -> Result<Vec<TechnicalSnapshotRecord>> {
    let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(format!(
        "SELECT {SELECT_COLUMNS_PG}
         FROM technical_snapshots t
         WHERE timeframe = "
    ));
    qb.push_bind(timeframe);
    qb.push(
        " AND computed_at = (
            SELECT MAX(ts.computed_at)
            FROM technical_snapshots ts
            WHERE ts.symbol = t.symbol AND ts.timeframe = t.timeframe
          )
          ORDER BY symbol ASC",
    );
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

    fn sample(symbol: &str, computed_at: &str, rsi: f64) -> TechnicalSnapshotRecord {
        TechnicalSnapshotRecord {
            symbol: symbol.to_string(),
            timeframe: "1d".to_string(),
            rsi_14: Some(rsi),
            macd: Some(1.2),
            macd_signal: Some(0.9),
            macd_histogram: Some(0.3),
            sma_20: Some(101.0),
            sma_50: Some(99.0),
            sma_200: Some(95.0),
            bollinger_upper: Some(110.0),
            bollinger_middle: Some(100.0),
            bollinger_lower: Some(90.0),
            range_52w_low: Some(80.0),
            range_52w_high: Some(120.0),
            range_52w_position: Some(50.0),
            volume_avg_20: Some(1_000.0),
            volume_ratio_20: Some(1.1),
            volume_regime: Some("normal".to_string()),
            above_sma_20: Some(true),
            above_sma_50: Some(true),
            above_sma_200: Some(true),
            atr_14: Some(3.5),
            atr_ratio: Some(3.5),
            range_expansion: Some(false),
            day_range_ratio: Some(1.0),
            computed_at: computed_at.to_string(),
        }
    }

    #[test]
    fn latest_snapshot_prefers_most_recent_row() {
        let conn = open_in_memory();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T10:00:00Z", 55.0)).unwrap();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T11:00:00Z", 62.0)).unwrap();

        let row = get_latest_snapshot(&conn, "AAPL", "1d").unwrap().unwrap();
        assert_eq!(row.rsi_14, Some(62.0));
        assert_eq!(row.computed_at, "2026-03-17T11:00:00Z");
    }

    #[test]
    fn list_latest_returns_one_row_per_symbol() {
        let conn = open_in_memory();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T10:00:00Z", 55.0)).unwrap();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T11:00:00Z", 62.0)).unwrap();
        insert_snapshot(&conn, &sample("BTC", "2026-03-17T09:00:00Z", 48.0)).unwrap();

        let rows = list_latest_snapshots(&conn, "1d", None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].symbol, "AAPL");
        assert_eq!(rows[0].rsi_14, Some(62.0));
        assert_eq!(rows[1].symbol, "BTC");
    }

    #[test]
    fn batch_empty_symbols_returns_empty() {
        let conn = open_in_memory();
        let result = get_latest_snapshots_batch(&conn, &[], "1d").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn batch_returns_latest_per_symbol() {
        let conn = open_in_memory();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T10:00:00Z", 55.0)).unwrap();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T11:00:00Z", 62.0)).unwrap();
        insert_snapshot(&conn, &sample("BTC", "2026-03-17T09:00:00Z", 48.0)).unwrap();
        insert_snapshot(&conn, &sample("TSLA", "2026-03-17T12:00:00Z", 70.0)).unwrap();

        let symbols = vec!["AAPL".to_string(), "BTC".to_string()];
        let result = get_latest_snapshots_batch(&conn, &symbols, "1d").unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result["AAPL"].rsi_14, Some(62.0), "should return latest AAPL snapshot");
        assert_eq!(result["BTC"].rsi_14, Some(48.0));
        assert!(!result.contains_key("TSLA"), "should not include unrequested symbols");
    }

    #[test]
    fn batch_missing_symbol_excluded() {
        let conn = open_in_memory();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T10:00:00Z", 55.0)).unwrap();

        let symbols = vec!["AAPL".to_string(), "MISSING".to_string()];
        let result = get_latest_snapshots_batch(&conn, &symbols, "1d").unwrap();

        assert_eq!(result.len(), 1);
        assert!(result.contains_key("AAPL"));
        assert!(!result.contains_key("MISSING"));
    }

    #[test]
    fn batch_respects_timeframe_filter() {
        let conn = open_in_memory();
        insert_snapshot(&conn, &sample("AAPL", "2026-03-17T10:00:00Z", 55.0)).unwrap();

        let symbols = vec!["AAPL".to_string()];
        // "1d" should find it
        let found = get_latest_snapshots_batch(&conn, &symbols, "1d").unwrap();
        assert_eq!(found.len(), 1);
        // "1w" should not find it (sample uses "1d")
        let not_found = get_latest_snapshots_batch(&conn, &symbols, "1w").unwrap();
        assert!(not_found.is_empty());
    }

    #[test]
    fn batch_single_symbol() {
        let conn = open_in_memory();
        insert_snapshot(&conn, &sample("BTC", "2026-03-17T09:00:00Z", 48.0)).unwrap();

        let symbols = vec!["BTC".to_string()];
        let result = get_latest_snapshots_batch(&conn, &symbols, "1d").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["BTC"].rsi_14, Some(48.0));
    }
}
