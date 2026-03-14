use anyhow::{bail, Result};
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use serde::Serialize;

use crate::alerts::engine::{check_alerts_backend_only, AlertCheckResult};
use crate::alerts::rules::parse_rule;
use crate::alerts::{AlertRule, AlertStatus};
use crate::db::alerts as alerts_db;
use crate::db::backend::BackendConnection;

/// Run the alerts CLI subcommand.
pub fn run(backend: &BackendConnection, action: &str, args: &AlertsArgs) -> Result<()> {
    match action {
        "add" => run_add(backend, args),
        "list" => run_list(backend, args),
        "remove" => run_remove(backend, args),
        "check" => run_check(backend, args),
        "ack" => run_ack(backend, args),
        "rearm" => run_rearm(backend, args),
        _ => bail!(
            "Unknown alerts action: '{}'. Expected: add, list, remove, check, ack, rearm",
            action
        ),
    }
}

/// Arguments for the alerts subcommand, parsed from CLI.
pub struct AlertsArgs {
    pub rule: Option<String>,
    pub id: Option<i64>,
    pub json: bool,
    pub status_filter: Option<String>,
    pub today: bool,
}

fn run_add(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    let rule_text = args.rule.as_deref().unwrap_or("");
    if rule_text.is_empty() {
        bail!("Usage: pftui alerts add \"<rule>\"\n\nExamples:\n  pftui alerts add \"GC=F above 5500\"\n  pftui alerts add \"BTC below 55000\"\n  pftui alerts add \"gold allocation above 30%\"\n  pftui alerts add \"GC=F RSI below 30\"");
    }

    let parsed = parse_rule(rule_text)?;
    let id = alerts_db::add_alert_backend(
        backend,
        &parsed.kind.to_string(),
        &parsed.symbol,
        &parsed.direction.to_string(),
        &parsed.threshold.to_string(),
        &parsed.rule_text,
    )?;

    println!("🟢 Alert #{} created: {}", id, parsed.rule_text);
    println!("   Type: {} | Status: armed", parsed.kind);
    Ok(())
}

fn run_list(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    let mut alerts = if let Some(ref status_str) = args.status_filter {
        let status: AlertStatus = status_str.parse()?;
        alerts_db::list_alerts_by_status_backend(backend, status)?
    } else {
        alerts_db::list_alerts_backend(backend)?
    };
    if args.today {
        alerts.retain(alert_matches_today_filter);
    }

    if alerts.is_empty() {
        if args.json {
            println!("[]");
        } else {
            println!("No alerts configured. Use: pftui alerts add \"GC=F above 5500\"");
        }
        return Ok(());
    }

    if args.json {
        let json = serde_json::to_string_pretty(&alerts)?;
        println!("{}", json);
        return Ok(());
    }

    println!("Alerts ({}):\n", alerts.len());
    for alert in &alerts {
        let status_icon = match alert.status {
            AlertStatus::Armed => "🟢",
            AlertStatus::Triggered => "🔴",
            AlertStatus::Acknowledged => "✅",
        };
        println!(
            "  {} [#{}] {} ({})",
            status_icon, alert.id, alert.rule_text, alert.kind
        );
        if let Some(ref triggered_at) = alert.triggered_at {
            println!("      Triggered: {}", triggered_at);
        }
    }
    Ok(())
}

fn run_remove(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    let id = args.id.unwrap_or_else(|| {
        eprintln!("Usage: pftui alerts remove <id>");
        std::process::exit(1);
    });

    if let Some(alert) = alerts_db::get_alert_backend(backend, id)? {
        alerts_db::remove_alert_backend(backend, id)?;
        println!("Removed alert #{}: {}", id, alert.rule_text);
    } else {
        bail!("No alert found with id #{}", id);
    }
    Ok(())
}

