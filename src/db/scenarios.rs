use anyhow::{bail, Result};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Canonical name for the residual "Other / Unmodelled" scenario row.
///
/// `docs/ANALYTICS-SPEC.md` (Scenario Probability Semantics section) defines the
/// normalized scenario-set model: modeled scenarios plus this residual sum to
/// 100%. The residual is system-managed: it must never be created by the
/// operator/agents directly and its probability is recomputed deterministically
/// from `100 - sum(active modeled scenarios)` after every mutation.
pub const RESIDUAL_SCENARIO_NAME: &str = "Other / Unmodelled";

/// Status marker for the system-managed residual row. Distinct from the
/// operator-facing `active` / `monitoring` / `resolved` lifecycle so that the
/// row is filtered out of the modeled set when computing sums.
pub const RESIDUAL_SCENARIO_STATUS: &str = "system-managed";

/// Tolerance (percentage points) for floating-point modeled-sum comparisons.
/// Small drift below 0.05pp is treated as 0 to avoid spurious overfill errors
/// from f64 accumulation.
const NORMALIZED_EPSILON: f64 = 0.05;

/// Total a normalized scenario set must sum to (in percent).
const NORMALIZED_TOTAL: f64 = 100.0;

/// Classification of the modeled-sum state under the normalized scenario-set
/// model. `Ok` means the modeled rows sum to exactly 100 (within epsilon).
/// `Overfilled` means modeled rows exceed 100 — invalid under the model;
/// `Underfilled` means modeled rows are strictly below 100 (residual covers
/// the gap, materialized or implicit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverfillState {
    Ok,
    Overfilled,
    Underfilled,
}

impl OverfillState {
    pub fn as_str(&self) -> &'static str {
        match self {
            OverfillState::Ok => "ok",
            OverfillState::Overfilled => "overfilled",
            OverfillState::Underfilled => "underfilled",
        }
    }
}

/// Summary of the active scenario set under the normalized model. Surfaces the
/// modeled probability sum, the residual ("Other / Unmodelled") probability,
/// and the overfill state for legacy data quality reporting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedScenarioSet {
    /// Sum of probabilities across active modeled (non-residual) scenarios.
    pub modeled_sum: f64,
    /// Materialized residual probability (the `Other / Unmodelled` row), if it
    /// exists, or the implicit residual `100 - modeled_sum` (clamped at 0) if
    /// not.
    pub residual_probability: f64,
    /// Whether the residual row exists in the database (vs being inferred).
    pub residual_materialized: bool,
    /// Classification of the modeled sum.
    pub overfill_state: OverfillState,
}

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
    Option<f64>,    // base_rate
    Option<String>, // base_rate_reference
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
    /// Reference-class base rate for this scenario (percent), if set.
    #[serde(default)]
    pub base_rate: Option<f64>,
    /// Description of the reference class the base rate comes from.
    #[serde(default)]
    pub base_rate_reference: Option<String>,
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
            base_rate: row.get(13).unwrap_or(None),
            base_rate_reference: row.get(14).unwrap_or(None),
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
    if name.trim().eq_ignore_ascii_case(RESIDUAL_SCENARIO_NAME) {
        bail!(
            "'{}' is the system-managed residual scenario and cannot be created directly",
            RESIDUAL_SCENARIO_NAME
        );
    }
    if !probability.is_finite() || probability < 0.0 {
        bail!(
            "scenario probability must be a non-negative number, got {}",
            probability
        );
    }
    if probability > NORMALIZED_TOTAL + NORMALIZED_EPSILON {
        bail!(
            "scenario probability {:.2}% exceeds 100%; normalized scenario-set model requires modeled probabilities sum to <= 100",
            probability
        );
    }
    let new_sum = modeled_probability_sum(conn)? + probability;
    if new_sum > NORMALIZED_TOTAL + NORMALIZED_EPSILON {
        bail!(
            "adding '{}' at {:.2}% would push modeled scenario sum to {:.2}% (>100%); rebalance the scenario set before adding (see docs/ANALYTICS-SPEC.md normalized scenario-set model)",
            name,
            probability,
            new_sum
        );
    }
    conn.execute(
        "INSERT INTO scenarios (name, probability, description, asset_impact, triggers, historical_precedent)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![name, probability, description, asset_impact, triggers, precedent],
    )?;
    let id = conn.last_insert_rowid();
    ensure_residual_scenario(conn)?;
    recompute_residual_scenario(conn)?;
    Ok(id)
}

