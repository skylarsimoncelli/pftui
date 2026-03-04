use anyhow::Result;
use rust_decimal::Decimal;
use rusqlite::Connection;
use std::collections::HashMap;
use std::str::FromStr;

use super::{AlertDirection, AlertKind, AlertRule, AlertStatus};
use crate::db::alerts;
use crate::db::price_cache;

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
    let all_alerts = alerts::list_alerts(conn)?;
    if all_alerts.is_empty() {
        return Ok(Vec::new());
    }

    // Build a price map from the cache for quick lookups
    let cached_prices = price_cache::get_all_cached_prices(conn)?;
    let price_map: HashMap<String, Decimal> = cached_prices
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    let mut results = Vec::new();

    for alert in all_alerts {
        let result = check_single_alert(conn, &alert, &price_map)?;
        results.push(result);
    }

    Ok(results)
}

/// Check a single alert against the price map. Updates DB if newly triggered.
fn check_single_alert(
    conn: &Connection,
    alert: &AlertRule,
    price_map: &HashMap<String, Decimal>,
) -> Result<AlertCheckResult> {
    let threshold = Decimal::from_str(&alert.threshold).unwrap_or(Decimal::ZERO);

    let current_value = match alert.kind {
        AlertKind::Price => {
            // Direct symbol lookup in price cache
            price_map.get(&alert.symbol).copied()
        }
        AlertKind::Allocation => {
            // Allocation alerts need portfolio context — skip in this basic engine.
            // Will be wired up when positions are available in the check context.
            None
        }
        AlertKind::Indicator => {
            // Indicator alerts (e.g. "GC=F RSI") need computed indicators.
            // The symbol field stores "SYMBOL INDICATOR", e.g. "GC=F RSI".
            // Will be wired up when indicator computation is available in check context.
            None
        }
    };

    let is_triggered = if let Some(current) = current_value {
        match alert.direction {
            AlertDirection::Above => current >= threshold,
            AlertDirection::Below => current <= threshold,
        }
    } else {
        false
    };

    let newly_triggered = is_triggered && alert.status == AlertStatus::Armed;

    // Update DB status if newly triggered
    if newly_triggered {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        alerts::update_alert_status(conn, alert.id, AlertStatus::Triggered, Some(&now))?;
    }

    let distance_pct = current_value.and_then(|current| {
        if threshold.is_zero() {
            return None;
        }
        let distance = match alert.direction {
            AlertDirection::Above => {
                // Positive = below threshold (not triggered), negative = above (triggered)
                (threshold - current) / threshold * Decimal::from(100)
            }
            AlertDirection::Below => {
                // Positive = above threshold (not triggered), negative = below (triggered)
                (current - threshold) / threshold * Decimal::from(100)
            }
        };
        Some(distance)
    });

    Ok(AlertCheckResult {
        rule: alert.clone(),
        current_value,
        newly_triggered,
        distance_pct,
    })
}

/// Get only newly triggered alerts (convenience for refresh output).
pub fn get_newly_triggered(results: &[AlertCheckResult]) -> Vec<&AlertCheckResult> {
    results.iter().filter(|r| r.newly_triggered).collect()
}

/// Get armed alerts with their distance to trigger (convenience for status display).
pub fn get_armed_with_distance(results: &[AlertCheckResult]) -> Vec<&AlertCheckResult> {
    results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Armed && !r.newly_triggered)
        .collect()
}

/// Count of currently triggered (not yet acknowledged) alerts.
pub fn triggered_count(conn: &Connection) -> Result<i64> {
    alerts::count_by_status(conn, AlertStatus::Triggered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
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
            },
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_check_empty_alerts() {
        let conn = setup_test_db();
        let results = check_alerts(&conn).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_check_price_alert_triggered() {
        let conn = setup_test_db();
        // GC=F is at 5600, alert for above 5500 should trigger
        alerts::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        let results = check_alerts(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
        assert_eq!(results[0].current_value, Some(Decimal::from(5600)));
    }

    #[test]
    fn test_check_price_alert_not_triggered() {
        let conn = setup_test_db();
        // GC=F is at 5600, alert for above 6000 should NOT trigger
        alerts::add_alert(&conn, "price", "GC=F", "above", "6000", "GC=F above 6000")
            .unwrap();
        let results = check_alerts(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_check_below_alert_triggered() {
        let conn = setup_test_db();
        // BTC is at 68000, alert for below 70000 should trigger
        alerts::add_alert(
            &conn,
            "price",
            "BTC",
            "below",
            "70000",
            "BTC below 70000",
        )
        .unwrap();
        let results = check_alerts(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
    }

    #[test]
    fn test_already_triggered_not_newly() {
        let conn = setup_test_db();
        let id = alerts::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        // Manually mark as triggered
        alerts::update_alert_status(&conn, id, AlertStatus::Triggered, Some("2026-03-04"))
            .unwrap();

        let results = check_alerts(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
    }

    #[test]
    fn test_distance_calculation() {
        let conn = setup_test_db();
        // GC=F at 5600, threshold 6000 above → distance = (6000-5600)/6000 * 100 ≈ 6.67%
        alerts::add_alert(&conn, "price", "GC=F", "above", "6000", "GC=F above 6000")
            .unwrap();
        let results = check_alerts(&conn).unwrap();
        let dist = results[0].distance_pct.unwrap();
        // (6000 - 5600) / 6000 * 100 = 6.666...
        assert!(dist > Decimal::from(6) && dist < Decimal::from(7));
    }

    #[test]
    fn test_unknown_symbol_no_crash() {
        let conn = setup_test_db();
        alerts::add_alert(
            &conn,
            "price",
            "UNKNOWN",
            "above",
            "100",
            "UNKNOWN above 100",
        )
        .unwrap();
        let results = check_alerts(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].newly_triggered);
        assert!(results[0].current_value.is_none());
    }

    #[test]
    fn test_get_newly_triggered_filter() {
        let conn = setup_test_db();
        // One that will trigger, one that won't
        alerts::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        alerts::add_alert(&conn, "price", "GC=F", "above", "6000", "GC=F above 6000")
            .unwrap();

        let results = check_alerts(&conn).unwrap();
        let newly = get_newly_triggered(&results);
        assert_eq!(newly.len(), 1);
        assert_eq!(newly[0].rule.threshold, "5500");
    }

    #[test]
    fn test_triggered_count() {
        let conn = setup_test_db();
        assert_eq!(triggered_count(&conn).unwrap(), 0);

        alerts::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500")
            .unwrap();
        check_alerts(&conn).unwrap(); // triggers it

        assert_eq!(triggered_count(&conn).unwrap(), 1);
    }
}
