use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::agent_messages;
use crate::db::backend::BackendConnection;
use crate::db::news_source_accuracy;
use crate::db::query;

const LOW_ANALYST_AGENT_ALIASES: &[&str] = &[
    "analyst-low",
    "low-agent",
    "low-analyst",
    "low-timeframe",
    "low-timeframe-analyst",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrediction {
    pub id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub conviction: String,
    pub timeframe: Option<String>,
    pub topic: String,
    pub confidence: Option<f64>,
    pub source_agent: Option<String>,
    pub source_article_id: Option<i64>,
    pub target_date: Option<String>,
    pub resolution_criteria: Option<String>,
    pub outcome: String,
    pub score_notes: Option<String>,
    pub lesson: Option<String>,
    pub lessons_applied: Vec<i64>,
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
            topic: row.get(5)?,
            confidence: row.get(6)?,
            source_agent: row.get(7)?,
            source_article_id: row.get(8)?,
            target_date: row.get(9)?,
            resolution_criteria: row.get(10)?,
            outcome: row.get(11)?,
            score_notes: row.get(12)?,
            lesson: row.get(13)?,
            lessons_applied: parse_lessons_applied(row.get(14)?),
            created_at: row.get(15)?,
            scored_at: row.get(16)?,
        })
    }
}

fn parse_lessons_applied(raw: Option<String>) -> Vec<i64> {
    raw.and_then(|value| serde_json::from_str::<Vec<i64>>(&value).ok())
        .unwrap_or_default()
}

/// Marker used on every `timeframe='macro-checkpoint'` prediction.
///
/// Macro-checkpoint predictions are short-horizon (≈90-day) falsifiable
/// sub-claims attached to a multi-year MACRO thesis (Stage 6, Fourth Turning,
/// de-dollarisation, Dalio composite, structural inflation). They must carry a
/// `[thesis=<slug>]` tag inside the `claim` so the scoring layer can group
/// failed checkpoints back to the parent thesis and surface a re-evaluation
/// signal to synthesis.
pub const MACRO_CHECKPOINT_TIMEFRAME: &str = "macro-checkpoint";

/// Extract the parent-thesis slug from a macro-checkpoint claim.
///
/// Convention: the analyst writes `[thesis=<slug>] <rest of claim>`.
/// `<slug>` is a short kebab-case identifier (e.g. `stage-6`, `fourth-turning`,
/// `de-dollarisation`, `dalio-composite`, `structural-inflation`).
///
/// Falls back to scanning `resolution_criteria` if the tag is not present in
/// the claim. Returns `None` when no tag is found — callers should still score
/// the prediction but cannot emit a thesis re-eval signal without it.
pub fn parse_thesis_tag(claim: &str, resolution_criteria: Option<&str>) -> Option<String> {
    if let Some(slug) = extract_thesis_tag(claim) {
        return Some(slug);
    }
    resolution_criteria.and_then(extract_thesis_tag)
}

fn extract_thesis_tag(text: &str) -> Option<String> {
    let needle = "thesis=";
    let start = text.find(needle)? + needle.len();
    let tail = &text[start..];
    let end = tail.find([']', ' ', '\t', '\n']).unwrap_or(tail.len());
    let slug = tail[..end].trim().trim_matches(|c: char| c == '"' || c == '\'');
    if slug.is_empty() {
        None
    } else {
        Some(slug.to_string())
    }
}

/// When a macro-checkpoint is scored Wrong, count how many checkpoints belong
/// to the parent thesis and how many of those are already Wrong, then emit one
/// agent message to synthesis (`analyst-evening`) flagging the parent thesis
/// for re-examination on the next macro run.
fn emit_macro_checkpoint_reeval_message(
    conn: &Connection,
    failed_id: i64,
    thesis_slug: &str,
) -> Result<()> {
    let (total, wrong): (i64, i64) = conn.query_row(
        "SELECT
            COUNT(*) AS total,
            SUM(CASE WHEN outcome = 'wrong' THEN 1 ELSE 0 END) AS wrong_count
         FROM user_predictions
         WHERE timeframe = ?1
           AND (claim LIKE ?2 OR COALESCE(resolution_criteria, '') LIKE ?2)",
        params![
            MACRO_CHECKPOINT_TIMEFRAME,
            format!("%thesis={}%", thesis_slug),
        ],
        |row| Ok((row.get(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0))),
    )?;
    let content = format!(
        "Macro thesis '{thesis}' has {wrong} of {total} checkpoint(s) failed (latest failure: prediction #{id}); analyst-macro should re-examine before next run.",
        thesis = thesis_slug,
        wrong = wrong,
        total = total,
        id = failed_id,
    );
    agent_messages::send_message(
        conn,
        "analyst-macro",
        Some("analyst-evening"),
        Some("high"),
        &content,
        Some("macro-checkpoint-reeval"),
        Some("macro"),
        None,
        None,
    )?;
    Ok(())
}

