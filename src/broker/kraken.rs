use anyhow::Result;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{BrokerKind, BrokerPosition, BrokerProvider};

pub struct KrakenProvider {
    api_key: String,
    private_key: Vec<u8>,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct KrakenResponse<T> {
    pub error: Vec<String>,
    pub result: Option<T>,
}

impl KrakenProvider {
    pub fn new(api_key: &str, private_key_b64: &str) -> Self {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(private_key_b64)
            .unwrap_or_default();
        Self {
            api_key: api_key.to_string(),
            private_key: decoded,
            base_url: "https://api.kraken.com".to_string(),
        }
    }

    fn sign(&self, path: &str, nonce: u64, post_data: &str) -> String {
        use sha2::Digest;
        // SHA256(nonce + post_data)
        let mut sha256 = Sha256::new();
        sha256.update(format!("{nonce}{post_data}").as_bytes());
        let sha256_hash = sha256.finalize();

        // HMAC-SHA512(path + sha256_hash, base64_decoded_secret)
        let mut hmac_input = path.as_bytes().to_vec();
        hmac_input.extend_from_slice(&sha256_hash);

        type HmacSha512 = Hmac<sha2::Sha512>;
        let mut mac = HmacSha512::new_from_slice(&self.private_key).expect("HMAC key");
        mac.update(&hmac_input);
        let result = mac.finalize();

        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(result.into_bytes())
    }

    fn nonce() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis() as u64
    }
}

impl BrokerProvider for KrakenProvider {
    fn kind(&self) -> BrokerKind {
        BrokerKind::Kraken
    }

    fn is_available(&self) -> Result<()> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}/0/public/SystemStatus", self.base_url))
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Kraken API not reachable (status {})", resp.status())
        }
    }

    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>> {
        let path = "/0/private/Balance";
        let nonce = Self::nonce();
        let post_data = format!("nonce={nonce}");
        let signature = self.sign(path, nonce, &post_data);

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}{}", self.base_url, path))
            .header("API-Key", &self.api_key)
            .header("API-Sign", &signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .timeout(std::time::Duration::from_secs(15))
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Kraken balance fetch failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
        }

        let body: KrakenResponse<HashMap<String, String>> = resp.json()?;
        if !body.error.is_empty() {
            anyhow::bail!("Kraken API error: {}", body.error.join(", "));
        }
        let balances = body.result.unwrap_or_default();

        // Fetch ticker prices for USD pairs
        let prices = self.fetch_ticker_prices()?;

        let mut result = Vec::new();
        for (asset, balance_str) in &balances {
            let qty = Decimal::from_str(balance_str).unwrap_or(Decimal::ZERO);
            if qty.is_zero() {
                continue;
            }

            let normalized = normalize_kraken_asset(asset);

            let avg_cost = if normalized == "USD" || normalized == "USDT" || normalized == "USDC" {
                Decimal::ONE
            } else {
                lookup_price(&prices, asset).unwrap_or(Decimal::ZERO)
            };

            result.push(BrokerPosition {
                symbol: normalized,
                quantity: qty,
                avg_cost,
                currency: "USD".to_string(),
                category: "crypto".to_string(),
            });
        }
        Ok(result)
    }
}

impl KrakenProvider {
    fn fetch_ticker_prices(&self) -> Result<HashMap<String, Decimal>> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}/0/public/Ticker", self.base_url))
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if !resp.status().is_success() {
            return Ok(HashMap::new());
        }

        let body: KrakenResponse<HashMap<String, serde_json::Value>> = resp.json()?;
        let tickers = body.result.unwrap_or_default();
        let mut prices = HashMap::new();
        for (pair, data) in &tickers {
            // "c" field is [price, lot_volume] — last trade close
            if let Some(c) = data.get("c").and_then(|v| v.as_array()) {
                if let Some(price_str) = c.first().and_then(|v| v.as_str()) {
                    if let Ok(price) = Decimal::from_str(price_str) {
                        prices.insert(pair.clone(), price);
                    }
                }
            }
        }
        Ok(prices)
    }
}

/// Kraken uses non-standard asset codes (XXBT for BTC, ZUSD for USD, etc.)
fn normalize_kraken_asset(asset: &str) -> String {
    match asset {
        "XXBT" | "XBT" => "BTC".to_string(),
        "XETH" => "ETH".to_string(),
        "XXRP" => "XRP".to_string(),
        "XLTC" => "LTC".to_string(),
        "XXLM" => "XLM".to_string(),
        "XXDG" | "XDOGE" => "DOGE".to_string(),
        "ZUSD" => "USD".to_string(),
        "ZEUR" => "EUR".to_string(),
        "ZGBP" => "GBP".to_string(),
        "ZJPY" => "JPY".to_string(),
        "ZCAD" => "CAD".to_string(),
        "ZAUD" => "AUD".to_string(),
        other => other.to_string(),
    }
}

/// Look up price from Kraken ticker data — tries common pair formats
fn lookup_price(prices: &HashMap<String, Decimal>, asset: &str) -> Option<Decimal> {
    // Try standard pairs: XXBTZUSD, XETHZUSD, etc.
    let prefixed = match asset {
        "XXBT" | "XBT" => Some("XXBTZUSD"),
        "XETH" => Some("XETHZUSD"),
        "XXRP" => Some("XXRPZUSD"),
        "XLTC" => Some("XLTCZUSD"),
        "XXLM" => Some("XXLMZUSD"),
        _ => None,
    };
    if let Some(key) = prefixed {
        if let Some(price) = prices.get(key) {
            return Some(*price);
        }
    }

    // Try ASSETUSD pair
    let pair = format!("{asset}USD");
    if let Some(price) = prices.get(&pair) {
        return Some(*price);
    }

    // Try ASSET/USD pair
    let pair_slash = format!("{asset}/USD");
    if let Some(price) = prices.get(&pair_slash) {
        return Some(*price);
    }

    None
}
