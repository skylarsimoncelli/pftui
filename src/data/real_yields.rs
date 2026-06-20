//! Real-yield curve ingestion: US TIPS, breakevens, and G10 sovereign 10Y yields.
//!
//! Sources every series from FRED via the existing public JSON API. When the
//! configured API key is missing (or the network is unreachable), all fetches
//! degrade gracefully and return empty observation vectors so the rest of the
//! refresh pipeline keeps working.
//!
//! Series tracked
//! --------------
//!
//! US TIPS (real yields):
//!   - DFII5  — 5Y constant-maturity TIPS
//!   - DFII10 — 10Y constant-maturity TIPS
//!   - DFII30 — 30Y constant-maturity TIPS
//!
//! US inflation breakevens:
//!   - T5YIE  — 5Y breakeven inflation rate
//!   - T10YIE — 10Y breakeven inflation rate
//!
//! G10 sovereign 10Y benchmark yields (OECD monthly via FRED):
//!   - IRLTLT01GBM156N — United Kingdom
//!   - IRLTLT01DEM156N — Germany
//!   - IRLTLT01JPM156N — Japan
//!   - IRLTLT01CAM156N — Canada
//!
//! For convenience the US nominal 10Y (`DGS10`) is also exposed here so
//! differential calculations can pull a single, normalized snapshot without
//! needing to coordinate with the broader FRED ingest pipeline.

use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;

/// FRED series identifier for the US 10Y nominal yield (used as the anchor for
/// the differential view but ingested independently here so this module can
/// stand alone if `FRED_SERIES` ever changes upstream).
pub const US_NOMINAL_10Y: &str = "DGS10";

/// All US TIPS real-yield series.
pub const US_TIPS_SERIES: &[&str] = &["DFII5", "DFII10", "DFII30"];

/// All US breakeven-inflation series.
pub const US_BREAKEVEN_SERIES: &[&str] = &["T5YIE", "T10YIE"];

/// G10 sovereign 10Y benchmark series (FRED-hosted OECD monthly data).
pub const G10_SOVEREIGN_10Y: &[(&str, &str)] = &[
    ("GB", "IRLTLT01GBM156N"),
    ("DE", "IRLTLT01DEM156N"),
    ("JP", "IRLTLT01JPM156N"),
    ("CA", "IRLTLT01CAM156N"),
];

/// Full set of real-yield series ingested by `data real-yields refresh`.
///
/// Includes the US nominal 10Y so the same code path keeps an authoritative
/// copy in `real_yields_history` for differential computations.
pub fn all_series_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    ids.push(US_NOMINAL_10Y);
    ids.extend_from_slice(US_TIPS_SERIES);
    ids.extend_from_slice(US_BREAKEVEN_SERIES);
    for (_, id) in G10_SOVEREIGN_10Y {
        ids.push(id);
    }
    ids
}

/// One real-yield observation parsed from FRED.
#[derive(Debug, Clone, PartialEq)]
pub struct RealYieldObservation {
    pub series_id: String,
    pub date: String,
    pub value: f64,
    pub source: String,
}

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

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(Into::into)
}

/// Fetch a single FRED series over the last `days` calendar days.
///
/// Returns an empty vector when `api_key` is empty or whitespace, so callers
/// can short-circuit cleanly when FRED is unavailable.
pub async fn fetch_series_history(
    api_key: &str,
    series_id: &str,
    days: u32,
) -> Result<Vec<RealYieldObservation>> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let client = build_client()?;
    let end = Utc::now().date_naive();
    let start = end - chrono::Duration::days(days as i64);

    let url = format!(
        "{}?series_id={}&api_key={}&file_type=json&sort_order=asc&observation_start={}&observation_end={}",
        FRED_BASE_URL,
        series_id,
        trimmed,
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    );

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return Ok(Vec::new()),
    };
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let body: FredResponse = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Ok(Vec::new()),
    };

    let mut out = Vec::with_capacity(body.observations.len());
    for raw in body.observations {
        if raw.value == "." {
            continue;
        }
        let Ok(value) = f64::from_str(raw.value.trim()) else {
            continue;
        };
        out.push(RealYieldObservation {
            series_id: series_id.to_string(),
            date: raw.date,
            value,
            source: "FRED".to_string(),
        });
    }
    Ok(out)
}

