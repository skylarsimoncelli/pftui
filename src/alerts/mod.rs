pub mod engine;
pub mod rules;

use serde::{Deserialize, Serialize};
use std::fmt;

/// The type of alert rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertKind {
    /// Price alert: "GC=F above 5500"
    Price,
    /// Allocation drift: "gold allocation above 30%"
    Allocation,
    /// Indicator alert: "VIX above 25"
    Indicator,
    /// Technical alert evaluated from cached price history/indicators.
    Technical,
    /// Macro alert evaluated from cached regime/economic/sentiment data.
    Macro,
    /// Scenario alert: fires when a scenario probability shifts by ≥ threshold in a single update.
    Scenario,
    /// Ratio alert: fires when the price ratio between two assets crosses a threshold.
    /// Symbol format: "NUMERATOR/DENOMINATOR" (e.g. "GC=F/CL=F").
    Ratio,
}

impl fmt::Display for AlertKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlertKind::Price => write!(f, "price"),
            AlertKind::Allocation => write!(f, "allocation"),
            AlertKind::Indicator => write!(f, "indicator"),
            AlertKind::Technical => write!(f, "technical"),
            AlertKind::Macro => write!(f, "macro"),
            AlertKind::Scenario => write!(f, "scenario"),
            AlertKind::Ratio => write!(f, "ratio"),
        }
    }
}

impl std::str::FromStr for AlertKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "price" => Ok(AlertKind::Price),
            "allocation" => Ok(AlertKind::Allocation),
            "indicator" => Ok(AlertKind::Indicator),
            "technical" => Ok(AlertKind::Technical),
            "macro" => Ok(AlertKind::Macro),
            "scenario" => Ok(AlertKind::Scenario),
            "ratio" => Ok(AlertKind::Ratio),
            _ => Err(anyhow::anyhow!("Unknown alert kind: {}", s)),
        }
    }
}

/// Comparison direction for alert thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertDirection {
    Above,
    Below,
}

impl fmt::Display for AlertDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlertDirection::Above => write!(f, "above"),
            AlertDirection::Below => write!(f, "below"),
        }
    }
}

impl std::str::FromStr for AlertDirection {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "above" | ">" | ">=" => Ok(AlertDirection::Above),
            "below" | "<" | "<=" => Ok(AlertDirection::Below),
            _ => Err(anyhow::anyhow!(
                "Unknown direction: {} (expected 'above' or 'below')",
                s
            )),
        }
    }
}

/// Lifecycle status of an alert rule.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertStatus {
    /// Waiting for threshold to be crossed.
    #[default]
    Armed,
    /// Threshold was crossed.
    Triggered,
    /// User acknowledged the triggered alert.
    Acknowledged,
}

impl fmt::Display for AlertStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlertStatus::Armed => write!(f, "armed"),
            AlertStatus::Triggered => write!(f, "triggered"),
            AlertStatus::Acknowledged => write!(f, "acknowledged"),
        }
    }
}

impl std::str::FromStr for AlertStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "armed" => Ok(AlertStatus::Armed),
            "triggered" => Ok(AlertStatus::Triggered),
            "acknowledged" => Ok(AlertStatus::Acknowledged),
            _ => Err(anyhow::anyhow!("Unknown alert status: {}", s)),
        }
    }
}

/// A stored alert rule with its DB id and timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: i64,
    pub kind: AlertKind,
    pub symbol: String,
    pub direction: AlertDirection,
    #[serde(default)]
    pub condition: Option<String>,
    /// Threshold value as string (decimal). For allocation alerts, this is a percentage (e.g. "30" = 30%).
    pub threshold: String,
    pub status: AlertStatus,
    /// The original human-readable rule text, e.g. "GC=F above 5500".
    pub rule_text: String,
    #[serde(default)]
    pub recurring: bool,
    #[serde(default)]
    pub cooldown_minutes: i64,
    pub created_at: String,
    pub triggered_at: Option<String>,
}

impl fmt::Display for AlertRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_icon = match self.status {
            AlertStatus::Armed => "🟢",
            AlertStatus::Triggered => "🔴",
            AlertStatus::Acknowledged => "✅",
        };
        write!(f, "{} [{}] {}", status_icon, self.id, self.rule_text)
    }
}
