use anyhow::{bail, Result};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

// ── Structs ────────────────────────────────────────────────────────────

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

// ── SQLite row parsers ─────────────────────────────────────────────────

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

// ── Validation helpers ─────────────────────────────────────────────────

const VALID_DIRECTIONS: &[&str] = &["bullish", "bearish", "volatile", "neutral"];
const VALID_TIERS: &[&str] = &["primary", "secondary", "tertiary"];
const VALID_OPERATORS: &[&str] = &[">", ">=", "<", "<=", "above_sma", "below_sma", "rsi_above", "rsi_below"];
const VALID_SEVERITIES: &[&str] = &["low", "normal", "elevated", "critical"];
const VALID_BRANCH_STATUSES: &[&str] = &["active", "resolved", "eliminated"];
const VALID_INDICATOR_STATUSES: &[&str] = &["watching", "triggered", "fading", "expired"];
const VALID_PHASES: &[&str] = &["hypothesis", "active", "resolved"];

fn validate_enum(value: &str, valid: &[&str], field_name: &str) -> Result<()> {
    if !valid.contains(&value) {
        bail!("Invalid {}: '{}'. Must be one of: {}", field_name, value, valid.join(", "));
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// BRANCHES — SQLite
// ══════════════════════════════════════════════════════════════════════

const BRANCH_SELECT: &str =
    "id, scenario_id, name, probability, description, sort_order, status, created_at, updated_at";

pub fn add_branch(
    conn: &Connection,
    scenario_id: i64,
    name: &str,
    probability: f64,
    description: Option<&str>,
    sort_order: Option<i32>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO scenario_branches (scenario_id, name, probability, description, sort_order)
         VALUES (?, ?, ?, ?, ?)",
        params![scenario_id, name, probability, description, sort_order.unwrap_or(0)],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_branches(conn: &Connection, scenario_id: i64) -> Result<Vec<ScenarioBranch>> {
    let sql = format!(
        "SELECT {} FROM scenario_branches WHERE scenario_id = ? ORDER BY sort_order, id",
        BRANCH_SELECT
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([scenario_id], ScenarioBranch::from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_branch_by_name(conn: &Connection, scenario_id: i64, name: &str) -> Result<Option<ScenarioBranch>> {
    let sql = format!(
        "SELECT {} FROM scenario_branches WHERE scenario_id = ? AND name = ?",
        BRANCH_SELECT
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![scenario_id, name], ScenarioBranch::from_row)?;
    Ok(rows.next().transpose()?)
}

pub fn update_branch(
    conn: &Connection,
    branch_id: i64,
    probability: Option<f64>,
    description: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    if let Some(s) = status {
        validate_enum(s, VALID_BRANCH_STATUSES, "branch status")?;
    }
    let mut updates = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(p) = probability {
        updates.push("probability = ?");
        params_vec.push(Box::new(p));
    }
    if let Some(d) = description {
        updates.push("description = ?");
        params_vec.push(Box::new(d.to_string()));
    }
    if let Some(s) = status {
        updates.push("status = ?");
        params_vec.push(Box::new(s.to_string()));
    }
    if updates.is_empty() {
        return Ok(());
    }
    updates.push("updated_at = datetime('now')");
    let sql = format!("UPDATE scenario_branches SET {} WHERE id = ?", updates.join(", "));
    params_vec.push(Box::new(branch_id));
    let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// IMPACTS — SQLite
// ══════════════════════════════════════════════════════════════════════

const IMPACT_SELECT: &str =
    "id, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id, created_at, updated_at";

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
    validate_enum(direction, VALID_DIRECTIONS, "direction")?;
    validate_enum(tier, VALID_TIERS, "tier")?;
    conn.execute(
        "INSERT INTO scenario_impacts (scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_impacts(conn: &Connection, scenario_id: i64) -> Result<Vec<ScenarioImpact>> {
    let sql = format!(
        "SELECT {} FROM scenario_impacts WHERE scenario_id = ? ORDER BY tier, id",
        IMPACT_SELECT
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([scenario_id], ScenarioImpact::from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn list_impacts_for_symbol(conn: &Connection, symbol: &str) -> Result<Vec<ScenarioImpact>> {
    let sql = format!(
        "SELECT {} FROM scenario_impacts WHERE symbol = ? ORDER BY scenario_id, tier, id",
        IMPACT_SELECT
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([symbol], ScenarioImpact::from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ══════════════════════════════════════════════════════════════════════
// INDICATORS — SQLite
// ══════════════════════════════════════════════════════════════════════

const INDICATOR_SELECT: &str =
    "id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, \
     status, triggered_at, last_value, last_checked, created_at, updated_at";

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
    validate_enum(operator, VALID_OPERATORS, "operator")?;
    conn.execute(
        "INSERT INTO scenario_indicators
         (scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_indicators(
    conn: &Connection,
    scenario_id: i64,
    status_filter: Option<&str>,
) -> Result<Vec<ScenarioIndicator>> {
    let sql = if let Some(status) = status_filter {
        validate_enum(status, VALID_INDICATOR_STATUSES, "indicator status")?;
        format!(
            "SELECT {} FROM scenario_indicators WHERE scenario_id = ? AND status = '{}' ORDER BY id",
            INDICATOR_SELECT, status
        )
    } else {
        format!(
            "SELECT {} FROM scenario_indicators WHERE scenario_id = ? ORDER BY id",
            INDICATOR_SELECT
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([scenario_id], ScenarioIndicator::from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn list_all_active_indicators(conn: &Connection) -> Result<Vec<ScenarioIndicator>> {
    let sql = format!(
        "SELECT {} FROM scenario_indicators WHERE status != 'expired' ORDER BY id",
        INDICATOR_SELECT
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], ScenarioIndicator::from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[allow(dead_code)]
pub fn list_indicators_for_scenario_name(
    conn: &Connection,
    scenario_id: i64,
    status_filter: Option<&str>,
) -> Result<Vec<ScenarioIndicator>> {
    list_indicators(conn, scenario_id, status_filter)
}

pub fn update_indicator_status(
    conn: &Connection,
    indicator_id: i64,
    new_status: &str,
    last_value: &str,
) -> Result<()> {
    validate_enum(new_status, VALID_INDICATOR_STATUSES, "indicator status")?;
    let triggered_clause = if new_status == "triggered" {
        ", triggered_at = COALESCE(triggered_at, datetime('now'))"
    } else {
        ""
    };
    let sql = format!(
        "UPDATE scenario_indicators SET status = ?, last_value = ?, last_checked = datetime('now'), updated_at = datetime('now'){} WHERE id = ?",
        triggered_clause
    );
    conn.execute(&sql, params![new_status, last_value, indicator_id])?;
    Ok(())
}

pub fn update_indicator_checked(
    conn: &Connection,
    indicator_id: i64,
    last_value: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE scenario_indicators SET last_value = ?, last_checked = datetime('now') WHERE id = ?",
        params![last_value, indicator_id],
    )?;
    Ok(())
}

pub fn expire_indicators_for_scenario(conn: &Connection, scenario_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE scenario_indicators SET status = 'expired', updated_at = datetime('now') WHERE scenario_id = ? AND status != 'expired'",
        [scenario_id],
    )?;
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════
// UPDATES — SQLite
// ══════════════════════════════════════════════════════════════════════

const UPDATE_SELECT: &str =
    "id, scenario_id, branch_id, headline, detail, severity, source, source_agent, \
     next_decision, next_decision_at, created_at";

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
    validate_enum(severity, VALID_SEVERITIES, "severity")?;
    conn.execute(
        "INSERT INTO scenario_updates
         (scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at)
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
    let sql = if let Some(lim) = limit {
        format!(
            "SELECT {} FROM scenario_updates WHERE scenario_id = ? ORDER BY created_at DESC LIMIT {}",
            UPDATE_SELECT, lim
        )
    } else {
        format!(
            "SELECT {} FROM scenario_updates WHERE scenario_id = ? ORDER BY created_at DESC",
            UPDATE_SELECT
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([scenario_id], ScenarioUpdate::from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ══════════════════════════════════════════════════════════════════════
// PHASE MANAGEMENT — SQLite
// ══════════════════════════════════════════════════════════════════════

pub fn get_scenario_phase(conn: &Connection, scenario_id: i64) -> Result<String> {
    let phase: String = conn.query_row(
        "SELECT phase FROM scenarios WHERE id = ?",
        [scenario_id],
        |row| row.get(0),
    )?;
    Ok(phase)
}

pub fn set_scenario_phase(
    conn: &Connection,
    scenario_id: i64,
    new_phase: &str,
    resolution_notes: Option<&str>,
) -> Result<String> {
    validate_enum(new_phase, VALID_PHASES, "phase")?;
    let old_phase: String = conn.query_row(
        "SELECT phase FROM scenarios WHERE id = ?",
        [scenario_id],
        |row| row.get(0),
    )?;

    let resolved_clause = if new_phase == "resolved" {
        ", resolved_at = datetime('now'), resolution_notes = ?"
    } else if new_phase == "hypothesis" || new_phase == "active" {
        // Clear resolved fields when moving away from resolved
        ", resolved_at = NULL, resolution_notes = NULL"
    } else {
        ""
    };

    let sql = format!(
        "UPDATE scenarios SET phase = ?, updated_at = datetime('now'){} WHERE id = ?",
        resolved_clause
    );

    if new_phase == "resolved" {
        conn.execute(&sql, params![new_phase, resolution_notes, scenario_id])?;
    } else {
        conn.execute(&sql, params![new_phase, scenario_id])?;
    }

    // Log to scenario_history
    let current_prob: f64 = conn.query_row(
        "SELECT probability FROM scenarios WHERE id = ?",
        [scenario_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO scenario_history (scenario_id, probability, driver) VALUES (?, ?, ?)",
        params![
            scenario_id,
            current_prob,
            format!("Phase transition: {} → {}", old_phase, new_phase)
        ],
    )?;

    // On resolution: expire indicators, eliminate non-resolved branches
    if new_phase == "resolved" {
        expire_indicators_for_scenario(conn, scenario_id)?;
        conn.execute(
            "UPDATE scenario_branches SET status = 'eliminated', updated_at = datetime('now')
             WHERE scenario_id = ? AND status = 'active'",
            [scenario_id],
        )?;
    }

    Ok(old_phase)
}

/// Count active branches for a scenario (used for promote warning).
pub fn count_branches(conn: &Connection, scenario_id: i64) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scenario_branches WHERE scenario_id = ?",
        [scenario_id],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

/// Count active situations (phase = 'active').
pub fn count_active_situations(conn: &Connection) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scenarios WHERE phase = 'active'",
        [],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

/// Count triggered indicators across all active scenarios.
pub fn count_triggered_indicators(conn: &Connection) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scenario_indicators si
         JOIN scenarios s ON si.scenario_id = s.id
         WHERE si.status = 'triggered' AND s.phase IN ('hypothesis', 'active')",
        [],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

// ══════════════════════════════════════════════════════════════════════
// POSTGRES IMPLEMENTATIONS
// ══════════════════════════════════════════════════════════════════════

type BranchRow = (i64, i64, String, f64, Option<String>, i32, String, String, String);
type ImpactRow = (i64, i64, Option<i64>, String, String, String, Option<String>, Option<i64>, String, String);
type IndicatorRow = (
    i64, i64, Option<i64>, Option<i64>, String, String, String, String, String,
    String, Option<String>, Option<String>, Option<String>, String, String,
);
type UpdateRow = (
    i64, i64, Option<i64>, String, Option<String>, String, Option<String>,
    Option<String>, Option<String>, Option<String>, String,
);

fn ensure_engine_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        // Add phase/resolved columns to scenarios
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS phase TEXT NOT NULL DEFAULT 'hypothesis'")
            .execute(pool).await?;
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMPTZ")
            .execute(pool).await?;
        sqlx::query("ALTER TABLE scenarios ADD COLUMN IF NOT EXISTS resolution_notes TEXT")
            .execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenarios_phase ON scenarios(phase)")
            .execute(pool).await?;

        // scenario_branches
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_branches (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                probability DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                description TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                CONSTRAINT scenario_branches_unique_name UNIQUE (scenario_id, name)
            )"
        ).execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_branches_scenario ON scenario_branches(scenario_id)")
            .execute(pool).await?;

        // scenario_impacts
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_impacts (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id BIGINT REFERENCES scenario_branches(id) ON DELETE CASCADE,
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                tier TEXT NOT NULL DEFAULT 'primary',
                mechanism TEXT,
                parent_id BIGINT REFERENCES scenario_impacts(id) ON DELETE SET NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"
        ).execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_impacts_scenario ON scenario_impacts(scenario_id)")
            .execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_impacts_symbol ON scenario_impacts(symbol)")
            .execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_impacts_parent ON scenario_impacts(parent_id)")
            .execute(pool).await?;

        // scenario_indicators
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_indicators (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id BIGINT REFERENCES scenario_branches(id) ON DELETE CASCADE,
                impact_id BIGINT REFERENCES scenario_impacts(id) ON DELETE SET NULL,
                symbol TEXT NOT NULL,
                metric TEXT NOT NULL DEFAULT 'close',
                operator TEXT NOT NULL,
                threshold TEXT NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'watching',
                triggered_at TIMESTAMPTZ,
                last_value TEXT,
                last_checked TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"
        ).execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_indicators_scenario ON scenario_indicators(scenario_id)")
            .execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_indicators_symbol ON scenario_indicators(symbol)")
            .execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_indicators_status ON scenario_indicators(status)")
            .execute(pool).await?;

        // scenario_updates
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scenario_updates (
                id BIGSERIAL PRIMARY KEY,
                scenario_id BIGINT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id BIGINT REFERENCES scenario_branches(id) ON DELETE CASCADE,
                headline TEXT NOT NULL,
                detail TEXT,
                severity TEXT NOT NULL DEFAULT 'normal',
                source TEXT,
                source_agent TEXT,
                next_decision TEXT,
                next_decision_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"
        ).execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_updates_scenario ON scenario_updates(scenario_id)")
            .execute(pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_scenario_updates_created ON scenario_updates(created_at DESC)")
            .execute(pool).await?;

        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

// ── Branches — Postgres ────────────────────────────────────────────────

const BRANCH_SELECT_PG: &str =
    "id, scenario_id, name, probability, description, sort_order, status, created_at::text, updated_at::text";

fn add_branch_postgres(pool: &PgPool, scenario_id: i64, name: &str, probability: f64, description: Option<&str>, sort_order: Option<i32>) -> Result<i64> {
    ensure_engine_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_branches (scenario_id, name, probability, description, sort_order)
             VALUES ($1, $2, $3, $4, $5) RETURNING id"
        )
        .bind(scenario_id).bind(name).bind(probability).bind(description).bind(sort_order.unwrap_or(0))
        .fetch_one(pool).await
    })?;
    Ok(id)
}

fn list_branches_postgres(pool: &PgPool, scenario_id: i64) -> Result<Vec<ScenarioBranch>> {
    ensure_engine_tables_postgres(pool)?;
    let rows: Vec<BranchRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&format!(
            "SELECT {} FROM scenario_branches WHERE scenario_id = $1 ORDER BY sort_order, id",
            BRANCH_SELECT_PG
        ))
        .bind(scenario_id)
        .fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(|r| ScenarioBranch {
        id: r.0, scenario_id: r.1, name: r.2, probability: r.3,
        description: r.4, sort_order: r.5, status: r.6, created_at: r.7, updated_at: r.8,
    }).collect())
}

fn get_branch_by_name_postgres(pool: &PgPool, scenario_id: i64, name: &str) -> Result<Option<ScenarioBranch>> {
    ensure_engine_tables_postgres(pool)?;
    let row: Option<BranchRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&format!(
            "SELECT {} FROM scenario_branches WHERE scenario_id = $1 AND name = $2",
            BRANCH_SELECT_PG
        ))
        .bind(scenario_id).bind(name)
        .fetch_optional(pool).await
    })?;
    Ok(row.map(|r| ScenarioBranch {
        id: r.0, scenario_id: r.1, name: r.2, probability: r.3,
        description: r.4, sort_order: r.5, status: r.6, created_at: r.7, updated_at: r.8,
    }))
}

fn update_branch_postgres(pool: &PgPool, branch_id: i64, probability: Option<f64>, description: Option<&str>, status: Option<&str>) -> Result<()> {
    if let Some(s) = status {
        validate_enum(s, VALID_BRANCH_STATUSES, "branch status")?;
    }
    ensure_engine_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE scenario_branches SET
                probability = COALESCE($1, probability),
                description = COALESCE($2, description),
                status = COALESCE($3, status),
                updated_at = NOW()
             WHERE id = $4"
        )
        .bind(probability).bind(description).bind(status).bind(branch_id)
        .execute(pool).await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

// ── Impacts — Postgres ─────────────────────────────────────────────────

const IMPACT_SELECT_PG: &str =
    "id, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id, created_at::text, updated_at::text";

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
    validate_enum(direction, VALID_DIRECTIONS, "direction")?;
    validate_enum(tier, VALID_TIERS, "tier")?;
    ensure_engine_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_impacts (scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id"
        )
        .bind(scenario_id).bind(branch_id).bind(symbol).bind(direction).bind(tier).bind(mechanism).bind(parent_id)
        .fetch_one(pool).await
    })?;
    Ok(id)
}

fn list_impacts_postgres(pool: &PgPool, scenario_id: i64) -> Result<Vec<ScenarioImpact>> {
    ensure_engine_tables_postgres(pool)?;
    let rows: Vec<ImpactRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&format!(
            "SELECT {} FROM scenario_impacts WHERE scenario_id = $1 ORDER BY tier, id",
            IMPACT_SELECT_PG
        ))
        .bind(scenario_id)
        .fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(|r| ScenarioImpact {
        id: r.0, scenario_id: r.1, branch_id: r.2, symbol: r.3,
        direction: r.4, tier: r.5, mechanism: r.6, parent_id: r.7,
        created_at: r.8, updated_at: r.9,
    }).collect())
}

fn list_impacts_for_symbol_postgres(pool: &PgPool, symbol: &str) -> Result<Vec<ScenarioImpact>> {
    ensure_engine_tables_postgres(pool)?;
    let rows: Vec<ImpactRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&format!(
            "SELECT {} FROM scenario_impacts WHERE symbol = $1 ORDER BY scenario_id, tier, id",
            IMPACT_SELECT_PG
        ))
        .bind(symbol)
        .fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(|r| ScenarioImpact {
        id: r.0, scenario_id: r.1, branch_id: r.2, symbol: r.3,
        direction: r.4, tier: r.5, mechanism: r.6, parent_id: r.7,
        created_at: r.8, updated_at: r.9,
    }).collect())
}

// ── Indicators — Postgres ──────────────────────────────────────────────

const INDICATOR_SELECT_PG: &str =
    "id, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label, \
     status, triggered_at::text, last_value, last_checked::text, created_at::text, updated_at::text";

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
    validate_enum(operator, VALID_OPERATORS, "operator")?;
    ensure_engine_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_indicators
             (scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id"
        )
        .bind(scenario_id).bind(branch_id).bind(impact_id).bind(symbol)
        .bind(metric).bind(operator).bind(threshold).bind(label)
        .fetch_one(pool).await
    })?;
    Ok(id)
}

fn list_indicators_postgres(pool: &PgPool, scenario_id: i64, status_filter: Option<&str>) -> Result<Vec<ScenarioIndicator>> {
    if let Some(s) = status_filter {
        validate_enum(s, VALID_INDICATOR_STATUSES, "indicator status")?;
    }
    ensure_engine_tables_postgres(pool)?;
    let rows: Vec<IndicatorRow> = if let Some(status) = status_filter {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(&format!(
                "SELECT {} FROM scenario_indicators WHERE scenario_id = $1 AND status = $2 ORDER BY id",
                INDICATOR_SELECT_PG
            ))
            .bind(scenario_id).bind(status)
            .fetch_all(pool).await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(&format!(
                "SELECT {} FROM scenario_indicators WHERE scenario_id = $1 ORDER BY id",
                INDICATOR_SELECT_PG
            ))
            .bind(scenario_id)
            .fetch_all(pool).await
        })?
    };
    Ok(rows.into_iter().map(|r| ScenarioIndicator {
        id: r.0, scenario_id: r.1, branch_id: r.2, impact_id: r.3,
        symbol: r.4, metric: r.5, operator: r.6, threshold: r.7, label: r.8,
        status: r.9, triggered_at: r.10, last_value: r.11, last_checked: r.12,
        created_at: r.13, updated_at: r.14,
    }).collect())
}

fn list_all_active_indicators_postgres(pool: &PgPool) -> Result<Vec<ScenarioIndicator>> {
    ensure_engine_tables_postgres(pool)?;
    let rows: Vec<IndicatorRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&format!(
            "SELECT {} FROM scenario_indicators WHERE status != 'expired' ORDER BY id",
            INDICATOR_SELECT_PG
        ))
        .fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(|r| ScenarioIndicator {
        id: r.0, scenario_id: r.1, branch_id: r.2, impact_id: r.3,
        symbol: r.4, metric: r.5, operator: r.6, threshold: r.7, label: r.8,
        status: r.9, triggered_at: r.10, last_value: r.11, last_checked: r.12,
        created_at: r.13, updated_at: r.14,
    }).collect())
}

fn update_indicator_status_postgres(pool: &PgPool, indicator_id: i64, new_status: &str, last_value: &str) -> Result<()> {
    validate_enum(new_status, VALID_INDICATOR_STATUSES, "indicator status")?;
    ensure_engine_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        if new_status == "triggered" {
            sqlx::query(
                "UPDATE scenario_indicators SET status = $1, last_value = $2, last_checked = NOW(),
                 triggered_at = COALESCE(triggered_at, NOW()), updated_at = NOW() WHERE id = $3"
            )
            .bind(new_status).bind(last_value).bind(indicator_id)
            .execute(pool).await?;
        } else {
            sqlx::query(
                "UPDATE scenario_indicators SET status = $1, last_value = $2, last_checked = NOW(),
                 updated_at = NOW() WHERE id = $3"
            )
            .bind(new_status).bind(last_value).bind(indicator_id)
            .execute(pool).await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn update_indicator_checked_postgres(pool: &PgPool, indicator_id: i64, last_value: &str) -> Result<()> {
    ensure_engine_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE scenario_indicators SET last_value = $1, last_checked = NOW() WHERE id = $2")
            .bind(last_value).bind(indicator_id)
            .execute(pool).await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(dead_code)]
fn expire_indicators_for_scenario_postgres(pool: &PgPool, scenario_id: i64) -> Result<()> {
    ensure_engine_tables_postgres(pool)?;
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE scenario_indicators SET status = 'expired', updated_at = NOW()
             WHERE scenario_id = $1 AND status != 'expired'"
        )
        .bind(scenario_id)
        .execute(pool).await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

// ── Updates — Postgres ─────────────────────────────────────────────────

const UPDATE_SELECT_PG: &str =
    "id, scenario_id, branch_id, headline, detail, severity, source, source_agent, \
     next_decision, next_decision_at::text, created_at::text";

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
    validate_enum(severity, VALID_SEVERITIES, "severity")?;
    ensure_engine_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO scenario_updates
             (scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::timestamptz) RETURNING id"
        )
        .bind(scenario_id).bind(branch_id).bind(headline).bind(detail).bind(severity)
        .bind(source).bind(source_agent).bind(next_decision).bind(next_decision_at)
        .fetch_one(pool).await
    })?;
    Ok(id)
}

