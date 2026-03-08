use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::alerts::{AlertKind, AlertDirection, AlertRule, AlertStatus};
use crate::db::backend::BackendConnection;
use crate::db::query;

/// Add a new alert rule. Returns the new row id.
pub fn add_alert(
    conn: &Connection,
    kind: &str,
    symbol: &str,
    direction: &str,
    threshold: &str,
    rule_text: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO alerts (kind, symbol, direction, threshold, status, rule_text)
         VALUES (?1, ?2, ?3, ?4, 'armed', ?5)",
        params![kind, symbol.to_uppercase(), direction, threshold, rule_text],
    )?;
    Ok(conn.last_insert_rowid())
}

/// List all alert rules, ordered by created_at descending.
pub fn list_alerts(conn: &Connection) -> Result<Vec<AlertRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, symbol, direction, threshold, status, rule_text, created_at, triggered_at
         FROM alerts ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(5)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            threshold: row.get(4)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(6)?,
            created_at: row.get(7)?,
            triggered_at: row.get(8)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// List alerts filtered by status.
pub fn list_alerts_by_status(conn: &Connection, status: AlertStatus) -> Result<Vec<AlertRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, symbol, direction, threshold, status, rule_text, created_at, triggered_at
         FROM alerts WHERE status = ?1 ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map(params![status.to_string()], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(5)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            threshold: row.get(4)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(6)?,
            created_at: row.get(7)?,
            triggered_at: row.get(8)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Update alert status. Optionally set triggered_at timestamp.
pub fn update_alert_status(
    conn: &Connection,
    id: i64,
    status: AlertStatus,
    triggered_at: Option<&str>,
) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE alerts SET status = ?1, triggered_at = COALESCE(?2, triggered_at) WHERE id = ?3",
        params![status.to_string(), triggered_at, id],
    )?;
    Ok(rows > 0)
}

/// Remove an alert by id. Returns true if a row was deleted.
pub fn remove_alert(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute("DELETE FROM alerts WHERE id = ?1", params![id])?;
    Ok(rows > 0)
}

/// Get a single alert by id.
pub fn get_alert(conn: &Connection, id: i64) -> Result<Option<AlertRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, symbol, direction, threshold, status, rule_text, created_at, triggered_at
         FROM alerts WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(5)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            threshold: row.get(4)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(6)?,
            created_at: row.get(7)?,
            triggered_at: row.get(8)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Count alerts by status.
pub fn count_by_status(conn: &Connection, status: AlertStatus) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM alerts WHERE status = ?1",
        params![status.to_string()],
        |r| r.get(0),
    )?;
    Ok(count)
}

/// Re-arm a triggered or acknowledged alert back to armed status.
pub fn rearm_alert(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE alerts SET status = 'armed', triggered_at = NULL WHERE id = ?1",
        params![id],
    )?;
    Ok(rows > 0)
}

/// Acknowledge a triggered alert.
pub fn acknowledge_alert(conn: &Connection, id: i64) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE alerts SET status = 'acknowledged' WHERE id = ?1 AND status = 'triggered'",
        params![id],
    )?;
    Ok(rows > 0)
}

pub fn add_alert_backend(
    backend: &BackendConnection,
    kind: &str,
    symbol: &str,
    direction: &str,
    threshold: &str,
    rule_text: &str,
) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_alert(conn, kind, symbol, direction, threshold, rule_text),
        |pool| add_alert_postgres(pool, kind, symbol, direction, threshold, rule_text),
    )
}

pub fn list_alerts_backend(backend: &BackendConnection) -> Result<Vec<AlertRule>> {
    query::dispatch(backend, list_alerts, list_alerts_postgres)
}

pub fn list_alerts_by_status_backend(
    backend: &BackendConnection,
    status: AlertStatus,
) -> Result<Vec<AlertRule>> {
    query::dispatch(
        backend,
        |conn| list_alerts_by_status(conn, status),
        |pool| list_alerts_by_status_postgres(pool, status),
    )
}

pub fn update_alert_status_backend(
    backend: &BackendConnection,
    id: i64,
    status: AlertStatus,
    triggered_at: Option<&str>,
) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| update_alert_status(conn, id, status, triggered_at),
        |pool| update_alert_status_postgres(pool, id, status, triggered_at),
    )
}

pub fn remove_alert_backend(backend: &BackendConnection, id: i64) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| remove_alert(conn, id),
        |pool| remove_alert_postgres(pool, id),
    )
}

pub fn get_alert_backend(backend: &BackendConnection, id: i64) -> Result<Option<AlertRule>> {
    query::dispatch(
        backend,
        |conn| get_alert(conn, id),
        |pool| get_alert_postgres(pool, id),
    )
}

