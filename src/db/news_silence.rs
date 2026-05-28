use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Datelike, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewsSilenceSample {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct NewsSilenceBaseline {
    pub topic: String,
    pub day_of_week: i64,
    pub samples: Vec<NewsSilenceSample>,
    pub status: String,
    pub changed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewsSilenceBaselineUpsert {
    pub topic: String,
    pub day_of_week: i64,
    pub samples: Vec<NewsSilenceSample>,
    pub median_count: f64,
    pub p30_count: f64,
    pub p80_count: f64,
    pub observed_count: i64,
    pub status: String,
    pub previous_status: Option<String>,
    pub changed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewsTopicDailyCount {
    pub topic: String,
    pub date: String,
    pub day_of_week: i64,
    pub count: i64,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS news_silence_baselines (
            topic TEXT NOT NULL,
            day_of_week INTEGER NOT NULL CHECK(day_of_week BETWEEN 1 AND 7),
            samples_json TEXT NOT NULL DEFAULT '[]',
            median_count REAL NOT NULL DEFAULT 0.0,
            p30_count REAL NOT NULL DEFAULT 0.0,
            p80_count REAL NOT NULL DEFAULT 0.0,
            observed_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'insufficient'
                CHECK(status IN ('insufficient','normal','silent','saturated')),
            previous_status TEXT,
            changed_at TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(topic, day_of_week)
        );
        CREATE INDEX IF NOT EXISTS idx_news_silence_baselines_status
            ON news_silence_baselines(status, updated_at);",
    )?;
    Ok(())
}

pub fn list_baselines(conn: &Connection) -> Result<Vec<NewsSilenceBaseline>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT topic, day_of_week, samples_json, median_count, p30_count, p80_count,
                observed_count, status, previous_status, changed_at, updated_at
         FROM news_silence_baselines
         ORDER BY topic ASC, day_of_week ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let samples_json: String = row.get(2)?;
        let samples =
            serde_json::from_str::<Vec<NewsSilenceSample>>(&samples_json).unwrap_or_default();
        Ok(NewsSilenceBaseline {
            topic: row.get(0)?,
            day_of_week: row.get(1)?,
            samples,
            status: row.get(7)?,
            changed_at: row.get(9)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn upsert_baselines(conn: &Connection, rows: &[NewsSilenceBaselineUpsert]) -> Result<usize> {
    ensure_table(conn)?;
    let mut inserted = 0usize;
    let mut stmt = conn.prepare(
        "INSERT INTO news_silence_baselines
         (topic, day_of_week, samples_json, median_count, p30_count, p80_count,
          observed_count, status, previous_status, changed_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, datetime('now'))
         ON CONFLICT(topic, day_of_week) DO UPDATE SET
            samples_json = excluded.samples_json,
            median_count = excluded.median_count,
            p30_count = excluded.p30_count,
            p80_count = excluded.p80_count,
            observed_count = excluded.observed_count,
            status = excluded.status,
            previous_status = excluded.previous_status,
            changed_at = excluded.changed_at,
            updated_at = datetime('now')",
    )?;
    for row in rows {
        let samples_json = serde_json::to_string(&row.samples)?;
        stmt.execute(params![
            row.topic,
            row.day_of_week,
            samples_json,
            row.median_count,
            row.p30_count,
            row.p80_count,
            row.observed_count,
            row.status,
            row.previous_status,
            row.changed_at,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

pub fn tier12_daily_counts(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<NewsTopicDailyCount>> {
    let mut stmt = conn.prepare(
        "SELECT topic, published_at
         FROM news_cache
         WHERE source_tier <= 2
           AND published_at >= ?1
           AND published_at < ?2",
    )?;
    let rows = stmt.query_map(params![start_ts, end_ts], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;

    let mut counts: BTreeMap<(String, String, i64), i64> = BTreeMap::new();
    for row in rows {
        let (topic, published_at) = row?;
        if let Some(date) = date_for_timestamp(published_at) {
            let date_str = date.format("%Y-%m-%d").to_string();
            let day_of_week = i64::from(date.weekday().number_from_monday());
            *counts.entry((topic, date_str, day_of_week)).or_default() += 1;
        }
    }

    Ok(counts
        .into_iter()
        .map(|((topic, date, day_of_week), count)| NewsTopicDailyCount {
            topic,
            date,
            day_of_week,
            count,
        })
        .collect())
}

pub fn ensure_table_backend(backend: &BackendConnection) -> Result<()> {
    query::dispatch(backend, ensure_table, ensure_table_postgres)
}

pub fn list_baselines_backend(backend: &BackendConnection) -> Result<Vec<NewsSilenceBaseline>> {
    query::dispatch(backend, list_baselines, list_baselines_postgres)
}

pub fn upsert_baselines_backend(
    backend: &BackendConnection,
    rows: &[NewsSilenceBaselineUpsert],
) -> Result<usize> {
    let sqlite_rows = rows.to_vec();
    let postgres_rows = rows.to_vec();
    query::dispatch(
        backend,
        move |conn| upsert_baselines(conn, &sqlite_rows),
        move |pool| upsert_baselines_postgres(pool, &postgres_rows),
    )
}

pub fn tier12_daily_counts_backend(
    backend: &BackendConnection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<NewsTopicDailyCount>> {
    query::dispatch(
        backend,
        |conn| tier12_daily_counts(conn, start_ts, end_ts),
        |pool| tier12_daily_counts_postgres(pool, start_ts, end_ts),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS news_silence_baselines (
                topic TEXT NOT NULL,
                day_of_week INTEGER NOT NULL CHECK(day_of_week BETWEEN 1 AND 7),
                samples_json TEXT NOT NULL DEFAULT '[]',
                median_count DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                p30_count DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                p80_count DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                observed_count BIGINT NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'insufficient'
                    CHECK(status IN ('insufficient','normal','silent','saturated')),
                previous_status TEXT,
                changed_at TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY(topic, day_of_week)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_news_silence_baselines_status
             ON news_silence_baselines(status, updated_at)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_baselines_postgres(pool: &PgPool) -> Result<Vec<NewsSilenceBaseline>> {
    ensure_table_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT topic, day_of_week::BIGINT, samples_json, median_count, p30_count,
                    p80_count, observed_count::BIGINT, status, previous_status,
                    changed_at, updated_at::text
             FROM news_silence_baselines
             ORDER BY topic ASC, day_of_week ASC",
        )
        .fetch_all(pool)
        .await
    })?;

    rows.into_iter()
        .map(|row| {
            let samples_json: String = row.try_get(2)?;
            let samples =
                serde_json::from_str::<Vec<NewsSilenceSample>>(&samples_json).unwrap_or_default();
            Ok(NewsSilenceBaseline {
                topic: row.try_get(0)?,
                day_of_week: row.try_get(1)?,
                samples,
                status: row.try_get(7)?,
                changed_at: row.try_get(9)?,
            })
        })
        .collect()
}

fn upsert_baselines_postgres(pool: &PgPool, rows: &[NewsSilenceBaselineUpsert]) -> Result<usize> {
    ensure_table_postgres(pool)?;
    let inserted = crate::db::pg_runtime::block_on(async {
        let mut inserted = 0usize;
        for row in rows {
            let samples_json = serde_json::to_string(&row.samples)
                .map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
            sqlx::query(
                "INSERT INTO news_silence_baselines
                 (topic, day_of_week, samples_json, median_count, p30_count, p80_count,
                  observed_count, status, previous_status, changed_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())
                 ON CONFLICT(topic, day_of_week) DO UPDATE SET
                    samples_json = EXCLUDED.samples_json,
                    median_count = EXCLUDED.median_count,
                    p30_count = EXCLUDED.p30_count,
                    p80_count = EXCLUDED.p80_count,
                    observed_count = EXCLUDED.observed_count,
                    status = EXCLUDED.status,
                    previous_status = EXCLUDED.previous_status,
                    changed_at = EXCLUDED.changed_at,
                    updated_at = NOW()",
            )
            .bind(&row.topic)
            .bind(row.day_of_week)
            .bind(samples_json)
            .bind(row.median_count)
            .bind(row.p30_count)
            .bind(row.p80_count)
            .bind(row.observed_count)
            .bind(&row.status)
            .bind(&row.previous_status)
            .bind(&row.changed_at)
            .execute(pool)
            .await?;
            inserted += 1;
        }
        Ok::<usize, sqlx::Error>(inserted)
    })?;
    Ok(inserted)
}

fn tier12_daily_counts_postgres(
    pool: &PgPool,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<NewsTopicDailyCount>> {
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "SELECT topic, published_at
             FROM news_cache
             WHERE source_tier <= 2
               AND published_at >= $1
               AND published_at < $2",
        )
        .bind(start_ts)
        .bind(end_ts)
        .fetch_all(pool)
        .await
    })?;

    let mut counts: BTreeMap<(String, String, i64), i64> = BTreeMap::new();
    for row in rows {
        let topic: String = row.try_get(0)?;
        let published_at: i64 = row.try_get(1)?;
        if let Some(date) = date_for_timestamp(published_at) {
            let date_str = date.format("%Y-%m-%d").to_string();
            let day_of_week = i64::from(date.weekday().number_from_monday());
            *counts.entry((topic, date_str, day_of_week)).or_default() += 1;
        }
    }

    Ok(counts
        .into_iter()
        .map(|((topic, date, day_of_week), count)| NewsTopicDailyCount {
            topic,
            date,
            day_of_week,
            count,
        })
        .collect())
}

fn date_for_timestamp(timestamp: i64) -> Option<chrono::NaiveDate> {
    Utc.timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baselines_roundtrip_samples_and_counts() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        let rows = vec![NewsSilenceBaselineUpsert {
            topic: "fed-policy".to_string(),
            day_of_week: 4,
            samples: vec![
                NewsSilenceSample {
                    date: "2026-05-07".to_string(),
                    count: 4,
                },
                NewsSilenceSample {
                    date: "2026-05-14".to_string(),
                    count: 6,
                },
            ],
            median_count: 5.0,
            p30_count: 4.6,
            p80_count: 5.6,
            observed_count: 2,
            status: "silent".to_string(),
            previous_status: Some("normal".to_string()),
            changed_at: Some("2026-05-28".to_string()),
        }];

        assert_eq!(upsert_baselines(&conn, &rows).unwrap(), 1);
        let loaded = list_baselines(&conn).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].topic, "fed-policy");
        assert_eq!(loaded[0].samples, rows[0].samples);
        assert_eq!(loaded[0].status, "silent");
    }
}
