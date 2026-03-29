//! `pftui analytics calibration` — compare scenario probabilities vs prediction market consensus.
//!
//! For each mapped scenario↔contract pair, shows the divergence between
//! pftui's scenario probability (set by agents/user) and the prediction
//! market's crowd-calibrated probability (from Polymarket contracts).
//!
//! Flags divergences >15pp as significant. Designed for agent consumption:
//! agents explain divergences between their estimates and market consensus.

use anyhow::Result;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::scenario_contract_mappings;

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

// ── Core logic ─────────────────────────────────────────────────────

pub fn run(backend: &BackendConnection, threshold: f64, json_output: bool) -> Result<()> {
    let mappings = scenario_contract_mappings::list_enriched_backend(backend)?;

    if mappings.is_empty() {
        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "total_mappings": 0,
                    "significant_divergences": 0,
                    "entries": [],
                    "note": "No scenario↔contract mappings found. Use `data predictions map` to link contracts to scenarios."
                }))?
            );
        } else {
            println!("No scenario↔contract mappings found.");
            println!();
            println!("Map prediction market contracts to scenarios first:");
            println!("  pftui data predictions map --scenario \"US Recession 2026\" --search \"recession\"");
            println!();
            println!("See: pftui data predictions map --help");
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
                format!(
                    "Aligned — pftui and market agree within {:.0}pp",
                    threshold
                )
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

    let overestimates = entries.iter().filter(|e| e.divergence_pp > threshold).count();
    let underestimates = entries.iter().filter(|e| e.divergence_pp < -threshold).count();
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
    let sig_entries: Vec<&CalibrationEntry> = report.entries.iter().filter(|e| e.significant).collect();
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
                arrow, entry.scenario_name, format_signed(entry.divergence_pp)
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

    #[test]
    fn calibration_empty_mappings() {
        let backend = setup_test_db();
        // Should not panic with empty data
        let result = run(&backend, 15.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn calibration_with_mappings() {
        let backend = setup_test_db();

        // Create a scenario at 40% and a contract at 0.22 (22%)
        let sid = insert_scenario(&backend, "US Recession 2026", 40.0);
        insert_contract(&backend, "contract-abc", "Will there be a US recession in 2026?", 0.22);
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "contract-abc").unwrap();

        // Run calibration
        let result = run(&backend, 15.0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn calibration_json_output() {
        let backend = setup_test_db();

        let sid = insert_scenario(&backend, "Fed Rate Cut", 65.0);
        insert_contract(&backend, "contract-fed", "Will the Fed cut rates by June 2026?", 0.70);
        scenario_contract_mappings::add_mapping(backend.sqlite(), sid, "contract-fed").unwrap();

        let result = run(&backend, 15.0, true);
        assert!(result.is_ok());
    }

    #[test]
    fn calibration_significant_divergence() {
        let backend = setup_test_db();

        // Scenario: 38%, Market: 22% → divergence +16pp → significant
        let sid = insert_scenario(&backend, "Iran War", 38.0);
        insert_contract(&backend, "contract-iran", "Will US attack Iran in 2026?", 0.22);
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
        insert_contract(&backend, "contract-btc", "Will BTC hit new ATH in 2026?", 0.48);
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

        assert!(div > 5.0);  // significant at threshold=5
        assert!(div < 15.0); // not significant at threshold=15
    }
}
