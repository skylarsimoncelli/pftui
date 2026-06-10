//! `standing_rules` table — consolidated operational rules distilled from
//! the `prediction_lessons` library. The lesson book injects only the 25
//! most recent lessons into analyst prompts, so recurring failure patterns
//! (e.g. the magnitude-overshoot lesson duplicated ~25 times) crowd out
//! older distinct lessons. A standing rule is the consolidation target:
//! one imperative rule, its rationale, and the source lesson ids it
//! replaces. Rules are injected into prompts in full (they are few and
//! compact), so render them tersely.
//!
//! Enforcement levels:
//!   advisory  — injected into prompts; analysts self-police
//!   validator — a write-time validator may cite it mechanically
//!
//! `violation_count` is incremented via `pftui analytics lessons rules cite <id>`
//! whenever an analyst (or validator) flags a rule being violated.

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandingRule {
    pub id: i64,
    pub rule: String,
    pub rationale: Option<String>,
    pub source_lesson_ids: Option<String>,
    pub enforcement: String,
    pub violation_count: i64,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl StandingRule {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            rule: row.get(1)?,
            rationale: row.get(2)?,
            source_lesson_ids: row.get(3)?,
            enforcement: row.get(4)?,
            violation_count: row.get(5)?,
            status: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }
}

/// Create the standing_rules table if it does not exist. Safe to call on
/// every startup (wired into `db::schema::run_migrations`).
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS standing_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            rule TEXT NOT NULL,
            rationale TEXT,
            source_lesson_ids TEXT,
            enforcement TEXT NOT NULL DEFAULT 'advisory'
                CHECK(enforcement IN ('advisory','validator')),
            violation_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active','retired')),
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_standing_rules_status
            ON standing_rules(status);",
    )?;
    Ok(())
}

pub fn validate_enforcement(value: &str) -> Result<()> {
    match value {
        "advisory" | "validator" => Ok(()),
        _ => anyhow::bail!(
            "invalid enforcement '{}'. Valid: advisory, validator",
            value
        ),
    }
}

pub fn add_rule(
    conn: &Connection,
    rule: &str,
    rationale: Option<&str>,
    source_lesson_ids: Option<&str>,
    enforcement: &str,
) -> Result<i64> {
    ensure_table(conn)?;
    validate_enforcement(enforcement)?;
    if rule.trim().is_empty() {
        anyhow::bail!("rule text must not be empty");
    }
    conn.execute(
        "INSERT INTO standing_rules (rule, rationale, source_lesson_ids, enforcement)
         VALUES (?, ?, ?, ?)",
        params![rule.trim(), rationale, source_lesson_ids, enforcement],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_rules(conn: &Connection, include_retired: bool) -> Result<Vec<StandingRule>> {
    ensure_table(conn)?;
    let sql = if include_retired {
        "SELECT id, rule, rationale, source_lesson_ids, enforcement,
                violation_count, status, created_at, updated_at
         FROM standing_rules ORDER BY status ASC, id ASC"
    } else {
        "SELECT id, rule, rationale, source_lesson_ids, enforcement,
                violation_count, status, created_at, updated_at
         FROM standing_rules WHERE status = 'active' ORDER BY id ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], StandingRule::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_rule(conn: &Connection, id: i64) -> Result<Option<StandingRule>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, rule, rationale, source_lesson_ids, enforcement,
                violation_count, status, created_at, updated_at
         FROM standing_rules WHERE id = ?",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(StandingRule::from_row(row)?))
    } else {
        Ok(None)
    }
}

/// Mark a rule retired. Returns false if the id does not exist.
pub fn retire_rule(conn: &Connection, id: i64) -> Result<bool> {
    ensure_table(conn)?;
    let n = conn.execute(
        "UPDATE standing_rules
         SET status = 'retired', updated_at = datetime('now')
         WHERE id = ?",
        params![id],
    )?;
    Ok(n > 0)
}

/// Increment a rule's violation_count (an analyst flagged the rule being
/// violated). Returns the new count, or None if the id does not exist.
pub fn cite_rule(conn: &Connection, id: i64) -> Result<Option<i64>> {
    ensure_table(conn)?;
    let n = conn.execute(
        "UPDATE standing_rules
         SET violation_count = violation_count + 1, updated_at = datetime('now')
         WHERE id = ?",
        params![id],
    )?;
    if n == 0 {
        return Ok(None);
    }
    let count: i64 = conn.query_row(
        "SELECT violation_count FROM standing_rules WHERE id = ?",
        params![id],
        |row| row.get(0),
    )?;
    Ok(Some(count))
}

// ---------------------------------------------------------------------------
// Backend wrappers (SQLite-native; Postgres not implemented yet, matching
// the other epistemics substrate tables such as lesson curation).
// ---------------------------------------------------------------------------

fn pg_unimplemented<T>() -> Result<T> {
    anyhow::bail!("standing rules are not implemented for the Postgres backend yet")
}

pub fn add_rule_backend(
    backend: &BackendConnection,
    rule: &str,
    rationale: Option<&str>,
    source_lesson_ids: Option<&str>,
    enforcement: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_rule(conn, rule, rationale, source_lesson_ids, enforcement),
        |_pool| pg_unimplemented(),
    )
}

