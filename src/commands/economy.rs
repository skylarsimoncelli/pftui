use anyhow::Result;
use rust_decimal::Decimal;

use crate::data::fred;
use crate::db::backend::BackendConnection;
use crate::db::economic_cache;
use crate::db::economic_data;
use crate::db::macro_events;

pub fn run(backend: &BackendConnection, indicator: Option<&str>, json: bool) -> Result<()> {
    let mut rows = economic_data::get_all_backend(backend)?;
    let macro_events = macro_events::list_recent_backend(backend, 10)?;

    // Cross-reference with FRED data from economic_cache for discrepancy detection
    let fred_observations = economic_cache::get_all_latest_backend(backend).unwrap_or_default();
    let discrepancies = detect_fred_discrepancies(&rows, &fred_observations);

    if let Some(ind) = indicator {
        let needle = ind.to_lowercase();
        rows.retain(|r| r.indicator.to_lowercase() == needle);
    }

    if json {
        let indicators: Vec<_> = rows
            .iter()
            .map(|r| {
                let (unit, display_name) = indicator_metadata(&r.indicator);
                // Check if FRED has a more authoritative value for this indicator.
                // Try direct FRED value first, then derived (for PAYEMS/CPIAUCSL).
                let fred_override = fred_value_for_indicator(&r.indicator, &fred_observations)
                    .or_else(|| fred_derived_value_for_indicator(&r.indicator, backend));
                let (final_value, source, confidence) =
                    if let Some((fval, fred_date)) = fred_override {
                        // FRED is more authoritative; use it
                        (
                            fval.to_string(),
                            "fred".to_string(),
                            confidence_for_fred_date(&fred_date),
                        )
                    } else {
                        (r.value.to_string(), r.source.clone(), r.confidence.clone())
                    };

                let disc = discrepancies.iter().find(|d| d.indicator == r.indicator);
                let mut obj = serde_json::json!({
                    "indicator": r.indicator,
                    "display_name": display_name,
                    "value": final_value,
                    "unit": unit,
                    "source": source,
                    "confidence": confidence,
                    "previous": r.previous.map(|v| v.to_string()),
                    "change": r.change.map(|v| v.to_string()),
                    "source_url": r.source_url,
                    "fetched_at": r.fetched_at,
                });
                if let Some(d) = disc {
                    obj["discrepancy"] = serde_json::json!({
                        "other_source": d.other_source,
                        "other_value": d.other_value.to_string(),
                        "diff_pct": d.diff_pct.to_string(),
                    });
                }
                obj
            })
            .collect();
        let surprises: Vec<_> = macro_events
            .iter()
            .map(|event| {
                let name = fred::series_by_id(&event.series_id)
                    .map(|series| series.name)
                    .unwrap_or(event.series_id.as_str());
                serde_json::json!({
                    "series_id": event.series_id,
                    "series_name": name,
                    "event_date": event.event_date,
                    "expected": event.expected.to_string(),
                    "actual": event.actual.to_string(),
                    "surprise_pct": event.surprise_pct.to_string(),
                    "created_at": event.created_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "indicators": indicators,
                "macro_events": surprises,
            }))?
        );
        return Ok(());
    }

    if rows.is_empty() {
        println!("No economy data available. Run `pftui refresh` first.");
        return Ok(());
    }

    println!(
        "{:<24} {:>12} {:>12} {:>12}  {:<8} Source",
        "Indicator", "Value", "Previous", "Change", "Conf."
    );
    println!("{}", "─".repeat(92));

    for r in &rows {
        let fred_override = fred_value_for_indicator(&r.indicator, &fred_observations)
            .or_else(|| fred_derived_value_for_indicator(&r.indicator, backend));
        let (display_val, source_label, conf) = if let Some((fval, fred_date)) = fred_override {
            (
                format!("{:.2}", fval),
                "FRED".to_string(),
                confidence_for_fred_date(&fred_date),
            )
        } else {
            (
                format!("{:.2}", r.value),
                r.source.to_uppercase(),
                r.confidence.clone(),
            )
        };

        let previous = r
            .previous
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());
        let change = r
            .change
            .map(|v| format!("{:+.2}", v))
            .unwrap_or_else(|| "—".to_string());
        println!(
            "{:<24} {:>12} {:>12} {:>12}  {:<8} {}",
            display_name(&r.indicator),
            display_val,
            previous,
            change,
            conf,
            source_label,
        );
    }

    if !discrepancies.is_empty() {
        println!();
        println!("⚠ Source discrepancies detected:");
        for d in &discrepancies {
            println!(
                "  {} — {} ({}) vs {} ({}) — diff {:.1}%",
                display_name(&d.indicator),
                d.preferred_source,
                d.preferred_value,
                d.other_source,
                d.other_value,
                d.diff_pct
            );
        }
    }

    if !macro_events.is_empty() {
        println!();
        println!("Recent macro surprises:");
        for event in macro_events.iter().take(5) {
            let name = fred::series_by_id(&event.series_id)
                .map(|series| series.name)
                .unwrap_or(event.series_id.as_str());
            println!(
                "  {} ({}) expected {} actual {} surprise {:+}%",
                name, event.event_date, event.expected, event.actual, event.surprise_pct
            );
        }
    }

    Ok(())
}

