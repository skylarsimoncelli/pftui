#![allow(dead_code)]

use anyhow::Result;
use rusqlite::{params, Connection, Row as SqliteRow};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;

// Type aliases for complex Postgres query rows
type PowerMetricRow = (
    i64,
    String,
    String,
    Option<f64>,
    Option<i32>,
    String,
    Option<String>,
    Option<String>,
    String,
);
type PowerMetricHistoryRow = (
    i64,
    String,
    String,
    i32,
    f64,
    Option<String>,
    Option<String>,
    String,
);
type StructuralCycleRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);
type StructuralOutcomeRow = (
    i64,
    String,
    f64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
);
type HistoricalParallelRow = (
    i64,
    String,
    String,
    String,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);
type StructuralLogRow = (i64, String, String, Option<String>, Option<String>, String);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerMetricHistory {
    pub id: i64,
    pub country: String,
    pub metric: String,
    pub decade: i32,
    pub score: f64,
    pub notes: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
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
    let params_refs: Vec<&dyn rusqlite::ToSql> = params
        .iter()
        .map(|p| p.as_ref() as &dyn rusqlite::ToSql)
        .collect();

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
    let rows = stmt.query_map(
        params![country, metric, limit_val as i64],
        PowerMetric::from_row,
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn add_metric_history(
    conn: &Connection,
    country: &str,
    metric: &str,
    decade: i32,
    score: f64,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO power_metrics_history (country, metric, decade, score, notes, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(country, metric, decade) DO UPDATE SET
            score = excluded.score,
            notes = excluded.notes,
            source = excluded.source",
        params![country, metric, decade, score, notes, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_metric_history(
    conn: &Connection,
    countries: &[String],
    metric: Option<&str>,
    decade: Option<i32>,
) -> Result<Vec<PowerMetricHistory>> {
    let mut query = String::from(
        "SELECT id, country, metric, decade, score, notes, source, created_at
         FROM power_metrics_history WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if !countries.is_empty() {
        let placeholders = std::iter::repeat_n("?", countries.len())
            .collect::<Vec<_>>()
            .join(", ");
        query.push_str(&format!(" AND country IN ({})", placeholders));
        for c in countries {
            params.push(Box::new(c.clone()));
        }
    }
    if let Some(m) = metric {
        query.push_str(" AND metric = ?");
        params.push(Box::new(m.to_string()));
    }
    if let Some(d) = decade {
        query.push_str(" AND decade = ?");
        params.push(Box::new(d));
    }
    query.push_str(" ORDER BY decade ASC, country ASC, metric ASC");

    let mut stmt = conn.prepare(&query)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params
        .iter()
        .map(|p| p.as_ref() as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&*params_refs, |row| {
        Ok(PowerMetricHistory {
            id: row.get(0)?,
            country: row.get(1)?,
            metric: row.get(2)?,
            decade: row.get(3)?,
            score: row.get(4)?,
            notes: row.get(5)?,
            source: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?;
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
    let params_refs: Vec<&dyn rusqlite::ToSql> = params
        .iter()
        .map(|p| p.as_ref() as &dyn rusqlite::ToSql)
        .collect();

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

pub fn list_log(
    conn: &Connection,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<StructuralLog>> {
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
    let params_refs: Vec<&dyn rusqlite::ToSql> = params
        .iter()
        .map(|p| p.as_ref() as &dyn rusqlite::ToSql)
        .collect();

    let rows = stmt.query_map(&*params_refs, StructuralLog::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Backend wrappers
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
fn set_metric_postgres(
    pool: &PgPool,
    country: &str,
    metric: &str,
    score: Option<f64>,
    rank: Option<i32>,
    trend: &str,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    let id = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO power_metrics (country, metric, score, rank, trend, notes, source)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(country)
        .bind(metric)
        .bind(score)
        .bind(rank)
        .bind(trend)
        .bind(notes)
        .bind(source)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_metrics_postgres(
    pool: &PgPool,
    country: Option<&str>,
    metric: Option<&str>,
) -> Result<Vec<PowerMetric>> {
    let rows: Vec<PowerMetricRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, country, metric, score, rank, trend, notes, source, recorded_at::TEXT
                 FROM power_metrics
                 WHERE ($1::TEXT IS NULL OR country = $1)
                   AND ($2::TEXT IS NULL OR metric = $2)
                 ORDER BY recorded_at DESC",
        )
        .bind(country)
        .bind(metric)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| PowerMetric {
            id: r.0,
            country: r.1,
            metric: r.2,
            score: r.3,
            rank: r.4,
            trend: r.5,
            notes: r.6,
            source: r.7,
            recorded_at: r.8,
        })
        .collect())
}

fn get_metric_history_postgres(
    pool: &PgPool,
    country: &str,
    metric: &str,
    limit: Option<usize>,
) -> Result<Vec<PowerMetric>> {
    let limit_val = limit.unwrap_or(50) as i64;
    let rows: Vec<PowerMetricRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, country, metric, score, rank, trend, notes, source, recorded_at::TEXT
                 FROM power_metrics
                 WHERE country = $1 AND metric = $2
                 ORDER BY recorded_at DESC
                 LIMIT $3",
        )
        .bind(country)
        .bind(metric)
        .bind(limit_val)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| PowerMetric {
            id: r.0,
            country: r.1,
            metric: r.2,
            score: r.3,
            rank: r.4,
            trend: r.5,
            notes: r.6,
            source: r.7,
            recorded_at: r.8,
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn add_metric_history_postgres(
    pool: &PgPool,
    country: &str,
    metric: &str,
    decade: i32,
    score: f64,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    let id = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO power_metrics_history (country, metric, decade, score, notes, source)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(country, metric, decade) DO UPDATE SET
                score = EXCLUDED.score,
                notes = EXCLUDED.notes,
                source = EXCLUDED.source
             RETURNING id",
        )
        .bind(country)
        .bind(metric)
        .bind(decade)
        .bind(score)
        .bind(notes)
        .bind(source)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_metric_history_postgres(
    pool: &PgPool,
    countries: &[String],
    metric: Option<&str>,
    decade: Option<i32>,
) -> Result<Vec<PowerMetricHistory>> {
    let rows: Vec<PowerMetricHistoryRow> = crate::db::pg_runtime::block_on(async {
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
            "SELECT id, country, metric, decade, score, notes, source, created_at::TEXT
             FROM power_metrics_history WHERE 1=1",
        );
        if !countries.is_empty() {
            qb.push(" AND country IN (");
            let mut separated = qb.separated(", ");
            for c in countries {
                separated.push_bind(c);
            }
            qb.push(")");
        }
        if let Some(m) = metric {
            qb.push(" AND metric = ").push_bind(m);
        }
        if let Some(d) = decade {
            qb.push(" AND decade = ").push_bind(d);
        }
        qb.push(" ORDER BY decade ASC, country ASC, metric ASC");
        qb.build_query_as::<PowerMetricHistoryRow>()
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| PowerMetricHistory {
            id: r.0,
            country: r.1,
            metric: r.2,
            decade: r.3,
            score: r.4,
            notes: r.5,
            source: r.6,
            created_at: r.7,
        })
        .collect())
}

fn set_cycle_postgres(
    pool: &PgPool,
    name: &str,
    stage: &str,
    entered: Option<&str>,
    description: Option<&str>,
    evidence: Option<&str>,
) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO structural_cycles (cycle_name, current_stage, stage_entered, description, evidence)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT(cycle_name) DO UPDATE SET
               current_stage = EXCLUDED.current_stage,
               stage_entered = EXCLUDED.stage_entered,
               description = EXCLUDED.description,
               evidence = EXCLUDED.evidence,
               updated_at = NOW()",
        )
        .bind(name)
        .bind(stage)
        .bind(entered)
        .bind(description)
        .bind(evidence)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_cycles_postgres(pool: &PgPool) -> Result<Vec<StructuralCycle>> {
    let rows: Vec<StructuralCycleRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
                "SELECT id, cycle_name, current_stage, stage_entered, description, evidence, updated_at::TEXT
                 FROM structural_cycles
                 ORDER BY cycle_name",
            )
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| StructuralCycle {
            id: r.0,
            cycle_name: r.1,
            current_stage: r.2,
            stage_entered: r.3,
            description: r.4,
            evidence: r.5,
            updated_at: r.6,
        })
        .collect())
}

fn get_cycle_postgres(pool: &PgPool, name: &str) -> Result<Option<StructuralCycle>> {
    let row: Option<StructuralCycleRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
                "SELECT id, cycle_name, current_stage, stage_entered, description, evidence, updated_at::TEXT
                 FROM structural_cycles
                 WHERE cycle_name = $1",
            )
            .bind(name)
            .fetch_optional(pool)
            .await
    })?;
    Ok(row.map(|r| StructuralCycle {
        id: r.0,
        cycle_name: r.1,
        current_stage: r.2,
        stage_entered: r.3,
        description: r.4,
        evidence: r.5,
        updated_at: r.6,
    }))
}

#[allow(clippy::too_many_arguments)]
fn add_outcome_postgres(
    pool: &PgPool,
    name: &str,
    probability: f64,
    time_horizon: Option<&str>,
    description: Option<&str>,
    historical_parallel: Option<&str>,
    asset_implications: Option<&str>,
    key_signals: Option<&str>,
) -> Result<i64> {
    let id = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO structural_outcomes (name, probability, time_horizon, description, historical_parallel, asset_implications, key_signals)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(name)
        .bind(probability)
        .bind(time_horizon)
        .bind(description)
        .bind(historical_parallel)
        .bind(asset_implications)
        .bind(key_signals)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_outcomes_postgres(pool: &PgPool) -> Result<Vec<StructuralOutcome>> {
    let rows: Vec<StructuralOutcomeRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
                "SELECT id, name, probability, time_horizon, description, historical_parallel, asset_implications, key_signals, status, created_at::TEXT, updated_at::TEXT
                 FROM structural_outcomes
                 WHERE status = 'active'
                 ORDER BY probability DESC",
            )
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| StructuralOutcome {
            id: r.0,
            name: r.1,
            probability: r.2,
            time_horizon: r.3,
            description: r.4,
            historical_parallel: r.5,
            asset_implications: r.6,
            key_signals: r.7,
            status: r.8,
            created_at: r.9,
            updated_at: r.10,
        })
        .collect())
}

fn update_outcome_probability_postgres(
    pool: &PgPool,
    name: &str,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        let mut tx = pool.begin().await?;
        let id: i64 = sqlx::query_scalar("SELECT id FROM structural_outcomes WHERE name = $1")
            .bind(name)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE structural_outcomes
             SET probability = $1, updated_at = NOW()
             WHERE id = $2",
        )
        .bind(probability)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO structural_outcome_history (outcome_id, probability, driver)
             VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(probability)
        .bind(driver)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_outcome_history_postgres(
    pool: &PgPool,
    name: &str,
    limit: Option<usize>,
) -> Result<Vec<(f64, Option<String>, String)>> {
    let limit_val = limit.unwrap_or(50) as i64;
    let rows: Vec<(f64, Option<String>, String)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT h.probability, h.driver, h.recorded_at::TEXT
             FROM structural_outcome_history h
             JOIN structural_outcomes o ON h.outcome_id = o.id
             WHERE o.name = $1
             ORDER BY h.recorded_at DESC
             LIMIT $2",
        )
        .bind(name)
        .bind(limit_val)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
fn add_parallel_postgres(
    pool: &PgPool,
    period: &str,
    event: &str,
    parallel_to: &str,
    score: Option<i32>,
    outcome: Option<&str>,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    let id = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO historical_parallels (period, event, parallel_to, similarity_score, asset_outcome, notes, source)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id",
        )
        .bind(period)
        .bind(event)
        .bind(parallel_to)
        .bind(score)
        .bind(outcome)
        .bind(notes)
        .bind(source)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_parallels_postgres(pool: &PgPool, period: Option<&str>) -> Result<Vec<HistoricalParallel>> {
    let rows: Vec<HistoricalParallelRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
                "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at::TEXT
                 FROM historical_parallels
                 WHERE ($1::TEXT IS NULL OR period = $1)
                 ORDER BY created_at DESC",
            )
            .bind(period)
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| HistoricalParallel {
            id: r.0,
            period: r.1,
            event: r.2,
            parallel_to: r.3,
            similarity_score: r.4,
            asset_outcome: r.5,
            notes: r.6,
            source: r.7,
            created_at: r.8,
        })
        .collect())
}