pub fn list_rules_backend(
    backend: &BackendConnection,
    include_retired: bool,
) -> Result<Vec<StandingRule>> {
    query::dispatch(
        backend,
        |conn| list_rules(conn, include_retired),
        |_pool| pg_unimplemented(),
    )
}

pub fn retire_rule_backend(backend: &BackendConnection, id: i64) -> Result<bool> {
    query::dispatch(backend, |conn| retire_rule(conn, id), |_pool| {
        pg_unimplemented()
    })
}

pub fn cite_rule_backend(backend: &BackendConnection, id: i64) -> Result<Option<i64>> {
    query::dispatch(backend, |conn| cite_rule(conn, id), |_pool| {
        pg_unimplemented()
    })
}

pub fn get_rule_backend(backend: &BackendConnection, id: i64) -> Result<Option<StandingRule>> {
    query::dispatch(backend, |conn| get_rule(conn, id), |_pool| {
        pg_unimplemented()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        ensure_table(&c).unwrap();
        c
    }

    #[test]
    fn add_list_roundtrip() {
        let c = conn();
        let id = add_rule(
            &c,
            "Cap magnitude forecasts at 1.5x trailing 30d realized vol.",
            Some("Prevents the magnitude-overshoot failure pattern (25 duplicate lessons)."),
            Some("12,40,77"),
            "advisory",
        )
        .unwrap();
        assert_eq!(id, 1);
        let rules = list_rules(&c, false).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].enforcement, "advisory");
        assert_eq!(rules[0].violation_count, 0);
        assert_eq!(rules[0].status, "active");
        assert_eq!(rules[0].source_lesson_ids.as_deref(), Some("12,40,77"));
    }

    #[test]
    fn rejects_invalid_enforcement_and_empty_rule() {
        let c = conn();
        assert!(add_rule(&c, "x", None, None, "mandatory").is_err());
        assert!(add_rule(&c, "   ", None, None, "advisory").is_err());
    }

    #[test]
    fn retire_hides_from_default_list() {
        let c = conn();
        let id = add_rule(&c, "Rule A", None, None, "validator").unwrap();
        add_rule(&c, "Rule B", None, None, "advisory").unwrap();
        assert!(retire_rule(&c, id).unwrap());
        let active = list_rules(&c, false).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].rule, "Rule B");
        let all = list_rules(&c, true).unwrap();
        assert_eq!(all.len(), 2);
        // retire of a missing id reports false
        assert!(!retire_rule(&c, 999).unwrap());
    }

    #[test]
    fn cite_increments_violation_count() {
        let c = conn();
        let id = add_rule(&c, "Rule A", None, None, "advisory").unwrap();
        assert_eq!(cite_rule(&c, id).unwrap(), Some(1));
        assert_eq!(cite_rule(&c, id).unwrap(), Some(2));
        assert_eq!(cite_rule(&c, 999).unwrap(), None);
    }
}
