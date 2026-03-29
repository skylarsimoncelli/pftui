use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A structured adversarial debate on an asset or scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debate {
    pub id: i64,
    pub topic: String,
    pub status: String,
    pub max_rounds: i64,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub resolution_summary: Option<String>,
}

/// A single round within a debate — bull or bear argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateRound {
    pub id: i64,
    pub debate_id: i64,
    pub round_num: i64,
    pub position: String,
    pub agent_source: Option<String>,
    pub argument_text: String,
    pub evidence_refs: Option<String>,
    pub created_at: String,
}

/// Full debate view with rounds included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateView {
    #[serde(flatten)]
    pub debate: Debate,
    pub rounds: Vec<DebateRound>,
    pub round_count: usize,
}

impl Debate {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            topic: row.get(1)?,
            status: row.get(2)?,
            max_rounds: row.get(3)?,
            created_at: row.get(4)?,
            resolved_at: row.get(5)?,
            resolution_summary: row.get(6)?,
        })
    }
}

impl DebateRound {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            debate_id: row.get(1)?,
            round_num: row.get(2)?,
            position: row.get(3)?,
            agent_source: row.get(4)?,
            argument_text: row.get(5)?,
            evidence_refs: row.get(6)?,
            created_at: row.get(7)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS debates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            topic TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            max_rounds INTEGER NOT NULL DEFAULT 3,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            resolved_at TEXT,
            resolution_summary TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_debates_status ON debates(status);
        CREATE INDEX IF NOT EXISTS idx_debates_created ON debates(created_at);

        CREATE TABLE IF NOT EXISTS debate_rounds (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            debate_id INTEGER NOT NULL,
            round_num INTEGER NOT NULL,
            position TEXT NOT NULL,
            agent_source TEXT,
            argument_text TEXT NOT NULL,
            evidence_refs TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (debate_id) REFERENCES debates(id)
        );
        CREATE INDEX IF NOT EXISTS idx_debate_rounds_debate ON debate_rounds(debate_id);",
    )?;
    Ok(())
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS debates (
                id BIGSERIAL PRIMARY KEY,
                topic TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                max_rounds INTEGER NOT NULL DEFAULT 3,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                resolved_at TIMESTAMPTZ,
                resolution_summary TEXT
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_debates_status ON debates(status)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_debates_created ON debates(created_at)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS debate_rounds (
                id BIGSERIAL PRIMARY KEY,
                debate_id BIGINT NOT NULL,
                round_num INTEGER NOT NULL,
                position TEXT NOT NULL,
                agent_source TEXT,
                argument_text TEXT NOT NULL,
                evidence_refs TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                CONSTRAINT fk_debate_rounds_debate
                    FOREIGN KEY (debate_id) REFERENCES debates(id)
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_debate_rounds_debate ON debate_rounds(debate_id)",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub fn validate_status(value: &str) -> Result<()> {
    match value {
        "active" | "resolved" => Ok(()),
        _ => anyhow::bail!("invalid status '{}'. Valid: active, resolved", value),
    }
}

pub fn validate_position(value: &str) -> Result<()> {
    match value {
        "bull" | "bear" => Ok(()),
        _ => anyhow::bail!("invalid position '{}'. Valid: bull, bear", value),
    }
}

// ---------------------------------------------------------------------------
// CRUD — SQLite
// ---------------------------------------------------------------------------

fn start_debate(conn: &Connection, topic: &str, max_rounds: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO debates (topic, status, max_rounds) VALUES (?, 'active', ?)",
        params![topic, max_rounds],
    )?;
    Ok(conn.last_insert_rowid())
}

fn add_round(
    conn: &Connection,
    debate_id: i64,
    round_num: i64,
    position: &str,
    agent_source: Option<&str>,
    argument_text: &str,
    evidence_refs: Option<&str>,
) -> Result<i64> {
    validate_position(position)?;
    conn.execute(
        "INSERT INTO debate_rounds (debate_id, round_num, position, agent_source, argument_text, evidence_refs)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![debate_id, round_num, position, agent_source, argument_text, evidence_refs],
    )?;
    Ok(conn.last_insert_rowid())
}

