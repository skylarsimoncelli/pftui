use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetCategory {
    Equity,
    Crypto,
    Forex,
    Cash,
    Commodity,
    Fund,
}

impl AssetCategory {
    pub fn all() -> &'static [AssetCategory] {
        &[
            AssetCategory::Equity,
            AssetCategory::Crypto,
            AssetCategory::Forex,
            AssetCategory::Cash,
            AssetCategory::Commodity,
            AssetCategory::Fund,
        ]
    }

}

impl fmt::Display for AssetCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetCategory::Equity => write!(f, "equity"),
            AssetCategory::Crypto => write!(f, "crypto"),
            AssetCategory::Forex => write!(f, "forex"),
            AssetCategory::Cash => write!(f, "cash"),
            AssetCategory::Commodity => write!(f, "commodity"),
            AssetCategory::Fund => write!(f, "fund"),
        }
    }
}

impl std::str::FromStr for AssetCategory {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "equity" => Ok(AssetCategory::Equity),
            "crypto" => Ok(AssetCategory::Crypto),
            "forex" => Ok(AssetCategory::Forex),
            "cash" => Ok(AssetCategory::Cash),
            "commodity" => Ok(AssetCategory::Commodity),
            "fund" => Ok(AssetCategory::Fund),
            _ => Err(anyhow::anyhow!("Unknown category: {}", s)),
        }
    }
}

