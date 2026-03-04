use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::Serialize;

use crate::alerts::engine::{check_alerts, AlertCheckResult};
use crate::alerts::rules::parse_rule;
use crate::alerts::AlertStatus;
use crate::db::alerts as alerts_db;

/// Run the alerts CLI subcommand.
pub fn run(conn: &Connection, action: &str, args: &AlertsArgs) -> Result<()> {
    match action {
        "add" => run_add(conn, args),
        "list" => run_list(conn, args),
        "remove" => run_remove(conn, args),
        "check" => run_check(conn, args),
        "ack" => run_ack(conn, args),
        "rearm" => run_rearm(conn, args),
        _ => bail!("Unknown alerts action: '{}'. Expected: add, list, remove, check, ack, rearm", action),
    }
}

/// Arguments for the alerts subcommand, parsed from CLI.
pub struct AlertsArgs {
    pub rule: Option<String>,
    pub id: Option<i64>,
    pub json: bool,
    pub status_filter: Option<String>,
}

fn run_add(conn: &Connection, args: &AlertsArgs) -> Result<()> {
    let rule_text = args.rule.as_deref().unwrap_or("");
    if rule_text.is_empty() {
        bail!("Usage: pftui alerts add \"<rule>\"\n\nExamples:\n  pftui alerts add \"GC=F above 5500\"\n  pftui alerts add \"BTC below 55000\"\n  pftui alerts add \"gold allocation above 30%\"\n  pftui alerts add \"GC=F RSI below 30\"");
    }

    let parsed = parse_rule(rule_text)?;
    let id = alerts_db::add_alert(
        conn,
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

fn run_list(conn: &Connection, args: &AlertsArgs) -> Result<()> {
    let alerts = if let Some(ref status_str) = args.status_filter {
        let status: AlertStatus = status_str.parse()?;
        alerts_db::list_alerts_by_status(conn, status)?
    } else {
        alerts_db::list_alerts(conn)?
    };

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
        println!("  {} [#{}] {} ({})", status_icon, alert.id, alert.rule_text, alert.kind);
        if let Some(ref triggered_at) = alert.triggered_at {
            println!("      Triggered: {}", triggered_at);
        }
    }
    Ok(())
}

fn run_remove(conn: &Connection, args: &AlertsArgs) -> Result<()> {
    let id = args.id.unwrap_or_else(|| {
        eprintln!("Usage: pftui alerts remove <id>");
        std::process::exit(1);
    });

    if let Some(alert) = alerts_db::get_alert(conn, id)? {
        alerts_db::remove_alert(conn, id)?;
        println!("Removed alert #{}: {}", id, alert.rule_text);
    } else {
        bail!("No alert found with id #{}", id);
    }
    Ok(())
}

fn run_check(conn: &Connection, args: &AlertsArgs) -> Result<()> {
    let results = check_alerts(conn)?;

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
    let newly_triggered: Vec<&AlertCheckResult> = results.iter().filter(|r| r.newly_triggered).collect();
    let armed: Vec<&AlertCheckResult> = results.iter().filter(|r| r.rule.status == AlertStatus::Armed && !r.newly_triggered).collect();
    let already_triggered: Vec<&AlertCheckResult> = results.iter().filter(|r| r.rule.status == AlertStatus::Triggered && !r.newly_triggered).collect();

    if !newly_triggered.is_empty() {
        println!("🔴 NEWLY TRIGGERED ({}):\n", newly_triggered.len());
        for r in &newly_triggered {
            let current = format_current_value(r);
            println!("  🔴 [#{}] {} — current: {}", r.rule.id, r.rule.rule_text, current);
        }
        println!();
    }

    if !already_triggered.is_empty() {
        println!("⚠️  Previously triggered ({}):\n", already_triggered.len());
        for r in &already_triggered {
            let current = format_current_value(r);
            let triggered_at = r.rule.triggered_at.as_deref().unwrap_or("unknown");
            println!("  ⚠️  [#{}] {} — current: {} (triggered: {})", r.rule.id, r.rule.rule_text, current, triggered_at);
        }
        println!();
    }

    if !armed.is_empty() {
        println!("🟢 Armed ({}):\n", armed.len());
        for r in &armed {
            let current = format_current_value(r);
            let distance = format_distance(r);
            println!("  🟢 [#{}] {} — current: {} {}", r.rule.id, r.rule.rule_text, current, distance);
        }
        println!();
    }

    let ack: Vec<&AlertCheckResult> = results.iter().filter(|r| r.rule.status == AlertStatus::Acknowledged).collect();
    if !ack.is_empty() {
        println!("✅ Acknowledged ({}):\n", ack.len());
        for r in &ack {
            println!("  ✅ [#{}] {}", r.rule.id, r.rule.rule_text);
        }
        println!();
    }

    Ok(())
}

fn run_ack(conn: &Connection, args: &AlertsArgs) -> Result<()> {
    let id = args.id.unwrap_or_else(|| {
        eprintln!("Usage: pftui alerts ack <id>");
        std::process::exit(1);
    });

    if let Some(alert) = alerts_db::get_alert(conn, id)? {
        if alert.status != AlertStatus::Triggered {
            bail!("Alert #{} is not triggered (status: {}). Only triggered alerts can be acknowledged.", id, alert.status);
        }
        alerts_db::acknowledge_alert(conn, id)?;
        println!("✅ Acknowledged alert #{}: {}", id, alert.rule_text);
    } else {
        bail!("No alert found with id #{}", id);
    }
    Ok(())
}

fn run_rearm(conn: &Connection, args: &AlertsArgs) -> Result<()> {
    let id = args.id.unwrap_or_else(|| {
        eprintln!("Usage: pftui alerts rearm <id>");
        std::process::exit(1);
    });

    if let Some(alert) = alerts_db::get_alert(conn, id)? {
        if alert.status == AlertStatus::Armed {
            bail!("Alert #{} is already armed.", id);
        }
        alerts_db::rearm_alert(conn, id)?;
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
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use crate::alerts::AlertRule;
    use crate::db::open_in_memory;
    use crate::models::price::PriceQuote;
    use crate::db::price_cache;

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
    fn test_add_alert_via_cli() {
        let conn = setup_db();
        let args = AlertsArgs {
            rule: Some("GC=F above 5500".to_string()),
            id: None,
            json: false,
            status_filter: None,
        };
        run_add(&conn, &args).unwrap();
        let alerts = alerts_db::list_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].symbol, "GC=F");
        assert_eq!(alerts[0].threshold, "5500");
    }

    #[test]
    fn test_add_allocation_alert() {
        let conn = setup_db();
        let args = AlertsArgs {
            rule: Some("gold allocation above 30%".to_string()),
            id: None,
            json: false,
            status_filter: None,
        };
        run_add(&conn, &args).unwrap();
        let alerts = alerts_db::list_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, crate::alerts::AlertKind::Allocation);
    }

    #[test]
    fn test_add_empty_rule_fails() {
        let conn = setup_db();
        let args = AlertsArgs {
            rule: Some(String::new()),
            id: None,
            json: false,
            status_filter: None,
        };
        assert!(run_add(&conn, &args).is_err());
    }

    #[test]
    fn test_remove_alert_via_cli() {
        let conn = setup_db();
        let id = alerts_db::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
        };
        run_remove(&conn, &args).unwrap();
        assert!(alerts_db::list_alerts(&conn).unwrap().is_empty());
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let conn = setup_db();
        let args = AlertsArgs {
            rule: None,
            id: Some(999),
            json: false,
            status_filter: None,
        };
        assert!(run_remove(&conn, &args).is_err());
    }

    #[test]
    fn test_check_triggers_armed_alert() {
        let conn = setup_db();
        // GC=F at 5600, alert for above 5500 → should trigger
        alerts_db::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();
        let results = check_alerts(&conn).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].newly_triggered);
    }

    #[test]
    fn test_check_json_output() {
        let conn = setup_db();
        alerts_db::add_alert(&conn, "price", "GC=F", "above", "6000", "GC=F above 6000").unwrap();
        let results = check_alerts(&conn).unwrap();
        let json_results: Vec<AlertCheckJson> = results.iter().map(AlertCheckJson::from).collect();
        let json = serde_json::to_string(&json_results).unwrap();
        assert!(json.contains("GC=F"));
        assert!(json.contains("\"newly_triggered\":false"));
    }

    #[test]
    fn test_ack_triggered_alert() {
        let conn = setup_db();
        let id = alerts_db::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();
        // Trigger it
        check_alerts(&conn).unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
        };
        run_ack(&conn, &args).unwrap();
        let alert = alerts_db::get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Acknowledged);
    }

    #[test]
    fn test_ack_armed_alert_fails() {
        let conn = setup_db();
        let id = alerts_db::add_alert(&conn, "price", "GC=F", "above", "6000", "GC=F above 6000").unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
        };
        assert!(run_ack(&conn, &args).is_err());
    }

    #[test]
    fn test_rearm_triggered_alert() {
        let conn = setup_db();
        let id = alerts_db::add_alert(&conn, "price", "GC=F", "above", "5500", "GC=F above 5500").unwrap();
        check_alerts(&conn).unwrap(); // triggers it
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
        };
        run_rearm(&conn, &args).unwrap();
        let alert = alerts_db::get_alert(&conn, id).unwrap().unwrap();
        assert_eq!(alert.status, AlertStatus::Armed);
    }

    #[test]
    fn test_rearm_already_armed_fails() {
        let conn = setup_db();
        let id = alerts_db::add_alert(&conn, "price", "GC=F", "above", "6000", "GC=F above 6000").unwrap();
        let args = AlertsArgs {
            rule: None,
            id: Some(id),
            json: false,
            status_filter: None,
        };
        assert!(run_rearm(&conn, &args).is_err());
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
}
