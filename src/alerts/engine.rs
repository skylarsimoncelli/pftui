use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::Connection;
use std::collections::HashMap;
use std::str::FromStr;

use super::{AlertDirection, AlertKind, AlertRule, AlertStatus};
use crate::db::alerts;
use crate::db::backend::BackendConnection;
use crate::db::price_cache;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::scan_queries;

/// Result of checking a single alert rule against current data.
#[derive(Debug, Clone)]
pub struct AlertCheckResult {
    pub rule: AlertRule,
    pub current_value: Option<Decimal>,
    /// True if the alert just transitioned from Armed → Triggered.
    pub newly_triggered: bool,
    /// Distance to trigger as a percentage (positive = not yet triggered).
    /// For armed alerts: how far from threshold.
    /// None if current value is unavailable.
    pub distance_pct: Option<Decimal>,
}

/// Check all armed alerts against current cached prices.
///
/// Returns check results for all alerts (armed + already triggered).
/// Newly triggered alerts are updated to `Triggered` status in the DB.
pub fn check_alerts(conn: &Connection) -> Result<Vec<AlertCheckResult>> {
    ensure_review_date_alerts(conn)?;
    ensure_scan_query_change_alerts(conn)?;
    let all_alerts = alerts::list_alerts(conn)?;
    if all_alerts.is_empty() {
        return Ok(Vec::new());
    }

    let cached_prices = price_cache::get_all_cached_prices(conn)?;
    let price_map: HashMap<String, Decimal> = cached_prices
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    let mut results = Vec::new();
    for alert in all_alerts {
        let result = check_single_alert_sqlite(conn, &alert, &price_map)?;
        results.push(result);
    }
    Ok(results)
}

pub fn check_alerts_backend(backend: &BackendConnection, conn: &Connection) -> Result<Vec<AlertCheckResult>> {
    ensure_review_date_alerts(conn)?;
    ensure_scan_query_change_alerts(conn)?;
    let all_alerts = alerts::list_alerts_backend(backend)?;
    if all_alerts.is_empty() {
        return Ok(Vec::new());
    }

    // Build a price map from the cache for quick lookups
    let cached_prices = get_all_cached_prices_backend(backend)?;
    let price_map: HashMap<String, Decimal> = cached_prices
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    let mut results = Vec::new();

    for alert in all_alerts {
        let result = check_single_alert_backend(backend, &alert, &price_map)?;
        results.push(result);
    }

    Ok(results)
}

