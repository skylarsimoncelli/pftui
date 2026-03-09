use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeframeSignal {
    pub id: i64,
    pub signal_type: String,
    pub layers: String,
    pub assets: String,
    pub description: String,
    pub severity: String,
    pub detected_at: String,
}

impl TimeframeSignal {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            signal_type: row.get(1)?,
            layers: row.get(2)?,
            assets: row.get(3)?,
            description: row.get(4)?,
            severity: row.get(5)?,
            detected_at: row.get(6)?,
        })
    }
}

pub fn add_signal(
    conn: &Connection,
    signal_type: &str,
    layers_json: &str,
    assets_json: &str,
    description: &str,
    severity: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO timeframe_signals (signal_type, layers, assets, description, severity)
         VALUES (?, ?, ?, ?, ?)",
        params![signal_type, layers_json, assets_json, description, severity],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn add_signal_backend(
    backend: &BackendConnection,
    signal_type: &str,
    layers_json: &str,
    assets_json: &str,
    description: &str,
    severity: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_signal(conn, signal_type, layers_json, assets_json, description, severity),
        |pool| add_signal_postgres(pool, signal_type, layers_json, assets_json, description, severity),
    )
}

pub fn list_signals(
    conn: &Connection,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TimeframeSignal>> {
    let mut query = String::from(
        "SELECT id, signal_type, layers, assets, description, severity, detected_at
         FROM timeframe_signals",
    );

    let mut clauses = Vec::new();
    if let Some(t) = signal_type {
        clauses.push(format!("signal_type = '{}'", t.replace('"', "''")));
    }
    if let Some(s) = severity {
        clauses.push(format!("severity = '{}'", s.replace('"', "''")));
    }
    if !clauses.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&clauses.join(" AND "));
    }

    query.push_str(" ORDER BY detected_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], TimeframeSignal::from_row)?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn list_signals_backend(
    backend: &BackendConnection,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TimeframeSignal>> {
    query::dispatch(
        backend,
        |conn| list_signals(conn, signal_type, severity, limit),
        |pool| list_signals_postgres(pool, signal_type, severity, limit),
    )
}

pub fn latest_signal(conn: &Connection) -> Result<Option<TimeframeSignal>> {
    let mut stmt = conn.prepare(
        "SELECT id, signal_type, layers, assets, description, severity, detected_at
         FROM timeframe_signals
         ORDER BY detected_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], TimeframeSignal::from_row)?;
    Ok(rows.next().transpose()?)
}

#[allow(dead_code)]
pub fn latest_signal_backend(backend: &BackendConnection) -> Result<Option<TimeframeSignal>> {
    query::dispatch(backend, latest_signal, latest_signal_postgres)
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS timeframe_signals (
                id BIGSERIAL PRIMARY KEY,
                signal_type TEXT NOT NULL,
                layers TEXT NOT NULL,
                assets TEXT NOT NULL,
                description TEXT NOT NULL,
                severity TEXT NOT NULL,
                detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_signal_postgres(
    pool: &PgPool,
    signal_type: &str,
    layers_json: &str,
    assets_json: &str,
    description: &str,
    severity: &str,
) -> Result<i64> {
    ensure_table_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO timeframe_signals (signal_type, layers, assets, description, severity)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(signal_type)
        .bind(layers_json)
        .bind(assets_json)
        .bind(description)
        .bind(severity)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

type SignalRow = (i64, String, String, String, String, String, String);

fn to_signal(row: SignalRow) -> TimeframeSignal {
    TimeframeSignal {
        id: row.0,
        signal_type: row.1,
        layers: row.2,
        assets: row.3,
        description: row.4,
        severity: row.5,
        detected_at: row.6,
    }
}

fn list_signals_postgres(
    pool: &PgPool,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TimeframeSignal>> {
    ensure_table_postgres(pool)?;
    let rows: Vec<SignalRow> = crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT id, signal_type, layers, assets, description, severity, detected_at::text
             FROM timeframe_signals
             WHERE TRUE",
        );
        if let Some(t) = signal_type {
            qb.push(" AND signal_type = ").push_bind(t);
        }
        if let Some(s) = severity {
            qb.push(" AND severity = ").push_bind(s);
        }
        qb.push(" ORDER BY detected_at DESC");
        if let Some(limit) = limit {
            qb.push(" LIMIT ").push_bind(limit as i64);
        }
        qb.build_query_as().fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(to_signal).collect())
}

#[allow(dead_code)]
fn latest_signal_postgres(pool: &PgPool) -> Result<Option<TimeframeSignal>> {
    ensure_table_postgres(pool)?;
    let row: Option<SignalRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, signal_type, layers, assets, description, severity, detected_at::text
             FROM timeframe_signals
             ORDER BY detected_at DESC
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(to_signal))
}
