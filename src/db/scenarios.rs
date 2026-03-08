use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

type ScenarioRow = (
    i64,
    String,
    f64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
);
type ScenarioSignalRow = (i64, i64, String, String, Option<String>, Option<String>, String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub id: i64,
    pub name: String,
    pub probability: f64,
    pub description: Option<String>,
    pub asset_impact: Option<String>,
    pub triggers: Option<String>,
    pub historical_precedent: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSignal {
    pub id: i64,
    pub scenario_id: i64,
    pub signal: String,
    pub status: String,
    pub evidence: Option<String>,
    pub source: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioHistoryEntry {
    pub id: i64,
    pub scenario_id: i64,
    pub probability: f64,
    pub driver: Option<String>,
    pub recorded_at: String,
}

impl Scenario {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            probability: row.get(2)?,
            description: row.get(3)?,
            asset_impact: row.get(4)?,
            triggers: row.get(5)?,
            historical_precedent: row.get(6)?,
            status: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    }
}

impl ScenarioSignal {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            scenario_id: row.get(1)?,
            signal: row.get(2)?,
            status: row.get(3)?,
            evidence: row.get(4)?,
            source: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }
}

impl ScenarioHistoryEntry {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            scenario_id: row.get(1)?,
            probability: row.get(2)?,
            driver: row.get(3)?,
            recorded_at: row.get(4)?,
        })
    }
}

pub fn add_scenario(
    conn: &Connection,
    name: &str,
    probability: f64,
    description: Option<&str>,
    asset_impact: Option<&str>,
    triggers: Option<&str>,
    precedent: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO scenarios (name, probability, description, asset_impact, triggers, historical_precedent)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![name, probability, description, asset_impact, triggers, precedent],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_scenarios(
    conn: &Connection,
    status_filter: Option<&str>,
) -> Result<Vec<Scenario>> {
    let query = if let Some(status) = status_filter {
        format!(
            "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at
             FROM scenarios
             WHERE status = '{}'
             ORDER BY probability DESC",
            status
        )
    } else {
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at
         FROM scenarios
         ORDER BY probability DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], Scenario::from_row)?;

    let mut scenarios = Vec::new();
    for row in rows {
        scenarios.push(row?);
    }
    Ok(scenarios)
}

pub fn get_scenario_by_name(conn: &Connection, name: &str) -> Result<Option<Scenario>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at
         FROM scenarios
         WHERE name = ?",
    )?;

    let mut rows = stmt.query_map([name], Scenario::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn update_scenario_probability(
    conn: &Connection,
    id: i64,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    // Snapshot to history before updating
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver)
         SELECT id, probability, ? FROM scenarios WHERE id = ?",
        params![driver, id],
    )?;

    conn.execute(
        "UPDATE scenarios SET probability = ?, updated_at = datetime('now') WHERE id = ?",
        params![probability, id],
    )?;
    Ok(())
}

pub fn update_scenario(
    conn: &Connection,
    id: i64,
    description: Option<&str>,
    asset_impact: Option<&str>,
    triggers: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    // Build dynamic UPDATE query for non-None fields
    let mut updates = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(d) = description {
        updates.push("description = ?");
        params_vec.push(Box::new(d.to_string()));
    }
    if let Some(a) = asset_impact {
        updates.push("asset_impact = ?");
        params_vec.push(Box::new(a.to_string()));
    }
    if let Some(t) = triggers {
        updates.push("triggers = ?");
        params_vec.push(Box::new(t.to_string()));
    }
    if let Some(s) = status {
        updates.push("status = ?");
        params_vec.push(Box::new(s.to_string()));
    }

    if updates.is_empty() {
        return Ok(());
    }

    updates.push("updated_at = datetime('now')");

    let query = format!("UPDATE scenarios SET {} WHERE id = ?", updates.join(", "));
    params_vec.push(Box::new(id));

    let params_refs: Vec<&dyn rusqlite::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();

    conn.execute(&query, params_refs.as_slice())?;
    Ok(())
}

pub fn remove_scenario(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM scenarios WHERE id = ?", [id])?;
    Ok(())
}

