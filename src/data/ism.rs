//! ISM PMI data fetcher via targeted web search.
//!
//! ISM Manufacturing PMI and Services PMI are proprietary indicators not
//! available through FRED or BLS APIs. This module uses targeted Brave Search
//! queries aimed at ISM press releases (via PR Newswire) and financial data
//! aggregators (Investing.com, TradingEconomics) to extract the headline PMI
//! values with higher confidence than the generic Brave economic scraper.
//!
//! The key improvements over the generic `economic::fetch_via_brave`:
//! 1. Queries target ISM-specific sources (PR Newswire, ISM press releases)
//! 2. Parsing looks for the "PMI® registered XX.X percent" pattern from ISM
//! 3. Multiple extraction strategies provide fallback
//! 4. Previous month's value is extracted when available

use anyhow::Result;
use regex::Regex;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::data::brave;
use crate::data::economic::{DataSource, EconomicReading};

/// PMI-specific search queries, ordered by expected reliability.
const MANUFACTURING_QUERIES: &[&str] = &[
    "ISM Manufacturing PMI latest percent site:prnewswire.com",
    "ISM manufacturing PMI registered percent",
];

const SERVICES_QUERIES: &[&str] = &[
    "ISM Services PMI latest percent site:prnewswire.com",
    "ISM services PMI registered percent",
];

/// Fetch ISM Manufacturing and Services PMI via targeted web search.
///
/// Returns up to 2 readings (manufacturing + services) with `DataSource::Ism`.
/// Falls back through multiple queries if the first yields no result.
pub async fn fetch_ism_pmi(brave_key: &str) -> Result<Vec<EconomicReading>> {
    let mut results = Vec::new();

    // Manufacturing PMI
    if let Some(reading) = fetch_single_pmi(brave_key, "pmi_manufacturing", MANUFACTURING_QUERIES).await? {
        results.push(reading);
    }

    // Services PMI
    if let Some(reading) = fetch_single_pmi(brave_key, "pmi_services", SERVICES_QUERIES).await? {
        results.push(reading);
    }

    Ok(results)
}

async fn fetch_single_pmi(
    brave_key: &str,
    indicator: &str,
    queries: &[&str],
) -> Result<Option<EconomicReading>> {
    for query in queries {
        let search_results = brave::brave_web_search(brave_key, query, Some("pm"), 5).await?;
        if search_results.is_empty() {
            continue;
        }

        // Build a haystack from all result text
        let haystack = search_results
            .iter()
            .flat_map(|r| {
                let mut parts = vec![r.title.clone(), r.description.clone()];
                parts.extend(r.extra_snippets.clone());
                parts
            })
            .collect::<Vec<_>>()
            .join(" ");

        let source_url = search_results[0].url.clone();

        if let Some((value, previous)) = extract_ism_pmi(&haystack) {
            if is_valid_pmi(value) {
                return Ok(Some(EconomicReading {
                    indicator: indicator.to_string(),
                    value,
                    previous: previous.filter(|p| is_valid_pmi(*p)),
                    change: previous
                        .filter(|p| is_valid_pmi(*p))
                        .map(|p| value - p),
                    source_url,
                    source: DataSource::Ism,
                }));
            }
        }
    }

    Ok(None)
}

/// Extract PMI value from ISM press release text.
///
/// ISM press releases consistently use the format:
///   "PMI® registered XX.X percent"
///   "Manufacturing PMI® registered XX.X percent"
///   "Services PMI® at XX.X%"
///
/// Also extracts previous month value from patterns like:
///   "compared to the reading of XX.X in January"
///   "from XX.X percent in January"
fn extract_ism_pmi(text: &str) -> Option<(Decimal, Option<Decimal>)> {
    // Strategy 1: ISM official format "PMI® registered XX.X percent"
    // or "PMI® at XX.X%" from title patterns
    let pmi_re = Regex::new(
        r"(?i)(?:PMI[®]?\s+(?:registered|at)\s+(\d{2}\.\d)\s*(?:percent|%))"
    ).ok()?;

    if let Some(caps) = pmi_re.captures(text) {
        let value = Decimal::from_str(caps.get(1)?.as_str()).ok()?;
        let previous = extract_previous_pmi(text);
        return Some((value, previous));
    }

    // Strategy 2: Title patterns like "Manufacturing PMI® at 52.4%"
    // or "PMI at 52.4; February 2026"
    let title_re = Regex::new(
        r"(?i)PMI[®]?\s+at\s+(\d{2}\.\d)"
    ).ok()?;

    if let Some(caps) = title_re.captures(text) {
        let value = Decimal::from_str(caps.get(1)?.as_str()).ok()?;
        let previous = extract_previous_pmi(text);
        return Some((value, previous));
    }

    // Strategy 3: Financial data sites "Actual52.4" or "Actual: 52.4"
    let actual_re = Regex::new(
        r"(?i)Actual[:\s]*(\d{2}\.\d)"
    ).ok()?;

    if let Some(caps) = actual_re.captures(text) {
        let value = Decimal::from_str(caps.get(1)?.as_str()).ok()?;
        // Try to extract previous from "Previous" field
        let prev_re = Regex::new(r"(?i)Previous[:\s·]*(\d{2}\.\d)").ok()?;
        let previous = prev_re
            .captures(text)
            .and_then(|c| Decimal::from_str(c.get(1)?.as_str()).ok());
        return Some((value, previous));
    }

    // Strategy 4: Simple "slipped to XX.X" or "rose to XX.X" or "came in at XX.X"
    let narrative_re = Regex::new(
        r"(?i)(?:slipped|rose|fell|increased|decreased|came\s+in|expanded|contracted)\s+to\s+(\d{2}\.\d)"
    ).ok()?;

    if let Some(caps) = narrative_re.captures(text) {
        let value = Decimal::from_str(caps.get(1)?.as_str()).ok()?;
        let previous = extract_previous_pmi(text);
        return Some((value, previous));
    }

    None
}

