use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A calendar event (economic data release or earnings).
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub id: i64,
    pub date: String,
    pub name: String,
    pub impact: String, // "high", "medium", "low"
    pub previous: Option<String>,
    pub forecast: Option<String>,
    pub event_type: String,     // "economic" or "earnings"
    pub symbol: Option<String>, // for earnings events
    pub fetched_at: String,
}

/// Insert or update a calendar event.
#[allow(clippy::too_many_arguments)]
pub fn upsert_event(
    conn: &Connection,
    date: &str,
    name: &str,
    impact: &str,
    previous: Option<&str>,
    forecast: Option<&str>,
    event_type: &str,
    symbol: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO calendar_events (date, name, impact, previous, forecast, event_type, symbol)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(date, name) DO UPDATE SET
           impact = excluded.impact,
           previous = excluded.previous,
           forecast = excluded.forecast,
           fetched_at = datetime('now')",
        params![date, name, impact, previous, forecast, event_type, symbol],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_event_backend(
    backend: &BackendConnection,
    date: &str,
    name: &str,
    impact: &str,
    previous: Option<&str>,
    forecast: Option<&str>,
    event_type: &str,
    symbol: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            upsert_event(
                conn, date, name, impact, previous, forecast, event_type, symbol,
            )
        },
        |pool| {
            upsert_event_postgres(
                pool, date, name, impact, previous, forecast, event_type, symbol,
            )
        },
    )
}

/// Get upcoming events starting from a date, limited by count.
pub fn get_upcoming_events(
    conn: &Connection,
    from_date: &str,
    limit: usize,
) -> Result<Vec<CalendarEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, name, impact, previous, forecast, event_type, symbol, fetched_at
         FROM calendar_events
         WHERE date >= ?1
         ORDER BY date ASC, impact DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![from_date, limit as i64], |row| {
        Ok(CalendarEvent {
            id: row.get(0)?,
            date: row.get(1)?,
            name: row.get(2)?,
            impact: row.get(3)?,
            previous: row.get(4)?,
            forecast: row.get(5)?,
            event_type: row.get(6)?,
            symbol: row.get(7)?,
            fetched_at: row.get(8)?,
        })
    })?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

pub fn get_upcoming_events_backend(
    backend: &BackendConnection,
    from_date: &str,
    limit: usize,
) -> Result<Vec<CalendarEvent>> {
    query::dispatch(
        backend,
        |conn| get_upcoming_events(conn, from_date, limit),
        |pool| get_upcoming_events_postgres(pool, from_date, limit),
    )
}

/// Get events by impact level.
pub fn get_events_by_impact(
    conn: &Connection,
    from_date: &str,
    impact: &str,
    limit: usize,
) -> Result<Vec<CalendarEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, name, impact, previous, forecast, event_type, symbol, fetched_at
         FROM calendar_events
         WHERE date >= ?1 AND impact = ?2
         ORDER BY date ASC
         LIMIT ?3",
    )?;

    let rows = stmt.query_map(params![from_date, impact, limit as i64], |row| {
        Ok(CalendarEvent {
            id: row.get(0)?,
            date: row.get(1)?,
            name: row.get(2)?,
            impact: row.get(3)?,
            previous: row.get(4)?,
            forecast: row.get(5)?,
            event_type: row.get(6)?,
            symbol: row.get(7)?,
            fetched_at: row.get(8)?,
        })
    })?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

#[allow(dead_code)]
pub fn get_events_by_impact_backend(
    backend: &BackendConnection,
    from_date: &str,
    impact: &str,
    limit: usize,
) -> Result<Vec<CalendarEvent>> {
    query::dispatch(
        backend,
        |conn| get_events_by_impact(conn, from_date, impact, limit),
        |pool| get_events_by_impact_postgres(pool, from_date, impact, limit),
    )
}

/// Delete events older than a given date (for cache cleanup).
pub fn delete_old_events(conn: &Connection, before_date: &str) -> Result<usize> {
    let rows = conn.execute(
        "DELETE FROM calendar_events WHERE date < ?1",
        params![before_date],
    )?;
    Ok(rows)
}

