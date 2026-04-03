use anyhow::Result;
use rusqlite::{params, Connection, Row as SqliteRow};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

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

impl Trend {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
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
}

impl TrendEvidence {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            trend_id: row.get(1)?,
            date: row.get(2)?,
            evidence: row.get(3)?,
            direction_impact: row.get(4)?,
            source: row.get(5)?,
            created_at: row.get(6)?,
        })
    }
}

impl TrendAssetImpact {
    fn from_row(row: &SqliteRow) -> Result<Self, rusqlite::Error> {
        Ok(Self {
            id: row.get(0)?,
            trend_id: row.get(1)?,
            symbol: row.get(2)?,
            impact: row.get(3)?,
            mechanism: row.get(4)?,
            timeframe: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }
}

// ───────────────────────────────────────────────────────────────────────────────
// Trend tracker CRUD
// ───────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn add_trend(
    conn: &Connection,
    name: &str,
    timeframe: &str,
    direction: &str,
    conviction: &str,
    category: Option<&str>,
    description: Option<&str>,
    asset_impact: Option<&str>,
    key_signal: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO trend_tracker (name, timeframe, direction, conviction, category, description, asset_impact, key_signal)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![name, timeframe, direction, conviction, category, description, asset_impact, key_signal],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
fn add_trend_postgres(
    pool: &PgPool,
    name: &str,
    timeframe: &str,
    direction: &str,
    conviction: &str,
    category: Option<&str>,
    description: Option<&str>,
    asset_impact: Option<&str>,
    key_signal: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        let row = sqlx::query(
            "INSERT INTO trend_tracker (name, timeframe, direction, conviction, category, description, asset_impact, key_signal)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(name)
        .bind(timeframe)
        .bind(direction)
        .bind(conviction)
        .bind(category)
        .bind(description)
        .bind(asset_impact)
        .bind(key_signal)
        .fetch_one(pool)
        .await?;
        Ok::<i64, sqlx::Error>(row.get(0))
    })?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub fn add_trend_backend(
    backend: &BackendConnection,
    name: &str,
    timeframe: &str,
    direction: &str,
    conviction: &str,
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

pub fn list_trends(
    conn: &Connection,
    status: Option<&str>,
    category: Option<&str>,
) -> Result<Vec<Trend>> {
    list_trends_filtered(conn, status, category, None, None, None, None)
}

/// List trends with full filter support: status, category, timeframe, direction, conviction, limit.
pub fn list_trends_filtered(
    conn: &Connection,
    status: Option<&str>,
    category: Option<&str>,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Trend>> {
    let mut sql = String::from(
        "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at, updated_at
         FROM trend_tracker WHERE 1=1",
    );
    let mut params_vec: Vec<String> = Vec::new();

    if let Some(s) = status {
        sql.push_str(" AND status = ?");
        params_vec.push(s.to_string());
    }
    if let Some(c) = category {
        sql.push_str(" AND category = ?");
        params_vec.push(c.to_string());
    }
    if let Some(tf) = timeframe {
        sql.push_str(" AND LOWER(timeframe) = LOWER(?)");
        params_vec.push(tf.to_string());
    }
    if let Some(dir) = direction {
        sql.push_str(" AND LOWER(direction) = LOWER(?)");
        params_vec.push(dir.to_string());
    }
    if let Some(conv) = conviction {
        sql.push_str(" AND LOWER(conviction) = LOWER(?)");
        params_vec.push(conv.to_string());
    }

    sql.push_str(" ORDER BY updated_at DESC");

    if let Some(lim) = limit {
        sql.push_str(&format!(" LIMIT {}", lim));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec
        .iter()
        .map(|p| p as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], Trend::from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn list_trends_postgres(
    pool: &PgPool,
    status: Option<&str>,
    category: Option<&str>,
) -> Result<Vec<Trend>> {
    list_trends_filtered_postgres(pool, status, category, None, None, None, None)
}

fn list_trends_filtered_postgres(
    pool: &PgPool,
    status: Option<&str>,
    category: Option<&str>,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Trend>> {
    crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT id, name, timeframe, direction, conviction, category, description, asset_impact, key_signal, status, created_at::text, updated_at::text
             FROM trend_tracker WHERE 1=1",
        );
        if let Some(s) = status {
            qb.push(" AND status = ");
            qb.push_bind(s);
        }
        if let Some(c) = category {
            qb.push(" AND category = ");
            qb.push_bind(c);
        }
        if let Some(tf) = timeframe {
            qb.push(" AND LOWER(timeframe) = LOWER(");
            qb.push_bind(tf);
            qb.push(")");
        }
        if let Some(dir) = direction {
            qb.push(" AND LOWER(direction) = LOWER(");
            qb.push_bind(dir);
            qb.push(")");
        }
        if let Some(conv) = conviction {
            qb.push(" AND LOWER(conviction) = LOWER(");
            qb.push_bind(conv);
            qb.push(")");
        }
        qb.push(" ORDER BY updated_at DESC");
        if let Some(lim) = limit {
            qb.push(format!(" LIMIT {}", lim));
        }

        let rows = qb.build().fetch_all(pool).await?;
        Ok::<Vec<Trend>, sqlx::Error>(
            rows.iter()
                .map(|r| Trend {
                    id: r.get(0),
                    name: r.get(1),
                    timeframe: r.get(2),
                    direction: r.get(3),
                    conviction: r.get(4),
                    category: r.get(5),
                    description: r.get(6),
                    asset_impact: r.get(7),
                    key_signal: r.get(8),
                    status: r.get(9),
                    created_at: r.get(10),
                    updated_at: r.get(11),
                })
                .collect(),
        )
    })
    .map_err(Into::into)
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

/// List trends with full filter support at the backend level.
pub fn list_trends_filtered_backend(
    backend: &BackendConnection,
    status: Option<&str>,
    category: Option<&str>,
    timeframe: Option<&str>,
    direction: Option<&str>,
    conviction: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Trend>> {
    query::dispatch(
        backend,
        |conn| list_trends_filtered(conn, status, category, timeframe, direction, conviction, limit),
        |pool| list_trends_filtered_postgres(pool, status, category, timeframe, direction, conviction, limit),
    )
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
    let mut params_vec: Vec<String> = Vec::new();

    if let Some(d) = direction {
        updates.push("direction = ?");
        params_vec.push(d.to_string());
    }
    if let Some(c) = conviction {
        updates.push("conviction = ?");
        params_vec.push(c.to_string());
    }
    if let Some(desc) = description {
        updates.push("description = ?");
        params_vec.push(desc.to_string());
    }
    if let Some(sig) = key_signal {
        updates.push("key_signal = ?");
        params_vec.push(sig.to_string());
    }
    if let Some(st) = status {
        updates.push("status = ?");
        params_vec.push(st.to_string());
    }
    updates.push("updated_at = datetime('now')");

    if params_vec.is_empty() && updates.len() == 1 {
        return Ok(());
    }

    let sql = format!(
        "UPDATE trend_tracker SET {} WHERE name = ?",
        updates.join(", ")
    );
    params_vec.push(name.to_string());

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec
        .iter()
        .map(|p| p as &dyn rusqlite::ToSql)
        .collect();
    conn.execute(&sql, &params_refs[..])?;
    Ok(())
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
    crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE trend_tracker SET ");
        let mut first = true;

        if let Some(d) = direction {
            if !first {
                qb.push(", ");
            }
            qb.push("direction = ");
            qb.push_bind(d);
            first = false;
        }
        if let Some(c) = conviction {
            if !first {
                qb.push(", ");
            }
            qb.push("conviction = ");
            qb.push_bind(c);
            first = false;
        }
        if let Some(desc) = description {
            if !first {
                qb.push(", ");
            }
            qb.push("description = ");
            qb.push_bind(desc);
            first = false;
        }
        if let Some(sig) = key_signal {
            if !first {
                qb.push(", ");
            }
            qb.push("key_signal = ");
            qb.push_bind(sig);
            first = false;
        }
        if let Some(st) = status {
            if !first {
                qb.push(", ");
            }
            qb.push("status = ");
            qb.push_bind(st);
            first = false;
        }

        if !first {
            qb.push(", ");
        }
        qb.push("updated_at = NOW()");

        qb.push(" WHERE name = ");
        qb.push_bind(name);

        qb.build().execute(pool).await?;
        Ok::<(), sqlx::Error>(())
    })
    .map_err(Into::into)
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
        |conn| {
            update_trend(
                conn,
                name,
                direction,
                conviction,
                description,
                key_signal,
                status,
            )
        },
        |pool| {
            update_trend_postgres(
                pool,
                name,
                direction,
                conviction,
                description,
                key_signal,
                status,
            )
        },
    )
}

// ───────────────────────────────────────────────────────────────────────────────
// Trend evidence CRUD
// ───────────────────────────────────────────────────────────────────────────────

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

fn add_evidence_postgres(
    pool: &PgPool,
    trend_id: i64,
    date: &str,
    evidence: &str,
    direction_impact: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        let row = sqlx::query(
            "INSERT INTO trend_evidence (trend_id, date, evidence, direction_impact, source)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(trend_id)
        .bind(date)
        .bind(evidence)
        .bind(direction_impact)
        .bind(source)
        .fetch_one(pool)
        .await?;
        Ok::<i64, sqlx::Error>(row.get(0))
    })?;
    Ok(id)
}

pub fn add_evidence_backend(
    backend: &BackendConnection,
    trend_id: i64,
    date: &str,
    evidence: &str,
    direction_impact: Option<&str>,
    source: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_evidence(conn, trend_id, date, evidence, direction_impact, source),
        |pool| add_evidence_postgres(pool, trend_id, date, evidence, direction_impact, source),
    )
}

pub fn list_evidence(
    conn: &Connection,
    trend_id: i64,
    limit: Option<usize>,
) -> Result<Vec<TrendEvidence>> {
    let sql = if let Some(lim) = limit {
        format!(
            "SELECT id, trend_id, date, evidence, direction_impact, source, created_at
             FROM trend_evidence WHERE trend_id = ? ORDER BY date DESC LIMIT {}",
            lim
        )
    } else {
        "SELECT id, trend_id, date, evidence, direction_impact, source, created_at
         FROM trend_evidence WHERE trend_id = ? ORDER BY date DESC"
            .to_string()
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![trend_id], TrendEvidence::from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn list_evidence_postgres(
    pool: &PgPool,
    trend_id: i64,
    limit: Option<usize>,
) -> Result<Vec<TrendEvidence>> {
    crate::db::pg_runtime::block_on(async {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT id, trend_id, date, evidence, direction_impact, source, created_at::text
             FROM trend_evidence WHERE trend_id = ",
        );
        qb.push_bind(trend_id);
        qb.push(" ORDER BY date DESC");
        if let Some(lim) = limit {
            qb.push(" LIMIT ");
            qb.push_bind(lim as i64);
        }

        let rows = qb.build().fetch_all(pool).await?;
        Ok::<Vec<TrendEvidence>, sqlx::Error>(
            rows.iter()
                .map(|r| TrendEvidence {
                    id: r.get(0),
                    trend_id: r.get(1),
                    date: r.get(2),
                    evidence: r.get(3),
                    direction_impact: r.get(4),
                    source: r.get(5),
                    created_at: r.get(6),
                })
                .collect(),
        )
    })
    .map_err(Into::into)
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

fn count_evidence(conn: &Connection, trend_id: i64) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trend_evidence WHERE trend_id = ?",
        params![trend_id],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

#[allow(dead_code)]
fn count_evidence_postgres(pool: &PgPool, trend_id: i64) -> Result<usize> {
    crate::db::pg_runtime::block_on(async {
        let row = sqlx::query("SELECT COUNT(*) FROM trend_evidence WHERE trend_id = $1")
            .bind(trend_id)
            .fetch_one(pool)
            .await?;
        let count: i64 = row.get(0);
        Ok::<usize, sqlx::Error>(count as usize)
    })
    .map_err(Into::into)
}

#[allow(dead_code)]
pub fn count_evidence_backend(
    backend: &BackendConnection,
    trend_id: i64,
) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| count_evidence(conn, trend_id),
        |pool| count_evidence_postgres(pool, trend_id),
    )
}

// ───────────────────────────────────────────────────────────────────────────────
// Trend asset impact CRUD
// ───────────────────────────────────────────────────────────────────────────────

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

fn add_asset_impact_postgres(
    pool: &PgPool,
    trend_id: i64,
    symbol: &str,
    impact: &str,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
) -> Result<i64> {
    let id: i64 = crate::db::pg_runtime::block_on(async {
        let row = sqlx::query(
            "INSERT INTO trend_asset_impact (trend_id, symbol, impact, mechanism, timeframe)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(trend_id)
        .bind(symbol)
        .bind(impact)
        .bind(mechanism)
        .bind(timeframe)
        .fetch_one(pool)
        .await?;
        Ok::<i64, sqlx::Error>(row.get(0))
    })?;
    Ok(id)
}

pub fn add_asset_impact_backend(
    backend: &BackendConnection,
    trend_id: i64,
    symbol: &str,
    impact: &str,
    mechanism: Option<&str>,
    timeframe: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_asset_impact(conn, trend_id, symbol, impact, mechanism, timeframe),
        |pool| add_asset_impact_postgres(pool, trend_id, symbol, impact, mechanism, timeframe),
    )
}

pub fn list_asset_impacts(conn: &Connection, trend_id: i64) -> Result<Vec<TrendAssetImpact>> {
    let mut stmt = conn.prepare(
        "SELECT id, trend_id, symbol, impact, mechanism, timeframe, updated_at
         FROM trend_asset_impact WHERE trend_id = ? ORDER BY symbol",
    )?;
    let rows = stmt.query_map(params![trend_id], TrendAssetImpact::from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn list_asset_impacts_postgres(pool: &PgPool, trend_id: i64) -> Result<Vec<TrendAssetImpact>> {
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT id, trend_id, symbol, impact, mechanism, timeframe, updated_at::text
             FROM trend_asset_impact WHERE trend_id = $1 ORDER BY symbol",
        )
        .bind(trend_id)
        .fetch_all(pool)
        .await?;

        Ok::<Vec<TrendAssetImpact>, sqlx::Error>(
            rows.iter()
                .map(|r| TrendAssetImpact {
                    id: r.get(0),
                    trend_id: r.get(1),
                    symbol: r.get(2),
                    impact: r.get(3),
                    mechanism: r.get(4),
                    timeframe: r.get(5),
                    updated_at: r.get(6),
                })
                .collect(),
        )
    })
    .map_err(Into::into)
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

// ───────────────────────────────────────────────────────────────────────────────
// Batch queries — fetch evidence/impacts for multiple trends in one query
// ───────────────────────────────────────────────────────────────────────────────

/// Batch-fetch evidence for multiple trends in one query, returning a map of trend_id -> Vec<TrendEvidence>.
/// Each trend's evidence is ordered by date DESC and limited to `per_trend_limit` entries if specified.
pub fn list_evidence_batch(
    conn: &Connection,
    trend_ids: &[i64],
    per_trend_limit: Option<usize>,
) -> Result<std::collections::HashMap<i64, Vec<TrendEvidence>>> {
    if trend_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Build query with IN clause
    let placeholders: Vec<&str> = trend_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT id, trend_id, date, evidence, direction_impact, source, created_at
         FROM trend_evidence WHERE trend_id IN ({}) ORDER BY trend_id, date DESC",
        placeholders.join(",")
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = trend_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], TrendEvidence::from_row)?;
    let all: Vec<TrendEvidence> = rows.collect::<Result<Vec<_>, _>>()?;

    let mut map: std::collections::HashMap<i64, Vec<TrendEvidence>> = std::collections::HashMap::new();
    for ev in all {
        map.entry(ev.trend_id).or_default().push(ev);
    }

    // Apply per-trend limit if specified
    if let Some(lim) = per_trend_limit {
        for entries in map.values_mut() {
            entries.truncate(lim);
        }
    }

    Ok(map)
}

