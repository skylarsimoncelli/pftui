use anyhow::Result;
use serde_json::{json, Value};

use crate::db::backend::BackendConnection;
use crate::db::onchain_cache;

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let payload = build_payload(backend)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_summary(&payload);
    }
    Ok(())
}

fn build_payload(backend: &BackendConnection) -> Result<Value> {
    let network = latest_metric(backend, "network")?;
    let exchange = latest_metric(backend, "exchange_reserve_proxy_btc")?;
    let whale_flow = latest_metric(backend, "largest_transactions_24h_btc")?;
    let active_addresses = latest_metric(backend, "active_addresses_24h")?;
    let wealth = latest_metric(backend, "wealth_distribution_top10_pct")?;

    let mut payload = serde_json::Map::new();

    if let Some(metric) = exchange {
        let metadata = parse_metadata(metric.metadata.as_deref());
        payload.insert("date".to_string(), json!(metric.date));
        payload.insert(
            "exchange_reserves_btc".to_string(),
            number_or_string(&metric.value),
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "reserve_usd",
            "exchange_reserves_usd",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "tracked_wallets",
            "exchange_wallets_tracked",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "exchange_labels",
            "exchange_labels",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "flow_7d_btc",
            "exchange_flow_7d_btc",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "flow_30d_btc",
            "exchange_flow_30d_btc",
        );
        if let Some(top_exchanges) = metadata.get("top_exchanges") {
            payload.insert("top_exchanges".to_string(), top_exchanges.clone());
        }
    }

    if let Some(metric) = network {
        let metadata = parse_metadata(metric.metadata.as_deref());
        payload.insert("hash_rate".to_string(), number_or_string(&metric.value));
        copy_metadata_field(&mut payload, &metadata, "difficulty", "difficulty");
        copy_metadata_field(&mut payload, &metadata, "blocks_24h", "blocks_24h");
        copy_metadata_field(&mut payload, &metadata, "mempool_size", "mempool_size");
        copy_metadata_field(&mut payload, &metadata, "avg_fee_sat_b", "avg_fee_sat_b");
    }

    if let Some(metric) = whale_flow {
        let metadata = parse_metadata(metric.metadata.as_deref());
        payload.insert(
            "largest_transactions_24h_btc".to_string(),
            number_or_string(&metric.value),
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "largest_transactions_24h_usd",
            "largest_transactions_24h_usd",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "largest_transactions_24h_share_pct",
            "largest_transactions_24h_share_pct",
        );
    }

    if let Some(metric) = active_addresses {
        payload.insert(
            "active_addresses_24h".to_string(),
            number_or_string(&metric.value),
        );
    }

    if let Some(metric) = wealth {
        let metadata = parse_metadata(metric.metadata.as_deref());
        payload.insert(
            "wealth_distribution_top10_pct".to_string(),
            number_or_string(&metric.value),
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "top_100_share_pct",
            "wealth_distribution_top100_pct",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "top_1000_share_pct",
            "wealth_distribution_top1000_pct",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "top_10000_share_pct",
            "wealth_distribution_top10000_pct",
        );
        copy_metadata_field(
            &mut payload,
            &metadata,
            "top_100_richest_btc",
            "top_100_richest_btc",
        );
    }

    payload.insert(
        "available".to_string(),
        json!({
            "exchange_reserves": payload.contains_key("exchange_reserves_btc"),
            "network": payload.contains_key("hash_rate"),
            "whale_activity": payload.contains_key("largest_transactions_24h_btc"),
            "wealth_distribution": payload.contains_key("wealth_distribution_top10_pct")
        }),
    );

    Ok(Value::Object(payload))
}

fn latest_metric(
    backend: &BackendConnection,
    metric: &str,
) -> Result<Option<onchain_cache::OnchainMetric>> {
    Ok(
        onchain_cache::get_metrics_by_type_backend(backend, metric, 1)?
            .into_iter()
            .next(),
    )
}

fn parse_metadata(metadata: Option<&str>) -> Value {
    metadata
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .unwrap_or_else(|| json!({}))
}

fn number_or_string(raw: &str) -> Value {
    raw.parse::<f64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(raw.to_string()))
}

fn copy_metadata_field(
    payload: &mut serde_json::Map<String, Value>,
    metadata: &Value,
    source_key: &str,
    target_key: &str,
) {
    if let Some(value) = metadata.get(source_key) {
        payload.insert(target_key.to_string(), value.clone());
    }
}

