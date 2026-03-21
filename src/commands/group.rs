use std::collections::HashMap;

use anyhow::{bail, Result};
use rust_decimal::Decimal;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::groups;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_prices_at_date_backend;
use crate::db::transactions::list_transactions_backend;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    action: &str,
    name: Option<&str>,
    symbols: Option<&str>,
    json: bool,
) -> Result<()> {
    match action {
        "create" => run_create(backend, name, symbols),
        "list" => run_list(backend, json),
        "show" => run_show(backend, config, name, json),
        "remove" => run_remove(backend, name),
        _ => bail!(
            "Unknown action '{}'. Use: create, list, show, remove",
            action
        ),
    }
}

fn run_create(
    backend: &BackendConnection,
    name: Option<&str>,
    symbols: Option<&str>,
) -> Result<()> {
    let group_name = normalize_name(name)?;
    let members = parse_symbols(symbols)?;
    groups::create_group_backend(backend, &group_name)?;
    groups::set_group_members_backend(backend, &group_name, &members)?;
    println!(
        "Saved group '{}' with {} symbols.",
        group_name,
        members.len()
    );
    Ok(())
}

fn run_list(backend: &BackendConnection, json: bool) -> Result<()> {
    let rows = groups::list_groups_backend(backend)?;
    if rows.is_empty() {
        println!("No groups defined. Create one with: pftui group create <name> --symbols A,B,C");
        return Ok(());
    }

    if json {
        let out: Vec<_> = rows
            .iter()
            .map(|g| {
                let members =
                    groups::get_group_members_backend(backend, &g.name).unwrap_or_default();
                serde_json::json!({
                    "name": g.name,
                    "created_at": g.created_at,
                    "symbols": members,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    for g in rows {
        let members = groups::get_group_members_backend(backend, &g.name)?;
        println!("{}: {}", g.name, members.join(", "));
    }
    Ok(())
}

fn run_show(
    backend: &BackendConnection,
    config: &Config,
    name: Option<&str>,
    json: bool,
) -> Result<()> {
    let group_name = normalize_name(name)?;
    let members = groups::get_group_members_backend(backend, &group_name)?;
    if members.is_empty() {
        bail!("Group '{}' has no symbols or does not exist.", group_name);
    }

    let positions = load_positions(backend, config)?;
    let group_positions: Vec<&Position> = positions
        .iter()
        .filter(|p| members.iter().any(|m| m == &p.symbol))
        .collect();

    if group_positions.is_empty() {
        bail!(
            "Group '{}' has no matching held symbols in the current portfolio.",
            group_name
        );
    }

    let total_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    let group_value: Decimal = group_positions.iter().filter_map(|p| p.current_value).sum();
    let total_cost: Decimal = group_positions.iter().map(|p| p.total_cost).sum();
    let gain = group_value - total_cost;
    let gain_pct = if total_cost > Decimal::ZERO {
        (gain / total_cost) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    let yesterday = (chrono::Utc::now().date_naive() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let hist = get_prices_at_date_backend(backend, &members, &yesterday).unwrap_or_default();
    let mut daily_pnl = Decimal::ZERO;
    for p in &group_positions {
        if let (Some(curr), Some(prev)) = (p.current_price, hist.get(&p.symbol)) {
            daily_pnl += (curr - *prev) * p.quantity;
        }
    }

    let group_alloc = if total_value > Decimal::ZERO {
        (group_value / total_value) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    if json {
        let out = serde_json::json!({
            "group": group_name,
            "members": members,
            "group_value": group_value.to_string(),
            "group_allocation_pct": group_alloc.round_dp(2).to_string(),
            "group_gain": gain.to_string(),
            "group_gain_pct": gain_pct.round_dp(2).to_string(),
            "group_daily_pnl": daily_pnl.to_string(),
            "positions": group_positions.iter().map(|p| serde_json::json!({
                "symbol": p.symbol,
                "category": format!("{}", p.category),
                "allocation_pct": p.allocation_pct.map(|v| v.round_dp(2).to_string()),
                "current_value": p.current_value.map(|v| v.to_string()),
                "gain_pct": p.gain_pct.map(|v| v.round_dp(2).to_string()),
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Group: {}", group_name);
    println!("Members: {}", members.join(", "));
    println!("Allocation: {:.2}%", group_alloc);
    println!("Value: {}", group_value.round_dp(2));
    println!("P&L: {:+.2} ({:+.2}%)", gain, gain_pct);
    println!("1D P&L: {:+.2}", daily_pnl.round_dp(2));
    println!();
    println!(
        "{:<12} {:>10} {:>10} {:>10}",
        "Symbol", "Alloc%", "Value", "Gain%"
    );
    println!("{}", "─".repeat(48));
    for p in &group_positions {
        println!(
            "{:<12} {:>10} {:>10} {:>10}",
            p.symbol,
            p.allocation_pct
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "—".to_string()),
            p.current_value
                .map(|v| format!("{:.0}", v))
                .unwrap_or_else(|| "—".to_string()),
            p.gain_pct
                .map(|v| format!("{:+.1}", v))
                .unwrap_or_else(|| "—".to_string())
        );
    }

    Ok(())
}

fn run_remove(backend: &BackendConnection, name: Option<&str>) -> Result<()> {
    let group_name = normalize_name(name)?;
    if groups::remove_group_backend(backend, &group_name)? {
        println!("Removed group '{}'.", group_name);
    } else {
        println!("Group '{}' not found.", group_name);
    }
    Ok(())
}

fn parse_symbols(symbols: Option<&str>) -> Result<Vec<String>> {
    let symbols = symbols.ok_or_else(|| anyhow::anyhow!("--symbols is required"))?;
    let members: Vec<String> = symbols
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    if members.is_empty() {
        bail!("--symbols must include at least one symbol");
    }
    Ok(members)
}

fn normalize_name(name: Option<&str>) -> Result<String> {
    let name = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Group name is required"))?;
    Ok(name.to_lowercase())
}

fn load_positions(backend: &BackendConnection, config: &Config) -> Result<Vec<Position>> {
    let prices: HashMap<String, Decimal> = get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|q| (q.symbol, q.price))
        .collect();
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();

    let positions = match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions_backend(backend)?;
            compute_positions(&txs, &prices, &fx_rates)
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations_backend(backend)?;
            compute_positions_from_allocations(&allocs, &prices, &fx_rates)
        }
    };
    Ok(positions)
}
