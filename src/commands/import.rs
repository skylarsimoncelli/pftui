use anyhow::{bail, Result};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::config::{Config, PortfolioMode};
use crate::db::allocations::insert_allocation_backend;
use crate::db::backend::BackendConnection;
use crate::db::transactions::{insert_transaction_backend, list_transactions_backend};
use crate::db::watchlist::{add_to_watchlist_backend, list_watchlist_backend};
use crate::models::asset::AssetCategory;
use crate::models::transaction::NewTransaction;

/// Config section from a JSON snapshot.
#[derive(Debug, Deserialize)]
struct ConfigImport {
    #[allow(dead_code)]
    base_currency: Option<String>,
    #[allow(dead_code)]
    refresh_interval: Option<u64>,
    portfolio_mode: Option<PortfolioMode>,
    #[allow(dead_code)]
    theme: Option<String>,
}

/// Transaction from a JSON snapshot (matches export format).
#[derive(Debug, Deserialize)]
struct TransactionImport {
    symbol: String,
    category: AssetCategory,
    tx_type: crate::models::transaction::TxType,
    quantity: Decimal,
    price_per: Decimal,
    #[serde(default = "default_currency")]
    currency: String,
    date: String,
    notes: Option<String>,
    // id and created_at are ignored on import
}

fn default_currency() -> String {
    "USD".to_string()
}

/// Allocation from a JSON snapshot.
#[derive(Debug, Deserialize)]
struct AllocationImport {
    symbol: String,
    category: AssetCategory,
    allocation_pct: Decimal,
    // id and created_at are ignored on import
}

/// Watchlist entry from a JSON snapshot.
#[derive(Debug, Deserialize)]
struct WatchlistImport {
    symbol: String,
    category: String,
    // added_at ignored on import
}

/// Top-level JSON snapshot (matches export format).
#[derive(Debug, Deserialize)]
struct Snapshot {
    #[serde(default)]
    config: Option<ConfigImport>,
    #[serde(default)]
    transactions: Vec<TransactionImport>,
    #[serde(default)]
    allocations: Vec<AllocationImport>,
    #[serde(default)]
    watchlist: Vec<WatchlistImport>,
    // positions field is ignored (computed, not stored)
}

/// Import mode: replace wipes existing data, merge adds without deleting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Replace,
    Merge,
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    path: &str,
    mode: ImportMode,
) -> Result<()> {
    // Read and parse
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;

    let snapshot: Snapshot = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Invalid JSON snapshot: {}", e))?;

    // Validate
    validate_snapshot(&snapshot, config)?;

    // Report what we're about to do
    let tx_count = snapshot.transactions.len();
    let alloc_count = snapshot.allocations.len();
    let watch_count = snapshot.watchlist.len();

    if tx_count == 0 && alloc_count == 0 && watch_count == 0 {
        println!("Snapshot is empty — nothing to import.");
        return Ok(());
    }

    match mode {
        ImportMode::Replace => import_replace(backend, &snapshot)?,
        ImportMode::Merge => import_merge(backend, &snapshot)?,
    }

    let mode_label = match mode {
        ImportMode::Replace => "replaced",
        ImportMode::Merge => "merged",
    };

    println!("Import complete ({mode_label}):");
    if tx_count > 0 {
        println!("  Transactions: {tx_count}");
    }
    if alloc_count > 0 {
        println!("  Allocations: {alloc_count}");
    }
    if watch_count > 0 {
        println!("  Watchlist: {watch_count}");
    }

    Ok(())
}

