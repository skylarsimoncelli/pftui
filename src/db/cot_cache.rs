//! SQLite cache for CFTC Commitments of Traders (COT) data.
//!
//! Stores weekly COT reports with positioning data by trader type.
//! Weekly refresh — data updates every Friday for the prior Tuesday.

use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A cached COT report.
#[derive(Debug, Clone)]
pub struct CotCacheEntry {
    pub cftc_code: String,
    pub report_date: String, // YYYY-MM-DD
    pub open_interest: i64,
    pub managed_money_long: i64,
    pub managed_money_short: i64,
    pub managed_money_net: i64,
    pub commercial_long: i64,
    pub commercial_short: i64,
    pub commercial_net: i64,
    pub fetched_at: String,
}

type CotRow = (String, String, i64, i64, i64, i64, i64, i64, i64, String);

/// Upsert a COT report into the cache.
///
/// Uses (cftc_code, report_date) as the primary key.
pub fn upsert_report(conn: &Connection, report: &CotCacheEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO cot_cache (
            cftc_code, report_date, open_interest,
            managed_money_long, managed_money_short, managed_money_net,
            commercial_long, commercial_short, commercial_net, fetched_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(cftc_code, report_date) DO UPDATE SET
            open_interest = excluded.open_interest,
            managed_money_long = excluded.managed_money_long,
            managed_money_short = excluded.managed_money_short,
            managed_money_net = excluded.managed_money_net,
            commercial_long = excluded.commercial_long,
            commercial_short = excluded.commercial_short,
            commercial_net = excluded.commercial_net,
            fetched_at = excluded.fetched_at",
        params![
            report.cftc_code,
            report.report_date,
            report.open_interest,
            report.managed_money_long,
            report.managed_money_short,
            report.managed_money_net,
            report.commercial_long,
            report.commercial_short,
            report.commercial_net,
            report.fetched_at,
        ],
    )?;
    Ok(())
}

pub fn upsert_report_backend(backend: &BackendConnection, report: &CotCacheEntry) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_report(conn, report),
        |pool| upsert_report_postgres(pool, report),
    )
}

/// Batch upsert multiple COT reports.
pub fn upsert_reports(conn: &Connection, reports: &[CotCacheEntry]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for report in reports {
        upsert_report(&tx, report)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn upsert_reports_backend(backend: &BackendConnection, reports: &[CotCacheEntry]) -> Result<()> {
    query::dispatch(
        backend,
        |conn| upsert_reports(conn, reports),
        |pool| upsert_reports_postgres(pool, reports),
    )
}

/// Get the most recent COT report for a contract.
pub fn get_latest(conn: &Connection, cftc_code: &str) -> Result<Option<CotCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT cftc_code, report_date, open_interest,
                managed_money_long, managed_money_short, managed_money_net,
                commercial_long, commercial_short, commercial_net, fetched_at
         FROM cot_cache
         WHERE cftc_code = ?1
         ORDER BY report_date DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query_map(params![cftc_code], |row| {
        Ok(CotCacheEntry {
            cftc_code: row.get(0)?,
            report_date: row.get(1)?,
            open_interest: row.get(2)?,
            managed_money_long: row.get(3)?,
            managed_money_short: row.get(4)?,
            managed_money_net: row.get(5)?,
            commercial_long: row.get(6)?,
            commercial_short: row.get(7)?,
            commercial_net: row.get(8)?,
            fetched_at: row.get(9)?,
        })
    })?;

    Ok(rows.next().transpose()?)
}

pub fn get_latest_backend(
    backend: &BackendConnection,
    cftc_code: &str,
) -> Result<Option<CotCacheEntry>> {
    query::dispatch(
        backend,
        |conn| get_latest(conn, cftc_code),
        |pool| get_latest_postgres(pool, cftc_code),
    )
}

/// Get historical COT reports for a contract (last N weeks).
pub fn get_history(
    conn: &Connection,
    cftc_code: &str,
    weeks: usize,
) -> Result<Vec<CotCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT cftc_code, report_date, open_interest,
                managed_money_long, managed_money_short, managed_money_net,
                commercial_long, commercial_short, commercial_net, fetched_at
         FROM cot_cache
         WHERE cftc_code = ?1
         ORDER BY report_date DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![cftc_code, weeks], |row| {
        Ok(CotCacheEntry {
            cftc_code: row.get(0)?,
            report_date: row.get(1)?,
            open_interest: row.get(2)?,
            managed_money_long: row.get(3)?,
            managed_money_short: row.get(4)?,
            managed_money_net: row.get(5)?,
            commercial_long: row.get(6)?,
            commercial_short: row.get(7)?,
            commercial_net: row.get(8)?,
            fetched_at: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

#[allow(dead_code)]
pub fn get_history_backend(
    backend: &BackendConnection,
    cftc_code: &str,
    weeks: usize,
) -> Result<Vec<CotCacheEntry>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, cftc_code, weeks),
        |pool| get_history_postgres(pool, cftc_code, weeks),
    )
}

/// Get all cached reports for all contracts.
pub fn get_all_latest(conn: &Connection) -> Result<Vec<CotCacheEntry>> {
    let mut stmt = conn.prepare(
        "SELECT cftc_code, report_date, open_interest,
                managed_money_long, managed_money_short, managed_money_net,
                commercial_long, commercial_short, commercial_net, fetched_at
         FROM cot_cache
         WHERE (cftc_code, report_date) IN (
             SELECT cftc_code, MAX(report_date)
             FROM cot_cache
             GROUP BY cftc_code
         )
         ORDER BY cftc_code",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CotCacheEntry {
            cftc_code: row.get(0)?,
            report_date: row.get(1)?,
            open_interest: row.get(2)?,
            managed_money_long: row.get(3)?,
            managed_money_short: row.get(4)?,
            managed_money_net: row.get(5)?,
            commercial_long: row.get(6)?,
            commercial_short: row.get(7)?,
            commercial_net: row.get(8)?,
            fetched_at: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_all_latest_backend(backend: &BackendConnection) -> Result<Vec<CotCacheEntry>> {
    query::dispatch(backend, get_all_latest, get_all_latest_postgres)
}

/// Delete all cached COT data older than N days.
#[allow(dead_code)]
pub fn delete_old_reports(conn: &Connection, days: i64) -> Result<usize> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

    let count = conn.execute(
        "DELETE FROM cot_cache WHERE report_date < ?1",
        params![cutoff_str],
    )?;

    Ok(count)
}

#[allow(dead_code)]
pub fn delete_old_reports_backend(backend: &BackendConnection, days: i64) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| delete_old_reports(conn, days),
        |pool| delete_old_reports_postgres(pool, days),
    )
}