fn list_evidence_batch_postgres(
    pool: &PgPool,
    trend_ids: &[i64],
    per_trend_limit: Option<usize>,
) -> Result<std::collections::HashMap<i64, Vec<TrendEvidence>>> {
    if trend_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT id, trend_id, date, evidence, direction_impact, source, created_at::text
             FROM trend_evidence WHERE trend_id = ANY($1) ORDER BY trend_id, date DESC",
        )
        .bind(trend_ids)
        .fetch_all(pool)
        .await?;

        let mut map: std::collections::HashMap<i64, Vec<TrendEvidence>> = std::collections::HashMap::new();
        for r in &rows {
            let ev = TrendEvidence {
                id: r.get(0),
                trend_id: r.get(1),
                date: r.get(2),
                evidence: r.get(3),
                direction_impact: r.get(4),
                source: r.get(5),
                created_at: r.get(6),
            };
            map.entry(ev.trend_id).or_default().push(ev);
        }

        if let Some(lim) = per_trend_limit {
            for entries in map.values_mut() {
                entries.truncate(lim);
            }
        }

        Ok::<std::collections::HashMap<i64, Vec<TrendEvidence>>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn list_evidence_batch_backend(
    backend: &BackendConnection,
    trend_ids: &[i64],
    per_trend_limit: Option<usize>,
) -> Result<std::collections::HashMap<i64, Vec<TrendEvidence>>> {
    query::dispatch(
        backend,
        |conn| list_evidence_batch(conn, trend_ids, per_trend_limit),
        |pool| list_evidence_batch_postgres(pool, trend_ids, per_trend_limit),
    )
}

