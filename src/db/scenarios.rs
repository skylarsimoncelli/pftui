use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

type ScenarioExtRow = (
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
    String,         // phase
    Option<String>, // resolved_at
    Option<String>, // resolution_notes
);
type ScenarioSignalRow = (
    i64,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
);

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
    #[serde(default = "default_phase")]
    pub phase: String,
    pub resolved_at: Option<String>,
    pub resolution_notes: Option<String>,
}

fn default_phase() -> String {
    "hypothesis".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioBranch {
    pub id: i64,
    pub scenario_id: i64,
    pub name: String,
    pub probability: f64,
    pub description: Option<String>,
    pub sort_order: i32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioImpact {
    pub id: i64,
    pub scenario_id: i64,
    pub branch_id: Option<i64>,
    pub symbol: String,
    pub direction: String,
    pub tier: String,
    pub mechanism: Option<String>,
    pub parent_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioIndicator {
    pub id: i64,
    pub scenario_id: i64,
    pub branch_id: Option<i64>,
    pub impact_id: Option<i64>,
    pub symbol: String,
    pub metric: String,
    pub operator: String,
    pub threshold: String,
    pub label: String,
    pub status: String,
    pub triggered_at: Option<String>,
    pub last_value: Option<String>,
    pub last_checked: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioUpdate {
    pub id: i64,
    pub scenario_id: i64,
    pub branch_id: Option<i64>,
    pub headline: String,
    pub detail: Option<String>,
    pub severity: String,
    pub source: Option<String>,
    pub source_agent: Option<String>,
    pub next_decision: Option<String>,
    pub next_decision_at: Option<String>,
    pub created_at: String,
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
            phase: row.get(10).unwrap_or_else(|_| "hypothesis".to_string()),
            resolved_at: row.get(11).unwrap_or(None),
            resolution_notes: row.get(12).unwrap_or(None),
        })
    }
}

impl ScenarioBranch {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            scenario_id: row.get(1)?,
            name: row.get(2)?,
            probability: row.get(3)?,
            description: row.get(4)?,
            sort_order: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }
}

impl ScenarioImpact {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            scenario_id: row.get(1)?,
            branch_id: row.get(2)?,
            symbol: row.get(3)?,
            direction: row.get(4)?,
            tier: row.get(5)?,
            mechanism: row.get(6)?,
            parent_id: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    }
}

impl ScenarioIndicator {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            scenario_id: row.get(1)?,
            branch_id: row.get(2)?,
            impact_id: row.get(3)?,
            symbol: row.get(4)?,
            metric: row.get(5)?,
            operator: row.get(6)?,
            threshold: row.get(7)?,
            label: row.get(8)?,
            status: row.get(9)?,
            triggered_at: row.get(10)?,
            last_value: row.get(11)?,
            last_checked: row.get(12)?,
            created_at: row.get(13)?,
            updated_at: row.get(14)?,
        })
    }
}

impl ScenarioUpdate {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            scenario_id: row.get(1)?,
            branch_id: row.get(2)?,
            headline: row.get(3)?,
            detail: row.get(4)?,
            severity: row.get(5)?,
            source: row.get(6)?,
            source_agent: row.get(7)?,
            next_decision: row.get(8)?,
            next_decision_at: row.get(9)?,
            created_at: row.get(10)?,
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

pub fn list_scenarios(conn: &Connection, status_filter: Option<&str>) -> Result<Vec<Scenario>> {
    let query = if let Some(status) = status_filter {
        format!(
            "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes
             FROM scenarios
             WHERE status = '{}'
             ORDER BY probability DESC",
            status
        )
    } else {
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes
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

/// List scenarios filtered by phase (hypothesis, active, resolved)
pub fn list_scenarios_by_phase(conn: &Connection, phase: &str) -> Result<Vec<Scenario>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes
         FROM scenarios
         WHERE phase = ?
         ORDER BY probability DESC",
    )?;
    let rows = stmt.query_map([phase], Scenario::from_row)?;
    let mut scenarios = Vec::new();
    for row in rows {
        scenarios.push(row?);
    }
    Ok(scenarios)
}

pub fn get_scenario_by_name(conn: &Connection, name: &str) -> Result<Option<Scenario>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes
         FROM scenarios
         WHERE name = ?",
    )?;

    let mut rows = stmt.query_map([name], Scenario::from_row)?;
    Ok(rows.next().transpose()?)
}

