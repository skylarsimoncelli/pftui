use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{bail, Result};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::prediction_lessons::{self, PredictionLesson};
use crate::db::user_predictions::{self, UserPrediction};

#[derive(Debug, Serialize)]
struct LessonsAppliedReport {
    since: String,
    cutoff_utc: String,
    total_predictions: usize,
    predictions_with_lessons: usize,
    unique_lessons: usize,
    lesson_references: Vec<LessonReference>,
    strongest_historical_analog: Option<HistoricalAnalog>,
    predictions: Vec<AppliedPrediction>,
}

#[derive(Debug, Clone, Serialize)]
struct LessonReference {
    lesson_id: i64,
    references: usize,
    missing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    miss_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    what_predicted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    why_wrong: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signal_misread: Option<String>,
    summary: String,
}

#[derive(Debug, Serialize)]
struct HistoricalAnalog {
    prediction_id: i64,
    claim: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeframe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_agent: Option<String>,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    score_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scored_at: Option<String>,
    overlap_count: usize,
    overlapping_lesson_ids: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lesson: Option<LessonReference>,
}

#[derive(Debug, Serialize)]
struct AppliedPrediction {
    id: i64,
    claim: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    conviction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeframe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_date: Option<String>,
    lessons_applied: Vec<i64>,
    created_at: String,
}

pub fn run(backend: &BackendConnection, since: &str, json_output: bool) -> Result<()> {
    let cutoff = cutoff_from_since(since)?;
    let predictions = user_predictions::list_predictions_backend(backend, None, None, None, None)?;
    let lessons = prediction_lessons::list_lessons_backend(backend, None, None)?;
    let report = build_report(&predictions, &lessons, since, cutoff);

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text(&report);
    }
    Ok(())
}

fn build_report(
    predictions: &[UserPrediction],
    lessons: &[PredictionLesson],
    since: &str,
    cutoff: DateTime<Utc>,
) -> LessonsAppliedReport {
    let lesson_by_id: HashMap<i64, &PredictionLesson> =
        lessons.iter().map(|lesson| (lesson.id, lesson)).collect();
    let lesson_by_prediction: HashMap<i64, &PredictionLesson> = lessons
        .iter()
        .map(|lesson| (lesson.prediction_id, lesson))
        .collect();

    let window_predictions: Vec<&UserPrediction> = predictions
        .iter()
        .filter(|prediction| is_since(prediction, cutoff))
        .collect();

    let mut lesson_counts: BTreeMap<i64, usize> = BTreeMap::new();
    let mut applied_predictions = Vec::new();
    for prediction in &window_predictions {
        if prediction.lessons_applied.is_empty() {
            continue;
        }
        let unique_ids: BTreeSet<i64> = prediction.lessons_applied.iter().copied().collect();
        for lesson_id in unique_ids {
            *lesson_counts.entry(lesson_id).or_default() += 1;
        }
        applied_predictions.push(AppliedPrediction {
            id: prediction.id,
            claim: prediction.claim.clone(),
            symbol: prediction.symbol.clone(),
            conviction: prediction.conviction.clone(),
            timeframe: prediction.timeframe.clone(),
            confidence: prediction.confidence,
            source_agent: prediction.source_agent.clone(),
            target_date: prediction.target_date.clone(),
            lessons_applied: prediction.lessons_applied.clone(),
            created_at: prediction.created_at.clone(),
        });
    }

    let mut lesson_references: Vec<LessonReference> = lesson_counts
        .iter()
        .map(|(lesson_id, references)| {
            lesson_reference(
                *lesson_id,
                *references,
                lesson_by_id.get(lesson_id).copied(),
            )
        })
        .collect();
    lesson_references.sort_by(|a, b| {
        b.references
            .cmp(&a.references)
            .then_with(|| a.lesson_id.cmp(&b.lesson_id))
    });

    let applied_lesson_ids: BTreeSet<i64> = lesson_counts.keys().copied().collect();
    let strongest_historical_analog = strongest_historical_analog(
        predictions,
        &lesson_by_prediction,
        &lesson_by_id,
        &applied_lesson_ids,
        cutoff,
    );

    LessonsAppliedReport {
        since: since.to_string(),
        cutoff_utc: cutoff.to_rfc3339(),
        total_predictions: window_predictions.len(),
        predictions_with_lessons: applied_predictions.len(),
        unique_lessons: lesson_references.len(),
        lesson_references,
        strongest_historical_analog,
        predictions: applied_predictions,
    }
}

