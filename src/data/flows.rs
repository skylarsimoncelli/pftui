//! Capital-flow ingestion provider scaffold (F59).
//!
//! Real ETF flow data and 13F filings live behind paid providers and slow
//! quarterly SEC EDGAR feeds respectively. This module ships the provider
//! contract, a working `NoopProvider`, and explicit stubs for the two
//! real-provider paths so the schema + CLI + DB plumbing can land safely
//! while the upstream integrations are still being researched.
//!
//! Selection contract
//! ------------------
//!
//! The environment variable `PFTUI_FLOWS_PROVIDER` selects which provider
//! the refresh path uses:
//!
//! - `noop` (default) — no-op provider, logs "capital flows provider not
//!   configured" and returns zero flows. Always safe.
//! - `etf_com_csv` — stub. Returns `bail!("provider etf_com_csv not yet
//!   implemented — see TODO follow-up")` until the real CSV ingest lands.
//! - `sec_edgar_13f` — stub. Returns `bail!("provider sec_edgar_13f not yet
//!   implemented — see TODO follow-up")` until the real EDGAR ingest lands.
//!
//! `amount_usd` is stored as a `rust_decimal::Decimal` because these are
//! money values (per the project `CLAUDE.md` standards) and serialised to
//! the SQLite `capital_flows.amount_usd TEXT` column as a decimal string.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Allowed values for the `capital_flows.flow_type` column.
pub const FLOW_TYPES: &[&str] = &[
    "etf_creation",
    "etf_redemption",
    "institutional_13f",
    "crypto_exchange_inflow",
    "crypto_exchange_outflow",
];

/// One canonical capital-flow observation, agnostic to the upstream provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapitalFlow {
    pub asset: String,
    pub flow_type: String,
    pub amount_usd: Decimal,
    /// ISO-8601 date (YYYY-MM-DD) for the period start.
    pub period_start: String,
    /// ISO-8601 date (YYYY-MM-DD) for the period end (inclusive).
    pub period_end: String,
    pub source: String,
}

/// Result returned by a provider's `fetch` call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowFetchResult {
    pub flows: Vec<CapitalFlow>,
    /// Human-readable note suitable for logging into the refresh DAG.
    pub note: String,
}

/// Provider contract — anything that can yield `CapitalFlow` rows.
pub trait FlowProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Fetch flows for an optional asset filter.
    ///
    /// Implementations MUST NOT panic. Return `Ok(FlowFetchResult { flows:
    /// vec![], note: "..." })` for the gracefully-degraded path, and
    /// `Err(...)` only when a real upstream failure means the caller needs
    /// to surface an error.
    fn fetch(&self, asset_filter: Option<&str>) -> Result<FlowFetchResult>;
}

/// Default provider — does nothing, returns no flows, never errors.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopProvider;

impl FlowProvider for NoopProvider {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn fetch(&self, _asset_filter: Option<&str>) -> Result<FlowFetchResult> {
        Ok(FlowFetchResult {
            flows: Vec::new(),
            note: "capital flows provider not configured".to_string(),
        })
    }
}

/// ETF.com CSV provider stub. Real implementation is a TODO follow-up.
#[derive(Debug, Default, Clone, Copy)]
pub struct EtfComCsvProvider;

impl FlowProvider for EtfComCsvProvider {
    fn name(&self) -> &'static str {
        "etf_com_csv"
    }

    fn fetch(&self, _asset_filter: Option<&str>) -> Result<FlowFetchResult> {
        bail!("provider etf_com_csv not yet implemented — see TODO follow-up");
    }
}

/// SEC EDGAR 13F provider stub. Real implementation is a TODO follow-up.
#[derive(Debug, Default, Clone, Copy)]
pub struct SecEdgar13fProvider;

impl FlowProvider for SecEdgar13fProvider {
    fn name(&self) -> &'static str {
        "sec_edgar_13f"
    }

    fn fetch(&self, _asset_filter: Option<&str>) -> Result<FlowFetchResult> {
        bail!("provider sec_edgar_13f not yet implemented — see TODO follow-up");
    }
}

/// Resolve the configured provider from the environment.
///
/// Reads `PFTUI_FLOWS_PROVIDER`; defaults to `noop`. Unknown values fall
/// back to `noop` rather than erroring so a misconfigured env var never
/// breaks the refresh pipeline (the chosen provider's name is observable
/// via `FlowProvider::name`).
pub fn provider_from_env() -> Box<dyn FlowProvider> {
    let raw = std::env::var("PFTUI_FLOWS_PROVIDER").unwrap_or_default();
    provider_from_str(raw.trim())
}

/// Resolve a provider by name. Exposed for tests and CLI plumbing.
pub fn provider_from_str(name: &str) -> Box<dyn FlowProvider> {
    match name.to_ascii_lowercase().as_str() {
        "" | "noop" => Box::new(NoopProvider),
        "etf_com_csv" => Box::new(EtfComCsvProvider),
        "sec_edgar_13f" => Box::new(SecEdgar13fProvider),
        _ => Box::new(NoopProvider),
    }
}

/// Validate a flow_type string against the allowed enum values.
pub fn validate_flow_type(flow_type: &str) -> Result<()> {
    if FLOW_TYPES.contains(&flow_type) {
        Ok(())
    } else {
        bail!(
            "unknown flow_type '{}' — expected one of: {}",
            flow_type,
            FLOW_TYPES.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_provider_returns_empty_flows_with_note() {
        let provider = NoopProvider;
        assert_eq!(provider.name(), "noop");
        let result = provider.fetch(None).expect("noop should never error");
        assert!(result.flows.is_empty());
        assert_eq!(result.note, "capital flows provider not configured");
    }

    #[test]
    fn etf_com_csv_provider_bails_with_followup_message() {
        let err = EtfComCsvProvider
            .fetch(None)
            .expect_err("stub provider must bail");
        let message = format!("{err}");
        assert!(message.contains("etf_com_csv"));
        assert!(message.contains("not yet implemented"));
    }

    #[test]
    fn sec_edgar_13f_provider_bails_with_followup_message() {
        let err = SecEdgar13fProvider
            .fetch(None)
            .expect_err("stub provider must bail");
        let message = format!("{err}");
        assert!(message.contains("sec_edgar_13f"));
        assert!(message.contains("not yet implemented"));
    }

    #[test]
    fn provider_from_str_resolves_known_names() {
        assert_eq!(provider_from_str("noop").name(), "noop");
        assert_eq!(provider_from_str("").name(), "noop");
        assert_eq!(provider_from_str("NOOP").name(), "noop");
        assert_eq!(provider_from_str("etf_com_csv").name(), "etf_com_csv");
        assert_eq!(provider_from_str("sec_edgar_13f").name(), "sec_edgar_13f");
        // Unknown falls back to noop rather than panicking.
        assert_eq!(provider_from_str("not_a_real_provider").name(), "noop");
    }

    #[test]
    fn validate_flow_type_accepts_canonical_values() {
        for ty in FLOW_TYPES {
            validate_flow_type(ty).expect("canonical flow type should validate");
        }
        assert!(validate_flow_type("bogus").is_err());
    }
}
