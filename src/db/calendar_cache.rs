use anyhow::Result;
use rusqlite::{params, Connection};

/// A calendar event (economic data release or earnings).
#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub id: i64,
    pub date: String,
    pub name: String,
    pub impact: String, // "high", "medium", "low"
    pub previous: Option<String>,
    pub forecast: Option<String>,
    pub event_type: String, // "economic" or "earnings"
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

/// Delete events older than a given date (for cache cleanup).
pub fn delete_old_events(conn: &Connection, before_date: &str) -> Result<usize> {
    let rows = conn.execute(
        "DELETE FROM calendar_events WHERE date < ?1",
        params![before_date],
    )?;
    Ok(rows)
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
        
        upsert_event(&conn, "2026-03-07", "NFP", "high", None, None, "economic", None).unwrap();
        upsert_event(&conn, "2026-03-08", "PPI", "medium", None, None, "economic", None).unwrap();

        let high_events = get_events_by_impact(&conn, "2026-03-01", "high", 10).unwrap();
        assert_eq!(high_events.len(), 1);
        assert_eq!(high_events[0].name, "NFP");
    }

    #[test]
    fn test_delete_old_events() {
        let conn = setup_test_db();
        
        upsert_event(&conn, "2026-02-01", "Old Event", "low", None, None, "economic", None).unwrap();
        upsert_event(&conn, "2026-03-07", "New Event", "high", None, None, "economic", None).unwrap();

        let deleted = delete_old_events(&conn, "2026-03-01").unwrap();
        assert_eq!(deleted, 1);

        let events = get_upcoming_events(&conn, "2026-01-01", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "New Event");
    }
}
