#![allow(dead_code)]

// =====================================================================
// Performance budget — `pftui report build daily --mode both`
// =====================================================================
//
// Target: < 2s end-to-end wall-time against the standard fixture
//         (`tests/fixtures/db/v0.27.0.sqlite` — ~90 days of history,
//         4 positions, ~800 predictions).
//
// Why this lives here:
//   `build daily` is the primary report-generation path for both the
//   operator workflow and every cron-driven autonomous run. Once a
//   section's render cost creeps into seconds, the whole pipeline
//   degrades silently. The budget is enforced by
//   `tests/report_build_daily_perf.rs`; if you change the budget,
//   update BOTH the test constant AND this comment in the same PR.
//
// Re-baseline policy:
//   - Raise the budget ONLY when a major feature intentionally adds
//     cost (e.g., a new heavy aggregate query) AND a reviewer signs
//     off in the PR description. Never silently.
//   - If a section regularly dominates the budget, add `--timing`
//     instrumentation so the test's failure path can name the section.
//
// =====================================================================

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
    /// Structured real-rates snapshot built from `real_yields_history`.
    /// Populated when a macro/high analyst routine pre-stamps it (or via the
    /// future assembler hook documented in AGENTS.md). The renderer in
    /// `report::sections::real_rates_macro` reads this directly and emits an
    /// empty string when the snapshot is absent.
    pub real_rates_snapshot: Option<crate::commands::real_yields::MacroBlockSnapshot>,
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
    pub private_calibration: Vec<CalibrationReliabilityRow>,
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
    pub private_asset_convergence: Vec<PrivateAssetConvergenceRow>,
    pub private_conviction_trajectories: Vec<PrivateConvictionTrajectoryRow>,
    pub private_outlooks: Vec<PrivateOutlookByHorizonRow>,
    pub private_risk_factor_mappings: Vec<PrivateRiskFactorMapping>,
    pub private_journal_views: Vec<PrivateJournalViewRow>,
    pub private_news_events: Vec<PrivateNewsCatalyst>,
    pub private_news_silence: Vec<NewsVolumeSignal>,
    pub private_open_predictions: Vec<PrivateOpenPredictionRow>,
    pub private_open_predictions_calibration: Option<PrivateOpenPredictionsCalibration>,
    pub private_lessons_applied: Option<PrivateLessonsAppliedSummary>,
    /// Optional regime-conditional hit-rate summary surfaced in the
    /// Self-Retrospective Calibration section. Populated by the report
    /// assembler from `pftui analytics backtest layer-bias` when the regime
    /// classifier has recorded a non-neutral regime for the current day.
    pub private_regime_conditional: Option<PrivateRegimeConditionalSummary>,
    /// 7-day rolling recommendation hit rate, surfaced in the public
    /// Methodology section so the report carries its own accuracy
    /// disclosure. Populated by `BuildContext::load` from
    /// `recommendation_outcomes`; `None` means insufficient scored
    /// outcomes (or no `recommendations` table on the active backend).
    pub recommendation_accuracy_7d: Option<RecommendationAccuracySummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationAccuracySummary {
    pub window_days: i64,
    pub scored: u32,
    pub hits: u32,
    pub hit_rate_pct: f64,
    pub avg_score: f64,
}

