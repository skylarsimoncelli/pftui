use anyhow::Result;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::data::{bls, brave};

/// Source of an economic data reading, ordered by authority.
/// Higher-authority sources are preferred during reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataSource {
    /// FRED (Federal Reserve Economic Data) — most authoritative
    Fred,
    /// Bureau of Labor Statistics
    Bls,
    /// Brave Search text extraction — least reliable
    Brave,
}

impl DataSource {
    /// Priority rank: lower = more authoritative.
    pub fn priority(&self) -> u8 {
        match self {
            DataSource::Fred => 0,
            DataSource::Bls => 1,
            DataSource::Brave => 2,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            DataSource::Fred => "fred",
            DataSource::Bls => "bls",
            DataSource::Brave => "brave",
        }
    }

    /// Confidence level for values from this source.
    pub fn confidence(&self) -> &'static str {
        match self {
            DataSource::Fred => "high",
            DataSource::Bls => "high",
            DataSource::Brave => "low",
        }
    }
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone)]
pub struct EconomicReading {
    pub indicator: String,
    pub value: Decimal,
    pub previous: Option<Decimal>,
    pub change: Option<Decimal>,
    pub source_url: String,
    pub source: DataSource,
}

/// A discrepancy found between two sources for the same indicator.
#[derive(Debug, Clone)]
pub struct SourceDiscrepancy {
    pub indicator: String,
    pub preferred_source: DataSource,
    pub preferred_value: Decimal,
    pub other_source: DataSource,
    pub other_value: Decimal,
    /// Absolute percentage difference between values.
    pub diff_pct: Decimal,
}

/// Maps FRED series IDs to economy indicator names.
/// Used for cross-source reconciliation.
pub fn fred_to_indicator(series_id: &str) -> Option<&'static str> {
    match series_id {
        "FEDFUNDS" => Some("fed_funds_rate"),
        "UNRATE" => Some("unemployment_rate"),
        "PAYEMS" => Some("nfp"),
        "NAPM" => Some("pmi_manufacturing"),
        "ICSA" => Some("initial_jobless_claims"),
        // CPIAUCSL is an index, not a YoY rate, so not directly comparable to Brave "cpi"
        // PPIACO is also an index, not a YoY rate
        _ => None,
    }
}

