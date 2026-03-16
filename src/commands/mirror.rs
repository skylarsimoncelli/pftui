use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, types::Value as SqlValue, Connection, OptionalExtension};
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use crate::cli::MirrorCommand;
use crate::config::{save_config, Config, DatabaseBackend};

const STARTUP_SYNC_MIN_INTERVAL_SECS: i64 = 300;

const FULL_SYNC_TABLES: &[&str] = &[
    "transactions",
    "watchlist",
    "alerts",
    "portfolio_allocations",
    "groups",
    "group_members",
    "dividends",
    "trend_evidence",
];

#[derive(Debug, Clone)]
struct ColumnMeta {
    name: String,
    data_type: String,
}

#[derive(Debug, Clone)]
struct TableMeta {
    columns: Vec<ColumnMeta>,
}

#[derive(Debug, Clone)]
struct SyncState {
    strategy: String,
    watermark: Option<String>,
}

#[derive(Debug, Clone)]
enum TableSyncStrategy {
    Full,
    Watermark { column: String, data_type: String },
}

pub fn run(config: &Config, sqlite_path: &Path, command: &MirrorCommand) -> Result<()> {
    match command {
        MirrorCommand::Sync {
            source_url,
            activate,
        } => run_sync(config, sqlite_path, source_url.as_deref(), *activate),
    }
}

pub fn sync_and_activate(config: &Config, sqlite_path: &Path, source_url: &str) -> Result<()> {
    run_sync(config, sqlite_path, Some(source_url), true)
}

#[allow(dead_code)]
pub fn sync_on_startup_if_needed(config: &Config, sqlite_path: &Path) -> Result<()> {
    if config.database_backend != DatabaseBackend::Sqlite {
        return Ok(());
    }
    if config
        .mirror_source_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return Ok(());
    }

    let started = Instant::now();
    eprintln!("Syncing local mirror from remote source...");
    match run_sync(config, sqlite_path, None, false) {
        Ok(()) => {
            eprintln!(
                "Local mirror updated in {:.2}s.",
                started.elapsed().as_secs_f64()
            );
            Ok(())
        }
        Err(err) => {
            eprintln!(
                "Mirror sync failed; continuing with existing local mirror: {}",
                err
            );
            Ok(())
        }
    }
}

pub fn spawn_startup_sync_if_needed(config: &Config, sqlite_path: &Path) {
    if config.database_backend != DatabaseBackend::Sqlite {
        return;
    }
    if config
        .mirror_source_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .is_none()
    {
        return;
    }
    if !should_run_startup_sync(sqlite_path) {
        return;
    }

    let cfg = config.clone();
    let path = sqlite_path.to_path_buf();
    std::thread::spawn(move || {
        let started = Instant::now();
        eprintln!("Background mirror sync started...");
        match run_sync(&cfg, &path, None, false) {
            Ok(()) => {
                eprintln!(
                    "Background mirror sync finished in {:.2}s.",
                    started.elapsed().as_secs_f64()
                );
            }
            Err(err) => {
                eprintln!("Background mirror sync failed: {}", err);
            }
        }
    });
}

fn run_sync(
    config: &Config,
    sqlite_path: &Path,
    source_url: Option<&str>,
    activate: bool,
) -> Result<()> {
    let remote_url = resolve_source_url(config, source_url)?;
    let remote_cfg = Config {
        database_backend: DatabaseBackend::Postgres,
        database_url: Some(remote_url.clone()),
        mirror_source_url: None,
        postgres_read_only: true,
        postgres_max_connections: 1,
        postgres_connect_timeout_secs: 30,
        ..Config::default()
    };
    let remote_backend = crate::db::backend::open_from_config(&remote_cfg, sqlite_path)
        .context("Failed to open remote Postgres mirror source")?;
    let pool = remote_backend
        .postgres_pool()
        .ok_or_else(|| anyhow!("Mirror source did not open as PostgreSQL"))?;

    let tables = list_remote_tables(pool)?;
    let remote_meta = list_remote_columns(pool)?;
    let local = crate::db::open_db(sqlite_path)?;
    let stats = sync_tables(pool, &local, &tables, &remote_meta)?;

    if activate {
        let mut next = config.clone();
        next.database_backend = DatabaseBackend::Sqlite;
        next.database_url = None;
        next.mirror_source_url = Some(remote_url);
        next.postgres_read_only = false;
        save_config(&next)?;
    }

    println!(
        "Mirror sync complete: {} tables updated, {} skipped ({} rows applied) into {}{}",
        stats.tables,
        stats.skipped,
        stats.rows,
        sqlite_path.display(),
        if activate {
            " (activated local SQLite mirror)"
        } else {
            ""
        }
    );
    Ok(())
}

