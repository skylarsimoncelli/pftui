use anyhow::Result;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, PartialEq)]
pub struct MacroEvent {
    pub series_id: String,
    pub event_date: String,
    pub expected: Decimal,
    pub actual: Decimal,
    pub surprise_pct: Decimal,
    pub created_at: String,
}

pub fn insert_event(conn: &Connection, event: &MacroEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO macro_events (series_id, event_date, expected, actual, surprise_pct, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(series_id, event_date) DO UPDATE SET
           expected = excluded.expected,
           actual = excluded.actual,
           surprise_pct = excluded.surprise_pct,
           created_at = excluded.created_at",
        params![
            event.series_id,
            event.event_date,
            event.expected.to_string(),
            event.actual.to_string(),
            event.surprise_pct.to_string(),
            event.created_at,
        ],
    )?;
    Ok(())
}

pub fn insert_event_backend(backend: &BackendConnection, event: &MacroEvent) -> Result<()> {
    query::dispatch(
        backend,
        |conn| insert_event(conn, event),
        |pool| insert_event_postgres(pool, event),
    )
}

pub fn list_recent(conn: &Connection, limit: usize) -> Result<Vec<MacroEvent>> {
    let mut stmt = conn.prepare(
        "SELECT series_id, event_date, expected, actual, surprise_pct, created_at
         FROM macro_events
         ORDER BY event_date DESC, created_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(MacroEvent {
            series_id: row.get(0)?,
            event_date: row.get(1)?,
            expected: row.get::<_, String>(2)?.parse().unwrap_or(Decimal::ZERO),
            actual: row.get::<_, String>(3)?.parse().unwrap_or(Decimal::ZERO),
            surprise_pct: row.get::<_, String>(4)?.parse().unwrap_or(Decimal::ZERO),
            created_at: row.get(5)?,
        })
    })?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

pub fn list_recent_backend(backend: &BackendConnection, limit: usize) -> Result<Vec<MacroEvent>> {
    query::dispatch(
        backend,
        |conn| list_recent(conn, limit),
        |pool| list_recent_postgres(pool, limit),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS macro_events (
                series_id TEXT NOT NULL,
                event_date TEXT NOT NULL,
                expected TEXT NOT NULL,
                actual TEXT NOT NULL,
                surprise_pct TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (series_id, event_date)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_macro_events_event_date ON macro_events(event_date)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn insert_event_postgres(pool: &PgPool, event: &MacroEvent) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO macro_events (series_id, event_date, expected, actual, surprise_pct, created_at)
             VALUES ($1, $2, $3, $4, $5, $6::timestamptz)
             ON CONFLICT (series_id, event_date) DO UPDATE SET
               expected = EXCLUDED.expected,
               actual = EXCLUDED.actual,
               surprise_pct = EXCLUDED.surprise_pct,
               created_at = EXCLUDED.created_at",
        )
        .bind(&event.series_id)
        .bind(&event.event_date)
        .bind(event.expected.to_string())
        .bind(event.actual.to_string())
        .bind(event.surprise_pct.to_string())
        .bind(&event.created_at)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_recent_postgres(pool: &PgPool, limit: usize) -> Result<Vec<MacroEvent>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<(String, String, String, String, String, String)> =
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT series_id, event_date, expected, actual, surprise_pct, created_at::text
                 FROM macro_events
                 ORDER BY event_date DESC, created_at DESC
                 LIMIT $1",
            )
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        })?;

    Ok(rows
        .into_iter()
        .map(
            |(series_id, event_date, expected, actual, surprise_pct, created_at)| MacroEvent {
                series_id,
                event_date,
                expected: expected.parse().unwrap_or(Decimal::ZERO),
                actual: actual.parse().unwrap_or(Decimal::ZERO),
                surprise_pct: surprise_pct.parse().unwrap_or(Decimal::ZERO),
                created_at,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn inserts_and_lists_recent_events() {
        let conn = crate::db::open_in_memory();
        let event = MacroEvent {
            series_id: "CPIAUCSL".to_string(),
            event_date: "2026-03-01".to_string(),
            expected: dec!(310),
            actual: dec!(314.5),
            surprise_pct: dec!(1.45),
            created_at: "2026-03-16T12:00:00Z".to_string(),
        };

        insert_event(&conn, &event).unwrap();
        let events = list_recent(&conn, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], event);
    }
}
