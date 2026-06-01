//! `sources_registry` — canonical lookup of people, frameworks, institutions
//! and outlets referenced by the analyst substrate. Mirrored from the live-DB
//! enrichment session (June 1 2026).

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source {
    pub canonical_id: String,
    pub display_name: String,
    /// One of: person, framework, institution, outlet.
    #[serde(rename = "type")]
    pub source_type: String,
    /// JSON array of aliases (stored as TEXT in SQLite).
    pub aliases: Vec<String>,
    /// JSON array of topic tags.
    pub topics: Vec<String>,
    pub framework_summary: Option<String>,
    pub first_referenced_at: Option<String>,
    pub last_referenced_at: Option<String>,
    pub reference_count: i64,
    pub operator_notes: Option<String>,
    pub accuracy_rating: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sources_registry (
            canonical_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            type TEXT NOT NULL CHECK(type IN ('person','framework','institution','outlet')),
            aliases TEXT NOT NULL DEFAULT '[]',
            topics TEXT NOT NULL DEFAULT '[]',
            framework_summary TEXT,
            first_referenced_at TEXT,
            last_referenced_at TEXT,
            reference_count INTEGER NOT NULL DEFAULT 0,
            operator_notes TEXT,
            accuracy_rating TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_sources_registry_type ON sources_registry(type);",
    )?;
    Ok(())
}

fn parse_json_array(raw: Option<String>) -> Vec<String> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

fn to_json_array(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

pub fn list(conn: &Connection, source_type: Option<&str>) -> Result<Vec<Source>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT canonical_id, display_name, type, aliases, topics, framework_summary,
                first_referenced_at, last_referenced_at, reference_count,
                operator_notes, accuracy_rating, created_at, updated_at
         FROM sources_registry",
    );
    if source_type.is_some() {
        sql.push_str(" WHERE type = ?1");
    }
    sql.push_str(" ORDER BY canonical_id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<Source> {
        Ok(Source {
            canonical_id: row.get(0)?,
            display_name: row.get(1)?,
            source_type: row.get(2)?,
            aliases: parse_json_array(row.get(3)?),
            topics: parse_json_array(row.get(4)?),
            framework_summary: row.get(5)?,
            first_referenced_at: row.get(6)?,
            last_referenced_at: row.get(7)?,
            reference_count: row.get(8)?,
            operator_notes: row.get(9)?,
            accuracy_rating: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    };
    let rows: Vec<Source> = if let Some(t) = source_type {
        stmt.query_map(params![t], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
pub fn upsert(
    conn: &Connection,
    canonical_id: &str,
    display_name: &str,
    source_type: &str,
    aliases: &[String],
    topics: &[String],
    accuracy_rating: Option<&str>,
    framework_summary: Option<&str>,
) -> Result<()> {
    ensure_table(conn)?;
    if !["person", "framework", "institution", "outlet"].contains(&source_type) {
        return Err(anyhow!(
            "invalid type '{}'; must be one of person|framework|institution|outlet",
            source_type
        ));
    }
    conn.execute(
        "INSERT INTO sources_registry
            (canonical_id, display_name, type, aliases, topics, framework_summary,
             accuracy_rating, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'), datetime('now'))
         ON CONFLICT(canonical_id) DO UPDATE SET
            display_name = excluded.display_name,
            type = excluded.type,
            aliases = excluded.aliases,
            topics = excluded.topics,
            framework_summary = COALESCE(excluded.framework_summary, sources_registry.framework_summary),
            accuracy_rating = COALESCE(excluded.accuracy_rating, sources_registry.accuracy_rating),
            updated_at = datetime('now')",
        params![
            canonical_id,
            display_name,
            source_type,
            to_json_array(aliases),
            to_json_array(topics),
            framework_summary,
            accuracy_rating,
        ],
    )?;
    Ok(())
}

pub fn remove(conn: &Connection, canonical_id: &str) -> Result<bool> {
    ensure_table(conn)?;
    let affected = conn.execute(
        "DELETE FROM sources_registry WHERE canonical_id = ?1",
        params![canonical_id],
    )?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn upsert_then_list_roundtrips_aliases_and_topics() {
        let conn = fresh_conn();
        upsert(
            &conn,
            "dalio",
            "Ray Dalio",
            "person",
            &["Bridgewater Ray".to_string()],
            &["macro".to_string(), "cycles".to_string()],
            Some("high"),
            None,
        )
        .unwrap();
        let rows = list(&conn, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].display_name, "Ray Dalio");
        assert_eq!(rows[0].source_type, "person");
        assert_eq!(rows[0].aliases, vec!["Bridgewater Ray".to_string()]);
        assert_eq!(rows[0].topics, vec!["macro".to_string(), "cycles".to_string()]);
        assert_eq!(rows[0].accuracy_rating.as_deref(), Some("high"));
    }

    #[test]
    fn list_filters_by_type() {
        let conn = fresh_conn();
        upsert(&conn, "dalio", "Ray Dalio", "person", &[], &[], None, None).unwrap();
        upsert(
            &conn,
            "fourth-turning",
            "Fourth Turning",
            "framework",
            &[],
            &[],
            None,
            None,
        )
        .unwrap();
        let people = list(&conn, Some("person")).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].canonical_id, "dalio");
        let frameworks = list(&conn, Some("framework")).unwrap();
        assert_eq!(frameworks.len(), 1);
        assert_eq!(frameworks[0].canonical_id, "fourth-turning");
    }

    #[test]
    fn remove_drops_row() {
        let conn = fresh_conn();
        upsert(&conn, "dalio", "Ray Dalio", "person", &[], &[], None, None).unwrap();
        assert!(remove(&conn, "dalio").unwrap());
        assert!(list(&conn, None).unwrap().is_empty());
        assert!(!remove(&conn, "dalio").unwrap());
    }

    #[test]
    fn upsert_rejects_invalid_type() {
        let conn = fresh_conn();
        let err = upsert(&conn, "x", "X", "alien", &[], &[], None, None).unwrap_err();
        assert!(err.to_string().contains("invalid type"));
    }
}
