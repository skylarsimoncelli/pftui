//! CFTC Commitments of Traders (COT) API client.
//!
//! Fetches positioning data from the CFTC's Socrata Open Data API.
//! Uses the Disaggregated Futures-Only report (Traders in Financial Futures).
//! Data updates every Friday around 3:30 PM ET for the prior Tuesday.
//!
//! API: https://publicreporting.cftc.gov/resource/<dataset>.json
//! No authentication required. Rate limit: ~1000 req/hour per IP.
//!
//! Supported contracts:
//! - Gold (088691): COMEX Gold Futures
//! - Silver (084691): COMEX Silver Futures
//! - WTI Crude Oil (067411): NYMEX WTI Light Sweet Crude Oil
//! - Bitcoin (133741): CME Bitcoin Futures

use anyhow::{anyhow, Result};
use serde::Deserialize;

/// CFTC contract codes we track and their pftui symbol mappings.
pub const COT_CONTRACTS: &[CotContract] = &[
    CotContract {
        cftc_code: "088691",
        symbol: "GC=F",
        name: "Gold Futures",
        category: "Metals",
    },
    CotContract {
        cftc_code: "084691",
        symbol: "SI=F",
        name: "Silver Futures",
        category: "Metals",
    },
    CotContract {
        cftc_code: "067411",
        symbol: "CL=F",
        name: "WTI Crude Oil Futures",
        category: "Energy",
    },
    CotContract {
        cftc_code: "133741",
        symbol: "BTC",
        name: "Bitcoin Futures",
        category: "Crypto",
    },
];

/// Metadata for a tracked COT contract.
#[derive(Debug, Clone)]
pub struct CotContract {
    pub cftc_code: &'static str,
    pub symbol: &'static str,
    pub name: &'static str,
    pub category: &'static str,
}

/// A single COT report observation.
#[derive(Debug, Clone)]
pub struct CotReport {
    pub cftc_code: String,
    pub report_date: String, // YYYY-MM-DD
    pub open_interest: i64,
    pub managed_money_long: i64,
    pub managed_money_short: i64,
    pub managed_money_net: i64,
    pub commercial_long: i64,
    pub commercial_short: i64,
    pub commercial_net: i64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct CotInterpretation {
    pub percentile_1y: f64,
    pub percentile_3y: f64,
    pub z_score: f64,
    pub extreme: bool,
}

impl CotReport {
    /// Net change in managed money positioning vs previous week.
    pub fn managed_money_change(&self, prev: &CotReport) -> i64 {
        self.managed_money_net - prev.managed_money_net
    }

