use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::asset::AssetCategory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Allocation {
    pub id: i64,
    pub symbol: String,
    pub category: AssetCategory,
    pub allocation_pct: Decimal,
    pub created_at: String,
}
