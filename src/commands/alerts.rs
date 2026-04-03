use anyhow::{bail, Result};
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

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
use crate::models::position::compute_positions;
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
    /// Multiple IDs for bulk operations (e.g. ack).
    pub ids: Vec<i64>,
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
    /// Show recently triggered/acknowledged alerts for investigation continuity.
    pub recent: bool,
    /// Number of hours for the recent filter (default: 24).
    pub recent_hours: i64,
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

    // --recent: show recently triggered/acknowledged alerts for investigation continuity
    if args.recent {
        let status_filter = args
            .status_filter
            .as_ref()
            .map(|s| s.parse::<AlertStatus>())
            .transpose()?;
        let alerts =
            alerts_db::list_alerts_recent_backend(backend, args.recent_hours, status_filter)?;
        if args.json {
            let payload: Vec<_> = alerts
                .iter()
                .map(|alert| {
                    json!({
                        "id": alert.id,
                        "kind": alert.kind.to_string(),
                        "symbol": alert.symbol,
                        "condition": alert.condition,
                        "rule_text": alert.rule_text,
                        "status": alert.status.to_string(),
                        "threshold": alert.threshold,
                        "direction": alert.direction.to_string(),
                        "triggered_at": alert.triggered_at,
                        "recurring": alert.recurring,
                        "cooldown_minutes": alert.cooldown_minutes,
                    })
                })
                .collect();
            let wrapper = json!({
                "recent_hours": args.recent_hours,
                "count": alerts.len(),
                "alerts": payload,
                "hint": "Use --recent-hours N to adjust the lookback window (default: 24)."
            });
            println!("{}", serde_json::to_string_pretty(&wrapper)?);
        } else if alerts.is_empty() {
            println!(
                "No recently triggered/acknowledged alerts in the last {} hours.",
                args.recent_hours
            );
            println!(
                "Hint: Use --recent-hours N to adjust the lookback window (default: 24)."
            );
        } else {
            println!(
                "Recent alerts (last {} hours) — {}:\n",
                args.recent_hours,
                alerts.len()
            );
            for alert in &alerts {
                let status_icon = match alert.status {
                    AlertStatus::Armed => "🟢",
                    AlertStatus::Triggered => "🔴",
                    AlertStatus::Acknowledged => "✅",
                };
                let triggered_at = alert.triggered_at.as_deref().unwrap_or("unknown");
                println!(
                    "  {} [#{}] {} ({}) — triggered: {}",
                    status_icon, alert.id, alert.rule_text, alert.kind, triggered_at
                );
                if let Some(ref condition) = alert.condition {
                    println!("      Condition: {}", condition);
                }
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
        "correlation_regime_break" => "0.3".to_string(),
        "scenario_probability_shift" => "10".to_string(),
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
    // Collect IDs from both the bulk `ids` vec and the legacy single `id` field.
    let mut all_ids = args.ids.clone();
    if let Some(single) = args.id {
        if !all_ids.contains(&single) {
            all_ids.push(single);
        }
    }

    if all_ids.is_empty() {
        eprintln!("Usage: pftui analytics alerts ack <ID> [<ID> ...]");
        std::process::exit(1);
    }

    let mut acked = Vec::new();
    let mut errors = Vec::new();

    for id in &all_ids {
        match alerts_db::get_alert_backend(backend, *id)? {
            Some(alert) => {
                if alert.status != AlertStatus::Triggered {
                    errors.push(format!(
                        "Alert #{} is not triggered (status: {})",
                        id, alert.status
                    ));
                    continue;
                }
                alerts_db::acknowledge_alert_backend(backend, *id)?;
                let _ = triggered_alerts_db::acknowledge_for_alert_backend(backend, *id)?;
                acked.push((*id, alert.rule_text.clone()));
            }
            None => {
                errors.push(format!("No alert found with id #{}", id));
            }
        }
    }

    if args.json {
        let result = serde_json::json!({
            "acked": acked.iter().map(|(id, rule)| serde_json::json!({"id": id, "rule": rule})).collect::<Vec<_>>(),
            "errors": errors,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        for (id, rule) in &acked {
            println!("✅ Acknowledged alert #{}: {}", id, rule);
        }
        for err in &errors {
            eprintln!("⚠️  {}", err);
        }
    }

    if !errors.is_empty() && acked.is_empty() {
        bail!("No alerts were acknowledged");
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
        (
            "scenario_probability_shift",
            "Scenario probability shifted ≥10pp",
            60,
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

// ── Alert Triage Dashboard ──────────────────────────────────────────

/// Urgency tier for alert triage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TriageUrgency {
    /// Newly triggered — needs immediate attention
    Critical,
    /// Previously triggered, not yet acknowledged
    High,
    /// Armed and within 5% of threshold
    Watch,
    /// Armed but far from threshold
    Low,
}

impl fmt::Display for TriageUrgency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriageUrgency::Critical => write!(f, "critical"),
            TriageUrgency::High => write!(f, "high"),
            TriageUrgency::Watch => write!(f, "watch"),
            TriageUrgency::Low => write!(f, "low"),
        }
    }
}

/// A single triaged alert entry.
#[derive(Debug, Clone, Serialize)]
pub struct TriageEntry {
    pub id: i64,
    pub urgency: TriageUrgency,
    pub kind: String,
    pub symbol: String,
    pub rule_text: String,
    pub status: String,
    pub current_value: Option<String>,
    pub threshold: String,
    pub direction: String,
    pub distance_pct: Option<String>,
    pub triggered_at: Option<String>,
    pub condition: Option<String>,
    pub recurring: bool,
    /// Portfolio allocation % for this alert's symbol (None = not held / watchlist only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portfolio_impact_pct: Option<String>,
    /// Whether this alert's symbol is in the portfolio (true) vs watchlist/external (false).
    pub in_portfolio: bool,
}

/// Per-kind group summary.
#[derive(Debug, Clone, Serialize)]
pub struct KindGroup {
    pub kind: String,
    pub count: usize,
    pub critical: usize,
    pub high: usize,
    pub watch: usize,
    pub low: usize,
}

/// Full triage dashboard output.
#[derive(Debug, Clone, Serialize)]
pub struct TriageDashboard {
    pub total: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub watch_count: usize,
    pub low_count: usize,
    pub acknowledged_count: usize,
    /// Total portfolio allocation % covered by alerts in each urgency tier.
    pub portfolio_exposure: PortfolioExposure,
    pub by_kind: Vec<KindGroup>,
    pub alerts: Vec<TriageEntry>,
}

/// Portfolio allocation exposure by urgency tier.
#[derive(Debug, Clone, Serialize)]
pub struct PortfolioExposure {
    /// Sum of portfolio allocation % for critical-tier alerts (deduplicated by symbol).
    pub critical_pct: String,
    /// Sum of portfolio allocation % for high-tier alerts (deduplicated by symbol).
    pub high_pct: String,
    /// Sum of portfolio allocation % for watch-tier alerts (deduplicated by symbol).
    pub watch_pct: String,
    /// Sum of portfolio allocation % for low-tier alerts (deduplicated by symbol).
    pub low_pct: String,
    /// Total portfolio % with at least one alert (deduplicated by symbol).
    pub total_covered_pct: String,
}

/// Classify an alert check result into an urgency tier.
fn classify_urgency(r: &AlertCheckResult) -> Option<TriageUrgency> {
    if r.rule.status == AlertStatus::Acknowledged {
        return None; // excluded from triage tiers
    }
    if r.newly_triggered {
        return Some(TriageUrgency::Critical);
    }
    if r.rule.status == AlertStatus::Triggered {
        return Some(TriageUrgency::High);
    }
    // Armed — check distance
    if let Some(dist) = r.distance_pct {
        let abs_dist = dist.abs();
        let five = Decimal::from(5);
        if abs_dist <= five {
            Some(TriageUrgency::Watch)
        } else {
            Some(TriageUrgency::Low)
        }
    } else {
        Some(TriageUrgency::Low)
    }
}

/// Build a symbol → allocation_pct map from current portfolio positions.
fn build_allocation_map(backend: &BackendConnection) -> HashMap<String, Decimal> {
    let txs = crate::db::transactions::list_transactions_backend(backend).unwrap_or_default();
    if txs.is_empty() {
        // Try percentage-mode allocations
        let allocs =
            crate::db::allocations::list_allocations_backend(backend).unwrap_or_default();
        return allocs
            .into_iter()
            .filter(|a| a.category != AssetCategory::Cash)
            .map(|a| (a.symbol.to_uppercase(), a.allocation_pct))
            .collect();
    }

    let cached = price_cache::get_all_cached_prices_backend(backend).unwrap_or_default();
    let mut prices: HashMap<String, Decimal> = cached
        .into_iter()
        .map(|q| (q.symbol.clone(), q.price))
        .collect();

    // Ensure cash assets price at 1.0
    for tx in &txs {
        if tx.category == AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions(&txs, &prices, &fx_rates);

    positions
        .into_iter()
        .filter_map(|p| {
            p.allocation_pct
                .map(|alloc| (p.symbol.to_uppercase(), alloc))
        })
        .collect()
}

/// Build and return the triage dashboard.
pub fn build_triage(backend: &BackendConnection) -> Result<TriageDashboard> {
    let results = check_alerts_backend_only(backend)?;
    let alloc_map = build_allocation_map(backend);

    let mut entries: Vec<TriageEntry> = Vec::new();
    let mut acknowledged_count: usize = 0;
    let mut kind_counts: std::collections::BTreeMap<
        String,
        (usize, usize, usize, usize, usize),
    > = std::collections::BTreeMap::new();

    for r in &results {
        let urgency = match classify_urgency(r) {
            Some(u) => u,
            None => {
                acknowledged_count += 1;
                continue;
            }
        };

        let kind_str = r.rule.kind.to_string();
        let entry = kind_counts.entry(kind_str.clone()).or_default();
        entry.0 += 1; // total
        match urgency {
            TriageUrgency::Critical => entry.1 += 1,
            TriageUrgency::High => entry.2 += 1,
            TriageUrgency::Watch => entry.3 += 1,
            TriageUrgency::Low => entry.4 += 1,
        }

        let symbol_upper = r.rule.symbol.to_uppercase();
        let alloc = alloc_map.get(&symbol_upper).copied();
        let in_portfolio = alloc.is_some();

        entries.push(TriageEntry {
            id: r.rule.id,
            urgency,
            kind: kind_str,
            symbol: r.rule.symbol.clone(),
            rule_text: r.rule.rule_text.clone(),
            status: if r.newly_triggered {
                "triggered".to_string()
            } else {
                r.rule.status.to_string()
            },
            current_value: r.current_value.map(|v| v.to_string()),
            threshold: r.rule.threshold.clone(),
            direction: r.rule.direction.to_string(),
            distance_pct: r.distance_pct.map(|d| d.round_dp(2).to_string()),
            triggered_at: r.rule.triggered_at.clone(),
            condition: r.rule.condition.clone(),
            recurring: r.rule.recurring,
            portfolio_impact_pct: alloc.map(|a| a.round_dp(2).to_string()),
            in_portfolio,
        });
    }

    // Sort entries: urgency first, then portfolio impact (highest first) within each tier
    entries.sort_by(|a, b| {
        a.urgency.cmp(&b.urgency).then_with(|| {
            let a_impact: Decimal = a
                .portfolio_impact_pct
                .as_ref()
                .and_then(|s| Decimal::from_str(s).ok())
                .unwrap_or_default();
            let b_impact: Decimal = b
                .portfolio_impact_pct
                .as_ref()
                .and_then(|s| Decimal::from_str(s).ok())
                .unwrap_or_default();
            b_impact.cmp(&a_impact) // descending — highest impact first
        })
    });

    let critical_count = entries
        .iter()
        .filter(|e| e.urgency == TriageUrgency::Critical)
        .count();
    let high_count = entries
        .iter()
        .filter(|e| e.urgency == TriageUrgency::High)
        .count();
    let watch_count = entries
        .iter()
        .filter(|e| e.urgency == TriageUrgency::Watch)
        .count();
    let low_count = entries
        .iter()
        .filter(|e| e.urgency == TriageUrgency::Low)
        .count();

    let by_kind: Vec<KindGroup> = kind_counts
        .into_iter()
        .map(|(kind, (count, critical, high, watch, low))| KindGroup {
            kind,
            count,
            critical,
            high,
            watch,
            low,
        })
        .collect();

    // Compute portfolio exposure by urgency tier (deduplicated by symbol)
    let portfolio_exposure = compute_portfolio_exposure(&entries, &alloc_map);

    Ok(TriageDashboard {
        total: entries.len(),
        critical_count,
        high_count,
        watch_count,
        low_count,
        acknowledged_count,
        portfolio_exposure,
        by_kind,
        alerts: entries,
    })
}

/// Compute portfolio allocation exposure by urgency tier (deduplicated by symbol).
fn compute_portfolio_exposure(
    entries: &[TriageEntry],
    alloc_map: &HashMap<String, Decimal>,
) -> PortfolioExposure {
    use std::collections::HashSet;

    let mut critical_symbols = HashSet::new();
    let mut high_symbols = HashSet::new();
    let mut watch_symbols = HashSet::new();
    let mut low_symbols = HashSet::new();
    let mut all_symbols = HashSet::new();

    for e in entries {
        let sym = e.symbol.to_uppercase();
        if alloc_map.contains_key(&sym) {
            all_symbols.insert(sym.clone());
            match e.urgency {
                TriageUrgency::Critical => {
                    critical_symbols.insert(sym);
                }
                TriageUrgency::High => {
                    high_symbols.insert(sym);
                }
                TriageUrgency::Watch => {
                    watch_symbols.insert(sym);
                }
                TriageUrgency::Low => {
                    low_symbols.insert(sym);
                }
            }
        }
    }

    let sum_alloc = |symbols: &HashSet<String>| -> Decimal {
        symbols
            .iter()
            .filter_map(|s| alloc_map.get(s))
            .copied()
            .sum()
    };

    let fmt = |d: Decimal| d.round_dp(2).to_string();

    PortfolioExposure {
        critical_pct: fmt(sum_alloc(&critical_symbols)),
        high_pct: fmt(sum_alloc(&high_symbols)),
        watch_pct: fmt(sum_alloc(&watch_symbols)),
        low_pct: fmt(sum_alloc(&low_symbols)),
        total_covered_pct: fmt(sum_alloc(&all_symbols)),
    }
}

/// Format a compact portfolio impact tag for terminal display.
/// Returns " [20.5% portfolio]" for held assets, "" for watchlist/external.
fn format_impact_tag(e: &TriageEntry) -> String {
    match &e.portfolio_impact_pct {
        Some(pct) => format!(" [{}% portfolio]", pct),
        None => String::new(),
    }
}

/// Run the triage dashboard CLI command.
pub fn run_triage(backend: &BackendConnection, json: bool) -> Result<()> {
    let dashboard = build_triage(backend)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&dashboard)?);
        return Ok(());
    }

    // Terminal output
    println!("╔══════════════════════════════════════╗");
    println!("║       ALERT TRIAGE DASHBOARD         ║");
    println!("╚══════════════════════════════════════╝\n");

    println!(
        "Total: {} alerts | 🔴 {} critical | 🟠 {} high | 🟡 {} watch | 🟢 {} low | ✅ {} ack'd\n",
        dashboard.total,
        dashboard.critical_count,
        dashboard.high_count,
        dashboard.watch_count,
        dashboard.low_count,
        dashboard.acknowledged_count
    );

    // Portfolio exposure summary
    let pe = &dashboard.portfolio_exposure;
    if pe.total_covered_pct != "0" && pe.total_covered_pct != "0.00" {
        println!(
            "Portfolio exposure: {}% covered | 🔴 {}% | 🟠 {}% | 🟡 {}% | 🟢 {}%\n",
            pe.total_covered_pct, pe.critical_pct, pe.high_pct, pe.watch_pct, pe.low_pct
        );
    }

    // By-kind breakdown
    if !dashboard.by_kind.is_empty() {
        println!("BY KIND:");
        for group in &dashboard.by_kind {
            println!(
                "  {:<12} {:>3} total  (🔴{} 🟠{} 🟡{} 🟢{})",
                group.kind, group.count, group.critical, group.high, group.watch, group.low
            );
        }
        println!();
    }

    // Critical tier
    let critical: Vec<&TriageEntry> = dashboard
        .alerts
        .iter()
        .filter(|e| e.urgency == TriageUrgency::Critical)
        .collect();
    if !critical.is_empty() {
        println!("🔴 CRITICAL — Newly Triggered ({}):\n", critical.len());
        for e in &critical {
            let current = e.current_value.as_deref().unwrap_or("N/A");
            let impact = format_impact_tag(e);
            println!("  🔴 [#{}] {} — current: {}{}", e.id, e.rule_text, current, impact);
        }
        println!();
    }

    // High tier
    let high: Vec<&TriageEntry> = dashboard
        .alerts
        .iter()
        .filter(|e| e.urgency == TriageUrgency::High)
        .collect();
    if !high.is_empty() {
        println!(
            "🟠 HIGH — Triggered, Unacknowledged ({}):\n",
            high.len()
        );
        for e in &high {
            let current = e.current_value.as_deref().unwrap_or("N/A");
            let triggered = e.triggered_at.as_deref().unwrap_or("unknown");
            let impact = format_impact_tag(e);
            println!(
                "  🟠 [#{}] {} — current: {} (triggered: {}){}",
                e.id, e.rule_text, current, triggered, impact
            );
        }
        println!();
    }

    // Watch tier
    let watch: Vec<&TriageEntry> = dashboard
        .alerts
        .iter()
        .filter(|e| e.urgency == TriageUrgency::Watch)
        .collect();
    if !watch.is_empty() {
        println!(
            "🟡 WATCH — Armed, Within 5% of Threshold ({}):\n",
            watch.len()
        );
        for e in &watch {
            let current = e.current_value.as_deref().unwrap_or("N/A");
            let dist = e
                .distance_pct
                .as_ref()
                .map(|d| format!("({}% to target)", d))
                .unwrap_or_default();
            let impact = format_impact_tag(e);
            println!(
                "  🟡 [#{}] {} — current: {} {}{}",
                e.id, e.rule_text, current, dist, impact
            );
        }
        println!();
    }

    // Low tier
    let low: Vec<&TriageEntry> = dashboard
        .alerts
        .iter()
        .filter(|e| e.urgency == TriageUrgency::Low)
        .collect();
    if !low.is_empty() {
        println!("🟢 LOW — Armed, >5% From Threshold ({}):\n", low.len());
        for e in &low {
            let current = e.current_value.as_deref().unwrap_or("N/A");
            let dist = e
                .distance_pct
                .as_ref()
                .map(|d| format!("({}% to target)", d))
                .unwrap_or_default();
            let impact = format_impact_tag(e);
            println!(
                "  🟢 [#{}] {} — current: {} {}{}",
                e.id, e.rule_text, current, dist, impact
            );
        }
        println!();
    }

    if dashboard.total == 0 {
        println!("No active alerts. Run `analytics alerts seed-defaults` to create smart defaults.");
    }

    Ok(())
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
            ids: vec![],
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
            recent: false,
            recent_hours: 24,
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
    fn test_bulk_ack_multiple_triggered_alerts() {
        let backend = setup_backend();
        let id1 = alerts_db::add_alert_backend(
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
        let id2 = alerts_db::add_alert_backend(
            &backend,
            NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "5000",
                rule_text: "GC=F above 5000",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        // Trigger both
        check_alerts_backend_only(&backend).unwrap();
        let args = AlertsArgs {
            ids: vec![id1, id2],
            ..default_args()
        };
        run_ack(&backend, &args).unwrap();
        let a1 = alerts_db::get_alert_backend(&backend, id1)
            .unwrap()
            .unwrap();
        let a2 = alerts_db::get_alert_backend(&backend, id2)
            .unwrap()
            .unwrap();
        assert_eq!(a1.status, AlertStatus::Acknowledged);
        assert_eq!(a2.status, AlertStatus::Acknowledged);
    }

    #[test]
    fn test_bulk_ack_partial_failure() {
        let backend = setup_backend();
        let id1 = alerts_db::add_alert_backend(
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
        let id2 = alerts_db::add_alert_backend(
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
        // Only trigger id1 (above 5500), id2 (above 6000) stays armed
        check_alerts_backend_only(&backend).unwrap();
        let args = AlertsArgs {
            ids: vec![id1, id2],
            ..default_args()
        };
        // Should succeed (partial) — id1 acked, id2 error
        run_ack(&backend, &args).unwrap();
        let a1 = alerts_db::get_alert_backend(&backend, id1)
            .unwrap()
            .unwrap();
        assert_eq!(a1.status, AlertStatus::Acknowledged);
        let a2 = alerts_db::get_alert_backend(&backend, id2)
            .unwrap()
            .unwrap();
        assert_eq!(a2.status, AlertStatus::Armed); // still armed
    }

    #[test]
    fn test_bulk_ack_nonexistent_id_fails() {
        let backend = setup_backend();
        let args = AlertsArgs {
            ids: vec![999],
            ..default_args()
        };
        // All failed → error
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
        // Verify new scenario_probability_shift alert is seeded
        assert!(alerts.iter().any(|alert| alert.kind == AlertKind::Macro
            && alert.condition.as_deref() == Some("scenario_probability_shift")));
        // Verify correlation_regime_break has configurable threshold
        let corr_alert = alerts.iter().find(|alert| alert.condition.as_deref() == Some("correlation_regime_break"));
        assert!(corr_alert.is_some(), "correlation_regime_break should be seeded");
        assert_eq!(corr_alert.unwrap().threshold, "0.3", "default correlation threshold should be 0.3");
        // Verify scenario_probability_shift has configurable threshold
        let scenario_alert = alerts.iter().find(|alert| alert.condition.as_deref() == Some("scenario_probability_shift"));
        assert!(scenario_alert.is_some());
        assert_eq!(scenario_alert.unwrap().threshold, "10", "default scenario threshold should be 10pp");
    }

    #[test]
    fn test_inferred_threshold_new_conditions() {
        assert_eq!(inferred_threshold("correlation_regime_break"), "0.3");
        assert_eq!(inferred_threshold("scenario_probability_shift"), "10");
    }

    #[test]
    fn test_list_recent_shows_recently_acked_alerts() {
        let backend = setup_backend();
        // Create and trigger an alert, then ack it
        let id = alerts_db::add_alert_backend(
            &backend,
            alerts_db::NewAlert {
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
        run_ack(
            &backend,
            &AlertsArgs {
                id: Some(id),
                ..default_args()
            },
        )
        .unwrap();

        // Now list with --recent --status acknowledged
        let args = AlertsArgs {
            recent: true,
            recent_hours: 24,
            status_filter: Some("acknowledged".to_string()),
            json: true,
            ..default_args()
        };
        // Should not panic/error
        run_list(&backend, &args).unwrap();
    }

    #[test]
    fn test_list_recent_without_status_filter() {
        let backend = setup_backend();
        let _id = alerts_db::add_alert_backend(
            &backend,
            alerts_db::NewAlert {
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

        // List with --recent (no status filter — should show triggered)
        let args = AlertsArgs {
            recent: true,
            recent_hours: 24,
            json: true,
            ..default_args()
        };
        run_list(&backend, &args).unwrap();
    }

    // ── Triage tests ────────────────────────────────────────────────

    #[test]
    fn test_classify_urgency_newly_triggered() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 1,
                kind: AlertKind::Price,
                symbol: "BTC".into(),
                direction: AlertDirection::Above,
                condition: None,
                threshold: "100000".into(),
                status: AlertStatus::Triggered,
                rule_text: "BTC above 100000".into(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-01-01".into(),
                triggered_at: Some("2026-03-28".into()),
            },
            current_value: Some(Decimal::from(105000)),
            newly_triggered: true,
            distance_pct: Some(Decimal::from(0)),
            trigger_data: json!({}),
        };
        assert_eq!(classify_urgency(&r), Some(TriageUrgency::Critical));
    }

    #[test]
    fn test_classify_urgency_previously_triggered() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 2,
                kind: AlertKind::Price,
                symbol: "GC=F".into(),
                direction: AlertDirection::Above,
                condition: None,
                threshold: "3000".into(),
                status: AlertStatus::Triggered,
                rule_text: "GC=F above 3000".into(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-01-01".into(),
                triggered_at: Some("2026-03-27".into()),
            },
            current_value: Some(Decimal::from(3100)),
            newly_triggered: false,
            distance_pct: Some(Decimal::from(0)),
            trigger_data: json!({}),
        };
        assert_eq!(classify_urgency(&r), Some(TriageUrgency::High));
    }

    #[test]
    fn test_classify_urgency_watch_within_5pct() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 3,
                kind: AlertKind::Price,
                symbol: "SI=F".into(),
                direction: AlertDirection::Above,
                condition: None,
                threshold: "30".into(),
                status: AlertStatus::Armed,
                rule_text: "SI=F above 30".into(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-01-01".into(),
                triggered_at: None,
            },
            current_value: Some(Decimal::from(29)),
            newly_triggered: false,
            distance_pct: Some(Decimal::from_str("3.4").unwrap()),
            trigger_data: json!({}),
        };
        assert_eq!(classify_urgency(&r), Some(TriageUrgency::Watch));
    }

    #[test]
    fn test_classify_urgency_low_far_from_threshold() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 4,
                kind: AlertKind::Price,
                symbol: "CL=F".into(),
                direction: AlertDirection::Below,
                condition: None,
                threshold: "50".into(),
                status: AlertStatus::Armed,
                rule_text: "CL=F below 50".into(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-01-01".into(),
                triggered_at: None,
            },
            current_value: Some(Decimal::from(70)),
            newly_triggered: false,
            distance_pct: Some(Decimal::from(28)),
            trigger_data: json!({}),
        };
        assert_eq!(classify_urgency(&r), Some(TriageUrgency::Low));
    }

    #[test]
    fn test_classify_urgency_acknowledged_excluded() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 5,
                kind: AlertKind::Price,
                symbol: "BTC".into(),
                direction: AlertDirection::Above,
                condition: None,
                threshold: "90000".into(),
                status: AlertStatus::Acknowledged,
                rule_text: "BTC above 90000".into(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-01-01".into(),
                triggered_at: Some("2026-03-25".into()),
            },
            current_value: Some(Decimal::from(95000)),
            newly_triggered: false,
            distance_pct: None,
            trigger_data: json!({}),
        };
        assert_eq!(classify_urgency(&r), None);
    }

    #[test]
    fn test_classify_urgency_watch_boundary_at_5pct() {
        let r = AlertCheckResult {
            rule: AlertRule {
                id: 6,
                kind: AlertKind::Price,
                symbol: "GC=F".into(),
                direction: AlertDirection::Above,
                condition: None,
                threshold: "3000".into(),
                status: AlertStatus::Armed,
                rule_text: "GC=F above 3000".into(),
                recurring: false,
                cooldown_minutes: 0,
                created_at: "2026-01-01".into(),
                triggered_at: None,
            },
            current_value: Some(Decimal::from(2850)),
            newly_triggered: false,
            distance_pct: Some(Decimal::from(5)),
            trigger_data: json!({}),
        };
        // Exactly 5% should be Watch (<=5)
        assert_eq!(classify_urgency(&r), Some(TriageUrgency::Watch));
    }

    fn make_price_quote(symbol: &str, price: i64) -> PriceQuote {
        PriceQuote {
            symbol: symbol.to_string(),
            price: Decimal::from(price),
            currency: "USD".to_string(),
            fetched_at: "2026-03-28".into(),
            source: "test".into(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
        }
    }

    #[test]
    fn test_triage_dashboard_groups_by_kind() {
        let conn = setup_db();
        let backend = BackendConnection::Sqlite { conn };

        // Add cached prices
        price_cache::upsert_price(
            backend.sqlite(),
            &make_price_quote("BTC", 85000),
        )
        .unwrap();
        price_cache::upsert_price(
            backend.sqlite(),
            &make_price_quote("GC=F", 3050),
        )
        .unwrap();

        // Add price alerts
        alerts_db::add_alert_backend(
            &backend,
            alerts_db::NewAlert {
                kind: "price",
                symbol: "BTC",
                direction: "above",
                condition: None,
                threshold: "100000",
                rule_text: "BTC above 100000",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();
        alerts_db::add_alert_backend(
            &backend,
            alerts_db::NewAlert {
                kind: "price",
                symbol: "GC=F",
                direction: "above",
                condition: None,
                threshold: "3200",
                rule_text: "GC=F above 3200",
                recurring: false,
                cooldown_minutes: 0,
            },
        )
        .unwrap();

        let dashboard = build_triage(&backend).unwrap();

        assert_eq!(dashboard.total, 2);
        assert_eq!(dashboard.critical_count, 0);
        assert_eq!(dashboard.high_count, 0);
        // Both are armed, BTC is ~17.6% away (low), GC=F is ~4.9% away (watch)
        assert!(dashboard.watch_count + dashboard.low_count == 2);
        assert_eq!(dashboard.acknowledged_count, 0);
        assert_eq!(dashboard.by_kind.len(), 1);
        assert_eq!(dashboard.by_kind[0].kind, "price");
        assert_eq!(dashboard.by_kind[0].count, 2);
    }

    #[test]
    fn test_triage_dashboard_empty() {
        let conn = setup_db();
        let backend = BackendConnection::Sqlite { conn };
        let dashboard = build_triage(&backend).unwrap();
        assert_eq!(dashboard.total, 0);
        assert_eq!(dashboard.critical_count, 0);
        assert_eq!(dashboard.by_kind.len(), 0);
        assert!(dashboard.alerts.is_empty());
    }

    #[test]
    fn test_triage_urgency_ordering() {
        // Verify that Critical < High < Watch < Low for sorting
        assert!(TriageUrgency::Critical < TriageUrgency::High);
        assert!(TriageUrgency::High < TriageUrgency::Watch);
        assert!(TriageUrgency::Watch < TriageUrgency::Low);
    }

    #[test]
    fn test_triage_entry_serializes_to_json() {
        let entry = TriageEntry {
            id: 1,
            urgency: TriageUrgency::Critical,
            kind: "price".into(),
            symbol: "BTC".into(),
            rule_text: "BTC above 100000".into(),
            status: "triggered".into(),
            current_value: Some("105000".into()),
            threshold: "100000".into(),
            direction: "above".into(),
            distance_pct: Some("0".into()),
            triggered_at: Some("2026-03-28".into()),
            condition: None,
            recurring: false,
            portfolio_impact_pct: Some("20.50".into()),
            in_portfolio: true,
        };
        let json_str = serde_json::to_string(&entry).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["urgency"], "critical");
        assert_eq!(parsed["kind"], "price");
        assert_eq!(parsed["symbol"], "BTC");
        assert_eq!(parsed["portfolio_impact_pct"], "20.50");
        assert_eq!(parsed["in_portfolio"], true);
    }

    #[test]
    fn test_triage_entry_no_portfolio_impact_omits_field() {
        let entry = TriageEntry {
            id: 2,
            urgency: TriageUrgency::Watch,
            kind: "price".into(),
            symbol: "TSLA".into(),
            rule_text: "TSLA above 300".into(),
            status: "armed".into(),
            current_value: Some("280".into()),
            threshold: "300".into(),
            direction: "above".into(),
            distance_pct: Some("3.50".into()),
            triggered_at: None,
            condition: None,
            recurring: false,
            portfolio_impact_pct: None,
            in_portfolio: false,
        };
        let json_str = serde_json::to_string(&entry).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.get("portfolio_impact_pct").is_none());
        assert_eq!(parsed["in_portfolio"], false);
    }

    #[test]
    fn test_portfolio_exposure_computation() {
        let alloc_map: HashMap<String, Decimal> = vec![
            ("BTC-USD".to_string(), Decimal::from_str("20.50").unwrap()),
            ("GC=F".to_string(), Decimal::from_str("15.30").unwrap()),
        ]
        .into_iter()
        .collect();

        let entries = vec![
            TriageEntry {
                id: 1,
                urgency: TriageUrgency::Critical,
                kind: "price".into(),
                symbol: "BTC-USD".into(),
                rule_text: "BTC above 100000".into(),
                status: "triggered".into(),
                current_value: Some("105000".into()),
                threshold: "100000".into(),
                direction: "above".into(),
                distance_pct: None,
                triggered_at: None,
                condition: None,
                recurring: false,
                portfolio_impact_pct: Some("20.50".into()),
                in_portfolio: true,
            },
            TriageEntry {
                id: 2,
                urgency: TriageUrgency::Watch,
                kind: "price".into(),
                symbol: "GC=F".into(),
                rule_text: "Gold above 3500".into(),
                status: "armed".into(),
                current_value: Some("3400".into()),
                threshold: "3500".into(),
                direction: "above".into(),
                distance_pct: Some("2.94".into()),
                triggered_at: None,
                condition: None,
                recurring: false,
                portfolio_impact_pct: Some("15.30".into()),
                in_portfolio: true,
            },
            TriageEntry {
                id: 3,
                urgency: TriageUrgency::Low,
                kind: "price".into(),
                symbol: "TSLA".into(),
                rule_text: "TSLA above 400".into(),
                status: "armed".into(),
                current_value: Some("280".into()),
                threshold: "400".into(),
                direction: "above".into(),
                distance_pct: Some("42.86".into()),
                triggered_at: None,
                condition: None,
                recurring: false,
                portfolio_impact_pct: None,
                in_portfolio: false,
            },
        ];

        let exposure = compute_portfolio_exposure(&entries, &alloc_map);
        assert_eq!(exposure.critical_pct, "20.50");
        assert_eq!(exposure.high_pct, "0");
        assert_eq!(exposure.watch_pct, "15.30");
        assert_eq!(exposure.low_pct, "0");
        assert_eq!(exposure.total_covered_pct, "35.80");
    }

    #[test]
    fn test_portfolio_exposure_deduplicates_symbols() {
        let alloc_map: HashMap<String, Decimal> = vec![(
            "BTC-USD".to_string(),
            Decimal::from_str("20.00").unwrap(),
        )]
        .into_iter()
        .collect();

        // Two alerts for the same symbol in the same tier
        let entries = vec![
            TriageEntry {
                id: 1,
                urgency: TriageUrgency::Critical,
                kind: "price".into(),
                symbol: "BTC-USD".into(),
                rule_text: "BTC above 100000".into(),
                status: "triggered".into(),
                current_value: Some("105000".into()),
                threshold: "100000".into(),
                direction: "above".into(),
                distance_pct: None,
                triggered_at: None,
                condition: None,
                recurring: false,
                portfolio_impact_pct: Some("20.00".into()),
                in_portfolio: true,
            },
            TriageEntry {
                id: 2,
                urgency: TriageUrgency::Critical,
                kind: "technical".into(),
                symbol: "BTC-USD".into(),
                rule_text: "BTC RSI overbought".into(),
                status: "triggered".into(),
                current_value: Some("75".into()),
                threshold: "70".into(),
                direction: "above".into(),
                distance_pct: None,
                triggered_at: None,
                condition: None,
                recurring: false,
                portfolio_impact_pct: Some("20.00".into()),
                in_portfolio: true,
            },
        ];

        let exposure = compute_portfolio_exposure(&entries, &alloc_map);
        // Should be 20%, not 40% — deduplicated by symbol
        assert_eq!(exposure.critical_pct, "20.00");
        assert_eq!(exposure.total_covered_pct, "20.00");
    }

    #[test]
    fn test_format_impact_tag() {
        let entry_with_impact = TriageEntry {
            id: 1,
            urgency: TriageUrgency::Critical,
            kind: "price".into(),
            symbol: "BTC".into(),
            rule_text: "test".into(),
            status: "triggered".into(),
            current_value: None,
            threshold: "100000".into(),
            direction: "above".into(),
            distance_pct: None,
            triggered_at: None,
            condition: None,
            recurring: false,
            portfolio_impact_pct: Some("20.50".into()),
            in_portfolio: true,
        };
        assert_eq!(format_impact_tag(&entry_with_impact), " [20.50% portfolio]");

        let entry_no_impact = TriageEntry {
            id: 2,
            urgency: TriageUrgency::Low,
            kind: "price".into(),
            symbol: "TSLA".into(),
            rule_text: "test".into(),
            status: "armed".into(),
            current_value: None,
            threshold: "400".into(),
            direction: "above".into(),
            distance_pct: None,
            triggered_at: None,
            condition: None,
            recurring: false,
            portfolio_impact_pct: None,
            in_portfolio: false,
        };
        assert_eq!(format_impact_tag(&entry_no_impact), "");
    }
}
