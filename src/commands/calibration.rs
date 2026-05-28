//! `pftui analytics calibration` — compare pftui probabilities vs realised signals.
//!
//! For each mapped scenario↔contract pair, shows the divergence between
//! pftui's scenario probability (set by agents/user) and the prediction
//! market's crowd-calibrated probability (from Polymarket contracts).
//! It also reports recent prediction accuracy calibration by timeframe layer
//! and conviction band for charting in daily reports.
//!
//! Flags divergences >15pp as significant. Designed for agent consumption:
//! agents explain divergences between their estimates and market consensus.

use anyhow::Result;
use chrono::{Duration, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

use crate::db::backend::BackendConnection;
use crate::db::scenario_contract_mappings;
use crate::db::user_predictions::{self, UserPrediction};

// ── JSON output structs ────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CalibrationReport {
    /// Total number of mapped scenario↔contract pairs
    total_mappings: usize,
    /// Pairs with |divergence| > threshold
    significant_divergences: usize,
    /// Divergence threshold in percentage points
    threshold_pp: f64,
    /// All calibration entries, sorted by divergence magnitude (largest first)
    entries: Vec<CalibrationEntry>,
    /// Summary statistics
    summary: CalibrationSummary,
    /// Realised hit rate vs predicted confidence for recent user predictions
    prediction_accuracy: PredictionAccuracyReport,
    /// Strict per-layer calibration with sample size and uncertainty
    #[serde(skip_serializing_if = "Option::is_none")]
    by_layer: Option<PredictionLayerCalibrationReport>,
}

#[derive(Debug, Serialize)]
struct CalibrationEntry {
    scenario_id: i64,
    scenario_name: String,
    /// pftui scenario probability (0–100)
    scenario_probability_pct: f64,
    contract_id: String,
    contract_question: String,
    contract_category: String,
    /// Prediction market probability (0–100)
    market_probability_pct: f64,
    /// scenario_probability - market_probability (in percentage points)
    divergence_pp: f64,
    /// |divergence_pp|
    abs_divergence_pp: f64,
    /// Whether this divergence exceeds the threshold
    significant: bool,
    /// Human-readable interpretation
    interpretation: String,
}

#[derive(Debug, Serialize)]
struct CalibrationSummary {
    /// Mean absolute divergence across all pairs
    mean_abs_divergence_pp: f64,
    /// Median absolute divergence
    median_abs_divergence_pp: f64,
    /// Number of pairs where pftui is more bullish than the market
    overestimates: usize,
    /// Number of pairs where pftui is less bullish than the market
    underestimates: usize,
    /// Number of pairs in agreement (within threshold)
    aligned: usize,
}

#[derive(Debug, Serialize)]
struct PredictionAccuracyReport {
    window_days: i64,
    total_scored: usize,
    scored_with_confidence: usize,
    rows: Vec<PredictionAccuracyRow>,
}

#[derive(Debug, Clone, Serialize)]
struct PredictionAccuracyRow {
    layer: String,
    band: String,
    predicted_conf_mean: f64,
    realised_hit_rate: f64,
    strict_hit_rate: f64,
    strict_hit_rate_pct: f64,
    n: usize,
    sigma: f64,
    sigma_pp: f64,
    low_sample: bool,
    correct: usize,
    partial: usize,
    wrong: usize,
    /// realised_hit_rate - predicted_conf_mean, in percentage points
    miscalibration_pp: f64,
}

#[derive(Debug, Clone, Serialize)]
struct PredictionLayerCalibrationReport {
    window_days: i64,
    rows: Vec<PredictionLayerCalibrationRow>,
}

#[derive(Debug, Clone, Serialize)]
struct PredictionLayerCalibrationRow {
    layer: String,
    strict_hit_rate: f64,
    strict_hit_rate_pct: f64,
    n: usize,
    sigma: f64,
    sigma_pp: f64,
    low_sample: bool,
    correct: usize,
    partial: usize,
    wrong: usize,
    bin_breakdown: Vec<PredictionLayerCalibrationBin>,
}

#[derive(Debug, Clone, Serialize)]
struct PredictionLayerCalibrationBin {
    band: String,
    strict_hit_rate: f64,
    strict_hit_rate_pct: f64,
    n: usize,
    sigma: f64,
    sigma_pp: f64,
    low_sample: bool,
    correct: usize,
    partial: usize,
    wrong: usize,
}

