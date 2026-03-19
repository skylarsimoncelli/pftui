use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::str::FromStr;

use crate::broker::{self, BrokerKind};
use crate::config::{load_config, save_config};
use crate::db::backend::BackendConnection;
use crate::db::broker_connections;
use crate::db::transactions::insert_transaction_backend;
use crate::models::asset::AssetCategory;
use crate::models::transaction::{NewTransaction, TxType};

pub fn run_add(
    backend: &BackendConnection,
    broker: BrokerKind,
    api_key: Option<&str>,
    secret: Option<&str>,
    label: Option<&str>,
    json_out: bool,
) -> Result<()> {
    // Save credentials to config
    let mut config = load_config()?;
    match broker {
        BrokerKind::Trading212 => {
            if let Some(key) = api_key {
                config.brokers.trading212_api_key = Some(key.to_string());
            } else if config.brokers.trading212_api_key.is_none() {
                anyhow::bail!("Trading212 requires --api-key");
            }
        }
        BrokerKind::Ibkr => {
            if let Some(key) = api_key {
                config.brokers.ibkr_account_id = Some(key.to_string());
            }
        }
        BrokerKind::Binance => {
            if let Some(key) = api_key {
                config.brokers.binance_api_key = Some(key.to_string());
            } else if config.brokers.binance_api_key.is_none() {
                anyhow::bail!("Binance requires --api-key");
            }
            if let Some(s) = secret {
                config.brokers.binance_secret_key = Some(s.to_string());
            } else if config.brokers.binance_secret_key.is_none() {
                anyhow::bail!("Binance requires --secret");
            }
        }
        BrokerKind::Kraken => {
            if let Some(key) = api_key {
                config.brokers.kraken_api_key = Some(key.to_string());
            } else if config.brokers.kraken_api_key.is_none() {
                anyhow::bail!("Kraken requires --api-key");
            }
            if let Some(s) = secret {
                config.brokers.kraken_private_key = Some(s.to_string());
            } else if config.brokers.kraken_private_key.is_none() {
                anyhow::bail!("Kraken requires --secret (private key)");
            }
        }
        BrokerKind::Coinbase => {
            if let Some(key) = api_key {
                config.brokers.coinbase_api_key = Some(key.to_string());
            } else if config.brokers.coinbase_api_key.is_none() {
                anyhow::bail!("Coinbase requires --api-key");
            }
            if let Some(s) = secret {
                config.brokers.coinbase_api_secret = Some(s.to_string());
            } else if config.brokers.coinbase_api_secret.is_none() {
                anyhow::bail!("Coinbase requires --secret");
            }
        }
        BrokerKind::CryptoCom => {
            if let Some(key) = api_key {
                config.brokers.crypto_com_api_key = Some(key.to_string());
            } else if config.brokers.crypto_com_api_key.is_none() {
                anyhow::bail!("Crypto.com requires --api-key");
            }
            if let Some(s) = secret {
                config.brokers.crypto_com_secret_key = Some(s.to_string());
            } else if config.brokers.crypto_com_secret_key.is_none() {
                anyhow::bail!("Crypto.com requires --secret");
            }
        }
    }
    save_config(&config)?;

    // Upsert DB record
    let account_id = match broker {
        BrokerKind::Ibkr => config.brokers.ibkr_account_id.as_deref(),
        _ => None,
    };
    let id = broker_connections::upsert_broker_connection_backend(
        backend,
        &broker.to_string(),
        account_id,
        label,
    )?;

    if json_out {
        println!(
            "{}",
            json!({
                "status": "ok",
                "broker": broker.to_string(),
                "id": id,
                "message": format!("Broker {} configured", broker),
            })
        );
    } else {
        println!("Broker {} configured (id: {})", broker, id);
        println!("Run `pftui portfolio broker sync {}` to import positions.", broker);
    }
    Ok(())
}

