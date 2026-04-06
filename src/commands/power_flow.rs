use crate::db::backend::BackendConnection;
use crate::db::power_flows;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub fn run_add(
    backend: &BackendConnection,
    event: &str,
    source: &str,
    direction: &str,
    target: Option<&str>,
    evidence: &str,
    magnitude: i32,
    agent_source: Option<&str>,
    date: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let date_val = date.unwrap_or(&today);

    let id = power_flows::add_power_flow_backend(
        backend,
        date_val,
        event,
        source,
        direction,
        target,
        evidence,
        magnitude,
        agent_source,
    )?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": id,
                "date": date_val,
                "event": event,
                "source_complex": source,
                "direction": direction,
                "target_complex": target,
                "evidence": evidence,
                "magnitude": magnitude,
                "agent_source": agent_source,
            }))?
        );
    } else {
        let target_str = target
            .map(|t| format!(" → {}", t))
            .unwrap_or_default();
        println!(
            "Logged power flow: {} {} (mag {}){} — {}",
            source, direction, magnitude, target_str, event
        );
    }

    Ok(())
}

pub fn run_list(
    backend: &BackendConnection,
    complex: Option<&str>,
    direction: Option<&str>,
    days: usize,
    json_output: bool,
) -> Result<()> {
    let entries = power_flows::list_power_flows_backend(backend, complex, direction, days)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "power_flows": entries,
                "count": entries.len(),
                "period_days": days,
                "filters": {
                    "complex": complex,
                    "direction": direction,
                }
            }))?
        );
    } else {
        if entries.is_empty() {
            println!("No power flow entries in the last {} days.", days);
            if complex.is_some() || direction.is_some() {
                println!("Try removing filters or increasing --days.");
            }
            return Ok(());
        }

        println!("POWER FLOWS (last {} days)", days);
        println!("{}", "─".repeat(100));
        let header = format!(
            "{:<12} {:<8} {:<9} {:<8} {:>3}  {}",
            "Date", "Source", "Direction", "Target", "Mag", "Event"
        );
        println!("{header}");
        println!("{}", "─".repeat(100));

        for entry in &entries {
            let target_str = entry
                .target_complex
                .as_deref()
                .unwrap_or("—");
            let event_display = if entry.event.len() > 50 {
                format!("{}...", &entry.event[..47])
            } else {
                entry.event.clone()
            };
            println!(
                "{:<12} {:<8} {:<9} {:<8} {:>3}  {}",
                entry.date,
                entry.source_complex,
                entry.direction,
                target_str,
                entry.magnitude,
                event_display
            );
        }

        println!();
        println!("{} entries", entries.len());
    }

    Ok(())
}

pub fn run_balance(
    backend: &BackendConnection,
    days: usize,
    json_output: bool,
) -> Result<()> {
    let balances = power_flows::compute_balance_backend(backend, days)?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "period_days": days,
                "balances": balances,
            }))?
        );
    } else {
        println!("POWER BALANCE (last {} days)", days);
        println!("{}", "─".repeat(60));

        let has_data = balances.iter().any(|b| b.gaining_count > 0 || b.losing_count > 0);

        if !has_data {
            println!("No power flow data in the last {} days.", days);
            println!("Use `pftui analytics power-flow add` to log power shift events.");
            return Ok(());
        }

        for balance in &balances {
            let sign = if balance.net >= 0 { "+" } else { "" };
            println!(
                "{}:  {}{} ({} gaining, {} losing)",
                balance.complex,
                sign,
                balance.net,
                balance.gaining_count,
                balance.losing_count,
            );
        }
    }

    Ok(())
}

// ── Assess ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AssessOutput {
    pub period_days: usize,
    pub total_events: usize,
    pub complexes: Vec<ComplexAssessment>,
    pub dominant_complex: Option<String>,
    pub power_shifts: Vec<PowerShift>,
    pub key_events: Vec<KeyEvent>,
    pub trend_analysis: TrendAnalysis,
    pub summary: String,
}

