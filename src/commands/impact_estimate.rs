//! `pftui analytics impact-estimate` — projected P&L under each active scenario.
//!
//! For every active scenario (and its branches), estimates how the current
//! portfolio would be affected based on scenario impacts (direction + tier).
//! Weights estimates by scenario/branch probability to produce an expected
//! portfolio-level P&L projection.

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;
use std::collections::HashMap;

use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::scenarios::{self, Scenario, ScenarioBranch, ScenarioImpact};
use crate::models::asset::AssetCategory;
use crate::models::position::{compute_positions, Position};

/// Assumed move size per direction+tier combination.
/// These are conservative scenario-analysis defaults, not predictions.
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

// ── JSON output structs ────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct EstimateReport {
    portfolio_value: String,
    scenarios: Vec<ScenarioEstimate>,
    expected_pnl: String,
    expected_pnl_pct: String,
}

#[derive(Debug, Serialize)]
struct ScenarioEstimate {
    scenario_id: i64,
    scenario_name: String,
    probability_pct: f64,
    branches: Vec<BranchEstimate>,
    /// Probability-weighted P&L across all branches of this scenario.
    weighted_pnl: String,
    weighted_pnl_pct: String,
}

#[derive(Debug, Serialize)]
struct BranchEstimate {
    branch_id: Option<i64>,
    branch_name: String,
    probability_pct: f64,
    asset_impacts: Vec<AssetImpactEstimate>,
    total_pnl: String,
    total_pnl_pct: String,
}

#[derive(Debug, Serialize)]
struct AssetImpactEstimate {
    symbol: String,
    direction: String,
    tier: String,
    move_pct: String,
    current_value: String,
    estimated_pnl: String,
}

