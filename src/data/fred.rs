//! FRED (Federal Reserve Economic Data) API client.
//!
//! Fetches macroeconomic indicators from the St. Louis Fed's FRED API.
//! Uses the public JSON endpoint which requires an API key.
//! Register at: https://fred.stlouisfed.org/docs/api/api_key.html
//!
//! Supported series:
//! - DGS10: 10-Year Treasury Constant Maturity Rate (daily)
//! - FEDFUNDS: Effective Federal Funds Rate (monthly)
//! - CPIAUCSL: Consumer Price Index for All Urban Consumers (monthly)
//! - PPIFIS: Producer Price Index - Final Demand (monthly)
//! - UNRATE: Unemployment Rate (monthly)
//! - T10Y2Y: 10-Year Treasury Minus 2-Year Treasury (daily, yield curve spread)
//! - RSAFS: Advance Retail Sales (monthly)
//! - INDPRO: Industrial Production Index (monthly)
//! - DGORDER: Manufacturers' New Orders: Durable Goods (monthly, GDP leading indicator)
//! - UMCSENT: University of Michigan Consumer Sentiment (monthly)

use anyhow::{bail, Result};
use chrono::{NaiveDate, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

/// Known FRED series IDs we track.
pub const FRED_SERIES: &[FredSeries] = &[
    FredSeries {
        id: "DGS10",
        name: "10-Year Treasury Yield",
        unit: "%",
        frequency: Frequency::Daily,
    },
    FredSeries {
        id: "FEDFUNDS",
        name: "Federal Funds Rate",
        unit: "%",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "CPIAUCSL",
        name: "Consumer Price Index (CPI)",
        unit: "index",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "PPIFIS",
        name: "Producer Price Index (PPI)",
        unit: "index",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "UNRATE",
        name: "Unemployment Rate",
        unit: "%",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "T10Y2Y",
        name: "10Y-2Y Yield Spread",
        unit: "%",
        frequency: Frequency::Daily,
    },
    FredSeries {
        id: "GDP",
        name: "Gross Domestic Product",
        unit: "billions_usd",
        frequency: Frequency::Quarterly,
    },
    FredSeries {
        id: "GDPNOW",
        name: "Atlanta Fed GDPNow Estimate",
        unit: "% annualized",
        frequency: Frequency::Weekly,
    },
    FredSeries {
        id: "A191RL1Q225SBEA",
        name: "Real GDP Growth Rate",
        unit: "% annualized",
        frequency: Frequency::Quarterly,
    },
    FredSeries {
        id: "PCE",
        name: "Personal Consumption Expenditures",
        unit: "billions_usd",
        frequency: Frequency::Monthly,
    },
    // NOTE: ISM Manufacturing PMI (formerly NAPM) is proprietary and not
    // available on FRED. PMI data comes from Brave web search or BLS instead.
    // The "NAPM" series was removed because it returns HTTP 400 from FRED.
    FredSeries {
        id: "JTSJOL",
        name: "JOLTS Job Openings",
        unit: "thousands",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "ICSA",
        name: "Initial Jobless Claims",
        unit: "claims",
        frequency: Frequency::Weekly,
    },
    FredSeries {
        id: "PAYEMS",
        name: "Nonfarm Payrolls",
        unit: "thousands",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "RSAFS",
        name: "Retail Sales",
        unit: "millions_usd",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "INDPRO",
        name: "Industrial Production Index",
        unit: "index",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "DGORDER",
        name: "Durable Goods Orders",
        unit: "millions_usd",
        frequency: Frequency::Monthly,
    },
    FredSeries {
        id: "UMCSENT",
        name: "Consumer Sentiment (UMich)",
        unit: "index",
        frequency: Frequency::Monthly,
    },
];

/// Metadata for a FRED series.
pub struct FredSeries {
    pub id: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub frequency: Frequency,
}

/// Update frequency of a FRED series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
}

/// A single observation from FRED.
#[derive(Debug, Clone)]
pub struct FredObservation {
    pub series_id: String,
    pub date: String,
    pub value: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EconomicSurprise {
    pub series_id: String,
    pub event_date: String,
    pub expected: Decimal,
    pub actual: Decimal,
    pub surprise_pct: Decimal,
}

/// Raw JSON response from FRED API.
#[derive(Debug, Deserialize)]
struct FredResponse {
    observations: Vec<RawObservation>,
}

#[derive(Debug, Deserialize)]
struct RawObservation {
    date: String,
    value: String,
}

const FRED_BASE_URL: &str = "https://api.stlouisfed.org/fred/series/observations";

/// Maximum number of retry attempts for FRED API calls.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (doubled each retry).
const BASE_RETRY_DELAY_MS: u64 = 500;

/// Build a reqwest client with proper User-Agent.
fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(Into::into)
}