#[derive(Debug, Serialize)]
pub struct ComplexAssessment {
    pub complex: String,
    pub net_score: i64,
    pub gaining_events: usize,
    pub losing_events: usize,
    pub gaining_magnitude: i64,
    pub losing_magnitude: i64,
    /// "ascending", "descending", "stable", "volatile"
    pub trend: String,
    /// First-half vs second-half net score
    pub first_half_net: i64,
    pub second_half_net: i64,
    /// Average magnitude of events involving this complex
    pub avg_magnitude: f64,
    pub top_events: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PowerShift {
    /// e.g. "FIC → MIC"
    pub flow: String,
    pub event_count: usize,
    pub total_magnitude: i64,
    /// Most recent event in this flow
    pub latest_event: String,
    pub latest_date: String,
}

#[derive(Debug, Serialize)]
pub struct KeyEvent {
    pub date: String,
    pub event: String,
    pub source_complex: String,
    pub direction: String,
    pub target_complex: Option<String>,
    pub magnitude: i32,
}

#[derive(Debug, Serialize)]
pub struct TrendAnalysis {
    /// "FIC-dominant", "MIC-dominant", "TIC-dominant", "contested", "no-data"
    pub regime: String,
    /// How concentrated power is (0.0 = evenly split, 1.0 = one complex has all)
    pub concentration: f64,
    /// Whether the period saw a regime change
    pub regime_shift_detected: bool,
    /// Description of the shift if detected
    pub shift_description: Option<String>,
}

pub fn build_assess_output(
    backend: &BackendConnection,
    days: usize,
) -> Result<AssessOutput> {
    let entries = power_flows::list_power_flows_backend(backend, None, None, days)?;
    let balances = power_flows::compute_balance_backend(backend, days)?;

    if entries.is_empty() {
        return Ok(AssessOutput {
            period_days: days,
            total_events: 0,
            complexes: vec![],
            dominant_complex: None,
            power_shifts: vec![],
            key_events: vec![],
            trend_analysis: TrendAnalysis {
                regime: "no-data".to_string(),
                concentration: 0.0,
                regime_shift_detected: false,
                shift_description: None,
            },
            summary: format!("No power flow data in the last {} days.", days),
        });
    }

    // Sort entries by date ascending for trend analysis
    let mut sorted_entries = entries.clone();
    sorted_entries.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));

    // Split into first half and second half for trend detection
    let midpoint = sorted_entries.len() / 2;
    let first_half = &sorted_entries[..midpoint];
    let second_half = &sorted_entries[midpoint..];

    // Build per-complex assessments
    let complexes_list = ["FIC", "MIC", "TIC"];
    let mut complex_assessments: Vec<ComplexAssessment> = Vec::new();

    for complex in &complexes_list {
        let balance = balances
            .iter()
            .find(|b| b.complex == *complex);

        let (gaining_events, losing_events, gaining_mag, losing_mag) = match balance {
            Some(b) => (
                b.gaining_count as usize,
                b.losing_count as usize,
                b.gaining_magnitude,
                b.losing_magnitude,
            ),
            None => (0, 0, 0, 0),
        };

        let net = gaining_mag - losing_mag;
        let total_events_for_complex = gaining_events + losing_events;
        let total_mag = gaining_mag + losing_mag;
        let avg_magnitude = if total_events_for_complex > 0 {
            total_mag as f64 / total_events_for_complex as f64
        } else {
            0.0
        };

        // Compute first/second half nets for this complex
        let first_half_net = compute_half_net(first_half, complex);
        let second_half_net = compute_half_net(second_half, complex);

        let trend = classify_trend(first_half_net, second_half_net, net);

        // Top events: highest magnitude events involving this complex
        let mut complex_events: Vec<&power_flows::PowerFlowEntry> = sorted_entries
            .iter()
            .filter(|e| {
                e.source_complex == *complex
                    || e.target_complex.as_deref() == Some(complex)
            })
            .collect();
        complex_events.sort_by(|a, b| b.magnitude.cmp(&a.magnitude));
        let top_events: Vec<String> = complex_events
            .iter()
            .take(3)
            .map(|e| format!("[{}] {} (mag {})", e.date, e.event, e.magnitude))
            .collect();

        complex_assessments.push(ComplexAssessment {
            complex: complex.to_string(),
            net_score: net,
            gaining_events,
            losing_events,
            gaining_magnitude: gaining_mag,
            losing_magnitude: losing_mag,
            trend,
            first_half_net,
            second_half_net,
            avg_magnitude: (avg_magnitude * 100.0).round() / 100.0,
            top_events,
        });
    }

    // Determine dominant complex
    let dominant_complex = {
        let max_net = complex_assessments
            .iter()
            .max_by_key(|c| c.net_score);
        match max_net {
            Some(c) if c.net_score > 0 => Some(c.complex.clone()),
            _ => None,
        }
    };

    // Power shifts: directed flows between complexes
    let mut shift_map: std::collections::HashMap<String, (usize, i64, String, String)> =
        std::collections::HashMap::new();
    for entry in &sorted_entries {
        if let Some(target) = &entry.target_complex {
            let flow_key = if entry.direction == "gaining" {
                format!("{} → {}", entry.source_complex, target)
            } else {
                format!("{} → {}", target, entry.source_complex)
            };
            let e = shift_map
                .entry(flow_key)
                .or_insert((0, 0, String::new(), String::new()));
            e.0 += 1;
            e.1 += entry.magnitude as i64;
            // Keep latest
            if entry.date >= e.3 || e.3.is_empty() {
                e.2 = entry.event.clone();
                e.3 = entry.date.clone();
            }
        }
    }

    let mut power_shifts: Vec<PowerShift> = shift_map
        .into_iter()
        .map(|(flow, (count, mag, event, date))| PowerShift {
            flow,
            event_count: count,
            total_magnitude: mag,
            latest_event: event,
            latest_date: date,
        })
        .collect();
    power_shifts.sort_by(|a, b| b.total_magnitude.cmp(&a.total_magnitude));

    // Key events: magnitude >= 4
    let key_events: Vec<KeyEvent> = sorted_entries
        .iter()
        .filter(|e| e.magnitude >= 4)
        .map(|e| KeyEvent {
            date: e.date.clone(),
            event: e.event.clone(),
            source_complex: e.source_complex.clone(),
            direction: e.direction.clone(),
            target_complex: e.target_complex.clone(),
            magnitude: e.magnitude,
        })
        .collect();

    // Trend analysis
    let trend_analysis = compute_trend_analysis(&complex_assessments, &dominant_complex);

    // Summary text
    let summary = build_summary(
        &complex_assessments,
        &dominant_complex,
        &trend_analysis,
        &power_shifts,
        days,
        sorted_entries.len(),
    );

    Ok(AssessOutput {
        period_days: days,
        total_events: sorted_entries.len(),
        complexes: complex_assessments,
        dominant_complex,
        power_shifts,
        key_events,
        trend_analysis,
        summary,
    })
}

