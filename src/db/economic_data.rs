use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct EconomicDataEntry {
    pub indicator: String,
    pub value: Decimal,
    pub previous: Option<Decimal>,
    pub change: Option<Decimal>,
    pub source_url: String,
    pub fetched_at: String,
}

pub fn upsert_entry(conn: &Connection, entry: &EconomicDataEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO economic_data (indicator, value, previous, change, source_url, fetched_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(indicator) DO UPDATE SET
            value = excluded.value,
            previous = excluded.previous,
            change = excluded.change,
            source_url = excluded.source_url,
            fetched_at = excluded.fetched_at",
        params![
            entry.indicator,
            entry.value.to_string(),
            entry.previous.map(|v| v.to_string()),
            entry.change.map(|v| v.to_string()),
            entry.source_url,
            entry.fetched_at
        ],
    )?;
    Ok(())
}

pub fn get_all(conn: &Connection) -> Result<Vec<EconomicDataEntry>> {
    let mut stmt = conn.prepare(
        "SELECT indicator, value, previous, change, source_url, fetched_at
         FROM economic_data
         ORDER BY indicator",
    )?;
    let rows = stmt.query_map([], |row| {
        let value: String = row.get(1)?;
        let previous: Option<String> = row.get(2)?;
        let change: Option<String> = row.get(3)?;
        Ok(EconomicDataEntry {
            indicator: row.get(0)?,
            value: value.parse().unwrap_or(Decimal::ZERO),
            previous: previous.and_then(|v| v.parse().ok()),
            change: change.and_then(|v| v.parse().ok()),
            source_url: row.get(4)?,
            fetched_at: row.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