#[derive(Debug, Default)]
struct SyncStats {
    tables: usize,
    rows: usize,
    skipped: usize,
}

#[derive(Debug, Default)]
struct TableSyncResult {
    rows: usize,
    watermark: Option<String>,
}

#[derive(Debug, Clone)]
struct TableSyncPlan {
    name: String,
    pk_columns: Vec<String>,
    strategy: TableSyncStrategy,
    state: Option<SyncState>,
    use_full_sync: bool,
}

fn resolve_source_url(config: &Config, source_url: Option<&str>) -> Result<String> {
    if let Some(url) = source_url.map(str::trim).filter(|v| !v.is_empty()) {
        return Ok(url.to_string());
    }
    if let Some(url) = config
        .mirror_source_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(url.to_string());
    }
    if config.database_backend == DatabaseBackend::Postgres {
        if let Some(url) = config
            .database_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return Ok(url.to_string());
        }
    }
    bail!("No mirror source URL configured. Use --source-url or set mirror_source_url.");
}

fn list_remote_tables(pool: &PgPool) -> Result<Vec<String>> {
    crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "SELECT table_name
             FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_type = 'BASE TABLE'
             ORDER BY table_name ASC",
        )
        .fetch_all(pool)
        .await
    })
    .map_err(Into::into)
}

fn list_remote_columns(pool: &PgPool) -> Result<HashMap<String, TableMeta>> {
    crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(
            "SELECT table_name, column_name, data_type
             FROM information_schema.columns
             WHERE table_schema = 'public'
             ORDER BY table_name ASC, ordinal_position ASC",
        )
        .fetch_all(pool)
        .await?;

        let mut out: HashMap<String, TableMeta> = HashMap::new();
        for row in rows {
            let table_name: String = row.get(0);
            let column = ColumnMeta {
                name: row.get(1),
                data_type: row.get(2),
            };
            out.entry(table_name)
                .or_insert_with(|| TableMeta {
                    columns: Vec::new(),
                })
                .columns
                .push(column);
        }
        Ok::<HashMap<String, TableMeta>, sqlx::Error>(out)
    })
    .map_err(Into::into)
}