/// Compact regime-aware calibration prior emitted by the Self-Retrospective
/// Calibration section. Surfaces the strongest layer/topic deviation when
/// conditioned on the current regime.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivateRegimeConditionalSummary {
    pub current_regime: String,
    pub top_layer: String,
    pub top_topic: String,
    pub hit_rate_pct: f64,
    pub sample_size: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrivateLessonsAppliedSummary {
    pub since: String,
    pub total_predictions: u32,
    pub guarded_predictions: u32,
    pub unique_lessons: u32,
    pub lesson_references: Vec<PrivateLessonReferenceRow>,
    pub strongest_analog: Option<PrivateHistoricalAnalogRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateLessonReferenceRow {
    pub lesson_id: i64,
    pub references: u32,
    pub miss_type: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateHistoricalAnalogRow {
    pub prediction_id: i64,
    pub claim: String,
    pub overlap_count: u32,
    pub overlapping_lesson_ids: Vec<i64>,
    pub outcome: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateAssetConvergenceRow {
    pub symbol: String,
    pub target_pct: Option<f64>,
    pub views: Vec<PrivateAssetConvergenceView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateAssetConvergenceView {
    pub analyst: String,
    pub conviction: i64,
    pub reasoning_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateConvictionTrajectoryRow {
    pub symbol: String,
    pub layer: String,
    pub points: Vec<PrivateConvictionTrajectoryPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateConvictionTrajectoryPoint {
    pub date: String,
    pub conviction: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateOutlookByHorizonRow {
    pub symbol: String,
    pub days: Option<PrivateOutlookPoint>,
    pub weeks: Option<PrivateOutlookPoint>,
    pub months: Option<PrivateOutlookPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateOutlookPoint {
    pub direction: String,
    pub conviction: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateRiskFactorMapping {
    pub symbol: String,
    pub factor: String,
    pub direction: String,
    pub exposure_multiplier: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateJournalViewRow {
    pub symbol: String,
    pub author: String,
    pub conviction: i64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateNewsCatalyst {
    pub headline: String,
    pub what_happened: Option<String>,
    pub money_moved: Option<String>,
    pub who_benefits: Option<String>,
    pub what_it_means: Option<String>,
    pub domain: String,
    pub source_tier: Option<u8>,
    pub independence: Option<String>,
    pub topic: Option<String>,
    pub related_assets: Vec<String>,
    pub related_scenarios: Vec<String>,
    pub impact_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateOpenPredictionRow {
    pub id: Option<i64>,
    pub symbol: String,
    pub claim: String,
    pub target_date: String,
    pub days_remaining: i64,
    pub confidence: Option<f64>,
    pub conviction: Option<i64>,
    pub direction: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrivateOpenPredictionsCalibration {
    pub layer: Option<String>,
    pub sample_size: u32,
    pub predicted_pct: Option<f64>,
    pub observed_pct: Option<f64>,
}

// ---------------------------------------------------------------------------
// Assembler
// ---------------------------------------------------------------------------

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::db::backend::BackendConnection;
use crate::report::sections;

/// Mode for `pftui report build daily`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Public,
    Private,
    Both,
}

impl BuildMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildMode::Public => "public",
            BuildMode::Private => "private",
            BuildMode::Both => "both",
        }
    }
}

/// One section in the canonical assembly order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionSpec {
    pub name: &'static str,
    pub visibility: SectionVisibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionVisibility {
    Public,
    Private,
}

/// Canonical ordering of the public daily report sections (Step 5a).
pub fn public_section_plan() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "public_executive_summary",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_market_snapshot",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_macro",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_bitcoin",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_gold_precious_metals",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_equities",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_news_catalysts",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_scenario_dashboard",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_how_we_analyse",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_allocation_framework",
            visibility: SectionVisibility::Public,
        },
        SectionSpec {
            name: "public_methodology",
            visibility: SectionVisibility::Public,
        },
    ]
}

/// Canonical ordering of the private daily report sections (Step 5b).
pub fn private_section_plan() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "private_bottom_line",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_portfolio_snapshot",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_macro_context",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_per_asset_convergence",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_conviction_trajectory",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_outlook_by_horizon",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_risk_concentration",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_mismatch_surface",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_news_catalysts",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_upcoming_calendar",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_open_predictions",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_lessons_applied",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_self_retrospective_calibration",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_decisions_pending",
            visibility: SectionVisibility::Private,
        },
    ]
}

/// Return the ordered section plan that applies for a given mode.
pub fn section_plan_for(mode: BuildMode) -> Vec<SectionSpec> {
    match mode {
        BuildMode::Public => public_section_plan(),
        BuildMode::Private => private_section_plan(),
        BuildMode::Both => {
            let mut plan = public_section_plan();
            plan.extend(private_section_plan());
            plan
        }
    }
}

/// Render a single section by name against a `BuildContext`.
pub fn render_section(name: &str, ctx: &BuildContext) -> Result<String> {
    match name {
        "public_executive_summary" => sections::public_executive_summary::render_public_executive_summary(ctx),
        "public_market_snapshot" => sections::public_market_snapshot::render_public_market_snapshot(ctx),
        "public_macro" => sections::public_macro::render_public_macro(ctx),
        "public_bitcoin" => sections::public_bitcoin::render_public_bitcoin(ctx),
        "public_gold_precious_metals" => {
            sections::public_gold_precious_metals::render_public_gold_precious_metals(ctx)
        }
        "public_equities" => sections::public_equities::render_public_equities(ctx),
        "public_news_catalysts" => sections::public_news_catalysts::render_public_news_catalysts(ctx),
        "public_scenario_dashboard" => {
            sections::public_scenario_dashboard::render_public_scenario_dashboard(ctx)
        }
        "public_how_we_analyse" => sections::public_how_we_analyse::render_public_how_we_analyse(ctx),
        "public_allocation_framework" => {
            sections::public_allocation_framework::render_public_allocation_framework(ctx)
        }
        "public_methodology" => sections::public_methodology::render_public_methodology(ctx),
        "private_bottom_line" => sections::private_bottom_line::render_private_bottom_line(ctx),
        "private_portfolio_snapshot" => {
            sections::private_portfolio_snapshot::render_private_portfolio_snapshot(ctx)
        }
        "private_macro_context" => sections::private_macro_context::render_private_macro_context(ctx),
        "private_per_asset_convergence" => {
            sections::private_per_asset_convergence::render_private_per_asset_convergence(ctx)
        }
        "private_conviction_trajectory" => {
            sections::private_conviction_trajectory::render_private_conviction_trajectory(ctx)
        }
        "private_outlook_by_horizon" => {
            sections::private_outlook_by_horizon::render_private_outlook_by_horizon(ctx)
        }
        "private_risk_concentration" => {
            sections::private_risk_concentration::render_private_risk_concentration(ctx)
        }
        "private_mismatch_surface" => {
            sections::private_mismatch_surface::render_private_mismatch_surface(ctx)
        }
        "private_news_catalysts" => {
            sections::private_news_catalysts::render_private_news_catalysts(ctx)
        }
        "private_upcoming_calendar" => {
            sections::private_upcoming_calendar::render_private_upcoming_calendar(ctx)
        }
        "private_open_predictions" => {
            sections::private_open_predictions::render_private_open_predictions(ctx)
        }
        "private_lessons_applied" => {
            sections::private_lessons_applied::render_private_lessons_applied(ctx)
        }
        "private_self_retrospective_calibration" => {
            sections::private_self_retrospective_calibration::render_private_self_retrospective_calibration(ctx)
        }
        "private_decisions_pending" => {
            sections::private_decisions_pending::render_private_decisions_pending(ctx)
        }
        other => bail!("unknown report section: {other}"),
    }
}

impl BuildContext {
    /// Load a fresh `BuildContext` for a given report date.
    ///
    /// This is the minimal context loader landed alongside the assembler. It
    /// stamps the report date but leaves the data slots at their defaults so
    /// section renderers degrade to their documented empty-state output. The
    /// richer per-source loaders are tracked as separate TODO items so each
    /// landing stays focused.
    pub fn load(backend: &BackendConnection, report_date: &str) -> Result<Self> {
        let recommendation_accuracy_7d = backend
            .sqlite_native()
            .and_then(|conn| {
                crate::db::recommendations::rolling_hit_rate(conn, report_date, 7, 0.0).ok()
            })
            .flatten()
            .map(|r| RecommendationAccuracySummary {
                window_days: r.window_days,
                scored: r.scored,
                hits: r.hits,
                hit_rate_pct: r.hit_rate_pct,
                avg_score: r.avg_score,
            });
        Ok(BuildContext {
            report_date: Some(report_date.to_string()),
            recommendation_accuracy_7d,
            ..BuildContext::default()
        })
    }

    /// Convenience: a context with only the report date populated, for tests
    /// and dry-runs that don't need DB access.
    pub fn for_date(report_date: &str) -> Self {
        BuildContext {
            report_date: Some(report_date.to_string()),
            ..BuildContext::default()
        }
    }
}

/// Snapshot of which data slots in a `BuildContext` are populated. Used by the
/// dry-run output so operators can see what would feed the assembly without
/// triggering a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataAvailabilityRow {
    pub field: &'static str,
    pub populated: bool,
}

pub fn data_availability(ctx: &BuildContext) -> Vec<DataAvailabilityRow> {
    macro_rules! vec_row {
        ($field:expr, $value:expr) => {
            DataAvailabilityRow {
                field: $field,
                populated: !$value.is_empty(),
            }
        };
    }
    macro_rules! opt_row {
        ($field:expr, $value:expr) => {
            DataAvailabilityRow {
                field: $field,
                populated: $value.is_some(),
            }
        };
    }

    vec![
        vec_row!("data_freshness", ctx.data_freshness),
        opt_row!("synthesis", ctx.synthesis),
        opt_row!("regime", ctx.regime),
        vec_row!("market_snapshot", ctx.market_snapshot),
        vec_row!("news_catalysts", ctx.news_catalysts),
        vec_row!("macro_indicators", ctx.macro_indicators),
        vec_row!("economic_calendar", ctx.economic_calendar),
        opt_row!("bitcoin_market", ctx.bitcoin_market),
        vec_row!("precious_metals_market", ctx.precious_metals_market),
        vec_row!("equity_indices", ctx.equity_indices),
        vec_row!("public_scenarios", ctx.public_scenarios),
        opt_row!("private_portfolio_snapshot", ctx.private_portfolio_snapshot),
        vec_row!("private_positions", ctx.private_positions),
        vec_row!("private_open_predictions", ctx.private_open_predictions),
        opt_row!("private_lessons_applied", ctx.private_lessons_applied),
    ]
}

/// What the assembler will do without doing it.
#[derive(Debug, Clone)]
pub struct DryRunSummary {
    pub mode: BuildMode,
    pub report_date: String,
    pub plan: Vec<SectionSpec>,
    pub data_availability: Vec<DataAvailabilityRow>,
    pub output_paths: Vec<PathBuf>,
    pub privacy_audit_status: String,
}

impl DryRunSummary {
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "pftui report build daily --dry-run\n  mode: {}\n  date: {}\n\n",
            self.mode.as_str(),
            self.report_date
        ));
        out.push_str("Section plan:\n");
        for (idx, spec) in self.plan.iter().enumerate() {
            out.push_str(&format!(
                "  {:>2}. [{}] {}\n",
                idx + 1,
                match spec.visibility {
                    SectionVisibility::Public => "pub",
                    SectionVisibility::Private => "prv",
                },
                spec.name
            ));
        }
        out.push_str("\nData availability:\n");
        for row in &self.data_availability {
            out.push_str(&format!(
                "  - {:<32} {}\n",
                row.field,
                if row.populated { "present" } else { "missing" }
            ));
        }
        out.push_str("\nOutput paths (not written):\n");
        for path in &self.output_paths {
            out.push_str(&format!("  - {}\n", path.display()));
        }
        out.push_str(&format!(
            "\nPrivacy audit: {}\n",
            self.privacy_audit_status
        ));
        out
    }
}

