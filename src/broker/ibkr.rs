use anyhow::Result;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

use super::{BrokerKind, BrokerPosition, BrokerProvider};

pub struct IbkrProvider {
    account_id: Option<String>,
    gateway_url: String,
}

#[derive(Debug, Deserialize)]
struct IbkrAccount {
    #[serde(alias = "accountId")]
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
struct IbkrPortfolioPosition {
    #[serde(alias = "contractDesc")]
    pub contract_desc: Option<String>,
    pub ticker: Option<String>,
    pub position: Option<f64>,
    #[serde(alias = "avgCost")]
    pub avg_cost: Option<f64>,
    pub currency: Option<String>,
    #[serde(alias = "assetClass")]
    pub asset_class: Option<String>,
}

impl IbkrProvider {
    pub fn new(account_id: Option<String>) -> Self {
        Self {
            account_id,
            gateway_url: "https://localhost:5000".to_string(),
        }
    }

    fn resolve_account_id(&self) -> Result<String> {
        if let Some(ref id) = self.account_id {
            return Ok(id.clone());
        }
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = client
            .get(format!("{}/v1/api/portfolio/accounts", self.gateway_url))
            .send()?;
        if !resp.status().is_success() {
            anyhow::bail!("IBKR gateway returned {}", resp.status());
        }
        let accounts: Vec<IbkrAccount> = resp.json()?;
        accounts
            .first()
            .map(|a| a.account_id.clone())
            .ok_or_else(|| anyhow::anyhow!("No IBKR accounts found"))
    }
}

impl BrokerProvider for IbkrProvider {
    fn kind(&self) -> BrokerKind {
        BrokerKind::Ibkr
    }

    fn is_available(&self) -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = client
            .get(format!(
                "{}/v1/api/iserver/auth/status",
                self.gateway_url
            ))
            .send()?;
        if resp.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!(
                "IBKR Client Portal Gateway not reachable (status {}). Make sure the gateway is running on localhost:5000.",
                resp.status()
            )
        }
    }

    fn fetch_positions(&self) -> Result<Vec<BrokerPosition>> {
        let account_id = self.resolve_account_id()?;
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let resp = client
            .get(format!(
                "{}/v1/api/portfolio/{}/positions/0",
                self.gateway_url, account_id
            ))
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!(
                "IBKR positions fetch failed ({}): {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
        }

        let positions: Vec<IbkrPortfolioPosition> = resp.json()?;
        let mut result = Vec::new();
        for p in positions {
            let symbol = p
                .ticker
                .or(p.contract_desc)
                .unwrap_or_default();
            if symbol.is_empty() {
                continue;
            }
            let qty_f = p.position.unwrap_or(0.0);
            let avg_f = p.avg_cost.unwrap_or(0.0);
            let qty = Decimal::from_str(&format!("{qty_f}")).unwrap_or(Decimal::ZERO);
            let avg = Decimal::from_str(&format!("{avg_f}")).unwrap_or(Decimal::ZERO);
            if qty.is_zero() {
                continue;
            }

            let category = match p.asset_class.as_deref() {
                Some("STK") => "equity",
                Some("CRYPTO") => "crypto",
                Some("CASH") | Some("FX") => "forex",
                Some("CMDTY") => "commodity",
                Some("FUT") | Some("OPT") | Some("FUND") => "fund",
                _ => "equity",
            };

            result.push(BrokerPosition {
                symbol,
                quantity: qty,
                avg_cost: avg,
                currency: p.currency.unwrap_or_else(|| "USD".to_string()),
                category: category.to_string(),
            });
        }
        Ok(result)
    }
}
