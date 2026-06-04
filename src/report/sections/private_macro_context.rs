#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateMacroCatalyst, PrivateMacroRegimeQuadrant, PrivateMacroScenarioRow,
    PrivateNarrativeMoneyDivergence, PrivateRegimeTrailPoint,
};
use crate::report::charts::prob_bar::{render_svg as prob_bar_svg, ProbBarInput};
use crate::report::charts::regime_quadrant::{
    render_svg as regime_quadrant_svg, RegimeQuadrantInput, RegimeTrailPoint,
};

const NORMALIZED_TOTAL: f64 = 100.0;
const EPSILON: f64 = 0.05;

pub fn render_private_macro_context(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Macro Context\n\n");
    output.push_str("<!-- macro_dashboard - regime quadrant and scenario probability bars -->\n");
    output.push_str(&render_chart_placeholders(ctx));
    output.push_str("\n\n");
    output.push_str(&render_macro_paragraph(ctx));
    output.push_str("\n\n");
    output.push_str(&render_scenario_semantics(&ctx.private_macro_scenarios));

    Ok(output.trim_end().to_string())
}

fn render_chart_placeholders(ctx: &BuildContext) -> String {
    let regime = ctx
        .private_macro_regime
        .as_ref()
        .map(render_regime_quadrant)
        .unwrap_or_default();
    let bars = render_probability_bars(&ctx.private_macro_scenarios);
    let mut parts = Vec::new();
    if !regime.is_empty() {
        parts.push(regime);
    }
    if !bars.is_empty() {
        parts.push(bars);
    }
    parts.join("\n")
}

fn render_regime_quadrant(row: &PrivateMacroRegimeQuadrant) -> String {
    let trail = row
        .trail
        .iter()
        .map(|point| RegimeTrailPoint(point.growth, point.inflation))
        .collect();
    regime_quadrant_svg(&RegimeQuadrantInput {
        growth: row.growth,
        inflation: row.inflation,
        trail,
        width: None,
        height: None,
    })
}

