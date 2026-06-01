#![allow(dead_code)]

#[derive(Debug, Clone, Default)]
pub struct BuildContext {
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
