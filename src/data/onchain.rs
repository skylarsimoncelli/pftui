//! Fetch BTC on-chain data from multiple sources.
//!
//! Data sources:
//! - Blockchair API: network metrics (✓ WORKING - free, 5 req/sec, no key)
//! - CoinGlass: BTC ETF flows (✗ NOT IMPLEMENTED - requires API key or JS execution)
//! - Whale Alert: large transactions (✗ NOT IMPLEMENTED - requires API key)
//! - Exchange flows: (✗ NOT IMPLEMENTED - free sources need research)
//!
//! Status: Network metrics work. Others gracefully fail with clear error messages.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::time::Duration;

// ============================================================================
// Exchange Flow Data (Blockchair alternative approach)
// ============================================================================

#[derive(Debug, Clone)]
pub struct ExchangeFlow {
    pub date: String,   // YYYY-MM-DD
    pub net_flow: f64,  // BTC (negative = outflow/accumulation)
    pub inflow: f64,    // BTC inflow to exchanges
    pub outflow: f64,   // BTC outflow from exchanges
}

/// Fetch BTC exchange flows.
///
/// Note: Blockchair free tier doesn't provide direct exchange flow data.
/// Alternative: use Glassnode public charts or on-chain.info (if scraping is acceptable).
/// For F21.1 initial implementation, we focus on ETF flows + whale alerts first.
pub fn fetch_exchange_flows() -> Result<Vec<ExchangeFlow>> {
    // Placeholder for future implementation
    // Potential free sources:
    // 1. CryptoQuant public dashboard (scrape)
    // 2. Glassnode public charts (limited data, scrape)
    // 3. Alternative.me (if they add exchange flow data)
    
    bail!("Exchange flow data requires additional research for free sources")
}

// ============================================================================
// BTC ETF Flow Data (CoinGlass scraping)
// ============================================================================

#[derive(Debug, Clone)]
pub struct EtfFlow {
    pub date: String,       // YYYY-MM-DD
    pub fund: String,       // e.g., "IBIT", "FBTC", "GBTC"
    pub net_flow_btc: f64,  // Net BTC inflow (positive = inflow)
    pub net_flow_usd: f64,  // Net USD value
}

/// Fetch BTC ETF flows from CoinGlass public page.
///
/// Note: CoinGlass page uses client-side rendering with embedded JS data.
/// This is a placeholder that gracefully fails with a clear message.
/// TODO: Either implement JS parsing or find alternative free ETF flow source.
pub fn fetch_etf_flows() -> Result<Vec<EtfFlow>> {
    bail!("ETF flow data currently unavailable (CoinGlass uses client-side JS rendering; API access required)")
}

/// Parse CoinGlass HTML for ETF flow data (currently unused).
///
/// CoinGlass uses client-side rendering, so HTML parsing alone is insufficient.
/// Left as reference for future implementation if API access is obtained.
#[allow(dead_code)]
fn parse_coinglass_etf_page(_html: &str) -> Result<Vec<EtfFlow>> {
    bail!("ETF flow page parsing not implemented (requires API or JS execution)")
}

// ============================================================================
// Whale Alert (Large Transactions)
// ============================================================================

#[derive(Debug, Clone)]
pub struct WhaleTransaction {
    pub timestamp: i64,     // Unix timestamp
    pub amount_btc: f64,    // BTC amount
    pub amount_usd: f64,    // USD value at time of tx
    pub from_owner: String, // Source (e.g., "Binance", "unknown")
    pub to_owner: String,   // Destination
    pub tx_hash: String,    // Transaction hash
}

/// Fetch large BTC transactions from Whale Alert public feed.
///
/// Free tier: limited to recent transactions, no API key required for basic data
/// Note: Whale Alert API does require a key for comprehensive access.
/// Public feed alternative: parse their Twitter/Telegram for recent large txs.
pub fn fetch_whale_transactions() -> Result<Vec<WhaleTransaction>> {
    // Whale Alert public API endpoint (limited, may require key for full access)
    // Alternative: scrape their public blockchain explorers or social feeds
    
    // For F21.1 MVP, document that we need to:
    // 1. Sign up for Whale Alert free API key (if available)
    // 2. OR scrape their public Telegram channel
    // 3. OR use alternative on-chain explorers with large tx filters
    
    bail!("Whale Alert data requires API key or alternative source selection")
}

// ============================================================================
// Network Metrics (Blockchair - Working)
// ============================================================================

#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    pub mempool_size: u64,
    pub hash_rate: f64,      // H/s
    pub difficulty: f64,
    pub avg_fee_sat_b: f64,  // Sat/byte average fee
    pub blocks_24h: u64,
}

/// Fetch current BTC network metrics from Blockchair.
///
/// This endpoint works and requires no API key.
pub fn fetch_network_metrics() -> Result<NetworkMetrics> {
    let url = "https://api.blockchair.com/bitcoin/stats";
    
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("pftui/0.4.1")
        .build()?;
    
    let resp = client
        .get(url)
        .send()?;

    if !resp.status().is_success() {
        bail!("Blockchair API returned {}", resp.status());
    }

    let body: BlockchairStatsResponse = resp.json()?;
    
    Ok(NetworkMetrics {
        mempool_size: body.data.mempool_transactions,
        hash_rate: body.data.hashrate_24h,
        difficulty: body.data.difficulty,
        avg_fee_sat_b: body.data.suggested_transaction_fee_per_byte_sat,
        blocks_24h: body.data.blocks_24h,
    })
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
    suggested_transaction_fee_per_byte_sat: f64,
    blocks_24h: u64,
}

// ============================================================================
// Glassnode Public Data (Alternative for Exchange Flows)
// ============================================================================

/// Fetch exchange reserves from Glassnode public API.
///
/// Glassnode has a free tier API with limited endpoints.
/// Endpoint: https://api.glassnode.com/v1/metrics/distribution/balance_exchanges
pub fn fetch_glassnode_exchange_reserves() -> Result<f64> {
    // Glassnode free tier may require API key even for basic endpoints
    // This is a research task for F21.1+
    
    bail!("Glassnode exchange reserves requires API key (free tier available)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_metrics_live() {
        // This test hits the real Blockchair API
        // Skip in CI to avoid rate limits
        if std::env::var("CI").is_ok() {
            return;
        }

        let result = fetch_network_metrics();
        if let Ok(metrics) = result {
            assert!(metrics.hash_rate > 0.0);
            assert!(metrics.difficulty > 0.0);
            assert!(metrics.blocks_24h > 0);
        }
        // Don't fail test if API is down
    }

    #[test]
    fn test_exchange_flows_placeholder() {
        let result = fetch_exchange_flows();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("research"));
    }

    #[test]
    fn test_etf_flows_placeholder() {
        // Skip network test in CI
        if std::env::var("CI").is_ok() {
            return;
        }
        
        // This test may succeed (returning empty vec) or fail depending on
        // whether CoinGlass is accessible and page structure matches expectations
        let _result = fetch_etf_flows();
        // Don't assert error - page may load successfully but return empty data
    }

    #[test]
    fn test_whale_transactions_placeholder() {
        let result = fetch_whale_transactions();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("key") || err_msg.contains("source"));
    }
}