/// Extract the previous month's PMI value from surrounding text.
fn extract_previous_pmi(text: &str) -> Option<Decimal> {
    // Pattern: "from XX.X [percent|%|in month]" or "compared to ... XX.X"
    // Also handles "from XX.X in January" without "percent" suffix
    let prev_re = Regex::new(
        r"(?i)(?:from\s+|compared\s+to\s+(?:the\s+)?(?:reading\s+of\s+)?|down\s+from\s+|up\s+from\s+)(\d{2}\.\d)\s*(?:percent|%|in\s+\w+)?"
    ).ok()?;

    prev_re
        .captures(text)
        .and_then(|c| Decimal::from_str(c.get(1)?.as_str()).ok())
        .filter(|v| is_valid_pmi(*v))
}

/// Validate that a PMI value is in a reasonable range.
/// ISM PMI historically ranges from ~29 (2008 crisis low) to ~64.
fn is_valid_pmi(value: Decimal) -> bool {
    let v: f64 = value.to_string().parse().unwrap_or(0.0);
    (25.0..=80.0).contains(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn extracts_ism_registered_format() {
        let text = "The Manufacturing PMI® registered 52.4 percent in February";
        let (value, _) = extract_ism_pmi(text).unwrap();
        assert_eq!(value, dec!(52.4));
    }

    #[test]
    fn extracts_pmi_at_format() {
        let text = "Manufacturing PMI® at 52.4%; February 2026 ISM® Manufacturing PMI® Report";
        let (value, _) = extract_ism_pmi(text).unwrap();
        assert_eq!(value, dec!(52.4));
    }

    #[test]
    fn extracts_actual_format() {
        let text = "Latest ReleaseMar 02, 2026 · Actual52.4 · Forecast51.7 · Previous · 52.6";
        let (value, previous) = extract_ism_pmi(text).unwrap();
        assert_eq!(value, dec!(52.4));
        assert_eq!(previous, Some(dec!(52.6)));
    }

    #[test]
    fn extracts_narrative_format() {
        let text = "The ISM Manufacturing PMI slipped to 52.4 in February 2026 from 52.6 in January";
        let (value, previous) = extract_ism_pmi(text).unwrap();
        assert_eq!(value, dec!(52.4));
        assert_eq!(previous, Some(dec!(52.6)));
    }

    #[test]
    fn extracts_previous_from_comparison() {
        let text = "registered 52.4 percent, a 0.2-percentage point decrease compared to the reading of 52.6 in January";
        let prev = extract_previous_pmi(text);
        assert_eq!(prev, Some(dec!(52.6)));
    }

    #[test]
    fn extracts_previous_from_from_pattern() {
        let text = "fell to 48.1 from 49.3 percent in December";
        let prev = extract_previous_pmi(text);
        assert_eq!(prev, Some(dec!(49.3)));
    }

    #[test]
    fn rejects_invalid_pmi() {
        assert!(!is_valid_pmi(dec!(2.5)));
        assert!(!is_valid_pmi(dec!(24.9)));
        assert!(!is_valid_pmi(dec!(80.1)));
        assert!(!is_valid_pmi(dec!(100.0)));
    }

    #[test]
    fn accepts_valid_pmi() {
        assert!(is_valid_pmi(dec!(25.0)));
        assert!(is_valid_pmi(dec!(50.0)));
        assert!(is_valid_pmi(dec!(52.4)));
        assert!(is_valid_pmi(dec!(80.0)));
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(extract_ism_pmi("no PMI data here at all").is_none());
        assert!(extract_ism_pmi("PMI registered 2025 percent").is_none());
        assert!(extract_ism_pmi("").is_none());
    }

    #[test]
    fn services_pmi_format() {
        let text = "Services PMI® registered 54.1 percent in February, higher than 52.8 percent in January";
        let (value, _) = extract_ism_pmi(text).unwrap();
        assert_eq!(value, dec!(54.1));
    }

    #[test]
    fn extracts_with_registered_and_previous() {
        let text = "The Manufacturing PMI® registered 52.4 percent in February, a 0.2-percentage point decrease compared to the reading of 52.6 in January";
        let (value, previous) = extract_ism_pmi(text).unwrap();
        assert_eq!(value, dec!(52.4));
        assert_eq!(previous, Some(dec!(52.6)));
    }
}