/// Promote a scenario from hypothesis to active situation
pub fn promote_scenario(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE scenarios SET phase = 'active', updated_at = datetime('now') WHERE id = ? AND phase = 'hypothesis'",
        [id],
    )?;
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver) SELECT id, probability, 'Promoted to active situation' FROM scenarios WHERE id = ?",
        [id],
    )?;
    Ok(())
}

/// Demote a scenario from active back to hypothesis
pub fn demote_scenario(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE scenarios SET phase = 'hypothesis', updated_at = datetime('now') WHERE id = ? AND phase = 'active'",
        [id],
    )?;
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver) SELECT id, probability, 'Demoted back to hypothesis' FROM scenarios WHERE id = ?",
        [id],
    )?;
    Ok(())
}

/// Resolve a scenario with outcome notes
pub fn resolve_scenario(conn: &Connection, id: i64, resolution_notes: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE scenarios SET phase = 'resolved', status = 'resolved', resolved_at = datetime('now'), resolution_notes = ?, updated_at = datetime('now') WHERE id = ?",
        params![resolution_notes, id],
    )?;
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver) SELECT id, probability, 'Resolved' FROM scenarios WHERE id = ?",
        [id],
    )?;
    Ok(())
}

// --- Branch CRUD ---

pub fn add_branch(
    conn: &Connection,
    scenario_id: i64,
    name: &str,
    probability: f64,
    description: Option<&str>,
) -> Result<i64> {
    let sort_order: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM scenario_branches WHERE scenario_id = ?",
            [scenario_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO scenario_branches (scenario_id, name, probability, description, sort_order) VALUES (?, ?, ?, ?, ?)",
        params![scenario_id, name, probability, description, sort_order],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_branches(conn: &Connection, scenario_id: i64) -> Result<Vec<ScenarioBranch>> {
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, name, probability, description, sort_order, status, created_at, updated_at
         FROM scenario_branches WHERE scenario_id = ? ORDER BY sort_order",
    )?;
    let rows = stmt.query_map([scenario_id], ScenarioBranch::from_row)?;
    let mut branches = Vec::new();
    for row in rows {
        branches.push(row?);
    }
    Ok(branches)
}

pub fn update_branch(
    conn: &Connection,
    branch_id: i64,
    probability: Option<f64>,
    status: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    let mut updates = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(p) = probability {
        updates.push("probability = ?");
        params_vec.push(Box::new(p));
    }
    if let Some(s) = status {
        updates.push("status = ?");
        params_vec.push(Box::new(s.to_string()));
    }
    if let Some(d) = description {
        updates.push("description = ?");
        params_vec.push(Box::new(d.to_string()));
    }
    if updates.is_empty() {
        return Ok(());
    }
    updates.push("updated_at = datetime('now')");
    let query = format!(
        "UPDATE scenario_branches SET {} WHERE id = ?",
        updates.join(", ")
    );
    params_vec.push(Box::new(branch_id));
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    conn.execute(&query, params_refs.as_slice())?;
    Ok(())
}

// --- Impact CRUD ---

#[allow(clippy::too_many_arguments)]
pub fn add_impact(
    conn: &Connection,
    scenario_id: i64,
    branch_id: Option<i64>,
    symbol: &str,
    direction: &str,
    tier: &str,
    mechanism: Option<&str>,
    parent_id: Option<i64>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO scenario_impacts (scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_impacts(conn: &Connection, scenario_id: i64) -> Result<Vec<ScenarioImpact>> {
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id, created_at, updated_at
         FROM scenario_impacts WHERE scenario_id = ? ORDER BY tier, id",
    )?;
    let rows = stmt.query_map([scenario_id], ScenarioImpact::from_row)?;
    let mut impacts = Vec::new();
    for row in rows {
        impacts.push(row?);
    }
    Ok(impacts)
}

pub fn list_impacts_by_symbol(conn: &Connection, symbol: &str) -> Result<Vec<ScenarioImpact>> {
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id, created_at, updated_at
         FROM scenario_impacts WHERE symbol = ? ORDER BY scenario_id, tier",
    )?;
    let rows = stmt.query_map([symbol], ScenarioImpact::from_row)?;
    let mut impacts = Vec::new();
    for row in rows {
        impacts.push(row?);
    }
    Ok(impacts)
}

// --- Indicator CRUD ---

#[allow(clippy::too_many_arguments)]
pub fn add_indicator(
    conn: &Connection,
    scenario_id: i64,
    branch_id: Option<i64>,
    impact_id: Option<i64>,
    symbol: &str,
    metric: &str,
    operator: &str,
    threshold: &str,
    label: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO scenario_indicators (scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_indicators(conn: &Connection, scenario_id: i64) -> Result<Vec<ScenarioIndicator>> {
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at, last_value, last_checked, created_at, updated_at
         FROM scenario_indicators WHERE scenario_id = ? ORDER BY status, id",
    )?;
    let rows = stmt.query_map([scenario_id], ScenarioIndicator::from_row)?;
    let mut indicators = Vec::new();
    for row in rows {
        indicators.push(row?);
    }
    Ok(indicators)
}

/// List ALL indicators with status='watching' across all scenarios (for refresh pipeline evaluation).
pub fn list_all_watching_indicators(
    conn: &Connection,
) -> Result<Vec<ScenarioIndicator>> {
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at, last_value, last_checked, created_at, updated_at
         FROM scenario_indicators WHERE status = 'watching' ORDER BY scenario_id, id",
    )?;
    let rows = stmt.query_map([], ScenarioIndicator::from_row)?;
    let mut indicators = Vec::new();
    for row in rows {
        indicators.push(row?);
    }
    Ok(indicators)
}

/// Update an indicator's evaluation result (last_value, last_checked, and optionally trigger it).
pub fn update_indicator_evaluation(
    conn: &Connection,
    indicator_id: i64,
    last_value: &str,
    triggered: bool,
) -> Result<()> {
    if triggered {
        conn.execute(
            "UPDATE scenario_indicators SET last_value = ?, last_checked = datetime('now'), status = 'triggered', triggered_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
            params![last_value, indicator_id],
        )?;
    } else {
        conn.execute(
            "UPDATE scenario_indicators SET last_value = ?, last_checked = datetime('now'), updated_at = datetime('now') WHERE id = ?",
            params![last_value, indicator_id],
        )?;
    }
    Ok(())
}

// --- Update log CRUD ---

#[allow(clippy::too_many_arguments)]
pub fn add_update(
    conn: &Connection,
    scenario_id: i64,
    branch_id: Option<i64>,
    headline: &str,
    detail: Option<&str>,
    severity: &str,
    source: Option<&str>,
    source_agent: Option<&str>,
    next_decision: Option<&str>,
    next_decision_at: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO scenario_updates (scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_updates(
    conn: &Connection,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioUpdate>> {
    let query = if let Some(lim) = limit {
        format!(
            "SELECT id, scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at, created_at
             FROM scenario_updates WHERE scenario_id = {} ORDER BY created_at DESC LIMIT {}",
            scenario_id, lim
        )
    } else {
        format!(
            "SELECT id, scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at, created_at
             FROM scenario_updates WHERE scenario_id = {} ORDER BY created_at DESC",
            scenario_id
        )
    };
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], ScenarioUpdate::from_row)?;
    let mut updates = Vec::new();
    for row in rows {
        updates.push(row?);
    }
    Ok(updates)
}

pub fn update_scenario_probability(
    conn: &Connection,
    id: i64,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    // Update scenario first
    conn.execute(
        "UPDATE scenarios SET probability = ?, updated_at = datetime('now') WHERE id = ?",
        params![probability, id],
    )?;

    // Snapshot new probability to history after update
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver) VALUES (?, ?, ?)",
        params![id, probability, driver],
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

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

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

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

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
             ORDER BY id DESC
             LIMIT {}",
            scenario_id, lim
        )
    } else {
        format!(
            "SELECT id, scenario_id, probability, driver, recorded_at
             FROM scenario_history
             WHERE scenario_id = {}
             ORDER BY id DESC",
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
        |conn| {
            add_scenario(
                conn,
                name,
                probability,
                description,
                asset_impact,
                triggers,
                precedent,
            )
        },
        |pool| {
            add_scenario_postgres(
                pool,
                name,
                probability,
                description,
                asset_impact,
                triggers,
                precedent,
            )
        },
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

pub fn list_scenarios_by_phase_backend(
    backend: &BackendConnection,
    phase: &str,
) -> Result<Vec<Scenario>> {
    query::dispatch(
        backend,
        |conn| list_scenarios_by_phase(conn, phase),
        |pool| list_scenarios_by_phase_postgres(pool, phase),
    )
}

pub fn promote_scenario_backend(backend: &BackendConnection, id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| promote_scenario(conn, id),
        |pool| promote_scenario_postgres(pool, id),
    )
}

pub fn demote_scenario_backend(backend: &BackendConnection, id: i64) -> Result<()> {
    query::dispatch(
        backend,
        |conn| demote_scenario(conn, id),
        |pool| demote_scenario_postgres(pool, id),
    )
}

pub fn resolve_scenario_backend(
    backend: &BackendConnection,
    id: i64,
    resolution_notes: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| resolve_scenario(conn, id, resolution_notes),
        |pool| resolve_scenario_postgres(pool, id, resolution_notes),
    )
}

