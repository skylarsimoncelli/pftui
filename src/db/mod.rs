pub mod agent_messages;
#[allow(dead_code)] // Infrastructure for F6 alert engine — consumed by F6.2+ (CLI, TUI, refresh)
pub mod alerts;
pub mod allocation_targets;
pub mod allocations;
pub mod annotations;
pub mod backend;
#[allow(dead_code)] // Infrastructure for F24.1+ consumers (BLS indicators, Economy tab)
pub mod bls_cache;
pub mod broker_connections;
#[allow(dead_code)] // Infrastructure for F12.1+ consumers (calendar CLI, Economy tab)
pub mod calendar_cache;
pub mod chart_state;
#[allow(dead_code)] // Infrastructure for F22.1+ consumers (COMEX supply panel, CLI)
pub mod comex_cache;
pub mod consensus;
#[allow(dead_code)] // Infrastructure for F31.3 (conviction CLI)
pub mod convictions;
pub mod correlation_snapshots;
#[allow(dead_code)] // Infrastructure for F18.1+ consumers (COT section, CLI)
pub mod cot_cache;
pub mod daily_notes;
pub mod dividends;
#[allow(dead_code)] // Infrastructure for F3.2+ consumers (macro dashboard, refresh)
pub mod economic_cache;
#[allow(dead_code)] // Infrastructure for F28.1+ consumers (Brave economy fetcher)
pub mod economic_data;
pub mod fedwatch_cache;
pub mod fx_cache;
pub mod groups;
pub mod journal;
pub mod macro_events;
pub mod mobile_timeframe_scores;
#[allow(dead_code)] // Infrastructure for F20.1+ consumers (News tab, CLI)
pub mod news_cache;
#[allow(dead_code)] // Infrastructure for F21.1+ consumers (on-chain panel, CLI)
pub mod onchain_cache;
pub mod opportunity_cost;
pub mod pg_runtime;
pub mod postgres_schema;
#[allow(dead_code)] // Infrastructure for F17.1+ consumers (Predictions panel, CLI)
pub mod prediction_cache;
pub mod predictions_cache;
pub mod predictions_history;
pub mod price_cache;
pub mod price_history;
pub mod query;
pub mod regime_snapshots;
pub mod research_questions;
pub mod scan_queries;
pub mod scenarios;
pub mod schema;
#[allow(dead_code)] // Infrastructure for F19.1+ consumers (sentiment gauges, CLI)
pub mod sentiment_cache;
pub mod situation_snapshots;
pub mod snapshots;
pub mod structural;
pub mod technical_levels;
pub mod technical_signals;
pub mod technical_snapshots;
pub mod thesis;
pub mod timeframe_signals;
pub mod transactions;
pub mod trends;
pub mod triggered_alerts;
pub mod user_predictions;
pub mod watchlist;
pub mod watchlist_groups;
#[allow(dead_code)] // Infrastructure for F25.1+ consumers (Global macro panel, CLI)
pub mod worldbank_cache;

use anyhow::Result;
use rusqlite::Connection;

pub fn open_db(path: &std::path::Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    schema::run_migrations(&conn)?;
    Ok(conn)
}

fn base_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("pftui")
}

fn portfolios_dir() -> std::path::PathBuf {
    base_data_dir().join("portfolios")
}

fn active_portfolio_path() -> std::path::PathBuf {
    base_data_dir().join("active_portfolio")
}

pub fn sanitize_portfolio_name(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Some(trimmed.to_lowercase())
    } else {
        None
    }
}

pub fn read_active_portfolio() -> String {
    std::fs::read_to_string(active_portfolio_path())
        .ok()
        .and_then(|s| sanitize_portfolio_name(&s))
        .unwrap_or_else(|| "default".to_string())
}

pub fn write_active_portfolio(name: &str) -> Result<()> {
    let safe = sanitize_portfolio_name(name)
        .ok_or_else(|| anyhow::anyhow!("Invalid portfolio name: {}", name))?;
    std::fs::create_dir_all(base_data_dir())?;
    std::fs::write(active_portfolio_path(), format!("{}\n", safe))?;
    Ok(())
}

pub fn db_path_for_portfolio(name: &str) -> std::path::PathBuf {
    let safe = sanitize_portfolio_name(name).unwrap_or_else(|| "default".to_string());
    let legacy_default = base_data_dir().join("pftui.db");
    if safe == "default" && legacy_default.exists() {
        return legacy_default;
    }
    portfolios_dir().join(format!("{}.db", safe))
}

pub fn list_portfolios() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    names.insert("default".to_string());

    if let Ok(entries) = std::fs::read_dir(portfolios_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("db") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Some(safe) = sanitize_portfolio_name(stem) {
                        names.insert(safe);
                    }
                }
            }
        }
    }

    names.into_iter().collect()
}

pub fn default_db_path() -> std::path::PathBuf {
    let active = read_active_portfolio();
    db_path_for_portfolio(&active)
}

#[cfg(test)]
pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    conn
}
