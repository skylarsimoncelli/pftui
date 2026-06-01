#![allow(dead_code)]

#[derive(Debug, Clone, Default)]
pub struct BuildContext {
    pub report_date: Option<String>,
    pub data_freshness: Vec<DataFreshnessSummary>,
    pub synthesis: Option<SynthesisSnapshot>,
    pub regime: Option<RegimeSummary>,
    pub analyst_convergence: Vec<AnalystConvergenceSummary>,
    pub scenario_deltas: Vec<ScenarioDeltaSummary>,
    pub news_catalysts: Vec<CatalystSummary>,
    pub market_snapshot: Vec<MarketSnapshotRow>,
    pub macro_indicators: Vec<MacroIndicatorSummary>,
    pub economic_calendar: Vec<EconomicCalendarEvent>,
    pub macro_analyst_views: Vec<AnalystViewSummary>,
    pub macro_news_volume: Vec<NewsVolumeSignal>,
    pub bitcoin_market: Option<BitcoinMarketSummary>,
    pub bitcoin_etf_flows: Vec<BitcoinEtfFlowSummary>,
    pub bitcoin_onchain: Vec<BitcoinOnChainSummary>,
    pub bitcoin_analyst_views: Vec<AnalystViewSummary>,
    pub bitcoin_news: Vec<BitcoinCatalystSummary>,
    pub bitcoin_prediction_signals: Vec<BitcoinPredictionSignal>,
    pub precious_metals_market: Vec<PreciousMetalMarketRow>,
    pub precious_metals_supply: Vec<PreciousMetalsSupplyRow>,
    pub precious_metals_analyst_views: Vec<AnalystViewSummary>,
    pub precious_metals_news: Vec<PreciousMetalsNewsSignal>,
    pub real_yield_context: Option<RealYieldSummary>,
    pub sovereign_gold_holdings: Vec<SovereignHoldingSummary>,
    pub equity_indices: Vec<EquityMarketRow>,
    pub equity_sectors: Vec<EquityMarketRow>,
    pub equity_breadth: Option<EquityBreadthSummary>,
    pub equity_earnings: Option<EquityEarningsSummary>,
    pub equity_analyst_views: Vec<AnalystViewSummary>,
    pub equity_news: Vec<EquityNewsSignal>,
    pub public_news_events: Vec<PublicNewsEvent>,
    pub public_news_silence: Vec<NewsVolumeSignal>,
    pub public_scenarios: Vec<PublicScenarioRow>,
    pub public_calibration: Vec<CalibrationReliabilityRow>,
    pub public_lessons_applied: Vec<LessonAppliedSummary>,
    pub public_prediction_intelligence: Vec<PredictionMarketIntelligence>,
    pub public_source_tier_overrides: Vec<SourceTierOverrideSummary>,
    pub private_portfolio_snapshot: Option<PrivatePortfolioSnapshotSummary>,
    pub private_derived_actions: Vec<DerivedActionSummary>,
    pub private_binary_catalysts: Vec<BinaryCatalystSummary>,
    pub private_what_changed_deltas: Vec<WhatChangedDeltaSummary>,
    pub private_positions: Vec<PrivatePositionSnapshotRow>,
    pub private_drift_rows: Vec<PrivateDriftRow>,
    pub private_macro_regime: Option<PrivateMacroRegimeQuadrant>,
    pub private_macro_scenarios: Vec<PrivateMacroScenarioRow>,
    pub private_macro_divergences: Vec<PrivateNarrativeMoneyDivergence>,
    pub private_macro_catalysts: Vec<PrivateMacroCatalyst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFreshnessSummary {
    pub source: String,
    pub last_fetch: Option<String>,
    pub records: Option<u64>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisSnapshot {
    pub summary: String,
    pub central_tension: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegimeSummary {
    pub classification: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalystConvergenceSummary {
    pub asset: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioDeltaSummary {
    pub name: String,
    pub probability: f64,
    pub delta_7d: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalystSummary {
    pub headline: String,
    pub market_read: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketSnapshotRow {
    pub asset: String,
    pub price: Option<String>,
    pub daily_change_pct: Option<f64>,
    pub weekly_change_pct: Option<f64>,
    pub signal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroIndicatorSummary {
    pub name: String,
    pub value: Option<String>,
    pub trend: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomicCalendarEvent {
    pub date: String,
    pub event: String,
    pub importance: Option<String>,
    pub market_relevance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalystViewSummary {
    pub layer: String,
    pub asset: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewsVolumeSignal {
    pub topic: String,
    pub current_count: u32,
    pub baseline_count: Option<f64>,
    pub status: String,
    pub caveat: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitcoinMarketSummary {
    pub price: Option<String>,
    pub daily_change_pct: Option<f64>,
    pub weekly_change_pct: Option<f64>,
    pub trend: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitcoinEtfFlowSummary {
    pub period: String,
    pub net_flow: Option<String>,
    pub detail: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitcoinOnChainSummary {
    pub metric: String,
    pub value: Option<String>,
    pub interpretation: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitcoinCatalystSummary {
    pub headline: String,
    pub source: Option<String>,
    pub relevance: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitcoinPredictionSignal {
    pub market: String,
    pub probability: Option<f64>,
    pub delta_7d: Option<f64>,
    pub relevance: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreciousMetalMarketRow {
    pub asset: String,
    pub symbol: String,
    pub price: Option<String>,
    pub daily_change_pct: Option<f64>,
    pub weekly_change_pct: Option<f64>,
    pub trend: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciousMetalsSupplyRow {
    pub asset: String,
    pub metric: String,
    pub value: Option<String>,
    pub interpretation: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciousMetalsNewsSignal {
    pub headline: String,
    pub domain: String,
    pub source_tier: Option<u8>,
    pub independence: Option<String>,
    pub topic: Option<String>,
    pub relevance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealYieldSummary {
    pub value: Option<String>,
    pub direction: Option<String>,
    pub interpretation: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SovereignHoldingSummary {
    pub holder: String,
    pub latest: Option<String>,
    pub change: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EquityMarketRow {
    pub name: String,
    pub symbol: String,
    pub price: Option<String>,
    pub daily_change_pct: Option<f64>,
    pub weekly_change_pct: Option<f64>,
    pub trend: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquityBreadthSummary {
    pub label: String,
    pub value: Option<String>,
    pub interpretation: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquityEarningsSummary {
    pub label: String,
    pub value: Option<String>,
    pub interpretation: Option<String>,
    pub freshness: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquityNewsSignal {
    pub headline: String,
    pub domain: String,
    pub source_tier: Option<u8>,
    pub independence: Option<String>,
    pub topic: Option<String>,
    pub relevance: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicNewsEvent {
    pub headline: String,
    pub summary: Option<String>,
    pub domain: String,
    pub source_tier: Option<u8>,
    pub independence: Option<String>,
    pub topic: Option<String>,
    pub bound_market: Option<String>,
    pub impact_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublicScenarioRow {
    pub name: String,
    pub probability: f64,
    pub delta_7d: Option<f64>,
    pub narrative_vs_money: Option<String>,
    pub key_driver: Option<String>,
    pub confirmation: Option<String>,
    pub invalidation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationReliabilityRow {
    pub layer: String,
    pub conviction_band: String,
    pub predicted_pct: f64,
    pub observed_pct: f64,
    pub sample_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LessonAppliedSummary {
    pub lesson_id: String,
    pub summary: String,
    pub applied_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PredictionMarketIntelligence {
    pub market: String,
    pub probability: Option<f64>,
    pub delta_7d: Option<f64>,
    pub read: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTierOverrideSummary {
    pub domain: String,
    pub tier: u8,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivatePortfolioSnapshotSummary {
    pub total_value: Option<String>,
    pub daily_pnl: Option<String>,
    pub daily_pnl_pct: Option<f64>,
    pub allocation_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedActionSummary {
    pub asset: String,
    pub action: String,
    pub urgency: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryCatalystSummary {
    pub date: String,
    pub event: String,
    pub impact: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhatChangedDeltaSummary {
    pub label: String,
    pub delta: String,
    pub direction: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivatePositionSnapshotRow {
    pub symbol: String,
    pub price: Option<String>,
    pub daily_change: Option<String>,
    pub allocation_pct: f64,
    pub unrealized_pnl: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateDriftRow {
    pub symbol: String,
    pub target_pct: f64,
    pub actual_pct: f64,
    pub band_pct: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateMacroRegimeQuadrant {
    pub growth: f64,
    pub inflation: f64,
    pub trail: Vec<PrivateRegimeTrailPoint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateRegimeTrailPoint {
    pub growth: f64,
    pub inflation: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateMacroScenarioRow {
    pub name: String,
    pub probability: f64,
    pub prior_7d: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateNarrativeMoneyDivergence {
    pub scenario: String,
    pub summary: String,
    pub material: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateMacroCatalyst {
    pub date: String,
    pub event: String,
    pub impact: String,
}