pub fn add_signal(
    conn: &Connection,
    scenario_id: i64,
    signal: &str,
    status: Option<&str>,
    evidence: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO scenario_signals (scenario_id, signal, status, evidence, source)
         VALUES (?, ?, ?, ?, ?)",
        params![
            scenario_id,
            signal,
            status.unwrap_or("watching"),
            evidence,
            source
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_signals(
    conn: &Connection,
    scenario_id: i64,
    status_filter: Option<&str>,
) -> Result<Vec<ScenarioSignal>> {
    let query = if let Some(status) = status_filter {
        format!(
            "SELECT id, scenario_id, signal, status, evidence, source, updated_at
             FROM scenario_signals
             WHERE scenario_id = {} AND status = '{}'
             ORDER BY updated_at DESC",
            scenario_id, status
        )
    } else {
        format!(
            "SELECT id, scenario_id, signal, status, evidence, source, updated_at
             FROM scenario_signals
             WHERE scenario_id = {}
             ORDER BY updated_at DESC",
            scenario_id
        )
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], ScenarioSignal::from_row)?;

    let mut signals = Vec::new();
    for row in rows {
        signals.push(row?);
    }
    Ok(signals)
}

pub fn update_signal(
    conn: &Connection,
    signal_id: i64,
    status: Option<&str>,
    evidence: Option<&str>,
) -> Result<()> {
    let mut updates = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(s) = status {
        updates.push("status = ?");
        params_vec.push(Box::new(s.to_string()));
    }
    if let Some(e) = evidence {
        updates.push("evidence = ?");
        params_vec.push(Box::new(e.to_string()));
    }

    if updates.is_empty() {
        return Ok(());
    }

    updates.push("updated_at = datetime('now')");

    let query = format!(
        "UPDATE scenario_signals SET {} WHERE id = ?",
        updates.join(", ")
    );
    params_vec.push(Box::new(signal_id));

    let params_refs: Vec<&dyn rusqlite::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();

    conn.execute(&query, params_refs.as_slice())?;
    Ok(())
}

pub fn remove_signal(conn: &Connection, signal_id: i64) -> Result<()> {
    conn.execute("DELETE FROM scenario_signals WHERE id = ?", [signal_id])?;
    Ok(())
}

pub fn get_history(
    conn: &Connection,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioHistoryEntry>> {
    let query = if let Some(lim) = limit {
        format!(
            "SELECT id, scenario_id, probability, driver, recorded_at
             FROM scenario_history
             WHERE scenario_id = {}
             ORDER BY recorded_at DESC
             LIMIT {}",
            scenario_id, lim
        )
    } else {
        format!(
            "SELECT id, scenario_id, probability, driver, recorded_at
             FROM scenario_history
             WHERE scenario_id = {}
             ORDER BY recorded_at DESC",
            scenario_id
        )
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], ScenarioHistoryEntry::from_row)?;

    let mut history = Vec::new();
    for row in rows {
        history.push(row?);
    }
    Ok(history)
}

pub fn add_scenario_backend(
    backend: &BackendConnection,
    name: &str,
    probability: f64,
    description: Option<&str>,
    asset_impact: Option<&str>,
    triggers: Option<&str>,
    precedent: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_scenario(conn, name, probability, description, asset_impact, triggers, precedent),
        |pool| add_scenario_postgres(pool, name, probability, description, asset_impact, triggers, precedent),
    )
}

pub fn list_scenarios_backend(
    backend: &BackendConnection,
    status_filter: Option<&str>,
) -> Result<Vec<Scenario>> {
    query::dispatch(
        backend,
        |conn| list_scenarios(conn, status_filter),
        |pool| list_scenarios_postgres(pool, status_filter),
    )
}

pub fn get_scenario_by_name_backend(
    backend: &BackendConnection,
    name: &str,
) -> Result<Option<Scenario>> {
    query::dispatch(
        backend,
        |conn| get_scenario_by_name(conn, name),
        |pool| get_scenario_by_name_postgres(pool, name),
    )
}

pub fn update_scenario_probability_backend(
    backend: &BackendConnection,
    id: i64,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_scenario_probability(conn, id, probability, driver),
        |pool| update_scenario_probability_postgres(pool, id, probability, driver),
    )
}