fn upsert_report_postgres(pool: &PgPool, report: &CotCacheEntry) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO cot_cache (
                cftc_code, report_date, open_interest,
                managed_money_long, managed_money_short, managed_money_net,
                commercial_long, commercial_short, commercial_net, fetched_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::timestamptz)
            ON CONFLICT (cftc_code, report_date) DO UPDATE SET
                open_interest = EXCLUDED.open_interest,
                managed_money_long = EXCLUDED.managed_money_long,
                managed_money_short = EXCLUDED.managed_money_short,
                managed_money_net = EXCLUDED.managed_money_net,
                commercial_long = EXCLUDED.commercial_long,
                commercial_short = EXCLUDED.commercial_short,
                commercial_net = EXCLUDED.commercial_net,
                fetched_at = EXCLUDED.fetched_at",
        )
        .bind(&report.cftc_code)
        .bind(&report.report_date)
        .bind(report.open_interest)
        .bind(report.managed_money_long)
        .bind(report.managed_money_short)
        .bind(report.managed_money_net)
        .bind(report.commercial_long)
        .bind(report.commercial_short)
        .bind(report.commercial_net)
        .bind(&report.fetched_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn upsert_reports_postgres(pool: &PgPool, reports: &[CotCacheEntry]) -> Result<()> {
    for report in reports {
        upsert_report_postgres(pool, report)?;
    }
    Ok(())
}

fn get_latest_postgres(pool: &PgPool, cftc_code: &str) -> Result<Option<CotCacheEntry>> {
        let row: Option<CotRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT cftc_code, report_date, open_interest,
                    managed_money_long, managed_money_short, managed_money_net,
                    commercial_long, commercial_short, commercial_net, fetched_at::text
             FROM cot_cache
             WHERE cftc_code = $1
             ORDER BY report_date DESC
             LIMIT 1",
        )
        .bind(cftc_code)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(|r| CotCacheEntry {
        cftc_code: r.0,
        report_date: r.1,
        open_interest: r.2,
        managed_money_long: r.3,
        managed_money_short: r.4,
        managed_money_net: r.5,
        commercial_long: r.6,
        commercial_short: r.7,
        commercial_net: r.8,
        fetched_at: r.9,
    }))
}

fn get_history_postgres(pool: &PgPool, cftc_code: &str, weeks: usize) -> Result<Vec<CotCacheEntry>> {
        let rows: Vec<CotRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT cftc_code, report_date, open_interest,
                    managed_money_long, managed_money_short, managed_money_net,
                    commercial_long, commercial_short, commercial_net, fetched_at::text
             FROM cot_cache
             WHERE cftc_code = $1
             ORDER BY report_date DESC
             LIMIT $2",
        )
        .bind(cftc_code)
        .bind(weeks as i64)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| CotCacheEntry {
            cftc_code: r.0,
            report_date: r.1,
            open_interest: r.2,
            managed_money_long: r.3,
            managed_money_short: r.4,
            managed_money_net: r.5,
            commercial_long: r.6,
            commercial_short: r.7,
            commercial_net: r.8,
            fetched_at: r.9,
        })
        .collect())
}

fn get_all_latest_postgres(pool: &PgPool) -> Result<Vec<CotCacheEntry>> {
        let rows: Vec<CotRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT cftc_code, report_date, open_interest,
                    managed_money_long, managed_money_short, managed_money_net,
                    commercial_long, commercial_short, commercial_net, fetched_at::text
             FROM cot_cache c
             WHERE report_date = (
               SELECT MAX(c2.report_date) FROM cot_cache c2 WHERE c2.cftc_code = c.cftc_code
             )
             ORDER BY cftc_code",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| CotCacheEntry {
            cftc_code: r.0,
            report_date: r.1,
            open_interest: r.2,
            managed_money_long: r.3,
            managed_money_short: r.4,
            managed_money_net: r.5,
            commercial_long: r.6,
            commercial_short: r.7,
            commercial_net: r.8,
            fetched_at: r.9,
        })
        .collect())
}

fn delete_old_reports_postgres(pool: &PgPool, days: i64) -> Result<usize> {
        let deleted = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "DELETE FROM cot_cache
             WHERE report_date < TO_CHAR(NOW() - ($1 * INTERVAL '1 day'), 'YYYY-MM-DD')",
        )
        .bind(days)
        .execute(pool)
        .await
    })?;
    Ok(deleted.rows_affected() as usize)
}
