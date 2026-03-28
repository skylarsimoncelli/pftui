//! `pftui analytics scenario impact-matrix` — consolidated portfolio stress matrix.
//!
//! Runs every active scenario (using defined impacts) AND all built-in stress
//! presets through the portfolio, producing a ranked matrix of outcomes sorted
//! by impact severity.  Designed for agent consumption: one JSON call returns
//! the complete risk landscape across all known scenarios.

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use std::collections::HashMap;

use crate::analytics::scenarios::{apply_preset, ScenarioPreset};
use crate::config::{Config, PortfolioMode};
use crate::db::allocations::list_allocations_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::scenarios as scenario_db;
use crate::db::transactions::list_transactions_backend;
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, compute_positions_from_allocations, Position};

// ── JSON output structs ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ImpactMatrixReport {
    pub portfolio_value: String,
    pub scenario_count: usize,
    pub preset_count: usize,
    pub entries: Vec<MatrixEntry>,
    pub worst_case: Option<MatrixSummaryEntry>,
    pub best_case: Option<MatrixSummaryEntry>,
    pub expected_pnl: String,
    pub expected_pnl_pct: String,
}

#[derive(Debug, Serialize)]
pub struct MatrixEntry {
    pub source: String, // "scenario" or "preset"
    pub name: String,
    pub probability_pct: Option<f64>,
    pub pnl: String,
    pub pnl_pct: String,
    pub severity: String, // "extreme-loss", "major-loss", "moderate-loss", "minor-loss", "neutral", "minor-gain", "moderate-gain", "major-gain", "extreme-gain"
    pub asset_impacts: Vec<MatrixAssetImpact>,
}

#[derive(Debug, Serialize)]
pub struct MatrixAssetImpact {
    pub symbol: String,
    pub base_value: String,
    pub stressed_value: String,
    pub pnl: String,
    pub pnl_pct: String,
}

#[derive(Debug, Serialize)]
pub struct MatrixSummaryEntry {
    pub name: String,
    pub pnl: String,
    pub pnl_pct: String,
}

// ── Preset definitions for matrix ──────────────────────────────────

struct PresetDef {
    name: &'static str,
    preset: ScenarioPreset,
}

const PRESETS: &[PresetDef] = &[
    PresetDef {
        name: "2008 GFC",
        preset: ScenarioPreset::Gfc2008,
    },
    PresetDef {
        name: "1973 Oil Crisis",
        preset: ScenarioPreset::OilCrisis1973,
    },
    PresetDef {
        name: "Oil $100",
        preset: ScenarioPreset::Oil100,
    },
    PresetDef {
        name: "BTC 40k",
        preset: ScenarioPreset::Btc40k,
    },
    PresetDef {
        name: "Gold $6000",
        preset: ScenarioPreset::Gold6000,
    },
];

// ── Severity classification ────────────────────────────────────────

fn classify_severity(pnl_pct: Decimal) -> &'static str {
    if pnl_pct <= dec!(-20) {
        "extreme-loss"
    } else if pnl_pct <= dec!(-10) {
        "major-loss"
    } else if pnl_pct <= dec!(-5) {
        "moderate-loss"
    } else if pnl_pct < dec!(-1) {
        "minor-loss"
    } else if pnl_pct <= dec!(1) {
        "neutral"
    } else if pnl_pct <= dec!(5) {
        "minor-gain"
    } else if pnl_pct <= dec!(10) {
        "moderate-gain"
    } else if pnl_pct <= dec!(20) {
        "major-gain"
    } else {
        "extreme-gain"
    }
}

fn severity_icon(severity: &str) -> &'static str {
    match severity {
        "extreme-loss" => "🔴🔴",
        "major-loss" => "🔴",
        "moderate-loss" => "🟠",
        "minor-loss" => "🟡",
        "neutral" => "⚪",
        "minor-gain" => "🟢",
        "moderate-gain" => "🟢",
        "major-gain" => "🟢🟢",
        "extreme-gain" => "🟢🟢",
        _ => "⚪",
    }
}

// ── Assumed move per direction+tier (mirrors impact_estimate.rs) ───

fn tier_move_pct(tier: &str) -> Decimal {
    match tier.to_ascii_lowercase().as_str() {
        "primary" => dec!(15),
        "secondary" => dec!(8),
        "tertiary" => dec!(4),
        _ => dec!(5),
    }
}

