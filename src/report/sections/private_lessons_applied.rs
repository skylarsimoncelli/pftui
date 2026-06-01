#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateHistoricalAnalogRow, PrivateLessonReferenceRow,
    PrivateLessonsAppliedSummary,
};

pub const SECTION_PRIVACY: &str = "private";

const MAX_LESSON_ROWS: usize = 5;
const LESSON_SUMMARY_CHAR_CAP: usize = 160;
const ANALOG_CLAIM_CHAR_CAP: usize = 140;

pub fn render_private_lessons_applied(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Lessons Applied This Run\n\n");

    let Some(summary) = ctx.private_lessons_applied.as_ref() else {
        output.push_str(accountability_gap_sentence());
        return Ok(output);
    };

    if summary.guarded_predictions == 0 || summary.lesson_references.is_empty() {
        output.push_str(accountability_gap_sentence());
        return Ok(output);
    }

    output.push_str(&render_headline(summary));
    output.push_str("\n\n");
    output.push_str(&render_top_lessons(&summary.lesson_references));

    if let Some(analog) = summary.strongest_analog.as_ref() {
        output.push_str("\n\n");
        output.push_str(&render_strongest_analog(analog));
    } else {
        output.push_str("\n\n");
        output.push_str(
            "Strongest historical analog: no prior wrong-scored prediction overlaps this run's applied lesson set."
        );
    }

    Ok(output.trim_end().to_string())
}

fn accountability_gap_sentence() -> &'static str {
    "No structured lessons were referenced by this run's predictions. Treat this as an accountability gap rather than evidence that no prior lessons applied."
}

fn render_headline(summary: &PrivateLessonsAppliedSummary) -> String {
    let window = if summary.since.trim().is_empty() {
        "24h".to_string()
    } else {
        clean_text(&summary.since)
    };
    let prediction_word = if summary.guarded_predictions == 1 {
        "prediction"
    } else {
        "predictions"
    };
    let lesson_word = if summary.unique_lessons == 1 {
        "lesson"
    } else {
        "lessons"
    };
    format!(
        "Guarded predictions in the last {}: {} of {} referenced {} unique {}.",
        window,
        summary.guarded_predictions,
        summary.total_predictions,
        summary.unique_lessons,
        lesson_word,
    ) + &format!(
        " {} of those {} carry at least one applied lesson id.",
        summary.guarded_predictions, prediction_word
    )
}

fn render_top_lessons(rows: &[PrivateLessonReferenceRow]) -> String {
    let mut sorted = rows.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        b.references
            .cmp(&a.references)
            .then_with(|| a.lesson_id.cmp(&b.lesson_id))
    });

    let mut output = String::from("Top referenced lessons:\n");
    for lesson in sorted.into_iter().take(MAX_LESSON_ROWS) {
        let miss = lesson
            .miss_type
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!(" [{}]", clean_text(value)))
            .unwrap_or_default();
        output.push_str(&format!(
            "- L-{} x{}{}: {}.\n",
            lesson.lesson_id,
            lesson.references,
            miss,
            sentence_fragment(&truncate(&lesson.summary, LESSON_SUMMARY_CHAR_CAP))
        ));
    }
    output.trim_end().to_string()
}