fn search_parallels_postgres(pool: &PgPool, query: &str) -> Result<Vec<HistoricalParallel>> {
    let like = format!("%{}%", query);
    let rows: Vec<HistoricalParallelRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
                "SELECT id, period, event, parallel_to, similarity_score, asset_outcome, notes, source, created_at::TEXT
                 FROM historical_parallels
                 WHERE event ILIKE $1
                    OR parallel_to ILIKE $1
                    OR notes ILIKE $1
                 ORDER BY created_at DESC",
            )
            .bind(&like)
            .fetch_all(pool)
            .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| HistoricalParallel {
            id: r.0,
            period: r.1,
            event: r.2,
            parallel_to: r.3,
            similarity_score: r.4,
            asset_outcome: r.5,
            notes: r.6,
            source: r.7,
            created_at: r.8,
        })
        .collect())
}

fn add_log_postgres(
    pool: &PgPool,
    date: &str,
    development: &str,
    cycle_impact: Option<&str>,
    outcome_shift: Option<&str>,
) -> Result<i64> {
    let id = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO structural_log (date, development, cycle_impact, outcome_shift)
             VALUES ($1, $2, $3, $4)
             RETURNING id",
        )
        .bind(date)
        .bind(development)
        .bind(cycle_impact)
        .bind(outcome_shift)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_log_postgres(
    pool: &PgPool,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<StructuralLog>> {
    let limit_val = limit.unwrap_or(50) as i64;
    let rows: Vec<StructuralLogRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, date, development, cycle_impact, outcome_shift, created_at::TEXT
                 FROM structural_log
                 WHERE ($1::TEXT IS NULL OR date >= $1)
                 ORDER BY date DESC
                 LIMIT $2",
        )
        .bind(since)
        .bind(limit_val)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| StructuralLog {
            id: r.0,
            date: r.1,
            development: r.2,
            cycle_impact: r.3,
            outcome_shift: r.4,
            created_at: r.5,
        })
        .collect())
}

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
        BackendConnection::Sqlite { conn } => {
            set_metric(conn, country, metric, score, rank, trend, notes, source)
        }
        BackendConnection::Postgres { pool } => {
            set_metric_postgres(pool, country, metric, score, rank, trend, notes, source)
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
        BackendConnection::Postgres { pool } => list_metrics_postgres(pool, country, metric),
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
        BackendConnection::Postgres { pool } => {
            get_metric_history_postgres(pool, country, metric, limit)
        }
    }
}