#[allow(dead_code)]
pub fn count_by_status_backend(backend: &BackendConnection, status: AlertStatus) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| count_by_status(conn, status),
        |pool| count_by_status_postgres(pool, status),
    )
}

pub fn rearm_alert_backend(backend: &BackendConnection, id: i64) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| rearm_alert(conn, id),
        |pool| rearm_alert_postgres(pool, id),
    )
}

pub fn acknowledge_alert_backend(backend: &BackendConnection, id: i64) -> Result<bool> {
    query::dispatch(
        backend,
        |conn| acknowledge_alert(conn, id),
        |pool| acknowledge_alert_postgres(pool, id),
    )
}

fn ensure_tables_postgres(pool: &PgPool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alerts (
                id BIGSERIAL PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'price',
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                threshold TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'armed',
                rule_text TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                triggered_at TIMESTAMPTZ
            )",
        )
        .execute(pool)
        .await?;
        Ok::<(), sqlx::Error>(())
    })?;
    Ok(())
}

fn add_alert_postgres(
    pool: &PgPool,
    kind: &str,
    symbol: &str,
    direction: &str,
    threshold: &str,
    rule_text: &str,
) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let id: i64 = runtime.block_on(async {
        sqlx::query_scalar(
            "INSERT INTO alerts (kind, symbol, direction, threshold, status, rule_text)
             VALUES ($1, $2, $3, $4, 'armed', $5)
             RETURNING id",
        )
        .bind(kind)
        .bind(symbol.to_uppercase())
        .bind(direction)
        .bind(threshold)
        .bind(rule_text)
        .fetch_one(pool)
        .await
    })?;
    Ok(id)
}

type AlertRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

fn alert_from_row(row: AlertRow) -> AlertRule {
    AlertRule {
        id: row.0,
        kind: row.1.parse().unwrap_or(AlertKind::Price),
        symbol: row.2,
        direction: row.3.parse().unwrap_or(AlertDirection::Above),
        threshold: row.4,
        status: row.5.parse().unwrap_or(AlertStatus::Armed),
        rule_text: row.6,
        created_at: row.7,
        triggered_at: row.8,
    }
}

fn list_alerts_postgres(pool: &PgPool) -> Result<Vec<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<AlertRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT id, kind, symbol, direction, threshold, status, rule_text, created_at::text, triggered_at::text
             FROM alerts
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(alert_from_row).collect())
}

fn list_alerts_by_status_postgres(pool: &PgPool, status: AlertStatus) -> Result<Vec<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let status_str = status.to_string();
    let runtime = tokio::runtime::Runtime::new()?;
    let rows: Vec<AlertRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT id, kind, symbol, direction, threshold, status, rule_text, created_at::text, triggered_at::text
             FROM alerts
             WHERE status = $1
             ORDER BY created_at DESC",
        )
        .bind(status_str)
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(alert_from_row).collect())
}

fn update_alert_status_postgres(
    pool: &PgPool,
    id: i64,
    status: AlertStatus,
    triggered_at: Option<&str>,
) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let status_str = status.to_string();
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query(
            "UPDATE alerts
             SET status = $1, triggered_at = COALESCE($2::timestamptz, triggered_at)
             WHERE id = $3",
        )
        .bind(status_str)
        .bind(triggered_at)
        .bind(id)
        .execute(pool)
        .await
    })?;
    Ok(rows.rows_affected() > 0)
}

fn remove_alert_postgres(pool: &PgPool, id: i64) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query("DELETE FROM alerts WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
    })?;
    Ok(rows.rows_affected() > 0)
}

fn get_alert_postgres(pool: &PgPool, id: i64) -> Result<Option<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let row: Option<AlertRow> = runtime.block_on(async {
        sqlx::query_as(
            "SELECT id, kind, symbol, direction, threshold, status, rule_text, created_at::text, triggered_at::text
             FROM alerts
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    })?;
    Ok(row.map(alert_from_row))
}

fn count_by_status_postgres(pool: &PgPool, status: AlertStatus) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let status_str = status.to_string();
    let runtime = tokio::runtime::Runtime::new()?;
    let count: i64 = runtime.block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM alerts WHERE status = $1")
            .bind(status_str)
            .fetch_one(pool)
            .await
    })?;
    Ok(count)
}

fn rearm_alert_postgres(pool: &PgPool, id: i64) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query("UPDATE alerts SET status = 'armed', triggered_at = NULL WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
    })?;
    Ok(rows.rows_affected() > 0)
}

