#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, CalibrationReliabilityRow, PrivateRegimeConditionalSummary,
};

pub const SECTION_PRIVACY: &str = "private";

const LOW_SAMPLE_THRESHOLD: u32 = 20;
const MAX_HIGHLIGHT_BULLETS: usize = 3;
const MIN_HIGHLIGHT_BULLETS: usize = 2;

pub fn render_private_self_retrospective_calibration(ctx: &BuildContext) -> Result<String> {
    let rows = &ctx.private_calibration;
    if rows.is_empty() && ctx.private_regime_conditional.is_none() {
        // Suppress the entire section when no calibration substrate is
        // attached. The previous "No 90-day calibration rows…" disclosure
        // wasted a page on every report and produced operator complaint.
        return Ok(String::new());
    }
    let mut output = String::from("## Self-Retrospective Calibration\n\n");
    if rows.is_empty() {
        // Calibration empty but regime-conditional present — fall through
        // to the regime block at the bottom of the function.
        if let Some(regime) = &ctx.private_regime_conditional {
            output.push_str(&render_regime_conditional(regime));
            return Ok(output.trim_end().to_string());
        }
        return Ok(String::new());
    }

    output.push_str("{calibration_dot_plot(private_calibration)}\n\n");

    let highlights = largest_miscalibrations(rows);
    if highlights.is_empty() {
        output.push_str(
            "- Calibration rows are attached but every row sits at zero miscalibration delta.",
        );
        return Ok(output.trim_end().to_string());
    }

    for row in highlights {
        output.push_str(&render_bullet(row));
        output.push('\n');
    }

    if let Some(regime) = &ctx.private_regime_conditional {
        output.push('\n');
        output.push_str(&render_regime_conditional(regime));
        output.push('\n');
    }

    Ok(output.trim_end().to_string())
}

fn render_regime_conditional(summary: &PrivateRegimeConditionalSummary) -> String {
    format!(
        "- Current regime is {regime}; under similar past regimes, the {layer} layer hit rate on {topic} was {hit:.0}% (n={n}).",
        regime = summary.current_regime,
        layer = summary.top_layer.to_uppercase(),
        topic = summary.top_topic,
        hit = summary.hit_rate_pct,
        n = summary.sample_size,
    )
}

fn largest_miscalibrations(rows: &[CalibrationReliabilityRow]) -> Vec<&CalibrationReliabilityRow> {
    let mut indexed = rows
        .iter()
        .enumerate()
        .map(|(idx, row)| (idx, row, miscalibration_delta(row).abs()))
        .collect::<Vec<_>>();
    indexed.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.sample_size.cmp(&a.1.sample_size))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut picked: Vec<&CalibrationReliabilityRow> = indexed
        .iter()
        .filter(|(_, _, delta)| *delta > 0.0)
        .take(MAX_HIGHLIGHT_BULLETS)
        .map(|(_, row, _)| *row)
        .collect();
    if picked.len() < MIN_HIGHLIGHT_BULLETS {
        for (_, row, _) in &indexed {
            if picked.len() >= MIN_HIGHLIGHT_BULLETS {
                break;
            }
            if !picked.iter().any(|existing| std::ptr::eq(*existing, *row)) {
                picked.push(*row);
            }
        }
    }
    picked
}

fn render_bullet(row: &CalibrationReliabilityRow) -> String {
    let delta = miscalibration_delta(row);
    let direction = if delta > 0.0 {
        "overconfident"
    } else if delta < 0.0 {
        "underconfident"
    } else {
        "calibrated"
    };
    let caveat = if row.sample_size < LOW_SAMPLE_THRESHOLD {
        " (low sample)"
    } else {
        ""
    };
    format!(
        "- {} {} band: predicted {}, observed {} ({} by {}, n={}){}.\n",
        clean_cell(&row.layer),
        clean_cell(&row.conviction_band),
        format_pct(row.predicted_pct),
        format_pct(row.observed_pct),
        direction,
        format_delta_pp(delta),
        row.sample_size,
        caveat,
    )
}

fn miscalibration_delta(row: &CalibrationReliabilityRow) -> f64 {
    row.predicted_pct - row.observed_pct
}

fn format_pct(value: f64) -> String {
    format!("{value:.0}%")
}