/// Per-pair US-minus-X differential for one G10 partner.
#[derive(Debug, Clone, PartialEq)]
pub struct PairDifferential {
    pub country: String,
    pub partner_series: String,
    pub us_value: f64,
    pub partner_value: f64,
    pub spread_bp: f64,
}

/// Differential snapshot for a single date.
#[derive(Debug, Clone, PartialEq)]
pub struct DifferentialSnapshot {
    pub date: String,
    pub us_tips_10y: Option<f64>,
    pub us_breakeven_10y: Option<f64>,
    pub us_nominal_10y: Option<f64>,
    /// US 10Y nominal minus the simple average of GB/DE/JP/CA 10Y, in bp.
    pub us_minus_g10_avg_bp: Option<f64>,
    pub pairs: Vec<PairDifferential>,
}

/// Pure helper used by both the CLI and unit tests.
///
/// `rows` is an iterator of `(date, series_id, value)` tuples — anything that
/// can be loaded from the database. Returns one snapshot per date that has at
/// least one G10 partner present.
pub fn compute_differentials<I>(rows: I) -> Vec<DifferentialSnapshot>
where
    I: IntoIterator<Item = (String, String, f64)>,
{
    use std::collections::BTreeMap;

    let mut by_date: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for (date, series, value) in rows {
        by_date.entry(date).or_default().insert(series, value);
    }

    // As-of carry of the most recent value per partner series. The G10 OECD
    // series are MONTHLY (dated the 1st) while the US series are DAILY, so an
    // exact same-date join almost never matches — forward-fill the latest prior
    // monthly G10 value onto each US daily date instead.
    let mut carry_partner: BTreeMap<&str, f64> = BTreeMap::new();

    let mut snapshots = Vec::new();
    for (date, series_map) in by_date {
        // Fold any G10 prints on THIS date into the carry before using it.
        for (_country, partner_series) in G10_SOVEREIGN_10Y {
            if let Some(v) = series_map.get(*partner_series).copied() {
                carry_partner.insert(*partner_series, v);
            }
        }
        let us_nominal = series_map.get(US_NOMINAL_10Y).copied();
        let us_tips = series_map.get("DFII10").copied();
        let us_be = series_map.get("T10YIE").copied();

        let mut pairs: Vec<PairDifferential> = Vec::new();
        let mut partners_for_avg: Vec<f64> = Vec::new();
        if let Some(us_val) = us_nominal {
            for (country, partner_series) in G10_SOVEREIGN_10Y {
                if let Some(partner_val) = carry_partner.get(*partner_series).copied() {
                    let spread_bp = (us_val - partner_val) * 100.0;
                    pairs.push(PairDifferential {
                        country: (*country).to_string(),
                        partner_series: (*partner_series).to_string(),
                        us_value: us_val,
                        partner_value: partner_val,
                        spread_bp,
                    });
                    partners_for_avg.push(partner_val);
                }
            }
        }

        if pairs.is_empty() && us_tips.is_none() && us_be.is_none() {
            continue;
        }

        let us_minus_g10_avg_bp =
            if let (Some(us_val), false) = (us_nominal, partners_for_avg.is_empty()) {
                let avg = partners_for_avg.iter().sum::<f64>() / partners_for_avg.len() as f64;
                Some((us_val - avg) * 100.0)
            } else {
                None
            };

        snapshots.push(DifferentialSnapshot {
            date,
            us_tips_10y: us_tips,
            us_breakeven_10y: us_be,
            us_nominal_10y: us_nominal,
            us_minus_g10_avg_bp,
            pairs,
        });
    }
    snapshots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_series_includes_us_anchor_tips_breakevens_and_g10() {
        let ids = all_series_ids();
        assert!(ids.contains(&US_NOMINAL_10Y));
        for s in US_TIPS_SERIES {
            assert!(ids.contains(s));
        }
        for s in US_BREAKEVEN_SERIES {
            assert!(ids.contains(s));
        }
        for (_, s) in G10_SOVEREIGN_10Y {
            assert!(ids.contains(s));
        }
        // 1 anchor + 3 tips + 2 breakevens + 4 G10 = 10
        assert_eq!(ids.len(), 10);
    }

    #[test]
    fn fetch_with_empty_api_key_returns_empty() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let out = rt.block_on(fetch_series_history("", "DFII10", 30)).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn differentials_computes_us_minus_g10_average_and_pairs() {
        // Synthetic three-day fixture covering enough partners to exercise both
        // the average and per-pair math.
        let rows: Vec<(String, String, f64)> = vec![
            // Day 1: US 4.20%, DE 2.20%, JP 0.80%, GB 4.00%, CA 3.50%
            ("2026-04-01".into(), US_NOMINAL_10Y.into(), 4.20),
            ("2026-04-01".into(), "DFII10".into(), 2.10),
            ("2026-04-01".into(), "T10YIE".into(), 2.40),
            ("2026-04-01".into(), "IRLTLT01DEM156N".into(), 2.20),
            ("2026-04-01".into(), "IRLTLT01JPM156N".into(), 0.80),
            ("2026-04-01".into(), "IRLTLT01GBM156N".into(), 4.00),
            ("2026-04-01".into(), "IRLTLT01CAM156N".into(), 3.50),
            // Day 2: only US present — skip
            ("2026-04-02".into(), US_NOMINAL_10Y.into(), 4.25),
            // Day 3: US + just Germany — pair-only snapshot still emitted
            ("2026-04-03".into(), US_NOMINAL_10Y.into(), 4.30),
            ("2026-04-03".into(), "IRLTLT01DEM156N".into(), 2.25),
        ];
        let snaps = compute_differentials(rows);
        // Forward-fill: Day 2 (US-only) now CARRIES Day 1's monthly G10 values
        // rather than being skipped — all three days emit with 4 pairs.
        assert_eq!(snaps.len(), 3);

        let day1 = &snaps[0];
        assert_eq!(day1.date, "2026-04-01");
        assert_eq!(day1.us_nominal_10y, Some(4.20));
        assert_eq!(day1.us_tips_10y, Some(2.10));
        assert_eq!(day1.us_breakeven_10y, Some(2.40));
        assert_eq!(day1.pairs.len(), 4);
        // Avg of partners = (2.20 + 0.80 + 4.00 + 3.50) / 4 = 2.625
        // Diff bp = (4.20 - 2.625) * 100 = 157.5
        let avg = day1.us_minus_g10_avg_bp.expect("avg present");
        assert!((avg - 157.5).abs() < 1e-6, "got {}", avg);
        let de = day1.pairs.iter().find(|p| p.country == "DE").expect("DE pair");
        assert!((de.spread_bp - 200.0).abs() < 1e-6);

        // Day 2: US 4.25, G10 forward-filled from Day 1 → 4 pairs, avg still 2.625.
        let day2 = &snaps[1];
        assert_eq!(day2.date, "2026-04-02");
        assert_eq!(day2.pairs.len(), 4);
        let avg2 = day2.us_minus_g10_avg_bp.expect("avg present");
        assert!((avg2 - 162.5).abs() < 1e-6, "got {}", avg2); // (4.25 − 2.625)·100

        // Day 3: DE updated to 2.25; JP/GB/CA carried → still 4 pairs.
        let day3 = &snaps[2];
        assert_eq!(day3.date, "2026-04-03");
        assert_eq!(day3.pairs.len(), 4);
        let de3 = day3.pairs.iter().find(|p| p.country == "DE").expect("DE pair");
        assert!((de3.spread_bp - 205.0).abs() < 1e-6); // (4.30 − 2.25)·100
    }
}