pub fn run_assess(
    backend: &BackendConnection,
    days: usize,
    complex_filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let output = build_assess_output(backend, days)?;

    // Apply complex filter for display if specified
    let filtered_assessments: Vec<&ComplexAssessment> = if let Some(filter) = complex_filter {
        output
            .complexes
            .iter()
            .filter(|c| c.complex == filter)
            .collect()
    } else {
        output.complexes.iter().collect()
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("POWER FLOW ASSESSMENT (last {} days)", days);
        println!("{}", "═".repeat(70));
        println!();

        for ca in &filtered_assessments {
            let sign = if ca.net_score >= 0 { "+" } else { "" };
            let trend_icon = match ca.trend.as_str() {
                "ascending" => "↑",
                "descending" => "↓",
                "volatile" => "↕",
                _ => "→",
            };
            println!(
                "  {} {}  Net: {}{}  (gaining: {} events/{}mag, losing: {} events/{}mag)",
                ca.complex,
                trend_icon,
                sign,
                ca.net_score,
                ca.gaining_events,
                ca.gaining_magnitude,
                ca.losing_events,
                ca.losing_magnitude,
            );
            println!(
                "         Trend: {} (1st half: {}{}, 2nd half: {}{})",
                ca.trend,
                if ca.first_half_net >= 0 { "+" } else { "" },
                ca.first_half_net,
                if ca.second_half_net >= 0 { "+" } else { "" },
                ca.second_half_net,
            );
            if !ca.top_events.is_empty() {
                println!("         Top events:");
                for event in &ca.top_events {
                    println!("           • {}", event);
                }
            }
            println!();
        }

        if complex_filter.is_none() {
            if !output.power_shifts.is_empty() {
                println!("POWER SHIFTS");
                println!("{}", "─".repeat(70));
                for ps in &output.power_shifts {
                    println!(
                        "  {} — {} events, total mag {} (latest: {})",
                        ps.flow, ps.event_count, ps.total_magnitude, ps.latest_date
                    );
                }
                println!();
            }

            if !output.key_events.is_empty() {
                println!("KEY EVENTS (magnitude ≥ 4)");
                println!("{}", "─".repeat(70));
                for ke in &output.key_events {
                    let target_str = ke
                        .target_complex
                        .as_deref()
                        .map(|t| format!(" → {}", t))
                        .unwrap_or_default();
                    println!(
                        "  [{}] {} {} (mag {}){} — {}",
                        ke.date,
                        ke.source_complex,
                        ke.direction,
                        ke.magnitude,
                        target_str,
                        ke.event,
                    );
                }
                println!();
            }

            println!("REGIME: {}", output.trend_analysis.regime);
            println!(
                "Concentration: {:.0}%{}",
                output.trend_analysis.concentration * 100.0,
                if output.trend_analysis.regime_shift_detected {
                    format!(
                        " — SHIFT DETECTED: {}",
                        output
                            .trend_analysis
                            .shift_description
                            .as_deref()
                            .unwrap_or("unknown")
                    )
                } else {
                    String::new()
                }
            );
            println!();
            println!("{}", output.summary);
        }
    }

    Ok(())
}