/// Tokens the public report MUST NOT contain. The presence of any of these is
/// a signal that personal-portfolio framing leaked into the analytical core.
pub const PUBLIC_PRIVACY_FORBIDDEN_TOKENS: &[&str] = &[
    "my portfolio",
    "my position",
    "my holdings",
    "i hold",
    "i own",
    "we own",
    "our position",
    "our holdings",
    "your portfolio",
    "your position",
    "cost basis",
    "unrealized pnl",
    "unrealised pnl",
    "skylar",
    "CONFIDENTIAL",
    "For the operator",
];

/// Tokens that name SECTION HEADINGS exclusive to the private build. These
/// must never appear in public markdown — they signal a private section was
/// concatenated into the public output.
pub const PRIVATE_SECTION_HEADINGS: &[&str] = &[
    "## Bottom Line",
    "## Portfolio Snapshot",
    "## Per-Asset Convergence",
    "## Decisions Pending",
    "## Mismatch Surface",
    "## Open Predictions Resolving",
    "## Self-Retrospective Calibration",
];

/// A privacy-guard violation discovered in candidate public markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivacyViolation {
    pub token: String,
    pub line_number: usize,
    pub line: String,
}

/// Scan a candidate public-mode markdown body for tokens that would leak
/// personal-portfolio framing. Returns the list of violations (empty when the
/// document is clean). The check is intentionally case-insensitive on prose
/// tokens and exact-match on section headings.
pub fn audit_public_markdown(body: &str) -> Vec<PrivacyViolation> {
    let mut violations = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        for token in PUBLIC_PRIVACY_FORBIDDEN_TOKENS {
            let needle = token.to_ascii_lowercase();
            if lower.contains(&needle) {
                violations.push(PrivacyViolation {
                    token: (*token).to_string(),
                    line_number: idx + 1,
                    line: line.to_string(),
                });
            }
        }
        for heading in PRIVATE_SECTION_HEADINGS {
            if line.starts_with(heading) {
                violations.push(PrivacyViolation {
                    token: (*heading).to_string(),
                    line_number: idx + 1,
                    line: line.to_string(),
                });
            }
        }
    }
    violations
}

