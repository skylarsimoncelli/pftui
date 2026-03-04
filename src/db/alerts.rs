use anyhow::Result;
use rusqlite::{params, Connection};

use crate::alerts::{AlertKind, AlertDirection, AlertRule, AlertStatus};

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