// ── Core logic ─────────────────────────────────────────────────────

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    // 1. Load positions
    let txs = db::transactions::list_transactions_backend(backend)?;
    let cached = get_all_cached_prices_backend(backend)?;
    let mut prices: HashMap<String, Decimal> = cached.into_iter().map(|q| (q.symbol, q.price)).collect();
    for tx in &txs {
        if tx.category == AssetCategory::Cash {
            prices.insert(tx.symbol.clone(), Decimal::ONE);
        }
    }
    let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();
    let positions = compute_positions(&txs, &prices, &fx_rates);

    let portfolio_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
    if portfolio_value <= Decimal::ZERO {
        if json_output {
            println!("{{\"error\":\"No portfolio value\"}}");
        } else {
            println!("No portfolio value to estimate impact against.");
        }
        return Ok(());
    }

    let position_map: HashMap<String, &Position> =
        positions.iter().map(|p| (p.symbol.to_uppercase(), p)).collect();

    // 2. Load active scenarios
    let scenarios = scenarios::list_scenarios_backend(backend, Some("active"))?;
    if scenarios.is_empty() {
        if json_output {
            println!("{{\"error\":\"No active scenarios\"}}");
        } else {
            println!("No active scenarios to estimate impact from.");
        }
        return Ok(());
    }

    // 3. Build estimates per scenario
    let mut scenario_estimates = Vec::new();
    let mut total_expected_pnl = Decimal::ZERO;

    for scenario in &scenarios {
        let branches = scenarios::list_branches_backend(backend, scenario.id)?;
        let impacts = scenarios::list_impacts_backend(backend, scenario.id)?;

        let estimate = build_scenario_estimate(
            scenario,
            &branches,
            &impacts,
            &position_map,
            portfolio_value,
        );

        total_expected_pnl += decimal_from_str(&estimate.weighted_pnl);
        scenario_estimates.push(estimate);
    }

    let expected_pnl_pct = if portfolio_value > Decimal::ZERO {
        (total_expected_pnl / portfolio_value * dec!(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };

    let report = EstimateReport {
        portfolio_value: portfolio_value.round_dp(2).to_string(),
        scenarios: scenario_estimates,
        expected_pnl: total_expected_pnl.round_dp(2).to_string(),
        expected_pnl_pct: expected_pnl_pct.to_string(),
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }

    Ok(())
}

fn build_scenario_estimate(
    scenario: &Scenario,
    branches: &[ScenarioBranch],
    impacts: &[ScenarioImpact],
    position_map: &HashMap<String, &Position>,
    portfolio_value: Decimal,
) -> ScenarioEstimate {
    let scenario_prob = Decimal::try_from(scenario.probability).unwrap_or_default() / dec!(100);
    let mut branch_estimates = Vec::new();
    let mut weighted_pnl = Decimal::ZERO;

    if branches.is_empty() {
        // No branches — use scenario-level impacts directly
        let branch_est =
            estimate_branch(None, "Base", 100.0, impacts, None, position_map, portfolio_value);
        weighted_pnl += decimal_from_str(&branch_est.total_pnl) * scenario_prob;
        branch_estimates.push(branch_est);
    } else {
        for branch in branches {
            let branch_prob =
                Decimal::try_from(branch.probability).unwrap_or_default() / dec!(100);
            let effective_prob = scenario_prob * branch_prob;

            let branch_est = estimate_branch(
                Some(branch.id),
                &branch.name,
                branch.probability,
                impacts,
                Some(branch.id),
                position_map,
                portfolio_value,
            );

            weighted_pnl += decimal_from_str(&branch_est.total_pnl) * effective_prob;
            branch_estimates.push(branch_est);
        }
    }

    let weighted_pnl_pct = if portfolio_value > Decimal::ZERO {
        (weighted_pnl / portfolio_value * dec!(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };

    ScenarioEstimate {
        scenario_id: scenario.id,
        scenario_name: scenario.name.clone(),
        probability_pct: scenario.probability,
        branches: branch_estimates,
        weighted_pnl: weighted_pnl.round_dp(2).to_string(),
        weighted_pnl_pct: weighted_pnl_pct.to_string(),
    }
}

fn estimate_branch(
    branch_id: Option<i64>,
    branch_name: &str,
    probability: f64,
    all_impacts: &[ScenarioImpact],
    filter_branch_id: Option<i64>,
    position_map: &HashMap<String, &Position>,
    portfolio_value: Decimal,
) -> BranchEstimate {
    let impacts: Vec<&ScenarioImpact> = all_impacts
        .iter()
        .filter(|i| match filter_branch_id {
            Some(bid) => i.branch_id == Some(bid) || i.branch_id.is_none(),
            None => true,
        })
        .collect();

    let mut asset_impacts = Vec::new();
    let mut total_pnl = Decimal::ZERO;

    for impact in &impacts {
        let symbol_upper = impact.symbol.to_uppercase();
        if let Some(pos) = position_map.get(&symbol_upper) {
            if let Some(value) = pos.current_value {
                let sign = direction_sign(&impact.direction);
                let move_pct = tier_move_pct(&impact.tier);
                let estimated_pnl = value * sign * move_pct / dec!(100);

                total_pnl += estimated_pnl;

                asset_impacts.push(AssetImpactEstimate {
                    symbol: symbol_upper,
                    direction: impact.direction.clone(),
                    tier: impact.tier.clone(),
                    move_pct: format!("{}{}%", if sign >= Decimal::ZERO { "+" } else { "" }, move_pct * sign),
                    current_value: value.round_dp(2).to_string(),
                    estimated_pnl: estimated_pnl.round_dp(2).to_string(),
                });
            }
        }
    }

    let total_pnl_pct = if portfolio_value > Decimal::ZERO {
        (total_pnl / portfolio_value * dec!(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };

    BranchEstimate {
        branch_id,
        branch_name: branch_name.to_string(),
        probability_pct: probability,
        asset_impacts,
        total_pnl: total_pnl.round_dp(2).to_string(),
        total_pnl_pct: total_pnl_pct.to_string(),
    }
}

fn decimal_from_str(s: &str) -> Decimal {
    s.parse::<Decimal>().unwrap_or_default()
}

fn print_text(report: &EstimateReport) {
    println!("Portfolio Impact Estimate");
    println!("════════════════════════════════════════════════════════════════");
    println!("Portfolio value: ${}", report.portfolio_value);
    println!();

    for scenario in &report.scenarios {
        println!(
            "▸ {} ({:.0}% probability)  →  weighted P&L: ${} ({}%)",
            scenario.scenario_name,
            scenario.probability_pct,
            scenario.weighted_pnl,
            scenario.weighted_pnl_pct,
        );

        for branch in &scenario.branches {
            let branch_label = if scenario.branches.len() == 1 && branch.branch_name == "Base" {
                String::new()
            } else {
                format!(" ── {} ({:.0}%)", branch.branch_name, branch.probability_pct)
            };
            println!(
                "  {}  P&L: ${} ({}%)",
                if branch_label.is_empty() { "  Direct impacts:".to_string() } else { branch_label },
                branch.total_pnl,
                branch.total_pnl_pct,
            );

            for ai in &branch.asset_impacts {
                println!(
                    "    {:<8} {:>7} ({:<9} {})  →  ${}",
                    ai.symbol, ai.move_pct, ai.tier, ai.direction, ai.estimated_pnl,
                );
            }
        }
        println!();
    }

    println!("────────────────────────────────────────────────────────────────");
    println!(
        "Expected P&L (probability-weighted): ${} ({}%)",
        report.expected_pnl, report.expected_pnl_pct,
    );
    println!();
    println!("Note: Move estimates are {}/{}%/{}% for primary/secondary/tertiary tiers.", 15, 8, 4);
    println!("These are analytical defaults, not predictions. Actual moves may differ significantly.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::backend::BackendConnection;
    use crate::db::scenarios;
    use crate::models::asset::AssetCategory;
    use crate::models::price::PriceQuote;
    use crate::models::transaction::{NewTransaction, TxType};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn setup_test_db() -> BackendConnection {
        let conn = db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };

        // Add a position: 10 BTC at $50k each = $500k
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

        // Cache current price at $60k
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

        // Add gold position: 100 oz at $2000 = $200k
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

    fn add_scenario_with_impacts(backend: &BackendConnection) -> i64 {
        let scenario_id = scenarios::add_scenario(
            backend.sqlite(),
            "Dollar collapse",
            40.0,
            Some("USD loses reserve status"),
            Some("BTC bullish, gold bullish"),
            Some("BRICS currency, dedollarization"),
            None,
        )
        .unwrap();

        // Promote to active
        scenarios::update_scenario(backend.sqlite(), scenario_id, None, None, None, Some("active"))
            .unwrap();

        // Add impacts: BTC primary bullish, GLD secondary bullish
        scenarios::add_impact(
            backend.sqlite(),
            scenario_id,
            None,
            "BTC",
            "bullish",
            "primary",
            Some("safe haven demand"),
            None,
        )
        .unwrap();

        scenarios::add_impact(
            backend.sqlite(),
            scenario_id,
            None,
            "GLD",
            "bullish",
            "secondary",
            Some("monetary metal demand"),
            None,
        )
        .unwrap();

        scenario_id
    }

    #[test]
    fn test_impact_estimate_basic() {
        let backend = setup_test_db();
        add_scenario_with_impacts(&backend);

        // Run the estimate
        let txs = db::transactions::list_transactions_backend(&backend).unwrap();
        let cached = get_all_cached_prices_backend(&backend).unwrap();
        let prices: HashMap<String, Decimal> =
            cached.into_iter().map(|q| (q.symbol, q.price)).collect();
        let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
        let positions = compute_positions(&txs, &prices, &fx_rates);
        let portfolio_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();

        assert!(portfolio_value > Decimal::ZERO);
        // BTC: 10 * 60000 = 600000, GLD: 100 * 2200 = 220000
        // Total = 820000
        assert_eq!(portfolio_value, dec!(820000));
    }

    #[test]
    fn test_tier_move_pct() {
        assert_eq!(tier_move_pct("primary"), dec!(15));
        assert_eq!(tier_move_pct("secondary"), dec!(8));
        assert_eq!(tier_move_pct("tertiary"), dec!(4));
        assert_eq!(tier_move_pct("unknown"), dec!(5));
    }

    #[test]
    fn test_direction_sign() {
        assert_eq!(direction_sign("bullish"), Decimal::ONE);
        assert_eq!(direction_sign("bearish"), dec!(-1));
        assert_eq!(direction_sign("neutral"), Decimal::ZERO);
    }

    #[test]
    fn test_estimate_branch_pnl() {
        let backend = setup_test_db();
        let scenario_id = add_scenario_with_impacts(&backend);

        let txs = db::transactions::list_transactions_backend(&backend).unwrap();
        let cached = get_all_cached_prices_backend(&backend).unwrap();
        let prices: HashMap<String, Decimal> =
            cached.into_iter().map(|q| (q.symbol, q.price)).collect();
        let fx_rates = crate::db::fx_cache::get_all_fx_rates_backend(&backend).unwrap_or_default();
        let positions = compute_positions(&txs, &prices, &fx_rates);
        let portfolio_value: Decimal = positions.iter().filter_map(|p| p.current_value).sum();
        let position_map: HashMap<String, &Position> =
            positions.iter().map(|p| (p.symbol.to_uppercase(), p)).collect();

        let impacts = scenarios::list_impacts_backend(&backend, scenario_id).unwrap();

        let branch_est = estimate_branch(
            None,
            "Base",
            100.0,
            &impacts,
            None,
            &position_map,
            portfolio_value,
        );

        // BTC: 600000 * 15% = 90000
        // GLD: 220000 * 8% = 17600
        // Total: 107600
        let total_pnl: Decimal = branch_est.total_pnl.parse().unwrap();
        assert_eq!(total_pnl, dec!(107600.00));
        assert_eq!(branch_est.asset_impacts.len(), 2);
    }
}
