use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

use crate::analytics::catalysts;
use crate::analytics::impact;
use crate::analytics::situation::{self, TimeframeScore};
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::{convictions, power_flows, regime_snapshots, scenarios, trends, watchlist};
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;

#[derive(Debug, Clone, Serialize)]
pub struct SynthesisReport {
    pub generated_at: String,
    pub strongest_alignment: Vec<AlignmentState>,
    pub highest_confidence_divergence: Vec<DivergenceState>,
    pub constraint_flows: Vec<ConstraintState>,
    pub power_structure: Option<PowerStructureContext>,
    pub unresolved_tensions: Vec<SynthesisNote>,
    pub watch_tomorrow: Vec<WatchTomorrowCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerStructureContext {
    pub period_days: usize,
    pub total_events: usize,
    pub complexes: Vec<ComplexSummary>,
    pub dominant_complex: Option<String>,
    pub regime_classification: String,
    pub concentration: f64,
    pub regime_shift_detected: bool,
    pub shift_description: Option<String>,
    pub regime_overlay: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComplexSummary {
    pub complex: String,
    pub net_score: i64,
    pub trend: String,
    pub gaining_events: usize,
    pub losing_events: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlignmentState {
    pub symbol: String,
    pub name: String,
    pub low: String,
    pub medium: String,
    pub high: String,
    pub macro_bias: String,
    pub consensus: String,
    pub score_pct: f64,
    pub bull_layers: usize,
    pub bear_layers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DivergenceState {
    pub symbol: String,
    pub name: String,
    pub low: String,
    pub medium: String,
    pub high: String,
    pub macro_bias: String,
    pub dominant_side: String,
    pub disagreement_pct: f64,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstraintState {
    pub title: String,
    pub from_timeframe: String,
    pub to_timeframe: String,
    pub direction: String,
    pub severity: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SynthesisNote {
    pub title: String,
    pub detail: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchTomorrowCandidate {
    pub symbol: String,
    pub name: String,
    pub reason: String,
    pub trigger: String,
    pub severity: String,
}

pub fn build_report_backend(backend: &BackendConnection) -> Result<SynthesisReport> {
    let inputs = situation::collect_inputs_backend(backend)?;
    let rows = build_alignment_rows_backend(backend)?;
    let strongest_alignment = strongest_alignment(&rows);
    let divergences = highest_confidence_divergence(&rows);
    let constraint_flows = constraint_flows(&inputs.timeframes);
    let power_structure = build_power_structure(backend, &constraint_flows);
    let unresolved_tensions =
        unresolved_tensions_with_power(&constraint_flows, &divergences, &power_structure);
    let watch_tomorrow = watch_tomorrow_candidates(backend, &rows)?;

    Ok(SynthesisReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        strongest_alignment,
        highest_confidence_divergence: divergences,
        constraint_flows,
        power_structure,
        unresolved_tensions,
        watch_tomorrow,
    })
}

const POWER_STRUCTURE_DAYS: usize = 7;

fn build_power_structure(
    backend: &BackendConnection,
    constraint_flows: &[ConstraintState],
) -> Option<PowerStructureContext> {
    let entries =
        power_flows::list_power_flows_backend(backend, None, None, POWER_STRUCTURE_DAYS)
            .unwrap_or_default();
    let balances =
        power_flows::compute_balance_backend(backend, POWER_STRUCTURE_DAYS).unwrap_or_default();

    if entries.is_empty() {
        return None;
    }

    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));
    let midpoint = sorted.len() / 2;
    let first_half = &sorted[..midpoint];
    let second_half = &sorted[midpoint..];

    let complex_names = ["FIC", "MIC", "TIC"];
    let mut complexes = Vec::new();
    let mut nets: Vec<(String, i64)> = Vec::new();

    for name in &complex_names {
        let balance = balances.iter().find(|b| b.complex == *name);
        let (gaining_ev, losing_ev, gaining_mag, losing_mag) = match balance {
            Some(b) => (
                b.gaining_count as usize,
                b.losing_count as usize,
                b.gaining_magnitude,
                b.losing_magnitude,
            ),
            None => (0, 0, 0, 0),
        };
        let net = gaining_mag - losing_mag;
        let fh = compute_half_net_for_complex(first_half, name);
        let sh = compute_half_net_for_complex(second_half, name);
        let trend = classify_power_trend(fh, sh, net);

        nets.push((name.to_string(), net));
        complexes.push(ComplexSummary {
            complex: name.to_string(),
            net_score: net,
            trend,
            gaining_events: gaining_ev,
            losing_events: losing_ev,
        });
    }

    let dominant_complex = nets
        .iter()
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c.clone());

    // Regime classification
    let positive: Vec<_> = nets.iter().filter(|(_, n)| *n > 0).collect();
    let regime_classification = if positive.len() >= 2 {
        "contested".to_string()
    } else if let Some(dom) = &dominant_complex {
        format!("{}-dominant", dom.to_lowercase())
    } else {
        "no-clear-dominant".to_string()
    };

    // Concentration: how much of total power is held by the dominant complex
    let total_positive: i64 = nets.iter().map(|(_, n)| (*n).max(0)).sum();
    let max_positive = nets.iter().map(|(_, n)| (*n).max(0)).max().unwrap_or(0);
    let concentration = if total_positive > 0 {
        max_positive as f64 / total_positive as f64
    } else {
        0.0
    };

    // Regime shift detection: a complex was losing in first half but gaining in second half
    let mut regime_shift_detected = false;
    let mut shift_description = None;
    for name in &complex_names {
        let fh = compute_half_net_for_complex(first_half, name);
        let sh = compute_half_net_for_complex(second_half, name);
        if (fh < 0 && sh > 0) || (fh > 0 && sh < 0) {
            regime_shift_detected = true;
            let direction = if fh < 0 && sh > 0 {
                "losing → gaining"
            } else {
                "gaining → losing"
            };
            shift_description =
                Some(format!("{} reversed from {} over {}-day window", name, direction, POWER_STRUCTURE_DAYS));
            break;
        }
    }

    // Regime overlay: cross-reference power structure with constraint flows
    let regime_overlay = build_regime_overlay(&complexes, constraint_flows, &dominant_complex);

    Some(PowerStructureContext {
        period_days: POWER_STRUCTURE_DAYS,
        total_events: entries.len(),
        complexes,
        dominant_complex,
        regime_classification,
        concentration: (concentration * 1000.0).round() / 1000.0,
        regime_shift_detected,
        shift_description,
        regime_overlay,
    })
}

fn compute_half_net_for_complex(entries: &[power_flows::PowerFlowEntry], complex: &str) -> i64 {
    let mut net: i64 = 0;
    for e in entries {
        if e.source_complex == complex {
            if e.direction == "gaining" {
                net += e.magnitude as i64;
            } else {
                net -= e.magnitude as i64;
            }
        }
    }
    net
}

fn classify_power_trend(first_half_net: i64, second_half_net: i64, total_net: i64) -> String {
    if first_half_net == 0 && second_half_net == 0 {
        return "stable".to_string();
    }
    let diff = second_half_net - first_half_net;
    let scale = total_net.unsigned_abs().max(1);
    let ratio = diff.unsigned_abs() as f64 / scale as f64;

    if ratio < 0.25 {
        "stable".to_string()
    } else if diff > 0 && second_half_net > 0 {
        "ascending".to_string()
    } else if diff < 0 && second_half_net < 0 {
        "descending".to_string()
    } else {
        "volatile".to_string()
    }
}

fn build_regime_overlay(
    complexes: &[ComplexSummary],
    constraint_flows: &[ConstraintState],
    dominant_complex: &Option<String>,
) -> Option<String> {
    // If there are constraint flows AND a dominant complex, generate an overlay narrative
    if constraint_flows.is_empty() {
        return dominant_complex.as_ref().map(|dom| {
            let dom_summary = complexes.iter().find(|c| c.complex == *dom);
            let trend = dom_summary
                .map(|s| s.trend.as_str())
                .unwrap_or("stable");
            format!(
                "{} dominant ({}), no cross-timeframe constraints detected",
                dom, trend
            )
        });
    }

    let critical_constraints: Vec<_> = constraint_flows
        .iter()
        .filter(|c| c.severity == "critical")
        .collect();

    let has_shift = complexes.iter().any(|c| c.trend == "ascending" || c.trend == "descending");

    match (dominant_complex, critical_constraints.is_empty(), has_shift) {
        (Some(dom), false, true) => Some(format!(
            "{} dominant but critical constraint flows + power shift in progress — regime transition likely",
            dom
        )),
        (Some(dom), false, false) => Some(format!(
            "{} dominant with critical cross-timeframe constraints — watch for resolution",
            dom
        )),
        (Some(dom), true, true) => Some(format!(
            "{} dominant with power structure shift underway — monitor for acceleration",
            dom
        )),
        (Some(dom), true, false) => Some(format!(
            "{} dominant, stable power structure, no critical constraints",
            dom
        )),
        (None, false, _) => Some(
            "No dominant complex + critical constraints — high uncertainty regime".to_string(),
        ),
        (None, true, true) => Some(
            "No dominant complex but power shifts underway — emerging regime".to_string(),
        ),
        (None, true, false) => None,
    }
}

#[derive(Debug, Clone)]
struct AlignmentRow {
    symbol: String,
    name: String,
    low: String,
    medium: String,
    high: String,
    macro_bias: String,
    consensus: String,
    score_pct: f64,
    bull_layers: usize,
    bear_layers: usize,
}

fn build_alignment_rows_backend(backend: &BackendConnection) -> Result<Vec<AlignmentRow>> {
    let symbols = discover_symbols(backend);
    let low_regime = regime_snapshots::get_current_backend(backend).unwrap_or(None);
    let low_bias = low_regime
        .as_ref()
        .map(|r| regime_to_bias(&r.regime).to_string())
        .unwrap_or_else(|| "neutral".to_string());
    let low_conf = low_regime
        .as_ref()
        .and_then(|r| r.confidence)
        .map(normalize_confidence)
        .unwrap_or(0.5);

    let conviction_rows = convictions::list_current_backend(backend).unwrap_or_default();
    let conviction_bias_map: HashMap<String, String> = conviction_rows
        .iter()
        .map(|c| (c.symbol.to_uppercase(), bias_from_score(c.score)))
        .collect();
    let conviction_score_map: HashMap<String, f64> = conviction_rows
        .iter()
        .map(|c| {
            (
                c.symbol.to_uppercase(),
                (c.score as f64 / 5.0).clamp(-1.0, 1.0),
            )
        })
        .collect();

    let scenarios_list =
        scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let impact_rows = trends::list_all_impacts_backend(backend).unwrap_or_default();
    let mut impacts_by_symbol: HashMap<String, Vec<(trends::Trend, trends::TrendAssetImpact)>> =
        HashMap::new();
    for (trend, impact) in impact_rows {
        impacts_by_symbol
            .entry(impact.symbol.to_uppercase())
            .or_default()
            .push((trend, impact));
    }

    let mut rows = Vec::new();
    for sym in symbols {
        let medium = conviction_bias_map
            .get(&sym)
            .cloned()
            .unwrap_or_else(|| "neutral".to_string());
        let medium_signal = conviction_score_map.get(&sym).copied().unwrap_or(0.0);

        let high_impacts = impacts_by_symbol.get(&sym).cloned().unwrap_or_default();
        let bull_high = high_impacts
            .iter()
            .filter(|(_, i)| i.impact.eq_ignore_ascii_case("bullish"))
            .count();
        let bear_high = high_impacts
            .iter()
            .filter(|(_, i)| i.impact.eq_ignore_ascii_case("bearish"))
            .count();
        let high = if bull_high > bear_high {
            "bull"
        } else if bear_high > bull_high {
            "bear"
        } else {
            "neutral"
        }
        .to_string();
        let high_total = bull_high + bear_high;
        let high_signal = if high_total == 0 {
            0.0
        } else {
            (bull_high as f64 - bear_high as f64) / high_total as f64
        };

        let macro_signal = scenario_bias_for_symbol(&sym, &scenarios_list);
        let macro_bias = if macro_signal > 0.05 {
            "bull".to_string()
        } else if macro_signal < -0.05 {
            "bear".to_string()
        } else {
            "neutral".to_string()
        };

        let low_signal = bias_to_signal(&low_bias) * low_conf;
        let bull = [low_signal, medium_signal, high_signal, macro_signal]
            .iter()
            .filter(|v| **v > 0.05)
            .count();
        let bear = [low_signal, medium_signal, high_signal, macro_signal]
            .iter()
            .filter(|v| **v < -0.05)
            .count();
        let consensus = consensus_from_counts(bull, bear);
        let weighted = (0.20 * low_signal)
            + (0.30 * medium_signal)
            + (0.25 * high_signal)
            + (0.25 * macro_signal);
        let score_pct = (weighted.abs() * 100.0).clamp(0.0, 100.0);

        rows.push(AlignmentRow {
            symbol: sym.clone(),
            name: resolve_name(&sym),
            low: low_bias.clone(),
            medium,
            high,
            macro_bias,
            consensus,
            score_pct,
            bull_layers: bull,
            bear_layers: bear,
        });
    }
    Ok(rows)
}

fn strongest_alignment(rows: &[AlignmentRow]) -> Vec<AlignmentState> {
    let mut out = rows
        .iter()
        .filter(|row| row.bull_layers >= 3 || row.bear_layers >= 3)
        .map(|row| AlignmentState {
            symbol: row.symbol.clone(),
            name: row.name.clone(),
            low: row.low.clone(),
            medium: row.medium.clone(),
            high: row.high.clone(),
            macro_bias: row.macro_bias.clone(),
            consensus: row.consensus.clone(),
            score_pct: row.score_pct,
            bull_layers: row.bull_layers,
            bear_layers: row.bear_layers,
        })
        .collect::<Vec<_>>();
    if out.is_empty() {
        out = rows
            .iter()
            .filter(|row| row.score_pct > 0.0)
            .map(|row| AlignmentState {
                symbol: row.symbol.clone(),
                name: row.name.clone(),
                low: row.low.clone(),
                medium: row.medium.clone(),
                high: row.high.clone(),
                macro_bias: row.macro_bias.clone(),
                consensus: row.consensus.clone(),
                score_pct: row.score_pct,
                bull_layers: row.bull_layers,
                bear_layers: row.bear_layers,
            })
            .collect::<Vec<_>>();
    }
    out.sort_by(|a, b| {
        b.score_pct
            .partial_cmp(&a.score_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    out.truncate(5);
    out
}

fn highest_confidence_divergence(rows: &[AlignmentRow]) -> Vec<DivergenceState> {
    let mut out = rows
        .iter()
        .filter(|row| row.bull_layers > 0 && row.bear_layers > 0)
        .map(|row| {
            let dominant_side = if row.bull_layers > row.bear_layers {
                "bull"
            } else if row.bear_layers > row.bull_layers {
                "bear"
            } else {
                "split"
            }
            .to_string();
            let disagreement_pct = (row.bull_layers.min(row.bear_layers) as f64 / 4.0) * 100.0;
            DivergenceState {
                symbol: row.symbol.clone(),
                name: row.name.clone(),
                low: row.low.clone(),
                medium: row.medium.clone(),
                high: row.high.clone(),
                macro_bias: row.macro_bias.clone(),
                dominant_side: dominant_side.clone(),
                disagreement_pct,
                summary: format!(
                    "{} has {} low/medium/high/macro disagreement (bull {} vs bear {}).",
                    row.symbol, dominant_side, row.bull_layers, row.bear_layers
                ),
            }
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| {
        b.disagreement_pct
            .partial_cmp(&a.disagreement_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    out.truncate(5);
    out
}

fn constraint_flows(timeframes: &[TimeframeScore]) -> Vec<ConstraintState> {
    let ordered = ["macro", "high", "medium", "low"]
        .iter()
        .filter_map(|key| timeframes.iter().find(|row| row.timeframe == *key))
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for pair in ordered.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let from_bias = score_bias(from.score);
        let to_bias = score_bias(to.score);
        if from_bias == "mixed" || to_bias == "mixed" || from_bias == to_bias {
            continue;
        }
        let severity = if from.score.abs() >= 35.0 && to.score.abs() >= 35.0 {
            "critical"
        } else {
            "elevated"
        };
        out.push(ConstraintState {
            title: format!("{} constrains {}", from.label, to.label),
            from_timeframe: from.timeframe.clone(),
            to_timeframe: to.timeframe.clone(),
            direction: format!("{from_bias} -> {to_bias}"),
            severity: severity.to_string(),
            summary: format!(
                "{} is {} ({:.0}) while {} is {} ({:.0}).",
                from.label, from_bias, from.score, to.label, to_bias, to.score
            ),
        });
    }
    out
}

fn unresolved_tensions_with_power(
    constraints: &[ConstraintState],
    divergences: &[DivergenceState],
    power_structure: &Option<PowerStructureContext>,
) -> Vec<SynthesisNote> {
    let mut out = Vec::new();
    for constraint in constraints.iter().take(3) {
        out.push(SynthesisNote {
            title: constraint.title.clone(),
            detail: constraint.summary.clone(),
            severity: constraint.severity.clone(),
        });
    }
    for divergence in divergences.iter().take(2) {
        out.push(SynthesisNote {
            title: format!("Divergence: {}", divergence.symbol),
            detail: divergence.summary.clone(),
            severity: if divergence.disagreement_pct >= 50.0 {
                "critical".to_string()
            } else {
                "elevated".to_string()
            },
        });
    }

    // Power structure tensions
    if let Some(ps) = power_structure {
        if ps.regime_shift_detected {
            out.push(SynthesisNote {
                title: "Power structure shift".to_string(),
                detail: ps
                    .shift_description
                    .clone()
                    .unwrap_or_else(|| "Regime shift detected in FIC/MIC/TIC balance".to_string()),
                severity: "critical".to_string(),
            });
        }
        if ps.regime_classification == "contested" {
            out.push(SynthesisNote {
                title: "Contested power structure".to_string(),
                detail: format!(
                    "Multiple complexes gaining simultaneously — {} events in {} days, no clear dominant",
                    ps.total_events, ps.period_days
                ),
                severity: "elevated".to_string(),
            });
        }
    }

    out
}

fn watch_tomorrow_candidates(
    backend: &BackendConnection,
    rows: &[AlignmentRow],
) -> Result<Vec<WatchTomorrowCandidate>> {
    let catalyst_report =
        catalysts::build_report_backend(backend, catalysts::CatalystWindow::Tomorrow)?;
    let opportunities = impact::build_opportunities_report_backend(backend)?;
    let mut candidates = Vec::new();

    for catalyst in catalyst_report.catalysts.iter().take(5) {
        let symbol = catalyst
            .affected_assets
            .first()
            .cloned()
            .unwrap_or_else(|| "SPY".to_string());
        let alignment = rows.iter().find(|row| row.symbol == symbol);
        let severity = if catalyst.significance.eq_ignore_ascii_case("high") {
            "critical"
        } else {
            "elevated"
        };
        candidates.push(WatchTomorrowCandidate {
            symbol: symbol.clone(),
            name: resolve_name(&symbol),
            reason: format!(
                "{} catalyst with {} consensus",
                catalyst.title,
                alignment
                    .map(|row| row.consensus.clone())
                    .unwrap_or_else(|| "mixed".to_string())
            ),
            trigger: catalyst.countdown_bucket.clone(),
            severity: severity.to_string(),
        });
    }

    for item in opportunities.opportunities.iter().take(3) {
        candidates.push(WatchTomorrowCandidate {
            symbol: item.symbol.clone(),
            name: item.name.clone(),
            reason: item.summary.clone(),
            trigger: item
                .evidence_chain
                .first()
                .cloned()
                .unwrap_or_else(|| "alignment".to_string()),
            severity: item.severity.clone(),
        });
    }

    candidates.sort_by(|a, b| {
        severity_weight(&b.severity)
            .cmp(&severity_weight(&a.severity))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    candidates.dedup_by(|a, b| a.symbol == b.symbol);
    candidates.truncate(6);
    Ok(candidates)
}

fn discover_symbols(backend: &BackendConnection) -> Vec<String> {
    let mut symbols: BTreeSet<String> = BTreeSet::new();
    if let Ok(rows) = db::transactions::get_unique_symbols_backend(backend) {
        for (symbol, category) in rows {
            if category != AssetCategory::Cash {
                symbols.insert(symbol.to_uppercase());
            }
        }
    }
    if let Ok(rows) = watchlist::list_watchlist_backend(backend) {
        for row in rows {
            if !row.category.eq_ignore_ascii_case("cash") {
                symbols.insert(row.symbol.to_uppercase());
            }
        }
    }
    symbols.into_iter().collect()
}

fn regime_to_bias(regime: &str) -> &'static str {
    match regime {
        "risk-on" => "bull",
        "risk-off" | "crisis" | "stagflation" => "bear",
        _ => "neutral",
    }
}

fn bias_from_score(score: i32) -> String {
    if score > 0 {
        "bull".to_string()
    } else if score < 0 {
        "bear".to_string()
    } else {
        "neutral".to_string()
    }
}

fn bias_to_signal(bias: &str) -> f64 {
    match bias {
        "bull" => 1.0,
        "bear" => -1.0,
        _ => 0.0,
    }
}

fn consensus_from_counts(bull: usize, bear: usize) -> String {
    if bull == 4 {
        "STRONG BUY".to_string()
    } else if bear == 4 {
        "STRONG AVOID".to_string()
    } else if bull >= 3 {
        "BULLISH".to_string()
    } else if bear >= 3 {
        "BEARISH".to_string()
    } else {
        "MIXED".to_string()
    }
}

fn scenario_bias_for_symbol(symbol: &str, scenarios_list: &[db::scenarios::Scenario]) -> f64 {
    let needle = symbol.to_lowercase();
    let name = resolve_name(symbol).to_lowercase();
    let mut macro_signal = 0.0;
    for s in scenarios_list {
        let text = format!(
            "{} {} {} {}",
            s.name,
            s.asset_impact.as_deref().unwrap_or(""),
            s.description.as_deref().unwrap_or(""),
            s.triggers.as_deref().unwrap_or("")
        )
        .to_lowercase();
        if !(text.contains(&needle) || (!name.is_empty() && text.contains(&name))) {
            continue;
        }
        let direction = if text.contains("bull") && !text.contains("bear") {
            1.0
        } else if text.contains("bear") && !text.contains("bull") {
            -1.0
        } else {
            0.0
        };
        macro_signal += direction * (s.probability / 100.0);
    }
    macro_signal.clamp(-1.0, 1.0)
}

fn normalize_confidence(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
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

fn severity_weight(raw: &str) -> i32 {
    match raw {
        "critical" => 3,
        "elevated" => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::predictions::{MarketCategory, PredictionMarket};
    use crate::db::backend::BackendConnection;
    use crate::db::technical_signals::NewSignal;
    use crate::models::asset::AssetCategory;
    use crate::models::price::PriceQuote;
    use crate::models::transaction::{NewTransaction, TxType};
    use rust_decimal_macros::dec;

    #[test]
    fn classifies_alignment_and_divergence() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);

        let report = build_report_backend(&backend).unwrap();
        assert!(!report.strongest_alignment.is_empty());
        assert!(!report.highest_confidence_divergence.is_empty());
        assert!(!report.constraint_flows.is_empty());
    }

    #[test]
    fn watch_tomorrow_prioritizes_catalysts_and_opportunities() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);

        let report = build_report_backend(&backend).unwrap();
        assert!(!report.watch_tomorrow.is_empty());
    }

    #[test]
    fn power_structure_none_when_no_events() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);

        let report = build_report_backend(&backend).unwrap();
        assert!(
            report.power_structure.is_none(),
            "power_structure should be None when no power flow events logged"
        );
    }

    #[test]
    fn power_structure_present_with_events() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);
        seed_power_flows(&backend);

        let report = build_report_backend(&backend).unwrap();
        let ps = report.power_structure.as_ref().expect("power_structure should be Some");
        assert_eq!(ps.period_days, 7);
        assert_eq!(ps.complexes.len(), 3);
        assert!(ps.total_events > 0);
    }

    #[test]
    fn power_structure_detects_dominant_complex() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);
        seed_power_flows(&backend);

        let report = build_report_backend(&backend).unwrap();
        let ps = report.power_structure.as_ref().unwrap();
        assert_eq!(
            ps.dominant_complex.as_deref(),
            Some("FIC"),
            "FIC should dominate with net +5 gaining events"
        );
        assert!(
            ps.regime_classification.contains("fic"),
            "regime should be fic-dominant, got: {}",
            ps.regime_classification
        );
    }

    #[test]
    fn power_structure_regime_shift_detected() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);

        // First half: MIC losing
        let today = chrono::Utc::now().date_naive();
        let day1 = (today - chrono::Duration::days(6)).format("%Y-%m-%d").to_string();
        let day2 = (today - chrono::Duration::days(5)).format("%Y-%m-%d").to_string();
        // Second half: MIC gaining
        let day3 = (today - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
        let day4 = today.format("%Y-%m-%d").to_string();

        power_flows::add_power_flow_backend(
            &backend, &day1, "Defense cut", "MIC", "losing", None, "budget reduction", 3, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            &backend, &day2, "Arms deal cancelled", "MIC", "losing", None, "geopolitical shift", 4, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            &backend, &day3, "New contract", "MIC", "gaining", None, "procurement surge", 5, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            &backend, &day4, "Military expansion", "MIC", "gaining", None, "threat escalation", 4, Some("test"),
        ).unwrap();

        let report = build_report_backend(&backend).unwrap();
        let ps = report.power_structure.as_ref().unwrap();
        assert!(ps.regime_shift_detected, "should detect MIC reversal from losing to gaining");
        assert!(ps.shift_description.as_ref().unwrap().contains("MIC"));
    }

    #[test]
    fn power_structure_contested_regime() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);

        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        power_flows::add_power_flow_backend(
            &backend, &today, "Fiscal expansion", "FIC", "gaining", None, "spending bill", 4, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            &backend, &today, "Arms surge", "MIC", "gaining", None, "defense budget", 4, Some("test"),
        ).unwrap();