/// Ensure a `Other / Unmodelled` system-managed residual row exists in
/// `scenarios`. Idempotent: returns the existing row's id if present, otherwise
/// inserts a new row at probability 100 (the entire set is residual when no
/// modeled scenarios exist yet) and returns its id.
pub fn ensure_residual_scenario(conn: &Connection) -> Result<i64> {
    if let Ok(existing) = conn.query_row(
        "SELECT id FROM scenarios WHERE name = ?1",
        params![RESIDUAL_SCENARIO_NAME],
        |row| row.get::<_, i64>(0),
    ) {
        // Ensure the status marker is current even if the row pre-existed.
        conn.execute(
            "UPDATE scenarios SET status = ?1, phase = 'active' WHERE id = ?2 AND status != ?1",
            params![RESIDUAL_SCENARIO_STATUS, existing],
        )?;
        return Ok(existing);
    }
    conn.execute(
        "INSERT INTO scenarios (name, probability, description, status, phase)
         VALUES (?1, ?2, ?3, ?4, 'active')",
        params![
            RESIDUAL_SCENARIO_NAME,
            NORMALIZED_TOTAL,
            "System-managed residual: 100 - sum(active modeled scenarios). Represents outcomes outside the named scenario set.",
            RESIDUAL_SCENARIO_STATUS,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Sum probabilities across active modeled scenarios (excluding the
/// system-managed residual row and resolved rows). Uses SQLite arithmetic so it
/// is consistent with the constraint checks in `add_scenario` /
/// `update_scenario_probability`.
pub fn modeled_probability_sum(conn: &Connection) -> Result<f64> {
    let sum: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(probability), 0.0)
             FROM scenarios
             WHERE status != ?1
               AND status != 'resolved'
               AND phase != 'resolved'
               AND name != ?2",
            params![RESIDUAL_SCENARIO_STATUS, RESIDUAL_SCENARIO_NAME],
            |row| row.get(0),
        )
        .unwrap_or(0.0);
    Ok(sum)
}

/// Recompute the residual probability as `100 - modeled_sum`, clamping at 0 so
/// the row never goes negative even if legacy data is overfilled (the overfill
/// state surfaces the data-quality issue separately). No-op if the residual row
/// does not exist; call `ensure_residual_scenario` first when needed.
pub fn recompute_residual_scenario(conn: &Connection) -> Result<()> {
    let modeled_sum = modeled_probability_sum(conn)?;
    let residual = (NORMALIZED_TOTAL - modeled_sum).max(0.0);
    conn.execute(
        "UPDATE scenarios SET probability = ?1, updated_at = datetime('now') WHERE name = ?2",
        params![residual, RESIDUAL_SCENARIO_NAME],
    )?;
    Ok(())
}

/// Compute the current normalized scenario set summary: modeled sum, residual
/// probability (materialized or inferred), and overfill classification.
pub fn compute_normalized_set(conn: &Connection) -> Result<NormalizedScenarioSet> {
    let modeled_sum = modeled_probability_sum(conn)?;
    let residual_row: Option<f64> = conn
        .query_row(
            "SELECT probability FROM scenarios WHERE name = ?1",
            params![RESIDUAL_SCENARIO_NAME],
            |row| row.get(0),
        )
        .ok();
    let residual_materialized = residual_row.is_some();
    let residual_probability = residual_row.unwrap_or_else(|| (NORMALIZED_TOTAL - modeled_sum).max(0.0));
    let overfill_state = classify_overfill(modeled_sum);
    Ok(NormalizedScenarioSet {
        modeled_sum,
        residual_probability,
        residual_materialized,
        overfill_state,
    })
}

/// Classify a modeled-probability sum into `Ok` / `Overfilled` / `Underfilled`.
/// Pure function — exposed for tests and callers operating on summed values
/// computed elsewhere (e.g. report build contexts).
pub fn classify_overfill(modeled_sum: f64) -> OverfillState {
    if modeled_sum > NORMALIZED_TOTAL + NORMALIZED_EPSILON {
        OverfillState::Overfilled
    } else if modeled_sum < NORMALIZED_TOTAL - NORMALIZED_EPSILON {
        OverfillState::Underfilled
    } else {
        OverfillState::Ok
    }
}

pub fn list_scenarios(conn: &Connection, status_filter: Option<&str>) -> Result<Vec<Scenario>> {
    let query = if let Some(status) = status_filter {
        format!(
            "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes, base_rate, base_rate_reference
             FROM scenarios
             WHERE status = '{}'
             ORDER BY probability DESC",
            status
        )
    } else {
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes, base_rate, base_rate_reference
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

/// List scenarios filtered by phase (hypothesis, active, resolved). The
/// system-managed residual row is excluded — it has no lifecycle and should not
/// appear in operator-facing phase queries.
pub fn list_scenarios_by_phase(conn: &Connection, phase: &str) -> Result<Vec<Scenario>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes, base_rate, base_rate_reference
         FROM scenarios
         WHERE phase = ?1 AND status != ?2
         ORDER BY probability DESC",
    )?;
    let rows = stmt.query_map(params![phase, RESIDUAL_SCENARIO_STATUS], Scenario::from_row)?;
    let mut scenarios = Vec::new();
    for row in rows {
        scenarios.push(row?);
    }
    Ok(scenarios)
}

pub fn get_scenario_by_name(conn: &Connection, name: &str) -> Result<Option<Scenario>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at, updated_at, phase, resolved_at, resolution_notes, base_rate, base_rate_reference
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
    // A resolved row drops out of the modeled set; the residual absorbs its
    // share so the active scenario set still sums to 100%.
    ensure_residual_scenario(conn)?;
    recompute_residual_scenario(conn)?;
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

/// List ALL active indicators (status='watching' OR 'triggered') across all scenarios.
/// Used by the refresh pipeline to keep last_value and last_checked current for all indicators,
/// not just those still in the 'watching' state.
pub fn list_all_watching_indicators(
    conn: &Connection,
) -> Result<Vec<ScenarioIndicator>> {
    let mut stmt = conn.prepare(
        "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at, last_value, last_checked, created_at, updated_at
         FROM scenario_indicators WHERE status IN ('watching', 'triggered') ORDER BY scenario_id, id",
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
    let now = Utc::now().to_rfc3339();
    if triggered {
        conn.execute(
            "UPDATE scenario_indicators SET last_value = ?, last_checked = ?, status = 'triggered', triggered_at = ?, updated_at = ? WHERE id = ?",
            params![last_value, now, now, now, indicator_id],
        )?;
    } else {
        conn.execute(
            "UPDATE scenario_indicators SET last_value = ?, last_checked = ?, updated_at = ? WHERE id = ?",
            params![last_value, now, now, indicator_id],
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
    let next_decision_at = normalize_optional_timestamp(next_decision_at)?;
    conn.execute(
        "INSERT INTO scenario_updates (scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            scenario_id,
            branch_id,
            headline,
            detail,
            severity,
            source,
            source_agent,
            next_decision,
            next_decision_at
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn normalize_optional_timestamp(raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    normalize_timestamp(raw).map(Some)
}

fn normalize_timestamp(raw: &str) -> Result<String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.with_timezone(&Utc).to_rfc3339());
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z") {
        return Ok(dt.with_timezone(&Utc).to_rfc3339());
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f%#z") {
        return Ok(dt.with_timezone(&Utc).to_rfc3339());
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z") {
        return Ok(dt.with_timezone(&Utc).to_rfc3339());
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%#z") {
        return Ok(dt.with_timezone(&Utc).to_rfc3339());
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).to_rfc3339());
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let dt = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid date '{}'", raw))?;
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).to_rfc3339());
    }

    bail!(
        "invalid next_decision_at '{}'; expected RFC3339, YYYY-MM-DD, or a timestamp with timezone",
        raw
    )
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

/// Default threshold (percentage points) for scenario probability shift alerts.
const SCENARIO_SHIFT_THRESHOLD_PP: f64 = 10.0;

pub fn update_scenario_probability(
    conn: &Connection,
    id: i64,
    probability: f64,
    driver: Option<&str>,
) -> Result<()> {
    // Read current probability before updating
    let old_prob: f64 = conn.query_row(
        "SELECT probability FROM scenarios WHERE id = ?",
        params![id],
        |row| row.get(0),
    )?;
    let scenario_name: String = conn.query_row(
        "SELECT name FROM scenarios WHERE id = ?",
        params![id],
        |row| row.get(0),
    )?;
    let scenario_status: String = conn.query_row(
        "SELECT status FROM scenarios WHERE id = ?",
        params![id],
        |row| row.get(0),
    )?;

    // The residual row is recomputed deterministically by
    // recompute_residual_scenario; reject direct probability writes against it.
    if scenario_name.eq_ignore_ascii_case(RESIDUAL_SCENARIO_NAME)
        || scenario_status == RESIDUAL_SCENARIO_STATUS
    {
        bail!(
            "'{}' is the system-managed residual scenario; its probability is recomputed automatically from 100 - sum(active modeled scenarios)",
            scenario_name
        );
    }

    if !probability.is_finite() || probability < 0.0 {
        bail!(
            "scenario probability must be a non-negative number, got {}",
            probability
        );
    }
    if probability > NORMALIZED_TOTAL + NORMALIZED_EPSILON {
        bail!(
            "scenario probability {:.2}% exceeds 100%; normalized scenario-set model requires modeled probabilities sum to <= 100",
            probability
        );
    }

    // Enforce normalized-set constraint: modeled sum (with this row swapped to
    // the new value) must not exceed 100. We compute the projected sum without
    // mutating storage first so the rejection is atomic.
    let modeled_sum_excluding_self = modeled_probability_sum(conn)? - old_prob;
    let projected_sum = modeled_sum_excluding_self + probability;
    if projected_sum > NORMALIZED_TOTAL + NORMALIZED_EPSILON {
        bail!(
            "updating '{}' to {:.2}% would push modeled scenario sum to {:.2}% (>100%); rebalance the scenario set before updating (see docs/ANALYTICS-SPEC.md normalized scenario-set model)",
            scenario_name,
            probability,
            projected_sum
        );
    }

    // Update scenario
    conn.execute(
        "UPDATE scenarios SET probability = ?, updated_at = datetime('now') WHERE id = ?",
        params![probability, id],
    )?;

    // Snapshot new probability to history after update
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver) VALUES (?, ?, ?)",
        params![id, probability, driver],
    )?;

    // Detect large probability shifts and auto-create scenario alerts
    let delta = probability - old_prob;
    let abs_delta = delta.abs();
    if abs_delta >= SCENARIO_SHIFT_THRESHOLD_PP {
        let direction = if delta > 0.0 { "above" } else { "below" };
        let sign = if delta > 0.0 { "+" } else { "" };
        let rule_text = format!(
            "{} probability shifted {}{:.1}pp ({:.1}% → {:.1}%)",
            scenario_name, sign, delta, old_prob, probability
        );
        let driver_text = driver.unwrap_or("(no driver specified)");
        let trigger_data = json!({
            "scenario_id": id,
            "scenario_name": scenario_name,
            "old_probability": old_prob,
            "new_probability": probability,
            "delta_pp": delta,
            "threshold_pp": SCENARIO_SHIFT_THRESHOLD_PP,
            "driver": driver_text,
        });

        // Create the alert in triggered state
        conn.execute(
            "INSERT INTO alerts (kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, triggered_at)
             VALUES ('scenario', ?, ?, 'probability_shift', ?, 'triggered', ?, 0, 0, datetime('now'))",
            params![
                scenario_name,
                direction,
                SCENARIO_SHIFT_THRESHOLD_PP.to_string(),
                rule_text,
            ],
        )?;
        let alert_id = conn.last_insert_rowid();

        // Also log to triggered_alerts for history
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO triggered_alerts (alert_id, triggered_at, trigger_data, acknowledged) VALUES (?, ?, ?, 0)",
            params![alert_id, now, trigger_data.to_string()],
        )?;
    }

    // Recompute the system-managed residual so the displayed scenario set
    // continues to sum to 100% under the normalized model.
    ensure_residual_scenario(conn)?;
    recompute_residual_scenario(conn)?;

    Ok(())
}

// --- Probability ledger discipline (epistemics R4) ---

/// Maximum cumulative |Δprobability| (percentage points) a single scenario may
/// accrue across all updates in one day without a `--hard-print` bypass.
pub const SCENARIO_DAILY_DELTA_CAP_PP: f64 = 5.0;

/// Default proposer recorded when `--proposer` is not supplied.
pub const DEFAULT_SCENARIO_PROPOSER: &str = "synthesis";