/// Concatenate sections in `plan` order, separating with a blank line.
pub fn assemble_markdown(ctx: &BuildContext, plan: &[SectionSpec]) -> Result<String> {
    let mut parts = Vec::with_capacity(plan.len());
    for spec in plan {
        let body = render_section(spec.name, ctx)
            .with_context(|| format!("failed to render section {}", spec.name))?;
        parts.push(body);
    }
    Ok(parts.join("\n\n"))
}

/// Assemble the public analytical-core markdown only. Enforces the privacy
/// guarantee before returning.
pub fn assemble_public(ctx: &BuildContext) -> Result<String> {
    let plan = public_section_plan();
    let body = assemble_markdown(ctx, &plan)?;
    let violations = audit_public_markdown(&body);
    if !violations.is_empty() {
        let detail = violations
            .iter()
            .take(5)
            .map(|v| format!("L{}: {} ({})", v.line_number, v.token, v.line))
            .collect::<Vec<_>>()
            .join("; ");
        bail!(
            "public privacy guard rejected assembled markdown ({} violation(s)): {}",
            violations.len(),
            detail
        );
    }
    Ok(body)
}

/// Assemble the private markdown (uses private section plan only — the public
/// analytical core is intentionally not duplicated into the private file;
/// `--mode both` produces TWO separate documents, one per destination).
pub fn assemble_private(ctx: &BuildContext) -> Result<String> {
    let plan = private_section_plan();
    assemble_markdown(ctx, &plan)
}

/// Same as [`assemble_private`] but, before rendering, persists any decision
/// cards to the `recommendations` table and annotates the rendered
/// `private_decisions_pending` section with `<!-- rec_id: N -->` markers so
/// every card resolves to a stable database row.
pub fn assemble_private_with_persist(
    ctx: &BuildContext,
    backend: &crate::db::backend::BackendConnection,
    report_date: &str,
) -> Result<String> {
    let cards = crate::report::sections::private_decisions_pending::build_cards(ctx);
    let mut annotated = cards.clone();
    if let Some(conn) = backend.sqlite_native() {
        for card in annotated.iter_mut() {
            let id = crate::db::recommendations::upsert_recommendation(
                conn,
                &crate::db::recommendations::RecommendationInsert {
                    report_date,
                    asset: Some(card.symbol.as_str()),
                    recommendation_type: card.recommendation_type.as_str(),
                    urgency: card.urgency.as_str(),
                    rationale_summary: Some(card.context_lines.join(" | ").as_str()),
                },
            )?;
            card.rec_id = Some(id);
        }
    }
    let plan = private_section_plan();
    let mut parts = Vec::with_capacity(plan.len());
    for spec in plan.iter() {
        let body = if spec.name == "private_decisions_pending" {
            crate::report::sections::private_decisions_pending::render_private_decisions_pending_with_cards(&annotated)
        } else {
            render_section(spec.name, ctx)
                .with_context(|| format!("failed to render section {}", spec.name))?
        };
        parts.push(body);
    }
    Ok(parts.join("\n\n"))
}

