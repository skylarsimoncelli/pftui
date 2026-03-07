use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceQuote {
    pub symbol: String,
    pub price: Decimal,
    pub currency: String,
    pub source: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub date: String,
    pub close: Decimal,
    /// Daily trading volume (None if unavailable, e.g. ratio charts)
    #[serde(default)]
    pub volume: Option<u64>,
    /// Open price (None if unavailable)
    #[serde(default)]
    pub open: Option<Decimal>,
    /// High price (None if unavailable)
    #[serde(default)]
    pub high: Option<Decimal>,
    /// Low price (None if unavailable)
    #[serde(default)]
    pub low: Option<Decimal>,
}