fn acknowledge_alert_postgres(pool: &PgPool, id: i64) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let rows = runtime.block_on(async {
        sqlx::query(
            "UPDATE alerts
             SET status = 'acknowledged'
             WHERE id = $1 AND status = 'triggered'",
        )
        .bind(id)
        .execute(pool)
        .await
    })?;
    Ok(rows.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn test_add_and_list() {
        let conn = open_in_memory();
        add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();
        add_alert(&conn, "price", "BTC", "below", "55000", "BTC below 55000").unwrap();

        let alerts = list_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 2);
        // Both inserted at same datetime — just verify both are present
        let symbols: Vec<&str> = alerts.iter().map(|a| a.symbol.as_str()).collect();
        assert!(symbols.contains(&"GC=F"));
        assert!(symbols.contains(&"BTC"));
    }

    #[test]
    fn test_add_stores_correct_fields() {
        let conn = open_in_memory();
        let id = add_alert(&conn, "price", "gc=f", "above", "5500", "GC=F above 5500").unwrap();

        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.kind, AlertKind::Price);
        assert_eq!(alert.symbol, "GC=F"); // uppercased
        assert_eq!(alert.direction, AlertDirection::Above);
        assert_eq!(alert.threshold, "5500");
        assert_eq!(alert.status, AlertStatus::Armed);
        assert_eq!(alert.rule_text, "GC=F above 5500");
        assert!(alert.triggered_at.is_none());
    }

    #[test]
    fn test_remove_alert() {
        let conn = open_in_memory();
        let id = add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();
        assert!(remove_alert(&conn, id).unwrap());
        assert!(list_alerts(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let conn = open_in_memory();
        assert!(!remove_alert(&conn, 999).unwrap());
    }

    #[test]
    fn test_update_status_to_triggered() {
        let conn = open_in_memory();
        let id = add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();

        update_alert_status(&conn, id, AlertStatus::Triggered, Some("2026-03-04T08:00:00Z"))
            .unwrap();

        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Triggered);
        assert_eq!(alert.triggered_at, Some("2026-03-04T08:00:00Z".to_string()));
    }

    #[test]
    fn test_list_by_status() {
        let conn = open_in_memory();
        let id1 = add_alert(&conn, "price", "GC=F", "above", "5500", "rule1").unwrap();
        add_alert(&conn, "price", "BTC", "below", "55000", "rule2").unwrap();

        update_alert_status(&conn, id1, AlertStatus::Triggered, Some("2026-03-04"))
            .unwrap();

        let armed = list_alerts_by_status(&conn, AlertStatus::Armed).unwrap();
        assert_eq!(armed.len(), 1);
        assert_eq!(armed[0].symbol, "BTC");

        let triggered = list_alerts_by_status(&conn, AlertStatus::Triggered).unwrap();
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].symbol, "GC=F");
    }

    #[test]
    fn test_count_by_status() {
        let conn = open_in_memory();
        add_alert(&conn, "price", "A", "above", "1", "r1").unwrap();
        add_alert(&conn, "price", "B", "above", "2", "r2").unwrap();

        assert_eq!(count_by_status(&conn, AlertStatus::Armed).unwrap(), 2);
        assert_eq!(count_by_status(&conn, AlertStatus::Triggered).unwrap(), 0);
    }

    #[test]
    fn test_rearm_alert() {
        let conn = open_in_memory();
        let id = add_alert(&conn, "price", "GC=F", "above", "5500", "rule").unwrap();
        update_alert_status(&conn, id, AlertStatus::Triggered, Some("2026-03-04")).unwrap();

        assert!(rearm_alert(&conn, id).unwrap());
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Armed);
        assert!(alert.triggered_at.is_none());
    }

    #[test]
    fn test_acknowledge_alert() {
        let conn = open_in_memory();
        let id = add_alert(&conn, "price", "GC=F", "above", "5500", "rule").unwrap();
        update_alert_status(&conn, id, AlertStatus::Triggered, Some("2026-03-04")).unwrap();

        assert!(acknowledge_alert(&conn, id).unwrap());
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Acknowledged);
    }

    #[test]
    fn test_acknowledge_only_triggered() {
        let conn = open_in_memory();
        let id = add_alert(&conn, "price", "GC=F", "above", "5500", "rule").unwrap();
        // Try to acknowledge an armed alert — should fail
        assert!(!acknowledge_alert(&conn, id).unwrap());
    }

    #[test]
    fn test_allocation_alert_kind() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            "allocation",
            "gold",
            "above",
            "30",
            "gold allocation above 30%",
        )
        .unwrap();
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.kind, AlertKind::Allocation);
        assert_eq!(alert.symbol, "GOLD"); // uppercased
    }

    #[test]
    fn test_indicator_alert_kind() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            "indicator",
            "GC=F RSI",
            "below",
            "30",
            "GC=F RSI below 30",
        )
        .unwrap();
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.kind, AlertKind::Indicator);
        assert_eq!(alert.symbol, "GC=F RSI");
    }
}
