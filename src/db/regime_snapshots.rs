use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSnapshot {
    pub id: i64,
    pub regime: String,
    pub confidence: Option<f64>,
    pub drivers: Option<String>,
    pub vix: Option<f64>,
    pub dxy: Option<f64>,
    pub yield_10y: Option<f64>,
    pub oil: Option<f64>,
    pub gold: Option<f64>,
    pub btc: Option<f64>,
    pub recorded_at: String,
}

impl RegimeSnapshot {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            regime: row.get(1)?,
            confidence: row.get(2)?,
            drivers: row.get(3)?,
            vix: row.get(4)?,
            dxy: row.get(5)?,
            yield_10y: row.get(6)?,
            oil: row.get(7)?,
            gold: row.get(8)?,
            btc: row.get(9)?,
            recorded_at: row.get(10)?,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn store_regime(
    conn: &Connection,
    regime: &str,
    confidence: Option<f64>,
    drivers_json: Option<&str>,
    vix: Option<f64>,
    dxy: Option<f64>,
    yield_10y: Option<f64>,
    oil: Option<f64>,
    gold: Option<f64>,
    btc: Option<f64>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO regime_snapshots
         (regime, confidence, drivers, vix, dxy, yield_10y, oil, gold, btc)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            regime,
            confidence,
            drivers_json,
            vix,
            dxy,
            yield_10y,
            oil,
            gold,
            btc
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_current(conn: &Connection) -> Result<Option<RegimeSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT id, regime, confidence, drivers, vix, dxy, yield_10y, oil, gold, btc, recorded_at
         FROM regime_snapshots
         ORDER BY recorded_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], RegimeSnapshot::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn get_history(conn: &Connection, limit: Option<usize>) -> Result<Vec<RegimeSnapshot>> {
    get_history_filtered(conn, limit, None, None)
}

pub fn get_history_filtered(
    conn: &Connection,
    limit: Option<usize>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<RegimeSnapshot>> {
    let mut query = String::from(
        "SELECT id, regime, confidence, drivers, vix, dxy, yield_10y, oil, gold, btc, recorded_at
         FROM regime_snapshots",
    );
    let mut conditions = Vec::new();
    let mut param_values: Vec<String> = Vec::new();
    if let Some(f) = from {
        conditions.push(format!("recorded_at >= ?{}", param_values.len() + 1));
        param_values.push(f.to_string());
    }
    if let Some(t) = to {
        // Include the entire day by comparing with t + 1 day or using substr
        conditions.push(format!(
            "substr(recorded_at, 1, 10) <= ?{}",
            param_values.len() + 1
        ));
        param_values.push(t.to_string());
    }
    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }
    query.push_str(" ORDER BY recorded_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(param_values.iter()),
        RegimeSnapshot::from_row,
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[allow(dead_code)]
pub fn get_transitions(conn: &Connection, limit: Option<usize>) -> Result<Vec<RegimeSnapshot>> {
    let all = get_history(conn, None)?;
    if all.is_empty() {
        return Ok(Vec::new());
    }

    let mut transitions = Vec::new();
    let mut prev = &all[0].regime;
    transitions.push(all[0].clone());

    for row in all.iter().skip(1) {
        if &row.regime != prev {
            transitions.push(row.clone());
            prev = &row.regime;
        }
    }

    if let Some(n) = limit {
        transitions.truncate(n);
    }
    Ok(transitions)
}

#[allow(clippy::too_many_arguments)]
pub fn store_regime_backend(
    backend: &BackendConnection,
    regime: &str,
    confidence: Option<f64>,
    drivers_json: Option<&str>,
    vix: Option<f64>,
    dxy: Option<f64>,
    yield_10y: Option<f64>,
    oil: Option<f64>,
    gold: Option<f64>,
    btc: Option<f64>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            store_regime(
                conn,
                regime,
                confidence,
                drivers_json,
                vix,
                dxy,
                yield_10y,
                oil,
                gold,
                btc,
            )
        },
        |pool| {
            store_regime_postgres(
                pool,
                regime,
                confidence,
                drivers_json,
                vix,
                dxy,
                yield_10y,
                oil,
                gold,
                btc,
            )
        },
    )
}

pub fn get_current_backend(backend: &BackendConnection) -> Result<Option<RegimeSnapshot>> {
    query::dispatch(backend, get_current, get_current_postgres)
}

pub fn get_history_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
) -> Result<Vec<RegimeSnapshot>> {
    get_history_filtered_backend(backend, limit, None, None)
}