pub fn add_branch_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    name: &str,
    probability: f64,
    description: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_branch(conn, scenario_id, name, probability, description),
        |pool| add_branch_postgres(pool, scenario_id, name, probability, description),
    )
}

pub fn list_branches_backend(
    backend: &BackendConnection,
    scenario_id: i64,
) -> Result<Vec<ScenarioBranch>> {
    query::dispatch(
        backend,
        |conn| list_branches(conn, scenario_id),
        |pool| list_branches_postgres(pool, scenario_id),
    )
}

pub fn update_branch_backend(
    backend: &BackendConnection,
    branch_id: i64,
    probability: Option<f64>,
    status: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_branch(conn, branch_id, probability, status, description),
        |pool| update_branch_postgres(pool, branch_id, probability, status, description),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_impact_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    branch_id: Option<i64>,
    symbol: &str,
    direction: &str,
    tier: &str,
    mechanism: Option<&str>,
    parent_id: Option<i64>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            add_impact(
                conn,
                scenario_id,
                branch_id,
                symbol,
                direction,
                tier,
                mechanism,
                parent_id,
            )
        },
        |pool| {
            add_impact_postgres(
                pool,
                scenario_id,
                branch_id,
                symbol,
                direction,
                tier,
                mechanism,
                parent_id,
            )
        },
    )
}