/// Persist every decision card derived from the context as a `recommendations`
/// row. Idempotent: if a `(report_date, asset, recommendation_type)` row
/// already exists, its id is returned without modification. The returned
/// vector is parallel to the card order.
pub fn persist_recommendations_from_context(
    backend: &crate::db::backend::BackendConnection,
    ctx: &BuildContext,
    report_date: &str,
) -> Result<Vec<i64>> {
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };
    let cards = crate::report::sections::private_decisions_pending::build_cards(ctx);
    let mut ids = Vec::with_capacity(cards.len());
    for card in &cards {
        let rationale = card.context_lines.join(" | ");
        let id = crate::db::recommendations::upsert_recommendation(
            conn,
            &crate::db::recommendations::RecommendationInsert {
                report_date,
                asset: Some(card.symbol.as_str()),
                recommendation_type: card.recommendation_type.as_str(),
                urgency: card.urgency.as_str(),
                rationale_summary: Some(rationale.as_str()),
            },
        )?;
        ids.push(id);
    }
    Ok(ids)
}

/// Default public output directory: `<HOME>/pftui/reports`.
pub fn default_public_out_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        home.join("pftui").join("reports")
    } else {
        PathBuf::from("./reports")
    }
}

/// Default private output directory: `/tmp`.
pub fn default_private_out_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Compute the markdown destination for one mode/date pair.
pub fn output_path(mode: BuildMode, date: &str, out_dir: &Path) -> PathBuf {
    let filename = match mode {
        BuildMode::Public => format!("daily-{date}.md"),
        BuildMode::Private => format!("pftui-private-{date}.md"),
        BuildMode::Both => format!("daily-{date}.md"),
    };
    out_dir.join(filename)
}

/// Plan of what `assemble` would produce, before any I/O.
#[derive(Debug, Clone)]
pub struct AssemblyPlan {
    pub mode: BuildMode,
    pub date: String,
    pub public_path: Option<PathBuf>,
    pub private_path: Option<PathBuf>,
    pub sections: Vec<SectionSpec>,
}

/// Build a plan describing every output the assembler will write.
pub fn plan_assembly(
    mode: BuildMode,
    date: &str,
    public_out_dir: Option<&Path>,
    private_out_dir: Option<&Path>,
) -> AssemblyPlan {
    let pub_dir = public_out_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_public_out_dir);
    let prv_dir = private_out_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_private_out_dir);

    let public_path = match mode {
        BuildMode::Public | BuildMode::Both => Some(output_path(BuildMode::Public, date, &pub_dir)),
        BuildMode::Private => None,
    };
    let private_path = match mode {
        BuildMode::Private | BuildMode::Both => {
            Some(output_path(BuildMode::Private, date, &prv_dir))
        }
        BuildMode::Public => None,
    };

    AssemblyPlan {
        mode,
        date: date.to_string(),
        public_path,
        private_path,
        sections: section_plan_for(mode),
    }
}

/// Outcome of a successful assembly write.
#[derive(Debug, Clone)]
pub struct AssemblyOutcome {
    pub public_written: Option<PathBuf>,
    pub private_written: Option<PathBuf>,
    pub bytes_written: usize,
}

/// Resolve the report date string, defaulting to today's UTC date.
pub fn resolve_report_date(date: Option<&str>) -> String {
    date.map(|value| value.to_string())
        .unwrap_or_else(|| Utc::now().date_naive().format("%Y-%m-%d").to_string())
}

/// Render a dry-run summary for the requested build.
pub fn render_dry_run(
    ctx: &BuildContext,
    mode: BuildMode,
    date: &str,
    public_out_dir: Option<&Path>,
    private_out_dir: Option<&Path>,
) -> DryRunSummary {
    let plan = plan_assembly(mode, date, public_out_dir, private_out_dir);
    let mut output_paths = Vec::new();
    if let Some(p) = plan.public_path.as_ref() {
        output_paths.push(p.clone());
    }
    if let Some(p) = plan.private_path.as_ref() {
        output_paths.push(p.clone());
    }

    let audit_status = match mode {
        BuildMode::Public | BuildMode::Both => {
            // Run the privacy audit against an assembled public-mode draft.
            match assemble_markdown(ctx, &public_section_plan()) {
                Ok(body) => {
                    let violations = audit_public_markdown(&body);
                    if violations.is_empty() {
                        "would PASS (0 violations against current context)".to_string()
                    } else {
                        format!("would FAIL ({} violations)", violations.len())
                    }
                }
                Err(err) => format!("render-failed: {err}"),
            }
        }
        BuildMode::Private => "skipped (private-only mode)".to_string(),
    };

    DryRunSummary {
        mode,
        report_date: date.to_string(),
        plan: plan.sections,
        data_availability: data_availability(ctx),
        output_paths,
        privacy_audit_status: audit_status,
    }
}