/// A discrepancy between economy data (Brave/BLS) and FRED for the same indicator.
struct Discrepancy {
    indicator: String,
    preferred_source: String,
    preferred_value: rust_decimal::Decimal,
    other_source: String,
    other_value: rust_decimal::Decimal,
    diff_pct: rust_decimal::Decimal,
}

/// Detect discrepancies between economy data table values and FRED cache values.
///
/// Skips PAYEMS and CPIAUCSL since those are raw levels in FRED but derived
/// values (MoM change, YoY%) in the economy data table — comparing them
/// directly would always produce a false discrepancy.
fn detect_fred_discrepancies(
    rows: &[economic_data::EconomicDataEntry],
    fred_obs: &[economic_cache::EconomicObservation],
) -> Vec<Discrepancy> {
    let mut discrepancies = Vec::new();

    for row in rows {
        if let Some(fred_indicator) = indicator_to_fred_series(&row.indicator) {
            // Skip series where FRED stores raw levels but economy table has
            // derived values — these are not directly comparable.
            if fred_indicator == "PAYEMS"
                || fred_indicator == "CPIAUCSL"
                || fred_indicator == "PPIACO"
            {
                continue;
            }

            if let Some(obs) = fred_obs.iter().find(|o| o.series_id == fred_indicator) {
                let diff_abs = (row.value - obs.value).abs();
                let denominator = obs.value.abs();
                if denominator > Decimal::ZERO {
                    let diff_pct = (diff_abs * Decimal::from(100)) / denominator;
                    // Flag differences > 0.5%
                    if diff_pct > Decimal::new(5, 1) {
                        discrepancies.push(Discrepancy {
                            indicator: row.indicator.clone(),
                            // FRED is preferred (more authoritative)
                            preferred_source: "FRED".to_string(),
                            preferred_value: obs.value,
                            other_source: row.source.to_uppercase(),
                            other_value: row.value,
                            diff_pct: diff_pct.round_dp(1),
                        });
                    }
                }
            }
        }
    }

    discrepancies
}

/// Get the FRED value for an economy indicator if available.
/// Returns (value, date) from FRED cache.
///
/// For indicators where the FRED series is a raw level (not a rate), this
/// returns None — those need historical data to compute derived values.
/// Use `fred_derived_value_for_indicator` with backend access instead.
fn fred_value_for_indicator(
    indicator: &str,
    fred_obs: &[economic_cache::EconomicObservation],
) -> Option<(rust_decimal::Decimal, String)> {
    let fred_series = indicator_to_fred_series(indicator)?;

    // PAYEMS (total employment), CPIAUCSL (CPI index), and PPIACO (PPI index)
    // are raw levels, not the derived values agents expect (MoM change, YoY%).
    // Skip these — they need historical computation via fred_derived_value_for_indicator.
    match fred_series {
        "PAYEMS" | "CPIAUCSL" | "PPIACO" => return None,
        _ => {}
    }

    let obs = fred_obs.iter().find(|o| o.series_id == fred_series)?;
    Some((obs.value, obs.date.clone()))
}