fn resolve_debate(
    conn: &Connection,
    debate_id: i64,
    resolution_summary: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE debates SET status = 'resolved', resolved_at = datetime('now'), resolution_summary = ? WHERE id = ?",
        params![resolution_summary, debate_id],
    )?;
    Ok(())
}

fn get_debate(conn: &Connection, debate_id: i64) -> Result<Option<Debate>> {
    let mut stmt = conn.prepare(
        "SELECT id, topic, status, max_rounds, created_at, resolved_at, resolution_summary
         FROM debates WHERE id = ?",
    )?;
    let mut rows = stmt.query_map([debate_id], Debate::from_row)?;
    match rows.next() {
        Some(Ok(d)) => Ok(Some(d)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn get_rounds(conn: &Connection, debate_id: i64) -> Result<Vec<DebateRound>> {
    let mut stmt = conn.prepare(
        "SELECT id, debate_id, round_num, position, agent_source, argument_text, evidence_refs, created_at
         FROM debate_rounds WHERE debate_id = ? ORDER BY round_num, position",
    )?;
    let rows = stmt.query_map([debate_id], DebateRound::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn list_debates(
    conn: &Connection,
    status: Option<&str>,
    topic_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Debate>> {
    let mut query = String::from(
        "SELECT id, topic, status, max_rounds, created_at, resolved_at, resolution_summary
         FROM debates WHERE 1=1",
    );
    if let Some(s) = status {
        query.push_str(&format!(" AND status = '{}'", s.replace('\'', "''")));
    }
    if let Some(t) = topic_filter {
        query.push_str(&format!(
            " AND topic LIKE '%{}%'",
            t.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], Debate::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn get_debate_view(conn: &Connection, debate_id: i64) -> Result<Option<DebateView>> {
    let debate = match get_debate(conn, debate_id)? {
        Some(d) => d,
        None => return Ok(None),
    };
    let rounds = get_rounds(conn, debate_id)?;
    let round_count = rounds.len();
    Ok(Some(DebateView {
        debate,
        rounds,
        round_count,
    }))
}

// ---------------------------------------------------------------------------
// CRUD — PostgreSQL
// ---------------------------------------------------------------------------

type DebatePgRow = (i64, String, String, i64, String, Option<String>, Option<String>);

fn debate_from_pg(r: DebatePgRow) -> Debate {
    Debate {
        id: r.0,
        topic: r.1,
        status: r.2,
        max_rounds: r.3,
        created_at: r.4,
        resolved_at: r.5,
        resolution_summary: r.6,
    }
}

type RoundPgRow = (i64, i64, i64, String, Option<String>, String, Option<String>, String);

fn round_from_pg(r: RoundPgRow) -> DebateRound {
    DebateRound {
        id: r.0,
        debate_id: r.1,
        round_num: r.2,
        position: r.3,
        agent_source: r.4,
        argument_text: r.5,
        evidence_refs: r.6,
        created_at: r.7,
    }
}

fn start_debate_postgres(pool: &PgPool, topic: &str, max_rounds: i64) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO debates (topic, status, max_rounds)
             VALUES ($1, 'active', $2) RETURNING id",
        )
        .bind(topic)
        .bind(max_rounds)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn add_round_postgres(
    pool: &PgPool,
    debate_id: i64,
    round_num: i64,
    position: &str,
    agent_source: Option<&str>,
    argument_text: &str,
    evidence_refs: Option<&str>,
) -> Result<i64> {
    validate_position(position)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO debate_rounds (debate_id, round_num, position, agent_source, argument_text, evidence_refs)
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(debate_id)
        .bind(round_num)
        .bind(position)
        .bind(agent_source)
        .bind(argument_text)
        .bind(evidence_refs)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn resolve_debate_postgres(
    pool: &PgPool,
    debate_id: i64,
    resolution_summary: Option<&str>,
) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "UPDATE debates SET status = 'resolved', resolved_at = NOW(), resolution_summary = $1 WHERE id = $2",
        )
        .bind(resolution_summary)
        .bind(debate_id)
        .execute(pool)
        .await
    })?;
    Ok(())
}

fn get_debate_postgres(pool: &PgPool, debate_id: i64) -> Result<Option<Debate>> {
    let row: Option<DebatePgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, topic, status, max_rounds::bigint, created_at::text, resolved_at::text, resolution_summary
             FROM debates WHERE id = $1",
        )
        .bind(debate_id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(debate_from_pg))
}

fn get_rounds_postgres(pool: &PgPool, debate_id: i64) -> Result<Vec<DebateRound>> {
    let rows: Vec<RoundPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, debate_id, round_num::bigint, position, agent_source, argument_text, evidence_refs, created_at::text
             FROM debate_rounds WHERE debate_id = $1 ORDER BY round_num, position",
        )
        .bind(debate_id)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(round_from_pg).collect())
}

fn list_debates_postgres(
    pool: &PgPool,
    status: Option<&str>,
    topic_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Debate>> {
    let mut query = String::from(
        "SELECT id, topic, status, max_rounds::bigint, created_at::text, resolved_at::text, resolution_summary
         FROM debates WHERE 1=1",
    );
    if let Some(s) = status {
        query.push_str(&format!(" AND status = '{}'", s.replace('\'', "''")));
    }
    if let Some(t) = topic_filter {
        query.push_str(&format!(
            " AND topic ILIKE '%{}%'",
            t.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY created_at DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let rows: Vec<DebatePgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&query).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(debate_from_pg).collect())
}

fn get_debate_view_postgres(pool: &PgPool, debate_id: i64) -> Result<Option<DebateView>> {
    let debate = match get_debate_postgres(pool, debate_id)? {
        Some(d) => d,
        None => return Ok(None),
    };
    let rounds = get_rounds_postgres(pool, debate_id)?;
    let round_count = rounds.len();
    Ok(Some(DebateView {
        debate,
        rounds,
        round_count,
    }))
}

// ---------------------------------------------------------------------------
// Backend dispatch
// ---------------------------------------------------------------------------

pub fn start_debate_backend(
    backend: &BackendConnection,
    topic: &str,
    max_rounds: i64,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            start_debate(conn, topic, max_rounds)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            start_debate_postgres(pool, topic, max_rounds)
        },
    )
}

pub fn add_round_backend(
    backend: &BackendConnection,
    debate_id: i64,
    round_num: i64,
    position: &str,
    agent_source: Option<&str>,
    argument_text: &str,
    evidence_refs: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            add_round(
                conn,
                debate_id,
                round_num,
                position,
                agent_source,
                argument_text,
                evidence_refs,
            )
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            add_round_postgres(
                pool,
                debate_id,
                round_num,
                position,
                agent_source,
                argument_text,
                evidence_refs,
            )
        },
    )
}

