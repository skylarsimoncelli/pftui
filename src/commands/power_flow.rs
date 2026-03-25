use crate::db::backend::BackendConnection;
use crate::db::power_flows;
use anyhow::Result;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run_add(
    backend: &BackendConnection,
    event: &str,
    source: &str,
    direction: &str,
    target: Option<&str>,
    evidence: &str,
    magnitude: i32,
    agent_source: Option<&str>,
    date: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let date_val = date.unwrap_or(&today);

    let id = power_flows::add_power_flow_backend(
        backend,
        date_val,
        event,
        source,
        direction,
        target,
        evidence,
        magnitude,
        agent_source,
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": id,
                "date": date_val,
                "event": event,
                "source_complex": source,
                "direction": direction,
                "target_complex": target,
                "evidence": evidence,
                "magnitude": magnitude,
                "agent_source": agent_source,
            }))?
        );
    } else {
        let target_str = target
            .map(|t| format!(" → {}", t))
            .unwrap_or_default();
        println!(
            "Logged power flow: {} {} (mag {}){} — {}",
            source, direction, magnitude, target_str, event
        );
    }

    Ok(())
}

pub fn run_list(
    backend: &BackendConnection,
    complex: Option<&str>,
    direction: Option<&str>,
    days: usize,
    json_output: bool,
) -> Result<()> {
    let entries = power_flows::list_power_flows_backend(backend, complex, direction, days)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "power_flows": entries,
                "count": entries.len(),
                "period_days": days,
                "filters": {
                    "complex": complex,
                    "direction": direction,
                }
            }))?
        );
    } else {
        if entries.is_empty() {
            println!("No power flow entries in the last {} days.", days);
            if complex.is_some() || direction.is_some() {
                println!("Try removing filters or increasing --days.");
            }
            return Ok(());
        }

        println!("POWER FLOWS (last {} days)", days);
        println!("{}", "─".repeat(100));
        let header = format!(
            "{:<12} {:<8} {:<9} {:<8} {:>3}  {}",
            "Date", "Source", "Direction", "Target", "Mag", "Event"
        );
        println!("{header}");
        println!("{}", "─".repeat(100));

        for entry in &entries {
            let target_str = entry
                .target_complex
                .as_deref()
                .unwrap_or("—");
            let event_display = if entry.event.len() > 50 {
                format!("{}...", &entry.event[..47])
            } else {
                entry.event.clone()
            };
            println!(
                "{:<12} {:<8} {:<9} {:<8} {:>3}  {}",
                entry.date,
                entry.source_complex,
                entry.direction,
                target_str,
                entry.magnitude,
                event_display
            );
        }

        println!();
        println!("{} entries", entries.len());
    }

    Ok(())
}

pub fn run_balance(
    backend: &BackendConnection,
    days: usize,
    json_output: bool,
) -> Result<()> {
    let balances = power_flows::compute_balance_backend(backend, days)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "period_days": days,
                "balances": balances,
            }))?
        );
    } else {
        println!("POWER BALANCE (last {} days)", days);
        println!("{}", "─".repeat(60));

        let has_data = balances.iter().any(|b| b.gaining_count > 0 || b.losing_count > 0);

        if !has_data {
            println!("No power flow data in the last {} days.", days);
            println!("Use `pftui analytics power-flow add` to log power shift events.");
            return Ok(());
        }

        for balance in &balances {
            let sign = if balance.net >= 0 { "+" } else { "" };
            println!(
                "{}:  {}{} ({} gaining, {} losing)",
                balance.complex,
                sign,
                balance.net,
                balance.gaining_count,
                balance.losing_count,
            );
        }
    }

    Ok(())
}