/// Compute a FRED-derived economy value that requires historical data.
/// For PAYEMS: month-over-month change (NFP jobs added).
/// For CPIAUCSL: year-over-year percentage change (CPI inflation rate).
fn fred_derived_value_for_indicator(
    indicator: &str,
    backend: &BackendConnection,
) -> Option<(Decimal, String)> {
    let fred_series = indicator_to_fred_series(indicator)?;

    match fred_series {
        "PAYEMS" => {
            // NFP: compute month-over-month change from FRED history
            let history = economic_cache::get_history_backend(backend, "PAYEMS", 3).ok()?;
            if history.len() < 2 {
                return None;
            }
            // history is ascending by date
            let latest = &history[history.len() - 1];
            let previous = &history[history.len() - 2];
            let mom_change = latest.value - previous.value;
            Some((mom_change, latest.date.clone()))
        }
        "CPIAUCSL" => {
            // CPI: compute YoY% from FRED history (need 13+ months)
            let history = economic_cache::get_history_backend(backend, "CPIAUCSL", 14).ok()?;
            if history.len() < 13 {
                return None;
            }
            // history is ascending by date; latest is last element
            let latest = &history[history.len() - 1];
            let year_ago = &history[history.len() - 13];
            if year_ago.value == Decimal::ZERO {
                return None;
            }
            let yoy =
                ((latest.value / year_ago.value) - Decimal::ONE) * Decimal::from(100);
            Some((yoy.round_dp(1), latest.date.clone()))
        }
        "PPIACO" => {
            // PPI: compute YoY% from FRED history (need 13+ months)
            let history = economic_cache::get_history_backend(backend, "PPIACO", 14).ok()?;
            if history.len() < 13 {
                return None;
            }
            let latest = &history[history.len() - 1];
            let year_ago = &history[history.len() - 13];
            if year_ago.value == Decimal::ZERO {
                return None;
            }
            let yoy =
                ((latest.value / year_ago.value) - Decimal::ONE) * Decimal::from(100);
            Some((yoy.round_dp(1), latest.date.clone()))
        }
        _ => None,
    }
}

/// Map economy indicator names to FRED series IDs for cross-referencing.
fn indicator_to_fred_series(indicator: &str) -> Option<&'static str> {
    match indicator {
        "fed_funds_rate" => Some("FEDFUNDS"),
        "unemployment_rate" => Some("UNRATE"),
        "nfp" => Some("PAYEMS"),
        "pmi_manufacturing" => Some("NAPM"),
        "initial_jobless_claims" => Some("ICSA"),
        "cpi" => Some("CPIAUCSL"),
        "ppi" => Some("PPIACO"),
        _ => None,
    }
}

/// Determine confidence for a FRED-sourced indicator.
///
/// FRED data is inherently authoritative — source reliability is high regardless
/// of age. Confidence reflects whether the data is current relative to the
/// indicator's release frequency (monthly, weekly, etc.).
///
/// Most economic indicators are released monthly (CPI, NFP, unemployment, PMI,
/// PPI, fed funds). Within two release cycles (60 days) the data is current.
/// Weekly indicators (jobless claims) use a tighter window.
fn confidence_for_fred_date(date: &str) -> String {
    use chrono::{NaiveDate, Utc};
    let Ok(obs_date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return "medium".to_string();
    };
    let age_days = (Utc::now().date_naive() - obs_date).num_days();
    // FRED data is authoritative. Most economic indicators update monthly,
    // so data within 60 days (two release cycles) is fully current.
    // Even older FRED data is more reliable than Brave scraping.
    if age_days <= 60 {
        "high".to_string()
    } else if age_days <= 120 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn display_name(indicator: &str) -> &str {
    match indicator {
        "cpi" => "CPI",
        "unemployment_rate" => "Unemployment",
        "nfp" => "Nonfarm Payrolls",
        "pmi_manufacturing" => "PMI Manufacturing",
        "pmi_services" => "PMI Services",
        "fed_funds_rate" => "Fed Funds Rate",
        "initial_jobless_claims" => "Initial Jobless Claims",
        "ppi" => "PPI",
        _ => indicator,
    }
}

/// Return (unit, display_name) for an economy indicator.
/// Units help agents and users interpret raw values correctly.
fn indicator_metadata(indicator: &str) -> (&str, &str) {
    match indicator {
        "cpi" => ("% YoY", "CPI (YoY Inflation)"),
        "unemployment_rate" => ("%", "Unemployment Rate"),
        "nfp" => ("thousands", "Nonfarm Payrolls"),
        "pmi_manufacturing" => ("index (0-100)", "ISM Manufacturing PMI"),
        "pmi_services" => ("index (0-100)", "ISM Services PMI"),
        "fed_funds_rate" => ("%", "Federal Funds Rate"),
        "initial_jobless_claims" => ("claims", "Initial Jobless Claims"),
        "ppi" => ("% YoY", "PPI (Producer Prices)"),
        "gdp" => ("billions USD", "Gross Domestic Product"),
        "pce" => ("billions USD", "Personal Consumption Expenditures"),
        "jolts" => ("thousands", "JOLTS Job Openings"),
        "treasury_10y" => ("%", "10-Year Treasury Yield"),
        "yield_spread_10y2y" => ("%", "10Y-2Y Yield Spread"),
        _ => ("", indicator),
    }
}

fn _truncate_url(url: &str, max: usize) -> String {
    if url.len() <= max {
        return url.to_string();
    }
    format!("{}...", &url[..max.saturating_sub(3)])
}