fn lesson_reference(
    lesson_id: i64,
    references: usize,
    lesson: Option<&PredictionLesson>,
) -> LessonReference {
    let Some(lesson) = lesson else {
        return LessonReference {
            lesson_id,
            references,
            missing: true,
            miss_type: None,
            what_predicted: None,
            why_wrong: None,
            signal_misread: None,
            summary: "Lesson metadata not found".to_string(),
        };
    };

    let summary = format!(
        "{}: {}",
        lesson.miss_type,
        truncate(&lesson.why_wrong.replace('\n', " "), 160)
    );
    LessonReference {
        lesson_id,
        references,
        missing: false,
        miss_type: Some(lesson.miss_type.clone()),
        what_predicted: Some(lesson.what_predicted.clone()),
        why_wrong: Some(lesson.why_wrong.clone()),
        signal_misread: lesson.signal_misread.clone(),
        summary,
    }
}

fn strongest_historical_analog(
    predictions: &[UserPrediction],
    lesson_by_prediction: &HashMap<i64, &PredictionLesson>,
    lesson_by_id: &HashMap<i64, &PredictionLesson>,
    applied_lesson_ids: &BTreeSet<i64>,
    cutoff: DateTime<Utc>,
) -> Option<HistoricalAnalog> {
    if applied_lesson_ids.is_empty() {
        return None;
    }

    let mut best: Option<(usize, DateTime<Utc>, &UserPrediction, Vec<i64>)> = None;
    for prediction in predictions {
        if prediction.outcome != "wrong" || is_since(prediction, cutoff) {
            continue;
        }

        let mut candidate_ids: BTreeSet<i64> = prediction.lessons_applied.iter().copied().collect();
        if let Some(lesson) = lesson_by_prediction.get(&prediction.id) {
            candidate_ids.insert(lesson.id);
        }
        let overlaps: Vec<i64> = candidate_ids
            .intersection(applied_lesson_ids)
            .copied()
            .collect();
        if overlaps.is_empty() {
            continue;
        }

        let timestamp = prediction
            .scored_at
            .as_deref()
            .and_then(timestamp_to_utc)
            .or_else(|| timestamp_to_utc(&prediction.created_at))
            .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        let replace = best
            .as_ref()
            .map(|(best_count, best_ts, _, _)| {
                overlaps.len() > *best_count
                    || (overlaps.len() == *best_count && timestamp > *best_ts)
            })
            .unwrap_or(true);
        if replace {
            best = Some((overlaps.len(), timestamp, prediction, overlaps));
        }
    }

    best.map(|(overlap_count, _, prediction, overlapping_lesson_ids)| {
        let lesson = lesson_by_prediction.get(&prediction.id).map(|lesson| {
            lesson_reference(
                lesson.id,
                1,
                lesson_by_id.get(&lesson.id).copied().or(Some(*lesson)),
            )
        });
        HistoricalAnalog {
            prediction_id: prediction.id,
            claim: prediction.claim.clone(),
            symbol: prediction.symbol.clone(),
            timeframe: prediction.timeframe.clone(),
            source_agent: prediction.source_agent.clone(),
            outcome: prediction.outcome.clone(),
            score_notes: prediction.score_notes.clone(),
            scored_at: prediction.scored_at.clone(),
            overlap_count,
            overlapping_lesson_ids,
            lesson,
        }
    })
}

fn print_text(report: &LessonsAppliedReport) {
    println!("Lessons Applied");
    println!("----------------------------------------------------------------");
    println!(
        "{} predictions since {} | {} with lessons | {} unique lessons",
        report.total_predictions,
        report.since,
        report.predictions_with_lessons,
        report.unique_lessons
    );

    if report.lesson_references.is_empty() {
        println!("No predictions in this window recorded applied lesson IDs.");
        return;
    }

    println!();
    println!("Most referenced lessons:");
    for lesson in &report.lesson_references {
        println!(
            "  #{} x{}  {}",
            lesson.lesson_id, lesson.references, lesson.summary
        );
    }

    if let Some(analog) = &report.strongest_historical_analog {
        println!();
        println!(
            "Strongest historical analog: prediction #{} ({} overlapping lessons: {:?})",
            analog.prediction_id, analog.overlap_count, analog.overlapping_lesson_ids
        );
        println!("  {}", truncate(&analog.claim, 120));
    }
}