fn sync_tables(
    pool: &PgPool,
    local: &Connection,
    tables: &[String],
    remote_meta: &HashMap<String, TableMeta>,
) -> Result<SyncStats> {
    ensure_sync_state_table(local)?;
    let local_tables = list_local_tables(local)?;
    let states = load_sync_states(local)?;
    let mut stats = SyncStats::default();
    let mut plans = Vec::new();

    for table in tables {
        if !local_tables.contains(table.as_str()) {
            continue;
        }
        let Some(meta) = remote_meta.get(table) else {
            continue;
        };
        let pk_columns = list_local_pk_columns(local, table)?;
        let strategy = choose_strategy(table, meta, &pk_columns);
        let state = states.get(table);
        let use_full_sync = matches!(strategy, TableSyncStrategy::Full)
            || state
                .map(|s| s.strategy.as_str() != strategy_name(&strategy))
                .unwrap_or(true);
        plans.push(TableSyncPlan {
            name: table.clone(),
            pk_columns,
            strategy,
            state: state.cloned(),
            use_full_sync,
        });
    }

    let watermark_plans = plans
        .iter()
        .filter(|plan| {
            !plan.use_full_sync && matches!(plan.strategy, TableSyncStrategy::Watermark { .. })
        })
        .collect::<Vec<_>>();
    let remote_watermarks = fetch_remote_max_watermarks(pool, &watermark_plans)?;

    for plan in &plans {
        if !plan.use_full_sync {
            if let (Some(state), Some(remote_watermark)) =
                (plan.state.as_ref(), remote_watermarks.get(&plan.name))
            {
                if state.watermark.as_deref() == remote_watermark.as_deref() {
                    save_sync_state(
                        local,
                        &plan.name,
                        strategy_name(&plan.strategy),
                        remote_watermark.clone(),
                    )?;
                    stats.skipped += 1;
                    continue;
                }
            }
        }

        let meta = remote_meta
            .get(&plan.name)
            .ok_or_else(|| anyhow!("Missing metadata for mirrored table {}", plan.name))?;
        let result = if plan.use_full_sync {
            full_sync_table(local, pool, &plan.name, meta, &plan.strategy)?
        } else {
            incremental_sync_table(
                local,
                pool,
                &plan.name,
                meta,
                &plan.pk_columns,
                &plan.strategy,
                plan.state.as_ref(),
            )?
        };

        save_sync_state(
            local,
            &plan.name,
            strategy_name(&plan.strategy),
            result.watermark,
        )?;
        stats.tables += 1;
        stats.rows += result.rows;
    }

    Ok(stats)
}

fn ensure_sync_state_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mirror_sync_state (
            table_name TEXT PRIMARY KEY,
            strategy TEXT NOT NULL,
            watermark TEXT,
            last_synced_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    Ok(())
}

fn load_sync_states(conn: &Connection) -> Result<HashMap<String, SyncState>> {
    let mut stmt = conn.prepare(
        "SELECT table_name, strategy, watermark
         FROM mirror_sync_state",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SyncState {
                    strategy: row.get(1)?,
                    watermark: row.get(2)?,
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().collect())
}

fn save_sync_state(
    conn: &Connection,
    table: &str,
    strategy: &str,
    watermark: Option<String>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO mirror_sync_state (table_name, strategy, watermark, last_synced_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(table_name) DO UPDATE SET
            strategy = excluded.strategy,
            watermark = excluded.watermark,
            last_synced_at = excluded.last_synced_at",
        params![table, strategy, watermark],
    )?;
    Ok(())
}

fn list_local_tables(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT name
         FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'",
    )?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names.into_iter().collect())
}

fn list_local_pk_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let pragma = format!("PRAGMA table_info({})", quote_ident(table));
    let mut stmt = conn.prepare(&pragma)?;
    let mut rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.sort_by_key(|(_, order)| *order);
    Ok(rows
        .into_iter()
        .filter(|(_, order)| *order > 0)
        .map(|(name, _)| name)
        .collect())
}

fn choose_strategy(table: &str, meta: &TableMeta, pk_columns: &[String]) -> TableSyncStrategy {
    if FULL_SYNC_TABLES.contains(&table) {
        return TableSyncStrategy::Full;
    }

    for candidate in [
        "updated_at",
        "fetched_at",
        "recorded_at",
        "created_at",
        "published_at",
        "timestamp",
        "date",
        "id",
    ] {
        if let Some(col) = meta.columns.iter().find(|c| c.name == candidate) {
            if candidate == "id"
                && !(pk_columns.len() == 1
                    && pk_columns[0] == "id"
                    && is_integer_type(&col.data_type))
            {
                continue;
            }
            return TableSyncStrategy::Watermark {
                column: col.name.clone(),
                data_type: col.data_type.clone(),
            };
        }
    }

    TableSyncStrategy::Full
}

fn strategy_name(strategy: &TableSyncStrategy) -> &'static str {
    match strategy {
        TableSyncStrategy::Full => "full",
        TableSyncStrategy::Watermark { .. } => "watermark",
    }
}

