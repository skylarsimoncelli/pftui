use anyhow::Result;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

use super::{BrokerKind, BrokerPosition, BrokerProvider};

pub struct Trading212Provider {
    api_key: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct T212Position {
    pub ticker: String,
    #[serde(alias = "currentPrice")]
    pub _current_price: f64,
    pub quantity: f64,
    #[serde(alias = "averagePrice")]
    pub average_price: f64,
    #[serde(alias = "ppl", default)]
    pub _ppl: f64,
}

impl Trading212Provider {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            base_url: "https://live.trading212.com".to_string(),
        }
    }
}

impl BrokerProvider for Trading212Provider {
    fn kind(&self) -> BrokerKind {
        BrokerKind::Trading212
    }

    fn is_available(&self) -> Result<()> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}/api/v0/equity/account/info", self.base_url))
            .header("Authorization", &self.api_key)
            .timeout(std::time::Duration::from_secs(10))
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!(
                "Trading212 API returned status {}: {}",
                resp.status(),
                resp.text().unwrap_or_default()
            )
        }
    }

    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}/api/v0/equity/portfolio", self.base_url))
            .header("Authorization", &self.api_key)
            .timeout(std::time::Duration::from_secs(15))
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "Trading212 portfolio fetch failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
        }

        let positions: Vec<T212Position> = resp.json()?;
        let mut result = Vec::new();
        for p in positions {
            let qty = Decimal::from_str(&format!("{}", p.quantity)).unwrap_or(Decimal::ZERO);
            let avg = Decimal::from_str(&format!("{}", p.average_price)).unwrap_or(Decimal::ZERO);
            if qty.is_zero() {
                continue;
            }
            result.push(BrokerPosition {
                symbol: p.ticker.clone(),
                quantity: qty,
                avg_cost: avg,
                currency: "USD".to_string(),
                category: "equity".to_string(),
            });
        }
        Ok(result)
    }
}
