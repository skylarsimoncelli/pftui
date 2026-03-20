use anyhow::Result;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::data::{bls, brave};

#[derive(Debug, Clone)]
pub struct EconomicReading {
    pub indicator: String,
    pub value: Decimal,
    pub previous: Option<Decimal>,
    pub change: Option<Decimal>,
    pub source_url: String,
}

const ECONOMIC_QUERIES: &[(&str, &str)] = &[
    ("cpi", "latest US CPI inflation rate"),
    ("unemployment_rate", "latest US unemployment rate nonfarm payrolls"),
    ("nfp", "latest US unemployment rate nonfarm payrolls"),
    ("pmi_manufacturing", "latest ISM manufacturing PMI services PMI"),
    ("pmi_services", "latest ISM manufacturing PMI services PMI"),
    ("fed_funds_rate", "latest FOMC federal funds rate"),
    ("initial_jobless_claims", "latest US initial jobless claims"),
    ("ppi", "latest US PPI producer price index"),
];

pub async fn fetch_via_brave(key: &str) -> Result<Vec<EconomicReading>> {
    let mut out = Vec::new();

    for (indicator, query) in ECONOMIC_QUERIES {
        let results = brave::brave_web_search(key, query, Some("pd"), 5).await?;
        if results.is_empty() {
            continue;
        }
        let haystack = results
            .iter()
            .flat_map(|r| {
                let mut parts = vec![r.description.clone()];
                parts.extend(r.extra_snippets.clone());
                parts
            })
            .collect::<Vec<_>>()
            .join(" ");

        if let Some(value) = extract_value(indicator, &haystack) {
            out.push(EconomicReading {
                indicator: (*indicator).to_string(),
                value,
                previous: None,
                change: None,
                source_url: results[0].url.clone(),
            });
        }
    }

    Ok(out)
}

pub async fn fetch_bls_fallback() -> Result<Vec<EconomicReading>> {
    let data = bls::fetch_all_key_series().await?;
    let mut cpi = None;
    let mut unemp = None;
    let mut nfp = None;
    let mut ppi = None;

    for p in data {
        match p.series_id.as_str() {
            bls::SERIES_CPI_U => cpi = Some(p.value),
            bls::SERIES_UNEMPLOYMENT => unemp = Some(p.value),
            bls::SERIES_NFP => nfp = Some(p.value),
            "WPUFD4" => ppi = Some(p.value),
            _ => {}
        }
    }

    let mut out = Vec::new();
    if let Some(v) = cpi {
        out.push(reading("cpi", v));
    }
    if let Some(v) = unemp {
        out.push(reading("unemployment_rate", v));
    }
    if let Some(v) = nfp {
        out.push(reading("nfp", v));
    }
    if let Some(v) = ppi {
        out.push(reading("ppi", v));
    }
    Ok(out)
}

fn reading(indicator: &str, value: Decimal) -> EconomicReading {
    EconomicReading {
        indicator: indicator.to_string(),
        value,
        previous: None,
        change: None,
        source_url: "https://api.bls.gov/publicAPI/v1/timeseries/data/".to_string(),
    }
}

fn extract_value(indicator: &str, text: &str) -> Option<Decimal> {
    let raw = match indicator {
        "nfp" | "initial_jobless_claims" => extract_integer_like(text),
        _ => extract_percent_like(text).or_else(|| extract_decimal_like(text)),
    };

    // Validate extracted values against reasonable bounds to reject garbage.
    // These ranges are deliberately wide to avoid false negatives.
    raw.filter(|v| is_plausible(indicator, *v))
}

