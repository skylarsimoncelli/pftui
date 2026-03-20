use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A named refresh source with its policy and dependency information.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RefreshSource {
    /// Unique identifier for this source (e.g. "yahoo_prices", "coingecko", "bls").
    pub name: &'static str,
    /// Human-readable label for display/JSON output.
    pub label: &'static str,
    /// Which DAG layer this source belongs to.
    pub layer: DagLayer,
    /// Runtime policy controlling freshness, retries, and timeouts.
    pub policy: SourcePolicy,
}

/// DAG layers define the execution order. Sources within the same layer
/// run concurrently (subject to concurrency limits). Layers execute
/// sequentially: layer N must complete before layer N+1 starts.
///
/// Layer 0 — Independent sources: FX, economy, news, predictions, sentiment,
///           calendar, COT, BLS, FRED, World Bank, COMEX, on-chain, FedWatch.
///           These have no dependency on other refresh sources.
///
/// Layer 1 — Prices: Yahoo (equities/indices/commodities), CoinGecko (crypto).
///           Independent of layer 0 but grouped separately because technicals
///           depend on them.
///
/// Layer 2 — Post-price analytics: technical snapshots, market structure levels,
///           technical signals, correlation snapshots, regime classification.
///           Depends on layer 1 (prices) completing.
///
/// Layer 3 — Portfolio + alerts: portfolio snapshots, cross-timeframe signals,
///           alert evaluation. Depends on layer 2 (analytics).
///
/// Layer 4 — Cleanup: prune old data. Runs after everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[allow(dead_code)]
pub enum DagLayer {
    /// Layer 0: independent data sources (economy, news, predictions, etc.)
    Independent = 0,
    /// Layer 1: price fetching (Yahoo, CoinGecko, FX)
    Prices = 1,
    /// Layer 2: analytics that depend on fresh prices
    PostPrice = 2,
    /// Layer 3: portfolio snapshots and alerts
    Portfolio = 3,
    /// Layer 4: cleanup old data
    Cleanup = 4,
}

/// Per-source execution policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SourcePolicy {
    /// Minimum interval between refreshes. If the source was refreshed
    /// more recently than this, it will be skipped.
    pub min_refresh_interval: Duration,
    /// Maximum number of retry attempts on failure.
    pub max_retries: u32,
    /// Base delay for exponential backoff between retries (milliseconds).
    pub backoff_base_ms: u64,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Maximum concurrency for this specific source (e.g. Yahoo = 2).
    pub max_concurrency: u32,
}

impl Default for SourcePolicy {
    fn default() -> Self {
        Self {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 2,
            backoff_base_ms: 500,
            timeout: Duration::from_secs(30),
            max_concurrency: 4,
        }
    }
}

/// Result of executing a single source in the refresh pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceResult {
    /// Source name
    pub name: String,
    /// Human-readable label
    pub label: String,
    /// Outcome status
    pub status: SourceStatus,
    /// Number of items/symbols updated (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items_updated: Option<usize>,
    /// Duration of this source's execution in milliseconds
    pub duration_ms: u64,
    /// If skipped, the reason
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// If skipped due to freshness, the age in minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_minutes: Option<i64>,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Human-readable detail line (the existing ✓/✗/⊘ output)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Outcome status for a refresh source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Ok,
    Skipped,
    Failed,
    Deferred,
}

/// Aggregate result of the entire refresh pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    /// Total wall-clock duration in milliseconds
    pub duration_ms: u64,
    /// Per-source results
    pub sources: Vec<SourceResult>,
    /// Sources that failed
    pub failures: Vec<SourceResult>,
    /// Total items updated across all sources
    pub total_items_updated: usize,
}

impl RefreshResult {
    pub fn new() -> Self {
        Self {
            duration_ms: 0,
            sources: Vec::new(),
            failures: Vec::new(),
            total_items_updated: 0,
        }
    }

    pub fn add(&mut self, result: SourceResult) {
        if result.status == SourceStatus::Failed {
            self.failures.push(result.clone());
        }
        if let Some(n) = result.items_updated {
            self.total_items_updated += n;
        }
        self.sources.push(result);
    }