fn list_updates_postgres(pool: &PgPool, scenario_id: i64, limit: Option<usize>) -> Result<Vec<ScenarioUpdate>> {
    ensure_engine_tables_postgres(pool)?;
    let rows: Vec<UpdateRow> = if let Some(lim) = limit {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(&format!(
                "SELECT {} FROM scenario_updates WHERE scenario_id = $1 ORDER BY created_at DESC LIMIT $2",
                UPDATE_SELECT_PG
            ))
            .bind(scenario_id).bind(lim as i64)
            .fetch_all(pool).await
        })?
    } else {
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(&format!(
                "SELECT {} FROM scenario_updates WHERE scenario_id = $1 ORDER BY created_at DESC",
                UPDATE_SELECT_PG
            ))
            .bind(scenario_id)
            .fetch_all(pool).await
        })?
    };
    Ok(rows.into_iter().map(|r| ScenarioUpdate {
        id: r.0, scenario_id: r.1, branch_id: r.2, headline: r.3,
        detail: r.4, severity: r.5, source: r.6, source_agent: r.7,
        next_decision: r.8, next_decision_at: r.9, created_at: r.10,
    }).collect())
}

// ── Phase management — Postgres ────────────────────────────────────────

fn get_scenario_phase_postgres(pool: &PgPool, scenario_id: i64) -> Result<String> {
    ensure_engine_tables_postgres(pool)?;
    let phase: String = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT phase FROM scenarios WHERE id = $1")
            .bind(scenario_id)
            .fetch_one(pool).await
    })?;
    Ok(phase)
}

