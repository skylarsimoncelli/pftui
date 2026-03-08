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
    match indicator {
        "nfp" | "initial_jobless_claims" => extract_integer_like(text),
        _ => extract_percent_like(text).or_else(|| extract_decimal_like(text)),
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
}