fn run_check(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    let mut results = check_alerts_backend_only(backend)?;
    if args.today {
        results.retain(|r| alert_matches_today_filter(&r.rule));
    }

    if results.is_empty() {
        if args.json {
            println!("[]");
        } else {
            println!("No alerts to check.");
        }
        return Ok(());
    }

    if args.json {
        let json_results: Vec<AlertCheckJson> = results.iter().map(AlertCheckJson::from).collect();
        let json = serde_json::to_string_pretty(&json_results)?;
        println!("{}", json);
        return Ok(());
    }

    // Separate by status for display
    let newly_triggered: Vec<&AlertCheckResult> =
        results.iter().filter(|r| r.newly_triggered).collect();
    let armed: Vec<&AlertCheckResult> = results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Armed && !r.newly_triggered)
        .collect();
    let already_triggered: Vec<&AlertCheckResult> = results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Triggered && !r.newly_triggered)
        .collect();

    if !newly_triggered.is_empty() {
        println!("🔴 NEWLY TRIGGERED ({}):\n", newly_triggered.len());
        for r in &newly_triggered {
            let current = format_current_value(r);
            println!(
                "  🔴 [#{}] {} — current: {}",
                r.rule.id, r.rule.rule_text, current
            );
        }
        println!();
    }

    if !already_triggered.is_empty() {
        println!("⚠️  Previously triggered ({}):\n", already_triggered.len());
        for r in &already_triggered {
            let current = format_current_value(r);
            let triggered_at = r.rule.triggered_at.as_deref().unwrap_or("unknown");
            println!(
                "  ⚠️  [#{}] {} — current: {} (triggered: {})",
                r.rule.id, r.rule.rule_text, current, triggered_at
            );
        }
        println!();
    }

    if !armed.is_empty() {
        println!("🟢 Armed ({}):\n", armed.len());
        for r in &armed {
            let current = format_current_value(r);
            let distance = format_distance(r);
            println!(
                "  🟢 [#{}] {} — current: {} {}",
                r.rule.id, r.rule.rule_text, current, distance
            );
        }
        println!();
    }

    let ack: Vec<&AlertCheckResult> = results
        .iter()
        .filter(|r| r.rule.status == AlertStatus::Acknowledged)
        .collect();
    if !ack.is_empty() {
        println!("✅ Acknowledged ({}):\n", ack.len());
        for r in &ack {
            println!("  ✅ [#{}] {}", r.rule.id, r.rule.rule_text);
        }
        println!();
    }

    Ok(())
}

fn alert_matches_today_filter(alert: &AlertRule) -> bool {
    let Some(triggered_at) = alert.triggered_at.as_deref() else {
        return false;
    };
    let Some(dt_utc) = parse_timestamp_utc(triggered_at) else {
        return false;
    };
    let local_dt = dt_utc.with_timezone(&Local);
    local_dt.date_naive() == Local::now().date_naive()
}

