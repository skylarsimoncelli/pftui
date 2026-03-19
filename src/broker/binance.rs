use anyhow::Result;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{BrokerKind, BrokerPosition, BrokerProvider};

pub struct BinanceProvider {
    api_key: String,
    secret_key: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    pub balances: Vec<Balance>,
}

#[derive(Debug, Deserialize)]
struct Balance {
    pub asset: String,
    pub free: String,
    pub locked: String,
}

#[derive(Debug, Deserialize)]
struct TickerPrice {
    pub symbol: String,
    pub price: String,
}

impl BinanceProvider {
    pub fn new(api_key: &str, secret_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            base_url: "https://api.binance.com".to_string(),
        }
    }

    fn sign(&self, query: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.secret_key.as_bytes()).expect("HMAC key size");
        mac.update(query.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn timestamp_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis()
    }

    fn fetch_avg_prices(&self) -> Result<std::collections::HashMap<String, Decimal>> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}/api/v3/ticker/price", self.base_url))
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if !resp.status().is_success() {
            return Ok(std::collections::HashMap::new());
        }
        let tickers: Vec<TickerPrice> = resp.json()?;
        let mut map = std::collections::HashMap::new();
        for t in tickers {
            if let Ok(price) = Decimal::from_str(&t.price) {
                map.insert(t.symbol, price);
            }
        }
        Ok(map)
    }
}

impl BrokerProvider for BinanceProvider {
    fn kind(&self) -> BrokerKind {
        BrokerKind::Binance
    }

    fn is_available(&self) -> Result<()> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}/api/v3/ping", self.base_url))
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Binance API not reachable (status {})", resp.status())
        }
    }

    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>> {
        let ts = Self::timestamp_ms();
        let query = format!("timestamp={ts}");
        let signature = self.sign(&query);
        let url = format!(
            "{}/api/v3/account?{}&signature={}",
            self.base_url, query, signature
        );

        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .timeout(std::time::Duration::from_secs(15))
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Binance account fetch failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
        }

        let account: AccountInfo = resp.json()?;
        let prices = self.fetch_avg_prices().unwrap_or_default();

        let mut result = Vec::new();
        for b in account.balances {
            let free = Decimal::from_str(&b.free).unwrap_or(Decimal::ZERO);
            let locked = Decimal::from_str(&b.locked).unwrap_or(Decimal::ZERO);
            let total = free + locked;
            if total.is_zero() {
                continue;
            }

            // Use current price as avg_cost (Binance doesn't expose avg cost via REST)
            let price_key = format!("{}USDT", b.asset);
            let avg_cost = if b.asset == "USDT" || b.asset == "USD" {
                Decimal::ONE
            } else {
                prices.get(&price_key).copied().unwrap_or(Decimal::ZERO)
            };

            result.push(BrokerPosition {
                symbol: b.asset.clone(),
                quantity: total,
                avg_cost,
                currency: "USD".to_string(),
                category: "crypto".to_string(),
            });
        }
        Ok(result)
    }
}
