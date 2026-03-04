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
//! - PPIACO: Producer Price Index - All Commodities (monthly)
//! - UNRATE: Unemployment Rate (monthly)
//! - T10Y2Y: 10-Year Treasury Minus 2-Year Treasury (daily, yield curve spread)

use anyhow::{bail, Result};
use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

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
        id: "PPIACO",
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
    Monthly,
}

/// A single observation from FRED.
#[derive(Debug, Clone)]
pub struct FredObservation {
    pub series_id: String,
    pub date: String,
    pub value: Decimal,
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

/// Build a reqwest client with proper User-Agent.
fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(Into::into)
}

/// Fetch the latest observations for a single FRED series.
///
/// Returns up to `limit` most recent observations (default 10).
/// The API key is required — pass it from config.
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

    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        bail!(
            "FRED API returned status {} for series {}",
            resp.status(),
            series_id
        );
    }

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
pub async fn fetch_latest(
    api_key: &str,
    series_id: &str,
) -> Result<Option<FredObservation>> {
    let observations = fetch_series(api_key, series_id, 5).await?;
    Ok(observations.into_iter().next())
}

/// Fetch historical observations for a FRED series within a date range.
///
/// Useful for sparklines and trend analysis.
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

    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        bail!(
            "FRED API returned status {} for series {} history",
            resp.status(),
            series_id
        );
    }

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
        Frequency::Monthly => age_days > 45,
    }
}

/// Look up series metadata by ID.
pub fn series_by_id(id: &str) -> Option<&'static FredSeries> {
    FRED_SERIES.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_series_lookup() {
        let s = series_by_id("DGS10").unwrap();
        assert_eq!(s.name, "10-Year Treasury Yield");
        assert_eq!(s.frequency, Frequency::Daily);

        let s = series_by_id("CPIAUCSL").unwrap();
        assert_eq!(s.unit, "index");
        assert_eq!(s.frequency, Frequency::Monthly);

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
    fn test_is_stale_bad_date() {
        assert!(is_stale("not-a-date", Frequency::Daily));
        assert!(is_stale("", Frequency::Monthly));
    }

    #[test]
    fn test_fred_series_count() {
        assert_eq!(FRED_SERIES.len(), 6);
    }

    #[test]
    fn test_all_series_have_valid_metadata() {
        for s in FRED_SERIES {
            assert!(!s.id.is_empty());
            assert!(!s.name.is_empty());
            assert!(!s.unit.is_empty());
        }
    }
}
