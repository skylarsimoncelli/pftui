use anyhow::Result;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::Row as _;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    pub id: i64,
    pub name: String,
    pub timeframe: String,
    pub direction: String,
    pub conviction: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub asset_impact: Option<String>,
    pub key_signal: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendEvidence {
    pub id: i64,
    pub trend_id: i64,
    pub date: String,
    pub evidence: String,
    pub direction_impact: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAssetImpact {
    pub id: i64,
    pub trend_id: i64,
    pub symbol: String,
    pub impact: String,
    pub mechanism: Option<String>,
    pub timeframe: Option<String>,
    pub updated_at: String,
}

fn trend_from_row(row: &Row) -> Result<Trend, rusqlite::Error> {
    Ok(Trend {
        id: row.get(0)?,
        name: row.get(1)?,
        timeframe: row.get(2)?,
        direction: row.get(3)?,
        conviction: row.get(4)?,
        category: row.get(5)?,
        description: row.get(6)?,
        asset_impact: row.get(7)?,
        key_signal: row.get(8)?,
        status: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn evidence_from_row(row: &Row) -> Result<TrendEvidence, rusqlite::Error> {
    Ok(TrendEvidence {
        id: row.get(0)?,
        trend_id: row.get(1)?,
        date: row.get(2)?,
        evidence: row.get(3)?,
        direction_impact: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn impact_from_row(row: &Row) -> Result<TrendAssetImpact, rusqlite::Error> {
    Ok(TrendAssetImpact {
        id: row.get(0)?,
        trend_id: row.get(1)?,
        symbol: row.get(2)?,
        impact: row.get(3)?,
        mechanism: row.get(4)?,
        timeframe: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn add_trend(
    conn: &Connection,
    name: &str,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    category: Option<&str>,
    description: Option<&str>,
    asset_impact: Option<&str>,
    key_signal: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO trend_tracker
         (name, timeframe, direction, conviction, category, description, asset_impact, key_signal)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            name,
            timeframe.unwrap_or("high"),
            direction.unwrap_or("neutral"),
            conviction.unwrap_or("medium"),
            category,
            description,
            asset_impact,
            key_signal
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_trends(conn: &Connection, status: Option<&str>, category: Option<&str>) -> Result<Vec<Trend>> {
    let mut query = String::from(
        "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at, updated_at
         FROM trend_tracker",
    );
    let mut where_parts = Vec::new();
    if let Some(s) = status { where_parts.push(format!("status = '{}'", s.replace('"', "''"))); }
    if let Some(c) = category { where_parts.push(format!("category = '{}'", c.replace('"', "''"))); }
    if !where_parts.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&where_parts.join(" AND "));
    }
    query.push_str(" ORDER BY updated_at DESC");

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], trend_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn update_trend(
    conn: &Connection,
    name: &str,
    direction: Option<&str>,
    conviction: Option<&str>,
    description: Option<&str>,
    key_signal: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let mut updates = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(v) = direction { updates.push("direction = ?"); params_vec.push(Box::new(v.to_string())); }
    if let Some(v) = conviction { updates.push("conviction = ?"); params_vec.push(Box::new(v.to_string())); }
    if let Some(v) = description { updates.push("description = ?"); params_vec.push(Box::new(v.to_string())); }
    if let Some(v) = key_signal { updates.push("key_signal = ?"); params_vec.push(Box::new(v.to_string())); }
    if let Some(v) = status { updates.push("status = ?"); params_vec.push(Box::new(v.to_string())); }

    if updates.is_empty() { return Ok(()); }
    updates.push("updated_at = datetime('now')");

    let sql = format!("UPDATE trend_tracker SET {} WHERE name = ?", updates.join(", "));
    params_vec.push(Box::new(name.to_string()));
    let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

fn trend_id_by_name(conn: &Connection, name: &str) -> Result<i64> {
    let id = conn.query_row("SELECT id FROM trend_tracker WHERE name = ?", [name], |r| r.get(0))?;
    Ok(id)
}

pub fn add_evidence(
    conn: &Connection,
    trend_id: i64,
    date: &str,
    evidence: &str,
    direction_impact: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO trend_evidence (trend_id, date, evidence, direction_impact, source)
         VALUES (?, ?, ?, ?, ?)",
        params![trend_id, date, evidence, direction_impact, source],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn add_evidence_by_name(
    conn: &Connection,
    trend_name: &str,
    date: &str,
    evidence: &str,
    direction_impact: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    let trend_id = trend_id_by_name(conn, trend_name)?;
    add_evidence(conn, trend_id, date, evidence, direction_impact, source)
}

pub fn list_evidence(conn: &Connection, trend_id: i64, limit: Option<usize>) -> Result<Vec<TrendEvidence>> {
    let mut query = format!(
        "SELECT id, trend_id, date, evidence, direction_impact, source, created_at
         FROM trend_evidence
         WHERE trend_id = {}
         ORDER BY date DESC, created_at DESC",
        trend_id
    );
    if let Some(n) = limit { query.push_str(&format!(" LIMIT {}", n)); }

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map([], evidence_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn add_asset_impact(
    conn: &Connection,
    trend_id: i64,
    symbol: &str,
    impact: &str,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO trend_asset_impact (trend_id, symbol, impact, mechanism, timeframe)
         VALUES (?, ?, ?, ?, ?)",
        params![trend_id, symbol, impact, mechanism, timeframe],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn add_asset_impact_by_name(
    conn: &Connection,
    trend_name: &str,
    symbol: &str,
    impact: &str,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
) -> Result<i64> {
    let trend_id = trend_id_by_name(conn, trend_name)?;
    add_asset_impact(conn, trend_id, symbol, impact, mechanism, timeframe)
}

pub fn list_asset_impacts(conn: &Connection, trend_id: i64) -> Result<Vec<TrendAssetImpact>> {
    let mut stmt = conn.prepare(
        "SELECT id, trend_id, symbol, impact, mechanism, timeframe, updated_at
         FROM trend_asset_impact
         WHERE trend_id = ?
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([trend_id], impact_from_row)?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn get_impacts_for_symbol(conn: &Connection, symbol: &str) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.timeframe, t.direction, t.conviction, t.category, t.description, t.asset_impact, t.key_signal, t.status, t.created_at, t.updated_at,
                i.id, i.trend_id, i.symbol, i.impact, i.mechanism, i.timeframe, i.updated_at
         FROM trend_asset_impact i
         INNER JOIN trend_tracker t ON t.id = i.trend_id
         WHERE i.symbol = ?
         ORDER BY i.updated_at DESC",
    )?;

    let rows = stmt.query_map([symbol], |row| {
        let trend = Trend {
            id: row.get(0)?,
            name: row.get(1)?,
            timeframe: row.get(2)?,
            direction: row.get(3)?,
            conviction: row.get(4)?,
            category: row.get(5)?,
            description: row.get(6)?,
            asset_impact: row.get(7)?,
            key_signal: row.get(8)?,
            status: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        };
        let impact = TrendAssetImpact {
            id: row.get(12)?,
            trend_id: row.get(13)?,
            symbol: row.get(14)?,
            impact: row.get(15)?,
            mechanism: row.get(16)?,
            timeframe: row.get(17)?,
            updated_at: row.get(18)?,
        };
        Ok((trend, impact))
    })?;

    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn add_trend_backend(
    backend: &BackendConnection,
    name: &str,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    category: Option<&str>,
    description: Option<&str>,
    asset_impact: Option<&str>,
    key_signal: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| {
            add_trend(
                conn,
                name,
                timeframe,
                direction,
                conviction,
                category,
                description,
                asset_impact,
                key_signal,
            )
        },
        |pool| {
            add_trend_postgres(
                pool,
                name,
                timeframe,
                direction,
                conviction,
                category,
                description,
                asset_impact,
                key_signal,
            )
        },
    )
}

pub fn list_trends_backend(
    backend: &BackendConnection,
    status: Option<&str>,
    category: Option<&str>,
) -> Result<Vec<Trend>> {
    query::dispatch(
        backend,
        |conn| list_trends(conn, status, category),
        |pool| list_trends_postgres(pool, status, category),
    )
}

pub fn update_trend_backend(
    backend: &BackendConnection,
    name: &str,
    direction: Option<&str>,
    conviction: Option<&str>,
    description: Option<&str>,
    key_signal: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_trend(conn, name, direction, conviction, description, key_signal, status),
        |pool| update_trend_postgres(pool, name, direction, conviction, description, key_signal, status),
    )
}

pub fn add_evidence_by_name_backend(
    backend: &BackendConnection,
    trend_name: &str,
    date: &str,
    evidence: &str,
    direction_impact: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_evidence_by_name(conn, trend_name, date, evidence, direction_impact, source),
        |pool| add_evidence_by_name_postgres(pool, trend_name, date, evidence, direction_impact, source),
    )
}

pub fn list_evidence_backend(
    backend: &BackendConnection,
    trend_id: i64,
    limit: Option<usize>,
) -> Result<Vec<TrendEvidence>> {
    query::dispatch(
        backend,
        |conn| list_evidence(conn, trend_id, limit),
        |pool| list_evidence_postgres(pool, trend_id, limit),
    )
}

pub fn add_asset_impact_by_name_backend(
    backend: &BackendConnection,
    trend_name: &str,
    symbol: &str,
    impact: &str,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_asset_impact_by_name(conn, trend_name, symbol, impact, mechanism, timeframe),
        |pool| {
            add_asset_impact_by_name_postgres(pool, trend_name, symbol, impact, mechanism, timeframe)
        },
    )
}

pub fn list_asset_impacts_backend(
    backend: &BackendConnection,
    trend_id: i64,
) -> Result<Vec<TrendAssetImpact>> {
    query::dispatch(
        backend,
        |conn| list_asset_impacts(conn, trend_id),
        |pool| list_asset_impacts_postgres(pool, trend_id),
    )
}

pub fn get_impacts_for_symbol_backend(
    backend: &BackendConnection,
    symbol: &str,
) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    query::dispatch(
        backend,
        |conn| get_impacts_for_symbol(conn, symbol),
        |pool| get_impacts_for_symbol_postgres(pool, symbol),
    )
}

type TrendRow = (
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
);
type EvidenceRow = (i64, i64, String, String, Option<String>, Option<String>, String);
type ImpactRow = (i64, i64, String, String, Option<String>, Option<String>, String);
fn trend_from_pg_row(r: TrendRow) -> Trend {
    Trend {
        id: r.0,
        name: r.1,
        timeframe: r.2,
        direction: r.3,
        conviction: r.4,
        category: r.5,
        description: r.6,
        asset_impact: r.7,
        key_signal: r.8,
        status: r.9,
        created_at: r.10,
        updated_at: r.11,
    }
}

fn trend_id_by_name_postgres(pool: &PgPool, name: &str) -> Result<i64> {
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar("SELECT id FROM trend_tracker WHERE name = $1")
            .bind(name)
            .fetch_one(pool)
            .await
    })?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn add_trend_postgres(
    pool: &PgPool,
    name: &str,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    category: Option<&str>,
    description: Option<&str>,
    asset_impact: Option<&str>,
    key_signal: Option<&str>,
) -> Result<i64> {
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO trend_tracker
             (name, timeframe, direction, conviction, category, description, asset_impact, key_signal)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
        )
        .bind(name)
        .bind(timeframe.unwrap_or("high"))
        .bind(direction.unwrap_or("neutral"))
        .bind(conviction.unwrap_or("medium"))
        .bind(category)
        .bind(description)
        .bind(asset_impact)
        .bind(key_signal)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_trends_postgres(
    pool: &PgPool,
    status: Option<&str>,
    category: Option<&str>,
) -> Result<Vec<Trend>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<TrendRow> = match (status, category) {
        (Some(s), Some(c)) => runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at::text, updated_at::text
                 FROM trend_tracker
                 WHERE status = $1 AND category = $2
                 ORDER BY updated_at DESC",
            )
            .bind(s)
            .bind(c)
            .fetch_all(pool)
            .await
        })?,
        (Some(s), None) => runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at::text, updated_at::text
                 FROM trend_tracker
                 WHERE status = $1
                 ORDER BY updated_at DESC",
            )
            .bind(s)
            .fetch_all(pool)
            .await
        })?,
        (None, Some(c)) => runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at::text, updated_at::text
                 FROM trend_tracker
                 WHERE category = $1
                 ORDER BY updated_at DESC",
            )
            .bind(c)
            .fetch_all(pool)
            .await
        })?,
        (None, None) => runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at::text, updated_at::text
                 FROM trend_tracker
                 ORDER BY updated_at DESC",
            )
            .fetch_all(pool)
            .await
        })?,
    };
    Ok(rows.into_iter().map(trend_from_pg_row).collect())
}

