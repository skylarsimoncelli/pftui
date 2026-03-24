use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::analytics::situation::{CorrelationState, SituationInputs, SituationInsight};
use crate::db;
use crate::db::backend::BackendConnection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSituationSnapshot {
    pub recorded_at: String,
    pub inputs: SituationInputs,
}

#[derive(Debug, Clone, Serialize)]
pub struct SituationDeltaReport {
    pub window: String,
    pub label: String,
    pub current_at: String,
    pub baseline_at: Option<String>,
    pub coverage: String,
    pub change_radar: Vec<SituationInsight>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaWindow {
    LastRefresh,
    PriorClose,
    Hours24,
    Days7,
}

impl DeltaWindow {
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        match raw
            .unwrap_or("last-refresh")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "last-refresh" | "last_refresh" | "refresh" => Ok(Self::LastRefresh),
            "close" | "prior-close" | "prior_close" => Ok(Self::PriorClose),
            "24h" | "1d" => Ok(Self::Hours24),
            "7d" | "1w" | "week" => Ok(Self::Days7),
            other => Err(anyhow!(
                "invalid delta window '{}'. Use last-refresh, close, 24h, or 7d",
                other
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LastRefresh => "last-refresh",
            Self::PriorClose => "close",
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LastRefresh => "last refresh",
            Self::PriorClose => "prior close",
            Self::Hours24 => "24 hours",
            Self::Days7 => "7 days",
        }
    }
}

pub fn build_report_backend(
    backend: &BackendConnection,
    window: DeltaWindow,
    persist_current: bool,
) -> Result<SituationDeltaReport> {
    let current = StoredSituationSnapshot {
        recorded_at: Utc::now().to_rfc3339(),
        inputs: crate::analytics::situation::collect_inputs_backend(backend)?,
    };
    let baseline = select_baseline_snapshot(backend, window, &current.recorded_at)?;
    let coverage = baseline_coverage(window, &baseline, &current.recorded_at);
    let change_radar = compare_snapshots(&current, baseline.as_ref());

    if persist_current {
        store_snapshot_if_changed(backend, &current)?;
    }

    Ok(SituationDeltaReport {
        window: window.as_str().to_string(),
        label: window.label().to_string(),
        current_at: current.recorded_at.clone(),
        baseline_at: baseline.as_ref().map(|row| row.recorded_at.clone()),
        coverage,
        change_radar,
    })
}

fn select_baseline_snapshot(
    backend: &BackendConnection,
    window: DeltaWindow,
    current_at: &str,
) -> Result<Option<StoredSituationSnapshot>> {
    let now = parse_ts(current_at)?;
    let baseline_row = match window {
        DeltaWindow::LastRefresh => db::situation_snapshots::latest_snapshot_backend(backend)?,
        _ => {
            let cutoff = cutoff_for_window(window, now);
            db::situation_snapshots::latest_snapshot_before_backend(backend, &cutoff.to_rfc3339())?
                .or_else(|| {
                    db::situation_snapshots::latest_snapshot_backend(backend)
                        .ok()
                        .flatten()
                })
        }
    };

    baseline_row
        .map(|row| serde_json::from_str::<StoredSituationSnapshot>(&row.snapshot_json))
        .transpose()
        .map_err(Into::into)
}

fn store_snapshot_if_changed(
    backend: &BackendConnection,
    snapshot: &StoredSituationSnapshot,
) -> Result<()> {
    let encoded = serde_json::to_string(snapshot)?;
    let latest = db::situation_snapshots::latest_snapshot_backend(backend)?;
    if latest
        .as_ref()
        .map(|row| row.snapshot_json == encoded)
        .unwrap_or(false)
    {
        return Ok(());
    }
    db::situation_snapshots::insert_snapshot_backend(backend, &snapshot.recorded_at, &encoded)?;
    Ok(())
}

fn compare_snapshots(
    current: &StoredSituationSnapshot,
    previous: Option<&StoredSituationSnapshot>,
) -> Vec<SituationInsight> {
    let Some(previous) = previous else {
        return vec![SituationInsight {
            title: "Baseline forming".to_string(),
            detail: "The analytics layer has stored the first server-side situation snapshot. The next refresh will surface ranked deltas here.".to_string(),
            value: "Warmup".to_string(),
            severity: "normal".to_string(),
        }];
    };

    let mut items = Vec::new();

    push_timeframe_deltas(&mut items, current, previous);
    push_signal_delta(&mut items, current, previous);
    push_count_delta(
        &mut items,
        current.inputs.triggered_alert_count as i32,
        previous.inputs.triggered_alert_count as i32,
        CountDeltaSpec {
            up_title: "Triggered alerts increased",
            down_title: "Triggered alerts cooled",
            detail_template: "Alert load moved from {from} to {to}.",
            up_severity: "critical",
            down_severity: "normal",
        },
    );
    push_count_delta(
        &mut items,
        current.inputs.stale_sources as i32,
        previous.inputs.stale_sources as i32,
        CountDeltaSpec {
            up_title: "Data trust worsened",
            down_title: "Data freshness improved",
            detail_template: "Stale sources moved from {from} to {to}.",
            up_severity: "elevated",
            down_severity: "normal",
        },
    );
    push_regime_delta(&mut items, current, previous);
    push_sentiment_deltas(&mut items, current, previous);
    push_market_pulse_deltas(&mut items, current, previous);
    push_scenario_deltas(&mut items, current, previous);
    push_conviction_deltas(&mut items, current, previous);
    push_correlation_deltas(&mut items, current, previous);

    if items.is_empty() {
        items.push(SituationInsight {
            title: "No major deltas".to_string(),
            detail: "The latest server-side snapshot did not materially change the system state."
                .to_string(),
            value: "Stable".to_string(),
            severity: "normal".to_string(),
        });
    }

    items.sort_by(|left, right| {
        severity_weight(&right.severity)
            .cmp(&severity_weight(&left.severity))
            .then_with(|| left.title.cmp(&right.title))
    });
    items.truncate(8);
    items
}

fn push_timeframe_deltas(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let current_average = average_score(&current.inputs.timeframes);
    let previous_average = average_score(&previous.inputs.timeframes);
    let average_delta = current_average - previous_average;
    if average_delta.abs() >= 8.0 {
        items.push(SituationInsight {
            title: if average_delta > 0.0 {
                "Risk tone improved".to_string()
            } else {
                "Risk tone weakened".to_string()
            },
            detail: format!(
                "Average timeframe score moved from {:.0} to {:.0}.",
                previous_average, current_average
            ),
            value: signed_f64(average_delta),
            severity: if average_delta.abs() >= 18.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }

    let previous_map = previous
        .inputs
        .timeframes
        .iter()
        .map(|row| (row.timeframe.as_str(), row))
        .collect::<HashMap<_, _>>();
    for timeframe in &current.inputs.timeframes {
        let Some(prior) = previous_map.get(timeframe.timeframe.as_str()) else {
            continue;
        };
        let delta = timeframe.score - prior.score;
        if delta.abs() < 10.0 {
            continue;
        }
        items.push(SituationInsight {
            title: format!("{} repriced", timeframe.label),
            detail: format!(
                "{} score moved from {:.0} to {:.0}.",
                timeframe.label, prior.score, timeframe.score
            ),
            value: signed_f64(delta),
            severity: if delta.abs() >= 20.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }
}

fn push_signal_delta(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let current_signal = current.inputs.latest_timeframe_signal.as_ref();
    let previous_signal = previous.inputs.latest_timeframe_signal.as_ref();
    if current_signal.map(|s| (&s.signal_type, &s.severity, &s.description))
        != previous_signal.map(|s| (&s.signal_type, &s.severity, &s.description))
    {
        if let Some(signal) = current_signal {
            items.push(SituationInsight {
                title: "Lead signal changed".to_string(),
                detail: signal.description.clone(),
                value: signal.signal_type.clone(),
                severity: normalize_signal_severity(&signal.severity).to_string(),
            });
        }
    }
}

fn push_regime_delta(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    if current
        .inputs
        .regime
        .as_ref()
        .map(|row| row.regime.as_str())
        != previous
            .inputs
            .regime
            .as_ref()
            .map(|row| row.regime.as_str())
    {
        if let Some(regime) = &current.inputs.regime {
            items.push(SituationInsight {
                title: "Regime shifted".to_string(),
                detail: "The current regime changed since the baseline snapshot.".to_string(),
                value: regime.regime.replace('_', " "),
                severity: "critical".to_string(),
            });
        }
    }
}

fn push_sentiment_deltas(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let previous_map = previous
        .inputs
        .sentiment
        .iter()
        .map(|row| (row.index_type.to_ascii_lowercase(), row))
        .collect::<HashMap<_, _>>();
    for sentiment in &current.inputs.sentiment {
        let key = sentiment.index_type.to_ascii_lowercase();
        let Some(prior) = previous_map.get(&key) else {
            continue;
        };
        let delta = sentiment.value as i32 - prior.value as i32;
        if delta.abs() < 10 && sentiment.classification == prior.classification {
            continue;
        }
        items.push(SituationInsight {
            title: format!("{} sentiment shifted", title_case(&sentiment.index_type)),
            detail: format!(
                "{} -> {}",
                title_case(&prior.classification),
                title_case(&sentiment.classification)
            ),
            value: signed_i32(delta),
            severity: if delta.abs() >= 20 || sentiment.value <= 20 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }
}

fn push_market_pulse_deltas(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let previous_map = previous
        .inputs
        .market_pulse
        .iter()
        .map(|row| (row.symbol.as_str(), row))
        .collect::<HashMap<_, _>>();
    for item in &current.inputs.market_pulse {
        let Some(prior) = previous_map.get(item.symbol.as_str()) else {
            continue;
        };
        let delta = decimal_change(item.day_change_pct) - decimal_change(prior.day_change_pct);
        if delta.abs() < 1.5 {
            continue;
        }
        items.push(SituationInsight {
            title: format!("{} momentum re-priced", item.symbol),
            detail: item.name.clone(),
            value: signed_f64(delta),
            severity: if delta.abs() >= 3.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }
}

fn push_scenario_deltas(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let previous_map = previous
        .inputs
        .scenarios
        .iter()
        .map(|row| (row.name.as_str(), row.probability))
        .collect::<HashMap<_, _>>();
    for scenario in &current.inputs.scenarios {
        let prior = previous_map
            .get(scenario.name.as_str())
            .copied()
            .unwrap_or(0.0);
        let delta = scenario.probability - prior;
        if delta.abs() < 5.0 {
            continue;
        }
        items.push(SituationInsight {
            title: format!("Scenario re-ranked: {}", scenario.name),
            detail: format!(
                "Probability moved from {:.0}% to {:.0}%.",
                prior, scenario.probability
            ),
            value: signed_f64(delta),
            severity: if delta.abs() >= 15.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }
}

fn push_conviction_deltas(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let previous_map = previous
        .inputs
        .convictions
        .iter()
        .map(|row| (row.symbol.as_str(), row.score))
        .collect::<HashMap<_, _>>();
    for conviction in &current.inputs.convictions {
        let prior = previous_map
            .get(conviction.symbol.as_str())
            .copied()
            .unwrap_or(0);
        let delta = conviction.score - prior;
        if delta == 0 {
            continue;
        }
        items.push(SituationInsight {
            title: format!("Conviction changed: {}", conviction.symbol),
            detail: format!("Score moved from {prior} to {}.", conviction.score),
            value: signed_i32(delta),
            severity: if delta.abs() >= 3 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }
}

fn push_correlation_deltas(
    items: &mut Vec<SituationInsight>,
    current: &StoredSituationSnapshot,
    previous: &StoredSituationSnapshot,
) {
    let previous_map = previous
        .inputs
        .correlations
        .iter()
        .map(correlation_key)
        .collect::<HashMap<_, _>>();
    for correlation in &current.inputs.correlations {
        let key = format!(
            "{}:{}:{}",
            correlation.symbol_a, correlation.symbol_b, correlation.period
        );
        let prior = previous_map.get(&key).copied().unwrap_or(0.0);
        let delta = correlation.correlation - prior;
        if delta.abs() < 0.15 {
            continue;
        }
        items.push(SituationInsight {
            title: format!(
                "Correlation shifted: {} / {}",
                correlation.symbol_a, correlation.symbol_b
            ),
            detail: format!(
                "{} moved from {:.2} to {:.2}.",
                correlation.period, prior, correlation.correlation
            ),
            value: signed_f64(delta),
            severity: if delta.abs() >= 0.30 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }
}

struct CountDeltaSpec<'a> {
    up_title: &'a str,
    down_title: &'a str,
    detail_template: &'a str,
    up_severity: &'a str,
    down_severity: &'a str,
}

fn push_count_delta(
    items: &mut Vec<SituationInsight>,
    current: i32,
    previous: i32,
    spec: CountDeltaSpec<'_>,
) {
    let delta = current - previous;
    if delta == 0 {
        return;
    }
    items.push(SituationInsight {
        title: if delta > 0 {
            spec.up_title
        } else {
            spec.down_title
        }
        .to_string(),
        detail: spec
            .detail_template
            .replace("{from}", &previous.to_string())
            .replace("{to}", &current.to_string()),
        value: signed_i32(delta),
        severity: if delta > 0 {
            spec.up_severity
        } else {
            spec.down_severity
        }
        .to_string(),
    });
}

fn cutoff_for_window(window: DeltaWindow, now: DateTime<Utc>) -> DateTime<Utc> {
    match window {
        DeltaWindow::LastRefresh => now,
        DeltaWindow::PriorClose => prior_market_close(now),
        DeltaWindow::Hours24 => now - Duration::hours(24),
        DeltaWindow::Days7 => now - Duration::days(7),
    }
}

fn prior_market_close(now: DateTime<Utc>) -> DateTime<Utc> {
    let mut cursor = now.date_naive();
    loop {
        let weekday = cursor.weekday();
        if weekday != Weekday::Sat && weekday != Weekday::Sun {
            let close = eastern_market_close_utc(cursor);
            if close < now {
                return close;
            }
        }
        cursor -= Duration::days(1);
    }
}

fn eastern_market_close_utc(date: NaiveDate) -> DateTime<Utc> {
    let offset_hours = if is_us_eastern_dst_date(date) { 4 } else { 5 };
    let naive = date
        .and_hms_opt(16 + offset_hours, 0, 0)
        .unwrap_or_else(|| date.and_hms_opt(21, 0, 0).unwrap());
    DateTime::from_naive_utc_and_offset(naive, Utc)
}

fn is_us_eastern_dst_date(date: NaiveDate) -> bool {
    let year = date.year();
    let march_start = match NaiveDate::from_ymd_opt(year, 3, 1) {
        Some(d) => d,
        None => return false,
    };
    let march_first_wd = march_start.weekday().num_days_from_sunday();
    let first_sunday_day = if march_first_wd == 0 {
        1
    } else {
        8 - march_first_wd
    };
    let second_sunday_day = first_sunday_day + 7;
    let dst_start = NaiveDate::from_ymd_opt(year, 3, second_sunday_day).unwrap_or(march_start);

    let november_start = match NaiveDate::from_ymd_opt(year, 11, 1) {
        Some(d) => d,
        None => return false,
    };
    let november_first_wd = november_start.weekday().num_days_from_sunday();
    let november_first_sunday = if november_first_wd == 0 {
        1
    } else {
        8 - november_first_wd
    };
    let dst_end =
        NaiveDate::from_ymd_opt(year, 11, november_first_sunday).unwrap_or(november_start);

    date >= dst_start && date < dst_end
}

fn baseline_coverage(
    window: DeltaWindow,
    baseline: &Option<StoredSituationSnapshot>,
    current_at: &str,
) -> String {
    let Some(baseline) = baseline else {
        return "warming".to_string();
    };
    if window == DeltaWindow::LastRefresh {
        return "exact".to_string();
    }
    let Ok(current_ts) = parse_ts(current_at) else {
        return "partial".to_string();
    };
    let Ok(previous_ts) = parse_ts(&baseline.recorded_at) else {
        return "partial".to_string();
    };
    let delta = current_ts.signed_duration_since(previous_ts);
    match window {
        DeltaWindow::PriorClose if delta.num_hours() >= 12 => "exact".to_string(),
        DeltaWindow::Hours24 if delta.num_hours() >= 24 => "exact".to_string(),
        DeltaWindow::Days7 if delta.num_days() >= 7 => "exact".to_string(),
        _ => "partial".to_string(),
    }
}

fn parse_ts(raw: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(Into::into)
}

fn severity_weight(value: &str) -> i32 {
    match value {
        "critical" => 3,
        "elevated" | "warning" => 2,
        _ => 1,
    }
}

fn average_score(rows: &[crate::analytics::situation::TimeframeScore]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    rows.iter().map(|row| row.score).sum::<f64>() / rows.len() as f64
}

fn signed_i32(value: i32) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn signed_f64(value: f64) -> String {
    if value > 0.0 {
        format!("+{value:.1}")
    } else {
        format!("{value:.1}")
    }
}

fn title_case(raw: &str) -> String {
    raw.replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn decimal_change(value: Option<rust_decimal::Decimal>) -> f64 {
    value
        .and_then(|number| number.to_string().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn normalize_signal_severity(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "warning" | "notable" | "elevated" => "elevated",
        _ => "normal",
    }
}

fn correlation_key(row: &CorrelationState) -> (String, f64) {
    (
        format!("{}:{}:{}", row.symbol_a, row.symbol_b, row.period),
        row.correlation,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::situation::{
        ConvictionState, LatestSignal, MarketPulseItem, RegimeContext, ScenarioState,
        SentimentGauge, SituationInputs, TimeframeScore,
    };
    use rust_decimal_macros::dec;

    fn sample_snapshot(recorded_at: &str) -> StoredSituationSnapshot {
        StoredSituationSnapshot {
            recorded_at: recorded_at.to_string(),
            inputs: SituationInputs {
                position_count: 0,
                positions: Vec::new(),
                timeframes: vec![
                    TimeframeScore {
                        timeframe: "low".to_string(),
                        label: "Low Timeframe".to_string(),
                        score: -5.0,
                        summary: None,
                        updated_at: None,
                    },
                    TimeframeScore {
                        timeframe: "macro".to_string(),
                        label: "Macro Timeframe".to_string(),
                        score: -10.0,
                        summary: None,
                        updated_at: None,
                    },
                ],
                regime: Some(RegimeContext {
                    regime: "risk_off".to_string(),
                    confidence: Some(0.8),
                    drivers: vec!["stress".to_string()],
                    vix: Some(25.0),
                    dxy: Some(105.5),
                }),
                sentiment: vec![SentimentGauge {
                    index_type: "crypto".to_string(),
                    value: 25,
                    classification: "fear".to_string(),
                }],
                latest_timeframe_signal: Some(LatestSignal {
                    signal_type: "divergence".to_string(),
                    severity: "elevated".to_string(),
                    description: "Split tape".to_string(),
                }),
                technical_signal_count: 2,
                triggered_alert_count: 1,
                armed_alert_count: 3,
                acknowledged_alert_count: 0,
                recent_triggered_alerts: Vec::new(),
                market_pulse: vec![MarketPulseItem {
                    symbol: "BTC-USD".to_string(),
                    name: "Bitcoin".to_string(),
                    value: Some(dec!(65000)),
                    day_change_pct: Some(dec!(-1.0)),
                }],
                stale_sources: 1,
                scenarios: vec![ScenarioState {
                    name: "Hard landing".to_string(),
                    probability: 30.0,
                }],
                convictions: vec![ConvictionState {
                    symbol: "BTC".to_string(),
                    score: 1,
                }],
                correlations: vec![CorrelationState {
                    symbol_a: "BTC".to_string(),
                    symbol_b: "SPY".to_string(),
                    correlation: 0.20,
                    period: "30d".to_string(),
                }],
            },
        }
    }

    #[test]
    fn warmup_report_is_non_empty_without_baseline() {
        let current = sample_snapshot("2026-03-20T12:00:00Z");
        let items = compare_snapshots(&current, None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].value, "Warmup");
    }

    #[test]
    fn detects_major_state_changes() {
        let previous = sample_snapshot("2026-03-20T10:00:00Z");
        let mut current = sample_snapshot("2026-03-20T12:00:00Z");
        current.inputs.timeframes[0].score = -30.0;
        current.inputs.triggered_alert_count = 4;
        current.inputs.sentiment[0].value = 10;
        current.inputs.sentiment[0].classification = "extreme_fear".to_string();
        current.inputs.market_pulse[0].day_change_pct = Some(dec!(-5.0));
        current.inputs.scenarios[0].probability = 50.0;
        current.inputs.convictions[0].score = -2;
        current.inputs.correlations[0].correlation = 0.55;

        let items = compare_snapshots(&current, Some(&previous));
        assert!(items.iter().any(|item| item.title.contains("Risk tone")));
        assert!(items
            .iter()
            .any(|item| item.title.contains("Triggered alerts")));
        assert!(items
            .iter()
            .any(|item| item.title.contains("sentiment shifted")));
        assert!(items
            .iter()
            .any(|item| item.title.contains("Scenario re-ranked")));
        assert!(items
            .iter()
            .any(|item| item.title.contains("Conviction changed")));
        assert!(items
            .iter()
            .any(|item| item.title.contains("Correlation shifted")));
    }

    #[test]
    fn deserialize_pre_alert_snapshot_defaults_missing_fields() {
        // Simulate a snapshot stored before #240 added alert fields to SituationInputs.
        // The JSON has no armed_alert_count, acknowledged_alert_count, or recent_triggered_alerts.
        let old_json = r#"{
            "recorded_at": "2026-03-20T10:00:00Z",
            "inputs": {
                "position_count": 1,
                "positions": [],
                "timeframes": [],
                "regime": null,
                "sentiment": [],
                "latest_timeframe_signal": null,
                "technical_signal_count": 0,
                "triggered_alert_count": 2,
                "market_pulse": [],
                "stale_sources": 0,
                "scenarios": [],
                "convictions": [],
                "correlations": []
            }
        }"#;

        let snapshot: StoredSituationSnapshot =
            serde_json::from_str(old_json).expect("should deserialize old snapshot format");
        assert_eq!(snapshot.inputs.triggered_alert_count, 2);
        assert_eq!(snapshot.inputs.armed_alert_count, 0);
        assert_eq!(snapshot.inputs.acknowledged_alert_count, 0);
        assert!(snapshot.inputs.recent_triggered_alerts.is_empty());
    }
}