fn set_scenario_phase_postgres(pool: &PgPool, scenario_id: i64, new_phase: &str, resolution_notes: Option<&str>) -> Result<String> {
    validate_enum(new_phase, VALID_PHASES, "phase")?;
    ensure_engine_tables_postgres(pool)?;

    let old_phase: String = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT phase FROM scenarios WHERE id = $1")
            .bind(scenario_id)
            .fetch_one(pool).await
    })?;

    crate::db::pg_runtime::block_on(async {
        if new_phase == "resolved" {
            sqlx::query("UPDATE scenarios SET phase = $1, resolved_at = NOW(), resolution_notes = $2, updated_at = NOW() WHERE id = $3")
                .bind(new_phase).bind(resolution_notes).bind(scenario_id)
                .execute(pool).await?;
        } else {
            sqlx::query("UPDATE scenarios SET phase = $1, resolved_at = NULL, resolution_notes = NULL, updated_at = NOW() WHERE id = $2")
                .bind(new_phase).bind(scenario_id)
                .execute(pool).await?;
        }

        // Log to scenario_history
        let current_prob: f64 = sqlx::query_scalar("SELECT probability FROM scenarios WHERE id = $1")
            .bind(scenario_id)
            .fetch_one(pool).await?;
        sqlx::query("INSERT INTO scenario_history (scenario_id, probability, driver) VALUES ($1, $2, $3)")
            .bind(scenario_id).bind(current_prob)
            .bind(format!("Phase transition: {} → {}", old_phase, new_phase))
            .execute(pool).await?;

        // On resolution: expire indicators, eliminate non-resolved branches
        if new_phase == "resolved" {
            sqlx::query("UPDATE scenario_indicators SET status = 'expired', updated_at = NOW() WHERE scenario_id = $1 AND status != 'expired'")
                .bind(scenario_id)
                .execute(pool).await?;
            sqlx::query("UPDATE scenario_branches SET status = 'eliminated', updated_at = NOW() WHERE scenario_id = $1 AND status = 'active'")
                .bind(scenario_id)
                .execute(pool).await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(old_phase)
}

fn count_branches_postgres(pool: &PgPool, scenario_id: i64) -> Result<usize> {
    ensure_engine_tables_postgres(pool)?;
    let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM scenario_branches WHERE scenario_id = $1")
            .bind(scenario_id)
            .fetch_one(pool).await
    })?;
    Ok(count as usize)
}

