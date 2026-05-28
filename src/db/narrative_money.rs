use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone)]
pub struct NarrativeMoneyHistoryInsert {
    pub scenario_id: i64,
    pub news_volume: f64,
    pub news_sentiment: f64,
    pub market_price: Option<f64>,
    pub market_delta_24h: Option<f64>,
    pub divergence_score: f64,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS narrative_money_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
            news_volume REAL NOT NULL,
            news_sentiment REAL NOT NULL,
            market_price REAL,
            market_delta_24h REAL,
            divergence_score REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_narrative_money_history_scenario
            ON narrative_money_history(scenario_id, recorded_at);
        CREATE INDEX IF NOT EXISTS idx_narrative_money_history_recorded
            ON narrative_money_history(recorded_at);",
    )?;
    Ok(())
}

pub fn record_history(conn: &Connection, rows: &[NarrativeMoneyHistoryInsert]) -> Result<usize> {
    ensure_table(conn)?;
    let mut inserted = 0usize;
    let mut stmt = conn.prepare(
        "INSERT INTO narrative_money_history
         (scenario_id, news_volume, news_sentiment, market_price, market_delta_24h, divergence_score)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for row in rows {
        stmt.execute(params![
            row.scenario_id,
            row.news_volume,
            row.news_sentiment,
            row.market_price,
            row.market_delta_24h,
            row.divergence_score,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

pub fn ensure_table_backend(backend: &BackendConnection) -> Result<()> {
    query::dispatch(backend, ensure_table, ensure_table_postgres)
}

pub fn record_history_backend(
    backend: &BackendConnection,
    rows: &[NarrativeMoneyHistoryInsert],
) -> Result<usize> {
    let sqlite_rows = rows.to_vec();
    let postgres_rows = rows.to_vec();
    query::dispatch(
        backend,
        move |conn| record_history(conn, &sqlite_rows),
        move |pool| record_history_postgres(pool, &postgres_rows),
    )
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS narrative_money_history (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                news_volume DOUBLE PRECISION NOT NULL,
                news_sentiment DOUBLE PRECISION NOT NULL,
                market_price DOUBLE PRECISION,
                market_delta_24h DOUBLE PRECISION,
                divergence_score DOUBLE PRECISION NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_narrative_money_history_scenario
             ON narrative_money_history(scenario_id, recorded_at)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_narrative_money_history_recorded
             ON narrative_money_history(recorded_at)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn record_history_postgres(pool: &PgPool, rows: &[NarrativeMoneyHistoryInsert]) -> Result<usize> {
    ensure_table_postgres(pool)?;
    let inserted = crate::db::pg_runtime::block_on(async {
        let mut inserted = 0usize;
        for row in rows {
            sqlx::query(
                "INSERT INTO narrative_money_history
                 (scenario_id, news_volume, news_sentiment, market_price, market_delta_24h, divergence_score)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(row.scenario_id)
            .bind(row.news_volume)
            .bind(row.news_sentiment)
            .bind(row.market_price)
            .bind(row.market_delta_24h)
            .bind(row.divergence_score)
            .execute(pool)
            .await?;
            inserted += 1;
        }
        Ok::<usize, sqlx::Error>(inserted)
    })?;
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_history_roundtrips_count() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scenarios (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                probability REAL NOT NULL DEFAULT 0.0,
                status TEXT NOT NULL DEFAULT 'active'
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scenarios (name, probability) VALUES ('Test', 50)",
            [],
        )
        .unwrap();

        let rows = vec![NarrativeMoneyHistoryInsert {
            scenario_id: 1,
            news_volume: 2.5,
            news_sentiment: -20.0,
            market_price: Some(44.0),
            market_delta_24h: Some(1.5),
            divergence_score: 2.1,
        }];

        assert_eq!(record_history(&conn, &rows).unwrap(), 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM narrative_money_history", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
