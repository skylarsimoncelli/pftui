#![allow(dead_code)]

use anyhow::Result;
use std::collections::BTreeMap;

use crate::report::build::daily::{
    BuildContext, PrivateMacroScenarioRow, PrivatePositionSnapshotRow, PrivateRiskFactorMapping,
};

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;
const HIGH_PROBABILITY_THRESHOLD_PCT: f64 = 60.0;
const HIGH_EXPOSURE_THRESHOLD_PCT: f64 = 50.0;

#[derive(Debug, Clone, PartialEq)]
struct FactorAggregate {
    name: String,
    exposure_pct: f64,
    direction: String,
    prob_pct: Option<f64>,
    contributors: Vec<FactorContributor>,
}

#[derive(Debug, Clone, PartialEq)]
struct FactorContributor {
    symbol: String,
    exposure_pct: f64,
}

pub fn render_private_risk_concentration(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Risk Concentration\n\n");
    let held = qualifying_positions(&ctx.private_positions);
    if held.is_empty() {
        output.push_str("No held assets above 1% are attached to this private build.");
        return Ok(output);
    }

    let factors = factor_aggregates(
        &held,
        &ctx.private_risk_factor_mappings,
        &ctx.private_macro_scenarios,
    );
    if factors.is_empty() {
        output.push_str(
            "No factor mapping rows are attached for qualifying held assets. Risk concentration is limited to allocation concentration until scenario-to-asset mappings are loaded.",
        );
        return Ok(output);
    }

    output.push_str(&native_placeholder(&factors));
    output.push_str("\n\n");
    output.push_str(&render_factor_table(&factors));
    output.push_str("\n\n");
    output.push_str(&risk_paragraph(&factors));

    Ok(output.trim_end().to_string())
}

