//! `reasoning_fragments` + `lesson_fragment_edges` — typed heuristics and
//! rules distilled from the lesson book, plus the lesson↔fragment edges that
//! let the system answer "which fragments apply to this claim?". Mirrored from
//! the live-DB enrichment session (June 1 2026).

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub const VALID_FRAGMENT_TYPES: &[&str] = &[
    "heuristic",
    "signal-rule",
    "correlation-rule",
    "threshold-rule",
    "base-rate",
    "anti-pattern",
];

#[allow(dead_code)]
pub const VALID_CONFIDENCE: &[&str] = &["low", "medium", "high"];

#[allow(dead_code)]
pub const VALID_EDGE_STRENGTH: &[&str] = &["primary", "secondary", "tangential"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReasoningFragment {
    pub canonical_id: String,
    pub fragment: String,
    pub fragment_type: String,
    pub topic: String,
    pub related_lessons: Vec<i64>,
    pub cited_count: i64,
    pub confidence: String,
    pub derivation: Option<String>,
    pub operator_endorsed: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FragmentWithEdges {
    #[serde(flatten)]
    pub fragment: ReasoningFragment,
    pub edges: Vec<FragmentEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FragmentEdge {
    pub lesson_id: i64,
    pub fragment_canonical_id: String,
    pub edge_strength: String,
    pub created_at: String,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS reasoning_fragments (
            canonical_id TEXT PRIMARY KEY,
            fragment TEXT NOT NULL,
            fragment_type TEXT NOT NULL CHECK(fragment_type IN (
                'heuristic','signal-rule','correlation-rule',
                'threshold-rule','base-rate','anti-pattern'
            )),
            topic TEXT NOT NULL DEFAULT 'other',
            related_lessons TEXT NOT NULL DEFAULT '[]',
            cited_count INTEGER NOT NULL DEFAULT 0,
            confidence TEXT NOT NULL DEFAULT 'medium'
                CHECK(confidence IN ('low','medium','high')),
            derivation TEXT,
            operator_endorsed INTEGER NOT NULL DEFAULT 0
                CHECK(operator_endorsed IN (0,1)),
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_reasoning_fragments_type
            ON reasoning_fragments(fragment_type);
        CREATE INDEX IF NOT EXISTS idx_reasoning_fragments_topic
            ON reasoning_fragments(topic);
        CREATE TABLE IF NOT EXISTS lesson_fragment_edges (
            lesson_id INTEGER NOT NULL,
            fragment_canonical_id TEXT NOT NULL,
            edge_strength TEXT NOT NULL CHECK(
                edge_strength IN ('primary','secondary','tangential')
            ),
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(lesson_id, fragment_canonical_id)
        );
        CREATE INDEX IF NOT EXISTS idx_lesson_fragment_edges_fragment
            ON lesson_fragment_edges(fragment_canonical_id);
        CREATE INDEX IF NOT EXISTS idx_lesson_fragment_edges_lesson
            ON lesson_fragment_edges(lesson_id);",
    )?;
    Ok(())
}

fn parse_i64_array(raw: Option<String>) -> Vec<i64> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<Vec<i64>>(s).ok())
        .unwrap_or_default()
}

#[allow(clippy::type_complexity)]
fn row_to_fragment(row: &rusqlite::Row) -> rusqlite::Result<ReasoningFragment> {
    Ok(ReasoningFragment {
        canonical_id: row.get(0)?,
        fragment: row.get(1)?,
        fragment_type: row.get(2)?,
        topic: row.get(3)?,
        related_lessons: parse_i64_array(row.get(4)?),
        cited_count: row.get(5)?,
        confidence: row.get(6)?,
        derivation: row.get(7)?,
        operator_endorsed: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

pub fn list(
    conn: &Connection,
    fragment_type: Option<&str>,
    topic: Option<&str>,
) -> Result<Vec<ReasoningFragment>> {
    ensure_table(conn)?;
    let mut sql = String::from(
        "SELECT canonical_id, fragment, fragment_type, topic, related_lessons,
                cited_count, confidence, derivation, operator_endorsed, created_at
         FROM reasoning_fragments WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(t) = fragment_type {
        sql.push_str(" AND fragment_type = ?");
        args.push(Box::new(t.to_string()));
    }
    if let Some(t) = topic {
        sql.push_str(" AND topic = ?");
        args.push(Box::new(t.to_string()));
    }
    sql.push_str(" ORDER BY cited_count DESC, canonical_id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_slice.as_slice(), row_to_fragment)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, canonical_id: &str) -> Result<Option<ReasoningFragment>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT canonical_id, fragment, fragment_type, topic, related_lessons,
                cited_count, confidence, derivation, operator_endorsed, created_at
         FROM reasoning_fragments WHERE canonical_id = ?1",
    )?;
    let result = stmt.query_row(params![canonical_id], row_to_fragment);
    match result {
        Ok(f) => Ok(Some(f)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn edges_for_fragment(conn: &Connection, canonical_id: &str) -> Result<Vec<FragmentEdge>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT lesson_id, fragment_canonical_id, edge_strength, created_at
         FROM lesson_fragment_edges WHERE fragment_canonical_id = ?1
         ORDER BY lesson_id ASC",
    )?;
    let rows = stmt
        .query_map(params![canonical_id], |row| {
            Ok(FragmentEdge {
                lesson_id: row.get(0)?,
                fragment_canonical_id: row.get(1)?,
                edge_strength: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Return all fragments transitively reachable from lessons in the given
/// cluster (via `prediction_lessons.cluster_key` + `lesson_fragment_edges`).
pub fn fragments_for_cluster(conn: &Connection, cluster_key: &str) -> Result<Vec<ReasoningFragment>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT rf.canonical_id, rf.fragment, rf.fragment_type, rf.topic,
                rf.related_lessons, rf.cited_count, rf.confidence, rf.derivation,
                rf.operator_endorsed, rf.created_at
         FROM reasoning_fragments rf
         JOIN lesson_fragment_edges lfe
            ON lfe.fragment_canonical_id = rf.canonical_id
         JOIN prediction_lessons pl
            ON pl.id = lfe.lesson_id
         WHERE pl.cluster_key = ?1
         ORDER BY rf.cited_count DESC, rf.canonical_id ASC",
    )?;
    let rows = stmt
        .query_map(params![cluster_key], row_to_fragment)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn upsert_fragment(
    conn: &Connection,
    canonical_id: &str,
    fragment: &str,
    fragment_type: &str,
    topic: &str,
    confidence: &str,
    derivation: Option<&str>,
    operator_endorsed: bool,
) -> Result<()> {
    ensure_table(conn)?;
    if !VALID_FRAGMENT_TYPES.contains(&fragment_type) {
        return Err(anyhow!(
            "invalid fragment_type '{}'; must be one of {}",
            fragment_type,
            VALID_FRAGMENT_TYPES.join("|")
        ));
    }
    if !VALID_CONFIDENCE.contains(&confidence) {
        return Err(anyhow!(
            "invalid confidence '{}'; must be one of {}",
            confidence,
            VALID_CONFIDENCE.join("|")
        ));
    }
    conn.execute(
        "INSERT INTO reasoning_fragments
            (canonical_id, fragment, fragment_type, topic, confidence,
             derivation, operator_endorsed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(canonical_id) DO UPDATE SET
            fragment = excluded.fragment,
            fragment_type = excluded.fragment_type,
            topic = excluded.topic,
            confidence = excluded.confidence,
            derivation = excluded.derivation,
            operator_endorsed = excluded.operator_endorsed",
        params![
            canonical_id,
            fragment,
            fragment_type,
            topic,
            confidence,
            derivation,
            if operator_endorsed { 1 } else { 0 },
        ],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub fn upsert_edge(
    conn: &Connection,
    lesson_id: i64,
    fragment_canonical_id: &str,
    edge_strength: &str,
) -> Result<()> {
    ensure_table(conn)?;
    if !VALID_EDGE_STRENGTH.contains(&edge_strength) {
        return Err(anyhow!(
            "invalid edge_strength '{}'; must be one of {}",
            edge_strength,
            VALID_EDGE_STRENGTH.join("|")
        ));
    }
    conn.execute(
        "INSERT INTO lesson_fragment_edges
            (lesson_id, fragment_canonical_id, edge_strength)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(lesson_id, fragment_canonical_id) DO UPDATE SET
            edge_strength = excluded.edge_strength",
        params![lesson_id, fragment_canonical_id, edge_strength],
    )?;
    Ok(())
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
    fn upsert_then_get_roundtrips_fragment() {
        let conn = fresh_conn();
        upsert_fragment(
            &conn,
            "realrates-dominates-gold",
            "Real yields dominate gold direction over a 90d window",
            "correlation-rule",
            "gold",
            "high",
            Some("backfilled from 2024-25 series"),
            true,
        )
        .unwrap();
        let got = get(&conn, "realrates-dominates-gold").unwrap().unwrap();
        assert_eq!(got.fragment_type, "correlation-rule");
        assert_eq!(got.topic, "gold");
        assert!(got.operator_endorsed);
    }

    #[test]
    fn edges_roundtrip_per_fragment() {
        let conn = fresh_conn();
        upsert_fragment(
            &conn,
            "options-gamma-pinning",
            "Round-number strikes pin price intraday",
            "anti-pattern",
            "options",
            "medium",
            None,
            false,
        )
        .unwrap();
        upsert_edge(&conn, 11, "options-gamma-pinning", "primary").unwrap();
        upsert_edge(&conn, 12, "options-gamma-pinning", "secondary").unwrap();
        let edges = edges_for_fragment(&conn, "options-gamma-pinning").unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].lesson_id, 11);
        assert_eq!(edges[0].edge_strength, "primary");
    }

    #[test]
    fn list_filters_by_type_and_topic() {
        let conn = fresh_conn();
        upsert_fragment(&conn, "a", "frag a", "heuristic", "gold", "medium", None, false).unwrap();
        upsert_fragment(
            &conn,
            "b",
            "frag b",
            "anti-pattern",
            "gold",
            "medium",
            None,
            false,
        )
        .unwrap();
        upsert_fragment(
            &conn,
            "c",
            "frag c",
            "anti-pattern",
            "btc",
            "medium",
            None,
            false,
        )
        .unwrap();
        assert_eq!(list(&conn, Some("anti-pattern"), None).unwrap().len(), 2);
        assert_eq!(list(&conn, Some("anti-pattern"), Some("btc")).unwrap().len(), 1);
        assert_eq!(list(&conn, None, Some("gold")).unwrap().len(), 2);
    }

    #[test]
    fn upsert_rejects_invalid_inputs() {
        let conn = fresh_conn();
        assert!(upsert_fragment(&conn, "a", "x", "wat", "gold", "medium", None, false).is_err());
        assert!(upsert_fragment(&conn, "a", "x", "heuristic", "gold", "nope", None, false).is_err());
        upsert_fragment(&conn, "a", "x", "heuristic", "gold", "medium", None, false).unwrap();
        assert!(upsert_edge(&conn, 1, "a", "bad-strength").is_err());
    }
}
