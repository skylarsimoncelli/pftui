use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrediction {
    pub id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub conviction: String,
    pub timeframe: Option<String>,
    pub confidence: Option<f64>,
    pub source_agent: Option<String>,
    pub target_date: Option<String>,
    pub resolution_criteria: Option<String>,
    pub outcome: String,
    pub score_notes: Option<String>,
    pub lesson: Option<String>,
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
    pub by_timeframe: HashMap<String, ConvictionStats>,
    pub by_source_agent: HashMap<String, ConvictionStats>,
}

impl UserPrediction {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            claim: row.get(1)?,
            symbol: row.get(2)?,
            conviction: row.get(3)?,
            timeframe: row.get(4)?,
            confidence: row.get(5)?,
            source_agent: row.get(6)?,
            target_date: row.get(7)?,
            resolution_criteria: row.get(8)?,
            outcome: row.get(9)?,
            score_notes: row.get(10)?,
            lesson: row.get(11)?,
            created_at: row.get(12)?,
            scored_at: row.get(13)?,
        })
    }
}

fn ensure_prediction_columns(conn: &Connection) -> Result<()> {
    let required = [
        ("timeframe", "TEXT NOT NULL DEFAULT 'medium'"),
        ("confidence", "REAL"),
        ("source_agent", "TEXT"),
        ("lesson", "TEXT"),
        ("resolution_criteria", "TEXT"),
    ];
    for (col, ty) in required {
        let exists: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('user_predictions') WHERE name = ?1")?
            .query_row([col], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !exists {
            conn.execute(
                &format!("ALTER TABLE user_predictions ADD COLUMN {} {}", col, ty),
                [],
            )?;
        }
    }
    Ok(())
}

fn ensure_prediction_columns_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS user_predictions (
                id BIGSERIAL PRIMARY KEY,
                claim TEXT NOT NULL,
                symbol TEXT,
                conviction TEXT NOT NULL DEFAULT 'medium',
                timeframe TEXT NOT NULL DEFAULT 'medium',
                confidence DOUBLE PRECISION,
                source_agent TEXT,
                target_date TEXT,
                resolution_criteria TEXT,
                outcome TEXT NOT NULL DEFAULT 'pending',
                score_notes TEXT,
                lesson TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                scored_at TIMESTAMPTZ
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_user_predictions_outcome ON user_predictions(outcome)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_user_predictions_symbol ON user_predictions(symbol)",
        )
        .execute(pool)
        .await?;
        sqlx::query("ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS timeframe TEXT NOT NULL DEFAULT 'medium'")
            .execute(pool)
            .await?;
        sqlx::query(
            "ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS confidence DOUBLE PRECISION",
        )
        .execute(pool)
        .await?;
        sqlx::query("ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS source_agent TEXT")
            .execute(pool)
            .await?;
        sqlx::query(
            "ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS resolution_criteria TEXT",
        )
        .execute(pool)
        .await?;
        sqlx::query("ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS lesson TEXT")
            .execute(pool)
            .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn add_prediction(
    conn: &Connection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, confidence, source_agent, target_date, resolution_criteria)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            claim,
            symbol,
            conviction.unwrap_or("medium"),
            timeframe.unwrap_or("medium"),
            confidence,
            source_agent,
            target_date,
            resolution_criteria
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_predictions(
    conn: &Connection,
    outcome_filter: Option<&str>,
    symbol: Option<&str>,
    timeframe_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<UserPrediction>> {
    let mut query = String::from(
        "SELECT id, claim, symbol, conviction, timeframe, confidence, source_agent, target_date, resolution_criteria, outcome, score_notes, lesson, created_at, scored_at
         FROM user_predictions",
    );

    let mut where_parts = Vec::new();
    if let Some(filter) = outcome_filter {
        where_parts.push(format!("outcome = '{}'", filter.replace('"', "''")));
    }
    if let Some(sym) = symbol {
        where_parts.push(format!("symbol = '{}'", sym.replace('"', "''")));
    }
    if let Some(tf) = timeframe_filter {
        where_parts.push(format!("timeframe = '{}'", tf.replace('"', "''")));
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

pub fn score_prediction(
    conn: &Connection,
    id: i64,
    outcome: &str,
    notes: Option<&str>,
    lesson: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE user_predictions
         SET outcome = ?, score_notes = ?, lesson = ?, scored_at = datetime('now')
         WHERE id = ?",
        params![outcome, notes, lesson, id],
    )?;
    Ok(())
}

fn compute_stats(items: &[UserPrediction]) -> ConvictionStats {
    let mut s = ConvictionStats {
        total: items.len(),
        ..Default::default()
    };

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
        s.hit_rate_pct =
            ((s.correct as f64) + 0.5 * (s.partial as f64)) / (s.scored as f64) * 100.0;
    }

    s
}

#[allow(dead_code)]
pub fn get_stats(conn: &Connection) -> Result<PredictionStats> {
    let all = list_predictions(conn, None, None, None, None)?;
    let overall = compute_stats(&all);

    let mut by_conviction_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_symbol_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_timeframe_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_source_agent_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();

    for item in &all {
        by_conviction_map
            .entry(item.conviction.clone())
            .or_default()
            .push(item.clone());

        let sym = item.symbol.clone().unwrap_or_else(|| "unknown".to_string());
        by_symbol_map.entry(sym).or_default().push(item.clone());
        let timeframe = item
            .timeframe
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_timeframe_map
            .entry(timeframe)
            .or_default()
            .push(item.clone());
        let source = item
            .source_agent
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_source_agent_map
            .entry(source)
            .or_default()
            .push(item.clone());
    }

    let by_conviction = by_conviction_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();

    let by_symbol = by_symbol_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();
    let by_timeframe = by_timeframe_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();
    let by_source_agent = by_source_agent_map
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
        by_timeframe,
        by_source_agent,
    })
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn add_prediction_backend(
    backend: &BackendConnection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            ensure_prediction_columns(conn)?;
            add_prediction(
                conn,
                claim,
                symbol,
                conviction,
                timeframe,
                confidence,
                source_agent,
                target_date,
                resolution_criteria,
            )
        },
        |pool| {
            ensure_prediction_columns_postgres(pool)?;
            add_prediction_postgres(
                pool,
                claim,
                symbol,
                conviction,
                timeframe,
                confidence,
                source_agent,
                target_date,
                resolution_criteria,
            )
        },
    )
}

