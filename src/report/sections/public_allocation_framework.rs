#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{BuildContext, PublicScenarioRow};

const ASSET_CLASSES: [&str; 6] = [
    "Cash",
    "BTC",
    "Gold/Silver",
    "Equities",
    "Commodities",
    "Treasuries",
];

const PROFILES: [AllocationProfile; 3] = [
    AllocationProfile {
        name: "Conservative",
        investor_type: "capital-preservation investors in a volatile regime",
        ranges: ["25-40%", "0-5%", "10-20%", "20-35%", "5-10%", "20-35%"],
        rationale: "emphasises liquidity, sovereign duration, and modest hard-asset protection while keeping equity and BTC exposure constrained",
    },
    AllocationProfile {
        name: "Balanced",
        investor_type: "investors balancing drawdown control with participation",
        ranges: ["10-25%", "5-15%", "10-20%", "35-50%", "5-15%", "10-25%"],
        rationale: "keeps broad equity participation while reserving meaningful liquidity, hard-asset, and rates-sensitive ballast",
    },
    AllocationProfile {
        name: "Conviction-Driven",
        investor_type: "investors with high tolerance for volatility and thesis concentration",
        ranges: ["5-15%", "15-30%", "15-30%", "25-45%", "5-15%", "0-15%"],
        rationale: "allows higher BTC and precious-metals exposure when the thesis is strong, while retaining some liquidity and cyclical optionality",
    },
];

struct AllocationProfile {
    name: &'static str,
    investor_type: &'static str,
    ranges: [&'static str; 6],
    rationale: &'static str,
}

pub fn render_public_allocation_framework(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Allocation Framework\n\n");
    output.push_str("These are generic, regime-aware ranges for different investor types. They are not account-specific sizing, execution instructions, or a statement of the correct mix for any specific account.\n\n");
    output.push_str(&render_context(ctx));
    output.push_str("\n\n");
    output.push_str(&render_framework_table());
    output.push_str("\n\n");
    output.push_str(&render_profile_notes());

    Ok(output.trim_end().to_string())
}

fn render_context(ctx: &BuildContext) -> String {
    let regime = ctx
        .regime
        .as_ref()
        .map(|regime| {
            let detail = regime
                .detail
                .as_deref()
                .map(sentence_fragment)
                .unwrap_or_else(|| "no regime detail attached".to_string());
            format!("Regime context: {} ({detail})", readable(&regime.classification))
        })
        .unwrap_or_else(|| {
            "Regime context: unavailable, so the ranges remain template-level rather than regime-tuned".to_string()
        });

    let scenario = dominant_scenario(&ctx.public_scenarios)
        .map(|scenario| {
            format!(
                "Dominant scenario input: {} at {:.0}% probability",
                clean_text(&scenario.name),
                scenario.probability
            )
        })
        .unwrap_or_else(|| "Dominant scenario input: unavailable".to_string());

    format!("{regime}. {scenario}.")
}

fn render_framework_table() -> String {
    let mut output = String::from("| Framework | Investor Type | Cash | BTC | Gold/Silver | Equities | Commodities | Treasuries |\n");
    output.push_str("|---|---|---:|---:|---:|---:|---:|---:|\n");
    for profile in &PROFILES {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            profile.name,
            profile.investor_type,
            profile.ranges[0],
            profile.ranges[1],
            profile.ranges[2],
            profile.ranges[3],
            profile.ranges[4],
            profile.ranges[5],
        ));
    }
    output.trim_end().to_string()
}

fn render_profile_notes() -> String {
    let mut output = String::from("Framework interpretation:\n");
    for profile in &PROFILES {
        output.push_str(&format!(
            "- {}: {}.\n",
            profile.name,
            sentence_fragment(profile.rationale)
        ));
    }
    output.push_str(
        "- Range endpoints are guardrails for generic discussion, not live trading bands.",
    );
    output
}

fn dominant_scenario(rows: &[PublicScenarioRow]) -> Option<&PublicScenarioRow> {
    rows.iter().max_by(|a, b| {
        a.probability
            .partial_cmp(&b.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn readable(value: &str) -> String {
    value.replace(['_', '-'], " ")
}

fn sentence_fragment(value: &str) -> String {
    value.trim().trim_end_matches(['.', '!', '?']).to_string()
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{RegimeSummary, SynthesisSnapshot};

    #[test]
    fn public_allocation_framework_renders_generic_profiles() {
        let ctx = BuildContext {
            synthesis: Some(SynthesisSnapshot {
                summary: "Liquidity is defensive while hard assets hold relative strength"
                    .to_string(),
                central_tension: None,
            }),
            regime: Some(RegimeSummary {
                classification: "defensive_liquidity".to_string(),
                detail: Some("volatility is elevated and breadth remains narrow".to_string()),
            }),
            public_scenarios: vec![
                scenario("Soft Landing", 35.0),
                scenario("Inflation Reacceleration", 45.0),
            ],
            ..BuildContext::default()
        };

        let rendered = render_public_allocation_framework(&ctx).unwrap();

        assert!(rendered.starts_with("## Allocation Framework\n\n"));
        assert!(rendered.contains("defensive liquidity"));
        assert!(rendered.contains("Inflation Reacceleration at 45% probability"));
        assert!(rendered.contains("| Conservative | capital-preservation investors"));
        assert!(rendered.contains("| Balanced | investors balancing drawdown control"));
        assert!(rendered.contains("| Conviction-Driven | investors with high tolerance"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_allocation_framework_contains_each_asset_class_for_each_profile() {
        let rendered = render_public_allocation_framework(&BuildContext::default()).unwrap();
        let table_lines = rendered
            .lines()
            .filter(|line| {
                line.starts_with("| Conservative |")
                    || line.starts_with("| Balanced |")
                    || line.starts_with("| Conviction-Driven |")
            })
            .collect::<Vec<_>>();

        assert_eq!(table_lines.len(), 3);
        for asset_class in ASSET_CLASSES {
            assert!(
                rendered.contains(asset_class),
                "missing asset class {asset_class}: {rendered}"
            );
        }
        for line in table_lines {
            let range_count = line.matches('%').count();
            assert_eq!(range_count, ASSET_CLASSES.len(), "bad row: {line}");
        }
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_allocation_framework_avoids_imperative_personal_advice() {
        let rendered = render_public_allocation_framework(&BuildContext::default()).unwrap();
        let lowered = rendered.to_ascii_lowercase();

        for forbidden in [
            "you should",
            "you must",
            "buy ",
            "sell ",
            "increase your",
            "trim your",
            "right allocation",
            "current allocation",
            "personal portfolio",
            "transaction",
            "cost basis",
            "unrealized",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public allocation framework leaked advice/private phrase {forbidden}: {rendered}"
            );
        }
    }

    fn scenario(name: &str, probability: f64) -> PublicScenarioRow {
        PublicScenarioRow {
            name: name.to_string(),
            probability,
            delta_7d: None,
            narrative_vs_money: None,
            key_driver: None,
            confirmation: None,
            invalidation: None,
        }
    }

    fn assert_public_safe(markdown: &str) {
        let lowered = markdown.to_ascii_lowercase();
        for forbidden in [
            "i hold",
            "we own",
            "our position",
            "my position",
            "cost basis",
            "unrealized",
            "transaction",
            "current allocation",
            "position size",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public allocation framework leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
