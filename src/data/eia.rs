//! EIA (U.S. Energy Information Administration) API client.
//!
//! Fetches weekly petroleum status report data:
//! - Crude oil commercial inventories (excluding SPR)
//! - Strategic Petroleum Reserve (SPR) levels
//! - Total crude oil stocks (commercial + SPR)
//! - Weekly inventory change
//!
//! API: https://api.eia.gov/v2/
//! Requires a free API key: https://www.eia.gov/opendata/register.php
//!
//! Series IDs:
//! - PET.WCESTUS1.W — Weekly U.S. ending stocks excluding SPR (thousand barrels)
//! - PET.WCSSTUS1.W — Weekly U.S. ending stocks of crude in SPR (thousand barrels)
//! - PET.WTTSTUS1.W — Weekly U.S. total crude oil stocks (thousand barrels)

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// EIA petroleum series we track.
pub const EIA_SERIES: &[EiaSeries] = &[
    EiaSeries {
        series_id: "PET.WCESTUS1.W",
        name: "Commercial Crude Inventories",
        description: "Weekly U.S. ending stocks of crude oil excluding SPR",
        unit: "thousand barrels",
    },
    EiaSeries {
        series_id: "PET.WCSSTUS1.W",
        name: "Strategic Petroleum Reserve",
        description: "Weekly U.S. ending stocks of crude oil in SPR",
        unit: "thousand barrels",
    },
    EiaSeries {
        series_id: "PET.WTTSTUS1.W",
        name: "Total Crude Stocks",
        description: "Weekly U.S. total ending stocks of crude oil (commercial + SPR)",
        unit: "thousand barrels",
    },
];

/// Metadata for a tracked EIA series.
#[derive(Debug, Clone)]
pub struct EiaSeries {
    pub series_id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub unit: &'static str,
}

/// A single EIA observation (one week's data point).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EiaObservation {
    pub series_id: String,
    pub period: String, // YYYY-MM-DD (Friday)
    pub value: f64,     // thousand barrels
}

/// EIA API v2 response envelope.
#[derive(Debug, Deserialize)]
struct EiaApiResponse {
    response: Option<EiaResponseBody>,
}

#[derive(Debug, Deserialize)]
struct EiaResponseBody {
    data: Option<Vec<EiaDataRow>>,
}

#[derive(Debug, Deserialize)]
struct EiaDataRow {
    period: Option<String>,
    value: Option<serde_json::Value>,
}

/// Fetch the latest N observations for a given EIA series.
///
/// This is a blocking call -- run in a background thread if called from TUI.
pub fn fetch_series(api_key: &str, series_id: &str, limit: usize) -> Result<Vec<EiaObservation>> {
    // EIA v2 API: the series_id like "PET.WCESTUS1.W" maps to
    // /v2/petroleum/stoc/wstk/data/ with frequency=weekly and data=value
    // But the simpler approach is the v2 seriesid query parameter.
    let url = format!(
        "https://api.eia.gov/v2/seriesid/{}?api_key={}&frequency=weekly&data[0]=value&sort[0][column]=period&sort[0][direction]=desc&length={}",
        series_id, api_key, limit
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp_text = client
        .get(&url)
        .send()
        .map_err(|e| anyhow!("EIA API request failed: {}", e))?
        .text()
        .map_err(|e| anyhow!("EIA API response read failed: {}", e))?;

    let envelope: EiaApiResponse = serde_json::from_str(&resp_text)
        .map_err(|e| anyhow!("Failed to parse EIA response: {} (body: {})", e, &resp_text[..resp_text.len().min(200)]))?;

    let body = envelope
        .response
        .ok_or_else(|| anyhow!("EIA API returned no response body"))?;

    let rows = body
        .data
        .ok_or_else(|| anyhow!("EIA API returned no data array"))?;

    let mut observations = Vec::new();
    for row in rows {
        let period = row.period.unwrap_or_default();
        if period.is_empty() {
            continue;
        }
        let value = match row.value {
            Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(serde_json::Value::String(s)) => s.parse::<f64>().unwrap_or(0.0),
            _ => continue,
        };
        observations.push(EiaObservation {
            series_id: series_id.to_string(),
            period,
            value,
        });
    }

    Ok(observations)
}

/// Fetch latest observation for a series.
pub fn fetch_latest(api_key: &str, series_id: &str) -> Result<Option<EiaObservation>> {
    let obs = fetch_series(api_key, series_id, 1)?;
    Ok(obs.into_iter().next())
}

/// Compute weekly change from the last two observations.
pub fn weekly_change(observations: &[EiaObservation]) -> Option<f64> {
    if observations.len() < 2 {
        return None;
    }
    Some(observations[0].value - observations[1].value)
}

/// Compute 5-year average from historical observations.
/// Assumes observations are sorted most-recent-first.
pub fn five_year_average(observations: &[EiaObservation]) -> Option<f64> {
    // ~260 weekly observations = 5 years
    let samples: Vec<f64> = observations.iter().take(260).map(|o| o.value).collect();
    if samples.is_empty() {
        return None;
    }
    Some(samples.iter().sum::<f64>() / samples.len() as f64)
}

/// Deviation from the 5-year average (in thousand barrels).
pub fn deviation_from_avg(current: f64, avg: f64) -> f64 {
    current - avg
}

/// Deviation from the 5-year average as a percentage.
pub fn deviation_pct(current: f64, avg: f64) -> f64 {
    if avg == 0.0 {
        return 0.0;
    }
    ((current - avg) / avg) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_change_computes_diff() {
        let obs = vec![
            EiaObservation {
                series_id: "TEST".to_string(),
                period: "2026-03-14".to_string(),
                value: 440_000.0,
            },
            EiaObservation {
                series_id: "TEST".to_string(),
                period: "2026-03-07".to_string(),
                value: 438_000.0,
            },
        ];
        let change = weekly_change(&obs);
        assert_eq!(change, Some(2_000.0));
    }

    #[test]
    fn weekly_change_none_for_single_observation() {
        let obs = vec![EiaObservation {
            series_id: "TEST".to_string(),
            period: "2026-03-14".to_string(),
            value: 440_000.0,
        }];
        assert_eq!(weekly_change(&obs), None);
    }

    #[test]
    fn five_year_average_computes_mean() {
        let obs: Vec<EiaObservation> = (0..10)
            .map(|i| EiaObservation {
                series_id: "TEST".to_string(),
                period: format!("2026-01-{:02}", i + 1),
                value: 400_000.0 + (i as f64 * 1_000.0),
            })
            .collect();
        let avg = five_year_average(&obs).unwrap();
        assert!((avg - 404_500.0).abs() < 0.01);
    }

    #[test]
    fn deviation_pct_correct() {
        let pct = deviation_pct(440_000.0, 420_000.0);
        assert!((pct - 4.7619).abs() < 0.01);
    }

    #[test]
    fn deviation_pct_zero_avg() {
        assert_eq!(deviation_pct(100.0, 0.0), 0.0);
    }

    #[test]
    fn eia_series_have_correct_ids() {
        assert_eq!(EIA_SERIES.len(), 3);
        assert!(EIA_SERIES.iter().any(|s| s.series_id == "PET.WCESTUS1.W"));
        assert!(EIA_SERIES.iter().any(|s| s.series_id == "PET.WCSSTUS1.W"));
        assert!(EIA_SERIES.iter().any(|s| s.series_id == "PET.WTTSTUS1.W"));
    }
}