fn full_sync_table(
    local: &Connection,
    pool: &PgPool,
    table: &str,
    meta: &TableMeta,
    strategy: &TableSyncStrategy,
) -> Result<TableSyncResult> {
    let rows = fetch_full_rows(pool, table)?;
    let watermark = watermark_for_rows_data(&rows, strategy);
    let tx = local.unchecked_transaction()?;
    tx.execute(&format!("DELETE FROM {}", quote_ident(table)), [])?;
    let _ = tx.execute(
        "DELETE FROM sqlite_sequence WHERE name = ?1",
        params![table],
    );
    insert_rows(&tx, table, &meta.columns, &[], rows.as_slice())?;
    tx.commit()?;
    Ok(TableSyncResult {
        rows: rows.len(),
        watermark,
    })
}

fn incremental_sync_table(
    local: &Connection,
    pool: &PgPool,
    table: &str,
    meta: &TableMeta,
    pk_columns: &[String],
    strategy: &TableSyncStrategy,
    state: Option<&SyncState>,
) -> Result<TableSyncResult> {
    let Some(state) = state else {
        return full_sync_table(local, pool, table, meta, strategy);
    };

    let rows = fetch_incremental_rows(pool, table, strategy, state.watermark.as_deref())?;
    if rows.is_empty() {
        return Ok(TableSyncResult {
            rows: 0,
            watermark: state.watermark.clone(),
        });
    }

    let watermark = watermark_for_rows_data(&rows, strategy);
    let tx = local.unchecked_transaction()?;
    insert_rows(&tx, table, &meta.columns, pk_columns, rows.as_slice())?;
    tx.commit()?;
    Ok(TableSyncResult {
        rows: rows.len(),
        watermark,
    })
}

fn fetch_full_rows(pool: &PgPool, table: &str) -> Result<Vec<Map<String, Value>>> {
    let sql = format!(
        "SELECT row_to_json(t)::text FROM (SELECT * FROM {} ORDER BY 1) t",
        quote_ident(table)
    );
    fetch_json_rows(pool, &sql, None)
}

fn fetch_incremental_rows(
    pool: &PgPool,
    table: &str,
    strategy: &TableSyncStrategy,
    watermark: Option<&str>,
) -> Result<Vec<Map<String, Value>>> {
    let Some(watermark) = watermark.filter(|v| !v.is_empty()) else {
        return fetch_full_rows(pool, table);
    };

    let TableSyncStrategy::Watermark { column, data_type } = strategy else {
        return fetch_full_rows(pool, table);
    };

    let sql = format!(
        "SELECT row_to_json(t)::text FROM (
            SELECT * FROM {}
            WHERE {} >= $1{}
            ORDER BY {}
        ) t",
        quote_ident(table),
        quote_ident(column),
        pg_cast_suffix(data_type),
        quote_ident(column),
    );
    let bind = if is_integer_type(data_type) {
        Some(BindValue::I64(watermark.parse::<i64>().with_context(
            || format!("Invalid integer watermark '{}' for {}", watermark, table),
        )?))
    } else {
        Some(BindValue::Text(watermark.to_string()))
    };
    fetch_json_rows(pool, &sql, bind)
}

fn fetch_remote_max_watermarks(
    pool: &PgPool,
    plans: &[&TableSyncPlan],
) -> Result<HashMap<String, Option<String>>> {
    if plans.is_empty() {
        return Ok(HashMap::new());
    }

    let mut sql_parts = Vec::with_capacity(plans.len());
    let mut data_types = HashMap::with_capacity(plans.len());
    for plan in plans {
        let TableSyncStrategy::Watermark { column, data_type } = &plan.strategy else {
            continue;
        };
        data_types.insert(plan.name.clone(), data_type.clone());
        sql_parts.push(format!(
            "SELECT {} AS table_name, MAX({})::text AS watermark FROM {}",
            quote_literal(&plan.name),
            quote_ident(column),
            quote_ident(&plan.name),
        ));
    }

    let sql = sql_parts.join(" UNION ALL ");
    let rows: Vec<(String, Option<String>)> = crate::db::pg_runtime::block_on(async {
        let rows = sqlx::query(&sql).fetch_all(pool).await?;
        Ok::<_, sqlx::Error>(
            rows.into_iter()
                .map(|row| (row.get(0), row.get(1)))
                .collect::<Vec<(String, Option<String>)>>(),
        )
    })?;

    let mut watermarks = HashMap::with_capacity(rows.len());
    for (table, watermark) in rows {
        let data_type = data_types
            .get(&table)
            .ok_or_else(|| anyhow!("Missing watermark data type for table {}", table))?;
        watermarks.insert(
            table,
            watermark.map(|value| normalize_watermark(value, data_type)),
        );
    }
    Ok(watermarks)
}