fn compute_half_net(entries: &[power_flows::PowerFlowEntry], complex: &str) -> i64 {
    let mut net: i64 = 0;
    for entry in entries {
        if entry.source_complex == complex {
            if entry.direction == "gaining" {
                net += entry.magnitude as i64;
            } else {
                net -= entry.magnitude as i64;
            }
        }
        if entry.target_complex.as_deref() == Some(complex) {
            // Inverse: if source is gaining from target, target is losing
            if entry.direction == "gaining" {
                net -= entry.magnitude as i64;
            } else {
                net += entry.magnitude as i64;
            }
        }
    }
    net
}

fn classify_trend(first_half_net: i64, second_half_net: i64, total_net: i64) -> String {
    if first_half_net == 0 && second_half_net == 0 {
        return "stable".to_string();
    }

    let diff = second_half_net - first_half_net;

    // Check for volatility: opposite signs in halves
    if (first_half_net > 0 && second_half_net < 0)
        || (first_half_net < 0 && second_half_net > 0)
    {
        return "volatile".to_string();
    }

    if diff > 0 && total_net >= 0 {
        "ascending".to_string()
    } else if diff < 0 && total_net <= 0 {
        "descending".to_string()
    } else if diff.abs() <= 1 {
        "stable".to_string()
    } else if diff > 0 {
        "ascending".to_string()
    } else {
        "descending".to_string()
    }
}

fn compute_trend_analysis(
    assessments: &[ComplexAssessment],
    dominant: &Option<String>,
) -> TrendAnalysis {
    let total_abs: i64 = assessments.iter().map(|a| a.net_score.abs()).sum();

    let concentration = if total_abs == 0 {
        0.0
    } else {
        let max_abs = assessments
            .iter()
            .map(|a| a.net_score.abs())
            .max()
            .unwrap_or(0);
        max_abs as f64 / total_abs as f64
    };

    let regime = match dominant {
        Some(d) if concentration > 0.5 => format!("{}-dominant", d),
        Some(_) => "contested".to_string(),
        None => {
            if total_abs == 0 {
                "no-data".to_string()
            } else {
                "contested".to_string()
            }
        }
    };

    // Detect regime shift: a complex whose first half was negative and second half is positive (or vice versa)
    let mut regime_shift_detected = false;
    let mut shift_description = None;

    for assessment in assessments {
        if assessment.first_half_net < 0 && assessment.second_half_net > 2 {
            regime_shift_detected = true;
            shift_description = Some(format!(
                "{} reversed from losing ({}) to gaining ({})",
                assessment.complex,
                assessment.first_half_net,
                assessment.second_half_net,
            ));
            break;
        }
        if assessment.first_half_net > 0 && assessment.second_half_net < -2 {
            regime_shift_detected = true;
            shift_description = Some(format!(
                "{} reversed from gaining ({}) to losing ({})",
                assessment.complex,
                assessment.first_half_net,
                assessment.second_half_net,
            ));
            break;
        }
    }

    TrendAnalysis {
        regime,
        concentration: (concentration * 1000.0).round() / 1000.0,
        regime_shift_detected,
        shift_description,
    }
}