fn render_probability_bars(rows: &[PrivateMacroScenarioRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let mut sorted = rows.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        b.probability
            .partial_cmp(&a.probability)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    sorted
        .into_iter()
        .map(|row| {
            prob_bar_svg(&ProbBarInput {
                name: row.name.clone(),
                current: row.probability,
                prior_7d: row.prior_7d,
                color: "cyan".to_string(),
                max_pct: None,
                width: None,
                height: None,
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_macro_paragraph(ctx: &BuildContext) -> String {
    let regime = match (&ctx.regime, &ctx.private_macro_regime) {
        (Some(regime), Some(quadrant)) => format!(
            "Regime is {} with growth {} and inflation {} on the native quadrant",
            readable(&regime.classification),
            format_axis(quadrant.growth),
            format_axis(quadrant.inflation)
        ),
        (Some(regime), None) => format!(
            "Regime is {}, but no quadrant inputs are attached",
            readable(&regime.classification)
        ),
        (None, Some(quadrant)) => format!(
            "Regime classification is unavailable, while quadrant inputs show growth {} and inflation {}",
            format_axis(quadrant.growth),
            format_axis(quadrant.inflation)
        ),
        (None, None) => {
            "Regime classification and quadrant inputs are unavailable".to_string()
        }
    };
    let divergence = render_material_divergence(&ctx.private_macro_divergences);
    let catalysts = render_catalysts(&ctx.private_macro_catalysts);
    sentence(&format!("{regime}; {divergence}; {catalysts}"))
}

fn render_material_divergence(rows: &[PrivateNarrativeMoneyDivergence]) -> String {
    let material = rows
        .iter()
        .filter(|row| row.material)
        .map(|row| {
            format!(
                "{}: {}",
                clean_text(&row.scenario),
                sentence_fragment(&row.summary)
            )
        })
        .collect::<Vec<_>>();
    if material.is_empty() {
        "no material narrative-vs-money divergence is attached".to_string()
    } else {
        format!(
            "material narrative-vs-money divergence: {}",
            material.join("; ")
        )
    }
}

fn render_catalysts(rows: &[PrivateMacroCatalyst]) -> String {
    if rows.is_empty() {
        return "no near-term macro catalysts are attached".to_string();
    }

    rows.iter()
        .take(2)
        .map(|row| {
            format!(
                "{} on {} ({})",
                clean_text(&row.event),
                clean_text(&row.date),
                sentence_fragment(&row.impact)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn render_scenario_semantics(rows: &[PrivateMacroScenarioRow]) -> String {
    if rows.is_empty() {
        return "Scenario probability bars are unavailable until active scenario rows are attached to this private build.".to_string();
    }

    let sum: f64 = rows.iter().map(|row| row.probability).sum();
    if sum < NORMALIZED_TOTAL - EPSILON {
        format!(
            "Scenario bars use normalized scenario-set semantics: named rows sum to {}, leaving {} in Other / Unmodelled.",
            format_probability(sum),
            format_probability(NORMALIZED_TOTAL - sum),
        )
    } else if sum > NORMALIZED_TOTAL + EPSILON {
        format!(
            "Scenario bars use normalized scenario-set semantics, but attached rows sum to {}; treat this as overfilled legacy data before using expected-value math.",
            format_probability(sum),
        )
    } else {
        "Scenario bars use normalized scenario-set semantics: attached rows sum to 100%."
            .to_string()
    }
}

fn format_trail(points: &[PrivateRegimeTrailPoint]) -> String {
    let joined = points
        .iter()
        .map(|point| {
            format!(
                "({}, {})",
                format_axis(point.growth),
                format_axis(point.inflation)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{joined}]")
}

fn format_axis(value: f64) -> String {
    format!("{value:.2}")
}

fn format_pct_arg(value: f64) -> String {
    format!("{value:.0}")
}

fn format_probability(value: f64) -> String {
    format!("{value:.0}%")
}

fn readable(value: &str) -> String {
    value.replace(['_', '-'], " ")
}

fn sentence(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

fn sentence_fragment(value: &str) -> String {
    value.trim().trim_end_matches(['.', '!', '?']).to_string()
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn clean_arg(value: &str) -> String {
    clean_text(value).replace(',', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{RegimeSummary, SynthesisSnapshot};

    #[test]
    fn private_macro_context_scenario_bars_use_normalized_semantics() {
        let rendered = render_private_macro_context(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## Macro Context\n\n"));
        // Charts now render as inline SVG, not as token placeholders.
        assert!(
            !rendered.contains("{regime_quadrant("),
            "must not leak the regime_quadrant token: {rendered}"
        );
        assert!(
            !rendered.contains("{prob_bar("),
            "must not leak the prob_bar token: {rendered}"
        );
        assert!(rendered.contains("<svg"), "expected inline SVG charts");
        // Scenario names are present in the rendered SVG.
        assert!(rendered.contains("Hard Landing"));
        assert!(rendered.contains("Inflation Reacceleration"));
        assert!(rendered.contains("Soft Landing"));
        assert!(rendered.contains("named rows sum to 90%, leaving 10% in Other / Unmodelled"));
    }

    #[test]
    fn private_macro_context_includes_material_divergence() {
        let rendered = render_private_macro_context(&fixture_context()).unwrap();

        assert!(rendered.contains("material narrative-vs-money divergence"));
        assert!(rendered.contains("Inflation Reacceleration: headlines are outrunning priced odds"));
        assert!(!rendered.contains("Soft Landing: aligned and not material"));
    }

    #[test]
    fn private_macro_context_stays_concise() {
        let rendered = render_private_macro_context(&fixture_context()).unwrap();
        let prose_paragraphs = rendered
            .split("\n\n")
            .filter(|paragraph| {
                !paragraph.starts_with("##")
                    && !paragraph.starts_with("<!--")
                    && !paragraph.starts_with("{")
            })
            .count();

        assert!(prose_paragraphs <= 2, "{rendered}");
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            synthesis: Some(SynthesisSnapshot {
                summary: "macro risk is concentrated in inflation persistence".to_string(),
                central_tension: None,
            }),
            regime: Some(RegimeSummary {
                classification: "stagflation_watch".to_string(),
                detail: Some("growth is cooling while inflation pressure remains firm".to_string()),
            }),
            private_macro_regime: Some(PrivateMacroRegimeQuadrant {
                growth: -0.35,
                inflation: 0.70,
                trail: vec![trail(-0.20, 0.40), trail(-0.35, 0.70)],
            }),
            private_macro_scenarios: vec![
                scenario("Soft Landing", 30.0, 35.0),
                scenario("Hard Landing", 35.0, 30.0),
                scenario("Inflation Reacceleration", 25.0, 20.0),
            ],
            private_macro_divergences: vec![
                divergence(
                    "Inflation Reacceleration",
                    "headlines are outrunning priced odds",
                    true,
                ),
                divergence("Soft Landing", "aligned and not material", false),
            ],
            private_macro_catalysts: vec![
                catalyst("2026-06-03", "FOMC decision", "rates path reprices risk"),
                catalyst("2026-06-05", "Payrolls", "growth read updates landing odds"),
            ],
            ..BuildContext::default()
        }
    }

    fn trail(growth: f64, inflation: f64) -> PrivateRegimeTrailPoint {
        PrivateRegimeTrailPoint { growth, inflation }
    }

    fn scenario(name: &str, probability: f64, prior_7d: f64) -> PrivateMacroScenarioRow {
        PrivateMacroScenarioRow {
            name: name.to_string(),
            probability,
            prior_7d,
        }
    }

    fn divergence(
        scenario: &str,
        summary: &str,
        material: bool,
    ) -> PrivateNarrativeMoneyDivergence {
        PrivateNarrativeMoneyDivergence {
            scenario: scenario.to_string(),
            summary: summary.to_string(),
            material,
        }
    }

    fn catalyst(date: &str, event: &str, impact: &str) -> PrivateMacroCatalyst {
        PrivateMacroCatalyst {
            date: date.to_string(),
            event: event.to_string(),
            impact: impact.to_string(),
        }
    }
}