enum BindValue {
    Text(String),
    I64(i64),
}

fn fetch_json_rows(
    pool: &PgPool,
    sql: &str,
    bind: Option<BindValue>,
) -> Result<Vec<Map<String, Value>>> {
    let raw: Vec<String> = crate::db::pg_runtime::block_on(async {
        match bind {
            Some(BindValue::Text(value)) => {
                sqlx::query_scalar(sql).bind(value).fetch_all(pool).await
            }
            Some(BindValue::I64(value)) => {
                sqlx::query_scalar(sql).bind(value).fetch_all(pool).await
            }
            None => sqlx::query_scalar(sql).fetch_all(pool).await,
        }
    })?;

    raw.into_iter()
        .map(|row| serde_json::from_str::<Map<String, Value>>(&row).map_err(Into::into))
        .collect()
}

fn insert_rows(
    conn: &Connection,
    table: &str,
    columns: &[ColumnMeta],
    pk_columns: &[String],
    rows: &[Map<String, Value>],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let col_sql = columns
        .iter()
        .map(|c| quote_ident(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=columns.len())
        .map(|i| format!("?{}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let mut insert_sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_ident(table),
        col_sql,
        placeholders
    );

    if !pk_columns.is_empty() {
        let update_cols = columns
            .iter()
            .filter(|c| !pk_columns.iter().any(|pk| pk == &c.name))
            .map(|c| {
                format!(
                    "{} = excluded.{}",
                    quote_ident(&c.name),
                    quote_ident(&c.name)
                )
            })
            .collect::<Vec<_>>();
        if !update_cols.is_empty() {
            insert_sql.push_str(&format!(
                " ON CONFLICT ({}) DO UPDATE SET {}",
                pk_columns
                    .iter()
                    .map(|pk| quote_ident(pk))
                    .collect::<Vec<_>>()
                    .join(", "),
                update_cols.join(", ")
            ));
        } else {
            insert_sql.push_str(" ON CONFLICT DO NOTHING");
        }
    }

    let mut stmt = conn.prepare(&insert_sql)?;
    for row in rows {
        let values = columns
            .iter()
            .map(|col| json_to_sql_value(row.get(&col.name), &col.data_type))
            .collect::<Vec<_>>();
        stmt.execute(rusqlite::params_from_iter(values))?;
    }
    Ok(())
}

fn watermark_for_rows_data(
    rows: &[Map<String, Value>],
    strategy: &TableSyncStrategy,
) -> Option<String> {
    match strategy {
        TableSyncStrategy::Full => None,
        TableSyncStrategy::Watermark { column, data_type } => rows
            .iter()
            .filter_map(|row| row.get(column))
            .map(|value| value_to_watermark(value, data_type))
            .max_by(|left, right| compare_watermarks(left, right, data_type)),
    }
}

fn compare_watermarks(left: &str, right: &str, data_type: &str) -> std::cmp::Ordering {
    if is_integer_type(data_type) {
        left.parse::<i64>()
            .unwrap_or_default()
            .cmp(&right.parse::<i64>().unwrap_or_default())
    } else {
        left.cmp(right)
    }
}

fn value_to_watermark(value: &Value, data_type: &str) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => i64::from(*v).to_string(),
        Value::Number(v) if is_integer_type(data_type) => {
            v.as_i64().unwrap_or_default().to_string()
        }
        Value::Number(v) => v.to_string(),
        Value::String(v) => normalize_watermark(v.clone(), data_type),
        other => other.to_string(),
    }
}

