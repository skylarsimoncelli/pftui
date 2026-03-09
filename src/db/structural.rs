use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralLog {
    pub id: i64,
    pub date: String,
    pub development: String,
    pub cycle_impact: Option<String>,
    pub outcome_shift: Option<String>,
    pub created_at: String,
}

fn metric_from_row(row: &Row) -> Result<PowerMetric, rusqlite::Error> {
    Ok(PowerMetric {
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

fn cycle_from_row(row: &Row) -> Result<StructuralCycle, rusqlite::Error> {
    Ok(StructuralCycle {
        id: row.get(0)?,
        cycle_name: row.get(1)?,
        current_stage: row.get(2)?,
        stage_entered: row.get(3)?,
        description: row.get(4)?,
        evidence: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn outcome_from_row(row: &Row) -> Result<StructuralOutcome, rusqlite::Error> {
    Ok(StructuralOutcome {
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

fn parallel_from_row(row: &Row) -> Result<HistoricalParallel, rusqlite::Error> {
    Ok(HistoricalParallel {
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

fn log_from_row(row: &Row) -> Result<StructuralLog, rusqlite::Error> {
    Ok(StructuralLog {
        id: row.get(0)?,
        date: row.get(1)?,
        development: row.get(2)?,
        cycle_impact: row.get(3)?,
        outcome_shift: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn set_metric(
    conn: &Connection,
    country: &str,
    metric: &str,
    score: Option<f64>,
    rank: Option<i32>,
    trend: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO power_metrics (country, metric, score, rank, trend, notes, source)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![country, metric, score, rank, trend.unwrap_or("stable"), notes, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_metrics(conn: &Connection, country: Option<&str>, metric: Option<&str>) -> Result<Vec<PowerMetric>> {
    let mut query = String::from(
        "SELECT id, country, metric, score, rank, trend, notes, source, recorded_at
         FROM power_metrics",
    );
    let mut where_parts = Vec::new();
    if let Some(c) = country {
        where_parts.push(format!("country = '{}'", c.replace('"', "''")));
    }
    if let Some(m) = metric {
        where_parts.push(format!("metric = '{}'", m.replace('"', "''")));
    }
    if !where_parts.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&where_parts.join(" AND "));
    }
    query.push_str(" ORDER BY recorded_at DESC");

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], metric_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn get_metric_history(conn: &Connection, country: &str, metric: &str, limit: Option<usize>) -> Result<Vec<PowerMetric>> {
    let mut query = format!(
        "SELECT id, country, metric, score, rank, trend, notes, source, recorded_at
         FROM power_metrics
         WHERE country = '{}' AND metric = '{}'
         ORDER BY recorded_at DESC",
        country.replace('"', "''"), metric.replace('"', "''")
    );
    if let Some(n) = limit { query.push_str(&format!(" LIMIT {}", n)); }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], metric_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
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
             description = COALESCE(excluded.description, structural_cycles.description),
             evidence = COALESCE(excluded.evidence, structural_cycles.evidence),
             updated_at = datetime('now')",
        params![name, stage, entered, description, evidence],
    )?;
    Ok(())
}

pub fn list_cycles(conn: &Connection) -> Result<Vec<StructuralCycle>> {
    let mut stmt = conn.prepare(
        "SELECT id, cycle_name, current_stage, stage_entered, description, evidence, updated_at
         FROM structural_cycles
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], cycle_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn add_outcome(
    conn: &Connection,
    name: &str,
    probability: f64,
    horizon: Option<&str>,
    description: Option<&str>,
    parallel: Option<&str>,
    impact: Option<&str>,
    signals: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO structural_outcomes
         (name, probability, time_horizon, description, historical_parallel, asset_implications, key_signals)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![name, probability, horizon, description, parallel, impact, signals],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_outcomes(conn: &Connection) -> Result<Vec<StructuralOutcome>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, time_horizon, description, historical_parallel, asset_implications, key_signals, status, created_at, updated_at
         FROM structural_outcomes
         ORDER BY probability DESC, updated_at DESC",
    )?;
    let rows = stmt.query_map([], outcome_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn update_outcome_probability(conn: &Connection, name: &str, probability: f64, driver: Option<&str>) -> Result<()> {
    let outcome_id: i64 = conn.query_row(
        "SELECT id FROM structural_outcomes WHERE name = ?",
        [name],
        |r| r.get(0),
    )?;

    conn.execute(
        "INSERT INTO structural_outcome_history (outcome_id, probability, driver)
         VALUES (?, ?, ?)",
        params![outcome_id, probability, driver],
    )?;

    conn.execute(
        "UPDATE structural_outcomes SET probability = ?, updated_at = datetime('now') WHERE id = ?",
        params![probability, outcome_id],
    )?;

    Ok(())
}

pub fn get_outcome_history(conn: &Connection, name: &str, limit: Option<usize>) -> Result<Vec<(f64, Option<String>, String)>> {
    let mut query = format!(
        "SELECT h.probability, h.driver, h.recorded_at
         FROM structural_outcome_history h
         INNER JOIN structural_outcomes o ON o.id = h.outcome_id
         WHERE o.name = '{}'
         ORDER BY h.recorded_at DESC",
        name.replace('"', "''")
    );
    if let Some(n) = limit { query.push_str(&format!(" LIMIT {}", n)); }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
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
        "INSERT INTO historical_parallels
         (period, event, parallel_to, similarity_score, asset_outcome, notes, source)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![period, event, parallel_to, score, outcome, notes, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_parallels(conn: &Connection, period: Option<&str>) -> Result<Vec<HistoricalParallel>> {
    let query = if let Some(p) = period {
        format!(
            "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at
             FROM historical_parallels
             WHERE period = '{}'
             ORDER BY created_at DESC",
            p.replace('"', "''")
        )
    } else {
        "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at
         FROM historical_parallels
         ORDER BY created_at DESC".to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], parallel_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn search_parallels(conn: &Connection, query: &str) -> Result<Vec<HistoricalParallel>> {
    let mut stmt = conn.prepare(
        "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at
         FROM historical_parallels
         WHERE period LIKE ? OR event LIKE ? OR parallel_to LIKE ? OR notes LIKE ?
         ORDER BY created_at DESC",
    )?;
    let pattern = format!("%{}%", query);
    let rows = stmt.query_map(params![pattern, pattern, pattern, pattern], parallel_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
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
         FROM structural_log",
    );
    if let Some(s) = since {
        query.push_str(&format!(" WHERE date >= '{}'", s.replace('"', "''")));
    }
    query.push_str(" ORDER BY date DESC, created_at DESC");
    if let Some(n) = limit { query.push_str(&format!(" LIMIT {}", n)); }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], log_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}
