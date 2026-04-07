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
use regex::Regex;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::sleep;

use crate::price::yahoo;

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

#[derive(Debug, Clone, PartialEq)]
pub struct GdpNowSnapshot {
    pub quarter: String,
    pub updated_date: String,
    pub next_update: Option<String>,
    pub value: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BeaGdpReleaseContext {
    pub current_release_date: String,
    pub next_release_date: Option<String>,
    pub release_label: Option<String>,
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
const DGS10_STALE_THRESHOLD_DAYS: i64 = 2;
const GDPNOW_STALE_THRESHOLD_DAYS: i64 = 7;
const GDPNOW_PAGE_URL: &str = "https://www.atlantafed.org/research-and-data/data/gdpnow";
const GDPNOW_COMMENTARY_URL: &str =
    "https://www.atlantafed.org/research-and-data/data/gdpnow/current-and-past-gdpnow-commentaries";
const BEA_GDP_URL: &str = "https://www.bea.gov/data/gdp/gross-domestic-product";

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

pub fn is_series_stale(series_id: &str, date_str: &str) -> bool {
    let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
        return true;
    };

    let today = Utc::now().date_naive();
    let age_days = (today - date).num_days();

    if series_id == "DGS10" || series_id == "DGS10_YAHOO" {
        return age_days > DGS10_STALE_THRESHOLD_DAYS;
    }
    if series_id == "GDPNOW" || series_id == "GDPNOW_WEB" {
        return age_days > GDPNOW_STALE_THRESHOLD_DAYS;
    }

    series_by_id(series_id)
        .map(|series| is_stale(date_str, series.frequency))
        .unwrap_or(true)
}

fn normalize_tnx_quote_to_percent(value: Decimal) -> Decimal {
    if value >= Decimal::TEN {
        (value / Decimal::TEN).round_dp(3)
    } else {
        value.round_dp(3)
    }
}

pub async fn fetch_dgs10_yahoo_fallback() -> Result<FredObservation> {
    let quote = yahoo::fetch_price("^TNX").await?;
    Ok(FredObservation {
        series_id: "DGS10_YAHOO".to_string(),
        date: Utc::now().date_naive().format("%Y-%m-%d").to_string(),
        value: normalize_tnx_quote_to_percent(quote.price),
    })
}

pub async fn fetch_gdpnow_web_fallback() -> Result<FredObservation> {
    let client = build_client()?;
    let main_html = get_with_retry(&client, GDPNOW_PAGE_URL).await?.text().await?;

    let snapshot = if let Some(snapshot) = parse_gdpnow_main_page(&main_html) {
        snapshot
    } else {
        let commentary_html = get_with_retry(&client, GDPNOW_COMMENTARY_URL)
            .await?
            .text()
            .await?;
        parse_gdpnow_commentary_page(&commentary_html)
            .ok_or_else(|| anyhow::anyhow!("GDPNow web page parse failed"))?
    };

    Ok(FredObservation {
        series_id: "GDPNOW_WEB".to_string(),
        date: snapshot.updated_date,
        value: snapshot.value.round_dp(1),
    })
}

pub fn fetch_bea_gdp_release_context() -> Result<BeaGdpReleaseContext> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(Duration::from_secs(15))
        .build()?;
    let html = client
        .get(BEA_GDP_URL)
        .send()?
        .error_for_status()?
        .text()?;
    parse_bea_gdp_page(&html).ok_or_else(|| anyhow::anyhow!("BEA GDP page parse failed"))
}

