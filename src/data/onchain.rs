//! Fetch BTC on-chain data from Blockchair API.
//!
//! Free tier: 5 requests/second, no API key required.
//! Docs: https://blockchair.com/api/docs

use anyhow::{bail, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct ExchangeFlow {
    pub date: String,        // YYYY-MM-DD
    pub net_flow: f64,       // BTC net flow (negative = outflow/accumulation)
    pub inflow: f64,         // BTC inflow to exchanges
    pub outflow: f64,        // BTC outflow from exchanges
}

#[derive(Debug, Deserialize)]
struct BlockchairResponse {
    data: BlockchairData,
}

#[derive(Debug, Deserialize)]
struct BlockchairData {
    #[serde(default)]
    transactions: Vec<BlockchairTransaction>,
}

#[derive(Debug, Deserialize)]
struct BlockchairTransaction {
    time: String,
    #[serde(default)]
    value: f64,
}

/// Fetch recent BTC exchange net flows from Blockchair.
///
/// Returns the last 7 days of exchange flow data.
/// Free API, no authentication required, rate limit: 5 req/sec.
pub fn fetch_exchange_flows() -> Result<Vec<ExchangeFlow>> {
    // Blockchair doesn't provide direct exchange flow endpoints in the free tier.
    // For now, return placeholder data structure that can be enhanced later
    // with either paid tier access or alternative free sources.
    
    // TODO: Implement actual Blockchair API call when endpoint is available
    // For F21.1 initial implementation, we'll focus on the data structure
    // and caching layer, with placeholder data.
    
    bail!("Blockchair exchange flow data requires additional API research")
}

/// Fetch current BTC network metrics from Blockchair.
///
/// Returns: mempool size, hash rate, difficulty, avg block time.
pub fn fetch_network_metrics() -> Result<NetworkMetrics> {
    let url = "https://api.blockchair.com/bitcoin/stats";
    
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    
    let resp = client
        .get(url)
        .header("User-Agent", "pftui/0.4.1")
        .send()?;

    if !resp.status().is_success() {
        bail!("Blockchair API returned {}", resp.status());
    }

    let body: BlockchairStatsResponse = resp.json()?;
    
    Ok(NetworkMetrics {
        mempool_size: body.data.mempool_transactions,
        hash_rate: body.data.hashrate_24h,
        difficulty: body.data.difficulty,
        avg_block_time: body.data.average_transaction_fee_24h.unwrap_or(0.0),
    })
}

#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    pub mempool_size: u64,
    pub hash_rate: f64,      // H/s
    pub difficulty: f64,
    pub avg_block_time: f64, // seconds
}

#[derive(Debug, Deserialize)]
struct BlockchairStatsResponse {
    data: BlockchairStats,
}

#[derive(Debug, Deserialize)]
struct BlockchairStats {
    mempool_transactions: u64,
    hashrate_24h: f64,
    difficulty: f64,
    average_transaction_fee_24h: Option<f64>,
}
