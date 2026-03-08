#[allow(dead_code)] // Infrastructure for F6 alert engine — consumed by F6.2+ (CLI, TUI, refresh)
pub mod alerts;
pub mod annotations;
pub mod allocation_targets;
pub mod allocations;
#[allow(dead_code)] // Infrastructure for F24.1+ consumers (BLS indicators, Economy tab)
pub mod bls_cache;
#[allow(dead_code)] // Infrastructure for F12.1+ consumers (calendar CLI, Economy tab)
pub mod calendar_cache;
pub mod chart_state;
#[allow(dead_code)] // Infrastructure for F22.1+ consumers (COMEX supply panel, CLI)
pub mod comex_cache;
#[allow(dead_code)] // Infrastructure for F18.1+ consumers (COT section, CLI)
pub mod cot_cache;
#[allow(dead_code)] // Infrastructure for F3.2+ consumers (macro dashboard, refresh)
pub mod economic_cache;
#[allow(dead_code)] // Infrastructure for F28.1+ consumers (Brave economy fetcher)
pub mod economic_data;
pub mod fx_cache;
pub mod journal;
#[allow(dead_code)] // Infrastructure for F20.1+ consumers (News tab, CLI)
pub mod news_cache;
#[allow(dead_code)] // Infrastructure for F21.1+ consumers (on-chain panel, CLI)
pub mod onchain_cache;
#[allow(dead_code)] // Infrastructure for F17.1+ consumers (Predictions panel, CLI)
pub mod prediction_cache;
pub mod predictions_cache;
pub mod predictions_history;
pub mod price_cache;
#[allow(dead_code)] // Infrastructure for F19.1+ consumers (sentiment gauges, CLI)
pub mod sentiment_cache;
pub mod price_history;
pub mod schema;
pub mod snapshots;
pub mod transactions;
pub mod watchlist;
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

pub fn default_db_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("pftui")
        .join("pftui.db")
}

#[cfg(test)]
pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    conn
}
