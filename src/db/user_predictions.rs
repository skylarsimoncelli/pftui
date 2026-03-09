use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrediction {
    pub id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub conviction: String,
    pub target_date: Option<String>,
    pub outcome: String,
    pub score_notes: Option<String>,
    pub created_at: String,
    pub scored_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConvictionStats {
    pub total: usize,
    pub scored: usize,
    pub pending: usize,
    pub correct: usize,
    pub partial: usize,
    pub wrong: usize,
    pub hit_rate_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionStats {
    pub total: usize,
    pub scored: usize,
    pub pending: usize,
    pub correct: usize,
    pub partial: usize,
    pub wrong: usize,
    pub hit_rate_pct: f64,
    pub by_conviction: HashMap<String, ConvictionStats>,
    pub by_symbol: HashMap<String, ConvictionStats>,
}

impl UserPrediction {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            claim: row.get(1)?,
            symbol: row.get(2)?,
            conviction: row.get(3)?,
            target_date: row.get(4)?,
            outcome: row.get(5)?,
            score_notes: row.get(6)?,
            created_at: row.get(7)?,
            scored_at: row.get(8)?,
        })
    }
}

pub fn add_prediction(
    conn: &Connection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    target_date: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO user_predictions (claim, symbol, conviction, target_date)
         VALUES (?, ?, ?, ?)",
        params![claim, symbol, conviction.unwrap_or("medium"), target_date],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_predictions(
    conn: &Connection,
    outcome_filter: Option<&str>,
    symbol: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<UserPrediction>> {
    let mut query = String::from(
        "SELECT id, claim, symbol, conviction, target_date, outcome, score_notes, created_at, scored_at
         FROM user_predictions",
    );

    let mut where_parts = Vec::new();
    if let Some(filter) = outcome_filter {
        where_parts.push(format!("outcome = '{}'", filter.replace('"', "''")));
    }
    if let Some(sym) = symbol {
        where_parts.push(format!("symbol = '{}'", sym.replace('"', "''")));
    }
    if !where_parts.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&where_parts.join(" AND "));
    }

    query.push_str(" ORDER BY created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], UserPrediction::from_row)?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

pub fn score_prediction(conn: &Connection, id: i64, outcome: &str, notes: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE user_predictions
         SET outcome = ?, score_notes = ?, scored_at = datetime('now')
         WHERE id = ?",
        params![outcome, notes, id],
    )?;
    Ok(())
}

fn compute_stats(items: &[UserPrediction]) -> ConvictionStats {
    let mut s = ConvictionStats::default();
    s.total = items.len();

    for item in items {
        match item.outcome.as_str() {
            "pending" => s.pending += 1,
            "correct" => {
                s.correct += 1;
                s.scored += 1;
            }
            "partial" => {
                s.partial += 1;
                s.scored += 1;
            }
            "wrong" => {
                s.wrong += 1;
                s.scored += 1;
            }
            _ => {}
        }
    }

    if s.scored > 0 {
        s.hit_rate_pct = ((s.correct as f64) + 0.5 * (s.partial as f64)) / (s.scored as f64) * 100.0;
    }

    s
}

pub fn get_stats(conn: &Connection) -> Result<PredictionStats> {
    let all = list_predictions(conn, None, None, None)?;
    let overall = compute_stats(&all);

    let mut by_conviction_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_symbol_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();

    for item in &all {
        by_conviction_map
            .entry(item.conviction.clone())
            .or_default()
            .push(item.clone());

        let sym = item.symbol.clone().unwrap_or_else(|| "unknown".to_string());
        by_symbol_map.entry(sym).or_default().push(item.clone());
    }

    let by_conviction = by_conviction_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();

    let by_symbol = by_symbol_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();

    Ok(PredictionStats {
        total: overall.total,
        scored: overall.scored,
        pending: overall.pending,
        correct: overall.correct,
        partial: overall.partial,
        wrong: overall.wrong,
        hit_rate_pct: overall.hit_rate_pct,
        by_conviction,
        by_symbol,
    })
}
