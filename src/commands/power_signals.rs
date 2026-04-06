use anyhow::Result;
use serde::Serialize;

use crate::commands::{power_flow, power_flow_conflicts, regime_flows};
use crate::db::backend::BackendConnection;

#[derive(Debug, Serialize)]
pub struct PowerSignalsOutput {
    pub summary: PowerSignalsSummary,
    pub signals: Vec<PowerSignalRow>,
}

#[derive(Debug, Serialize)]
pub struct PowerSignalsSummary {
    pub period_days: usize,
    pub overall_bias: String,
    pub alert_level: String,
    pub dominant_complex: Option<String>,
    pub composite_score: u32,
    pub regime_consistent: bool,
}

#[derive(Debug, Serialize)]
pub struct PowerSignalRow {
    pub rank: usize,
    pub category: String,
    pub name: String,
    pub signal: String,
    pub score: u32,
    pub detail: String,
}

pub fn build_output(backend: &BackendConnection, days: usize) -> Result<PowerSignalsOutput> {
    let regime = regime_flows::build_output(backend)?;
    let conflicts = power_flow_conflicts::build_output(backend, days)?;
    let power = power_flow::build_assess_output(backend, days)?;

    let mut signals = Vec::new();

    for pattern in &regime.patterns {
        let confidence_score = match pattern.confidence.as_str() {
            "high" => 85,
            "medium" => 70,
            "low" => 55,
            _ => 60,
        };
        signals.push(PowerSignalRow {
            rank: 0,
            category: "regime".to_string(),
            name: pattern.pattern_name.clone(),
            signal: pattern.confidence.clone(),
            score: confidence_score,
            detail: pattern.description.clone(),
        });
    }

    for signal in conflicts
        .conflict_indicators
        .signals
        .iter()
        .filter(|signal| signal.active)
    {
        signals.push(PowerSignalRow {
            rank: 0,
            category: "conflict".to_string(),
            name: signal.name.clone(),
            signal: conflicts.conflict_indicators.stress_level.clone(),
            score: conflict_signal_score(
                conflicts.conflict_indicators.composite_score,
                &conflicts.assessment.alert_level,
            ),
            detail: signal.detail.clone(),
        });
    }

    for complex in power.complexes.iter().filter(|complex| complex.net_score != 0) {
        let score = ((complex.net_score.abs() as u32) * 10).min(90);
        let direction = if complex.net_score > 0 {
            "gaining"
        } else {
            "losing"
        };
        signals.push(PowerSignalRow {
            rank: 0,
            category: "power-flow".to_string(),
            name: format!("{} balance", complex.complex),
            signal: format!("{} {}", direction, complex.trend),
            score,
            detail: format!(
                "Net {} with {} gaining / {} losing events over {}d",
                complex.net_score, complex.gaining_events, complex.losing_events, days
            ),
        });
    }

    if let Some(ratio) = &conflicts.defense_energy_ratio {
        if let Some(change) = ratio.change_5d_pct {
            if change.abs() >= 1.0 {
                signals.push(PowerSignalRow {
                    rank: 0,
                    category: "ratio".to_string(),
                    name: ratio.name.clone(),
                    signal: if change > 0.0 {
                        "rising".to_string()
                    } else {
                        "falling".to_string()
                    },
                    score: (change.abs().round() as u32 * 5).clamp(50, 80),
                    detail: ratio.interpretation.clone(),
                });
            }
        }
    }

    if signals.is_empty() {
        signals.push(PowerSignalRow {
            rank: 1,
            category: "summary".to_string(),
            name: "No strong power signals".to_string(),
            signal: "neutral".to_string(),
            score: 25,
            detail: "No regime pattern, conflict trigger, or power-flow imbalance cleared the signal threshold.".to_string(),
        });
    } else {
        signals.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.category.cmp(&b.category))
                .then_with(|| a.name.cmp(&b.name))
        });
        for (idx, signal) in signals.iter_mut().enumerate() {
            signal.rank = idx + 1;
        }
    }

    let composite_score = signals.iter().take(3).map(|signal| signal.score).sum::<u32>() / 3.max(signals.len().min(3) as u32);
    let overall_bias = derive_bias(&regime, &conflicts, &power);

    Ok(PowerSignalsOutput {
        summary: PowerSignalsSummary {
            period_days: days,
            overall_bias,
            alert_level: conflicts.assessment.alert_level.clone(),
            dominant_complex: power.dominant_complex.clone(),
            composite_score,
            regime_consistent: regime.summary.regime_consistent,
        },
        signals,
    })
}