pub fn add_metric_history_backend(
    backend: &BackendConnection,
    country: &str,
    metric: &str,
    decade: i32,
    score: f64,
    notes: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    match backend {
        BackendConnection::Sqlite { conn } => {
            add_metric_history(conn, country, metric, decade, score, notes, source)
        }
        BackendConnection::Postgres { pool } => {
            add_metric_history_postgres(pool, country, metric, decade, score, notes, source)
        }
    }
}

pub fn list_metric_history_backend(
    backend: &BackendConnection,
    countries: &[String],
    metric: Option<&str>,
    decade: Option<i32>,
) -> Result<Vec<PowerMetricHistory>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_metric_history(conn, countries, metric, decade),
        BackendConnection::Postgres { pool } => {
            list_metric_history_postgres(pool, countries, metric, decade)
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
        BackendConnection::Sqlite { conn } => {
            set_cycle(conn, name, stage, entered, description, evidence)
        }
        BackendConnection::Postgres { pool } => {
            set_cycle_postgres(pool, name, stage, entered, description, evidence)
        }
    }
}

pub fn list_cycles_backend(backend: &BackendConnection) -> Result<Vec<StructuralCycle>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_cycles(conn),
        BackendConnection::Postgres { pool } => list_cycles_postgres(pool),
    }
}

