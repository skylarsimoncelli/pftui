use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionEntry {
    pub id: i64,
    pub symbol: String,
    pub score: i32,
    pub notes: Option<String>,
    pub recorded_at: String,
}

impl ConvictionEntry {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol: row.get(1)?,
            score: row.get(2)?,
            notes: row.get(3)?,
            recorded_at: row.get(4)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionChange {
    pub symbol: String,
    pub old_score: i32,
    pub new_score: i32,
    pub old_date: String,
    pub new_date: String,
    pub change_delta: i32,
}

pub fn set_conviction(
    conn: &Connection,
    symbol: &str,
    score: i32,
    notes: Option<&str>,
) -> Result<i64> {
    if !(-5..=5).contains(&score) {
        anyhow::bail!("Score must be between -5 and +5, got {}", score);
    }

    conn.execute(
        "INSERT INTO convictions (symbol, score, notes)
         VALUES (?, ?, ?)",
        params![symbol, score, notes],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_conviction_backend(
    backend: &BackendConnection,
    symbol: &str,
    score: i32,
    notes: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| set_conviction(conn, symbol, score, notes),
        |pool| set_conviction_postgres(pool, symbol, score, notes),
    )
}

pub fn list_current(conn: &Connection) -> Result<Vec<ConvictionEntry>> {
    let mut stmt = conn.prepare(
        "WITH latest AS (
             SELECT symbol, MAX(id) as max_id
             FROM convictions
             GROUP BY symbol
         )
         SELECT c.id, c.symbol, c.score, c.notes, c.recorded_at
         FROM convictions c
         INNER JOIN latest l ON c.symbol = l.symbol AND c.id = l.max_id
         ORDER BY ABS(c.score) DESC, c.symbol ASC",
    )?;

    let rows = stmt.query_map([], ConvictionEntry::from_row)?;
    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn list_current_backend(backend: &BackendConnection) -> Result<Vec<ConvictionEntry>> {
    query::dispatch(backend, list_current, list_current_postgres)
}

pub fn get_history(
    conn: &Connection,
    symbol: &str,
    limit: Option<usize>,
) -> Result<Vec<ConvictionEntry>> {
    let query = if let Some(lim) = limit {
        format!(
            "SELECT id, symbol, score, notes, recorded_at
             FROM convictions
             WHERE symbol = ?
             ORDER BY id DESC
             LIMIT {}",
            lim
        )
    } else {
        "SELECT id, symbol, score, notes, recorded_at
         FROM convictions
         WHERE symbol = ?
         ORDER BY id DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params![symbol], ConvictionEntry::from_row)?;
    let mut entries = Vec::new();
    for entry in rows {
        entries.push(entry?);
    }
    Ok(entries)
}

pub fn get_history_backend(
    backend: &BackendConnection,
    symbol: &str,
    limit: Option<usize>,
) -> Result<Vec<ConvictionEntry>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, symbol, limit),
        |pool| get_history_postgres(pool, symbol, limit),
    )
}

pub fn get_changes(conn: &Connection, days: usize) -> Result<Vec<ConvictionChange>> {
    let query = format!(
        "WITH recent AS (
             SELECT id, symbol, score, recorded_at
             FROM convictions
             WHERE recorded_at >= datetime('now', '-{} days')
         ),
         latest_per_symbol AS (
             SELECT symbol, MAX(id) as max_id
             FROM recent
             GROUP BY symbol
         ),
         current_scores AS (
             SELECT r.symbol, r.score as new_score, r.recorded_at as new_date, r.id as current_id
             FROM recent r
             INNER JOIN latest_per_symbol l ON r.symbol = l.symbol AND r.id = l.max_id
         ),
         prior_scores AS (
             SELECT c.symbol, c.score as old_score, c.recorded_at as old_date
             FROM convictions c
             INNER JOIN current_scores cs ON c.symbol = cs.symbol
             WHERE c.id < cs.current_id
             AND c.id = (
                 SELECT MAX(id)
                 FROM convictions
                 WHERE symbol = c.symbol AND id < cs.current_id
             )
         )
         SELECT cs.symbol, COALESCE(ps.old_score, 0), cs.new_score, 
                COALESCE(ps.old_date, ''), cs.new_date,
                cs.new_score - COALESCE(ps.old_score, 0) as delta
         FROM current_scores cs
         LEFT JOIN prior_scores ps ON cs.symbol = ps.symbol
         WHERE cs.new_score != COALESCE(ps.old_score, 0)
         ORDER BY ABS(cs.new_score - COALESCE(ps.old_score, 0)) DESC",
        days
    );

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok(ConvictionChange {
            symbol: row.get(0)?,
            old_score: row.get(1)?,
            new_score: row.get(2)?,
            old_date: row.get(3)?,
            new_date: row.get(4)?,
            change_delta: row.get(5)?,
        })
    })?;

    let mut changes = Vec::new();
    for change in rows {
        changes.push(change?);
    }
    Ok(changes)
}

