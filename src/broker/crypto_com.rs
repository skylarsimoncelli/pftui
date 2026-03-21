use anyhow::Result;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{BrokerKind, BrokerPosition, BrokerProvider};

pub struct CryptoComProvider {
    api_key: String,
    secret_key: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct CdcResponse {
    pub code: i64,
    pub result: Option<CdcResult>,
}

#[derive(Debug, Deserialize)]
struct CdcResult {
    pub data: Option<Vec<CdcBalance>>,
}

#[derive(Debug, Deserialize)]
struct CdcBalance {
    pub currency: String,
    pub balance: f64,
    pub _available: f64,
}

impl CryptoComProvider {
    pub fn new(api_key: &str, secret_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            base_url: "https://api.crypto.com/exchange/v1".to_string(),
        }
    }

    fn sign(&self, method: &str, id: u64, nonce: u64, params: &serde_json::Value) -> String {
        // Crypto.com signing: HMAC-SHA256 of method + id + api_key + sorted_params + nonce
        let params_str = if params.is_object() {
            let map = params.as_object().unwrap();
            let mut sorted_keys: Vec<&String> = map.keys().collect();
            sorted_keys.sort();
            sorted_keys
                .iter()
                .map(|k| format!("{}{}", k, map[*k]))
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };

        let sig_payload = format!("{}{}{}{}{}", method, id, self.api_key, params_str, nonce);

        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret_key.as_bytes()).expect("HMAC key");
        mac.update(sig_payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn nonce() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis() as u64
    }

    fn fetch_ticker_prices() -> HashMap<String, Decimal> {
        let mut prices = HashMap::new();
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get("https://api.crypto.com/exchange/v1/public/get-tickers")
            .timeout(std::time::Duration::from_secs(10))
            .send();
        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            _ => return prices,
        };

        #[derive(Deserialize)]
        struct TickerResponse {
            result: Option<TickerResult>,
        }
        #[derive(Deserialize)]
        struct TickerResult {
            data: Option<Vec<TickerData>>,
        }
        #[derive(Deserialize)]
        struct TickerData {
            i: Option<String>, // instrument name e.g. "BTC_USDT"
            a: Option<f64>,    // last trade price
        }

        if let Ok(body) = resp.json::<TickerResponse>() {
            if let Some(result) = body.result {
                for t in result.data.unwrap_or_default() {
                    if let (Some(instrument), Some(price)) = (t.i, t.a) {
                        if instrument.ends_with("_USDT") || instrument.ends_with("_USD") {
                            let asset = instrument.split('_').next().unwrap_or("");
                            if !asset.is_empty() {
                                if let Ok(d) = Decimal::from_str(&format!("{price}")) {
                                    prices.insert(asset.to_string(), d);
                                }
                            }
                        }
                    }
                }
            }
        }
        prices
    }
}

impl BrokerProvider for CryptoComProvider {
    fn kind(&self) -> BrokerKind {
        BrokerKind::CryptoCom
    }

    fn is_available(&self) -> Result<()> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get("https://api.crypto.com/exchange/v1/public/get-tickers")
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("Crypto.com API not reachable (status {})", resp.status())
        }
    }

    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>> {
        let method = "private/user-balance";
        let id = 1u64;
        let nonce = Self::nonce();
        let params = serde_json::json!({});
        let sig = self.sign(method, id, nonce, &params);

        let body = serde_json::json!({
            "id": id,
            "method": method,
            "api_key": self.api_key,
            "params": params,
            "sig": sig,
            "nonce": nonce,
        });

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}/{}", self.base_url, method))
            .json(&body)
            .timeout(std::time::Duration::from_secs(15))
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Crypto.com balance fetch failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
        }

        let cdc_resp: CdcResponse = resp.json()?;
        if cdc_resp.code != 0 {
            anyhow::bail!("Crypto.com API error code: {}", cdc_resp.code);
        }

        let balances = cdc_resp.result.and_then(|r| r.data).unwrap_or_default();

        let prices = Self::fetch_ticker_prices();

        let mut result = Vec::new();
        for b in &balances {
            let qty = Decimal::from_str(&format!("{}", b.balance)).unwrap_or(Decimal::ZERO);
            if qty.is_zero() {
                continue;
            }

            let avg_cost = if b.currency == "USDT" || b.currency == "USD" || b.currency == "USDC" {
                Decimal::ONE
            } else {
                prices.get(&b.currency).copied().unwrap_or(Decimal::ZERO)
            };

            result.push(BrokerPosition {
                symbol: b.currency.clone(),
                quantity: qty,
                avg_cost,
                currency: "USD".to_string(),
                category: "crypto".to_string(),
            });
        }
        Ok(result)
    }
}
