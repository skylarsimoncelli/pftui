use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A structured lesson extracted from a wrong prediction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionLesson {
    pub id: i64,
    pub prediction_id: i64,
    /// Type of miss: directional, timing, magnitude
    pub miss_type: String,
    /// What was originally predicted
    pub what_predicted: String,
    /// What actually happened
    pub what_happened: String,
    /// Why the prediction was wrong — root cause analysis
    pub why_wrong: String,
    /// What signal was misread or missed
    pub signal_misread: Option<String>,
    pub created_at: String,
    /// Lifecycle status: active | retired | superseded. Defaults to active.
    #[serde(default = "default_status")]
    pub status: String,
    /// Last time this lesson was cited from another row (typically
    /// `user_predictions.lessons_applied`). Denormalised from
    /// `lesson_citations` by the curation routine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_cited_at: Option<String>,
}

fn default_status() -> String {
    "active".to_string()
}

/// Summary joining prediction data with its lesson (if any).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionLessonView {
    pub prediction_id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub conviction: String,
    pub timeframe: Option<String>,
    pub confidence: Option<f64>,
    pub source_agent: Option<String>,
    pub target_date: Option<String>,
    pub outcome: String,
    pub score_notes: Option<String>,
    pub created_at: String,
    pub scored_at: Option<String>,
    /// Structured lesson, if one has been added
    pub lesson: Option<PredictionLesson>,
}

impl PredictionLesson {
    #[allow(dead_code)]
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            prediction_id: row.get(1)?,
            miss_type: row.get(2)?,
            what_predicted: row.get(3)?,
            what_happened: row.get(4)?,
            why_wrong: row.get(5)?,
            signal_misread: row.get(6)?,
            created_at: row.get(7)?,
            status: row.get::<_, Option<String>>(8)?.unwrap_or_else(default_status),
            last_cited_at: row.get(9)?,
        })
    }
}

fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS prediction_lessons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prediction_id INTEGER NOT NULL UNIQUE,
            miss_type TEXT NOT NULL,
            what_predicted TEXT NOT NULL,
            what_happened TEXT NOT NULL,
            why_wrong TEXT NOT NULL,
            signal_misread TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (prediction_id) REFERENCES user_predictions(id)
        );
        CREATE INDEX IF NOT EXISTS idx_prediction_lessons_pid
            ON prediction_lessons(prediction_id);",
    )?;
    // Idempotent forward-migration so paths that only call ensure_table
    // (in-module tests, ad-hoc callers) also pick up the half-life columns.
    let has_status: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('prediction_lessons') WHERE name = 'status'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_status {
        conn.execute_batch(
            "ALTER TABLE prediction_lessons ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
                CHECK(status IN ('active','retired','superseded'))",
        )?;
    }
    let has_last_cited_at: bool = conn
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('prediction_lessons') WHERE name = 'last_cited_at'",
        )?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !has_last_cited_at {
        conn.execute_batch(
            "ALTER TABLE prediction_lessons ADD COLUMN last_cited_at TEXT",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_prediction_lessons_status
            ON prediction_lessons(status);
         CREATE INDEX IF NOT EXISTS idx_prediction_lessons_last_cited_at
            ON prediction_lessons(last_cited_at);",
    )?;
    Ok(())
}