fn cutoff_from_since(since: &str) -> Result<DateTime<Utc>> {
    let value = since.trim().to_ascii_lowercase();
    if let Some(hours) = value.strip_suffix('h') {
        let hours: i64 = hours.parse().map_err(|_| {
            anyhow::anyhow!(
                "invalid --since '{}'. Use 24h, 7d, today, yesterday, or YYYY-MM-DD",
                since
            )
        })?;
        if hours < 0 {
            bail!("invalid --since '{}'. Duration must be positive", since);
        }
        return Ok(Utc::now() - Duration::hours(hours));
    }
    if let Some(days) = value.strip_suffix('d') {
        let days: i64 = days.parse().map_err(|_| {
            anyhow::anyhow!(
                "invalid --since '{}'. Use 24h, 7d, today, yesterday, or YYYY-MM-DD",
                since
            )
        })?;
        if days < 0 {
            bail!("invalid --since '{}'. Duration must be positive", since);
        }
        return Ok(Utc::now() - Duration::days(days));
    }
    if value == "today" {
        return Ok(midnight_utc(Utc::now().date_naive()));
    }
    if value == "yesterday" {
        return Ok(midnight_utc(Utc::now().date_naive() - Duration::days(1)));
    }
    let date = NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(|_| {
        anyhow::anyhow!(
            "invalid --since '{}'. Use 24h, 7d, today, yesterday, or YYYY-MM-DD",
            since
        )
    })?;
    Ok(midnight_utc(date))
}

fn midnight_utc(date: NaiveDate) -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(date.and_hms_opt(0, 0, 0).unwrap(), Utc)
}

fn is_since(prediction: &UserPrediction, cutoff: DateTime<Utc>) -> bool {
    timestamp_to_utc(&prediction.created_at).is_some_and(|created_at| created_at >= cutoff)
}

fn timestamp_to_utc(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%#z") {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z") {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Some(midnight_utc(date));
    }
    None
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prediction(
        id: i64,
        claim: &str,
        created_at: &str,
        outcome: &str,
        lessons: Vec<i64>,
    ) -> UserPrediction {
        UserPrediction {
            id,
            claim: claim.to_string(),
            symbol: Some("BTC-USD".to_string()),
            conviction: "high".to_string(),
            timeframe: Some("low".to_string()),
            topic: "other".to_string(),
            confidence: Some(0.7),
            source_agent: Some("low-agent".to_string()),
            source_article_id: None,
            target_date: Some("2026-05-29".to_string()),
            resolution_criteria: None,
            outcome: outcome.to_string(),
            score_notes: None,
            lesson: None,
            lessons_applied: lessons,
            created_at: created_at.to_string(),
            scored_at: if outcome == "wrong" {
                Some(created_at.to_string())
            } else {
                None
            },
        }
    }

    fn lesson(id: i64, prediction_id: i64, why_wrong: &str) -> PredictionLesson {
        PredictionLesson {
            id,
            prediction_id,
            miss_type: "timing".to_string(),
            what_predicted: format!("prediction {}", prediction_id),
            what_happened: "market moved later".to_string(),
            why_wrong: why_wrong.to_string(),
            signal_misread: Some("volume did not confirm".to_string()),
            created_at: "2026-05-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn aggregate_counts_applied_lessons_and_selects_analog() {
        let cutoff = timestamp_to_utc("2026-05-28T00:00:00Z").unwrap();
        let predictions = vec![
            prediction(
                1,
                "Current guarded by two lessons",
                "2026-05-28T08:00:00Z",
                "pending",
                vec![218, 240],
            ),
            prediction(
                2,
                "Current guarded by one lesson",
                "2026-05-28T09:00:00Z",
                "pending",
                vec![218],
            ),
            prediction(
                3,
                "Current unguarded",
                "2026-05-28T10:00:00Z",
                "pending",
                vec![],
            ),
            prediction(
                4,
                "Old wrong analog",
                "2026-05-01T10:00:00Z",
                "wrong",
                vec![218, 240],
            ),
            prediction(
                5,
                "Old weak analog",
                "2026-05-02T10:00:00Z",
                "wrong",
                vec![218],
            ),
        ];
        let lessons = vec![
            lesson(218, 4, "Overweighted headline momentum"),
            lesson(240, 6, "Ignored liquidity regime"),
        ];

        let report = build_report(&predictions, &lessons, "24h", cutoff);

        assert_eq!(report.total_predictions, 3);
        assert_eq!(report.predictions_with_lessons, 2);
        assert_eq!(report.unique_lessons, 2);
        assert_eq!(report.lesson_references[0].lesson_id, 218);
        assert_eq!(report.lesson_references[0].references, 2);
        let analog = report.strongest_historical_analog.unwrap();
        assert_eq!(analog.prediction_id, 4);
        assert_eq!(analog.overlap_count, 2);
        assert_eq!(analog.overlapping_lesson_ids, vec![218, 240]);
    }

    #[test]
    fn cutoff_parser_accepts_hours_days_and_dates() {
        assert!(cutoff_from_since("24h").is_ok());
        assert!(cutoff_from_since("7d").is_ok());
        assert!(cutoff_from_since("today").is_ok());
        assert!(cutoff_from_since("2026-05-28").is_ok());
        assert!(cutoff_from_since("soon").is_err());
    }
}