pub fn list_impacts_backend(
    backend: &BackendConnection,
    scenario_id: i64,
) -> Result<Vec<ScenarioImpact>> {
    query::dispatch(
        backend,
        |conn| list_impacts(conn, scenario_id),
        |pool| list_impacts_postgres(pool, scenario_id),
    )
}

pub fn list_impacts_by_symbol_backend(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<Vec<ScenarioImpact>> {
    query::dispatch(
        backend,
        |conn| list_impacts_by_symbol(conn, symbol),
        |pool| list_impacts_by_symbol_postgres(pool, symbol),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_indicator_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    branch_id: Option<i64>,
    impact_id: Option<i64>,
    symbol: &str,
    metric: &str,
    operator: &str,
    threshold: &str,
    label: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            add_indicator(
                conn,
                scenario_id,
                branch_id,
                impact_id,
                symbol,
                metric,
                operator,
                threshold,
                label,
            )
        },
        |pool| {
            add_indicator_postgres(
                pool,
                scenario_id,
                branch_id,
                impact_id,
                symbol,
                metric,
                operator,
                threshold,
                label,
            )
        },
    )
}

pub fn list_indicators_backend(
    backend: &BackendConnection,
    scenario_id: i64,
) -> Result<Vec<ScenarioIndicator>> {
    query::dispatch(
        backend,
        |conn| list_indicators(conn, scenario_id),
        |pool| list_indicators_postgres(pool, scenario_id),
    )
}

pub fn list_all_watching_indicators_backend(
    backend: &BackendConnection,
) -> Result<Vec<ScenarioIndicator>> {
    query::dispatch(
        backend,
        list_all_watching_indicators,
        list_all_watching_indicators_postgres,
    )
}