pub fn resolve_debate_backend(
    backend: &BackendConnection,
    debate_id: i64,
    resolution_summary: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            resolve_debate(conn, debate_id, resolution_summary)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            resolve_debate_postgres(pool, debate_id, resolution_summary)
        },
    )
}

pub fn get_debate_view_backend(
    backend: &BackendConnection,
    debate_id: i64,
) -> Result<Option<DebateView>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            get_debate_view(conn, debate_id)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            get_debate_view_postgres(pool, debate_id)
        },
    )
}

pub fn list_debates_backend(
    backend: &BackendConnection,
    status: Option<&str>,
    topic_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Debate>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            list_debates(conn, status, topic_filter, limit)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            list_debates_postgres(pool, status, topic_filter, limit)
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
        ensure_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_tables() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='debates'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='debate_rounds'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_start_and_get_debate() {
        let conn = setup_db();
        let id = start_debate(&conn, "Is BTC going to 200k this cycle?", 3).unwrap();
        assert!(id > 0);

        let debate = get_debate(&conn, id).unwrap().unwrap();
        assert_eq!(debate.topic, "Is BTC going to 200k this cycle?");
        assert_eq!(debate.status, "active");
        assert_eq!(debate.max_rounds, 3);
        assert!(debate.resolved_at.is_none());
    }

    #[test]
    fn test_add_rounds() {
        let conn = setup_db();
        let debate_id = start_debate(&conn, "Gold to 5000?", 2).unwrap();

        let r1 = add_round(
            &conn,
            debate_id,
            1,
            "bull",
            Some("high-agent"),
            "Central bank buying is structural and accelerating.",
            Some("WGC Q4 2025 report, PBOC reserves data"),
        )
        .unwrap();
        assert!(r1 > 0);

        let r2 = add_round(
            &conn,
            debate_id,
            1,
            "bear",
            Some("medium-agent"),
            "Gold is already priced for perfection. Any risk-on shift sends it down 10%.",
            Some("Gold/DXY correlation, positioning data"),
        )
        .unwrap();
        assert!(r2 > 0);

        let rounds = get_rounds(&conn, debate_id).unwrap();
        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].position, "bear"); // ordered by round_num, then position alphabetically
        assert_eq!(rounds[1].position, "bull");
    }

    #[test]
    fn test_resolve_debate() {
        let conn = setup_db();
        let debate_id = start_debate(&conn, "US recession in 2026?", 3).unwrap();

        resolve_debate(
            &conn,
            debate_id,
            Some("Bear case stronger — leading indicators deteriorating."),
        )
        .unwrap();

        let debate = get_debate(&conn, debate_id).unwrap().unwrap();
        assert_eq!(debate.status, "resolved");
        assert!(debate.resolved_at.is_some());
        assert_eq!(
            debate.resolution_summary.as_deref(),
            Some("Bear case stronger — leading indicators deteriorating.")
        );
    }

    #[test]
    fn test_list_debates_filter() {
        let conn = setup_db();
        start_debate(&conn, "BTC super cycle", 3).unwrap();
        let id2 = start_debate(&conn, "Gold correction imminent?", 2).unwrap();
        start_debate(&conn, "Silver squeeze potential", 3).unwrap();

        resolve_debate(&conn, id2, Some("Gold held support")).unwrap();

        let all = list_debates(&conn, None, None, None).unwrap();
        assert_eq!(all.len(), 3);

        let active = list_debates(&conn, Some("active"), None, None).unwrap();
        assert_eq!(active.len(), 2);

        let resolved = list_debates(&conn, Some("resolved"), None, None).unwrap();
        assert_eq!(resolved.len(), 1);

        let gold = list_debates(&conn, None, Some("Gold"), None).unwrap();
        assert_eq!(gold.len(), 1);

        let limited = list_debates(&conn, None, None, Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_debate_view() {
        let conn = setup_db();
        let debate_id = start_debate(&conn, "TSLA generational buy?", 2).unwrap();
        add_round(&conn, debate_id, 1, "bull", None, "Autonomous driving + energy + AI.", None)
            .unwrap();
        add_round(&conn, debate_id, 1, "bear", None, "Valuation insane, competition rising.", None)
            .unwrap();
        add_round(
            &conn,
            debate_id,
            2,
            "bull",
            None,
            "FSD v13 changes everything.",
            Some("Tesla AI day"),
        )
        .unwrap();
        add_round(
            &conn,
            debate_id,
            2,
            "bear",
            None,
            "Musk distracted, brand damaged.",
            Some("Brand sentiment surveys"),
        )
        .unwrap();

        let view = get_debate_view(&conn, debate_id).unwrap().unwrap();
        assert_eq!(view.round_count, 4);
        assert_eq!(view.rounds.len(), 4);
        assert_eq!(view.debate.topic, "TSLA generational buy?");
    }

    #[test]
    fn test_invalid_position() {
        let conn = setup_db();
        let debate_id = start_debate(&conn, "Test", 1).unwrap();
        let result = add_round(&conn, debate_id, 1, "neutral", None, "test", None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid position")
        );
    }

    #[test]
    fn test_validate_status() {
        assert!(validate_status("active").is_ok());
        assert!(validate_status("resolved").is_ok());
        assert!(validate_status("pending").is_err());
    }

    #[test]
    fn test_nonexistent_debate() {
        let conn = setup_db();
        let result = get_debate(&conn, 999).unwrap();
        assert!(result.is_none());

        let view = get_debate_view(&conn, 999).unwrap();
        assert!(view.is_none());
    }
}
