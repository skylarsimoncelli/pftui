use std::collections::HashMap;

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use crate::models::price::{HistoryRecord, PriceQuote};

pub fn ticker_to_coingecko_id(ticker: &str) -> Option<&'static str> {
    match ticker.to_uppercase().as_str() {
        "BTC" => Some("bitcoin"),
        "ETH" => Some("ethereum"),
        "SOL" => Some("solana"),
        "ADA" => Some("cardano"),
        "DOT" => Some("polkadot"),
        "DOGE" => Some("dogecoin"),
        "AVAX" => Some("avalanche-2"),
        "MATIC" | "POL" => Some("matic-network"),
        "LINK" => Some("chainlink"),
        "UNI" => Some("uniswap"),
        "ATOM" => Some("cosmos"),
        "XRP" => Some("ripple"),
        "LTC" => Some("litecoin"),
        "BCH" => Some("bitcoin-cash"),
        "NEAR" => Some("near"),
        "FIL" => Some("filecoin"),
        "APT" => Some("aptos"),
        "ARB" => Some("arbitrum"),
        "OP" => Some("optimism"),
        "SUI" => Some("sui"),
        "SEI" => Some("sei-network"),
        "TIA" => Some("celestia"),
        "INJ" => Some("injective-protocol"),
        "RENDER" | "RNDR" => Some("render-token"),
        "FET" => Some("fetch-ai"),
        "GRT" => Some("the-graph"),
        "AAVE" => Some("aave"),
        "MKR" => Some("maker"),
        "CRV" => Some("curve-dao-token"),
        "SNX" => Some("havven"),
        "COMP" => Some("compound-governance-token"),
        "LDO" => Some("lido-dao"),
        "RPL" => Some("rocket-pool"),
        "PEPE" => Some("pepe"),
        "SHIB" => Some("shiba-inu"),
        "BONK" => Some("bonk"),
        "WIF" => Some("dogwifcoin"),
        "JUP" => Some("jupiter-exchange-solana"),
        "RAY" => Some("raydium"),
        "ONDO" => Some("ondo-finance"),
        "PENDLE" => Some("pendle"),
        "ENA" => Some("ethena"),
        "EIGEN" => Some("eigenlayer"),
        "STRK" => Some("starknet"),
        "ZK" => Some("zksync"),
        "W" => Some("wormhole"),
        "JTO" => Some("jito-governance-token"),
        "TRX" => Some("tron"),
        "TON" => Some("the-open-network"),
        "BNB" => Some("binancecoin"),
        "XLM" => Some("stellar"),
        "ALGO" => Some("algorand"),
        _ => None,
    }
}

/// Build a reqwest client with a proper User-Agent header.
/// CoinGecko may reject or rate-limit requests without one.
fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(Into::into)
}

/// Send a GET request with retry on 429 rate limit.
async fn get_with_retry(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let resp = client.get(url).send().await?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        // Wait 2 seconds and retry once on rate limit
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let retry_resp = client.get(url).send().await?;
        if !retry_resp.status().is_success() {
            bail!(
                "CoinGecko rate limited (429), retry failed with status {}",
                retry_resp.status()
            );
        }
        return Ok(retry_resp);
    }

    if !resp.status().is_success() {
        bail!("CoinGecko API returned status {}", resp.status());
    }

    Ok(resp)
}

pub async fn fetch_prices(tickers: &[String]) -> Result<Vec<PriceQuote>> {
    // Map tickers to CoinGecko IDs
    let mut id_to_ticker: HashMap<String, String> = HashMap::new();
    let mut ids = Vec::new();

    for ticker in tickers {
        if let Some(id) = ticker_to_coingecko_id(ticker) {
            id_to_ticker.insert(id.to_string(), ticker.to_uppercase());
            ids.push(id.to_string());
        }
    }

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let ids_param = ids.join(",");
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd&include_24hr_change=true",
        ids_param
    );

    let client = build_client()?;
    let resp = get_with_retry(&client, &url).await?;

    let data: HashMap<String, HashMap<String, f64>> = resp.json().await?;
    parse_price_response(data, &id_to_ticker)
}

fn parse_price_response(
    data: HashMap<String, HashMap<String, f64>>,
    id_to_ticker: &HashMap<String, String>,
) -> Result<Vec<PriceQuote>> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut quotes = Vec::new();

    for (id, prices) in &data {
        if let (Some(ticker), Some(&usd_price)) = (id_to_ticker.get(id), prices.get("usd")) {
            let price = Decimal::try_from(usd_price)?;
            quotes.push(PriceQuote {
                symbol: ticker.clone(),
                price,
                currency: "USD".to_string(),
                source: "coingecko".to_string(),
                fetched_at: now.clone(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                    previous_close: None,
            });
        }
    }

    Ok(quotes)
}

