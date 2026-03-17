use std::collections::HashMap;
use std::io::Write;

use anyhow::Result;
#[cfg(test)]
use rusqlite::Connection;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::cli::ExportFormat;
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::transactions::list_transactions_backend;
use crate::db::watchlist::list_watchlist_backend;
use crate::models::allocation::Allocation;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};
use crate::models::transaction::Transaction;

/// Round a Decimal to 2 decimal places for human-readable CSV output.
fn round2(d: Decimal) -> String {
    d.round_dp(2).to_string()
}

/// Serializable watchlist entry for JSON export.
#[derive(Serialize)]
struct WatchlistExport {
    symbol: String,
    category: String,
    added_at: String,
}

/// Full portfolio snapshot for JSON export.
#[derive(Serialize)]
struct FullSnapshot {
    config: ConfigExport,
    transactions: Vec<Transaction>,
    allocations: Vec<Allocation>,
    watchlist: Vec<WatchlistExport>,
    positions: Vec<Position>,
}

/// Config subset for export (avoids leaking internal fields).
#[derive(Serialize)]
struct ConfigExport {
    base_currency: String,
    refresh_interval: u64,
    auto_refresh: bool,
    refresh_interval_secs: u64,
    portfolio_mode: PortfolioMode,
    theme: String,
}

impl From<&Config> for ConfigExport {
    fn from(c: &Config) -> Self {
        ConfigExport {
            base_currency: c.base_currency.clone(),
            refresh_interval: c.refresh_interval,
            auto_refresh: c.auto_refresh,
            refresh_interval_secs: c.refresh_interval_secs,
            portfolio_mode: c.portfolio_mode,
            theme: c.theme.clone(),
        }
    }
}

/// Get a writer: either a file or stdout.
fn get_writer(output: Option<&str>) -> Result<Box<dyn Write>> {
    match output {
        Some(path) => {
            let file = std::fs::File::create(path)?;
            Ok(Box::new(std::io::BufWriter::new(file)))
        }
        None => Ok(Box::new(std::io::stdout().lock())),
    }
}

pub fn run(
    backend: &BackendConnection,
    format: &ExportFormat,
    config: &Config,
    output: Option<&str>,
) -> Result<()> {
    let cached = get_all_cached_prices_backend(backend)?;
    let prices: HashMap<String, Decimal> =
        cached.into_iter().map(|q| (q.symbol, q.price)).collect();

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

    match format {
        ExportFormat::Json => export_json_snapshot(backend, config, &positions, output),
        ExportFormat::Csv => export_csv_positions(config, &positions, output),
    }
}

/// JSON export: full database snapshot with config, transactions, allocations, watchlist, and positions.
fn export_json_snapshot(
    backend: &BackendConnection,
    config: &Config,
    positions: &[Position],
    output: Option<&str>,
) -> Result<()> {
    let transactions = list_transactions_backend(backend).unwrap_or_default();
    let allocations = list_allocations_backend(backend).unwrap_or_default();
    let watchlist_entries = list_watchlist_backend(backend).unwrap_or_default();

    let watchlist: Vec<WatchlistExport> = watchlist_entries
        .into_iter()
        .map(|e| WatchlistExport {
            symbol: e.symbol,
            category: e.category,
            added_at: e.added_at,
        })
        .collect();

    let snapshot = FullSnapshot {
        config: ConfigExport::from(config),
        transactions,
        allocations,
        watchlist,
        positions: positions.to_vec(),
    };

    let json = serde_json::to_string_pretty(&snapshot)?;
    let mut writer = get_writer(output)?;
    writeln!(writer, "{json}")?;

    if let Some(path) = output {
        eprintln!("Exported full snapshot to {path}");
    }

    Ok(())
}

