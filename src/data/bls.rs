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
    #[serde(default)]
    message: Vec<String>,
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

fn parse_bls_data_point(series_id: &str, item: &BlsDataItem) -> Option<BlsDataPoint> {
    // Parse value, handling BLS formatting like "278,802" and placeholders like "-"
    let raw_value = item.value.trim();
    if raw_value.is_empty() || raw_value == "-" {
        return None;
    }
    let cleaned_value = raw_value.replace(',', "");
    let value = Decimal::from_str(&cleaned_value).ok()?;

    let year: i32 = item.year.parse().ok()?;

    // Keep only regular monthly buckets (M01..M12). Skip M13 annual average.
    if !item.period.starts_with('M') {
        return None;
    }
    let month: u32 = item.period[1..].parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }

    let date = NaiveDate::from_ymd_opt(year, month, 1)?;
    Some(BlsDataPoint {
        series_id: series_id.to_string(),
        year,
        period: item.period.clone(),
        value,
        date,
    })
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

    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 429 {
        anyhow::bail!("BLS API rate limited (429). Free tier allows 10 calls/day. Skipping until next day.");
    }

    if !status.is_success() {
        anyhow::bail!("BLS API HTTP error: {}", status);
    }

    // BLS returns HTML instead of JSON when rate-limited (even with 200 status).
    // Peek at the body to detect this before JSON parsing fails.
    let body_text = resp.text().await.context("BLS API response read failed")?;
    if body_text.trim_start().starts_with('<') {
        anyhow::bail!("BLS API rate limited (returned HTML instead of JSON). Free tier allows 10 calls/day.");
    }

    let api_resp: BlsApiResponse = serde_json::from_str(&body_text)
        .context("BLS API JSON parse failed")?;

    if api_resp.status != "REQUEST_SUCCEEDED" {
        // Detect rate-limiting: BLS returns REQUEST_NOT_PROCESSED with a threshold message
        let is_rate_limited = api_resp.status == "REQUEST_NOT_PROCESSED"
            && api_resp.message.iter().any(|m| m.contains("threshold"));
        if is_rate_limited {
            anyhow::bail!("BLS API rate limited (daily threshold reached). Free tier allows 10 calls/day.");
        }
        anyhow::bail!("BLS API status: {}", api_resp.status);
    }

    let results = api_resp
        .results
        .ok_or_else(|| anyhow::anyhow!("BLS API returned no results"))?;

    let mut data_points = Vec::new();

    for series in results.series {
        for item in series.data {
            if let Some(point) = parse_bls_data_point(&series.series_id, &item) {
                data_points.push(point);
            }
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

    #[test]
    fn parse_skips_m13_annual_average() {
        let item = BlsDataItem {
            year: "2025".to_string(),
            period: "M13".to_string(),
            value: "4.1".to_string(),
        };
        assert!(parse_bls_data_point(SERIES_UNEMPLOYMENT, &item).is_none());
    }

    #[test]
    fn parse_handles_comma_separated_values() {
        let item = BlsDataItem {
            year: "2025".to_string(),
            period: "M01".to_string(),
            value: "278,802".to_string(),
        };
        let parsed = parse_bls_data_point(SERIES_NFP, &item).unwrap();
        assert_eq!(parsed.value, Decimal::from_str("278802").unwrap());
    }
}
