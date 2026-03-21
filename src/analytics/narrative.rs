use anyhow::Result;
use chrono::{Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::analytics::deltas::{self, DeltaWindow};
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::calendar_cache::CalendarEvent;
use crate::models::asset_names::resolve_name;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeReport {
    pub generated_at: String,
    pub requested_date: String,
    pub source_date: String,
    pub headline: String,
    pub subtitle: String,
    pub coverage_note: Option<String>,
    pub recap: RecapReport,
    pub scenario_shifts: Vec<ScenarioShift>,
    pub conviction_changes: Vec<ConvictionShift>,
    pub trend_changes: Vec<TrendShift>,
    pub prediction_scorecard: PredictionScorecardSummary,
    pub surprises: Vec<NarrativeInsight>,
    pub lessons: Vec<LessonEntry>,
    pub catalyst_outcomes: Vec<CatalystOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecapReport {
    pub date: String,
    pub note: Option<String>,
    pub events: Vec<RecapEvent>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecapEvent {
    pub at: String,
    pub event_type: String,
    pub source: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioShift {
    pub name: String,
    pub previous_probability: f64,
    pub current_probability: f64,
    pub delta_pct: f64,
    pub driver: Option<String>,
    pub updated_at: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionShift {
    pub symbol: String,
    pub name: String,
    pub old_score: i32,
    pub new_score: i32,
    pub delta: i32,
    pub updated_at: String,
    pub notes: Option<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendShift {
    pub name: String,
    pub timeframe: String,
    pub direction: String,
    pub conviction: String,
    pub updated_at: String,
    pub latest_evidence: Option<String>,
    pub affected_assets: Vec<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionScorecardSummary {
    pub total: usize,
    pub scored: usize,
    pub pending: usize,
    pub correct: usize,
    pub partial: usize,
    pub wrong: usize,
    pub hit_rate_pct: f64,
    pub recent_resolutions: Vec<PredictionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResolution {
    pub id: i64,
    pub claim: String,
    pub symbol: Option<String>,
    pub outcome: String,
    pub lesson: Option<String>,
    pub scored_at: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeInsight {
    pub title: String,
    pub detail: String,
    pub value: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonEntry {
    pub title: String,
    pub detail: String,
    pub symbol: Option<String>,
    pub recorded_at: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalystOutcome {
    pub title: String,
    pub date: String,
    pub category: String,
    pub linked_assets: Vec<String>,
    pub outcome: String,
    pub detail: String,
    pub severity: String,
}

pub fn build_report_backend(
    backend: &BackendConnection,
    persist_current: bool,
) -> Result<NarrativeReport> {
    let today = Utc::now().date_naive();
    let recap = build_recap_backend(backend, Some(today), Some(14), true)?;
    let scenario_shifts = scenario_shifts_backend(backend, 7);
    let conviction_changes = conviction_changes_backend(backend, 7);
    let trend_changes = trend_changes_backend(backend, 7);
    let prediction_scorecard = prediction_scorecard_backend(backend, 7);
    let surprises = surprise_items_backend(backend);
    let lessons = lesson_items_backend(backend, 30);
    let catalyst_outcomes = catalyst_outcomes_backend(backend, 7);

    let requested_date = today.to_string();
    let headline = if surprises
        .first()
        .is_some_and(|item| item.title == "No major deltas")
        && scenario_shifts.is_empty()
        && conviction_changes.is_empty()
        && trend_changes.is_empty()
    {
        "Narrative state stable".to_string()
    } else {
        surprises
            .first()
            .map(|item| item.title.clone())
            .or_else(|| {
                scenario_shifts
                    .first()
                    .map(|item| format!("{} repriced", item.name))
            })
            .or_else(|| {
                conviction_changes
                    .first()
                    .map(|item| format!("{} conviction moved", item.symbol))
            })
            .unwrap_or_else(|| "Narrative state stable".to_string())
    };
    let subtitle = format!(
        "{} recap events • {} surprises • {:.0}% hit rate",
        recap.count,
        surprises.len(),
        prediction_scorecard.hit_rate_pct
    );
    let coverage_note = recap.note.clone();

    let report = NarrativeReport {
        generated_at: Utc::now().to_rfc3339(),
        requested_date,
        source_date: recap.date.clone(),
        headline,
        subtitle,
        coverage_note,
        recap,
        scenario_shifts,
        conviction_changes,
        trend_changes,
        prediction_scorecard,
        surprises,
        lessons,
        catalyst_outcomes,
    };

    if persist_current {
        let encoded = serde_json::to_string(&report)?;
        db::narrative_snapshots::insert_snapshot_backend(backend, &report.generated_at, &encoded)?;
    }

    Ok(report)
}

pub fn build_recap_backend(
    backend: &BackendConnection,
    requested_day: Option<NaiveDate>,
    limit: Option<usize>,
    fallback_today: bool,
) -> Result<RecapReport> {
    let requested = requested_day.unwrap_or_else(|| Utc::now().date_naive());
    let mut events = collect_recap_events_backend(backend, Some(requested));
    let mut source_date = requested;
    let mut note = None;

    if events.is_empty() && fallback_today && requested == Utc::now().date_naive() {
        let fallback_day = requested - Duration::days(1);
        let fallback_events = collect_recap_events_backend(backend, Some(fallback_day));
        if fallback_events.is_empty() {
            note = Some("No events recorded yet today.".to_string());
        } else {
            source_date = fallback_day;
            events = fallback_events;
            note = Some("No events recorded yet today; showing yesterday's recap.".to_string());
        }
    }

    events.sort_by(|left, right| right.at.cmp(&left.at));
    if let Some(max) = limit {
        events.truncate(max);
    }

    Ok(RecapReport {
        date: source_date.to_string(),
        note,
        count: events.len(),
        events,
    })
}

pub fn collect_recap_events_backend(
    backend: &BackendConnection,
    day: Option<NaiveDate>,
) -> Vec<RecapEvent> {
    let mut events = Vec::new();

    let predictions =
        db::user_predictions::list_predictions_backend(backend, None, None, None, None)
            .unwrap_or_default();
    for prediction in predictions {
        if ts_matches_day(&prediction.created_at, day) {
            events.push(RecapEvent {
                at: prediction.created_at.clone(),
                event_type: "prediction_added".to_string(),
                source: prediction
                    .source_agent
                    .clone()
                    .unwrap_or_else(|| "predict".to_string()),
                summary: format!("#{} {}", prediction.id, prediction.claim),
            });
        }
        if let Some(scored_at) = prediction.scored_at.as_ref() {
            if ts_matches_day(scored_at, day) {
                events.push(RecapEvent {
                    at: scored_at.clone(),
                    event_type: "prediction_scored".to_string(),
                    source: prediction
                        .source_agent
                        .clone()
                        .unwrap_or_else(|| "predict".to_string()),
                    summary: format!("#{} -> {}", prediction.id, prediction.outcome),
                });
            }
        }
    }

    let scenario_rows = db::scenarios::list_scenarios_backend(backend, None).unwrap_or_default();
    for scenario in scenario_rows {
        if ts_matches_day(&scenario.updated_at, day) {
            events.push(RecapEvent {
                at: scenario.updated_at.clone(),
                event_type: "scenario_updated".to_string(),
                source: "scenario".to_string(),
                summary: format!(
                    "{} {:.1}% ({})",
                    scenario.name, scenario.probability, scenario.status
                ),
            });
        }
    }

    let conviction_rows = db::convictions::list_current_backend(backend).unwrap_or_default();
    for conviction in conviction_rows {
        if ts_matches_day(&conviction.recorded_at, day) {
            events.push(RecapEvent {
                at: conviction.recorded_at.clone(),
                event_type: "conviction_set".to_string(),
                source: "conviction".to_string(),
                summary: format!("{} -> {}", conviction.symbol, conviction.score),
            });
        }
    }

    let signal_rows =
        db::timeframe_signals::list_signals_backend(backend, None, None, None).unwrap_or_default();
    for signal in signal_rows {
        if ts_matches_day(&signal.detected_at, day) {
            events.push(RecapEvent {
                at: signal.detected_at.clone(),
                event_type: "timeframe_signal".to_string(),
                source: "analytics".to_string(),
                summary: format!("[{}] {}", signal.severity, signal.description),
            });
        }
    }

    let regime_rows =
        db::regime_snapshots::get_history_backend(backend, Some(50)).unwrap_or_default();
    for regime in regime_rows {
        if ts_matches_day(&regime.recorded_at, day) {
            events.push(RecapEvent {
                at: regime.recorded_at.clone(),
                event_type: "regime_snapshot".to_string(),
                source: "regime".to_string(),
                summary: format!(
                    "{} ({:.2})",
                    regime.regime,
                    regime.confidence.unwrap_or(0.0)
                ),
            });
        }
    }

    let message_rows = db::agent_messages::list_messages_backend(
        backend, None, None, None, false, None, None, None,
    )
    .unwrap_or_default();
    for message in message_rows {
        if ts_matches_day(&message.created_at, day) {
            events.push(RecapEvent {
                at: message.created_at.clone(),
                event_type: "agent_message".to_string(),
                source: message.from_agent.clone(),
                summary: message.content.chars().take(120).collect(),
            });
        }
    }

    events
}

fn scenario_shifts_backend(backend: &BackendConnection, days: i64) -> Vec<ScenarioShift> {
    let cutoff = Utc::now().date_naive() - Duration::days(days);
    let mut shifts = Vec::new();

    for scenario in
        db::scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default()
    {
        if !date_on_or_after(&scenario.updated_at, cutoff) {
            continue;
        }
        let history =
            db::scenarios::get_history_backend(backend, scenario.id, Some(2)).unwrap_or_default();
        if history.len() < 2 {
            continue;
        }
        let previous_probability = history[1].probability;
        let driver = history[0].driver.clone();
        let delta = scenario.probability - previous_probability;
        if delta.abs() < 0.1 {
            continue;
        }
        shifts.push(ScenarioShift {
            name: scenario.name.clone(),
            previous_probability,
            current_probability: scenario.probability,
            delta_pct: delta,
            driver,
            updated_at: scenario.updated_at.clone(),
            severity: probability_severity(delta),
        });
    }

    shifts.sort_by(|left, right| {
        right
            .delta_pct
            .abs()
            .partial_cmp(&left.delta_pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    shifts.truncate(6);
    shifts
}

fn conviction_changes_backend(backend: &BackendConnection, days: usize) -> Vec<ConvictionShift> {
    let current = db::convictions::list_current_backend(backend).unwrap_or_default();
    let current_map = current
        .into_iter()
        .map(|item| (item.symbol.to_uppercase(), item))
        .collect::<std::collections::HashMap<_, _>>();
    let mut changes = db::convictions::get_changes_backend(backend, days)
        .unwrap_or_default()
        .into_iter()
        .map(|change| {
            let current = current_map.get(&change.symbol.to_uppercase());
            ConvictionShift {
                symbol: change.symbol.clone(),
                name: resolve_name(&change.symbol),
                old_score: change.old_score,
                new_score: change.new_score,
                delta: change.change_delta,
                updated_at: change.new_date.clone(),
                notes: current.and_then(|entry| entry.notes.clone()),
                severity: conviction_severity(change.change_delta),
            }
        })
        .collect::<Vec<_>>();
    changes.sort_by(|left, right| {
        right
            .delta
            .abs()
            .cmp(&left.delta.abs())
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    changes.truncate(8);
    changes
}

fn trend_changes_backend(backend: &BackendConnection, days: i64) -> Vec<TrendShift> {
    let cutoff = Utc::now().date_naive() - Duration::days(days);
    let mut changes = Vec::new();
    for trend in db::trends::list_trends_backend(backend, Some("active"), None).unwrap_or_default()
    {
        if !date_on_or_after(&trend.updated_at, cutoff) {
            continue;
        }
        let evidence = db::trends::list_evidence_backend(backend, trend.id, Some(1))
            .unwrap_or_default()
            .into_iter()
            .next();
        let impacts = db::trends::list_asset_impacts_backend(backend, trend.id)
            .unwrap_or_default()
            .into_iter()
            .map(|impact| impact.symbol)
            .take(4)
            .collect::<Vec<_>>();
        changes.push(TrendShift {
            name: trend.name.clone(),
            timeframe: trend.timeframe.clone(),
            direction: trend.direction.clone(),
            conviction: trend.conviction.clone(),
            updated_at: trend.updated_at.clone(),
            latest_evidence: evidence.map(|item| item.evidence),
            affected_assets: impacts,
            severity: trend_severity(&trend.conviction),
        });
    }

    changes.sort_by(|left, right| {
        severity_rank(&right.severity)
            .cmp(&severity_rank(&left.severity))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    changes.truncate(6);
    changes
}

fn prediction_scorecard_backend(
    backend: &BackendConnection,
    resolution_days: i64,
) -> PredictionScorecardSummary {
    let stats = db::user_predictions::get_stats_backend(backend).unwrap_or(
        db::user_predictions::PredictionStats {
            total: 0,
            scored: 0,
            pending: 0,
            correct: 0,
            partial: 0,
            wrong: 0,
            hit_rate_pct: 0.0,
            by_conviction: std::collections::HashMap::new(),
            by_symbol: std::collections::HashMap::new(),
            by_timeframe: std::collections::HashMap::new(),
            by_source_agent: std::collections::HashMap::new(),
        },
    );
    let cutoff = Utc::now() - Duration::days(resolution_days);
    let mut resolutions =
        db::user_predictions::list_predictions_backend(backend, None, None, None, None)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|prediction| {
                if !matches!(prediction.outcome.as_str(), "correct" | "partial" | "wrong") {
                    return None;
                }
                let scored_at = prediction.scored_at.as_ref()?;
                let scored_day = parse_day(scored_at)?;
                if scored_day
                    .and_hms_opt(0, 0, 0)
                    .map(|ts| ts < cutoff.naive_utc())
                    .unwrap_or(false)
                {
                    return None;
                }
                Some(PredictionResolution {
                    id: prediction.id,
                    claim: prediction.claim,
                    symbol: prediction.symbol,
                    outcome: prediction.outcome.clone(),
                    lesson: prediction.lesson,
                    scored_at: scored_at.clone(),
                    severity: prediction_outcome_severity(&prediction.outcome),
                })
            })
            .collect::<Vec<_>>();
    resolutions.sort_by(|left, right| right.scored_at.cmp(&left.scored_at));
    resolutions.truncate(6);

    PredictionScorecardSummary {
        total: stats.total,
        scored: stats.scored,
        pending: stats.pending,
        correct: stats.correct,
        partial: stats.partial,
        wrong: stats.wrong,
        hit_rate_pct: stats.hit_rate_pct,
        recent_resolutions: resolutions,
    }
}

fn surprise_items_backend(backend: &BackendConnection) -> Vec<NarrativeInsight> {
    deltas::build_report_backend(backend, DeltaWindow::Hours24, true)
        .map(|report| {
            report
                .change_radar
                .into_iter()
                .map(|item| NarrativeInsight {
                    title: item.title,
                    detail: item.detail,
                    value: item.value,
                    severity: item.severity,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn lesson_items_backend(backend: &BackendConnection, days: i64) -> Vec<LessonEntry> {
    let cutoff = Utc::now() - Duration::days(days);
    let mut lessons =
        db::user_predictions::list_predictions_backend(backend, None, None, None, None)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|prediction| {
                let lesson = prediction.lesson?;
                let scored_at = prediction.scored_at?;
                let parsed = parse_day(&scored_at)?
                    .and_hms_opt(0, 0, 0)
                    .map(|dt| dt < cutoff.naive_utc())
                    .unwrap_or(true);
                if parsed {
                    return None;
                }
                Some(LessonEntry {
                    title: format!("#{} {}", prediction.id, prediction.claim),
                    detail: lesson,
                    symbol: prediction.symbol,
                    recorded_at: scored_at,
                    severity: prediction_outcome_severity(&prediction.outcome),
                })
            })
            .collect::<Vec<_>>();
    lessons.sort_by(|left, right| right.recorded_at.cmp(&left.recorded_at));
    lessons.truncate(6);
    lessons
}

fn catalyst_outcomes_backend(backend: &BackendConnection, days: i64) -> Vec<CatalystOutcome> {
    let from_date = (Utc::now().date_naive() - Duration::days(days)).to_string();
    let today = Utc::now().date_naive().to_string();
    let predictions =
        db::user_predictions::list_predictions_backend(backend, None, None, None, None)
            .unwrap_or_default();
    let mut outcomes = db::calendar_cache::get_upcoming_events_backend(backend, &from_date, 48)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| event.date < today)
        .map(|event| catalyst_outcome_from_event(&event, &predictions))
        .collect::<Vec<_>>();
    outcomes.sort_by(|left, right| right.date.cmp(&left.date));
    outcomes.truncate(6);
    outcomes
}

fn catalyst_outcome_from_event(
    event: &CalendarEvent,
    predictions: &[db::user_predictions::UserPrediction],
) -> CatalystOutcome {
    let matched = predictions
        .iter()
        .filter(|prediction| prediction_matches_event(prediction, event))
        .collect::<Vec<_>>();
    let linked_assets = event.symbol.clone().into_iter().collect::<Vec<_>>();
    let scored = matched
        .iter()
        .filter(|prediction| matches!(prediction.outcome.as_str(), "correct" | "partial" | "wrong"))
        .collect::<Vec<_>>();
    let pending = matched
        .iter()
        .filter(|prediction| {
            !matches!(prediction.outcome.as_str(), "correct" | "partial" | "wrong")
        })
        .count();

    let (outcome, severity) = if scored
        .iter()
        .any(|prediction| prediction.outcome == "wrong")
    {
        ("missed".to_string(), "elevated".to_string())
    } else if scored
        .iter()
        .any(|prediction| prediction.outcome == "partial")
    {
        ("mixed".to_string(), "elevated".to_string())
    } else if scored
        .iter()
        .any(|prediction| prediction.outcome == "correct")
    {
        ("validated".to_string(), "normal".to_string())
    } else if pending > 0 {
        ("awaiting-score".to_string(), "watch".to_string())
    } else {
        ("passed".to_string(), "normal".to_string())
    };

    let detail = if !scored.is_empty() {
        format!(
            "{} linked scored prediction(s). Latest outcome: {}.",
            scored.len(),
            scored[0].outcome
        )
    } else if pending > 0 {
        format!(
            "{} linked prediction(s) still waiting for scoring.",
            pending
        )
    } else {
        format!(
            "Previous {} • Forecast {}",
            event.previous.clone().unwrap_or_else(|| "—".to_string()),
            event.forecast.clone().unwrap_or_else(|| "—".to_string())
        )
    };

    CatalystOutcome {
        title: event.name.clone(),
        date: event.date.clone(),
        category: event.event_type.clone(),
        linked_assets,
        outcome,
        detail,
        severity,
    }
}

fn prediction_matches_event(
    prediction: &db::user_predictions::UserPrediction,
    event: &CalendarEvent,
) -> bool {
    if prediction
        .target_date
        .as_deref()
        .is_some_and(|value| value.starts_with(&event.date))
    {
        return true;
    }
    if let Some(symbol) = prediction.symbol.as_deref() {
        if event
            .symbol
            .as_deref()
            .is_some_and(|event_symbol| event_symbol.eq_ignore_ascii_case(symbol))
        {
            return true;
        }
    }
    let claim = prediction.claim.to_ascii_uppercase();
    claim.contains(&event.name.to_ascii_uppercase())
}

fn ts_matches_day(ts: &str, day: Option<NaiveDate>) -> bool {
    let Some(target) = day else {
        return true;
    };
    parse_day(ts)
        .map(|parsed| parsed == target)
        .unwrap_or(false)
}

fn parse_day(raw: &str) -> Option<NaiveDate> {
    if raw.len() < 10 {
        return None;
    }
    NaiveDate::parse_from_str(&raw[..10], "%Y-%m-%d").ok()
}

fn date_on_or_after(raw: &str, cutoff: NaiveDate) -> bool {
    parse_day(raw).map(|day| day >= cutoff).unwrap_or(false)
}

fn probability_severity(delta: f64) -> String {
    if delta.abs() >= 15.0 {
        "critical".to_string()
    } else if delta.abs() >= 7.5 {
        "elevated".to_string()
    } else {
        "normal".to_string()
    }
}

fn conviction_severity(delta: i32) -> String {
    if delta.abs() >= 3 {
        "critical".to_string()
    } else if delta.abs() >= 2 {
        "elevated".to_string()
    } else {
        "normal".to_string()
    }
}

fn trend_severity(conviction: &str) -> String {
    match conviction.to_ascii_lowercase().as_str() {
        "high" => "elevated".to_string(),
        "very high" | "extreme" => "critical".to_string(),
        _ => "normal".to_string(),
    }
}

fn prediction_outcome_severity(outcome: &str) -> String {
    match outcome {
        "wrong" => "critical".to_string(),
        "partial" => "elevated".to_string(),
        _ => "normal".to_string(),
    }
}

fn severity_rank(severity: &str) -> usize {
    match severity {
        "critical" => 3,
        "elevated" => 2,
        "watch" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;

    #[test]
    fn narrative_empty_state_is_stable() {
        let backend = BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };

        let report = build_report_backend(&backend, true).unwrap();

        assert!(!report.headline.is_empty());
        assert_eq!(report.scenario_shifts.len(), 0);
        assert_eq!(report.conviction_changes.len(), 0);
        assert_eq!(report.trend_changes.len(), 0);
        assert_eq!(report.prediction_scorecard.total, 0);
    }

    #[test]
    fn narrative_orders_scenario_and_conviction_shifts_by_materiality() {
        let backend = BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };

        let btc = db::scenarios::add_scenario(
            backend.sqlite(),
            "BTC breakout",
            30.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario_probability(backend.sqlite(), btc, 35.0, Some("warming"))
            .unwrap();
        db::scenarios::update_scenario_probability(backend.sqlite(), btc, 55.0, Some("ETF flows"))
            .unwrap();
        let gold = db::scenarios::add_scenario(
            backend.sqlite(),
            "Gold consolidation",
            60.0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        db::scenarios::update_scenario_probability(backend.sqlite(), gold, 62.0, Some("range"))
            .unwrap();
        db::scenarios::update_scenario_probability(backend.sqlite(), gold, 64.0, Some("range"))
            .unwrap();

        db::convictions::set_conviction_backend(&backend, "BTC", 1, Some("starter")).unwrap();
        db::convictions::set_conviction_backend(&backend, "BTC", 4, Some("repriced")).unwrap();
        db::convictions::set_conviction_backend(&backend, "GLD", 2, Some("hedge")).unwrap();
        db::convictions::set_conviction_backend(&backend, "GLD", 3, Some("slightly higher"))
            .unwrap();

        let report = build_report_backend(&backend, false).unwrap();

        assert_eq!(
            report
                .scenario_shifts
                .first()
                .map(|item| item.name.as_str()),
            Some("BTC breakout")
        );
        assert_eq!(
            report
                .conviction_changes
                .first()
                .map(|item| item.symbol.as_str()),
            Some("BTC")
        );
    }

    #[test]
    fn recap_falls_back_to_yesterday_for_today_when_empty() {
        let backend = BackendConnection::Sqlite {
            conn: crate::db::open_in_memory(),
        };
        let yesterday = (Utc::now().date_naive() - Duration::days(1)).to_string();
        db::convictions::set_conviction_backend(&backend, "ETH", 3, Some("carry")).unwrap();
        backend
            .sqlite()
            .execute(
                "UPDATE convictions SET recorded_at = ?1",
                rusqlite::params![format!("{yesterday} 10:00:00")],
            )
            .unwrap();

        let recap =
            build_recap_backend(&backend, Some(Utc::now().date_naive()), Some(10), true).unwrap();

        assert_eq!(recap.date, yesterday);
        assert!(recap.note.unwrap_or_default().contains("showing yesterday"));
        assert_eq!(recap.count, 1);
    }
}