fn direction_sign(direction: &str) -> Decimal {
    match direction.to_ascii_lowercase().as_str() {
        "bullish" => Decimal::ONE,
        "bearish" => dec!(-1),
        _ => Decimal::ZERO,
    }
}

// ── Core logic ─────────────────────────────────────────────────────

pub fn run(backend: &BackendConnection, config: &Config, json_output: bool) -> Result<()> {
    // 1. Load positions
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> =
        cached.into_iter().map(|q| (q.symbol, q.price)).collect();
    if prices.is_empty() {
        anyhow::bail!("No cached prices. Run `pftui data refresh` first.");
    }

    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let base_positions = load_positions(backend, config, &prices, &fx_rates)?;

    // Inject cash at $1 for category matching
    for pos in &base_positions {
        if pos.category == AssetCategory::Cash {
            prices.insert(pos.symbol.clone(), Decimal::ONE);
        }
    }

    let portfolio_value: Decimal = base_positions.iter().filter_map(|p| p.current_value).sum();
    if portfolio_value <= Decimal::ZERO {
        if json_output {
            println!("{{\"error\":\"No portfolio value\"}}");
        } else {
            println!("No portfolio value to run impact matrix against.");
        }
        return Ok(());
    }

    let position_map: HashMap<String, &Position> = base_positions
        .iter()
        .map(|p| (p.symbol.to_uppercase(), p))
        .collect();

    let mut entries: Vec<MatrixEntry> = Vec::new();

    // 2. Active scenarios (using impact definitions from DB)
    let scenarios = scenario_db::list_scenarios_backend(backend, Some("active"))?;
    for scenario in &scenarios {
        let impacts = scenario_db::list_impacts_backend(backend, scenario.id)?;
        let branches = scenario_db::list_branches_backend(backend, scenario.id)?;

        let entry = build_scenario_entry(
            scenario,
            &impacts,
            &branches,
            &position_map,
            portfolio_value,
        );
        entries.push(entry);
    }
    let scenario_count = entries.len();

    // 3. Built-in stress presets
    for preset_def in PRESETS {
        let overrides = apply_preset(preset_def.preset, &prices);
        let mut stressed_prices = prices.clone();
        for (sym, px) in &overrides {
            stressed_prices.insert(sym.clone(), *px);
        }

        let stressed_positions = load_positions(backend, config, &stressed_prices, &fx_rates)?;
        let stressed_total: Decimal = stressed_positions
            .iter()
            .filter_map(|p| p.current_value)
            .sum();

        let pnl = stressed_total - portfolio_value;
        let pnl_pct = if portfolio_value > Decimal::ZERO {
            (pnl / portfolio_value * dec!(100)).round_dp(2)
        } else {
            Decimal::ZERO
        };

        let asset_impacts = build_preset_asset_impacts(
            &base_positions,
            &stressed_positions,
        );

        let severity = classify_severity(pnl_pct);

        entries.push(MatrixEntry {
            source: "preset".to_string(),
            name: preset_def.name.to_string(),
            probability_pct: None,
            pnl: pnl.round_dp(2).to_string(),
            pnl_pct: pnl_pct.to_string(),
            severity: severity.to_string(),
            asset_impacts,
        });
    }
    let preset_count = entries.len() - scenario_count;

    // 4. Sort by P&L ascending (worst first)
    entries.sort_by(|a, b| {
        let a_pnl: Decimal = a.pnl.parse().unwrap_or_default();
        let b_pnl: Decimal = b.pnl.parse().unwrap_or_default();
        a_pnl.cmp(&b_pnl)
    });

    // 5. Compute expected P&L (probability-weighted for scenarios only)
    let expected_pnl: Decimal = entries
        .iter()
        .filter(|e| e.source == "scenario" && e.probability_pct.is_some())
        .map(|e| {
            let pnl: Decimal = e.pnl.parse().unwrap_or_default();
            let prob = Decimal::try_from(e.probability_pct.unwrap_or(0.0)).unwrap_or_default()
                / dec!(100);
            pnl * prob
        })
        .sum();

    let expected_pnl_pct = if portfolio_value > Decimal::ZERO {
        (expected_pnl / portfolio_value * dec!(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };

    // 6. Worst/best case
    let worst_case = entries.first().map(|e| MatrixSummaryEntry {
        name: e.name.clone(),
        pnl: e.pnl.clone(),
        pnl_pct: e.pnl_pct.clone(),
    });
    let best_case = entries.last().map(|e| MatrixSummaryEntry {
        name: e.name.clone(),
        pnl: e.pnl.clone(),
        pnl_pct: e.pnl_pct.clone(),
    });

    let report = ImpactMatrixReport {
        portfolio_value: portfolio_value.round_dp(2).to_string(),
        scenario_count,
        preset_count,
        entries,
        worst_case,
        best_case,
        expected_pnl: expected_pnl.round_dp(2).to_string(),
        expected_pnl_pct: expected_pnl_pct.to_string(),
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }

    Ok(())
}

fn load_positions(
    backend: &BackendConnection,
    config: &Config,
    prices: &HashMap<String, Decimal>,
    fx_rates: &HashMap<String, Decimal>,
) -> Result<Vec<Position>> {
    match config.portfolio_mode {
        PortfolioMode::Full => {
            let txs = list_transactions_backend(backend)?;
            Ok(compute_positions(&txs, prices, fx_rates))
        }
        PortfolioMode::Percentage => {
            let allocs = list_allocations_backend(backend)?;
            Ok(compute_positions_from_allocations(&allocs, prices, fx_rates))
        }
    }
}

fn build_scenario_entry(
    scenario: &scenario_db::Scenario,
    impacts: &[scenario_db::ScenarioImpact],
    branches: &[scenario_db::ScenarioBranch],
    position_map: &HashMap<String, &Position>,
    portfolio_value: Decimal,
) -> MatrixEntry {
    // Compute P&L from impacts on held positions.
    // If branches exist, use probability-weighted branch impacts;
    // otherwise use scenario-level impacts directly.
    let mut total_pnl = Decimal::ZERO;
    let mut asset_pnl_map: HashMap<String, (Decimal, Decimal)> = HashMap::new(); // symbol -> (base_value, pnl)

    if branches.is_empty() {
        for impact in impacts {
            let symbol_upper = impact.symbol.to_uppercase();
            if let Some(pos) = position_map.get(&symbol_upper) {
                if let Some(value) = pos.current_value {
                    let sign = direction_sign(&impact.direction);
                    let move_pct = tier_move_pct(&impact.tier);
                    let pnl = value * sign * move_pct / dec!(100);
                    total_pnl += pnl;

                    let entry = asset_pnl_map.entry(symbol_upper).or_insert((value, Decimal::ZERO));
                    entry.1 += pnl;
                }
            }
        }
    } else {
        for branch in branches {
            let branch_prob =
                Decimal::try_from(branch.probability).unwrap_or_default() / dec!(100);

            let branch_impacts: Vec<&scenario_db::ScenarioImpact> = impacts
                .iter()
                .filter(|i| i.branch_id == Some(branch.id) || i.branch_id.is_none())
                .collect();

            for impact in &branch_impacts {
                let symbol_upper = impact.symbol.to_uppercase();
                if let Some(pos) = position_map.get(&symbol_upper) {
                    if let Some(value) = pos.current_value {
                        let sign = direction_sign(&impact.direction);
                        let move_pct = tier_move_pct(&impact.tier);
                        let pnl = value * sign * move_pct / dec!(100) * branch_prob;
                        total_pnl += pnl;

                        let entry =
                            asset_pnl_map.entry(symbol_upper).or_insert((value, Decimal::ZERO));
                        entry.1 += pnl;
                    }
                }
            }
        }
    }

    let pnl_pct = if portfolio_value > Decimal::ZERO {
        (total_pnl / portfolio_value * dec!(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };

    let severity = classify_severity(pnl_pct);

    let mut asset_impacts: Vec<MatrixAssetImpact> = asset_pnl_map
        .into_iter()
        .map(|(symbol, (base_value, pnl))| {
            let stressed_value = base_value + pnl;
            let asset_pnl_pct = if base_value > Decimal::ZERO {
                (pnl / base_value * dec!(100)).round_dp(2)
            } else {
                Decimal::ZERO
            };
            MatrixAssetImpact {
                symbol,
                base_value: base_value.round_dp(2).to_string(),
                stressed_value: stressed_value.round_dp(2).to_string(),
                pnl: pnl.round_dp(2).to_string(),
                pnl_pct: asset_pnl_pct.to_string(),
            }
        })
        .collect();
    asset_impacts.sort_by(|a, b| {
        let a_pnl: Decimal = a.pnl.parse().unwrap_or_default();
        let b_pnl: Decimal = b.pnl.parse().unwrap_or_default();
        a_pnl.cmp(&b_pnl)
    });

    MatrixEntry {
        source: "scenario".to_string(),
        name: scenario.name.clone(),
        probability_pct: Some(scenario.probability),
        pnl: total_pnl.round_dp(2).to_string(),
        pnl_pct: pnl_pct.to_string(),
        severity: severity.to_string(),
        asset_impacts,
    }
}

fn build_preset_asset_impacts(
    base_positions: &[Position],
    stressed_positions: &[Position],
) -> Vec<MatrixAssetImpact> {
    let stressed_map: HashMap<String, &Position> = stressed_positions
        .iter()
        .map(|p| (p.symbol.to_uppercase(), p))
        .collect();

    let mut impacts: Vec<MatrixAssetImpact> = base_positions
        .iter()
        .filter_map(|base_pos| {
            let base_value = base_pos.current_value?;
            if base_value <= Decimal::ZERO {
                return None;
            }
            let symbol_upper = base_pos.symbol.to_uppercase();
            let stressed_value = stressed_map
                .get(&symbol_upper)
                .and_then(|p| p.current_value)
                .unwrap_or(base_value);
            let pnl = stressed_value - base_value;
            if pnl == Decimal::ZERO {
                return None;
            }
            let pnl_pct = (pnl / base_value * dec!(100)).round_dp(2);
            Some(MatrixAssetImpact {
                symbol: symbol_upper,
                base_value: base_value.round_dp(2).to_string(),
                stressed_value: stressed_value.round_dp(2).to_string(),
                pnl: pnl.round_dp(2).to_string(),
                pnl_pct: pnl_pct.to_string(),
            })
        })
        .collect();

    impacts.sort_by(|a, b| {
        let a_pnl: Decimal = a.pnl.parse().unwrap_or_default();
        let b_pnl: Decimal = b.pnl.parse().unwrap_or_default();
        a_pnl.cmp(&b_pnl)
    });
    impacts
}

fn print_text(report: &ImpactMatrixReport) {
    println!("Portfolio Impact Matrix");
    println!("════════════════════════════════════════════════════════════════");
    println!(
        "Portfolio value: ${}  |  {} scenarios + {} presets",
        report.portfolio_value, report.scenario_count, report.preset_count
    );
    println!();

    for entry in &report.entries {
        let prob_str = match entry.probability_pct {
            Some(p) => format!(" ({:.0}%)", p),
            None => String::new(),
        };
        let icon = severity_icon(&entry.severity);
        let source_tag = if entry.source == "preset" {
            " [preset]"
        } else {
            ""
        };

        println!(
            "{} {:40}{}{} → ${} ({}%)",
            icon, entry.name, prob_str, source_tag, entry.pnl, entry.pnl_pct
        );

        for ai in &entry.asset_impacts {
            println!(
                "   {:<8} ${} → ${} ({}%)",
                ai.symbol, ai.base_value, ai.stressed_value, ai.pnl_pct
            );
        }
        println!();
    }

    println!("────────────────────────────────────────────────────────────────");
    if let Some(ref worst) = report.worst_case {
        println!("Worst case: {} → ${} ({}%)", worst.name, worst.pnl, worst.pnl_pct);
    }
    if let Some(ref best) = report.best_case {
        println!("Best case:  {} → ${} ({}%)", best.name, best.pnl, best.pnl_pct);
    }
    println!(
        "Expected P&L (scenario-weighted): ${} ({}%)",
        report.expected_pnl, report.expected_pnl_pct
    );
    println!();
    println!(
        "Note: Scenario impacts use {}/{}%/{}% for primary/secondary/tertiary tiers.",
        15, 8, 4
    );
    println!("Preset impacts use fixed historical-analog shocks.");
    println!("These are analytical defaults, not predictions.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::backend::BackendConnection;
    use crate::db::scenarios as scenario_db;
    use crate::models::asset::AssetCategory;
    use crate::models::price::PriceQuote;
    use crate::models::transaction::{NewTransaction, TxType};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn setup_test_db() -> BackendConnection {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        // BTC position: 10 @ $50k = $500k value, current price $60k = $600k
        db::transactions::insert_transaction_backend(
            &backend,
            &NewTransaction {
                symbol: "BTC".to_string(),
                category: AssetCategory::Crypto,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(50000),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        db::price_cache::upsert_price_backend(
            &backend,
            &PriceQuote {
                symbol: "BTC".to_string(),
                price: dec!(60000),
                currency: "USD".to_string(),
                fetched_at: Utc::now().to_rfc3339(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        // GLD position: 100 oz @ $2000, current $2200 = $220k
        db::transactions::insert_transaction_backend(
            &backend,
            &NewTransaction {
                symbol: "GLD".to_string(),
                category: AssetCategory::Commodity,
                tx_type: TxType::Buy,
                quantity: dec!(100),
                price_per: dec!(2000),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        db::price_cache::upsert_price_backend(
            &backend,
            &PriceQuote {
                symbol: "GLD".to_string(),
                price: dec!(2200),
                currency: "USD".to_string(),
                fetched_at: Utc::now().to_rfc3339(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        backend
    }

    fn add_test_scenario(backend: &BackendConnection) -> i64 {
        let id = scenario_db::add_scenario(
            backend.sqlite(),
            "Dollar collapse",
            40.0,
            Some("USD loses reserve status"),
            Some("BTC bullish, gold bullish"),
            Some("BRICS currency"),
            None,
        )
        .unwrap();
        scenario_db::update_scenario(backend.sqlite(), id, None, None, None, Some("active"))
            .unwrap();
        scenario_db::add_impact(
            backend.sqlite(),
            id,
            None,
            "BTC",
            "bullish",
            "primary",
            Some("safe haven demand"),
            None,
        )
        .unwrap();
        scenario_db::add_impact(
            backend.sqlite(),
            id,
            None,
            "GLD",
            "bullish",
            "secondary",
            Some("monetary metal demand"),
            None,
        )
        .unwrap();
        id
    }

    #[test]
    fn classify_severity_thresholds() {
        assert_eq!(classify_severity(dec!(-25)), "extreme-loss");
        assert_eq!(classify_severity(dec!(-15)), "major-loss");
        assert_eq!(classify_severity(dec!(-7)), "moderate-loss");
        assert_eq!(classify_severity(dec!(-3)), "minor-loss");
        assert_eq!(classify_severity(dec!(0)), "neutral");
        assert_eq!(classify_severity(dec!(3)), "minor-gain");
        assert_eq!(classify_severity(dec!(7)), "moderate-gain");
        assert_eq!(classify_severity(dec!(15)), "major-gain");
        assert_eq!(classify_severity(dec!(25)), "extreme-gain");
    }

    #[test]
    fn build_scenario_entry_no_branches() {
        let backend = setup_test_db();
        let scenario_id = add_test_scenario(&backend);

        let txs = list_transactions_backend(&backend).unwrap();
        let cached = get_all_cached_prices_backend(&backend).unwrap();
        let prices: HashMap<String, Decimal> =
            cached.into_iter().map(|q| (q.symbol, q.price)).collect();
        let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
        let positions = compute_positions(&txs, &prices, &fx_rates);
        let portfolio_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
        let position_map: HashMap<String, &Position> = positions
            .iter()
            .map(|p| (p.symbol.to_uppercase(), p))
            .collect();

        let scenario = &scenario_db::list_scenarios_backend(&backend, Some("active")).unwrap()[0];
        let impacts = scenario_db::list_impacts_backend(&backend, scenario_id).unwrap();
        let branches = scenario_db::list_branches_backend(&backend, scenario_id).unwrap();

        let entry = build_scenario_entry(scenario, &impacts, &branches, &position_map, portfolio_value);

        assert_eq!(entry.source, "scenario");
        assert_eq!(entry.name, "Dollar collapse");
        assert_eq!(entry.probability_pct, Some(40.0));
        // BTC: 600000 * 15% = 90000; GLD: 220000 * 8% = 17600 → total = 107600
        let pnl: Decimal = entry.pnl.parse().unwrap();
        assert_eq!(pnl, dec!(107600.00));
        assert_eq!(entry.asset_impacts.len(), 2);
        assert_eq!(entry.severity, "major-gain"); // 107600/820000 = 13.12%
    }

    #[test]
    fn build_preset_asset_impacts_detects_changes() {
        let backend = setup_test_db();
        let txs = list_transactions_backend(&backend).unwrap();
        let cached = get_all_cached_prices_backend(&backend).unwrap();
        let prices: HashMap<String, Decimal> =
            cached.into_iter().map(|q| (q.symbol, q.price)).collect();
        let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
        let base = compute_positions(&txs, &prices, &fx_rates);

        // Simulate gold going to $6000
        let overrides = apply_preset(ScenarioPreset::Gold6000, &prices);
        let mut stressed_prices = prices.clone();
        for (sym, px) in &overrides {
            stressed_prices.insert(sym.clone(), *px);
        }
        let stressed = compute_positions(&txs, &stressed_prices, &fx_rates);

        let impacts = build_preset_asset_impacts(&base, &stressed);
        assert!(!impacts.is_empty());

        // GLD should move from 2200 to ~2684 (22% commodity shock applied to GLD)
        let gld_impact = impacts.iter().find(|i| i.symbol == "GLD");
        assert!(gld_impact.is_some());
    }

    #[test]
    fn severity_icon_returns_correct_emoji() {
        assert_eq!(severity_icon("extreme-loss"), "🔴🔴");
        assert_eq!(severity_icon("major-loss"), "🔴");
        assert_eq!(severity_icon("neutral"), "⚪");
        assert_eq!(severity_icon("major-gain"), "🟢🟢");
    }

    #[test]
    fn direction_sign_and_tier_move() {
        assert_eq!(direction_sign("bullish"), Decimal::ONE);
        assert_eq!(direction_sign("bearish"), dec!(-1));
        assert_eq!(direction_sign("neutral"), Decimal::ZERO);
        assert_eq!(tier_move_pct("primary"), dec!(15));
        assert_eq!(tier_move_pct("secondary"), dec!(8));
        assert_eq!(tier_move_pct("tertiary"), dec!(4));
        assert_eq!(tier_move_pct("unknown"), dec!(5));
    }

    #[test]
    fn entries_sorted_worst_first() {
        let mut entries = Vec::from([
            MatrixEntry {
                source: "preset".into(),
                name: "Good".into(),
                probability_pct: None,
                pnl: "10000.00".into(),
                pnl_pct: "5.00".into(),
                severity: "minor-gain".into(),
                asset_impacts: vec![],
            },
            MatrixEntry {
                source: "preset".into(),
                name: "Bad".into(),
                probability_pct: None,
                pnl: "-20000.00".into(),
                pnl_pct: "-10.00".into(),
                severity: "major-loss".into(),
                asset_impacts: vec![],
            },
            MatrixEntry {
                source: "scenario".into(),
                name: "Neutral".into(),
                probability_pct: Some(30.0),
                pnl: "0.00".into(),
                pnl_pct: "0.00".into(),
                severity: "neutral".into(),
                asset_impacts: vec![],
            },
        ]);

        entries.sort_by(|a, b| {
            let a_pnl: Decimal = a.pnl.parse().unwrap_or_default();
            let b_pnl: Decimal = b.pnl.parse().unwrap_or_default();
            a_pnl.cmp(&b_pnl)
        });

        assert_eq!(entries[0].name, "Bad");
        assert_eq!(entries[1].name, "Neutral");
        assert_eq!(entries[2].name, "Good");
    }

    #[test]
    fn expected_pnl_weights_scenarios_only() {
        // Scenario with 50% probability and $10000 P&L = $5000 expected
        let entries: Vec<MatrixEntry> = Vec::from([
            MatrixEntry {
                source: "scenario".into(),
                name: "Bull".into(),
                probability_pct: Some(50.0),
                pnl: "10000.00".into(),
                pnl_pct: "5.00".into(),
                severity: "minor-gain".into(),
                asset_impacts: vec![],
            },
            MatrixEntry {
                source: "preset".into(),
                name: "GFC".into(),
                probability_pct: None,
                pnl: "-50000.00".into(),
                pnl_pct: "-25.00".into(),
                severity: "extreme-loss".into(),
                asset_impacts: vec![],
            },
        ]);

        let expected_pnl: Decimal = entries
            .iter()
            .filter(|e| e.source == "scenario" && e.probability_pct.is_some())
            .map(|e| {
                let pnl: Decimal = e.pnl.parse().unwrap_or_default();
                let prob =
                    Decimal::try_from(e.probability_pct.unwrap_or(0.0)).unwrap_or_default()
                        / dec!(100);
                pnl * prob
            })
            .sum();

        assert_eq!(expected_pnl, dec!(5000));
    }
}