pub fn get_history_filtered_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<RegimeSnapshot>> {
    query::dispatch(
        backend,
        |conn| get_history_filtered(conn, limit, from, to),
        |pool| get_history_filtered_postgres(pool, limit, from, to),
    )
}

#[allow(dead_code)]
pub fn get_transitions_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
) -> Result<Vec<RegimeSnapshot>> {
    get_transitions_filtered_backend(backend, limit, None, None)
}

pub fn get_transitions_filtered_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<RegimeSnapshot>> {
    // Get all history (with date filter applied), then extract transitions
    let all = get_history_filtered_backend(backend, None, from, to)?;
    if all.is_empty() {
        return Ok(Vec::new());
    }

    let mut transitions = Vec::new();
    let mut prev = &all[0].regime;
    transitions.push(all[0].clone());

    for row in all.iter().skip(1) {
        if &row.regime != prev {
            transitions.push(row.clone());
            prev = &row.regime;
        }
    }

    if let Some(n) = limit {
        transitions.truncate(n);
    }
    Ok(transitions)
}

#[allow(clippy::type_complexity)]
type RegimeRow = (
    i64,
    String,
    Option<f64>,
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    String,
);

fn from_pg_row(row: RegimeRow) -> RegimeSnapshot {
    RegimeSnapshot {
        id: row.0,
        regime: row.1,
        confidence: row.2,
        drivers: row.3,
        vix: row.4,
        dxy: row.5,
        yield_10y: row.6,
        oil: row.7,
        gold: row.8,
        btc: row.9,
        recorded_at: row.10,
    }
}

#[allow(clippy::too_many_arguments)]
fn store_regime_postgres(
    pool: &PgPool,
    regime: &str,
    confidence: Option<f64>,
    drivers_json: Option<&str>,
    vix: Option<f64>,
    dxy: Option<f64>,
    yield_10y: Option<f64>,
    oil: Option<f64>,
    gold: Option<f64>,
    btc: Option<f64>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO regime_snapshots
             (regime, confidence, drivers, vix, dxy, yield_10y, oil, gold, btc)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id",
        )
        .bind(regime)
        .bind(confidence)
        .bind(drivers_json)
        .bind(vix)
        .bind(dxy)
        .bind(yield_10y)
        .bind(oil)
        .bind(gold)
        .bind(btc)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn get_current_postgres(pool: &PgPool) -> Result<Option<RegimeSnapshot>> {
    let row: Option<RegimeRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, regime, confidence, drivers, vix, dxy, yield_10y, oil, gold, btc, recorded_at::text
             FROM regime_snapshots
             ORDER BY recorded_at DESC
             LIMIT 1",
        )
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(from_pg_row))
}

#[allow(dead_code)]
fn get_history_postgres(pool: &PgPool, limit: Option<usize>) -> Result<Vec<RegimeSnapshot>> {
    get_history_filtered_postgres(pool, limit, None, None)
}

fn get_history_filtered_postgres(
    pool: &PgPool,
    limit: Option<usize>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<RegimeSnapshot>> {
    let from_owned = from.map(|s| s.to_string());
    let to_owned = to.map(|s| s.to_string());
    let limit_val = limit.map(|n| n as i64);

    let rows: Vec<RegimeRow> = crate::db::pg_runtime::block_on(async {
        // Build dynamic query based on filters
        let mut query = String::from(
            "SELECT id, regime, confidence, drivers, vix, dxy, yield_10y, oil, gold, btc, recorded_at::text
             FROM regime_snapshots",
        );
        let mut conditions = Vec::new();
        let mut param_idx = 1;
        if from_owned.is_some() {
            conditions.push(format!("recorded_at >= ${}::timestamptz", param_idx));
            param_idx += 1;
        }
        if to_owned.is_some() {
            conditions.push(format!(
                "recorded_at::date <= ${}::date",
                param_idx
            ));
            param_idx += 1;
        }
        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }
        query.push_str(" ORDER BY recorded_at DESC");
        if limit_val.is_some() {
            query.push_str(&format!(" LIMIT ${}", param_idx));
        }

        let mut q = sqlx::query_as::<_, RegimeRow>(&query);
        if let Some(ref f) = from_owned {
            q = q.bind(f);
        }
        if let Some(ref t) = to_owned {
            q = q.bind(t);
        }
        if let Some(l) = limit_val {
            q = q.bind(l);
        }
        q.fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(from_pg_row).collect())
}