fn count_active_situations_postgres(pool: &PgPool) -> Result<usize> {
    ensure_engine_tables_postgres(pool)?;
    let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM scenarios WHERE phase = 'active'")
            .fetch_one(pool).await
    })?;
    Ok(count as usize)
}

fn count_triggered_indicators_postgres(pool: &PgPool) -> Result<usize> {
    ensure_engine_tables_postgres(pool)?;
    let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM scenario_indicators si
             JOIN scenarios s ON si.scenario_id = s.id
             WHERE si.status = 'triggered' AND s.phase IN ('hypothesis', 'active')"
        )
        .fetch_one(pool).await
    })?;
    Ok(count as usize)
}

// ══════════════════════════════════════════════════════════════════════
// BACKEND DISPATCH (public API)
// ══════════════════════════════════════════════════════════════════════

// Branches
pub fn add_branch_backend(backend: &BackendConnection, scenario_id: i64, name: &str, probability: f64, description: Option<&str>, sort_order: Option<i32>) -> Result<i64> {
    query::dispatch(backend, |c| add_branch(c, scenario_id, name, probability, description, sort_order), |p| add_branch_postgres(p, scenario_id, name, probability, description, sort_order))
}

pub fn list_branches_backend(backend: &BackendConnection, scenario_id: i64) -> Result<Vec<ScenarioBranch>> {
    query::dispatch(backend, |c| list_branches(c, scenario_id), |p| list_branches_postgres(p, scenario_id))
}

