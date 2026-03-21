use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

use crate::analytics::catalysts::{self, CatalystEvent};
use crate::analytics::situation::{self, SituationPosition};
use crate::db;
use crate::db::backend::BackendConnection;
use crate::db::scenarios::Scenario;
use crate::db::technical_signals::TechnicalSignalRecord;
use crate::db::trends;
use crate::db::watchlist::WatchlistEntry;
use crate::models::asset_names::resolve_name;

#[derive(Debug, Clone, Serialize)]
pub struct ImpactReport {
    pub generated_at: String,
    pub exposures: Vec<AssetInsight>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpportunitiesReport {
    pub generated_at: String,
    pub opportunities: Vec<AssetInsight>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetInsight {
    pub symbol: String,
    pub name: String,
    pub held: bool,
    pub watchlist: bool,
    pub allocation_pct: Option<String>,
    pub current_value: Option<String>,
    pub consensus: String,
    pub score: i32,
    pub severity: String,
    pub summary: String,
    pub evidence_chain: Vec<String>,
}

#[derive(Debug, Clone)]
struct AssetState {
    symbol: String,
    name: String,
    held: bool,
    watchlist: bool,
    allocation_pct: Option<Decimal>,
    current_value: Option<Decimal>,
    alignment_score_pct: f64,
    consensus: String,
    conviction_score: i32,
    bull_layers: usize,
    bear_layers: usize,
}

pub fn build_impact_report_backend(backend: &BackendConnection) -> Result<ImpactReport> {
    let context = build_context(backend)?;
    let mut exposures = context
        .states
        .values()
        .filter(|state| state.held || state.watchlist)
        .map(|state| to_asset_insight(state, &context, true))
        .collect::<Vec<_>>();
    exposures.sort_by(|left, right| {
        right
            .held
            .cmp(&left.held)
            .then_with(|| right.watchlist.cmp(&left.watchlist))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    exposures.truncate(8);

    Ok(ImpactReport {
        generated_at: Utc::now().to_rfc3339(),
        exposures,
    })
}

pub fn build_opportunities_report_backend(
    backend: &BackendConnection,
) -> Result<OpportunitiesReport> {
    let context = build_context(backend)?;
    let mut opportunities = context
        .states
        .values()
        .filter(|state| !state.held)
        .map(|state| to_asset_insight(state, &context, false))
        .filter(|row| row.score >= 35)
        .collect::<Vec<_>>();
    opportunities.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    opportunities.truncate(8);

    Ok(OpportunitiesReport {
        generated_at: Utc::now().to_rfc3339(),
        opportunities,
    })
}

struct ImpactContext {
    states: HashMap<String, AssetState>,
    catalysts_by_symbol: HashMap<String, Vec<CatalystEvent>>,
    technicals_by_symbol: HashMap<String, Vec<TechnicalSignalRecord>>,
    trends_by_symbol: HashMap<String, Vec<(trends::Trend, trends::TrendAssetImpact)>>,
    scenarios: Vec<Scenario>,
}

fn build_context(backend: &BackendConnection) -> Result<ImpactContext> {
    let inputs = situation::collect_inputs_backend(backend)?;
    let watchlist = db::watchlist::list_watchlist_backend(backend).unwrap_or_default();
    let convictions = db::convictions::list_current_backend(backend).unwrap_or_default();
    let conviction_map = convictions
        .into_iter()
        .map(|row| (row.symbol.to_uppercase(), row.score))
        .collect::<HashMap<_, _>>();
    let scenarios =
        db::scenarios::list_scenarios_backend(backend, Some("active")).unwrap_or_default();
    let all_impacts = trends::list_all_impacts_backend(backend).unwrap_or_default();
    let technicals = db::technical_signals::list_signals_backend(backend, None, None, Some(250))
        .unwrap_or_default();
    let catalysts = catalysts::build_report_backend(backend, catalysts::CatalystWindow::Week)
        .map(|report| report.catalysts)
        .unwrap_or_default();

    let mut symbols = discover_symbols(
        &inputs.positions,
        &watchlist,
        &all_impacts,
        &technicals,
        &catalysts,
    );
    let alignment = build_alignment_states(&symbols, &conviction_map, &scenarios, &all_impacts);
    let catalysts_by_symbol = index_catalysts(&catalysts);
    let technicals_by_symbol = index_technicals(&technicals);
    let trends_by_symbol = index_trends(&all_impacts);

    let held_symbols = inputs
        .positions
        .iter()
        .map(|row| row.symbol.to_uppercase())
        .collect::<BTreeSet<_>>();
    let watch_symbols = watchlist
        .iter()
        .map(|row| row.symbol.to_uppercase())
        .collect::<BTreeSet<_>>();
    let position_map = inputs
        .positions
        .iter()
        .map(|row| (row.symbol.to_uppercase(), row))
        .collect::<HashMap<_, _>>();

    let mut states = HashMap::new();
    for symbol in symbols.drain(..) {
        let alignment_state = alignment.get(&symbol).cloned().unwrap_or_default();
        let position = position_map.get(&symbol).copied();
        states.insert(
            symbol.clone(),
            AssetState {
                name: resolve_name(&symbol),
                symbol: symbol.clone(),
                held: held_symbols.contains(&symbol),
                watchlist: watch_symbols.contains(&symbol),
                allocation_pct: position.and_then(|row| row.allocation_pct),
                current_value: position.and_then(|row| row.current_value),
                alignment_score_pct: alignment_state.score_pct,
                consensus: alignment_state.consensus,
                conviction_score: *conviction_map.get(&symbol).unwrap_or(&0),
                bull_layers: alignment_state.bull_layers,
                bear_layers: alignment_state.bear_layers,
            },
        );
    }

    Ok(ImpactContext {
        states,
        catalysts_by_symbol,
        technicals_by_symbol,
        trends_by_symbol,
        scenarios,
    })
}

fn discover_symbols(
    positions: &[SituationPosition],
    watchlist: &[WatchlistEntry],
    all_impacts: &[(trends::Trend, trends::TrendAssetImpact)],
    technicals: &[TechnicalSignalRecord],
    catalysts: &[CatalystEvent],
) -> Vec<String> {
    let mut symbols = BTreeSet::new();
    for row in positions {
        symbols.insert(row.symbol.to_uppercase());
    }
    for row in watchlist {
        symbols.insert(row.symbol.to_uppercase());
    }
    for (_, impact) in all_impacts {
        symbols.insert(impact.symbol.to_uppercase());
    }
    for row in technicals {
        symbols.insert(row.symbol.to_uppercase());
    }
    for row in catalysts {
        for symbol in &row.affected_assets {
            symbols.insert(symbol.to_uppercase());
        }
    }
    symbols.into_iter().collect()
}

#[derive(Debug, Clone, Default)]
struct AlignmentState {
    consensus: String,
    score_pct: f64,
    bull_layers: usize,
    bear_layers: usize,
}

fn build_alignment_states(
    symbols: &[String],
    conviction_map: &HashMap<String, i32>,
    scenarios: &[Scenario],
    all_impacts: &[(trends::Trend, trends::TrendAssetImpact)],
) -> HashMap<String, AlignmentState> {
    let trend_bias_map = all_impacts
        .iter()
        .fold(HashMap::new(), |mut acc, (_, impact)| {
            let entry = acc.entry(impact.symbol.to_uppercase()).or_insert(0.0f64);
            *entry += match impact.impact.to_ascii_lowercase().as_str() {
                "bullish" => 1.0,
                "bearish" => -1.0,
                _ => 0.0,
            };
            acc
        });

    let mut map = HashMap::new();
    for symbol in symbols {
        let conviction_signal =
            (*conviction_map.get(symbol).unwrap_or(&0) as f64 / 5.0).clamp(-1.0, 1.0);
        let trend_bias = trend_bias_map
            .get(symbol)
            .copied()
            .unwrap_or(0.0)
            .clamp(-2.0, 2.0)
            / 2.0;
        let macro_bias = scenario_bias_for_symbol(symbol, scenarios);
        let bull_layers = [conviction_signal, trend_bias, macro_bias]
            .iter()
            .filter(|row| **row > 0.05)
            .count();
        let bear_layers = [conviction_signal, trend_bias, macro_bias]
            .iter()
            .filter(|row| **row < -0.05)
            .count();
        let weighted = 0.35 * conviction_signal + 0.35 * trend_bias + 0.30 * macro_bias;
        map.insert(
            symbol.clone(),
            AlignmentState {
                consensus: consensus_from_counts(bull_layers, bear_layers),
                score_pct: (weighted.abs() * 100.0).clamp(0.0, 100.0),
                bull_layers,
                bear_layers,
            },
        );
    }

    map
}

fn scenario_bias_for_symbol(symbol: &str, scenarios: &[Scenario]) -> f64 {
    let symbol_lower = symbol.to_ascii_lowercase();
    let name_lower = resolve_name(symbol).to_ascii_lowercase();
    let mut score = 0.0;
    for scenario in scenarios {
        let text = format!(
            "{} {} {} {}",
            scenario.name,
            scenario.description.as_deref().unwrap_or(""),
            scenario.asset_impact.as_deref().unwrap_or(""),
            scenario.triggers.as_deref().unwrap_or("")
        )
        .to_ascii_lowercase();
        if !text.contains(&symbol_lower) && !name_lower.is_empty() && !text.contains(&name_lower) {
            continue;
        }
        let direction = if text.contains("bull") && !text.contains("bear") {
            1.0
        } else if text.contains("bear") && !text.contains("bull") {
            -1.0
        } else {
            0.0
        };
        score += direction * (scenario.probability / 100.0);
    }
    score.clamp(-1.0, 1.0)
}

fn to_asset_insight(state: &AssetState, context: &ImpactContext, held_view: bool) -> AssetInsight {
    let catalysts = context
        .catalysts_by_symbol
        .get(&state.symbol)
        .cloned()
        .unwrap_or_default();
    let technicals = context
        .technicals_by_symbol
        .get(&state.symbol)
        .cloned()
        .unwrap_or_default();
    let trends = context
        .trends_by_symbol
        .get(&state.symbol)
        .cloned()
        .unwrap_or_default();
    let scenario_count = scenario_links_for_symbol(&state.symbol, &context.scenarios);

    let mut score = state.alignment_score_pct.round() as i32;
    score += catalysts.len() as i32 * 10;
    score += technicals.len().min(2) as i32 * 6;
    score += trends.len().min(2) as i32 * 8;
    score += scenario_count.min(2) as i32 * 7;
    score += state.conviction_score.abs() * 4;
    if held_view {
        if state.held {
            score += 20;
        } else if state.watchlist {
            score += 6;
        }
        score += state
            .allocation_pct
            .map(|row| row.round_dp(0).to_i32().unwrap_or(0).min(20))
            .unwrap_or(0);
    }

    let evidence_chain =
        build_evidence_chain(state, &catalysts, &technicals, &trends, &context.scenarios);
    let severity = if score >= 85 {
        "critical"
    } else if score >= 55 {
        "elevated"
    } else {
        "normal"
    };
    let summary = if held_view {
        format!(
            "{} exposure with {} consensus, {} catalyst(s), {} scenario link(s).",
            if state.held { "Held" } else { "Watchlist" },
            state.consensus,
            catalysts.len(),
            scenario_count
        )
    } else {
        format!(
            "Non-held idea with {} consensus, {} catalyst(s), {} trend impact(s).",
            state.consensus,
            catalysts.len(),
            trends.len()
        )
    };

    AssetInsight {
        symbol: state.symbol.clone(),
        name: state.name.clone(),
        held: state.held,
        watchlist: state.watchlist,
        allocation_pct: state.allocation_pct.map(|row| row.round_dp(2).to_string()),
        current_value: state.current_value.map(|row| row.round_dp(2).to_string()),
        consensus: state.consensus.clone(),
        score,
        severity: severity.to_string(),
        summary,
        evidence_chain,
    }
}

fn build_evidence_chain(
    state: &AssetState,
    catalysts: &[CatalystEvent],
    technicals: &[TechnicalSignalRecord],
    trends: &[(trends::Trend, trends::TrendAssetImpact)],
    scenarios: &[Scenario],
) -> Vec<String> {
    let mut evidence = vec![format!(
        "Alignment {} with {} bull / {} bear layers ({:.0}%).",
        state.consensus, state.bull_layers, state.bear_layers, state.alignment_score_pct
    )];

    if state.conviction_score != 0 {
        evidence.push(format!(
            "Conviction score {}.",
            signed_int(state.conviction_score)
        ));
    }
    if let Some(first) = catalysts.first() {
        evidence.push(format!(
            "Catalyst {}: {}.",
            first.countdown_bucket.replace('-', " "),
            first.title
        ));
    }
    if let Some((trend, impact)) = trends.first() {
        evidence.push(format!(
            "Trend {} is {} via {}.",
            trend.name,
            impact.impact,
            impact
                .mechanism
                .clone()
                .unwrap_or_else(|| "linked impact".to_string())
        ));
    }
    if let Some(signal) = technicals.first() {
        evidence.push(format!(
            "Technical {} {}.",
            signal.signal_type, signal.description
        ));
    }
    if let Some(scenario) = first_matching_scenario(&state.symbol, scenarios) {
        evidence.push(format!(
            "Scenario {} at {:.0}% probability.",
            scenario.name, scenario.probability
        ));
    }

    evidence.truncate(5);
    evidence
}

fn scenario_links_for_symbol(symbol: &str, scenarios: &[Scenario]) -> usize {
    let symbol_lower = symbol.to_ascii_lowercase();
    let name_lower = resolve_name(symbol).to_ascii_lowercase();
    scenarios
        .iter()
        .filter(|scenario| {
            let text = format!(
                "{} {} {} {}",
                scenario.name,
                scenario.description.as_deref().unwrap_or(""),
                scenario.asset_impact.as_deref().unwrap_or(""),
                scenario.triggers.as_deref().unwrap_or("")
            )
            .to_ascii_lowercase();
            text.contains(&symbol_lower) || (!name_lower.is_empty() && text.contains(&name_lower))
        })
        .count()
}

fn first_matching_scenario<'a>(symbol: &str, scenarios: &'a [Scenario]) -> Option<&'a Scenario> {
    let symbol_lower = symbol.to_ascii_lowercase();
    let name_lower = resolve_name(symbol).to_ascii_lowercase();
    scenarios.iter().find(|scenario| {
        let text = format!(
            "{} {} {} {}",
            scenario.name,
            scenario.description.as_deref().unwrap_or(""),
            scenario.asset_impact.as_deref().unwrap_or(""),
            scenario.triggers.as_deref().unwrap_or("")
        )
        .to_ascii_lowercase();
        text.contains(&symbol_lower) || (!name_lower.is_empty() && text.contains(&name_lower))
    })
}

fn index_catalysts(catalysts: &[CatalystEvent]) -> HashMap<String, Vec<CatalystEvent>> {
    let mut map: HashMap<String, Vec<CatalystEvent>> = HashMap::new();
    for catalyst in catalysts {
        for symbol in &catalyst.affected_assets {
            map.entry(symbol.to_uppercase())
                .or_default()
                .push(catalyst.clone());
        }
    }
    map
}

fn index_technicals(rows: &[TechnicalSignalRecord]) -> HashMap<String, Vec<TechnicalSignalRecord>> {
    let mut map: HashMap<String, Vec<TechnicalSignalRecord>> = HashMap::new();
    for row in rows {
        map.entry(row.symbol.to_uppercase())
            .or_default()
            .push(row.clone());
    }
    map
}

fn index_trends(
    rows: &[(trends::Trend, trends::TrendAssetImpact)],
) -> HashMap<String, Vec<(trends::Trend, trends::TrendAssetImpact)>> {
    let mut map: HashMap<String, Vec<(trends::Trend, trends::TrendAssetImpact)>> = HashMap::new();
    for row in rows {
        map.entry(row.1.symbol.to_uppercase())
            .or_default()
            .push(row.clone());
    }
    map
}

fn consensus_from_counts(bull: usize, bear: usize) -> String {
    if bull >= 2 && bear == 0 {
        "bullish".to_string()
    } else if bear >= 2 && bull == 0 {
        "bearish".to_string()
    } else if bull > bear {
        "lean-bull".to_string()
    } else if bear > bull {
        "lean-bear".to_string()
    } else {
        "mixed".to_string()
    }
}

fn signed_int(value: i32) -> String {
    if value >= 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

trait DecimalToI32 {
    fn to_i32(&self) -> Option<i32>;
}

impl DecimalToI32 for Decimal {
    fn to_i32(&self) -> Option<i32> {
        self.round_dp(0).to_string().parse::<i32>().ok()
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
    use crate::models::transaction::NewTransaction;
    use crate::models::transaction::TxType;
    use rust_decimal_macros::dec;

    #[test]
    fn exposure_ranking_prefers_held_assets_with_evidence() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_held_and_nonheld(&backend);

        let report = build_impact_report_backend(&backend).unwrap();
        assert!(!report.exposures.is_empty());
        assert_eq!(report.exposures[0].symbol, "AAPL");
        assert!(report.exposures[0].held);
        assert!(!report.exposures[0].evidence_chain.is_empty());
    }

    #[test]
    fn opportunities_exclude_held_assets_and_rank_nonheld_candidates() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        seed_held_and_nonheld(&backend);

        let report = build_opportunities_report_backend(&backend).unwrap();
        assert!(report.opportunities.iter().all(|row| !row.held));
        assert!(report.opportunities.iter().any(|row| row.symbol == "NVDA"));
        assert!(!report.opportunities.iter().any(|row| row.symbol == "AAPL"));
    }

    fn seed_held_and_nonheld(backend: &BackendConnection) {
        db::price_cache::upsert_price_backend(
            backend,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(200),
                currency: "USD".to_string(),
                fetched_at: Utc::now().to_rfc3339(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: Some(dec!(195)),
            },
        )
        .unwrap();
        db::price_cache::upsert_price_backend(
            backend,
            &PriceQuote {
                symbol: "NVDA".to_string(),
                price: dec!(900),
                currency: "USD".to_string(),
                fetched_at: Utc::now().to_rfc3339(),
                source: "test".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: Some(dec!(880)),
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
        db::convictions::set_conviction_backend(backend, "AAPL", 4, Some("core holding")).unwrap();
        db::convictions::set_conviction_backend(backend, "NVDA", 5, Some("AI leader")).unwrap();

        let trend_id = trends::add_trend_backend(
            backend,
            "AI buildout",
            "high",
            "up",
            "high",
            Some("technology"),
            Some("Hyperscaler capex remains strong"),
            Some("Bullish for NVDA"),
            Some("datacenter demand"),
        )
        .unwrap();
        trends::add_asset_impact_backend(
            backend,
            trend_id,
            "NVDA",
            "bullish",
            Some("datacenter demand"),
            Some("high"),
        )
        .unwrap();

        let scenario_id = db::scenarios::add_scenario(
            backend.sqlite(),
            "AI capex upside",
            65.0,
            Some("AI spend keeps expanding"),
            Some("NVDA bullish"),
            Some("earnings and capex commentary"),
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
        db::predictions_cache::upsert_predictions_backend(
            backend,
            &[PredictionMarket {
                id: "ai-spend".to_string(),
                question: "Will AI spending accelerate in 2026?".to_string(),
                probability: 0.61,
                volume_24h: 500_000.0,
                category: MarketCategory::AI,
                updated_at: Utc::now().timestamp(),
            }],
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
        db::calendar_cache::upsert_event_backend(
            backend,
            &Utc::now().date_naive().format("%Y-%m-%d").to_string(),
            "NVDA Earnings",
            "high",
            None,
            None,
            "earnings",
            Some("NVDA"),
        )
        .unwrap();
    }
}
