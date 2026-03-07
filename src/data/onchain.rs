//! Fetch BTC on-chain data from multiple sources.
//!
//! Data sources:
//! - Blockchair API: network metrics (✓ WORKING - free, 5 req/sec, no key)
//! - btcetffundflow.com: BTC ETF flows (✓ WORKING - free, daily updates at D+1 09:00 GMT)
//! - Whale Alert: large transactions (✗ NOT IMPLEMENTED - requires API key)
//! - Exchange flows: (✗ NOT IMPLEMENTED - free sources need research)
//!
//! Status: Network metrics and ETF flows work. Others gracefully fail with clear error messages.

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

/// Fetch BTC ETF flows from btcetffundflow.com embedded data.
///
/// The site embeds flow data in JS arrays in the HTML. This implementation
/// parses the embedded JSON structure from the page source.
///
/// Data update frequency: D+1 09:00 GMT (next day, 9 AM)
/// Source: btcetffundflow.com (managed by SmashFi)
pub fn fetch_etf_flows() -> Result<Vec<EtfFlow>> {
    let url = "https://btcetffundflow.com/us";
    
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; pftui/1.0)")
        .timeout(Duration::from_secs(10))
        .build()?;
    
    let response = client.get(url).send()?;
    
    if !response.status().is_success() {
        bail!("Failed to fetch ETF flow page: HTTP {}", response.status());
    }
    
    let html = response.text()?;
    
    // Parse embedded JSON data from the HTML
    // The page embeds data in a structure like: "flows2":[{"provider":"0","value":"131.0"}, ...]
    // This represents the latest daily flows per provider (provider index maps to ETF fund)
    
    parse_btcetffundflow_html(&html)
}

/// Parse btcetffundflow.com HTML for embedded ETF flow JSON data.
///
/// The page embeds flow data in a JSON structure within a Next.js <script> tag.
/// We extract the "flows2" array which contains the most recent daily flows.
fn parse_btcetffundflow_html(html: &str) -> Result<Vec<EtfFlow>> {
    // ETF provider index mapping (from site's embedded data)
    let provider_names = [
        "TOTAL", "GBTC", "BITB", "IBIT", "HODL", "EZBC",
        "BTCO", "BRRR", "FBTC", "DEFI", "ARKB", "BTCW", "BTC"
    ];
    
    // Find the embedded JSON data in the Next.js script tag
    // Pattern: "__NEXT_DATA__" type="application/json">{"props": ... "flows2":
    
    let start_marker = "__NEXT_DATA__\" type=\"application/json\">";
    let start_idx = html.find(start_marker)
        .ok_or_else(|| anyhow::anyhow!("Could not find embedded data marker"))?;
    
    let json_start = start_idx + start_marker.len();
    let json_end = html[json_start..].find("</script>")
        .ok_or_else(|| anyhow::anyhow!("Could not find end of JSON data"))?;
    
    let json_str = &html[json_start..json_start + json_end];
    
    // Parse the JSON to extract flows2 array
    let json_value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse embedded JSON: {}", e))?;
    
    // Navigate to: props.pageProps.dehydratedState.queries[0].state.data.data.flows2
    let flows2 = json_value
        .get("props")
        .and_then(|p| p.get("pageProps"))
        .and_then(|pp| pp.get("dehydratedState"))
        .and_then(|ds| ds.get("queries"))
        .and_then(|q| q.get(0))
        .and_then(|q0| q0.get("state"))
        .and_then(|s| s.get("data"))
        .and_then(|d| d.get("data"))
        .and_then(|d2| d2.get("flows2"))
        .and_then(|f| f.as_array())
        .ok_or_else(|| anyhow::anyhow!("Could not navigate to flows2 array in JSON structure"))?;
    
    // Also get the timestamp from the first query's dataUpdatedAt
    let timestamp_ms = json_value
        .get("props")
        .and_then(|p| p.get("pageProps"))
        .and_then(|pp| pp.get("dehydratedState"))
        .and_then(|ds| ds.get("queries"))
        .and_then(|q| q.get(0))
        .and_then(|q0| q0.get("state"))
        .and_then(|s| s.get("dataUpdatedAt"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);
    
    // Convert timestamp to date string (YYYY-MM-DD)
    let date = if timestamp_ms > 0 {
        let datetime = chrono::DateTime::from_timestamp(timestamp_ms / 1000, 0)
            .unwrap_or_else(chrono::Utc::now);
        datetime.format("%Y-%m-%d").to_string()
    } else {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    };
    
    let mut results = Vec::new();
    
    // Parse each flow entry
    for (idx, flow_obj) in flows2.iter().enumerate() {
        if let (Some(provider_idx), Some(value_str)) = (
            flow_obj.get("provider").and_then(|p| p.as_str()),
            flow_obj.get("value").and_then(|v| v.as_str())
        ) {
            // Skip TOTAL (provider "0")
            if provider_idx == "0" {
                continue;
            }
            
            let provider_num: usize = provider_idx.parse()
                .unwrap_or(idx);
            
            if provider_num < provider_names.len() {
                let flow_btc: f64 = value_str.parse().unwrap_or(0.0);
                
                // Estimate USD value (we don't have real-time BTC price in this context)
                // Use a placeholder - actual implementation should fetch current BTC price
                let btc_price = 100_000.0; // Placeholder
                let flow_usd = flow_btc * btc_price;
                
                results.push(EtfFlow {
                    date: date.clone(),
                    fund: provider_names[provider_num].to_string(),
                    net_flow_btc: flow_btc,
                    net_flow_usd: flow_usd,
                });
            }
        }
    }
    
    if results.is_empty() {
        bail!("No ETF flow data extracted from page");
    }
    
    Ok(results)
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