fn is_integer_type(data_type: &str) -> bool {
    matches!(data_type, "smallint" | "integer" | "bigint")
}

fn pg_cast_suffix(data_type: &str) -> &'static str {
    match data_type {
        "timestamp with time zone" => "::timestamptz",
        "timestamp without time zone" => "::timestamp",
        "date" => "::date",
        "smallint" => "::smallint",
        "integer" => "::integer",
        "bigint" => "::bigint",
        _ => "",
    }
}

fn normalize_watermark(value: String, data_type: &str) -> String {
    if is_integer_type(data_type) {
        value
            .split('.')
            .next()
            .unwrap_or(value.as_str())
            .to_string()
    } else {
        value
    }
}

fn json_to_sql_value(value: Option<&Value>, data_type: &str) -> SqlValue {
    match value.unwrap_or(&Value::Null) {
        Value::Null => SqlValue::Null,
        Value::Bool(v) => SqlValue::Integer(i64::from(*v)),
        Value::Number(v) => match data_type {
            "smallint" | "integer" | "bigint" => v
                .as_i64()
                .map(SqlValue::Integer)
                .unwrap_or_else(|| SqlValue::Text(v.to_string())),
            "real" | "double precision" => v
                .as_f64()
                .map(SqlValue::Real)
                .unwrap_or_else(|| SqlValue::Text(v.to_string())),
            _ => SqlValue::Text(v.to_string()),
        },
        Value::String(v) => SqlValue::Text(v.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn should_run_startup_sync(sqlite_path: &Path) -> bool {
    if !sqlite_path.exists() {
        return true;
    }

    match crate::db::open_db(sqlite_path).and_then(|conn| mirror_last_sync_age_secs(&conn)) {
        Ok(Some(age_secs)) => age_secs >= STARTUP_SYNC_MIN_INTERVAL_SECS,
        Ok(None) => true,
        Err(err) => {
            eprintln!(
                "Unable to inspect mirror sync freshness; syncing anyway: {}",
                err
            );
            true
        }
    }
}

fn mirror_last_sync_age_secs(conn: &Connection) -> Result<Option<i64>> {
    let table_exists = conn
        .query_row(
            "SELECT 1
             FROM sqlite_master
             WHERE type = 'table'
               AND name = 'mirror_sync_state'
             LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if table_exists.is_none() {
        return Ok(None);
    }

    conn.query_row(
        "SELECT CAST(strftime('%s', 'now') AS INTEGER) - CAST(strftime('%s', MAX(last_synced_at)) AS INTEGER)
         FROM mirror_sync_state",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mirror_last_sync_age_is_none_without_state_table() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        assert_eq!(mirror_last_sync_age_secs(&conn).expect("query age"), None);
    }

    #[test]
    fn mirror_last_sync_age_reads_recent_timestamp() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        ensure_sync_state_table(&conn).expect("create state table");
        save_sync_state(&conn, "price_cache", "watermark", Some("123".to_string()))
            .expect("save sync state");

        let age = mirror_last_sync_age_secs(&conn)
            .expect("query age")
            .expect("age present");
        assert!((0..=5).contains(&age), "unexpected age: {age}");
    }

    #[test]
    fn startup_sync_is_skipped_when_recently_synced() {
        let path = unique_temp_sqlite_path();
        let conn = Connection::open(&path).expect("open sqlite");
        ensure_sync_state_table(&conn).expect("create state table");
        save_sync_state(&conn, "price_cache", "watermark", Some("123".to_string()))
            .expect("save sync state");
        drop(conn);

        assert!(!should_run_startup_sync(&path));

        let _ = fs::remove_file(path);
    }

    fn unique_temp_sqlite_path() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("pftui-mirror-test-{nanos}.db"))
    }
}
