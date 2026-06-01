#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{BuildContext, PublicScenarioRow};

const NORMALIZED_TOTAL: f64 = 100.0;
const EPSILON: f64 = 0.05;

pub fn render_public_scenario_dashboard(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Scenario Dashboard\n\n");

    if ctx.public_scenarios.is_empty() {
        output.push_str("No active scenario rows are attached to this report build. Scenario probabilities are unavailable until the scenario set refreshes.");
        return Ok(output);
    }

    output.push_str("| Scenario | Probability | 7d Delta | Narrative vs Money | Key Driver | Confirmation | Invalidation |\n");
    output.push_str("|---|---:|---:|---|---|---|---|\n");

    for row in &ctx.public_scenarios {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            clean_cell(&row.name),
            format_probability(row.probability),
            format_delta(row.delta_7d),
            clean_cell(row.narrative_vs_money.as_deref().unwrap_or("n/a")),
            clean_cell(row.key_driver.as_deref().unwrap_or("n/a")),
            clean_cell(row.confirmation.as_deref().unwrap_or("n/a")),
            clean_cell(row.invalidation.as_deref().unwrap_or("n/a")),
        ));
    }

    let modeled_sum = modeled_probability_sum(&ctx.public_scenarios);
    if modeled_sum < NORMALIZED_TOTAL - EPSILON {
        output.push_str(&format!(
            "| Other / Unmodelled | {} | n/a | residual uncertainty bucket | Outcomes outside named scenarios | n/a | n/a |\n",
            format_probability(NORMALIZED_TOTAL - modeled_sum)
        ));
        output.push_str("\nScenario probabilities use a normalized scenario-set model: named scenarios plus Other / Unmodelled sum to 100%.");
    } else if modeled_sum > NORMALIZED_TOTAL + EPSILON {
        output.push_str(&format!(
            "\nData-quality warning: modeled scenario probabilities sum to {}. Under the normalized scenario-set model this is overfilled legacy data, not evidence that scenarios overlap. Rebalance the scenario set before using expected-value math.",
            format_probability(modeled_sum)
        ));
    } else {
        output.push_str("\nScenario probabilities use a normalized scenario-set model and the named rows sum to 100%.");
    }

    Ok(output.trim_end().to_string())
}

fn modeled_probability_sum(rows: &[PublicScenarioRow]) -> f64 {
    rows.iter()
        .filter(|row| !row.name.eq_ignore_ascii_case("Other / Unmodelled"))
        .map(|row| row.probability)
        .sum()
}

fn format_probability(value: f64) -> String {
    format!("{value:.0}%")
}

fn format_delta(value: Option<f64>) -> String {
    match value {
        Some(value) if value > 0.0 => format!("+{value:.0}pp"),
        Some(value) => format!("{value:.0}pp"),
        None => "n/a".to_string(),
    }
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_scenario_dashboard_renders_probability_deltas() {
        let ctx = BuildContext {
            public_scenarios: vec![
                scenario(
                    "Soft Landing",
                    45.0,
                    Some(5.0),
                    "money confirming narrative",
                    "disinflation with resilient labor",
                ),
                scenario(
                    "Hard Recession",
                    30.0,
                    Some(-3.0),
                    "money weaker than narrative",
                    "credit stress",
                ),
                scenario(
                    "Inflation Reacceleration",
                    15.0,
                    None,
                    "narrative leading money",
                    "oil and wage pressure",
                ),
            ],
            ..BuildContext::default()
        };

        let rendered = render_public_scenario_dashboard(&ctx).unwrap();

        assert!(rendered.starts_with("## Scenario Dashboard\n\n"));
        assert!(rendered.contains("| Soft Landing | 45% | +5pp | money confirming narrative | disinflation with resilient labor |"));
        assert!(rendered.contains(
            "| Hard Recession | 30% | -3pp | money weaker than narrative | credit stress |"
        ));
        assert!(rendered.contains("| Inflation Reacceleration | 15% | n/a | narrative leading money | oil and wage pressure |"));
        assert!(
            rendered.contains("| Other / Unmodelled | 10% | n/a | residual uncertainty bucket |")
        );
        assert!(rendered.contains("named scenarios plus Other / Unmodelled sum to 100%"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_scenario_dashboard_residual_semantics_match_analytics_spec() {
        let ctx = BuildContext {
            public_scenarios: vec![
                scenario(
                    "Inflation Spike",
                    25.0,
                    Some(2.0),
                    "aligned",
                    "CPI surprise",
                ),
                scenario(
                    "Hard Recession",
                    35.0,
                    Some(-1.0),
                    "divergent",
                    "labor break",
                ),
                scenario(
                    "Soft Landing",
                    30.0,
                    Some(1.0),
                    "aligned",
                    "earnings breadth",
                ),
            ],
            ..BuildContext::default()
        };

        let rendered = render_public_scenario_dashboard(&ctx).unwrap();

        assert!(rendered.contains("| Other / Unmodelled | 10% |"));
        assert!(!rendered
            .to_ascii_lowercase()
            .contains("independent marginal"));
        assert!(!rendered.to_ascii_lowercase().contains("overlap"));
    }

    #[test]
    fn public_scenario_dashboard_warns_on_overfilled_legacy_data() {
        let ctx = BuildContext {
            public_scenarios: vec![
                scenario(
                    "Oil Shock",
                    60.0,
                    None,
                    "narrative ahead",
                    "shipping disruption",
                ),
                scenario("Hard Recession", 55.0, None, "money ahead", "credit stress"),
            ],
            ..BuildContext::default()
        };

        let rendered = render_public_scenario_dashboard(&ctx).unwrap();

        assert!(rendered.contains("Data-quality warning"));
        assert!(rendered.contains("sum to 115%"));
        assert!(rendered.contains("overfilled legacy data, not evidence that scenarios overlap"));
        assert!(!rendered.contains("Other / Unmodelled | -15%"));
        assert_public_safe(&rendered);
    }

    fn scenario(
        name: &str,
        probability: f64,
        delta_7d: Option<f64>,
        narrative_vs_money: &str,
        key_driver: &str,
    ) -> PublicScenarioRow {
        PublicScenarioRow {
            name: name.to_string(),
            probability,
            delta_7d,
            narrative_vs_money: Some(narrative_vs_money.to_string()),
            key_driver: Some(key_driver.to_string()),
            confirmation: Some("confirmation trigger".to_string()),
            invalidation: Some("invalidation trigger".to_string()),
        }
    }

    fn assert_public_safe(markdown: &str) {
        let lowered = markdown.to_ascii_lowercase();
        for forbidden in [
            "i hold",
            "we own",
            "our position",
            "cost basis",
            "unrealized",
            "transaction",
            "allocation",
            "position size",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public scenario dashboard leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