pub fn list_predictions_backend(
    backend: &BackendConnection,
    outcome_filter: Option<&str>,
    symbol: Option<&str>,
    timeframe_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<UserPrediction>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_prediction_columns(conn)?;
            list_predictions(conn, outcome_filter, symbol, timeframe_filter, limit)
        },
        |pool| {
            ensure_prediction_columns_postgres(pool)?;
            list_predictions_postgres(pool, outcome_filter, symbol, timeframe_filter, limit)
        },
    )
}

#[allow(dead_code)]
pub fn score_prediction_backend(
    backend: &BackendConnection,
    id: i64,
    outcome: &str,
    notes: Option<&str>,
    lesson: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| {
            ensure_prediction_columns(conn)?;
            score_prediction(conn, id, outcome, notes, lesson)
        },
        |pool| {
            ensure_prediction_columns_postgres(pool)?;
            score_prediction_postgres(pool, id, outcome, notes, lesson)
        },
    )
}

#[allow(dead_code)]
pub fn get_stats_filtered_backend(
    backend: &BackendConnection,
    timeframe_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<PredictionStats> {
    let mut all = list_predictions_backend(backend, None, None, timeframe_filter, None)?;
    if let Some(agent) = agent_filter {
        all.retain(|p| {
            p.source_agent
                .as_deref()
                .map(|a| a.eq_ignore_ascii_case(agent))
                .unwrap_or(false)
        });
    }
    let overall = compute_stats(&all);

    let mut by_conviction_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_symbol_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_timeframe_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_source_agent_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();

    for item in &all {
        by_conviction_map
            .entry(item.conviction.clone())
            .or_default()
            .push(item.clone());

        let sym = item.symbol.clone().unwrap_or_else(|| "unknown".to_string());
        by_symbol_map.entry(sym).or_default().push(item.clone());
        let timeframe = item
            .timeframe
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_timeframe_map
            .entry(timeframe)
            .or_default()
            .push(item.clone());
        let source = item
            .source_agent
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_source_agent_map
            .entry(source)
            .or_default()
            .push(item.clone());
    }

    let by_conviction = by_conviction_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();

    let by_symbol = by_symbol_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();
    let by_timeframe = by_timeframe_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();
    let by_source_agent = by_source_agent_map
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
        by_timeframe,
        by_source_agent,
    })
}