pub fn get_branch_by_name_backend(backend: &BackendConnection, scenario_id: i64, name: &str) -> Result<Option<ScenarioBranch>> {
    query::dispatch(backend, |c| get_branch_by_name(c, scenario_id, name), |p| get_branch_by_name_postgres(p, scenario_id, name))
}

pub fn update_branch_backend(backend: &BackendConnection, branch_id: i64, probability: Option<f64>, description: Option<&str>, status: Option<&str>) -> Result<()> {
    query::dispatch(backend, |c| update_branch(c, branch_id, probability, description, status), |p| update_branch_postgres(p, branch_id, probability, description, status))
}

// Impacts
#[allow(clippy::too_many_arguments)]
pub fn add_impact_backend(backend: &BackendConnection, scenario_id: i64, branch_id: Option<i64>, symbol: &str, direction: &str, tier: &str, mechanism: Option<&str>, parent_id: Option<i64>) -> Result<i64> {
    query::dispatch(backend, |c| add_impact(c, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id), |p| add_impact_postgres(p, scenario_id, branch_id, symbol, direction, tier, mechanism, parent_id))
}

pub fn list_impacts_backend(backend: &BackendConnection, scenario_id: i64) -> Result<Vec<ScenarioImpact>> {
    query::dispatch(backend, |c| list_impacts(c, scenario_id), |p| list_impacts_postgres(p, scenario_id))
}