fn parse_timestamp_utc(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(ts) = raw.parse::<i64>() {
        if let Some(dt) = DateTime::from_timestamp(ts, 0) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
}

fn run_ack(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    let id = args.id.unwrap_or_else(|| {
        eprintln!("Usage: pftui alerts ack <id>");
        std::process::exit(1);
    });

    if let Some(alert) = alerts_db::get_alert_backend(backend, id)? {
        if alert.status != AlertStatus::Triggered {
            bail!("Alert #{} is not triggered (status: {}). Only triggered alerts can be acknowledged.", id, alert.status);
        }
        alerts_db::acknowledge_alert_backend(backend, id)?;
        println!("✅ Acknowledged alert #{}: {}", id, alert.rule_text);
    } else {
        bail!("No alert found with id #{}", id);
    }
    Ok(())
}

fn run_rearm(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    let id = args.id.unwrap_or_else(|| {
        eprintln!("Usage: pftui alerts rearm <id>");
        std::process::exit(1);
    });

    if let Some(alert) = alerts_db::get_alert_backend(backend, id)? {
        if alert.status == AlertStatus::Armed {
            bail!("Alert #{} is already armed.", id);
        }
        alerts_db::rearm_alert_backend(backend, id)?;
        println!("🟢 Re-armed alert #{}: {}", id, alert.rule_text);
    } else {
        bail!("No alert found with id #{}", id);
    }
    Ok(())
}

fn format_current_value(r: &AlertCheckResult) -> String {
    match r.current_value {
        Some(v) => format!("{}", v),
        None => "N/A".to_string(),
    }
}

fn format_distance(r: &AlertCheckResult) -> String {
    match r.distance_pct {
        Some(d) => {
            let sign = if d.is_sign_positive() { "+" } else { "" };
            let rounded = d.round_dp(1);
            format!("({}{}% to target)", sign, rounded)
        }
        None => String::new(),
    }
}

/// JSON-serializable check result for `--json` output.
#[derive(Serialize)]
struct AlertCheckJson {
    id: i64,
    kind: String,
    symbol: String,
    rule_text: String,
    status: String,
    threshold: String,
    direction: String,
    current_value: Option<String>,
    newly_triggered: bool,
    distance_pct: Option<String>,
    triggered_at: Option<String>,
}

impl From<&AlertCheckResult> for AlertCheckJson {
    fn from(r: &AlertCheckResult) -> Self {
        AlertCheckJson {
            id: r.rule.id,
            kind: r.rule.kind.to_string(),
            symbol: r.rule.symbol.clone(),
            rule_text: r.rule.rule_text.clone(),
            status: if r.newly_triggered {
                "triggered".to_string()
            } else {
                r.rule.status.to_string()
            },
            threshold: r.rule.threshold.clone(),
            direction: r.rule.direction.to_string(),
            current_value: r.current_value.map(|v| v.to_string()),
            newly_triggered: r.newly_triggered,
            distance_pct: r.distance_pct.map(|d| d.round_dp(2).to_string()),
            triggered_at: r.rule.triggered_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::AlertRule;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::price_cache;
    use crate::models::price::PriceQuote;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn setup_db() -> rusqlite::Connection {
        let conn = open_in_memory();
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

    fn setup_backend() -> BackendConnection {
        BackendConnection::Sqlite { conn: setup_db() }
    }

    #[test]
    fn test_add_alert_via_cli() {
        let backend = setup_backend();
        let args = AlertsArgs {
            rule: Some("GC=F above 5500".to_string()),
            id: None,
            json: false,
            status_filter: None,
            today: false,
        };
        run_add(&backend, &args).unwrap();
        let alerts = alerts_db::list_alerts_backend(&backend).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].symbol, "GC=F");
        assert_eq!(alerts[0].threshold, "5500");
    }

    #[test]
    fn test_add_allocation_alert() {
        let backend = setup_backend();
        let args = AlertsArgs {
            rule: Some("gold allocation above 30%".to_string()),
            id: None,
            json: false,
            status_filter: None,
            today: false,
        };
        run_add(&backend, &args).unwrap();
        let alerts = alerts_db::list_alerts_backend(&backend).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, crate::alerts::AlertKind::Allocation);
    }

    #[test]
    fn test_add_empty_rule_fails() {
        let backend = setup_backend();
        let args = AlertsArgs {
            rule: Some(String::new()),
            id: None,
            json: false,
            status_filter: None,
            today: false,
        };
        assert!(run_add(&backend, &args).is_err());
    }

    #[test]
    fn test_remove_alert_via_cli() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "5500",
            "GC=F above 5500",
        )
        .unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
            today: false,
        };
        run_remove(&backend, &args).unwrap();
        assert!(alerts_db::list_alerts_backend(&backend).unwrap().is_empty());
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let backend = setup_backend();
        let args = AlertsArgs {
            rule: None,
            id: Some(999),
            json: false,
            status_filter: None,
            today: false,
        };
        assert!(run_remove(&backend, &args).is_err());
    }

    #[test]
    fn test_check_triggers_armed_alert() {
        let backend = setup_backend();
        // GC=F at 5600, alert for above 5500 → should trigger
        alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "5500",
            "GC=F above 5500",
        )
        .unwrap();
        let results = check_alerts_backend_only(&backend).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
    }

    #[test]
    fn test_check_json_output() {
        let backend = setup_backend();
        alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "6000",
            "GC=F above 6000",
        )
        .unwrap();
        let results = check_alerts_backend_only(&backend).unwrap();
        let json_results: Vec<AlertCheckJson> = results.iter().map(AlertCheckJson::from).collect();
        let json = serde_json::to_string(&json_results).unwrap();
        assert!(json.contains("GC=F"));
        assert!(json.contains("\"newly_triggered\":false"));
    }

    #[test]
    fn test_ack_triggered_alert() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "5500",
            "GC=F above 5500",
        )
        .unwrap();
        // Trigger it
        check_alerts_backend_only(&backend).unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
            today: false,
        };
        run_ack(&backend, &args).unwrap();
        let alert = alerts_db::get_alert_backend(&backend, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Acknowledged);
    }

    #[test]
    fn test_ack_armed_alert_fails() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "6000",
            "GC=F above 6000",
        )
        .unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
            today: false,
        };
        assert!(run_ack(&backend, &args).is_err());
    }

    #[test]
    fn test_rearm_triggered_alert() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "5500",
            "GC=F above 5500",
        )
        .unwrap();
        check_alerts_backend_only(&backend).unwrap(); // triggers it
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
            today: false,
        };
        run_rearm(&backend, &args).unwrap();
        let alert = alerts_db::get_alert_backend(&backend, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Armed);
    }

    #[test]
    fn test_rearm_already_armed_fails() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            "price",
            "GC=F",
            "above",
            "6000",
            "GC=F above 6000",
        )
        .unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
            today: false,
        };
        assert!(run_rearm(&backend, &args).is_err());
    }

    #[test]
    fn test_format_distance_positive() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 1,
                kind: crate::alerts::AlertKind::Price,
                symbol: "GC=F".to_string(),
                direction: crate::alerts::AlertDirection::Above,
                threshold: "6000".to_string(),
                status: AlertStatus::Armed,
                rule_text: "GC=F above 6000".to_string(),
                created_at: "2026-03-04".to_string(),
                triggered_at: None,
            },
            current_value: Some(Decimal::from(5600)),
            newly_triggered: false,
            distance_pct: Some(Decimal::from_str("6.67").unwrap()),
        };
        let dist = format_distance(&r);
        assert!(dist.contains("6.7"));
        assert!(dist.contains("+"));
    }

    #[test]
    fn test_today_filter_accepts_triggered_today() {
        let today = Local::now().format("%Y-%m-%dT08:30:00Z").to_string();
        let alert = AlertRule {
            id: 9,
            kind: crate::alerts::AlertKind::Price,
            symbol: "GC=F".to_string(),
            direction: crate::alerts::AlertDirection::Above,
            threshold: "6000".to_string(),
            status: AlertStatus::Triggered,
            rule_text: "GC=F above 6000".to_string(),
            created_at: "2026-03-04".to_string(),
            triggered_at: Some(today),
        };
        assert!(alert_matches_today_filter(&alert));
    }

    #[test]
    fn test_today_filter_rejects_old_triggered_alert() {
        let old = (Local::now() - chrono::Duration::days(2))
            .format("%Y-%m-%dT08:30:00Z")
            .to_string();
        let alert = AlertRule {
            id: 10,
            kind: crate::alerts::AlertKind::Price,
            symbol: "BTC".to_string(),
            direction: crate::alerts::AlertDirection::Below,
            threshold: "90000".to_string(),
            status: AlertStatus::Triggered,
            rule_text: "BTC below 90000".to_string(),
            created_at: "2026-03-04".to_string(),
            triggered_at: Some(old),
        };
        assert!(!alert_matches_today_filter(&alert));
    }
}
