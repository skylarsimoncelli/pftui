use crate::data::bls::BlsDataPoint;
use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::str::FromStr;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Initialize BLS cache table.
pub fn init_bls_cache(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bls_cache (
            series_id TEXT NOT NULL,
            year INTEGER NOT NULL,
            period TEXT NOT NULL,
            value TEXT NOT NULL,
            date TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (series_id, year, period)
        )",
        [],
    )
    .context("Failed to create bls_cache table")?;

    // Index for querying by series + date range
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_bls_series_date ON bls_cache (series_id, date)",
        [],
    )
    .context("Failed to create bls_cache index")?;

    Ok(())
}

/// Insert or replace BLS data points.
pub fn upsert_bls_data(conn: &Connection, data: &[BlsDataPoint]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO bls_cache (series_id, year, period, value, date, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
    )?;

    for point in data {
        stmt.execute(params![
            point.series_id,
            point.year,
            point.period,
            point.value.to_string(),
            point.date.format("%Y-%m-%d").to_string(),
        ])?;
    }

    Ok(())
}

pub fn upsert_bls_data_backend(backend: &BackendConnection, data: &[BlsDataPoint]) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_bls_data(conn, data),
        |pool| upsert_bls_data_postgres(pool, data),
    )
}

/// Get cached BLS data for a series, optionally filtered by date range.
pub fn get_cached_bls_data(
    conn: &Connection,
    series_id: &str,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
) -> Result<Vec<BlsDataPoint>> {
    let mut sql = "SELECT series_id, year, period, value, date FROM bls_cache WHERE series_id = ?1"
        .to_string();

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(series_id.to_string())];

    if let Some(start) = start_date {
        sql.push_str(" AND date >= ?");
        params.push(Box::new(start.format("%Y-%m-%d").to_string()));
    }

    if let Some(end) = end_date {
        sql.push_str(" AND date <= ?");
        params.push(Box::new(end.format("%Y-%m-%d").to_string()));
    }

    sql.push_str(" ORDER BY date DESC");

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let value_str: String = row.get(3)?;
        let date_str: String = row.get(4)?;

        Ok(BlsDataPoint {
            series_id: row.get(0)?,
            year: row.get(1)?,
            period: row.get(2)?,
            value: Decimal::from_str(&value_str).unwrap_or_default(),
            date: NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap_or_default(),
        })
    })?;

    let mut data = Vec::new();
    for row in rows {
        data.push(row?);
    }

    Ok(data)
}

/// Get the latest data point for a series.
pub fn get_latest_bls_data(conn: &Connection, series_id: &str) -> Result<Option<BlsDataPoint>> {
    let mut stmt = conn.prepare(
        "SELECT series_id, year, period, value, date FROM bls_cache
         WHERE series_id = ?1
         ORDER BY date DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query(params![series_id])?;

    if let Some(row) = rows.next()? {
        let value_str: String = row.get(3)?;
        let date_str: String = row.get(4)?;

        Ok(Some(BlsDataPoint {
            series_id: row.get(0)?,
            year: row.get(1)?,
            period: row.get(2)?,
            value: Decimal::from_str(&value_str).unwrap_or_default(),
            date: NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap_or_default(),
        }))
    } else {
        Ok(None)
    }
}

pub fn get_latest_bls_data_backend(
    backend: &BackendConnection,
    series_id: &str,
) -> Result<Option<BlsDataPoint>> {
    query::dispatch(
        backend,
        |conn| get_latest_bls_data(conn, series_id),
        |pool| get_latest_bls_data_postgres(pool, series_id),
    )
}

/// Check if cached data for a series is fresh (updated within N days).
pub fn is_cache_fresh(conn: &Connection, series_id: &str, max_age_days: i64) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT updated_at FROM bls_cache
         WHERE series_id = ?1
         ORDER BY updated_at DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query(params![series_id])?;

    if let Some(row) = rows.next()? {
        let updated_at: String = row.get(0)?;
        let updated = chrono::NaiveDateTime::parse_from_str(&updated_at, "%Y-%m-%d %H:%M:%S")
            .context("Failed to parse updated_at timestamp")?;

        let now = chrono::Utc::now().naive_utc();
        let age = now.signed_duration_since(updated);

        Ok(age.num_days() < max_age_days)
    } else {
        Ok(false)
    }
}

pub fn is_cache_fresh_backend(
    backend: &BackendConnection,
    series_id: &str,
    max_age_days: i64,
) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| is_cache_fresh(conn, series_id, max_age_days),
        |pool| is_cache_fresh_postgres(pool, series_id, max_age_days),
    )
}

