use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

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