// ── Core logic ─────────────────────────────────────────────────────

pub fn run(
    backend: &BackendConnection,
    threshold: f64,
    window_days: i64,
    by_layer: bool,
    json_output: bool,
) -> Result<()> {
    let mappings = scenario_contract_mappings::list_enriched_backend(backend)?;
    let prediction_accuracy = build_prediction_accuracy_report(backend, window_days)?;
    let by_layer = if by_layer {
        Some(build_prediction_layer_calibration_report(
            backend,
            window_days,
        )?)
    } else {
        None
    };

    if mappings.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&CalibrationReport {
                    total_mappings: 0,
                    significant_divergences: 0,
                    threshold_pp: threshold,
                    entries: Vec::new(),
                    summary: CalibrationSummary {
                        mean_abs_divergence_pp: 0.0,
                        median_abs_divergence_pp: 0.0,
                        overestimates: 0,
                        underestimates: 0,
                        aligned: 0,
                    },
                    prediction_accuracy,
                    by_layer,
                })?
            );
        } else {
            println!("No scenario↔contract mappings found.");
            println!();
            println!("Map prediction market contracts to scenarios first:");
            println!("  pftui data predictions map --scenario \"US Recession 2026\" --search \"recession\"");
            println!();
            println!("See: pftui data predictions map --help");
            println!();
            print_prediction_accuracy(&prediction_accuracy);
            if let Some(report) = &by_layer {
                println!();
                print_layer_calibration(report);
            }
        }
        return Ok(());
    }

    let mut entries: Vec<CalibrationEntry> = mappings
        .iter()
        .map(|m| {
            let scenario_pct = m.scenario_probability;
            let market_pct = m.contract_probability * 100.0;
            let divergence = scenario_pct - market_pct;
            let abs_div = divergence.abs();
            let significant = abs_div > threshold;

            let interpretation = if abs_div <= threshold {
                format!("Aligned — pftui and market agree within {:.0}pp", threshold)
            } else if divergence > 0.0 {
                format!(
                    "pftui OVERESTIMATES by {:.1}pp — your estimate: {:.0}%, market: {:.0}%",
                    abs_div, scenario_pct, market_pct
                )
            } else {
                format!(
                    "pftui UNDERESTIMATES by {:.1}pp — your estimate: {:.0}%, market: {:.0}%",
                    abs_div, scenario_pct, market_pct
                )
            };

            CalibrationEntry {
                scenario_id: m.scenario_id,
                scenario_name: m.scenario_name.clone(),
                scenario_probability_pct: scenario_pct,
                contract_id: m.contract_id.clone(),
                contract_question: m.contract_question.clone(),
                contract_category: m.contract_category.clone(),
                market_probability_pct: round2(market_pct),
                divergence_pp: round2(divergence),
                abs_divergence_pp: round2(abs_div),
                significant,
                interpretation,
            }
        })
        .collect();

    // Sort by absolute divergence, largest first
    entries.sort_by(|a, b| {
        b.abs_divergence_pp
            .partial_cmp(&a.abs_divergence_pp)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let significant_count = entries.iter().filter(|e| e.significant).count();

    let abs_divs: Vec<f64> = entries.iter().map(|e| e.abs_divergence_pp).collect();
    let mean_abs = if abs_divs.is_empty() {
        0.0
    } else {
        abs_divs.iter().sum::<f64>() / abs_divs.len() as f64
    };
    let median_abs = median(&abs_divs);

    let overestimates = entries
        .iter()
        .filter(|e| e.divergence_pp > threshold)
        .count();
    let underestimates = entries
        .iter()
        .filter(|e| e.divergence_pp < -threshold)
        .count();
    let aligned = entries.iter().filter(|e| !e.significant).count();

    let summary = CalibrationSummary {
        mean_abs_divergence_pp: round2(mean_abs),
        median_abs_divergence_pp: round2(median_abs),
        overestimates,
        underestimates,
        aligned,
    };

    let report = CalibrationReport {
        total_mappings: entries.len(),
        significant_divergences: significant_count,
        threshold_pp: threshold,
        entries,
        summary,
        prediction_accuracy,
        by_layer,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }

    Ok(())
}

// ── Display ────────────────────────────────────────────────────────

fn print_text(report: &CalibrationReport) {
    println!("Prediction Market Calibration");
    println!("════════════════════════════════════════════════════════════════");
    println!(
        "{} mappings  •  {} significant divergences (>{:.0}pp)",
        report.total_mappings, report.significant_divergences, report.threshold_pp
    );
    println!();

    // Significant divergences first
    let sig_entries: Vec<&CalibrationEntry> =
        report.entries.iter().filter(|e| e.significant).collect();
    if !sig_entries.is_empty() {
        println!("⚠️  SIGNIFICANT DIVERGENCES");
        println!("────────────────────────────────────────────────────────────────");
        for entry in &sig_entries {
            let arrow = if entry.divergence_pp > 0.0 {
                "▲"
            } else {
                "▼"
            };
            println!(
                "  {} {} ({}pp)",
                arrow,
                entry.scenario_name,
                format_signed(entry.divergence_pp)
            );
            println!(
                "    Your estimate: {:.0}%  •  Market: {:.0}%",
                entry.scenario_probability_pct, entry.market_probability_pct
            );
            println!("    Contract: {}", truncate(&entry.contract_question, 60));
            println!();
        }
    }

    // Aligned pairs
    let aligned_entries: Vec<&CalibrationEntry> =
        report.entries.iter().filter(|e| !e.significant).collect();
    if !aligned_entries.is_empty() {
        println!("✅  ALIGNED (within {:.0}pp)", report.threshold_pp);
        println!("────────────────────────────────────────────────────────────────");
        for entry in &aligned_entries {
            println!(
                "  ≈ {} — you: {:.0}%, market: {:.0}% ({}pp)",
                entry.scenario_name,
                entry.scenario_probability_pct,
                entry.market_probability_pct,
                format_signed(entry.divergence_pp)
            );
        }
        println!();
    }

    // Summary
    println!("────────────────────────────────────────────────────────────────");
    println!("Summary");
    println!(
        "  Mean absolute divergence:   {:.1}pp",
        report.summary.mean_abs_divergence_pp
    );
    println!(
        "  Median absolute divergence: {:.1}pp",
        report.summary.median_abs_divergence_pp
    );
    println!(
        "  Overestimates: {}  •  Underestimates: {}  •  Aligned: {}",
        report.summary.overestimates, report.summary.underestimates, report.summary.aligned
    );
    println!();
    println!("Lower divergence = better calibrated. Target: mean <10pp.");
    println!();
    if let Some(layer_report) = &report.by_layer {
        print_layer_calibration(layer_report);
        println!();
    }
    print_prediction_accuracy(&report.prediction_accuracy);
}

fn build_prediction_accuracy_report(
    backend: &BackendConnection,
    window_days: i64,
) -> Result<PredictionAccuracyReport> {
    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    Ok(build_prediction_accuracy_from_predictions(
        &predictions,
        window_days,
    ))
}

fn build_prediction_layer_calibration_report(
    backend: &BackendConnection,
    window_days: i64,
) -> Result<PredictionLayerCalibrationReport> {
    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    Ok(build_prediction_layer_calibration_from_predictions(
        &predictions,
        window_days,
    ))
}

fn build_prediction_accuracy_from_predictions(
    predictions: &[UserPrediction],
    window_days: i64,
) -> PredictionAccuracyReport {
    let cutoff = (Utc::now().date_naive() - Duration::days(window_days))
        .format("%Y-%m-%d")
        .to_string();
    let mut buckets: BTreeMap<(String, String), Vec<&UserPrediction>> = BTreeMap::new();
    let mut total_scored = 0;
    let mut scored_with_confidence = 0;

    for prediction in predictions {
        if !is_scored_outcome(&prediction.outcome) || !is_in_window(prediction, &cutoff) {
            continue;
        }
        total_scored += 1;

        let Some(confidence) = prediction.confidence else {
            continue;
        };
        if !(0.0..=1.0).contains(&confidence) {
            continue;
        }
        scored_with_confidence += 1;

        let Some(layer) = prediction_layer(prediction) else {
            continue;
        };
        let Some(band) = prediction_band(&prediction.conviction) else {
            continue;
        };
        buckets.entry((layer, band)).or_default().push(prediction);
    }

    let mut rows: Vec<PredictionAccuracyRow> = buckets
        .into_iter()
        .map(|((layer, band), bucket)| {
            let n = bucket.len();
            let predicted_conf_mean =
                bucket.iter().filter_map(|p| p.confidence).sum::<f64>() / n as f64;
            let correct = bucket.iter().filter(|p| p.outcome == "correct").count();
            let partial = bucket.iter().filter(|p| p.outcome == "partial").count();
            let wrong = bucket.iter().filter(|p| p.outcome == "wrong").count();
            let realised_hit_rate = (correct as f64 + 0.5 * partial as f64) / n as f64;
            let strict_hit_rate = strict_hit_rate(correct, n);
            let sigma = standard_error(strict_hit_rate, n);
            PredictionAccuracyRow {
                layer,
                band,
                predicted_conf_mean: round4(predicted_conf_mean),
                realised_hit_rate: round4(realised_hit_rate),
                strict_hit_rate: round4(strict_hit_rate),
                strict_hit_rate_pct: round2(strict_hit_rate * 100.0),
                n,
                sigma: round4(sigma),
                sigma_pp: round2(sigma * 100.0),
                low_sample: is_low_sample(n),
                correct,
                partial,
                wrong,
                miscalibration_pp: round2((realised_hit_rate - predicted_conf_mean) * 100.0),
            }
        })
        .collect();

    rows.sort_by(|a, b| {
        layer_order(&a.layer)
            .cmp(&layer_order(&b.layer))
            .then_with(|| band_order(&a.band).cmp(&band_order(&b.band)))
    });

    PredictionAccuracyReport {
        window_days,
        total_scored,
        scored_with_confidence,
        rows,
    }
}

fn build_prediction_layer_calibration_from_predictions(
    predictions: &[UserPrediction],
    window_days: i64,
) -> PredictionLayerCalibrationReport {
    let cutoff = (Utc::now().date_naive() - Duration::days(window_days))
        .format("%Y-%m-%d")
        .to_string();
    let mut buckets: BTreeMap<String, Vec<&UserPrediction>> = BTreeMap::new();

    for prediction in predictions {
        if !is_scored_outcome(&prediction.outcome) || !is_in_window(prediction, &cutoff) {
            continue;
        }
        let Some(layer) = prediction_layer(prediction) else {
            continue;
        };
        buckets.entry(layer).or_default().push(prediction);
    }

    let mut rows: Vec<PredictionLayerCalibrationRow> = buckets
        .into_iter()
        .map(|(layer, bucket)| build_layer_calibration_row(layer, &bucket))
        .collect();
    rows.sort_by_key(|row| layer_order(&row.layer));

    PredictionLayerCalibrationReport { window_days, rows }
}

fn build_layer_calibration_row(
    layer: String,
    bucket: &[&UserPrediction],
) -> PredictionLayerCalibrationRow {
    let n = bucket.len();
    let correct = bucket.iter().filter(|p| p.outcome == "correct").count();
    let partial = bucket.iter().filter(|p| p.outcome == "partial").count();
    let wrong = bucket.iter().filter(|p| p.outcome == "wrong").count();
    let strict_hit_rate = strict_hit_rate(correct, n);
    let sigma = standard_error(strict_hit_rate, n);

    let mut bin_buckets: BTreeMap<String, Vec<&UserPrediction>> = BTreeMap::new();
    for prediction in bucket {
        if let Some(band) = prediction_band(&prediction.conviction) {
            bin_buckets.entry(band).or_default().push(*prediction);
        }
    }
    let mut bin_breakdown: Vec<PredictionLayerCalibrationBin> = bin_buckets
        .into_iter()
        .map(|(band, bucket)| build_layer_calibration_bin(band, &bucket))
        .collect();
    bin_breakdown.sort_by_key(|bin| band_order(&bin.band));

    PredictionLayerCalibrationRow {
        layer,
        strict_hit_rate: round4(strict_hit_rate),
        strict_hit_rate_pct: round2(strict_hit_rate * 100.0),
        n,
        sigma: round4(sigma),
        sigma_pp: round2(sigma * 100.0),
        low_sample: is_low_sample(n),
        correct,
        partial,
        wrong,
        bin_breakdown,
    }
}

fn build_layer_calibration_bin(
    band: String,
    bucket: &[&UserPrediction],
) -> PredictionLayerCalibrationBin {
    let n = bucket.len();
    let correct = bucket.iter().filter(|p| p.outcome == "correct").count();
    let partial = bucket.iter().filter(|p| p.outcome == "partial").count();
    let wrong = bucket.iter().filter(|p| p.outcome == "wrong").count();
    let strict_hit_rate = strict_hit_rate(correct, n);
    let sigma = standard_error(strict_hit_rate, n);

    PredictionLayerCalibrationBin {
        band,
        strict_hit_rate: round4(strict_hit_rate),
        strict_hit_rate_pct: round2(strict_hit_rate * 100.0),
        n,
        sigma: round4(sigma),
        sigma_pp: round2(sigma * 100.0),
        low_sample: is_low_sample(n),
        correct,
        partial,
        wrong,
    }
}

fn print_prediction_accuracy(report: &PredictionAccuracyReport) {
    println!("Prediction Accuracy Calibration");
    println!("────────────────────────────────────────────────────────────────");
    println!(
        "{} scored predictions in trailing {}d  •  {} with confidence",
        report.total_scored, report.window_days, report.scored_with_confidence
    );
    if report.rows.is_empty() {
        println!("No scored predictions with confidence by layer and conviction band.");
        return;
    }

    for row in &report.rows {
        let direction = if row.miscalibration_pp < -5.0 {
            "overconfident"
        } else if row.miscalibration_pp > 5.0 {
            "underconfident"
        } else {
            "calibrated"
        };
        println!(
            "  {:<6} {:<6} strict {:.1}% ({} scored, σ ±{:.1}pp)  predicted {:.0}%  weighted {:.0}%  {} ({:+.0}pp)",
            row.layer.to_uppercase(),
            row.band.to_uppercase(),
            row.strict_hit_rate_pct,
            row.n,
            row.sigma_pp,
            row.predicted_conf_mean * 100.0,
            row.realised_hit_rate * 100.0,
            direction,
            row.miscalibration_pp
        );
    }
}

fn print_layer_calibration(report: &PredictionLayerCalibrationReport) {
    println!("Per-Layer Strict Calibration");
    println!("────────────────────────────────────────────────────────────────");
    if report.rows.is_empty() {
        println!(
            "No scored predictions by layer in trailing {}d.",
            report.window_days
        );
        return;
    }

    for row in &report.rows {
        println!("  {}", format_layer_calibration_cell(row));
        if !row.bin_breakdown.is_empty() {
            let bins = row
                .bin_breakdown
                .iter()
                .map(format_layer_calibration_bin)
                .collect::<Vec<_>>()
                .join("  ");
            println!("    bins: {}", bins);
        }
    }
}

fn format_layer_calibration_cell(row: &PredictionLayerCalibrationRow) -> String {
    let low_sample = if row.low_sample { " [low sample]" } else { "" };
    format!(
        "{}: {:.1}% strict ({} scored, σ ±{:.1}pp){}",
        row.layer.to_uppercase(),
        row.strict_hit_rate_pct,
        row.n,
        row.sigma_pp,
        low_sample
    )
}

fn format_layer_calibration_bin(bin: &PredictionLayerCalibrationBin) -> String {
    let low_sample = if bin.low_sample { " [low sample]" } else { "" };
    format!(
        "{} {:.1}% (n={}, σ ±{:.1}pp){}",
        bin.band, bin.strict_hit_rate_pct, bin.n, bin.sigma_pp, low_sample
    )
}

fn is_scored_outcome(outcome: &str) -> bool {
    matches!(outcome, "correct" | "partial" | "wrong")
}

fn is_in_window(prediction: &UserPrediction, cutoff: &str) -> bool {
    let ts = prediction
        .scored_at
        .as_deref()
        .unwrap_or(prediction.created_at.as_str());
    ts.get(..10).is_some_and(|date| date >= cutoff)
}

fn prediction_layer(prediction: &UserPrediction) -> Option<String> {
    if let Some(timeframe) = prediction.timeframe.as_deref() {
        if let Some(layer) = normalize_layer(timeframe) {
            return Some(layer.to_string());
        }
    }
    prediction
        .source_agent
        .as_deref()
        .and_then(normalize_layer)
        .map(str::to_string)
}

fn normalize_layer(value: &str) -> Option<&'static str> {
    let v = value.trim().to_ascii_lowercase();
    if v.contains("low") || v == "short" {
        Some("low")
    } else if v.contains("medium") || v == "med" {
        Some("medium")
    } else if v.contains("high") || v == "long" {
        Some("high")
    } else if v.contains("macro") {
        Some("macro")
    } else {
        None
    }
}

