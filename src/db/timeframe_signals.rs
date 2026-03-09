use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeframeSignal {
    pub id: i64,
    pub signal_type: String,
    pub layers: String,
    pub assets: String,
    pub description: String,
    pub severity: String,
    pub detected_at: String,
}

impl TimeframeSignal {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            signal_type: row.get(1)?,
            layers: row.get(2)?,
            assets: row.get(3)?,
            description: row.get(4)?,
            severity: row.get(5)?,
            detected_at: row.get(6)?,
        })
    }
}

pub fn add_signal(
    conn: &Connection,
    signal_type: &str,
    layers_json: &str,
    assets_json: &str,
    description: &str,
    severity: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO timeframe_signals (signal_type, layers, assets, description, severity)
         VALUES (?, ?, ?, ?, ?)",
        params![signal_type, layers_json, assets_json, description, severity],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_signals(
    conn: &Connection,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<TimeframeSignal>> {
    let mut query = String::from(
        "SELECT id, signal_type, layers, assets, description, severity, detected_at
         FROM timeframe_signals",
    );

    let mut clauses = Vec::new();
    if let Some(t) = signal_type {
        clauses.push(format!("signal_type = '{}'", t.replace('"', "''")));
    }
    if let Some(s) = severity {
        clauses.push(format!("severity = '{}'", s.replace('"', "''")));
    }
    if !clauses.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&clauses.join(" AND "));
    }

    query.push_str(" ORDER BY detected_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], TimeframeSignal::from_row)?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn latest_signal(conn: &Connection) -> Result<Option<TimeframeSignal>> {
    let mut stmt = conn.prepare(
        "SELECT id, signal_type, layers, assets, description, severity, detected_at
         FROM timeframe_signals
         ORDER BY detected_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], TimeframeSignal::from_row)?;
    Ok(rows.next().transpose()?)
}
