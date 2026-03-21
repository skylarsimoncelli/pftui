use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

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

pub fn save_timeframe_backend(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| save_timeframe(conn, symbol, timeframe),
        |pool| save_timeframe_postgres(pool, symbol, timeframe),
    )
}

pub fn load_timeframe_backend(backend: &BackendConnection, symbol: &str) -> Result<Option<String>> {
    query::dispatch(
        backend,
        |conn| load_timeframe(conn, symbol),
        |pool| load_timeframe_postgres(pool, symbol),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chart_state (
                symbol TEXT PRIMARY KEY,
                timeframe TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn save_timeframe_postgres(pool: &PgPool, symbol: &str, timeframe: &str) -> Result<()> {
    ensure_table_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO chart_state (symbol, timeframe, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT(symbol) DO UPDATE SET
               timeframe = EXCLUDED.timeframe,
               updated_at = NOW()",
        )
        .bind(symbol.to_uppercase())
        .bind(timeframe)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn load_timeframe_postgres(pool: &PgPool, symbol: &str) -> Result<Option<String>> {
    ensure_table_postgres(pool)?;
    let row: Option<(String,)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as("SELECT timeframe FROM chart_state WHERE symbol = $1")
            .bind(symbol.to_uppercase())
            .fetch_optional(pool)
            .await
    })?;
    Ok(row.map(|r| r.0))
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