/// Reconcile readings from multiple sources for the same indicators.
///
/// When multiple sources provide the same indicator, this function:
/// 1. Groups readings by indicator name
/// 2. Picks the most authoritative source (FRED > BLS > Brave)
/// 3. Returns the winning readings along with any discrepancies found
///
/// `fred_readings` should be pre-mapped from FRED series IDs to indicator names.
pub fn reconcile(
    readings: Vec<EconomicReading>,
) -> (Vec<EconomicReading>, Vec<SourceDiscrepancy>) {
    use std::collections::HashMap;

    let mut by_indicator: HashMap<String, Vec<EconomicReading>> = HashMap::new();
    for r in readings {
        by_indicator
            .entry(r.indicator.clone())
            .or_default()
            .push(r);
    }

    let mut winners = Vec::new();
    let mut discrepancies = Vec::new();

    for (indicator, mut sources) in by_indicator {
        // Sort by source priority (lower = more authoritative)
        sources.sort_by_key(|r| r.source.priority());

        let best = sources[0].clone();

        // Check for discrepancies between sources
        for other in &sources[1..] {
            let denominator = best.value.abs();
            let diff = if denominator > Decimal::ZERO {
                let diff_abs = (best.value - other.value).abs();
                (diff_abs * Decimal::from(100)) / denominator
            } else {
                Decimal::ZERO
            };
            // Flag if difference > 0.5% (significant enough to note)
            if diff > Decimal::from_str("0.5").unwrap_or(Decimal::ONE) {
                discrepancies.push(SourceDiscrepancy {
                    indicator: indicator.clone(),
                    preferred_source: best.source.clone(),
                    preferred_value: best.value,
                    other_source: other.source.clone(),
                    other_value: other.value,
                    diff_pct: diff.round_dp(2),
                });
            }
        }

        winners.push(best);
    }

    // Sort by indicator name for consistent output
    winners.sort_by(|a, b| a.indicator.cmp(&b.indicator));
    discrepancies.sort_by(|a, b| a.indicator.cmp(&b.indicator));

    (winners, discrepancies)
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
                source: DataSource::Brave,
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
        source: DataSource::Bls,
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

    fn make_reading(indicator: &str, value: Decimal, source: DataSource) -> EconomicReading {
        EconomicReading {
            indicator: indicator.to_string(),
            value,
            previous: None,
            change: None,
            source_url: format!("https://{}.example.com", source.name()),
            source,
        }
    }

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
        assert!(!is_plausible("nfp", dec!(19)));
    }

    #[test]
    fn plausibility_accepts_valid_nfp() {
        assert!(is_plausible("nfp", dec!(275000)));
        assert!(is_plausible("nfp", dec!(151)));
    }

    #[test]
    fn plausibility_rejects_low_jobless_claims() {
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
        assert!(extract_value("pmi_manufacturing", "ISM PMI fell in 2025 outlook uncertain").is_none());
        assert_eq!(
            extract_value("cpi", "CPI rose 3.2% year-over-year"),
            Some(dec!(3.2))
        );
    }

    // ─── Reconciliation tests ───

    #[test]
    fn data_source_priority_ordering() {
        assert!(DataSource::Fred.priority() < DataSource::Bls.priority());
        assert!(DataSource::Bls.priority() < DataSource::Brave.priority());
    }

    #[test]
    fn data_source_names() {
        assert_eq!(DataSource::Fred.name(), "fred");
        assert_eq!(DataSource::Bls.name(), "bls");
        assert_eq!(DataSource::Brave.name(), "brave");
    }

    #[test]
    fn data_source_confidence() {
        assert_eq!(DataSource::Fred.confidence(), "high");
        assert_eq!(DataSource::Bls.confidence(), "high");
        assert_eq!(DataSource::Brave.confidence(), "low");
    }

    #[test]
    fn reconcile_prefers_fred_over_brave() {
        let readings = vec![
            make_reading("fed_funds_rate", dec!(4.33), DataSource::Brave),
            make_reading("fed_funds_rate", dec!(4.50), DataSource::Fred),
        ];
        let (winners, discrepancies) = reconcile(readings);
        assert_eq!(winners.len(), 1);
        assert_eq!(winners[0].source, DataSource::Fred);
        assert_eq!(winners[0].value, dec!(4.50));
        // Should flag the discrepancy
        assert_eq!(discrepancies.len(), 1);
        assert_eq!(discrepancies[0].preferred_value, dec!(4.50));
        assert_eq!(discrepancies[0].other_value, dec!(4.33));
    }

    #[test]
    fn reconcile_prefers_fred_over_bls() {
        let readings = vec![
            make_reading("unemployment_rate", dec!(4.2), DataSource::Bls),
            make_reading("unemployment_rate", dec!(4.1), DataSource::Fred),
        ];
        let (winners, discrepancies) = reconcile(readings);
        assert_eq!(winners.len(), 1);
        assert_eq!(winners[0].source, DataSource::Fred);
        assert_eq!(winners[0].value, dec!(4.1));
        // ~2.4% difference — should be flagged
        assert_eq!(discrepancies.len(), 1);
    }

    #[test]
    fn reconcile_prefers_bls_over_brave() {
        let readings = vec![
            make_reading("cpi", dec!(3.1), DataSource::Brave),
            make_reading("cpi", dec!(3.2), DataSource::Bls),
        ];
        let (winners, _) = reconcile(readings);
        assert_eq!(winners.len(), 1);
        assert_eq!(winners[0].source, DataSource::Bls);
        assert_eq!(winners[0].value, dec!(3.2));
    }

    #[test]
    fn reconcile_no_discrepancy_when_values_match() {
        let readings = vec![
            make_reading("fed_funds_rate", dec!(4.50), DataSource::Brave),
            make_reading("fed_funds_rate", dec!(4.50), DataSource::Fred),
        ];
        let (winners, discrepancies) = reconcile(readings);
        assert_eq!(winners.len(), 1);
        assert_eq!(winners[0].source, DataSource::Fred);
        // 0% difference — no discrepancy
        assert!(discrepancies.is_empty());
    }

    #[test]
    fn reconcile_no_discrepancy_under_threshold() {
        // 0.22% difference — below 0.5% threshold
        let readings = vec![
            make_reading("fed_funds_rate", dec!(4.50), DataSource::Fred),
            make_reading("fed_funds_rate", dec!(4.51), DataSource::Brave),
        ];
        let (_, discrepancies) = reconcile(readings);
        assert!(discrepancies.is_empty());
    }

    #[test]
    fn reconcile_handles_single_source() {
        let readings = vec![
            make_reading("cpi", dec!(3.2), DataSource::Brave),
            make_reading("ppi", dec!(2.1), DataSource::Bls),
        ];
        let (winners, discrepancies) = reconcile(readings);
        assert_eq!(winners.len(), 2);
        assert!(discrepancies.is_empty());
    }

    #[test]
    fn reconcile_handles_three_sources() {
        let readings = vec![
            make_reading("fed_funds_rate", dec!(4.33), DataSource::Brave),
            make_reading("fed_funds_rate", dec!(4.40), DataSource::Bls),
            make_reading("fed_funds_rate", dec!(4.50), DataSource::Fred),
        ];
        let (winners, discrepancies) = reconcile(readings);
        assert_eq!(winners.len(), 1);
        assert_eq!(winners[0].source, DataSource::Fred);
        assert_eq!(winners[0].value, dec!(4.50));
        // Two discrepancies: FRED vs BLS, FRED vs Brave
        assert_eq!(discrepancies.len(), 2);
    }

    #[test]
    fn fred_to_indicator_mappings() {
        assert_eq!(fred_to_indicator("FEDFUNDS"), Some("fed_funds_rate"));
        assert_eq!(fred_to_indicator("UNRATE"), Some("unemployment_rate"));
        assert_eq!(fred_to_indicator("PAYEMS"), Some("nfp"));
        assert_eq!(fred_to_indicator("NAPM"), Some("pmi_manufacturing"));
        assert_eq!(fred_to_indicator("ICSA"), Some("initial_jobless_claims"));
        // CPI index is not directly comparable to CPI YoY rate
        assert_eq!(fred_to_indicator("CPIAUCSL"), None);
        assert_eq!(fred_to_indicator("BOGUS"), None);
    }
}