pub async fn fetch_history(ticker: &str, days: u32) -> Result<Vec<HistoryRecord>> {
    let id = match ticker_to_coingecko_id(ticker) {
        Some(id) => id,
        None => bail!("Unknown CoinGecko ticker: {}", ticker),
    };

    let url = format!(
        "https://api.coingecko.com/api/v3/coins/{}/market_chart?vs_currency=usd&days={}&interval=daily",
        id, days
    );

    let client = build_client()?;
    let resp = get_with_retry(&client, &url).await?;

    #[derive(serde::Deserialize)]
    struct MarketChart {
        prices: Vec<(f64, f64)>, // [timestamp_ms, price]
        #[serde(default)]
        total_volumes: Vec<(f64, f64)>, // [timestamp_ms, volume]
    }

    let data: MarketChart = resp.json().await?;

    // Build a volume lookup by date
    let mut volume_by_date: HashMap<String, u64> = HashMap::new();
    for (ts_ms, vol) in &data.total_volumes {
        let ts_secs = (*ts_ms / 1000.0) as i64;
        if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs, 0) {
            let date = dt.format("%Y-%m-%d").to_string();
            volume_by_date.insert(date, *vol as u64);
        }
    }

    let mut records = Vec::new();
    for (ts_ms, price) in &data.prices {
        let ts_secs = (*ts_ms / 1000.0) as i64;
        if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs, 0) {
            let date = dt.format("%Y-%m-%d").to_string();
            if let Ok(close) = Decimal::try_from(*price) {
                let volume = volume_by_date.get(&date).copied();
                records.push(HistoryRecord {
                    date,
                    close,
                    volume,
                    open: None,
                    high: None,
                    low: None,
                });
            }
        }
    }

    // Deduplicate by date (keep last entry per date)
    let mut seen = HashMap::new();
    for rec in records {
        seen.insert(rec.date.clone(), rec);
    }
    let mut result: Vec<HistoryRecord> = seen.into_values().collect();
    result.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticker_to_id_known_coins() {
        assert_eq!(ticker_to_coingecko_id("BTC"), Some("bitcoin"));
        assert_eq!(ticker_to_coingecko_id("btc"), Some("bitcoin"));
        assert_eq!(ticker_to_coingecko_id("ETH"), Some("ethereum"));
        assert_eq!(ticker_to_coingecko_id("SOL"), Some("solana"));
    }

    #[test]
    fn ticker_to_id_unknown_returns_none() {
        assert_eq!(ticker_to_coingecko_id("AAPL"), None);
        assert_eq!(ticker_to_coingecko_id("UNKNOWN"), None);
    }

    #[test]
    fn ticker_to_id_aliases() {
        // MATIC and POL both map to matic-network
        assert_eq!(ticker_to_coingecko_id("MATIC"), Some("matic-network"));
        assert_eq!(ticker_to_coingecko_id("POL"), Some("matic-network"));
        // RENDER and RNDR both map to render-token
        assert_eq!(ticker_to_coingecko_id("RENDER"), Some("render-token"));
        assert_eq!(ticker_to_coingecko_id("RNDR"), Some("render-token"));
    }

    #[test]
    fn parse_price_response_extracts_quotes() {
        let mut data = HashMap::new();
        let mut btc_prices = HashMap::new();
        btc_prices.insert("usd".to_string(), 50000.0);
        data.insert("bitcoin".to_string(), btc_prices);

        let mut id_to_ticker = HashMap::new();
        id_to_ticker.insert("bitcoin".to_string(), "BTC".to_string());

        let quotes = parse_price_response(data, &id_to_ticker).unwrap();
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].symbol, "BTC");
        assert_eq!(quotes[0].source, "coingecko");
        assert_eq!(quotes[0].currency, "USD");
    }

    #[test]
    fn parse_price_response_skips_missing_ticker() {
        let mut data = HashMap::new();
        let mut unknown_prices = HashMap::new();
        unknown_prices.insert("usd".to_string(), 100.0);
        data.insert("unknown-coin".to_string(), unknown_prices);

        let id_to_ticker = HashMap::new(); // no mappings
        let quotes = parse_price_response(data, &id_to_ticker).unwrap();
        assert!(quotes.is_empty());
    }

    #[test]
    fn parse_price_response_skips_missing_usd() {
        let mut data = HashMap::new();
        let mut btc_prices = HashMap::new();
        btc_prices.insert("eur".to_string(), 45000.0); // no "usd" key
        data.insert("bitcoin".to_string(), btc_prices);

        let mut id_to_ticker = HashMap::new();
        id_to_ticker.insert("bitcoin".to_string(), "BTC".to_string());

        let quotes = parse_price_response(data, &id_to_ticker).unwrap();
        assert!(quotes.is_empty());
    }

    #[test]
    fn build_client_succeeds() {
        let client = build_client();
        assert!(client.is_ok());
    }
}