/// Assemble + write the daily report(s) for the requested mode.
///
/// When `backend` is supplied AND points at a SQLite store, the private
/// assembly persists each derived decision card to the `recommendations`
/// table and inlines a `<!-- rec_id: N -->` marker per card. This is the
/// mechanism that drives the Recommendation → action → outcome chain.
pub fn assemble_with_backend(
    ctx: &BuildContext,
    mode: BuildMode,
    date: &str,
    public_out_dir: Option<&Path>,
    private_out_dir: Option<&Path>,
    backend: Option<&crate::db::backend::BackendConnection>,
) -> Result<AssemblyOutcome> {
    let plan = plan_assembly(mode, date, public_out_dir, private_out_dir);
    let mut outcome = AssemblyOutcome {
        public_written: None,
        private_written: None,
        bytes_written: 0,
    };
    if let Some(public_path) = plan.public_path.as_ref() {
        let body = assemble_public(ctx)?;
        if let Some(parent) = public_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create public out-dir {}", parent.display()))?;
        }
        fs::write(public_path, &body)
            .with_context(|| format!("failed to write {}", public_path.display()))?;
        outcome.bytes_written += body.len();
        outcome.public_written = Some(public_path.clone());
    }
    if let Some(private_path) = plan.private_path.as_ref() {
        let body = match backend {
            Some(b) => assemble_private_with_persist(ctx, b, date)?,
            None => assemble_private(ctx)?,
        };
        if let Some(parent) = private_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create private out-dir {}", parent.display())
            })?;
        }
        fs::write(private_path, &body)
            .with_context(|| format!("failed to write {}", private_path.display()))?;
        outcome.bytes_written += body.len();
        outcome.private_written = Some(private_path.clone());
    }
    Ok(outcome)
}

pub fn assemble(
    ctx: &BuildContext,
    mode: BuildMode,
    date: &str,
    public_out_dir: Option<&Path>,
    private_out_dir: Option<&Path>,
) -> Result<AssemblyOutcome> {
    let plan = plan_assembly(mode, date, public_out_dir, private_out_dir);
    let mut outcome = AssemblyOutcome {
        public_written: None,
        private_written: None,
        bytes_written: 0,
    };

    if let Some(public_path) = plan.public_path.as_ref() {
        let body = assemble_public(ctx)?;
        if let Some(parent) = public_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create public out-dir {}", parent.display())
            })?;
        }
        fs::write(public_path, &body)
            .with_context(|| format!("failed to write {}", public_path.display()))?;
        outcome.bytes_written += body.len();
        outcome.public_written = Some(public_path.clone());
    }

    if let Some(private_path) = plan.private_path.as_ref() {
        let body = assemble_private(ctx)?;
        if let Some(parent) = private_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create private out-dir {}", parent.display())
            })?;
        }
        fs::write(private_path, &body)
            .with_context(|| format!("failed to write {}", private_path.display()))?;
        outcome.bytes_written += body.len();
        outcome.private_written = Some(private_path.clone());
    }

    Ok(outcome)
}

#[cfg(test)]
mod assembler_tests {
    use super::*;

    #[test]
    fn section_plan_for_public_only_has_public_sections() {
        let plan = section_plan_for(BuildMode::Public);
        assert!(plan
            .iter()
            .all(|s| matches!(s.visibility, SectionVisibility::Public)));
        assert!(plan.iter().any(|s| s.name == "public_executive_summary"));
        assert!(plan.iter().any(|s| s.name == "public_methodology"));
    }

    #[test]
    fn section_plan_for_private_only_has_private_sections() {
        let plan = section_plan_for(BuildMode::Private);
        assert!(plan
            .iter()
            .all(|s| matches!(s.visibility, SectionVisibility::Private)));
        assert!(plan.iter().any(|s| s.name == "private_bottom_line"));
        assert!(plan.iter().any(|s| s.name == "private_decisions_pending"));
    }

    #[test]
    fn section_plan_for_both_is_public_then_private_in_canonical_order() {
        let plan = section_plan_for(BuildMode::Both);
        let pub_len = public_section_plan().len();
        let prv_len = private_section_plan().len();
        assert_eq!(plan.len(), pub_len + prv_len);
        // First `pub_len` are public, in order.
        for (idx, spec) in public_section_plan().iter().enumerate() {
            assert_eq!(plan[idx].name, spec.name, "public section order");
        }
        for (idx, spec) in private_section_plan().iter().enumerate() {
            assert_eq!(plan[pub_len + idx].name, spec.name, "private section order");
        }
    }