pub fn get_changes_backend(backend: &BackendConnection, days: usize) -> Result<Vec<ConvictionChange>> {
    query::dispatch(
        backend,
        |conn| get_changes(conn, days),
        |pool| get_changes_postgres(pool, days),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS convictions (
                id BIGSERIAL PRIMARY KEY,
                symbol TEXT NOT NULL,
                score INTEGER NOT NULL,
                notes TEXT,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type ConvictionRow = (i64, String, i32, Option<String>, String);

fn to_conviction_entry(row: ConvictionRow) -> ConvictionEntry {
    ConvictionEntry {
        id: row.0,
        symbol: row.1,
        score: row.2,
        notes: row.3,
        recorded_at: row.4,
    }
}

fn set_conviction_postgres(
    pool: &PgPool,
    symbol: &str,
    score: i32,
    notes: Option<&str>,
) -> Result<i64> {
    if !(-5..=5).contains(&score) {
        anyhow::bail!("Score must be between -5 and +5, got {}", score);
    }
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO convictions (symbol, score, notes)
             VALUES ($1, $2, $3)
             RETURNING id",
        )
        .bind(symbol)
        .bind(score)
        .bind(notes)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_current_postgres(pool: &PgPool) -> Result<Vec<ConvictionEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<ConvictionRow> = runtime.block_on(async {
        sqlx::query_as(
            "WITH latest AS (
                 SELECT symbol, MAX(id) AS max_id
                 FROM convictions
                 GROUP BY symbol
             )
             SELECT c.id, c.symbol, c.score, c.notes, c.recorded_at::text
             FROM convictions c
             INNER JOIN latest l ON c.symbol = l.symbol AND c.id = l.max_id
             ORDER BY ABS(c.score) DESC, c.symbol ASC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(to_conviction_entry).collect())
}

fn get_history_postgres(
    pool: &PgPool,
    symbol: &str,
    limit: Option<usize>,
) -> Result<Vec<ConvictionEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<ConvictionRow> = runtime.block_on(async {
        if let Some(limit) = limit {
            sqlx::query_as(
                "SELECT id, symbol, score, notes, recorded_at::text
                 FROM convictions
                 WHERE symbol = $1
                 ORDER BY id DESC
                 LIMIT $2",
            )
            .bind(symbol)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT id, symbol, score, notes, recorded_at::text
                 FROM convictions
                 WHERE symbol = $1
                 ORDER BY id DESC",
            )
            .bind(symbol)
            .fetch_all(pool)
            .await
        }
    })?;
    Ok(rows.into_iter().map(to_conviction_entry).collect())
}

type ConvictionChangeRow = (String, i32, i32, String, String, i32);

fn get_changes_postgres(pool: &PgPool, days: usize) -> Result<Vec<ConvictionChange>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<ConvictionChangeRow> = runtime.block_on(async {
        sqlx::query_as(
            "WITH recent AS (
                 SELECT id, symbol, score, recorded_at
                 FROM convictions
                 WHERE recorded_at >= NOW() - ($1::int * INTERVAL '1 day')
             ),
             latest_per_symbol AS (
                 SELECT symbol, MAX(id) AS max_id
                 FROM recent
                 GROUP BY symbol
             ),
             current_scores AS (
                 SELECT r.symbol, r.score AS new_score, r.recorded_at AS new_date, r.id AS current_id
                 FROM recent r
                 INNER JOIN latest_per_symbol l ON r.symbol = l.symbol AND r.id = l.max_id
             ),
             prior_scores AS (
                 SELECT c.symbol, c.score AS old_score, c.recorded_at AS old_date
                 FROM convictions c
                 INNER JOIN current_scores cs ON c.symbol = cs.symbol
                 WHERE c.id < cs.current_id
                 AND c.id = (
                     SELECT MAX(id)
                     FROM convictions
                     WHERE symbol = c.symbol AND id < cs.current_id
                 )
             )
             SELECT
                 cs.symbol,
                 COALESCE(ps.old_score, 0) AS old_score,
                 cs.new_score,
                 COALESCE(ps.old_date::text, '') AS old_date,
                 cs.new_date::text AS new_date,
                 cs.new_score - COALESCE(ps.old_score, 0) AS delta
             FROM current_scores cs
             LEFT JOIN prior_scores ps ON cs.symbol = ps.symbol
             WHERE cs.new_score != COALESCE(ps.old_score, 0)
             ORDER BY ABS(cs.new_score - COALESCE(ps.old_score, 0)) DESC",
        )
        .bind(days as i32)
        .fetch_all(pool)
        .await
    })?;

    Ok(rows
        .into_iter()
        .map(|r| ConvictionChange {
            symbol: r.0,
            old_score: r.1,
            new_score: r.2,
            old_date: r.3,
            new_date: r.4,
            change_delta: r.5,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_set_and_list_current() {
        let conn = setup_test_db();

        set_conviction(&conn, "BTC", 4, Some("Strong bullish thesis")).unwrap();
        set_conviction(&conn, "GC=F", -2, Some("Bearish short-term")).unwrap();
        set_conviction(&conn, "BTC", 5, Some("Updated to max conviction")).unwrap();

        let current = list_current(&conn).unwrap();
        assert_eq!(current.len(), 2);

        let btc = current.iter().find(|e| e.symbol == "BTC").unwrap();
        assert_eq!(btc.score, 5);
        assert_eq!(btc.notes.as_deref(), Some("Updated to max conviction"));

        let gold = current.iter().find(|e| e.symbol == "GC=F").unwrap();
        assert_eq!(gold.score, -2);
    }

    #[test]
    fn test_get_history() {
        let conn = setup_test_db();

        set_conviction(&conn, "SPY", 3, Some("Bull run")).unwrap();
        set_conviction(&conn, "SPY", -1, Some("Market turned")).unwrap();
        set_conviction(&conn, "SPY", 0, Some("Neutral now")).unwrap();

        let history = get_history(&conn, "SPY", None).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].score, 0); // Most recent first

        let limited = get_history(&conn, "SPY", Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_score_validation() {
        let conn = setup_test_db();

        let result = set_conviction(&conn, "TEST", 6, None);
        assert!(result.is_err());

        let result = set_conviction(&conn, "TEST", -6, None);
        assert!(result.is_err());

        let result = set_conviction(&conn, "TEST", 5, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_changes() {
        let conn = setup_test_db();

        // Simulate conviction changes
        set_conviction(&conn, "BTC", 2, Some("Initial")).unwrap();
        set_conviction(&conn, "BTC", 5, Some("Upgraded")).unwrap();
        set_conviction(&conn, "ETH", -1, Some("Bearish")).unwrap();

        let changes = get_changes(&conn, 7).unwrap();
        assert!(!changes.is_empty());

        let btc_change = changes.iter().find(|c| c.symbol == "BTC").unwrap();
        assert_eq!(btc_change.old_score, 2);
        assert_eq!(btc_change.new_score, 5);
        assert_eq!(btc_change.change_delta, 3);
    }
}