pub fn list_impacts_for_symbol_backend(backend: &BackendConnection, symbol: &str) -> Result<Vec<ScenarioImpact>> {
    query::dispatch(backend, |c| list_impacts_for_symbol(c, symbol), |p| list_impacts_for_symbol_postgres(p, symbol))
}

// Indicators
#[allow(clippy::too_many_arguments)]
pub fn add_indicator_backend(backend: &BackendConnection, scenario_id: i64, branch_id: Option<i64>, impact_id: Option<i64>, symbol: &str, metric: &str, operator: &str, threshold: &str, label: &str) -> Result<i64> {
    query::dispatch(backend, |c| add_indicator(c, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label), |p| add_indicator_postgres(p, scenario_id, branch_id, impact_id, symbol, metric, operator, threshold, label))
}

pub fn list_indicators_backend(backend: &BackendConnection, scenario_id: i64, status_filter: Option<&str>) -> Result<Vec<ScenarioIndicator>> {
    query::dispatch(backend, |c| list_indicators(c, scenario_id, status_filter), |p| list_indicators_postgres(p, scenario_id, status_filter))
}

pub fn list_all_active_indicators_backend(backend: &BackendConnection) -> Result<Vec<ScenarioIndicator>> {
    query::dispatch(backend, list_all_active_indicators, list_all_active_indicators_postgres)
}