fn lessons_applied_json(lesson_ids: &[i64]) -> String {
    serde_json::to_string(lesson_ids).unwrap_or_else(|_| "[]".to_string())
}

fn ensure_prediction_columns(conn: &Connection) -> Result<()> {
    let required = [
        ("timeframe", "TEXT NOT NULL DEFAULT 'medium'"),
        (
            "topic",
            "TEXT NOT NULL DEFAULT 'other' CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other'))",
        ),
        ("confidence", "REAL"),
        ("source_agent", "TEXT"),
        ("source_article_id", "INTEGER"),
        ("lesson", "TEXT"),
        ("resolution_criteria", "TEXT"),
        ("lessons_applied", "TEXT NOT NULL DEFAULT '[]'"),
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
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_user_predictions_topic ON user_predictions(topic);
         CREATE INDEX IF NOT EXISTS idx_user_predictions_source_article
            ON user_predictions(source_article_id);",
    )?;
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
                topic TEXT NOT NULL DEFAULT 'other'
                    CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other')),
                confidence DOUBLE PRECISION,
                source_agent TEXT,
                source_article_id BIGINT,
                target_date TEXT,
                resolution_criteria TEXT,
                outcome TEXT NOT NULL DEFAULT 'pending',
                score_notes TEXT,
                lesson TEXT,
                lessons_applied TEXT NOT NULL DEFAULT '[]',
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
        sqlx::query("ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS topic TEXT NOT NULL DEFAULT 'other' CHECK(topic IN ('fed','inflation','geopolitics','commodities','crypto','equities','other'))")
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
            "ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS source_article_id BIGINT",
        )
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
        sqlx::query(
            "ALTER TABLE user_predictions ADD COLUMN IF NOT EXISTS lessons_applied TEXT NOT NULL DEFAULT '[]'",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_user_predictions_topic ON user_predictions(topic)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_user_predictions_source_article
             ON user_predictions(source_article_id)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

pub fn is_low_analyst_agent(agent: &str) -> bool {
    let normalized = agent.trim().to_ascii_lowercase();
    LOW_ANALYST_AGENT_ALIASES
        .iter()
        .any(|alias| normalized == *alias)
}

#[allow(clippy::too_many_arguments)]
/// Normalize a symbol string: treat empty, "null", "NULL", "none", "NONE", "MACRO"
/// as None (NULL in DB) so macro predictions without an asset symbol are first-class.
fn normalize_symbol(s: Option<&str>) -> Option<&str> {
    match s {
        None => None,
        Some(v)
            if matches!(
                v.trim(),
                "" | "null" | "NULL" | "none" | "NONE" | "MACRO" | "NFP" | "CPI" | "PMI" | "GDP"
            ) =>
        {
            None
        }
        Some(v) => Some(v.trim()),
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
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
    add_prediction_with_lessons(
        conn,
        claim,
        symbol,
        conviction,
        timeframe,
        confidence,
        source_agent,
        target_date,
        resolution_criteria,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_prediction_with_lessons(
    conn: &Connection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
    lessons_applied: &[i64],
) -> Result<i64> {
    add_prediction_with_details(
        conn,
        claim,
        symbol,
        conviction,
        timeframe,
        confidence,
        source_agent,
        target_date,
        resolution_criteria,
        lessons_applied,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_prediction_with_details(
    conn: &Connection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
    lessons_applied: &[i64],
    topic: Option<&str>,
    source_article_id: Option<i64>,
) -> Result<i64> {
    ensure_prediction_columns(conn)?;
    let symbol = normalize_symbol(symbol);
    let lessons_applied = lessons_applied_json(lessons_applied);
    let topic = news_source_accuracy::normalize_topic(topic)?;
    if let Some(article_id) = source_article_id {
        if article_id <= 0 {
            anyhow::bail!("source_article_id must be positive");
        }
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM news_cache WHERE id = ?1",
                params![article_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !exists {
            anyhow::bail!(
                "source_article_id {} does not exist in news_cache",
                article_id
            );
        }
    }
    conn.execute(
        "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, topic, confidence, source_agent, source_article_id, target_date, resolution_criteria, lessons_applied)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            claim,
            symbol,
            conviction.unwrap_or("medium"),
            timeframe.unwrap_or("medium"),
            topic,
            confidence,
            source_agent,
            source_article_id,
            target_date,
            resolution_criteria,
            lessons_applied
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
        "SELECT id, claim, symbol, conviction, timeframe, topic, confidence, source_agent, source_article_id, target_date, resolution_criteria, outcome, score_notes, lesson, lessons_applied, created_at, scored_at
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

pub fn count_recent_low_analyst_predictions(conn: &Connection) -> Result<usize> {
    ensure_prediction_columns(conn)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM user_predictions
         WHERE lower(COALESCE(timeframe, '')) = 'low'
           AND lower(COALESCE(source_agent, '')) IN (
                'analyst-low',
                'low-agent',
                'low-analyst',
                'low-timeframe',
                'low-timeframe-analyst'
           )
           AND datetime(created_at) >= datetime('now', '-1 hour')",
        [],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

fn get_prediction(conn: &Connection, id: i64) -> Result<Option<UserPrediction>> {
    let mut stmt = conn.prepare(
        "SELECT id, claim, symbol, conviction, timeframe, topic, confidence, source_agent, source_article_id, target_date, resolution_criteria, outcome, score_notes, lesson, lessons_applied, created_at, scored_at
         FROM user_predictions
         WHERE id = ?1",
    )?;
    let row = stmt
        .query_row(params![id], UserPrediction::from_row)
        .optional()?;
    Ok(row)
}

pub fn score_prediction(
    conn: &Connection,
    id: i64,
    outcome: &str,
    notes: Option<&str>,
    lesson: Option<&str>,
) -> Result<()> {
    ensure_prediction_columns(conn)?;
    let existing = get_prediction(conn, id)?;
    let updated = conn.execute(
        "UPDATE user_predictions
         SET outcome = ?, score_notes = ?, lesson = ?, scored_at = datetime('now')
         WHERE id = ?",
        params![outcome, notes, lesson, id],
    )?;
    if updated > 0 {
        if let Some(prediction) = existing {
            news_source_accuracy::sync_prediction_outcome(
                conn,
                id,
                prediction.source_article_id,
                &prediction.topic,
                outcome,
            )?;
            // Macro-checkpoint failure surfaces a re-evaluation message to
            // synthesis so the next macro run is forced to re-examine the
            // parent thesis. Only fires when a `macro-checkpoint` prediction
            // is scored Wrong AND the claim carries a `[thesis=<slug>]` tag.
            if outcome == "wrong"
                && prediction.timeframe.as_deref() == Some(MACRO_CHECKPOINT_TIMEFRAME)
            {
                if let Some(slug) = parse_thesis_tag(
                    &prediction.claim,
                    prediction.resolution_criteria.as_deref(),
                ) {
                    emit_macro_checkpoint_reeval_message(conn, id, &slug)?;
                }
            }
        }
    }
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
    lessons_applied: &[i64],
) -> Result<i64> {
    add_prediction_backend_with_details(
        backend,
        claim,
        symbol,
        conviction,
        timeframe,
        confidence,
        source_agent,
        target_date,
        resolution_criteria,
        lessons_applied,
        None,
        None,
    )
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn add_prediction_backend_with_details(
    backend: &BackendConnection,
    claim: &str,
    symbol: Option<&str>,
    conviction: Option<&str>,
    timeframe: Option<&str>,
    confidence: Option<f64>,
    source_agent: Option<&str>,
    target_date: Option<&str>,
    resolution_criteria: Option<&str>,
    lessons_applied: &[i64],
    topic: Option<&str>,
    source_article_id: Option<i64>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            ensure_prediction_columns(conn)?;
            add_prediction_with_details(
                conn,
                claim,
                symbol,
                conviction,
                timeframe,
                confidence,
                source_agent,
                target_date,
                resolution_criteria,
                lessons_applied,
                topic,
                source_article_id,
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
                lessons_applied,
                topic,
                source_article_id,
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

pub fn count_recent_low_analyst_predictions_backend(backend: &BackendConnection) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| {
            ensure_prediction_columns(conn)?;
            count_recent_low_analyst_predictions(conn)
        },
        |pool| {
            ensure_prediction_columns_postgres(pool)?;
            count_recent_low_analyst_predictions_postgres(pool)
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

#[derive(sqlx::FromRow)]
struct PredictionRow {
    id: i64,
    claim: String,
    symbol: Option<String>,
    conviction: String,
    timeframe: Option<String>,
    topic: String,
    confidence: Option<f64>,
    source_agent: Option<String>,
    source_article_id: Option<i64>,
    target_date: Option<String>,
    resolution_criteria: Option<String>,
    outcome: String,
    score_notes: Option<String>,
    lesson: Option<String>,
    lessons_applied: Option<String>,
    created_at: String,
    scored_at: Option<String>,
}

fn from_pg_row(r: PredictionRow) -> UserPrediction {
    UserPrediction {
        id: r.id,
        claim: r.claim,
        symbol: r.symbol,
        conviction: r.conviction,
        timeframe: r.timeframe,
        topic: r.topic,
        confidence: r.confidence,
        source_agent: r.source_agent,
        source_article_id: r.source_article_id,
        target_date: r.target_date,
        resolution_criteria: r.resolution_criteria,
        outcome: r.outcome,
        score_notes: r.score_notes,
        lesson: r.lesson,
        lessons_applied: parse_lessons_applied(r.lessons_applied),
        created_at: r.created_at,
        scored_at: r.scored_at,
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
    lessons_applied: &[i64],
    topic: Option<&str>,
    source_article_id: Option<i64>,
) -> Result<i64> {
    let symbol = normalize_symbol(symbol);
    let lessons_applied = lessons_applied_json(lessons_applied);
    let topic = news_source_accuracy::normalize_topic(topic)?;
    if let Some(article_id) = source_article_id {
        if article_id <= 0 {
            anyhow::bail!("source_article_id must be positive");
        }
    }
    let id: i64 = crate::db::pg_runtime::block_on(async {
        if let Some(article_id) = source_article_id {
            let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM news_cache WHERE id = $1")
                .bind(article_id)
                .fetch_optional(pool)
                .await?;
            if exists.is_none() {
                anyhow::bail!(
                    "source_article_id {} does not exist in news_cache",
                    article_id
                );
            }
        }
        let id = sqlx::query_scalar(
            "INSERT INTO user_predictions (claim, symbol, conviction, timeframe, topic, confidence, source_agent, source_article_id, target_date, resolution_criteria, lessons_applied)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             RETURNING id",
        )
        .bind(claim)
        .bind(symbol)
        .bind(conviction.unwrap_or("medium"))
        .bind(timeframe.unwrap_or("medium"))
        .bind(topic)
        .bind(confidence)
        .bind(source_agent)
        .bind(source_article_id)
        .bind(target_date)
        .bind(resolution_criteria)
        .bind(lessons_applied)
        .fetch_one(pool)
        .await?;
        Ok::<i64, anyhow::Error>(id)
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
            "SELECT id, claim, symbol, conviction, timeframe, topic, confidence, source_agent, source_article_id, target_date, resolution_criteria, outcome, score_notes, lesson, lessons_applied, created_at::text AS created_at, scored_at::text AS scored_at
             FROM user_predictions
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    if let Some(o) = outcome_filter {
        rows.retain(|r| r.outcome == o);
    }
    if let Some(s) = symbol {
        rows.retain(|r| r.symbol.as_deref().is_some_and(|v| v == s));
    }
    if let Some(tf) = timeframe_filter {
        rows.retain(|r| r.timeframe.as_deref().is_some_and(|v| v == tf));
    }
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Ok(rows.into_iter().map(from_pg_row).collect())
}

fn count_recent_low_analyst_predictions_postgres(pool: &PgPool) -> Result<usize> {
    let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT
             FROM user_predictions
             WHERE lower(COALESCE(timeframe, '')) = 'low'
               AND lower(COALESCE(source_agent, '')) IN (
                    'analyst-low',
                    'low-agent',
                    'low-analyst',
                    'low-timeframe',
                    'low-timeframe-analyst'
               )
               AND created_at >= NOW() - INTERVAL '1 hour'",
        )
        .fetch_one(pool)
        .await
    })?;
    Ok(count as usize)
}

#[allow(dead_code)]
fn score_prediction_postgres(
    pool: &PgPool,
    id: i64,
    outcome: &str,
    notes: Option<&str>,
    lesson: Option<&str>,
) -> Result<()> {
    let existing = crate::db::pg_runtime::block_on(async {
        let existing: Option<(Option<i64>, String)> = sqlx::query_as(
            "SELECT source_article_id, topic
             FROM user_predictions
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
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
        Ok::<_, anyhow::Error>(existing)
    })?;
    if let Some((source_article_id, topic)) = existing {
        news_source_accuracy::sync_prediction_outcome_postgres(
            pool,
            id,
            source_article_id,
            &topic,
            outcome,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn lessons_applied_round_trips_as_json_ids() {
        let conn = db::open_in_memory();

        let id = add_prediction_with_lessons(
            &conn,
            "BTC holds support because Lesson 218 applies",
            Some("BTC-USD"),
            Some("high"),
            Some("low"),
            Some(0.72),
            Some("low-agent"),
            Some("2026-05-29"),
            Some("Daily close above support"),
            &[218, 240],
        )
        .unwrap();

        let rows = list_predictions(&conn, None, None, None, None).unwrap();
        let row = rows.into_iter().find(|row| row.id == id).unwrap();
        assert_eq!(row.lessons_applied, vec![218, 240]);
    }

    #[test]
    fn scoring_source_attributed_prediction_updates_news_source_accuracy() {
        let conn = db::open_in_memory();
        crate::db::news_cache::insert_news(
            &conn,
            "Fed cut odds rise",
            "https://www.bloomberg.com/news/fed-cut-odds",
            "Bloomberg",
            "macro",
            1_709_610_000,
        )
        .unwrap();
        let article_id: i64 = conn
            .query_row(
                "SELECT id FROM news_cache WHERE url = ?1",
                params!["https://www.bloomberg.com/news/fed-cut-odds"],
                |row| row.get(0),
            )
            .unwrap();

        let id = add_prediction_with_details(
            &conn,
            "Fed cut odds will continue rising",
            None,
            Some("medium"),
            Some("medium"),
            Some(0.65),
            Some("medium-agent"),
            Some("2026-06-30"),
            Some("Fed funds futures price higher odds"),
            &[],
            Some("fed"),
            Some(article_id),
        )
        .unwrap();

        score_prediction(&conn, id, "correct", Some("Resolved higher"), None).unwrap();

        let rows = crate::db::news_source_accuracy::list_accuracy(
            &conn,
            Some("bloomberg.com"),
            Some("fed"),
            None,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].n_predictions_implied, 1);
        assert_eq!(rows[0].n_correct, 1);
        assert_eq!(rows[0].weight, 1.0);
    }

    #[test]
    fn legacy_predictions_default_to_no_applied_lessons() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_predictions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                claim TEXT NOT NULL,
                symbol TEXT,
                conviction TEXT NOT NULL DEFAULT 'medium',
                timeframe TEXT NOT NULL DEFAULT 'medium',
                confidence REAL,
                source_agent TEXT,
                target_date TEXT,
                resolution_criteria TEXT,
                outcome TEXT NOT NULL DEFAULT 'pending',
                score_notes TEXT,
                lesson TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                scored_at TEXT
            )",
        )
        .unwrap();

        ensure_prediction_columns(&conn).unwrap();
        add_prediction(
            &conn,
            "Legacy-compatible call",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let rows = list_predictions(&conn, None, None, None, None).unwrap();
        assert_eq!(rows[0].lessons_applied, Vec::<i64>::new());
    }

    #[test]
    fn parse_thesis_tag_finds_slug_in_claim() {
        assert_eq!(
            parse_thesis_tag(
                "[thesis=stage-6] By 2026-09-28, IF DXY > 95 then thesis degraded",
                None,
            ),
            Some("stage-6".to_string())
        );
        assert_eq!(
            parse_thesis_tag(
                "By 2026-09-28 CB gold purchases below 800t [thesis=de-dollarisation]",
                None,
            ),
            Some("de-dollarisation".to_string())
        );
    }

    #[test]
    fn parse_thesis_tag_falls_back_to_resolution_criteria() {
        assert_eq!(
            parse_thesis_tag(
                "Plain checkpoint claim with no inline tag",
                Some("Resolves when thesis=fourth-turning indicator triggers"),
            ),
            Some("fourth-turning".to_string())
        );
    }

    #[test]
    fn parse_thesis_tag_returns_none_when_absent() {
        assert_eq!(
            parse_thesis_tag("Plain claim with no tag", Some("Plain resolution")),
            None
        );
    }

    #[test]
    fn macro_checkpoint_creation_persists_with_new_timeframe() {
        let conn = db::open_in_memory();
        let id = add_prediction(
            &conn,
            "[thesis=stage-6] By 2026-09-28, IF DXY > 95 my stage-6 thesis is degraded",
            None,
            Some("medium"),
            Some(MACRO_CHECKPOINT_TIMEFRAME),
            Some(0.6),
            Some("analyst-macro"),
            Some("2026-09-28"),
            Some("DXY closes > 95 on target_date"),
        )
        .unwrap();

        let row = list_predictions(&conn, None, None, Some(MACRO_CHECKPOINT_TIMEFRAME), None)
            .unwrap()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap();
        assert_eq!(row.timeframe.as_deref(), Some(MACRO_CHECKPOINT_TIMEFRAME));
        assert_eq!(row.source_agent.as_deref(), Some("analyst-macro"));
        assert_eq!(row.target_date.as_deref(), Some("2026-09-28"));
    }

    #[test]
    fn scoring_macro_checkpoint_wrong_emits_synthesis_reeval_message() {
        let conn = db::open_in_memory();
        // Three checkpoints under the same parent thesis.
        let id1 = add_prediction(
            &conn,
            "[thesis=stage-6] DXY breaks 95 by 2026-09-28",
            None,
            Some("medium"),
            Some(MACRO_CHECKPOINT_TIMEFRAME),
            None,
            Some("analyst-macro"),
            Some("2026-09-28"),
            None,
        )
        .unwrap();
        add_prediction(
            &conn,
            "[thesis=stage-6] CB gold purchases stay > 800t/yr",
            None,
            Some("medium"),
            Some(MACRO_CHECKPOINT_TIMEFRAME),
            None,
            Some("analyst-macro"),
            Some("2026-09-28"),
            None,
        )
        .unwrap();
        add_prediction(
            &conn,
            "[thesis=stage-6] 10y real yields stay below 2%",
            None,
            Some("medium"),
            Some(MACRO_CHECKPOINT_TIMEFRAME),
            None,
            Some("analyst-macro"),
            Some("2026-09-28"),
            None,
        )
        .unwrap();

        // Pre-condition: no prior synthesis messages.
        let before =
            agent_messages::list_messages(&conn, Some("analyst-macro"), None, None, false, None, None, None)
                .unwrap();
        assert!(before.is_empty());

        // Score the first checkpoint Wrong → must surface a re-eval message.
        score_prediction(&conn, id1, "wrong", Some("DXY held above 95"), None).unwrap();

        let after =
            agent_messages::list_messages(&conn, Some("analyst-macro"), None, None, false, None, None, None)
                .unwrap();
        assert_eq!(after.len(), 1, "expected exactly one re-eval message");
        let msg = &after[0];
        assert_eq!(msg.to_agent.as_deref(), Some("analyst-evening"));
        assert_eq!(msg.category.as_deref(), Some("macro-checkpoint-reeval"));
        assert_eq!(msg.layer.as_deref(), Some("macro"));
        assert!(
            msg.content.contains("stage-6"),
            "message must name parent thesis slug, got: {}",
            msg.content
        );
        assert!(
            msg.content.contains("1 of 3"),
            "message must report 1 of 3 checkpoints failed, got: {}",
            msg.content
        );
    }

    #[test]
    fn scoring_macro_checkpoint_correct_does_not_emit_reeval_message() {
        let conn = db::open_in_memory();
        let id = add_prediction(
            &conn,
            "[thesis=fourth-turning] Institutional approval ratings fall below 30%",
            None,
            Some("medium"),
            Some(MACRO_CHECKPOINT_TIMEFRAME),
            None,
            Some("analyst-macro"),
            Some("2026-09-28"),
            None,
        )
        .unwrap();
        score_prediction(&conn, id, "correct", None, None).unwrap();
        let msgs =
            agent_messages::list_messages(&conn, Some("analyst-macro"), None, None, false, None, None, None)
                .unwrap();
        assert!(msgs.is_empty(), "correct scoring must not surface a re-eval");
    }

    #[test]
    fn scoring_macro_prediction_wrong_does_not_emit_reeval_message() {
        let conn = db::open_in_memory();
        // A multi-year structural macro call — NOT a checkpoint.
        let id = add_prediction(
            &conn,
            "[thesis=de-dollarisation] By 2029 USD reserves fall under 50%",
            None,
            Some("medium"),
            Some("macro"),
            None,
            Some("analyst-macro"),
            Some("2029-01-01"),
            None,
        )
        .unwrap();
        score_prediction(&conn, id, "wrong", None, None).unwrap();
        let msgs =
            agent_messages::list_messages(&conn, Some("analyst-macro"), None, None, false, None, None, None)
                .unwrap();
        assert!(
            msgs.is_empty(),
            "scoring a `timeframe='macro'` row must NOT trigger checkpoint re-eval"
        );
    }
}
