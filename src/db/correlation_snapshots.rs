use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationSnapshot {
    pub id: i64,
    pub symbol_a: String,
    pub symbol_b: String,
    pub correlation: f64,
    pub period: String,
    pub recorded_at: String,
}

impl CorrelationSnapshot {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            symbol_a: row.get(1)?,
            symbol_b: row.get(2)?,
            correlation: row.get(3)?,
            period: row.get(4)?,
            recorded_at: row.get(5)?,
        })
    }
}

pub fn store_snapshot(
    conn: &Connection,
    symbol_a: &str,
    symbol_b: &str,
    correlation: f64,
    period: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO correlation_snapshots (symbol_a, symbol_b, correlation, period)
         VALUES (?, ?, ?, ?)",
        params![symbol_a, symbol_b, correlation, period],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_current(conn: &Connection, period: Option<&str>) -> Result<Vec<CorrelationSnapshot>> {
    let query = if let Some(p) = period {
        format!(
            "SELECT c.id, c.symbol_a, c.symbol_b, c.correlation, c.period, c.recorded_at
             FROM correlation_snapshots c
             INNER JOIN (
               SELECT symbol_a, symbol_b, period, MAX(recorded_at) AS max_ts
               FROM correlation_snapshots
               WHERE period = '{}'
               GROUP BY symbol_a, symbol_b, period
             ) latest ON c.symbol_a = latest.symbol_a
                       AND c.symbol_b = latest.symbol_b
                       AND c.period = latest.period
                       AND c.recorded_at = latest.max_ts
             ORDER BY ABS(c.correlation) DESC",
            p.replace('"', "''")
        )
    } else {
        "SELECT c.id, c.symbol_a, c.symbol_b, c.correlation, c.period, c.recorded_at
         FROM correlation_snapshots c
         INNER JOIN (
           SELECT symbol_a, symbol_b, period, MAX(recorded_at) AS max_ts
           FROM correlation_snapshots
           GROUP BY symbol_a, symbol_b, period
         ) latest ON c.symbol_a = latest.symbol_a
                   AND c.symbol_b = latest.symbol_b
                   AND c.period = latest.period
                   AND c.recorded_at = latest.max_ts
         ORDER BY ABS(c.correlation) DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], CorrelationSnapshot::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get_history(
    conn: &Connection,
    symbol_a: &str,
    symbol_b: &str,
    period: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<CorrelationSnapshot>> {
    let mut query = format!(
        "SELECT id, symbol_a, symbol_b, correlation, period, recorded_at
         FROM correlation_snapshots
         WHERE symbol_a = '{}' AND symbol_b = '{}'",
        symbol_a.replace('"', "''"),
        symbol_b.replace('"', "''")
    );

    if let Some(p) = period {
        query.push_str(&format!(" AND period = '{}'", p.replace('"', "''")));
    }

    query.push_str(" ORDER BY recorded_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], CorrelationSnapshot::from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
