use anyhow::Result;
use rust_decimal::Decimal;

use crate::data::fred;
use crate::db::backend::BackendConnection;
use crate::db::economic_cache;
use crate::db::economic_data;
use crate::db::macro_events;

/// A unified indicator row for output, combining economy_data + FRED cache.
struct UnifiedIndicator {
    indicator: String,
    value: Decimal,
    previous: Option<Decimal>,
    change: Option<Decimal>,
    source: String,
    confidence: String,
    confidence_reason: String,
    source_url: String,
    fetched_at: String,
}

pub fn run(backend: &BackendConnection, indicator: Option<&str>, json: bool) -> Result<()> {
    let rows = economic_data::get_all_backend(backend)?;
    let macro_events = macro_events::list_recent_backend(backend, 10)?;

    // Cross-reference with FRED data from economic_cache for discrepancy detection
    let fred_observations = economic_cache::get_all_latest_backend(backend).unwrap_or_default();
    let discrepancies = detect_fred_discrepancies(&rows, &fred_observations);

    // Build unified indicator list: start with economy_data rows, then add
    // FRED-only indicators that have no economy_data counterpart.
    let mut unified = build_unified_indicators(&rows, &fred_observations, backend);

    if let Some(ind) = indicator {
        let needle = ind.to_lowercase();
        unified.retain(|u| u.indicator.to_lowercase() == needle);
    }

    if json {
        let indicators: Vec<_> = unified
            .iter()
            .map(|u| {
                let (unit, display_name) = indicator_metadata(&u.indicator);
                let disc = discrepancies.iter().find(|d| d.indicator == u.indicator);
                let mut obj = serde_json::json!({
                    "indicator": u.indicator,
                    "display_name": display_name,
                    "value": u.value.to_string(),
                    "unit": unit,
                    "source": u.source,
                    "confidence": u.confidence,
                    "confidence_reason": u.confidence_reason,
                    "previous": u.previous.map(|v| v.to_string()),
                    "change": u.change.map(|v| v.to_string()),
                    "source_url": u.source_url,
                    "fetched_at": u.fetched_at,
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

    if unified.is_empty() {
        println!("No economy data available. Run `pftui refresh` first.");
        return Ok(());
    }

    println!(
        "{:<24} {:>12} {:>12} {:>12}  {:<8} Source",
        "Indicator", "Value", "Previous", "Change", "Conf."
    );
    println!("{}", "─".repeat(92));

    for u in &unified {
        let display_val = format!("{:.2}", u.value);
        let previous = u
            .previous
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "—".to_string());
        let change = u
            .change
            .map(|v| format!("{:+.2}", v))
            .unwrap_or_else(|| "—".to_string());
        println!(
            "{:<24} {:>12} {:>12} {:>12}  {:<8} {}",
            display_name(&u.indicator),
            display_val,
            previous,
            change,
            u.confidence,
            u.source.to_uppercase(),
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

/// Build a unified list of indicators from economy_data + FRED cache.
///
/// For each economy_data row, check if FRED has a better value and upgrade.
/// Then, for FRED-only indicators that have no economy_data counterpart,
/// synthesize them directly from FRED cache. This ensures agents always get
/// the maximum set of indicators even when Brave/BLS scraping fails.
fn build_unified_indicators(
    rows: &[economic_data::EconomicDataEntry],
    fred_obs: &[economic_cache::EconomicObservation],
    backend: &BackendConnection,
) -> Vec<UnifiedIndicator> {
    let mut unified = Vec::new();
    let mut seen_indicators: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Phase 1: Process economy_data rows, upgrading with FRED where available
    for r in rows {
        seen_indicators.insert(r.indicator.clone());

        let fred_override = fred_value_for_indicator(&r.indicator, fred_obs)
            .or_else(|| fred_derived_value_for_indicator(&r.indicator, backend));

        if let Some((fval, fred_date)) = fred_override {
            let conf = confidence_for_fred_date(&fred_date);
            let reason = confidence_reason_fred(&fred_date);
            unified.push(UnifiedIndicator {
                indicator: r.indicator.clone(),
                value: fval,
                previous: r.previous,
                change: r.change,
                source: "fred".to_string(),
                confidence: conf,
                confidence_reason: reason,
                source_url: format!("https://fred.stlouisfed.org/series/{}", indicator_to_fred_series(&r.indicator).unwrap_or("UNKNOWN")),
                fetched_at: r.fetched_at.clone(),
            });
        } else {
            let reason = confidence_reason_non_fred(&r.source, &r.confidence);
            unified.push(UnifiedIndicator {
                indicator: r.indicator.clone(),
                value: r.value,
                previous: r.previous,
                change: r.change,
                source: r.source.clone(),
                confidence: r.confidence.clone(),
                confidence_reason: reason,
                source_url: r.source_url.clone(),
                fetched_at: r.fetched_at.clone(),
            });
        }
    }

    // Phase 2: Synthesize FRED-only indicators not present in economy_data.
    // These are indicators we can derive entirely from the FRED cache.
    let fred_only_indicators = [
        // Direct-value FRED series (value is used as-is)
        ("fed_funds_rate", "FEDFUNDS"),
        ("unemployment_rate", "UNRATE"),
        ("initial_jobless_claims", "ICSA"),
        ("consumer_sentiment", "UMCSENT"),
        ("treasury_10y", "DGS10"),
        ("yield_spread_10y2y", "T10Y2Y"),
        ("gdp", "GDP"),
        ("pce", "PCE"),
        ("jolts", "JTSJOL"),
        // Derived series handled by fred_derived_value_for_indicator
        ("cpi", "CPIAUCSL"),
        ("ppi", "PPIACO"),
        ("nfp", "PAYEMS"),
        ("retail_sales", "RSAFS"),
        ("industrial_production", "INDPRO"),
    ];

    for (indicator, fred_series_id) in &fred_only_indicators {
        if seen_indicators.contains(*indicator) {
            continue;
        }

        // Try direct FRED value first, then derived
        let fred_val = fred_value_for_indicator(indicator, fred_obs)
            .or_else(|| fred_derived_value_for_indicator(indicator, backend));

        if let Some((value, fred_date)) = fred_val {
            let conf = confidence_for_fred_date(&fred_date);
            let reason = confidence_reason_fred(&fred_date);
            // Try to compute previous + change from FRED history
            let (previous, change) = fred_previous_and_change(indicator, fred_series_id, backend);
            unified.push(UnifiedIndicator {
                indicator: indicator.to_string(),
                value,
                previous,
                change,
                source: "fred".to_string(),
                confidence: conf,
                confidence_reason: reason,
                source_url: format!("https://fred.stlouisfed.org/series/{}", fred_series_id),
                fetched_at: fred_date,
            });
            seen_indicators.insert(indicator.to_string());
        }
    }

    // Sort by a canonical order for consistent output
    let order = indicator_sort_order();
    unified.sort_by(|a, b| {
        let a_ord = order.iter().position(|i| *i == a.indicator).unwrap_or(999);
        let b_ord = order.iter().position(|i| *i == b.indicator).unwrap_or(999);
        a_ord.cmp(&b_ord)
    });

    unified
}

/// Canonical sort order for economy indicators.
fn indicator_sort_order() -> Vec<&'static str> {
    vec![
        "fed_funds_rate",
        "cpi",
        "ppi",
        "unemployment_rate",
        "nfp",
        "initial_jobless_claims",
        "pmi_manufacturing",
        "pmi_services",
        "retail_sales",
        "industrial_production",
        "consumer_sentiment",
        "gdp",
        "pce",
        "jolts",
        "treasury_10y",
        "yield_spread_10y2y",
    ]
}

/// Compute previous value and change from FRED history for synthesized indicators.
fn fred_previous_and_change(
    indicator: &str,
    fred_series_id: &str,
    backend: &BackendConnection,
) -> (Option<Decimal>, Option<Decimal>) {
    // For derived indicators (NFP, CPI, PPI, retail_sales, industrial_production),
    // we need multiple history points to compute both current and previous derived values.
    match indicator {
        "nfp" => {
            let history = economic_cache::get_history_backend(backend, "PAYEMS", 4).ok();
            if let Some(h) = history {
                if h.len() >= 3 {
                    let current_mom = h[h.len() - 1].value - h[h.len() - 2].value;
                    let prev_mom = h[h.len() - 2].value - h[h.len() - 3].value;
                    return (Some(prev_mom), Some(current_mom - prev_mom));
                }
            }
            (None, None)
        }
        "cpi" | "ppi" => {
            let series = if indicator == "cpi" { "CPIAUCSL" } else { "PPIACO" };
            let history = economic_cache::get_history_backend(backend, series, 15).ok();
            if let Some(h) = history {
                if h.len() >= 14 {
                    let latest = &h[h.len() - 1];
                    let year_ago = &h[h.len() - 13];
                    let prev = &h[h.len() - 2];
                    let prev_year_ago = &h[h.len() - 14];
                    if year_ago.value != Decimal::ZERO && prev_year_ago.value != Decimal::ZERO {
                        let current_yoy = ((latest.value / year_ago.value) - Decimal::ONE) * Decimal::from(100);
                        let prev_yoy = ((prev.value / prev_year_ago.value) - Decimal::ONE) * Decimal::from(100);
                        return (Some(prev_yoy.round_dp(1)), Some((current_yoy - prev_yoy).round_dp(1)));
                    }
                }
            }
            (None, None)
        }
        "retail_sales" | "industrial_production" => {
            let history = economic_cache::get_history_backend(backend, fred_series_id, 4).ok();
            if let Some(h) = history {
                if h.len() >= 3 {
                    let latest = &h[h.len() - 1];
                    let prev = &h[h.len() - 2];
                    let prev_prev = &h[h.len() - 3];
                    if prev.value != Decimal::ZERO && prev_prev.value != Decimal::ZERO {
                        let current_mom_pct = ((latest.value / prev.value) - Decimal::ONE) * Decimal::from(100);
                        let prev_mom_pct = ((prev.value / prev_prev.value) - Decimal::ONE) * Decimal::from(100);
                        return (Some(prev_mom_pct.round_dp(1)), Some((current_mom_pct - prev_mom_pct).round_dp(1)));
                    }
                }
            }
            (None, None)
        }
        _ => {
            // Direct-value indicators: get previous observation from FRED
            let history = economic_cache::get_history_backend(backend, fred_series_id, 3).ok();
            if let Some(h) = history {
                if h.len() >= 2 {
                    let current = &h[h.len() - 1];
                    let prev = &h[h.len() - 2];
                    return (Some(prev.value), Some(current.value - prev.value));
                }
            }
            (None, None)
        }
    }
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
                || fred_indicator == "RSAFS"
                || fred_indicator == "INDPRO"
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

    // These series store raw levels, not the derived values agents expect.
    // Skip — they need historical computation via fred_derived_value_for_indicator.
    match fred_series {
        "PAYEMS" | "CPIAUCSL" | "PPIACO" | "RSAFS" | "INDPRO" => return None,
        _ => {}
    }

    let obs = fred_obs.iter().find(|o| o.series_id == fred_series)?;
    Some((obs.value, obs.date.clone()))
}

/// Compute a FRED-derived economy value that requires historical data.
///
/// For PAYEMS: month-over-month change (NFP jobs added).
/// For CPIAUCSL: year-over-year percentage change (CPI inflation rate).
/// For PPIACO: year-over-year percentage change (PPI inflation rate).
/// For RSAFS: month-over-month percentage change (Retail Sales growth).
/// For INDPRO: month-over-month percentage change (Industrial Production growth).
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
            // Retail Sales: compute MoM% from FRED history
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
            // Industrial Production: compute MoM% from FRED history
            let history = economic_cache::get_history_backend(backend, "INDPRO", 3).ok()?;
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
        _ => None,
    }
}

/// Map economy indicator names to FRED series IDs for cross-referencing.
fn indicator_to_fred_series(indicator: &str) -> Option<&'static str> {
    match indicator {
        "fed_funds_rate" => Some("FEDFUNDS"),
        "unemployment_rate" => Some("UNRATE"),
        "nfp" => Some("PAYEMS"),
        // ISM PMI is proprietary, not available on FRED
        "initial_jobless_claims" => Some("ICSA"),
        "cpi" => Some("CPIAUCSL"),
        "ppi" => Some("PPIACO"),
        "retail_sales" => Some("RSAFS"),
        "industrial_production" => Some("INDPRO"),
        "consumer_sentiment" => Some("UMCSENT"),
        "treasury_10y" => Some("DGS10"),
        "yield_spread_10y2y" => Some("T10Y2Y"),
        "gdp" => Some("GDP"),
        "pce" => Some("PCE"),
        "jolts" => Some("JTSJOL"),
        _ => None,
    }
}

/// Determine confidence for a FRED-sourced indicator.
///
/// FRED data is inherently authoritative — source reliability is high regardless
/// of age. Confidence reflects whether the data is current relative to the
/// indicator's release frequency (monthly, weekly, etc.).
fn confidence_for_fred_date(date: &str) -> String {
    use chrono::{NaiveDate, Utc};
    let Ok(obs_date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return "medium".to_string();
    };
    let age_days = (Utc::now().date_naive() - obs_date).num_days();
    if age_days <= 60 {
        "high".to_string()
    } else if age_days <= 120 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

/// Human-readable explanation of why a FRED indicator has its confidence level.
fn confidence_reason_fred(date: &str) -> String {
    use chrono::{NaiveDate, Utc};
    let Ok(obs_date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return "FRED observation date could not be parsed".to_string();
    };
    let age_days = (Utc::now().date_naive() - obs_date).num_days();
    if age_days <= 60 {
        format!(
            "FRED authoritative source, observation {}d old (within 2 release cycles)",
            age_days
        )
    } else if age_days <= 120 {
        format!(
            "FRED authoritative source, observation {}d old (2-4 release cycles behind)",
            age_days
        )
    } else {
        format!(
            "FRED authoritative source but observation {}d old (>4 release cycles, consider web_search supplement)",
            age_days
        )
    }
}

/// Confidence reason for non-FRED sources.
fn confidence_reason_non_fred(source: &str, confidence: &str) -> String {
    match (source, confidence) {
        ("brave", _) => "Brave web scraping — text extraction may be inaccurate, recommend cross-validation with FRED or BLS".to_string(),
        ("bls", "high") => "Bureau of Labor Statistics official API — authoritative for employment/CPI data".to_string(),
        ("bls", _) => "Bureau of Labor Statistics API — data may be stale, check release schedule".to_string(),
        (_, "high") => format!("Source '{}' rated high confidence", source),
        (_, "low") => format!("Source '{}' rated low confidence — consider supplementing with web_search", source),
        _ => format!("Source '{}', confidence '{}'", source, confidence),
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
        "retail_sales" => "Retail Sales",
        "industrial_production" => "Industrial Production",
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
        "nfp" => ("thousands", "Nonfarm Payrolls (MoM Change)"),
        "pmi_manufacturing" => ("index (0-100)", "ISM Manufacturing PMI"),
        "pmi_services" => ("index (0-100)", "ISM Services PMI"),
        "fed_funds_rate" => ("%", "Federal Funds Rate"),
        "initial_jobless_claims" => ("claims", "Initial Jobless Claims"),
        "ppi" => ("% YoY", "PPI (Producer Prices YoY)"),
        "retail_sales" => ("% MoM", "Retail Sales (MoM Change)"),
        "industrial_production" => ("% MoM", "Industrial Production (MoM Change)"),
        "consumer_sentiment" => ("index", "Consumer Sentiment (UMich)"),
        "gdp" => ("billions USD", "Gross Domestic Product"),
        "pce" => ("billions USD", "Personal Consumption Expenditures"),
        "jolts" => ("thousands", "JOLTS Job Openings"),
        "treasury_10y" => ("%", "10-Year Treasury Yield"),
        "yield_spread_10y2y" => ("%", "10Y-2Y Yield Spread"),
        _ => ("", indicator),
    }
}

/// Synthesize economy indicator entries from FRED cache when economic_data table is empty.
///
fn _truncate_url(url: &str, max: usize) -> String {
    if url.len() <= max {
        return url.to_string();
    }
    format!("{}...", &url[..max.saturating_sub(3)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn indicator_to_fred_includes_new_series() {
        assert_eq!(indicator_to_fred_series("retail_sales"), Some("RSAFS"));
        assert_eq!(indicator_to_fred_series("industrial_production"), Some("INDPRO"));
        assert_eq!(indicator_to_fred_series("consumer_sentiment"), Some("UMCSENT"));
    }

    #[test]
    fn indicator_to_fred_existing_series() {
        assert_eq!(indicator_to_fred_series("fed_funds_rate"), Some("FEDFUNDS"));
        assert_eq!(indicator_to_fred_series("unemployment_rate"), Some("UNRATE"));
        assert_eq!(indicator_to_fred_series("nfp"), Some("PAYEMS"));
        assert_eq!(indicator_to_fred_series("cpi"), Some("CPIAUCSL"));
        assert_eq!(indicator_to_fred_series("ppi"), Some("PPIACO"));
        assert_eq!(indicator_to_fred_series("initial_jobless_claims"), Some("ICSA"));
        assert_eq!(indicator_to_fred_series("treasury_10y"), Some("DGS10"));
        assert_eq!(indicator_to_fred_series("yield_spread_10y2y"), Some("T10Y2Y"));
        assert_eq!(indicator_to_fred_series("gdp"), Some("GDP"));
        assert_eq!(indicator_to_fred_series("pce"), Some("PCE"));
        assert_eq!(indicator_to_fred_series("jolts"), Some("JTSJOL"));
    }

    #[test]
    fn indicator_to_fred_unknown() {
        assert_eq!(indicator_to_fred_series("bogus"), None);
        assert_eq!(indicator_to_fred_series(""), None);
    }

    #[test]
    fn confidence_reason_fred_recent() {
        // Use today's date — should be "high" with recent message
        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let reason = confidence_reason_fred(&today);
        assert!(reason.contains("FRED authoritative"));
        assert!(reason.contains("within 2 release cycles"));
    }

    #[test]
    fn confidence_reason_fred_stale() {
        let reason = confidence_reason_fred("2020-01-01");
        assert!(reason.contains("FRED authoritative"));
        assert!(reason.contains("consider web_search supplement"));
    }

    #[test]
    fn confidence_reason_non_fred_brave() {
        let reason = confidence_reason_non_fred("brave", "low");
        assert!(reason.contains("Brave web scraping"));
        assert!(reason.contains("cross-validation"));
    }

    #[test]
    fn confidence_reason_non_fred_bls() {
        let reason = confidence_reason_non_fred("bls", "high");
        assert!(reason.contains("Bureau of Labor Statistics"));
    }

    #[test]
    fn indicator_metadata_new_indicators() {
        let (unit, name) = indicator_metadata("retail_sales");
        assert_eq!(unit, "% MoM");
        assert!(name.contains("Retail Sales"));

        let (unit, name) = indicator_metadata("industrial_production");
        assert_eq!(unit, "% MoM");
        assert!(name.contains("Industrial Production"));

        let (unit, name) = indicator_metadata("consumer_sentiment");
        assert_eq!(unit, "index");
        assert!(name.contains("Consumer Sentiment"));
    }

    #[test]
    fn display_name_new_indicators() {
        assert_eq!(display_name("retail_sales"), "Retail Sales");
        assert_eq!(display_name("industrial_production"), "Industrial Production");
        assert_eq!(display_name("consumer_sentiment"), "Consumer Sentiment");
    }

    #[test]
    fn indicator_sort_order_has_all_core() {
        let order = indicator_sort_order();
        assert!(order.contains(&"fed_funds_rate"));
        assert!(order.contains(&"cpi"));
        assert!(order.contains(&"retail_sales"));
        assert!(order.contains(&"industrial_production"));
        assert!(order.contains(&"consumer_sentiment"));
    }

    #[test]
    fn fred_value_skips_level_series() {
        // RSAFS and INDPRO should be skipped (raw levels, need derived computation)
        let obs = vec![
            economic_cache::EconomicObservation {
                series_id: "RSAFS".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(600000),
                fetched_at: "2026-03-27".to_string(),
            },
            economic_cache::EconomicObservation {
                series_id: "INDPRO".to_string(),
                date: "2026-02-01".to_string(),
                value: dec!(103.5),
                fetched_at: "2026-03-27".to_string(),
            },
        ];
        assert!(fred_value_for_indicator("retail_sales", &obs).is_none());
        assert!(fred_value_for_indicator("industrial_production", &obs).is_none());
    }

    #[test]
    fn fred_value_returns_direct_value() {
        let obs = vec![economic_cache::EconomicObservation {
            series_id: "FEDFUNDS".to_string(),
            date: "2026-03-01".to_string(),
            value: dec!(4.50),
            fetched_at: "2026-03-27".to_string(),
        }];
        let result = fred_value_for_indicator("fed_funds_rate", &obs);
        assert!(result.is_some());
        let (val, date) = result.unwrap();
        assert_eq!(val, dec!(4.50));
        assert_eq!(date, "2026-03-01");
    }
}