/// Check a single alert against the price map. Updates DB if newly triggered.
fn check_single_alert_sqlite(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertCheckResult> {
    let (current_value, is_triggered, distance_pct) = match alert.kind {
        AlertKind::Price => {
            let threshold = Decimal::from_str(&alert.threshold).unwrap_or(Decimal::ZERO);
            let current = price_map.get(&alert.symbol).copied();
            let triggered = if let Some(current) = current {
                match alert.direction {
                    AlertDirection::Above => current >= threshold,
                    AlertDirection::Below => current <= threshold,
                }
            } else {
                false
            };
            let distance = current.and_then(|current| {
                if threshold.is_zero() {
                    return None;
                }
                let pct = match alert.direction {
                    AlertDirection::Above => {
                        (threshold - current) / threshold * Decimal::from(100)
                    }
                    AlertDirection::Below => {
                        (current - threshold) / threshold * Decimal::from(100)
                    }
                };
                Some(pct)
            });
            (current, triggered, distance)
        }
        AlertKind::Allocation => (None, false, None),
        AlertKind::Indicator => {
            if alert.symbol.starts_with("REVIEW:") {
                let review_date = chrono::NaiveDate::parse_from_str(&alert.threshold, "%Y-%m-%d").ok();
                let today = chrono::Utc::now().date_naive();
                let triggered = review_date.map(|d| today >= d).unwrap_or(false);
                (None, triggered, None)
            } else {
                (None, false, None)
            }
        }
    };

    let newly_triggered = is_triggered && alert.status == AlertStatus::Armed;
    if newly_triggered {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        alerts::update_alert_status(conn, alert.id, AlertStatus::Triggered, Some(&now))?;
    }

    Ok(AlertCheckResult {
        rule: alert.clone(),
        current_value,
        newly_triggered,
        distance_pct,
    })
}

fn check_single_alert_backend(
    backend: &BackendConnection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertCheckResult> {
    let (current_value, is_triggered, distance_pct) = match alert.kind {
        AlertKind::Price => {
            let threshold = Decimal::from_str(&alert.threshold).unwrap_or(Decimal::ZERO);
            let current = price_map.get(&alert.symbol).copied();
            let triggered = if let Some(current) = current {
                match alert.direction {
                    AlertDirection::Above => current >= threshold,
                    AlertDirection::Below => current <= threshold,
                }
            } else {
                false
            };
            let distance = current.and_then(|current| {
                if threshold.is_zero() {
                    return None;
                }
                let pct = match alert.direction {
                    AlertDirection::Above => {
                        (threshold - current) / threshold * Decimal::from(100)
                    }
                    AlertDirection::Below => {
                        (current - threshold) / threshold * Decimal::from(100)
                    }
                };
                Some(pct)
            });
            (current, triggered, distance)
        }
        AlertKind::Allocation => (None, false, None),
        AlertKind::Indicator => {
            // Special indicator alert for annotation review dates.
            // Symbol format: REVIEW:<SYMBOL>, threshold: YYYY-MM-DD.
            if alert.symbol.starts_with("REVIEW:") {
                let review_date = chrono::NaiveDate::parse_from_str(&alert.threshold, "%Y-%m-%d").ok();
                let today = chrono::Utc::now().date_naive();
                let triggered = review_date.map(|d| today >= d).unwrap_or(false);
                (None, triggered, None)
            } else {
                (None, false, None)
            }
        }
    };

    let newly_triggered = is_triggered && alert.status == AlertStatus::Armed;

    // Update DB status if newly triggered
    if newly_triggered {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        alerts::update_alert_status_backend(
            backend,
            alert.id,
            AlertStatus::Triggered,
            Some(&now),
        )?;
    }

    Ok(AlertCheckResult {
        rule: alert.clone(),
        current_value,
        newly_triggered,
        distance_pct,
    })
}

fn ensure_review_date_alerts(conn: &Connection) -> Result<()> {
    let annotations = crate::db::annotations::list_annotations(conn)?;
    if annotations.is_empty() {
        return Ok(());
    }

    let existing = alerts::list_alerts(conn)?;
    for ann in annotations {
        let Some(review_date) = ann.review_date.clone() else {
            continue;
        };
        if chrono::NaiveDate::parse_from_str(&review_date, "%Y-%m-%d").is_err() {
            continue;
        }
        let symbol_key = format!("REVIEW:{}", ann.symbol.to_uppercase());
        let rule_text = format!("Review {} thesis by {}", ann.symbol.to_uppercase(), review_date);

        if let Some(existing_alert) = existing.iter().find(|a| {
            a.kind == AlertKind::Indicator && a.symbol == symbol_key
        }) {
            // Keep alert synced when review date changes.
            if existing_alert.threshold != review_date || existing_alert.rule_text != rule_text {
                conn.execute(
                    "UPDATE alerts
                     SET threshold = ?1, rule_text = ?2, status = 'armed', triggered_at = NULL
                     WHERE id = ?3",
                    rusqlite::params![review_date, rule_text, existing_alert.id],
                )?;
            }
        } else {
            let _ = alerts::add_alert(
                conn,
                "indicator",
                &symbol_key,
                "below",
                &review_date,
                &rule_text,
            )?;
        }
    }
    Ok(())
}

fn ensure_scan_query_change_alerts(conn: &Connection) -> Result<()> {
    let queries = scan_queries::list_scan_queries(conn)?;
    if queries.is_empty() {
        return Ok(());
    }

    for q in queries {
        let current = crate::commands::scan::count_matches(conn, &q.filter_expr)? as i64;
        let previous: Option<i64> = conn
            .query_row(
                "SELECT last_count FROM scan_alert_state WHERE name = ?1",
                rusqlite::params![q.name],
                |r| r.get(0),
            )
            .ok();

        if let Some(prev) = previous {
            if prev != current {
                let rule_text = format!(
                    "Scan '{}' result count changed: {} -> {}",
                    q.name, prev, current
                );
                let symbol = format!("SCAN:{}", q.name.to_uppercase());
                let id = alerts::add_alert(
                    conn,
                    "indicator",
                    &symbol,
                    "above",
                    &current.to_string(),
                    &rule_text,
                )?;
                let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                alerts::update_alert_status(conn, id, AlertStatus::Triggered, Some(&now))?;
            }
        }

        conn.execute(
            "INSERT INTO scan_alert_state (name, last_count, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(name) DO UPDATE
             SET last_count = excluded.last_count,
                 updated_at = datetime('now')",
            rusqlite::params![q.name, current],
        )?;
    }

    Ok(())
}

/// Get only newly triggered alerts (convenience for refresh output).
#[allow(dead_code)] // Used by F6.5+ (TUI badge, refresh integration)
pub fn get_newly_triggered(results: &[AlertCheckResult]) -> Vec<&AlertCheckResult> {
    results.iter().filter(|r| r.newly_triggered).collect()
}

/// Get armed alerts with their distance to trigger (convenience for status display).
#[allow(dead_code)] // Used by F6.5+ (TUI status bar)
pub fn get_armed_with_distance(results: &[AlertCheckResult]) -> Vec<&AlertCheckResult> {
    results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Armed && !r.newly_triggered)
        .collect()
}

/// Count of currently triggered (not yet acknowledged) alerts.
#[allow(dead_code)] // Used by F6.5+ (TUI alert badge)
pub fn triggered_count(conn: &Connection) -> Result<i64> {
    alerts::count_by_status(conn, AlertStatus::Triggered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::price_cache;
    use crate::models::price::PriceQuote;

    fn setup_test_db() -> Connection {
        let conn = open_in_memory();
        // Insert some test prices
        price_cache::upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".to_string(),
                price: Decimal::from(5600),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),
            
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
        },
        )
        .unwrap();
        price_cache::upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: Decimal::from(68000),
                currency: "USD".to_string(),
                fetched_at: "2026-03-04T00:00:00Z".to_string(),
                source: "test".to_string(),
            
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
        },
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_check_empty_alerts() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_check_price_alert_triggered() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        // GC=F is at 5600, alert for above 5500 should trigger
        alerts::add_alert(conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
        assert_eq!(results[0].current_value, Some(Decimal::from(5600)));
    }

    #[test]
    fn test_check_price_alert_not_triggered() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        // GC=F is at 5600, alert for above 6000 should NOT trigger
        alerts::add_alert(conn, "price", "GC=F", "above", "6000", "GC=F above 6000")
            .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_check_below_alert_triggered() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        // BTC is at 68000, alert for below 70000 should trigger
        alerts::add_alert(
            conn,
            "price",
            "BTC",
            "below",
            "70000",
            "BTC below 70000",
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
    }

    #[test]
    fn test_already_triggered_not_newly() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        let id = alerts::add_alert(conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        // Manually mark as triggered
        alerts::update_alert_status(conn, id, AlertStatus::Triggered, Some("2026-03-04"))
            .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_distance_calculation() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        // GC=F at 5600, threshold 6000 above → distance = (6000-5600)/6000 * 100 ≈ 6.67%
        alerts::add_alert(conn, "price", "GC=F", "above", "6000", "GC=F above 6000")
            .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        let dist = results[0].distance_pct.unwrap();
        // (6000 - 5600) / 6000 * 100 = 6.666...
        assert!(dist > Decimal::from(6) && dist < Decimal::from(7));
    }

    #[test]
    fn test_unknown_symbol_no_crash() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        alerts::add_alert(
            conn,
            "price",
            "UNKNOWN",
            "above",
            "100",
            "UNKNOWN above 100",
        )
        .unwrap();
        let results = check_alerts_backend(&backend, conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
        assert!(results[0].current_value.is_none());
    }

    #[test]
    fn test_get_newly_triggered_filter() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        // One that will trigger, one that won't
        alerts::add_alert(conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        alerts::add_alert(conn, "price", "GC=F", "above", "6000", "GC=F above 6000")
            .unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        let newly = get_newly_triggered(&results);
        assert_eq!(newly.len(), 1);
        assert_eq!(newly[0].rule.threshold, "5500");
    }

    #[test]
    fn test_triggered_count() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        assert_eq!(triggered_count(conn).unwrap(), 0);

        alerts::add_alert(conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        check_alerts_backend(&backend, conn).unwrap(); // triggers it

        assert_eq!(triggered_count(conn).unwrap(), 1);
    }

    #[test]
    fn test_review_date_alert_auto_created_and_triggered() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        let ann = crate::db::annotations::Annotation {
            symbol: "GC=F".to_string(),
            thesis: "Test".to_string(),
            invalidation: None,
            review_date: Some("2000-01-01".to_string()),
            target_price: None,
            updated_at: String::new(),
        };
        crate::db::annotations::upsert_annotation(conn, &ann).unwrap();

        let results = check_alerts_backend(&backend, conn).unwrap();
        assert!(results.iter().any(|r| r.rule.symbol == "REVIEW:GC=F"));
        assert!(results
            .iter()
            .any(|r| r.rule.symbol == "REVIEW:GC=F" && (r.newly_triggered || r.rule.status == AlertStatus::Triggered)));
    }

    #[test]
    fn test_scan_query_change_creates_triggered_alert() {
        let backend = BackendConnection::Sqlite { conn: setup_test_db() };
        let conn = backend.sqlite();
        crate::db::scan_queries::upsert_scan_query(conn, "equities", "category == equity")
            .unwrap();

        // First check seeds baseline state and should not emit change alert.
        let _ = check_alerts_backend(&backend, conn).unwrap();
        assert!(alerts::list_alerts(conn)
            .unwrap()
            .iter()
            .all(|a| !a.symbol.starts_with("SCAN:")));

        // Add one matching position and check again -> should emit triggered scan alert.
        crate::db::transactions::insert_transaction(
            conn,
            &crate::models::transaction::NewTransaction {
                symbol: "AAPL".to_string(),
                category: crate::models::asset::AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: Decimal::from(2),
                price_per: Decimal::from(180),
                currency: "USD".to_string(),
                date: "2026-03-09".to_string(),
                notes: None,
            },
        )
        .unwrap();
        let _ = check_alerts_backend(&backend, conn).unwrap();
        let all = alerts::list_alerts(conn).unwrap();
        let scan_alerts: Vec<_> = all
            .iter()
            .filter(|a| a.symbol.starts_with("SCAN:"))
            .collect();
        assert_eq!(scan_alerts.len(), 1);
        assert_eq!(scan_alerts[0].status, AlertStatus::Triggered);
    }
}