fn prediction_band(value: &str) -> Option<String> {
    let v = value.trim().to_ascii_lowercase();
    match v.as_str() {
        "low" | "medium" | "high" => Some(v),
        _ => None,
    }
}

fn layer_order(layer: &str) -> u8 {
    match layer {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        "macro" => 3,
        _ => 4,
    }
}

fn band_order(band: &str) -> u8 {
    match band {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        _ => 3,
    }
}

fn strict_hit_rate(correct: usize, n: usize) -> f64 {
    if n == 0 {
        0.0
    } else {
        correct as f64 / n as f64
    }
}

fn standard_error(p: f64, n: usize) -> f64 {
    if n == 0 {
        0.0
    } else {
        (p * (1.0 - p) / n as f64).sqrt()
    }
}

fn is_low_sample(n: usize) -> bool {
    n < 10
}

fn format_signed(v: f64) -> String {
    if v >= 0.0 {
        format!("+{:.1}", v)
    } else {
        format!("{:.1}", v)
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.min(s.len())])
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::backend::BackendConnection;
    use crate::db::scenario_contract_mappings;
    use crate::db::scenarios;

    fn setup_test_db() -> BackendConnection {
        let conn = db::open_in_memory();
        BackendConnection::Sqlite { conn }
    }

    fn insert_scenario(backend: &BackendConnection, name: &str, probability: f64) -> i64 {
        let conn = backend.sqlite();
        let id = scenarios::add_scenario(conn, name, probability, None, None, None, None).unwrap();
        scenarios::update_scenario(conn, id, None, None, None, Some("active")).unwrap();
        id
    }

    fn insert_contract(backend: &BackendConnection, contract_id: &str, question: &str, price: f64) {
        let conn = backend.sqlite();
        conn.execute(
            "INSERT OR REPLACE INTO prediction_market_contracts
             (contract_id, exchange, event_id, event_title, question, category,
              last_price, volume_24h, liquidity, end_date, updated_at)
             VALUES (?, 'polymarket', 'evt1', 'Test Event', ?, 'economics', ?, 1000.0, 5000.0, NULL, 1711670000)",
            rusqlite::params![contract_id, question, price],
        )
        .unwrap();
    }

    fn prediction(
        timeframe: &str,
        conviction: &str,
        confidence: Option<f64>,
        outcome: &str,
        scored_at: &str,
    ) -> UserPrediction {
        UserPrediction {
            id: 1,
            claim: "test prediction".to_string(),
            symbol: None,
            conviction: conviction.to_string(),
            timeframe: Some(timeframe.to_string()),
            topic: "other".to_string(),
            confidence,
            source_agent: None,
            source_article_id: None,
            target_date: None,
            resolution_criteria: None,
            outcome: outcome.to_string(),
            score_notes: None,
            lesson: None,
            lessons_applied: Vec::new(),
            created_at: scored_at.to_string(),
            scored_at: Some(scored_at.to_string()),
        }
    }

    #[test]
    fn calibration_empty_mappings() {
        let backend = setup_test_db();
        // Should not panic with empty data
        let result = run(&backend, 15.0, 90, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn calibration_with_mappings() {
        let backend = setup_test_db();

        // Create a scenario at 40% and a contract at 0.22 (22%)
        let sid = insert_scenario(&backend, "US Recession 2026", 40.0);
        insert_contract(
            &backend,
            "contract-abc",
            "Will there be a US recession in 2026?",
            0.22,
        );
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "contract-abc").unwrap();

        // Run calibration
        let result = run(&backend, 15.0, 90, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn calibration_json_output() {
        let backend = setup_test_db();

        let sid = insert_scenario(&backend, "Fed Rate Cut", 65.0);
        insert_contract(
            &backend,
            "contract-fed",
            "Will the Fed cut rates by June 2026?",
            0.70,
        );
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "contract-fed").unwrap();

        let result = run(&backend, 15.0, 90, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn calibration_significant_divergence() {
        let backend = setup_test_db();

        // Scenario: 38%, Market: 22% → divergence +16pp → significant
        let sid = insert_scenario(&backend, "Iran War", 38.0);
        insert_contract(
            &backend,
            "contract-iran",
            "Will US attack Iran in 2026?",
            0.22,
        );
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "contract-iran").unwrap();

        let mappings = scenario_contract_mappings::list_enriched(backend.sqlite()).unwrap();
        assert_eq!(mappings.len(), 1);

        let m = &mappings[0];
        let scenario_pct = m.scenario_probability;
        let market_pct = m.contract_probability * 100.0;
        let divergence = scenario_pct - market_pct;

        assert!((divergence - 16.0).abs() < 0.01);
        assert!(divergence.abs() > 15.0); // significant
    }

    #[test]
    fn calibration_aligned() {
        let backend = setup_test_db();

        // Scenario: 50%, Market: 48% → divergence +2pp → aligned
        let sid = insert_scenario(&backend, "BTC ATH 2026", 50.0);
        insert_contract(
            &backend,
            "contract-btc",
            "Will BTC hit new ATH in 2026?",
            0.48,
        );
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "contract-btc").unwrap();

        let mappings = scenario_contract_mappings::list_enriched(backend.sqlite()).unwrap();
        let m = &mappings[0];
        let divergence = (m.scenario_probability - m.contract_probability * 100.0).abs();
        assert!(divergence < 15.0); // aligned
    }

    #[test]
    fn calibration_multiple_mappings_sorted_by_divergence() {
        let backend = setup_test_db();

        // Small divergence: 50% vs 48% = 2pp
        let s1 = insert_scenario(&backend, "BTC ATH", 50.0);
        insert_contract(&backend, "c1", "BTC ATH?", 0.48);
        scenario_contract_mappings::add_mapping(backend.sqlite(), s1, "c1").unwrap();

        // Large divergence: 80% vs 30% = 50pp
        let s2 = insert_scenario(&backend, "Dollar Collapse", 80.0);
        insert_contract(&backend, "c2", "Dollar collapse?", 0.30);
        scenario_contract_mappings::add_mapping(backend.sqlite(), s2, "c2").unwrap();

        // Medium divergence: 40% vs 22% = 18pp
        let s3 = insert_scenario(&backend, "Recession", 40.0);
        insert_contract(&backend, "c3", "Recession?", 0.22);
        scenario_contract_mappings::add_mapping(backend.sqlite(), s3, "c3").unwrap();

        let mappings = scenario_contract_mappings::list_enriched(backend.sqlite()).unwrap();
        assert_eq!(mappings.len(), 3);

        // The run function sorts by abs divergence descending — verify logic
        let mut divs: Vec<f64> = mappings
            .iter()
            .map(|m| (m.scenario_probability - m.contract_probability * 100.0).abs())
            .collect();
        divs.sort_by(|a, b| b.partial_cmp(a).unwrap());

        assert!((divs[0] - 50.0).abs() < 0.01); // Dollar Collapse
        assert!((divs[1] - 18.0).abs() < 0.01); // Recession
        assert!((divs[2] - 2.0).abs() < 0.01); // BTC ATH
    }

    #[test]
    fn test_median() {
        assert_eq!(median(&[]), 0.0);
        assert_eq!(median(&[5.0]), 5.0);
        assert_eq!(median(&[1.0, 3.0]), 2.0);
        assert_eq!(median(&[1.0, 3.0, 5.0]), 3.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
    }

    #[test]
    fn test_round2() {
        assert_eq!(round2(7.12659), 7.13);
        assert_eq!(round2(0.0), 0.0);
        assert_eq!(round2(-1.555), -1.56);
    }

    #[test]
    fn calibration_standard_error_uses_binomial_formula() {
        let sigma = standard_error(0.445, 137);
        assert!((sigma - 0.04245).abs() < 0.0001);
        assert_eq!(round2(sigma * 100.0), 4.25);
    }

    #[test]
    fn test_format_signed() {
        assert_eq!(format_signed(16.0), "+16.0");
        assert_eq!(format_signed(-5.3), "-5.3");
        assert_eq!(format_signed(0.0), "+0.0");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello…");
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn calibration_custom_threshold() {
        let backend = setup_test_db();

        // Divergence of 10pp — significant at threshold=5, not at threshold=15
        let sid = insert_scenario(&backend, "Rate Cut", 60.0);
        insert_contract(&backend, "c-rate", "Rate cut?", 0.50);
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "c-rate").unwrap();

        let mappings = scenario_contract_mappings::list_enriched(backend.sqlite()).unwrap();
        let m = &mappings[0];
        let div = (m.scenario_probability - m.contract_probability * 100.0).abs();

        assert!(div > 5.0); // significant at threshold=5
        assert!(div < 15.0); // not significant at threshold=15
    }

    #[test]
    fn prediction_accuracy_groups_by_layer_and_conviction_band() {
        let today = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let rows = vec![
            prediction("low", "high", Some(0.8), "correct", &today),
            prediction("low", "high", Some(0.6), "wrong", &today),
            prediction("low", "medium", Some(0.5), "partial", &today),
            prediction("macro", "low", Some(0.25), "wrong", &today),
            prediction("high", "high", None, "correct", &today),
            prediction("low", "high", Some(0.9), "pending", &today),
            prediction("low", "high", Some(0.9), "correct", "2020-01-01 00:00:00"),
        ];

        let report = build_prediction_accuracy_from_predictions(&rows, 90);
        assert_eq!(report.total_scored, 5);
        assert_eq!(report.scored_with_confidence, 4);
        assert_eq!(report.rows.len(), 3);

        let low_high = report
            .rows
            .iter()
            .find(|row| row.layer == "low" && row.band == "high")
            .unwrap();
        assert_eq!(low_high.n, 2);
        assert_eq!(low_high.correct, 1);
        assert_eq!(low_high.wrong, 1);
        assert_eq!(low_high.predicted_conf_mean, 0.7);
        assert_eq!(low_high.realised_hit_rate, 0.5);
        assert_eq!(low_high.strict_hit_rate, 0.5);
        assert_eq!(low_high.strict_hit_rate_pct, 50.0);
        assert_eq!(low_high.sigma_pp, 35.36);
        assert!(low_high.low_sample);
        assert_eq!(low_high.miscalibration_pp, -20.0);
    }

    #[test]
    fn calibration_by_layer_returns_strict_rates_sample_size_and_bins() {
        let today = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let rows = vec![
            prediction("low", "high", Some(0.8), "correct", &today),
            prediction("low", "high", Some(0.7), "correct", &today),
            prediction("low", "high", Some(0.6), "wrong", &today),
            prediction("low", "medium", Some(0.5), "partial", &today),
            prediction("low", "medium", Some(0.5), "wrong", &today),
            prediction("low", "low", Some(0.4), "correct", &today),
            prediction("low", "low", Some(0.4), "wrong", &today),
            prediction("low", "low", Some(0.4), "wrong", &today),
            prediction("low", "low", Some(0.4), "partial", &today),
            prediction("low", "low", Some(0.4), "correct", &today),
            prediction("high", "high", Some(0.7), "correct", &today),
        ];

        let report = build_prediction_layer_calibration_from_predictions(&rows, 90);
        let low = report.rows.iter().find(|row| row.layer == "low").unwrap();
        assert_eq!(low.n, 10);
        assert_eq!(low.correct, 4);
        assert_eq!(low.partial, 2);
        assert_eq!(low.wrong, 4);
        assert_eq!(low.strict_hit_rate, 0.4);
        assert_eq!(low.strict_hit_rate_pct, 40.0);
        assert_eq!(low.sigma_pp, 15.49);
        assert!(!low.low_sample);
        assert_eq!(low.bin_breakdown.len(), 3);

        let high_bin = low
            .bin_breakdown
            .iter()
            .find(|bin| bin.band == "high")
            .unwrap();
        assert_eq!(high_bin.n, 3);
        assert_eq!(high_bin.correct, 2);
        assert_eq!(high_bin.strict_hit_rate_pct, 66.67);
        assert!(high_bin.low_sample);
    }

    #[test]
    fn layer_calibration_cell_flags_low_sample_with_adjacent_n_and_sigma() {
        let today = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let rows = vec![
            prediction("low", "high", Some(0.8), "correct", &today),
            prediction("low", "high", Some(0.8), "correct", &today),
            prediction("low", "high", Some(0.8), "correct", &today),
            prediction("low", "medium", Some(0.5), "correct", &today),
            prediction("low", "medium", Some(0.5), "wrong", &today),
            prediction("low", "medium", Some(0.5), "wrong", &today),
            prediction("low", "low", Some(0.4), "wrong", &today),
            prediction("low", "low", Some(0.4), "wrong", &today),
            prediction("low", "low", Some(0.4), "partial", &today),
        ];

        let report = build_prediction_layer_calibration_from_predictions(&rows, 90);
        let low = report.rows.iter().find(|row| row.layer == "low").unwrap();
        let cell = format_layer_calibration_cell(low);

        assert_eq!(cell, "LOW: 44.4% strict (9 scored, σ ±16.6pp) [low sample]");
    }
}