pub fn update_indicator_evaluation_backend(
    backend: &BackendConnection,
    indicator_id: i64,
    last_value: &str,
    triggered: bool,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_indicator_evaluation(conn, indicator_id, last_value, triggered),
        |pool| update_indicator_evaluation_postgres(pool, indicator_id, last_value, triggered),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_update_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    branch_id: Option<i64>,
    headline: &str,
    detail: Option<&str>,
    severity: &str,
    source: Option<&str>,
    source_agent: Option<&str>,
    next_decision: Option<&str>,
    next_decision_at: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            add_update(
                conn,
                scenario_id,
                branch_id,
                headline,
                detail,
                severity,
                source,
                source_agent,
                next_decision,
                next_decision_at,
            )
        },
        |pool| {
            add_update_postgres(
                pool,
                scenario_id,
                branch_id,
                headline,
                detail,
                severity,
                source,
                source_agent,
                next_decision,
                next_decision_at,
            )
        },
    )
}

pub fn list_updates_backend(
    backend: &BackendConnection,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioUpdate>> {
    query::dispatch(
        backend,
        |conn| list_updates(conn, scenario_id, limit),
        |pool| list_updates_postgres(pool, scenario_id, limit),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
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
    let id: i64 = crate::db::pg_runtime::block_on(async {
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

fn scenario_from_ext_row(r: ScenarioExtRow) -> Scenario {
    Scenario {
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
        phase: r.10,
        resolved_at: r.11,
        resolution_notes: r.12,
    }
}

fn list_scenarios_postgres(pool: &PgPool, status_filter: Option<&str>) -> Result<Vec<Scenario>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<ScenarioExtRow> = if let Some(status) = status_filter {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                    "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes
                     FROM scenarios
                     WHERE status = $1
                     ORDER BY probability DESC",
                )
                .bind(status)
                .fetch_all(pool)
                .await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                    "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes
                     FROM scenarios
                     ORDER BY probability DESC",
                )
                .fetch_all(pool)
                .await
        })?
    };

    Ok(rows.into_iter().map(scenario_from_ext_row).collect())
}

fn get_scenario_by_name_postgres(pool: &PgPool, name: &str) -> Result<Option<Scenario>> {
    ensure_tables_postgres(pool)?;
    let row: Option<ScenarioExtRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
                "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes
                 FROM scenarios
                 WHERE name = $1",
            )
            .bind(name)
            .fetch_optional(pool)
            .await
    })?;
    Ok(row.map(scenario_from_ext_row))
}