fn upsert_bls_data_postgres(pool: &PgPool, data: &[BlsDataPoint]) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        for point in data {
            sqlx::query(
                "INSERT INTO bls_cache (series_id, year, period, value, date, updated_at)
                 VALUES ($1, $2, $3, $4, $5, NOW())
                 ON CONFLICT (series_id, year, period) DO UPDATE SET
                   value = EXCLUDED.value,
                   date = EXCLUDED.date,
                   updated_at = NOW()",
            )
            .bind(&point.series_id)
            .bind(point.year)
            .bind(&point.period)
            .bind(point.value.to_string())
            .bind(point.date.format("%Y-%m-%d").to_string())
            .execute(pool)
            .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn is_cache_fresh_postgres(pool: &PgPool, series_id: &str, max_age_days: i64) -> Result<bool> {
        let updated_at: Option<String> = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "SELECT updated_at::text
             FROM bls_cache
             WHERE series_id = $1
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(series_id)
        .fetch_optional(pool)
        .await
    })?;

    let Some(updated_raw) = updated_at else {
        return Ok(false);
    };

    let parsed = chrono::DateTime::parse_from_rfc3339(&updated_raw)
        .map(|d| d.naive_utc())
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&updated_raw, "%Y-%m-%d %H:%M:%S%.f"))
        .context("Failed to parse updated_at timestamp")?;

    let now = chrono::Utc::now().naive_utc();
    let age = now.signed_duration_since(parsed);
    Ok(age.num_days() < max_age_days)
}

fn get_latest_bls_data_postgres(pool: &PgPool, series_id: &str) -> Result<Option<BlsDataPoint>> {
        let row: Option<(String, i32, String, String, String)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT series_id, year, period, value, date
             FROM bls_cache
             WHERE series_id = $1
             ORDER BY date DESC
             LIMIT 1",
        )
        .bind(series_id)
        .fetch_optional(pool)
        .await
    })?;

    Ok(row.map(|(series_id, year, period, value_str, date_str)| BlsDataPoint {
        series_id,
        year,
        period,
        value: Decimal::from_str(&value_str).unwrap_or_default(),
        date: NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_bls_cache(&conn).unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get() {
        let conn = setup_test_db();

        let data = vec![
            BlsDataPoint {
                series_id: "CUUR0000SA0".to_string(),
                year: 2026,
                period: "M01".to_string(),
                value: dec!(308.417),
                date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            },
            BlsDataPoint {
                series_id: "CUUR0000SA0".to_string(),
                year: 2026,
                period: "M02".to_string(),
                value: dec!(309.123),
                date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            },
        ];

        upsert_bls_data(&conn, &data).unwrap();

        let cached = get_cached_bls_data(&conn, "CUUR0000SA0", None, None).unwrap();
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].value, dec!(309.123)); // DESC order
    }

    #[test]
    fn test_get_latest() {
        let conn = setup_test_db();

        let data = vec![
            BlsDataPoint {
                series_id: "LNS14000000".to_string(),
                year: 2026,
                period: "M01".to_string(),
                value: dec!(3.8),
                date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            },
            BlsDataPoint {
                series_id: "LNS14000000".to_string(),
                year: 2026,
                period: "M02".to_string(),
                value: dec!(3.9),
                date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            },
        ];

        upsert_bls_data(&conn, &data).unwrap();

        let latest = get_latest_bls_data(&conn, "LNS14000000").unwrap().unwrap();
        assert_eq!(latest.value, dec!(3.9));
        assert_eq!(latest.period, "M02");
    }

    #[test]
    fn test_is_cache_fresh() {
        let conn = setup_test_db();

        let data = vec![BlsDataPoint {
            series_id: "CES0000000001".to_string(),
            year: 2026,
            period: "M01".to_string(),
            value: dec!(158_900_000),
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        }];

        upsert_bls_data(&conn, &data).unwrap();

        // Should be fresh (just inserted)
        assert!(is_cache_fresh(&conn, "CES0000000001", 30).unwrap());

        // Non-existent series should not be fresh
        assert!(!is_cache_fresh(&conn, "NONEXISTENT", 30).unwrap());
    }

    #[test]
    fn test_date_range_filter() {
        let conn = setup_test_db();

        let data = vec![
            BlsDataPoint {
                series_id: "CUUR0000SA0".to_string(),
                year: 2025,
                period: "M12".to_string(),
                value: dec!(307.5),
                date: NaiveDate::from_ymd_opt(2025, 12, 1).unwrap(),
            },
            BlsDataPoint {
                series_id: "CUUR0000SA0".to_string(),
                year: 2026,
                period: "M01".to_string(),
                value: dec!(308.417),
                date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            },
            BlsDataPoint {
                series_id: "CUUR0000SA0".to_string(),
                year: 2026,
                period: "M02".to_string(),
                value: dec!(309.123),
                date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            },
        ];

        upsert_bls_data(&conn, &data).unwrap();

        // Filter to 2026 only
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();

        let filtered = get_cached_bls_data(&conn, "CUUR0000SA0", Some(start), Some(end)).unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|p| p.year == 2026));
    }
}
