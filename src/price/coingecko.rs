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
        "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
        ids_param
    );

    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        bail!("CoinGecko API returned status {}", resp.status());
    }

    let data: HashMap<String, HashMap<String, f64>> = resp.json().await?;
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

    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        bail!("CoinGecko history API returned status {}", resp.status());
    }

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
                records.push(HistoryRecord { date, close, volume });
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