/// One probability-update ledger entry (a `scenario_updates` row that carries
/// the old→new probability move).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioProbabilityLedgerEntry {
    pub proposer: Option<String>,
    pub evidence: Option<String>,
    pub old_probability: Option<f64>,
    pub new_probability: Option<f64>,
    pub hard_print_event: Option<String>,
    pub created_at: String,
}

impl ScenarioProbabilityLedgerEntry {
    fn delta(&self) -> f64 {
        match (self.old_probability, self.new_probability) {
            (Some(old), Some(new)) => new - old,
            _ => 0.0,
        }
    }
}

/// Render a day's probability ledger for inclusion in guard error messages.
fn format_probability_ledger(entries: &[ScenarioProbabilityLedgerEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let delta = e.delta();
            let sign = if delta >= 0.0 { "+" } else { "" };
            format!(
                "  {}  {}  {:.1}% → {:.1}% ({}{:.1}pp){}",
                e.created_at.get(..16).unwrap_or(&e.created_at),
                e.proposer.as_deref().unwrap_or("?"),
                e.old_probability.unwrap_or(0.0),
                e.new_probability.unwrap_or(0.0),
                sign,
                delta,
                e.hard_print_event
                    .as_deref()
                    .map(|h| format!("  [hard-print: {}]", h))
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pure guard checks for a pending probability update against the same-day
/// ledger. Enforces:
///   1. Same-day conflict guard: a second update by a DIFFERENT proposer on
///      the same day requires `--override-conflict`.
///   2. Daily delta cap: today's cumulative |Δ| (including the pending move)
///      must not exceed `SCENARIO_DAILY_DELTA_CAP_PP` — unless a hard data
///      print is cited via `--hard-print`.
pub fn check_probability_ledger_guards(
    scenario_name: &str,
    today_entries: &[ScenarioProbabilityLedgerEntry],
    old_prob: f64,
    new_prob: f64,
    proposer: &str,
    hard_print: Option<&str>,
    override_conflict: bool,
) -> Result<()> {
    // Conflict guard first: silent same-day cross-proposer overwrites were the
    // original failure mode (last-writer-wins between analyst layers).
    let mut other_proposers: Vec<&str> = today_entries
        .iter()
        .filter_map(|e| e.proposer.as_deref())
        .filter(|p| !p.eq_ignore_ascii_case(proposer))
        .collect();
    other_proposers.dedup();
    if !other_proposers.is_empty() && !override_conflict {
        bail!(
            "same-day conflict: '{}' was already updated today by {} — a second update by a different proposer ('{}') requires --override-conflict.\nToday's ledger:\n{}\nIf you genuinely have newer information, re-run with --override-conflict and cite it in --evidence.",
            scenario_name,
            other_proposers.join(", "),
            proposer,
            format_probability_ledger(today_entries),
        );
    }

    let prior_delta: f64 = today_entries.iter().map(|e| e.delta().abs()).sum();
    let pending_delta = (new_prob - old_prob).abs();
    let total = prior_delta + pending_delta;
    if total > SCENARIO_DAILY_DELTA_CAP_PP + NORMALIZED_EPSILON && hard_print.is_none() {
        let ledger = if today_entries.is_empty() {
            "  (no prior updates today — this single move exceeds the cap)".to_string()
        } else {
            format_probability_ledger(today_entries)
        };
        bail!(
            "daily delta cap: updating '{}' {:.1}% → {:.1}% would put today's cumulative |Δ| at {:.1}pp (cap {:.1}pp).\nToday's ledger:\n{}\nProbability beliefs should not swing this much without hard data. If a hard print justifies it, re-run with --hard-print \"<event>\" (e.g. --hard-print \"CPI 2026-06-10 print\").",
            scenario_name,
            old_prob,
            new_prob,
            total,
            SCENARIO_DAILY_DELTA_CAP_PP,
            ledger,
        );
    }

    Ok(())
}

/// SQLite: load the probability-update ledger entries for one scenario on one
/// UTC day (`YYYY-MM-DD`).
fn probability_ledger_for_day(
    conn: &Connection,
    scenario_id: i64,
    day: &str,
) -> Result<Vec<ScenarioProbabilityLedgerEntry>> {
    let mut stmt = conn.prepare(
        "SELECT proposer, evidence, old_probability, new_probability, hard_print_event, created_at
         FROM scenario_updates
         WHERE scenario_id = ?1
           AND date(created_at) = ?2
           AND old_probability IS NOT NULL
           AND new_probability IS NOT NULL
         ORDER BY created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![scenario_id, day], |row| {
        Ok(ScenarioProbabilityLedgerEntry {
            proposer: row.get(0)?,
            evidence: row.get(1)?,
            old_probability: row.get(2)?,
            new_probability: row.get(3)?,
            hard_print_event: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// SQLite: total |Δprobability| across ALL scenarios' ledger entries on one
/// UTC day. Feeds `run_health.scenario_delta_total`.
pub fn scenario_delta_total_for_day(conn: &Connection, day: &str) -> Result<f64> {
    let total: f64 = conn.query_row(
        "SELECT COALESCE(SUM(ABS(new_probability - old_probability)), 0.0)
         FROM scenario_updates
         WHERE date(created_at) = ?1
           AND old_probability IS NOT NULL
           AND new_probability IS NOT NULL",
        params![day],
        |row| row.get(0),
    )?;
    Ok(total)
}

/// Guarded probability update (epistemics R4): enforces evidence-cited,
/// delta-capped, conflict-checked probability moves and records every update
/// in the `scenario_updates` ledger with proposer + evidence.
#[allow(clippy::too_many_arguments)]
pub fn guarded_update_scenario_probability(
    conn: &Connection,
    id: i64,
    probability: f64,
    driver: Option<&str>,
    proposer: &str,
    evidence: &str,
    hard_print: Option<&str>,
    override_conflict: bool,
) -> Result<()> {
    let old_prob: f64 = conn.query_row(
        "SELECT probability FROM scenarios WHERE id = ?",
        params![id],
        |row| row.get(0),
    )?;
    let scenario_name: String = conn.query_row(
        "SELECT name FROM scenarios WHERE id = ?",
        params![id],
        |row| row.get(0),
    )?;

    let today = Utc::now().format("%Y-%m-%d").to_string();
    let today_entries = probability_ledger_for_day(conn, id, &today)?;
    check_probability_ledger_guards(
        &scenario_name,
        &today_entries,
        old_prob,
        probability,
        proposer,
        hard_print,
        override_conflict,
    )?;

    update_scenario_probability(conn, id, probability, driver)?;

    let delta = probability - old_prob;
    let sign = if delta >= 0.0 { "+" } else { "" };
    let headline = format!(
        "probability {:.1}% → {:.1}% ({}{:.1}pp)",
        old_prob, probability, sign, delta
    );
    conn.execute(
        "INSERT INTO scenario_updates
            (scenario_id, headline, detail, severity, source_agent,
             proposer, evidence, old_probability, new_probability, hard_print_event)
         VALUES (?, ?, ?, 'normal', ?, ?, ?, ?, ?, ?)",
        params![
            id,
            headline,
            evidence,
            proposer,
            proposer,
            evidence,
            old_prob,
            probability,
            hard_print,
        ],
    )?;
    Ok(())
}

/// SQLite: set (or update) a scenario's reference-class base rate.
pub fn set_base_rate(conn: &Connection, id: i64, rate: f64, reference: &str) -> Result<()> {
    if !rate.is_finite() || !(0.0..=100.0).contains(&rate) {
        bail!("base rate must be a percentage in 0..=100, got {}", rate);
    }
    let affected = conn.execute(
        "UPDATE scenarios SET base_rate = ?1, base_rate_reference = ?2,
                updated_at = datetime('now')
         WHERE id = ?3",
        params![rate, reference, id],
    )?;
    if affected == 0 {
        bail!("scenario id {} not found", id);
    }
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
    let (name, status): (String, String) = conn
        .query_row(
            "SELECT name, status FROM scenarios WHERE id = ?",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or_else(|_| (String::new(), String::new()));
    if name.eq_ignore_ascii_case(RESIDUAL_SCENARIO_NAME) || status == RESIDUAL_SCENARIO_STATUS {
        bail!(
            "'{}' is the system-managed residual scenario and cannot be removed; it is recomputed automatically from 100 - sum(active modeled scenarios)",
            RESIDUAL_SCENARIO_NAME
        );
    }
    conn.execute("DELETE FROM scenarios WHERE id = ?", [id])?;
    // After removing a modeled scenario the residual absorbs its share of the
    // probability space.
    ensure_residual_scenario(conn)?;
    recompute_residual_scenario(conn)?;
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

/// Timeline entry: a probability snapshot for a named scenario on a specific date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioTimelinePoint {
    pub date: String,
    pub probability: f64,
}

/// A scenario's probability trajectory over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioTimeline {
    pub scenario_id: i64,
    pub name: String,
    pub current_probability: f64,
    pub status: String,
    pub phase: String,
    pub data_points: Vec<ScenarioTimelinePoint>,
    pub change: Option<f64>,
}

/// Get probability timelines for all active scenarios, optionally filtered to last N days.
/// Returns one ScenarioTimeline per scenario, with daily-deduplicated data points (last entry per day).
pub fn get_all_timelines(
    conn: &Connection,
    days: Option<u32>,
) -> Result<Vec<ScenarioTimeline>> {
    let scenarios_list = list_scenarios(conn, Some("active"))?;
    let mut timelines = Vec::new();

    for scenario in &scenarios_list {
        let query = if let Some(d) = days {
            format!(
                "SELECT probability, recorded_at
                 FROM scenario_history
                 WHERE scenario_id = {}
                   AND recorded_at >= datetime('now', '-{} days')
                 ORDER BY id ASC",
                scenario.id, d
            )
        } else {
            format!(
                "SELECT probability, recorded_at
                 FROM scenario_history
                 WHERE scenario_id = {}
                 ORDER BY id ASC",
                scenario.id
            )
        };

        let mut stmt = conn.prepare(&query)?;
        let rows: Vec<(f64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        // Deduplicate to last entry per date (YYYY-MM-DD)
        let mut day_map: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
        for (prob, recorded_at) in &rows {
            // Extract YYYY-MM-DD from either "YYYY-MM-DDTHH:MM:SS" or "YYYY-MM-DD HH:MM:SS" formats
            let date = if recorded_at.contains('T') {
                recorded_at.split('T').next().unwrap_or(recorded_at)
            } else if recorded_at.contains(' ') {
                recorded_at.split(' ').next().unwrap_or(recorded_at)
            } else {
                recorded_at.as_str()
            };
            // BTreeMap insert overwrites, so last entry per day wins
            day_map.insert(date.to_string(), *prob);
        }

        let data_points: Vec<ScenarioTimelinePoint> = day_map
            .into_iter()
            .map(|(date, probability)| ScenarioTimelinePoint { date, probability })
            .collect();

        let change = if data_points.len() >= 2 {
            Some(data_points.last().unwrap().probability - data_points.first().unwrap().probability)
        } else {
            None
        };

        timelines.push(ScenarioTimeline {
            scenario_id: scenario.id,
            name: scenario.name.clone(),
            current_probability: scenario.probability,
            status: scenario.status.clone(),
            phase: scenario.phase.clone(),
            data_points,
            change,
        });
    }

    // Sort by current probability descending
    timelines.sort_by(|a, b| b.current_probability.partial_cmp(&a.current_probability).unwrap_or(std::cmp::Ordering::Equal));

    Ok(timelines)
}

pub fn get_all_timelines_backend(
    backend: &BackendConnection,
    days: Option<u32>,
) -> Result<Vec<ScenarioTimeline>> {
    query::dispatch(
        backend,
        |conn| get_all_timelines(conn, days),
        |pool| get_all_timelines_postgres(pool, days),
    )
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

/// Backend wrapper for `compute_normalized_set`. Postgres backend currently
/// returns a stub set (`Underfilled`, residual=100) because the normalized-set
/// enforcement only ships for SQLite in this iteration; the report builders run
/// against SQLite installs.
pub fn compute_normalized_set_backend(
    backend: &BackendConnection,
) -> Result<NormalizedScenarioSet> {
    query::dispatch(
        backend,
        compute_normalized_set,
        |_pool| {
            Ok(NormalizedScenarioSet {
                modeled_sum: 0.0,
                residual_probability: NORMALIZED_TOTAL,
                residual_materialized: false,
                overfill_state: OverfillState::Underfilled,
            })
        },
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

/// Guarded probability update (epistemics R4): evidence-cited, delta-capped,
/// conflict-checked. Records the move in the `scenario_updates` ledger.
#[allow(clippy::too_many_arguments)]
pub fn guarded_update_scenario_probability_backend(
    backend: &BackendConnection,
    id: i64,
    probability: f64,
    driver: Option<&str>,
    proposer: &str,
    evidence: &str,
    hard_print: Option<&str>,
    override_conflict: bool,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| {
            guarded_update_scenario_probability(
                conn,
                id,
                probability,
                driver,
                proposer,
                evidence,
                hard_print,
                override_conflict,
            )
        },
        |pool| {
            guarded_update_scenario_probability_postgres(
                pool,
                id,
                probability,
                driver,
                proposer,
                evidence,
                hard_print,
                override_conflict,
            )
        },
    )
}

/// Set (or update) a scenario's reference-class base rate.
pub fn set_base_rate_backend(
    backend: &BackendConnection,
    id: i64,
    rate: f64,
    reference: &str,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| set_base_rate(conn, id, rate, reference),
        |pool| set_base_rate_postgres(pool, id, rate, reference),
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
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS base_rate DOUBLE PRECISION")
            .execute(pool)
            .await?;
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS base_rate_reference TEXT")
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
        base_rate: r.13,
        base_rate_reference: r.14,
    }
}

fn list_scenarios_postgres(pool: &PgPool, status_filter: Option<&str>) -> Result<Vec<Scenario>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<ScenarioExtRow> = if let Some(status) = status_filter {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                    "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes, base_rate, base_rate_reference
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
                    "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes, base_rate, base_rate_reference
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
                "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes, base_rate, base_rate_reference
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

    // Read old probability and name before updating
    let (old_prob, scenario_name): (f64, String) =
        crate::db::pg_runtime::block_on(async {
            let row: (f64, String) = sqlx::query_as(
                "SELECT probability, name FROM scenarios WHERE id = $1",
            )
            .bind(id)
            .fetch_one(pool)
            .await?;
            Ok::<(f64, String), sqlx::Error>(row)
        })?;

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

    // Detect large probability shifts and auto-create scenario alerts
    let delta = probability - old_prob;
    let abs_delta = delta.abs();
    if abs_delta >= SCENARIO_SHIFT_THRESHOLD_PP {
        let direction = if delta > 0.0 { "above" } else { "below" };
        let sign = if delta > 0.0 { "+" } else { "" };
        let rule_text = format!(
            "{} probability shifted {}{:.1}pp ({:.1}% → {:.1}%)",
            scenario_name, sign, delta, old_prob, probability
        );
        let driver_text = driver.unwrap_or("(no driver specified)");
        let trigger_data = json!({
            "scenario_id": id,
            "scenario_name": scenario_name,
            "old_probability": old_prob,
            "new_probability": probability,
            "delta_pp": delta,
            "threshold_pp": SCENARIO_SHIFT_THRESHOLD_PP,
            "driver": driver_text,
        });
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        crate::db::pg_runtime::block_on(async {
            let alert_id: (i64,) = sqlx::query_as(
                "INSERT INTO alerts (kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, triggered_at)
                 VALUES ('scenario', $1, $2, 'probability_shift', $3, 'triggered', $4, false, 0, NOW())
                 RETURNING id",
            )
            .bind(&scenario_name)
            .bind(direction)
            .bind(SCENARIO_SHIFT_THRESHOLD_PP.to_string())
            .bind(&rule_text)
            .fetch_one(pool)
            .await?;

            sqlx::query(
                "INSERT INTO triggered_alerts (alert_id, triggered_at, trigger_data, acknowledged) VALUES ($1, $2, $3, false)",
            )
            .bind(alert_id.0)
            .bind(&now)
            .bind(trigger_data.to_string())
            .execute(pool)
            .await?;

            Ok::<(), sqlx::Error>(())
        })?;
    }

    Ok(())
}

/// Postgres: load the probability-update ledger entries for one scenario on
/// one UTC day.
fn probability_ledger_for_day_postgres(
    pool: &PgPool,
    scenario_id: i64,
    day: &str,
) -> Result<Vec<ScenarioProbabilityLedgerEntry>> {
    type Row = (
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<f64>,
        Option<String>,
        String,
    );
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT proposer, evidence, old_probability, new_probability, hard_print_event, created_at::text
             FROM scenario_updates
             WHERE scenario_id = $1
               AND created_at::date = $2::date
               AND old_probability IS NOT NULL
               AND new_probability IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )
        .bind(scenario_id)
        .bind(day)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(
            |(proposer, evidence, old_probability, new_probability, hard_print_event, created_at)| {
                ScenarioProbabilityLedgerEntry {
                    proposer,
                    evidence,
                    old_probability,
                    new_probability,
                    hard_print_event,
                    created_at,
                }
            },
        )
        .collect())
}

/// Postgres twin of `guarded_update_scenario_probability`.
#[allow(clippy::too_many_arguments)]
fn guarded_update_scenario_probability_postgres(
    pool: &PgPool,
    id: i64,
    probability: f64,
    driver: Option<&str>,
    proposer: &str,
    evidence: &str,
    hard_print: Option<&str>,
    override_conflict: bool,
) -> Result<()> {
    ensure_tables_postgres(pool)?;
    let (old_prob, scenario_name): (f64, String) = crate::db::pg_runtime::block_on(async {
        let row: (f64, String) =
            sqlx::query_as("SELECT probability, name FROM scenarios WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await?;
        Ok::<(f64, String), sqlx::Error>(row)
    })?;

    let today = Utc::now().format("%Y-%m-%d").to_string();
    let today_entries = probability_ledger_for_day_postgres(pool, id, &today)?;
    check_probability_ledger_guards(
        &scenario_name,
        &today_entries,
        old_prob,
        probability,
        proposer,
        hard_print,
        override_conflict,
    )?;

    update_scenario_probability_postgres(pool, id, probability, driver)?;

    let delta = probability - old_prob;
    let sign = if delta >= 0.0 { "+" } else { "" };
    let headline = format!(
        "probability {:.1}% → {:.1}% ({}{:.1}pp)",
        old_prob, probability, sign, delta
    );
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "INSERT INTO scenario_updates
                (scenario_id, headline, detail, severity, source_agent,
                 proposer, evidence, old_probability, new_probability, hard_print_event)
             VALUES ($1, $2, $3, 'normal', $4, $5, $6, $7, $8, $9)",
        )
        .bind(id)
        .bind(&headline)
        .bind(evidence)
        .bind(proposer)
        .bind(proposer)
        .bind(evidence)
        .bind(old_prob)
        .bind(probability)
        .bind(hard_print)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

/// Postgres: set (or update) a scenario's reference-class base rate.
fn set_base_rate_postgres(pool: &PgPool, id: i64, rate: f64, reference: &str) -> Result<()> {
    if !rate.is_finite() || !(0.0..=100.0).contains(&rate) {
        bail!("base rate must be a percentage in 0..=100, got {}", rate);
    }
    ensure_tables_postgres(pool)?;
    let affected = crate::db::pg_runtime::block_on(async {
        let result = sqlx::query(
            "UPDATE scenarios SET base_rate = $1, base_rate_reference = $2, updated_at = NOW()
             WHERE id = $3",
        )
        .bind(rate)
        .bind(reference)
        .bind(id)
        .execute(pool)
        .await?;
        Ok::<u64, sqlx::Error>(result.rows_affected())
    })?;
    if affected == 0 {
        bail!("scenario id {} not found", id);
    }
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
            "SELECT id, name, probability, description, asset_impact, triggers, historical_precedent, status, created_at::text, updated_at::text, phase, resolved_at::text, resolution_notes, base_rate, base_rate_reference
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
             FROM scenario_indicators WHERE status IN ('watching', 'triggered') ORDER BY scenario_id, id",
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
    let now = Utc::now().to_rfc3339();
    if triggered {
        crate::db::pg_runtime::block_on(async {
            sqlx::query(
                "UPDATE scenario_indicators
                 SET last_value = $1,
                     last_checked = $2::timestamptz,
                     status = 'triggered',
                     triggered_at = $3::timestamptz,
                     updated_at = $4::timestamptz
                 WHERE id = $5",
            )
            .bind(last_value)
            .bind(&now)
            .bind(&now)
            .bind(&now)
            .bind(indicator_id)
            .execute(pool)
            .await
        })?;
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query(
                "UPDATE scenario_indicators
                 SET last_value = $1,
                     last_checked = $2::timestamptz,
                     updated_at = $3::timestamptz
                 WHERE id = $4",
            )
            .bind(last_value)
            .bind(&now)
            .bind(&now)
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
    let next_decision_at = normalize_optional_timestamp(next_decision_at)?;
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

fn get_all_timelines_postgres(
    pool: &PgPool,
    days: Option<u32>,
) -> Result<Vec<ScenarioTimeline>> {
    ensure_tables_postgres(pool)?;
    let scenarios_list = list_scenarios_postgres(pool, Some("active"))?;
    let mut timelines = Vec::new();

    for scenario in &scenarios_list {
        let rows: Vec<(f64, String)> = if let Some(d) = days {
            crate::db::pg_runtime::block_on(async {
                sqlx::query_as(
                    "SELECT probability, recorded_at::text
                     FROM scenario_history
                     WHERE scenario_id = $1
                       AND recorded_at >= NOW() - make_interval(days => $2)
                     ORDER BY id ASC",
                )
                .bind(scenario.id)
                .bind(d as i32)
                .fetch_all(pool)
                .await
            })?
        } else {
            crate::db::pg_runtime::block_on(async {
                sqlx::query_as(
                    "SELECT probability, recorded_at::text
                     FROM scenario_history
                     WHERE scenario_id = $1
                     ORDER BY id ASC",
                )
                .bind(scenario.id)
                .fetch_all(pool)
                .await
            })?
        };

        // Deduplicate to last entry per date
        let mut day_map: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
        for (prob, recorded_at) in &rows {
            let date = if recorded_at.contains('T') {
                recorded_at.split('T').next().unwrap_or(recorded_at)
            } else if recorded_at.contains(' ') {
                recorded_at.split(' ').next().unwrap_or(recorded_at)
            } else {
                recorded_at.as_str()
            };
            day_map.insert(date.to_string(), *prob);
        }

        let data_points: Vec<ScenarioTimelinePoint> = day_map
            .into_iter()
            .map(|(date, probability)| ScenarioTimelinePoint { date, probability })
            .collect();

        let change = if data_points.len() >= 2 {
            Some(data_points.last().unwrap().probability - data_points.first().unwrap().probability)
        } else {
            None
        };

        timelines.push(ScenarioTimeline {
            scenario_id: scenario.id,
            name: scenario.name.clone(),
            current_probability: scenario.probability,
            status: scenario.status.clone(),
            phase: scenario.phase.clone(),
            data_points,
            change,
        });
    }

    timelines.sort_by(|a, b| b.current_probability.partial_cmp(&a.current_probability).unwrap_or(std::cmp::Ordering::Equal));

    Ok(timelines)
}

// --- Batch query functions for N+1 elimination ---

/// Batch count branches for multiple scenarios (SQLite).
/// Returns HashMap<scenario_id, count>.
pub fn count_branches_batch(
    conn: &Connection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<&str> = scenario_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT scenario_id, COUNT(*) FROM scenario_branches WHERE scenario_id IN ({}) GROUP BY scenario_id",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = scenario_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (sid, cnt) = row?;
        map.insert(sid, cnt as usize);
    }
    Ok(map)
}

fn count_branches_batch_postgres(
    pool: &PgPool,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    use sqlx::Row as _;
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT scenario_id, COUNT(*)::bigint FROM scenario_branches WHERE scenario_id = ANY($1) GROUP BY scenario_id",
        )
        .bind(scenario_ids)
        .fetch_all(pool)
        .await?;
        let mut map = std::collections::HashMap::new();
        for r in &rows {
            let sid: i64 = r.get(0);
            let cnt: i64 = r.get(1);
            map.insert(sid, cnt as usize);
        }
        Ok::<std::collections::HashMap<i64, usize>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn count_branches_batch_backend(
    backend: &BackendConnection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    query::dispatch(
        backend,
        |conn| count_branches_batch(conn, scenario_ids),
        |pool| count_branches_batch_postgres(pool, scenario_ids),
    )
}

/// Batch count impacts for multiple scenarios (SQLite).
pub fn count_impacts_batch(
    conn: &Connection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<&str> = scenario_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT scenario_id, COUNT(*) FROM scenario_impacts WHERE scenario_id IN ({}) GROUP BY scenario_id",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = scenario_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (sid, cnt) = row?;
        map.insert(sid, cnt as usize);
    }
    Ok(map)
}

fn count_impacts_batch_postgres(
    pool: &PgPool,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    use sqlx::Row as _;
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT scenario_id, COUNT(*)::bigint FROM scenario_impacts WHERE scenario_id = ANY($1) GROUP BY scenario_id",
        )
        .bind(scenario_ids)
        .fetch_all(pool)
        .await?;
        let mut map = std::collections::HashMap::new();
        for r in &rows {
            let sid: i64 = r.get(0);
            let cnt: i64 = r.get(1);
            map.insert(sid, cnt as usize);
        }
        Ok::<std::collections::HashMap<i64, usize>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn count_impacts_batch_backend(
    backend: &BackendConnection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    query::dispatch(
        backend,
        |conn| count_impacts_batch(conn, scenario_ids),
        |pool| count_impacts_batch_postgres(pool, scenario_ids),
    )
}

/// Batch fetch indicators for multiple scenarios (SQLite).
/// Returns full indicator data grouped by scenario_id (needed to count triggered status).
pub fn list_indicators_batch(
    conn: &Connection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<ScenarioIndicator>>> {
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<&str> = scenario_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at, last_value, last_checked, created_at, updated_at
         FROM scenario_indicators WHERE scenario_id IN ({}) ORDER BY scenario_id, status, id",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = scenario_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], ScenarioIndicator::from_row)?;
    let all: Vec<ScenarioIndicator> = rows.collect::<Result<Vec<_>, _>>()?;

    let mut map: std::collections::HashMap<i64, Vec<ScenarioIndicator>> =
        std::collections::HashMap::new();
    for ind in all {
        map.entry(ind.scenario_id).or_default().push(ind);
    }
    Ok(map)
}

fn list_indicators_batch_postgres(
    pool: &PgPool,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<ScenarioIndicator>>> {
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    crate::db::pg_runtime::block_on(async {
        let rows: Vec<IndicatorRow> = sqlx::query_as(
            "SELECT id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, status, triggered_at::text, last_value, last_checked::text, created_at::text, updated_at::text
             FROM scenario_indicators WHERE scenario_id = ANY($1) ORDER BY scenario_id, status, id",
        )
        .bind(scenario_ids)
        .fetch_all(pool)
        .await?;
        let mut map: std::collections::HashMap<i64, Vec<ScenarioIndicator>> =
            std::collections::HashMap::new();
        for r in rows {
            let ind = ScenarioIndicator {
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
            };
            map.entry(ind.scenario_id).or_default().push(ind);
        }
        Ok::<std::collections::HashMap<i64, Vec<ScenarioIndicator>>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn list_indicators_batch_backend(
    backend: &BackendConnection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<ScenarioIndicator>>> {
    query::dispatch(
        backend,
        |conn| list_indicators_batch(conn, scenario_ids),
        |pool| list_indicators_batch_postgres(pool, scenario_ids),
    )
}

/// Batch count updates for multiple scenarios (SQLite).
pub fn count_updates_batch(
    conn: &Connection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let placeholders: Vec<&str> = scenario_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT scenario_id, COUNT(*) FROM scenario_updates WHERE scenario_id IN ({}) GROUP BY scenario_id",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = scenario_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (sid, cnt) = row?;
        map.insert(sid, cnt as usize);
    }
    Ok(map)
}

fn count_updates_batch_postgres(
    pool: &PgPool,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    use sqlx::Row as _;
    if scenario_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT scenario_id, COUNT(*)::bigint FROM scenario_updates WHERE scenario_id = ANY($1) GROUP BY scenario_id",
        )
        .bind(scenario_ids)
        .fetch_all(pool)
        .await?;
        let mut map = std::collections::HashMap::new();
        for r in &rows {
            let sid: i64 = r.get(0);
            let cnt: i64 = r.get(1);
            map.insert(sid, cnt as usize);
        }
        Ok::<std::collections::HashMap<i64, usize>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn count_updates_batch_backend(
    backend: &BackendConnection,
    scenario_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    query::dispatch(
        backend,
        |conn| count_updates_batch(conn, scenario_ids),
        |pool| count_updates_batch_postgres(pool, scenario_ids),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Result<Connection> {
        let conn = Connection::open_in_memory()?;
        crate::db::schema::run_migrations(&conn)?;
        Ok(conn)
    }

    #[test]
    fn test_probability_shift_creates_scenario_alert() -> Result<()> {
        let conn = test_db()?;
        let id = add_scenario(
            &conn,
            "Recession",
            20.0,
            Some("Test recession scenario"),
            None,
            None,
            None,
        )?;

        // Small shift — should NOT create an alert
        update_scenario_probability(&conn, id, 25.0, Some("minor update"))?;
        let alerts: Vec<String> = conn
            .prepare("SELECT rule_text FROM alerts WHERE kind = 'scenario'")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert!(alerts.is_empty(), "Small shift should not create alert");

        // Large shift (+15pp) — should create an alert
        update_scenario_probability(&conn, id, 40.0, Some("major data shift"))?;
        let alerts: Vec<String> = conn
            .prepare("SELECT rule_text FROM alerts WHERE kind = 'scenario'")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(alerts.len(), 1, "Large shift should create one alert");
        assert!(
            alerts[0].contains("Recession"),
            "Alert should mention scenario name"
        );
        assert!(
            alerts[0].contains("+15.0pp"),
            "Alert should show delta: {}",
            alerts[0]
        );
        assert!(
            alerts[0].contains("25.0%"),
            "Alert should show old probability"
        );
        assert!(
            alerts[0].contains("40.0%"),
            "Alert should show new probability"
        );

        // Check triggered_alerts was also populated
        let triggered_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM triggered_alerts",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(triggered_count, 1, "Should have one triggered_alert entry");

        // Check trigger_data contains scenario info
        let trigger_data: String = conn.query_row(
            "SELECT trigger_data FROM triggered_alerts LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        let parsed: serde_json::Value = serde_json::from_str(&trigger_data)?;
        assert_eq!(parsed["scenario_name"], "Recession");
        assert_eq!(parsed["old_probability"], 25.0);
        assert_eq!(parsed["new_probability"], 40.0);
        assert_eq!(parsed["delta_pp"], 15.0);

        Ok(())
    }

    #[test]
    fn test_probability_decrease_creates_alert() -> Result<()> {
        let conn = test_db()?;
        let id = add_scenario(
            &conn,
            "Hyperinflation",
            50.0,
            Some("Test inflation scenario"),
            None,
            None,
            None,
        )?;

        // Large decrease (-12pp) — should create an alert with "below" direction
        update_scenario_probability(&conn, id, 38.0, Some("CPI came in lower"))?;
        let direction: String = conn.query_row(
            "SELECT direction FROM alerts WHERE kind = 'scenario' LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(direction, "below", "Decreasing probability should use 'below'");

        let rule_text: String = conn.query_row(
            "SELECT rule_text FROM alerts WHERE kind = 'scenario' LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert!(
            rule_text.contains("-12.0pp"),
            "Should show negative delta: {}",
            rule_text
        );

        Ok(())
    }

    #[test]
    fn test_exact_threshold_creates_alert() -> Result<()> {
        let conn = test_db()?;
        let id = add_scenario(
            &conn,
            "ExactTest",
            30.0,
            None,
            None,
            None,
            None,
        )?;

        // Exactly 10pp shift — should create alert (>= threshold)
        update_scenario_probability(&conn, id, 40.0, None)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM alerts WHERE kind = 'scenario'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1, "Exactly 10pp shift should create alert");

        Ok(())
    }

    #[test]
    fn test_just_below_threshold_no_alert() -> Result<()> {
        let conn = test_db()?;
        let id = add_scenario(
            &conn,
            "NearMiss",
            30.0,
            None,
            None,
            None,
            None,
        )?;

        // 9.9pp shift — should NOT create alert
        update_scenario_probability(&conn, id, 39.9, None)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM alerts WHERE kind = 'scenario'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0, "9.9pp shift should not create alert");

        Ok(())
    }

    #[test]
    fn test_get_all_timelines_empty() -> Result<()> {
        let conn = test_db()?;
        let timelines = get_all_timelines(&conn, None)?;
        assert!(timelines.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_all_timelines_with_history() -> Result<()> {
        let conn = test_db()?;

        // Create two scenarios — totals stay under 100% so the residual stays
        // non-negative.
        let id1 = add_scenario(&conn, "Recession", 25.0, Some("test"), None, None, None)?;
        let id2 = add_scenario(&conn, "Inflation", 70.0, Some("test2"), None, None, None)?;

        // Add history entries
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id1, 40.0, "initial", "2026-03-01 12:00:00"],
        )?;
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id1, 50.0, "updated", "2026-03-02 12:00:00"],
        )?;
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id2, 65.0, "initial", "2026-03-01 12:00:00"],
        )?;
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id2, 70.0, "updated", "2026-03-02 12:00:00"],
        )?;

        let timelines = get_all_timelines(&conn, None)?;
        assert_eq!(timelines.len(), 2);

        // Should be sorted by current probability descending (Inflation=70 first)
        assert_eq!(timelines[0].name, "Inflation");
        assert_eq!(timelines[1].name, "Recession");

        // Check data points
        assert_eq!(timelines[0].data_points.len(), 2);
        assert!((timelines[0].data_points[0].probability - 65.0).abs() < 0.01);
        assert!((timelines[0].data_points[1].probability - 70.0).abs() < 0.01);

        // Check change
        assert!((timelines[0].change.unwrap() - 5.0).abs() < 0.01);
        assert!((timelines[1].change.unwrap() - 10.0).abs() < 0.01);

        Ok(())
    }

    #[test]
    fn test_get_all_timelines_deduplicates_same_day() -> Result<()> {
        let conn = test_db()?;

        let id = add_scenario(&conn, "Test", 60.0, None, None, None, None)?;

        // Multiple entries on same day — should keep last
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, 40.0, "morning", "2026-03-15 08:00:00"],
        )?;
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, 55.0, "evening", "2026-03-15 20:00:00"],
        )?;

        let timelines = get_all_timelines(&conn, None)?;
        assert_eq!(timelines.len(), 1);
        assert_eq!(timelines[0].data_points.len(), 1);
        // Last entry of the day wins
        assert!((timelines[0].data_points[0].probability - 55.0).abs() < 0.01);

        Ok(())
    }

    #[test]
    fn test_get_all_timelines_only_active() -> Result<()> {
        let conn = test_db()?;

        let id1 = add_scenario(&conn, "Active", 50.0, None, None, None, None)?;
        let _id2 = add_scenario(&conn, "Resolved", 30.0, None, None, None, None)?;

        // Resolve the second scenario
        conn.execute(
            "UPDATE scenarios SET status = 'resolved' WHERE id = ?",
            params![_id2],
        )?;

        // Add history to both
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![id1, 45.0, "test", "2026-03-01 12:00:00"],
        )?;
        conn.execute(
            "INSERT INTO scenario_history (scenario_id, probability, driver, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            params![_id2, 35.0, "test", "2026-03-01 12:00:00"],
        )?;

        let timelines = get_all_timelines(&conn, None)?;
        // Only active scenario should appear
        assert_eq!(timelines.len(), 1);
        assert_eq!(timelines[0].name, "Active");

        Ok(())
    }

    #[test]
    fn test_timeline_struct_serialization() -> Result<()> {
        let timeline = ScenarioTimeline {
            scenario_id: 1,
            name: "Test".to_string(),
            current_probability: 75.0,
            status: "active".to_string(),
            phase: "hypothesis".to_string(),
            data_points: vec![
                ScenarioTimelinePoint { date: "2026-03-01".to_string(), probability: 50.0 },
                ScenarioTimelinePoint { date: "2026-03-02".to_string(), probability: 75.0 },
            ],
            change: Some(25.0),
        };
        let json = serde_json::to_string(&timeline)?;
        assert!(json.contains("\"name\":\"Test\""));
        assert!(json.contains("\"change\":25.0"));
        assert!(json.contains("\"data_points\""));
        Ok(())
    }

    // --- Batch query tests ---

    #[test]
    fn count_branches_batch_empty_ids() -> Result<()> {
        let conn = test_db()?;
        let result = count_branches_batch(&conn, &[])?;
        assert!(result.is_empty());
        Ok(())
    }

    #[test]
    fn count_branches_batch_multiple_scenarios() -> Result<()> {
        let conn = test_db()?;
        // Keep modeled sum <= 100 under the normalized scenario-set model.
        let s1 = add_scenario(&conn, "Scenario A", 30.0, None, None, None, None)?;
        let s2 = add_scenario(&conn, "Scenario B", 30.0, None, None, None, None)?;
        let s3 = add_scenario(&conn, "Scenario C", 40.0, None, None, None, None)?;

        add_branch(&conn, s1, "Branch 1A", 60.0, None)?;
        add_branch(&conn, s1, "Branch 1B", 40.0, None)?;
        add_branch(&conn, s2, "Branch 2A", 100.0, None)?;
        // s3 has no branches

        let counts = count_branches_batch(&conn, &[s1, s2, s3])?;
        assert_eq!(*counts.get(&s1).unwrap_or(&0), 2);
        assert_eq!(*counts.get(&s2).unwrap_or(&0), 1);
        assert_eq!(counts.get(&s3), None); // no entry for zero-count
        Ok(())
    }

    #[test]
    fn count_impacts_batch_empty_ids() -> Result<()> {
        let conn = test_db()?;
        let result = count_impacts_batch(&conn, &[])?;
        assert!(result.is_empty());
        Ok(())
    }

    #[test]
    fn count_impacts_batch_multiple_scenarios() -> Result<()> {
        let conn = test_db()?;
        let s1 = add_scenario(&conn, "Scenario A", 50.0, None, None, None, None)?;
        let s2 = add_scenario(&conn, "Scenario B", 30.0, None, None, None, None)?;

        add_impact(&conn, s1, None, "BTC", "up", "primary", None, None)?;
        add_impact(&conn, s1, None, "ETH", "up", "secondary", None, None)?;
        add_impact(&conn, s1, None, "GLD", "down", "tertiary", None, None)?;
        add_impact(&conn, s2, None, "SPY", "down", "primary", None, None)?;

        let counts = count_impacts_batch(&conn, &[s1, s2])?;
        assert_eq!(*counts.get(&s1).unwrap_or(&0), 3);
        assert_eq!(*counts.get(&s2).unwrap_or(&0), 1);
        Ok(())
    }

    #[test]
    fn list_indicators_batch_empty_ids() -> Result<()> {
        let conn = test_db()?;
        let result = list_indicators_batch(&conn, &[])?;
        assert!(result.is_empty());
        Ok(())
    }

    #[test]
    fn list_indicators_batch_multiple_scenarios() -> Result<()> {
        let conn = test_db()?;
        let s1 = add_scenario(&conn, "Scenario A", 50.0, None, None, None, None)?;
        let s2 = add_scenario(&conn, "Scenario B", 30.0, None, None, None, None)?;

        add_indicator(&conn, s1, None, None, "BTC", "price", ">", "100000", "BTC 100k")?;
        add_indicator(&conn, s1, None, None, "ETH", "price", ">", "5000", "ETH 5k")?;
        add_indicator(&conn, s2, None, None, "SPY", "price", "<", "400", "SPY drop")?;

        let map = list_indicators_batch(&conn, &[s1, s2])?;
        assert_eq!(map.get(&s1).unwrap().len(), 2);
        assert_eq!(map.get(&s2).unwrap().len(), 1);
        assert_eq!(map.get(&s2).unwrap()[0].label, "SPY drop");
        Ok(())
    }

    #[test]
    fn list_indicators_batch_triggered_filter() -> Result<()> {
        let conn = test_db()?;
        let s1 = add_scenario(&conn, "Scenario A", 50.0, None, None, None, None)?;

        let ind1 = add_indicator(&conn, s1, None, None, "BTC", "price", ">", "100000", "BTC 100k")?;
        add_indicator(&conn, s1, None, None, "ETH", "price", ">", "5000", "ETH 5k")?;

        // Trigger one indicator
        update_indicator_evaluation(&conn, ind1, "105000", true)?;

        let map = list_indicators_batch(&conn, &[s1])?;
        let indicators = map.get(&s1).unwrap();
        assert_eq!(indicators.len(), 2);
        let triggered = indicators.iter().filter(|i| i.status == "triggered").count();
        assert_eq!(triggered, 1);
        Ok(())
    }

    #[test]
    fn count_updates_batch_empty_ids() -> Result<()> {
        let conn = test_db()?;
        let result = count_updates_batch(&conn, &[])?;
        assert!(result.is_empty());
        Ok(())
    }

    #[test]
    fn count_updates_batch_multiple_scenarios() -> Result<()> {
        let conn = test_db()?;
        let s1 = add_scenario(&conn, "Scenario A", 50.0, None, None, None, None)?;
        let s2 = add_scenario(&conn, "Scenario B", 30.0, None, None, None, None)?;

        add_update(&conn, s1, None, "Update 1", None, "info", None, None, None, None)?;
        add_update(&conn, s1, None, "Update 2", None, "warning", None, None, None, None)?;
        add_update(&conn, s1, None, "Update 3", None, "critical", None, None, None, None)?;
        add_update(&conn, s2, None, "Update A", None, "info", None, None, None, None)?;

        let counts = count_updates_batch(&conn, &[s1, s2])?;
        assert_eq!(*counts.get(&s1).unwrap_or(&0), 3);
        assert_eq!(*counts.get(&s2).unwrap_or(&0), 1);
        Ok(())
    }

    #[test]
    fn add_update_normalizes_next_decision_at_date() -> Result<()> {
        let conn = test_db()?;
        let scenario_id = add_scenario(&conn, "Scenario A", 50.0, None, None, None, None)?;

        add_update(
            &conn,
            scenario_id,
            None,
            "Update 1",
            Some("Detail with spaces, commas, and symbols."),
            "normal",
            None,
            None,
            Some("Re-check after CPI"),
            Some("2026-04-20"),
        )?;

        let updates = list_updates(&conn, scenario_id, Some(1))?;
        assert_eq!(
            updates[0].next_decision_at.as_deref(),
            Some("2026-04-20T00:00:00+00:00")
        );
        Ok(())
    }

    #[test]
    fn add_update_rejects_invalid_next_decision_at() -> Result<()> {
        let conn = test_db()?;
        let scenario_id = add_scenario(&conn, "Scenario A", 50.0, None, None, None, None)?;

        let err = add_update(
            &conn,
            scenario_id,
            None,
            "Update 1",
            None,
            "normal",
            None,
            None,
            None,
            Some("tomorrow morning maybe"),
        )
        .expect_err("invalid timestamp should fail");

        assert!(
            err.to_string().contains("invalid next_decision_at"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    // --- Normalized scenario-set model tests --------------------------------

    /// The schema migration must seed the system-managed residual row on a
    /// fresh DB so every read sees the normalized set semantics from day one.
    #[test]
    fn migration_seeds_system_managed_residual_row() -> Result<()> {
        let conn = test_db()?;
        let residual = get_scenario_by_name(&conn, RESIDUAL_SCENARIO_NAME)?;
        let row = residual.expect("residual row should be seeded by migration");
        assert_eq!(row.status, RESIDUAL_SCENARIO_STATUS);
        // No modeled scenarios yet => residual fills the entire space.
        assert!(
            (row.probability - 100.0).abs() < 0.001,
            "expected residual=100, got {}",
            row.probability
        );
        Ok(())
    }

    #[test]
    fn residual_is_recomputed_when_modeled_scenarios_are_added() -> Result<()> {
        let conn = test_db()?;
        add_scenario(&conn, "Inflation Spike", 45.0, None, None, None, None)?;
        add_scenario(&conn, "Hard Recession", 25.0, None, None, None, None)?;
        add_scenario(&conn, "Soft Landing", 20.0, None, None, None, None)?;
        let set = compute_normalized_set(&conn)?;
        assert!((set.modeled_sum - 90.0).abs() < 0.001);
        assert!((set.residual_probability - 10.0).abs() < 0.001);
        assert!(set.residual_materialized);
        assert_eq!(set.overfill_state, OverfillState::Underfilled);

        let residual = get_scenario_by_name(&conn, RESIDUAL_SCENARIO_NAME)?.unwrap();
        assert!((residual.probability - 10.0).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn add_scenario_rejects_modeled_sum_overfill() -> Result<()> {
        let conn = test_db()?;
        add_scenario(&conn, "Inflation Spike", 60.0, None, None, None, None)?;
        let err = add_scenario(&conn, "Hard Recession", 50.0, None, None, None, None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("modeled scenario sum") && msg.contains("110"),
            "expected overfill rejection message, got {msg}"
        );
        // The residual stays consistent with what got committed (only the
        // first scenario was inserted before the second was rejected).
        let set = compute_normalized_set(&conn)?;
        assert!((set.modeled_sum - 60.0).abs() < 0.001);
        assert!((set.residual_probability - 40.0).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn update_scenario_probability_rejects_overfill() -> Result<()> {
        let conn = test_db()?;
        let a = add_scenario(&conn, "A", 40.0, None, None, None, None)?;
        let _b = add_scenario(&conn, "B", 40.0, None, None, None, None)?;
        // Bumping A to 70 would push modeled sum to 110.
        let err = update_scenario_probability(&conn, a, 70.0, Some("test")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("modeled scenario sum") && msg.contains("110"),
            "expected overfill rejection on update, got {msg}"
        );
        // Bumping A to 55 (sum 95) is allowed and recomputes the residual.
        update_scenario_probability(&conn, a, 55.0, Some("ok"))?;
        let set = compute_normalized_set(&conn)?;
        assert!((set.modeled_sum - 95.0).abs() < 0.001);
        assert!((set.residual_probability - 5.0).abs() < 0.001);
        assert_eq!(set.overfill_state, OverfillState::Underfilled);
        Ok(())
    }

    #[test]
    fn modeled_sum_exactly_100_classifies_ok() -> Result<()> {
        let conn = test_db()?;
        add_scenario(&conn, "A", 60.0, None, None, None, None)?;
        add_scenario(&conn, "B", 40.0, None, None, None, None)?;
        let set = compute_normalized_set(&conn)?;
        assert_eq!(set.overfill_state, OverfillState::Ok);
        assert!((set.modeled_sum - 100.0).abs() < 0.001);
        assert!(set.residual_probability.abs() < 0.001);
        Ok(())
    }

    #[test]
    fn residual_row_cannot_be_mutated_directly() -> Result<()> {
        let conn = test_db()?;
        let residual = get_scenario_by_name(&conn, RESIDUAL_SCENARIO_NAME)?.unwrap();
        let err = update_scenario_probability(&conn, residual.id, 42.0, None).unwrap_err();
        assert!(
            err.to_string().contains("system-managed"),
            "expected system-managed rejection, got {err}"
        );
        let err = remove_scenario(&conn, residual.id).unwrap_err();
        assert!(
            err.to_string().contains("system-managed"),
            "expected system-managed rejection on remove, got {err}"
        );
        // The row by-name path must also reject re-creation.
        let err = add_scenario(&conn, RESIDUAL_SCENARIO_NAME, 5.0, None, None, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("residual"), "got {err}");
        Ok(())
    }

    #[test]
    fn legacy_overfill_is_detected_and_reported() -> Result<()> {
        // Simulate a pre-enforcement DB by inserting rows directly that sum
        // above 100. The normalized-set summary must surface this as
        // `Overfilled` and the residual must clamp at 0 rather than going
        // negative.
        let conn = test_db()?;
        conn.execute(
            "INSERT INTO scenarios (name, probability, status, phase) VALUES (?1, ?2, 'active', 'active')",
            params!["Oil Shock", 70.0],
        )?;
        conn.execute(
            "INSERT INTO scenarios (name, probability, status, phase) VALUES (?1, ?2, 'active', 'active')",
            params!["Hard Recession", 50.0],
        )?;
        let set = compute_normalized_set(&conn)?;
        assert_eq!(set.overfill_state, OverfillState::Overfilled);
        assert!((set.modeled_sum - 120.0).abs() < 0.001);
        // Recompute clamps residual at zero so the row never goes negative.
        recompute_residual_scenario(&conn)?;
        let residual = get_scenario_by_name(&conn, RESIDUAL_SCENARIO_NAME)?.unwrap();
        assert!(
            residual.probability >= 0.0,
            "residual must clamp at 0, got {}",
            residual.probability
        );
        Ok(())
    }

    #[test]
    fn classify_overfill_handles_epsilon_band() {
        assert_eq!(classify_overfill(100.0), OverfillState::Ok);
        assert_eq!(classify_overfill(100.02), OverfillState::Ok); // within epsilon
        assert_eq!(classify_overfill(99.98), OverfillState::Ok); // within epsilon
        assert_eq!(classify_overfill(100.1), OverfillState::Overfilled);
        assert_eq!(classify_overfill(99.5), OverfillState::Underfilled);
    }

    #[test]
    fn resolve_scenario_recomputes_residual() -> Result<()> {
        let conn = test_db()?;
        let a = add_scenario(&conn, "A", 50.0, None, None, None, None)?;
        let _b = add_scenario(&conn, "B", 40.0, None, None, None, None)?;
        // residual should be 10
        let set = compute_normalized_set(&conn)?;
        assert!((set.residual_probability - 10.0).abs() < 0.001);

        resolve_scenario(&conn, a, Some("collapsed"))?;
        // After resolve, modeled set is only B=40, residual absorbs to 60.
        let set = compute_normalized_set(&conn)?;
        assert!((set.modeled_sum - 40.0).abs() < 0.001);
        assert!((set.residual_probability - 60.0).abs() < 0.001);
        Ok(())
    }
}
