use anyhow::Result;
use rusqlite::{params, Connection, Row as SqliteRow};
use serde::{Deserialize, Serialize};

use crate::db::backend::BackendConnection;

// ═══════════════════════════════════════════════════════════════════════════════
// Power Metrics
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerMetric {
    pub id: i64,
    pub country: String,
    pub metric: String,
    pub score: Option<f64>,
    pub rank: Option<i32>,
    pub trend: String,
    pub notes: Option<String>,
    pub source: Option<String>,
    pub recorded_at: String,
}

impl PowerMetric {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            country: row.get(1)?,
            metric: row.get(2)?,
            score: row.get(3)?,
            rank: row.get(4)?,
            trend: row.get(5)?,
            notes: row.get(6)?,
            source: row.get(7)?,
            recorded_at: row.get(8)?,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn set_metric(
    conn: &Connection,
    country: &str,
    metric: &str,
    score: Option<f64>,
    rank: Option<i32>,
    trend: &str,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO power_metrics (country, metric, score, rank, trend, notes, source)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![country, metric, score, rank, trend, notes, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_metrics(
    conn: &Connection,
    country: Option<&str>,
    metric: Option<&str>,
) -> Result<Vec<PowerMetric>> {
    let mut query = String::from(
        "SELECT id, country, metric, score, rank, trend, notes, source, recorded_at
         FROM power_metrics WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(c) = country {
        query.push_str(" AND country = ?");
        params.push(Box::new(c.to_string()));
    }
    if let Some(m) = metric {
        query.push_str(" AND metric = ?");
        params.push(Box::new(m.to_string()));
    }

    query.push_str(" ORDER BY recorded_at DESC");

    let mut stmt = conn.prepare(&query)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p.as_ref() as &dyn rusqlite::ToSql).collect();

    let rows = stmt.query_map(&*params_refs, PowerMetric::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_metric_history(
    conn: &Connection,
    country: &str,
    metric: &str,
    limit: Option<usize>,
) -> Result<Vec<PowerMetric>> {
    let limit_val = limit.unwrap_or(50);
    let mut stmt = conn.prepare(
        "SELECT id, country, metric, score, rank, trend, notes, source, recorded_at
         FROM power_metrics
         WHERE country = ? AND metric = ?
         ORDER BY recorded_at DESC
         LIMIT ?",
    )?;
    let rows = stmt.query_map(params![country, metric, limit_val as i64], PowerMetric::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Structural Cycles
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralCycle {
    pub id: i64,
    pub cycle_name: String,
    pub current_stage: String,
    pub stage_entered: Option<String>,
    pub description: Option<String>,
    pub evidence: Option<String>,
    pub updated_at: String,
}

impl StructuralCycle {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            cycle_name: row.get(1)?,
            current_stage: row.get(2)?,
            stage_entered: row.get(3)?,
            description: row.get(4)?,
            evidence: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }
}

pub fn set_cycle(
    conn: &Connection,
    name: &str,
    stage: &str,
    entered: Option<&str>,
    description: Option<&str>,
    evidence: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO structural_cycles (cycle_name, current_stage, stage_entered, description, evidence)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(cycle_name) DO UPDATE SET
           current_stage = excluded.current_stage,
           stage_entered = excluded.stage_entered,
           description = excluded.description,
           evidence = excluded.evidence,
           updated_at = datetime('now')",
        params![name, stage, entered, description, evidence],
    )?;
    Ok(())
}

pub fn list_cycles(conn: &Connection) -> Result<Vec<StructuralCycle>> {
    let mut stmt = conn.prepare(
        "SELECT id, cycle_name, current_stage, stage_entered, description, evidence, updated_at
         FROM structural_cycles
         ORDER BY cycle_name",
    )?;
    let rows = stmt.query_map([], StructuralCycle::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[allow(dead_code)]
pub fn get_cycle(conn: &Connection, name: &str) -> Result<Option<StructuralCycle>> {
    let mut stmt = conn.prepare(
        "SELECT id, cycle_name, current_stage, stage_entered, description, evidence, updated_at
         FROM structural_cycles
         WHERE cycle_name = ?",
    )?;
    let mut rows = stmt.query_map(params![name], StructuralCycle::from_row)?;
    if let Some(row) = rows.next() {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Structural Outcomes
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralOutcome {
    pub id: i64,
    pub name: String,
    pub probability: f64,
    pub time_horizon: Option<String>,
    pub description: Option<String>,
    pub historical_parallel: Option<String>,
    pub asset_implications: Option<String>,
    pub key_signals: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl StructuralOutcome {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            probability: row.get(2)?,
            time_horizon: row.get(3)?,
            description: row.get(4)?,
            historical_parallel: row.get(5)?,
            asset_implications: row.get(6)?,
            key_signals: row.get(7)?,
            status: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn add_outcome(
    conn: &Connection,
    name: &str,
    probability: f64,
    time_horizon: Option<&str>,
    description: Option<&str>,
    historical_parallel: Option<&str>,
    asset_implications: Option<&str>,
    key_signals: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO structural_outcomes (name, probability, time_horizon, description, historical_parallel, asset_implications, key_signals)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            name,
            probability,
            time_horizon,
            description,
            historical_parallel,
            asset_implications,
            key_signals
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_outcomes(conn: &Connection) -> Result<Vec<StructuralOutcome>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, time_horizon, description, historical_parallel, asset_implications, key_signals, status, created_at, updated_at
         FROM structural_outcomes
         WHERE status = 'active'
         ORDER BY probability DESC",
    )?;
    let rows = stmt.query_map([], StructuralOutcome::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn update_outcome_probability(
    conn: &Connection,
    name: &str,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    // Get outcome ID
    let id: i64 = conn.query_row(
        "SELECT id FROM structural_outcomes WHERE name = ?",
        params![name],
        |row| row.get(0),
    )?;

    // Update probability
    conn.execute(
        "UPDATE structural_outcomes SET probability = ?, updated_at = datetime('now') WHERE id = ?",
        params![probability, id],
    )?;

    // Log to history
    conn.execute(
        "INSERT INTO structural_outcome_history (outcome_id, probability, driver)
         VALUES (?, ?, ?)",
        params![id, probability, driver],
    )?;

    Ok(())
}

pub fn get_outcome_history(
    conn: &Connection,
    name: &str,
    limit: Option<usize>,
) -> Result<Vec<(f64, Option<String>, String)>> {
    let limit_val = limit.unwrap_or(50);
    let mut stmt = conn.prepare(
        "SELECT h.probability, h.driver, h.recorded_at
         FROM structural_outcome_history h
         JOIN structural_outcomes o ON h.outcome_id = o.id
         WHERE o.name = ?
         ORDER BY h.recorded_at DESC
         LIMIT ?",
    )?;
    let rows = stmt.query_map(params![name, limit_val as i64], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Historical Parallels
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalParallel {
    pub id: i64,
    pub period: String,
    pub event: String,
    pub parallel_to: String,
    pub similarity_score: Option<i32>,
    pub asset_outcome: Option<String>,
    pub notes: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
}

impl HistoricalParallel {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            period: row.get(1)?,
            event: row.get(2)?,
            parallel_to: row.get(3)?,
            similarity_score: row.get(4)?,
            asset_outcome: row.get(5)?,
            notes: row.get(6)?,
            source: row.get(7)?,
            created_at: row.get(8)?,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn add_parallel(
    conn: &Connection,
    period: &str,
    event: &str,
    parallel_to: &str,
    score: Option<i32>,
    outcome: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO historical_parallels (period, event, parallel_to, similarity_score, asset_outcome, notes, source)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![period, event, parallel_to, score, outcome, notes, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_parallels(conn: &Connection, period: Option<&str>) -> Result<Vec<HistoricalParallel>> {
    let mut query = String::from(
        "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at
         FROM historical_parallels WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(p) = period {
        query.push_str(" AND period = ?");
        params.push(Box::new(p.to_string()));
    }

    query.push_str(" ORDER BY created_at DESC");

    let mut stmt = conn.prepare(&query)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p.as_ref() as &dyn rusqlite::ToSql).collect();

    let rows = stmt.query_map(&*params_refs, HistoricalParallel::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_parallels(conn: &Connection, query: &str) -> Result<Vec<HistoricalParallel>> {
    let search_term = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at
         FROM historical_parallels
         WHERE period LIKE ? OR event LIKE ? OR parallel_to LIKE ? OR notes LIKE ?
         ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map(
        params![&search_term, &search_term, &search_term, &search_term],
        HistoricalParallel::from_row,
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Structural Log
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralLog {
    pub id: i64,
    pub date: String,
    pub development: String,
    pub cycle_impact: Option<String>,
    pub outcome_shift: Option<String>,
    pub created_at: String,
}

impl StructuralLog {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            date: row.get(1)?,
            development: row.get(2)?,
            cycle_impact: row.get(3)?,
            outcome_shift: row.get(4)?,
            created_at: row.get(5)?,
        })
    }
}

pub fn add_log(
    conn: &Connection,
    date: &str,
    development: &str,
    cycle_impact: Option<&str>,
    outcome_shift: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO structural_log (date, development, cycle_impact, outcome_shift)
         VALUES (?, ?, ?, ?)",
        params![date, development, cycle_impact, outcome_shift],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_log(conn: &Connection, since: Option<&str>, limit: Option<usize>) -> Result<Vec<StructuralLog>> {
    let mut query = String::from(
        "SELECT id, date, development, cycle_impact, outcome_shift, created_at
         FROM structural_log WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(s) = since {
        query.push_str(" AND date >= ?");
        params.push(Box::new(s.to_string()));
    }

    query.push_str(" ORDER BY date DESC");

    if let Some(lim) = limit {
        query.push_str(" LIMIT ?");
        params.push(Box::new(lim as i64));
    }

    let mut stmt = conn.prepare(&query)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p.as_ref() as &dyn rusqlite::ToSql).collect();

    let rows = stmt.query_map(&*params_refs, StructuralLog::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Backend wrappers
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
pub fn set_metric_backend(
    backend: &BackendConnection,
    country: &str,
    metric: &str,
    score: Option<f64>,
    rank: Option<i32>,
    trend: &str,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    match backend {
        BackendConnection::Sqlite { conn } => set_metric(conn, country, metric, score, rank, trend, notes, source),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn list_metrics_backend(
    backend: &BackendConnection,
    country: Option<&str>,
    metric: Option<&str>,
) -> Result<Vec<PowerMetric>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_metrics(conn, country, metric),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn get_metric_history_backend(
    backend: &BackendConnection,
    country: &str,
    metric: &str,
    limit: Option<usize>,
) -> Result<Vec<PowerMetric>> {
    match backend {
        BackendConnection::Sqlite { conn } => get_metric_history(conn, country, metric, limit),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn set_cycle_backend(
    backend: &BackendConnection,
    name: &str,
    stage: &str,
    entered: Option<&str>,
    description: Option<&str>,
    evidence: Option<&str>,
) -> Result<()> {
    match backend {
        BackendConnection::Sqlite { conn } => set_cycle(conn, name, stage, entered, description, evidence),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn list_cycles_backend(backend: &BackendConnection) -> Result<Vec<StructuralCycle>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_cycles(conn),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

#[allow(dead_code)]
pub fn get_cycle_backend(backend: &BackendConnection, name: &str) -> Result<Option<StructuralCycle>> {
    match backend {
        BackendConnection::Sqlite { conn } => get_cycle(conn, name),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn add_outcome_backend(
    backend: &BackendConnection,
    name: &str,
    probability: f64,
    time_horizon: Option<&str>,
    description: Option<&str>,
    historical_parallel: Option<&str>,
    asset_implications: Option<&str>,
    key_signals: Option<&str>,
) -> Result<i64> {
    match backend {
        BackendConnection::Sqlite { conn } => add_outcome(
            conn,
            name,
            probability,
            time_horizon,
            description,
            historical_parallel,
            asset_implications,
            key_signals,
        ),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn list_outcomes_backend(backend: &BackendConnection) -> Result<Vec<StructuralOutcome>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_outcomes(conn),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn update_outcome_probability_backend(
    backend: &BackendConnection,
    name: &str,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    match backend {
        BackendConnection::Sqlite { conn } => update_outcome_probability(conn, name, probability, driver),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn get_outcome_history_backend(
    backend: &BackendConnection,
    name: &str,
    limit: Option<usize>,
) -> Result<Vec<(f64, Option<String>, String)>> {
    match backend {
        BackendConnection::Sqlite { conn } => get_outcome_history(conn, name, limit),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn add_parallel_backend(
    backend: &BackendConnection,
    period: &str,
    event: &str,
    parallel_to: &str,
    score: Option<i32>,
    outcome: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    match backend {
        BackendConnection::Sqlite { conn } => add_parallel(conn, period, event, parallel_to, score, outcome, notes, source),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn list_parallels_backend(backend: &BackendConnection, period: Option<&str>) -> Result<Vec<HistoricalParallel>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_parallels(conn, period),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn search_parallels_backend(backend: &BackendConnection, query: &str) -> Result<Vec<HistoricalParallel>> {
    match backend {
        BackendConnection::Sqlite { conn } => search_parallels(conn, query),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn add_log_backend(
    backend: &BackendConnection,
    date: &str,
    development: &str,
    cycle_impact: Option<&str>,
    outcome_shift: Option<&str>,
) -> Result<i64> {
    match backend {
        BackendConnection::Sqlite { conn } => add_log(conn, date, development, cycle_impact, outcome_shift),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}

pub fn list_log_backend(
    backend: &BackendConnection,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<StructuralLog>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_log(conn, since, limit),
        BackendConnection::Postgres { pool: _ } => {
            anyhow::bail!("Postgres structural storage not yet implemented")
        }
    }
}
