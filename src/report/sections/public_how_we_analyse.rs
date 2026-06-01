#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, CalibrationReliabilityRow, LessonAppliedSummary, PredictionMarketIntelligence,
    SourceTierOverrideSummary,
};

pub fn render_public_how_we_analyse(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## How We Analyse\n\n");

    output.push_str("We track prediction accountability, calibration, source quality, and lessons from prior misses so daily market reads do not stand alone as unsupported narrative.\n\n");
    output.push_str(&render_calibration(&ctx.public_calibration));
    output.push_str("\n\n");
    output.push_str(&render_lessons(&ctx.public_lessons_applied));
    output.push_str("\n\n");
    output.push_str(&render_prediction_intelligence(
        &ctx.public_prediction_intelligence,
    ));
    output.push_str("\n\n");
    output.push_str(&render_source_quality(&ctx.public_source_tier_overrides));

    Ok(output.trim_end().to_string())
}

fn render_calibration(rows: &[CalibrationReliabilityRow]) -> String {
    if rows.is_empty() {
        return "Calibration: no reliability rows are attached to this build, so the report cannot show a calibration chart for this run.".to_string();
    }

    let mut output = String::from("Calibration:\n\n{calibration_dot_plot(public_calibration)}\n\n| Layer | Conviction Band | Predicted | Observed | Samples | Caveat |\n|---|---|---:|---:|---:|---|\n");
    for row in rows {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            clean_cell(&row.layer),
            clean_cell(&row.conviction_band),
            format_pct(row.predicted_pct),
            format_pct(row.observed_pct),
            row.sample_size,
            sample_caveat(row.sample_size),
        ));
    }
    output.trim_end().to_string()
}

fn render_lessons(rows: &[LessonAppliedSummary]) -> String {
    if rows.is_empty() {
        return "Lessons applied: no structured lessons were attached in the last 24h. Treat this as an accountability gap rather than evidence that no prior lessons mattered.".to_string();
    }

    let mut output = String::from("Lessons applied:\n");
    for lesson in rows.iter().take(5) {
        output.push_str(&format!(
            "- {}: {}{}.\n",
            clean_text(&lesson.lesson_id),
            sentence_fragment(&lesson.summary),
            lesson
                .applied_to
                .as_deref()
                .map(|target| format!(" Applied to: {}", sentence_fragment(target)))
                .unwrap_or_default()
        ));
    }
    output.trim_end().to_string()
}

fn render_prediction_intelligence(rows: &[PredictionMarketIntelligence]) -> String {
    if rows.is_empty() {
        return "Prediction-market intelligence: no active or newly relevant prediction rows are attached to this build.".to_string();
    }

    let mut output =
        String::from("Prediction-market intelligence:\n\n| Market | Probability | 7d Delta | Read |\n|---|---:|---:|---|\n");
    for row in rows.iter().take(5) {
        output.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            clean_cell(&row.market),
            row.probability
                .map(format_pct)
                .unwrap_or_else(|| "n/a".to_string()),
            format_delta(row.delta_7d),
            clean_cell(row.read.as_deref().unwrap_or("n/a")),
        ));
    }
    output.trim_end().to_string()
}

fn render_source_quality(rows: &[SourceTierOverrideSummary]) -> String {
    if rows.is_empty() {
        return "News-quality filter: no source-tier overrides were attached. Use `pftui data news sources list --json` to audit current source tiers.".to_string();
    }

    let mut output = String::from("News-quality filter: source tiers weight catalyst confidence, and overrides are managed with parseable CLI commands.\n");
    for row in rows.iter().take(5) {
        output.push_str(&format!(
            "- {} -> Tier {}. {} Command: `pftui data news sources set {} --tier {} --json`.\n",
            clean_text(&row.domain),
            row.tier,
            sentence(
                row.reason
                    .as_deref()
                    .unwrap_or("No override reason attached")
            ),
            clean_text(&row.domain),
            row.tier,
        ));
    }
    output.trim_end().to_string()
}

fn sample_caveat(sample_size: u32) -> &'static str {
    if sample_size < 20 {
        "low sample"
    } else {
        "sample ok"
    }
}

fn format_pct(value: f64) -> String {
    format!("{value:.0}%")
}

fn format_delta(value: Option<f64>) -> String {
    match value {
        Some(value) if value > 0.0 => format!("+{value:.0}pp"),
        Some(value) => format!("{value:.0}pp"),
        None => "n/a".to_string(),
    }
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
    value.trim().to_string()
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn public_how_we_analyse_includes_calibration_chart_when_data_exists() {
        let ctx = BuildContext {
            public_calibration: vec![
                calibration("LOW", "60-70", 65.0, 62.0, 48),
                calibration("MACRO", "80-90", 85.0, 100.0, 6),
            ],
            public_lessons_applied: vec![LessonAppliedSummary {
                lesson_id: "L-42".to_string(),
                summary: "Do not overweight single-source ceasefire headlines".to_string(),
                applied_to: Some("geopolitical catalyst ranking".to_string()),
            }],
            public_prediction_intelligence: vec![PredictionMarketIntelligence {
                market: "Fed cuts by September".to_string(),
                probability: Some(42.0),
                delta_7d: Some(-6.0),
                read: Some("Rates market is fading near-term easing".to_string()),
            }],
            public_source_tier_overrides: vec![SourceTierOverrideSummary {
                domain: "centralbank.test".to_string(),
                tier: 1,
                reason: Some("Primary source for policy remarks".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_how_we_analyse(&ctx).unwrap();

        assert!(rendered.starts_with("## How We Analyse\n\n"));
        assert!(rendered.contains("{calibration_dot_plot(public_calibration)}"));
        assert!(rendered.contains("| LOW | 60-70 | 65% | 62% | 48 | sample ok |"));
        assert!(rendered.contains("| MACRO | 80-90 | 85% | 100% | 6 | low sample |"));
        assert!(rendered.contains("L-42: Do not overweight single-source ceasefire headlines Applied to: geopolitical catalyst ranking."));
        assert!(rendered.contains(
            "| Fed cuts by September | 42% | -6pp | Rates market is fading near-term easing |"
        ));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_how_we_analyse_marks_low_sample_rows() {
        let ctx = BuildContext {
            public_calibration: vec![calibration("HIGH", "70-80", 75.0, 50.0, 3)],
            ..BuildContext::default()
        };

        let rendered = render_public_how_we_analyse(&ctx).unwrap();

        assert!(rendered.contains("| HIGH | 70-80 | 75% | 50% | 3 | low sample |"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_how_we_analyse_source_tier_command_references_parse() {
        let ctx = BuildContext {
            public_source_tier_overrides: vec![SourceTierOverrideSummary {
                domain: "example.com".to_string(),
                tier: 2,
                reason: Some("Known editorial process".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_how_we_analyse(&ctx).unwrap();
        assert!(rendered.contains("pftui data news sources set example.com --tier 2 --json"));

        Cli::try_parse_from([
            "pftui",
            "data",
            "news",
            "sources",
            "set",
            "example.com",
            "--tier",
            "2",
            "--json",
        ])
        .unwrap();
    }

    fn calibration(
        layer: &str,
        conviction_band: &str,
        predicted_pct: f64,
        observed_pct: f64,
        sample_size: u32,
    ) -> CalibrationReliabilityRow {
        CalibrationReliabilityRow {
            layer: layer.to_string(),
            conviction_band: conviction_band.to_string(),
            predicted_pct,
            observed_pct,
            sample_size,
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
                "public analysis leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