/// Batch-fetch evidence counts for multiple trends in one query.
pub fn count_evidence_batch(
    conn: &Connection,
    trend_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    if trend_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<&str> = trend_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT trend_id, COUNT(*) FROM trend_evidence WHERE trend_id IN ({}) GROUP BY trend_id",
        placeholders.join(",")
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = trend_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], |row| {
        let trend_id: i64 = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok((trend_id, count as usize))
    })?;

    rows.collect::<Result<std::collections::HashMap<i64, usize>, _>>().map_err(Into::into)
}

fn count_evidence_batch_postgres(
    pool: &PgPool,
    trend_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    if trend_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT trend_id, COUNT(*) FROM trend_evidence WHERE trend_id = ANY($1) GROUP BY trend_id",
        )
        .bind(trend_ids)
        .fetch_all(pool)
        .await?;

        let map: std::collections::HashMap<i64, usize> = rows
            .iter()
            .map(|r| {
                let trend_id: i64 = r.get(0);
                let count: i64 = r.get(1);
                (trend_id, count as usize)
            })
            .collect();

        Ok::<std::collections::HashMap<i64, usize>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn count_evidence_batch_backend(
    backend: &BackendConnection,
    trend_ids: &[i64],
) -> Result<std::collections::HashMap<i64, usize>> {
    query::dispatch(
        backend,
        |conn| count_evidence_batch(conn, trend_ids),
        |pool| count_evidence_batch_postgres(pool, trend_ids),
    )
}