fn ensure_table_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS prediction_lessons (
                id BIGSERIAL PRIMARY KEY,
                prediction_id BIGINT NOT NULL UNIQUE,
                miss_type TEXT NOT NULL,
                what_predicted TEXT NOT NULL,
                what_happened TEXT NOT NULL,
                why_wrong TEXT NOT NULL,
                signal_misread TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                status TEXT NOT NULL DEFAULT 'active'
                    CHECK(status IN ('active','retired','superseded')),
                last_cited_at TIMESTAMPTZ,
                CONSTRAINT fk_prediction_lessons_pid
                    FOREIGN KEY (prediction_id) REFERENCES user_predictions(id)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE prediction_lessons ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active'",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE prediction_lessons ADD COLUMN IF NOT EXISTS last_cited_at TIMESTAMPTZ",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_prediction_lessons_pid
             ON prediction_lessons(prediction_id)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_prediction_lessons_status
             ON prediction_lessons(status)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_prediction_lessons_last_cited_at
             ON prediction_lessons(last_cited_at)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

/// Add a structured lesson for a wrong prediction.
pub fn add_lesson(
    conn: &Connection,
    prediction_id: i64,
    miss_type: &str,
    what_predicted: &str,
    what_happened: &str,
    why_wrong: &str,
    signal_misread: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT OR REPLACE INTO prediction_lessons
         (prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            prediction_id,
            miss_type,
            what_predicted,
            what_happened,
            why_wrong,
            signal_misread
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn add_lesson_postgres(
    pool: &PgPool,
    prediction_id: i64,
    miss_type: &str,
    what_predicted: &str,
    what_happened: &str,
    why_wrong: &str,
    signal_misread: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO prediction_lessons
             (prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (prediction_id) DO UPDATE SET
                miss_type = EXCLUDED.miss_type,
                what_predicted = EXCLUDED.what_predicted,
                what_happened = EXCLUDED.what_happened,
                why_wrong = EXCLUDED.why_wrong,
                signal_misread = EXCLUDED.signal_misread,
                created_at = NOW()
             RETURNING id",
        )
        .bind(prediction_id)
        .bind(miss_type)
        .bind(what_predicted)
        .bind(what_happened)
        .bind(why_wrong)
        .bind(signal_misread)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

pub fn add_lesson_backend(
    backend: &BackendConnection,
    prediction_id: i64,
    miss_type: &str,
    what_predicted: &str,
    what_happened: &str,
    why_wrong: &str,
    signal_misread: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            ensure_table(conn)?;
            add_lesson(
                conn,
                prediction_id,
                miss_type,
                what_predicted,
                what_happened,
                why_wrong,
                signal_misread,
            )
        },
        |pool| {
            ensure_table_postgres(pool)?;
            add_lesson_postgres(
                pool,
                prediction_id,
                miss_type,
                what_predicted,
                what_happened,
                why_wrong,
                signal_misread,
            )
        },
    )
}

/// Get lesson for a specific prediction.
#[allow(dead_code)]
pub fn get_lesson_by_prediction(
    conn: &Connection,
    prediction_id: i64,
) -> Result<Option<PredictionLesson>> {
    let mut stmt = conn.prepare(
        "SELECT id, prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, created_at, status, last_cited_at
         FROM prediction_lessons WHERE prediction_id = ?",
    )?;
    let mut rows = stmt.query_map([prediction_id], PredictionLesson::from_row)?;
    match rows.next() {
        Some(Ok(lesson)) => Ok(Some(lesson)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

#[allow(dead_code)]
type LessonRow = (
    i64,
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

fn lesson_from_pg_row(r: LessonRow) -> PredictionLesson {
    PredictionLesson {
        id: r.0,
        prediction_id: r.1,
        miss_type: r.2,
        what_predicted: r.3,
        what_happened: r.4,
        why_wrong: r.5,
        signal_misread: r.6,
        created_at: r.7,
        status: r.8.unwrap_or_else(default_status),
        last_cited_at: r.9,
    }
}

#[allow(dead_code)]
fn get_lesson_by_prediction_postgres(
    pool: &PgPool,
    prediction_id: i64,
) -> Result<Option<PredictionLesson>> {
    let row: Option<LessonRow> =
        crate::db::pg_runtime::block_on(async {
            sqlx::query_as(
                "SELECT id, prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, created_at::text, status, last_cited_at
                 FROM prediction_lessons WHERE prediction_id = $1",
            )
            .bind(prediction_id)
            .fetch_optional(pool)
            .await
        })?;
    Ok(row.map(lesson_from_pg_row))
}

/// List all lessons, optionally filtered by miss_type.
#[allow(dead_code)]
pub fn list_lessons(
    conn: &Connection,
    miss_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PredictionLesson>> {
    let mut query = String::from(
        "SELECT id, prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, created_at, status, last_cited_at
         FROM prediction_lessons",
    );
    if let Some(mt) = miss_type {
        query.push_str(&format!(
            " WHERE miss_type = '{}'",
            mt.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], PredictionLesson::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn list_lessons_postgres(
    pool: &PgPool,
    miss_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PredictionLesson>> {
    let rows: Vec<LessonRow> = crate::db::pg_runtime::block_on(async {
        let mut query = String::from(
            "SELECT id, prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, created_at::text, status, last_cited_at
             FROM prediction_lessons",
        );
        if let Some(mt) = miss_type {
            query.push_str(&format!(
                " WHERE miss_type = '{}'",
                mt.replace('\'', "''")
            ));
        }
        query.push_str(" ORDER BY created_at DESC");
        if let Some(n) = limit {
            query.push_str(&format!(" LIMIT {}", n));
        }
        sqlx::query_as(&query).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(lesson_from_pg_row).collect())
}

#[allow(dead_code)]
pub fn list_lessons_backend(
    backend: &BackendConnection,
    miss_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PredictionLesson>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_table(conn)?;
            list_lessons(conn, miss_type, limit)
        },
        |pool| {
            ensure_table_postgres(pool)?;
            list_lessons_postgres(pool, miss_type, limit)
        },
    )
}

/// Get lessons joined with their prediction data for a full view.
pub fn list_lesson_views(
    conn: &Connection,
    miss_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PredictionLessonView>> {
    let mut query = String::from(
        "SELECT p.id, p.claim, p.symbol, p.conviction, p.timeframe, p.confidence,
                p.source_agent, p.target_date, p.outcome, p.score_notes,
                p.created_at, p.scored_at,
                l.id, l.prediction_id, l.miss_type, l.what_predicted, l.what_happened,
                l.why_wrong, l.signal_misread, l.created_at, l.status, l.last_cited_at
         FROM user_predictions p
         LEFT JOIN prediction_lessons l ON p.id = l.prediction_id
         WHERE p.outcome = 'wrong'",
    );
    if let Some(mt) = miss_type {
        query.push_str(&format!(
            " AND l.miss_type = '{}'",
            mt.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY p.scored_at DESC NULLS LAST");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        let lesson_id: Option<i64> = row.get(12)?;
        let lesson = lesson_id.map(|lid| PredictionLesson {
            id: lid,
            prediction_id: row.get::<_, i64>(13).unwrap_or_default(),
            miss_type: row.get::<_, String>(14).unwrap_or_default(),
            what_predicted: row.get::<_, String>(15).unwrap_or_default(),
            what_happened: row.get::<_, String>(16).unwrap_or_default(),
            why_wrong: row.get::<_, String>(17).unwrap_or_default(),
            signal_misread: row.get::<_, Option<String>>(18).unwrap_or_default(),
            created_at: row.get::<_, String>(19).unwrap_or_default(),
            status: row
                .get::<_, Option<String>>(20)
                .unwrap_or_default()
                .unwrap_or_else(default_status),
            last_cited_at: row.get::<_, Option<String>>(21).unwrap_or_default(),
        });
        Ok(PredictionLessonView {
            prediction_id: row.get(0)?,
            claim: row.get(1)?,
            symbol: row.get(2)?,
            conviction: row.get(3)?,
            timeframe: row.get(4)?,
            confidence: row.get(5)?,
            source_agent: row.get(6)?,
            target_date: row.get(7)?,
            outcome: row.get(8)?,
            score_notes: row.get(9)?,
            created_at: row.get(10)?,
            scored_at: row.get(11)?,
            lesson,
        })
    })?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn list_lesson_views_postgres(
    pool: &PgPool,
    miss_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PredictionLessonView>> {
    // sqlx FromRow for tuples maxes out at 16 columns.
    // Fetch wrong predictions first, then join lessons in a second query.
    use crate::db::user_predictions;

    let mut wrong_predictions =
        user_predictions::list_predictions_backend(
            &BackendConnection::Postgres { pool: pool.clone() },
            Some("wrong"),
            None,
            None,
            limit,
        )?;

    // Build lesson map
    let lessons = list_lessons_postgres(pool, miss_type, None)?;
    let lesson_map: std::collections::HashMap<i64, PredictionLesson> = lessons
        .into_iter()
        .map(|l| (l.prediction_id, l))
        .collect();

    // If filtering by miss_type, only keep predictions that have a matching lesson
    if miss_type.is_some() {
        wrong_predictions.retain(|p| lesson_map.contains_key(&p.id));
    }

    Ok(wrong_predictions
        .into_iter()
        .map(|p| {
            let lesson = lesson_map.get(&p.id).cloned();
            PredictionLessonView {
                prediction_id: p.id,
                claim: p.claim,
                symbol: p.symbol,
                conviction: p.conviction,
                timeframe: p.timeframe,
                confidence: p.confidence,
                source_agent: p.source_agent,
                target_date: p.target_date,
                outcome: p.outcome,
                score_notes: p.score_notes,
                created_at: p.created_at,
                scored_at: p.scored_at,
                lesson,
            }
        })
        .collect())
}

pub fn list_lesson_views_backend(
    backend: &BackendConnection,
    miss_type: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PredictionLessonView>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_table(conn)?;
            // Ensure user_predictions columns exist too
            crate::db::user_predictions::list_predictions(conn, Some("wrong"), None, None, Some(0))?;
            list_lesson_views(conn, miss_type, limit)
        },
        |pool| {
            ensure_table_postgres(pool)?;
            list_lesson_views_postgres(pool, miss_type, limit)
        },
    )
}

/// Count wrong predictions with and without lessons.
pub fn lesson_coverage(conn: &Connection) -> Result<(usize, usize)> {
    let total_wrong: usize = conn.query_row(
        "SELECT COUNT(*) FROM user_predictions WHERE outcome = 'wrong'",
        [],
        |row| row.get(0),
    )?;
    let with_lessons: usize = conn.query_row(
        "SELECT COUNT(*) FROM prediction_lessons",
        [],
        |row| row.get(0),
    )?;
    Ok((total_wrong, with_lessons))
}

fn lesson_coverage_postgres(pool: &PgPool) -> Result<(usize, usize)> {
    let (total_wrong, with_lessons): (i64, i64) = crate::db::pg_runtime::block_on(async {
        let tw: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM user_predictions WHERE outcome = 'wrong'",
        )
        .fetch_one(pool)
        .await?;
        let wl: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prediction_lessons")
            .fetch_one(pool)
            .await?;
        Ok::<(i64, i64), sqlx::Error>((tw.0, wl.0))
    })?;
    Ok((total_wrong as usize, with_lessons as usize))
}

pub fn lesson_coverage_backend(backend: &BackendConnection) -> Result<(usize, usize)> {
    query::dispatch(
        backend,
        |conn| {
            ensure_table(conn)?;
            lesson_coverage(conn)
        },
        |pool| {
            ensure_table_postgres(pool)?;
            lesson_coverage_postgres(pool)
        },
    )
}

fn validate_miss_type(value: &str) -> Result<()> {
    match value {
        "directional" | "timing" | "magnitude" => Ok(()),
        _ => anyhow::bail!(
            "invalid miss_type '{}'. Valid: directional, timing, magnitude",
            value
        ),
    }
}

pub fn validate_miss_type_str(value: &str) -> Result<()> {
    validate_miss_type(value)
}

// ---------------------------------------------------------------------------
// Lesson half-life curation
// ---------------------------------------------------------------------------

/// Valid status values for the `status` column on `prediction_lessons`.
pub const STATUS_ACTIVE: &str = "active";
#[allow(dead_code)]
pub const STATUS_RETIRED: &str = "retired";
#[allow(dead_code)]
pub const STATUS_SUPERSEDED: &str = "superseded";

#[allow(dead_code)]
fn validate_status(value: &str) -> Result<()> {
    match value {
        STATUS_ACTIVE | STATUS_RETIRED | STATUS_SUPERSEDED => Ok(()),
        _ => anyhow::bail!(
            "invalid status '{}'. Valid: active, retired, superseded",
            value
        ),
    }
}

/// Set the status for a specific lesson row. Returns true if a row was
/// updated.
#[allow(dead_code)]
pub fn set_status(conn: &Connection, id: i64, status: &str) -> Result<bool> {
    validate_status(status)?;
    ensure_table(conn)?;
    let rows = conn.execute(
        "UPDATE prediction_lessons SET status = ?1 WHERE id = ?2",
        params![status, id],
    )?;
    Ok(rows > 0)
}

/// Recompute the denormalised `last_cited_at` column on every lesson from
/// the `lesson_citations` table. Returns the number of lessons updated.
pub fn recompute_last_cited_at(conn: &Connection) -> Result<usize> {
    ensure_table(conn)?;
    crate::db::lesson_citations::ensure_table(conn)?;
    let pairs = crate::db::lesson_citations::latest_citation_per_lesson(conn)?;
    let mut updated = 0;
    for (lesson_id, cited_at) in pairs {
        let n = conn.execute(
            "UPDATE prediction_lessons SET last_cited_at = ?1 WHERE id = ?2",
            params![cited_at, lesson_id],
        )?;
        updated += n;
    }
    Ok(updated)
}

/// Result of one lesson considered for retirement during a curate pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateAction {
    pub lesson_id: i64,
    pub miss_type: String,
    pub created_at: String,
    pub last_cited_at: Option<String>,
    pub action: String, // "retire" | "skip"
    pub reason: String,
}

/// Summary returned from a curate pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateSummary {
    pub retire_after_days: i64,
    pub cutoff_iso: String,
    pub dry_run: bool,
    pub considered: usize,
    pub retired: usize,
    pub skipped: usize,
    pub actions: Vec<CurateAction>,
}

/// Curate the lesson library: retire lessons that have been `active` but
/// uncited and stale for longer than `retire_after_days`, and that have not
/// had a related wrong-scored prediction recently (by `topic` proxy where
/// available, otherwise unconditionally on the staleness rule).
///
/// When `dry_run` is true, no rows are mutated; the returned summary still
/// describes what would have happened.
pub fn curate(
    conn: &Connection,
    retire_after_days: i64,
    dry_run: bool,
) -> Result<CurateSummary> {
    if retire_after_days < 1 {
        anyhow::bail!("--retire-after-days must be >= 1 (got {})", retire_after_days);
    }
    ensure_table(conn)?;
    crate::db::lesson_citations::ensure_table(conn)?;

    // Best-effort: refresh denormalised last_cited_at from lesson_citations
    // before evaluating. Cheap on small libraries (<1k rows).
    let _ = recompute_last_cited_at(conn);

    let cutoff_iso = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(retire_after_days))
        .ok_or_else(|| anyhow::anyhow!("retire_after_days overflow"))?
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    // Pull active lessons + the topic of their originating wrong prediction
    // (joined as best-effort; topic may be NULL if user_predictions row was
    // deleted or the column was absent on an old DB).
    let mut stmt = conn.prepare(
        "SELECT l.id, l.prediction_id, l.miss_type, l.created_at, l.last_cited_at,
                COALESCE(p.topic, '')
         FROM prediction_lessons l
         LEFT JOIN user_predictions p ON p.id = l.prediction_id
         WHERE l.status = 'active'",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut summary = CurateSummary {
        retire_after_days,
        cutoff_iso: cutoff_iso.clone(),
        dry_run,
        considered: rows.len(),
        retired: 0,
        skipped: 0,
        actions: Vec::new(),
    };

    for (id, _pid, miss_type, created_at, last_cited_at, topic) in rows {
        // Staleness rule: lesson is stale if its last_cited_at < cutoff, OR
        // (last_cited_at IS NULL AND created_at < cutoff).
        let stale = match last_cited_at.as_deref() {
            Some(ts) => ts < cutoff_iso.as_str(),
            None => created_at.as_str() < cutoff_iso.as_str(),
        };
        if !stale {
            summary.skipped += 1;
            summary.actions.push(CurateAction {
                lesson_id: id,
                miss_type: miss_type.clone(),
                created_at: created_at.clone(),
                last_cited_at: last_cited_at.clone(),
                action: "skip".to_string(),
                reason: "fresh: cited or created within window".to_string(),
            });
            continue;
        }

        // Cluster freshness rule: if the lesson's prediction has a topic,
        // skip retirement when there has been at least one wrong-scored
        // prediction in the same topic since the cutoff. This preserves
        // lessons whose cluster is still producing fresh evidence.
        let cluster_active = if topic.is_empty() {
            false
        } else {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM user_predictions
                 WHERE outcome = 'wrong' AND topic = ?1 AND scored_at >= ?2",
                params![topic, cutoff_iso],
                |row| row.get(0),
            )
            .unwrap_or(0);
            count > 0
        };
        if cluster_active {
            summary.skipped += 1;
            summary.actions.push(CurateAction {
                lesson_id: id,
                miss_type: miss_type.clone(),
                created_at: created_at.clone(),
                last_cited_at: last_cited_at.clone(),
                action: "skip".to_string(),
                reason: format!(
                    "cluster '{}' still producing wrong-scored predictions",
                    topic
                ),
            });
            continue;
        }

        if !dry_run {
            conn.execute(
                "UPDATE prediction_lessons SET status = 'retired' WHERE id = ?1",
                params![id],
            )?;
        }
        summary.retired += 1;
        summary.actions.push(CurateAction {
            lesson_id: id,
            miss_type: miss_type.clone(),
            created_at: created_at.clone(),
            last_cited_at: last_cited_at.clone(),
            action: "retire".to_string(),
            reason: match last_cited_at.as_deref() {
                Some(_) => format!(
                    "last cited >{} days ago and cluster idle",
                    retire_after_days
                ),
                None => format!(
                    "never cited and created >{} days ago and cluster idle",
                    retire_after_days
                ),
            },
        });
    }

    Ok(summary)
}

/// Curate via backend dispatch. Postgres path is not implemented for now and
/// will be added when the lesson half-life routine is wired to the Postgres
/// substrate; SQLite is the production backend for `prediction_lessons`.
pub fn curate_backend(
    backend: &BackendConnection,
    retire_after_days: i64,
    dry_run: bool,
) -> Result<CurateSummary> {
    query::dispatch(
        backend,
        |conn| curate(conn, retire_after_days, dry_run),
        |_pool| {
            anyhow::bail!(
                "lesson curate is not implemented for the Postgres backend yet"
            )
        },
    )
}

/// Revive a previously retired lesson by id. Returns true if the lesson was
/// updated, false if not found.
pub fn revive(conn: &Connection, id: i64) -> Result<bool> {
    ensure_table(conn)?;
    let updated = conn.execute(
        "UPDATE prediction_lessons SET status = 'active' WHERE id = ?1",
        params![id],
    )?;
    Ok(updated > 0)
}

pub fn revive_backend(backend: &BackendConnection, id: i64) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| revive(conn, id),
        |_pool| {
            anyhow::bail!(
                "lesson revive is not implemented for the Postgres backend yet"
            )
        },
    )
}

/// Library-wide health summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryHealth {
    pub total: i64,
    pub active: i64,
    pub retired: i64,
    pub superseded: i64,
    pub citations_total: i64,
    pub avg_citations_per_active: f64,
}

pub fn library_health(conn: &Connection) -> Result<LibraryHealth> {
    ensure_table(conn)?;
    crate::db::lesson_citations::ensure_table(conn)?;
    let (total, active, retired, superseded): (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*),
                SUM(CASE WHEN status='active' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status='retired' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status='superseded' THEN 1 ELSE 0 END)
         FROM prediction_lessons",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        },
    )?;
    let citations_total: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(citation_count), 0) FROM lesson_citations
             WHERE lesson_id IN (SELECT id FROM prediction_lessons WHERE status='active')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let avg_citations_per_active = if active > 0 {
        citations_total as f64 / active as f64
    } else {
        0.0
    };
    Ok(LibraryHealth {
        total,
        active,
        retired,
        superseded,
        citations_total,
        avg_citations_per_active,
    })
}

pub fn library_health_backend(backend: &BackendConnection) -> Result<LibraryHealth> {
    query::dispatch(
        backend,
        library_health,
        |_pool| {
            anyhow::bail!(
                "lesson library_health is not implemented for the Postgres backend yet"
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::user_predictions;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_predictions (
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
                lessons_applied TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                scored_at TEXT
            )",
        )
        .unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_table() {
        let conn = setup_db();
        // Table should exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='prediction_lessons'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_add_and_get_lesson() {
        let conn = setup_db();
        // Add a wrong prediction
        user_predictions::add_prediction(
            &conn,
            "BTC above 80k by March",
            Some("BTC"),
            Some("high"),
            Some("low"),
            Some(0.8),
            Some("low-agent"),
            Some("2026-03-15"),
            None,
        )
        .unwrap();
        user_predictions::score_prediction(&conn, 1, "wrong", Some("BTC stayed at 70k"), None)
            .unwrap();

        // Add a lesson
        let id = add_lesson(
            &conn,
            1,
            "directional",
            "BTC above 80k by March",
            "BTC traded sideways at 70k, never approached 80k",
            "Overweighted bullish momentum, ignored resistance at 75k and declining volume",
            Some("Volume divergence on daily chart was bearish signal"),
        )
        .unwrap();
        assert!(id > 0);

        // Get lesson by prediction
        let lesson = get_lesson_by_prediction(&conn, 1).unwrap();
        assert!(lesson.is_some());
        let lesson = lesson.unwrap();
        assert_eq!(lesson.prediction_id, 1);
        assert_eq!(lesson.miss_type, "directional");
        assert_eq!(lesson.what_predicted, "BTC above 80k by March");
    }

    #[test]
    fn test_list_lessons() {
        let conn = setup_db();
        // Add two wrong predictions
        user_predictions::add_prediction(&conn, "Gold to 3000", Some("GC=F"), Some("high"), Some("medium"), Some(0.7), None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 1, "wrong", None, None).unwrap();
        user_predictions::add_prediction(&conn, "Silver to 30", Some("SI=F"), Some("medium"), Some("low"), Some(0.6), None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 2, "wrong", None, None).unwrap();

        // Add lessons for both
        add_lesson(&conn, 1, "timing", "Gold to 3000", "Gold stayed at 2800", "Too aggressive on timeline", None).unwrap();
        add_lesson(&conn, 2, "magnitude", "Silver to 30", "Silver only reached 28", "Underestimated resistance", Some("COT positioning was not as bullish as assumed")).unwrap();

        let all = list_lessons(&conn, None, None).unwrap();
        assert_eq!(all.len(), 2);

        let timing_only = list_lessons(&conn, Some("timing"), None).unwrap();
        assert_eq!(timing_only.len(), 1);
        assert_eq!(timing_only[0].miss_type, "timing");
    }

    #[test]
    fn test_lesson_views() {
        let conn = setup_db();
        // Add one wrong prediction with lesson and one without
        user_predictions::add_prediction(&conn, "BTC to 100k", Some("BTC"), Some("high"), Some("high"), Some(0.9), Some("high-agent"), None, None).unwrap();
        user_predictions::score_prediction(&conn, 1, "wrong", Some("Never got close"), None).unwrap();
        user_predictions::add_prediction(&conn, "ETH to 5k", Some("ETH"), Some("medium"), Some("medium"), Some(0.6), None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 2, "wrong", None, None).unwrap();

        add_lesson(&conn, 1, "directional", "BTC to 100k", "BTC dropped to 60k", "Macro headwinds ignored", Some("Fed hawkishness underweighted")).unwrap();

        let views = list_lesson_views(&conn, None, None).unwrap();
        assert_eq!(views.len(), 2);

        // One should have a lesson, one shouldn't
        let with_lesson = views.iter().filter(|v| v.lesson.is_some()).count();
        let without_lesson = views.iter().filter(|v| v.lesson.is_none()).count();
        assert_eq!(with_lesson, 1);
        assert_eq!(without_lesson, 1);
    }

    #[test]
    fn test_lesson_coverage() {
        let conn = setup_db();
        user_predictions::add_prediction(&conn, "Test 1", None, None, None, None, None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 1, "wrong", None, None).unwrap();
        user_predictions::add_prediction(&conn, "Test 2", None, None, None, None, None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 2, "wrong", None, None).unwrap();
        user_predictions::add_prediction(&conn, "Test 3", None, None, None, None, None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 3, "correct", None, None).unwrap();

        let (total_wrong, with_lessons) = lesson_coverage(&conn).unwrap();
        assert_eq!(total_wrong, 2);
        assert_eq!(with_lessons, 0);

        add_lesson(&conn, 1, "directional", "Test 1", "Wrong", "Reason", None).unwrap();
        let (total_wrong, with_lessons) = lesson_coverage(&conn).unwrap();
        assert_eq!(total_wrong, 2);
        assert_eq!(with_lessons, 1);
    }

    #[test]
    fn test_upsert_lesson() {
        let conn = setup_db();
        user_predictions::add_prediction(&conn, "Test", None, None, None, None, None, None, None).unwrap();
        user_predictions::score_prediction(&conn, 1, "wrong", None, None).unwrap();

        add_lesson(&conn, 1, "directional", "Test", "Wrong v1", "Reason v1", None).unwrap();
        let l1 = get_lesson_by_prediction(&conn, 1).unwrap().unwrap();
        assert_eq!(l1.why_wrong, "Reason v1");

        // Upsert should replace
        add_lesson(&conn, 1, "timing", "Test", "Wrong v2", "Reason v2", Some("Missed signal")).unwrap();
        let l2 = get_lesson_by_prediction(&conn, 1).unwrap().unwrap();
        assert_eq!(l2.miss_type, "timing");
        assert_eq!(l2.why_wrong, "Reason v2");
        assert_eq!(l2.signal_misread.as_deref(), Some("Missed signal"));
    }

    #[test]
    fn test_validate_miss_type() {
        assert!(validate_miss_type("directional").is_ok());
        assert!(validate_miss_type("timing").is_ok());
        assert!(validate_miss_type("magnitude").is_ok());
        assert!(validate_miss_type("invalid").is_err());
        assert!(validate_miss_type("").is_err());
    }

    #[test]
    fn test_list_with_limit() {
        let conn = setup_db();
        for i in 1..=5 {
            user_predictions::add_prediction(&conn, &format!("Pred {}", i), None, None, None, None, None, None, None).unwrap();
            user_predictions::score_prediction(&conn, i, "wrong", None, None).unwrap();
            add_lesson(&conn, i, "directional", &format!("Pred {}", i), "Wrong", "Reason", None).unwrap();
        }

        let limited = list_lessons(&conn, None, Some(3)).unwrap();
        assert_eq!(limited.len(), 3);

        let all = list_lessons(&conn, None, None).unwrap();
        assert_eq!(all.len(), 5);
    }

    // -----------------------------------------------------------------
    // Lesson half-life curation
    // -----------------------------------------------------------------

    fn seed_old_lesson(conn: &Connection, prediction_id: i64, days_old: i64) -> i64 {
        // Insert at a deterministic timestamp days_old in the past.
        let ts = chrono::Utc::now()
            - chrono::Duration::days(days_old);
        let created_at = ts.format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            "INSERT INTO prediction_lessons
                (prediction_id, miss_type, what_predicted, what_happened,
                 why_wrong, signal_misread, created_at)
             VALUES (?, 'directional', 'pred', 'happened', 'why', NULL, ?)",
            params![prediction_id, created_at],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn curate_retires_stale_uncited_active_lesson() {
        let conn = setup_db();
        // Stale uncited lesson (120 days old) → should retire.
        user_predictions::add_prediction(
            &conn, "stale claim", Some("BTC"), None, None, None, None, None, None,
        )
        .unwrap();
        let lesson_id = seed_old_lesson(&conn, 1, 120);

        let summary = curate(&conn, 60, false).unwrap();
        assert_eq!(summary.considered, 1);
        assert_eq!(summary.retired, 1);
        assert_eq!(summary.skipped, 0);

        let status: String = conn
            .query_row(
                "SELECT status FROM prediction_lessons WHERE id = ?",
                [lesson_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "retired");
    }

    #[test]
    fn curate_dry_run_does_not_mutate() {
        let conn = setup_db();
        user_predictions::add_prediction(
            &conn, "stale claim", Some("BTC"), None, None, None, None, None, None,
        )
        .unwrap();
        let lesson_id = seed_old_lesson(&conn, 1, 120);

        let summary = curate(&conn, 60, true).unwrap();
        assert_eq!(summary.retired, 1);
        assert!(summary.dry_run);

        let status: String = conn
            .query_row(
                "SELECT status FROM prediction_lessons WHERE id = ?",
                [lesson_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active", "dry-run must not mutate");
    }

    #[test]
    fn curate_skips_fresh_lessons() {
        let conn = setup_db();
        // Fresh (created 10 days ago) → should not retire under 60d threshold.
        user_predictions::add_prediction(
            &conn, "fresh claim", Some("BTC"), None, None, None, None, None, None,
        )
        .unwrap();
        let lesson_id = seed_old_lesson(&conn, 1, 10);

        let summary = curate(&conn, 60, false).unwrap();
        assert_eq!(summary.retired, 0);
        assert_eq!(summary.skipped, 1);

        let status: String = conn
            .query_row(
                "SELECT status FROM prediction_lessons WHERE id = ?",
                [lesson_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
    }

    #[test]
    fn curate_skips_when_last_cited_at_is_fresh() {
        let conn = setup_db();
        user_predictions::add_prediction(
            &conn, "stale claim", Some("BTC"), None, None, None, None, None, None,
        )
        .unwrap();
        let lesson_id = seed_old_lesson(&conn, 1, 120);
        // Mark it as recently cited (5d ago) — should be skipped.
        let ts = (chrono::Utc::now() - chrono::Duration::days(5))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        conn.execute(
            "UPDATE prediction_lessons SET last_cited_at = ?1 WHERE id = ?2",
            params![ts, lesson_id],
        )
        .unwrap();

        let summary = curate(&conn, 60, false).unwrap();
        assert_eq!(summary.retired, 0);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn revive_flips_retired_lesson_back_to_active() {
        let conn = setup_db();
        user_predictions::add_prediction(
            &conn, "claim", Some("BTC"), None, None, None, None, None, None,
        )
        .unwrap();
        let lesson_id = seed_old_lesson(&conn, 1, 120);
        // Retire it via curate.
        curate(&conn, 60, false).unwrap();

        let updated = revive(&conn, lesson_id).unwrap();
        assert!(updated);
        let status: String = conn
            .query_row(
                "SELECT status FROM prediction_lessons WHERE id = ?",
                [lesson_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
    }

    #[test]
    fn revive_returns_false_for_unknown_id() {
        let conn = setup_db();
        assert!(!revive(&conn, 99_999).unwrap());
    }

    #[test]
    fn library_health_counts_and_averages() {
        let conn = setup_db();
        // Two active lessons, one of which retires; total citations across
        // active lessons drives the average.
        for i in 1..=3 {
            user_predictions::add_prediction(
                &conn, &format!("claim {}", i), Some("BTC"), None, None, None, None, None, None,
            )
            .unwrap();
        }
        let l1 = seed_old_lesson(&conn, 1, 5);
        let l2 = seed_old_lesson(&conn, 2, 5);
        let _l3 = seed_old_lesson(&conn, 3, 200); // will retire via curate

        // Add citations: 3 on l1, 1 on l2.
        crate::db::lesson_citations::ensure_table(&conn).unwrap();
        conn.execute(
            "INSERT INTO lesson_citations (lesson_id, cited_in_table, cited_in_id, citation_count)
             VALUES (?, 'user_predictions', 100, 3)",
            params![l1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO lesson_citations (lesson_id, cited_in_table, cited_in_id, citation_count)
             VALUES (?, 'user_predictions', 101, 1)",
            params![l2],
        )
        .unwrap();

        // Retire the third lesson.
        curate(&conn, 60, false).unwrap();

        let h = library_health(&conn).unwrap();
        assert_eq!(h.total, 3);
        assert_eq!(h.active, 2);
        assert_eq!(h.retired, 1);
        assert_eq!(h.citations_total, 4);
        assert!((h.avg_citations_per_active - 2.0).abs() < 1e-9);
    }

    #[test]
    fn set_status_rejects_invalid_value() {
        let conn = setup_db();
        user_predictions::add_prediction(
            &conn, "claim", Some("BTC"), None, None, None, None, None, None,
        )
        .unwrap();
        let lesson_id = seed_old_lesson(&conn, 1, 10);
        assert!(set_status(&conn, lesson_id, "bogus").is_err());
        // valid transitions
        assert!(set_status(&conn, lesson_id, "retired").unwrap());
        assert!(set_status(&conn, lesson_id, "superseded").unwrap());
        assert!(set_status(&conn, lesson_id, "active").unwrap());
    }

    #[test]
    fn curate_rejects_zero_retire_after_days() {
        let conn = setup_db();
        assert!(curate(&conn, 0, false).is_err());
    }
}