pub fn delete_old_events_backend(backend: &BackendConnection, before_date: &str) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| delete_old_events(conn, before_date),
        |pool| delete_old_events_postgres(pool, before_date),
    )
}

type CalendarRow = (
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    String,
);

#[allow(clippy::too_many_arguments)]
fn upsert_event_postgres(
    pool: &PgPool,
    date: &str,
    name: &str,
    impact: &str,
    previous: Option<&str>,
    forecast: Option<&str>,
    event_type: &str,
    symbol: Option<&str>,
) -> Result<i64> {
    let id = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO calendar_events (date, name, impact, previous, forecast, event_type, symbol)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (date, name) DO UPDATE SET
               impact = EXCLUDED.impact,
               previous = EXCLUDED.previous,
               forecast = EXCLUDED.forecast,
               fetched_at = NOW()
             RETURNING id",
        )
        .bind(date)
        .bind(name)
        .bind(impact)
        .bind(previous)
        .bind(forecast)
        .bind(event_type)
        .bind(symbol)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn get_upcoming_events_postgres(
    pool: &PgPool,
    from_date: &str,
    limit: usize,
) -> Result<Vec<CalendarEvent>> {
    let rows: Vec<CalendarRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, date, name, impact, previous, forecast, event_type, symbol, fetched_at::text
             FROM calendar_events
             WHERE date >= $1
             ORDER BY date ASC, impact DESC
             LIMIT $2",
        )
        .bind(from_date)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| CalendarEvent {
            id: r.0,
            date: r.1,
            name: r.2,
            impact: r.3,
            previous: r.4,
            forecast: r.5,
            event_type: r.6,
            symbol: r.7,
            fetched_at: r.8,
        })
        .collect())
}

fn get_events_by_impact_postgres(
    pool: &PgPool,
    from_date: &str,
    impact: &str,
    limit: usize,
) -> Result<Vec<CalendarEvent>> {
    let rows: Vec<CalendarRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, date, name, impact, previous, forecast, event_type, symbol, fetched_at::text
             FROM calendar_events
             WHERE date >= $1 AND impact = $2
             ORDER BY date ASC
             LIMIT $3",
        )
        .bind(from_date)
        .bind(impact)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| CalendarEvent {
            id: r.0,
            date: r.1,
            name: r.2,
            impact: r.3,
            previous: r.4,
            forecast: r.5,
            event_type: r.6,
            symbol: r.7,
            fetched_at: r.8,
        })
        .collect())
}

fn delete_old_events_postgres(pool: &PgPool, before_date: &str) -> Result<usize> {
    let deleted = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM calendar_events WHERE date < $1")
            .bind(before_date)
            .execute(pool)
            .await
    })?;
    Ok(deleted.rows_affected() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_get_events() {
        let conn = setup_test_db();

        upsert_event(
            &conn,
            "2026-03-07",
            "Non-Farm Payrolls",
            "high",
            Some("143K"),
            Some("160K"),
            "economic",
            None,
        )
        .unwrap();

        let events = get_upcoming_events(&conn, "2026-03-01", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "Non-Farm Payrolls");
        assert_eq!(events[0].impact, "high");
    }

    #[test]
    fn test_get_by_impact() {
        let conn = setup_test_db();

        upsert_event(
            &conn,
            "2026-03-07",
            "NFP",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        upsert_event(
            &conn,
            "2026-03-08",
            "PPI",
            "medium",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();

        let high_events = get_events_by_impact(&conn, "2026-03-01", "high", 10).unwrap();
        assert_eq!(high_events.len(), 1);
        assert_eq!(high_events[0].name, "NFP");
    }

    #[test]
    fn test_delete_old_events() {
        let conn = setup_test_db();

        upsert_event(
            &conn,
            "2026-02-01",
            "Old Event",
            "low",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();
        upsert_event(
            &conn,
            "2026-03-07",
            "New Event",
            "high",
            None,
            None,
            "economic",
            None,
        )
        .unwrap();

        let deleted = delete_old_events(&conn, "2026-03-01").unwrap();
        assert_eq!(deleted, 1);

        let events = get_upcoming_events(&conn, "2026-01-01", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "New Event");
    }
}
