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
    pub allocation_bias: Option<String>,
    pub updated_at: String,
}

/// Matrix row: all analyst views for one asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetViewMatrix {
    pub asset: String,
    pub views: Vec<AnalystView>,
}

/// A historical snapshot of an analyst view (immutable append-only log).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystViewHistoryEntry {
    pub id: i64,
    pub analyst: String,
    pub asset: String,
    pub direction: String,
    pub conviction: i64,
    pub reasoning_summary: String,
    pub key_evidence: Option<String>,
    pub blind_spots: Option<String>,
    pub allocation_bias: Option<String>,
    pub recorded_at: String,
}

impl AnalystViewHistoryEntry {
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
            allocation_bias: row.get(8)?,
            recorded_at: row.get(9)?,
        })
    }
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
            allocation_bias: row.get(8)?,
            updated_at: row.get(9)?,
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
            allocation_bias TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_analyst_views_analyst_asset
            ON analyst_views(analyst, asset);
        CREATE INDEX IF NOT EXISTS idx_analyst_views_asset
            ON analyst_views(asset);
        CREATE INDEX IF NOT EXISTS idx_analyst_views_updated
            ON analyst_views(updated_at);
        CREATE TABLE IF NOT EXISTS analyst_view_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            analyst TEXT NOT NULL,
            asset TEXT NOT NULL,
            direction TEXT NOT NULL,
            conviction INTEGER NOT NULL,
            reasoning_summary TEXT NOT NULL,
            key_evidence TEXT,
            blind_spots TEXT,
            allocation_bias TEXT,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_avh_analyst_asset
            ON analyst_view_history(analyst, asset);
        CREATE INDEX IF NOT EXISTS idx_avh_asset
            ON analyst_view_history(asset);
        CREATE INDEX IF NOT EXISTS idx_avh_recorded
            ON analyst_view_history(recorded_at);",
    )?;

    // Idempotent migration: add allocation_bias to legacy tables that pre-date the column.
    let views_has_bias: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('analyst_views') WHERE name = 'allocation_bias'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !views_has_bias {
        conn.execute_batch("ALTER TABLE analyst_views ADD COLUMN allocation_bias TEXT")?;
    }
    let history_has_bias: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('analyst_view_history') WHERE name = 'allocation_bias'")?
        .query_row([], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0;
    if !history_has_bias {
        conn.execute_batch(
            "ALTER TABLE analyst_view_history ADD COLUMN allocation_bias TEXT",
        )?;
    }

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
                allocation_bias TEXT,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        // Idempotent migration for upgraded databases.
        sqlx::query(
            "ALTER TABLE analyst_views ADD COLUMN IF NOT EXISTS allocation_bias TEXT",
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
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS analyst_view_history (
                id BIGSERIAL PRIMARY KEY,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL,
                key_evidence TEXT,
                blind_spots TEXT,
                allocation_bias TEXT,
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .execute(pool)
        .await?;
        // Idempotent migration for upgraded databases.
        sqlx::query(
            "ALTER TABLE analyst_view_history ADD COLUMN IF NOT EXISTS allocation_bias TEXT",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_avh_analyst_asset
             ON analyst_view_history(analyst, asset)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_avh_asset
             ON analyst_view_history(asset)",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_avh_recorded
             ON analyst_view_history(recorded_at)",
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

/// Direction-authoritative effective conviction: bear views are always
/// negative, bull views always positive, neutral views keep their stored
/// value. Used both at write time (normalization) and at read time as a
/// defense against legacy rows written with a contradicting sign.
pub fn effective_conviction(direction: &str, conviction: i64) -> i64 {
    match direction {
        "bear" => -conviction.abs(),
        "bull" => conviction.abs(),
        _ => conviction,
    }
}

/// Normalize a (direction, conviction) pair at write time. Direction is
/// authoritative: a sign that contradicts the direction is flipped and an
/// informational notice is returned (e.g. `bear` with `+3` becomes `-3`,
/// `bull` with `-3` becomes `+3`). Matching signs and neutral views pass
/// through unchanged with no notice.
pub fn normalize_conviction(direction: &str, conviction: i64) -> (i64, Option<String>) {
    let normalized = effective_conviction(direction, conviction);
    if normalized != conviction {
        let notice = format!(
            "conviction {:+} contradicts direction '{}'; normalized to {:+} (direction is authoritative)",
            conviction, direction, normalized
        );
        (normalized, Some(notice))
    } else {
        (normalized, None)
    }
}

/// Validate an `allocation_bias` value. `None` (i.e. no bias supplied) is allowed.
pub fn validate_allocation_bias(value: Option<&str>) -> Result<()> {
    match value {
        None => Ok(()),
        Some(v) => match v {
            "overweight"
            | "slight-overweight"
            | "at-target"
            | "slight-underweight"
            | "underweight" => Ok(()),
            _ => anyhow::bail!(
                "invalid allocation_bias '{}'. Valid: overweight, slight-overweight, at-target, slight-underweight, underweight",
                v
            ),
        },
    }
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
    allocation_bias: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO analyst_views (analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(analyst, asset) DO UPDATE SET
            direction = excluded.direction,
            conviction = excluded.conviction,
            reasoning_summary = excluded.reasoning_summary,
            key_evidence = excluded.key_evidence,
            blind_spots = excluded.blind_spots,
            allocation_bias = excluded.allocation_bias,
            updated_at = datetime('now')",
        params![analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias],
    )?;
    // Also append to history log
    conn.execute(
        "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias],
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
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, updated_at
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
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, updated_at
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

fn get_portfolio_view_matrix(
    conn: &Connection,
    portfolio_symbols: &[String],
) -> Result<Vec<AssetViewMatrix>> {
    // Merge portfolio symbols with any assets that already have views
    let mut all_assets: Vec<String> = portfolio_symbols.to_vec();
    let viewed = list_assets_with_views(conn)?;
    for a in viewed {
        let upper = a.to_uppercase();
        if !all_assets.iter().any(|s| s.to_uppercase() == upper) {
            all_assets.push(a);
        }
    }
    all_assets.sort_by_key(|a| a.to_uppercase());
    all_assets.dedup_by(|a, b| a.to_uppercase() == b.to_uppercase());

    let mut matrix = Vec::new();
    for asset in all_assets {
        let views = list_views(conn, None, Some(&asset), None)?;
        matrix.push(AssetViewMatrix { asset, views });
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

fn get_view_history(
    conn: &Connection,
    asset: &str,
    analyst: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystViewHistoryEntry>> {
    let mut query = String::from(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, recorded_at
         FROM analyst_view_history WHERE UPPER(asset) = UPPER(?)",
    );
    if let Some(a) = analyst {
        query.push_str(&format!(" AND analyst = '{}'", a.replace('\'', "''")));
    }
    query.push_str(" ORDER BY recorded_at DESC, id DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params![asset], AnalystViewHistoryEntry::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
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
        allocation_bias: r.8,
        updated_at: r.9,
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
    allocation_bias: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO analyst_views (analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT(analyst, asset) DO UPDATE SET
                direction = EXCLUDED.direction,
                conviction = EXCLUDED.conviction,
                reasoning_summary = EXCLUDED.reasoning_summary,
                key_evidence = EXCLUDED.key_evidence,
                blind_spots = EXCLUDED.blind_spots,
                allocation_bias = EXCLUDED.allocation_bias,
                updated_at = NOW()
             RETURNING id",
        )
        .bind(analyst)
        .bind(asset)
        .bind(direction)
        .bind(conviction as i32)
        .bind(reasoning_summary)
        .bind(key_evidence)
        .bind(blind_spots)
        .bind(allocation_bias)
        .fetch_one(pool)
        .await?;
        // Also append to history log
        sqlx::query(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(analyst)
        .bind(asset)
        .bind(direction)
        .bind(conviction as i32)
        .bind(reasoning_summary)
        .bind(key_evidence)
        .bind(blind_spots)
        .bind(allocation_bias)
        .execute(pool)
        .await?;
        Ok::<i64, sqlx::Error>(id)
    })?;
    Ok(id)
}

fn get_view_postgres(pool: &PgPool, analyst: &str, asset: &str) -> Result<Option<AnalystView>> {
    let row: Option<ViewPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, updated_at::text
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
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, updated_at::text
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

fn get_portfolio_view_matrix_postgres(
    pool: &PgPool,
    portfolio_symbols: &[String],
) -> Result<Vec<AssetViewMatrix>> {
    let mut all_assets: Vec<String> = portfolio_symbols.to_vec();
    let viewed = list_assets_with_views_postgres(pool)?;
    for a in viewed {
        let upper = a.to_uppercase();
        if !all_assets.iter().any(|s| s.to_uppercase() == upper) {
            all_assets.push(a);
        }
    }
    all_assets.sort_by_key(|a| a.to_uppercase());
    all_assets.dedup_by(|a, b| a.to_uppercase() == b.to_uppercase());

    let mut matrix = Vec::new();
    for asset in &all_assets {
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

type HistoryPgRow = (
    i64,
    String,
    String,
    String,
    i32,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

fn history_from_pg(r: HistoryPgRow) -> AnalystViewHistoryEntry {
    AnalystViewHistoryEntry {
        id: r.0,
        analyst: r.1,
        asset: r.2,
        direction: r.3,
        conviction: r.4 as i64,
        reasoning_summary: r.5,
        key_evidence: r.6,
        blind_spots: r.7,
        allocation_bias: r.8,
        recorded_at: r.9,
    }
}

fn get_view_history_postgres(
    pool: &PgPool,
    asset: &str,
    analyst: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystViewHistoryEntry>> {
    let mut query = String::from(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, recorded_at::text
         FROM analyst_view_history WHERE UPPER(asset) = UPPER($1)",
    );
    if let Some(a) = analyst {
        query.push_str(&format!(" AND analyst = '{}'", a.replace('\'', "''")));
    }
    query.push_str(" ORDER BY recorded_at DESC, id DESC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let rows: Vec<HistoryPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&query).bind(asset).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(history_from_pg).collect())
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
    allocation_bias: Option<&str>,
) -> Result<i64> {
    validate_analyst(analyst)?;
    validate_direction(direction)?;
    validate_conviction(conviction)?;
    validate_allocation_bias(allocation_bias)?;
    // Direction is authoritative: store bear views with negative conviction
    // and bull views with positive conviction regardless of the sign passed.
    let (conviction, _notice) = normalize_conviction(direction, conviction);
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            upsert_view(conn, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            upsert_view_postgres(pool, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias)
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

pub fn get_portfolio_view_matrix_backend(
    backend: &BackendConnection,
    portfolio_symbols: &[String],
) -> Result<Vec<AssetViewMatrix>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            get_portfolio_view_matrix(conn, portfolio_symbols)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            get_portfolio_view_matrix_postgres(pool, portfolio_symbols)
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

pub fn get_view_history_backend(
    backend: &BackendConnection,
    asset: &str,
    analyst: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystViewHistoryEntry>> {
    if let Some(a) = analyst {
        validate_analyst(a)?;
    }
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            get_view_history(conn, asset, analyst, limit)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            get_view_history_postgres(pool, asset, analyst, limit)
        },
    )
}

/// A divergence record: one asset where analysts disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewDivergence {
    pub asset: String,
    /// Absolute spread between most-bullish and most-bearish conviction
    pub spread: i64,
    /// The most bullish view
    pub most_bullish: AnalystView,
    /// The most bearish view
    pub most_bearish: AnalystView,
    /// All views for this asset (for context)
    pub all_views: Vec<AnalystView>,
}

// ---------------------------------------------------------------------------
// Divergence — SQLite
// ---------------------------------------------------------------------------

fn compute_divergence(
    conn: &Connection,
    min_spread: i64,
    asset_filter: Option<&str>,
    analyst_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ViewDivergence>> {
    // Get the full matrix (or filtered to one asset)
    let matrix = if let Some(asset) = asset_filter {
        let views = list_views(conn, None, Some(asset), None)?;
        if views.is_empty() {
            return Ok(Vec::new());
        }
        vec![AssetViewMatrix {
            asset: asset.to_uppercase(),
            views,
        }]
    } else {
        get_view_matrix(conn)?
    };

    let mut divergences = divergences_from_matrix(matrix, min_spread, analyst_filter);
    if let Some(n) = limit {
        divergences.truncate(n);
    }
    Ok(divergences)
}

// ---------------------------------------------------------------------------
// Divergence — PostgreSQL
// ---------------------------------------------------------------------------

fn compute_divergence_postgres(
    pool: &PgPool,
    min_spread: i64,
    asset_filter: Option<&str>,
    analyst_filter: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ViewDivergence>> {
    let matrix = if let Some(asset) = asset_filter {
        let views = list_views_postgres(pool, None, Some(asset), None)?;
        if views.is_empty() {
            return Ok(Vec::new());
        }
        vec![AssetViewMatrix {
            asset: asset.to_uppercase(),
            views,
        }]
    } else {
        get_view_matrix_postgres(pool)?
    };

    let mut divergences = divergences_from_matrix(matrix, min_spread, analyst_filter);
    if let Some(n) = limit {
        divergences.truncate(n);
    }
    Ok(divergences)
}

/// Shared logic: compute divergences from a matrix of views.
fn divergences_from_matrix(
    matrix: Vec<AssetViewMatrix>,
    min_spread: i64,
    analyst_filter: Option<&str>,
) -> Vec<ViewDivergence> {
    let mut divergences: Vec<ViewDivergence> = Vec::new();

    for row in matrix {
        if row.views.len() < 2 {
            continue; // need at least 2 analysts to have divergence
        }

        // Find most-bullish and most-bearish by conviction score
        let most_bullish = row
            .views
            .iter()
            .max_by_key(|v| v.conviction)
            .unwrap()
            .clone();
        let most_bearish = row
            .views
            .iter()
            .min_by_key(|v| v.conviction)
            .unwrap()
            .clone();

        let spread = most_bullish.conviction - most_bearish.conviction;
        let matches_analyst = analyst_filter.is_none_or(|analyst| {
            most_bullish.analyst == analyst || most_bearish.analyst == analyst
        });

        if spread >= min_spread && matches_analyst {
            divergences.push(ViewDivergence {
                asset: row.asset,
                spread,
                most_bullish,
                most_bearish,
                all_views: row.views,
            });
        }
    }

    // Sort by spread descending (biggest divergence first)
    divergences.sort_by_key(|b| std::cmp::Reverse(b.spread));
    divergences
}

// ---------------------------------------------------------------------------
// Divergence — backend dispatch
// ---------------------------------------------------------------------------

pub fn compute_divergence_backend(
    backend: &BackendConnection,
    min_spread: i64,
    asset: Option<&str>,
    analyst: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ViewDivergence>> {
    query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            compute_divergence(conn, min_spread, asset, analyst, limit)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            compute_divergence_postgres(pool, min_spread, asset, analyst, limit)
        },
    )
}

// ---------------------------------------------------------------------------
// Accuracy — per-analyst accuracy measurement against price outcomes
// ---------------------------------------------------------------------------

/// Evaluation window in days for each analyst timeframe.
fn eval_window_days(analyst: &str) -> i64 {
    match analyst {
        "low" => 3,
        "medium" => 14,
        "high" => 30,
        "macro" => 90,
        _ => 7, // fallback
    }
}

/// Format a date string offset by N days.  Expects `YYYY-MM-DD...` prefix.
fn date_plus_days(date_str: &str, days: i64) -> Option<String> {
    let date_part = date_str.get(..10)?;
    let parsed = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    let target = parsed + chrono::Duration::days(days);
    Some(target.format("%Y-%m-%d").to_string())
}

/// A single evaluated view call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatedCall {
    pub analyst: String,
    pub asset: String,
    pub direction: String,
    pub conviction: i64,
    pub recorded_at: String,
    pub entry_price: String,
    pub exit_price: String,
    pub price_change_pct: f64,
    pub correct: bool,
    pub eval_window_days: i64,
}

/// Per-analyst accuracy summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystAccuracy {
    pub analyst: String,
    pub total_calls: usize,
    pub evaluated: usize,
    pub correct: usize,
    pub incorrect: usize,
    pub neutral_skipped: usize,
    pub hit_rate_pct: f64,
    pub avg_conviction_correct: f64,
    pub avg_conviction_incorrect: f64,
    pub eval_window_days: i64,
    pub by_asset: Vec<AssetAccuracy>,
}

/// Per-analyst-per-asset accuracy breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetAccuracy {
    pub asset: String,
    pub evaluated: usize,
    pub correct: usize,
    pub incorrect: usize,
    pub hit_rate_pct: f64,
}

/// Full accuracy report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyReport {
    pub analysts: Vec<AnalystAccuracy>,
    pub total_history_entries: usize,
    pub total_evaluated: usize,
    pub total_correct: usize,
    pub overall_hit_rate_pct: f64,
    pub evaluated_calls: Vec<EvaluatedCall>,
}

/// Retrieve all view history entries (all analysts, all assets) — SQLite.
fn get_all_view_history(
    conn: &Connection,
    analyst: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystViewHistoryEntry>> {
    ensure_tables(conn)?;
    let mut query = String::from(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, recorded_at
         FROM analyst_view_history WHERE 1=1",
    );
    if let Some(a) = analyst {
        query.push_str(&format!(" AND analyst = '{}'", a.replace('\'', "''")));
    }
    if let Some(sym) = asset {
        query.push_str(&format!(
            " AND UPPER(asset) = UPPER('{}')",
            sym.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY recorded_at ASC, id ASC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], AnalystViewHistoryEntry::from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

/// Retrieve all view history entries — PostgreSQL.
fn get_all_view_history_postgres(
    pool: &PgPool,
    analyst: Option<&str>,
    asset: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<AnalystViewHistoryEntry>> {
    ensure_tables_postgres(pool)?;
    let mut query = String::from(
        "SELECT id, analyst, asset, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, recorded_at::text
         FROM analyst_view_history WHERE 1=1",
    );
    if let Some(a) = analyst {
        query.push_str(&format!(" AND analyst = '{}'", a.replace('\'', "''")));
    }
    if let Some(sym) = asset {
        query.push_str(&format!(
            " AND UPPER(asset) = UPPER('{}')",
            sym.replace('\'', "''")
        ));
    }
    query.push_str(" ORDER BY recorded_at ASC, id ASC");
    if let Some(n) = limit {
        query.push_str(&format!(" LIMIT {}", n));
    }
    let rows: Vec<HistoryPgRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&query).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(history_from_pg).collect())
}

/// Compute accuracy report — SQLite path.
fn compute_accuracy_sqlite(
    conn: &Connection,
    analyst_filter: Option<&str>,
    asset_filter: Option<&str>,
) -> Result<AccuracyReport> {
    let entries = get_all_view_history(conn, analyst_filter, asset_filter, None)?;
    compute_accuracy_from_entries(&entries, |symbol, date| {
        crate::db::price_history::get_price_at_date(conn, symbol, date)
    })
}

/// Compute accuracy report — PostgreSQL path.
fn compute_accuracy_postgres(
    pool: &PgPool,
    analyst_filter: Option<&str>,
    asset_filter: Option<&str>,
) -> Result<AccuracyReport> {
    let entries = get_all_view_history_postgres(pool, analyst_filter, asset_filter, None)?;
    compute_accuracy_from_entries(&entries, |symbol, date| {
        crate::db::price_history::get_price_at_date_postgres(pool, symbol, date)
    })
}

/// Per-asset call tracking: (evaluated calls, correct count, incorrect count).
type AssetCallTracker = (Vec<EvaluatedCall>, usize, usize);
/// Per-analyst map of per-asset call trackers.
type AnalystCallMap = std::collections::BTreeMap<String, std::collections::BTreeMap<String, AssetCallTracker>>;

/// Shared accuracy computation from a set of history entries and a price lookup function.
fn compute_accuracy_from_entries<F>(
    entries: &[AnalystViewHistoryEntry],
    get_price: F,
) -> Result<AccuracyReport>
where
    F: Fn(&str, &str) -> Result<Option<rust_decimal::Decimal>>,
{
    let mut evaluated_calls = Vec::new();
    // Group tracking: analyst → asset → (correct, incorrect)
    let mut analyst_map: AnalystCallMap =
        std::collections::BTreeMap::new();
    let mut neutral_by_analyst: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut total_by_analyst: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    for entry in entries {
        *total_by_analyst.entry(entry.analyst.clone()).or_default() += 1;

        // Skip neutral calls — they make no directional claim
        if entry.direction == "neutral" {
            *neutral_by_analyst.entry(entry.analyst.clone()).or_default() += 1;
            continue;
        }

        let window = eval_window_days(&entry.analyst);
        let exit_date = match date_plus_days(&entry.recorded_at, window) {
            Some(d) => d,
            None => continue,
        };

        // Skip if the evaluation window hasn't passed yet
        if exit_date > today {
            continue;
        }

        let entry_date = match entry.recorded_at.get(..10) {
            Some(d) => d,
            None => continue,
        };

        let entry_price = match get_price(&entry.asset, entry_date)? {
            Some(p) if !p.is_zero() => p,
            _ => continue, // no price data → can't evaluate
        };
        let exit_price = match get_price(&entry.asset, &exit_date)? {
            Some(p) if !p.is_zero() => p,
            _ => continue, // no price data → can't evaluate
        };

        let change = exit_price - entry_price;
        let change_pct = if !entry_price.is_zero() {
            use rust_decimal::prelude::ToPrimitive;
            (change / entry_price * rust_decimal::Decimal::ONE_HUNDRED)
                .to_f64()
                .unwrap_or(0.0)
        } else {
            0.0
        };

        let correct = match entry.direction.as_str() {
            "bull" => change_pct > 0.0,
            "bear" => change_pct < 0.0,
            _ => true, // should not happen given neutral filter
        };

        let call = EvaluatedCall {
            analyst: entry.analyst.clone(),
            asset: entry.asset.clone(),
            direction: entry.direction.clone(),
            conviction: entry.conviction,
            recorded_at: entry.recorded_at.clone(),
            entry_price: entry_price.to_string(),
            exit_price: exit_price.to_string(),
            price_change_pct: (change_pct * 100.0).round() / 100.0,
            correct,
            eval_window_days: window,
        };

        let asset_entry = analyst_map
            .entry(entry.analyst.clone())
            .or_default()
            .entry(entry.asset.clone())
            .or_insert((Vec::new(), 0, 0));
        if correct {
            asset_entry.1 += 1;
        } else {
            asset_entry.2 += 1;
        }
        asset_entry.0.push(call.clone());
        evaluated_calls.push(call);
    }

    // Build per-analyst summaries
    let all_analysts = ["low", "medium", "high", "macro"];
    let mut analysts = Vec::new();
    let mut total_evaluated = 0usize;
    let mut total_correct = 0usize;

    for analyst_name in &all_analysts {
        let name = analyst_name.to_string();
        let total_calls = total_by_analyst.get(&name).copied().unwrap_or(0);
        let neutral = neutral_by_analyst.get(&name).copied().unwrap_or(0);

        let asset_map = analyst_map.get(&name);
        let mut by_asset = Vec::new();
        let mut sum_correct = 0usize;
        let mut sum_incorrect = 0usize;
        let mut conv_correct_sum = 0i64;
        let mut conv_incorrect_sum = 0i64;

        if let Some(am) = asset_map {
            for (asset_sym, (calls, corr, incorr)) in am {
                sum_correct += corr;
                sum_incorrect += incorr;
                for c in calls {
                    if c.correct {
                        conv_correct_sum += c.conviction.unsigned_abs() as i64;
                    } else {
                        conv_incorrect_sum += c.conviction.unsigned_abs() as i64;
                    }
                }
                let asset_total = corr + incorr;
                by_asset.push(AssetAccuracy {
                    asset: asset_sym.clone(),
                    evaluated: asset_total,
                    correct: *corr,
                    incorrect: *incorr,
                    hit_rate_pct: if asset_total > 0 {
                        (*corr as f64 / asset_total as f64 * 100.0 * 10.0).round() / 10.0
                    } else {
                        0.0
                    },
                });
            }
        }

        by_asset.sort_by_key(|b| std::cmp::Reverse(b.evaluated));

        let evaluated = sum_correct + sum_incorrect;
        let hit_rate = if evaluated > 0 {
            (sum_correct as f64 / evaluated as f64 * 100.0 * 10.0).round() / 10.0
        } else {
            0.0
        };

        total_evaluated += evaluated;
        total_correct += sum_correct;

        // Only include analysts that have at least some history
        if total_calls > 0 {
            analysts.push(AnalystAccuracy {
                analyst: name,
                total_calls,
                evaluated,
                correct: sum_correct,
                incorrect: sum_incorrect,
                neutral_skipped: neutral,
                hit_rate_pct: hit_rate,
                avg_conviction_correct: if sum_correct > 0 {
                    (conv_correct_sum as f64 / sum_correct as f64 * 10.0).round() / 10.0
                } else {
                    0.0
                },
                avg_conviction_incorrect: if sum_incorrect > 0 {
                    (conv_incorrect_sum as f64 / sum_incorrect as f64 * 10.0).round() / 10.0
                } else {
                    0.0
                },
                eval_window_days: eval_window_days(analyst_name),
                by_asset,
            });
        }
    }

    // Include any custom analyst names not in the standard four
    if has_extra_analysts(&analyst_map, &all_analysts) {
        for extra_name in extra_analysts(&analyst_map, &all_analysts) {
            let total_calls = total_by_analyst.get(extra_name.as_str()).copied().unwrap_or(0);
            let neutral = neutral_by_analyst.get(extra_name.as_str()).copied().unwrap_or(0);
            let mut by_asset = Vec::new();
            let mut sum_correct = 0usize;
            let mut sum_incorrect = 0usize;
            let mut conv_correct_sum = 0i64;
            let mut conv_incorrect_sum = 0i64;

            if let Some(asset_data) = analyst_map.get(&extra_name) {
                for (asset_sym, (calls, corr, incorr)) in asset_data {
                    sum_correct += corr;
                    sum_incorrect += incorr;
                    for c in calls {
                        if c.correct {
                            conv_correct_sum += c.conviction.unsigned_abs() as i64;
                        } else {
                            conv_incorrect_sum += c.conviction.unsigned_abs() as i64;
                        }
                    }
                    let asset_total = corr + incorr;
                    by_asset.push(AssetAccuracy {
                        asset: asset_sym.clone(),
                        evaluated: asset_total,
                        correct: *corr,
                        incorrect: *incorr,
                        hit_rate_pct: if asset_total > 0 {
                            (*corr as f64 / asset_total as f64 * 100.0 * 10.0).round() / 10.0
                        } else {
                            0.0
                        },
                    });
                }
            }

            by_asset.sort_by_key(|b| std::cmp::Reverse(b.evaluated));

            let evaluated = sum_correct + sum_incorrect;
            total_evaluated += evaluated;
            total_correct += sum_correct;

            analysts.push(AnalystAccuracy {
                analyst: extra_name.clone(),
                total_calls,
                evaluated,
                correct: sum_correct,
                incorrect: sum_incorrect,
                neutral_skipped: neutral,
                hit_rate_pct: if evaluated > 0 {
                    (sum_correct as f64 / evaluated as f64 * 100.0 * 10.0).round() / 10.0
                } else {
                    0.0
                },
                avg_conviction_correct: if sum_correct > 0 {
                    (conv_correct_sum as f64 / sum_correct as f64 * 10.0).round() / 10.0
                } else {
                    0.0
                },
                avg_conviction_incorrect: if sum_incorrect > 0 {
                    (conv_incorrect_sum as f64 / sum_incorrect as f64 * 10.0).round() / 10.0
                } else {
                    0.0
                },
                eval_window_days: eval_window_days(&extra_name),
                by_asset,
            });
        }
    }

    let overall_hit_rate = if total_evaluated > 0 {
        (total_correct as f64 / total_evaluated as f64 * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };

    Ok(AccuracyReport {
        analysts,
        total_history_entries: entries.len(),
        total_evaluated,
        total_correct,
        overall_hit_rate_pct: overall_hit_rate,
        evaluated_calls,
    })
}

/// Helper: check if any non-standard analyst names exist in the map.
fn has_extra_analysts(map: &AnalystCallMap, standard: &[&str]) -> bool {
    map.keys().any(|k| !standard.contains(&k.as_str()))
}

/// Helper: iterate non-standard analyst names from the map.
fn extra_analysts(map: &AnalystCallMap, standard: &[&str]) -> Vec<String> {
    map.keys()
        .filter(|k| !standard.contains(&k.as_str()))
        .cloned()
        .collect()
}

/// Public backend dispatch for accuracy computation.
pub fn compute_accuracy_backend(
    backend: &BackendConnection,
    analyst: Option<&str>,
    asset: Option<&str>,
) -> Result<AccuracyReport> {
    query::dispatch(
        backend,
        |conn| compute_accuracy_sqlite(conn, analyst, asset),
        |pool| compute_accuracy_postgres(pool, analyst, asset),
    )
}

// ---------------------------------------------------------------------------
// Convergence — deterministic aggregation across analysts for a single asset
// ---------------------------------------------------------------------------

/// Per-view payload included in a `ConvergenceReport`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvergenceView {
    pub analyst: String,
    pub direction: String,
    pub conviction: i64,
    pub reasoning_summary: String,
    pub key_evidence: Option<String>,
    pub blind_spots: Option<String>,
    pub allocation_bias: Option<String>,
    pub recorded_at: String,
}

/// Aggregate statistics across the views included in a convergence report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvergenceStats {
    pub n_views: usize,
    pub avg_conviction: f64,
    pub min_conviction: i64,
    pub max_conviction: i64,
    pub max_divergence: i64,
    pub alloc_bias_counts: std::collections::BTreeMap<String, usize>,
}

/// Deterministic convergence report for one asset within a lookback window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvergenceReport {
    pub asset: String,
    pub as_of: String,
    pub views: Vec<ConvergenceView>,
    pub stats: ConvergenceStats,
    pub summary: String,
}

/// Internal tuple shape of a view row loaded for convergence aggregation.
/// Fields: (analyst, direction, conviction, reasoning_summary, key_evidence,
/// blind_spots, allocation_bias, recorded_at).
type ConvergenceRow = (
    String,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

/// The five allowed allocation-bias buckets, plus a synthetic `null` bucket
/// for views that did not specify one.
const ALLOC_BIAS_BUCKETS: [&str; 6] = [
    "overweight",
    "slight-overweight",
    "at-target",
    "slight-underweight",
    "underweight",
    "null",
];

/// Parse a `--since <duration>` string into an RFC3339 timestamp `Some` (relative)
/// or `None` (no window — include all rows).
///
/// Accepted suffixes: `h` (hours), `d` (days), `w` (weeks), `m` (months ≈ 30d).
/// Absolute RFC3339 or `YYYY-MM-DD` timestamps are also accepted and returned as-is
/// (normalised to RFC3339).
pub fn parse_since(value: &str) -> Result<String> {
    use chrono::{DateTime, Utc};

    if let Some(stripped) = value.strip_suffix('h') {
        let hours: i64 = stripped.parse()?;
        Ok((Utc::now() - chrono::Duration::hours(hours)).to_rfc3339())
    } else if let Some(stripped) = value.strip_suffix('d') {
        let days: i64 = stripped.parse()?;
        Ok((Utc::now() - chrono::Duration::days(days)).to_rfc3339())
    } else if let Some(stripped) = value.strip_suffix('w') {
        let weeks: i64 = stripped.parse()?;
        Ok((Utc::now() - chrono::Duration::weeks(weeks)).to_rfc3339())
    } else if let Some(stripped) = value.strip_suffix('m') {
        let months: i64 = stripped.parse()?;
        Ok((Utc::now() - chrono::Duration::days(months * 30)).to_rfc3339())
    } else if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        Ok(dt.to_rfc3339())
    } else if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let naive_dt = naive_date.and_hms_opt(0, 0, 0).unwrap();
        Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc).to_rfc3339())
    } else {
        anyhow::bail!(
            "could not parse --since '{}': expected Nh/Nd/Nw/Nm, RFC3339, or YYYY-MM-DD",
            value
        );
    }
}

/// Deterministic classifier for a convergence summary based on aggregate stats.
///
/// Branches (evaluated in order):
/// 1. `n_views < 2`                                       → `"insufficient-views"`
/// 2. `max_divergence > 4`                                → `"divergent"`
/// 3. `max_divergence > 2` && `-1 <= avg <= 1`            → `"neutral-with-divergence"`
/// 4. `avg >= 3` && `max_divergence <= 2`                 → `"strong-convergent-bull"`
/// 5. `avg >= 1` && `max_divergence <= 3`                 → `"convergent-bull"`
/// 6. `avg <= -3` && `max_divergence <= 2`                → `"strong-convergent-bear"`
/// 7. `avg <= -1` && `max_divergence <= 3`                → `"convergent-bear"`
/// 8. otherwise                                           → `"convergent-neutral"`
pub fn classify_convergence(n_views: usize, avg_conviction: f64, max_divergence: i64) -> &'static str {
    if n_views < 2 {
        return "insufficient-views";
    }
    if max_divergence > 4 {
        return "divergent";
    }
    if max_divergence > 2 && (-1.0..=1.0).contains(&avg_conviction) {
        return "neutral-with-divergence";
    }
    if avg_conviction >= 3.0 && max_divergence <= 2 {
        return "strong-convergent-bull";
    }
    if avg_conviction >= 1.0 && max_divergence <= 3 {
        return "convergent-bull";
    }
    if avg_conviction <= -3.0 && max_divergence <= 2 {
        return "strong-convergent-bear";
    }
    if avg_conviction <= -1.0 && max_divergence <= 3 {
        return "convergent-bear";
    }
    "convergent-neutral"
}

/// Internal de-duplicated entry: everything except the analyst (used as map key).
type LatestEntry = (
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

/// Build a `ConvergenceReport` from a flat list of historical view rows for one asset.
fn build_report_for_asset(
    asset: &str,
    rows: Vec<ConvergenceRow>,
    as_of: &str,
) -> ConvergenceReport {
    // De-duplicate per-analyst: keep the most recent view per analyst layer in the window.
    let mut latest: std::collections::BTreeMap<String, LatestEntry> =
        std::collections::BTreeMap::new();
    for (analyst, direction, conviction, reasoning, evidence, blind, bias, recorded_at) in rows {
        let existing = latest.get(&analyst);
        let keep_new = match existing {
            None => true,
            Some(prev) => recorded_at >= prev.6,
        };
        if keep_new {
            latest.insert(
                analyst,
                (
                    direction,
                    conviction,
                    reasoning,
                    evidence,
                    blind,
                    bias,
                    recorded_at,
                ),
            );
        }
    }

    let mut views: Vec<ConvergenceView> = latest
        .into_iter()
        .map(
            |(analyst, (direction, conviction, reasoning, evidence, blind, bias, recorded_at))| {
                // Defense for legacy rows written before write-time sign
                // normalization: direction is authoritative, so a bear view
                // stored with +3 contributes -3 to convergence stats.
                let conviction = effective_conviction(&direction, conviction);
                ConvergenceView {
                    analyst,
                    direction,
                    conviction,
                    reasoning_summary: reasoning,
                    key_evidence: evidence,
                    blind_spots: blind,
                    allocation_bias: bias,
                    recorded_at,
                }
            },
        )
        .collect();
    views.sort_by(|a, b| a.analyst.cmp(&b.analyst));

    let n_views = views.len();
    let (avg_conviction, min_conv, max_conv, max_divergence) = if n_views == 0 {
        (0.0_f64, 0_i64, 0_i64, 0_i64)
    } else {
        let convs: Vec<i64> = views.iter().map(|v| v.conviction).collect();
        let sum: i64 = convs.iter().sum();
        let avg = sum as f64 / n_views as f64;
        let min = *convs.iter().min().unwrap();
        let max = *convs.iter().max().unwrap();
        (avg, min, max, max - min)
    };

    let mut alloc_bias_counts: std::collections::BTreeMap<String, usize> = ALLOC_BIAS_BUCKETS
        .iter()
        .map(|b| ((*b).to_string(), 0usize))
        .collect();
    for v in &views {
        let key = match v.allocation_bias.as_deref() {
            Some(b) if ALLOC_BIAS_BUCKETS.contains(&b) => b.to_string(),
            _ => "null".to_string(),
        };
        *alloc_bias_counts.entry(key).or_insert(0) += 1;
    }

    let summary = classify_convergence(n_views, avg_conviction, max_divergence).to_string();

    ConvergenceReport {
        asset: asset.to_string(),
        as_of: as_of.to_string(),
        views,
        stats: ConvergenceStats {
            n_views,
            avg_conviction: (avg_conviction * 100.0).round() / 100.0,
            min_conviction: min_conv,
            max_conviction: max_conv,
            max_divergence,
            alloc_bias_counts,
        },
        summary,
    }
}

/// SQLite path: load all view rows for the asset whose `recorded_at >= since`
/// (or all rows if `since` is `None`).
fn load_convergence_rows_sqlite(
    conn: &Connection,
    asset: &str,
    since: Option<&str>,
) -> Result<Vec<ConvergenceRow>> {
    let mut q = String::from(
        "SELECT analyst, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, recorded_at
         FROM analyst_view_history
         WHERE UPPER(asset) = UPPER(?)",
    );
    if let Some(s) = since {
        q.push_str(&format!(" AND recorded_at >= '{}'", s.replace('\'', "''")));
    }
    q.push_str(" ORDER BY recorded_at ASC, id ASC");
    let mut stmt = conn.prepare(&q)?;
    let rows = stmt.query_map(params![asset], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// SQLite path: list every asset symbol with at least one view in the window.
fn load_convergence_assets_sqlite(
    conn: &Connection,
    since: Option<&str>,
) -> Result<Vec<String>> {
    let q = if let Some(s) = since {
        format!(
            "SELECT DISTINCT asset FROM analyst_view_history WHERE recorded_at >= '{}' ORDER BY asset",
            s.replace('\'', "''")
        )
    } else {
        String::from("SELECT DISTINCT asset FROM analyst_view_history ORDER BY asset")
    };
    let mut stmt = conn.prepare(&q)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Postgres path counterparts.
fn load_convergence_rows_postgres(
    pool: &PgPool,
    asset: &str,
    since: Option<&str>,
) -> Result<Vec<ConvergenceRow>> {
    type Row = (
        String,
        String,
        i32,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let mut q = String::from(
        "SELECT analyst, direction, conviction, reasoning_summary, key_evidence, blind_spots, allocation_bias, recorded_at::text
         FROM analyst_view_history
         WHERE UPPER(asset) = UPPER($1)",
    );
    if let Some(s) = since {
        q.push_str(&format!(
            " AND recorded_at >= '{}'::timestamptz",
            s.replace('\'', "''")
        ));
    }
    q.push_str(" ORDER BY recorded_at ASC, id ASC");
    let rows: Vec<Row> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&q).bind(asset).fetch_all(pool).await
    })?;
    Ok(rows
        .into_iter()
        .map(|(analyst, direction, conviction, reasoning, evidence, blind, bias, recorded_at)| {
            (
                analyst,
                direction,
                conviction as i64,
                reasoning,
                evidence,
                blind,
                bias,
                recorded_at,
            )
        })
        .collect())
}

fn load_convergence_assets_postgres(pool: &PgPool, since: Option<&str>) -> Result<Vec<String>> {
    let q = if let Some(s) = since {
        format!(
            "SELECT DISTINCT asset FROM analyst_view_history WHERE recorded_at >= '{}'::timestamptz ORDER BY asset",
            s.replace('\'', "''")
        )
    } else {
        String::from("SELECT DISTINCT asset FROM analyst_view_history ORDER BY asset")
    };
    let rows: Vec<(String,)> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(&q).fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Public dispatch: build a convergence report for a single asset.
pub fn convergence_report_backend(
    backend: &BackendConnection,
    asset: &str,
    since: Option<&str>,
) -> Result<ConvergenceReport> {
    let as_of = chrono::Utc::now().to_rfc3339();
    let rows = query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            load_convergence_rows_sqlite(conn, asset, since)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            load_convergence_rows_postgres(pool, asset, since)
        },
    )?;
    Ok(build_report_for_asset(&asset.to_uppercase(), rows, &as_of))
}

/// Public dispatch: build a convergence report for every asset with ≥1 view in window.
pub fn convergence_all_backend(
    backend: &BackendConnection,
    since: Option<&str>,
) -> Result<Vec<ConvergenceReport>> {
    let assets = query::dispatch(
        backend,
        |conn| {
            ensure_tables(conn)?;
            load_convergence_assets_sqlite(conn, since)
        },
        |pool| {
            ensure_tables_postgres(pool)?;
            load_convergence_assets_postgres(pool, since)
        },
    )?;
    let mut reports = Vec::new();
    for asset in assets {
        let report = convergence_report_backend(backend, &asset, since)?;
        reports.push(report);
    }
    Ok(reports)
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

    /// Setup DB with both analyst_views and price_history tables.
    fn setup_db_with_prices() -> Connection {
        let conn = setup_db();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS price_history (
                symbol TEXT NOT NULL,
                date TEXT NOT NULL,
                close TEXT NOT NULL,
                source TEXT NOT NULL,
                volume TEXT,
                open TEXT,
                high TEXT,
                low TEXT,
                PRIMARY KEY (symbol, date)
            )"
        ).unwrap();
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
            Some("Whale selling could cap upside"), None,
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
            None, None,
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
            Some("Could resume if DXY weakens"), None,
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
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None, None).unwrap();
        upsert_view(&conn, "medium", "BTC", "bull", 2, "Swing bullish", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Valuation stretched", None, None, None).unwrap();
        upsert_view(&conn, "low", "GLD", "bull", 4, "Gold momentum", None, None, None).unwrap();

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
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Overvalued", None, None, None).unwrap();
        upsert_view(&conn, "low", "GLD", "bull", 4, "Safe haven bid", None, None, None).unwrap();
        upsert_view(&conn, "macro", "GLD", "bull", 5, "Structural", None, None, None).unwrap();

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
        upsert_view(&conn, "low", "BTC", "bull", 3, "Test", None, None, None).unwrap();
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
        upsert_view(&conn, "low", "BTC", "bull", 3, "Test", None, None, None).unwrap();

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

    #[test]
    fn test_portfolio_matrix_includes_all_symbols() {
        let conn = setup_db();
        // Only BTC has a view
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None, None).unwrap();

        // Portfolio has BTC, GLD, SLV — GLD and SLV have no views
        let symbols = vec!["BTC".to_string(), "GLD".to_string(), "SLV".to_string()];
        let matrix = get_portfolio_view_matrix(&conn, &symbols).unwrap();

        assert_eq!(matrix.len(), 3);
        let btc = matrix.iter().find(|m| m.asset == "BTC").unwrap();
        assert_eq!(btc.views.len(), 1);
        let gld = matrix.iter().find(|m| m.asset == "GLD").unwrap();
        assert_eq!(gld.views.len(), 0); // no views yet
        let slv = matrix.iter().find(|m| m.asset == "SLV").unwrap();
        assert_eq!(slv.views.len(), 0);
    }

    #[test]
    fn test_portfolio_matrix_includes_viewed_assets_not_in_portfolio() {
        let conn = setup_db();
        // TSLA has a view but is not in portfolio
        upsert_view(&conn, "high", "TSLA", "bear", -2, "Overvalued", None, None, None).unwrap();

        let symbols = vec!["BTC".to_string(), "GLD".to_string()];
        let matrix = get_portfolio_view_matrix(&conn, &symbols).unwrap();

        // Should include BTC, GLD (portfolio) + TSLA (has views)
        assert_eq!(matrix.len(), 3);
        assert!(matrix.iter().any(|m| m.asset == "BTC"));
        assert!(matrix.iter().any(|m| m.asset == "GLD"));
        assert!(matrix.iter().any(|m| m.asset == "TSLA"));
    }

    #[test]
    fn test_portfolio_matrix_deduplicates() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Test", None, None, None).unwrap();

        // BTC is in both portfolio and has views — should not be duplicated
        let symbols = vec!["BTC".to_string()];
        let matrix = get_portfolio_view_matrix(&conn, &symbols).unwrap();

        assert_eq!(matrix.len(), 1);
        assert_eq!(matrix[0].asset, "BTC");
        assert_eq!(matrix[0].views.len(), 1);
    }

    #[test]
    fn test_portfolio_matrix_empty_portfolio() {
        let conn = setup_db();
        upsert_view(&conn, "macro", "GLD", "bull", 5, "Structural", None, None, None).unwrap();

        // Empty portfolio — should still show assets with views
        let symbols: Vec<String> = vec![];
        let matrix = get_portfolio_view_matrix(&conn, &symbols).unwrap();

        assert_eq!(matrix.len(), 1);
        assert_eq!(matrix[0].asset, "GLD");
    }

    #[test]
    fn test_portfolio_matrix_sorted() {
        let conn = setup_db();
        upsert_view(&conn, "low", "TSLA", "bear", -1, "Test", None, None, None).unwrap();

        let symbols = vec!["SLV".to_string(), "BTC".to_string(), "GLD".to_string()];
        let matrix = get_portfolio_view_matrix(&conn, &symbols).unwrap();

        let assets: Vec<&str> = matrix.iter().map(|m| m.asset.as_str()).collect();
        assert_eq!(assets, vec!["BTC", "GLD", "SLV", "TSLA"]);
    }

    // --- History tests ---

    #[test]
    fn test_history_table_created() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='analyst_view_history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_upsert_appends_to_history() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None, None).unwrap();

        let history = get_view_history(&conn, "BTC", None, None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].analyst, "low");
        assert_eq!(history[0].direction, "bull");
        assert_eq!(history[0].conviction, 3);
    }

    #[test]
    fn test_multiple_upserts_create_multiple_history_entries() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None, None).unwrap();
        upsert_view(&conn, "low", "BTC", "bull", 4, "Momentum accelerating", None, None, None).unwrap();
        upsert_view(&conn, "low", "BTC", "bear", -2, "Reversal signal", None, None, None).unwrap();

        // Current view should be latest
        let current = get_view(&conn, "low", "BTC").unwrap().unwrap();
        assert_eq!(current.direction, "bear");
        assert_eq!(current.conviction, -2);

        // History should have all 3 entries
        let history = get_view_history(&conn, "BTC", None, None).unwrap();
        assert_eq!(history.len(), 3);
        // Ordered DESC (newest first)
        assert_eq!(history[0].direction, "bear");
        assert_eq!(history[1].conviction, 4);
        assert_eq!(history[2].conviction, 3);
    }

    #[test]
    fn test_history_filter_by_analyst() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Low view", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "High view", None, None, None).unwrap();

        let low_history = get_view_history(&conn, "BTC", Some("low"), None).unwrap();
        assert_eq!(low_history.len(), 1);
        assert_eq!(low_history[0].analyst, "low");

        let high_history = get_view_history(&conn, "BTC", Some("high"), None).unwrap();
        assert_eq!(high_history.len(), 1);
        assert_eq!(high_history[0].analyst, "high");
    }

    #[test]
    fn test_history_limit() {
        let conn = setup_db();
        for i in 0..5 {
            upsert_view(
                &conn,
                "low",
                "BTC",
                "bull",
                i,
                &format!("Update {}", i),
                None,
                None, None,
            )
            .unwrap();
        }

        let limited = get_view_history(&conn, "BTC", None, Some(3)).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_history_case_insensitive_asset() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Test", None, None, None).unwrap();

        let history_lower = get_view_history(&conn, "btc", None, None).unwrap();
        assert_eq!(history_lower.len(), 1);

        let history_upper = get_view_history(&conn, "BTC", None, None).unwrap();
        assert_eq!(history_upper.len(), 1);
    }

    #[test]
    fn test_history_preserves_evidence_and_blind_spots() {
        let conn = setup_db();
        upsert_view(
            &conn,
            "macro",
            "GLD",
            "bull",
            5,
            "Structural central bank buying",
            Some("WGC Q4, PBOC reserves"),
            Some("Risk-on shift"), None,
        )
        .unwrap();

        let history = get_view_history(&conn, "GLD", None, None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].key_evidence.as_deref(), Some("WGC Q4, PBOC reserves"));
        assert_eq!(history[0].blind_spots.as_deref(), Some("Risk-on shift"));
    }

    #[test]
    fn test_history_empty_for_unknown_asset() {
        let conn = setup_db();
        let history = get_view_history(&conn, "DOESNOTEXIST", None, None).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_history_multi_analyst_interleaved() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 2, "Low initial", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -3, "High initial", None, None, None).unwrap();
        upsert_view(&conn, "low", "BTC", "bull", 4, "Low updated", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "neutral", 0, "High revised", None, None, None).unwrap();

        // All history for BTC
        let all = get_view_history(&conn, "BTC", None, None).unwrap();
        assert_eq!(all.len(), 4);

        // Filtered to low
        let low = get_view_history(&conn, "BTC", Some("low"), None).unwrap();
        assert_eq!(low.len(), 2);
        assert_eq!(low[0].conviction, 4); // newest first
        assert_eq!(low[1].conviction, 2);

        // Filtered to high
        let high = get_view_history(&conn, "BTC", Some("high"), None).unwrap();
        assert_eq!(high.len(), 2);
        assert_eq!(high[0].direction, "neutral"); // newest first
        assert_eq!(high[1].direction, "bear");
    }

    // --- Divergence tests ---

    #[test]
    fn test_divergence_basic() {
        let conn = setup_db();
        // BTC: LOW bull +3, HIGH bear -2 → spread = 5
        upsert_view(&conn, "low", "BTC", "bull", 3, "Momentum up", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Overvalued", None, None, None).unwrap();

        let divs = compute_divergence(&conn, 2, None, None, None).unwrap();
        assert_eq!(divs.len(), 1);
        assert_eq!(divs[0].asset, "BTC");
        assert_eq!(divs[0].spread, 5);
        assert_eq!(divs[0].most_bullish.analyst, "low");
        assert_eq!(divs[0].most_bullish.conviction, 3);
        assert_eq!(divs[0].most_bearish.analyst, "high");
        assert_eq!(divs[0].most_bearish.conviction, -2);
        assert_eq!(divs[0].all_views.len(), 2);
    }

    #[test]
    fn test_divergence_min_spread_filter() {
        let conn = setup_db();
        // BTC: spread 5 (bull +3, bear -2)
        upsert_view(&conn, "low", "BTC", "bull", 3, "Up", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Down", None, None, None).unwrap();
        // GLD: spread 2 (bull +4, neutral +2)
        upsert_view(&conn, "low", "GLD", "bull", 4, "Safe haven", None, None, None).unwrap();
        upsert_view(&conn, "macro", "GLD", "bull", 2, "Moderate", None, None, None).unwrap();

        // min_spread 3: only BTC qualifies
        let divs = compute_divergence(&conn, 3, None, None, None).unwrap();
        assert_eq!(divs.len(), 1);
        assert_eq!(divs[0].asset, "BTC");

        // min_spread 2: both qualify
        let divs = compute_divergence(&conn, 2, None, None, None).unwrap();
        assert_eq!(divs.len(), 2);
    }

    #[test]
    fn test_divergence_sorted_by_spread_desc() {
        let conn = setup_db();
        // GLD: spread 9 (bull +5, bear -4)
        upsert_view(&conn, "macro", "GLD", "bull", 5, "Structural", None, None, None).unwrap();
        upsert_view(&conn, "low", "GLD", "bear", -4, "Short-term sell", None, None, None).unwrap();
        // BTC: spread 5 (bull +3, bear -2)
        upsert_view(&conn, "low", "BTC", "bull", 3, "Up", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Down", None, None, None).unwrap();

        let divs = compute_divergence(&conn, 2, None, None, None).unwrap();
        assert_eq!(divs.len(), 2);
        assert_eq!(divs[0].asset, "GLD"); // spread 9 first
        assert_eq!(divs[0].spread, 9);
        assert_eq!(divs[1].asset, "BTC"); // spread 5 second
        assert_eq!(divs[1].spread, 5);
    }

    #[test]
    fn test_divergence_asset_filter() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "Up", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -2, "Down", None, None, None).unwrap();
        upsert_view(&conn, "low", "GLD", "bull", 4, "Up", None, None, None).unwrap();
        upsert_view(&conn, "high", "GLD", "bear", -3, "Down", None, None, None).unwrap();

        let divs = compute_divergence(&conn, 2, Some("BTC"), None, None).unwrap();
        assert_eq!(divs.len(), 1);
        assert_eq!(divs[0].asset, "BTC");
    }

    #[test]
    fn test_divergence_limit() {
        let conn = setup_db();
        // Create 3 divergent assets
        for (asset, conv_hi, conv_lo) in [("BTC", 3, -2), ("GLD", 5, -4), ("SLV", 2, -1)] {
            upsert_view(&conn, "low", asset, "bull", conv_hi, "Up", None, None, None).unwrap();
            upsert_view(&conn, "high", asset, "bear", conv_lo, "Down", None, None, None).unwrap();
        }

        let divs = compute_divergence(&conn, 2, None, None, Some(2)).unwrap();
        assert_eq!(divs.len(), 2);
        // Should be top 2 by spread: GLD (9), BTC (5)
        assert_eq!(divs[0].asset, "GLD");
        assert_eq!(divs[1].asset, "BTC");
    }

    #[test]
    fn test_divergence_layer_filter_matches_extremes_only() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 4, "Momentum", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -3, "Resistance", None, None, None).unwrap();
        upsert_view(&conn, "medium", "GLD", "bull", 1, "Rotation", None, None, None).unwrap();
        upsert_view(&conn, "macro", "GLD", "bear", -4, "Deflation", None, None, None).unwrap();
        upsert_view(&conn, "high", "GLD", "neutral", 0, "Waiting", None, None, None).unwrap();

        let high_divs = compute_divergence(&conn, 2, None, Some("high"), None).unwrap();
        assert_eq!(high_divs.len(), 1);
        assert_eq!(high_divs[0].asset, "BTC");

        let macro_divs = compute_divergence(&conn, 2, None, Some("macro"), None).unwrap();
        assert_eq!(macro_divs.len(), 1);
        assert_eq!(macro_divs[0].asset, "GLD");
    }

    #[test]
    fn test_divergence_single_analyst_excluded() {
        let conn = setup_db();
        // Only one analyst on this asset → no divergence
        upsert_view(&conn, "low", "BTC", "bull", 3, "Up", None, None, None).unwrap();

        let divs = compute_divergence(&conn, 0, None, None, None).unwrap();
        assert!(divs.is_empty());
    }

    #[test]
    fn test_divergence_all_agree() {
        let conn = setup_db();
        // All analysts agree on bull +3 → spread 0
        upsert_view(&conn, "low", "BTC", "bull", 3, "Up", None, None, None).unwrap();
        upsert_view(&conn, "high", "BTC", "bull", 3, "Up", None, None, None).unwrap();
        upsert_view(&conn, "macro", "BTC", "bull", 3, "Up", None, None, None).unwrap();

        let divs = compute_divergence(&conn, 1, None, None, None).unwrap();
        assert!(divs.is_empty());
    }

    #[test]
    fn test_divergence_empty_db() {
        let conn = setup_db();
        let divs = compute_divergence(&conn, 2, None, None, None).unwrap();
        assert!(divs.is_empty());
    }

    #[test]
    fn test_divergence_nonexistent_asset() {
        let conn = setup_db();
        let divs = compute_divergence(&conn, 0, Some("NOPE"), None, None).unwrap();
        assert!(divs.is_empty());
    }

    // -----------------------------------------------------------------------
    // Accuracy tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_date_plus_days() {
        assert_eq!(date_plus_days("2026-03-20", 3), Some("2026-03-23".to_string()));
        assert_eq!(date_plus_days("2026-03-30", 7), Some("2026-04-06".to_string()));
        assert_eq!(date_plus_days("2026-12-29", 5), Some("2027-01-03".to_string()));
        // Works with datetime suffix
        assert_eq!(
            date_plus_days("2026-03-20 14:30:00", 3),
            Some("2026-03-23".to_string())
        );
        // Too short
        assert_eq!(date_plus_days("2026-03", 3), None);
    }

    #[test]
    fn test_eval_window_days() {
        assert_eq!(eval_window_days("low"), 3);
        assert_eq!(eval_window_days("medium"), 14);
        assert_eq!(eval_window_days("high"), 30);
        assert_eq!(eval_window_days("macro"), 90);
        assert_eq!(eval_window_days("unknown"), 7);
    }

    #[test]
    fn test_accuracy_empty_history() {
        let conn = setup_db();
        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_history_entries, 0);
        assert_eq!(report.total_evaluated, 0);
        assert_eq!(report.overall_hit_rate_pct, 0.0);
        assert!(report.analysts.is_empty());
        assert!(report.evaluated_calls.is_empty());
    }


    #[test]
    fn test_accuracy_with_history_no_prices() {
        let conn = setup_db_with_prices();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 3, 'Test', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_history_entries, 1);
        assert_eq!(report.total_evaluated, 0);
        assert_eq!(report.analysts.len(), 1);
        assert_eq!(report.analysts[0].analyst, "low");
        assert_eq!(report.analysts[0].total_calls, 1);
        assert_eq!(report.analysts[0].evaluated, 0);
    }

    #[test]
    fn test_accuracy_bull_correct() {
        let conn = setup_db_with_prices();
        crate::db::price_history::upsert_history(
            &conn,
            "BTC",
            "test",
            &[
                crate::models::price::HistoryRecord {
                    date: "2025-01-01".to_string(),
                    close: rust_decimal::Decimal::new(10000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-04".to_string(),
                    close: rust_decimal::Decimal::new(11000, 2),
                    volume: None, open: None, high: None, low: None,
                },
            ],
        ).unwrap();

        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 3, 'Momentum strong', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_evaluated, 1);
        assert_eq!(report.total_correct, 1);
        assert_eq!(report.overall_hit_rate_pct, 100.0);
        assert_eq!(report.evaluated_calls.len(), 1);
        assert!(report.evaluated_calls[0].correct);
        assert!(report.evaluated_calls[0].price_change_pct > 0.0);
    }

    #[test]
    fn test_accuracy_bear_correct() {
        let conn = setup_db_with_prices();
        crate::db::price_history::upsert_history(
            &conn,
            "BTC",
            "test",
            &[
                crate::models::price::HistoryRecord {
                    date: "2025-01-01".to_string(),
                    close: rust_decimal::Decimal::new(10000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-04".to_string(),
                    close: rust_decimal::Decimal::new(9000, 2),
                    volume: None, open: None, high: None, low: None,
                },
            ],
        ).unwrap();

        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bear', -3, 'Weakness', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_evaluated, 1);
        assert_eq!(report.total_correct, 1);
        assert!(report.evaluated_calls[0].correct);
    }

    #[test]
    fn test_accuracy_bull_incorrect() {
        let conn = setup_db_with_prices();
        crate::db::price_history::upsert_history(
            &conn,
            "BTC",
            "test",
            &[
                crate::models::price::HistoryRecord {
                    date: "2025-01-01".to_string(),
                    close: rust_decimal::Decimal::new(10000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-04".to_string(),
                    close: rust_decimal::Decimal::new(9000, 2),
                    volume: None, open: None, high: None, low: None,
                },
            ],
        ).unwrap();

        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 4, 'Wrong call', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_evaluated, 1);
        assert_eq!(report.total_correct, 0);
        assert!(!report.evaluated_calls[0].correct);
        assert_eq!(report.overall_hit_rate_pct, 0.0);
    }

    #[test]
    fn test_accuracy_neutral_skipped() {
        let conn = setup_db_with_prices();
        crate::db::price_history::upsert_history(
            &conn,
            "BTC",
            "test",
            &[
                crate::models::price::HistoryRecord {
                    date: "2025-01-01".to_string(),
                    close: rust_decimal::Decimal::new(10000, 2),
                    volume: None, open: None, high: None, low: None,
                },
            ],
        ).unwrap();

        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'neutral', 0, 'No view', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_history_entries, 1);
        assert_eq!(report.total_evaluated, 0);
        assert_eq!(report.analysts.len(), 1);
        assert_eq!(report.analysts[0].neutral_skipped, 1);
    }

    #[test]
    fn test_accuracy_analyst_filter() {
        let conn = setup_db_with_prices();
        crate::db::price_history::upsert_history(
            &conn,
            "BTC",
            "test",
            &[
                crate::models::price::HistoryRecord {
                    date: "2025-01-01".to_string(),
                    close: rust_decimal::Decimal::new(10000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-04".to_string(),
                    close: rust_decimal::Decimal::new(11000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-15".to_string(),
                    close: rust_decimal::Decimal::new(10500, 2),
                    volume: None, open: None, high: None, low: None,
                },
            ],
        ).unwrap();

        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 3, 'Up', '2025-01-01 12:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('medium', 'BTC', 'bull', 2, 'Medium up', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, Some("low"), None).unwrap();
        assert_eq!(report.total_history_entries, 1);
        assert_eq!(report.analysts.len(), 1);
        assert_eq!(report.analysts[0].analyst, "low");
    }

    #[test]
    fn test_accuracy_multiple_analysts() {
        let conn = setup_db_with_prices();
        crate::db::price_history::upsert_history(
            &conn,
            "BTC",
            "test",
            &[
                crate::models::price::HistoryRecord {
                    date: "2025-01-01".to_string(),
                    close: rust_decimal::Decimal::new(10000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-04".to_string(),
                    close: rust_decimal::Decimal::new(11000, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-15".to_string(),
                    close: rust_decimal::Decimal::new(10500, 2),
                    volume: None, open: None, high: None, low: None,
                },
                crate::models::price::HistoryRecord {
                    date: "2025-01-31".to_string(),
                    close: rust_decimal::Decimal::new(9500, 2),
                    volume: None, open: None, high: None, low: None,
                },
            ],
        ).unwrap();

        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 3, 'Up', '2025-01-01 12:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('medium', 'BTC', 'bear', -2, 'Down medium', '2025-01-01 12:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('high', 'BTC', 'bear', -4, 'Structural down', '2025-01-01 12:00:00')",
            [],
        ).unwrap();

        let report = compute_accuracy_sqlite(&conn, None, None).unwrap();
        assert_eq!(report.total_history_entries, 3);
        assert_eq!(report.total_evaluated, 3);
        assert_eq!(report.total_correct, 2);
        assert!((report.overall_hit_rate_pct - 66.7).abs() < 0.1);

        let low = report.analysts.iter().find(|a| a.analyst == "low").unwrap();
        assert_eq!(low.evaluated, 1);
        assert_eq!(low.correct, 1);
        assert_eq!(low.hit_rate_pct, 100.0);

        let med = report.analysts.iter().find(|a| a.analyst == "medium").unwrap();
        assert_eq!(med.evaluated, 1);
        assert_eq!(med.correct, 0);
        assert_eq!(med.hit_rate_pct, 0.0);

        let high = report.analysts.iter().find(|a| a.analyst == "high").unwrap();
        assert_eq!(high.evaluated, 1);
        assert_eq!(high.correct, 1);
        assert_eq!(high.hit_rate_pct, 100.0);
    }

    #[test]
    fn test_get_all_view_history() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 3, 'Up', '2025-01-01 12:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('high', 'GLD', 'bear', -2, 'Down', '2025-01-02 12:00:00')",
            [],
        ).unwrap();

        let all = get_all_view_history(&conn, None, None, None).unwrap();
        assert_eq!(all.len(), 2);

        let low_only = get_all_view_history(&conn, Some("low"), None, None).unwrap();
        assert_eq!(low_only.len(), 1);
        assert_eq!(low_only[0].analyst, "low");

        let gld_only = get_all_view_history(&conn, None, Some("GLD"), None).unwrap();
        assert_eq!(gld_only.len(), 1);
        assert_eq!(gld_only[0].asset, "GLD");

        let limited = get_all_view_history(&conn, None, None, Some(1)).unwrap();
        assert_eq!(limited.len(), 1);
    }

    // -----------------------------------------------------------------------
    // allocation_bias migration + round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_allocation_bias_column_present() {
        let conn = setup_db();
        // Column exists on both tables after ensure_tables runs.
        for table in ["analyst_views", "analyst_view_history"] {
            let has: i64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name = 'allocation_bias'",
                        table
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has, 1, "{} missing allocation_bias column", table);
        }
    }

    #[test]
    fn test_allocation_bias_migration_adds_column_to_legacy_table() {
        // Simulate a legacy DB with the column missing.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE analyst_views (
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
            CREATE TABLE analyst_view_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL,
                key_evidence TEXT,
                blind_spots TEXT,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        // Pre-condition: column missing.
        let has_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('analyst_views') WHERE name = 'allocation_bias'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_before, 0);

        // Run migration.
        ensure_tables(&conn).unwrap();

        // Post-condition: column present on both tables.
        for table in ["analyst_views", "analyst_view_history"] {
            let has: i64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name = 'allocation_bias'",
                        table
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has, 1, "{} migration failed", table);
        }
        // Idempotent: running again should be a no-op (no errors).
        ensure_tables(&conn).unwrap();
    }

    #[test]
    fn test_allocation_bias_round_trip() {
        let conn = setup_db();
        upsert_view(
            &conn,
            "low",
            "BTC",
            "bull",
            3,
            "Momentum strong",
            Some("RSI 62"),
            Some("Whale selling"),
            Some("slight-overweight"),
        )
        .unwrap();

        let v = get_view(&conn, "low", "BTC").unwrap().unwrap();
        assert_eq!(v.allocation_bias.as_deref(), Some("slight-overweight"));

        let history = get_view_history(&conn, "BTC", None, None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].allocation_bias.as_deref(), Some("slight-overweight"));
    }

    #[test]
    fn test_validate_allocation_bias() {
        assert!(validate_allocation_bias(None).is_ok());
        for v in [
            "overweight",
            "slight-overweight",
            "at-target",
            "slight-underweight",
            "underweight",
        ] {
            assert!(validate_allocation_bias(Some(v)).is_ok(), "expected {} ok", v);
        }
        assert!(validate_allocation_bias(Some("neutral")).is_err());
        assert!(validate_allocation_bias(Some("")).is_err());
        assert!(validate_allocation_bias(Some("OVERWEIGHT")).is_err());
    }

    // -----------------------------------------------------------------------
    // classify_convergence — the 8 branches
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_insufficient_views() {
        assert_eq!(classify_convergence(0, 0.0, 0), "insufficient-views");
        assert_eq!(classify_convergence(1, 5.0, 0), "insufficient-views");
    }

    #[test]
    fn test_classify_divergent_when_spread_above_4() {
        // max_divergence > 4 short-circuits before any avg check
        assert_eq!(classify_convergence(2, 0.0, 5), "divergent");
        assert_eq!(classify_convergence(4, 3.0, 8), "divergent");
    }

    #[test]
    fn test_classify_neutral_with_divergence() {
        // max_divergence in (2, 4], avg in [-1, 1]
        assert_eq!(classify_convergence(2, 0.0, 3), "neutral-with-divergence");
        assert_eq!(classify_convergence(2, 1.0, 4), "neutral-with-divergence");
        assert_eq!(classify_convergence(2, -1.0, 3), "neutral-with-divergence");
    }

    #[test]
    fn test_classify_strong_convergent_bull() {
        assert_eq!(classify_convergence(2, 3.0, 0), "strong-convergent-bull");
        assert_eq!(classify_convergence(4, 4.5, 2), "strong-convergent-bull");
    }

    #[test]
    fn test_classify_convergent_bull() {
        assert_eq!(classify_convergence(2, 1.0, 0), "convergent-bull");
        assert_eq!(classify_convergence(3, 2.0, 3), "convergent-bull");
        // avg >= 3 but divergence too wide for strong → falls through to convergent-bull
        assert_eq!(classify_convergence(3, 3.0, 3), "convergent-bull");
    }

    #[test]
    fn test_classify_strong_convergent_bear() {
        assert_eq!(classify_convergence(2, -3.0, 0), "strong-convergent-bear");
        assert_eq!(classify_convergence(3, -4.0, 2), "strong-convergent-bear");
    }

    #[test]
    fn test_classify_convergent_bear() {
        assert_eq!(classify_convergence(2, -1.0, 0), "convergent-bear");
        assert_eq!(classify_convergence(3, -2.0, 3), "convergent-bear");
        // avg <= -3 but divergence > 2 → falls through to convergent-bear
        assert_eq!(classify_convergence(3, -3.0, 3), "convergent-bear");
    }

    #[test]
    fn test_classify_convergent_neutral_default() {
        // n_views >= 2, low divergence, avg in (-1, 1) excl
        assert_eq!(classify_convergence(2, 0.5, 0), "convergent-neutral");
        assert_eq!(classify_convergence(3, 0.0, 1), "convergent-neutral");
        // avg=-0.5 with no divergence
        assert_eq!(classify_convergence(2, -0.5, 2), "convergent-neutral");
    }

    // -----------------------------------------------------------------------
    // convergence aggregation end-to-end (8 branches via synthetic data)
    // -----------------------------------------------------------------------

    fn convergence_for(conn: &Connection, asset: &str) -> ConvergenceReport {
        let rows = load_convergence_rows_sqlite(conn, asset, None).unwrap();
        build_report_for_asset(&asset.to_uppercase(), rows, "test")
    }

    #[test]
    fn test_convergence_insufficient_views() {
        let conn = setup_db();
        // No views
        let r = convergence_for(&conn, "BTC");
        assert_eq!(r.stats.n_views, 0);
        assert_eq!(r.summary, "insufficient-views");

        // Exactly one view
        upsert_view(&conn, "low", "BTC", "bull", 3, "Reasoning", None, None, None).unwrap();
        let r = convergence_for(&conn, "BTC");
        assert_eq!(r.stats.n_views, 1);
        assert_eq!(r.summary, "insufficient-views");
    }

    #[test]
    fn test_convergence_strong_convergent_bull() {
        let conn = setup_db();
        upsert_view(&conn, "low", "GC=F", "bull", 4, "r", None, None, Some("overweight")).unwrap();
        upsert_view(&conn, "medium", "GC=F", "bull", 3, "r", None, None, Some("slight-overweight")).unwrap();
        upsert_view(&conn, "high", "GC=F", "bull", 5, "r", None, None, Some("overweight")).unwrap();

        let r = convergence_for(&conn, "GC=F");
        assert_eq!(r.stats.n_views, 3);
        assert_eq!(r.summary, "strong-convergent-bull");
        assert!(r.stats.avg_conviction >= 3.0);
        assert!(r.stats.max_divergence <= 2);
        // alloc bias buckets
        assert_eq!(r.stats.alloc_bias_counts.get("overweight").copied(), Some(2));
        assert_eq!(r.stats.alloc_bias_counts.get("slight-overweight").copied(), Some(1));
        assert_eq!(r.stats.alloc_bias_counts.get("null").copied(), Some(0));
    }

    #[test]
    fn test_convergence_convergent_bull() {
        let conn = setup_db();
        upsert_view(&conn, "low", "SLV", "bull", 2, "r", None, None, None).unwrap();
        upsert_view(&conn, "medium", "SLV", "bull", 1, "r", None, None, None).unwrap();

        let r = convergence_for(&conn, "SLV");
        assert_eq!(r.summary, "convergent-bull");
    }

    #[test]
    fn test_convergence_strong_convergent_bear() {
        let conn = setup_db();
        upsert_view(&conn, "low", "ARKK", "bear", -4, "r", None, None, Some("underweight")).unwrap();
        upsert_view(&conn, "medium", "ARKK", "bear", -3, "r", None, None, Some("underweight")).unwrap();
        upsert_view(&conn, "high", "ARKK", "bear", -3, "r", None, None, Some("underweight")).unwrap();

        let r = convergence_for(&conn, "ARKK");
        assert_eq!(r.summary, "strong-convergent-bear");
    }

    #[test]
    fn test_convergence_convergent_bear() {
        let conn = setup_db();
        upsert_view(&conn, "low", "QQQ", "bear", -2, "r", None, None, None).unwrap();
        upsert_view(&conn, "medium", "QQQ", "bear", -1, "r", None, None, None).unwrap();

        let r = convergence_for(&conn, "QQQ");
        assert_eq!(r.summary, "convergent-bear");
    }

    #[test]
    fn test_convergence_convergent_neutral() {
        let conn = setup_db();
        upsert_view(&conn, "low", "MID", "neutral", 0, "r", None, None, Some("at-target")).unwrap();
        upsert_view(&conn, "medium", "MID", "neutral", 1, "r", None, None, Some("at-target")).unwrap();

        let r = convergence_for(&conn, "MID");
        // avg 0.5, max_div 1 → falls to convergent-neutral
        assert_eq!(r.summary, "convergent-neutral");
    }

    #[test]
    fn test_convergence_divergent() {
        let conn = setup_db();
        // Spread 9 (>4) → divergent
        upsert_view(&conn, "low", "BTC", "bull", 5, "r", None, None, Some("overweight")).unwrap();
        upsert_view(&conn, "high", "BTC", "bear", -4, "r", None, None, Some("underweight")).unwrap();

        let r = convergence_for(&conn, "BTC");
        assert_eq!(r.summary, "divergent");
        assert_eq!(r.stats.max_divergence, 9);
    }

    #[test]
    fn test_convergence_neutral_with_divergence() {
        let conn = setup_db();
        // avg = 0, max_div = 4
        upsert_view(&conn, "low", "XX", "bull", 2, "r", None, None, None).unwrap();
        upsert_view(&conn, "high", "XX", "bear", -2, "r", None, None, None).unwrap();

        let r = convergence_for(&conn, "XX");
        // avg 0, divergence 4 → neutral-with-divergence
        assert_eq!(r.summary, "neutral-with-divergence");
    }

    #[test]
    fn test_convergence_dedupes_per_analyst_keeping_latest() {
        let conn = setup_db();
        // Same analyst writes twice — only latest should be in the report
        upsert_view(&conn, "low", "BTC", "bull", 2, "first", None, None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        upsert_view(&conn, "low", "BTC", "bear", -3, "second", None, None, None).unwrap();
        upsert_view(&conn, "medium", "BTC", "bear", -2, "r", None, None, None).unwrap();

        let r = convergence_for(&conn, "BTC");
        assert_eq!(r.stats.n_views, 2); // low (latest) + medium
        let low = r.views.iter().find(|v| v.analyst == "low").unwrap();
        assert_eq!(low.direction, "bear");
        assert_eq!(low.conviction, -3);
    }

    #[test]
    fn test_convergence_all_returns_one_per_distinct_asset() {
        let conn = setup_db();
        upsert_view(&conn, "low", "BTC", "bull", 3, "r", None, None, None).unwrap();
        upsert_view(&conn, "medium", "BTC", "bull", 2, "r", None, None, None).unwrap();
        upsert_view(&conn, "low", "GC=F", "bull", 4, "r", None, None, None).unwrap();
        upsert_view(&conn, "low", "SLV", "neutral", 0, "r", None, None, None).unwrap();

        let assets = load_convergence_assets_sqlite(&conn, None).unwrap();
        assert_eq!(assets, vec!["BTC", "GC=F", "SLV"]);
    }

    #[test]
    fn test_convergence_since_filter() {
        let conn = setup_db();
        // Insert old + new directly into history with explicit recorded_at timestamps.
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('low', 'BTC', 'bull', 3, 'old', '2020-01-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
             VALUES ('medium', 'BTC', 'bull', 4, 'fresh', ?)",
            params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        // Window = last 1 hour → only the fresh row qualifies
        let since = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let rows = load_convergence_rows_sqlite(&conn, "BTC", Some(&since)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "medium");

        // No window → both rows included
        let rows_all = load_convergence_rows_sqlite(&conn, "BTC", None).unwrap();
        assert_eq!(rows_all.len(), 2);
    }

    #[test]
    fn test_parse_since_variants() {
        // Relative suffixes parse without panicking.
        for s in ["24h", "7d", "2w", "3m"] {
            assert!(parse_since(s).is_ok(), "expected {} to parse", s);
        }
        // Absolute YYYY-MM-DD parses.
        assert!(parse_since("2025-01-01").is_ok());
        // Garbage rejected.
        assert!(parse_since("not-a-duration").is_err());
    }

    // -----------------------------------------------------------------------
    // conviction sign normalization (direction is authoritative)
    // -----------------------------------------------------------------------

    #[test]
    fn test_normalize_conviction_contradicting_signs_flipped_with_notice() {
        let (c, notice) = normalize_conviction("bear", 3);
        assert_eq!(c, -3);
        assert!(notice.is_some());

        let (c, notice) = normalize_conviction("bull", -3);
        assert_eq!(c, 3);
        assert!(notice.is_some());
    }

    #[test]
    fn test_normalize_conviction_matching_signs_pass_through() {
        assert_eq!(normalize_conviction("bear", -3), (-3, None));
        assert_eq!(normalize_conviction("bull", 4), (4, None));
        assert_eq!(normalize_conviction("bear", 0), (0, None));
        // Neutral never flips, regardless of sign.
        assert_eq!(normalize_conviction("neutral", 1), (1, None));
        assert_eq!(normalize_conviction("neutral", -1), (-1, None));
    }

    #[test]
    fn test_effective_conviction_direction_authoritative() {
        assert_eq!(effective_conviction("bear", 3), -3);
        assert_eq!(effective_conviction("bear", -3), -3);
        assert_eq!(effective_conviction("bull", -2), 2);
        assert_eq!(effective_conviction("bull", 2), 2);
        assert_eq!(effective_conviction("neutral", -1), -1);
    }

    #[test]
    fn test_upsert_backend_normalizes_contradicting_sign() {
        let conn = setup_db();
        let backend = crate::db::backend::BackendConnection::Sqlite { conn };
        // bear written with positive conviction (the live USD bug) must be
        // stored negative in both analyst_views and analyst_view_history.
        upsert_view_backend(&backend, "high", "USD", "bear", 3, "r", None, None, None).unwrap();
        let view = get_view_backend(&backend, "high", "USD").unwrap().unwrap();
        assert_eq!(view.direction, "bear");
        assert_eq!(view.conviction, -3);
        let conn = backend.sqlite_native().unwrap();
        let hist_conv: i64 = conn
            .query_row(
                "SELECT conviction FROM analyst_view_history WHERE analyst='high' AND asset='USD'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hist_conv, -3);
    }

    /// Regression: legacy rows wrote bear views with positive conviction;
    /// `classify_convergence` only saw the sign and labeled the asset
    /// convergent-bull. With direction-authoritative effective conviction,
    /// 2 bears (stored +3) + 1 bull (+1) + 1 neutral (0) must NOT classify
    /// as bullish.
    #[test]
    fn test_convergence_mixed_sign_legacy_rows_not_bullish() {
        let conn = setup_db();
        // Insert raw history rows, bypassing write-time normalization, to
        // simulate the pre-fix on-disk state.
        for (analyst, direction, conviction) in [
            ("high", "bear", 3_i64),
            ("macro", "bear", 3),
            ("low", "bull", 1),
            ("medium", "neutral", 0),
        ] {
            conn.execute(
                "INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary)
                 VALUES (?, 'USD', ?, ?, 'r')",
                params![analyst, direction, conviction],
            )
            .unwrap();
        }

        let r = convergence_for(&conn, "USD");
        assert_eq!(r.stats.n_views, 4);
        // Effective convictions: -3, -3, +1, 0 → avg -1.25, bearish lean.
        assert!(r.stats.avg_conviction < 0.0, "avg should be bearish, got {}", r.stats.avg_conviction);
        assert_ne!(r.summary, "convergent-bull");
        assert_ne!(r.summary, "strong-convergent-bull");
        // The bear views must surface with negative conviction.
        let high = r.views.iter().find(|v| v.analyst == "high").unwrap();
        assert_eq!(high.conviction, -3);
    }

    /// The one-time schema migration must flip contradicting signs in BOTH
    /// analyst_views and analyst_view_history, idempotently.
    #[test]
    fn test_migration_normalizes_existing_conviction_signs() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        ensure_tables(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO analyst_views (analyst, asset, direction, conviction, reasoning_summary)
             VALUES ('high', 'USD', 'bear', 3, 'r'),
                    ('macro', 'USD', 'bear', -2, 'r'),
                    ('low', 'BTC', 'bull', -4, 'r'),
                    ('medium', 'GLD', 'neutral', 1, 'r');
             INSERT INTO analyst_view_history (analyst, asset, direction, conviction, reasoning_summary)
             VALUES ('high', 'USD', 'bear', 3, 'r'),
                    ('low', 'BTC', 'bull', -4, 'r');",
        )
        .unwrap();

        // Re-run migrations: signs normalized where direction contradicts.
        crate::db::schema::run_migrations(&conn).unwrap();

        let read = |table: &str, analyst: &str, asset: &str| -> i64 {
            conn.query_row(
                &format!("SELECT conviction FROM {table} WHERE analyst=? AND asset=?"),
                params![analyst, asset],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(read("analyst_views", "high", "USD"), -3);
        assert_eq!(read("analyst_views", "macro", "USD"), -2); // already correct
        assert_eq!(read("analyst_views", "low", "BTC"), 4);
        assert_eq!(read("analyst_views", "medium", "GLD"), 1); // neutral untouched
        assert_eq!(read("analyst_view_history", "high", "USD"), -3);
        assert_eq!(read("analyst_view_history", "low", "BTC"), 4);

        // Idempotent: a second pass changes nothing.
        crate::db::schema::run_migrations(&conn).unwrap();
        assert_eq!(read("analyst_views", "high", "USD"), -3);
        assert_eq!(read("analyst_view_history", "low", "BTC"), 4);
    }
}