/// CSV export: positions table only (CSV can't represent multiple tables cleanly).
fn export_csv_positions(
    config: &Config,
    positions: &[Position],
    output: Option<&str>,
) -> Result<()> {
    let writer = get_writer(output)?;

    match config.portfolio_mode {
        PortfolioMode::Full => {
            let mut wtr = csv::Writer::from_writer(writer);
            wtr.write_record([
                "symbol",
                "name",
                "category",
                "quantity",
                "avg_cost",
                "total_cost",
                "currency",
                "current_price",
                "current_value",
                "gain",
                "gain_pct",
                "allocation_pct",
            ])?;
            for pos in positions {
                wtr.write_record([
                    &pos.symbol,
                    &pos.name,
                    &pos.category.to_string(),
                    &pos.quantity.to_string(),
                    &pos.avg_cost.round_dp(2).to_string(),
                    &pos.total_cost.round_dp(2).to_string(),
                    &pos.currency,
                    &pos.current_price.map(round2).unwrap_or_default(),
                    &pos.current_value.map(round2).unwrap_or_default(),
                    &pos.gain.map(round2).unwrap_or_default(),
                    &pos.gain_pct.map(round2).unwrap_or_default(),
                    &pos.allocation_pct.map(round2).unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;
        }
        PortfolioMode::Percentage => {
            let mut wtr = csv::Writer::from_writer(writer);
            wtr.write_record([
                "symbol",
                "name",
                "category",
                "current_price",
                "allocation_pct",
            ])?;
            for pos in positions {
                wtr.write_record([
                    &pos.symbol,
                    &pos.name,
                    &pos.category.to_string(),
                    &pos.current_price.map(round2).unwrap_or_default(),
                    &pos.allocation_pct.map(round2).unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;
        }
    }

    if let Some(path) = output {
        eprintln!("Exported positions to {path}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn to_backend(conn: Connection) -> crate::db::backend::BackendConnection {
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn round2_basic() {
        assert_eq!(round2(dec!(33.333333333333333333333333333)), "33.33");
    }

    #[test]
    fn round2_rounds_up() {
        assert_eq!(round2(dec!(49.999)), "50.00");
    }

    #[test]
    fn round2_whole_number() {
        assert_eq!(round2(dec!(100)), "100");
    }

    #[test]
    fn round2_small() {
        assert_eq!(round2(dec!(0.006)), "0.01");
    }

    #[test]
    fn round2_negative() {
        assert_eq!(round2(dec!(-12.3456)), "-12.35");
    }

    #[test]
    fn config_export_from_config() {
        let config = Config {
            database_backend: crate::config::DatabaseBackend::Sqlite,
            database_url: None,
            mirror_source_url: None,
            postgres_read_only: false,
            postgres_max_connections: 5,
            postgres_connect_timeout_secs: 10,
            base_currency: "EUR".to_string(),
            refresh_interval: 30,
            auto_refresh: true,
            refresh_interval_secs: 300,
            portfolio_mode: PortfolioMode::Percentage,
            theme: "nord".to_string(),
            home_tab: "positions".to_string(),
            layout: crate::config::WorkspaceLayout::Split,
            fred_api_key: None,
            brave_api_key: None,
            news_poll_interval: 600,
            custom_news_feeds: Vec::new(),
            brave_news_queries: Vec::new(),
            chart_sma: vec![20, 50],
            watchlist: crate::config::WatchlistConfig::default(),
            keybindings: crate::config::KeybindingsConfig::default(),
            mobile: crate::config::MobileServerConfig::default(),
        };
        let export = ConfigExport::from(&config);
        assert_eq!(export.base_currency, "EUR");
        assert_eq!(export.refresh_interval, 30);
        assert_eq!(export.theme, "nord");
    }

    #[test]
    fn json_snapshot_serializes() {
        let snapshot = FullSnapshot {
            config: ConfigExport {
                base_currency: "USD".to_string(),
                refresh_interval: 60,
                auto_refresh: true,
                refresh_interval_secs: 300,
                portfolio_mode: PortfolioMode::Full,
                theme: "midnight".to_string(),
            },
            transactions: vec![],
            allocations: vec![],
            watchlist: vec![],
            positions: vec![],
        };
        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        assert!(json.contains("\"config\""));
        assert!(json.contains("\"transactions\""));
        assert!(json.contains("\"allocations\""));
        assert!(json.contains("\"watchlist\""));
        assert!(json.contains("\"positions\""));
    }

    #[test]
    fn json_snapshot_with_data() {
        let snapshot = FullSnapshot {
            config: ConfigExport {
                base_currency: "USD".to_string(),
                refresh_interval: 60,
                auto_refresh: true,
                refresh_interval_secs: 300,
                portfolio_mode: PortfolioMode::Full,
                theme: "midnight".to_string(),
            },
            transactions: vec![],
            allocations: vec![],
            watchlist: vec![WatchlistExport {
                symbol: "AAPL".to_string(),
                category: "equity".to_string(),
                added_at: "2026-01-01 00:00:00".to_string(),
            }],
            positions: vec![],
        };
        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        assert!(json.contains("AAPL"));
        assert!(json.contains("equity"));
    }

    #[test]
    fn csv_export_full_mode() {
        use crate::models::asset::AssetCategory;
        let positions = vec![Position {
            symbol: "AAPL".to_string(),
            name: "Apple Inc.".to_string(),
            category: AssetCategory::Equity,
            quantity: dec!(10),
            avg_cost: dec!(150.50),
            total_cost: dec!(1505),
            currency: "USD".to_string(),
            current_price: Some(dec!(175.25)),
            current_value: Some(dec!(1752.50)),
            gain: Some(dec!(247.50)),
            gain_pct: Some(dec!(16.44)),
            allocation_pct: Some(dec!(100)),
            native_currency: None,
            fx_rate: None,
        }];
        let mut buf = Vec::new();
        {
            let mut wtr = csv::Writer::from_writer(&mut buf);
            wtr.write_record([
                "symbol",
                "name",
                "category",
                "quantity",
                "avg_cost",
                "total_cost",
                "currency",
                "current_price",
                "current_value",
                "gain",
                "gain_pct",
                "allocation_pct",
            ])
            .unwrap();
            for pos in &positions {
                wtr.write_record([
                    &pos.symbol,
                    &pos.name,
                    &pos.category.to_string(),
                    &pos.quantity.to_string(),
                    &pos.avg_cost.round_dp(2).to_string(),
                    &pos.total_cost.round_dp(2).to_string(),
                    &pos.currency,
                    &pos.current_price.map(round2).unwrap_or_default(),
                    &pos.current_value.map(round2).unwrap_or_default(),
                    &pos.gain.map(round2).unwrap_or_default(),
                    &pos.gain_pct.map(round2).unwrap_or_default(),
                    &pos.allocation_pct.map(round2).unwrap_or_default(),
                ])
                .unwrap();
            }
            wtr.flush().unwrap();
        }
        let csv_str = String::from_utf8(buf).unwrap();
        assert!(csv_str.contains("symbol,name,category,quantity"));
        assert!(csv_str.contains("AAPL,Apple Inc.,equity,10"));
    }

    #[test]
    fn get_writer_stdout() {
        // Smoke test: None returns stdout (won't panic)
        let writer = get_writer(None);
        assert!(writer.is_ok());
    }

    #[test]
    fn export_json_full_db_snapshot() {
        use crate::db::open_in_memory;
        use crate::db::transactions::insert_transaction;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::asset::AssetCategory;
        use crate::models::transaction::{NewTransaction, TxType};

        let conn = open_in_memory();
        let config = Config::default();

        // Add a transaction
        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-06-01".to_string(),
                notes: None,
            },
        )
        .unwrap();

        // Add a watchlist entry
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();

        // Export to a temp file
        let dir = std::env::temp_dir();
        let path = dir.join("pftui_test_export.json");
        let path_str = path.to_str().unwrap();

        let backend = to_backend(conn);
        run(&backend, &ExportFormat::Json, &config, Some(path_str)).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"transactions\""));
        assert!(content.contains("AAPL"));
        assert!(content.contains("\"watchlist\""));
        assert!(content.contains("BTC"));
        assert!(content.contains("\"config\""));
        assert!(content.contains("\"positions\""));

        // Parse back to validate JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["config"]["base_currency"].as_str().unwrap() == "USD");
        assert!(parsed["transactions"].as_array().unwrap().len() == 1);
        assert!(parsed["watchlist"].as_array().unwrap().len() == 1);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn export_csv_to_file() {
        use crate::db::open_in_memory;
        use crate::db::transactions::insert_transaction;
        use crate::models::asset::AssetCategory;
        use crate::models::transaction::{NewTransaction, TxType};

        let conn = open_in_memory();
        let config = Config::default();

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "SPY".to_string(),
                category: AssetCategory::Fund,
                tx_type: TxType::Buy,
                quantity: dec!(5),
                price_per: dec!(420),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let dir = std::env::temp_dir();
        let path = dir.join("pftui_test_export.csv");
        let path_str = path.to_str().unwrap();

        let backend = to_backend(conn);
        run(&backend, &ExportFormat::Csv, &config, Some(path_str)).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("symbol,name,category,quantity"));
        assert!(content.contains("SPY,"));
        assert!(content.contains(",fund,5"));

        std::fs::remove_file(&path).ok();
    }
}
