use anyhow::Result;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{BrokerKind, BrokerPosition, BrokerProvider};

pub struct CoinbaseProvider {
    api_key: String,
    api_secret: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct AccountsResponse {
    pub accounts: Vec<CoinbaseAccount>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseAccount {
    pub currency: Option<String>,
    pub available_balance: Option<CoinbaseBalance>,
}

#[derive(Debug, Deserialize)]
struct CoinbaseBalance {
    pub value: Option<String>,
    pub _currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotPriceResponse {
    pub data: Option<SpotPriceData>,
}

#[derive(Debug, Deserialize)]
struct SpotPriceData {
    pub amount: Option<String>,
}

impl CoinbaseProvider {
    pub fn new(api_key: &str, api_secret: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            base_url: "https://api.coinbase.com".to_string(),
        }
    }

    fn sign(&self, timestamp: u64, method: &str, path: &str, body: &str) -> String {
        let message = format!("{timestamp}{method}{path}{body}");
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.api_secret.as_bytes()).expect("HMAC key");
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
    }

    fn fetch_spot_price(&self, currency: &str) -> Option<Decimal> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!(
                "https://api.coinbase.com/v2/prices/{}-USD/spot",
                currency
            ))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body: SpotPriceResponse = resp.json().ok()?;
        body.data
            .and_then(|d| d.amount)
            .and_then(|a| Decimal::from_str(&a).ok())
    }
}

impl BrokerProvider for CoinbaseProvider {
    fn kind(&self) -> BrokerKind {
        BrokerKind::Coinbase
    }

    fn is_available(&self) -> Result<()> {
        let path = "/api/v3/brokerage/accounts?limit=1";
        let ts = Self::timestamp();
        let sig = self.sign(ts, "GET", path, "");

        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}{}", self.base_url, path))
            .header("CB-ACCESS-KEY", &self.api_key)
            .header("CB-ACCESS-SIGN", &sig)
            .header("CB-ACCESS-TIMESTAMP", ts.to_string())
            .timeout(std::time::Duration::from_secs(10))
            .send()?;

        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!(
                "Coinbase API not reachable (status {}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )
        }
    }

    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>> {
        let path = "/api/v3/brokerage/accounts?limit=250";
        let ts = Self::timestamp();
        let sig = self.sign(ts, "GET", path, "");

        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}{}", self.base_url, path))
            .header("CB-ACCESS-KEY", &self.api_key)
            .header("CB-ACCESS-SIGN", &sig)
            .header("CB-ACCESS-TIMESTAMP", ts.to_string())
            .timeout(std::time::Duration::from_secs(15))
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Coinbase accounts fetch failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
        }

        let accounts: AccountsResponse = resp.json()?;
        let mut result = Vec::new();

        for account in &accounts.accounts {
            let currency = match &account.currency {
                Some(c) => c.clone(),
                None => continue,
            };
            let qty = account
                .available_balance
                .as_ref()
                .and_then(|b| b.value.as_deref())
                .and_then(|v| Decimal::from_str(v).ok())
                .unwrap_or(Decimal::ZERO);
            if qty.is_zero() {
                continue;
            }

            let avg_cost = if currency == "USD" || currency == "USDT" || currency == "USDC" {
                Decimal::ONE
            } else {
                self.fetch_spot_price(&currency).unwrap_or(Decimal::ZERO)
            };

            result.push(BrokerPosition {
                symbol: currency,
                quantity: qty,
                avg_cost,
                currency: "USD".to_string(),
                category: "crypto".to_string(),
            });
        }
        Ok(result)
    }
}
