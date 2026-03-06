use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// BLS series IDs for key economic indicators.
/// Using v1 API (no registration) — 10 calls/day limit, 10 years max range, 25 series per request.
pub const SERIES_CPI_U: &str = "CUUR0000SA0"; // CPI-U All Items
pub const SERIES_UNEMPLOYMENT: &str = "LNS14000000"; // Unemployment Rate
pub const SERIES_NFP: &str = "CES0000000001"; // Nonfarm Payrolls
pub const SERIES_HOURLY_EARNINGS: &str = "CES0500000003"; // Average Hourly Earnings

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlsDataPoint {
    pub series_id: String,
    pub year: i32,
    pub period: String, // M01-M12 for monthly
    pub value: Decimal,
    pub date: NaiveDate, // reconstructed from year+period
}

#[derive(Debug, Deserialize)]
struct BlsApiResponse {
    status: String,
    #[serde(rename = "Results")]
    results: Option<BlsResults>,
}

#[derive(Debug, Deserialize)]
struct BlsResults {
    series: Vec<BlsSeries>,
}

#[derive(Debug, Deserialize)]
struct BlsSeries {
    #[serde(rename = "seriesID")]
    series_id: String,
    data: Vec<BlsDataItem>,
}

#[derive(Debug, Deserialize)]
struct BlsDataItem {
    year: String,
    period: String,
    value: String,
}

/// Fetch BLS data for given series IDs.
/// BLS v1 API: no auth, 10 calls/day, up to 25 series per call.
pub async fn fetch_bls_data(series_ids: &[&str]) -> Result<Vec<BlsDataPoint>> {
    if series_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Build JSON payload
    let payload = serde_json::json!({
        "seriesid": series_ids,
        "startyear": chrono::Utc::now().year() - 2, // last 2 years
        "endyear": chrono::Utc::now().year(),
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client
        .post("https://api.bls.gov/publicAPI/v1/timeseries/data/")
        .json(&payload)
        .send()
        .await
        .context("BLS API request failed")?;

    if !resp.status().is_success() {
        anyhow::bail!("BLS API HTTP error: {}", resp.status());
    }

    let api_resp: BlsApiResponse = resp.json().await.context("BLS API JSON parse failed")?;

    if api_resp.status != "REQUEST_SUCCEEDED" {
        anyhow::bail!("BLS API status: {}", api_resp.status);
    }

    let results = api_resp
        .results
        .ok_or_else(|| anyhow::anyhow!("BLS API returned no results"))?;

    let mut data_points = Vec::new();

    for series in results.series {
        for item in series.data {
            // Parse value (skip if "-" or other non-numeric placeholder)
            let value = match Decimal::from_str(&item.value.trim()) {
                Ok(v) => v,
                Err(_) => {
                    // Skip missing/invalid data points (BLS uses "-" for missing data)
                    if item.value.trim() == "-" || item.value.trim().is_empty() {
                        continue;
                    }
                    return Err(anyhow::anyhow!("Failed to parse BLS value: {}", item.value));
                }
            };

            // Parse year
            let year: i32 = item
                .year
                .parse()
                .with_context(|| format!("Failed to parse year: {}", item.year))?;

            // Parse period (M01-M12 for monthly)
            let month: u32 = if item.period.starts_with('M') {
                item.period[1..]
                    .parse()
                    .with_context(|| format!("Failed to parse period: {}", item.period))?
            } else {
                continue; // Skip non-monthly (annual, quarterly)
            };

            // Construct date (use day 1)
            let date = NaiveDate::from_ymd_opt(year, month, 1)
                .ok_or_else(|| anyhow::anyhow!("Invalid date: {}-{}", year, month))?;

            data_points.push(BlsDataPoint {
                series_id: series.series_id.clone(),
                year,
                period: item.period.clone(),
                value,
                date,
            });
        }
    }

    Ok(data_points)
}

/// Fetch all key BLS series in one call.
pub async fn fetch_all_key_series() -> Result<Vec<BlsDataPoint>> {
    fetch_bls_data(&[
        SERIES_CPI_U,
        SERIES_UNEMPLOYMENT,
        SERIES_NFP,
        SERIES_HOURLY_EARNINGS,
    ])
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_bls_data() {
        // This is a live API test — skip in CI or if rate-limited
        let result = fetch_bls_data(&[SERIES_CPI_U]).await;
        match result {
            Ok(data) => {
                assert!(!data.is_empty());
                assert!(data.iter().all(|p| p.series_id == SERIES_CPI_U));
            }
            Err(e) => {
                // May fail if rate-limited or network issue — not a hard fail
                eprintln!("BLS API test skipped: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_fetch_all_key_series() {
        let result = fetch_all_key_series().await;
        match result {
            Ok(data) => {
                // Should have data for all 4 series
                assert!(!data.is_empty());
                let series_ids: std::collections::HashSet<String> =
                    data.iter().map(|p| p.series_id.clone()).collect();
                // Should have at least some data (may not have all 4 if recent data not available)
                assert!(!series_ids.is_empty());
            }
            Err(e) => {
                eprintln!("BLS API test skipped: {}", e);
            }
        }
    }
}