fn format_delta_pp(value: f64) -> String {
    format!("{:.0}pp", value.abs())
}

fn clean_cell(value: &str) -> String {
    value.replace(['|', '\n'], " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_self_retrospective_calibration_renders_chart_placeholder() {
        let rendered =
            render_private_self_retrospective_calibration(&fixture_context()).unwrap();
        assert!(rendered.starts_with("## Self-Retrospective Calibration\n\n"));
        assert!(rendered.contains("{calibration_dot_plot(private_calibration)}"));
    }

    #[test]
    fn private_self_retrospective_calibration_selects_largest_absolute_deltas() {
        let rendered =
            render_private_self_retrospective_calibration(&fixture_context()).unwrap();
        // HIGH 80-90 has 30pp gap (largest absolute), should appear first after chart.
        let pos_high = rendered.find("HIGH 80-90 band").expect("HIGH bullet present");
        let pos_low = rendered.find("LOW 30-40 band").expect("LOW bullet present");
        let pos_medium = rendered.find("MEDIUM 60-70 band");
        assert!(
            pos_high < pos_low,
            "largest absolute delta should be listed before smaller delta: {rendered}"
        );
        // 5pp aligned row should NOT be highlighted before the LOW 20pp underconfident row.
        if let Some(medium) = pos_medium {
            assert!(
                pos_low < medium,
                "20pp gap should rank above 5pp gap: {rendered}"
            );
        }
    }

    #[test]
    fn private_self_retrospective_calibration_marks_low_sample_caveats() {
        let rendered =
            render_private_self_retrospective_calibration(&fixture_context()).unwrap();
        assert!(rendered.contains("HIGH 80-90 band"));
        assert!(rendered.contains("(low sample)"), "low-sample caveat present: {rendered}");
    }

    #[test]
    fn private_self_retrospective_calibration_directional_labels() {
        let rendered =
            render_private_self_retrospective_calibration(&fixture_context()).unwrap();
        assert!(rendered.contains("overconfident"));
        assert!(rendered.contains("underconfident"));
    }

    #[test]
    fn private_self_retrospective_calibration_empty_fixture_suppresses_section() {
        // Empty calibration substrate => suppress section entirely.
        let rendered =
            render_private_self_retrospective_calibration(&BuildContext::default()).unwrap();
        assert!(rendered.is_empty());
    }

    #[test]
    fn private_self_retrospective_calibration_emits_at_least_two_bullets() {
        let rendered =
            render_private_self_retrospective_calibration(&fixture_context()).unwrap();
        let bullet_count = rendered.matches("\n- ").count();
        assert!(
            bullet_count >= MIN_HIGHLIGHT_BULLETS,
            "expected >= {MIN_HIGHLIGHT_BULLETS} bullets, got {bullet_count}: {rendered}"
        );
        assert!(
            bullet_count <= MAX_HIGHLIGHT_BULLETS,
            "expected <= {MAX_HIGHLIGHT_BULLETS} bullets, got {bullet_count}: {rendered}"
        );
    }

    #[test]
    fn private_self_retrospective_calibration_is_marked_private_only() {
        assert_eq!(SECTION_PRIVACY, "private");
    }

    #[test]
    fn private_self_retrospective_calibration_appends_regime_conditional() {
        let mut ctx = fixture_context();
        ctx.private_regime_conditional = Some(PrivateRegimeConditionalSummary {
            current_regime: "stagflation-iran-cool".to_string(),
            top_layer: "low".to_string(),
            top_topic: "commodities".to_string(),
            hit_rate_pct: 65.0,
            sample_size: 18,
        });
        let rendered = render_private_self_retrospective_calibration(&ctx).unwrap();
        assert!(
            rendered.contains("Current regime is stagflation-iran-cool"),
            "{rendered}"
        );
        assert!(rendered.contains("LOW layer hit rate on commodities was 65%"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_calibration: vec![
                calibration("HIGH", "80-90", 85.0, 55.0, 6),
                calibration("LOW", "30-40", 35.0, 55.0, 42),
                calibration("MEDIUM", "60-70", 65.0, 60.0, 38),
                calibration("MACRO", "70-80", 75.0, 75.0, 30),
            ],
            ..BuildContext::default()
        }
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
}
