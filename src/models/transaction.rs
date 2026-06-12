use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::asset::AssetCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxType {
    Buy,
    Sell,
    /// External capital entering the portfolio (deposit from outside).
    /// Position math treats it like a buy; flow analytics treat it as an
    /// external contribution, never a trade.
    #[serde(rename = "transfer_in")]
    TransferIn,
    /// External capital leaving the portfolio (withdrawal to outside).
    /// Position math treats it like a sell; flow analytics treat it as an
    /// external distribution, never a trade.
    #[serde(rename = "transfer_out")]
    TransferOut,
}

impl TxType {
    /// True for trade legs (buy/sell); false for external transfers.
    pub fn is_trade(self) -> bool {
        matches!(self, TxType::Buy | TxType::Sell)
    }

    /// True when the row increases the held quantity (buy / transfer_in).
    pub fn increases_quantity(self) -> bool {
        matches!(self, TxType::Buy | TxType::TransferIn)
    }
}

impl fmt::Display for TxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxType::Buy => write!(f, "buy"),
            TxType::Sell => write!(f, "sell"),
            TxType::TransferIn => write!(f, "transfer_in"),
            TxType::TransferOut => write!(f, "transfer_out"),
        }
    }
}

impl std::str::FromStr for TxType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "buy" => Ok(TxType::Buy),
            "sell" => Ok(TxType::Sell),
            "transfer_in" | "transfer-in" => Ok(TxType::TransferIn),
            "transfer_out" | "transfer-out" => Ok(TxType::TransferOut),
            _ => Err(anyhow::anyhow!(
                "Unknown tx type: {} (expected buy, sell, transfer_in, or transfer_out)",
                s
            )),
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
    pub paired_tx_id: Option<i64>,
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