pub fn update_indicator_status_backend(backend: &BackendConnection, indicator_id: i64, new_status: &str, last_value: &str) -> Result<()> {
    query::dispatch(backend, |c| update_indicator_status(c, indicator_id, new_status, last_value), |p| update_indicator_status_postgres(p, indicator_id, new_status, last_value))
}

pub fn update_indicator_checked_backend(backend: &BackendConnection, indicator_id: i64, last_value: &str) -> Result<()> {
    query::dispatch(backend, |c| update_indicator_checked(c, indicator_id, last_value), |p| update_indicator_checked_postgres(p, indicator_id, last_value))
}

// Updates
#[allow(clippy::too_many_arguments)]
pub fn add_update_backend(backend: &BackendConnection, scenario_id: i64, branch_id: Option<i64>, headline: &str, detail: Option<&str>, severity: &str, source: Option<&str>, source_agent: Option<&str>, next_decision: Option<&str>, next_decision_at: Option<&str>) -> Result<i64> {
    query::dispatch(backend, |c| add_update(c, scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at), |p| add_update_postgres(p, scenario_id, branch_id, headline, detail, severity, source, source_agent, next_decision, next_decision_at))
}

pub fn list_updates_backend(backend: &BackendConnection, scenario_id: i64, limit: Option<usize>) -> Result<Vec<ScenarioUpdate>> {
    query::dispatch(backend, |c| list_updates(c, scenario_id, limit), |p| list_updates_postgres(p, scenario_id, limit))
}

// Phase management
pub fn get_scenario_phase_backend(backend: &BackendConnection, scenario_id: i64) -> Result<String> {
    query::dispatch(backend, |c| get_scenario_phase(c, scenario_id), |p| get_scenario_phase_postgres(p, scenario_id))
}

pub fn set_scenario_phase_backend(backend: &BackendConnection, scenario_id: i64, new_phase: &str, resolution_notes: Option<&str>) -> Result<String> {
    query::dispatch(backend, |c| set_scenario_phase(c, scenario_id, new_phase, resolution_notes), |p| set_scenario_phase_postgres(p, scenario_id, new_phase, resolution_notes))
}

pub fn count_branches_backend(backend: &BackendConnection, scenario_id: i64) -> Result<usize> {
    query::dispatch(backend, |c| count_branches(c, scenario_id), |p| count_branches_postgres(p, scenario_id))
}

pub fn count_active_situations_backend(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(backend, count_active_situations, count_active_situations_postgres)
}

pub fn count_triggered_indicators_backend(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(backend, count_triggered_indicators, count_triggered_indicators_postgres)
}