/// Reject obviously implausible values that result from naive text extraction
/// (e.g. extracting a year "2025" as a PMI reading, or "19" as NFP).
fn is_plausible(indicator: &str, value: Decimal) -> bool {
    let v = value.to_string().parse::<f64>().unwrap_or(0.0);
    match indicator {
        // CPI YoY inflation: -5% to 25% (even hyperinflation scenarios)
        "cpi" => (-5.0..=25.0).contains(&v),
        // Unemployment rate: 0% to 30%
        "unemployment_rate" => (0.0..=30.0).contains(&v),
        // NFP: typically 50K-500K range, but can go -20M in crisis. Reject < 50 (likely noise).
        "nfp" => v.abs() >= 50.0 && v.abs() <= 20_000_000.0,
        // PMI: 0-100 index (never > 100)
        "pmi_manufacturing" | "pmi_services" => (0.0..=100.0).contains(&v),
        // Fed funds rate: 0% to 25%
        "fed_funds_rate" => (0.0..=25.0).contains(&v),
        // Initial jobless claims: typically 150K-1M+. Values under 50K are noise
        // (likely extracted a random number from article text, not the actual figure).
        "initial_jobless_claims" => (50_000.0..=10_000_000.0).contains(&v),
        // PPI: can be negative to ~20%
        "ppi" => (-10.0..=30.0).contains(&v),
        _ => true,
    }
}

fn extract_percent_like(text: &str) -> Option<Decimal> {
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| ",.;:()[]{}".contains(c));
        if let Some(num) = t.strip_suffix('%') {
            if let Ok(v) = Decimal::from_str(num) {
                return Some(v);
            }
        }
    }
    None
}

fn extract_decimal_like(text: &str) -> Option<Decimal> {
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| ",.;:()[]{}".contains(c));
        if let Ok(v) = Decimal::from_str(t) {
            return Some(v);
        }
    }
    None
}

fn extract_integer_like(text: &str) -> Option<Decimal> {
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| ".;:()[]{}".contains(c)).replace(',', "");
        if let Ok(v) = Decimal::from_str(&t) {
            if v > Decimal::ZERO {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn parses_percent_like() {
        assert_eq!(extract_percent_like("CPI rose 3.2% year-over-year"), Some(dec!(3.2)));
    }

    #[test]
    fn parses_integer_like() {
        assert_eq!(
            extract_integer_like("economy added 275,000 jobs"),
            Some(dec!(275000))
        );
    }

    #[test]
    fn plausibility_rejects_year_as_pmi() {
        // PMI must be 0-100; "2025" (a year) should be rejected
        assert!(!is_plausible("pmi_manufacturing", dec!(2025)));
        assert!(!is_plausible("pmi_services", dec!(2025)));
    }

    #[test]
    fn plausibility_accepts_valid_pmi() {
        assert!(is_plausible("pmi_manufacturing", dec!(52.3)));
        assert!(is_plausible("pmi_services", dec!(48.1)));
    }

    #[test]
    fn plausibility_rejects_tiny_nfp() {
        // "19" is not a plausible NFP reading (thousands)
        assert!(!is_plausible("nfp", dec!(19)));
    }

    #[test]
    fn plausibility_accepts_valid_nfp() {
        assert!(is_plausible("nfp", dec!(275000)));
        assert!(is_plausible("nfp", dec!(151)));
    }

    #[test]
    fn plausibility_rejects_low_jobless_claims() {
        // 8000 is implausibly low for initial jobless claims (typically 150K+)
        assert!(!is_plausible("initial_jobless_claims", dec!(8000)));
        assert!(!is_plausible("initial_jobless_claims", dec!(500)));
        assert!(!is_plausible("initial_jobless_claims", dec!(49999)));
    }

    #[test]
    fn plausibility_accepts_valid_jobless_claims() {
        assert!(is_plausible("initial_jobless_claims", dec!(225000)));
        assert!(is_plausible("initial_jobless_claims", dec!(50000)));
    }

    #[test]
    fn plausibility_cpi_bounds() {
        assert!(is_plausible("cpi", dec!(3.2)));
        assert!(is_plausible("cpi", dec!(0.1)));
        assert!(!is_plausible("cpi", dec!(50)));
    }

    #[test]
    fn plausibility_fed_funds_rate() {
        assert!(is_plausible("fed_funds_rate", dec!(3.5)));
        assert!(is_plausible("fed_funds_rate", dec!(0)));
        assert!(!is_plausible("fed_funds_rate", dec!(30)));
    }

    #[test]
    fn extract_value_filters_implausible() {
        // "economy grew 2025 percent" — 2025 extracted as PMI → rejected
        assert!(extract_value("pmi_manufacturing", "ISM PMI fell in 2025 outlook uncertain").is_none());
        // Valid extraction
        assert_eq!(
            extract_value("cpi", "CPI rose 3.2% year-over-year"),
            Some(dec!(3.2))
        );
    }
}