pub fn update_scenario_backend(
    backend: &BackendConnection,
    id: i64,
    description: Option<&str>,
    asset_impact: Option<&str>,
    triggers: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_scenario(conn, id, description, asset_impact, triggers, status),
        |pool| update_scenario_postgres(pool, id, description, asset_impact, triggers, status),
    )
}

pub fn remove_scenario_backend(backend: &BackendConnection, id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_scenario(conn, id),
        |pool| remove_scenario_postgres(pool, id),
    )
}

pub fn add_signal_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    signal: &str,
    status: Option<&str>,
    evidence: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_signal(conn, scenario_id, signal, status, evidence, source),
        |pool| add_signal_postgres(pool, scenario_id, signal, status, evidence, source),
    )
}

pub fn list_signals_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    status_filter: Option<&str>,
) -> Result<Vec<ScenarioSignal>> {
    query::dispatch(
        backend,
        |conn| list_signals(conn, scenario_id, status_filter),
        |pool| list_signals_postgres(pool, scenario_id, status_filter),
    )
}

pub fn update_signal_backend(
    backend: &BackendConnection,
    signal_id: i64,
    status: Option<&str>,
    evidence: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_signal(conn, signal_id, status, evidence),
        |pool| update_signal_postgres(pool, signal_id, status, evidence),
    )
}

pub fn remove_signal_backend(backend: &BackendConnection, signal_id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| remove_signal(conn, signal_id),
        |pool| remove_signal_postgres(pool, signal_id),
    )
}