/// Batch-fetch asset impacts for multiple trends in one query.
pub fn list_asset_impacts_batch(
    conn: &Connection,
    trend_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<TrendAssetImpact>>> {
    if trend_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<&str> = trend_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "SELECT id, trend_id, symbol, impact, mechanism, timeframe, updated_at
         FROM trend_asset_impact WHERE trend_id IN ({}) ORDER BY trend_id, symbol",
        placeholders.join(",")
    );

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = trend_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(&params_refs[..], TrendAssetImpact::from_row)?;
    let all: Vec<TrendAssetImpact> = rows.collect::<Result<Vec<_>, _>>()?;

    let mut map: std::collections::HashMap<i64, Vec<TrendAssetImpact>> = std::collections::HashMap::new();
    for impact in all {
        map.entry(impact.trend_id).or_default().push(impact);
    }

    Ok(map)
}

fn list_asset_impacts_batch_postgres(
    pool: &PgPool,
    trend_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<TrendAssetImpact>>> {
    if trend_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT id, trend_id, symbol, impact, mechanism, timeframe, updated_at::text
             FROM trend_asset_impact WHERE trend_id = ANY($1) ORDER BY trend_id, symbol",
        )
        .bind(trend_ids)
        .fetch_all(pool)
        .await?;

        let mut map: std::collections::HashMap<i64, Vec<TrendAssetImpact>> = std::collections::HashMap::new();
        for r in &rows {
            let impact = TrendAssetImpact {
                id: r.get(0),
                trend_id: r.get(1),
                symbol: r.get(2),
                impact: r.get(3),
                mechanism: r.get(4),
                timeframe: r.get(5),
                updated_at: r.get(6),
            };
            map.entry(impact.trend_id).or_default().push(impact);
        }

        Ok::<std::collections::HashMap<i64, Vec<TrendAssetImpact>>, sqlx::Error>(map)
    })
    .map_err(Into::into)
}