#[allow(dead_code)]
pub fn get_stats_backend(backend: &BackendConnection) -> Result<PredictionStats> {
    let all = list_predictions_backend(backend, None, None, None, None)?;
    let overall = compute_stats(&all);

    let mut by_conviction_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_symbol_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_timeframe_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();
    let mut by_source_agent_map: HashMap<String, Vec<UserPrediction>> = HashMap::new();

    for item in &all {
        by_conviction_map
            .entry(item.conviction.clone())
            .or_default()
            .push(item.clone());

        let sym = item.symbol.clone().unwrap_or_else(|| "unknown".to_string());
        by_symbol_map.entry(sym).or_default().push(item.clone());
        let timeframe = item
            .timeframe
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_timeframe_map
            .entry(timeframe)
            .or_default()
            .push(item.clone());
        let source = item
            .source_agent
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_source_agent_map
            .entry(source)
            .or_default()
            .push(item.clone());
    }

    let by_conviction = by_conviction_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();

    let by_symbol = by_symbol_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();
    let by_timeframe = by_timeframe_map
        .into_iter()
        .map(|(k, v)| (k, compute_stats(&v)))
        .collect();
    let by_source_agent = by_source_agent_map
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
        by_timeframe,
        by_source_agent,
    })
}

type PredictionRow = (
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<f64>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
);

fn from_pg_row(r: PredictionRow) -> UserPrediction {
    UserPrediction {
        id: r.0,
        claim: r.1,
        symbol: r.2,
        conviction: r.3,
        timeframe: r.4,
        confidence: r.5,
        source_agent: r.6,
        target_date: r.7,
        resolution_criteria: r.8,
        outcome: r.9,
        score_notes: r.10,
        lesson: r.11,
        created_at: r.12,
        scored_at: r.13,
    }
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
fn add_prediction_postgres(
    pool: &PgPool,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, confidence, source_agent, target_date, resolution_criteria)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
        )
        .bind(claim)
        .bind(symbol)
        .bind(conviction.unwrap_or("medium"))
        .bind(timeframe.unwrap_or("medium"))
        .bind(confidence)
        .bind(source_agent)
        .bind(target_date)
        .bind(resolution_criteria)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_predictions_postgres(
    pool: &PgPool,
    outcome_filter: Option<&str>,
    symbol: Option<&str>,
    timeframe_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<UserPrediction>> {
    let mut rows: Vec<PredictionRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, claim, symbol, conviction, timeframe, confidence, source_agent, target_date, resolution_criteria, outcome, score_notes, lesson, created_at::text, scored_at::text
             FROM user_predictions
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    if let Some(o) = outcome_filter {
        rows.retain(|r| r.9 == o);
    }
    if let Some(s) = symbol {
        rows.retain(|r| r.2.as_deref().is_some_and(|v| v == s));
    }
    if let Some(tf) = timeframe_filter {
        rows.retain(|r| r.4.as_deref().is_some_and(|v| v == tf));
    }
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Ok(rows.into_iter().map(from_pg_row).collect())
}

#[allow(dead_code)]
fn score_prediction_postgres(
    pool: &PgPool,
    id: i64,
    outcome: &str,
    notes: Option<&str>,
    lesson: Option<&str>,
) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE user_predictions
             SET outcome = $1, score_notes = $2, lesson = $3, scored_at = NOW()
             WHERE id = $4",
        )
        .bind(outcome)
        .bind(notes)
        .bind(lesson)
        .bind(id)
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}