fn update_scenario_probability_postgres(
    pool: &PgPool,
    id: i64,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE scenarios SET probability = $1, updated_at = NOW() WHERE id = $2")
            .bind(probability)
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query(
            "INSERT INTO scenario_history (scenario_id, probability, driver) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(probability)
        .bind(driver)
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
    crate::db::pg_runtime::block_on(async {
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
    crate::db::pg_runtime::block_on(async {
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
    let id: i64 = crate::db::pg_runtime::block_on(async {
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
    let rows: Vec<ScenarioSignalRow> = if let Some(status) = status_filter {
        crate::db::pg_runtime::block_on(async {
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
        crate::db::pg_runtime::block_on(async {
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
    crate::db::pg_runtime::block_on(async {
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
    crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM scenario_signals WHERE id = $1")
            .bind(signal_id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn list_scenarios_by_phase_postgres(pool: &PgPool, phase: &str) -> Result<Vec<Scenario>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<ScenarioExtRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes
             FROM scenarios WHERE phase = $1 ORDER BY probability DESC",
        )
        .bind(phase)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(scenario_from_ext_row).collect())
}

fn promote_scenario_postgres(pool: &PgPool, id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE scenarios SET phase = 'active', updated_at = NOW() WHERE id = $1 AND phase = 'hypothesis'")
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query("INSERT INTO scenario_history (scenario_id, probability, driver) SELECT id, probability, 'Promoted to active situation' FROM scenarios WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn demote_scenario_postgres(pool: &PgPool, id: i64) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE scenarios SET phase = 'hypothesis', updated_at = NOW() WHERE id = $1 AND phase = 'active'")
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query("INSERT INTO scenario_history (scenario_id, probability, driver) SELECT id, probability, 'Demoted back to hypothesis' FROM scenarios WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn resolve_scenario_postgres(pool: &PgPool, id: i64, resolution_notes: Option<&str>) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE scenarios SET phase = 'resolved', status = 'resolved', resolved_at = NOW(), resolution_notes = $1, updated_at = NOW() WHERE id = $2")
            .bind(resolution_notes)
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query("INSERT INTO scenario_history (scenario_id, probability, driver) SELECT id, probability, 'Resolved' FROM scenarios WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type BranchRow = (
    i64,
    i64,
    String,
    f64,
    Option<String>,
    i32,
    String,
    String,
    String,
);

fn add_branch_postgres(
    pool: &PgPool,
    scenario_id: i64,
    name: &str,
    probability: f64,
    description: Option<&str>,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_branches (scenario_id, name, probability, description, sort_order)
             VALUES ($1, $2, $3, $4, COALESCE((SELECT MAX(sort_order) + 1 FROM scenario_branches WHERE scenario_id = $1), 0))
             RETURNING id",
        )
        .bind(scenario_id)
        .bind(name)
        .bind(probability)
        .bind(description)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_branches_postgres(pool: &PgPool, scenario_id: i64) -> Result<Vec<ScenarioBranch>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<BranchRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, scenario_id, name, probability, description, sort_order, status, created_at::text, updated_at::text
             FROM scenario_branches WHERE scenario_id = $1 ORDER BY sort_order",
        )
        .bind(scenario_id)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ScenarioBranch {
            id: r.0,
            scenario_id: r.1,
            name: r.2,
            probability: r.3,
            description: r.4,
            sort_order: r.5,
            status: r.6,
            created_at: r.7,
            updated_at: r.8,
        })
        .collect())
}

fn update_branch_postgres(
    pool: &PgPool,
    branch_id: i64,
    probability: Option<f64>,
    status: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE scenario_branches SET
               probability = COALESCE($1, probability),
               status = COALESCE($2, status),
               description = COALESCE($3, description),
               updated_at = NOW()
             WHERE id = $4",
        )
        .bind(probability)
        .bind(status)
        .bind(description)
        .bind(branch_id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

type ImpactRow = (
    i64,
    i64,
    Option<i64>,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    String,
    String,
);

#[allow(clippy::too_many_arguments)]
fn add_impact_postgres(
    pool: &PgPool,
    scenario_id: i64,
    branch_id: Option<i64>,
    symbol: &str,
    direction: &str,
    tier: &str,
    mechanism: Option<&str>,
    parent_id: Option<i64>,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_impacts (scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(scenario_id).bind(branch_id).bind(symbol).bind(direction).bind(tier).bind(mechanism).bind(parent_id)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_impacts_postgres(pool: &PgPool, scenario_id: i64) -> Result<Vec<ScenarioImpact>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<ImpactRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id, created_at::text, updated_at::text
             FROM scenario_impacts WHERE scenario_id = $1 ORDER BY tier, id",
        )
        .bind(scenario_id)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ScenarioImpact {
            id: r.0,
            scenario_id: r.1,
            branch_id: r.2,
            symbol: r.3,
            direction: r.4,
            tier: r.5,
            mechanism: r.6,
            parent_id: r.7,
            created_at: r.8,
            updated_at: r.9,
        })
        .collect())
}

fn list_impacts_by_symbol_postgres(pool: &PgPool, symbol: &str) -> Result<Vec<ScenarioImpact>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<ImpactRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id, created_at::text, updated_at::text
             FROM scenario_impacts WHERE symbol = $1 ORDER BY scenario_id, tier",
        )
        .bind(symbol)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ScenarioImpact {
            id: r.0,
            scenario_id: r.1,
            branch_id: r.2,
            symbol: r.3,
            direction: r.4,
            tier: r.5,
            mechanism: r.6,
            parent_id: r.7,
            created_at: r.8,
            updated_at: r.9,
        })
        .collect())
}

type IndicatorRow = (
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
);

#[allow(clippy::too_many_arguments)]
fn add_indicator_postgres(
    pool: &PgPool,
    scenario_id: i64,
    branch_id: Option<i64>,
    impact_id: Option<i64>,
    symbol: &str,
    metric: &str,
    operator: &str,
    threshold: &str,
    label: &str,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_indicators (scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(scenario_id).bind(branch_id).bind(impact_id).bind(symbol).bind(metric).bind(operator).bind(threshold).bind(label)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_indicators_postgres(pool: &PgPool, scenario_id: i64) -> Result<Vec<ScenarioIndicator>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<IndicatorRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at::text, last_value, last_checked::text, created_at::text, updated_at::text
             FROM scenario_indicators WHERE scenario_id = $1 ORDER BY status, id",
        )
        .bind(scenario_id)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ScenarioIndicator {
            id: r.0,
            scenario_id: r.1,
            branch_id: r.2,
            impact_id: r.3,
            symbol: r.4,
            metric: r.5,
            operator: r.6,
            threshold: r.7,
            label: r.8,
            status: r.9,
            triggered_at: r.10,
            last_value: r.11,
            last_checked: r.12,
            created_at: r.13,
            updated_at: r.14,
        })
        .collect())
}

fn list_all_watching_indicators_postgres(pool: &PgPool) -> Result<Vec<ScenarioIndicator>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<IndicatorRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at::text, last_value, last_checked::text, created_at::text, updated_at::text
             FROM scenario_indicators WHERE status = 'watching' ORDER BY scenario_id, id",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| ScenarioIndicator {
            id: r.0,
            scenario_id: r.1,
            branch_id: r.2,
            impact_id: r.3,
            symbol: r.4,
            metric: r.5,
            operator: r.6,
            threshold: r.7,
            label: r.8,
            status: r.9,
            triggered_at: r.10,
            last_value: r.11,
            last_checked: r.12,
            created_at: r.13,
            updated_at: r.14,
        })
        .collect())
}