pub fn list_asset_impacts_batch_backend(
    backend: &BackendConnection,
    trend_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<TrendAssetImpact>>> {
    query::dispatch(
        backend,
        |conn| list_asset_impacts_batch(conn, trend_ids),
        |pool| list_asset_impacts_batch_postgres(pool, trend_ids),
    )
}

pub fn get_impacts_for_symbol(
    conn: &Connection,
    symbol: &str,
) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.timeframe, t.direction, t.conviction, t.category, t.description, t.asset_impact, t.key_signal, t.status, t.created_at, t.updated_at,
                i.id, i.trend_id, i.symbol, i.impact, i.mechanism, i.timeframe, i.updated_at
         FROM trend_tracker t
         JOIN trend_asset_impact i ON t.id = i.trend_id
         WHERE i.symbol = ?
         ORDER BY t.updated_at DESC",
    )?;

    let rows = stmt.query_map(params![symbol], |row| {
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

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn list_all_impacts(conn: &Connection) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.timeframe, t.direction, t.conviction, t.category, t.description, t.asset_impact, t.key_signal, t.status, t.created_at, t.updated_at,
                i.id, i.trend_id, i.symbol, i.impact, i.mechanism, i.timeframe, i.updated_at
         FROM trend_tracker t
         JOIN trend_asset_impact i ON t.id = i.trend_id
         ORDER BY t.updated_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
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

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn get_impacts_for_symbol_postgres(
    pool: &PgPool,
    symbol: &str,
) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT t.id, t.name, t.timeframe, t.direction, t.conviction, t.category, t.description, t.asset_impact, t.key_signal, t.status, t.created_at::text, t.updated_at::text,
                    i.id, i.trend_id, i.symbol, i.impact, i.mechanism, i.timeframe, i.updated_at::text
             FROM trend_tracker t
             JOIN trend_asset_impact i ON t.id = i.trend_id
             WHERE i.symbol = $1
             ORDER BY t.updated_at DESC",
        )
        .bind(symbol)
        .fetch_all(pool)
        .await?;

        Ok::<Vec<(Trend, TrendAssetImpact)>, sqlx::Error>(
            rows.iter()
                .map(|r| {
                    let trend = Trend {
                        id: r.get(0),
                        name: r.get(1),
                        timeframe: r.get(2),
                        direction: r.get(3),
                        conviction: r.get(4),
                        category: r.get(5),
                        description: r.get(6),
                        asset_impact: r.get(7),
                        key_signal: r.get(8),
                        status: r.get(9),
                        created_at: r.get(10),
                        updated_at: r.get(11),
                    };
                    let impact = TrendAssetImpact {
                        id: r.get(12),
                        trend_id: r.get(13),
                        symbol: r.get(14),
                        impact: r.get(15),
                        mechanism: r.get(16),
                        timeframe: r.get(17),
                        updated_at: r.get(18),
                    };
                    (trend, impact)
                })
                .collect(),
        )
    })
    .map_err(Into::into)
}