fn print_summary(payload: &Value) {
    if payload
        .as_object()
        .map(|obj| obj.is_empty())
        .unwrap_or(true)
    {
        println!("No cached on-chain metrics found. Run `pftui data refresh` first.");
        return;
    }

    println!("On-Chain Metrics");
    if let Some(date) = payload.get("date").and_then(Value::as_str) {
        println!("  Date: {}", date);
    }
    if let Some(value) = payload.get("exchange_reserves_btc") {
        println!("  Exchange reserves (BTC): {}", render_value(value));
    }
    if let Some(value) = payload.get("exchange_flow_7d_btc") {
        println!("  Exchange flow 7D (BTC): {}", render_value(value));
    }
    if let Some(value) = payload.get("hash_rate") {
        println!("  Hash rate: {}", render_value(value));
    }
    if let Some(value) = payload.get("active_addresses_24h") {
        println!("  Active addresses 24H: {}", render_value(value));
    }
    if let Some(value) = payload.get("largest_transactions_24h_btc") {
        println!("  Largest tx 24H (BTC): {}", render_value(value));
    }
    if let Some(value) = payload.get("wealth_distribution_top10_pct") {
        println!("  Top 10 wealth share %: {}", render_value(value));
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_payload_surfaces_cached_metrics() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let sqlite = backend.sqlite();

        let rows = [
            onchain_cache::OnchainMetric {
                metric: "exchange_reserve_proxy_btc".to_string(),
                date: "2026-03-16".to_string(),
                value: "2750000".to_string(),
                metadata: Some(
                    json!({
                        "reserve_usd": 180000000000_u64,
                        "tracked_wallets": 220,
                        "exchange_labels": 12,
                        "flow_7d_btc": -15000.5,
                        "flow_30d_btc": -42000.0,
                        "top_exchanges": [{"label": "Binance", "balance_btc": 500000.0}]
                    })
                    .to_string(),
                ),
                fetched_at: "2026-03-16T00:00:00Z".to_string(),
            },
            onchain_cache::OnchainMetric {
                metric: "network".to_string(),
                date: "2026-03-16".to_string(),
                value: "825.4".to_string(),
                metadata: Some(
                    json!({
                        "difficulty": 92.1,
                        "blocks_24h": 143,
                        "mempool_size": 12345,
                        "avg_fee_sat_b": 3.2
                    })
                    .to_string(),
                ),
                fetched_at: "2026-03-16T00:00:00Z".to_string(),
            },
            onchain_cache::OnchainMetric {
                metric: "largest_transactions_24h_btc".to_string(),
                date: "2026-03-16".to_string(),
                value: "145000".to_string(),
                metadata: Some(
                    json!({
                        "largest_transactions_24h_usd": 9800000000_u64,
                        "largest_transactions_24h_share_pct": 0.73
                    })
                    .to_string(),
                ),
                fetched_at: "2026-03-16T00:00:00Z".to_string(),
            },
            onchain_cache::OnchainMetric {
                metric: "active_addresses_24h".to_string(),
                date: "2026-03-16".to_string(),
                value: "815432".to_string(),
                metadata: Some("{}".to_string()),
                fetched_at: "2026-03-16T00:00:00Z".to_string(),
            },
            onchain_cache::OnchainMetric {
                metric: "wealth_distribution_top10_pct".to_string(),
                date: "2026-03-16".to_string(),
                value: "5.1".to_string(),
                metadata: Some(
                    json!({
                        "top_100_share_pct": 14.2,
                        "top_1000_share_pct": 34.8,
                        "top_10000_share_pct": 58.1,
                        "top_100_richest_btc": 2890000.0
                    })
                    .to_string(),
                ),
                fetched_at: "2026-03-16T00:00:00Z".to_string(),
            },
        ];

        for row in rows {
            onchain_cache::upsert_metric(sqlite, &row).unwrap();
        }

        let payload = build_payload(&backend).unwrap();
        assert_eq!(payload["exchange_reserves_btc"], json!(2750000.0));
        assert_eq!(payload["exchange_flow_7d_btc"], json!(-15000.5));
        assert_eq!(payload["hash_rate"], json!(825.4));
        assert_eq!(payload["active_addresses_24h"], json!(815432.0));
        assert_eq!(payload["wealth_distribution_top100_pct"], json!(14.2));
        assert_eq!(payload["available"]["exchange_reserves"], json!(true));
    }
}