pub fn get_history_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioHistoryEntry>> {
    query::dispatch(
        backend,
        |conn| get_history(conn, scenario_id, limit),
        |pool| get_history_postgres(pool, scenario_id, limit),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenarios (
                id BIGSERIAL PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                probability DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                description TEXT,
                asset_impact TEXT,
                triggers TEXT,
                historical_precedent TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_signals (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                signal TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'watching',
                evidence TEXT,
                source TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_history (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                probability DOUBLE PRECISION NOT NULL,
                driver TEXT,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_signals_scenario ON scenario_signals(scenario_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_history_scenario ON scenario_history(scenario_id)")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_scenario_postgres(
    pool: &PgPool,
    name: &str,
    probability: f64,
    description: Option<&str>,
    asset_impact: Option<&str>,
    triggers: Option<&str>,
    precedent: Option<&str>,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenarios (name, probability, description, asset_impact, triggers, historical_precedent)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(name)
        .bind(probability)
        .bind(description)
        .bind(asset_impact)
        .bind(triggers)
        .bind(precedent)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_scenarios_postgres(pool: &PgPool, status_filter: Option<&str>) -> Result<Vec<Scenario>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<ScenarioRow> =
        if let Some(status) = status_filter {
            runtime.block_on(async {
                sqlx::query_as(
                    "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text
                     FROM scenarios
                     WHERE status = $1
                     ORDER BY probability DESC",
                )
                .bind(status)
                .fetch_all(pool)
                .await
            })?
        } else {
            runtime.block_on(async {
                sqlx::query_as(
                    "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text
                     FROM scenarios
                     ORDER BY probability DESC",
                )
                .fetch_all(pool)
                .await
            })?
        };

    Ok(rows
        .into_iter()
        .map(|r| Scenario {
            id: r.0,
            name: r.1,
            probability: r.2,
            description: r.3,
            asset_impact: r.4,
            triggers: r.5,
            historical_precedent: r.6,
            status: r.7,
            created_at: r.8,
            updated_at: r.9,
        })
        .collect())
}

fn get_scenario_by_name_postgres(pool: &PgPool, name: &str) -> Result<Option<Scenario>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let row: Option<ScenarioRow> =
        runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text
                 FROM scenarios
                 WHERE name = $1",
            )
            .bind(name)
            .fetch_optional(pool)
            .await
        })?;
    Ok(row.map(|r| Scenario {
        id: r.0,
        name: r.1,
        probability: r.2,
        description: r.3,
        asset_impact: r.4,
        triggers: r.5,
        historical_precedent: r.6,
        status: r.7,
        created_at: r.8,
        updated_at: r.9,
    }))
}

fn update_scenario_probability_postgres(
    pool: &PgPool,
    id: i64,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "INSERT INTO scenario_history (scenario_id, probability, driver)
             SELECT id, probability, $1 FROM scenarios WHERE id = $2",
        )
        .bind(driver)
        .bind(id)
        .execute(pool)
        .await?;
        sqlx::query("UPDATE scenarios SET probability = $1, updated_at = NOW() WHERE id = $2")
            .bind(probability)
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn update_scenario_postgres(
    pool: &PgPool,
    id: i64,
    description: Option<&str>,
    asset_impact: Option<&str>,
    triggers: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "UPDATE scenarios
             SET description = COALESCE($1, description),
                 asset_impact = COALESCE($2, asset_impact),
                 triggers = COALESCE($3, triggers),
                 status = COALESCE($4, status),
                 updated_at = NOW()
             WHERE id = $5",
        )
        .bind(description)
        .bind(asset_impact)
        .bind(triggers)
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn remove_scenario_postgres(pool: &PgPool, id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query("DELETE FROM scenarios WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_signal_postgres(
    pool: &PgPool,
    scenario_id: i64,
    signal: &str,
    status: Option<&str>,
    evidence: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_signals (scenario_id, signal, status, evidence, source)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(scenario_id)
        .bind(signal)
        .bind(status.unwrap_or("watching"))
        .bind(evidence)
        .bind(source)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_signals_postgres(
    pool: &PgPool,
    scenario_id: i64,
    status_filter: Option<&str>,
) -> Result<Vec<ScenarioSignal>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<ScenarioSignalRow> =
        if let Some(status) = status_filter {
            runtime.block_on(async {
                sqlx::query_as(
                    "SELECT id, scenario_id, signal, status, evidence, source, updated_at::text
                     FROM scenario_signals
                     WHERE scenario_id = $1 AND status = $2
                     ORDER BY updated_at DESC",
                )
                .bind(scenario_id)
                .bind(status)
                .fetch_all(pool)
                .await
            })?
        } else {
            runtime.block_on(async {
                sqlx::query_as(
                    "SELECT id, scenario_id, signal, status, evidence, source, updated_at::text
                     FROM scenario_signals
                     WHERE scenario_id = $1
                     ORDER BY updated_at DESC",
                )
                .bind(scenario_id)
                .fetch_all(pool)
                .await
            })?
        };
    Ok(rows
        .into_iter()
        .map(|r| ScenarioSignal {
            id: r.0,
            scenario_id: r.1,
            signal: r.2,
            status: r.3,
            evidence: r.4,
            source: r.5,
            updated_at: r.6,
        })
        .collect())
}

fn update_signal_postgres(
    pool: &PgPool,
    signal_id: i64,
    status: Option<&str>,
    evidence: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "UPDATE scenario_signals
             SET status = COALESCE($1, status),
                 evidence = COALESCE($2, evidence),
                 updated_at = NOW()
             WHERE id = $3",
        )
        .bind(status)
        .bind(evidence)
        .bind(signal_id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn remove_signal_postgres(pool: &PgPool, signal_id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query("DELETE FROM scenario_signals WHERE id = $1")
            .bind(signal_id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn get_history_postgres(
    pool: &PgPool,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioHistoryEntry>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<(i64, i64, f64, Option<String>, String)> = if let Some(limit) = limit {
        runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, scenario_id, probability, driver, recorded_at::text
                 FROM scenario_history
                 WHERE scenario_id = $1
                 ORDER BY recorded_at DESC
                 LIMIT $2",
            )
            .bind(scenario_id)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        })?
    } else {
        runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, scenario_id, probability, driver, recorded_at::text
                 FROM scenario_history
                 WHERE scenario_id = $1
                 ORDER BY recorded_at DESC",
            )
            .bind(scenario_id)
            .fetch_all(pool)
            .await
        })?
    };
    Ok(rows
        .into_iter()
        .map(|r| ScenarioHistoryEntry {
            id: r.0,
            scenario_id: r.1,
            probability: r.2,
            driver: r.3,
            recorded_at: r.4,
        })
        .collect())
}
