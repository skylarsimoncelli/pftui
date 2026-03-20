use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct EconomicDataEntry {
    pub indicator: String,
    pub value: Decimal,
    pub previous: Option<Decimal>,
    pub change: Option<Decimal>,
    pub source_url: String,
    pub source: String,
    pub confidence: String,
    pub fetched_at: String,
}

pub fn upsert_entry(conn: &Connection, entry: &EconomicDataEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO economic_data (indicator, value, previous, change, source_url, source, confidence, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(indicator) DO UPDATE SET
            value = excluded.value,
            previous = excluded.previous,
            change = excluded.change,
            source_url = excluded.source_url,
            source = excluded.source,
            confidence = excluded.confidence,
            fetched_at = excluded.fetched_at",
        params![
            entry.indicator,
            entry.value.to_string(),
            entry.previous.map(|v| v.to_string()),
            entry.change.map(|v| v.to_string()),
            entry.source_url,
            entry.source,
            entry.confidence,
            entry.fetched_at
        ],
    )?;
    Ok(())
}

pub fn upsert_entry_backend(backend: &BackendConnection, entry: &EconomicDataEntry) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_entry(conn, entry),
        |pool| upsert_entry_postgres(pool, entry),
    )
}

pub fn get_all(conn: &Connection) -> Result<Vec<EconomicDataEntry>> {
    let mut stmt = conn.prepare(
        "SELECT indicator, value, previous, change, source_url, source, confidence, fetched_at
         FROM economic_data
         ORDER BY indicator",
    )?;
    let rows = stmt.query_map([], |row| {
        let value: String = row.get(1)?;
        let previous: Option<String> = row.get(2)?;
        let change: Option<String> = row.get(3)?;
        Ok(EconomicDataEntry {
            indicator: row.get(0)?,
            value: value.parse().unwrap_or(Decimal::ZERO),
            previous: previous.and_then(|v| v.parse().ok()),
            change: change.and_then(|v| v.parse().ok()),
            source_url: row.get(4)?,
            source: row.get(5)?,
            confidence: row.get(6)?,
            fetched_at: row.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_all_backend(backend: &BackendConnection) -> Result<Vec<EconomicDataEntry>> {
    query::dispatch(backend, get_all, get_all_postgres)
}

fn get_all_postgres(pool: &PgPool) -> Result<Vec<EconomicDataEntry>> {
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query_as::<_, (String, String, Option<String>, Option<String>, String, String, String, String)>(
            "SELECT indicator, value, previous, change, source_url,
                    COALESCE(source, 'unknown'), COALESCE(confidence, 'medium'), fetched_at
             FROM economic_data
             ORDER BY indicator",
        )
        .fetch_all(pool)
        .await
    })?;

    Ok(rows
        .into_iter()
        .map(|(indicator, value, previous, change, source_url, source, confidence, fetched_at)| EconomicDataEntry {
            indicator,
            value: value.parse().unwrap_or(Decimal::ZERO),
            previous: previous.and_then(|v| v.parse().ok()),
            change: change.and_then(|v| v.parse().ok()),
            source_url,
            source,
            confidence,
            fetched_at,
        })
        .collect())
}

fn upsert_entry_postgres(pool: &PgPool, entry: &EconomicDataEntry) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO economic_data (indicator, value, previous, change, source_url, source, confidence, fetched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (indicator) DO UPDATE SET
               value = EXCLUDED.value,
               previous = EXCLUDED.previous,
               change = EXCLUDED.change,
               source_url = EXCLUDED.source_url,
               source = EXCLUDED.source,
               confidence = EXCLUDED.confidence,
               fetched_at = EXCLUDED.fetched_at",
        )
        .bind(&entry.indicator)
        .bind(entry.value.to_string())
        .bind(entry.previous.map(|v| v.to_string()))
        .bind(entry.change.map(|v| v.to_string()))
        .bind(&entry.source_url)
        .bind(&entry.source)
        .bind(&entry.confidence)
        .bind(&entry.fetched_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
