use anyhow::Result;
use rusqlite::{params, Connection};

/// Save the selected chart timeframe for a symbol
pub fn save_timeframe(conn: &Connection, symbol: &str, timeframe: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chart_state (symbol, timeframe, updated_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(symbol) DO UPDATE SET timeframe = ?2, updated_at = datetime('now')",
        params![symbol.to_uppercase(), timeframe],
    )?;
    Ok(())
}

/// Load the saved chart timeframe for a symbol
pub fn load_timeframe(conn: &Connection, symbol: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT timeframe FROM chart_state WHERE symbol = ?1")?;
    let result = stmt.query_row([symbol.to_uppercase()], |row| row.get::<_, String>(0));
    match result {
        Ok(tf) => Ok(Some(tf)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_timeframe() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();

        save_timeframe(&conn, "AAPL", "3m").unwrap();
        let loaded = load_timeframe(&conn, "AAPL").unwrap();
        assert_eq!(loaded, Some("3m".to_string()));
    }

    #[test]
    fn test_load_missing_timeframe() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();

        let loaded = load_timeframe(&conn, "MISSING").unwrap();
        assert_eq!(loaded, None);
    }

    #[test]
    fn test_update_timeframe() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();

        save_timeframe(&conn, "AAPL", "1m").unwrap();
        save_timeframe(&conn, "AAPL", "1y").unwrap();
        let loaded = load_timeframe(&conn, "AAPL").unwrap();
        assert_eq!(loaded, Some("1y".to_string()));
    }
}