/// Perform an HTTP GET with exponential backoff retry.
///
/// Retries on 5xx errors and network failures. Does NOT retry on 4xx
/// (client errors like bad API key or invalid series).
async fn get_with_retry(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let mut last_err = None;
    for attempt in 0..MAX_RETRIES {
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(resp);
                }
                // Don't retry client errors (4xx) — they won't resolve
                if status.is_client_error() {
                    bail!("FRED API returned client error {} (not retryable)", status);
                }
                // Server error (5xx) — retry
                last_err = Some(format!("FRED API returned server error {}", status));
            }
            Err(e) => {
                last_err = Some(format!("FRED request failed: {}", e));
            }
        }
        if attempt + 1 < MAX_RETRIES {
            let delay = Duration::from_millis(BASE_RETRY_DELAY_MS * 2u64.pow(attempt));
            sleep(delay).await;
        }
    }
    bail!(
        "{} (after {} retries)",
        last_err.unwrap_or_else(|| "unknown error".to_string()),
        MAX_RETRIES
    );
}

/// Fetch the latest observations for a single FRED series.
///
/// Returns up to `limit` most recent observations (default 10).
/// The API key is required — pass it from config.
/// Uses exponential backoff retry on server errors and network failures.
pub async fn fetch_series(
    api_key: &str,
    series_id: &str,
    limit: u32,
) -> Result<Vec<FredObservation>> {
    let client = build_client()?;

    let url = format!(
        "{}?series_id={}&api_key={}&file_type=json&sort_order=desc&limit={}",
        FRED_BASE_URL, series_id, api_key, limit
    );

    let resp = get_with_retry(&client, &url).await?;

    let body: FredResponse = resp.json().await?;

    let mut observations = Vec::new();
    for obs in body.observations {
        // FRED uses "." as a placeholder for missing/unavailable data
        if obs.value == "." {
            continue;
        }
        let value = match Decimal::from_str(&obs.value) {
            Ok(v) => v,
            Err(_) => continue,
        };
        observations.push(FredObservation {
            series_id: series_id.to_string(),
            date: obs.date.clone(),
            value,
        });
    }

    Ok(observations)
}

/// Fetch the latest observation for a single FRED series.
///
/// Returns the most recent non-missing value, or None if unavailable.
pub async fn fetch_latest(api_key: &str, series_id: &str) -> Result<Option<FredObservation>> {
    let observations = fetch_series(api_key, series_id, 5).await?;
    Ok(observations.into_iter().next())
}

/// Fetch historical observations for a FRED series within a date range.
///
/// Useful for sparklines and trend analysis.
/// Uses exponential backoff retry on server errors and network failures.
pub async fn fetch_history(
    api_key: &str,
    series_id: &str,
    days_back: u32,
) -> Result<Vec<FredObservation>> {
    let client = build_client()?;

    let end = Utc::now().date_naive();
    let start = end - chrono::Duration::days(days_back as i64);

    let url = format!(
        "{}?series_id={}&api_key={}&file_type=json&sort_order=asc&observation_start={}&observation_end={}",
        FRED_BASE_URL,
        series_id,
        api_key,
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    );

    let resp = get_with_retry(&client, &url).await?;

    let body: FredResponse = resp.json().await?;

    let mut observations = Vec::new();
    for obs in body.observations {
        if obs.value == "." {
            continue;
        }
        let value = match Decimal::from_str(&obs.value) {
            Ok(v) => v,
            Err(_) => continue,
        };
        observations.push(FredObservation {
            series_id: series_id.to_string(),
            date: obs.date.clone(),
            value,
        });
    }

    Ok(observations)
}

/// Fetch latest values for all tracked FRED series.
///
/// Returns a vec of the most recent observation for each series.
/// Silently skips series that fail (logs could be added later).
pub async fn fetch_all_latest(api_key: &str) -> Result<Vec<FredObservation>> {
    let mut results = Vec::new();

    for series in FRED_SERIES {
        match fetch_latest(api_key, series.id).await {
            Ok(Some(obs)) => results.push(obs),
            Ok(None) => {} // no data available, skip
            Err(_) => {}   // API error, skip silently
        }
    }

    Ok(results)
}