    /// Net change in commercial positioning vs previous week.
    pub fn commercial_change(&self, prev: &CotReport) -> i64 {
        self.commercial_net - prev.commercial_net
    }
}

/// Socrata API response record (disaggregated futures).
#[derive(Debug, Deserialize)]
struct SocrataRecord {
    #[serde(rename = "cftc_contract_market_code")]
    cftc_code: String,
    #[serde(rename = "report_date_as_yyyy_mm_dd")]
    report_date: String,
    #[serde(rename = "open_interest_all")]
    open_interest: String,
    #[serde(rename = "noncomm_positions_long_all")]
    managed_money_long: String,
    #[serde(rename = "noncomm_positions_short_all")]
    managed_money_short: String,
    #[serde(rename = "comm_positions_long_all")]
    commercial_long: String,
    #[serde(rename = "comm_positions_short_all")]
    commercial_short: String,
}

/// Fetch latest COT report for a specific contract.
///
/// Uses the Disaggregated Futures-Only report (TFF = Traders in Financial Futures).
/// Endpoint: https://publicreporting.cftc.gov/resource/jun7-fc8e.json
/// Query: ?cftc_contract_market_code=<code>&$order=report_date DESC&$limit=1
///
/// This is a blocking call — run in a background thread if called from TUI.
pub fn fetch_latest_report(cftc_code: &str) -> Result<CotReport> {
    let url = format!(
        "https://publicreporting.cftc.gov/resource/jun7-fc8e.json?cftc_contract_market_code={}&$order=report_date_as_yyyy_mm_dd%20DESC&$limit=1",
        cftc_code
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp: Vec<SocrataRecord> = client
        .get(&url)
        .send()
        .map_err(|e| anyhow!("CFTC API request failed: {}", e))?
        .json()
        .map_err(|e| anyhow!("Failed to parse CFTC response: {}", e))?;

    let record = resp
        .first()
        .ok_or_else(|| anyhow!("No COT data found for contract code {}", cftc_code))?;

    parse_record(record)
}

/// Fetch historical COT reports for a contract (last N weeks).
///
/// This is a blocking call — run in a background thread if called from TUI.
pub fn fetch_historical_reports(cftc_code: &str, weeks: usize) -> Result<Vec<CotReport>> {
    let url = format!(
        "https://publicreporting.cftc.gov/resource/jun7-fc8e.json?cftc_contract_market_code={}&$order=report_date_as_yyyy_mm_dd%20DESC&$limit={}",
        cftc_code, weeks
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp: Vec<SocrataRecord> = client
        .get(&url)
        .send()
        .map_err(|e| anyhow!("CFTC API request failed: {}", e))?
        .json()
        .map_err(|e| anyhow!("Failed to parse CFTC response: {}", e))?;

    resp.iter().map(parse_record).collect::<Result<Vec<_>>>()
}

/// Parse a Socrata API record into CotReport.
fn parse_record(record: &SocrataRecord) -> Result<CotReport> {
    let managed_money_long = parse_i64(&record.managed_money_long)?;
    let managed_money_short = parse_i64(&record.managed_money_short)?;
    let commercial_long = parse_i64(&record.commercial_long)?;
    let commercial_short = parse_i64(&record.commercial_short)?;

    Ok(CotReport {
        cftc_code: record.cftc_code.clone(),
        report_date: record.report_date.clone(),
        open_interest: parse_i64(&record.open_interest)?,
        managed_money_long,
        managed_money_short,
        managed_money_net: managed_money_long - managed_money_short,
        commercial_long,
        commercial_short,
        commercial_net: commercial_long - commercial_short,
    })
}

/// Parse integer from string field (handles commas).
fn parse_i64(s: &str) -> Result<i64> {
    let cleaned = s.replace(',', "");
    cleaned
        .parse::<i64>()
        .map_err(|e| anyhow::anyhow!("Failed to parse integer '{}': {}", s, e))
}

/// Find the pftui symbol for a CFTC contract code.
pub fn cftc_code_to_symbol(cftc_code: &str) -> Option<&'static str> {
    COT_CONTRACTS
        .iter()
        .find(|c| c.cftc_code == cftc_code)
        .map(|c| c.symbol)
}

/// Find the CFTC contract code for a pftui symbol.
pub fn symbol_to_cftc_code(symbol: &str) -> Option<&'static str> {
    COT_CONTRACTS
        .iter()
        .find(|c| c.symbol == symbol)
        .map(|c| c.cftc_code)
}

pub fn interpret_managed_money(history_desc: &[i64]) -> Option<CotInterpretation> {
    let (&current, rest) = history_desc.split_first()?;
    if rest.is_empty() {
        return None;
    }

    let history_1y = &history_desc[..history_desc.len().min(52)];
    let history_3y = &history_desc[..history_desc.len().min(156)];

    let percentile_1y = percentile_rank(current, history_1y)?;
    let percentile_3y = percentile_rank(current, history_3y)?;
    let z_score = z_score(current, history_3y).or_else(|| z_score(current, history_1y))?;
    let extreme = percentile_1y >= 90.0
        || percentile_1y <= 10.0
        || percentile_3y >= 90.0
        || percentile_3y <= 10.0;

    Some(CotInterpretation {
        percentile_1y,
        percentile_3y,
        z_score,
        extreme,
    })
}

fn percentile_rank(current: i64, history: &[i64]) -> Option<f64> {
    if history.is_empty() {
        return None;
    }

    let below_or_equal = history.iter().filter(|&&value| value <= current).count() as f64;
    Some((below_or_equal / history.len() as f64) * 100.0)
}

fn z_score(current: i64, history: &[i64]) -> Option<f64> {
    if history.len() < 2 {
        return None;
    }

    let mean = history.iter().map(|&value| value as f64).sum::<f64>() / history.len() as f64;
    let variance = history
        .iter()
        .map(|&value| {
            let diff = value as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / history.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        return Some(0.0);
    }
    Some((current as f64 - mean) / std_dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_interpretation_metrics() {
        let mut history = vec![120_000];
        history.extend((0..60).map(|idx| 60_000 + idx * 1_000));

        let stats = interpret_managed_money(&history).expect("stats should compute");
        assert!(stats.percentile_1y > 90.0);
        assert!(stats.percentile_3y > 90.0);
        assert!(stats.z_score > 1.0);
        assert!(stats.extreme);
    }

    #[test]
    fn flat_series_has_zero_z_score() {
        let history = vec![10_000; 60];
        let stats = interpret_managed_money(&history).expect("stats should compute");
        assert_eq!(stats.z_score, 0.0);
        assert!(stats.extreme);
    }
}
