//! Analyst views storage — structured per-analyst, per-asset directional views
//! with conviction scores. Each timeframe analyst (LOW/MEDIUM/HIGH/MACRO) writes
//! a view per asset on every run.
//!
//! Table: `analyst_views`
//!   - analyst: low | medium | high | macro
//!   - asset: symbol (e.g. BTC, GLD, TSLA)
//!   - direction: bull | bear | neutral
//!   - conviction: -5 to +5 (negative = bearish conviction, positive = bullish)
//!   - reasoning_summary: why this view
//!   - key_evidence: supporting data points
//!   - blind_spots: what could invalidate this view
//!   - updated_at: auto-managed

use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::db::backend::BackendConnection;
use crate::db::query;

/// A single analyst's view on an asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystView {
    pub id: i64,
    pub analyst: String,
    pub asset: String,
    pub direction: String,
    pub conviction: i64,
    pub reasoning_summary: String,
    pub key_evidence: Option<String>,
    pub blind_spots: Option<String>,
    pub updated_at: String,
}

/// Matrix row: all analyst views for one asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetViewMatrix {
    pub asset: String,
    pub views: Vec<AnalystView>,
}

impl AnalystView {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            analyst: row.get(1)?,
            asset: row.get(2)?,
            direction: row.get(3)?,
            conviction: row.get(4)?,
            reasoning_summary: row.get(5)?,
            key_evidence: row.get(6)?,
            blind_spots: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

fn ensure_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS analyst_views (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            analyst TEXT NOT NULL,
            asset TEXT NOT NULL,
            direction TEXT NOT NULL DEFAULT 'neutral',
            conviction INTEGER NOT NULL DEFAULT 0,
            reasoning_summary TEXT NOT NULL,
            key_evidence TEXT,
            blind_spots TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_analyst_views_analyst_asset
            ON analyst_views(analyst, asset);
        CREATE INDEX IF NOT EXISTS idx_analyst_views_asset
            ON analyst_views(asset);
        CREATE INDEX IF NOT EXISTS idx_analyst_views_updated
            ON analyst_views(updated_at);",
    )?;
    Ok(())
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS analyst_views (
                id BIGSERIAL PRIMARY KEY,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL DEFAULT 'neutral',
                conviction INTEGER NOT NULL DEFAULT 0,
                reasoning_summary TEXT NOT NULL,
                key_evidence TEXT,
                blind_spots TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_analyst_views_analyst_asset
             ON analyst_views(analyst, asset)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_analyst_views_asset
             ON analyst_views(asset)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_analyst_views_updated
             ON analyst_views(updated_at)",
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

pub fn validate_analyst(value: &str) -> Result<()> {
    match value {
        "low" | "medium" | "high" | "macro" => Ok(()),
        _ => anyhow::bail!(
            "invalid analyst '{}'. Valid: low, medium, high, macro",
            value
        ),
    }
}

pub fn validate_direction(value: &str) -> Result<()> {
    match value {
        "bull" | "bear" | "neutral" => Ok(()),
        _ => anyhow::bail!(
            "invalid direction '{}'. Valid: bull, bear, neutral",
            value
        ),
    }
}

pub fn validate_conviction(value: i64) -> Result<()> {
    if !(-5..=5).contains(&value) {
        anyhow::bail!(
            "conviction {} out of range. Valid: -5 to +5",
            value
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CRUD — SQLite
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn upsert_view(
    conn: &Connection,
    analyst: &str,
    asset: &str,
    direction: &str,
    conviction: i64,
    reasoning_summary: &str,
    key_evidence: Option<&str>,
    blind_spots: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO analyst_views (analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(analyst, asset) DO UPDATE SET
            direction = excluded.direction,
            conviction = excluded.conviction,
            reasoning_summary = excluded.reasoning_summary,
            key_evidence = excluded.key_evidence,
            blind_spots = excluded.blind_spots,
            updated_at = datetime('now')",
        params![analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots],
    )?;
    // Get the id (could be new or existing)
    let id: i64 = conn.query_row(
        "SELECT id FROM analyst_views WHERE analyst = ? AND asset = ?",
        params![analyst, asset],
        |row| row.get(0),
    )?;
    Ok(id)
}

fn get_view(conn: &Connection, analyst: &str, asset: &str) -> Result<Option<AnalystView>> {
    let mut stmt = conn.prepare(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, updated_at
         FROM analyst_views WHERE analyst = ? AND asset = ?",
    )?;
    let mut rows = stmt.query_map(params![analyst, asset], AnalystView::from_row)?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

fn list_views(
    conn: &Connection,
    analyst: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystView>> {
    let mut query = String::from(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, updated_at
         FROM analyst_views WHERE 1=1",
    );
    if let Some(a) = analyst {
        query.push_str(&format!(" AND analyst = '{}'", a.replace('\'', "''")));
    }
    if let Some(s) = asset {
        query.push_str(&format!(
            " AND UPPER(asset) = UPPER('{}')",
            s.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY asset, analyst");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], AnalystView::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn list_assets_with_views(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT asset FROM analyst_views ORDER BY asset",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

fn get_view_matrix(conn: &Connection) -> Result<Vec<AssetViewMatrix>> {
    let assets = list_assets_with_views(conn)?;
    let mut matrix = Vec::new();
    for asset in assets {
        let views = list_views(conn, None, Some(&asset), None)?;
        matrix.push(AssetViewMatrix {
            asset,
            views,
        });
    }
    Ok(matrix)
}

fn delete_view(conn: &Connection, analyst: &str, asset: &str) -> Result<bool> {
    let affected = conn.execute(
        "DELETE FROM analyst_views WHERE analyst = ? AND asset = ?",
        params![analyst, asset],
    )?;
    Ok(affected > 0)
}

// ---------------------------------------------------------------------------
// CRUD — PostgreSQL
// ---------------------------------------------------------------------------

type ViewPgRow = (
    i64,
    String,
    String,
    String,
    i32,
    String,
    Option<String>,
    Option<String>,
    String,
);

fn view_from_pg(r: ViewPgRow) -> AnalystView {
    AnalystView {
        id: r.0,
        analyst: r.1,
        asset: r.2,
        direction: r.3,
        conviction: r.4 as i64,
        reasoning_summary: r.5,
        key_evidence: r.6,
        blind_spots: r.7,
        updated_at: r.8,
    }
}

#[allow(clippy::too_many_arguments)]
fn upsert_view_postgres(
    pool: &PgPool,
    analyst: &str,
    asset: &str,
    direction: &str,
    conviction: i64,
    reasoning_summary: &str,
    key_evidence: Option<&str>,
    blind_spots: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO analyst_views (analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT(analyst, asset) DO UPDATE SET
                direction = EXCLUDED.direction,
                conviction = EXCLUDED.conviction,
                reasoning_summary = EXCLUDED.reasoning_summary,
                key_evidence = EXCLUDED.key_evidence,
                blind_spots = EXCLUDED.blind_spots,
                updated_at = NOW()
             RETURNING id",
        )
        .bind(analyst)
        .bind(asset)
        .bind(direction)
        .bind(conviction)
        .bind(reasoning_summary)
        .bind(key_evidence)
        .bind(blind_spots)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn get_view_postgres(pool: &PgPool, analyst: &str, asset: &str) -> Result<Option<AnalystView>> {
    let row: Option<ViewPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, updated_at::text
             FROM analyst_views WHERE analyst = $1 AND asset = $2",
        )
        .bind(analyst)
        .bind(asset)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(view_from_pg))
}

fn list_views_postgres(
    pool: &PgPool,
    analyst: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystView>> {
    let mut query = String::from(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, updated_at::text
         FROM analyst_views WHERE 1=1",
    );
    if let Some(a) = analyst {
        query.push_str(&format!(" AND analyst = '{}'", a.replace('\'', "''")));
    }
    if let Some(s) = asset {
        query.push_str(&format!(
            " AND UPPER(asset) = UPPER('{}')",
            s.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY asset, analyst");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let rows: Vec<ViewPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&query).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(view_from_pg).collect())
}

fn list_assets_with_views_postgres(pool: &PgPool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as("SELECT DISTINCT asset FROM analyst_views ORDER BY asset")
            .fetch_all(pool)
            .await
    })?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

fn get_view_matrix_postgres(pool: &PgPool) -> Result<Vec<AssetViewMatrix>> {
    let assets = list_assets_with_views_postgres(pool)?;
    let mut matrix = Vec::new();
    for asset in &assets {
        let views = list_views_postgres(pool, None, Some(asset), None)?;
        matrix.push(AssetViewMatrix {
            asset: asset.clone(),
            views,
        });
    }
    Ok(matrix)
}

fn delete_view_postgres(pool: &PgPool, analyst: &str, asset: &str) -> Result<bool> {
    let result = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM analyst_views WHERE analyst = $1 AND asset = $2")
            .bind(analyst)
            .bind(asset)
            .execute(pool)
            .await
    })?;
    Ok(result.rows_affected() > 0)
}

// ---------------------------------------------------------------------------
// Backend dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn upsert_view_backend(
    backend: &BackendConnection,
    analyst: &str,
    asset: &str,
    direction: &str,
    conviction: i64,
    reasoning_summary: &str,
    key_evidence: Option<&str>,
    blind_spots: Option<&str>,
) -> Result<i64> {
    validate_analyst(analyst)?;
    validate_direction(direction)?;
    validate_conviction(conviction)?;
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            upsert_view(conn, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            upsert_view_postgres(pool, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots)
        },
    )
}

pub fn get_view_backend(
    backend: &BackendConnection,
    analyst: &str,
    asset: &str,
) -> Result<Option<AnalystView>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            get_view(conn, analyst, asset)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            get_view_postgres(pool, analyst, asset)
        },
    )
}

pub fn list_views_backend(
    backend: &BackendConnection,
    analyst: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystView>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            list_views(conn, analyst, asset, limit)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            list_views_postgres(pool, analyst, asset, limit)
        },
    )
}

pub fn get_view_matrix_backend(
    backend: &BackendConnection,
) -> Result<Vec<AssetViewMatrix>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            get_view_matrix(conn)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            get_view_matrix_postgres(pool)
        },
    )
}

pub fn delete_view_backend(
    backend: &BackendConnection,
    analyst: &str,
    asset: &str,
) -> Result<bool> {
    validate_analyst(analyst)?;
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            delete_view(conn, analyst, asset)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            delete_view_postgres(pool, analyst, asset)
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
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='analyst_views'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_upsert_and_get_view() {
        let conn = setup_db();
        let id = upsert_view(
            &conn,
            "low",
            "BTC",
            "bull",
            3,
            "Short-term momentum strong, breaking resistance.",
            Some("RSI 62, MACD cross bullish, volume surge"),
            Some("Whale selling could cap upside"),
        )
        .unwrap();
        assert!(id > 0);

        let view = get_view(&conn, "low", "BTC").unwrap().unwrap();
        assert_eq!(view.analyst, "low");
        assert_eq!(view.asset, "BTC");
        assert_eq!(view.direction, "bull");
        assert_eq!(view.conviction, 3);
        assert_eq!(
            view.reasoning_summary,
            "Short-term momentum strong, breaking resistance."
        );
        assert_eq!(
            view.key_evidence.as_deref(),
            Some("RSI 62, MACD cross bullish, volume surge")
        );
        assert_eq!(
            view.blind_spots.as_deref(),
            Some("Whale selling could cap upside")
        );
    }

    #[test]
    fn test_upsert_updates_existing() {
        let conn = setup_db();
        upsert_view(
            &conn,
            "high",
            "GLD",
            "bull",
            4,
            "Structural central bank buying.",
            None,
            None,
        )
        .unwrap();

        // Update the same analyst+asset
        upsert_view(
            &conn,
            "high",
            "GLD",
            "neutral",
            1,
            "Central bank buying slowing. Mixed signals.",
            Some("PBOC paused, WGC data"),
            Some("Could resume if DXY weakens"),
        )
        .unwrap();

        let views = list_views(&conn, Some("high"), Some("GLD"), None).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].direction, "neutral");
        assert_eq!(views[0].conviction, 1);
        assert_eq!(
            views[0].reasoning_summary,
            "Central bank buying slowing. Mixed signals."
        );
    }

    #[test]
    fn test_list_views_filters() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None).unwrap();
        upsert_view(&conn, "medium", "BTC", "bull", 2, "Swing bullish", None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Valuation stretched", None, None).unwrap();
        upsert_view(&conn, "low", "GLD", "bull", 4, "Gold momentum", None, None).unwrap();

        // All views
        let all = list_views(&conn, None, None, None).unwrap();
        assert_eq!(all.len(), 4);

        // Filter by analyst
        let low_views = list_views(&conn, Some("low"), None, None).unwrap();
        assert_eq!(low_views.len(), 2);

        // Filter by asset
        let btc_views = list_views(&conn, None, Some("BTC"), None).unwrap();
        assert_eq!(btc_views.len(), 3);

        // Filter by both
        let specific = list_views(&conn, Some("high"), Some("BTC"), None).unwrap();
        assert_eq!(specific.len(), 1);
        assert_eq!(specific[0].direction, "bear");

        // Limit
        let limited = list_views(&conn, None, None, Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_view_matrix() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Overvalued", None, None).unwrap();
        upsert_view(&conn, "low", "GLD", "bull", 4, "Safe haven bid", None, None).unwrap();
        upsert_view(&conn, "macro", "GLD", "bull", 5, "Structural", None, None).unwrap();

        let matrix = get_view_matrix(&conn).unwrap();
        assert_eq!(matrix.len(), 2); // BTC and GLD

        let btc = matrix.iter().find(|m| m.asset == "BTC").unwrap();
        assert_eq!(btc.views.len(), 2);

        let gld = matrix.iter().find(|m| m.asset == "GLD").unwrap();
        assert_eq!(gld.views.len(), 2);
    }

    #[test]
    fn test_delete_view() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Test", None, None).unwrap();
        assert!(get_view(&conn, "low", "BTC").unwrap().is_some());

        let deleted = delete_view(&conn, "low", "BTC").unwrap();
        assert!(deleted);
        assert!(get_view(&conn, "low", "BTC").unwrap().is_none());

        // Delete non-existent
        let deleted_again = delete_view(&conn, "low", "BTC").unwrap();
        assert!(!deleted_again);
    }

    #[test]
    fn test_validate_analyst() {
        assert!(validate_analyst("low").is_ok());
        assert!(validate_analyst("medium").is_ok());
        assert!(validate_analyst("high").is_ok());
        assert!(validate_analyst("macro").is_ok());
        assert!(validate_analyst("ultra").is_err());
        assert!(validate_analyst("LOW").is_err()); // case-sensitive
    }

    #[test]
    fn test_validate_direction() {
        assert!(validate_direction("bull").is_ok());
        assert!(validate_direction("bear").is_ok());
        assert!(validate_direction("neutral").is_ok());
        assert!(validate_direction("sideways").is_err());
    }

    #[test]
    fn test_validate_conviction() {
        assert!(validate_conviction(0).is_ok());
        assert!(validate_conviction(5).is_ok());
        assert!(validate_conviction(-5).is_ok());
        assert!(validate_conviction(6).is_err());
        assert!(validate_conviction(-6).is_err());
    }

    #[test]
    fn test_case_insensitive_asset_filter() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Test", None, None).unwrap();

        let btc_lower = list_views(&conn, None, Some("btc"), None).unwrap();
        assert_eq!(btc_lower.len(), 1);

        let btc_upper = list_views(&conn, None, Some("BTC"), None).unwrap();
        assert_eq!(btc_upper.len(), 1);
    }

    #[test]
    fn test_nonexistent_view() {
        let conn = setup_db();
        let result = get_view(&conn, "low", "DOESNOTEXIST").unwrap();
        assert!(result.is_none());
    }
}