        let report = build_report_backend(&backend).unwrap();
        let ps = report.power_structure.as_ref().unwrap();
        assert_eq!(ps.regime_classification, "contested");
    }

    #[test]
    fn power_structure_adds_tension_on_shift() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_synthesis_fixture(&backend);

        let today = chrono::Utc::now().date_naive();
        let early = (today - chrono::Duration::days(5)).format("%Y-%m-%d").to_string();
        let late = today.format("%Y-%m-%d").to_string();

        power_flows::add_power_flow_backend(
            &backend, &early, "Decline", "TIC", "losing", None, "regulation", 4, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            &backend, &late, "Recovery", "TIC", "gaining", None, "AI boom", 5, Some("test"),
        ).unwrap();

        let report = build_report_backend(&backend).unwrap();
        let has_power_tension = report
            .unresolved_tensions
            .iter()
            .any(|t| t.title.contains("Power structure shift"));
        assert!(
            has_power_tension,
            "should have power structure shift tension in unresolved_tensions"
        );
    }

    #[test]
    fn classify_power_trend_stable_when_zero() {
        assert_eq!(classify_power_trend(0, 0, 0), "stable");
    }

    #[test]
    fn classify_power_trend_ascending() {
        assert_eq!(classify_power_trend(1, 5, 6), "ascending");
    }

    #[test]
    fn classify_power_trend_descending() {
        assert_eq!(classify_power_trend(3, -2, 1), "descending");
    }

    #[test]
    fn regime_overlay_with_constraints_and_dominant() {
        let complexes = vec![
            ComplexSummary { complex: "FIC".to_string(), net_score: 5, trend: "ascending".to_string(), gaining_events: 3, losing_events: 0 },
            ComplexSummary { complex: "MIC".to_string(), net_score: -2, trend: "stable".to_string(), gaining_events: 0, losing_events: 1 },
            ComplexSummary { complex: "TIC".to_string(), net_score: 0, trend: "stable".to_string(), gaining_events: 0, losing_events: 0 },
        ];
        let constraints = vec![ConstraintState {
            title: "Macro constrains Low".to_string(),
            from_timeframe: "macro".to_string(),
            to_timeframe: "low".to_string(),
            direction: "defensive -> bullish".to_string(),
            severity: "critical".to_string(),
            summary: "test".to_string(),
        }];
        let dominant = Some("FIC".to_string());
        let overlay = build_regime_overlay(&complexes, &constraints, &dominant);
        assert!(overlay.is_some());
        let text = overlay.unwrap();
        assert!(text.contains("FIC dominant"), "overlay should mention FIC dominant: {}", text);
        assert!(text.contains("regime transition"), "overlay should mention regime transition: {}", text);
    }

    #[test]
    fn power_structure_serializes_to_json() {
        let ps = PowerStructureContext {
            period_days: 7,
            total_events: 4,
            complexes: vec![
                ComplexSummary { complex: "FIC".to_string(), net_score: 5, trend: "ascending".to_string(), gaining_events: 3, losing_events: 0 },
            ],
            dominant_complex: Some("FIC".to_string()),
            regime_classification: "fic-dominant".to_string(),
            concentration: 1.0,
            regime_shift_detected: false,
            shift_description: None,
            regime_overlay: Some("FIC dominant, stable power structure, no critical constraints".to_string()),
        };
        let json = serde_json::to_string(&ps).unwrap();
        assert!(json.contains("fic-dominant"));
        assert!(json.contains("\"concentration\":1.0"));
    }

    fn seed_power_flows(backend: &BackendConnection) {
        let today = chrono::Utc::now().date_naive();
        let day1 = (today - chrono::Duration::days(3)).format("%Y-%m-%d").to_string();
        let day2 = (today - chrono::Duration::days(2)).format("%Y-%m-%d").to_string();
        let day3 = (today - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();

        power_flows::add_power_flow_backend(
            backend, &day1, "Fed fiscal expansion", "FIC", "gaining", Some("MIC"), "Treasury spending up", 3, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            backend, &day2, "BRICS summit outcomes", "FIC", "gaining", None, "Multilateral fiscal pacts", 4, Some("test"),
        ).unwrap();
        power_flows::add_power_flow_backend(
            backend, &day3, "Budget continuing resolution", "FIC", "gaining", None, "No defense increase", 2, Some("test"),
        ).unwrap();
    }

    fn seed_synthesis_fixture(backend: &BackendConnection) {
        db::price_cache::upsert_price_backend(
            backend,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(200),
                currency: "USD".to_string(),
                fetched_at: chrono::Utc::now().to_rfc3339(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: Some(dec!(195)),
            },
        )
        .unwrap();
        db::transactions::insert_transaction_backend(
            backend,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2026-03-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        db::watchlist::add_to_watchlist_backend(backend, "NVDA", AssetCategory::Equity).unwrap();
        db::convictions::set_conviction_backend(backend, "AAPL", 4, Some("bull")).unwrap();
        db::convictions::set_conviction_backend(backend, "NVDA", -3, Some("watch divergence"))
            .unwrap();
        db::mobile_timeframe_scores::upsert_score_backend(
            backend,
            "low",
            35.0,
            Some("short term bullish"),
        )
        .unwrap();
        db::mobile_timeframe_scores::upsert_score_backend(
            backend,
            "medium",
            -20.0,
            Some("medium term weak"),
        )
        .unwrap();
        db::mobile_timeframe_scores::upsert_score_backend(
            backend,
            "high",
            30.0,
            Some("high term bullish"),
        )
        .unwrap();
        db::mobile_timeframe_scores::upsert_score_backend(
            backend,
            "macro",
            -40.0,
            Some("macro defensive"),
        )
        .unwrap();
        let trend_id = trends::add_trend_backend(
            backend,
            "AI capex",
            "high",
            "up",
            "high",
            Some("tech"),
            Some("AI demand"),
            Some("bullish NVDA"),
            Some("earnings"),
        )
        .unwrap();
        trends::add_asset_impact_backend(
            backend,
            trend_id,
            "NVDA",
            "bullish",
            Some("earnings"),
            Some("high"),
        )
        .unwrap();
        let scenario_id = db::scenarios::add_scenario(
            backend.sqlite(),
            "Hard Landing",
            70.0,
            Some("growth scare"),
            Some("AAPL bear, NVDA bear"),
            Some("labor and CPI"),
            None,
        )
        .unwrap();
        db::scenarios::update_scenario(
            backend.sqlite(),
            scenario_id,
            None,
            None,
            None,
            Some("active"),
        )
        .unwrap();
        db::technical_signals::add_signal_backend(
            backend,
            &NewSignal {
                symbol: "NVDA",
                signal_type: "macd_cross",
                direction: "up",
                severity: "elevated",
                trigger_price: Some(890.0),
                description: "bullish crossover",
                timeframe: "daily",
            },
        )
        .unwrap();
        let tomorrow = (chrono::Utc::now().date_naive() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        db::calendar_cache::upsert_event_backend(
            backend,
            &tomorrow,
            "NVDA Earnings",
            "high",
            None,
            None,
            "earnings",
            Some("NVDA"),
        )
        .unwrap();
        db::predictions_cache::upsert_predictions_backend(
            backend,
            &[PredictionMarket {
                id: "hard-landing".to_string(),
                question: "Will the US enter recession in 2026?".to_string(),
                probability: 0.58,
                volume_24h: 100000.0,
                category: MarketCategory::Economics,
                updated_at: chrono::Utc::now().timestamp(),
            }],
        )
        .unwrap();
    }
}