// ══════════════════════════════════════════════════════════════════════
// TESTS
// ══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = crate::db::open_in_memory();
        // Create engine tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scenario_branches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                probability REAL NOT NULL DEFAULT 0.0,
                description TEXT,
                sort_order INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE (scenario_id, name)
            );
            CREATE TABLE IF NOT EXISTS scenario_impacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id INTEGER REFERENCES scenario_branches(id) ON DELETE CASCADE,
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                tier TEXT NOT NULL DEFAULT 'primary',
                mechanism TEXT,
                parent_id INTEGER REFERENCES scenario_impacts(id) ON DELETE SET NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS scenario_indicators (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id INTEGER REFERENCES scenario_branches(id) ON DELETE CASCADE,
                impact_id INTEGER REFERENCES scenario_impacts(id) ON DELETE SET NULL,
                symbol TEXT NOT NULL,
                metric TEXT NOT NULL DEFAULT 'close',
                operator TEXT NOT NULL,
                threshold TEXT NOT NULL,
                label TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'watching',
                triggered_at TEXT,
                last_value TEXT,
                last_checked TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS scenario_updates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scenario_id INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                branch_id INTEGER REFERENCES scenario_branches(id) ON DELETE CASCADE,
                headline TEXT NOT NULL,
                detail TEXT,
                severity TEXT NOT NULL DEFAULT 'normal',
                source TEXT,
                source_agent TEXT,
                next_decision TEXT,
                next_decision_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        ).unwrap();
        // Add phase column to scenarios
        let has_phase: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('scenarios') WHERE name = 'phase'")
            .unwrap()
            .query_row([], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !has_phase {
            conn.execute_batch(
                "ALTER TABLE scenarios ADD COLUMN phase TEXT NOT NULL DEFAULT 'hypothesis';
                 ALTER TABLE scenarios ADD COLUMN resolved_at TEXT;
                 ALTER TABLE scenarios ADD COLUMN resolution_notes TEXT;"
            ).unwrap();
        }
        conn
    }

    fn add_test_scenario(conn: &Connection) -> i64 {
        crate::db::scenarios::add_scenario(conn, "Test Scenario", 50.0, Some("desc"), None, None, None).unwrap()
    }

    #[test]
    fn test_branch_crud() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        let bid = add_branch(&conn, sid, "Branch A", 60.0, Some("first branch"), None).unwrap();
        assert!(bid > 0);

        let branches = list_branches(&conn, sid).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].name, "Branch A");
        assert_eq!(branches[0].probability, 60.0);

        update_branch(&conn, bid, Some(70.0), None, None).unwrap();
        let branches = list_branches(&conn, sid).unwrap();
        assert_eq!(branches[0].probability, 70.0);

        let found = get_branch_by_name(&conn, sid, "Branch A").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().probability, 70.0);

        let not_found = get_branch_by_name(&conn, sid, "Nonexistent").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_impact_crud() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        let iid = add_impact(&conn, sid, None, "CL=F", "bullish", "primary", Some("oil supply"), None).unwrap();
        assert!(iid > 0);

        let iid2 = add_impact(&conn, sid, None, "GC=F", "bullish", "secondary", Some("inflation"), Some(iid)).unwrap();
        assert!(iid2 > 0);

        let impacts = list_impacts(&conn, sid).unwrap();
        assert_eq!(impacts.len(), 2);
        assert_eq!(impacts[0].symbol, "CL=F");
        assert_eq!(impacts[0].tier, "primary");
        assert_eq!(impacts[1].parent_id, Some(iid));

        let sym_impacts = list_impacts_for_symbol(&conn, "GC=F").unwrap();
        assert_eq!(sym_impacts.len(), 1);
    }

    #[test]
    fn test_indicator_crud() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        let ind_id = add_indicator(&conn, sid, None, None, "CL=F", "close", ">", "110.00", "Oil above $110").unwrap();
        assert!(ind_id > 0);

        let indicators = list_indicators(&conn, sid, None).unwrap();
        assert_eq!(indicators.len(), 1);
        assert_eq!(indicators[0].status, "watching");
        assert_eq!(indicators[0].operator, ">");

        // Test status update
        update_indicator_status(&conn, ind_id, "triggered", "115.50").unwrap();
        let indicators = list_indicators(&conn, sid, Some("triggered")).unwrap();
        assert_eq!(indicators.len(), 1);
        assert!(indicators[0].triggered_at.is_some());

        // Test fading
        update_indicator_status(&conn, ind_id, "fading", "108.00").unwrap();
        let indicators = list_indicators(&conn, sid, Some("fading")).unwrap();
        assert_eq!(indicators.len(), 1);

        // Test expire all
        let _ind2 = add_indicator(&conn, sid, None, None, "SPY", "close", "<", "500", "SPY below 500").unwrap();
        expire_indicators_for_scenario(&conn, sid).unwrap();
        let all = list_indicators(&conn, sid, None).unwrap();
        assert!(all.iter().all(|i| i.status == "expired"));
    }

    #[test]
    fn test_update_crud() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        let uid = add_update(&conn, sid, None, "Breaking news", Some("Details"), "critical", Some("Reuters"), Some("low-agent"), Some("UN vote"), Some("2026-03-17T15:00:00Z")).unwrap();
        assert!(uid > 0);

        let updates = list_updates(&conn, sid, None).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].headline, "Breaking news");
        assert_eq!(updates[0].severity, "critical");

        let limited = list_updates(&conn, sid, Some(0)).unwrap();
        assert_eq!(limited.len(), 0);
    }

    #[test]
    fn test_phase_management() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        let phase = get_scenario_phase(&conn, sid).unwrap();
        assert_eq!(phase, "hypothesis");

        let old = set_scenario_phase(&conn, sid, "active", None).unwrap();
        assert_eq!(old, "hypothesis");

        let phase = get_scenario_phase(&conn, sid).unwrap();
        assert_eq!(phase, "active");

        // Add a branch and indicator then resolve
        let bid = add_branch(&conn, sid, "Test Branch", 100.0, None, None).unwrap();
        let _ind = add_indicator(&conn, sid, Some(bid), None, "CL=F", "close", ">", "100", "test").unwrap();

        let old = set_scenario_phase(&conn, sid, "resolved", Some("Resolved ok")).unwrap();
        assert_eq!(old, "active");

        // Branch should be eliminated, indicator expired
        let branches = list_branches(&conn, sid).unwrap();
        assert_eq!(branches[0].status, "eliminated");
        let indicators = list_indicators(&conn, sid, None).unwrap();
        assert_eq!(indicators[0].status, "expired");
    }

    #[test]
    fn test_validation_rejects_invalid_enum() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        assert!(add_impact(&conn, sid, None, "CL=F", "invalid_dir", "primary", None, None).is_err());
        assert!(add_impact(&conn, sid, None, "CL=F", "bullish", "invalid_tier", None, None).is_err());
        assert!(add_indicator(&conn, sid, None, None, "CL=F", "close", "invalid_op", "100", "test").is_err());
    }

    #[test]
    fn test_count_active_situations() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        assert_eq!(count_active_situations(&conn).unwrap(), 0);
        set_scenario_phase(&conn, sid, "active", None).unwrap();
        assert_eq!(count_active_situations(&conn).unwrap(), 1);
    }

    #[test]
    fn test_count_triggered_indicators() {
        let conn = setup_db();
        let sid = add_test_scenario(&conn);

        let ind = add_indicator(&conn, sid, None, None, "CL=F", "close", ">", "100", "test").unwrap();
        assert_eq!(count_triggered_indicators(&conn).unwrap(), 0);
        update_indicator_status(&conn, ind, "triggered", "105").unwrap();
        assert_eq!(count_triggered_indicators(&conn).unwrap(), 1);
    }
}