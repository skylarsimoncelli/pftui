use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::asset::AssetCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxType {
    Buy,
    Sell,
}

impl fmt::Display for TxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxType::Buy => write!(f, "buy"),
            TxType::Sell => write!(f, "sell"),
        }
    }
}

impl std::str::FromStr for TxType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "buy" => Ok(TxType::Buy),
            "sell" => Ok(TxType::Sell),
            _ => Err(anyhow::anyhow!("Unknown tx type: {} (expected buy or sell)", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub symbol: String,
    pub category: AssetCategory,
    pub tx_type: TxType,
    pub quantity: Decimal,
    pub price_per: Decimal,
    pub currency: String,
    pub date: String,
    pub notes: Option<String>,
    pub created_at: String,
}

impl Transaction {
    #[allow(dead_code)]
    #[allow(dead_code)]
    pub fn cost_basis(&self) -> Decimal {
        self.quantity * self.price_per
    }
}

pub struct NewTransaction {
    pub symbol: String,
    pub category: AssetCategory,
    pub tx_type: TxType,
    pub quantity: Decimal,
    pub price_per: Decimal,
    pub currency: String,
    pub date: String,
    pub notes: Option<String>,
}