    pub fn finalize(&mut self, total_elapsed: Duration) {
        self.duration_ms = total_elapsed.as_millis() as u64;
    }
}

// Note: Parallel execution is implemented directly in refresh.rs using
// tokio::join! for concurrent async fetches. The DAG layer model here
// provides the structural framework; refresh.rs implements the execution.

/// Build the default source catalog with policies derived from the
/// existing hardcoded freshness thresholds.
#[allow(dead_code)]
pub fn default_source_policies() -> HashMap<&'static str, SourcePolicy> {
    let mut policies = HashMap::new();

    // FX rates - fetch on every price run
    policies.insert(
        "fx_rates",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 2,
            backoff_base_ms: 500,
            timeout: Duration::from_secs(20),
            max_concurrency: 1,
        },
    );

    // Yahoo prices - rate limited, sequential or max 2
    policies.insert(
        "yahoo_prices",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(120),
            max_retries: 1,
            backoff_base_ms: 200,
            timeout: Duration::from_secs(20),
            max_concurrency: 2,
        },
    );

    // CoinGecko - batch endpoint, single request
    policies.insert(
        "coingecko",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(120),
            max_retries: 2,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(20),
            max_concurrency: 1,
        },
    );

    // Price history backfill
    policies.insert(
        "price_history",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 1,
            backoff_base_ms: 200,
            timeout: Duration::from_secs(20),
            max_concurrency: 2,
        },
    );

    // News (RSS)
    policies.insert(
        "news_rss",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(10 * 60),
            max_retries: 1,
            backoff_base_ms: 500,
            timeout: Duration::from_secs(30),
            max_concurrency: 4,
        },
    );

    // News (Brave)
    policies.insert(
        "news_brave",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(4 * 60 * 60),
            max_retries: 1,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // Predictions (Polymarket)
    policies.insert(
        "predictions",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(60 * 60),
            max_retries: 2,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // FedWatch
    policies.insert(
        "fedwatch",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(60 * 60),
            max_retries: 2,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // COT (CFTC)
    policies.insert(
        "cot",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(7 * 24 * 60 * 60),
            max_retries: 2,
            backoff_base_ms: 2000,
            timeout: Duration::from_secs(60),
            max_concurrency: 1,
        },
    );

    // Sentiment
    policies.insert(
        "sentiment",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(60 * 60),
            max_retries: 1,
            backoff_base_ms: 500,
            timeout: Duration::from_secs(20),
            max_concurrency: 1,
        },
    );

    // Calendar
    policies.insert(
        "calendar",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(24 * 60 * 60),
            max_retries: 1,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // Economy
    policies.insert(
        "economy",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(60 * 60),
            max_retries: 1,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // FRED
    policies.insert(
        "fred",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(6 * 60 * 60),
            max_retries: 1,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // BLS
    policies.insert(
        "bls",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(30 * 24 * 60 * 60),
            max_retries: 1,
            backoff_base_ms: 2000,
            timeout: Duration::from_secs(60),
            max_concurrency: 1,
        },
    );

    // World Bank
    policies.insert(
        "worldbank",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(7 * 24 * 60 * 60),
            max_retries: 1,
            backoff_base_ms: 2000,
            timeout: Duration::from_secs(60),
            max_concurrency: 1,
        },
    );

    // COMEX
    policies.insert(
        "comex",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(6 * 60 * 60),
            max_retries: 1,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // On-chain
    policies.insert(
        "onchain",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(60 * 60),
            max_retries: 1,
            backoff_base_ms: 1000,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // Analytics (post-price)
    policies.insert(
        "analytics",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 0,
            backoff_base_ms: 0,
            timeout: Duration::from_secs(60),
            max_concurrency: 1,
        },
    );

    // Technical snapshots (post-price)
    policies.insert(
        "technical_snapshots",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 0,
            backoff_base_ms: 0,
            timeout: Duration::from_secs(60),
            max_concurrency: 1,
        },
    );

    // Alerts
    policies.insert(
        "alerts",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 0,
            backoff_base_ms: 0,
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
        },
    );

    // Cleanup
    policies.insert(
        "cleanup",
        SourcePolicy {
            min_refresh_interval: Duration::from_secs(300),
            max_retries: 0,
            backoff_base_ms: 0,
            timeout: Duration::from_secs(60),
            max_concurrency: 1,
        },
    );

    policies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_layer_ordering() {
        assert!(DagLayer::Independent < DagLayer::Prices);
        assert!(DagLayer::Prices < DagLayer::PostPrice);
        assert!(DagLayer::PostPrice < DagLayer::Portfolio);
        assert!(DagLayer::Portfolio < DagLayer::Cleanup);
    }

    #[test]
    fn source_status_serialize() {
        let json = serde_json::to_string(&SourceStatus::Ok).unwrap();
        assert_eq!(json, "\"ok\"");
        let json = serde_json::to_string(&SourceStatus::Skipped).unwrap();
        assert_eq!(json, "\"skipped\"");
    }

    #[test]
    fn refresh_result_tracks_failures() {
        let mut result = RefreshResult::new();
        result.add(SourceResult {
            name: "test_ok".to_string(),
            label: "Test OK".to_string(),
            status: SourceStatus::Ok,
            items_updated: Some(10),
            duration_ms: 100,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
        result.add(SourceResult {
            name: "test_fail".to_string(),
            label: "Test Fail".to_string(),
            status: SourceStatus::Failed,
            items_updated: None,
            duration_ms: 50,
            reason: None,
            age_minutes: None,
            error: Some("connection refused".to_string()),
            detail: None,
        });
        result.add(SourceResult {
            name: "test_skip".to_string(),
            label: "Test Skip".to_string(),
            status: SourceStatus::Skipped,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: Some(5),
            error: None,
            detail: None,
        });

        assert_eq!(result.sources.len(), 3);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.total_items_updated, 10);
    }

    #[test]
    fn default_policies_cover_all_sources() {
        let policies = default_source_policies();
        assert!(policies.contains_key("yahoo_prices"));
        assert!(policies.contains_key("coingecko"));
        assert!(policies.contains_key("news_rss"));
        assert!(policies.contains_key("news_brave"));
        assert!(policies.contains_key("predictions"));
        assert!(policies.contains_key("bls"));
        assert!(policies.contains_key("fred"));
        assert!(policies.contains_key("cot"));
        assert!(policies.contains_key("sentiment"));
        assert!(policies.contains_key("calendar"));
        assert!(policies.contains_key("economy"));
        assert!(policies.contains_key("worldbank"));
        assert!(policies.contains_key("comex"));
        assert!(policies.contains_key("onchain"));
        assert!(policies.contains_key("analytics"));
        assert!(policies.contains_key("alerts"));
        assert!(policies.contains_key("cleanup"));
    }

    #[test]
    fn yahoo_has_low_concurrency() {
        let policies = default_source_policies();
        let yahoo = &policies["yahoo_prices"];
        assert!(yahoo.max_concurrency <= 2);
    }

    #[test]
    fn source_result_json_serialization() {
        let result = SourceResult {
            name: "bls".to_string(),
            label: "BLS".to_string(),
            status: SourceStatus::Skipped,
            items_updated: None,
            duration_ms: 0,
            reason: Some("fresh".to_string()),
            age_minutes: Some(12),
            error: None,
            detail: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["name"], "bls");
        assert_eq!(json["status"], "skipped");
        assert_eq!(json["reason"], "fresh");
        assert_eq!(json["age_minutes"], 12);
        // items_updated should be absent (skip_serializing_if)
        assert!(json.get("items_updated").is_none());
        // error should be absent
        assert!(json.get("error").is_none());
    }

    #[test]
    fn refresh_result_json_structure() {
        let mut result = RefreshResult::new();
        result.add(SourceResult {
            name: "yahoo_prices".to_string(),
            label: "Yahoo Prices".to_string(),
            status: SourceStatus::Ok,
            items_updated: Some(80),
            duration_ms: 1200,
            reason: None,
            age_minutes: None,
            error: None,
            detail: None,
        });
        result.finalize(std::time::Duration::from_millis(3200));

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["duration_ms"], 3200);
        assert_eq!(json["total_items_updated"], 80);
        assert!(json["sources"].is_array());
        assert!(json["failures"].is_array());
        assert_eq!(json["failures"].as_array().unwrap().len(), 0);
    }
}
