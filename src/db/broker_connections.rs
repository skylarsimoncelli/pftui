use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Debug, Clone, Serialize)]
pub struct BrokerConnection {
    pub id: i64,
    pub broker_name: String,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub last_sync_at: Option<String>,
    pub sync_status: String,
    pub sync_error: Option<String>,
    pub created_at: String,
}

pub fn upsert_broker_connection(
    conn: &Connection,
    broker_name: &str,
    account_id: Option<&str>,
    label: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO broker_connections (broker_name, account_id, label)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(broker_name) DO UPDATE SET
             account_id = COALESCE(?2, broker_connections.account_id),
             label = COALESCE(?3, broker_connections.label)",
        params![broker_name, account_id, label],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM broker_connections WHERE broker_name = ?1",
        params![broker_name],
        |r| r.get(0),
    )?;
    Ok(id)
}

pub fn get_broker_connection(conn: &Connection, broker_name: &str) -> Result<Option<BrokerConnection>> {
    let mut stmt = conn.prepare(
        "SELECT id, broker_name, account_id, label, last_sync_at, sync_status, sync_error, created_at
         FROM broker_connections WHERE broker_name = ?1",
    )?;
    let mut rows = stmt.query_map(params![broker_name], map_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_broker_connections(conn: &Connection) -> Result<Vec<BrokerConnection>> {
    let mut stmt = conn.prepare(
        "SELECT id, broker_name, account_id, label, last_sync_at, sync_status, sync_error, created_at
         FROM broker_connections ORDER BY broker_name",
    )?;
    let rows = stmt.query_map([], map_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn delete_broker_connection(conn: &Connection, broker_name: &str) -> Result<bool> {
    let affected = conn.execute(
        "DELETE FROM broker_connections WHERE broker_name = ?1",
        params![broker_name],
    )?;
    Ok(affected > 0)
}

pub fn update_sync_status(
    conn: &Connection,
    broker_name: &str,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE broker_connections
         SET sync_status = ?1, sync_error = ?2, last_sync_at = datetime('now')
         WHERE broker_name = ?3",
        params![status, error, broker_name],
    )?;
    Ok(())
}

pub fn delete_broker_transactions(conn: &Connection, broker_tag: &str) -> Result<usize> {
    let pattern = format!("{}%", broker_tag);
    let affected = conn.execute(
        "DELETE FROM transactions WHERE notes LIKE ?1",
        params![pattern],
    )?;
    Ok(affected)
}

// Backend-dispatched wrappers

pub fn upsert_broker_connection_backend(
    backend: &BackendConnection,
    broker_name: &str,
    account_id: Option<&str>,
    label: Option<&str>,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| upsert_broker_connection(conn, broker_name, account_id, label),
        |_pool| anyhow::bail!("Broker connections not yet supported on Postgres"),
    )
}

pub fn get_broker_connection_backend(
    backend: &BackendConnection,
    broker_name: &str,
) -> Result<Option<BrokerConnection>> {
    query::dispatch(
        backend,
        |conn| get_broker_connection(conn, broker_name),
        |_pool| anyhow::bail!("Broker connections not yet supported on Postgres"),
    )
}

pub fn list_broker_connections_backend(
    backend: &BackendConnection,
) -> Result<Vec<BrokerConnection>> {
    query::dispatch(
        backend,
        list_broker_connections,
        |_pool| anyhow::bail!("Broker connections not yet supported on Postgres"),
    )
}

pub fn delete_broker_connection_backend(
    backend: &BackendConnection,
    broker_name: &str,
) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| delete_broker_connection(conn, broker_name),
        |_pool| anyhow::bail!("Broker connections not yet supported on Postgres"),
    )
}

pub fn update_sync_status_backend(
    backend: &BackendConnection,
    broker_name: &str,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    query::dispatch(
        backend,
        |conn| update_sync_status(conn, broker_name, status, error),
        |_pool| anyhow::bail!("Broker connections not yet supported on Postgres"),
    )
}

pub fn delete_broker_transactions_backend(
    backend: &BackendConnection,
    broker_tag: &str,
) -> Result<usize> {
    query::dispatch(
        backend,
        |conn| delete_broker_transactions(conn, broker_tag),
        |_pool| anyhow::bail!("Broker connections not yet supported on Postgres"),
    )
}

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<BrokerConnection> {
    Ok(BrokerConnection {
        id: row.get(0)?,
        broker_name: row.get(1)?,
        account_id: row.get(2)?,
        label: row.get(3)?,
        last_sync_at: row.get(4)?,
        sync_status: row.get(5)?,
        sync_error: row.get(6)?,
        created_at: row.get(7)?,
    })
}