pub fn run(backend: &BackendConnection, days: usize, json_output: bool) -> Result<()> {
    let output = build_output(backend, days)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_terminal(&output);
    }
    Ok(())
}

fn conflict_signal_score(composite_score: u32, alert_level: &str) -> u32 {
    let floor = match alert_level {
        "high_alert" => 85,
        "elevated" => 70,
        "monitoring" => 55,
        _ => 40,
    };
    composite_score.max(floor)
}

fn derive_bias(
    regime: &regime_flows::RegimeFlowsOutput,
    conflicts: &power_flow_conflicts::ConflictsOutput,
    power: &power_flow::AssessOutput,
) -> String {
    if conflicts.assessment.alert_level == "high_alert" {
        return "conflict-escalation".to_string();
    }
    if regime
        .patterns
        .iter()
        .any(|pattern| pattern.pattern_name.contains("Safe-Haven"))
    {
        return "defensive".to_string();
    }
    if power.dominant_complex.as_deref() == Some("MIC") {
        return "multipolar-shift".to_string();
    }
    if regime.summary.risk_appetite == "strong" {
        return "risk-on".to_string();
    }
    "mixed".to_string()
}

fn print_terminal(output: &PowerSignalsOutput) {
    println!("═══ Power Signals ═══\n");
    println!(
        "Bias: {} | Alert: {} | Dominant complex: {} | Composite: {}/100",
        output.summary.overall_bias,
        output.summary.alert_level,
        output
            .summary
            .dominant_complex
            .as_deref()
            .unwrap_or("none"),
        output.summary.composite_score
    );
    println!(
        "Regime consistent: {} | Lookback: {}d",
        if output.summary.regime_consistent { "yes" } else { "no" },
        output.summary.period_days
    );
    println!();

    for signal in &output.signals {
        println!(
            "{:>2}. {:<11} {:<24} {:<18} score {:>3}  {}",
            signal.rank, signal.category, signal.name, signal.signal, signal.score, signal.detail
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::power_flow::{AssessOutput, ComplexAssessment, KeyEvent, PowerShift, TrendAnalysis};
    use crate::commands::power_flow_conflicts::{
        AssetMetric, ConflictAssessment, ConflictIndicators, ConflictSignal, ConflictsOutput,
        FicMicBalance, PowerFlowContext, RatioMetric, RecentConflictEvent, RegimeContext,
        SectorSnapshot,
    };
    use crate::commands::regime_flows::{
        AssetRatio, DetectedPattern, FlowSignal, FlowSummary, RegimeContext as RegimeFlowContext,
        RegimeFlowsOutput,
    };

    #[test]
    fn derive_bias_prefers_high_alert_conflict() {
        let regime = stub_regime("moderate");
        let conflicts = stub_conflicts("high_alert", 92);
        let power = stub_power(Some("MIC"));
        assert_eq!(derive_bias(&regime, &conflicts, &power), "conflict-escalation");
    }

    #[test]
    fn conflict_signal_score_honors_alert_floor() {
        assert_eq!(conflict_signal_score(60, "high_alert"), 85);
        assert_eq!(conflict_signal_score(72, "elevated"), 72);
    }

    fn stub_regime(risk_appetite: &str) -> RegimeFlowsOutput {
        RegimeFlowsOutput {
            regime: RegimeFlowContext {
                current_regime: "risk-off".to_string(),
                confidence: Some(0.7),
                vix: Some(22.0),
                dxy: Some(104.0),
                yield_10y: Some(4.2),
                oil: Some(82.0),
                gold: Some(2400.0),
                btc: Some(91000.0),
            },
            ratios: vec![AssetRatio {
                name: "Gold/Oil".to_string(),
                numerator: "GC=F".to_string(),
                denominator: "CL=F".to_string(),
                current_value: Some(29.0),
                change_5d: Some(3.0),
                direction: "rising".to_string(),
                interpretation: "stub".to_string(),
            }],
            flow_signals: vec![FlowSignal {
                asset_class: "Safe Haven".to_string(),
                symbol: "GC=F".to_string(),
                price: Some(2400.0),
                change_5d_pct: Some(2.0),
                flow_direction: "inflow".to_string(),
                regime_alignment: "aligned".to_string(),
            }],
            patterns: vec![DetectedPattern {
                pattern_name: "Safe-Haven Rotation".to_string(),
                confidence: "high".to_string(),
                description: "stub".to_string(),
                supporting_signals: vec!["Gold rising".to_string()],
            }],
            summary: FlowSummary {
                dominant_flow: "safe_haven".to_string(),
                safe_haven_bid: "strong".to_string(),
                risk_appetite: risk_appetite.to_string(),
                energy_stress: "neutral".to_string(),
                pattern_count: 1,
                regime_consistent: true,
            },
        }
    }

    fn stub_conflicts(alert_level: &str, composite_score: u32) -> ConflictsOutput {
        ConflictsOutput {
            regime: RegimeContext {
                current_regime: "risk-off".to_string(),
                confidence: Some(0.7),
                vix: Some(25.0),
                is_crisis: false,
            },
            defense: SectorSnapshot {
                group: "defense".to_string(),
                assets: vec![],
                avg_change_5d_pct: Some(1.5),
                direction: "rising".to_string(),
            },
            energy: SectorSnapshot {
                group: "energy".to_string(),
                assets: vec![],
                avg_change_5d_pct: Some(2.1),
                direction: "rising".to_string(),
            },
            context_assets: vec![AssetMetric {
                symbol: "^VIX".to_string(),
                label: "VIX".to_string(),
                group: "volatility".to_string(),
                price: Some(25.0),
                change_5d_pct: Some(10.0),
                change_20d_pct: Some(15.0),
                direction: "rising".to_string(),
            }],
            defense_energy_ratio: Some(RatioMetric {
                name: "Defense/Energy".to_string(),
                numerator: "ITA".to_string(),
                denominator: "XLE".to_string(),
                current_value: Some(1.2),
                change_5d_pct: Some(2.0),
                interpretation: "ratio stub".to_string(),
            }),
            conflict_indicators: ConflictIndicators {
                stress_level: "active".to_string(),
                composite_score,
                signals: vec![ConflictSignal {
                    name: "VIX fear regime".to_string(),
                    active: true,
                    detail: "stub".to_string(),
                }],
            },
            power_flow_context: PowerFlowContext {
                recent_events: vec![RecentConflictEvent {
                    date: "2026-04-06".to_string(),
                    event: "stub".to_string(),
                    source_complex: "MIC".to_string(),
                    direction: "gaining".to_string(),
                    magnitude: 4,
                }],
                fic_mic_balance: FicMicBalance {
                    fic_net: -2,
                    mic_net: 5,
                    dominant: "MIC".to_string(),
                    interpretation: "stub".to_string(),
                },
                conflict_events_30d: 3,
            },
            assessment: ConflictAssessment {
                alert_level: alert_level.to_string(),
                summary: "stub".to_string(),
                portfolio_implications: vec![],
            },
        }
    }

    fn stub_power(dominant_complex: Option<&str>) -> AssessOutput {
        AssessOutput {
            period_days: 30,
            total_events: 4,
            complexes: vec![ComplexAssessment {
                complex: "MIC".to_string(),
                net_score: 5,
                gaining_events: 3,
                losing_events: 1,
                gaining_magnitude: 8,
                losing_magnitude: 3,
                trend: "ascending".to_string(),
                first_half_net: 1,
                second_half_net: 4,
                avg_magnitude: 2.5,
                top_events: vec![],
            }],
            dominant_complex: dominant_complex.map(str::to_string),
            power_shifts: vec![PowerShift {
                flow: "FIC → MIC".to_string(),
                event_count: 2,
                total_magnitude: 6,
                latest_event: "stub".to_string(),
                latest_date: "2026-04-06".to_string(),
            }],
            key_events: vec![KeyEvent {
                date: "2026-04-06".to_string(),
                event: "stub".to_string(),
                source_complex: "MIC".to_string(),
                direction: "gaining".to_string(),
                target_complex: Some("FIC".to_string()),
                magnitude: 4,
            }],
            trend_analysis: TrendAnalysis {
                regime: "MIC-dominant".to_string(),
                concentration: 0.6,
                regime_shift_detected: false,
                shift_description: None,
            },
            summary: "stub".to_string(),
        }
    }
}