    #[test]
    fn section_ordering_fixture_is_stable() {
        let expected_public: Vec<&str> = vec![
            "public_executive_summary",
            "public_market_snapshot",
            "public_macro",
            "public_bitcoin",
            "public_gold_precious_metals",
            "public_equities",
            "public_news_catalysts",
            "public_scenario_dashboard",
            "public_how_we_analyse",
            "public_allocation_framework",
            "public_methodology",
        ];
        let expected_private: Vec<&str> = vec![
            "private_bottom_line",
            "private_portfolio_snapshot",
            "private_macro_context",
            "private_per_asset_convergence",
            "private_conviction_trajectory",
            "private_outlook_by_horizon",
            "private_risk_concentration",
            "private_mismatch_surface",
            "private_news_catalysts",
            "private_upcoming_calendar",
            "private_open_predictions",
            "private_lessons_applied",
            "private_self_retrospective_calibration",
            "private_decisions_pending",
        ];
        let pub_actual: Vec<&str> = public_section_plan()
            .iter()
            .map(|s| s.name)
            .collect();
        let prv_actual: Vec<&str> = private_section_plan()
            .iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(pub_actual, expected_public);
        assert_eq!(prv_actual, expected_private);
    }

    #[test]
    fn output_path_public_uses_daily_prefix() {
        let p = output_path(BuildMode::Public, "2026-06-02", Path::new("/tmp/x"));
        assert_eq!(p, PathBuf::from("/tmp/x/daily-2026-06-02.md"));
    }

    #[test]
    fn output_path_private_uses_pftui_private_prefix() {
        let p = output_path(BuildMode::Private, "2026-06-02", Path::new("/tmp/x"));
        assert_eq!(p, PathBuf::from("/tmp/x/pftui-private-2026-06-02.md"));
    }

    #[test]
    fn plan_assembly_public_mode_only_emits_public_path() {
        let plan = plan_assembly(
            BuildMode::Public,
            "2026-06-02",
            Some(Path::new("/tmp/a")),
            Some(Path::new("/tmp/b")),
        );
        assert!(plan.public_path.is_some());
        assert!(plan.private_path.is_none());
    }

    #[test]
    fn plan_assembly_private_mode_only_emits_private_path() {
        let plan = plan_assembly(
            BuildMode::Private,
            "2026-06-02",
            Some(Path::new("/tmp/a")),
            Some(Path::new("/tmp/b")),
        );
        assert!(plan.public_path.is_none());
        assert!(plan.private_path.is_some());
    }

    #[test]
    fn plan_assembly_both_emits_two_distinct_paths() {
        let plan = plan_assembly(
            BuildMode::Both,
            "2026-06-02",
            Some(Path::new("/tmp/a")),
            Some(Path::new("/tmp/b")),
        );
        assert_eq!(
            plan.public_path.as_deref(),
            Some(Path::new("/tmp/a/daily-2026-06-02.md"))
        );
        assert_eq!(
            plan.private_path.as_deref(),
            Some(Path::new("/tmp/b/pftui-private-2026-06-02.md"))
        );
    }

    #[test]
    fn resolve_report_date_uses_supplied_value() {
        assert_eq!(resolve_report_date(Some("2026-06-02")), "2026-06-02");
    }

    #[test]
    fn resolve_report_date_falls_back_to_today() {
        let resolved = resolve_report_date(None);
        // Just sanity-check the shape: YYYY-MM-DD, length 10.
        assert_eq!(resolved.len(), 10);
        assert_eq!(resolved.matches('-').count(), 2);
    }

    #[test]
    fn audit_public_markdown_clean_document_has_no_violations() {
        let body = "## Executive Summary\n\nMarkets traded mixed. BTC up 2%.";
        assert!(audit_public_markdown(body).is_empty());
    }

    #[test]
    fn audit_public_markdown_rejects_personal_first_person_tokens() {
        let body = "## Executive Summary\n\nI hold a large BTC position with my portfolio in tact.";
        let violations = audit_public_markdown(body);
        assert!(violations.len() >= 2, "expected at least two violations");
        let tokens: Vec<&str> = violations.iter().map(|v| v.token.as_str()).collect();
        assert!(tokens.iter().any(|t| t.contains("my portfolio")));
        assert!(tokens.iter().any(|t| t.contains("I hold") || t.contains("i hold")));
    }

    #[test]
    fn audit_public_markdown_rejects_private_section_headings() {
        let body = "## Executive Summary\n\nBTC trended higher.\n\n## Bottom Line\n\nLeak.";
        let violations = audit_public_markdown(body);
        assert!(
            violations.iter().any(|v| v.token == "## Bottom Line"),
            "expected '## Bottom Line' to be flagged"
        );
    }

    #[test]
    fn audit_public_markdown_rejects_skylar_token() {
        let body = "## Executive Summary\n\nSkylar's journal said the regime is shifting.";
        let violations = audit_public_markdown(body);
        assert!(violations.iter().any(|v| v.token.eq_ignore_ascii_case("skylar")));
    }

    #[test]
    fn assemble_public_clean_context_succeeds() {
        let ctx = BuildContext::for_date("2026-06-02");
        let body = assemble_public(&ctx).expect("public assembly should succeed");
        assert!(body.contains("## Executive Summary"));
        assert!(body.contains("## Methodology"));
    }

