use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// Score for a resolved debate — which side was right.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateScore {
    pub id: i64,
    pub debate_id: i64,
    /// Which side won: "bull", "bear", or "mixed" (both had valid points)
    pub winner: String,
    /// How decisive was the outcome: "decisive", "marginal", "mixed"
    pub margin: String,
    /// What actually happened — the factual outcome
    pub actual_outcome: String,
    /// Which specific arguments from each side were validated or invalidated
    pub argument_assessment: Option<String>,
    /// Agent that scored this debate
    pub scored_by: Option<String>,
    pub scored_at: String,
}

/// Aggregate accuracy stats per topic keyword or overall.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateAccuracy {
    pub total_scored: usize,
    pub bull_wins: usize,
    pub bear_wins: usize,
    pub mixed: usize,
    pub decisive_count: usize,
    pub marginal_count: usize,
    /// Bull win rate as a percentage (0-100)
    pub bull_win_rate_pct: f64,
    /// Bear win rate as a percentage (0-100)
    pub bear_win_rate_pct: f64,
}

/// Full view of a scored debate (debate + rounds + score).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredDebateView {
    pub debate_id: i64,
    pub topic: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub resolution_summary: Option<String>,
    pub score: DebateScore,
}

impl DebateScore {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            debate_id: row.get(1)?,
            winner: row.get(2)?,
            margin: row.get(3)?,
            actual_outcome: row.get(4)?,
            argument_assessment: row.get(5)?,
            scored_by: row.get(6)?,
            scored_at: row.get(7)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn validate_winner(value: &str) -> Result<()> {
    match value {
        "bull" | "bear" | "mixed" => Ok(()),
        _ => anyhow::bail!(
            "invalid winner '{}'. Valid: bull, bear, mixed",
            value
        ),
    }
}

pub fn validate_margin(value: &str) -> Result<()> {
    match value {
        "decisive" | "marginal" | "mixed" => Ok(()),
        _ => anyhow::bail!(
            "invalid margin '{}'. Valid: decisive, marginal, mixed",
            value
        ),
    }
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS debate_scores (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            debate_id INTEGER NOT NULL UNIQUE,
            winner TEXT NOT NULL,
            margin TEXT NOT NULL DEFAULT 'marginal',
            actual_outcome TEXT NOT NULL,
            argument_assessment TEXT,
            scored_by TEXT,
            scored_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (debate_id) REFERENCES debates(id)
        );
        CREATE INDEX IF NOT EXISTS idx_debate_scores_debate ON debate_scores(debate_id);
        CREATE INDEX IF NOT EXISTS idx_debate_scores_winner ON debate_scores(winner);",
    )?;
    Ok(())
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS debate_scores (
                id BIGSERIAL PRIMARY KEY,
                debate_id BIGINT NOT NULL UNIQUE,
                winner TEXT NOT NULL,
                margin TEXT NOT NULL DEFAULT 'marginal',
                actual_outcome TEXT NOT NULL,
                argument_assessment TEXT,
                scored_by TEXT,
                scored_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                CONSTRAINT fk_debate_scores_debate
                    FOREIGN KEY (debate_id) REFERENCES debates(id)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_debate_scores_debate ON debate_scores(debate_id)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_debate_scores_winner ON debate_scores(winner)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CRUD — SQLite
// ---------------------------------------------------------------------------

fn score_debate(
    conn: &Connection,
    debate_id: i64,
    winner: &str,
    margin: &str,
    actual_outcome: &str,
    argument_assessment: Option<&str>,
    scored_by: Option<&str>,
) -> Result<i64> {
    validate_winner(winner)?;
    validate_margin(margin)?;
    conn.execute(
        "INSERT OR REPLACE INTO debate_scores
         (debate_id, winner, margin, actual_outcome, argument_assessment, scored_by)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![debate_id, winner, margin, actual_outcome, argument_assessment, scored_by],
    )?;
    Ok(conn.last_insert_rowid())
}

fn get_score(conn: &Connection, debate_id: i64) -> Result<Option<DebateScore>> {
    let mut stmt = conn.prepare(
        "SELECT id, debate_id, winner, margin, actual_outcome, argument_assessment, scored_by, scored_at
         FROM debate_scores WHERE debate_id = ?",
    )?;
    let mut rows = stmt.query_map([debate_id], DebateScore::from_row)?;
    match rows.next() {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn list_scored_debates(
    conn: &Connection,
    topic_filter: Option<&str>,
    winner_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ScoredDebateView>> {
    let mut query = String::from(
        "SELECT d.id, d.topic, d.created_at, d.resolved_at, d.resolution_summary,
                s.id, s.debate_id, s.winner, s.margin, s.actual_outcome,
                s.argument_assessment, s.scored_by, s.scored_at
         FROM debate_scores s
         JOIN debates d ON d.id = s.debate_id
         WHERE 1=1",
    );
    if let Some(t) = topic_filter {
        query.push_str(&format!(
            " AND d.topic LIKE '%{}%'",
            t.replace('\'', "''")
        ));
    }
    if let Some(w) = winner_filter {
        query.push_str(&format!(" AND s.winner = '{}'", w.replace('\'', "''")));
    }
    query.push_str(" ORDER BY s.scored_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok(ScoredDebateView {
            debate_id: row.get(0)?,
            topic: row.get(1)?,
            created_at: row.get(2)?,
            resolved_at: row.get(3)?,
            resolution_summary: row.get(4)?,
            score: DebateScore {
                id: row.get(5)?,
                debate_id: row.get(6)?,
                winner: row.get(7)?,
                margin: row.get(8)?,
                actual_outcome: row.get(9)?,
                argument_assessment: row.get(10)?,
                scored_by: row.get(11)?,
                scored_at: row.get(12)?,
            },
        })
    })?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn compute_accuracy(
    conn: &Connection,
    topic_filter: Option<&str>,
) -> Result<DebateAccuracy> {
    let mut query = String::from(
        "SELECT s.winner, s.margin
         FROM debate_scores s
         JOIN debates d ON d.id = s.debate_id
         WHERE 1=1",
    );
    if let Some(t) = topic_filter {
        query.push_str(&format!(
            " AND d.topic LIKE '%{}%'",
            t.replace('\'', "''")
        ));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
        ))
    })?;