#[allow(dead_code)]
pub fn get_cycle_backend(
    backend: &BackendConnection,
    name: &str,
) -> Result<Option<StructuralCycle>> {
    match backend {
        BackendConnection::Sqlite { conn } => get_cycle(conn, name),
        BackendConnection::Postgres { pool } => get_cycle_postgres(pool, name),
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
        BackendConnection::Postgres { pool } => add_outcome_postgres(
            pool,
            name,
            probability,
            time_horizon,
            description,
            historical_parallel,
            asset_implications,
            key_signals,
        ),
    }
}

pub fn list_outcomes_backend(backend: &BackendConnection) -> Result<Vec<StructuralOutcome>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_outcomes(conn),
        BackendConnection::Postgres { pool } => list_outcomes_postgres(pool),
    }
}

pub fn update_outcome_probability_backend(
    backend: &BackendConnection,
    name: &str,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    match backend {
        BackendConnection::Sqlite { conn } => {
            update_outcome_probability(conn, name, probability, driver)
        }
        BackendConnection::Postgres { pool } => {
            update_outcome_probability_postgres(pool, name, probability, driver)
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
        BackendConnection::Postgres { pool } => get_outcome_history_postgres(pool, name, limit),
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
        BackendConnection::Sqlite { conn } => add_parallel(
            conn,
            period,
            event,
            parallel_to,
            score,
            outcome,
            notes,
            source,
        ),
        BackendConnection::Postgres { pool } => add_parallel_postgres(
            pool,
            period,
            event,
            parallel_to,
            score,
            outcome,
            notes,
            source,
        ),
    }
}

pub fn list_parallels_backend(
    backend: &BackendConnection,
    period: Option<&str>,
) -> Result<Vec<HistoricalParallel>> {
    match backend {
        BackendConnection::Sqlite { conn } => list_parallels(conn, period),
        BackendConnection::Postgres { pool } => list_parallels_postgres(pool, period),
    }
}

pub fn search_parallels_backend(
    backend: &BackendConnection,
    query: &str,
) -> Result<Vec<HistoricalParallel>> {
    match backend {
        BackendConnection::Sqlite { conn } => search_parallels(conn, query),
        BackendConnection::Postgres { pool } => search_parallels_postgres(pool, query),
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
        BackendConnection::Sqlite { conn } => {
            add_log(conn, date, development, cycle_impact, outcome_shift)
        }
        BackendConnection::Postgres { pool } => {
            add_log_postgres(pool, date, development, cycle_impact, outcome_shift)
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
        BackendConnection::Postgres { pool } => list_log_postgres(pool, since, limit),
    }
}