fn build_summary(
    assessments: &[ComplexAssessment],
    dominant: &Option<String>,
    trend: &TrendAnalysis,
    shifts: &[PowerShift],
    days: usize,
    total_events: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!(
        "{} power flow events logged over {} days.",
        total_events, days
    ));

    match dominant {
        Some(d) => {
            let ca = assessments.iter().find(|a| a.complex == *d);
            if let Some(ca) = ca {
                parts.push(format!(
                    "{} is the dominant complex (net +{}, {} trending).",
                    d, ca.net_score, ca.trend
                ));
            }
        }
        None => {
            parts.push("No single complex is dominant — power is contested.".to_string());
        }
    }

    if trend.regime_shift_detected {
        if let Some(desc) = &trend.shift_description {
            parts.push(format!("Regime shift detected: {}", desc));
        }
    }

    if let Some(biggest_shift) = shifts.first() {
        parts.push(format!(
            "Largest power flow: {} ({} events, total magnitude {}).",
            biggest_shift.flow,
            biggest_shift.event_count,
            biggest_shift.total_magnitude
        ));
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_trend_ascending() {
        assert_eq!(classify_trend(1, 5, 6), "ascending");
    }

    #[test]
    fn test_classify_trend_descending() {
        assert_eq!(classify_trend(-1, -5, -6), "descending");
    }

    #[test]
    fn test_classify_trend_stable() {
        assert_eq!(classify_trend(0, 0, 0), "stable");
        assert_eq!(classify_trend(3, 3, 6), "stable");
    }

    #[test]
    fn test_classify_trend_volatile() {
        assert_eq!(classify_trend(5, -3, 2), "volatile");
        assert_eq!(classify_trend(-4, 2, -2), "volatile");
    }

    #[test]
    fn test_compute_half_net_basic() {
        let entries = vec![
            power_flows::PowerFlowEntry {
                id: 1,
                date: "2026-03-20".to_string(),
                event: "Test event".to_string(),
                source_complex: "FIC".to_string(),
                direction: "gaining".to_string(),
                target_complex: Some("MIC".to_string()),
                evidence: "test".to_string(),
                magnitude: 4,
                agent_source: None,
                created_at: String::new(),
            },
            power_flows::PowerFlowEntry {
                id: 2,
                date: "2026-03-21".to_string(),
                event: "Test event 2".to_string(),
                source_complex: "FIC".to_string(),
                direction: "losing".to_string(),
                target_complex: None,
                evidence: "test".to_string(),
                magnitude: 2,
                agent_source: None,
                created_at: String::new(),
            },
        ];

        // FIC: gaining +4, losing -2 = net +2
        assert_eq!(compute_half_net(&entries, "FIC"), 2);
        // MIC: target of FIC gaining = MIC losing -4
        assert_eq!(compute_half_net(&entries, "MIC"), -4);
        // TIC: not involved
        assert_eq!(compute_half_net(&entries, "TIC"), 0);
    }

    #[test]
    fn test_compute_trend_analysis_dominant() {
        let assessments = vec![
            ComplexAssessment {
                complex: "FIC".to_string(),
                net_score: 10,
                gaining_events: 3,
                losing_events: 0,
                gaining_magnitude: 10,
                losing_magnitude: 0,
                trend: "ascending".to_string(),
                first_half_net: 3,
                second_half_net: 7,
                avg_magnitude: 3.33,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "MIC".to_string(),
                net_score: -5,
                gaining_events: 0,
                losing_events: 2,
                gaining_magnitude: 0,
                losing_magnitude: 5,
                trend: "descending".to_string(),
                first_half_net: -2,
                second_half_net: -3,
                avg_magnitude: 2.5,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "TIC".to_string(),
                net_score: 0,
                gaining_events: 0,
                losing_events: 0,
                gaining_magnitude: 0,
                losing_magnitude: 0,
                trend: "stable".to_string(),
                first_half_net: 0,
                second_half_net: 0,
                avg_magnitude: 0.0,
                top_events: vec![],
            },
        ];

        let dominant = Some("FIC".to_string());
        let trend = compute_trend_analysis(&assessments, &dominant);

        assert_eq!(trend.regime, "FIC-dominant");
        assert!(trend.concentration > 0.5);
        assert!(!trend.regime_shift_detected);
    }

    #[test]
    fn test_compute_trend_analysis_contested() {
        let assessments = vec![
            ComplexAssessment {
                complex: "FIC".to_string(),
                net_score: 5,
                gaining_events: 2,
                losing_events: 1,
                gaining_magnitude: 7,
                losing_magnitude: 2,
                trend: "ascending".to_string(),
                first_half_net: 2,
                second_half_net: 3,
                avg_magnitude: 3.0,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "MIC".to_string(),
                net_score: 4,
                gaining_events: 2,
                losing_events: 0,
                gaining_magnitude: 4,
                losing_magnitude: 0,
                trend: "ascending".to_string(),
                first_half_net: 1,
                second_half_net: 3,
                avg_magnitude: 2.0,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "TIC".to_string(),
                net_score: 3,
                gaining_events: 1,
                losing_events: 0,
                gaining_magnitude: 3,
                losing_magnitude: 0,
                trend: "stable".to_string(),
                first_half_net: 1,
                second_half_net: 2,
                avg_magnitude: 3.0,
                top_events: vec![],
            },
        ];

        // FIC has most but 5/12 = ~0.42 concentration — contested
        let dominant = Some("FIC".to_string());
        let trend = compute_trend_analysis(&assessments, &dominant);

        assert_eq!(trend.regime, "contested");
    }

    #[test]
    fn test_compute_trend_analysis_regime_shift() {
        let assessments = vec![
            ComplexAssessment {
                complex: "FIC".to_string(),
                net_score: 3,
                gaining_events: 2,
                losing_events: 1,
                gaining_magnitude: 6,
                losing_magnitude: 3,
                trend: "ascending".to_string(),
                first_half_net: -3,
                second_half_net: 6,
                avg_magnitude: 3.0,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "MIC".to_string(),
                net_score: -2,
                gaining_events: 0,
                losing_events: 1,
                gaining_magnitude: 0,
                losing_magnitude: 2,
                trend: "descending".to_string(),
                first_half_net: -1,
                second_half_net: -1,
                avg_magnitude: 2.0,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "TIC".to_string(),
                net_score: 0,
                gaining_events: 0,
                losing_events: 0,
                gaining_magnitude: 0,
                losing_magnitude: 0,
                trend: "stable".to_string(),
                first_half_net: 0,
                second_half_net: 0,
                avg_magnitude: 0.0,
                top_events: vec![],
            },
        ];

        let dominant = Some("FIC".to_string());
        let trend = compute_trend_analysis(&assessments, &dominant);

        assert!(trend.regime_shift_detected);
        assert!(trend.shift_description.unwrap().contains("FIC reversed"));
    }

    #[test]
    fn test_compute_trend_analysis_no_data() {
        let assessments = vec![
            ComplexAssessment {
                complex: "FIC".to_string(),
                net_score: 0,
                gaining_events: 0,
                losing_events: 0,
                gaining_magnitude: 0,
                losing_magnitude: 0,
                trend: "stable".to_string(),
                first_half_net: 0,
                second_half_net: 0,
                avg_magnitude: 0.0,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "MIC".to_string(),
                net_score: 0,
                gaining_events: 0,
                losing_events: 0,
                gaining_magnitude: 0,
                losing_magnitude: 0,
                trend: "stable".to_string(),
                first_half_net: 0,
                second_half_net: 0,
                avg_magnitude: 0.0,
                top_events: vec![],
            },
            ComplexAssessment {
                complex: "TIC".to_string(),
                net_score: 0,
                gaining_events: 0,
                losing_events: 0,
                gaining_magnitude: 0,
                losing_magnitude: 0,
                trend: "stable".to_string(),
                first_half_net: 0,
                second_half_net: 0,
                avg_magnitude: 0.0,
                top_events: vec![],
            },
        ];

        let dominant = None;
        let trend = compute_trend_analysis(&assessments, &dominant);

        assert_eq!(trend.regime, "no-data");
        assert_eq!(trend.concentration, 0.0);
    }

    #[test]
    fn test_build_summary_with_dominant() {
        let assessments = vec![ComplexAssessment {
            complex: "FIC".to_string(),
            net_score: 8,
            gaining_events: 3,
            losing_events: 0,
            gaining_magnitude: 8,
            losing_magnitude: 0,
            trend: "ascending".to_string(),
            first_half_net: 2,
            second_half_net: 6,
            avg_magnitude: 2.67,
            top_events: vec![],
        }];
        let dominant = Some("FIC".to_string());
        let trend = TrendAnalysis {
            regime: "FIC-dominant".to_string(),
            concentration: 0.8,
            regime_shift_detected: false,
            shift_description: None,
        };
        let shifts = vec![];

        let summary = build_summary(&assessments, &dominant, &trend, &shifts, 7, 5);
        assert!(summary.contains("5 power flow events"));
        assert!(summary.contains("7 days"));
        assert!(summary.contains("FIC is the dominant complex"));
    }

    #[test]
    fn test_build_summary_no_dominant() {
        let assessments = vec![];
        let dominant = None;
        let trend = TrendAnalysis {
            regime: "contested".to_string(),
            concentration: 0.33,
            regime_shift_detected: false,
            shift_description: None,
        };
        let shifts = vec![];

        let summary = build_summary(&assessments, &dominant, &trend, &shifts, 7, 3);
        assert!(summary.contains("contested"));
    }
}