    #[test]
    fn assemble_private_clean_context_includes_bottom_line() {
        let ctx = BuildContext::for_date("2026-06-02");
        let body = assemble_private(&ctx).expect("private assembly should succeed");
        assert!(body.contains("## Bottom Line"));
        assert!(body.contains("## Decisions Pending"));
    }

    /// Make a fresh per-test temp directory. Removes on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let dir = std::env::temp_dir()
                .join(format!("pftui-assembler-{tag}-{pid}-{n}"));
            std::fs::create_dir_all(&dir).expect("create tempdir");
            TempDir(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn assemble_writes_public_only_for_public_mode() {
        let tmp = TempDir::new("pub-only");
        let ctx = BuildContext::for_date("2026-06-02");
        let outcome = assemble(
            &ctx,
            BuildMode::Public,
            "2026-06-02",
            Some(tmp.path()),
            Some(tmp.path()),
        )
        .unwrap();
        assert!(outcome.public_written.is_some());
        assert!(outcome.private_written.is_none());
        assert!(outcome.public_written.unwrap().exists());
        // Confirm private path was NOT created.
        let private_path = tmp.path().join("pftui-private-2026-06-02.md");
        assert!(!private_path.exists());
    }

    #[test]
    fn assemble_writes_both_files_for_both_mode() {
        let tmp_pub = TempDir::new("both-pub");
        let tmp_prv = TempDir::new("both-prv");
        let ctx = BuildContext::for_date("2026-06-02");
        let outcome = assemble(
            &ctx,
            BuildMode::Both,
            "2026-06-02",
            Some(tmp_pub.path()),
            Some(tmp_prv.path()),
        )
        .unwrap();
        assert!(outcome.public_written.is_some());
        assert!(outcome.private_written.is_some());
        assert!(outcome.public_written.unwrap().exists());
        assert!(outcome.private_written.unwrap().exists());
    }

    #[test]
    fn dry_run_writes_no_files() {
        let tmp = TempDir::new("dry-run");
        let ctx = BuildContext::for_date("2026-06-02");
        let summary = render_dry_run(
            &ctx,
            BuildMode::Both,
            "2026-06-02",
            Some(tmp.path()),
            Some(tmp.path()),
        );
        // Sanity-check summary content.
        assert_eq!(summary.mode, BuildMode::Both);
        assert!(!summary.plan.is_empty());
        assert!(summary.output_paths.len() == 2);
        // Confirm no files were created.
        let pub_path = tmp.path().join("daily-2026-06-02.md");
        let prv_path = tmp.path().join("pftui-private-2026-06-02.md");
        assert!(!pub_path.exists());
        assert!(!prv_path.exists());
    }

    #[test]
    fn dry_run_render_text_includes_section_plan_and_paths() {
        let ctx = BuildContext::for_date("2026-06-02");
        let summary = render_dry_run(
            &ctx,
            BuildMode::Public,
            "2026-06-02",
            Some(Path::new("/tmp/dry")),
            None,
        );
        let text = summary.render_text();
        assert!(text.contains("Section plan"));
        assert!(text.contains("public_executive_summary"));
        assert!(text.contains("/tmp/dry/daily-2026-06-02.md"));
        assert!(text.contains("Privacy audit"));
    }

    #[test]
    fn dry_run_privacy_status_reports_pass_on_clean_context() {
        let ctx = BuildContext::for_date("2026-06-02");
        let summary = render_dry_run(
            &ctx,
            BuildMode::Public,
            "2026-06-02",
            Some(Path::new("/tmp/dry")),
            None,
        );
        assert!(
            summary.privacy_audit_status.contains("PASS"),
            "expected privacy audit to pass on clean context, got: {}",
            summary.privacy_audit_status
        );
    }

    #[test]
    fn dry_run_privacy_status_skipped_for_private_only_mode() {
        let ctx = BuildContext::for_date("2026-06-02");
        let summary = render_dry_run(
            &ctx,
            BuildMode::Private,
            "2026-06-02",
            None,
            Some(Path::new("/tmp/dry")),
        );
        assert!(summary.privacy_audit_status.contains("skipped"));
    }

    #[test]
    fn assembled_markdown_golden_for_public_mode_is_stable() {
        // Pin the assembled-public markdown for a minimal context to a golden
        // SHA-256 digest. Any drift in section content or ordering will surface
        // here, forcing a deliberate update to the golden.
        let ctx = BuildContext::for_date("2026-06-02");
        let body = assemble_public(&ctx).expect("public assembly should succeed");
        let digest = sha256_hex(body.as_bytes());
        // Update this constant when an intentional change to public assembly
        // lands. The point of the golden is to catch *unintentional* drift.
        let golden = include_str!("daily_public_golden.sha256");
        assert_eq!(
            digest.trim(),
            golden.trim(),
            "public assembled-markdown drift; if intentional, refresh src/report/build/daily_public_golden.sha256 to: {}",
            digest
        );
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let out = hasher.finalize();
        let mut s = String::with_capacity(out.len() * 2);
        for byte in out {
            use std::fmt::Write;
            let _ = write!(&mut s, "{:02x}", byte);
        }
        s
    }
}
