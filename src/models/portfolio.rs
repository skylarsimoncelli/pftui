use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::position::Position;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct PortfolioSummary {
    pub total_value: Decimal,
    pub total_cost: Decimal,
    pub total_gain: Decimal,
    pub total_gain_pct: Decimal,
    pub positions: Vec<Position>,
    pub base_currency: String,
}