pub fn run_list(backend: &BackendConnection, json_out: bool) -> Result<()> {
    let connections = broker_connections::list_broker_connections_backend(backend)?;
    let config = load_config()?;

    if json_out {
        let items: Vec<_> = connections
            .iter()
            .map(|c| {
                json!({
                    "broker": c.broker_name,
                    "label": c.label,
                    "account_id": c.account_id,
                    "last_sync_at": c.last_sync_at,
                    "sync_status": c.sync_status,
                    "sync_error": c.sync_error,
                    "created_at": c.created_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if connections.is_empty() {
        println!("No brokers configured.");
        println!("Add one with: pftui portfolio broker add <BROKER> [--api-key KEY]");
        return Ok(());
    }

    println!("{:<14} {:<12} {:<12} {:<22} CREDENTIAL", "BROKER", "STATUS", "LABEL", "LAST SYNC");
    println!("{}", "-".repeat(76));
    for c in &connections {
        let cred_status = match c.broker_name.as_str() {
            "trading212" => {
                if config.brokers.trading212_api_key.is_some() { "configured" } else { "missing" }
            }
            "ibkr" => {
                if config.brokers.ibkr_account_id.is_some() { "configured" } else { "auto-detect" }
            }
            "binance" => {
                if config.brokers.binance_api_key.is_some() && config.brokers.binance_secret_key.is_some() { "configured" } else { "missing" }
            }
            "kraken" => {
                if config.brokers.kraken_api_key.is_some() && config.brokers.kraken_private_key.is_some() { "configured" } else { "missing" }
            }
            "coinbase" => {
                if config.brokers.coinbase_api_key.is_some() && config.brokers.coinbase_api_secret.is_some() { "configured" } else { "missing" }
            }
            "crypto-com" => {
                if config.brokers.crypto_com_api_key.is_some() && config.brokers.crypto_com_secret_key.is_some() { "configured" } else { "missing" }
            }
            _ => "n/a",
        };
        let label = c.label.as_deref().unwrap_or("-");
        let last_sync = c.last_sync_at.as_deref().unwrap_or("never");
        println!(
            "{:<14} {:<12} {:<12} {:<22} {}",
            c.broker_name, c.sync_status, label, last_sync, cred_status
        );
    }
    Ok(())
}

pub fn run_remove(backend: &BackendConnection, broker: BrokerKind, json_out: bool) -> Result<()> {
    let broker_name = broker.to_string();
    let tag = broker::broker_tag(broker);

    // Delete synced transactions
    let deleted_txs = broker_connections::delete_broker_transactions_backend(backend, &tag)?;

    // Delete DB record
    let deleted = broker_connections::delete_broker_connection_backend(backend, &broker_name)?;

    // Remove credentials from config
    let mut config = load_config()?;
    match broker {
        BrokerKind::Trading212 => config.brokers.trading212_api_key = None,
        BrokerKind::Ibkr => config.brokers.ibkr_account_id = None,
        BrokerKind::Binance => {
            config.brokers.binance_api_key = None;
            config.brokers.binance_secret_key = None;
        }
        BrokerKind::Kraken => {
            config.brokers.kraken_api_key = None;
            config.brokers.kraken_private_key = None;
        }
        BrokerKind::Coinbase => {
            config.brokers.coinbase_api_key = None;
            config.brokers.coinbase_api_secret = None;
        }
        BrokerKind::CryptoCom => {
            config.brokers.crypto_com_api_key = None;
            config.brokers.crypto_com_secret_key = None;
        }
    }
    save_config(&config)?;

    if json_out {
        println!(
            "{}",
            json!({
                "status": "ok",
                "broker": broker_name,
                "removed": deleted,
                "transactions_deleted": deleted_txs,
            })
        );
    } else if deleted {
        println!("Broker {} removed.", broker_name);
        if deleted_txs > 0 {
            println!("Deleted {} synced transactions.", deleted_txs);
        }
    } else {
        println!("Broker {} was not configured.", broker_name);
    }
    Ok(())
}

pub fn run_sync(
    backend: &BackendConnection,
    broker_filter: Option<BrokerKind>,
    dry_run: bool,
    json_out: bool,
) -> Result<()> {
    let config = load_config()?;

    let brokers: Vec<BrokerKind> = if let Some(b) = broker_filter {
        vec![b]
    } else {
        let connections = broker_connections::list_broker_connections_backend(backend)?;
        if connections.is_empty() {
            if json_out {
                println!("{}", json!({"status": "error", "message": "No brokers configured"}));
            } else {
                println!("No brokers configured. Add one first with: pftui portfolio broker add <BROKER>");
            }
            return Ok(());
        }
        connections
            .iter()
            .filter_map(|c| BrokerKind::from_str(&c.broker_name).ok())
            .collect()
    };

    let mut all_results = Vec::new();

    for broker_kind in brokers {
        let broker_name = broker_kind.to_string();

        // Ensure connection exists
        let conn = broker_connections::get_broker_connection_backend(backend, &broker_name)?;
        if conn.is_none() {
            let msg = format!("Broker {} is not configured. Run: pftui portfolio broker add {}", broker_name, broker_name);
            if json_out {
                all_results.push(json!({"broker": broker_name, "status": "error", "message": msg}));
            } else {
                println!("{}", msg);
            }
            continue;
        }

        // Create provider
        let provider = match broker::create_provider(broker_kind, &config) {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("{}", e);
                broker_connections::update_sync_status_backend(backend, &broker_name, "error", Some(&msg))?;
                if json_out {
                    all_results.push(json!({"broker": broker_name, "status": "error", "message": msg}));
                } else {
                    println!("[{}] Error: {}", broker_name, msg);
                }
                continue;
            }
        };

        // Check availability
        if let Err(e) = provider.is_available() {
            let msg = format!("Broker not reachable: {}", e);
            broker_connections::update_sync_status_backend(backend, &broker_name, "error", Some(&msg))?;
            if json_out {
                all_results.push(json!({"broker": broker_name, "status": "error", "message": msg}));
            } else {
                println!("[{}] {}", broker_name, msg);
            }
            continue;
        }

        // Fetch positions
        let positions = match provider.fetch_positions() {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("Failed to fetch positions: {}", e);
                broker_connections::update_sync_status_backend(backend, &broker_name, "error", Some(&msg))?;
                if json_out {
                    all_results.push(json!({"broker": broker_name, "status": "error", "message": msg}));
                } else {
                    println!("[{}] {}", broker_name, msg);
                }
                continue;
            }
        };

        let tag = broker::broker_tag(broker_kind);
        let today = Utc::now().format("%Y-%m-%d").to_string();

        if dry_run {
            if json_out {
                let items: Vec<_> = positions
                    .iter()
                    .map(|p| {
                        json!({
                            "symbol": p.symbol,
                            "quantity": p.quantity.to_string(),
                            "avg_cost": p.avg_cost.to_string(),
                            "currency": p.currency,
                            "category": p.category,
                        })
                    })
                    .collect();
                all_results.push(json!({
                    "broker": broker_name,
                    "status": "dry_run",
                    "positions": items,
                    "count": positions.len(),
                }));
            } else {
                println!("[{}] Dry run — {} positions would be synced:", broker_name, positions.len());
                for p in &positions {
                    println!(
                        "  {} {} @ {} {}",
                        p.symbol, p.quantity, p.avg_cost, p.currency
                    );
                }
            }
            continue;
        }

        // Delete existing broker transactions
        let deleted = broker_connections::delete_broker_transactions_backend(backend, &tag)?;

        // Insert new transactions
        let mut inserted = 0;
        for p in &positions {
            let category: AssetCategory = p.category.parse().unwrap_or(AssetCategory::Equity);
            let note = format!("{} synced position", tag);
            let tx = NewTransaction {
                symbol: p.symbol.clone(),
                category,
                tx_type: TxType::Buy,
                quantity: p.quantity,
                price_per: p.avg_cost,
                currency: p.currency.clone(),
                date: today.clone(),
                notes: Some(note),
            };
            insert_transaction_backend(backend, &tx)?;
            inserted += 1;
        }

        // Update sync status
        broker_connections::update_sync_status_backend(backend, &broker_name, "synced", None)?;

        if json_out {
            all_results.push(json!({
                "broker": broker_name,
                "status": "synced",
                "positions_synced": inserted,
                "old_transactions_deleted": deleted,
            }));
        } else {
            println!(
                "[{}] Synced {} positions (replaced {} old transactions)",
                broker_name, inserted, deleted
            );
        }
    }

    if json_out {
        if all_results.len() == 1 {
            println!("{}", serde_json::to_string_pretty(&all_results[0])?);
        } else {
            println!("{}", serde_json::to_string_pretty(&all_results)?);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rust_decimal::Decimal;

    #[test]
    fn broker_add_list_remove_roundtrip() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        // List should be empty
        let connections = broker_connections::list_broker_connections_backend(&backend).unwrap();
        assert!(connections.is_empty());

        // Add a broker connection (without config save — just DB)
        broker_connections::upsert_broker_connection_backend(
            &backend,
            "trading212",
            None,
            Some("test"),
        )
        .unwrap();

        // List should have one
        let connections = broker_connections::list_broker_connections_backend(&backend).unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].broker_name, "trading212");
        assert_eq!(connections[0].label.as_deref(), Some("test"));
        assert_eq!(connections[0].sync_status, "configured");

        // Remove
        let removed =
            broker_connections::delete_broker_connection_backend(&backend, "trading212").unwrap();
        assert!(removed);

        let connections = broker_connections::list_broker_connections_backend(&backend).unwrap();
        assert!(connections.is_empty());
    }

    #[test]
    fn broker_tag_format() {
        assert_eq!(broker::broker_tag(BrokerKind::Trading212), "[broker:trading212]");
        assert_eq!(broker::broker_tag(BrokerKind::Ibkr), "[broker:ibkr]");
    }

    #[test]
    fn broker_kind_display_roundtrip() {
        for kind in [BrokerKind::Trading212, BrokerKind::Ibkr, BrokerKind::Binance, BrokerKind::Kraken, BrokerKind::Coinbase, BrokerKind::CryptoCom] {
            let s = kind.to_string();
            let parsed: BrokerKind = s.parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn delete_broker_transactions_by_tag() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        // Insert a tagged transaction
        let tx = NewTransaction {
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: Decimal::from(10),
            price_per: Decimal::from(150),
            currency: "USD".to_string(),
            date: "2026-01-01".to_string(),
            notes: Some("[broker:trading212] synced position".to_string()),
        };
        insert_transaction_backend(&backend, &tx).unwrap();

        // Insert a regular transaction
        let tx2 = NewTransaction {
            symbol: "MSFT".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: Decimal::from(5),
            price_per: Decimal::from(300),
            currency: "USD".to_string(),
            date: "2026-01-01".to_string(),
            notes: Some("manual entry".to_string()),
        };
        insert_transaction_backend(&backend, &tx2).unwrap();

        // Delete broker-tagged transactions
        let deleted = broker_connections::delete_broker_transactions_backend(
            &backend,
            "[broker:trading212]",
        )
        .unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn update_sync_status_works() {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        broker_connections::upsert_broker_connection_backend(&backend, "ibkr", None, None).unwrap();
        broker_connections::update_sync_status_backend(&backend, "ibkr", "synced", None).unwrap();

        let bc = broker_connections::get_broker_connection_backend(&backend, "ibkr")
            .unwrap()
            .unwrap();
        assert_eq!(bc.sync_status, "synced");
        assert!(bc.last_sync_at.is_some());
    }
}