/// Validate snapshot data before importing.
fn validate_snapshot(snapshot: &Snapshot, config: &Config) -> Result<()> {
    // Check for mode mismatch
    if let Some(ref snap_config) = snapshot.config {
        if let Some(snap_mode) = snap_config.portfolio_mode {
            if snap_mode != config.portfolio_mode {
                let snap_label = match snap_mode {
                    PortfolioMode::Full => "full",
                    PortfolioMode::Percentage => "percentage",
                };
                let curr_label = match config.portfolio_mode {
                    PortfolioMode::Full => "full",
                    PortfolioMode::Percentage => "percentage",
                };
                bail!(
                    "Portfolio mode mismatch: snapshot is {snap_label} but current config is {curr_label}.\n\
                     Run `pftui setup` to change your portfolio mode first."
                );
            }
        }
    }

    // Validate transactions
    for (i, tx) in snapshot.transactions.iter().enumerate() {
        if tx.symbol.is_empty() {
            bail!("Transaction #{} has empty symbol", i + 1);
        }
        if tx.quantity <= Decimal::ZERO {
            bail!(
                "Transaction #{} ({}) has non-positive quantity: {}",
                i + 1,
                tx.symbol,
                tx.quantity
            );
        }
        if tx.price_per < Decimal::ZERO {
            bail!(
                "Transaction #{} ({}) has negative price: {}",
                i + 1,
                tx.symbol,
                tx.price_per
            );
        }
        if tx.date.len() != 10 || tx.date.chars().filter(|c| *c == '-').count() != 2 {
            bail!(
                "Transaction #{} ({}) has invalid date format: {} (expected YYYY-MM-DD)",
                i + 1,
                tx.symbol,
                tx.date
            );
        }
    }

    // Validate allocations
    for (i, alloc) in snapshot.allocations.iter().enumerate() {
        if alloc.symbol.is_empty() {
            bail!("Allocation #{} has empty symbol", i + 1);
        }
        if alloc.allocation_pct < Decimal::ZERO || alloc.allocation_pct > Decimal::from(100) {
            bail!(
                "Allocation #{} ({}) has invalid percentage: {} (must be 0-100)",
                i + 1,
                alloc.symbol,
                alloc.allocation_pct
            );
        }
    }

    // Validate watchlist
    for (i, entry) in snapshot.watchlist.iter().enumerate() {
        if entry.symbol.is_empty() {
            bail!("Watchlist entry #{} has empty symbol", i + 1);
        }
    }

    Ok(())
}

/// Replace mode: wipe existing data, then insert everything from snapshot.
fn import_replace(backend: &BackendConnection, snapshot: &Snapshot) -> Result<()> {
    crate::db::query::dispatch(
        backend,
        |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch("DELETE FROM transactions")?;
            tx.execute_batch("DELETE FROM portfolio_allocations")?;
            tx.execute_batch("DELETE FROM watchlist")?;
            tx.commit()?;
            Ok(())
        },
        |pool| {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                sqlx::query("DELETE FROM transactions")
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM portfolio_allocations")
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM watchlist").execute(pool).await?;
                Ok::<(), sqlx::Error>(())
            })?;
            Ok(())
        },
    )?;

    // Insert transactions
    for t in &snapshot.transactions {
        insert_transaction_backend(
            backend,
            &NewTransaction {
                symbol: t.symbol.clone(),
                category: t.category,
                tx_type: t.tx_type,
                quantity: t.quantity,
                price_per: t.price_per,
                currency: t.currency.clone(),
                date: t.date.clone(),
                notes: t.notes.clone(),
            },
        )?;
    }

    // Insert allocations
    for a in &snapshot.allocations {
        insert_allocation_backend(backend, &a.symbol, a.category, a.allocation_pct)?;
    }

    // Insert watchlist
    for w in &snapshot.watchlist {
        let cat: AssetCategory = w.category.parse().unwrap_or(AssetCategory::Equity);
        add_to_watchlist_backend(backend, &w.symbol, cat)?;
    }
    Ok(())
}