fn update_trend_postgres(
    pool: &PgPool,
    name: &str,
    direction: Option<&str>,
    conviction: Option<&str>,
    description: Option<&str>,
    key_signal: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        if let Some(v) = direction {
            sqlx::query("UPDATE trend_tracker SET direction = $1, updated_at = NOW() WHERE name = $2")
                .bind(v)
                .bind(name)
                .execute(pool)
                .await?;
        }
        if let Some(v) = conviction {
            sqlx::query("UPDATE trend_tracker SET conviction = $1, updated_at = NOW() WHERE name = $2")
                .bind(v)
                .bind(name)
                .execute(pool)
                .await?;
        }
        if let Some(v) = description {
            sqlx::query("UPDATE trend_tracker SET description = $1, updated_at = NOW() WHERE name = $2")
                .bind(v)
                .bind(name)
                .execute(pool)
                .await?;
        }
        if let Some(v) = key_signal {
            sqlx::query("UPDATE trend_tracker SET key_signal = $1, updated_at = NOW() WHERE name = $2")
                .bind(v)
                .bind(name)
                .execute(pool)
                .await?;
        }
        if let Some(v) = status {
            sqlx::query("UPDATE trend_tracker SET status = $1, updated_at = NOW() WHERE name = $2")
                .bind(v)
                .bind(name)
                .execute(pool)
                .await?;
        }
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_evidence_by_name_postgres(
    pool: &PgPool,
    trend_name: &str,
    date: &str,
    evidence: &str,
    direction_impact: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    let trend_id = trend_id_by_name_postgres(pool, trend_name)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO trend_evidence (trend_id, date, evidence, direction_impact, source)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(trend_id)
        .bind(date)
        .bind(evidence)
        .bind(direction_impact)
        .bind(source)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_evidence_postgres(
    pool: &PgPool,
    trend_id: i64,
    limit: Option<usize>,
) -> Result<Vec<TrendEvidence>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<EvidenceRow> = if let Some(n) = limit {
        runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, trend_id, date, evidence, direction_impact, source, created_at::text
                 FROM trend_evidence
                 WHERE trend_id = $1
                 ORDER BY date DESC, created_at DESC
                 LIMIT $2",
            )
            .bind(trend_id)
            .bind(n as i64)
            .fetch_all(pool)
            .await
        })?
    } else {
        runtime.block_on(async {
            sqlx::query_as(
                "SELECT id, trend_id, date, evidence, direction_impact, source, created_at::text
                 FROM trend_evidence
                 WHERE trend_id = $1
                 ORDER BY date DESC, created_at DESC",
            )
            .bind(trend_id)
            .fetch_all(pool)
            .await
        })?
    };
    Ok(rows
        .into_iter()
        .map(|r| TrendEvidence {
            id: r.0,
            trend_id: r.1,
            date: r.2,
            evidence: r.3,
            direction_impact: r.4,
            source: r.5,
            created_at: r.6,
        })
        .collect())
}

