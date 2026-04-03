use anyhow::{Context, Result};
use chrono::Datelike;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// World Bank indicator codes for key structural macro data.
/// API: free, no auth, unlimited calls.
pub const INDICATOR_GDP_GROWTH: &str = "NY.GDP.MKTP.KD.ZG"; // GDP growth (annual %)
pub const INDICATOR_DEBT_GDP: &str = "GC.DOD.TOTL.GD.ZS"; // Central gov't debt to GDP (%)
pub const INDICATOR_CURRENT_ACCOUNT: &str = "BN.CAB.XOKA.GD.ZS"; // Current account balance (% of GDP)
pub const INDICATOR_RESERVES: &str = "FI.RES.TOTL.CD"; // Total reserves (current USD)

/// ISO country codes for tracked economies.
pub const COUNTRY_US: &str = "USA";
pub const COUNTRY_CHINA: &str = "CHN";
pub const COUNTRY_INDIA: &str = "IND";
pub const COUNTRY_RUSSIA: &str = "RUS";
pub const COUNTRY_BRAZIL: &str = "BRA";
pub const COUNTRY_SOUTH_AFRICA: &str = "ZAF";
pub const COUNTRY_UK: &str = "GBR";
pub const COUNTRY_EU: &str = "EUU"; // European Union

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldBankDataPoint {
    pub country_code: String,
    pub country_name: String,
    pub indicator_code: String,
    pub indicator_name: String,
    pub year: i32,
    pub value: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
struct WorldBankApiResponse {
    #[serde(rename = "page")]
    _page: i32,
    #[serde(rename = "pages")]
    _pages: i32,
    #[serde(rename = "per_page")]
    _per_page: i32,
    #[serde(rename = "total")]
    _total: i32,
}

#[derive(Debug, Deserialize)]
struct WorldBankDataItem {
    #[serde(rename = "countryiso3code")]
    country_code: String,
    country: WorldBankCountry,
    indicator: WorldBankIndicator,
    date: String,
    value: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct WorldBankCountry {
    value: String, // country name
}

#[derive(Debug, Deserialize)]
struct WorldBankIndicator {
    id: String,
    value: String, // indicator name
}

/// Fetch World Bank data for given countries and indicator.
/// API endpoint: https://api.worldbank.org/v2/country/{countries}/indicator/{indicator}?format=json&date={start}:{end}
pub async fn fetch_worldbank_indicator(
    countries: &[&str],
    indicator: &str,
) -> Result<Vec<WorldBankDataPoint>> {
    if countries.is_empty() {
        return Ok(Vec::new());
    }

    let current_year = chrono::Utc::now().year();
    let start_year = current_year - 5; // Last 5 years of data

    let countries_str = countries.join(";");
    let url = format!(
        "https://api.worldbank.org/v2/country/{}/indicator/{}?format=json&date={}:{}",
        countries_str, indicator, start_year, current_year
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build HTTP client")?;

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to fetch World Bank data")?;

    if !resp.status().is_success() {
        anyhow::bail!("World Bank API returned status {}", resp.status());
    }

    let body = resp.text().await.context("Failed to read response body")?;

    // World Bank API returns JSON array: [metadata, data_array]
    let json: serde_json::Value =
        serde_json::from_str(&body).context("Failed to parse World Bank JSON")?;

    let data_array = json
        .as_array()
        .and_then(|arr| arr.get(1))
        .and_then(|v| v.as_array())
        .context("Invalid World Bank API response structure")?;

    let mut results = Vec::new();

    for item_value in data_array {
        let item: WorldBankDataItem = serde_json::from_value(item_value.clone())
            .context("Failed to parse World Bank data item")?;

        let year = item.date.parse::<i32>().context("Failed to parse year")?;

        let value = item.value.and_then(|v| {
            // Convert to Decimal, rounding to 2 decimal places
            Decimal::from_f64_retain(v)
        });

        results.push(WorldBankDataPoint {
            country_code: item.country_code,
            country_name: item.country.value,
            indicator_code: item.indicator.id,
            indicator_name: item.indicator.value,
            year,
            value,
        });
    }

    Ok(results)
}

/// Fetch all key indicators for tracked countries.
/// Returns a flat vec of all data points across indicators and countries.
pub async fn fetch_all_indicators() -> Result<Vec<WorldBankDataPoint>> {
    let countries = vec![
        COUNTRY_US,
        COUNTRY_CHINA,
        COUNTRY_INDIA,
        COUNTRY_RUSSIA,
        COUNTRY_BRAZIL,
        COUNTRY_SOUTH_AFRICA,
        COUNTRY_UK,
        COUNTRY_EU,
    ];

    let indicators = vec![
        INDICATOR_GDP_GROWTH,
        INDICATOR_DEBT_GDP,
        INDICATOR_CURRENT_ACCOUNT,
        INDICATOR_RESERVES,
    ];

    let mut all_data = Vec::new();

    for indicator in indicators {
        match fetch_worldbank_indicator(&countries, indicator).await {
            Ok(data) => all_data.extend(data),
            Err(e) => {
                eprintln!(
                    "Warning: Failed to fetch World Bank indicator {}: {}",
                    indicator, e
                );
            }
        }
    }

    Ok(all_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Hits live World Bank API — run with `cargo test -- --ignored`
    async fn test_fetch_gdp_growth() {
        let data = fetch_worldbank_indicator(&[COUNTRY_US, COUNTRY_CHINA], INDICATOR_GDP_GROWTH)
            .await
            .unwrap();
        assert!(!data.is_empty());
        assert!(data.iter().any(|d| d.country_code == COUNTRY_US));
    }

    #[tokio::test]
    #[ignore] // Hits live World Bank API — run with `cargo test -- --ignored`
    async fn test_fetch_all_indicators() {
        let data = fetch_all_indicators().await.unwrap();
        assert!(!data.is_empty());
        // Should have data for multiple countries
        let unique_countries: std::collections::HashSet<_> =
            data.iter().map(|d| d.country_code.as_str()).collect();
        assert!(unique_countries.len() >= 2);
        // Should have data for multiple indicators
        let unique_indicators: std::collections::HashSet<_> =
            data.iter().map(|d| d.indicator_code.as_str()).collect();
        assert!(unique_indicators.len() >= 2);
    }
}
