use anyhow::Result;
use rusqlite::{params, Connection};
use sqlx::PgPool;

use crate::alerts::{AlertDirection, AlertKind, AlertRule, AlertStatus};
use crate::db::backend::BackendConnection;
use crate::db::query;

#[derive(Clone, Copy)]
pub struct NewAlert<'a> {
    pub kind: &'a str,
    pub symbol: &'a str,
    pub direction: &'a str,
    pub condition: Option<&'a str>,
    pub threshold: &'a str,
    pub rule_text: &'a str,
    pub recurring: bool,
    pub cooldown_minutes: i64,
}

/// Add a new alert rule. Returns the new row id.
pub fn add_alert(conn: &Connection, new_alert: NewAlert<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO alerts (kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes)
         VALUES (?1, ?2, ?3, ?4, ?5, 'armed', ?6, ?7, ?8)",
        params![
            new_alert.kind,
            new_alert.symbol.to_uppercase(),
            new_alert.direction,
            new_alert.condition,
            new_alert.threshold,
            new_alert.rule_text,
            if new_alert.recurring { 1 } else { 0 },
            new_alert.cooldown_minutes
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// List all alert rules, ordered by created_at descending.
pub fn list_alerts(conn: &Connection) -> Result<Vec<AlertRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at, triggered_at
         FROM alerts ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(6)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            condition: row.get(4)?,
            threshold: row.get(5)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(7)?,
            recurring: row.get::<_, i64>(8)? != 0,
            cooldown_minutes: row.get(9)?,
            created_at: row.get(10)?,
            triggered_at: row.get(11)?,
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
        "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at, triggered_at
         FROM alerts WHERE status = ?1 ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map(params![status.to_string()], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(6)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            condition: row.get(4)?,
            threshold: row.get(5)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(7)?,
            recurring: row.get::<_, i64>(8)? != 0,
            cooldown_minutes: row.get(9)?,
            created_at: row.get(10)?,
            triggered_at: row.get(11)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// List alerts that were triggered/acknowledged within the last N hours.
/// Optionally filter by status.
pub fn list_alerts_recent(
    conn: &Connection,
    hours: i64,
    status_filter: Option<AlertStatus>,
) -> Result<Vec<AlertRule>> {
    let mut query = String::from(
        "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at, triggered_at
         FROM alerts
         WHERE triggered_at IS NOT NULL
           AND triggered_at >= datetime('now', ?1)",
    );
    if let Some(ref status) = status_filter {
        query.push_str(&format!(" AND status = '{}'", status));
    }
    query.push_str(" ORDER BY triggered_at DESC");

    let interval = format!("-{} hours", hours.max(0));
    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params![interval], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(6)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            condition: row.get(4)?,
            threshold: row.get(5)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(7)?,
            recurring: row.get::<_, i64>(8)? != 0,
            cooldown_minutes: row.get(9)?,
            created_at: row.get(10)?,
            triggered_at: row.get(11)?,
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
        "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at, triggered_at
         FROM alerts WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        let kind_str: String = row.get(1)?;
        let dir_str: String = row.get(3)?;
        let status_str: String = row.get(6)?;
        Ok(AlertRule {
            id: row.get(0)?,
            kind: kind_str.parse().unwrap_or(AlertKind::Price),
            symbol: row.get(2)?,
            direction: dir_str.parse().unwrap_or(AlertDirection::Above),
            condition: row.get(4)?,
            threshold: row.get(5)?,
            status: status_str.parse().unwrap_or(AlertStatus::Armed),
            rule_text: row.get(7)?,
            recurring: row.get::<_, i64>(8)? != 0,
            cooldown_minutes: row.get(9)?,
            created_at: row.get(10)?,
            triggered_at: row.get(11)?,
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

pub fn add_alert_backend(backend: &BackendConnection, new_alert: NewAlert<'_>) -> Result<i64> {
    query::dispatch(
        backend,
        |conn| add_alert(conn, new_alert),
        |pool| add_alert_postgres(pool, new_alert),
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

pub fn list_alerts_recent_backend(
    backend: &BackendConnection,
    hours: i64,
    status_filter: Option<AlertStatus>,
) -> Result<Vec<AlertRule>> {
    query::dispatch(
        backend,
        |conn| list_alerts_recent(conn, hours, status_filter),
        |pool| list_alerts_recent_postgres(pool, hours, status_filter),
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
    crate::db::pg_runtime::block_on(async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alerts (
                id BIGSERIAL PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'price',
                symbol TEXT NOT NULL,
                direction TEXT NOT NULL,
                condition TEXT,
                threshold TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'armed',
                rule_text TEXT NOT NULL,
                recurring BOOLEAN NOT NULL DEFAULT FALSE,
                cooldown_minutes BIGINT NOT NULL DEFAULT 0,
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

fn add_alert_postgres(pool: &PgPool, new_alert: NewAlert<'_>) -> Result<i64> {
    ensure_tables_postgres(pool)?;
    let id: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar(
            "INSERT INTO alerts (kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes)
             VALUES ($1, $2, $3, $4, $5, 'armed', $6, $7, $8)
             RETURNING id",
        )
        .bind(new_alert.kind)
        .bind(new_alert.symbol.to_uppercase())
        .bind(new_alert.direction)
        .bind(new_alert.condition)
        .bind(new_alert.threshold)
        .bind(new_alert.rule_text)
        .bind(new_alert.recurring)
        .bind(new_alert.cooldown_minutes)
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
    Option<String>,
    String,
    String,
    String,
    bool,
    i64,
    String,
    Option<String>,
);

fn alert_from_row(row: AlertRow) -> AlertRule {
    AlertRule {
        id: row.0,
        kind: row.1.parse().unwrap_or(AlertKind::Price),
        symbol: row.2,
        direction: row.3.parse().unwrap_or(AlertDirection::Above),
        condition: row.4,
        threshold: row.5,
        status: row.6.parse().unwrap_or(AlertStatus::Armed),
        rule_text: row.7,
        recurring: row.8,
        cooldown_minutes: row.9,
        created_at: row.10,
        triggered_at: row.11,
    }
}

fn list_alerts_postgres(pool: &PgPool) -> Result<Vec<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<AlertRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at::text, triggered_at::text
             FROM alerts
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    })?;
    Ok(rows.into_iter().map(alert_from_row).collect())
}

fn list_alerts_recent_postgres(
    pool: &PgPool,
    hours: i64,
    status_filter: Option<AlertStatus>,
) -> Result<Vec<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let rows: Vec<AlertRow> = crate::db::pg_runtime::block_on(async {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at::text, triggered_at::text
             FROM alerts
             WHERE triggered_at IS NOT NULL
               AND triggered_at >= NOW() - (",
        );
        qb.push_bind(hours.max(0))
            .push(" * INTERVAL '1 hour')");
        if let Some(status) = status_filter {
            qb.push(" AND status = ").push_bind(status.to_string());
        }
        qb.push(" ORDER BY triggered_at DESC");
        qb.build_query_as().fetch_all(pool).await
    })?;
    Ok(rows.into_iter().map(alert_from_row).collect())
}

fn list_alerts_by_status_postgres(pool: &PgPool, status: AlertStatus) -> Result<Vec<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let status_str = status.to_string();
    let rows: Vec<AlertRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at::text, triggered_at::text
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
    let rows = crate::db::pg_runtime::block_on(async {
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
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query("DELETE FROM alerts WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
    })?;
    Ok(rows.rows_affected() > 0)
}

fn get_alert_postgres(pool: &PgPool, id: i64) -> Result<Option<AlertRule>> {
    ensure_tables_postgres(pool)?;
    let row: Option<AlertRow> = crate::db::pg_runtime::block_on(async {
        sqlx::query_as(
            "SELECT id, kind, symbol, direction, condition, threshold, status, rule_text, recurring, cooldown_minutes, created_at::text, triggered_at::text
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
    let count: i64 = crate::db::pg_runtime::block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM alerts WHERE status = $1")
            .bind(status_str)
            .fetch_one(pool)
            .await
    })?;
    Ok(count)
}

fn rearm_alert_postgres(pool: &PgPool, id: i64) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
        sqlx::query("UPDATE alerts SET status = 'armed', triggered_at = NULL WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
    })?;
    Ok(rows.rows_affected() > 0)
}

fn acknowledge_alert_postgres(pool: &PgPool, id: i64) -> Result<bool> {
    ensure_tables_postgres(pool)?;
    let rows = crate::db::pg_runtime::block_on(async {
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

    fn new_alert<'a>(
        kind: &'a str,
        symbol: &'a str,
        direction: &'a str,
        condition: Option<&'a str>,
        threshold: &'a str,
        rule_text: &'a str,
    ) -> NewAlert<'a> {
        NewAlert {
            kind,
            symbol,
            direction,
            condition,
            threshold,
            rule_text,
            recurring: false,
            cooldown_minutes: 0,
        }
    }

    #[test]
    fn test_add_and_list() {
        let conn = open_in_memory();
        add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        add_alert(
            &conn,
            new_alert("price", "BTC", "below", None, "55000", "BTC below 55000"),
        )
        .unwrap();

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
        let id = add_alert(
            &conn,
            new_alert("price", "gc=f", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();

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
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
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
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();

        update_alert_status(
            &conn,
            id,
            AlertStatus::Triggered,
            Some("2026-03-04T08:00:00Z"),
        )
        .unwrap();

        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Triggered);
        assert_eq!(alert.triggered_at, Some("2026-03-04T08:00:00Z".to_string()));
    }

    #[test]
    fn test_list_by_status() {
        let conn = open_in_memory();
        let id1 = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "rule1"),
        )
        .unwrap();
        add_alert(
            &conn,
            new_alert("price", "BTC", "below", None, "55000", "rule2"),
        )
        .unwrap();

        update_alert_status(&conn, id1, AlertStatus::Triggered, Some("2026-03-04")).unwrap();

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
        add_alert(&conn, new_alert("price", "A", "above", None, "1", "r1")).unwrap();
        add_alert(&conn, new_alert("price", "B", "above", None, "2", "r2")).unwrap();

        assert_eq!(count_by_status(&conn, AlertStatus::Armed).unwrap(), 2);
        assert_eq!(count_by_status(&conn, AlertStatus::Triggered).unwrap(), 0);
    }

    #[test]
    fn test_rearm_alert() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "rule"),
        )
        .unwrap();
        update_alert_status(&conn, id, AlertStatus::Triggered, Some("2026-03-04")).unwrap();

        assert!(rearm_alert(&conn, id).unwrap());
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Armed);
        assert!(alert.triggered_at.is_none());
    }

    #[test]
    fn test_acknowledge_alert() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "rule"),
        )
        .unwrap();
        update_alert_status(&conn, id, AlertStatus::Triggered, Some("2026-03-04")).unwrap();

        assert!(acknowledge_alert(&conn, id).unwrap());
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Acknowledged);
    }

    #[test]
    fn test_acknowledge_only_triggered() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "rule"),
        )
        .unwrap();
        // Try to acknowledge an armed alert — should fail
        assert!(!acknowledge_alert(&conn, id).unwrap());
    }

    #[test]
    fn test_allocation_alert_kind() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            new_alert(
                "allocation",
                "gold",
                "above",
                None,
                "30",
                "gold allocation above 30%",
            ),
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
            new_alert(
                "indicator",
                "GC=F RSI",
                "below",
                None,
                "30",
                "GC=F RSI below 30",
            ),
        )
        .unwrap();
        let alert = get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.kind, AlertKind::Indicator);
        assert_eq!(alert.symbol, "GC=F RSI");
    }

    #[test]
    fn test_list_alerts_recent_returns_recently_triggered() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        // Trigger the alert with a recent timestamp
        update_alert_status(
            &conn,
            id,
            AlertStatus::Triggered,
            Some(&chrono::Utc::now().to_rfc3339()),
        )
        .unwrap();

        let recent = list_alerts_recent(&conn, 24, None).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, id);
    }

    #[test]
    fn test_list_alerts_recent_excludes_old() {
        let conn = open_in_memory();
        let id = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        // Trigger with an old timestamp (48 hours ago)
        let old_time = (chrono::Utc::now() - chrono::Duration::hours(48)).to_rfc3339();
        update_alert_status(&conn, id, AlertStatus::Triggered, Some(&old_time)).unwrap();

        let recent = list_alerts_recent(&conn, 24, None).unwrap();
        assert!(recent.is_empty());
    }

    #[test]
    fn test_list_alerts_recent_filters_by_status() {
        let conn = open_in_memory();
        let id1 = add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "5500", "GC=F above 5500"),
        )
        .unwrap();
        let id2 = add_alert(
            &conn,
            new_alert("price", "BTC", "below", None, "90000", "BTC below 90000"),
        )
        .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        update_alert_status(&conn, id1, AlertStatus::Triggered, Some(&now)).unwrap();
        update_alert_status(&conn, id2, AlertStatus::Triggered, Some(&now)).unwrap();
        // Acknowledge only the first one
        acknowledge_alert(&conn, id1).unwrap();

        // All recent
        let all = list_alerts_recent(&conn, 24, None).unwrap();
        assert_eq!(all.len(), 2);

        // Only acknowledged
        let acked = list_alerts_recent(&conn, 24, Some(AlertStatus::Acknowledged)).unwrap();
        assert_eq!(acked.len(), 1);
        assert_eq!(acked[0].id, id1);

        // Only triggered (not yet acknowledged)
        let triggered = list_alerts_recent(&conn, 24, Some(AlertStatus::Triggered)).unwrap();
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id, id2);
    }

    #[test]
    fn test_list_alerts_recent_empty_when_none_triggered() {
        let conn = open_in_memory();
        // Add an armed alert with no triggered_at
        add_alert(
            &conn,
            new_alert("price", "GC=F", "above", None, "6000", "GC=F above 6000"),
        )
        .unwrap();

        let recent = list_alerts_recent(&conn, 24, None).unwrap();
        assert!(recent.is_empty());
    }
}