fn add_asset_impact_by_name_postgres(
    pool: &PgPool,
    trend_name: &str,
    symbol: &str,
    impact: &str,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
) -> Result<i64> {
    let trend_id = trend_id_by_name_postgres(pool, trend_name)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO trend_asset_impact (trend_id, symbol, impact, mechanism, timeframe)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
        )
        .bind(trend_id)
        .bind(symbol)
        .bind(impact)
        .bind(mechanism)
        .bind(timeframe)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

fn list_asset_impacts_postgres(pool: &PgPool, trend_id: i64) -> Result<Vec<TrendAssetImpact>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<ImpactRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT id, trend_id, symbol, impact, mechanism, timeframe, updated_at::text
             FROM trend_asset_impact
             WHERE trend_id = $1
             ORDER BY updated_at DESC",
        )
        .bind(trend_id)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| TrendAssetImpact {
            id: r.0,
            trend_id: r.1,
            symbol: r.2,
            impact: r.3,
            mechanism: r.4,
            timeframe: r.5,
            updated_at: r.6,
        })
        .collect())
}

fn get_impacts_for_symbol_postgres(
    pool: &PgPool,
    symbol: &str,
) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query(
            "SELECT t.id, t.name, t.timeframe, t.direction, t.conviction, t.category, t.description, t.asset_impact, t.key_signal, t.status, t.created_at::text, t.updated_at::text,
                    i.id, i.trend_id, i.symbol, i.impact, i.mechanism, i.timeframe, i.updated_at::text
             FROM trend_asset_impact i
             INNER JOIN trend_tracker t ON t.id = i.trend_id
             WHERE i.symbol = $1
             ORDER BY i.updated_at DESC",
        )
        .bind(symbol)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                Trend {
                    id: r.get::<i64, _>(0),
                    name: r.get::<String, _>(1),
                    timeframe: r.get::<String, _>(2),
                    direction: r.get::<String, _>(3),
                    conviction: r.get::<String, _>(4),
                    category: r.get::<Option<String>, _>(5),
                    description: r.get::<Option<String>, _>(6),
                    asset_impact: r.get::<Option<String>, _>(7),
                    key_signal: r.get::<Option<String>, _>(8),
                    status: r.get::<String, _>(9),
                    created_at: r.get::<String, _>(10),
                    updated_at: r.get::<String, _>(11),
                },
                TrendAssetImpact {
                    id: r.get::<i64, _>(12),
                    trend_id: r.get::<i64, _>(13),
                    symbol: r.get::<String, _>(14),
                    impact: r.get::<String, _>(15),
                    mechanism: r.get::<Option<String>, _>(16),
                    timeframe: r.get::<Option<String>, _>(17),
                    updated_at: r.get::<String, _>(18),
                },
            )
        })
        .collect())
}
