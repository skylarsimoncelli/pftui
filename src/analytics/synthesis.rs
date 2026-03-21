use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

use crate::analytics::catalysts;
use crate::analytics::impact;
use crate::analytics::situation::{self, TimeframeScore};
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::{convictions, regime_snapshots, scenarios, trends, watchlist};
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;

#[derive(Debug, Clone, Serialize)]
pub struct SynthesisReport {
    pub generated_at: String,
    pub strongest_alignment: Vec<AlignmentState>,
    pub highest_confidence_divergence: Vec<DivergenceState>,
    pub constraint_flows: Vec<ConstraintState>,
    pub unresolved_tensions: Vec<SynthesisNote>,
    pub watch_tomorrow: Vec<WatchTomorrowCandidate>,
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
    let unresolved_tensions = unresolved_tensions(&constraint_flows, &divergences);
    let watch_tomorrow = watch_tomorrow_candidates(backend, &rows)?;

    Ok(SynthesisReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        strongest_alignment,
        highest_confidence_divergence: divergences,
        constraint_flows,
        unresolved_tensions,
        watch_tomorrow,
    })
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

fn unresolved_tensions(
    constraints: &[ConstraintState],
    divergences: &[DivergenceState],
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