/// Merge mode: add new entries without deleting existing data.
fn import_merge(backend: &BackendConnection, snapshot: &Snapshot) -> Result<()> {
    // For transactions, we add all (no unique constraint to conflict on).
    // To avoid exact duplicates, check if an identical transaction already exists.
    let existing_txs = list_transactions_backend(backend)?;

    for t in &snapshot.transactions {
        let is_dup = existing_txs.iter().any(|e| {
            e.symbol == t.symbol
                && e.tx_type == t.tx_type
                && e.quantity == t.quantity
                && e.price_per == t.price_per
                && e.currency == t.currency
                && e.date == t.date
        });

        if !is_dup {
            insert_transaction_backend(
                backend,
                &NewTransaction {
                    symbol: t.symbol.clone(),
                    category: t.category,
                    tx_type: t.tx_type,
                    quantity: t.quantity,
                    price_per: t.price_per,
                    currency: t.currency.clone(),
                    date: t.date.clone(),
                    notes: t.notes.clone(),
                },
            )?;
        }
    }

    // Allocations use upsert (ON CONFLICT), so insert_allocation handles merge naturally
    for a in &snapshot.allocations {
        insert_allocation_backend(backend, &a.symbol, a.category, a.allocation_pct)?;
    }

    // Watchlist uses upsert (ON CONFLICT), so add_to_watchlist handles merge naturally
    let existing_watch = list_watchlist_backend(backend)?;
    for w in &snapshot.watchlist {
        let already = existing_watch
            .iter()
            .any(|e| e.symbol.eq_ignore_ascii_case(&w.symbol));
        if !already {
            let cat: AssetCategory = w.category.parse().unwrap_or(AssetCategory::Equity);
            add_to_watchlist_backend(backend, &w.symbol, cat)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::allocations::{insert_allocation, list_allocations};
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;
    use crate::db::transactions::{
        insert_transaction, insert_transaction_backend, list_transactions,
        list_transactions_backend,
    };
    use crate::db::watchlist::{
        add_to_watchlist, add_to_watchlist_backend, list_watchlist, list_watchlist_backend,
    };
    use crate::models::transaction::{NewTransaction, TxType};
    use rust_decimal_macros::dec;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    fn make_snapshot_json(txs: &str, allocs: &str, watchlist: &str, mode: &str) -> String {
        format!(
            r#"{{
                "config": {{ "base_currency": "USD", "refresh_interval": 60, "portfolio_mode": "{mode}", "theme": "midnight" }},
                "transactions": [{txs}],
                "allocations": [{allocs}],
                "watchlist": [{watchlist}],
                "positions": []
            }}"#
        )
    }

    fn write_tmp_file(content: &str) -> (std::path::PathBuf, String) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir();
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("pftui_import_test_{}_{}.json", std::process::id(), id);
        let path = dir.join(&name);
        std::fs::write(&path, content).unwrap();
        let path_str = path.to_str().unwrap().to_string();
        (path, path_str)
    }

    fn clear_portfolio_tables(backend: &BackendConnection) {
        crate::db::query::dispatch(
            backend,
            |conn| {
                conn.execute_batch(
                    "DELETE FROM transactions;
                     DELETE FROM portfolio_allocations;
                     DELETE FROM watchlist;",
                )?;
                Ok(())
            },
            |pool| {
                let runtime = tokio::runtime::Runtime::new().expect("runtime");
                runtime
                    .block_on(async {
                        sqlx::query("DELETE FROM transactions")
                            .execute(pool)
                            .await?;
                        sqlx::query("DELETE FROM portfolio_allocations")
                            .execute(pool)
                            .await?;
                        sqlx::query("DELETE FROM watchlist").execute(pool).await?;
                        Ok::<(), sqlx::Error>(())
                    })
                    .expect("clear tables");
                Ok(())
            },
        )
        .expect("clear portfolio tables");
    }

    fn postgres_backend_from_env() -> Option<BackendConnection> {
        let url = std::env::var("PFTUI_TEST_POSTGRES_URL").ok()?;
        let cfg = Config {
            database_backend: crate::config::DatabaseBackend::Postgres,
            database_url: Some(url),
            ..Config::default()
        };
        let db_path = crate::db::default_db_path();
        crate::db::backend::open_from_config(&cfg, &db_path).ok()
    }

    #[test]
    fn import_replace_transactions() {
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config::default();

        // Pre-existing transaction
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "OLD".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(1),
                price_per: dec!(100),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let json = make_snapshot_json(
            r#"{"symbol":"AAPL","category":"equity","tx_type":"buy","quantity":"10","price_per":"150","currency":"USD","date":"2025-06-01","notes":null}"#,
            "",
            "",
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        run(&backend, &config, &path_str, ImportMode::Replace).unwrap();

        let txs = list_transactions(conn).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].symbol, "AAPL");
        assert_eq!(txs[0].quantity, dec!(10));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_replace_allocations() {
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Config::default()
        };

        // Pre-existing allocation
        insert_allocation(conn, "OLD", AssetCategory::Equity, dec!(50)).unwrap();

        let json = make_snapshot_json(
            "",
            r#"{"symbol":"BTC","category":"crypto","allocation_pct":"25"}"#,
            "",
            "percentage",
        );
        let (_path, path_str) = write_tmp_file(&json);

        run(&backend, &config, &path_str, ImportMode::Replace).unwrap();

        let allocs = list_allocations(conn).unwrap();
        assert_eq!(allocs.len(), 1);
        assert_eq!(allocs[0].symbol, "BTC");
        assert_eq!(allocs[0].allocation_pct, dec!(25));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_replace_watchlist() {
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config::default();

        add_to_watchlist(conn, "OLD", AssetCategory::Equity).unwrap();

        let json = make_snapshot_json(
            "",
            "",
            r#"{"symbol":"ETH","category":"crypto","added_at":"2026-01-01"}"#,
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        run(&backend, &config, &path_str, ImportMode::Replace).unwrap();

        let entries = list_watchlist(conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].symbol, "ETH");

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_merge_adds_new_transactions() {
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config::default();

        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(5),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let json = make_snapshot_json(
            r#"{"symbol":"GOOG","category":"equity","tx_type":"buy","quantity":"3","price_per":"100","currency":"USD","date":"2025-02-01","notes":null}"#,
            "",
            "",
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        run(&backend, &config, &path_str, ImportMode::Merge).unwrap();

        let txs = list_transactions(conn).unwrap();
        assert_eq!(txs.len(), 2);
        let symbols: Vec<&str> = txs.iter().map(|t| t.symbol.as_str()).collect();
        assert!(symbols.contains(&"AAPL"));
        assert!(symbols.contains(&"GOOG"));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_merge_skips_duplicate_transactions() {
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config::default();

        insert_transaction(
            conn,
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

        // Import same transaction — should be skipped
        let json = make_snapshot_json(
            r#"{"symbol":"AAPL","category":"equity","tx_type":"buy","quantity":"10","price_per":"150","currency":"USD","date":"2025-06-01","notes":null}"#,
            "",
            "",
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        run(&backend, &config, &path_str, ImportMode::Merge).unwrap();

        let txs = list_transactions(conn).unwrap();
        assert_eq!(txs.len(), 1);

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_merge_watchlist_no_duplicates() {
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config::default();

        add_to_watchlist(conn, "BTC", AssetCategory::Crypto).unwrap();

        let json = make_snapshot_json(
            "",
            "",
            r#"{"symbol":"BTC","category":"crypto","added_at":"2026-01-01"},{"symbol":"ETH","category":"crypto","added_at":"2026-01-01"}"#,
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        run(&backend, &config, &path_str, ImportMode::Merge).unwrap();

        let entries = list_watchlist(conn).unwrap();
        assert_eq!(entries.len(), 2); // BTC kept, ETH added

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_rejects_mode_mismatch() {
        let backend = to_backend(open_in_memory());
        let config = Config::default(); // Full mode

        let json = make_snapshot_json("", "", "", "percentage");
        let (_path, path_str) = write_tmp_file(&json);

        let result = run(&backend, &config, &path_str, ImportMode::Replace);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("mode mismatch"));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_rejects_empty_symbol() {
        let backend = to_backend(open_in_memory());
        let config = Config::default();

        let json = make_snapshot_json(
            r#"{"symbol":"","category":"equity","tx_type":"buy","quantity":"10","price_per":"150","currency":"USD","date":"2025-06-01","notes":null}"#,
            "",
            "",
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        let result = run(&backend, &config, &path_str, ImportMode::Replace);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty symbol"));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_rejects_negative_quantity() {
        let backend = to_backend(open_in_memory());
        let config = Config::default();

        let json = make_snapshot_json(
            r#"{"symbol":"AAPL","category":"equity","tx_type":"buy","quantity":"-5","price_per":"150","currency":"USD","date":"2025-06-01","notes":null}"#,
            "",
            "",
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        let result = run(&backend, &config, &path_str, ImportMode::Replace);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("non-positive quantity"));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_rejects_invalid_date() {
        let backend = to_backend(open_in_memory());
        let config = Config::default();

        let json = make_snapshot_json(
            r#"{"symbol":"AAPL","category":"equity","tx_type":"buy","quantity":"10","price_per":"150","currency":"USD","date":"June 1","notes":null}"#,
            "",
            "",
            "full",
        );
        let (_path, path_str) = write_tmp_file(&json);

        let result = run(&backend, &config, &path_str, ImportMode::Replace);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid date"));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_rejects_invalid_allocation_pct() {
        let backend = to_backend(open_in_memory());
        let config = Config {
            portfolio_mode: PortfolioMode::Percentage,
            ..Config::default()
        };

        let json = make_snapshot_json(
            "",
            r#"{"symbol":"BTC","category":"crypto","allocation_pct":"150"}"#,
            "",
            "percentage",
        );
        let (_path, path_str) = write_tmp_file(&json);

        let result = run(&backend, &config, &path_str, ImportMode::Replace);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid percentage"));

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_empty_snapshot() {
        let backend = to_backend(open_in_memory());
        let config = Config::default();

        let json = make_snapshot_json("", "", "", "full");
        let (_path, path_str) = write_tmp_file(&json);

        // Should succeed with "nothing to import" message
        run(&backend, &config, &path_str, ImportMode::Replace).unwrap();

        std::fs::remove_file(&_path).ok();
    }

    #[test]
    fn import_invalid_json() {
        let backend = to_backend(open_in_memory());
        let config = Config::default();

        let dir = std::env::temp_dir();
        let path = dir.join("pftui_import_bad.json");
        std::fs::write(&path, "not valid json {{{").unwrap();

        let result = run(
            &backend,
            &config,
            path.to_str().unwrap(),
            ImportMode::Replace,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSON"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn import_file_not_found() {
        let backend = to_backend(open_in_memory());
        let config = Config::default();

        let result = run(
            &backend,
            &config,
            "/tmp/nonexistent_pftui_file.json",
            ImportMode::Replace,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn import_replace_full_roundtrip() {
        // Export → import replace should produce same data
        let backend = to_backend(open_in_memory());
        let conn = backend.sqlite();
        let config = Config::default();

        // Add data
        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "SPY".to_string(),
                category: AssetCategory::Fund,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(420),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: Some("initial".to_string()),
            },
        )
        .unwrap();

        insert_transaction(
            conn,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(0.5),
                price_per: dec!(28000),
                currency: "USD".to_string(),
                date: "2025-02-01".to_string(),
                notes: None,
            },
        )
        .unwrap();

        add_to_watchlist(conn, "ETH", AssetCategory::Crypto).unwrap();
        add_to_watchlist(conn, "GLD", AssetCategory::Commodity).unwrap();

        // Export
        let dir = std::env::temp_dir();
        let export_path = dir.join("pftui_roundtrip.json");
        crate::commands::export::run(
            &backend,
            &crate::cli::ExportFormat::Json,
            &config,
            Some(export_path.to_str().unwrap()),
        )
        .unwrap();

        // Import into fresh DB
        let backend2 = to_backend(open_in_memory());
        run(
            &backend2,
            &config,
            export_path.to_str().unwrap(),
            ImportMode::Replace,
        )
        .unwrap();
        let conn2 = backend2.sqlite();

        // Verify
        let txs = list_transactions(conn2).unwrap();
        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].symbol, "SPY");
        assert_eq!(txs[0].quantity, dec!(10));
        assert_eq!(txs[1].symbol, "BTC");
        assert_eq!(txs[1].quantity, dec!(0.5));

        let entries = list_watchlist(conn2).unwrap();
        assert_eq!(entries.len(), 2);

        std::fs::remove_file(&export_path).ok();
    }

    #[test]
    fn sqlite_export_import_into_postgres_when_env_set() {
        let Some(pg_backend) = postgres_backend_from_env() else {
            return;
        };
        clear_portfolio_tables(&pg_backend);

        let sqlite_backend = to_backend(open_in_memory());
        let sqlite_conn = sqlite_backend.sqlite();
        let config = Config::default();

        insert_transaction(
            sqlite_conn,
            &NewTransaction {
                symbol: "SPY".to_string(),
                category: AssetCategory::Fund,
                tx_type: TxType::Buy,
                quantity: dec!(12),
                price_per: dec!(430),
                currency: "USD".to_string(),
                date: "2025-01-01".to_string(),
                notes: Some("from sqlite".to_string()),
            },
        )
        .unwrap();
        add_to_watchlist(sqlite_conn, "ETH", AssetCategory::Crypto).unwrap();

        let export_path = std::env::temp_dir().join("pftui_sqlite_to_postgres_roundtrip.json");
        crate::commands::export::run(
            &sqlite_backend,
            &crate::cli::ExportFormat::Json,
            &config,
            Some(export_path.to_str().unwrap()),
        )
        .unwrap();

        run(
            &pg_backend,
            &config,
            export_path.to_str().unwrap(),
            ImportMode::Replace,
        )
        .unwrap();

        let txs = list_transactions_backend(&pg_backend).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].symbol, "SPY");
        assert_eq!(txs[0].quantity, dec!(12));

        let watch = list_watchlist_backend(&pg_backend).unwrap();
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].symbol, "ETH");

        std::fs::remove_file(&export_path).ok();
        clear_portfolio_tables(&pg_backend);
    }

    #[test]
    fn postgres_export_import_into_sqlite_when_env_set() {
        let Some(pg_backend) = postgres_backend_from_env() else {
            return;
        };
        clear_portfolio_tables(&pg_backend);

        let config = Config::default();
        insert_transaction_backend(
            &pg_backend,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(0.25),
                price_per: dec!(50000),
                currency: "USD".to_string(),
                date: "2025-03-01".to_string(),
                notes: Some("from postgres".to_string()),
            },
        )
        .unwrap();
        add_to_watchlist_backend(&pg_backend, "AAPL", AssetCategory::Equity).unwrap();

        let export_path = std::env::temp_dir().join("pftui_postgres_to_sqlite_roundtrip.json");
        crate::commands::export::run(
            &pg_backend,
            &crate::cli::ExportFormat::Json,
            &config,
            Some(export_path.to_str().unwrap()),
        )
        .unwrap();

        let sqlite_backend = to_backend(open_in_memory());
        run(
            &sqlite_backend,
            &config,
            export_path.to_str().unwrap(),
            ImportMode::Replace,
        )
        .unwrap();
        let sqlite_conn = sqlite_backend.sqlite();

        let txs = list_transactions(sqlite_conn).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].symbol, "BTC");
        assert_eq!(txs[0].quantity, dec!(0.25));

        let watch = list_watchlist(sqlite_conn).unwrap();
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].symbol, "AAPL");

        std::fs::remove_file(&export_path).ok();
        clear_portfolio_tables(&pg_backend);
    }
}