fn render_strongest_analog(analog: &PrivateHistoricalAnalogRow) -> String {
    let ids = if analog.overlapping_lesson_ids.is_empty() {
        "[]".to_string()
    } else {
        let joined = analog
            .overlapping_lesson_ids
            .iter()
            .map(|id| format!("L-{id}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{joined}]")
    };
    format!(
        "Strongest historical analog: prediction #{} ({}, {} overlapping {}: {}). Claim: {}.",
        analog.prediction_id,
        clean_text(&analog.outcome),
        analog.overlap_count,
        if analog.overlap_count == 1 {
            "lesson"
        } else {
            "lessons"
        },
        ids,
        sentence_fragment(&truncate(&analog.claim, ANALOG_CLAIM_CHAR_CAP))
    )
}

fn sentence_fragment(value: &str) -> String {
    value.trim().trim_end_matches(['.', '!', '?']).to_string()
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{
        BuildContext, PrivateHistoricalAnalogRow, PrivateLessonReferenceRow,
        PrivateLessonsAppliedSummary,
    };

    #[test]
    fn zero_lessons_renders_accountability_gap_sentence() {
        let ctx = BuildContext::default();
        let rendered = render_private_lessons_applied(&ctx).unwrap();

        assert!(rendered.starts_with("## Lessons Applied This Run\n\n"));
        assert!(rendered.contains("accountability gap"));
        assert!(!rendered.contains("Top referenced lessons:"));
        assert!(!rendered.contains("Strongest historical analog"));
    }

    #[test]
    fn empty_summary_struct_also_renders_accountability_gap() {
        let ctx = BuildContext {
            private_lessons_applied: Some(PrivateLessonsAppliedSummary {
                since: "24h".to_string(),
                total_predictions: 5,
                guarded_predictions: 0,
                unique_lessons: 0,
                lesson_references: vec![],
                strongest_analog: None,
            }),
            ..BuildContext::default()
        };

        let rendered = render_private_lessons_applied(&ctx).unwrap();
        assert!(rendered.contains("accountability gap"));
    }

    #[test]
    fn nonzero_fixture_lists_lesson_ids() {
        let ctx = fixture_with_two_lessons();

        let rendered = render_private_lessons_applied(&ctx).unwrap();

        assert!(rendered.starts_with("## Lessons Applied This Run\n\n"));
        assert!(rendered.contains("Guarded predictions in the last 24h"));
        assert!(rendered.contains("2 of 3 referenced 2 unique lessons"));
        assert!(rendered.contains("Top referenced lessons:"));
        assert!(rendered.contains("- L-218 x2 [timing]: Overweighted headline momentum"));
        assert!(rendered.contains("- L-240 x1 [signal]: Ignored liquidity regime"));
        assert!(!rendered.contains("accountability gap"));
    }

    #[test]
    fn top_lessons_sort_by_reference_count_then_id() {
        let ctx = BuildContext {
            private_lessons_applied: Some(PrivateLessonsAppliedSummary {
                since: "24h".to_string(),
                total_predictions: 10,
                guarded_predictions: 6,
                unique_lessons: 3,
                lesson_references: vec![
                    lesson_ref(300, 1, "timing", "Late entry on breakout"),
                    lesson_ref(100, 4, "signal", "Confused signal hierarchy"),
                    lesson_ref(200, 4, "regime", "Ignored regime shift"),
                ],
                strongest_analog: None,
            }),
            ..BuildContext::default()
        };

        let rendered = render_private_lessons_applied(&ctx).unwrap();
        let l100 = rendered.find("L-100").unwrap();
        let l200 = rendered.find("L-200").unwrap();
        let l300 = rendered.find("L-300").unwrap();
        // L-100 first (4 refs, lower id), L-200 second (4 refs, higher id), L-300 third (1 ref)
        assert!(l100 < l200);
        assert!(l200 < l300);
    }

    #[test]
    fn top_lessons_capped_at_five_rows() {
        let mut refs = Vec::new();
        for i in 1..=12 {
            refs.push(lesson_ref(i as i64, 13 - i, "timing", "lesson body"));
        }
        let ctx = BuildContext {
            private_lessons_applied: Some(PrivateLessonsAppliedSummary {
                since: "24h".to_string(),
                total_predictions: 30,
                guarded_predictions: 20,
                unique_lessons: 12,
                lesson_references: refs,
                strongest_analog: None,
            }),
            ..BuildContext::default()
        };
        let rendered = render_private_lessons_applied(&ctx).unwrap();
        let lesson_lines = rendered
            .lines()
            .filter(|line| line.starts_with("- L-"))
            .count();
        assert_eq!(lesson_lines, MAX_LESSON_ROWS);
    }

    #[test]
    fn renders_strongest_analog_when_present() {
        let ctx = fixture_with_two_lessons();

        let rendered = render_private_lessons_applied(&ctx).unwrap();

        assert!(rendered.contains(
            "Strongest historical analog: prediction #4 (wrong, 2 overlapping lessons: [L-218, L-240])."
        ));
        assert!(rendered.contains("Claim: BTC breaks 100k by month end"));
    }

    #[test]
    fn renders_explicit_gap_for_analog_when_absent() {
        let ctx = BuildContext {
            private_lessons_applied: Some(PrivateLessonsAppliedSummary {
                since: "24h".to_string(),
                total_predictions: 3,
                guarded_predictions: 2,
                unique_lessons: 1,
                lesson_references: vec![lesson_ref(99, 2, "timing", "Late entry")],
                strongest_analog: None,
            }),
            ..BuildContext::default()
        };

        let rendered = render_private_lessons_applied(&ctx).unwrap();

        assert!(rendered.contains(
            "Strongest historical analog: no prior wrong-scored prediction overlaps this run's applied lesson set."
        ));
    }

    #[test]
    fn output_is_marked_private_only() {
        // Section is wired into the private report bundle. This test asserts the
        // module-level marker is set and the rendered output does not look like
        // a public-facing newsletter section that should land on the public site.
        assert_eq!(SECTION_PRIVACY, "private");

        let rendered = render_private_lessons_applied(&fixture_with_two_lessons()).unwrap();
        // Headers that public sections use should not appear.
        assert!(!rendered.contains("## Executive Summary"));
        assert!(!rendered.contains("## Methodology"));
        assert!(!rendered.contains("## How We Analyse"));
        assert!(!rendered.contains("PFTUI Intelligence Report"));
        assert!(!rendered.contains("for informational purposes only"));
        // The header is the private-style "Lessons Applied This Run".
        assert!(rendered.contains("## Lessons Applied This Run"));
    }

    fn fixture_with_two_lessons() -> BuildContext {
        BuildContext {
            private_lessons_applied: Some(PrivateLessonsAppliedSummary {
                since: "24h".to_string(),
                total_predictions: 3,
                guarded_predictions: 2,
                unique_lessons: 2,
                lesson_references: vec![
                    lesson_ref(218, 2, "timing", "Overweighted headline momentum"),
                    lesson_ref(240, 1, "signal", "Ignored liquidity regime"),
                ],
                strongest_analog: Some(PrivateHistoricalAnalogRow {
                    prediction_id: 4,
                    claim: "BTC breaks 100k by month end on ETF flows".to_string(),
                    overlap_count: 2,
                    overlapping_lesson_ids: vec![218, 240],
                    outcome: "wrong".to_string(),
                }),
            }),
            ..BuildContext::default()
        }
    }

    fn lesson_ref(
        id: i64,
        references: u32,
        miss_type: &str,
        summary: &str,
    ) -> PrivateLessonReferenceRow {
        PrivateLessonReferenceRow {
            lesson_id: id,
            references,
            miss_type: Some(miss_type.to_string()),
            summary: summary.to_string(),
        }
    }
}