fn parse_gdpnow_main_page(html: &str) -> Option<GdpNowSnapshot> {
    static VALUE_RE: OnceLock<Regex> = OnceLock::new();
    static QUARTER_RE: OnceLock<Regex> = OnceLock::new();
    static UPDATED_RE: OnceLock<Regex> = OnceLock::new();
    static NEXT_RE: OnceLock<Regex> = OnceLock::new();

    let value_re = VALUE_RE.get_or_init(|| {
        Regex::new(r#"data-value">\s*([+-]?\d+(?:\.\d+)?)%"#).expect("valid GDPNow value regex")
    });
    let quarter_re = QUARTER_RE.get_or_init(|| {
        Regex::new(r#"Latest GDPNow Estimate for\s+(\d{4}:Q[1-4])"#)
            .expect("valid GDPNow quarter regex")
    });
    let updated_re = UPDATED_RE.get_or_init(|| {
        Regex::new(r#"Updated:</strong>\s*([A-Za-z]+ \d{1,2}, \d{4})"#)
            .expect("valid GDPNow updated regex")
    });
    let next_re = NEXT_RE.get_or_init(|| {
        Regex::new(r#"Next update:</strong>\s*([A-Za-z]+ \d{1,2}, \d{4})"#)
            .expect("valid GDPNow next-update regex")
    });

    let value = Decimal::from_str(value_re.captures(html)?.get(1)?.as_str()).ok()?;
    let quarter = quarter_re.captures(html)?.get(1)?.as_str().to_string();
    let updated_date = normalize_long_date(updated_re.captures(html)?.get(1)?.as_str())?;
    let next_update = next_re
        .captures(html)
        .and_then(|captures| captures.get(1))
        .and_then(|m| normalize_long_date(m.as_str()));

    Some(GdpNowSnapshot {
        quarter,
        updated_date,
        next_update,
        value,
    })
}

fn parse_gdpnow_commentary_page(html: &str) -> Option<GdpNowSnapshot> {
    static ENTRY_RE: OnceLock<Regex> = OnceLock::new();
    static NEXT_RE: OnceLock<Regex> = OnceLock::new();

    let entry_re = ENTRY_RE.get_or_init(|| {
        Regex::new(
            r#"The GDPNow model estimate for real GDP growth .*? in the ([a-z]+) quarter of (\d{4}) is <strong>\s*([+-]?\d+(?:\.\d+)?) percent</strong> on ([A-Za-z]+ \d{1,2})"#,
        )
        .expect("valid GDPNow commentary regex")
    });
    let next_re = NEXT_RE.get_or_init(|| {
        Regex::new(r#"The next(?:&nbsp;| )GDPNow(?:&nbsp;| )update is <strong>\s*(?:[A-Za-z]+,\s*)?([A-Za-z]+ \d{1,2})"#)
            .expect("valid GDPNow commentary next-update regex")
    });

    let captures = entry_re.captures(html)?;
    let quarter_word = captures.get(1)?.as_str();
    let year = captures.get(2)?.as_str();
    let value = Decimal::from_str(captures.get(3)?.as_str()).ok()?;
    let updated_date = normalize_month_day_with_year(captures.get(4)?.as_str(), year)?;
    let next_update = next_re
        .captures(html)
        .and_then(|c| c.get(1))
        .and_then(|m| normalize_month_day_with_year(m.as_str(), year));

    Some(GdpNowSnapshot {
        quarter: format!("{}:Q{}", year, quarter_word_to_number(quarter_word)?),
        updated_date,
        next_update,
        value,
    })
}

fn parse_bea_gdp_page(html: &str) -> Option<BeaGdpReleaseContext> {
    static CURRENT_RE: OnceLock<Regex> = OnceLock::new();
    static NEXT_RE: OnceLock<Regex> = OnceLock::new();
    static LABEL_RE: OnceLock<Regex> = OnceLock::new();

    let current_re = CURRENT_RE.get_or_init(|| {
        Regex::new(r#"Current Release:</strong>&nbsp;:\s*([A-Za-z]+ \d{1,2}, \d{4})"#)
            .expect("valid BEA current-release regex")
    });
    let next_re = NEXT_RE.get_or_init(|| {
        Regex::new(r#"Next Release:</strong>&nbsp;:\s*([A-Za-z]+ \d{1,2}, \d{4})"#)
            .expect("valid BEA next-release regex")
    });
    let label_re = LABEL_RE.get_or_init(|| {
        Regex::new(r#"field-subtitle[^>]*>\s*([^<]*GDP[^<]*)</div>"#)
            .expect("valid BEA release-label regex")
    });

    Some(BeaGdpReleaseContext {
        current_release_date: normalize_long_date(current_re.captures(html)?.get(1)?.as_str())?,
        next_release_date: next_re
            .captures(html)
            .and_then(|captures| captures.get(1))
            .and_then(|m| normalize_long_date(m.as_str())),
        release_label: label_re
            .captures(html)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().to_string()),
    })
}

fn normalize_long_date(input: &str) -> Option<String> {
    for fmt in ["%B %d, %Y", "%B %-d, %Y", "%B %e, %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(input.trim(), fmt) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    None
}

fn normalize_month_day_with_year(input: &str, year: &str) -> Option<String> {
    normalize_long_date(&format!("{}, {}", input.trim().trim_end_matches('.'), year))
}

fn quarter_word_to_number(word: &str) -> Option<u8> {
    match word.to_ascii_lowercase().as_str() {
        "first" => Some(1),
        "second" => Some(2),
        "third" => Some(3),
        "fourth" => Some(4),
        _ => None,
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
    fn test_is_series_stale_uses_stricter_dgs10_threshold() {
        let two_days_ago = (Utc::now().date_naive() - chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let three_days_ago = (Utc::now().date_naive() - chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();
        assert!(!is_series_stale("DGS10", &two_days_ago));
        assert!(is_series_stale("DGS10", &three_days_ago));
    }

    #[test]
    fn test_normalize_tnx_quote_to_percent_handles_yahoo_scale() {
        assert_eq!(normalize_tnx_quote_to_percent(Decimal::from_str("42.57").unwrap()), Decimal::from_str("4.257").unwrap());
        assert_eq!(normalize_tnx_quote_to_percent(Decimal::from_str("4.257").unwrap()), Decimal::from_str("4.257").unwrap());
    }

    #[test]
    fn parse_gdpnow_main_page_extracts_latest_card() {
        let html = r#"
        <div class="card-content">
            <p class="data-value">1.6%</p>
            <p><strong>Latest GDPNow Estimate for 2026:Q1</strong></p>
            <p><strong>Updated:</strong> April 02, 2026</p>
            <p><strong>Next update:</strong> April 07, 2026</p>
        </div>
        "#;
        let parsed = parse_gdpnow_main_page(html).expect("main page should parse");
        assert_eq!(parsed.quarter, "2026:Q1");
        assert_eq!(parsed.updated_date, "2026-04-02");
        assert_eq!(parsed.next_update.as_deref(), Some("2026-04-07"));
        assert_eq!(parsed.value, dec!(1.6));
    }

    #[test]
    fn parse_gdpnow_commentary_page_extracts_latest_entry() {
        let html = r#"
        <p>The GDPNow model estimate for real GDP growth (seasonally adjusted annual rate) in the first quarter of 2026 is <strong>1.6 percent</strong> on April 2, <strong>down from 1.9 percent</strong> on April 1.</p>
        <p><em>The next GDPNow update is <strong> Tuesday, April 7</strong>. Please see the "Release Dates" tab for a list of upcoming releases.</em></p>
        "#;
        let parsed =
            parse_gdpnow_commentary_page(html).expect("commentary page should parse");
        assert_eq!(parsed.quarter, "2026:Q1");
        assert_eq!(parsed.updated_date, "2026-04-02");
        assert_eq!(parsed.next_update.as_deref(), Some("2026-04-07"));
        assert_eq!(parsed.value, dec!(1.6));
    }

    #[test]
    fn parse_bea_gdp_page_extracts_release_dates() {
        let html = r#"
        <div class="field field--name-field-subtitle field--type-string field--label-hidden field--item">GDP (Second Estimate), 4th Quarter and Year 2025</div>
        <div class="field field--name-field-description field--type-text-long field--label-hidden field--item"><ul><li><strong>Current Release:</strong>&nbsp;: March 13, 2026</li><li><strong>Next Release:</strong>&nbsp;: April 9, 2026</li></ul></div>
        "#;
        let parsed = parse_bea_gdp_page(html).expect("BEA page should parse");
        assert_eq!(parsed.current_release_date, "2026-03-13");
        assert_eq!(parsed.next_release_date.as_deref(), Some("2026-04-09"));
        assert_eq!(
            parsed.release_label.as_deref(),
            Some("GDP (Second Estimate), 4th Quarter and Year 2025")
        );
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