/// Check if a date string is stale based on frequency.
///
/// Daily series: stale if older than 3 days (weekends/holidays).
/// Monthly series: stale if older than 45 days.
pub fn is_stale(date_str: &str, frequency: Frequency) -> bool {
    let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
        return true; // can't parse = treat as stale
    };

    let today = Utc::now().date_naive();
    let age_days = (today - date).num_days();

    match frequency {
        Frequency::Daily => age_days > 3,
        Frequency::Weekly => age_days > 10,
        Frequency::Monthly => age_days > 45,
        Frequency::Quarterly => age_days > 120,
    }
}

/// Look up series metadata by ID.
pub fn series_by_id(id: &str) -> Option<&'static FredSeries> {
    FRED_SERIES.iter().find(|s| s.id == id)
}

pub fn detect_surprise(observations: &[FredObservation]) -> Option<EconomicSurprise> {
    if observations.len() < 6 {
        return None;
    }

    let latest = &observations[0];
    let previous = &observations[1];
    let latest_value = latest.value.to_f64()?;
    let previous_value = previous.value.to_f64()?;
    let latest_change = latest_value - previous_value;

    let mut historical_changes = Vec::new();
    for pair in observations.windows(2).skip(1) {
        let newer = pair[0].value.to_f64()?;
        let older = pair[1].value.to_f64()?;
        historical_changes.push(newer - older);
    }

    if historical_changes.len() < 4 {
        return None;
    }

    let mean = historical_changes.iter().sum::<f64>() / historical_changes.len() as f64;
    let variance = historical_changes
        .iter()
        .map(|change| {
            let delta = change - mean;
            delta * delta
        })
        .sum::<f64>()
        / historical_changes.len() as f64;
    let std_dev = variance.sqrt();

    if std_dev <= f64::EPSILON {
        if latest_change.abs() <= f64::EPSILON {
            return None;
        }
    } else if latest_change.abs() <= std_dev {
        return None;
    }

    let denominator = previous_value.abs();
    let surprise_pct = if denominator <= f64::EPSILON {
        Decimal::ZERO
    } else {
        Decimal::from_f64_retain((latest_change / denominator) * 100.0)?
    };

    Some(EconomicSurprise {
        series_id: latest.series_id.clone(),
        event_date: latest.date.clone(),
        expected: previous.value,
        actual: latest.value,
        surprise_pct: surprise_pct.round_dp(2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_series_lookup() {
        let s = series_by_id("DGS10").unwrap();
        assert_eq!(s.name, "10-Year Treasury Yield");
        assert_eq!(s.frequency, Frequency::Daily);

        let s = series_by_id("CPIAUCSL").unwrap();
        assert_eq!(s.unit, "index");
        assert_eq!(s.frequency, Frequency::Monthly);

        let s = series_by_id("GDP").unwrap();
        assert_eq!(s.frequency, Frequency::Quarterly);

        assert!(series_by_id("BOGUS").is_none());
    }

    #[test]
    fn test_is_stale_daily() {
        // A date far in the past should be stale
        assert!(is_stale("2020-01-01", Frequency::Daily));

        // Today should not be stale
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        assert!(!is_stale(&today, Frequency::Daily));
    }

    #[test]
    fn test_is_stale_monthly() {
        // A date far in the past should be stale
        assert!(is_stale("2020-01-01", Frequency::Monthly));

        // Today should not be stale
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        assert!(!is_stale(&today, Frequency::Monthly));
    }

    #[test]
    fn test_is_stale_weekly_and_quarterly() {
        let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
        assert!(!is_stale(&today, Frequency::Weekly));
        assert!(!is_stale(&today, Frequency::Quarterly));
    }

    #[test]
    fn test_is_stale_bad_date() {
        assert!(is_stale("not-a-date", Frequency::Daily));
        assert!(is_stale("", Frequency::Monthly));
    }

    #[test]
    fn test_fred_series_count() {
        assert_eq!(FRED_SERIES.len(), 17);
    }

    #[test]
    fn test_all_series_have_valid_metadata() {
        for s in FRED_SERIES {
            assert!(!s.id.is_empty());
            assert!(!s.name.is_empty());
            assert!(!s.unit.is_empty());
        }
    }

    #[test]
    fn detects_large_surprise_from_history() {
        let observations = vec![
            FredObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2026-03-01".to_string(),
                value: dec!(115),
            },
            FredObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(108),
            },
            FredObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2026-01-01".to_string(),
                value: dec!(107),
            },
            FredObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2025-12-01".to_string(),
                value: dec!(106),
            },
            FredObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2025-11-01".to_string(),
                value: dec!(105),
            },
            FredObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2025-10-01".to_string(),
                value: dec!(104),
            },
        ];

        let surprise = detect_surprise(&observations).unwrap();
        assert_eq!(surprise.expected, dec!(108));
        assert_eq!(surprise.actual, dec!(115));
        assert_eq!(surprise.event_date, "2026-03-01");
    }

    #[test]
    fn ignores_normal_move_from_history() {
        let observations = vec![
            FredObservation {
                series_id: "UNRATE".to_string(),
                date: "2026-03-01".to_string(),
                value: dec!(4.18),
            },
            FredObservation {
                series_id: "UNRATE".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(4.1),
            },
            FredObservation {
                series_id: "UNRATE".to_string(),
                date: "2026-01-01".to_string(),
                value: dec!(4.22),
            },
            FredObservation {
                series_id: "UNRATE".to_string(),
                date: "2025-12-01".to_string(),
                value: dec!(4.05),
            },
            FredObservation {
                series_id: "UNRATE".to_string(),
                date: "2025-11-01".to_string(),
                value: dec!(4.17),
            },
            FredObservation {
                series_id: "UNRATE".to_string(),
                date: "2025-10-01".to_string(),
                value: dec!(4.02),
            },
        ];

        assert!(detect_surprise(&observations).is_none());
    }

    #[test]
    fn retry_constants_are_sane() {
        // Use const blocks to satisfy clippy::assertions_on_constants
        const {
            assert!(MAX_RETRIES >= 2, "Should retry at least twice");
            assert!(MAX_RETRIES <= 5, "Should not retry excessively");
            assert!(BASE_RETRY_DELAY_MS >= 100, "Base delay should be >= 100ms");
            assert!(BASE_RETRY_DELAY_MS <= 2000, "Base delay should be <= 2s");
        }
        // Runtime check: max total delay is reasonable (500 + 1000 + 2000 = 3500ms)
        let max_total: u64 = (0..MAX_RETRIES).map(|i| BASE_RETRY_DELAY_MS * 2u64.pow(i)).sum();
        assert!(max_total <= 10_000, "Total retry delay should be under 10s");
    }

    #[test]
    fn test_is_stale_boundary_daily() {
        // 3 days ago should NOT be stale (weekends/holidays buffer)
        let three_days_ago = (Utc::now().date_naive() - chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();
        assert!(!is_stale(&three_days_ago, Frequency::Daily));

        // 4 days ago SHOULD be stale
        let four_days_ago = (Utc::now().date_naive() - chrono::Duration::days(4))
            .format("%Y-%m-%d")
            .to_string();
        assert!(is_stale(&four_days_ago, Frequency::Daily));
    }

    #[test]
    fn test_is_stale_boundary_weekly() {
        // 10 days ago should NOT be stale
        let ten_days_ago = (Utc::now().date_naive() - chrono::Duration::days(10))
            .format("%Y-%m-%d")
            .to_string();
        assert!(!is_stale(&ten_days_ago, Frequency::Weekly));

        // 11 days ago SHOULD be stale
        let eleven_days_ago = (Utc::now().date_naive() - chrono::Duration::days(11))
            .format("%Y-%m-%d")
            .to_string();
        assert!(is_stale(&eleven_days_ago, Frequency::Weekly));
    }

    #[test]
    fn test_is_stale_boundary_quarterly() {
        // 120 days ago should NOT be stale
        let within = (Utc::now().date_naive() - chrono::Duration::days(120))
            .format("%Y-%m-%d")
            .to_string();
        assert!(!is_stale(&within, Frequency::Quarterly));

        // 121 days ago SHOULD be stale
        let beyond = (Utc::now().date_naive() - chrono::Duration::days(121))
            .format("%Y-%m-%d")
            .to_string();
        assert!(is_stale(&beyond, Frequency::Quarterly));
    }
}