fn list_all_impacts_postgres(pool: &PgPool) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT t.id, t.name, t.timeframe, t.direction, t.conviction, t.category, t.description, t.asset_impact, t.key_signal, t.status, t.created_at::text, t.updated_at::text,
                    i.id, i.trend_id, i.symbol, i.impact, i.mechanism, i.timeframe, i.updated_at::text
             FROM trend_tracker t
             JOIN trend_asset_impact i ON t.id = i.trend_id
             ORDER BY t.updated_at DESC",
        )
        .fetch_all(pool)
        .await?;

        Ok::<Vec<(Trend, TrendAssetImpact)>, sqlx::Error>(
            rows.iter()
                .map(|r| {
                    let trend = Trend {
                        id: r.get(0),
                        name: r.get(1),
                        timeframe: r.get(2),
                        direction: r.get(3),
                        conviction: r.get(4),
                        category: r.get(5),
                        description: r.get(6),
                        asset_impact: r.get(7),
                        key_signal: r.get(8),
                        status: r.get(9),
                        created_at: r.get(10),
                        updated_at: r.get(11),
                    };
                    let impact = TrendAssetImpact {
                        id: r.get(12),
                        trend_id: r.get(13),
                        symbol: r.get(14),
                        impact: r.get(15),
                        mechanism: r.get(16),
                        timeframe: r.get(17),
                        updated_at: r.get(18),
                    };
                    (trend, impact)
                })
                .collect(),
        )
    })
    .map_err(Into::into)
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

