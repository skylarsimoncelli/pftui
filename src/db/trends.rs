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
        |conn| add_trend(conn, name, timeframe, direction, conviction, category, description, asset_impact, key_signal),
        |pool| add_trend_postgres(pool, name, timeframe, direction, conviction, category, description, asset_impact, key_signal),
    )
}

pub fn list_trends(
    conn: &Connection,
    status: Option<&str>,
    category: Option<&str>,
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

    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
    let rows = stmt.query_map(&params_refs[..], Trend::from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn list_trends_postgres(
    pool: &PgPool,
    status: Option<&str>,
    category: Option<&str>,
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
        qb.push(" ORDER BY updated_at DESC");

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

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
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
        |conn| update_trend(conn, name, direction, conviction, description, key_signal, status),
        |pool| update_trend_postgres(pool, name, direction, conviction, description, key_signal, status),
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

pub fn list_evidence(conn: &Connection, trend_id: i64, limit: Option<usize>) -> Result<Vec<TrendEvidence>> {
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

fn list_evidence_postgres(pool: &PgPool, trend_id: i64, limit: Option<usize>) -> Result<Vec<TrendEvidence>> {
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

pub fn list_evidence_backend(backend: &BackendConnection, trend_id: i64, limit: Option<usize>) -> Result<Vec<TrendEvidence>> {
    query::dispatch(
        backend,
        |conn| list_evidence(conn, trend_id, limit),
        |pool| list_evidence_postgres(pool, trend_id, limit),
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

pub fn list_asset_impacts_backend(backend: &BackendConnection, trend_id: i64) -> Result<Vec<TrendAssetImpact>> {
    query::dispatch(
        backend,
        |conn| list_asset_impacts(conn, trend_id),
        |pool| list_asset_impacts_postgres(pool, trend_id),
    )
}

pub fn get_impacts_for_symbol(conn: &Connection, symbol: &str) -> Result<Vec<(Trend, TrendAssetImpact)>> {
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

fn get_impacts_for_symbol_postgres(pool: &PgPool, symbol: &str) -> Result<Vec<(Trend, TrendAssetImpact)>> {
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

pub fn get_impacts_for_symbol_backend(backend: &BackendConnection, symbol: &str) -> Result<Vec<(Trend, TrendAssetImpact)>> {
    query::dispatch(
        backend,
        |conn| get_impacts_for_symbol(conn, symbol),
        |pool| get_impacts_for_symbol_postgres(pool, symbol),
    )
}