    let mut total = 0usize;
    let mut bull_wins = 0usize;
    let mut bear_wins = 0usize;
    let mut mixed = 0usize;
    let mut decisive = 0usize;
    let mut marginal = 0usize;

    for row in rows {
        let (winner, margin) = row?;
        total += 1;
        match winner.as_str() {
            "bull" => bull_wins += 1,
            "bear" => bear_wins += 1,
            _ => mixed += 1,
        }
        match margin.as_str() {
            "decisive" => decisive += 1,
            "marginal" => marginal += 1,
            _ => {} // "mixed" counted in total only
        }
    }

    let bull_rate = if total > 0 {
        (bull_wins as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let bear_rate = if total > 0 {
        (bear_wins as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(DebateAccuracy {
        total_scored: total,
        bull_wins,
        bear_wins,
        mixed,
        decisive_count: decisive,
        marginal_count: marginal,
        bull_win_rate_pct: bull_rate,
        bear_win_rate_pct: bear_rate,
    })
}

// ---------------------------------------------------------------------------
// CRUD — PostgreSQL
// ---------------------------------------------------------------------------

type ScorePgRow = (i64, i64, String, String, String, Option<String>, Option<String>, String);

fn score_from_pg(r: ScorePgRow) -> DebateScore {
    DebateScore {
        id: r.0,
        debate_id: r.1,
        winner: r.2,
        margin: r.3,
        actual_outcome: r.4,
        argument_assessment: r.5,
        scored_by: r.6,
        scored_at: r.7,
    }
}

fn score_debate_postgres(
    pool: &PgPool,
    debate_id: i64,
    winner: &str,
    margin: &str,
    actual_outcome: &str,
    argument_assessment: Option<&str>,
    scored_by: Option<&str>,
) -> Result<i64> {
    validate_winner(winner)?;
    validate_margin(margin)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO debate_scores (debate_id, winner, margin, actual_outcome, argument_assessment, scored_by)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (debate_id) DO UPDATE SET
                winner = EXCLUDED.winner,
                margin = EXCLUDED.margin,
                actual_outcome = EXCLUDED.actual_outcome,
                argument_assessment = EXCLUDED.argument_assessment,
                scored_by = EXCLUDED.scored_by,
                scored_at = NOW()
             RETURNING id",
        )
        .bind(debate_id)
        .bind(winner)
        .bind(margin)
        .bind(actual_outcome)
        .bind(argument_assessment)
        .bind(scored_by)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn get_score_postgres(pool: &PgPool, debate_id: i64) -> Result<Option<DebateScore>> {
    let row: Option<ScorePgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, debate_id, winner, margin, actual_outcome, argument_assessment, scored_by, scored_at::text
             FROM debate_scores WHERE debate_id = $1",
        )
        .bind(debate_id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(score_from_pg))
}

/// Joined debate+score row from PostgreSQL (13 columns).
#[allow(clippy::type_complexity)]
type ScoredViewPgRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
);

fn scored_view_from_pg(r: ScoredViewPgRow) -> ScoredDebateView {
    ScoredDebateView {
        debate_id: r.0,
        topic: r.1,
        created_at: r.2,
        resolved_at: r.3,
        resolution_summary: r.4,
        score: DebateScore {
            id: r.5,
            debate_id: r.6,
            winner: r.7,
            margin: r.8,
            actual_outcome: r.9,
            argument_assessment: r.10,
            scored_by: r.11,
            scored_at: r.12,
        },
    }
}

fn list_scored_debates_postgres(
    pool: &PgPool,
    topic_filter: Option<&str>,
    winner_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ScoredDebateView>> {
    let mut query_str = String::from(
        "SELECT d.id, d.topic, d.created_at::text, d.resolved_at::text, d.resolution_summary,
                s.id, s.debate_id, s.winner, s.margin, s.actual_outcome,
                s.argument_assessment, s.scored_by, s.scored_at::text
         FROM debate_scores s
         JOIN debates d ON d.id = s.debate_id
         WHERE 1=1",
    );
    if let Some(t) = topic_filter {
        query_str.push_str(&format!(
            " AND d.topic ILIKE '%{}%'",
            t.replace('\'', "''")
        ));
    }
    if let Some(w) = winner_filter {
        query_str.push_str(&format!(" AND s.winner = '{}'", w.replace('\'', "''")));
    }
    query_str.push_str(" ORDER BY s.scored_at DESC");
    if let Some(n) = limit {
        query_str.push_str(&format!(" LIMIT {}", n));
    }
    let rows: Vec<ScoredViewPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&query_str).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(scored_view_from_pg).collect())
}

fn compute_accuracy_postgres(
    pool: &PgPool,
    topic_filter: Option<&str>,
) -> Result<DebateAccuracy> {
    let mut query_str = String::from(
        "SELECT s.winner, s.margin
         FROM debate_scores s
         JOIN debates d ON d.id = s.debate_id
         WHERE 1=1",
    );
    if let Some(t) = topic_filter {
        query_str.push_str(&format!(
            " AND d.topic ILIKE '%{}%'",
            t.replace('\'', "''")
        ));
    }
    let rows: Vec<(String, String)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&query_str).fetch_all(pool).await
    })?;

    let total = rows.len();
    let mut bull_wins = 0usize;
    let mut bear_wins = 0usize;
    let mut mixed = 0usize;
    let mut decisive = 0usize;
    let mut marginal = 0usize;

    for (winner, margin) in &rows {
        match winner.as_str() {
            "bull" => bull_wins += 1,
            "bear" => bear_wins += 1,
            _ => mixed += 1,
        }
        match margin.as_str() {
            "decisive" => decisive += 1,
            "marginal" => marginal += 1,
            _ => {}
        }
    }

    let bull_rate = if total > 0 {
        (bull_wins as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let bear_rate = if total > 0 {
        (bear_wins as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(DebateAccuracy {
        total_scored: total,
        bull_wins,
        bear_wins,
        mixed,
        decisive_count: decisive,
        marginal_count: marginal,
        bull_win_rate_pct: bull_rate,
        bear_win_rate_pct: bear_rate,
    })
}

// ---------------------------------------------------------------------------
// Backend dispatch
// ---------------------------------------------------------------------------

pub fn score_debate_backend(
    backend: &BackendConnection,
    debate_id: i64,
    winner: &str,
    margin: &str,
    actual_outcome: &str,
    argument_assessment: Option<&str>,
    scored_by: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            score_debate(conn, debate_id, winner, margin, actual_outcome, argument_assessment, scored_by)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            score_debate_postgres(pool, debate_id, winner, margin, actual_outcome, argument_assessment, scored_by)
        },
    )
}

pub fn get_score_backend(
    backend: &BackendConnection,
    debate_id: i64,
) -> Result<Option<DebateScore>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            get_score(conn, debate_id)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            get_score_postgres(pool, debate_id)
        },
    )
}

