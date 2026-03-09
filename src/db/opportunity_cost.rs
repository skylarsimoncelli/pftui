use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunityCostEntry {
    pub id: i64,
    pub date: String,
    pub event: String,
    pub asset: Option<String>,
    pub missed_gain_pct: Option<f64>,
    pub missed_gain_usd: Option<f64>,
    pub avoided_loss_pct: Option<f64>,
    pub avoided_loss_usd: Option<f64>,
    pub was_rational: i64,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpCostStats {
    pub total_entries: usize,
    pub total_missed_usd: f64,
    pub total_avoided_usd: f64,
    pub net_usd: f64,
    pub rational_misses: usize,
    pub mistakes: usize,
}

impl OpportunityCostEntry {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            date: row.get(1)?,
            event: row.get(2)?,
            asset: row.get(3)?,
            missed_gain_pct: row.get(4)?,
            missed_gain_usd: row.get(5)?,
            avoided_loss_pct: row.get(6)?,
            avoided_loss_usd: row.get(7)?,
            was_rational: row.get(8)?,
            notes: row.get(9)?,
            created_at: row.get(10)?,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn add_entry(
    conn: &Connection,
    date: &str,
    event: &str,
    asset: Option<&str>,
    missed_gain_pct: Option<f64>,
    missed_gain_usd: Option<f64>,
    avoided_loss_pct: Option<f64>,
    avoided_loss_usd: Option<f64>,
    was_rational: bool,
    notes: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO opportunity_cost
         (date, event, asset, missed_gain_pct, missed_gain_usd, avoided_loss_pct, avoided_loss_usd, was_rational, notes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            date,
            event,
            asset,
            missed_gain_pct,
            missed_gain_usd,
            avoided_loss_pct,
            avoided_loss_usd,
            if was_rational { 1 } else { 0 },
            notes
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_entries(
    conn: &Connection,
    since: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<OpportunityCostEntry>> {
    let mut query = String::from(
        "SELECT id, date, event, asset, missed_gain_pct, missed_gain_usd, avoided_loss_pct, avoided_loss_usd, was_rational, notes, created_at
         FROM opportunity_cost",
    );

    let mut where_parts = Vec::new();
    if let Some(s) = since {
        where_parts.push(format!("date >= '{}'", s.replace('"', "''")));
    }
    if let Some(a) = asset {
        where_parts.push(format!("asset = '{}'", a.replace('"', "''")));
    }

    if !where_parts.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&where_parts.join(" AND "));
    }

    query.push_str(" ORDER BY date DESC, created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], OpportunityCostEntry::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn get_stats(conn: &Connection, since: Option<&str>) -> Result<OpCostStats> {
    let rows = list_entries(conn, since, None, None)?;

    let total_entries = rows.len();
    let total_missed_usd: f64 = rows.iter().map(|r| r.missed_gain_usd.unwrap_or(0.0)).sum();
    let total_avoided_usd: f64 = rows.iter().map(|r| r.avoided_loss_usd.unwrap_or(0.0)).sum();
    let rational_misses = rows.iter().filter(|r| r.was_rational == 1).count();
    let mistakes = rows.iter().filter(|r| r.was_rational == 0).count();

    Ok(OpCostStats {
        total_entries,
        total_missed_usd,
        total_avoided_usd,
        net_usd: total_avoided_usd - total_missed_usd,
        rational_misses,
        mistakes,
    })
}