fn qualifying_positions(rows: &[PrivatePositionSnapshotRow]) -> Vec<&PrivatePositionSnapshotRow> {
    let mut held = rows
        .iter()
        .filter(|row| row.allocation_pct >= HELD_ASSET_THRESHOLD_PCT)
        .collect::<Vec<_>>();
    held.sort_by(|a, b| {
        b.allocation_pct
            .partial_cmp(&a.allocation_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    held
}

fn factor_aggregates(
    positions: &[&PrivatePositionSnapshotRow],
    mappings: &[PrivateRiskFactorMapping],
    scenarios: &[PrivateMacroScenarioRow],
) -> Vec<FactorAggregate> {
    let mut grouped: BTreeMap<String, Vec<(String, f64, String)>> = BTreeMap::new();
    for position in positions {
        for mapping in mappings
            .iter()
            .filter(|mapping| mapping.symbol.eq_ignore_ascii_case(&position.symbol))
        {
            let exposure = (position.allocation_pct * mapping.exposure_multiplier).max(0.0);
            if exposure <= 0.0 {
                continue;
            }
            grouped
                .entry(clean_name(&mapping.factor))
                .or_default()
                .push((
                    position.symbol.clone(),
                    exposure,
                    normalize_direction(&mapping.direction).to_string(),
                ));
        }
    }

    let mut factors = grouped
        .into_iter()
        .map(|(name, rows)| {
            let exposure_pct = rows.iter().map(|(_, exposure, _)| exposure).sum::<f64>();
            let direction = aggregate_direction(&rows);
            let mut contributors = rows
                .into_iter()
                .map(|(symbol, exposure_pct, _)| FactorContributor {
                    symbol,
                    exposure_pct,
                })
                .collect::<Vec<_>>();
            contributors.sort_by(|a, b| {
                b.exposure_pct
                    .partial_cmp(&a.exposure_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.symbol.cmp(&b.symbol))
            });
            FactorAggregate {
                prob_pct: scenario_probability(scenarios, &name),
                name,
                exposure_pct: exposure_pct.min(100.0),
                direction,
                contributors,
            }
        })
        .collect::<Vec<_>>();
    factors.sort_by(|a, b| {
        b.exposure_pct
            .partial_cmp(&a.exposure_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    factors
}

fn aggregate_direction(rows: &[(String, f64, String)]) -> String {
    let first = rows
        .first()
        .map(|(_, _, direction)| direction.as_str())
        .unwrap_or("mixed");
    if rows.iter().all(|(_, _, direction)| direction == first) {
        first.to_string()
    } else {
        "mixed".to_string()
    }
}

fn scenario_probability(scenarios: &[PrivateMacroScenarioRow], factor: &str) -> Option<f64> {
    scenarios
        .iter()
        .find(|scenario| scenario.name.eq_ignore_ascii_case(factor))
        .map(|scenario| scenario.probability.clamp(0.0, 100.0))
}

fn native_placeholder(factors: &[FactorAggregate]) -> String {
    let args = factors
        .iter()
        .map(|factor| {
            format!(
                "{}:{}:{}:{}",
                clean_arg(&factor.name),
                format_number(factor.exposure_pct),
                factor.direction,
                factor
                    .prob_pct
                    .map(format_number)
                    .unwrap_or_else(|| "n/a".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{factor_exposure(factors=[{args}])}}")
}

fn render_factor_table(factors: &[FactorAggregate]) -> String {
    let mut output =
        String::from("| Factor | Exposure | Direction | Scenario Probability | Contributors |\n");
    output.push_str("|---|---:|---|---:|---|\n");
    for factor in factors {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean_cell(&factor.name),
            format_pct(factor.exposure_pct),
            factor.direction,
            factor
                .prob_pct
                .map(format_pct)
                .unwrap_or_else(|| "n/a".to_string()),
            contributors_cell(&factor.contributors),
        ));
    }
    output.trim_end().to_string()
}

fn risk_paragraph(factors: &[FactorAggregate]) -> String {
    let top = factors.first().expect("risk paragraph requires factors");
    let high_prob = factors
        .iter()
        .find(|factor| factor.prob_pct.unwrap_or(0.0) >= HIGH_PROBABILITY_THRESHOLD_PCT);
    match high_prob {
        Some(factor) if factor.exposure_pct >= HIGH_EXPOSURE_THRESHOLD_PCT => format!(
            "{} is the dominant mapped concentration at {}, led by {}. High-probability scenario alignment is elevated: {} is at {} probability with {} mapped exposure, so hedge pressure should be read as active rather than theoretical.",
            clean_cell(&top.name),
            format_pct(top.exposure_pct),
            contributors_cell(&top.contributors),
            clean_cell(&factor.name),
            format_pct(factor.prob_pct.unwrap_or_default()),
            format_pct(factor.exposure_pct),
        ),
        Some(factor) => format!(
            "{} is the dominant mapped concentration at {}, led by {}. {} is high probability at {}, but mapped exposure is {}, so hedge pressure is present without a majority portfolio concentration.",
            clean_cell(&top.name),
            format_pct(top.exposure_pct),
            contributors_cell(&top.contributors),
            clean_cell(&factor.name),
            format_pct(factor.prob_pct.unwrap_or_default()),
            format_pct(factor.exposure_pct),
        ),
        None => format!(
            "{} is the dominant mapped concentration at {}, led by {}. No attached scenario probability is above {}, so hedge pressure is driven by allocation clustering rather than a high-probability scenario.",
            clean_cell(&top.name),
            format_pct(top.exposure_pct),
            contributors_cell(&top.contributors),
            format_pct(HIGH_PROBABILITY_THRESHOLD_PCT),
        ),
    }
}

fn contributors_cell(contributors: &[FactorContributor]) -> String {
    contributors
        .iter()
        .map(|contributor| {
            format!(
                "{} {}",
                clean_cell(&contributor.symbol),
                format_pct(contributor.exposure_pct)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn normalize_direction(direction: &str) -> &'static str {
    match direction.trim().to_ascii_lowercase().as_str() {
        "bull" | "bullish" | "positive" | "up" => "bull",
        "bear" | "bearish" | "negative" | "down" => "bear",
        "neutral" | "flat" => "neutral",
        _ => "mixed",
    }
}

fn format_pct(value: f64) -> String {
    format!("{}%", format_number(value))
}

fn format_number(value: f64) -> String {
    format!("{value:.2}")
}

fn clean_name(value: &str) -> String {
    value.trim().to_string()
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn clean_arg(value: &str) -> String {
    clean_cell(value).replace([',', '[', ']', '{', '}'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_risk_concentration_exposure_percentages_come_from_fixture_allocations() {
        let rendered = render_private_risk_concentration(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## Risk Concentration\n\n"));
        assert!(rendered.contains(
            "{factor_exposure(factors=[Inflation Spike:51.18:bull:88.00, Hard Recession:22.95:bear:32.00, Liquidity Shock:21.00:mixed:n/a])}"
        ));
        assert!(rendered
            .contains("| Inflation Spike | 51.18% | bull | 88.00% | BTC 33.60%, GLD 17.58% |"));
    }

    #[test]
    fn private_risk_concentration_describes_high_probability_alignment() {
        let rendered = render_private_risk_concentration(&fixture_context()).unwrap();

        assert!(rendered.contains(
            "High-probability scenario alignment is elevated: Inflation Spike is at 88.00% probability with 51.18% mapped exposure"
        ));
    }

    #[test]
    fn private_risk_concentration_missing_factor_mapping_has_fallback() {
        let rendered = render_private_risk_concentration(&BuildContext {
            private_positions: vec![position("BTC", 42.0)],
            ..BuildContext::default()
        })
        .unwrap();

        assert!(
            rendered.contains("No factor mapping rows are attached for qualifying held assets.")
        );
        assert!(!rendered.contains("{factor_exposure("));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_positions: vec![
                position("BTC", 42.0),
                position("GLD", 22.95),
                position("DOGE", 0.05),
            ],
            private_risk_factor_mappings: vec![
                mapping("BTC", "Inflation Spike", "bullish", 0.80),
                mapping("GLD", "Inflation Spike", "bull", 0.766),
                mapping("GLD", "Hard Recession", "bearish", 1.00),
                mapping("BTC", "Liquidity Shock", "bull", 0.25),
                mapping("GLD", "Liquidity Shock", "bear", 0.4575),
                mapping("DOGE", "Inflation Spike", "bull", 1.00),
            ],
            private_macro_scenarios: vec![
                scenario("Inflation Spike", 88.0),
                scenario("Hard Recession", 32.0),
            ],
            ..BuildContext::default()
        }
    }

    fn position(symbol: &str, allocation_pct: f64) -> PrivatePositionSnapshotRow {
        PrivatePositionSnapshotRow {
            symbol: symbol.to_string(),
            price: None,
            daily_change: None,
            allocation_pct,
            unrealized_pnl: None,
        }
    }

    fn mapping(
        symbol: &str,
        factor: &str,
        direction: &str,
        exposure_multiplier: f64,
    ) -> PrivateRiskFactorMapping {
        PrivateRiskFactorMapping {
            symbol: symbol.to_string(),
            factor: factor.to_string(),
            direction: direction.to_string(),
            exposure_multiplier,
        }
    }

    fn scenario(name: &str, probability: f64) -> PrivateMacroScenarioRow {
        PrivateMacroScenarioRow {
            name: name.to_string(),
            probability,
            prior_7d: probability,
        }
    }
}
