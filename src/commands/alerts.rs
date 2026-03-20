use anyhow::{bail, Result};
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use serde::Serialize;
use serde_json::json;

use crate::alerts::engine::{check_alerts_backend_only, AlertCheckResult};
use crate::alerts::rules::parse_rule;
use crate::alerts::{AlertDirection, AlertKind, AlertRule, AlertStatus};
use crate::analytics::levels::select_actionable_level;
use crate::db::alerts::{self as alerts_db, NewAlert};
use crate::db::backend::BackendConnection;
use crate::db::price_cache;
use crate::db::technical_levels;
use crate::db::triggered_alerts as triggered_alerts_db;
use crate::models::asset::AssetCategory;
use crate::models::transaction::TxType;

/// Run the alerts CLI subcommand.
pub fn run(backend: &BackendConnection, action: &str, args: &AlertsArgs) -> Result<()> {
    match action {
        "add" => run_add(backend, args),
        "list" => run_list(backend, args),
        "remove" => run_remove(backend, args),
        "check" => run_check(backend, args),
        "ack" => run_ack(backend, args),
        "rearm" => run_rearm(backend, args),
        "seed-defaults" => run_seed_defaults(backend),
        _ => bail!(
            "Unknown alerts action: '{}'. Expected: add, list, remove, check, ack, rearm, seed-defaults",
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
    pub kind: Option<String>,
    pub symbol: Option<String>,
    pub from_level: Option<String>,
    pub condition: Option<String>,
    pub label: Option<String>,
    pub triggered: bool,
    pub since_hours: Option<i64>,
    pub recurring: bool,
    pub cooldown_minutes: i64,
}

fn run_add(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    if let Some(selector) = args.from_level.as_deref() {
        return run_add_from_stored_level(backend, selector, args);
    }

    if args.kind.is_none() && args.condition.is_none() {
        let rule_text = args.rule.as_deref().unwrap_or("");
        if rule_text.is_empty() {
            bail!("Usage: pftui analytics alerts add \"<rule>\" OR pftui analytics alerts add --kind technical --symbol BTC --condition price_below_sma200 --label \"BTC lost 200-day SMA\"");
        }

        let parsed = parse_rule(rule_text)?;
        let kind = parsed.kind.to_string();
        let direction = parsed.direction.to_string();
        let threshold = parsed.threshold.to_string();
        let id = alerts_db::add_alert_backend(
            backend,
            NewAlert {
                kind: &kind,
                symbol: &parsed.symbol,
                direction: &direction,
                condition: None,
                threshold: &threshold,
                rule_text: &parsed.rule_text,
                recurring: args.recurring,
                cooldown_minutes: args.cooldown_minutes,
            },
        )?;

        println!("🟢 Alert #{} created: {}", id, parsed.rule_text);
        println!("   Type: {} | Status: armed", parsed.kind);
        return Ok(());
    }

    let kind: AlertKind = args
        .kind
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--kind is required for structured alerts"))?
        .parse()?;
    let symbol = args.symbol.clone().unwrap_or_default().to_uppercase();
    let condition = args
        .condition
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--condition is required for structured alerts"))?;
    let label = args
        .label
        .clone()
        .unwrap_or_else(|| default_label(&kind, &symbol, &condition));
    let direction = inferred_direction(&condition);
    let threshold = inferred_threshold(&condition);

    let kind_text = kind.to_string();
    let direction_text = direction.to_string();
    let id = alerts_db::add_alert_backend(
        backend,
        NewAlert {
            kind: &kind_text,
            symbol: &symbol,
            direction: &direction_text,
            condition: Some(&condition),
            threshold: &threshold,
            rule_text: &label,
            recurring: args.recurring,
            cooldown_minutes: args.cooldown_minutes,
        },
    )?;

    println!("🟢 Alert #{} created: {}", id, label);
    println!(
        "   Type: {} | Condition: {} | Recurring: {}",
        kind, condition, args.recurring
    );
    Ok(())
}

fn run_add_from_stored_level(
    backend: &BackendConnection,
    selector: &str,
    args: &AlertsArgs,
) -> Result<()> {
    let symbol = args
        .symbol
        .clone()
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    if symbol.is_empty() {
        bail!("--symbol is required when using --from-level");
    }
    if let Some(kind) = args.kind.as_deref() {
        if !kind.eq_ignore_ascii_case("price") {
            bail!("--from-level only supports price alerts");
        }
    }

    let prices = price_cache::get_all_cached_prices_backend(backend)?;
    let current_price = prices
        .into_iter()
        .find(|quote| quote.symbol.eq_ignore_ascii_case(&symbol))
        .map(|quote| quote.price)
        .ok_or_else(|| anyhow::anyhow!("No cached price available for {}", symbol))?;
    let current_f = current_price
        .to_string()
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("Failed to parse cached price for {}", symbol))?;

    let levels = technical_levels::get_levels_for_symbol_backend(backend, &symbol)?;
    if levels.is_empty() {
        bail!("No stored levels available for {}", symbol);
    }

    let selected = select_actionable_level(&levels, current_f, selector).ok_or_else(|| {
        anyhow::anyhow!("No stored '{}' level available for {}", selector, symbol)
    })?;
    let direction = if selected.price >= current_f {
        AlertDirection::Above
    } else {
        AlertDirection::Below
    };
    let threshold = format_level_threshold(selected.price);
    let label = args
        .label
        .clone()
        .unwrap_or_else(|| format!("{} {} stored {} {}", symbol, direction, selector, threshold));
    let direction_text = direction.to_string();
    let id = alerts_db::add_alert_backend(
        backend,
        NewAlert {
            kind: "price",
            symbol: &symbol,
            direction: &direction_text,
            condition: None,
            threshold: &threshold,
            rule_text: &label,
            recurring: args.recurring,
            cooldown_minutes: args.cooldown_minutes,
        },
    )?;

    println!("🟢 Alert #{} created: {}", id, label);
    println!(
        "   Type: price | Source: stored {} | Threshold: {}",
        selector, threshold
    );
    Ok(())
}

fn run_list(backend: &BackendConnection, args: &AlertsArgs) -> Result<()> {
    if args.triggered {
        let since_hours = if args.today {
            Some(hours_since_local_midnight())
        } else {
            args.since_hours
        };
        let rows = triggered_alerts_db::list_triggered_alerts_backend(backend, since_hours, false)?;
        if args.json {
            let payload: Vec<_> = rows
                .into_iter()
                .map(|row| {
                    let alert = alerts_db::get_alert_backend(backend, row.alert_id).ok().flatten();
                    json!({
                        "id": row.id,
                        "alert_id": row.alert_id,
                        "triggered_at": row.triggered_at,
                        "trigger_data": serde_json::from_str::<serde_json::Value>(&row.trigger_data).unwrap_or_else(|_| json!({ "raw": row.trigger_data })),
                        "acknowledged": row.acknowledged,
                        "alert": alert
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else if rows.is_empty() {
            println!("No triggered alerts logged.");
        } else {
            println!("Triggered alerts ({}):\n", rows.len());
            for row in rows {
                println!(
                    "  [#{}] alert #{} at {}",
                    row.id, row.alert_id, row.triggered_at
                );
            }
        }
        return Ok(());
    }

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
        if let Some(ref condition) = alert.condition {
            println!("      Condition: {}", condition);
        }
        if alert.recurring {
            println!("      Recurring cooldown: {}m", alert.cooldown_minutes);
        }
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

fn hours_since_local_midnight() -> i64 {
    let now = Local::now();
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap_or_else(|| now.naive_local());
    (now.naive_local() - midnight).num_hours().max(0)
}

fn default_label(kind: &AlertKind, symbol: &str, condition: &str) -> String {
    match kind {
        AlertKind::Technical => {
            if symbol.is_empty() {
                condition.to_string()
            } else {
                format!("{} {}", symbol, condition.replace('_', " "))
            }
        }
        AlertKind::Macro => condition.replace('_', " "),
        _ => condition.to_string(),
    }
}

fn inferred_direction(condition: &str) -> AlertDirection {
    if condition.contains("below") || condition.contains("bearish") || condition.contains("lower") {
        AlertDirection::Below
    } else {
        AlertDirection::Above
    }
}

fn inferred_threshold(condition: &str) -> String {
    if let Some(value) = condition.rsplit('_').next() {
        if value.chars().all(|c| c.is_ascii_digit()) {
            return value.to_string();
        }
    }
    match condition {
        "price_above_sma50" | "price_below_sma50" => "50".to_string(),
        "price_above_sma200" | "price_below_sma200" => "200".to_string(),
        "rsi_above_70" => "70".to_string(),
        "rsi_below_30" => "30".to_string(),
        "vix_regime_shift" => "20/25/30/35".to_string(),
        "fear_greed_extreme" => "15/85".to_string(),
        "dxy_century_cross" => "100".to_string(),
        _ => "0".to_string(),
    }
}

fn format_level_threshold(price: f64) -> String {
    if price >= 10000.0 {
        format!("{:.0}", price)
    } else if price >= 1.0 {
        format!("{:.2}", price)
    } else {
        format!("{:.4}", price)
    }
}

fn portfolio_symbols(backend: &BackendConnection) -> Result<Vec<String>> {
    let mut symbols = std::collections::BTreeSet::new();

    for tx in crate::db::transactions::list_transactions_backend(backend).unwrap_or_default() {
        if tx.tx_type == TxType::Buy && tx.category != AssetCategory::Cash {
            symbols.insert(tx.symbol.to_uppercase());
        }
    }

    for alloc in crate::db::allocations::list_allocations_backend(backend).unwrap_or_default() {
        if alloc.category != AssetCategory::Cash {
            symbols.insert(alloc.symbol.to_uppercase());
        }
    }

    Ok(symbols.into_iter().collect())
}

fn seed_alert(
    backend: &BackendConnection,
    kind: AlertKind,
    symbol: &str,
    condition: &str,
    label: &str,
    recurring: bool,
    cooldown_minutes: i64,
) -> Result<bool> {
    let existing = alerts_db::list_alerts_backend(backend)?;
    let symbol = symbol.to_uppercase();
    if existing.iter().any(|alert| {
        alert.kind == kind
            && alert.symbol == symbol
            && alert.condition.as_deref() == Some(condition)
    }) {
        return Ok(false);
    }

    let kind_text = kind.to_string();
    let direction = inferred_direction(condition).to_string();
    let threshold = inferred_threshold(condition);
    alerts_db::add_alert_backend(
        backend,
        NewAlert {
            kind: &kind_text,
            symbol: &symbol,
            direction: &direction,
            condition: Some(condition),
            threshold: &threshold,
            rule_text: label,
            recurring,
            cooldown_minutes,
        },
    )?;
    Ok(true)
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
        let _ = triggered_alerts_db::acknowledge_for_alert_backend(backend, id)?;
        println!("✅ Acknowledged alert #{}: {}", id, alert.rule_text);
    } else {
        bail!("No alert found with id #{}", id);
    }
    Ok(())
}

fn run_seed_defaults(backend: &BackendConnection) -> Result<()> {
    let mut seeded = 0usize;
    for symbol in portfolio_symbols(backend)? {
        let labels = [
            (
                "price_above_sma200",
                format!("{} reclaimed 200-day SMA", symbol),
                true,
                60,
            ),
            (
                "price_below_sma200",
                format!("{} lost 200-day SMA", symbol),
                true,
                60,
            ),
            (
                "price_change_pct_above_5",
                format!("{} daily move above 5%", symbol),
                true,
                60,
            ),
            (
                "price_change_pct_below_5",
                format!("{} daily move below -5%", symbol),
                true,
                60,
            ),
            (
                "rsi_above_70",
                format!("{} RSI overbought", symbol),
                true,
                240,
            ),
            (
                "rsi_below_30",
                format!("{} RSI oversold", symbol),
                true,
                240,
            ),
        ];
        for (condition, label, recurring, cooldown) in labels {
            seeded += seed_alert(
                backend,
                AlertKind::Technical,
                &symbol,
                condition,
                &label,
                recurring,
                cooldown,
            )? as usize;
        }
    }

    for (condition, label, cooldown) in [
        ("vix_regime_shift", "VIX crossed regime threshold", 60),
        ("dxy_century_cross", "DXY crossed 100", 60),
        ("regime_change", "Market regime shifted", 0),
        ("fear_greed_extreme", "Fear & Greed hit extreme", 240),
        (
            "yield_curve_inversion_change",
            "Yield curve inversion changed",
            0,
        ),
        (
            "correlation_regime_break",
            "Major correlation regime break",
            240,
        ),
    ] {
        seeded += seed_alert(
            backend,
            AlertKind::Macro,
            "",
            condition,
            label,
            true,
            cooldown,
        )? as usize;
    }

    println!("Seeded {} default smart alerts.", seeded);
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
    condition: Option<String>,
    rule_text: String,
    status: String,
    threshold: String,
    direction: String,
    current_value: Option<String>,
    newly_triggered: bool,
    distance_pct: Option<String>,
    triggered_at: Option<String>,
    recurring: bool,
    cooldown_minutes: i64,
    trigger_data: serde_json::Value,
}

impl From<&AlertCheckResult> for AlertCheckJson {
    fn from(r: &AlertCheckResult) -> Self {
        AlertCheckJson {
            id: r.rule.id,
            kind: r.rule.kind.to_string(),
            symbol: r.rule.symbol.clone(),
            condition: r.rule.condition.clone(),
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
            recurring: r.rule.recurring,
            cooldown_minutes: r.rule.cooldown_minutes,
            trigger_data: r.trigger_data.clone(),
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
    use crate::db::technical_levels::{self, TechnicalLevelRecord};
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
                    previous_close: None,
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
                    previous_close: None,
            },
        )
        .unwrap();
        conn
    }

    fn setup_backend() -> BackendConnection {
        BackendConnection::Sqlite { conn: setup_db() }
    }

    fn insert_level(backend: &BackendConnection, symbol: &str, level_type: &str, price: f64) {
        technical_levels::upsert_levels_backend(
            backend,
            symbol,
            &[TechnicalLevelRecord {
                id: None,
                symbol: symbol.to_string(),
                level_type: level_type.to_string(),
                price,
                strength: 0.8,
                source_method: "test".to_string(),
                timeframe: "1d".to_string(),
                notes: Some("test level".to_string()),
                computed_at: "2026-03-18T16:00:00Z".to_string(),
            }],
        )
        .unwrap();
    }

    fn default_args() -> AlertsArgs {
        AlertsArgs {
            rule: None,
            id: None,
            json: false,
            status_filter: None,
            today: false,
            kind: None,
            symbol: None,
            from_level: None,
            condition: None,
            label: None,
            triggered: false,
            since_hours: None,
            recurring: false,
            cooldown_minutes: 0,
        }
    }

    #[test]
    fn test_add_alert_via_cli() {
        let backend = setup_backend();
        let args = AlertsArgs {
            rule: Some("GC=F above 5500".to_string()),
            ..default_args()
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
            ..default_args()
        };
        run_add(&backend, &args).unwrap();
        let alerts = alerts_db::list_alerts_backend(&backend).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, crate::alerts::AlertKind::Allocation);
    }

    #[test]
    fn test_add_alert_from_stored_support_level() {
        let backend = setup_backend();
        insert_level(&backend, "BTC", "support", 65000.0);
        let args = AlertsArgs {
            kind: Some("price".to_string()),
            symbol: Some("BTC".to_string()),
            from_level: Some("support".to_string()),
            ..default_args()
        };

        run_add(&backend, &args).unwrap();
        let alerts = alerts_db::list_alerts_backend(&backend).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].threshold, "65000");
        assert_eq!(alerts[0].direction, AlertDirection::Below);
    }

    #[test]
    fn test_add_empty_rule_fails() {
        let backend = setup_backend();
        let args = AlertsArgs {
            rule: Some(String::new()),
            ..default_args()
        };
        assert!(run_add(&backend, &args).is_err());
    }

    #[test]
    fn test_remove_alert_via_cli() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5500",
                rule_text: "GC=F above 5500",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        let args = AlertsArgs {
            id: Some(id),
            ..default_args()
        };
        run_remove(&backend, &args).unwrap();
        assert!(alerts_db::list_alerts_backend(&backend).unwrap().is_empty());
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let backend = setup_backend();
        let args = AlertsArgs {
            id: Some(999),
            ..default_args()
        };
        assert!(run_remove(&backend, &args).is_err());
    }

    #[test]
    fn test_check_triggers_armed_alert() {
        let backend = setup_backend();
        // GC=F at 5600, alert for above 5500 → should trigger
        alerts_db::add_alert_backend(
            &backend,
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5500",
                rule_text: "GC=F above 5500",
                recurring: false,
                cooldown_minutes: 0,
            },
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
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "6000",
                rule_text: "GC=F above 6000",
                recurring: false,
                cooldown_minutes: 0,
            },
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
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5500",
                rule_text: "GC=F above 5500",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        // Trigger it
        check_alerts_backend_only(&backend).unwrap();
        let args = AlertsArgs {
            id: Some(id),
            ..default_args()
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
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "6000",
                rule_text: "GC=F above 6000",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        let args = AlertsArgs {
            id: Some(id),
            ..default_args()
        };
        assert!(run_ack(&backend, &args).is_err());
    }

    #[test]
    fn test_rearm_triggered_alert() {
        let backend = setup_backend();
        let id = alerts_db::add_alert_backend(
            &backend,
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5500",
                rule_text: "GC=F above 5500",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        check_alerts_backend_only(&backend).unwrap(); // triggers it
        let args = AlertsArgs {
            id: Some(id),
            ..default_args()
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
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "6000",
                rule_text: "GC=F above 6000",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        let args = AlertsArgs {
            id: Some(id),
            ..default_args()
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
                condition: None,
                threshold: "6000".to_string(),
                status: AlertStatus::Armed,
                rule_text: "GC=F above 6000".to_string(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-03-04".to_string(),
                triggered_at: None,
            },
            current_value: Some(Decimal::from(5600)),
            newly_triggered: false,
            distance_pct: Some(Decimal::from_str("6.67").unwrap()),
            trigger_data: json!({}),
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
            condition: None,
            threshold: "6000".to_string(),
            status: AlertStatus::Triggered,
            rule_text: "GC=F above 6000".to_string(),
            recurring: false,
            cooldown_minutes: 0,
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
            condition: None,
            threshold: "90000".to_string(),
            status: AlertStatus::Triggered,
            rule_text: "BTC below 90000".to_string(),
            recurring: false,
            cooldown_minutes: 0,
            created_at: "2026-03-04".to_string(),
            triggered_at: Some(old),
        };
        assert!(!alert_matches_today_filter(&alert));
    }

    #[test]
    fn test_add_structured_technical_alert() {
        let backend = setup_backend();
        let args = AlertsArgs {
            kind: Some("technical".to_string()),
            symbol: Some("BTC".to_string()),
            condition: Some("price_below_sma200".to_string()),
            label: Some("BTC lost 200-day SMA".to_string()),
            recurring: true,
            cooldown_minutes: 30,
            ..default_args()
        };
        run_add(&backend, &args).unwrap();
        let alerts = alerts_db::list_alerts_backend(&backend).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::Technical);
        assert_eq!(alerts[0].condition.as_deref(), Some("price_below_sma200"));
        assert!(alerts[0].recurring);
        assert_eq!(alerts[0].cooldown_minutes, 30);
    }

    #[test]
    fn test_seed_defaults_creates_macro_and_symbol_alerts() {
        let backend = setup_backend();
        let conn = backend.sqlite();
        crate::db::transactions::insert_transaction(
            conn,
            &crate::models::transaction::NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: Decimal::from(2),
                price_per: Decimal::from(100),
                currency: "USD".to_string(),
                date: "2026-03-12".to_string(),
                notes: None,
            },
        )
        .unwrap();

        run_seed_defaults(&backend).unwrap();
        let alerts = alerts_db::list_alerts_backend(&backend).unwrap();
        assert!(alerts.iter().any(|alert| alert.symbol == "AAPL"));
        assert!(alerts.iter().any(|alert| alert.kind == AlertKind::Macro
            && alert.condition.as_deref() == Some("regime_change")));
    }
}