pub fn list_all_impacts_backend(
    backend: &BackendConnection,
) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    query::dispatch(backend, list_all_impacts, list_all_impacts_postgres)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn count_evidence_empty() {
        let conn = setup_db();
        let trend_id = add_trend(&conn, "Test Trend", "high", "stable", "medium", None, None, None, None).unwrap();
        assert_eq!(count_evidence(&conn, trend_id).unwrap(), 0);
    }

    #[test]
    fn count_evidence_multiple() {
        let conn = setup_db();
        let trend_id = add_trend(&conn, "Test Trend", "high", "stable", "medium", None, None, None, None).unwrap();
        add_evidence(&conn, trend_id, "2026-03-01", "First evidence", None, None).unwrap();
        add_evidence(&conn, trend_id, "2026-03-02", "Second evidence", Some("strengthens"), None).unwrap();
        add_evidence(&conn, trend_id, "2026-03-03", "Third evidence", Some("weakens"), Some("Reuters")).unwrap();
        assert_eq!(count_evidence(&conn, trend_id).unwrap(), 3);
    }

    #[test]
    fn count_evidence_different_trends_isolated() {
        let conn = setup_db();
        let t1 = add_trend(&conn, "Trend A", "high", "stable", "medium", None, None, None, None).unwrap();
        let t2 = add_trend(&conn, "Trend B", "low", "accelerating", "high", None, None, None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-01", "Evidence for A", None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-02", "More evidence for A", None, None).unwrap();
        add_evidence(&conn, t2, "2026-03-01", "Evidence for B", None, None).unwrap();
        assert_eq!(count_evidence(&conn, t1).unwrap(), 2);
        assert_eq!(count_evidence(&conn, t2).unwrap(), 1);
    }

    #[test]
    fn list_evidence_respects_limit() {
        let conn = setup_db();
        let trend_id = add_trend(&conn, "Test Trend", "high", "stable", "medium", None, None, None, None).unwrap();
        add_evidence(&conn, trend_id, "2026-03-01", "First", None, None).unwrap();
        add_evidence(&conn, trend_id, "2026-03-02", "Second", None, None).unwrap();
        add_evidence(&conn, trend_id, "2026-03-03", "Third", None, None).unwrap();
        let all = list_evidence(&conn, trend_id, None).unwrap();
        assert_eq!(all.len(), 3);
        let limited = list_evidence(&conn, trend_id, Some(1)).unwrap();
        assert_eq!(limited.len(), 1);
        // Most recent first (ORDER BY date DESC)
        assert_eq!(limited[0].evidence, "Third");
    }

    #[test]
    fn list_trends_filtered_by_timeframe() {
        let conn = setup_db();
        add_trend(&conn, "High TF Trend", "high", "accelerating", "high", None, None, None, None).unwrap();
        add_trend(&conn, "Low TF Trend", "low", "stable", "medium", None, None, None, None).unwrap();
        add_trend(&conn, "Medium TF Trend", "medium", "decelerating", "low", None, None, None, None).unwrap();

        let high_only = list_trends_filtered(&conn, None, None, Some("high"), None, None, None).unwrap();
        assert_eq!(high_only.len(), 1);
        assert_eq!(high_only[0].name, "High TF Trend");

        // Case-insensitive
        let low_upper = list_trends_filtered(&conn, None, None, Some("LOW"), None, None, None).unwrap();
        assert_eq!(low_upper.len(), 1);
        assert_eq!(low_upper[0].name, "Low TF Trend");
    }

    #[test]
    fn list_trends_filtered_by_direction() {
        let conn = setup_db();
        add_trend(&conn, "Accel Trend", "high", "accelerating", "high", None, None, None, None).unwrap();
        add_trend(&conn, "Stable Trend", "high", "stable", "medium", None, None, None, None).unwrap();

        let accel = list_trends_filtered(&conn, None, None, None, Some("accelerating"), None, None).unwrap();
        assert_eq!(accel.len(), 1);
        assert_eq!(accel[0].name, "Accel Trend");
    }

    #[test]
    fn list_trends_filtered_by_conviction() {
        let conn = setup_db();
        add_trend(&conn, "High Conv", "high", "stable", "high", None, None, None, None).unwrap();
        add_trend(&conn, "Low Conv", "low", "stable", "low", None, None, None, None).unwrap();

        let high_conv = list_trends_filtered(&conn, None, None, None, None, Some("high"), None).unwrap();
        assert_eq!(high_conv.len(), 1);
        assert_eq!(high_conv[0].name, "High Conv");
    }

    #[test]
    fn list_trends_filtered_combined() {
        let conn = setup_db();
        add_trend(&conn, "Match", "high", "accelerating", "high", Some("energy"), None, None, None).unwrap();
        add_trend(&conn, "Wrong TF", "low", "accelerating", "high", Some("energy"), None, None, None).unwrap();
        add_trend(&conn, "Wrong Dir", "high", "stable", "high", Some("energy"), None, None, None).unwrap();

        let results = list_trends_filtered(&conn, None, Some("energy"), Some("high"), Some("accelerating"), Some("high"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Match");
    }

    #[test]
    fn list_trends_filtered_respects_limit() {
        let conn = setup_db();
        add_trend(&conn, "Trend A", "high", "stable", "high", None, None, None, None).unwrap();
        add_trend(&conn, "Trend B", "high", "stable", "high", None, None, None, None).unwrap();
        add_trend(&conn, "Trend C", "high", "stable", "high", None, None, None, None).unwrap();

        let limited = list_trends_filtered(&conn, None, None, Some("high"), None, None, Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn list_trends_filtered_no_filters_returns_all() {
        let conn = setup_db();
        add_trend(&conn, "A", "high", "stable", "high", None, None, None, None).unwrap();
        add_trend(&conn, "B", "low", "accelerating", "low", None, None, None, None).unwrap();

        let all = list_trends_filtered(&conn, None, None, None, None, None, None).unwrap();
        assert_eq!(all.len(), 2);
    }

    // ───────────────────────────────────────────────────────────────────────
    // Batch query tests
    // ───────────────────────────────────────────────────────────────────────

    #[test]
    fn list_evidence_batch_empty_ids() {
        let conn = setup_db();
        let result = list_evidence_batch(&conn, &[], None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_evidence_batch_multiple_trends() {
        let conn = setup_db();
        let t1 = add_trend(&conn, "Trend A", "high", "stable", "medium", None, None, None, None).unwrap();
        let t2 = add_trend(&conn, "Trend B", "low", "accelerating", "high", None, None, None, None).unwrap();
        let t3 = add_trend(&conn, "Trend C", "medium", "stable", "low", None, None, None, None).unwrap();

        add_evidence(&conn, t1, "2026-03-01", "Evidence A1", None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-02", "Evidence A2", Some("strengthens"), None).unwrap();
        add_evidence(&conn, t2, "2026-03-01", "Evidence B1", None, None).unwrap();
        // t3 has no evidence

        let map = list_evidence_batch(&conn, &[t1, t2, t3], None).unwrap();
        assert_eq!(map.get(&t1).map(|v| v.len()).unwrap_or(0), 2);
        assert_eq!(map.get(&t2).map(|v| v.len()).unwrap_or(0), 1);
        assert_eq!(map.get(&t3).map(|v| v.len()).unwrap_or(0), 0);
    }

    #[test]
    fn list_evidence_batch_respects_per_trend_limit() {
        let conn = setup_db();
        let t1 = add_trend(&conn, "Trend A", "high", "stable", "medium", None, None, None, None).unwrap();

        add_evidence(&conn, t1, "2026-03-01", "First", None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-02", "Second", None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-03", "Third", None, None).unwrap();

        let map = list_evidence_batch(&conn, &[t1], Some(2)).unwrap();
        assert_eq!(map.get(&t1).map(|v| v.len()).unwrap_or(0), 2);
        // Most recent first (ORDER BY date DESC)
        assert_eq!(map[&t1][0].evidence, "Third");
        assert_eq!(map[&t1][1].evidence, "Second");
    }

    #[test]
    fn count_evidence_batch_multiple_trends() {
        let conn = setup_db();
        let t1 = add_trend(&conn, "Trend A", "high", "stable", "medium", None, None, None, None).unwrap();
        let t2 = add_trend(&conn, "Trend B", "low", "accelerating", "high", None, None, None, None).unwrap();

        add_evidence(&conn, t1, "2026-03-01", "E1", None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-02", "E2", None, None).unwrap();
        add_evidence(&conn, t1, "2026-03-03", "E3", None, None).unwrap();
        add_evidence(&conn, t2, "2026-03-01", "E1", None, None).unwrap();

        let counts = count_evidence_batch(&conn, &[t1, t2]).unwrap();
        assert_eq!(counts.get(&t1).copied().unwrap_or(0), 3);
        assert_eq!(counts.get(&t2).copied().unwrap_or(0), 1);
    }

    #[test]
    fn count_evidence_batch_empty_ids() {
        let conn = setup_db();
        let result = count_evidence_batch(&conn, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_asset_impacts_batch_multiple_trends() {
        let conn = setup_db();
        let t1 = add_trend(&conn, "Trend A", "high", "stable", "medium", None, None, None, None).unwrap();
        let t2 = add_trend(&conn, "Trend B", "low", "accelerating", "high", None, None, None, None).unwrap();
        let t3 = add_trend(&conn, "Trend C", "medium", "stable", "low", None, None, None, None).unwrap();

        add_asset_impact(&conn, t1, "BTC", "bullish", Some("store of value"), None).unwrap();
        add_asset_impact(&conn, t1, "GC=F", "bullish", Some("safe haven"), None).unwrap();
        add_asset_impact(&conn, t2, "SPY", "bearish", Some("risk off"), None).unwrap();
        // t3 has no impacts

        let map = list_asset_impacts_batch(&conn, &[t1, t2, t3]).unwrap();
        assert_eq!(map.get(&t1).map(|v| v.len()).unwrap_or(0), 2);
        assert_eq!(map.get(&t2).map(|v| v.len()).unwrap_or(0), 1);
        assert_eq!(map.get(&t3).map(|v| v.len()).unwrap_or(0), 0);
    }

    #[test]
    fn list_asset_impacts_batch_empty_ids() {
        let conn = setup_db();
        let result = list_asset_impacts_batch(&conn, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_asset_impacts_batch_preserves_symbol_order() {
        let conn = setup_db();
        let t1 = add_trend(&conn, "Trend A", "high", "stable", "medium", None, None, None, None).unwrap();

        add_asset_impact(&conn, t1, "GC=F", "bullish", None, None).unwrap();
        add_asset_impact(&conn, t1, "BTC", "bullish", None, None).unwrap();
        add_asset_impact(&conn, t1, "SPY", "bearish", None, None).unwrap();

        let map = list_asset_impacts_batch(&conn, &[t1]).unwrap();
        let impacts = &map[&t1];
        assert_eq!(impacts.len(), 3);
        // ORDER BY symbol
        assert_eq!(impacts[0].symbol, "BTC");
        assert_eq!(impacts[1].symbol, "GC=F");
        assert_eq!(impacts[2].symbol, "SPY");
    }
}
