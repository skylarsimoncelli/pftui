use anyhow::Result;
use rust_decimal::Decimal;
use std::collections::HashSet;

use crate::data::fred;
use crate::db::backend::BackendConnection;
use crate::db::economic_cache;
use crate::db::economic_data;
use crate::db::macro_events;

pub fn run(backend: &BackendConnection, indicator: Option<&str>, json: bool) -> Result<()> {
    let mut rows = economic_data::get_all_backend(backend)?;

    // If economic_data table is empty (BLS/Brave both failed), synthesize
    // indicator rows from FRED cache data so agents always get values.
    if rows.is_empty() {
        rows = synthesize_from_fred(backend);
    }

    // Always merge FRED-only indicators that aren't already covered by BLS/Brave.
    // This ensures agents get treasury yields, yield spread, GDP, PCE, JOLTS,
    // retail sales, and industrial production even when partial BLS/Brave data exists.
    merge_fred_only_indicators(backend, &mut rows);

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
                let (final_value, source, confidence, confidence_reason) =
                    if let Some((fval, fred_date)) = &fred_override {
                        let conf = confidence_for_fred_date(fred_date);
                        let reason = confidence_reason_for_fred(fred_date, &r.indicator);
                        (
                            fval.to_string(),
                            "fred".to_string(),
                            conf,
                            reason,
                        )
                    } else {
                        let reason = confidence_reason_for_source(&r.source, &r.confidence);
                        (r.value.to_string(), r.source.clone(), r.confidence.clone(), reason)
                    };

                // Enrich previous/change from FRED history when the base row lacks them
                let (prev, chg) = if fred_override.is_some() || r.source == "fred" {
                    let fred_prev = fred_previous_for_indicator(&r.indicator, backend);
                    match (r.previous, fred_prev) {
                        (Some(p), _) => (Some(p), r.change),
                        (None, Some((prev_val, change_val))) => (Some(prev_val), Some(change_val)),
                        _ => (None, None),
                    }
                } else {
                    (r.previous, r.change)
                };

                // Count how many sources we have for cross-validation
                let sources_available = count_sources_for_indicator(&r.indicator, &fred_observations, &rows);

                let disc = discrepancies.iter().find(|d| d.indicator == r.indicator);
                let mut obj = serde_json::json!({
                    "indicator": r.indicator,
                    "display_name": display_name,
                    "value": final_value,
                    "unit": unit,
                    "source": source,
                    "confidence": confidence,
                    "confidence_reason": confidence_reason,
                    "sources_checked": sources_available,
                    "previous": prev.map(|v| v.to_string()),
                    "change": chg.map(|v| v.to_string()),
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
        "{:<28} {:>12} {:>12} {:>12}  {:<8} Source",
        "Indicator", "Value", "Previous", "Change", "Conf."
    );
    println!("{}", "─".repeat(96));

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

        // Enrich previous/change from FRED history
        let (prev, chg) = if r.source == "fred" && r.previous.is_none() {
            let fred_prev = fred_previous_for_indicator(&r.indicator, backend);
            match fred_prev {
                Some((prev_val, change_val)) => (Some(prev_val), Some(change_val)),
                None => (None, None),
            }
        } else {
            (r.previous, r.change)
        };

        let previous = prev
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());
        let change = chg
            .map(|v| format!("{:+.2}", v))
            .unwrap_or_else(|| "—".to_string());
        println!(
            "{:<28} {:>12} {:>12} {:>12}  {:<8} {}",
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

            // Skip rows that are already sourced from FRED (no cross-check needed)
            if row.source == "fred" {
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

    // PAYEMS (total employment), CPIAUCSL (CPI index), PPIACO (PPI index),
    // RSAFS (retail sales level), and INDPRO (industrial production index)
    // are raw levels, not the derived values agents expect.
    // Skip these — they need historical computation via fred_derived_value_for_indicator.
    match fred_series {
        "PAYEMS" | "CPIAUCSL" | "PPIACO" | "RSAFS" | "INDPRO" => return None,
        _ => {}
    }

    let obs = fred_obs.iter().find(|o| o.series_id == fred_series)?;
    Some((obs.value, obs.date.clone()))
}

/// Compute a FRED-derived economy value that requires historical data.
/// For PAYEMS: month-over-month change (NFP jobs added).
/// For CPIAUCSL: year-over-year percentage change (CPI inflation rate).
/// For PPIACO: year-over-year percentage change (PPI inflation rate).
/// For RSAFS: month-over-month percentage change (retail sales growth).
/// For INDPRO: year-over-year percentage change (industrial production growth).
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
        "RSAFS" => {
            // Retail Sales: compute MoM% change from FRED history
            let history = economic_cache::get_history_backend(backend, "RSAFS", 3).ok()?;
            if history.len() < 2 {
                return None;
            }
            let latest = &history[history.len() - 1];
            let previous = &history[history.len() - 2];
            if previous.value == Decimal::ZERO {
                return None;
            }
            let mom_pct =
                ((latest.value / previous.value) - Decimal::ONE) * Decimal::from(100);
            Some((mom_pct.round_dp(1), latest.date.clone()))
        }
        "INDPRO" => {
            // Industrial Production: compute YoY% from FRED history
            let history = economic_cache::get_history_backend(backend, "INDPRO", 14).ok()?;
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

/// Compute previous period value and change for a FRED-sourced indicator.
/// Returns (previous_value, change) where change = current - previous.
fn fred_previous_for_indicator(
    indicator: &str,
    backend: &BackendConnection,
) -> Option<(Decimal, Decimal)> {
    let fred_series = indicator_to_fred_series(indicator)?;

    match fred_series {
        // Direct-value series: previous is the second-most-recent observation
        "FEDFUNDS" | "UNRATE" | "ICSA" | "DGS10" | "T10Y2Y" | "GDP" | "PCE" | "JTSJOL" => {
            let history = economic_cache::get_history_backend(backend, fred_series, 3).ok()?;
            if history.len() < 2 {
                return None;
            }
            let latest = &history[history.len() - 1];
            let previous = &history[history.len() - 2];
            let change = latest.value - previous.value;
            Some((previous.value, change))
        }
        // NFP: previous is the prior month's MoM change
        "PAYEMS" => {
            let history = economic_cache::get_history_backend(backend, "PAYEMS", 4).ok()?;
            if history.len() < 3 {
                return None;
            }
            let current_mom = history[history.len() - 1].value - history[history.len() - 2].value;
            let prev_mom = history[history.len() - 2].value - history[history.len() - 3].value;
            Some((prev_mom, current_mom - prev_mom))
        }
        // CPI/PPI/INDPRO: previous is the prior month's YoY%
        "CPIAUCSL" | "PPIACO" | "INDPRO" => {
            let history = economic_cache::get_history_backend(backend, fred_series, 15).ok()?;
            if history.len() < 14 {
                return None;
            }
            let latest = &history[history.len() - 1];
            let year_ago = &history[history.len() - 13];
            let prev = &history[history.len() - 2];
            let prev_year_ago = &history[history.len() - 14];
            if year_ago.value == Decimal::ZERO || prev_year_ago.value == Decimal::ZERO {
                return None;
            }
            let current_yoy =
                ((latest.value / year_ago.value) - Decimal::ONE) * Decimal::from(100);
            let prev_yoy =
                ((prev.value / prev_year_ago.value) - Decimal::ONE) * Decimal::from(100);
            Some((prev_yoy.round_dp(1), (current_yoy - prev_yoy).round_dp(1)))
        }
        // Retail Sales: previous is the prior month's MoM%
        "RSAFS" => {
            let history = economic_cache::get_history_backend(backend, "RSAFS", 4).ok()?;
            if history.len() < 3 {
                return None;
            }
            let curr = &history[history.len() - 1];
            let prev = &history[history.len() - 2];
            let prev2 = &history[history.len() - 3];
            if prev.value == Decimal::ZERO || prev2.value == Decimal::ZERO {
                return None;
            }
            let curr_mom =
                ((curr.value / prev.value) - Decimal::ONE) * Decimal::from(100);
            let prev_mom =
                ((prev.value / prev2.value) - Decimal::ONE) * Decimal::from(100);
            Some((prev_mom.round_dp(1), (curr_mom - prev_mom).round_dp(1)))
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
        "initial_jobless_claims" => Some("ICSA"),
        "cpi" => Some("CPIAUCSL"),
        "ppi" => Some("PPIACO"),
        "treasury_10y" => Some("DGS10"),
        "yield_spread_10y2y" => Some("T10Y2Y"),
        "gdp" => Some("GDP"),
        "pce" => Some("PCE"),
        "jolts" => Some("JTSJOL"),
        "retail_sales" => Some("RSAFS"),
        "industrial_production" => Some("INDPRO"),
        "durable_goods" => Some("DGORDER"),
        "consumer_sentiment" => Some("UMCSENT"),
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

/// Build a human-readable confidence reason for a FRED-sourced indicator.
fn confidence_reason_for_fred(date: &str, indicator: &str) -> String {
    use chrono::{NaiveDate, Utc};
    let Ok(obs_date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return "FRED source, date unparseable".to_string();
    };
    let age_days = (Utc::now().date_naive() - obs_date).num_days();

    let freq_desc = match indicator_to_fred_series(indicator) {
        Some(series_id) => match fred::series_by_id(series_id) {
            Some(s) => match s.frequency {
                fred::Frequency::Daily => "daily release",
                fred::Frequency::Weekly => "weekly release",
                fred::Frequency::Monthly => "monthly release",
                fred::Frequency::Quarterly => "quarterly release",
            },
            None => "unknown frequency",
        },
        None => "unknown series",
    };

    if age_days <= 60 {
        format!(
            "FRED authoritative source, data {}d old ({}, within release cycle)",
            age_days, freq_desc
        )
    } else if age_days <= 120 {
        format!(
            "FRED authoritative source, data {}d old ({}, approaching staleness)",
            age_days, freq_desc
        )
    } else {
        format!(
            "FRED authoritative source, data {}d old ({}, stale — may not reflect current conditions)",
            age_days, freq_desc
        )
    }
}

/// Build a confidence reason for a non-FRED source.
fn confidence_reason_for_source(source: &str, confidence: &str) -> String {
    match (source, confidence) {
        ("brave", _) => "Brave web scraping — text extraction, no official API; verify independently".to_string(),
        ("bls", "high") => "Bureau of Labor Statistics — official government source, authoritative".to_string(),
        ("bls", _) => "Bureau of Labor Statistics — official source, data may be from prior release".to_string(),
        ("fred", _) => "FRED — Federal Reserve Economic Data, authoritative".to_string(),
        ("ism", _) => "ISM targeted extraction — structured parsing of ISM press releases via web search".to_string(),
        _ => format!("Source: {}, confidence: {}", source, confidence),
    }
}

/// Count how many independent sources have data for this indicator.
fn count_sources_for_indicator(
    indicator: &str,
    fred_obs: &[economic_cache::EconomicObservation],
    all_rows: &[economic_data::EconomicDataEntry],
) -> u8 {
    let mut count: u8 = 0;

    // Check if FRED has this indicator
    if let Some(fred_series) = indicator_to_fred_series(indicator) {
        if fred_obs.iter().any(|o| o.series_id == fred_series) {
            count += 1;
        }
    }

    // Check if BLS/Brave have this indicator (from economic_data table)
    for row in all_rows {
        if row.indicator == indicator && row.source != "fred" {
            count += 1;
            break; // count non-FRED as one source even if duplicated
        }
    }

    count
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
        "treasury_10y" => "10Y Treasury Yield",
        "yield_spread_10y2y" => "10Y-2Y Yield Spread",
        "gdp" => "GDP",
        "pce" => "PCE",
        "jolts" => "JOLTS Job Openings",
        "retail_sales" => "Retail Sales",
        "industrial_production" => "Industrial Production",
        "durable_goods" => "Durable Goods Orders",
        "consumer_sentiment" => "Consumer Sentiment",
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
        "retail_sales" => ("% MoM", "Retail Sales (MoM Change)"),
        "industrial_production" => ("% YoY", "Industrial Production (YoY Change)"),
        "durable_goods" => ("millions USD", "Durable Goods Orders"),
        "consumer_sentiment" => ("index", "Consumer Sentiment (UMich)"),
        _ => ("", indicator),
    }
}

/// Merge FRED-only indicators into the existing rows when they aren't already present.
///
/// This ensures agents always get the full set of FRED-backed indicators
/// (treasury yields, yield spread, GDP, PCE, JOLTS, retail sales, industrial
/// production) even when the economic_data table only has partial BLS/Brave data.
fn merge_fred_only_indicators(
    backend: &BackendConnection,
    rows: &mut Vec<economic_data::EconomicDataEntry>,
) {
    let fred_obs = economic_cache::get_all_latest_backend(backend).unwrap_or_default();
    if fred_obs.is_empty() {
        return;
    }

    let existing: HashSet<String> = rows.iter().map(|r| r.indicator.clone()).collect();
    let now = chrono::Utc::now().to_rfc3339();

    // Direct-value indicators (FRED value IS the indicator value)
    let direct_series = [
        ("FEDFUNDS", "fed_funds_rate"),
        ("UNRATE", "unemployment_rate"),
        ("ICSA", "initial_jobless_claims"),
        ("DGS10", "treasury_10y"),
        ("T10Y2Y", "yield_spread_10y2y"),
        ("GDP", "gdp"),
        ("PCE", "pce"),
        ("JTSJOL", "jolts"),
        ("DGORDER", "durable_goods"),
        ("UMCSENT", "consumer_sentiment"),
    ];

    for (series_id, indicator) in direct_series {
        if existing.contains(indicator) {
            continue;
        }
        if let Some(obs) = fred_obs.iter().find(|o| o.series_id == series_id) {
            let confidence = confidence_for_fred_date(&obs.date);
            rows.push(economic_data::EconomicDataEntry {
                indicator: indicator.to_string(),
                value: obs.value,
                previous: None,
                change: None,
                source_url: format!("https://fred.stlouisfed.org/series/{}", series_id),
                source: "fred".to_string(),
                confidence,
                fetched_at: now.clone(),
            });
        }
    }

    // Derived indicators that need historical computation
    let derived_indicators = ["nfp", "cpi", "ppi", "retail_sales", "industrial_production"];
    for indicator in derived_indicators {
        if existing.contains(indicator) {
            continue;
        }
        let series_id = match indicator_to_fred_series(indicator) {
            Some(s) => s,
            None => continue,
        };
        if let Some((value, date)) = fred_derived_value_for_indicator(indicator, backend) {
            let confidence = confidence_for_fred_date(&date);
            rows.push(economic_data::EconomicDataEntry {
                indicator: indicator.to_string(),
                value,
                previous: None,
                change: None,
                source_url: format!("https://fred.stlouisfed.org/series/{}", series_id),
                source: "fred".to_string(),
                confidence,
                fetched_at: now.clone(),
            });
        }
    }

    // Re-sort by indicator name for consistent output
    rows.sort_by(|a, b| a.indicator.cmp(&b.indicator));
}

/// Synthesize economy indicator entries from FRED cache when economic_data table is empty.
///
/// This ensures agents always get economy data even when BLS is rate-limited
/// and Brave scraping produces garbage. FRED is authoritative and already cached.
fn synthesize_from_fred(backend: &BackendConnection) -> Vec<economic_data::EconomicDataEntry> {
    let mut entries = Vec::new();
    // Delegate to merge_fred_only_indicators which handles the full set
    merge_fred_only_indicators(backend, &mut entries);
    entries
}

fn _truncate_url(url: &str, max: usize) -> String {
    if url.len() <= max {
        return url.to_string();
    }
    format!("{}...", &url[..max.saturating_sub(3)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_to_fred_series_mappings() {
        assert_eq!(indicator_to_fred_series("fed_funds_rate"), Some("FEDFUNDS"));
        assert_eq!(indicator_to_fred_series("unemployment_rate"), Some("UNRATE"));
        assert_eq!(indicator_to_fred_series("nfp"), Some("PAYEMS"));
        assert_eq!(
            indicator_to_fred_series("initial_jobless_claims"),
            Some("ICSA")
        );
        assert_eq!(indicator_to_fred_series("cpi"), Some("CPIAUCSL"));
        assert_eq!(indicator_to_fred_series("ppi"), Some("PPIACO"));
        assert_eq!(indicator_to_fred_series("treasury_10y"), Some("DGS10"));
        assert_eq!(
            indicator_to_fred_series("yield_spread_10y2y"),
            Some("T10Y2Y")
        );
        assert_eq!(indicator_to_fred_series("gdp"), Some("GDP"));
        assert_eq!(indicator_to_fred_series("pce"), Some("PCE"));
        assert_eq!(indicator_to_fred_series("jolts"), Some("JTSJOL"));
        assert_eq!(indicator_to_fred_series("retail_sales"), Some("RSAFS"));
        assert_eq!(
            indicator_to_fred_series("industrial_production"),
            Some("INDPRO")
        );
        assert_eq!(indicator_to_fred_series("durable_goods"), Some("DGORDER"));
        assert_eq!(
            indicator_to_fred_series("consumer_sentiment"),
            Some("UMCSENT")
        );
        assert_eq!(indicator_to_fred_series("bogus"), None);
    }

    #[test]
    fn confidence_reason_fred_includes_age() {
        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let reason = confidence_reason_for_fred(&today, "fed_funds_rate");
        assert!(reason.contains("FRED authoritative"));
        assert!(reason.contains("0d old"));
        assert!(reason.contains("monthly release"));
    }

    #[test]
    fn confidence_reason_fred_daily_series() {
        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let reason = confidence_reason_for_fred(&today, "treasury_10y");
        assert!(reason.contains("daily release"));
    }

    #[test]
    fn confidence_reason_brave_warns() {
        let reason = confidence_reason_for_source("brave", "low");
        assert!(reason.contains("Brave"));
        assert!(reason.contains("verify"));
    }

    #[test]
    fn confidence_reason_bls_high() {
        let reason = confidence_reason_for_source("bls", "high");
        assert!(reason.contains("Bureau of Labor Statistics"));
        assert!(reason.contains("authoritative"));
    }

    #[test]
    fn confidence_reason_ism_source() {
        let reason = confidence_reason_for_source("ism", "medium");
        assert!(reason.contains("ISM"));
        assert!(reason.contains("targeted"));
    }

    #[test]
    fn indicator_metadata_covers_durable_goods_and_sentiment() {
        let (unit, name) = indicator_metadata("durable_goods");
        assert_eq!(unit, "millions USD");
        assert!(name.contains("Durable Goods"));

        let (unit, name) = indicator_metadata("consumer_sentiment");
        assert_eq!(unit, "index");
        assert!(name.contains("Consumer Sentiment"));
    }

    #[test]
    fn display_name_covers_durable_goods_and_sentiment() {
        assert_eq!(display_name("durable_goods"), "Durable Goods Orders");
        assert_eq!(display_name("consumer_sentiment"), "Consumer Sentiment");
    }

    #[test]
    fn indicator_metadata_covers_new_indicators() {
        let (unit, name) = indicator_metadata("retail_sales");
        assert_eq!(unit, "% MoM");
        assert!(name.contains("Retail Sales"));

        let (unit, name) = indicator_metadata("industrial_production");
        assert_eq!(unit, "% YoY");
        assert!(name.contains("Industrial Production"));
    }

    #[test]
    fn display_name_covers_new_indicators() {
        assert_eq!(display_name("retail_sales"), "Retail Sales");
        assert_eq!(display_name("industrial_production"), "Industrial Production");
        assert_eq!(display_name("treasury_10y"), "10Y Treasury Yield");
        assert_eq!(display_name("yield_spread_10y2y"), "10Y-2Y Yield Spread");
        assert_eq!(display_name("gdp"), "GDP");
        assert_eq!(display_name("pce"), "PCE");
        assert_eq!(display_name("jolts"), "JOLTS Job Openings");
    }

    #[test]
    fn fred_value_skips_derived_series() {
        use rust_decimal_macros::dec;
        let obs = vec![
            economic_cache::EconomicObservation {
                series_id: "PAYEMS".to_string(),
                date: "2026-03-01".to_string(),
                value: dec!(158000),
                fetched_at: "2026-03-27T00:00:00Z".to_string(),
            },
            economic_cache::EconomicObservation {
                series_id: "CPIAUCSL".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(327.5),
                fetched_at: "2026-03-27T00:00:00Z".to_string(),
            },
            economic_cache::EconomicObservation {
                series_id: "RSAFS".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(600000),
                fetched_at: "2026-03-27T00:00:00Z".to_string(),
            },
            economic_cache::EconomicObservation {
                series_id: "INDPRO".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(105),
                fetched_at: "2026-03-27T00:00:00Z".to_string(),
            },
        ];

        // Raw-level series should return None (need derived computation)
        assert!(fred_value_for_indicator("nfp", &obs).is_none());
        assert!(fred_value_for_indicator("cpi", &obs).is_none());
        assert!(fred_value_for_indicator("retail_sales", &obs).is_none());
        assert!(fred_value_for_indicator("industrial_production", &obs).is_none());
    }

    #[test]
    fn fred_value_returns_direct_series() {
        use rust_decimal_macros::dec;
        let obs = vec![economic_cache::EconomicObservation {
            series_id: "FEDFUNDS".to_string(),
            date: "2026-03-01".to_string(),
            value: dec!(4.33),
            fetched_at: "2026-03-27T00:00:00Z".to_string(),
        }];

        let result = fred_value_for_indicator("fed_funds_rate", &obs);
        assert!(result.is_some());
        let (val, _) = result.unwrap();
        assert_eq!(val, dec!(4.33));
    }

    #[test]
    fn count_sources_fred_only() {
        use rust_decimal_macros::dec;
        let fred_obs = vec![economic_cache::EconomicObservation {
            series_id: "FEDFUNDS".to_string(),
            date: "2026-03-01".to_string(),
            value: dec!(4.33),
            fetched_at: "2026-03-27T00:00:00Z".to_string(),
        }];
        let rows: Vec<economic_data::EconomicDataEntry> = vec![];
        assert_eq!(
            count_sources_for_indicator("fed_funds_rate", &fred_obs, &rows),
            1
        );
    }

    #[test]
    fn count_sources_fred_and_brave() {
        use rust_decimal_macros::dec;
        let fred_obs = vec![economic_cache::EconomicObservation {
            series_id: "FEDFUNDS".to_string(),
            date: "2026-03-01".to_string(),
            value: dec!(4.33),
            fetched_at: "2026-03-27T00:00:00Z".to_string(),
        }];
        let rows = vec![economic_data::EconomicDataEntry {
            indicator: "fed_funds_rate".to_string(),
            value: dec!(4.50),
            previous: None,
            change: None,
            source_url: "https://example.com".to_string(),
            source: "brave".to_string(),
            confidence: "low".to_string(),
            fetched_at: "2026-03-27T00:00:00Z".to_string(),
        }];
        assert_eq!(
            count_sources_for_indicator("fed_funds_rate", &fred_obs, &rows),
            2
        );
    }
}
