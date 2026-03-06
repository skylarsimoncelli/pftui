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
//! - Gold (067651): COMEX Gold Futures
//! - Silver (084691): COMEX Silver Futures
//! - WTI Crude Oil (067411): NYMEX WTI Light Sweet Crude Oil
//! - Bitcoin (133741): CME Bitcoin Futures

use anyhow::{anyhow, Result};
use serde::Deserialize;

/// CFTC contract codes we track and their pftui symbol mappings.
pub const COT_CONTRACTS: &[CotContract] = &[
    CotContract {
        cftc_code: "067651",
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

    let record = resp.first().ok_or_else(|| {
        anyhow!("No COT data found for contract code {}", cftc_code)
    })?;

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

    resp.iter()
        .map(parse_record)
        .collect::<Result<Vec<_>>>()
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
