use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchQuestion {
    pub id: i64,
    pub question: String,
    pub evidence_tilt: String,
    pub key_signal: Option<String>,
    pub evidence: Option<String>,
    pub first_raised: String,
    pub last_updated: String,
    pub status: String,
    pub resolution: Option<String>,
}

impl ResearchQuestion {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            question: row.get(1)?,
            evidence_tilt: row.get(2)?,
            key_signal: row.get(3)?,
            evidence: row.get(4)?,
            first_raised: row.get(5)?,
            last_updated: row.get(6)?,
            status: row.get(7)?,
            resolution: row.get(8)?,
        })
    }
}

pub fn add_question(conn: &Connection, question: &str, key_signal: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO research_questions (question, key_signal)
         VALUES (?, ?)",
        params![question, key_signal],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_questions(conn: &Connection, status_filter: Option<&str>) -> Result<Vec<ResearchQuestion>> {
    let query = if let Some(status) = status_filter {
        format!(
            "SELECT id, question, evidence_tilt, key_signal, evidence, first_raised, last_updated, status, resolution
             FROM research_questions
             WHERE status = '{}'
             ORDER BY last_updated DESC",
            status.replace('"', "''")
        )
    } else {
        "SELECT id, question, evidence_tilt, key_signal, evidence, first_raised, last_updated, status, resolution
         FROM research_questions
         ORDER BY last_updated DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], ResearchQuestion::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn update_question(
    conn: &Connection,
    id: i64,
    tilt: Option<&str>,
    evidence: Option<&str>,
    key_signal: Option<&str>,
) -> Result<()> {
    if tilt.is_none() && evidence.is_none() && key_signal.is_none() {
        return Ok(());
    }

    let mut update_parts = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(t) = tilt {
        update_parts.push("evidence_tilt = ?");
        params.push(Box::new(t.to_string()));
    }

    if let Some(ev) = evidence {
        update_parts.push(
            "evidence = CASE
                WHEN evidence IS NULL OR evidence = '' THEN ?
                ELSE evidence || char(10) || ?
             END",
        );
        params.push(Box::new(ev.to_string()));
        params.push(Box::new(ev.to_string()));
    }

    if let Some(sig) = key_signal {
        update_parts.push("key_signal = ?");
        params.push(Box::new(sig.to_string()));
    }

    update_parts.push("last_updated = datetime('now')");

    let sql = format!(
        "UPDATE research_questions SET {} WHERE id = ?",
        update_parts.join(", ")
    );
    params.push(Box::new(id));

    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

pub fn resolve_question(conn: &Connection, id: i64, resolution: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE research_questions
         SET status = ?, resolution = ?, last_updated = datetime('now')
         WHERE id = ?",
        params![status, resolution, id],
    )?;
    Ok(())
}