fn update_indicator_evaluation_postgres(
    pool: &PgPool,
    indicator_id: i64,
    last_value: &str,
    triggered: bool,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    if triggered {
        crate::db::pg_runtime::block_on(async {
            sqlx::query(
                "UPDATE scenario_indicators SET last_value = $1, last_checked = now(), status = 'triggered', triggered_at = now(), updated_at = now() WHERE id = $2",
            )
            .bind(last_value)
            .bind(indicator_id)
            .execute(pool)
            .await
        })?;
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query(
                "UPDATE scenario_indicators SET last_value = $1, last_checked = now(), updated_at = now() WHERE id = $2",
            )
            .bind(last_value)
            .bind(indicator_id)
            .execute(pool)
            .await
        })?;
    }
    Ok(())
}

type UpdateRow = (
    i64,
    i64,
    Option<i64>,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

#[allow(clippy::too_many_arguments)]
fn add_update_postgres(
    pool: &PgPool,
    scenario_id: i64,
    branch_id: Option<i64>,
    headline: &str,
    detail: Option<&str>,
    severity: &str,
    source: Option<&str>,
    source_agent: Option<&str>,
    next_decision: Option<&str>,
    next_decision_at: Option<&str>,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_updates (scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::timestamptz) RETURNING id",
        )
        .bind(scenario_id).bind(branch_id).bind(headline).bind(detail).bind(severity).bind(source).bind(source_agent).bind(next_decision).bind(next_decision_at)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_updates_postgres(
    pool: &PgPool,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioUpdate>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<UpdateRow> = if let Some(lim) = limit {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at::text, created_at::text
                 FROM scenario_updates WHERE scenario_id = $1 ORDER BY created_at DESC LIMIT $2",
            )
            .bind(scenario_id).bind(lim as i64)
            .fetch_all(pool)
            .await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at::text, created_at::text
                 FROM scenario_updates WHERE scenario_id = $1 ORDER BY created_at DESC",
            )
            .bind(scenario_id)
            .fetch_all(pool)
            .await
        })?
    };
    Ok(rows
        .into_iter()
        .map(|r| ScenarioUpdate {
            id: r.0,
            scenario_id: r.1,
            branch_id: r.2,
            headline: r.3,
            detail: r.4,
            severity: r.5,
            source: r.6,
            source_agent: r.7,
            next_decision: r.8,
            next_decision_at: r.9,
            created_at: r.10,
        })
        .collect())
}

fn get_history_postgres(
    pool: &PgPool,
    scenario_id: i64,
    limit: Option<usize>,
) -> Result<Vec<ScenarioHistoryEntry>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<(i64, i64, f64, Option<String>, String)> = if let Some(limit) = limit {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, scenario_id, probability, driver, recorded_at::text
             FROM scenario_history
             WHERE scenario_id = $1
             ORDER BY id DESC
             LIMIT $2",
            )
            .bind(scenario_id)
            .bind(limit as i64)
            .fetch_all(pool)
            .await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, scenario_id, probability, driver, recorded_at::text
             FROM scenario_history
             WHERE scenario_id = $1
             ORDER BY id DESC",
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
