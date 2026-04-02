use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::alerts::AlertStatus;
use crate::commands::correlations;
pub use crate::commands::scan::ScanHighlight;
use crate::db;
use crate::db::backend::BackendConnection;
use crate::models::asset_names::resolve_name;
use crate::models::position::{compute_positions, compute_positions_from_allocations};
use crate::web::view_model;

#[derive(Debug, Clone, Serialize)]
pub struct SituationSnapshot {
    pub headline: String,
    pub subtitle: String,
    pub summary_stats: Vec<SituationStat>,
    pub alert_summary: AlertSummary,
    pub watch_now: Vec<SituationInsight>,
    pub portfolio_impacts: Vec<PortfolioImpact>,
    pub risk_matrix: Vec<RiskState>,
    pub cross_timeframe: Vec<CrossTimeframeState>,
    pub correlation_breaks: Vec<CorrelationBreakState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scan_highlights: Vec<ScanHighlight>,
}



#[derive(Debug, Clone, Serialize)]
pub struct CorrelationBreakState {
    pub symbol_a: String,
    pub symbol_b: String,
    pub corr_7d: Option<f64>,
    pub corr_90d: Option<f64>,
    pub break_delta: f64,
    pub severity: String,
    /// Human-readable explanation of what the break means
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<String>,
    /// What the break suggests for portfolio positioning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertSummary {
    pub total: usize,
    pub armed: usize,
    pub triggered: usize,
    pub acknowledged: usize,
    pub recent_triggered: Vec<AlertDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDetail {
    pub id: i64,
    pub rule_text: String,
    pub symbol: String,
    pub kind: String,
    pub triggered_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SituationStat {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SituationInsight {
    pub title: String,
    pub detail: String,
    pub value: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioImpact {
    pub title: String,
    pub detail: String,
    pub value: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskState {
    pub label: String,
    pub detail: String,
    pub value: String,
    pub status: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrossTimeframeState {
    pub timeframe: String,
    pub label: String,
    pub score: f64,
    pub bias: String,
    pub summary: Option<String>,
    pub updated_at: Option<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationInputs {
    pub position_count: usize,
    pub positions: Vec<SituationPosition>,
    pub timeframes: Vec<TimeframeScore>,
    pub regime: Option<RegimeContext>,
    pub sentiment: Vec<SentimentGauge>,
    pub latest_timeframe_signal: Option<LatestSignal>,
    pub technical_signal_count: usize,
    pub triggered_alert_count: usize,
    #[serde(default)]
    pub armed_alert_count: usize,
    #[serde(default)]
    pub acknowledged_alert_count: usize,
    #[serde(default)]
    pub recent_triggered_alerts: Vec<AlertDetail>,
    pub market_pulse: Vec<MarketPulseItem>,
    pub stale_sources: usize,
    pub scenarios: Vec<ScenarioState>,
    pub convictions: Vec<ConvictionState>,
    pub correlations: Vec<CorrelationState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationPosition {
    pub symbol: String,
    pub name: String,
    pub allocation_pct: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub current_value: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeframeScore {
    pub timeframe: String,
    pub label: String,
    pub score: f64,
    pub summary: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeContext {
    pub regime: String,
    pub confidence: Option<f64>,
    pub drivers: Vec<String>,
    pub vix: Option<f64>,
    pub dxy: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentGauge {
    pub index_type: String,
    pub value: u8,
    pub classification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestSignal {
    pub signal_type: String,
    pub severity: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPulseItem {
    pub symbol: String,
    pub name: String,
    pub value: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioState {
    pub name: String,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvictionState {
    pub symbol: String,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationState {
    pub symbol_a: String,
    pub symbol_b: String,
    pub correlation: f64,
    pub period: String,
}

pub fn build_snapshot_backend(backend: &BackendConnection) -> Result<SituationSnapshot> {
    let inputs = collect_inputs_backend(backend)?;
    let correlation_breaks = compute_correlation_breaks(backend);
    let scan_highlights = compute_scan_highlights_backend(backend);
    Ok(build_snapshot(&inputs, &correlation_breaks, &scan_highlights))
}

fn compute_correlation_breaks(backend: &BackendConnection) -> Vec<CorrelationBreakState> {
    let threshold = 0.30;
    let limit = 10;
    match correlations::compute_breaks_backend(backend, threshold, limit) {
        Ok(breaks) => breaks
            .into_iter()
            .map(|b| {
                let severity = if b.break_delta.abs() >= 0.60 {
                    "critical"
                } else if b.break_delta.abs() >= 0.40 {
                    "elevated"
                } else {
                    "normal"
                };
                let interp = correlations::interpret_break(&b);
                CorrelationBreakState {
                    symbol_a: b.symbol_a,
                    symbol_b: b.symbol_b,
                    corr_7d: b.corr_7d,
                    corr_90d: b.corr_90d,
                    break_delta: b.break_delta,
                    severity: severity.to_string(),
                    interpretation: Some(interp.interpretation),
                    signal: Some(interp.signal),
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Compute scan highlights for the Situation Room.
fn compute_scan_highlights_backend(backend: &BackendConnection) -> Vec<ScanHighlight> {
    crate::commands::scan::compute_scan_highlights(backend).unwrap_or_default()
}

pub fn collect_inputs_backend(backend: &BackendConnection) -> Result<SituationInputs> {
    let prices = db::price_cache::get_all_cached_prices_backend(backend)?
        .into_iter()
        .map(|quote| (quote.symbol, quote.price))
        .collect::<HashMap<_, _>>();
    let fx_rates = db::fx_cache::get_all_fx_rates_backend(backend).unwrap_or_default();

    let transactions = db::transactions::list_transactions_backend(backend).unwrap_or_default();
    let mut positions = compute_positions(&transactions, &prices, &fx_rates);
    if positions.is_empty() {
        let allocations = db::allocations::list_allocations_backend(backend).unwrap_or_default();
        if !allocations.is_empty() {
            positions = compute_positions_from_allocations(&allocations, &prices, &fx_rates);
        }
    }

    let positions = positions
        .into_iter()
        .map(|position| SituationPosition {
            symbol: position.symbol.clone(),
            name: resolve_name(&position.symbol),
            allocation_pct: position.allocation_pct,
            day_change_pct: day_change_pct_backend(backend, &position.symbol),
            current_value: position.current_value,
        })
        .collect::<Vec<_>>();

    let configured = db::mobile_timeframe_scores::list_scores_backend(backend)
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row.timeframe.clone(), row))
        .collect::<HashMap<_, _>>();
    let timeframes = ["low", "medium", "high", "macro"]
        .into_iter()
        .map(|timeframe| {
            if let Some(row) = configured.get(timeframe) {
                TimeframeScore {
                    timeframe: row.timeframe.clone(),
                    label: timeframe_label(timeframe).to_string(),
                    score: row.score,
                    summary: row.summary.clone(),
                    updated_at: Some(row.updated_at.clone()),
                }
            } else {
                TimeframeScore {
                    timeframe: timeframe.to_string(),
                    label: timeframe_label(timeframe).to_string(),
                    score: 0.0,
                    summary: None,
                    updated_at: None,
                }
            }
        })
        .collect();

    let regime = db::regime_snapshots::get_current_backend(backend)
        .unwrap_or(None)
        .map(|row| RegimeContext {
            regime: row.regime,
            confidence: row.confidence,
            drivers: parse_driver_list(row.drivers.as_deref()),
            vix: row.vix,
            dxy: row.dxy,
        });

    let sentiment = ["crypto", "traditional"]
        .into_iter()
        .filter_map(|kind| {
            db::sentiment_cache::get_latest_backend(backend, kind)
                .ok()
                .flatten()
        })
        .map(|row| SentimentGauge {
            index_type: row.index_type,
            value: row.value,
            classification: row.classification,
        })
        .collect();

    let latest_timeframe_signal =
        db::timeframe_signals::latest_signal_backend(backend)?.map(|signal| LatestSignal {
            signal_type: signal.signal_type,
            severity: signal.severity,
            description: signal.description,
        });

    let technical_signal_count =
        db::technical_signals::list_signals_backend(backend, None, None, Some(200))
            .map(|rows| rows.len())
            .unwrap_or(0);
    let all_alerts = db::alerts::list_alerts_backend(backend).unwrap_or_default();
    let triggered_alert_count = all_alerts
        .iter()
        .filter(|row| row.status == AlertStatus::Triggered)
        .count();
    let armed_alert_count = all_alerts
        .iter()
        .filter(|row| row.status == AlertStatus::Armed)
        .count();
    let acknowledged_alert_count = all_alerts
        .iter()
        .filter(|row| row.status == AlertStatus::Acknowledged)
        .count();
    let recent_triggered_alerts: Vec<AlertDetail> = all_alerts
        .iter()
        .filter(|row| row.status == AlertStatus::Triggered)
        .take(5)
        .map(|row| AlertDetail {
            id: row.id,
            rule_text: row.rule_text.clone(),
            symbol: row.symbol.clone(),
            kind: row.kind.to_string(),
            triggered_at: row.triggered_at.clone(),
        })
        .collect();

    let market_pulse = view_model::market_overview_symbols()
        .into_iter()
        .take(6)
        .map(|spec| MarketPulseItem {
            symbol: spec.symbol.clone(),
            name: spec.name,
            value: prices.get(&spec.symbol).copied(),
            day_change_pct: day_change_pct_backend(backend, &spec.symbol),
        })
        .collect();
    let scenarios = db::scenarios::list_scenarios_backend(backend, Some("active"))
        .unwrap_or_default()
        .into_iter()
        .map(|row| ScenarioState {
            name: row.name,
            probability: row.probability,
        })
        .collect();
    let convictions = db::convictions::list_current_backend(backend)
        .unwrap_or_default()
        .into_iter()
        .map(|row| ConvictionState {
            symbol: row.symbol,
            score: row.score,
        })
        .collect();
    let correlations = db::correlation_snapshots::list_current_backend(backend, Some("30d"))
        .unwrap_or_default()
        .into_iter()
        .map(|row| CorrelationState {
            symbol_a: row.symbol_a,
            symbol_b: row.symbol_b,
            correlation: row.correlation,
            period: row.period,
        })
        .collect();

    Ok(SituationInputs {
        position_count: positions.len(),
        positions,
        timeframes,
        regime,
        sentiment,
        latest_timeframe_signal,
        technical_signal_count,
        triggered_alert_count,
        armed_alert_count,
        acknowledged_alert_count,
        recent_triggered_alerts,
        market_pulse,
        stale_sources: count_stale_sources(backend),
        scenarios,
        convictions,
        correlations,
    })
}

pub fn build_snapshot(
    inputs: &SituationInputs,
    correlation_breaks: &[CorrelationBreakState],
    scan_highlights: &[ScanHighlight],
) -> SituationSnapshot {
    let average_score = average_timeframe_score(&inputs.timeframes);
    let mut watch_now = situation_watch_now(inputs, average_score);
    let portfolio_impacts = situation_portfolio_impacts(&inputs.positions);
    let risk_matrix = situation_risk_matrix(inputs);
    let cross_timeframe = situation_cross_timeframe(&inputs.timeframes);

    // Surface significant correlation breaks in watch_now
    if !correlation_breaks.is_empty() {
        let break_count = correlation_breaks.len();
        let worst = &correlation_breaks[0]; // Already sorted by |delta| descending
        let severity = if worst.break_delta.abs() >= 0.60 {
            "critical"
        } else if worst.break_delta.abs() >= 0.40 {
            "elevated"
        } else {
            "normal"
        };
        watch_now.push(SituationInsight {
            title: format!(
                "{} correlation break{}",
                break_count,
                if break_count == 1 { "" } else { "s" }
            ),
            detail: format!(
                "{} ↔ {} diverged {:+.2} (7d vs 90d)",
                worst.symbol_a, worst.symbol_b, worst.break_delta
            ),
            value: format!("{:+.2}", worst.break_delta),
            severity: severity.to_string(),
        });
        // Re-sort and truncate after adding correlation break insight
        watch_now.sort_by(|left, right| {
            severity_weight(&right.severity)
                .cmp(&severity_weight(&left.severity))
                .then_with(|| left.title.cmp(&right.title))
        });
        watch_now.truncate(6);
    }
    let headline = inputs
        .latest_timeframe_signal
        .as_ref()
        .map(|signal| pretty_signal(&signal.signal_type))
        .or_else(|| {
            inputs
                .regime
                .as_ref()
                .map(|regime| pretty_signal(&regime.regime))
        })
        .unwrap_or_else(|| "Situation Stable".to_string());
    let subtitle = inputs
        .latest_timeframe_signal
        .as_ref()
        .map(|signal| signal.description.clone())
        .unwrap_or_else(|| {
            format!(
                "{} positions • {} tracked layers",
                inputs.position_count,
                inputs.timeframes.len()
            )
        });

    SituationSnapshot {
        headline,
        subtitle,
        summary_stats: vec![
            SituationStat {
                label: "Avg Score".to_string(),
                value: format!("{:.0}", average_score),
            },
            SituationStat {
                label: "Alerts".to_string(),
                value: inputs.triggered_alert_count.to_string(),
            },
            SituationStat {
                label: "Tech Signals".to_string(),
                value: inputs.technical_signal_count.to_string(),
            },
            SituationStat {
                label: "Stale Sources".to_string(),
                value: inputs.stale_sources.to_string(),
            },
        ],
        alert_summary: AlertSummary {
            total: inputs.armed_alert_count
                + inputs.triggered_alert_count
                + inputs.acknowledged_alert_count,
            armed: inputs.armed_alert_count,
            triggered: inputs.triggered_alert_count,
            acknowledged: inputs.acknowledged_alert_count,
            recent_triggered: inputs.recent_triggered_alerts.clone(),
        },
        watch_now,
        portfolio_impacts,
        risk_matrix,
        cross_timeframe,
        correlation_breaks: correlation_breaks.to_vec(),
        scan_highlights: scan_highlights.to_vec(),
    }
}

fn situation_watch_now(inputs: &SituationInputs, average_score: f64) -> Vec<SituationInsight> {
    let mut items = Vec::new();

    if let Some(signal) = &inputs.latest_timeframe_signal {
        items.push(SituationInsight {
            title: pretty_signal(&signal.signal_type),
            detail: signal.description.clone(),
            value: signal.severity.clone(),
            severity: normalize_severity(&signal.severity).to_string(),
        });
    }

    if let Some(regime) = &inputs.regime {
        items.push(SituationInsight {
            title: format!("Regime: {}", pretty_signal(&regime.regime)),
            detail: regime
                .drivers
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • "),
            value: regime
                .confidence
                .map(|value| format!("{}%", (value * 100.0).round() as i32))
                .unwrap_or_else(|| "—".to_string()),
            severity: "normal".to_string(),
        });
    }

    if let Some(strongest) = inputs.market_pulse.iter().max_by(|left, right| {
        change_magnitude(right.day_change_pct).total_cmp(&change_magnitude(left.day_change_pct))
    }) {
        let change = strongest
            .day_change_pct
            .map(|value| value.round_dp(2).to_string())
            .unwrap_or_else(|| {
                strongest
                    .value
                    .map(|value| value.round_dp(2).to_string())
                    .unwrap_or_else(|| "—".to_string())
            });
        let severity = if change_magnitude(strongest.day_change_pct) >= 2.5 {
            "critical"
        } else {
            "elevated"
        };
        items.push(SituationInsight {
            title: format!("{} is leading the tape", strongest.symbol),
            detail: strongest.name.clone(),
            value: change,
            severity: severity.to_string(),
        });
    }

    if inputs.triggered_alert_count > 0 {
        items.push(SituationInsight {
            title: format!("{} live alerts need triage", inputs.triggered_alert_count),
            detail: "Triggered rules are active in the current monitoring stack.".to_string(),
            value: "alert".to_string(),
            severity: "critical".to_string(),
        });
    }

    if inputs.stale_sources > 0 {
        items.push(SituationInsight {
            title: format!("{} stale data sources", inputs.stale_sources),
            detail: "Operational trust is degraded until the slow feeds refresh.".to_string(),
            value: "ops".to_string(),
            severity: "elevated".to_string(),
        });
    }

    if average_score <= -15.0 {
        items.push(SituationInsight {
            title: "Average timeframe tone is soft".to_string(),
            detail: "Cross-layer analytics are leaning defensive.".to_string(),
            value: format!("{average_score:.0}"),
            severity: if average_score <= -35.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }

    items.sort_by(|left, right| {
        severity_weight(&right.severity)
            .cmp(&severity_weight(&left.severity))
            .then_with(|| left.title.cmp(&right.title))
    });
    items.truncate(6);
    items
}

fn situation_portfolio_impacts(positions: &[SituationPosition]) -> Vec<PortfolioImpact> {
    let mut items: Vec<_> = positions
        .iter()
        .map(|position| {
            let day_change = position
                .day_change_pct
                .map(|value| value.round_dp(2).to_string())
                .unwrap_or_else(|| "—".to_string());
            let magnitude = position
                .day_change_pct
                .map(|value| value.abs())
                .unwrap_or(dec!(0));
            let severity = if magnitude >= dec!(3) {
                "elevated"
            } else {
                "normal"
            };
            (
                change_magnitude(position.day_change_pct),
                position.current_value.unwrap_or(dec!(0)),
                PortfolioImpact {
                    title: position.symbol.clone(),
                    detail: format!(
                        "{} • {} allocation",
                        position.name,
                        position
                            .allocation_pct
                            .map(|value| value.round_dp(2).to_string())
                            .unwrap_or_else(|| "—".to_string())
                    ),
                    value: day_change,
                    severity: severity.to_string(),
                },
            )
        })
        .collect();

    items.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.title.cmp(&right.2.title))
    });
    items.truncate(6);
    items.into_iter().map(|(_, _, item)| item).collect()
}

fn situation_risk_matrix(inputs: &SituationInputs) -> Vec<RiskState> {
    let mut rows = Vec::new();

    if let Some(regime) = &inputs.regime {
        if let Some(vix) = regime.vix {
            rows.push(RiskState {
                label: "Volatility".to_string(),
                detail: "Equity stress proxy".to_string(),
                value: format!("{vix:.1}"),
                status: if vix >= 20.0 { "warning" } else { "fresh" }.to_string(),
                severity: if vix >= 25.0 {
                    "critical"
                } else if vix >= 20.0 {
                    "elevated"
                } else {
                    "normal"
                }
                .to_string(),
            });
        }
        if let Some(dxy) = regime.dxy {
            rows.push(RiskState {
                label: "Dollar".to_string(),
                detail: "Funding and global pressure".to_string(),
                value: format!("{dxy:.1}"),
                status: if dxy >= 105.0 { "warning" } else { "fresh" }.to_string(),
                severity: if dxy >= 106.0 {
                    "critical"
                } else if dxy >= 105.0 {
                    "elevated"
                } else {
                    "normal"
                }
                .to_string(),
            });
        }
    }

    if let Some(item) = inputs
        .market_pulse
        .iter()
        .find(|item| item.symbol.contains("BTC") || item.name.to_lowercase().contains("bitcoin"))
    {
        let change = item
            .day_change_pct
            .map(|value| value.round_dp(2).to_string())
            .unwrap_or_else(|| "—".to_string());
        let move_pct = change_magnitude(item.day_change_pct);
        rows.push(RiskState {
            label: "Crypto Risk".to_string(),
            detail: "High-beta sentiment read".to_string(),
            value: change,
            status: if move_pct <= -2.5 { "warning" } else { "fresh" }.to_string(),
            severity: if move_pct <= -4.0 {
                "critical"
            } else if move_pct <= -2.5 {
                "elevated"
            } else {
                "normal"
            }
            .to_string(),
        });
    }

    if let Some(sentiment) = inputs
        .sentiment
        .iter()
        .find(|row| row.index_type.eq_ignore_ascii_case("crypto"))
    {
        rows.push(RiskState {
            label: "Crypto Sentiment".to_string(),
            detail: pretty_signal(&sentiment.classification),
            value: sentiment.value.to_string(),
            status: if sentiment.value <= 25 {
                "warning"
            } else {
                "fresh"
            }
            .to_string(),
            severity: if sentiment.value <= 20 {
                "critical"
            } else if sentiment.value <= 25 {
                "elevated"
            } else {
                "normal"
            }
            .to_string(),
        });
    }

    if let Some(macro_row) = inputs
        .timeframes
        .iter()
        .find(|row| row.timeframe == "macro")
    {
        rows.push(RiskState {
            label: "Macro Stack".to_string(),
            detail: "Long-cycle conviction".to_string(),
            value: format!("{:.0}", macro_row.score),
            status: if macro_row.score < -15.0 {
                "warning"
            } else {
                "fresh"
            }
            .to_string(),
            severity: if macro_row.score < -35.0 {
                "critical"
            } else if macro_row.score < -15.0 {
                "elevated"
            } else {
                "normal"
            }
            .to_string(),
        });
    }

    rows
}

fn situation_cross_timeframe(timeframes: &[TimeframeScore]) -> Vec<CrossTimeframeState> {
    timeframes
        .iter()
        .map(|row| CrossTimeframeState {
            timeframe: row.timeframe.clone(),
            label: row.label.clone(),
            score: row.score,
            bias: score_bias(row.score).to_string(),
            summary: row.summary.clone(),
            updated_at: row.updated_at.clone(),
            severity: score_severity(row.score).to_string(),
        })
        .collect()
}

fn timeframe_label(value: &str) -> &'static str {
    match value {
        "low" => "Low Timeframe",
        "medium" => "Medium Timeframe",
        "high" => "High Timeframe",
        "macro" => "Macro Timeframe",
        _ => "Timeframe",
    }
}

fn average_timeframe_score(timeframes: &[TimeframeScore]) -> f64 {
    if timeframes.is_empty() {
        return 0.0;
    }
    timeframes.iter().map(|row| row.score).sum::<f64>() / timeframes.len() as f64
}

fn score_bias(score: f64) -> &'static str {
    if score >= 15.0 {
        "bullish"
    } else if score <= -15.0 {
        "defensive"
    } else {
        "mixed"
    }
}

fn score_severity(score: f64) -> &'static str {
    if score <= -35.0 {
        "critical"
    } else if score <= -15.0 || score >= 35.0 {
        "elevated"
    } else {
        "normal"
    }
}

fn severity_weight(value: &str) -> i32 {
    match value {
        "critical" => 3,
        "elevated" | "warning" => 2,
        _ => 1,
    }
}

fn normalize_severity(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "warning" | "notable" | "elevated" => "elevated",
        _ => "normal",
    }
}

fn change_magnitude(value: Option<Decimal>) -> f64 {
    value
        .and_then(|number| number.to_string().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn pretty_signal(raw: &str) -> String {
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

fn parse_driver_list(raw: Option<&str>) -> Vec<String> {
    raw.and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }

    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
}

fn count_stale_sources(backend: &BackendConnection) -> usize {
    let prices = db::price_cache::get_all_cached_prices_backend(backend).unwrap_or_default();
    let latest_price_fetch = prices
        .iter()
        .filter_map(|quote| parse_timestamp(&quote.fetched_at))
        .max();

    let news = db::news_cache::get_latest_news_backend(backend, 50, None, None, None, Some(72))
        .unwrap_or_default();
    let latest_news_fetch = news
        .iter()
        .filter_map(|entry| parse_timestamp(&entry.fetched_at))
        .max();

    let predictions =
        db::predictions_cache::get_cached_predictions_backend(backend, 200).unwrap_or_default();
    let latest_prediction_fetch = db::predictions_cache::get_last_update_backend(backend)
        .ok()
        .flatten()
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

    let sentiments = ["crypto", "traditional"]
        .into_iter()
        .filter_map(|kind| {
            db::sentiment_cache::get_latest_backend(backend, kind)
                .ok()
                .flatten()
        })
        .collect::<Vec<_>>();
    let latest_sentiment_fetch = sentiments
        .iter()
        .filter_map(|entry| parse_timestamp(&entry.fetched_at))
        .max();

    let now = Utc::now();
    let sources = [
        (prices.len(), latest_price_fetch, 15 * 60),
        (news.len(), latest_news_fetch, 30 * 60),
        (predictions.len(), latest_prediction_fetch, 2 * 60 * 60),
        (sentiments.len(), latest_sentiment_fetch, 2 * 60 * 60),
    ];

    sources
        .into_iter()
        .filter(
            |(records, last_fetch, fresh_within_secs)| match (*records, *last_fetch) {
                (0, _) => true,
                (_, Some(ts)) => now.signed_duration_since(ts).num_seconds() > *fresh_within_secs,
                _ => true,
            },
        )
        .count()
}

fn day_change_pct_backend(backend: &BackendConnection, symbol: &str) -> Option<Decimal> {
    let history = db::price_history::get_history_backend(backend, symbol, 2).ok()?;
    if history.len() < 2 {
        return None;
    }
    let latest = history.last()?.close;
    let previous = history.get(history.len() - 2)?.close;
    if previous == dec!(0) {
        return None;
    }
    Some((latest - previous) / previous * dec!(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_has_stable_defaults() {
        let snapshot = build_snapshot(
            &SituationInputs {
                position_count: 0,
                positions: Vec::new(),
                timeframes: Vec::new(),
                regime: None,
                sentiment: Vec::new(),
                latest_timeframe_signal: None,
                technical_signal_count: 0,
                triggered_alert_count: 0,
                armed_alert_count: 0,
                acknowledged_alert_count: 0,
                recent_triggered_alerts: Vec::new(),
                market_pulse: Vec::new(),
                stale_sources: 0,
                scenarios: Vec::new(),
                convictions: Vec::new(),
                correlations: Vec::new(),
            },
            &[],
            &[],
        );

        assert_eq!(snapshot.headline, "Situation Stable");
        assert_eq!(snapshot.watch_now.len(), 0);
        assert_eq!(snapshot.summary_stats.len(), 4);
        assert_eq!(snapshot.cross_timeframe.len(), 0);
        assert_eq!(snapshot.alert_summary.total, 0);
        assert_eq!(snapshot.alert_summary.triggered, 0);
        assert!(snapshot.correlation_breaks.is_empty());
        assert!(snapshot.scan_highlights.is_empty());
    }

    #[test]
    fn watch_now_prioritizes_critical_items() {
        let snapshot = build_snapshot(&SituationInputs {
            position_count: 2,
            positions: Vec::new(),
            timeframes: vec![
                TimeframeScore {
                    timeframe: "low".to_string(),
                    label: "Low Timeframe".to_string(),
                    score: -40.0,
                    summary: Some("Risk-off".to_string()),
                    updated_at: None,
                },
                TimeframeScore {
                    timeframe: "macro".to_string(),
                    label: "Macro Timeframe".to_string(),
                    score: -30.0,
                    summary: Some("Tight liquidity".to_string()),
                    updated_at: None,
                },
            ],
            regime: Some(RegimeContext {
                regime: "risk_off".to_string(),
                confidence: Some(0.75),
                drivers: vec!["USD strength".to_string(), "Vol up".to_string()],
                vix: Some(28.0),
                dxy: Some(106.2),
            }),
            sentiment: vec![SentimentGauge {
                index_type: "crypto".to_string(),
                value: 18,
                classification: "extreme_fear".to_string(),
            }],
            latest_timeframe_signal: Some(LatestSignal {
                signal_type: "transition".to_string(),
                severity: "critical".to_string(),
                description: "Cross-layer breakdown".to_string(),
            }),
            technical_signal_count: 7,
            triggered_alert_count: 2,
            armed_alert_count: 5,
            acknowledged_alert_count: 1,
            recent_triggered_alerts: vec![
                AlertDetail {
                    id: 1,
                    rule_text: "BTC-USD above 100000".to_string(),
                    symbol: "BTC-USD".to_string(),
                    kind: "price".to_string(),
                    triggered_at: Some("2026-03-23T10:00:00Z".to_string()),
                },
                AlertDetail {
                    id: 2,
                    rule_text: "VIX above 25".to_string(),
                    symbol: "VIX".to_string(),
                    kind: "indicator".to_string(),
                    triggered_at: Some("2026-03-23T09:30:00Z".to_string()),
                },
            ],
            market_pulse: vec![MarketPulseItem {
                symbol: "BTC-USD".to_string(),
                name: "Bitcoin".to_string(),
                value: Some(dec!(65000)),
                day_change_pct: Some(dec!(-4.6)),
            }],
            stale_sources: 1,
            scenarios: Vec::new(),
            convictions: Vec::new(),
            correlations: Vec::new(),
        }, &[], &[]);

        assert!(!snapshot.watch_now.is_empty());
        assert_eq!(snapshot.watch_now[0].severity, "critical");
        assert!(snapshot
            .watch_now
            .iter()
            .any(|item| item.title.contains("live alerts")));
        assert_eq!(snapshot.risk_matrix[0].severity, "critical");

        // Alert summary assertions
        assert_eq!(snapshot.alert_summary.total, 8);
        assert_eq!(snapshot.alert_summary.armed, 5);
        assert_eq!(snapshot.alert_summary.triggered, 2);
        assert_eq!(snapshot.alert_summary.acknowledged, 1);
        assert_eq!(snapshot.alert_summary.recent_triggered.len(), 2);
        assert_eq!(snapshot.alert_summary.recent_triggered[0].symbol, "BTC-USD");
    }

    #[test]
    fn correlation_breaks_surface_in_watch_now() {
        let breaks = vec![
            CorrelationBreakState {
                symbol_a: "BTC-USD".to_string(),
                symbol_b: "GC=F".to_string(),
                corr_7d: Some(-0.30),
                corr_90d: Some(0.45),
                break_delta: -0.75,
                severity: "critical".to_string(),
                interpretation: Some("Bitcoin and Gold have flipped to negative short-term correlation".to_string()),
                signal: Some("Gold-BTC divergence suggests capital rotating between hard assets".to_string()),
            },
            CorrelationBreakState {
                symbol_a: "BTC-USD".to_string(),
                symbol_b: "SPY".to_string(),
                corr_7d: Some(0.10),
                corr_90d: Some(0.50),
                break_delta: -0.40,
                severity: "elevated".to_string(),
                interpretation: Some("Bitcoin is decoupling from equities".to_string()),
                signal: Some("BTC-equity decoupling may signal BTC finding its own narrative".to_string()),
            },
        ];
        let snapshot = build_snapshot(
            &SituationInputs {
                position_count: 0,
                positions: Vec::new(),
                timeframes: Vec::new(),
                regime: None,
                sentiment: Vec::new(),
                latest_timeframe_signal: None,
                technical_signal_count: 0,
                triggered_alert_count: 0,
                armed_alert_count: 0,
                acknowledged_alert_count: 0,
                recent_triggered_alerts: Vec::new(),
                market_pulse: Vec::new(),
                stale_sources: 0,
                scenarios: Vec::new(),
                convictions: Vec::new(),
                correlations: Vec::new(),
            },
            &breaks,
            &[],
        );

        // Correlation breaks should appear in watch_now
        assert!(
            snapshot
                .watch_now
                .iter()
                .any(|item| item.title.contains("correlation break")),
            "Correlation breaks should appear in watch_now"
        );
        // Should be surfaced with critical severity (worst break is -0.75)
        let corr_item = snapshot
            .watch_now
            .iter()
            .find(|item| item.title.contains("correlation break"))
            .unwrap();
        assert_eq!(corr_item.severity, "critical");
        assert!(corr_item.detail.contains("BTC-USD"));
        assert!(corr_item.detail.contains("GC=F"));

        // correlation_breaks field should be populated
        assert_eq!(snapshot.correlation_breaks.len(), 2);
        assert_eq!(snapshot.correlation_breaks[0].symbol_a, "BTC-USD");
        assert_eq!(snapshot.correlation_breaks[0].symbol_b, "GC=F");
    }

    #[test]
    fn no_correlation_breaks_leaves_section_empty() {
        let snapshot = build_snapshot(
            &SituationInputs {
                position_count: 0,
                positions: Vec::new(),
                timeframes: Vec::new(),
                regime: None,
                sentiment: Vec::new(),
                latest_timeframe_signal: None,
                technical_signal_count: 0,
                triggered_alert_count: 0,
                armed_alert_count: 0,
                acknowledged_alert_count: 0,
                recent_triggered_alerts: Vec::new(),
                market_pulse: Vec::new(),
                stale_sources: 0,
                scenarios: Vec::new(),
                convictions: Vec::new(),
                correlations: Vec::new(),
            },
            &[],
            &[],
        );

        assert!(snapshot.correlation_breaks.is_empty());
        assert!(
            !snapshot
                .watch_now
                .iter()
                .any(|item| item.title.contains("correlation break")),
            "No correlation break insight when no breaks exist"
        );
    }

    #[test]
    fn scan_highlights_included_in_snapshot() {
        use crate::commands::scan::ScanHighlight;
        let highlights = vec![
            ScanHighlight {
                symbol: "BTC-USD".to_string(),
                name: "Bitcoin".to_string(),
                scan_type: "big_mover".to_string(),
                detail: "+5.2% daily change".to_string(),
                value_pct: Some(5.2),
                severity: "elevated".to_string(),
            },
            ScanHighlight {
                symbol: "AAPL".to_string(),
                name: "Apple Inc".to_string(),
                scan_type: "trackline_breach".to_string(),
                detail: "Below SMA50 (gap -3.1%)".to_string(),
                value_pct: Some(-3.1),
                severity: "normal".to_string(),
            },
        ];
        let snapshot = build_snapshot(
            &SituationInputs {
                position_count: 0,
                positions: Vec::new(),
                timeframes: Vec::new(),
                regime: None,
                sentiment: Vec::new(),
                latest_timeframe_signal: None,
                technical_signal_count: 0,
                triggered_alert_count: 0,
                armed_alert_count: 0,
                acknowledged_alert_count: 0,
                recent_triggered_alerts: Vec::new(),
                market_pulse: Vec::new(),
                stale_sources: 0,
                scenarios: Vec::new(),
                convictions: Vec::new(),
                correlations: Vec::new(),
            },
            &[],
            &highlights,
        );

        assert_eq!(snapshot.scan_highlights.len(), 2);
        assert_eq!(snapshot.scan_highlights[0].symbol, "BTC-USD");
        assert_eq!(snapshot.scan_highlights[0].scan_type, "big_mover");
        assert_eq!(snapshot.scan_highlights[1].symbol, "AAPL");
        assert_eq!(snapshot.scan_highlights[1].scan_type, "trackline_breach");
    }

    #[test]
    fn scan_highlights_omitted_from_json_when_empty() {
        let snapshot = build_snapshot(
            &SituationInputs {
                position_count: 0,
                positions: Vec::new(),
                timeframes: Vec::new(),
                regime: None,
                sentiment: Vec::new(),
                latest_timeframe_signal: None,
                technical_signal_count: 0,
                triggered_alert_count: 0,
                armed_alert_count: 0,
                acknowledged_alert_count: 0,
                recent_triggered_alerts: Vec::new(),
                market_pulse: Vec::new(),
                stale_sources: 0,
                scenarios: Vec::new(),
                convictions: Vec::new(),
                correlations: Vec::new(),
            },
            &[],
            &[],
        );

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(
            !json.contains("scan_highlights"),
            "scan_highlights should be omitted from JSON when empty"
        );
    }
}