pub fn list_scored_debates_backend(
    backend: &BackendConnection,
    topic_filter: Option<&str>,
    winner_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ScoredDebateView>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            list_scored_debates(conn, topic_filter, winner_filter, limit)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            list_scored_debates_postgres(pool, topic_filter, winner_filter, limit)
        },
    )
}

pub fn compute_accuracy_backend(
    backend: &BackendConnection,
    topic_filter: Option<&str>,
) -> Result<DebateAccuracy> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            compute_accuracy(conn, topic_filter)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            compute_accuracy_postgres(pool, topic_filter)
        },
    )
}

/// List resolved debates that haven't been scored yet.
pub fn list_unscored_backend(
    backend: &BackendConnection,
    limit: Option<usize>,
) -> Result<Vec<crate::db::debates::Debate>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            let mut q = String::from(
                "SELECT d.id, d.topic, d.status, d.max_rounds, d.created_at, d.resolved_at, d.resolution_summary
                 FROM debates d
                 LEFT JOIN debate_scores s ON s.debate_id = d.id
                 WHERE d.status = 'resolved' AND s.id IS NULL
                 ORDER BY d.resolved_at DESC",
            );
            if let Some(n) = limit {
                q.push_str(&format!(" LIMIT {}", n));
            }
            let mut stmt = conn.prepare(&q)?;
            let rows = stmt.query_map([], |row| {
                Ok(crate::db::debates::Debate {
                    id: row.get(0)?,
                    topic: row.get(1)?,
                    status: row.get(2)?,
                    max_rounds: row.get(3)?,
                    created_at: row.get(4)?,
                    resolved_at: row.get(5)?,
                    resolution_summary: row.get(6)?,
                })
            })?;
            let mut items = Vec::new();
            for row in rows {
                items.push(row?);
            }
            Ok(items)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            let mut q = String::from(
                "SELECT d.id, d.topic, d.status, d.max_rounds::bigint, d.created_at::text, d.resolved_at::text, d.resolution_summary
                 FROM debates d
                 LEFT JOIN debate_scores s ON s.debate_id = d.id
                 WHERE d.status = 'resolved' AND s.id IS NULL
                 ORDER BY d.resolved_at DESC",
            );
            if let Some(n) = limit {
                q.push_str(&format!(" LIMIT {}", n));
            }
            // Re-use the debate PG tuple from debates module.
            type DebatePgRow = (i64, String, String, i64, String, Option<String>, Option<String>);
            let rows: Vec<DebatePgRow> = crate::db::pg_runtime::block_on(async {
                sqlx::query_as(&q).fetch_all(pool).await
            })?;
            Ok(rows
                .into_iter()
                .map(|r| crate::db::debates::Debate {
                    id: r.0,
                    topic: r.1,
                    status: r.2,
                    max_rounds: r.3,
                    created_at: r.4,
                    resolved_at: r.5,
                    resolution_summary: r.6,
                })
                .collect())
        },
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Create debates table first (FK dependency)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS debates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                topic TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                max_rounds INTEGER NOT NULL DEFAULT 3,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                resolved_at TEXT,
                resolution_summary TEXT
            );",
        )
        .unwrap();
        ensure_tables(&conn).unwrap();
        conn
    }

    fn insert_debate(conn: &Connection, topic: &str, resolved: bool) -> i64 {
        conn.execute(
            "INSERT INTO debates (topic, status, resolved_at, resolution_summary)
             VALUES (?, ?, ?, ?)",
            params![
                topic,
                if resolved { "resolved" } else { "active" },
                if resolved {
                    Some("2026-03-29 12:00:00")
                } else {
                    None
                },
                if resolved {
                    Some("Test resolution")
                } else {
                    None
                },
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn test_create_tables() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='debate_scores'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_score_debate() {
        let conn = setup_db();
        let debate_id = insert_debate(&conn, "BTC to 200k?", true);

        let id = score_debate(
            &conn,
            debate_id,
            "bull",
            "decisive",
            "BTC reached 185k — bull case largely validated.",
            Some("Bull's ETF flow argument was correct. Bear's regulatory risk was overblown."),
            Some("evening-analysis"),
        )
        .unwrap();
        assert!(id > 0);

        let score = get_score(&conn, debate_id).unwrap().unwrap();
        assert_eq!(score.winner, "bull");
        assert_eq!(score.margin, "decisive");
        assert_eq!(
            score.actual_outcome,
            "BTC reached 185k — bull case largely validated."
        );
        assert_eq!(score.scored_by.as_deref(), Some("evening-analysis"));
    }

    #[test]
    fn test_score_upsert() {
        let conn = setup_db();
        let debate_id = insert_debate(&conn, "Gold correction?", true);

        score_debate(
            &conn,
            debate_id,
            "bear",
            "marginal",
            "Gold pulled back 3%.",
            None,
            None,
        )
        .unwrap();

        // Update with new score
        score_debate(
            &conn,
            debate_id,
            "bull",
            "decisive",
            "Gold recovered and broke ATH.",
            Some("Bear was right short-term, bull was right structurally."),
            Some("weekly-review"),
        )
        .unwrap();

        let score = get_score(&conn, debate_id).unwrap().unwrap();
        assert_eq!(score.winner, "bull");
        assert_eq!(score.margin, "decisive");
    }

    #[test]
    fn test_list_scored_debates() {
        let conn = setup_db();
        let d1 = insert_debate(&conn, "BTC cycle bottom timing", true);
        let d2 = insert_debate(&conn, "Gold structural bid", true);
        let d3 = insert_debate(&conn, "Silver squeeze", true);

        score_debate(&conn, d1, "bull", "decisive", "Bottom was Oct 2026.", None, None).unwrap();
        score_debate(&conn, d2, "bull", "marginal", "Gold held above 5000.", None, None).unwrap();
        score_debate(&conn, d3, "bear", "decisive", "Squeeze failed.", None, None).unwrap();

        let all = list_scored_debates(&conn, None, None, None).unwrap();
        assert_eq!(all.len(), 3);

        let bulls = list_scored_debates(&conn, None, Some("bull"), None).unwrap();
        assert_eq!(bulls.len(), 2);

        let bears = list_scored_debates(&conn, None, Some("bear"), None).unwrap();
        assert_eq!(bears.len(), 1);

        let gold = list_scored_debates(&conn, Some("Gold"), None, None).unwrap();
        assert_eq!(gold.len(), 1);
        assert_eq!(gold[0].topic, "Gold structural bid");

        let limited = list_scored_debates(&conn, None, None, Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_compute_accuracy() {
        let conn = setup_db();
        let d1 = insert_debate(&conn, "BTC to 200k?", true);
        let d2 = insert_debate(&conn, "Gold to 6000?", true);
        let d3 = insert_debate(&conn, "Silver squeeze?", true);
        let d4 = insert_debate(&conn, "TSLA generational buy?", true);
        let d5 = insert_debate(&conn, "Oil to 150?", true);

        score_debate(&conn, d1, "bull", "decisive", "Yes.", None, None).unwrap();
        score_debate(&conn, d2, "bull", "marginal", "Close.", None, None).unwrap();
        score_debate(&conn, d3, "bear", "decisive", "No.", None, None).unwrap();
        score_debate(&conn, d4, "mixed", "mixed", "Both had points.", None, None).unwrap();
        score_debate(&conn, d5, "bear", "marginal", "Oil peaked at 120.", None, None).unwrap();

        let acc = compute_accuracy(&conn, None).unwrap();
        assert_eq!(acc.total_scored, 5);
        assert_eq!(acc.bull_wins, 2);
        assert_eq!(acc.bear_wins, 2);
        assert_eq!(acc.mixed, 1);
        assert_eq!(acc.decisive_count, 2);
        assert_eq!(acc.marginal_count, 2);
        assert!((acc.bull_win_rate_pct - 40.0).abs() < 0.01);
        assert!((acc.bear_win_rate_pct - 40.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_accuracy_with_topic_filter() {
        let conn = setup_db();
        let d1 = insert_debate(&conn, "BTC cycle bottom", true);
        let d2 = insert_debate(&conn, "BTC hash rate impact", true);
        let d3 = insert_debate(&conn, "Gold structural bid", true);

        score_debate(&conn, d1, "bull", "decisive", "Bull correct.", None, None).unwrap();
        score_debate(&conn, d2, "bear", "marginal", "Bear correct.", None, None).unwrap();
        score_debate(&conn, d3, "bull", "decisive", "Bull correct.", None, None).unwrap();

        let btc_acc = compute_accuracy(&conn, Some("BTC")).unwrap();
        assert_eq!(btc_acc.total_scored, 2);
        assert_eq!(btc_acc.bull_wins, 1);
        assert_eq!(btc_acc.bear_wins, 1);

        let gold_acc = compute_accuracy(&conn, Some("Gold")).unwrap();
        assert_eq!(gold_acc.total_scored, 1);
        assert_eq!(gold_acc.bull_wins, 1);
    }

    #[test]
    fn test_list_unscored() {
        let conn = setup_db();
        let d1 = insert_debate(&conn, "Scored debate", true);
        let _d2 = insert_debate(&conn, "Unscored resolved debate", true);
        let _d3 = insert_debate(&conn, "Active debate", false);

        score_debate(&conn, d1, "bull", "decisive", "Test.", None, None).unwrap();

        // list_unscored is backend-dispatched, test via raw SQL
        let mut stmt = conn
            .prepare(
                "SELECT d.id, d.topic
                 FROM debates d
                 LEFT JOIN debate_scores s ON s.debate_id = d.id
                 WHERE d.status = 'resolved' AND s.id IS NULL
                 ORDER BY d.resolved_at DESC",
            )
            .unwrap();
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "Unscored resolved debate");
    }

    #[test]
    fn test_nonexistent_score() {
        let conn = setup_db();
        let result = get_score(&conn, 999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_winner() {
        assert!(validate_winner("bull").is_ok());
        assert!(validate_winner("bear").is_ok());
        assert!(validate_winner("mixed").is_ok());
        assert!(validate_winner("neutral").is_err());
    }

    #[test]
    fn test_validate_margin() {
        assert!(validate_margin("decisive").is_ok());
        assert!(validate_margin("marginal").is_ok());
        assert!(validate_margin("mixed").is_ok());
        assert!(validate_margin("huge").is_err());
    }

    #[test]
    fn test_empty_accuracy() {
        let conn = setup_db();
        let acc = compute_accuracy(&conn, None).unwrap();
        assert_eq!(acc.total_scored, 0);
        assert_eq!(acc.bull_wins, 0);
        assert_eq!(acc.bear_wins, 0);
        assert!((acc.bull_win_rate_pct - 0.0).abs() < 0.01);
    }
}
