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
    /// Cross-asset thesis-chain rows surfaced in the private Macro section
    /// via [`crate::report::sections::thesis_chains_macro::render_thesis_chains_block`].
    /// Populated by `BuildContext::load` from `thesis_dependencies::list`.
    /// Public mode never reads this slot — chains are private-only because
    /// some carry portfolio-framed antecedents.
    pub private_thesis_chains: Vec<crate::db::thesis_dependencies::ThesisDependency>,
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
    /// Latest-per-asset synthesis-time adversary views read from
    /// `adversary_synthesis_views` by `BuildContext::load`. The
    /// per-asset renderer in `report::sections::adversary_view::render_adversary_view_block`
    /// only emits a block when an entry exists for `asset` AND that
    /// entry's `fragility_score >= 3`. Empty when no rows match.
    pub synthesis_adversary_views: Vec<AdversarySynthesisSummary>,
    /// Today's analyst-written synthesis: the substantive content the four
    /// timeframe analysts plus the synthesis-bound agent messages produced
    /// for the report date. Surfaced by the private Bottom Line and public
    /// Executive Summary renderers so the report's opening reflects the
    /// analysts' actual narrative.
    pub todays_analyst_synthesis: Option<TodaysAnalystSynthesis>,
    /// Forward-return distributions from the parallels catalog runner
    /// (`~/.local/bin/pftui-parallels-run`). Loaded from
    /// `/tmp/pftui-parallels-<REPORT_DATE>.json` when present; empty when
    /// the JSON file is missing or malformed. Surfaced by the new
    /// `private_parallels` section as a per-set table of median forward
    /// returns and hit rates.
    pub parallels_results: Vec<ParallelsResult>,
    /// Cross-layer signals destined for the synthesis layer, sourced from
    /// the `agent_messages` table filtered to `to='synthesis'` on the
    /// report date with priority `high` or `normal`. Surfaced by the new
    /// `private_cross_layer_signals` section.
    pub cross_layer_signals: Vec<CrossLayerSignal>,
    /// Parsed investor-panel persona responses for the report date,
    /// pulled from `agent_messages` rows where `from_agent` starts with
    /// `panel-`. Populated when Phase 2b of the report skill spawned
    /// the panel; empty otherwise.
    pub investor_panel: Vec<InvestorPanelResponse>,
    /// Per-asset bullish/bearish/neutral vote tally aggregated across the
    /// panel responses. Derived in `BuildContext::load` directly from
    /// `investor_panel` so the renderer doesn't recompute it.
    pub investor_panel_consensus: Vec<InvestorPanelConsensus>,
    /// Portfolio decision cards written by the decision-architect agent
    /// (Phase 4). Pulled from `agent_messages` where `from_agent =
    /// 'analyst-decisions'` and `category = 'decision-card'`, parsed
    /// from JSON. Surfaced as actionable cards in `private_decisions_pending`
    /// alongside the calendar-event cards.
    pub portfolio_decision_cards: Vec<PortfolioDecisionCard>,
    /// Per-symbol synthesised asset-intelligence blobs derived from the
    /// same substrate as `pftui analytics asset <SYMBOL>`. Populated for
    /// each held private position. Used by the per-asset convergence
    /// renderer to append a "Deeper Analysis" sub-section.
    pub private_asset_intelligence: std::collections::HashMap<String, AssetIntelligenceBlob>,
    /// Pre-synthesised morning-brief summary used by the public Executive
    /// Summary to prepend a headline + central-tension lead. Populated
    /// from the same substrate the `pftui analytics morning-brief --json`
    /// command emits. `None` when the brief is unavailable for the
    /// report date.
    pub morning_brief: Option<MorningBriefSummary>,
    /// Per-asset synthesis digest (bull case / bear case / what-would-change-
    /// my-mind / risk-reward) plus the "economy this week" paragraph, written
    /// by the synthesis-writer pass as `daily_notes` rows with
    /// `author = 'analyst-synthesis'` whose content opens with a
    /// `[synthesis-<SYM>]` or `[synthesis-economy]` header. Surfaced by the
    /// `private_synthesis` section. Empty when no synthesis notes exist for
    /// the report date.
    pub synthesis_notes: SynthesisNotes,
    /// Epistemic-health instrumentation row (`run_health`) for the report
    /// date. Surfaced by the `private_epistemic_health` section, which
    /// auto-suppresses when no row was recorded for the date.
    pub epistemic_health: Option<crate::db::run_health::RunHealth>,
    /// Recommendation-ledger scoreboard lines for held assets that have at
    /// least one scored 90d forward return. Surfaced as a sub-block of the
    /// `private_epistemic_health` section; empty (sub-block suppressed)
    /// while the ledger is still accruing.
    pub recommendation_scoreboard: Vec<RecommendationScoreboardLine>,
    /// META (not a data slot): per-slot load issues recorded by the loaders
    /// in [`BuildContext::load`]. A loader that fails must NEVER abort the
    /// build, but it must NEVER be silent either — it records a
    /// [`SlotIssue::LoaderError`] here so [`data_availability`] and the
    /// integrity footer can distinguish "query failed" from "query
    /// succeeded, nothing there". Keys are slot field names.
    pub slot_issues: SlotIssues,
    /// META (not a data slot): build-time staleness warnings computed by
    /// [`compute_staleness`]. Each warning names the input, carries an
    /// operator-facing message, and lists the section names that must be
    /// annotated inline (the assembler injects a `> ⚠ …` blockquote under
    /// the section heading). Stale data is annotated, never suppressed.
    pub staleness: Vec<StalenessWarning>,
}

/// Fields on `BuildContext` that are NOT data slots (metadata / bookkeeping).
/// Everything else on the struct MUST appear in [`data_availability`] output —
/// enforced by the `every_build_context_slot_is_tracked` conformance test.
/// If you add a data-bearing field to `BuildContext`, add a matching
/// `vec_slot!`/`opt_slot!` line in [`data_availability`]; if you add a
/// metadata field, list it here. Do NOT weaken the conformance test.
pub const BUILD_CONTEXT_META_FIELDS: &[&str] = &["report_date", "slot_issues", "staleness"];

/// Why a data slot is unpopulated. Recorded by loaders into
/// [`BuildContext::slot_issues`]; consumed by [`data_availability`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotIssue {
    /// The query/computation failed. Carries the error string. Must surface
    /// in the integrity footer — a loader error must never render
    /// identically to genuinely-absent data.
    LoaderError(String),
    /// The query succeeded and found rows for EARLIER dates, but none for
    /// the report date — i.e. the upstream phase/routine that writes this
    /// slot did not run today.
    UpstreamNotRun(String),
    /// The query succeeded and there is genuinely nothing there (optional
    /// explanatory reason).
    NoData(String),
}

/// Map of slot field name → recorded issue.
pub type SlotIssues = std::collections::BTreeMap<&'static str, SlotIssue>;

/// A build-time staleness warning: the named input is older than its
/// freshness expectation, so the listed sections get an inline annotation
/// instead of silently rendering old data as current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalenessWarning {
    /// Input name (e.g. "prices", "analyst_views", "economic_data").
    pub input: &'static str,
    /// Operator-facing message, rendered as a `> ⚠ …` blockquote.
    pub message: String,
    /// Section names (from the section plans) that must carry the
    /// annotation when they render.
    pub sections: Vec<&'static str>,
}

/// One held symbol's recommendation-ledger summary for the epistemic-health
/// section: the recorded action mix, the 90d forward-return hit rate, and
/// the per-symbol window-quality delta (mean 90d after ADD − after WAIT).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecommendationScoreboardLine {
    pub symbol: String,
    /// e.g. `add×5 wait×3 hold×1` (every recorded ledger row, scored or not).
    pub action_mix: String,
    /// Scored 90d returns across all actions for the symbol.
    pub n_scored_90d: usize,
    /// Share of scored 90d returns that are positive (percent).
    pub pct_positive_90d: Option<f64>,
    /// Window quality: mean 90d fwd return after ADD − after WAIT (pp).
    /// None until both sides have a scored 90d return.
    pub window_quality_delta_pct: Option<f64>,
}

/// Collapse the ledger scoreboard onto held symbols with ≥1 scored 90d
/// return. Pure aggregation over `db::recommendations::scoreboard` output so
/// it can be unit-tested with synthetic boards.
pub fn scoreboard_lines_for_held(
    board: &crate::db::recommendations::Scoreboard,
    held_symbols: &[String],
) -> Vec<RecommendationScoreboardLine> {
    let held: std::collections::BTreeSet<String> =
        held_symbols.iter().map(|s| s.to_uppercase()).collect();
    /// (action mix as (action, n_total) pairs, scored 90d count, positive 90d count)
    type SymbolAgg = (Vec<(String, usize)>, usize, usize);
    let mut by_symbol: std::collections::BTreeMap<String, SymbolAgg> =
        std::collections::BTreeMap::new();
    for row in &board.rows {
        if !held.contains(&row.symbol) {
            continue;
        }
        let entry = by_symbol.entry(row.symbol.clone()).or_default();
        entry.0.push((row.action.clone(), row.n_total));
        if let Some(c) = &row.h90 {
            entry.1 += c.n;
            entry.2 += c.positive;
        }
    }
    let wq: std::collections::BTreeMap<&str, Option<f64>> = board
        .window_quality
        .iter()
        .map(|w| (w.symbol.as_str(), w.delta_pct))
        .collect();
    by_symbol
        .into_iter()
        .filter(|(_, (_, n90, _))| *n90 > 0)
        .map(|(symbol, (mut mix, n90, pos90))| {
            mix.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let action_mix = mix
                .iter()
                .map(|(action, n)| format!("{action}×{n}"))
                .collect::<Vec<_>>()
                .join(" ");
            RecommendationScoreboardLine {
                window_quality_delta_pct: wq.get(symbol.as_str()).copied().flatten(),
                pct_positive_90d: Some(pos90 as f64 / n90 as f64 * 100.0),
                n_scored_90d: n90,
                action_mix,
                symbol,
            }
        })
        .collect()
}

/// One parallel-set result mirrored from `/tmp/pftui-parallels-<DATE>.json`.
/// The catalog runner produces one entry per set whose `auto_run_when`
/// predicate matches today's market state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParallelsResult {
    pub id: String,
    pub name: String,
    pub symbol: String,
    pub narrative: String,
    pub match_count: u32,
    pub median_5d_pct: Option<f64>,
    pub median_30d_pct: Option<f64>,
    pub median_90d_pct: Option<f64>,
    pub median_180d_pct: Option<f64>,
    pub hit_rate_30d_pct: Option<f64>,
    pub hit_rate_90d_pct: Option<f64>,
    pub error: Option<String>,
}

/// Parsed synthesis digest for the report date, built from `daily_notes`
/// rows authored by `analyst-synthesis`. The synthesis-writer pass emits one
/// note per held asset (content opens `[synthesis-<SYM>]`) plus one economy
/// note (`[synthesis-economy]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SynthesisNotes {
    /// The "economy this week" paragraph (body of the `[synthesis-economy]`
    /// note), when present.
    pub economy: Option<String>,
    /// The long-form operator-focus deep dive (body of any
    /// `[synthesis-deep-dive ...]` note), when present. Rendered by the
    /// `private_operator_deep_dive` section. The header tag can carry a
    /// date suffix so the writer doesn't have to coordinate a single
    /// canonical key with the renderer.
    pub deep_dive: Option<String>,
    /// Macro + news outlook prose body (body of `[synthesis-macro-outlook]`).
    /// Replaces the standalone Macro Context atomic-data block and the
    /// News & Catalysts table with one synthesized 300-500 word read.
    pub macro_outlook: Option<String>,
    /// Closing conclusion prose body (body of `[synthesis-closing]`).
    /// Renders as the final section: gameplan for the coming week,
    /// portfolio reflection, top 3-5 falsifiable triggers to watch.
    pub closing: Option<String>,
    /// External technical-analysis comparison prose (body of
    /// `[synthesis-external-ta]`). The Phase 2c research agent web-
    /// searches outside pftui's news pipeline (TradingView ideas, sell-
    /// side notes, on-chain trackers, retail TA streams) and writes a
    /// per-asset comparison of external reads against our convergence.
    /// Suppressed when the Phase 2c agent didn't run.
    pub external_ta: Option<String>,
    /// Per-asset bull/bear/change-mind/risk-reward blocks, in the order the
    /// notes were written.
    pub assets: Vec<SynthesisAssetNote>,
}

impl SynthesisNotes {
    /// True when any synthesis-writer note landed for the report date.
    /// Drives the `synthesis_notes` slot's populated bit in
    /// [`data_availability`].
    pub fn has_content(&self) -> bool {
        self.economy.is_some()
            || self.deep_dive.is_some()
            || self.macro_outlook.is_some()
            || self.closing.is_some()
            || self.external_ta.is_some()
            || !self.assets.is_empty()
    }
}

/// One per-asset synthesis block: the symbol parsed from the
/// `[synthesis-<SYM>]` header and the note body that follows it (which
/// carries the BULL CASE / BEAR CASE / WHAT WOULD CHANGE MY MIND /
/// RISK / REWARD sub-sections verbatim).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisAssetNote {
    pub symbol: String,
    pub body: String,
}

/// One inbound cross-layer signal pulled from `agent_messages` where
/// `to_agent = 'synthesis'` for the report date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossLayerSignal {
    pub from_layer: String,
    pub to_layer: String,
    pub priority: String,
    pub category: String,
    pub summary: String,
}

/// One investor-panel persona response. Parsed from the JSON content of
/// an `agent_messages` row where `from_agent` starts with `panel-`.
/// Mirrors the structured shape produced by the persona subagent per
/// `~/pftui/agents/investor-panel/schema.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct InvestorPanelResponse {
    pub investor: String,
    pub overall_signal: String,
    pub confidence: u8,
    pub positioning: Vec<InvestorPanelPositioning>,
    pub key_insight: String,
    pub what_would_change_my_mind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestorPanelPositioning {
    pub asset: String,
    pub signal: String,
    pub weight: String,
    pub reasoning: String,
}

/// Aggregated consensus summary across the panel. Used by the renderer to
/// surface "strong consensus" and "high divergence" buckets at a glance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvestorPanelConsensus {
    pub asset: String,
    pub bullish_count: u32,
    pub bearish_count: u32,
    pub neutral_count: u32,
    pub label: String,
}

/// One portfolio decision card written by the decision-architect agent.
/// Parsed from the JSON content of an `agent_messages` row where
/// `from_agent='analyst-decisions'` and `category='decision-card'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioDecisionCard {
    pub symbol: String,
    pub question: String,
    pub evidence_for: Vec<String>,
    pub evidence_against: Vec<String>,
    pub recommendation: String,
    pub what_would_change_it: String,
    pub sizing_math: String,
}

/// Compact synthesised asset-intelligence blob persisted per-asset in the
/// `BuildContext`. Mirrors the most operator-relevant subset of the
/// `pftui analytics asset <SYM>` JSON output without duplicating the full
/// blob.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetIntelligenceBlob {
    pub symbol: String,
    pub spot_price: Option<String>,
    pub daily_change_pct: Option<f64>,
    pub rsi_14: Option<f64>,
    pub rsi_signal: Option<String>,
    pub trend: Option<String>,
    pub nearest_support: Option<String>,
    pub nearest_resistance: Option<String>,
    pub range_52w_position: Option<f64>,
    pub scenario_count: u32,
    pub open_predictions_count: u32,
    pub structural_context: Option<String>,
    /// One-line price-action structure verdict (daily bars) from the
    /// market-structure engine. None when history is too shallow.
    pub structure_verdict_daily: Option<String>,
    /// One-line price-action structure verdict (weekly bars).
    pub structure_verdict_weekly: Option<String>,
    /// Cycle-clock position verdict — BTC and GC=F only.
    pub cycle_clock_verdict: Option<String>,
    /// One-line composite Cyber Dots verdict (daily bars) from the
    /// `analytics::cyber` engine. None when history is too shallow.
    pub cyber_verdict_daily: Option<String>,
    /// Measured signal expectancy for registry signals that fired within
    /// the last 10 days, cited from the persisted `signal_expectancy` table
    /// (90d horizon vs baseline). None when nothing fired recently or no
    /// stats are persisted (run `pftui research backtest`).
    pub signal_expectancy: Option<String>,
}

/// Compact morning-brief summary used to prepend the public Executive
/// Summary. Captures only the most-actionable lead fields; the full
/// brief remains available via `pftui analytics morning-brief --json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MorningBriefSummary {
    pub headline: Option<String>,
    pub central_tension: Option<String>,
}

/// Per-day analyst synthesis, mirrored into the report by
/// [`BuildContext::load`] from `daily_notes` (filtered to
/// `author IN ('analyst-low','analyst-medium','analyst-high','analyst-macro')`)
/// and `agent_messages` (filtered to `to='synthesis'`).
#[derive(Debug, Clone, Default)]
pub struct TodaysAnalystSynthesis {
    pub headline_low: Option<String>,
    pub headline_medium: Option<String>,
    pub headline_high: Option<String>,
    pub headline_macro: Option<String>,
    pub leading_move: Option<MaterialMove>,
    pub action_summary: Option<String>,
}

/// A single material-move row surfaced by the analyst synthesis. Captures
/// the largest |%| move detected in today's analyst notes against a known
/// held asset, plus optional cumulative-from-baseline framing.
#[derive(Debug, Clone)]
pub struct MaterialMove {
    pub asset: String,
    pub move_pct: f64,
    pub cumulative_pct: Option<f64>,
    pub note: String,
}

/// Compact per-asset row mirrored from `adversary_synthesis_views` for
/// the daily-report renderer in `report::sections::adversary_view`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdversarySynthesisSummary {
    pub asset: String,
    pub current_convergence_summary: String,
    pub counter_case_summary: String,
    pub counter_case_evidence_points: Vec<String>,
    pub falsification_triggers: Vec<String>,
    pub fragility_score: i64,
    pub recorded_at: String,
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
    /// ACTIVE forecast misalignment on (layer, asset): rendered with a
    /// probation marker and excluded from the card's net conviction.
    pub probation: bool,
    /// Wrong-sign streak length backing the probation.
    pub probation_streak: Option<i64>,
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
            name: "public_allocation_framework",
            visibility: SectionVisibility::Public,
        },
    ]
}

/// Canonical ordering of the private daily report sections (Step 5b).
pub fn private_section_plan() -> Vec<SectionSpec> {
    vec![
        // Overview opens every private report — operator's explicit ask:
        // "the report should always open with an overview section, human
        // readable, engaging, high level discussion." Reads the
        // synthesis-economy note.
        SectionSpec {
            name: "private_overview",
            visibility: SectionVisibility::Private,
        },
        // Operator Deep Dive — the long-form synthesis tailored to the
        // operator's focus prompt for this run. Suppressed when no
        // deep-dive note exists (i.e. balanced-weekly runs).
        SectionSpec {
            name: "private_operator_deep_dive",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_bottom_line",
            visibility: SectionVisibility::Private,
        },
        // Per-Asset Briefing — one card per held asset with 5-block
        // structure. The 4-layer convergence + asset intelligence flow into
        // the card automatically.
        SectionSpec {
            name: "private_synthesis",
            visibility: SectionVisibility::Private,
        },
        SectionSpec {
            name: "private_portfolio_snapshot",
            visibility: SectionVisibility::Private,
        },
        // Macro & News Outlook — synthesized prose replacing the
        // standalone Macro Context atomic-data block + News & Catalysts
        // table. Suppressed when the synthesis writer didn't author one.
        SectionSpec {
            name: "private_macro_news_outlook",
            visibility: SectionVisibility::Private,
        },
        // Per-asset Convergence Trajectory + Outlook + Risk Concentration
        // are kept because they auto-suppress when there's no substantive
        // data. They render only when the analyst writes produced
        // something worth surfacing.
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
        // Investor Panel Consensus (panel section is now consensus-only
        // by default; the persona-detail table is suppressed unless a
        // strong divergence warrants surfacing it).
        SectionSpec {
            name: "private_investor_panel",
            visibility: SectionVisibility::Private,
        },
        // External TA & Comparison — Phase 2c research agent's web-
        // sourced TA reads + per-asset comparison vs our convergence.
        // Auto-suppressed when no external-ta note was attached.
        SectionSpec {
            name: "private_external_ta",
            visibility: SectionVisibility::Private,
        },
        // Quantitative Parallels — table self-suppresses when no set
        // returned matches.
        SectionSpec {
            name: "private_parallels",
            visibility: SectionVisibility::Private,
        },
        // Closing — synthesized conclusion (Gameplan / Portfolio
        // reflection / What to Watch). Last substantive section.
        SectionSpec {
            name: "private_closing",
            visibility: SectionVisibility::Private,
        },
        // Epistemic Health — run_health instrumentation for the report
        // date. Meta-content (how the machine ran, not what it believes),
        // so it goes after the closing, at the very end. Auto-suppressed
        // when no run_health row exists for the date.
        SectionSpec {
            name: "private_epistemic_health",
            visibility: SectionVisibility::Private,
        },
        // Sections intentionally dropped from the default plan per
        // operator feedback (2026-06-07): too much unformatted data,
        // walls of text. Still callable via render_section() and the
        // Composition step can pull them in when warranted, but they
        // don't render by default:
        //
        //   private_macro_context     — atomic data; macro+news outlook
        //                               (above) is the synthesized
        //                               replacement.
        //   private_macro_thesis_chains — niche; surfaced inline when
        //                                 thesis chains are written.
        //   private_mismatch_surface  — auto-suppressed when aligned;
        //                               the synthesis writer narrates
        //                               mismatches inline.
        //   private_news_catalysts    — replaced by macro+news outlook.
        //   private_upcoming_calendar — operator scans the synthesized
        //                               outlook for next-week binaries.
        //   private_open_predictions  — 8 pages of cards; operator can
        //                               run `pftui journal prediction
        //                               list` for the canonical view.
        //   private_lessons_applied   — surfaced inline in the synthesis
        //                               writer's prose when material.
        //   private_self_retrospective_calibration — auto-suppressed
        //                               when calibration_matrix empty.
        //   private_cross_layer_signals — replaced by the synthesis
        //                               writer's bullets in the per-
        //                               asset cards + the Outlook prose.
        //
        //   private_decisions_pending — surfaced via chat (Step 11),
        //                               not PDF.
        //   private_per_asset_convergence — integrated into per-asset
        //                                   card (Current bias block).
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
        "private_overview" => sections::private_overview::render_private_overview(ctx),
        "private_operator_deep_dive" => {
            sections::private_operator_deep_dive::render_private_operator_deep_dive(ctx)
        }
        "private_macro_news_outlook" => {
            sections::private_macro_news_outlook::render_private_macro_news_outlook(ctx)
        }
        "private_external_ta" => {
            sections::private_external_ta::render_private_external_ta(ctx)
        }
        "private_closing" => sections::private_closing::render_private_closing(ctx),
        "private_epistemic_health" => {
            sections::private_epistemic_health::render_private_epistemic_health(ctx)
        }
        "private_synthesis" => sections::private_synthesis::render_private_synthesis(ctx),
        "private_portfolio_snapshot" => {
            sections::private_portfolio_snapshot::render_private_portfolio_snapshot(ctx)
        }
        "private_macro_context" => sections::private_macro_context::render_private_macro_context(ctx),
        "private_macro_thesis_chains" => {
            // The block renderer is shared (also embeddable inside Macro
            // Context) so it returns a bare empty string; translate that to
            // a reasoned suppression at the section boundary.
            sections::thesis_chains_macro::render_thesis_chains_block(&ctx.private_thesis_chains)
                .map(|body| {
                    if body.trim().is_empty() {
                        sections::suppressed(
                            "no confirmed/disconfirmed thesis chains to surface",
                        )
                    } else {
                        body
                    }
                })
        }
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
        "private_cross_layer_signals" => {
            sections::private_cross_layer_signals::render_private_cross_layer_signals(ctx)
        }
        "private_investor_panel" => {
            sections::private_investor_panel::render_private_investor_panel(ctx)
        }
        "private_parallels" => {
            sections::private_parallels::render_private_parallels(ctx)
        }
        "private_decisions_pending" => {
            sections::private_decisions_pending::render_private_decisions_pending(ctx)
        }
        other => bail!("unknown report section: {other}"),
    }
}

/// Record a loader error against a slot. Loader errors never abort the
/// build (resilience) but never go silent (honesty): they surface in
/// `data_availability` and the private report's integrity footer.
fn note_error(issues: &mut SlotIssues, slot: &'static str, err: impl std::fmt::Display) {
    issues.insert(slot, SlotIssue::LoaderError(err.to_string()));
}

/// Record the same loader error against several slots (used when one shared
/// query feeds multiple slots — e.g. the news query feeds five).
fn note_error_many(issues: &mut SlotIssues, slots: &[&'static str], err: &str) {
    for slot in slots {
        issues.insert(slot, SlotIssue::LoaderError(err.to_string()));
    }
}

/// Unwrap a loader result, recording an error against `slot` on failure.
fn load_slot<T, E: std::fmt::Display>(
    issues: &mut SlotIssues,
    slot: &'static str,
    res: std::result::Result<T, E>,
) -> Option<T> {
    match res {
        Ok(v) => Some(v),
        Err(e) => {
            note_error(issues, slot, e);
            None
        }
    }
}

/// Run a SQLite-only loader for `slot`, recording a loader error when the
/// backend is not native SQLite or when the closure fails.
fn load_sqlite_slot<T, F>(
    issues: &mut SlotIssues,
    slot: &'static str,
    backend: &BackendConnection,
    f: F,
) -> Option<T>
where
    F: FnOnce(&rusqlite::Connection) -> Result<T>,
{
    match backend.sqlite_native() {
        Some(conn) => load_slot(issues, slot, f(conn)),
        None => {
            note_error(issues, slot, "backend is not native SQLite; slot loader skipped");
            None
        }
    }
}

/// Parse a flexible timestamp ("RFC3339", "YYYY-MM-DD HH:MM:SS", or bare
/// date) into a UTC datetime. Naive timestamps are assumed UTC.
fn parse_flexible_ts(raw: &str) -> Option<chrono::DateTime<Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, fmt) {
            return Some(chrono::DateTime::from_naive_utc_and_offset(dt, Utc));
        }
    }
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| chrono::DateTime::from_naive_utc_and_offset(dt, Utc))
}

/// Render an age in hours as a compact human string ("3h", "2 days").
fn human_age(hours: f64) -> String {
    if hours < 48.0 {
        format!("{:.0}h", hours.max(0.0))
    } else {
        format!("{:.0} days", hours / 24.0)
    }
}

/// Freshness gate for analyst views (hours) — mirrors the report skill's
/// "views within 6h" gate for Phase 1.
const ANALYST_VIEWS_FRESHNESS_HOURS: f64 = 6.0;

/// Build-time staleness pass. For inputs with freshness expectations,
/// compare the newest datapoint against the report date / wall clock and
/// emit inline-annotation warnings. Series with a registered SLA in
/// `series_registry` (sentiment, economic) reuse that SLA; analyst views
/// use the 6h skill gate; prices compare last fetch vs the report date.
/// Purely advisory — stale inputs are annotated, never suppressed.
pub fn compute_staleness(
    backend: &BackendConnection,
    report_date: &str,
    all_views: &[crate::db::analyst_views::AnalystView],
    prices: &[crate::models::price::PriceQuote],
) -> Vec<StalenessWarning> {
    let now = Utc::now();
    let today = now.date_naive().format("%Y-%m-%d").to_string();
    // For historical rebuilds, measure age against the report date's end of
    // day rather than the wall clock so old reports aren't all "stale".
    let reference = if report_date == today.as_str() {
        now
    } else {
        parse_flexible_ts(&format!("{report_date} 23:59:59")).unwrap_or(now)
    };
    let mut out = Vec::new();

    // 1. Analyst views — the skill's 6h gate.
    if let Some(newest) = all_views
        .iter()
        .filter_map(|v| parse_flexible_ts(&v.updated_at))
        .max()
    {
        let age_hours = (reference - newest).num_seconds() as f64 / 3600.0;
        if age_hours > ANALYST_VIEWS_FRESHNESS_HOURS {
            out.push(StalenessWarning {
                input: "analyst_views",
                message: format!(
                    "⚠ analyst views are {} old (freshness gate {}h) — run Phase 1 before trusting convergence",
                    human_age(age_hours),
                    ANALYST_VIEWS_FRESHNESS_HOURS as i64
                ),
                sections: vec![
                    "private_synthesis",
                    "private_outlook_by_horizon",
                    "private_conviction_trajectory",
                ],
            });
        }
    }

    // 2. Prices — the cache must have been refreshed on (or after) the
    //    report date for "current price" framing to be honest.
    if let Some(newest_fetch) = prices
        .iter()
        .filter_map(|q| parse_flexible_ts(&q.fetched_at))
        .max()
    {
        let fetch_date = newest_fetch.date_naive().format("%Y-%m-%d").to_string();
        if fetch_date.as_str() < report_date {
            out.push(StalenessWarning {
                input: "prices",
                message: format!(
                    "⚠ price cache last refreshed {fetch_date} — older than report date {report_date}; run `pftui data refresh` before trusting quoted prices"
                ),
                sections: vec!["public_market_snapshot", "private_portfolio_snapshot"],
            });
        }
    }

    // 3. Registered series SLAs — sentiment + economic kinds from
    //    `series_registry` (prices are covered by the report-date check
    //    above; other kinds have no report section to annotate yet).
    if let Some(conn) = backend.sqlite_native() {
        if let Ok(statuses) = crate::db::series_registry::status_all(conn, reference) {
            for (kind, sections) in [
                (
                    "economic",
                    vec!["public_macro", "private_macro_news_outlook"],
                ),
                ("sentiment", vec!["public_bitcoin"]),
            ] {
                let stale: Vec<&crate::db::series_registry::SeriesStatus> = statuses
                    .iter()
                    .filter(|s| s.entry.kind == kind && s.stale && s.age_hours.is_some())
                    .collect();
                let Some(worst) = stale.iter().max_by(|a, b| {
                    a.age_hours
                        .partial_cmp(&b.age_hours)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) else {
                    continue;
                };
                let age = worst.age_hours.unwrap_or_default();
                out.push(StalenessWarning {
                    input: if kind == "economic" {
                        "economic_data"
                    } else {
                        "sentiment"
                    },
                    message: format!(
                        "⚠ {} {kind} series past freshness SLA (worst: {} is {} old, SLA {}h) — figures below may lag",
                        stale.len(),
                        worst.entry.series_id,
                        human_age(age),
                        worst.entry.freshness_sla_hours
                    ),
                    sections,
                });
            }
        }
    }

    out
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
        // Every per-source loader below degrades to empty on error: a missing
        // or malformed source must never abort the whole report build. BUT a
        // failure is never silent — each loader records a `SlotIssue` into
        // `ctx.slot_issues` so `data_availability` and the integrity footer
        // can distinguish loader_error / upstream_not_run / no_data. We
        // thread `report_date` through so weekly-change / freshness / calendar
        // math is anchored to the report's day, not wall-clock now.
        let mut ctx = BuildContext {
            report_date: Some(report_date.to_string()),
            ..BuildContext::default()
        };

        ctx.recommendation_accuracy_7d = load_sqlite_slot(
            &mut ctx.slot_issues,
            "recommendation_accuracy_7d",
            backend,
            |conn| crate::db::recommendations::rolling_hit_rate(conn, report_date, 7, 0.0),
        )
        .flatten()
        .map(|r| RecommendationAccuracySummary {
            window_days: r.window_days,
            scored: r.scored,
            hits: r.hits,
            hit_rate_pct: r.hit_rate_pct,
            avg_score: r.avg_score,
        });
        ctx.synthesis_adversary_views = load_sqlite_slot(
            &mut ctx.slot_issues,
            "synthesis_adversary_views",
            backend,
            load_latest_synthesis_adversary_views,
        )
        .unwrap_or_default();
        // Load all chains for the private Macro thesis-chains renderer. The
        // renderer itself filters down to confirmed / disconfirmed rows, so
        // we pass the full list here. Public mode never reads this slot.
        ctx.private_thesis_chains = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_thesis_chains",
            backend,
            |conn| crate::db::thesis_dependencies::list(conn, None, None),
        )
        .unwrap_or_default();

        // Data freshness — reuse the `data status` backend so the report's
        // freshness table matches the operator-facing status command exactly.
        ctx.data_freshness = load_slot(
            &mut ctx.slot_issues,
            "data_freshness",
            crate::commands::status::source_statuses_backend(backend),
        )
            .map(|rows| {
                rows.into_iter()
                    .map(|s| DataFreshnessSummary {
                        source: s.name.to_string(),
                        last_fetch: s.last_fetch,
                        records: Some(s.records as u64),
                        status: s.status.as_lowercase_str().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Latest narrative snapshot drives synthesis + scenario 7d deltas.
        let narrative = load_sqlite_slot(&mut ctx.slot_issues, "synthesis", backend, |conn| {
            crate::db::narrative_snapshots::latest_snapshot(conn)
        })
        .flatten()
        .and_then(|rec| serde_json::from_str::<serde_json::Value>(&rec.report_json).ok());

        // Regime — latest classified snapshot.
        ctx.regime = load_slot(
            &mut ctx.slot_issues,
            "regime",
            crate::db::regime_snapshots::get_current_backend(backend),
        )
            .flatten()
            .map(|snap| {
                let detail = snap.drivers.as_deref().and_then(|raw| {
                    serde_json::from_str::<Vec<String>>(raw)
                        .ok()
                        .map(|v| v.join("; "))
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            let trimmed = raw.trim();
                            (!trimmed.is_empty()).then(|| trimmed.to_string())
                        })
                });
                let detail = match (detail, snap.confidence) {
                    (Some(d), Some(c)) => Some(format!("{d} (confidence {:.0}%)", c * 100.0)),
                    (Some(d), None) => Some(d),
                    (None, Some(c)) => Some(format!("confidence {:.0}%", c * 100.0)),
                    (None, None) => None,
                };
                RegimeSummary {
                    classification: snap.regime,
                    detail,
                }
            });

        // Synthesis — derived from the narrative snapshot headline/subtitle,
        // falling back to the regime classification so the section is never
        // empty when a regime exists.
        ctx.synthesis = narrative
            .as_ref()
            .and_then(synthesis_from_narrative)
            .or_else(|| {
                ctx.regime.as_ref().map(|r| SynthesisSnapshot {
                    summary: format!(
                        "Regime classifier reads {}.",
                        r.classification.replace(['_', '-'], " ")
                    ),
                    central_tension: r.detail.clone(),
                })
            });

        // Convergence across all assets with views in the lookback window.
        ctx.analyst_convergence = load_slot(
            &mut ctx.slot_issues,
            "analyst_convergence",
            crate::db::analyst_views::convergence_all_backend(backend, Some("7d")),
        )
                .map(|reports| {
                    reports
                        .into_iter()
                        .map(|r| AnalystConvergenceSummary {
                            asset: r.asset,
                            summary: r.summary,
                        })
                        .collect()
                })
                .unwrap_or_default();

        // Per-layer analyst views, grouped per asset-class for each section.
        let all_views =
            match crate::db::analyst_views::list_views_backend(backend, None, None, None) {
                Ok(views) => views,
                Err(e) => {
                    note_error_many(
                        &mut ctx.slot_issues,
                        &[
                            "macro_analyst_views",
                            "bitcoin_analyst_views",
                            "precious_metals_analyst_views",
                            "equity_analyst_views",
                            "private_outlooks",
                        ],
                        &e.to_string(),
                    );
                    Vec::new()
                }
            };
        ctx.macro_analyst_views = analyst_views_for(&all_views, MACRO_ASSETS);
        ctx.bitcoin_analyst_views = analyst_views_for(&all_views, BITCOIN_ASSETS);
        ctx.precious_metals_analyst_views = analyst_views_for(&all_views, METALS_ASSETS);
        ctx.equity_analyst_views = analyst_views_for(&all_views, EQUITY_ASSETS);

        // Scenarios — current probabilities from `scenarios`, 7d deltas mapped
        // from the latest narrative snapshot's `scenario_shifts`.
        let shift_map = scenario_shift_map(narrative.as_ref());
        let scenarios =
            match crate::db::scenarios::list_scenarios_backend(backend, Some("active")) {
                Ok(rows) => rows,
                Err(e) => {
                    note_error_many(
                        &mut ctx.slot_issues,
                        &["public_scenarios", "scenario_deltas"],
                        &e.to_string(),
                    );
                    Vec::new()
                }
            };
        ctx.public_scenarios = scenarios
            .iter()
            .map(|s| PublicScenarioRow {
                name: s.name.clone(),
                probability: s.probability,
                delta_7d: shift_map.get(&s.name).copied(),
                narrative_vs_money: None,
                key_driver: s
                    .triggers
                    .as_deref()
                    .map(first_sentence)
                    .filter(|s| !s.is_empty()),
                confirmation: None,
                invalidation: None,
            })
            .collect();
        ctx.scenario_deltas = scenarios
            .iter()
            .map(|s| ScenarioDeltaSummary {
                name: s.name.clone(),
                probability: s.probability,
                delta_7d: shift_map.get(&s.name).copied(),
            })
            .collect();

        // Economic calendar — upcoming high/medium-impact events from the
        // report date forward.
        let upcoming_events = match crate::db::calendar_cache::get_upcoming_events_backend(
            backend,
            report_date,
            60,
        ) {
            Ok(rows) => rows,
            Err(e) => {
                note_error_many(
                    &mut ctx.slot_issues,
                    &[
                        "economic_calendar",
                        "private_macro_catalysts",
                        "private_binary_catalysts",
                    ],
                    &e.to_string(),
                );
                Vec::new()
            }
        };
        // Surface the corrected impact bucket (effective_impact takes the
        // higher of the stored value vs. the name-based heuristic) so cache
        // rows mis-tagged "low" by the scraper render with their real weight.
        ctx.economic_calendar = upcoming_events
            .iter()
            .take(12)
            .map(|e| EconomicCalendarEvent {
                date: e.date.clone(),
                event: e.name.clone(),
                importance: Some(effective_impact(e)),
                market_relevance: e.forecast.as_ref().map(|f| format!("forecast {f}")),
            })
            .collect();

        // Macro catalysts (private, broader horizon) and binary catalysts
        // (private, decision-grade — narrowed to high-impact events in the
        // next two weeks). Both derive from the same upcoming-events list to
        // keep the report internally consistent.
        ctx.private_macro_catalysts = calendar_to_macro_catalysts(&upcoming_events, 10);
        ctx.private_binary_catalysts =
            calendar_to_binary_catalysts(&upcoming_events, report_date, 14, 6);

        // Bitcoin ETF flows: aggregate `capital_flows` rows for asset='BTC'
        // with etf_creation/etf_redemption flow_types into 1d / 7d / 30d
        // net buckets. Empty when the table has no rows.
        ctx.bitcoin_etf_flows = load_bitcoin_etf_flow_summaries(backend, report_date);

        // Bitcoin on-chain context: latest cached network + exchange-reserve
        // metrics from `onchain_cache`. Empty when neither metric is present.
        ctx.bitcoin_onchain = load_bitcoin_onchain_summaries(backend);

        // Macro indicators — latest economic-data cache rows (BLS/FRED).
        ctx.macro_indicators = load_sqlite_slot(
            &mut ctx.slot_issues,
            "macro_indicators",
            backend,
            crate::db::economic_data::get_all,
        )
            .map(|rows| {
                rows.into_iter()
                    .map(|e| MacroIndicatorSummary {
                        name: pretty_indicator(&e.indicator),
                        value: Some(e.value.normalize().to_string()),
                        trend: e.change.map(|c| {
                            if c.is_sign_negative() {
                                format!("down {}", c.abs().normalize())
                            } else if c.is_zero() {
                                "flat".to_string()
                            } else {
                                format!("up {}", c.normalize())
                            }
                        }),
                        freshness: Some(short_date(&e.fetched_at)),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // News — latest 48h, ranked into public events + per-asset signals.
        const NEWS_SLOTS: &[&str] = &[
            "public_news_events",
            "news_catalysts",
            "bitcoin_news",
            "precious_metals_news",
            "equity_news",
        ];
        let news = match backend.sqlite_native() {
            Some(conn) => {
                match crate::db::news_cache::get_latest_news(conn, 60, None, None, None, Some(48))
                {
                    Ok(rows) => rows,
                    Err(e) => {
                        note_error_many(&mut ctx.slot_issues, NEWS_SLOTS, &e.to_string());
                        Vec::new()
                    }
                }
            }
            None => {
                note_error_many(
                    &mut ctx.slot_issues,
                    NEWS_SLOTS,
                    "backend is not native SQLite; slot loader skipped",
                );
                Vec::new()
            }
        };
        ctx.public_news_events = news
            .iter()
            .take(12)
            .map(|n| PublicNewsEvent {
                headline: n.title.clone(),
                summary: (!n.description.is_empty()).then(|| n.description.clone()),
                domain: n.source_domain.clone(),
                source_tier: Some(n.source_tier as u8),
                independence: Some(independence_label(n.source_independence)),
                topic: (!n.topic.is_empty()).then(|| n.topic.clone()),
                bound_market: n.symbol_tag.clone(),
                impact_score: news_impact_score(n),
            })
            .collect();
        ctx.news_catalysts = news
            .iter()
            .take(8)
            .map(|n| CatalystSummary {
                headline: n.title.clone(),
                market_read: n
                    .symbol_tag
                    .clone()
                    .or_else(|| (!n.topic.is_empty()).then(|| n.topic.clone())),
            })
            .collect();
        ctx.bitcoin_news = news_for(&news, BITCOIN_NEWS_TOPICS)
            .into_iter()
            .map(|n| BitcoinCatalystSummary {
                headline: n.title.clone(),
                source: Some(n.source_domain.clone()),
                relevance: (!n.topic.is_empty()).then(|| n.topic.clone()),
            })
            .collect();
        ctx.precious_metals_news = news_for(&news, METALS_NEWS_TOPICS)
            .into_iter()
            .map(news_signal_for_metals)
            .collect();
        ctx.equity_news = news_for(&news, EQUITY_NEWS_TOPICS)
            .into_iter()
            .map(news_signal_for_equity)
            .collect();

        // Price-driven market tables. Cache holds spot + previous_close; weekly
        // change is computed against price_history at report_date - 7d.
        let prices = match crate::db::price_cache::get_all_cached_prices_backend(backend) {
            Ok(rows) => rows,
            Err(e) => {
                note_error_many(
                    &mut ctx.slot_issues,
                    &[
                        "market_snapshot",
                        "bitcoin_market",
                        "precious_metals_market",
                        "equity_indices",
                        "equity_sectors",
                        "private_portfolio_snapshot",
                        "private_positions",
                    ],
                    &e.to_string(),
                );
                Vec::new()
            }
        };
        let price_map: std::collections::HashMap<String, &crate::models::price::PriceQuote> =
            prices.iter().map(|q| (q.symbol.clone(), q)).collect();
        let week_ago = week_ago_date(report_date);

        ctx.market_snapshot = MARKET_SNAPSHOT_ASSETS
            .iter()
            .filter_map(|(symbol, label)| {
                let q = freshest_quote(&price_map, symbol)?;
                let weekly = weekly_change_pct(backend, &q.symbol, q.price, week_ago.as_deref());
                Some(MarketSnapshotRow {
                    asset: label.to_string(),
                    price: Some(format_price(q.price)),
                    daily_change_pct: daily_change_pct(q),
                    weekly_change_pct: weekly,
                    signal: trend_signal(backend, &q.symbol),
                })
            })
            .collect();

        // Bitcoin spot. Prefer the freshest BTC alias — the cache can hold a
        // stale legacy "BTC-USD" row alongside the current "BTC" spot.
        ctx.bitcoin_market = freshest_quote(&price_map, "BTC").map(|q| BitcoinMarketSummary {
            price: Some(format_price(q.price)),
            daily_change_pct: daily_change_pct(q),
            weekly_change_pct: weekly_change_pct(backend, &q.symbol, q.price, week_ago.as_deref()),
            trend: trend_signal(backend, &q.symbol),
            freshness: Some(short_date(&q.fetched_at)),
        });

        // Precious metals.
        ctx.precious_metals_market = METALS_MARKET_ASSETS
            .iter()
            .filter_map(|(symbol, name)| {
                let q = price_map.get(*symbol)?;
                Some(PreciousMetalMarketRow {
                    asset: name.to_string(),
                    symbol: symbol.to_string(),
                    price: Some(format_price(q.price)),
                    daily_change_pct: daily_change_pct(q),
                    weekly_change_pct: weekly_change_pct(
                        backend,
                        symbol,
                        q.price,
                        week_ago.as_deref(),
                    ),
                    trend: trend_signal(backend, symbol),
                    freshness: Some(short_date(&q.fetched_at)),
                })
            })
            .collect();

        // Real-yield context for the Gold section — latest TIPS real yield
        // (DFII series), with direction inferred from its recent history.
        ctx.real_yield_context = load_real_yield_context(backend);

        // Equity indices + sector ETFs.
        ctx.equity_indices =
            equity_rows(backend, &price_map, EQUITY_INDEX_ASSETS, week_ago.as_deref());
        ctx.equity_sectors =
            equity_rows(backend, &price_map, EQUITY_SECTOR_ASSETS, week_ago.as_deref());

        // ---- Private slots -------------------------------------------------
        // Portfolio snapshot + per-position rows from transactions × prices.
        let transactions = match crate::db::transactions::list_transactions_backend(backend) {
            Ok(rows) => rows,
            Err(e) => {
                note_error_many(
                    &mut ctx.slot_issues,
                    &["private_portfolio_snapshot", "private_positions"],
                    &e.to_string(),
                );
                Vec::new()
            }
        };
        if !transactions.is_empty() {
            let spot: std::collections::HashMap<String, rust_decimal::Decimal> =
                prices.iter().map(|q| (q.symbol.clone(), q.price)).collect();
            let positions = crate::models::position::compute_positions(
                &transactions,
                &spot,
                &std::collections::HashMap::new(),
            );
            if !positions.is_empty() {
                let total_value: rust_decimal::Decimal =
                    positions.iter().filter_map(|p| p.current_value).sum();
                let mut categories: std::collections::BTreeMap<String, rust_decimal::Decimal> =
                    std::collections::BTreeMap::new();
                for p in &positions {
                    if let Some(v) = p.current_value {
                        *categories.entry(p.category.to_string()).or_default() += v;
                    }
                }
                let alloc_summary = if total_value.is_zero() {
                    None
                } else {
                    let parts: Vec<String> = categories
                        .iter()
                        .map(|(cat, v)| {
                            let pct = (*v / total_value) * rust_decimal::Decimal::from(100);
                            format!("{cat} {pct:.0}%")
                        })
                        .collect();
                    Some(parts.join(", "))
                };
                // Daily P&L = sum across positions of
                //   quantity × (current_price - previous_close)
                // when both prices are available. previous_close comes off
                // the price_cache quote (Yahoo's regular-market prior close).
                // We compute against total_value to derive a percent.
                let mut pnl_total: rust_decimal::Decimal = rust_decimal::Decimal::ZERO;
                let mut any_pnl_component = false;
                for p in &positions {
                    let Some(curr) = p.current_price else { continue };
                    let qty = p.quantity;
                    let Some(quote) = price_map.get(&p.symbol) else {
                        continue;
                    };
                    let Some(prev) = quote.previous_close else {
                        continue;
                    };
                    if prev.is_zero() {
                        continue;
                    }
                    pnl_total += (curr - prev) * qty;
                    any_pnl_component = true;
                }
                let (daily_pnl, daily_pnl_pct) = if any_pnl_component {
                    let pct = if !total_value.is_zero() {
                        Some(dec_to_f64(
                            ((pnl_total / total_value)
                                * rust_decimal::Decimal::from(100))
                            .round_dp(2),
                        ))
                    } else {
                        None
                    };
                    (Some(format_signed_price(pnl_total)), pct)
                } else {
                    (None, None)
                };

                ctx.private_portfolio_snapshot = Some(PrivatePortfolioSnapshotSummary {
                    total_value: Some(format_price(total_value)),
                    daily_pnl,
                    daily_pnl_pct,
                    allocation_summary: alloc_summary,
                });
                ctx.private_positions = positions
                    .iter()
                    .map(|p| PrivatePositionSnapshotRow {
                        symbol: p.symbol.clone(),
                        price: p.current_price.map(format_price),
                        daily_change: price_map
                            .get(&p.symbol)
                            .and_then(|q| daily_change_pct(q))
                            .map(|d| format!("{d:+.1}%")),
                        allocation_pct: p.allocation_pct.map(dec_to_f64).unwrap_or(0.0),
                        unrealized_pnl: p.gain.map(format_price),
                    })
                    .collect();
            }
        }

        // Open (pending) predictions resolving — `journal prediction list`.
        ctx.private_open_predictions = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_open_predictions",
            backend,
            |conn| {
                crate::db::user_predictions::list_predictions(
                    conn,
                    Some("pending"),
                    None,
                    None,
                    None,
                )
            },
        )
            .map(|preds| {
                preds
                    .into_iter()
                    .map(|p| {
                        let target_date = p.target_date.clone().unwrap_or_default();
                        let days_remaining = days_between(report_date, &target_date);
                        PrivateOpenPredictionRow {
                            id: Some(p.id),
                            symbol: p.symbol.clone().unwrap_or_else(|| "—".to_string()),
                            claim: p.claim.clone(),
                            target_date,
                            days_remaining,
                            confidence: p.confidence,
                            conviction: None,
                            direction: None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Lessons applied — count lesson references over the trailing window.
        ctx.private_lessons_applied = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_lessons_applied",
            backend,
            load_lessons_applied,
        )
        .flatten();

        // Parallels — read the catalog runner's per-date JSON from /tmp.
        // Missing file = the Step 4.5 parallels runner did not run for this
        // date (upstream_not_run); malformed JSON = loader_error.
        let (parallels, parallels_issue) = load_parallels_results_classified(report_date);
        ctx.parallels_results = parallels;
        if let Some(issue) = parallels_issue {
            ctx.slot_issues.insert("parallels_results", issue);
        }

        // Cross-layer signals — agent_messages addressed to synthesis on the
        // report date with high/normal priority. Decision-card and panel-*
        // messages are filtered out inside the loader so they appear in their
        // own sections instead of dumping JSON into this table.
        ctx.cross_layer_signals = match load_cross_layer_signals(backend, report_date) {
            Ok(rows) => rows,
            Err(e) => {
                note_error(&mut ctx.slot_issues, "cross_layer_signals", e);
                Vec::new()
            }
        };
        if ctx.cross_layer_signals.is_empty()
            && !ctx.slot_issues.contains_key("cross_layer_signals")
        {
            if let Some(latest) = latest_agent_message_date_before(
                backend,
                report_date,
                |from| !from.starts_with("panel-") && from != "analyst-decisions",
            ) {
                ctx.slot_issues.insert(
                    "cross_layer_signals",
                    SlotIssue::UpstreamNotRun(format!(
                        "no synthesis-bound agent messages for {report_date}; latest are from {latest} — analyst layers did not message synthesis today"
                    )),
                );
            }
        }

        // Investor panel — parsed persona responses + per-asset consensus
        // tally. Both empty when the Phase 2b panel spawn produced nothing.
        ctx.investor_panel = match load_investor_panel_responses(backend, report_date) {
            Ok(rows) => rows,
            Err(e) => {
                note_error_many(
                    &mut ctx.slot_issues,
                    &["investor_panel", "investor_panel_consensus"],
                    &e.to_string(),
                );
                Vec::new()
            }
        };
        if ctx.investor_panel.is_empty() && !ctx.slot_issues.contains_key("investor_panel") {
            if let Some(latest) = latest_agent_message_date_before(backend, report_date, |from| {
                from.starts_with("panel-")
            }) {
                let issue = SlotIssue::UpstreamNotRun(format!(
                    "no panel-* messages for {report_date}; latest are from {latest} — Phase 2b investor panel did not run today"
                ));
                ctx.slot_issues.insert("investor_panel", issue.clone());
                ctx.slot_issues.insert("investor_panel_consensus", issue);
            }
        }
        ctx.investor_panel_consensus = aggregate_panel_consensus(&ctx.investor_panel);

        // Portfolio decision cards — JSON envelopes written by the
        // decision-architect (Phase 4) and parsed into a typed struct so the
        // Decisions Pending section can render them alongside calendar
        // catalyst cards.
        ctx.portfolio_decision_cards = match load_portfolio_decision_cards(backend, report_date) {
            Ok(rows) => rows,
            Err(e) => {
                note_error(&mut ctx.slot_issues, "portfolio_decision_cards", e);
                Vec::new()
            }
        };
        if ctx.portfolio_decision_cards.is_empty()
            && !ctx.slot_issues.contains_key("portfolio_decision_cards")
        {
            if let Some(latest) = latest_agent_message_date_before(backend, report_date, |from| {
                from == "analyst-decisions"
            }) {
                ctx.slot_issues.insert(
                    "portfolio_decision_cards",
                    SlotIssue::UpstreamNotRun(format!(
                        "no decision cards for {report_date}; latest are from {latest} — Phase 4 decision architect did not run today"
                    )),
                );
            }
        }

        // Risk-factor mappings per held asset. Empty when the macro / high
        // analyst routines have not populated the `risk_factor_mappings`
        // table via `pftui analytics risk-factors add`.
        ctx.private_risk_factor_mappings = load_slot(
            &mut ctx.slot_issues,
            "private_risk_factor_mappings",
            crate::db::risk_factor_mappings::list_backend(backend, None),
        )
        .map(|rows| {
            rows.into_iter()
                .map(|r| PrivateRiskFactorMapping {
                    symbol: r.symbol,
                    factor: r.factor,
                    direction: r.direction,
                    exposure_multiplier: r.exposure_multiplier,
                })
                .collect()
        })
        .unwrap_or_default();

        // Per-asset synthesised intelligence blobs — one per held position.
        if !ctx.private_positions.is_empty() {
            let symbols: Vec<String> = ctx
                .private_positions
                .iter()
                .map(|p| p.symbol.clone())
                .collect();
            for sym in symbols {
                if let Some(blob) = load_asset_intelligence_blob(backend, &sym) {
                    ctx.private_asset_intelligence.insert(sym, blob);
                }
            }
            if ctx.private_asset_intelligence.is_empty() {
                ctx.slot_issues.insert(
                    "private_asset_intelligence",
                    SlotIssue::NoData(
                        "no per-asset intelligence could be assembled for any held position"
                            .to_string(),
                    ),
                );
            }
        }

        // Morning-brief lead — pulled from the latest narrative snapshot if
        // we have one (same substrate the `morning-brief` command surfaces
        // for the headline/tension fields). We deliberately reuse the
        // narrative we already fetched above for the regime synthesis.
        ctx.morning_brief = narrative.as_ref().and_then(load_morning_brief_summary);

        // Today's analyst-written synthesis — the headline note per
        // timeframe analyst, the largest |%| move mentioned in those notes
        // against a held asset, and the highest-priority `to='synthesis'`
        // agent message of the day.
        let held_for_synthesis: Vec<String> = ctx
            .private_positions
            .iter()
            .map(|p| p.symbol.clone())
            .collect();
        ctx.todays_analyst_synthesis = load_sqlite_slot(
            &mut ctx.slot_issues,
            "todays_analyst_synthesis",
            backend,
            |conn| load_todays_analyst_synthesis(conn, report_date, &held_for_synthesis),
        )
        .flatten();

        // Synthesis digest — per-asset bull/bear/change-mind/risk-reward plus
        // the economy paragraph, parsed from `analyst-synthesis` daily_notes
        // for the report date. When no notes exist for the report date but
        // earlier synthesis notes do, the synthesis-writer pass didn't run
        // today — classify as upstream_not_run, not no_data.
        ctx.synthesis_notes = load_sqlite_slot(
            &mut ctx.slot_issues,
            "synthesis_notes",
            backend,
            |conn| load_synthesis_notes(conn, report_date),
        )
        .unwrap_or_default();
        if !ctx.synthesis_notes.has_content() && !ctx.slot_issues.contains_key("synthesis_notes")
        {
            if let Some(latest) = backend.sqlite_native().and_then(|conn| {
                latest_synthesis_note_date_before(conn, report_date)
            }) {
                ctx.slot_issues.insert(
                    "synthesis_notes",
                    SlotIssue::UpstreamNotRun(format!(
                        "no synthesis notes for {report_date}; latest are from {latest} — synthesis-writer pass did not run today"
                    )),
                );
                ctx.staleness.push(StalenessWarning {
                    input: "synthesis_notes",
                    message: format!(
                        "⚠ no same-day synthesis notes for {report_date} — newest are from {latest}; prose sections reflect an earlier run"
                    ),
                    sections: vec!["private_overview", "private_synthesis"],
                });
            }
        }

        // ---- Per-asset convergence + drift + derived actions -------------
        // These three slots together drive the per-asset cards. A bug where
        // any one of them was left empty caused every card to render with an
        // "INSUFFICIENT VIEWS" badge even when the analyst layers had
        // written 6+ views per asset. The loaders below are intentionally
        // best-effort: a missing source must degrade to empty, never abort.

        let since_ts = crate::db::analyst_views::parse_since("7d").ok();
        let targets_by_symbol: std::collections::HashMap<String, f64> = backend
            .sqlite_native()
            .and_then(|conn| crate::db::allocation_targets::list_targets(conn).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.symbol.to_uppercase(), dec_to_f64(t.target_pct)))
            .collect();
        ctx.private_asset_convergence = load_slot(
            &mut ctx.slot_issues,
            "private_asset_convergence",
            crate::db::analyst_views::convergence_all_backend(backend, since_ts.as_deref()),
        )
                .map(|reports| {
                    reports
                        .into_iter()
                        .filter(|r| !r.views.is_empty())
                        .map(|r| PrivateAssetConvergenceRow {
                            target_pct: targets_by_symbol.get(&r.asset.to_uppercase()).copied(),
                            symbol: r.asset,
                            views: r
                                .views
                                .into_iter()
                                .map(|v| PrivateAssetConvergenceView {
                                    analyst: v.analyst,
                                    conviction: v.conviction,
                                    reasoning_summary: v.reasoning_summary,
                                    probation: v.probation,
                                    probation_streak: v.probation_streak,
                                })
                                .collect(),
                        })
                        .collect()
                })
                .unwrap_or_default();

        // -----------------------------------------------------------------
        // W4 loaders: news, calibration, lessons, conviction trajectories,
        // and per-horizon outlooks. Every loader degrades to empty/None.
        // -----------------------------------------------------------------

        // Held-asset universe drives the private-news + trajectories +
        // outlooks loaders. Use the freshly computed positions from above if
        // available; otherwise fall back to the unique symbols on the
        // transactions list. Symbols are uppercased for case-insensitive
        // matching against news symbol_tags and analyst_views.asset.
        let held_symbols: Vec<String> = {
            let mut from_txs: std::collections::BTreeSet<String> = transactions
                .iter()
                .map(|t| t.symbol.to_uppercase())
                .collect();
            // Drop synthetic cash placeholders (e.g. "$CASH") — they have no
            // analyst views, news, or convictions worth surfacing.
            from_txs.retain(|s| !s.starts_with('$'));
            from_txs.into_iter().collect()
        };

        // 1. private_news_events — last 24h news mentioning a held asset
        //    via symbol_tag. Reuses the same news_cache loader as the public
        //    news pipeline, narrowed to a 24h window.
        let news_24h = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_news_events",
            backend,
            |conn| crate::db::news_cache::get_latest_news(conn, 200, None, None, None, Some(24)),
        )
        .unwrap_or_default();
        ctx.private_news_events =
            private_news_events_for_held(&news_24h, &held_symbols);

        // 2. private_news_silence — run the silence analyzer the same way
        //    the CLI does; map its entries onto NewsVolumeSignal.
        ctx.private_news_silence = load_slot(
            &mut ctx.slot_issues,
            "private_news_silence",
            crate::commands::news_silence::build_report_backend(backend, 28),
        )
            .map(|rep| {
                rep.entries
                    .into_iter()
                    .map(|e| NewsVolumeSignal {
                        topic: e.topic,
                        current_count: e.observed_count.max(0) as u32,
                        baseline_count: Some(e.median_count),
                        status: e.status,
                        caveat: (!e.label.is_empty()).then_some(e.label),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // 3. private_macro_divergences — narrative-vs-money divergence per
        //    scenario. Material when |divergence z-score| > 1.0 (the spec's
        //    "one sigma" gate).
        //    The news window MUST match the `pftui analytics narrative-divergence`
        //    CLI default (24h). The z-scores are population-relative across
        //    scenarios, so a wider window here shifts every score and makes the
        //    report's callout disagree with what an operator sees from the CLI.
        ctx.private_macro_divergences = load_slot(
            &mut ctx.slot_issues,
            "private_macro_divergences",
            crate::commands::narrative_divergence::build_report_backend(backend, 24, 1.0),
        )
                .map(|rep| {
                    rep.entries
                        .into_iter()
                        .map(|e| PrivateNarrativeMoneyDivergence {
                            scenario: e.scenario_name,
                            summary: e.label,
                            material: e.divergence_score.abs() > 1.0,
                        })
                        .collect()
                })
                .unwrap_or_default();

        let target_records: std::collections::HashMap<
            String,
            crate::db::allocation_targets::AllocationTarget,
        > = backend
            .sqlite_native()
            .and_then(|conn| crate::db::allocation_targets::list_targets(conn).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.symbol.to_uppercase(), t))
            .collect();
        ctx.private_drift_rows = ctx
            .private_positions
            .iter()
            .filter_map(|p| {
                let target = target_records.get(&p.symbol.to_uppercase())?;
                Some(PrivateDriftRow {
                    symbol: p.symbol.clone(),
                    target_pct: dec_to_f64(target.target_pct),
                    actual_pct: p.allocation_pct,
                    band_pct: dec_to_f64(target.drift_band_pct),
                })
            })
            .collect();

        ctx.private_derived_actions =
            derive_actions(&ctx.private_asset_convergence, &ctx.private_drift_rows);
        // What-changed deltas (private "What Changed in 7d" strip) — reuse the
        // `pftui analytics deltas --json` backend so the report and the CLI
        // surface the same change-radar items. We pass `persist_current=false`
        // so report generation never mutates the situation-snapshot history
        // table (only `data refresh` writes there).
        ctx.private_what_changed_deltas = load_slot(
            &mut ctx.slot_issues,
            "private_what_changed_deltas",
            crate::analytics::deltas::build_report_backend(
                backend,
                crate::analytics::deltas::DeltaWindow::Days7,
                false,
            ),
        )
        .map(|report| {
            report
                .change_radar
                .into_iter()
                .map(map_change_radar_to_delta)
                .collect()
        })
        .unwrap_or_default();

        // Private macro scenarios — current probability + 7d-prior probability
        // from `scenario_history`, sorted by current probability descending.
        // The public `ctx.public_scenarios` loader above already populates the
        // public-mode row shape from `scenarios`; here we layer on the
        // 7d-prior column from the same `scenario_history` source the CLI
        // timeline backend reads, so both report modes line up against the
        // same underlying history.
        ctx.private_macro_scenarios = load_slot(
            &mut ctx.slot_issues,
            "private_macro_scenarios",
            crate::db::scenarios::get_all_timelines_backend(backend, Some(7)),
        )
                .map(|timelines| {
                    let mut rows: Vec<PrivateMacroScenarioRow> = timelines
                        .into_iter()
                        .map(|t| {
                            // change = current - first; prior_7d = current - change.
                            // When no history exists, fall back to the current value
                            // so the row still renders deterministically.
                            let prior_7d = t
                                .change
                                .map(|d| t.current_probability - d)
                                .unwrap_or(t.current_probability);
                            PrivateMacroScenarioRow {
                                name: t.name,
                                probability: t.current_probability,
                                prior_7d,
                            }
                        })
                        .collect();
                    rows.sort_by(|a, b| {
                        b.probability
                            .partial_cmp(&a.probability)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    rows
                })
                .unwrap_or_default();

        // Private macro regime quadrant — derived from `regime_snapshots`. The
        // table doesn't carry explicit `growth` / `inflation` columns (its
        // schema records `regime, vix, dxy, yield_10y, oil, gold, btc`), so we
        // approximate the growth axis from the regime classifier's risk-on /
        // risk-off bucket. The inflation axis isn't computable from the
        // currently-stored fields (CPI YoY lives in `economic_data`, not in
        // the snapshot row), so we leave it at 0.0 and surface a TODO to wire
        // a richer macro-axis snapshot in a follow-up PR.
        // TODO: replace the placeholder inflation axis with a real CPI-derived
        //       value once a macro-axis snapshot table lands. Track at:
        //       <https://github.com/skylarsimoncelli/pftui/issues> (new issue).
        ctx.private_macro_regime = load_slot(
            &mut ctx.slot_issues,
            "private_macro_regime",
            crate::db::regime_snapshots::get_history_backend(backend, Some(7)),
        )
                .filter(|rows| !rows.is_empty())
                .and_then(|rows| {
                    let head = rows.first()?;
                    let (growth, inflation) = regime_to_axes(&head.regime)?;
                    let trail = rows
                        .iter()
                        .skip(1)
                        .filter_map(|snap| {
                            regime_to_axes(&snap.regime).map(|(g, i)| {
                                PrivateRegimeTrailPoint {
                                    growth: g,
                                    inflation: i,
                                }
                            })
                        })
                        .collect();
                    Some(PrivateMacroRegimeQuadrant {
                        growth,
                        inflation,
                        trail,
                    })
                });

        // 5 + 6. Calibration matrix rows. Public surface keeps every row;
        //        private surface filters to held-asset topics.
        ctx.public_calibration = load_sqlite_slot(
            &mut ctx.slot_issues,
            "public_calibration",
            backend,
            load_calibration_rows,
        )
        .unwrap_or_default();
        ctx.private_calibration = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_calibration",
            backend,
            |conn| load_calibration_rows_for_held(conn, &held_symbols),
        )
        .unwrap_or_default();

        // 4. private_open_predictions_calibration — pick the most populous
        //    (layer, topic, conviction_band) tuple from calibration_matrix
        //    that matches the dominant layer of the pending predictions, so
        //    the report can show "you've been X% calibrated at this layer".
        let open_preds_for_calibration = ctx.private_open_predictions.clone();
        ctx.private_open_predictions_calibration = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_open_predictions_calibration",
            backend,
            |conn| load_open_predictions_calibration(conn, &open_preds_for_calibration),
        )
        .flatten();

        // 7. public_lessons_applied — reuse the lessons-applied report
        //    over the trailing 24h window, mapped to the public summary.
        ctx.public_lessons_applied = load_sqlite_slot(
            &mut ctx.slot_issues,
            "public_lessons_applied",
            backend,
            load_public_lessons_applied,
        )
        .unwrap_or_default();

        // 8. private_conviction_trajectories — last 30 days of conviction
        //    points per (held asset, analyst layer). Uses analyst_view_history.
        ctx.private_conviction_trajectories = load_sqlite_slot(
            &mut ctx.slot_issues,
            "private_conviction_trajectories",
            backend,
            |conn| load_conviction_trajectories(conn, &held_symbols, 30),
        )
        .unwrap_or_default();

        // 9. private_outlooks — collapse the four analyst-views layers onto
        //    days/weeks/months horizons per held asset.
        ctx.private_outlooks = outlooks_for_held(&all_views, &held_symbols);

        // 10. epistemic_health — the run_health row for the report date,
        //     surfaced as the final (meta) private section. Best-effort:
        //     missing row or non-SQLite backend degrades to None.
        ctx.epistemic_health = load_sqlite_slot(
            &mut ctx.slot_issues,
            "epistemic_health",
            backend,
            |conn| crate::db::run_health::get_run_health(conn, report_date),
        )
        .flatten();

        // 11. recommendation_scoreboard — the ledger's per-held-symbol
        //     forward-return summary (action mix, 90d hit rate, ADD−WAIT
        //     window quality). Best-effort; empty while the ledger accrues.
        ctx.recommendation_scoreboard = load_sqlite_slot(
            &mut ctx.slot_issues,
            "recommendation_scoreboard",
            backend,
            |conn| crate::db::recommendations::scoreboard(conn, None),
        )
        .map(|board| scoreboard_lines_for_held(&board, &held_symbols))
        .unwrap_or_default();

        // ---- Build-time staleness pass ------------------------------------
        // For inputs with freshness expectations (prices, sentiment,
        // economic data via series-registry SLAs; analyst views via the
        // 6h skill gate), record warnings so the assembler can annotate the
        // affected sections inline instead of silently rendering old data
        // as current. Annotate, never suppress.
        let mut staleness = compute_staleness(backend, report_date, &all_views, &prices);
        ctx.staleness.append(&mut staleness);

        Ok(ctx)
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

/// Derive ADD/TRIM/HOLD action summaries from per-asset convergence and
/// drift rows. Pure function — no DB access, no I/O — so the rules can be
/// unit-tested with synthetic fixtures.
///
/// Rules (in priority order):
///   * insufficient-views → no action emitted
///   * convergent-bull / strong-convergent-bull AND actual < target − band
///     → ADD (urgency=high if strong, else normal)
///   * convergent-bear / strong-convergent-bear AND actual > target + band
///     → TRIM (urgency=high if strong, else normal)
///   * convergent-neutral within band → HOLD (urgency=low)
///   * everything else → no action
pub fn derive_actions(
    convergence: &[PrivateAssetConvergenceRow],
    drift_rows: &[PrivateDriftRow],
) -> Vec<DerivedActionSummary> {
    let mut out: Vec<DerivedActionSummary> = Vec::new();
    for row in convergence {
        // Probation views (active forecast misalignment on the layer/asset)
        // never vote — same exclusion as the convergence stats themselves.
        let voting: Vec<&PrivateAssetConvergenceView> =
            row.views.iter().filter(|v| !v.probation).collect();
        if voting.is_empty() {
            continue;
        }
        let n_views = voting.len();
        let convictions: Vec<i64> = voting.iter().map(|v| v.conviction).collect();
        let avg = convictions.iter().copied().map(|c| c as f64).sum::<f64>() / n_views as f64;
        let (min_c, max_c) = match (convictions.iter().min(), convictions.iter().max()) {
            (Some(min), Some(max)) => (*min, *max),
            _ => continue,
        };
        let max_divergence = max_c - min_c;
        let classification =
            crate::db::analyst_views::classify_convergence(n_views, avg, max_divergence);
        if classification == "insufficient-views" {
            continue;
        }
        let drift = drift_rows
            .iter()
            .find(|d| d.symbol.eq_ignore_ascii_case(&row.symbol));
        match classification {
            "convergent-bull" | "strong-convergent-bull" => {
                if let Some(d) = drift {
                    if d.actual_pct < d.target_pct - d.band_pct {
                        let urgency = if classification == "strong-convergent-bull" {
                            "high"
                        } else {
                            "normal"
                        };
                        out.push(DerivedActionSummary {
                            asset: row.symbol.clone(),
                            action: "ADD".to_string(),
                            urgency: urgency.to_string(),
                            rationale: format!(
                                "{classification}; allocation {:.1}% vs target {:.1}% (band ±{:.1}%)",
                                d.actual_pct, d.target_pct, d.band_pct
                            ),
                        });
                    }
                }
            }
            "convergent-bear" | "strong-convergent-bear" => {
                if let Some(d) = drift {
                    if d.actual_pct > d.target_pct + d.band_pct {
                        let urgency = if classification == "strong-convergent-bear" {
                            "high"
                        } else {
                            "normal"
                        };
                        out.push(DerivedActionSummary {
                            asset: row.symbol.clone(),
                            action: "TRIM".to_string(),
                            urgency: urgency.to_string(),
                            rationale: format!(
                                "{classification}; allocation {:.1}% vs target {:.1}% (band ±{:.1}%)",
                                d.actual_pct, d.target_pct, d.band_pct
                            ),
                        });
                    }
                }
            }
            "convergent-neutral" => {
                if let Some(d) = drift {
                    let lo = d.target_pct - d.band_pct;
                    let hi = d.target_pct + d.band_pct;
                    if d.actual_pct >= lo && d.actual_pct <= hi {
                        out.push(DerivedActionSummary {
                            asset: row.symbol.clone(),
                            action: "HOLD".to_string(),
                            urgency: "low".to_string(),
                            rationale: format!(
                                "convergent-neutral; allocation {:.1}% within band {:.1}–{:.1}%",
                                d.actual_pct, lo, hi
                            ),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Load the latest-per-asset synthesis-time adversary view from
/// `adversary_synthesis_views`. JSON fields are decoded into `Vec<String>`;
/// rows whose JSON arrays are malformed are skipped (the assembler
/// degrades silently rather than failing the whole report build).
fn load_latest_synthesis_adversary_views(
    conn: &rusqlite::Connection,
) -> Result<Vec<AdversarySynthesisSummary>> {
    crate::db::adversary_synthesis_views::ensure_table(conn)?;
    // Pull every row ordered by recorded_at DESC; for each asset keep only
    // the first (newest) we see. SQLite doesn't have window functions on
    // every backend variant we ship; this Rust-side fold avoids that.
    let rows = crate::db::adversary_synthesis_views::list(conn, None, None)?;
    let mut out: Vec<AdversarySynthesisSummary> = Vec::new();
    for r in rows {
        if out.iter().any(|s| s.asset == r.asset) {
            continue;
        }
        let evidence = serde_json::from_str::<Vec<String>>(&r.counter_case_evidence_points)
            .unwrap_or_default();
        let triggers = serde_json::from_str::<Vec<String>>(&r.falsification_triggers)
            .unwrap_or_default();
        out.push(AdversarySynthesisSummary {
            asset: r.asset,
            current_convergence_summary: r.current_convergence_summary,
            counter_case_summary: r.counter_case_summary,
            counter_case_evidence_points: evidence,
            falsification_triggers: triggers,
            fragility_score: r.fragility_score,
            recorded_at: r.recorded_at,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Per-source loader helpers
// ---------------------------------------------------------------------------

/// Curated cross-asset universe for the public Market Snapshot table.
/// (cache symbol, display label) — only rows present in the price cache render.
const MARKET_SNAPSHOT_ASSETS: &[(&str, &str)] = &[
    ("BTC-USD", "BTC"),
    ("GC=F", "Gold"),
    ("SI=F", "Silver"),
    ("CL=F", "WTI Crude"),
    ("DX-Y.NYB", "DXY"),
    ("^GSPC", "S&P 500"),
    ("^NDX", "Nasdaq 100"),
    ("^VIX", "VIX"),
    ("^TNX", "10Y Yield"),
];

/// Precious-metals market rows. (cache symbol, display name).
const METALS_MARKET_ASSETS: &[(&str, &str)] =
    &[("GC=F", "Gold"), ("SI=F", "Silver"), ("HG=F", "Copper")];

/// Broad equity index rows. (cache symbol, display name).
const EQUITY_INDEX_ASSETS: &[(&str, &str)] = &[
    ("^GSPC", "S&P 500"),
    ("^NDX", "Nasdaq 100"),
    ("^DJI", "Dow Jones"),
    ("^RUT", "Russell 2000"),
    ("SPY", "SPY"),
    ("QQQ", "QQQ"),
];

/// Sector ETF rows. (cache symbol, display name).
const EQUITY_SECTOR_ASSETS: &[(&str, &str)] = &[
    ("XLK", "Technology"),
    ("XLF", "Financials"),
    ("XLE", "Energy"),
    ("XLV", "Health Care"),
    ("XLI", "Industrials"),
    ("XLY", "Consumer Disc."),
    ("XLP", "Consumer Staples"),
    ("XLU", "Utilities"),
];

// Asset symbols whose analyst views feed each public section's multi-timeframe
// table. Matching is case-insensitive against the `analyst_views.asset` column.
const MACRO_ASSETS: &[&str] = &["DXY", "DX-Y.NYB", "USD", "^VIX", "SPY"];
const BITCOIN_ASSETS: &[&str] = &["BTC", "BTC-USD", "MSTR", "COIN"];
// Public silver/gold proxies only. Deliberately excludes niche vehicles a
// specific operator might personally trade (e.g. PSLV) — the public newsletter
// uses the standard SLV / SI=F / GLD proxies so the per-asset view table never
// mirrors an individual's personal asset universe (privacy gate, 2026-06-03).
const METALS_ASSETS: &[&str] = &["GC=F", "GLD", "GLC", "SI=F", "SLV", "HG=F"];
const EQUITY_ASSETS: &[&str] = &["SPY", "GOOG", "HOOD", "RKLB", "CCJ"];

// News topics routed to each per-asset "What to Watch" block.
const BITCOIN_NEWS_TOPICS: &[&str] = &["crypto"];
const METALS_NEWS_TOPICS: &[&str] = &["inflation", "geopolitics"];
const EQUITY_NEWS_TOPICS: &[&str] = &["equities", "ai"];

/// Map a `change_radar` `SituationInsight` (from the `analytics::deltas`
/// backend) onto the report-side `WhatChangedDeltaSummary` row shape.
///
/// Direction policy (matches the private "What Changed" strip contract):
///
/// * `"info"` — regime changes and correlation breaks (no signed direction).
/// * `"bull"` — any other insight whose `value` field begins with a `+`.
/// * `"bear"` — any other insight whose `value` field begins with a `-`.
///
/// Unsigned values (e.g. a categorical "RISK-ON" lead-signal change) fall back
/// to `"info"` so the strip can still render the row neutrally.
fn map_change_radar_to_delta(
    insight: crate::analytics::situation::SituationInsight,
) -> WhatChangedDeltaSummary {
    let title_lc = insight.title.to_ascii_lowercase();
    let is_info = title_lc.starts_with("regime ")
        || title_lc.starts_with("correlation ")
        || title_lc.contains("regime shifted")
        || title_lc.contains("correlation shifted");
    let direction = if is_info {
        "info"
    } else if insight.value.starts_with('+') {
        "bull"
    } else if insight.value.starts_with('-') {
        "bear"
    } else {
        "info"
    };
    WhatChangedDeltaSummary {
        label: insight.title,
        delta: insight.value,
        direction: direction.to_string(),
    }
}

/// Approximate (growth, inflation) axes from the regime classifier's label.
///
/// `regime_snapshots` doesn't carry explicit growth/inflation columns — the
/// table records `regime, vix, dxy, yield_10y, oil, gold, btc`. We project
/// the risk-on / risk-off bucket onto the growth axis (risk-on ⇒ growth+,
/// risk-off ⇒ growth−) and hold the inflation axis at 0.0 until a real
/// CPI-derived value is wired through. Returns `None` for labels the loader
/// doesn't recognise so the renderer can skip the block cleanly.
fn regime_to_axes(regime: &str) -> Option<(f64, f64)> {
    let key = regime.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let growth = match key.as_str() {
        "risk_on" | "riskon" => 1.0,
        "lean_risk_on" | "leanrisk_on" => 0.5,
        "neutral" | "transitioning" | "transition" => 0.0,
        "lean_risk_off" | "leanrisk_off" => -0.5,
        "risk_off" | "riskoff" => -1.0,
        _ => return None,
    };
    Some((growth, 0.0))
}

/// Convert a `Decimal` to `f64` via its string form (no precision-losing
/// arithmetic). Used only for display-layer percentages, never money math.
fn dec_to_f64(d: rust_decimal::Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

/// Format a money/price decimal for display with a leading `$` and grouped
/// thousands separators (e.g. `$61,667`). Delegates to the shared formatter.
fn format_price(d: rust_decimal::Decimal) -> String {
    crate::report::format::fmt_money(d)
}

/// Format a signed money delta — leading `+` for positive (or zero),
/// leading `-` for negative, with grouped thousands. Used for daily P&L
/// display in the Bottom Line bullet.
fn format_signed_price(d: rust_decimal::Decimal) -> String {
    crate::report::format::fmt_signed_money(d)
}

/// Truncate an RFC3339 / SQLite timestamp to its `YYYY-MM-DD` date part.
fn short_date(raw: &str) -> String {
    raw.split(['T', ' ']).next().unwrap_or(raw).to_string()
}

/// Human-readable label for a snake_case economic indicator key.
fn pretty_indicator(key: &str) -> String {
    match key {
        "fed_funds_rate" => "Fed Funds Rate".to_string(),
        "cpi" => "CPI YoY".to_string(),
        "ppi" => "PPI".to_string(),
        "unemployment_rate" => "Unemployment Rate".to_string(),
        "nfp" => "Nonfarm Payrolls".to_string(),
        "pmi_manufacturing" => "PMI (Manufacturing)".to_string(),
        "pmi_services" => "PMI (Services)".to_string(),
        "initial_jobless_claims" => "Initial Jobless Claims".to_string(),
        other => other
            .split('_')
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().chain(c).collect::<String>(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Resolve the freshest cached quote for `symbol`, transparently preferring a
/// fresher alias when the cache holds duplicate rows under legacy symbols. The
/// price cache can carry both a current row (e.g. "BTC") and a weeks-stale
/// legacy alias (e.g. "BTC-USD"); a naive first-match lookup would surface the
/// stale spot. For symbols without a known alias pair this is a direct lookup.
fn freshest_quote<'a>(
    price_map: &std::collections::HashMap<String, &'a crate::models::price::PriceQuote>,
    symbol: &str,
) -> Option<&'a crate::models::price::PriceQuote> {
    let aliases: &[&str] = match symbol {
        "BTC" | "BTC-USD" => &["BTC", "BTC-USD"],
        s => return price_map.get(s).copied(),
    };
    aliases
        .iter()
        .filter_map(|a| price_map.get(*a).copied())
        .max_by(|a, b| a.fetched_at.cmp(&b.fetched_at))
}

/// First sentence (up to the first sentence-ending period or newline) of a
/// free-text field, trimmed. A period is treated as a sentence boundary ONLY
/// when it is not a decimal point — i.e. NOT flanked by digits on both sides —
/// so figures like "COFER 56.1%" or "support 99-99.5" survive intact instead
/// of being chopped at the decimal.
fn first_sentence(text: &str) -> String {
    let trimmed = text.trim();
    let chars: Vec<(usize, char)> = trimmed.char_indices().collect();
    let mut boundary = None;
    for (k, (i, c)) in chars.iter().enumerate() {
        if *c == '\n' {
            boundary = Some(*i);
            break;
        }
        if *c == '.' {
            let prev_digit = k
                .checked_sub(1)
                .map(|j| chars[j].1.is_ascii_digit())
                .unwrap_or(false);
            let next_digit = chars
                .get(k + 1)
                .map(|(_, nc)| nc.is_ascii_digit())
                .unwrap_or(false);
            if prev_digit && next_digit {
                continue; // decimal point, not a sentence boundary
            }
            boundary = Some(*i);
            break;
        }
    }
    match boundary {
        Some(idx) => trimmed[..idx].trim().to_string(),
        None => trimmed.to_string(),
    }
}

/// Lowercase one-word label for a news independence classification.
fn independence_label(value: crate::db::news_cache::NewsSourceIndependence) -> String {
    value.as_str().to_string()
}

/// Deterministic news impact score in [0,1]: higher-tier (lower number) and
/// independent sources score higher. Used to rank public events.
fn news_impact_score(n: &crate::db::news_cache::NewsEntry) -> f64 {
    use crate::db::news_cache::NewsSourceIndependence::*;
    let tier_score = match n.source_tier {
        1 => 1.0,
        2 => 0.75,
        3 => 0.5,
        _ => 0.3,
    };
    let indep_score = match n.source_independence {
        Independent => 1.0,
        Wire => 0.8,
        Restatement => 0.5,
        Rumor => 0.3,
        Unknown => 0.4,
    };
    (tier_score + indep_score) / 2.0
}

/// Select the analyst-view rows whose asset is in `assets` (case-insensitive),
/// mapped to the section's compact `AnalystViewSummary`. Keeps one row per
/// (layer, asset) — the list is already newest-first from the backend.
fn analyst_views_for(
    views: &[crate::db::analyst_views::AnalystView],
    assets: &[&str],
) -> Vec<AnalystViewSummary> {
    let mut out: Vec<AnalystViewSummary> = Vec::new();
    for v in views {
        // Measurement layers (blind, antithesis) never feed report cards.
        if !crate::db::analyst_views::is_canonical_analyst(&v.analyst) {
            continue;
        }
        if !assets.iter().any(|a| a.eq_ignore_ascii_case(&v.asset)) {
            continue;
        }
        if out
            .iter()
            .any(|s| s.layer.eq_ignore_ascii_case(&v.analyst) && s.asset == v.asset)
        {
            continue;
        }
        out.push(AnalystViewSummary {
            layer: v.analyst.to_uppercase(),
            asset: v.asset.clone(),
            // Neutralise the markdown cell delimiter so a stray '|' in the
            // rationale can't break the multi-timeframe table layout.
            summary: first_sentence(&v.reasoning_summary).replace('|', "/"),
        });
    }
    out
}

/// Select recent news entries whose topic is in `topics` (case-insensitive),
/// limited to the top 5.
fn news_for<'a>(
    news: &'a [crate::db::news_cache::NewsEntry],
    topics: &[&str],
) -> Vec<&'a crate::db::news_cache::NewsEntry> {
    news.iter()
        .filter(|n| topics.iter().any(|t| t.eq_ignore_ascii_case(&n.topic)))
        .take(5)
        .collect()
}

fn news_signal_for_metals(n: &crate::db::news_cache::NewsEntry) -> PreciousMetalsNewsSignal {
    PreciousMetalsNewsSignal {
        headline: n.title.clone(),
        domain: n.source_domain.clone(),
        source_tier: Some(n.source_tier as u8),
        independence: Some(independence_label(n.source_independence)),
        topic: (!n.topic.is_empty()).then(|| n.topic.clone()),
        relevance: n.symbol_tag.clone(),
    }
}

fn news_signal_for_equity(n: &crate::db::news_cache::NewsEntry) -> EquityNewsSignal {
    EquityNewsSignal {
        headline: n.title.clone(),
        domain: n.source_domain.clone(),
        source_tier: Some(n.source_tier as u8),
        independence: Some(independence_label(n.source_independence)),
        topic: (!n.topic.is_empty()).then(|| n.topic.clone()),
        relevance: n.symbol_tag.clone(),
    }
}

/// Build the `SynthesisSnapshot` from the latest narrative-snapshot JSON. The
/// headline becomes the summary; the subtitle (or the strongest scenario shift
/// driver) becomes the central tension.
fn synthesis_from_narrative(narrative: &serde_json::Value) -> Option<SynthesisSnapshot> {
    let headline = narrative.get("headline").and_then(|v| v.as_str())?;
    if headline.trim().is_empty() {
        return None;
    }
    let central_tension = narrative
        .get("subtitle")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());
    Some(SynthesisSnapshot {
        summary: headline.to_string(),
        central_tension,
    })
}

/// Map scenario name → 7-day probability delta (percentage points) from the
/// narrative snapshot's `scenario_shifts` array. Empty when absent.
fn scenario_shift_map(narrative: Option<&serde_json::Value>) -> std::collections::HashMap<String, f64> {
    let mut map = std::collections::HashMap::new();
    let Some(shifts) = narrative
        .and_then(|v| v.get("scenario_shifts"))
        .and_then(|v| v.as_array())
    else {
        return map;
    };
    for shift in shifts {
        let (Some(name), Some(delta)) = (
            shift.get("name").and_then(|v| v.as_str()),
            shift.get("delta_pct").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };
        map.insert(name.to_string(), delta);
    }
    map
}

/// `YYYY-MM-DD` of the day 7 calendar days before `report_date`, or `None` if
/// the date is unparseable.
fn week_ago_date(report_date: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(report_date, "%Y-%m-%d")
        .ok()
        .map(|d| (d - chrono::Duration::days(7)).format("%Y-%m-%d").to_string())
}

/// Whole days between `from` and `to` (`YYYY-MM-DD`); negative if `to` is past.
fn days_between(from: &str, to: &str) -> i64 {
    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    match (parse(from), parse(to)) {
        (Some(a), Some(b)) => (b - a).num_days(),
        _ => 0,
    }
}

/// Daily change % from the cache row's `previous_close` (Yahoo's regular-market
/// previous close), `None` when no previous close is recorded.
fn daily_change_pct(q: &crate::models::price::PriceQuote) -> Option<f64> {
    let prev = q.previous_close?;
    if prev.is_zero() {
        return None;
    }
    let pct = ((q.price - prev) / prev) * rust_decimal::Decimal::from(100);
    Some(dec_to_f64(pct.round_dp(2)))
}

/// Weekly change % vs the close 7 days before the report date from
/// `price_history`. `None` when no historical anchor exists.
fn weekly_change_pct(
    backend: &BackendConnection,
    symbol: &str,
    current: rust_decimal::Decimal,
    week_ago: Option<&str>,
) -> Option<f64> {
    let date = week_ago?;
    let prior = crate::db::price_history::get_price_at_date_backend(backend, symbol, date)
        .ok()
        .flatten()?;
    if prior.is_zero() {
        return None;
    }
    let pct = ((current - prior) / prior) * rust_decimal::Decimal::from(100);
    Some(dec_to_f64(pct.round_dp(2)))
}

/// Compact trend signal from the latest daily technical snapshot: position
/// relative to the 50- and 200-day SMAs. `None` when no snapshot exists.
fn trend_signal(backend: &BackendConnection, symbol: &str) -> Option<String> {
    let snap =
        crate::db::technical_snapshots::get_latest_snapshot_backend(backend, symbol, "1d")
            .ok()
            .flatten()?;
    let above_50 = snap.above_sma_50.unwrap_or(false);
    let above_200 = snap.above_sma_200.unwrap_or(false);
    let trend = match (above_50, above_200) {
        (true, true) => "above 50/200 SMA (uptrend)",
        (true, false) => "above 50 SMA, below 200 SMA",
        (false, true) => "below 50 SMA, above 200 SMA",
        (false, false) => "below 50/200 SMA (downtrend)",
    };
    Some(trend.to_string())
}

/// Build the equity market-table rows for a curated symbol list.
fn equity_rows(
    backend: &BackendConnection,
    price_map: &std::collections::HashMap<String, &crate::models::price::PriceQuote>,
    assets: &[(&str, &str)],
    week_ago: Option<&str>,
) -> Vec<EquityMarketRow> {
    assets
        .iter()
        .filter_map(|(symbol, name)| {
            let q = price_map.get(*symbol)?;
            Some(EquityMarketRow {
                name: name.to_string(),
                symbol: symbol.to_string(),
                price: Some(format_price(q.price)),
                daily_change_pct: daily_change_pct(q),
                weekly_change_pct: weekly_change_pct(backend, symbol, q.price, week_ago),
                trend: trend_signal(backend, symbol),
                freshness: Some(short_date(&q.fetched_at)),
            })
        })
        .collect()
}

/// Build the Gold section's real-yield context from `real_yields_history`.
/// Prefers a TIPS real-yield series (DFII10 / DFII5 / DGS10); direction is
/// inferred from the latest vs prior observation. `None` when no series exists.
fn load_real_yield_context(backend: &BackendConnection) -> Option<RealYieldSummary> {
    let latest = crate::db::real_yields_history::fetch_latest_per_series_backend(backend).ok()?;
    // Preference order: 10Y TIPS real yield, 5Y TIPS, nominal 10Y as a last
    // resort so the section always has a rates anchor.
    let row = ["DFII10", "DFII5", "DGS10", "T5YIE"]
        .iter()
        .find_map(|series| latest.iter().find(|r| r.series == *series))?;
    // Direction from the two most recent observations of this series. History
    // is returned date-ascending, so the last two entries are the newest.
    let history =
        crate::db::real_yields_history::fetch_history_backend(backend, Some(&row.series), None)
            .unwrap_or_default();
    let direction = match history.as_slice() {
        [.., prior, newest] => {
            if newest.value > prior.value {
                "rising"
            } else if newest.value < prior.value {
                "falling"
            } else {
                "flat"
            }
        }
        _ => "unknown",
    };
    let label = match row.series.as_str() {
        "DFII10" => "10Y TIPS real yield",
        "DFII5" => "5Y TIPS real yield",
        "DGS10" => "10Y Treasury nominal yield",
        "T5YIE" => "5Y breakeven inflation",
        other => other,
    };
    let interpretation = match direction {
        "rising" => "Rising real yields raise the opportunity cost of holding non-yielding metals",
        "falling" => "Falling real yields lower the opportunity cost of holding non-yielding metals",
        _ => "Stable real yields leave the rate impulse on metals broadly neutral",
    };
    Some(RealYieldSummary {
        value: Some(format!("{} {:.2}%", label, row.value)),
        direction: Some(direction.to_string()),
        interpretation: Some(interpretation.to_string()),
        freshness: Some(short_date(&row.date)),
    })
}

/// Aggregate BTC ETF flow rows from `capital_flows` into 1d / 7d / 30d
/// net-flow summaries. The 1d window uses `period_end == report_date - 1d`;
/// 7d and 30d sum every row whose `period_end` falls within the window.
/// Degrades to an empty Vec when the table is absent or empty.
fn load_bitcoin_etf_flow_summaries(
    backend: &BackendConnection,
    report_date: &str,
) -> Vec<BitcoinEtfFlowSummary> {
    use chrono::{Duration, NaiveDate};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let parsed = NaiveDate::parse_from_str(report_date, "%Y-%m-%d").ok();
    let conn = match backend.sqlite_native() {
        Some(c) => c,
        None => return Vec::new(),
    };
    let since = parsed
        .map(|d| (d - Duration::days(30)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string());
    let filter = crate::db::capital_flows::FlowFilter {
        asset: Some("BTC"),
        since: Some(&since),
        flow_type: None,
    };
    let rows = match crate::db::capital_flows::list(conn, &filter) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let etf_rows: Vec<_> = rows
        .into_iter()
        .filter(|r| {
            r.flow_type == "etf_creation" || r.flow_type == "etf_redemption"
        })
        .collect();
    if etf_rows.is_empty() {
        return Vec::new();
    }

    let signed = |row: &crate::db::capital_flows::CapitalFlowRow| -> Option<Decimal> {
        let amount = Decimal::from_str(&row.amount_usd).ok()?;
        if row.flow_type == "etf_redemption" {
            Some(-amount)
        } else {
            Some(amount)
        }
    };

    let window_sum = |days: i64| -> (Decimal, usize, Option<String>) {
        let cutoff = parsed.map(|d| d - Duration::days(days));
        let mut net = Decimal::ZERO;
        let mut count = 0usize;
        let mut newest_fetched: Option<String> = None;
        for r in &etf_rows {
            if let Some(c) = cutoff {
                let end = NaiveDate::parse_from_str(&r.period_end, "%Y-%m-%d").ok();
                if end.is_none_or(|e| e < c) {
                    continue;
                }
            }
            if let Some(s) = signed(r) {
                net += s;
                count += 1;
                match &newest_fetched {
                    None => newest_fetched = Some(r.fetched_at.clone()),
                    Some(existing) if r.fetched_at > *existing => {
                        newest_fetched = Some(r.fetched_at.clone());
                    }
                    _ => {}
                }
            }
        }
        (net, count, newest_fetched)
    };

    let mut out = Vec::new();
    for (label, days) in [("1d", 1i64), ("7d", 7), ("30d", 30)] {
        let (net, count, newest) = window_sum(days);
        if count == 0 {
            continue;
        }
        let net_flow = Some(format_usd_compact(net));
        let detail = Some(format!(
            "{} fund-day flow row{}",
            count,
            if count == 1 { "" } else { "s" }
        ));
        let freshness = newest.map(|s| short_date(&s));
        out.push(BitcoinEtfFlowSummary {
            period: label.to_string(),
            net_flow,
            detail,
            freshness,
        });
    }
    out
}

/// Render a signed USD amount compactly (e.g. `+$245.3M`, `-$1.20B`).
fn format_usd_compact(amount: rust_decimal::Decimal) -> String {
    use rust_decimal::Decimal;
    let sign = if amount.is_sign_negative() { "-" } else { "+" };
    let abs = amount.abs();
    let billion = Decimal::from(1_000_000_000u64);
    let million = Decimal::from(1_000_000u64);
    let thousand = Decimal::from(1_000u64);
    if abs >= billion {
        format!("{}${:.2}B", sign, dec_to_f64(abs / billion))
    } else if abs >= million {
        format!("{}${:.1}M", sign, dec_to_f64(abs / million))
    } else if abs >= thousand {
        format!("{}${:.0}K", sign, dec_to_f64(abs / thousand))
    } else {
        format!("{}${}", sign, abs.normalize())
    }
}

/// Pull the latest cached BTC on-chain metrics from `onchain_cache` and
/// shape them for the report's "On-chain and exchange-reserve context"
/// table. Empty when neither metric has any rows.
fn load_bitcoin_onchain_summaries(backend: &BackendConnection) -> Vec<BitcoinOnChainSummary> {
    let mut out = Vec::new();

    // Network: hash rate is the most-stable cross-cycle signal we have. The
    // raw value stored is the hash rate in EH/s (best-effort numeric).
    if let Ok(latest) = crate::db::onchain_cache::get_metrics_by_type_backend(backend, "network", 1)
    {
        if let Some(row) = latest.first() {
            let value = format_hash_rate(&row.value);
            out.push(BitcoinOnChainSummary {
                metric: "Network hash rate".to_string(),
                value: Some(value),
                interpretation: Some(
                    "Higher hash rate = stronger miner conviction / network security".to_string(),
                ),
                freshness: Some(short_date(&row.fetched_at)),
            });
        }
    }

    // Exchange reserve proxy — the refresh hook stuffs 7d / 30d flow figures
    // in the metric metadata JSON, so surface those alongside the headline
    // reserve number when present.
    if let Ok(latest) = crate::db::onchain_cache::get_metrics_by_type_backend(
        backend,
        "exchange_reserve_proxy_btc",
        1,
    ) {
        if let Some(row) = latest.first() {
            let (flow_7d, flow_30d) = onchain_flow_from_metadata(row.metadata.as_deref());
            let mut interp = "Exchange reserve proxy — falling balance is bullish (coins moving to cold storage)".to_string();
            if let Some(net7) = flow_7d {
                interp.push_str(&format!(" · 7d net flow {:+.0} BTC", net7));
            }
            if let Some(net30) = flow_30d {
                interp.push_str(&format!(" · 30d net flow {:+.0} BTC", net30));
            }
            out.push(BitcoinOnChainSummary {
                metric: "Exchange reserve (proxy)".to_string(),
                value: Some(format!("{} BTC", row.value)),
                interpretation: Some(interp),
                freshness: Some(short_date(&row.fetched_at)),
            });
        }
    }

    out
}

/// Format a stored hash-rate value into a readable EH/s string. The cache
/// stores raw numerics, so trim long decimals and append the unit.
fn format_hash_rate(raw: &str) -> String {
    if let Ok(v) = raw.parse::<f64>() {
        // Heuristic: stored values are typically in EH/s already (current
        // network is ~600 EH/s as of 2025-2026), so format with one decimal.
        if v >= 1.0 {
            return format!("{:.1} EH/s", v);
        }
    }
    raw.to_string()
}

/// Pull the `flow_7d_btc` and `flow_30d_btc` fields out of an
/// exchange-reserve metric's JSON metadata, if present.
fn onchain_flow_from_metadata(metadata: Option<&str>) -> (Option<f64>, Option<f64>) {
    let json = match metadata.and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok()) {
        Some(v) => v,
        None => return (None, None),
    };
    let flow_7d = json.get("flow_7d_btc").and_then(|v| v.as_f64());
    let flow_30d = json.get("flow_30d_btc").and_then(|v| v.as_f64());
    (flow_7d, flow_30d)
}

/// Map a sorted upcoming-events list into the macro catalyst rows surfaced
/// in the private Macro Context section. Keep medium + high impact only;
/// catalyst readers expect actionable rather than exhaustive.
fn calendar_to_macro_catalysts(
    events: &[crate::db::calendar_cache::CalendarEvent],
    limit: usize,
) -> Vec<PrivateMacroCatalyst> {
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    events
        .iter()
        .filter(|e| {
            e.event_type == "economic"
                && matches!(effective_impact(e).as_str(), "high" | "medium")
        })
        .filter(|e| seen.insert((e.date.clone(), canonical_calendar_key(&e.name))))
        .take(limit)
        .map(|e| PrivateMacroCatalyst {
            date: e.date.clone(),
            event: e.name.clone(),
            impact: catalyst_impact_label(e),
        })
        .collect()
}

/// Collapse common feed variants onto a single canonical key so
/// "Non Farm Payrolls", "Non-Farm Payrolls", and "Nonfarm Payrolls
/// Private" (all referring to the same monthly release) dedup to one
/// entry. Mirrors the function used in `private_upcoming_calendar.rs`
/// but lives here so both the binary-catalysts loader and the macro-
/// catalysts loader see the deduped list — without it the 2026-06-05
/// weekly run rendered three NFP decision-pending cards with
/// conflicting forecasts.
fn canonical_calendar_key(headline: &str) -> String {
    let lower = headline
        .to_lowercase()
        .replace(['-', '_'], " ");
    let collapsed: String = lower
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let normalized = collapsed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    const FAMILIES: &[(&str, &str)] = &[
        ("non farm payrolls private", "nfp"),
        ("nonfarm payrolls private", "nfp"),
        ("non farm payrolls", "nfp"),
        ("nonfarm payrolls", "nfp"),
        ("nfp", "nfp"),
        ("average hourly earnings mom", "avg-hourly-earnings-mom"),
        ("average hourly earnings yoy", "avg-hourly-earnings-yoy"),
        ("core cpi yoy", "core-cpi-yoy"),
        ("core cpi mom", "core-cpi-mom"),
        ("cpi yoy", "cpi-yoy"),
        ("cpi mom", "cpi-mom"),
        ("core pce price index", "core-pce"),
        ("core pce", "core-pce"),
        ("pce price index", "pce"),
        ("fomc", "fomc"),
        ("federal funds rate", "fomc"),
        ("interest rate decision", "fomc"),
        ("u 6 unemployment rate", "u6-unemployment"),
        ("u6 unemployment rate", "u6-unemployment"),
        ("unemployment rate", "unemployment-rate"),
    ];
    for (variant, canonical) in FAMILIES {
        if normalized.contains(variant) {
            return (*canonical).to_string();
        }
    }
    normalized
}

/// Map the upcoming-events list into the binary catalyst rows that drive
/// the private Decisions-Pending and Bottom-Line catalyst card. Only
/// high-impact economic events within `horizon_days` of `report_date`
/// qualify — the section asks "what binary print could move the book?",
/// not "what's coming up in general?".
fn calendar_to_binary_catalysts(
    events: &[crate::db::calendar_cache::CalendarEvent],
    report_date: &str,
    horizon_days: i64,
    limit: usize,
) -> Vec<BinaryCatalystSummary> {
    use chrono::{Duration, NaiveDate};
    let from = match NaiveDate::parse_from_str(report_date, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let cutoff = from + Duration::days(horizon_days);
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    events
        .iter()
        .filter(|e| e.event_type == "economic" && effective_impact(e) == "high")
        .filter(|e| seen.insert((e.date.clone(), canonical_calendar_key(&e.name))))
        .filter(|e| {
            NaiveDate::parse_from_str(&e.date, "%Y-%m-%d")
                .map(|d| d >= from && d <= cutoff)
                .unwrap_or(false)
        })
        .take(limit)
        .map(|e| BinaryCatalystSummary {
            date: e.date.clone(),
            event: e.name.clone(),
            impact: catalyst_impact_label(e),
        })
        .collect()
}

/// Re-classify a calendar event's impact at read time. Returns the higher
/// of the stored impact and the impact inferred from the event name, so
/// upstream feeds that mis-tag NFP / CPI / FOMC as "low" don't silently
/// drop those events from the binary-catalyst slate. The name-based
/// heuristic mirrors the keyword set in `data::calendar::classify_impact`
/// but lives here as a read-time backstop — when the refresh upgrades the
/// stored impact, this function still agrees with it.
fn effective_impact(event: &crate::db::calendar_cache::CalendarEvent) -> String {
    let stored = event.impact.to_lowercase();
    let inferred = infer_impact_from_name(&event.name);
    higher_impact(&stored, &inferred)
}

fn infer_impact_from_name(name: &str) -> String {
    let lower = name.to_lowercase();
    const HIGH: &[&str] = &[
        "fomc",
        "fed minutes",
        "federal funds",
        "interest rate decision",
        "rate decision",
        "nonfarm payroll",
        "non farm payroll",
        "non-farm payroll",
        "nfp",
        "unemployment rate",
        "core cpi",
        "core pce",
        "cpi yoy",
        "cpi mom",
        "headline cpi",
        "inflation rate",
        "gdp growth",
        "gdp annualized",
        "advance gdp",
        "pce price",
        "core inflation",
        "retail sales",
        "advance retail sales",
        "jobless claims",
        "initial claims",
        "continuing claims",
        "ism manufacturing",
        "ism services",
        "ism non-manufacturing",
        "pmi composite",
        "manufacturing pmi",
        "services pmi",
        "jolts",
        "adp employment",
        "adp non-farm",
        "consumer confidence",
        "michigan sentiment",
        "consumer sentiment",
        "powell",
        "fed chair",
        "fed speaks",
        "ppi yoy",
        "ppi mom",
        "core ppi",
        "average hourly earnings",
    ];
    const MEDIUM: &[&str] = &[
        "housing",
        "durable goods",
        "factory orders",
        "wholesale",
        "trade balance",
        "business inventories",
        "capacity utilization",
        "participation rate",
        "average weekly",
        "government payroll",
        "manufacturing payroll",
        "construction spending",
        "industrial production",
        "redbook",
        "consumer credit",
        "philadelphia fed",
        "empire state",
        "import price",
        "export price",
        "personal income",
        "personal spending",
    ];
    if HIGH.iter().any(|k| lower.contains(k)) {
        return "high".to_string();
    }
    if MEDIUM.iter().any(|k| lower.contains(k)) {
        return "medium".to_string();
    }
    "low".to_string()
}

/// Expand a held-asset symbol into the set of canonical aliases news feeds
/// might use. Always includes the symbol itself in upper-case. Aliases are
/// uppercase; matching is case-insensitive after upper-casing the news tag.
fn symbol_aliases_for_news(symbol: &str) -> Vec<String> {
    let upper = symbol.to_uppercase();
    let mut out: Vec<String> = vec![upper.clone()];
    let extras: &[&str] = match upper.as_str() {
        // Gold — futures, ETF tickers, metal codes, common feed labels.
        "GC=F" | "GLD" | "GOLD" | "XAU" | "XAU=X" | "XAUUSD=X" => {
            &["GC=F", "GLD", "GOLD", "XAU", "XAU=X", "XAUUSD=X", "IAU"]
        }
        // Silver — same idea.
        "SI=F" | "SLV" | "XAG" | "XAG=X" | "XAGUSD=X" => {
            &["SI=F", "SLV", "XAG", "XAG=X", "XAGUSD=X"]
        }
        // Bitcoin — spot ticker, futures, ETF aliases.
        "BTC" | "BTC-USD" | "BTCUSD=X" | "IBIT" | "FBTC" => {
            &["BTC", "BTC-USD", "BTCUSD=X", "BITCOIN", "IBIT", "FBTC", "BITO"]
        }
        // Ethereum.
        "ETH" | "ETH-USD" | "ETHUSD=X" => &["ETH", "ETH-USD", "ETHUSD=X", "ETHEREUM"],
        // Dollar index — DXY is the feed canonical, but Yahoo uses DX-Y.NYB.
        "DX-Y.NYB" | "DXY" | "USD" => &["DX-Y.NYB", "DXY", "USD", "USDX"],
        // WTI crude oil.
        "CL=F" | "USO" | "WTI" => &["CL=F", "USO", "WTI", "OIL"],
        // 10Y treasury yield.
        "^TNX" | "TNX" | "10Y" => &["^TNX", "TNX", "10Y", "US10Y"],
        // S&P 500.
        "^GSPC" | "SPY" | "SPX" => &["^GSPC", "SPY", "SPX", "ES=F"],
        // VIX.
        "^VIX" | "VIX" | "VIXY" => &["^VIX", "VIX", "VIXY"],
        _ => &[],
    };
    for alias in extras {
        let s = alias.to_string();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn higher_impact(a: &str, b: &str) -> String {
    let rank = |s: &str| match s {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    };
    if rank(a) >= rank(b) {
        a.to_string()
    } else {
        b.to_string()
    }
}

/// Compose a human-readable impact sentence for a catalyst row. We blend the
/// raw impact bucket with the forecast / previous values when present so the
/// catalyst card has context rather than just "high".
fn catalyst_impact_label(event: &crate::db::calendar_cache::CalendarEvent) -> String {
    let effective = effective_impact(event);
    let bucket = match effective.as_str() {
        "high" => "High-impact",
        "medium" => "Medium-impact",
        "low" => "Low-impact",
        _ => effective.as_str(),
    };
    let context = match (event.forecast.as_deref(), event.previous.as_deref()) {
        (Some(f), Some(p)) => format!(" (forecast {f}, prior {p})"),
        (Some(f), None) => format!(" (forecast {f})"),
        (None, Some(p)) => format!(" (prior {p})"),
        (None, None) => String::new(),
    };
    format!("{bucket} {}{context}", event.event_type)
}

/// Build the private "lessons applied" summary directly from the prediction +
/// lesson tables. Counts how many predictions reference each lesson and surfaces
/// the most-referenced ones. `None` when there are no predictions at all.
fn load_lessons_applied(conn: &rusqlite::Connection) -> Result<Option<PrivateLessonsAppliedSummary>> {
    let predictions =
        crate::db::user_predictions::list_predictions(conn, None, None, None, None)?;
    if predictions.is_empty() {
        return Ok(None);
    }
    let lessons = crate::db::prediction_lessons::list_lessons(conn, None, None)?;
    let lesson_by_id: std::collections::HashMap<i64, &crate::db::prediction_lessons::PredictionLesson> =
        lessons.iter().map(|l| (l.id, l)).collect();

    let mut counts: std::collections::BTreeMap<i64, u32> = std::collections::BTreeMap::new();
    let mut guarded = 0u32;
    for p in &predictions {
        if p.lessons_applied.is_empty() {
            continue;
        }
        guarded += 1;
        let unique: std::collections::BTreeSet<i64> =
            p.lessons_applied.iter().copied().collect();
        for id in unique {
            *counts.entry(id).or_default() += 1;
        }
    }

    let mut lesson_references: Vec<PrivateLessonReferenceRow> = counts
        .iter()
        // Drop references whose lesson row cannot be resolved (the prediction
        // cited an out-of-range ID — a symptom of mixing prediction_id and
        // lesson.id at prompt time). These are not real lesson cites, so they
        // are excluded from the reader-facing list entirely rather than
        // surfaced as a bare "Lesson #N" placeholder or a maintainer
        // diagnostic. The discrepancy is still observable in the headline
        // counts (guarded predictions vs resolved unique lessons).
        .filter_map(|(id, references)| {
            let lesson = lesson_by_id.get(id)?;
            let summary = first_sentence(&lesson.what_predicted);
            if summary.is_empty() {
                return None;
            }
            Some(PrivateLessonReferenceRow {
                lesson_id: *id,
                references: *references,
                miss_type: Some(lesson.miss_type.clone()).filter(|s| !s.is_empty()),
                summary,
            })
        })
        .collect();
    lesson_references.sort_by(|a, b| {
        b.references
            .cmp(&a.references)
            .then_with(|| a.lesson_id.cmp(&b.lesson_id))
    });
    // Count of resolved unique lessons (excludes dropped unresolvable IDs) so
    // the headline never claims more unique lessons than are actually listed.
    let resolved_unique = lesson_references.len() as u32;
    lesson_references.truncate(8);

    Ok(Some(PrivateLessonsAppliedSummary {
        since: "all-time".to_string(),
        total_predictions: predictions.len() as u32,
        guarded_predictions: guarded,
        unique_lessons: resolved_unique,
        lesson_references,
        strongest_analog: None,
    }))
}

/// Cap an analyst-note excerpt to roughly `max_chars` characters so the
/// Bottom Line / Executive Summary stays scannable. We slice on byte
/// boundaries while respecting char boundaries to keep this UTF-8 safe.
fn truncate_excerpt(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(max_chars).collect();
    format!("{}…", truncated.trim_end())
}

/// Build `TodaysAnalystSynthesis` from today's `daily_notes` (one per
/// timeframe analyst, picking the longest substantive note as the
/// "headline" proxy) plus a scan for the largest |%| move tied to a held
/// asset, plus the highest-priority `to='synthesis'` agent message of the
/// day. Returns `None` when neither table contributes anything for the
/// day so the renderers fall back to their existing behavior.
fn load_todays_analyst_synthesis(
    conn: &rusqlite::Connection,
    report_date: &str,
    held_assets: &[String],
) -> Result<Option<TodaysAnalystSynthesis>> {
    let analyst_authors = [
        ("analyst-low", 0usize),
        ("analyst-medium", 1),
        ("analyst-high", 2),
        ("analyst-macro", 3),
    ];

    // 1) Headline note per analyst — pick the longest content row per
    //    author for the report date. Length is a cheap stand-in for
    //    "substantive" and avoids accidentally promoting a one-line ping.
    let mut headlines: [Option<String>; 4] = Default::default();
    let mut all_notes_today: Vec<crate::db::daily_notes::DailyNote> = Vec::new();
    for (author, idx) in analyst_authors.iter() {
        let notes = crate::db::daily_notes::list_notes(
            conn,
            Some(report_date),
            None,
            None,
            Some(*author),
        )
        .unwrap_or_default();
        if let Some(longest) = notes
            .iter()
            .max_by_key(|n| n.content.trim().chars().count())
            .filter(|n| !n.content.trim().is_empty())
        {
            headlines[*idx] = Some(truncate_excerpt(&longest.content, 200));
        }
        all_notes_today.extend(notes);
    }

    // 2) Leading move — scan today's analyst notes for tokens like
    //    `BTC -7.0%` or `GLD +1.25%` and keep the largest |move_pct|
    //    that names a currently held asset. Cumulative is captured when
    //    a second `cum ...%` token follows in the same sentence.
    let leading_move = scan_leading_move(&all_notes_today, held_assets);

    // 3) Action summary — highest-priority synthesis-bound agent message
    //    from today. `since` is the start of the day in the same
    //    YYYY-MM-DD format daily_notes uses, which agent_messages stores
    //    as `created_at`.
    let mut action_summary: Option<String> = None;
    let messages = crate::db::agent_messages::list_messages(
        conn,
        None,
        Some("synthesis"),
        None,
        false,
        Some(report_date),
        None,
        Some(32),
    )
    .unwrap_or_default();
    // Order by priority (high > normal > low) then created_at DESC.
    let priority_rank = |p: &str| match p {
        "high" => 0u8,
        "normal" => 1,
        _ => 2,
    };
    let mut prioritized: Vec<&crate::db::agent_messages::AgentMessage> = messages
        .iter()
        .filter(|m| matches!(m.priority.as_str(), "high" | "normal"))
        // Filter out messages that don't belong in the Bottom Line "Action"
        // bullet. Four gates:
        //   1. decision-card category — those are JSON envelopes for the
        //      Decisions Pending section (now chat-only).
        //   2. alert category — these are pftui-system divergence alerts
        //      (the 2026-06-07 run picked one up: "Narrative-vs-money
        //      divergence crossed 2.0σ for Hard Recession… Topic equities;
        //      weighted news volume 28.80, sentiment 5.3; market price
        //      unavailable…"). Not actionable, leaks raw metric text.
        //   3. sender-prefix — analyst-decisions / panel-* never belong here.
        //   4. JSON-prefix — defensive against any other JSON envelope.
        // The Bottom Line should pick a prose action_summary the synthesis
        // writer (analyst-synthesis) explicitly wrote for this slot.
        .filter(|m| {
            let cat = m.category.as_deref().unwrap_or("");
            if matches!(cat, "decision-card" | "alert") {
                return false;
            }
            if m.from_agent == "analyst-decisions"
                || m.from_agent.starts_with("panel-")
                || m.from_agent == "pftui"
            {
                return false;
            }
            !m.content.trim_start().starts_with('{')
        })
        .collect();
    prioritized.sort_by(|a, b| {
        priority_rank(&a.priority)
            .cmp(&priority_rank(&b.priority))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    if let Some(m) = prioritized.first() {
        action_summary = Some(truncate_excerpt(&m.content, 200));
    }

    let synthesis = TodaysAnalystSynthesis {
        headline_low: headlines[0].clone(),
        headline_medium: headlines[1].clone(),
        headline_high: headlines[2].clone(),
        headline_macro: headlines[3].clone(),
        leading_move,
        action_summary,
    };

    let empty = synthesis.headline_low.is_none()
        && synthesis.headline_medium.is_none()
        && synthesis.headline_high.is_none()
        && synthesis.headline_macro.is_none()
        && synthesis.leading_move.is_none()
        && synthesis.action_summary.is_none();

    Ok((!empty).then_some(synthesis))
}

/// Regex-scan today's analyst notes for the largest absolute %-move that
/// mentions a held asset. Matches tokens like `BTC -7.0%`, `GLD +1.25%`,
/// `SPY 2.4%`, optionally followed by `cum [-+]?N.M% from <baseline>`.
fn scan_leading_move(
    notes: &[crate::db::daily_notes::DailyNote],
    held_assets: &[String],
) -> Option<MaterialMove> {
    // `\b[A-Z][A-Z0-9.=^-]{0,9}\s*[-+]?\d{1,3}(?:\.\d+)?%`
    // Accept both `BTC -7%` and `BTC -7.0%`; trailing `=F` / `^VIX`-style
    // suffixes are common in our symbol set.
    let token_re = regex::Regex::new(
        r"\b([A-Z][A-Z0-9.=^-]{0,9})\s*([-+]?\d{1,3}(?:\.\d+)?)%",
    )
    .ok()?;
    let cum_re = regex::Regex::new(
        r"cum\s+([-+]?\d{1,3}(?:\.\d+)?)%(?:\s+from\s+([^.\n,;]+))?",
    )
    .ok()?;

    let held: std::collections::HashSet<String> =
        held_assets.iter().map(|s| s.to_ascii_uppercase()).collect();

    let mut best: Option<MaterialMove> = None;
    for note in notes {
        for cap in token_re.captures_iter(&note.content) {
            let asset = cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            let pct_str = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let Ok(pct) = pct_str.parse::<f64>() else {
                continue;
            };
            if !held.contains(&asset.to_ascii_uppercase()) {
                continue;
            }
            if pct.abs() < 0.5 {
                // Filter trivial moves so the lead bullet stays meaningful.
                continue;
            }
            let is_better = best
                .as_ref()
                .map(|b| pct.abs() > b.move_pct.abs())
                .unwrap_or(true);
            if !is_better {
                continue;
            }
            // Look at the surrounding ~160 chars of the matched token to
            // build the note + cumulative context.
            let match_end = cap
                .get(0)
                .map(|m| m.end())
                .unwrap_or(0)
                .min(note.content.len());
            let window_end = (match_end + 160).min(note.content.len());
            let window = &note.content[match_end..window_end];
            let cumulative_pct =
                cum_re.captures(window).and_then(|c| c.get(1)?.as_str().parse::<f64>().ok());
            // Note: trim to the matched sentence-ish window.
            let note_snippet = truncate_excerpt(window, 160);
            best = Some(MaterialMove {
                asset,
                move_pct: pct,
                cumulative_pct,
                note: note_snippet,
            });
        }
    }
    best
}

// ---------------------------------------------------------------------------
// W4 loader helpers: news / calibration / lessons / trajectories / outlooks
// ---------------------------------------------------------------------------

/// Pick the news entries whose `symbol_tag` matches any held symbol
/// (case-insensitive) and map them to the per-asset catalyst struct. The list
/// is already newest-first from `news_cache::get_latest_news`.
///
/// Matching expands each held symbol to its known canonical aliases — news
/// feeds tag gold rows as "XAU", "GOLD", or "GLD" but the operator's
/// portfolio symbol is the futures contract "GC=F"; without alias expansion
/// every gold-related news item silently fails to connect to the held
/// position and the report's News & Catalysts section renders empty.
fn private_news_events_for_held(
    news: &[crate::db::news_cache::NewsEntry],
    held: &[String],
) -> Vec<PrivateNewsCatalyst> {
    if held.is_empty() {
        return Vec::new();
    }
    let held_set: std::collections::HashSet<String> = held
        .iter()
        .flat_map(|s| symbol_aliases_for_news(s))
        .collect();
    news.iter()
        .filter_map(|n| {
            // First try the structured symbol_tag (exact-match against
            // alias-expanded held set).
            let tagged: Vec<String> = n
                .symbol_tag
                .as_deref()
                .map(|tag| {
                    tag.split([',', ';', ' '])
                        .map(|s| s.trim().to_uppercase())
                        .filter(|s| !s.is_empty() && held_set.contains(s))
                        .collect()
                })
                .unwrap_or_default();
            // Fallback: when symbol_tag is empty (news ingestion stopped
            // populating it sometime before the 2026-06-05 weekly run —
            // ~100% of recent rows had NULL/empty tags), do case-
            // insensitive substring matching against the title against
            // the same alias-expanded held set, but only for aliases
            // that read as proper words (skip tickers like "USD" /
            // "GLD" that would false-match common prose). Aliases ≥4
            // chars or containing a hyphen/equals are considered safe
            // matchers.
            let matched: Vec<String> = if !tagged.is_empty() {
                tagged
            } else {
                let title_upper = n.title.to_uppercase();
                held_set
                    .iter()
                    .filter(|alias| {
                        alias.len() >= 4
                            || alias.contains('-')
                            || alias.contains('=')
                            || alias.contains('^')
                    })
                    .filter(|alias| {
                        let needle = format!(" {alias} ");
                        let prefix = format!("{alias} ");
                        let suffix = format!(" {alias}");
                        title_upper.contains(&needle)
                            || title_upper.starts_with(&prefix)
                            || title_upper.ends_with(&suffix)
                            || title_upper == **alias
                    })
                    .cloned()
                    .collect()
            };
            if matched.is_empty() {
                return None;
            }
            Some(PrivateNewsCatalyst {
                headline: n.title.clone(),
                what_happened: (!n.description.is_empty()).then(|| n.description.clone()),
                money_moved: None,
                who_benefits: None,
                what_it_means: None,
                domain: n.source_domain.clone(),
                source_tier: Some(n.source_tier as u8),
                independence: Some(independence_label(n.source_independence)),
                topic: (!n.topic.is_empty()).then(|| n.topic.clone()),
                related_assets: matched,
                related_scenarios: Vec::new(),
                impact_score: news_impact_score(n),
            })
        })
        .take(12)
        .collect()
}

/// Read every row from `calibration_matrix` and project onto the renderer's
/// reliability shape. Latest snapshot per (layer, conviction_band) wins.
fn load_calibration_rows(
    conn: &rusqlite::Connection,
) -> Result<Vec<CalibrationReliabilityRow>> {
    let mut stmt = conn.prepare(
        "SELECT layer, COALESCE(topic, ''), COALESCE(conviction_band, ''),
                n, hit_rate, COALESCE(stated_confidence, 0.0), recorded_at
         FROM calibration_matrix
         ORDER BY recorded_at DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut out: Vec<CalibrationReliabilityRow> = Vec::new();
    while let Some(r) = rows.next()? {
        let layer: String = r.get::<_, Option<String>>(0)?.unwrap_or_default();
        let _topic: String = r.get(1)?;
        let band: String = r.get(2)?;
        let n: i64 = r.get(3)?;
        let hit_rate: f64 = r.get(4)?;
        let stated: f64 = r.get(5)?;
        let key = (layer.clone(), band.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(CalibrationReliabilityRow {
            layer: layer.to_uppercase(),
            conviction_band: band,
            predicted_pct: round1(stated * 100.0),
            observed_pct: round1(hit_rate * 100.0),
            sample_size: n.max(0) as u32,
        });
    }
    Ok(out)
}

/// Same as `load_calibration_rows`, but only return rows whose `topic` matches
/// a held-asset symbol (case-insensitive). When held is empty the result is
/// empty — private calibration with no portfolio is undefined.
fn load_calibration_rows_for_held(
    conn: &rusqlite::Connection,
    held: &[String],
) -> Result<Vec<CalibrationReliabilityRow>> {
    if held.is_empty() {
        return Ok(Vec::new());
    }
    let held_set: std::collections::HashSet<String> =
        held.iter().map(|s| s.to_uppercase()).collect();
    let mut stmt = conn.prepare(
        "SELECT layer, COALESCE(topic, ''), COALESCE(conviction_band, ''),
                n, hit_rate, COALESCE(stated_confidence, 0.0), recorded_at
         FROM calibration_matrix
         ORDER BY recorded_at DESC",
    )?;
    let mut rows = stmt.query([])?;
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut out: Vec<CalibrationReliabilityRow> = Vec::new();
    while let Some(r) = rows.next()? {
        let layer: String = r.get::<_, Option<String>>(0)?.unwrap_or_default();
        let topic: String = r.get(1)?;
        let band: String = r.get(2)?;
        let n: i64 = r.get(3)?;
        let hit_rate: f64 = r.get(4)?;
        let stated: f64 = r.get(5)?;
        if topic.is_empty() || !held_set.contains(&topic.to_uppercase()) {
            continue;
        }
        let key = (layer.clone(), band.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(CalibrationReliabilityRow {
            layer: layer.to_uppercase(),
            conviction_band: band,
            predicted_pct: round1(stated * 100.0),
            observed_pct: round1(hit_rate * 100.0),
            sample_size: n.max(0) as u32,
        });
    }
    Ok(out)
}

/// Pick the calibration_matrix row most relevant to the dominant layer of the
/// currently open predictions. "Dominant" = the prediction layer with the most
/// pending rows; tie-breaks alphabetically. Returns `None` when either side is
/// empty. Layer mapping mirrors `commands::calibration::normalize_layer`.
fn load_open_predictions_calibration(
    conn: &rusqlite::Connection,
    open: &[PrivateOpenPredictionRow],
) -> Result<Option<PrivateOpenPredictionsCalibration>> {
    if open.is_empty() {
        return Ok(None);
    }
    // Pull every open prediction's source_agent/timeframe to count by layer.
    let predictions =
        crate::db::user_predictions::list_predictions(conn, Some("pending"), None, None, None)?;
    if predictions.is_empty() {
        return Ok(None);
    }
    let mut layer_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for p in &predictions {
        let layer = p
            .timeframe
            .as_deref()
            .and_then(normalize_pred_layer)
            .or_else(|| {
                p.source_agent
                    .as_deref()
                    .and_then(normalize_pred_layer)
            })
            .unwrap_or_else(|| "unknown".to_string());
        *layer_counts.entry(layer).or_default() += 1;
    }
    let Some((dominant_layer, _)) = layer_counts
        .into_iter()
        .max_by(|(la, ca), (lb, cb)| ca.cmp(cb).then_with(|| lb.cmp(la)))
    else {
        return Ok(None);
    };
    if dominant_layer == "unknown" {
        return Ok(None);
    }
    // Pick the calibration_matrix row matching that layer with the largest n.
    let mut stmt = conn.prepare(
        "SELECT layer, COALESCE(topic, ''), COALESCE(conviction_band, ''),
                n, hit_rate, COALESCE(stated_confidence, 0.0)
         FROM calibration_matrix
         WHERE LOWER(COALESCE(layer, '')) = LOWER(?1)
         ORDER BY n DESC, recorded_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![dominant_layer])?;
    let Some(r) = rows.next()? else {
        return Ok(Some(PrivateOpenPredictionsCalibration {
            layer: Some(dominant_layer),
            sample_size: 0,
            predicted_pct: None,
            observed_pct: None,
        }));
    };
    let layer: String = r.get::<_, Option<String>>(0)?.unwrap_or(dominant_layer);
    let _topic: String = r.get(1)?;
    let _band: String = r.get(2)?;
    let n: i64 = r.get(3)?;
    let hit_rate: f64 = r.get(4)?;
    let stated: f64 = r.get(5)?;
    Ok(Some(PrivateOpenPredictionsCalibration {
        layer: Some(layer),
        sample_size: n.max(0) as u32,
        predicted_pct: Some(round1(stated * 100.0)),
        observed_pct: Some(round1(hit_rate * 100.0)),
    }))
}

/// Reuse the lessons-applied report over the trailing 24h, mapped to the
/// public LessonAppliedSummary shape: lesson_id is rendered as its numeric
/// value, summary is the lesson's miss-type-prefixed why_wrong (already
/// computed by the report), and applied_to lists how many predictions cited
/// the lesson in the window.
fn load_public_lessons_applied(
    conn: &rusqlite::Connection,
) -> Result<Vec<LessonAppliedSummary>> {
    let predictions =
        crate::db::user_predictions::list_predictions(conn, None, None, None, None)?;
    let lessons = crate::db::prediction_lessons::list_lessons(conn, None, None)?;
    let lesson_by_id: std::collections::HashMap<i64, &crate::db::prediction_lessons::PredictionLesson> =
        lessons.iter().map(|l| (l.id, l)).collect();

    // 24h window cutoff in the same UTC form predictions use.
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
    let mut counts: std::collections::BTreeMap<i64, u32> = std::collections::BTreeMap::new();
    for p in &predictions {
        if p.lessons_applied.is_empty() {
            continue;
        }
        // Best-effort created_at parse. Skip if unparseable.
        let is_recent = chrono::DateTime::parse_from_rfc3339(&p.created_at)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc) >= cutoff)
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&p.created_at, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .map(|naive| {
                        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc) >= cutoff
                    })
            })
            .unwrap_or(false);
        if !is_recent {
            continue;
        }
        let unique: std::collections::BTreeSet<i64> = p.lessons_applied.iter().copied().collect();
        for id in unique {
            *counts.entry(id).or_default() += 1;
        }
    }
    let mut out: Vec<LessonAppliedSummary> = counts
        .into_iter()
        .map(|(id, n)| {
            let summary = lesson_by_id
                .get(&id)
                .map(|l| {
                    if l.why_wrong.trim().is_empty() {
                        l.what_predicted.clone()
                    } else {
                        format!("{}: {}", l.miss_type, first_sentence(&l.why_wrong))
                    }
                })
                .unwrap_or_else(|| format!("Lesson #{id}"));
            LessonAppliedSummary {
                lesson_id: format!("L{id}"),
                summary,
                applied_to: Some(format!("{n} prediction{}", if n == 1 { "" } else { "s" })),
            }
        })
        .collect();
    // Most-referenced first, then by id ascending for stability.
    out.sort_by(|a, b| {
        let a_n = a
            .applied_to
            .as_deref()
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let b_n = b
            .applied_to
            .as_deref()
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        b_n.cmp(&a_n).then_with(|| a.lesson_id.cmp(&b.lesson_id))
    });
    out.truncate(10);
    Ok(out)
}

/// Last `days` of conviction values per (held symbol, analyst layer) from
/// `analyst_view_history`. Only emits rows where at least one history point
/// exists. Layer is upper-cased to match `AnalystViewSummary::layer`.
fn load_conviction_trajectories(
    conn: &rusqlite::Connection,
    held: &[String],
    days: i64,
) -> Result<Vec<PrivateConvictionTrajectoryRow>> {
    if held.is_empty() {
        return Ok(Vec::new());
    }
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let held_upper: std::collections::HashSet<String> =
        held.iter().map(|s| s.to_uppercase()).collect();
    let mut stmt = conn.prepare(
        "SELECT analyst, asset, conviction, recorded_at
         FROM analyst_view_history
         WHERE recorded_at >= ?1
         ORDER BY recorded_at ASC",
    )?;
    let mut rows = stmt.query(rusqlite::params![cutoff])?;
    let mut grouped: std::collections::BTreeMap<(String, String), Vec<PrivateConvictionTrajectoryPoint>> =
        std::collections::BTreeMap::new();
    while let Some(r) = rows.next()? {
        let analyst: String = r.get(0)?;
        let asset: String = r.get(1)?;
        let conviction: i64 = r.get(2)?;
        let recorded_at: String = r.get(3)?;
        // Measurement layers (blind, antithesis) never feed report cards.
        if !crate::db::analyst_views::is_canonical_analyst(&analyst) {
            continue;
        }
        let asset_upper = asset.to_uppercase();
        if !held_upper.contains(&asset_upper) {
            continue;
        }
        let date = short_date(&recorded_at);
        grouped
            .entry((asset_upper, analyst.to_uppercase()))
            .or_default()
            .push(PrivateConvictionTrajectoryPoint { date, conviction });
    }
    Ok(grouped
        .into_iter()
        .map(|((symbol, layer), points)| PrivateConvictionTrajectoryRow {
            symbol,
            layer,
            points,
        })
        .collect())
}

/// Per-held-asset outlook by horizon, derived from the latest analyst_views.
/// Mapping: LOW → days, MEDIUM → weeks, HIGH → months, MACRO → months (long-
/// range). When both HIGH and MACRO are present we keep HIGH; MACRO is only
/// surfaced when there is no HIGH view, to avoid double-counting months.
fn outlooks_for_held(
    views: &[crate::db::analyst_views::AnalystView],
    held: &[String],
) -> Vec<PrivateOutlookByHorizonRow> {
    if held.is_empty() {
        return Vec::new();
    }
    let held_upper: Vec<String> = held.iter().map(|s| s.to_uppercase()).collect();
    let mut rows: Vec<PrivateOutlookByHorizonRow> = Vec::new();
    for symbol in &held_upper {
        let mut days: Option<PrivateOutlookPoint> = None;
        let mut weeks: Option<PrivateOutlookPoint> = None;
        let mut months: Option<PrivateOutlookPoint> = None;
        let mut macro_fallback: Option<PrivateOutlookPoint> = None;
        for v in views {
            if !v.asset.eq_ignore_ascii_case(symbol) {
                continue;
            }
            let point = PrivateOutlookPoint {
                direction: v.direction.clone(),
                conviction: v.conviction.to_string(),
            };
            match v.analyst.to_ascii_lowercase().as_str() {
                "low" if days.is_none() => days = Some(point),
                "medium" if weeks.is_none() => weeks = Some(point),
                "high" if months.is_none() => months = Some(point),
                "macro" if macro_fallback.is_none() => macro_fallback = Some(point),
                _ => {}
            }
        }
        if months.is_none() {
            months = macro_fallback;
        }
        if days.is_none() && weeks.is_none() && months.is_none() {
            continue;
        }
        rows.push(PrivateOutlookByHorizonRow {
            symbol: symbol.clone(),
            days,
            weeks,
            months,
        });
    }
    rows
}

/// Round to one decimal place.
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Mirror of `commands::calibration::normalize_layer`, kept local so this
/// loader doesn't depend on a private helper.
fn normalize_pred_layer(value: &str) -> Option<String> {
    let v = value.trim().to_ascii_lowercase();
    if v == "macro-checkpoint" || v.contains("macro-checkpoint") {
        Some("macro-checkpoint".to_string())
    } else if v.contains("low") || v == "short" {
        Some("low".to_string())
    } else if v.contains("medium") || v == "med" {
        Some("medium".to_string())
    } else if v.contains("high") || v == "long" {
        Some("high".to_string())
    } else if v.contains("macro") {
        Some("macro".to_string())
    } else {
        None
    }
}

/// Why/whether a data slot is available for the build. The four states the
/// integrity contract requires: a loader ERROR must never render identically
/// to genuinely-absent data, and "the upstream phase didn't run today" must
/// be distinguishable from "there has never been anything there".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotStatus {
    Populated,
    NoData,
    UpstreamNotRun,
    LoaderError,
}

impl SlotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SlotStatus::Populated => "populated",
            SlotStatus::NoData => "no_data",
            SlotStatus::UpstreamNotRun => "upstream_not_run",
            SlotStatus::LoaderError => "loader_error",
        }
    }
}

/// Snapshot of one `BuildContext` data slot's availability. Used by the
/// dry-run output (the operator's audit surface) and the private report's
/// integrity footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataAvailabilityRow {
    pub field: &'static str,
    pub populated: bool,
    pub status: SlotStatus,
    /// Reason when unpopulated: the loader error text, the upstream-not-run
    /// explanation, or a no-data note. `None` when populated (or when no
    /// more specific reason is known than "no rows").
    pub reason: Option<String>,
}

/// Classify one slot from its populated bit + any recorded issue.
fn availability_row(ctx: &BuildContext, field: &'static str, populated: bool) -> DataAvailabilityRow {
    if populated {
        return DataAvailabilityRow {
            field,
            populated: true,
            status: SlotStatus::Populated,
            reason: None,
        };
    }
    let (status, reason) = match ctx.slot_issues.get(field) {
        Some(SlotIssue::LoaderError(e)) => (SlotStatus::LoaderError, Some(e.clone())),
        Some(SlotIssue::UpstreamNotRun(r)) => (SlotStatus::UpstreamNotRun, Some(r.clone())),
        Some(SlotIssue::NoData(r)) => (SlotStatus::NoData, Some(r.clone())),
        None => (SlotStatus::NoData, None),
    };
    DataAvailabilityRow {
        field,
        populated: false,
        status,
        reason,
    }
}

/// One availability row per data slot on `BuildContext`. EVERY data-bearing
/// field must appear here — enforced by the
/// `every_build_context_slot_is_tracked` conformance test, which parses the
/// struct definition and fails when a new slot ships untracked. The
/// `vec_slot!`/`opt_slot!`/`map_slot!` macros take the field IDENT (not a
/// string) so the reported name can never drift from the struct.
pub fn data_availability(ctx: &BuildContext) -> Vec<DataAvailabilityRow> {
    let mut rows: Vec<DataAvailabilityRow> = Vec::new();
    macro_rules! vec_slot {
        ($field:ident) => {
            rows.push(availability_row(
                ctx,
                stringify!($field),
                !ctx.$field.is_empty(),
            ));
        };
    }
    macro_rules! opt_slot {
        ($field:ident) => {
            rows.push(availability_row(
                ctx,
                stringify!($field),
                ctx.$field.is_some(),
            ));
        };
    }
    macro_rules! map_slot {
        ($field:ident) => {
            rows.push(availability_row(
                ctx,
                stringify!($field),
                !ctx.$field.is_empty(),
            ));
        };
    }

    vec_slot!(data_freshness);
    opt_slot!(synthesis);
    opt_slot!(regime);
    vec_slot!(analyst_convergence);
    vec_slot!(scenario_deltas);
    vec_slot!(news_catalysts);
    vec_slot!(market_snapshot);
    vec_slot!(macro_indicators);
    vec_slot!(economic_calendar);
    vec_slot!(macro_analyst_views);
    vec_slot!(macro_news_volume);
    opt_slot!(bitcoin_market);
    vec_slot!(bitcoin_etf_flows);
    vec_slot!(bitcoin_onchain);
    vec_slot!(bitcoin_analyst_views);
    vec_slot!(bitcoin_news);
    vec_slot!(bitcoin_prediction_signals);
    vec_slot!(precious_metals_market);
    vec_slot!(precious_metals_supply);
    vec_slot!(precious_metals_analyst_views);
    vec_slot!(precious_metals_news);
    opt_slot!(real_yield_context);
    opt_slot!(real_rates_snapshot);
    vec_slot!(sovereign_gold_holdings);
    vec_slot!(equity_indices);
    vec_slot!(equity_sectors);
    opt_slot!(equity_breadth);
    opt_slot!(equity_earnings);
    vec_slot!(equity_analyst_views);
    vec_slot!(equity_news);
    vec_slot!(public_news_events);
    vec_slot!(public_news_silence);
    vec_slot!(public_scenarios);
    vec_slot!(public_calibration);
    vec_slot!(private_calibration);
    vec_slot!(public_lessons_applied);
    vec_slot!(public_prediction_intelligence);
    vec_slot!(public_source_tier_overrides);
    opt_slot!(private_portfolio_snapshot);
    vec_slot!(private_derived_actions);
    vec_slot!(private_binary_catalysts);
    vec_slot!(private_what_changed_deltas);
    vec_slot!(private_positions);
    vec_slot!(private_drift_rows);
    opt_slot!(private_macro_regime);
    vec_slot!(private_macro_scenarios);
    vec_slot!(private_macro_divergences);
    vec_slot!(private_macro_catalysts);
    vec_slot!(private_thesis_chains);
    vec_slot!(private_asset_convergence);
    vec_slot!(private_conviction_trajectories);
    vec_slot!(private_outlooks);
    vec_slot!(private_risk_factor_mappings);
    vec_slot!(private_journal_views);
    vec_slot!(private_news_events);
    vec_slot!(private_news_silence);
    vec_slot!(private_open_predictions);
    opt_slot!(private_open_predictions_calibration);
    opt_slot!(private_lessons_applied);
    opt_slot!(private_regime_conditional);
    opt_slot!(recommendation_accuracy_7d);
    vec_slot!(synthesis_adversary_views);
    opt_slot!(todays_analyst_synthesis);
    vec_slot!(parallels_results);
    vec_slot!(cross_layer_signals);
    vec_slot!(investor_panel);
    vec_slot!(investor_panel_consensus);
    vec_slot!(portfolio_decision_cards);
    map_slot!(private_asset_intelligence);
    opt_slot!(morning_brief);
    rows.push(availability_row(
        ctx,
        "synthesis_notes",
        ctx.synthesis_notes.has_content(),
    ));
    opt_slot!(epistemic_health);
    vec_slot!(recommendation_scoreboard);

    rows
}

/// Conformance core: every struct field must be either a tracked slot or a
/// declared META field, every tracked slot must still exist on the struct,
/// and nothing may be tracked twice. Returns a human-actionable error
/// message on the first violation. Unit-tested with a fictional untracked
/// slot so the conformance test itself stays honest.
pub fn check_slot_conformance(
    struct_fields: &[String],
    tracked: &[&str],
    meta: &[&str],
) -> std::result::Result<(), String> {
    use std::collections::BTreeSet;
    let tracked_set: BTreeSet<&str> = tracked.iter().copied().collect();
    if tracked_set.len() != tracked.len() {
        let mut seen = BTreeSet::new();
        for t in tracked {
            if !seen.insert(t) {
                return Err(format!("slot `{t}` is tracked twice in data_availability()"));
            }
        }
    }
    let field_set: BTreeSet<&str> = struct_fields.iter().map(|s| s.as_str()).collect();
    for field in struct_fields {
        if meta.contains(&field.as_str()) {
            continue;
        }
        if !tracked_set.contains(field.as_str()) {
            return Err(format!(
                "data slot `{field}` was added to the report build context without \
                 availability tracking. Every data-bearing field must have a \
                 vec_slot!/opt_slot! row in data_availability() (so the dry-run \
                 audit and the integrity footer can report it), and its loader \
                 must record SlotIssue on failure. If it is metadata, add it to \
                 BUILD_CONTEXT_META_FIELDS instead. Do NOT weaken this test."
            ));
        }
    }
    for t in tracked {
        if !field_set.contains(t) {
            return Err(format!(
                "data_availability() tracks `{t}` but no such field exists on the \
                 report build context — remove the row or fix the rename."
            ));
        }
    }
    Ok(())
}

/// Per-section render accounting: did the section produce content, and if
/// not, which empty-state condition fired. The composition step may still
/// drop sections after assembly — this records what the ASSEMBLER produced
/// vs auto-suppressed, so a silently-missing section is always explainable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionOutcome {
    pub name: &'static str,
    pub visibility: SectionVisibility,
    pub rendered: bool,
    /// The renderer-stated suppression reason when not rendered.
    pub suppression_reason: Option<String>,
}

/// What the assembler will do without doing it.
#[derive(Debug, Clone)]
pub struct DryRunSummary {
    pub mode: BuildMode,
    pub report_date: String,
    pub plan: Vec<SectionSpec>,
    pub data_availability: Vec<DataAvailabilityRow>,
    pub section_outcomes: Vec<SectionOutcome>,
    pub staleness: Vec<StalenessWarning>,
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
            let outcome = self
                .section_outcomes
                .iter()
                .find(|o| o.name == spec.name);
            let note = match outcome {
                Some(o) if !o.rendered => format!(
                    "  [suppressed: {}]",
                    o.suppression_reason.as_deref().unwrap_or("no reason given")
                ),
                _ => String::new(),
            };
            out.push_str(&format!(
                "  {:>2}. [{}] {}{}\n",
                idx + 1,
                match spec.visibility {
                    SectionVisibility::Public => "pub",
                    SectionVisibility::Private => "prv",
                },
                spec.name,
                note
            ));
        }
        out.push_str("\nData availability:\n");
        for row in &self.data_availability {
            let detail = match (&row.status, row.reason.as_deref()) {
                (SlotStatus::Populated, _) => "populated".to_string(),
                (status, Some(reason)) => format!("{} — {}", status.as_str(), reason),
                (status, None) => status.as_str().to_string(),
            };
            out.push_str(&format!("  - {:<40} {}\n", row.field, detail));
        }
        if !self.staleness.is_empty() {
            out.push_str("\nStaleness warnings:\n");
            for w in &self.staleness {
                out.push_str(&format!(
                    "  - {} → {} (annotates: {})\n",
                    w.input,
                    w.message,
                    w.sections.join(", ")
                ));
            }
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

/// Extract the suppression reason from a section body that consists solely
/// of a `<!-- suppressed: … -->` marker (the suppression-reason channel each
/// renderer's empty-state return uses — see `sections::suppressed`).
pub fn extract_suppression_reason(body: &str) -> Option<String> {
    let trimmed = body.trim();
    let inner = trimmed
        .strip_prefix(crate::report::sections::SUPPRESSED_PREFIX)?
        .strip_suffix("-->")?;
    Some(inner.trim().to_string())
}

/// Insert staleness annotations (as `> ⚠ …` blockquotes) after the first
/// line of a rendered section body. The first line is the `## Heading`, so
/// the warning lands directly under the section title.
fn inject_staleness_annotations(body: &str, warnings: &[&StalenessWarning]) -> String {
    if warnings.is_empty() {
        return body.to_string();
    }
    let mut lines = body.lines();
    let first = lines.next().unwrap_or_default();
    let rest: Vec<&str> = lines.collect();
    let mut out = String::from(first);
    for w in warnings {
        out.push_str("\n\n> ");
        out.push_str(&w.message);
    }
    if !rest.is_empty() {
        out.push('\n');
        out.push_str(&rest.join("\n"));
    }
    out
}

/// Concatenate sections in `plan` order, separating with a blank line, and
/// account for every section: rendered vs auto-suppressed (with the
/// renderer-stated empty-state reason). Suppressed sections leave no
/// dangling blank lines. Staleness warnings targeting a rendered section
/// are injected directly under its heading. The assembled markdown is
/// scanned for leaked internal/debug text; findings are logged but
/// non-fatal so report generation is never blocked.
pub fn assemble_markdown_accounted(
    ctx: &BuildContext,
    plan: &[SectionSpec],
) -> Result<(String, Vec<SectionOutcome>)> {
    assemble_markdown_with_override(ctx, plan, |_| None)
}

/// Core assembly: like [`assemble_markdown_accounted`] but allows one
/// caller-supplied override body per section (used by the persist path to
/// swap in the rec-id-annotated Decisions Pending render).
fn assemble_markdown_with_override(
    ctx: &BuildContext,
    plan: &[SectionSpec],
    override_body: impl Fn(&str) -> Option<String>,
) -> Result<(String, Vec<SectionOutcome>)> {
    let mut parts = Vec::with_capacity(plan.len());
    let mut outcomes = Vec::with_capacity(plan.len());
    for spec in plan {
        let body = match override_body(spec.name) {
            Some(b) => b,
            None => render_section(spec.name, ctx)
                .with_context(|| format!("failed to render section {}", spec.name))?,
        };
        if let Some(reason) = extract_suppression_reason(&body) {
            outcomes.push(SectionOutcome {
                name: spec.name,
                visibility: spec.visibility,
                rendered: false,
                suppression_reason: Some(reason),
            });
            continue;
        }
        if body.trim().is_empty() {
            outcomes.push(SectionOutcome {
                name: spec.name,
                visibility: spec.visibility,
                rendered: false,
                suppression_reason: Some(
                    "renderer returned an empty body without a stated reason".to_string(),
                ),
            });
            continue;
        }
        let warnings: Vec<&StalenessWarning> = ctx
            .staleness
            .iter()
            .filter(|w| w.sections.contains(&spec.name))
            .collect();
        parts.push(inject_staleness_annotations(&body, &warnings));
        outcomes.push(SectionOutcome {
            name: spec.name,
            visibility: spec.visibility,
            rendered: true,
            suppression_reason: None,
        });
    }
    let markdown = parts.join("\n\n");
    warn_on_leaks(&markdown);
    Ok((markdown, outcomes))
}

/// Backwards-compatible body-only assembly (no accounting).
pub fn assemble_markdown(ctx: &BuildContext, plan: &[SectionSpec]) -> Result<String> {
    assemble_markdown_accounted(ctx, plan).map(|(body, _)| body)
}

/// Marker comment that opens the integrity footer. The composition step
/// edits the report ABOVE this marker and must never remove the block.
pub const INTEGRITY_FOOTER_MARKER: &str = "<!-- integrity-footer: do not remove -->";

/// Render the unconditional integrity footer appended to the PRIVATE
/// report: slot accounting (populated / no-data / upstream-not-run /
/// LOADER ERRORS in bold with error text), section render-vs-suppression
/// accounting, and stale-input notes. When everything is populated the
/// slot line collapses to one quiet sentence.
pub fn render_integrity_footer(
    availability: &[DataAvailabilityRow],
    outcomes: &[SectionOutcome],
    staleness: &[StalenessWarning],
) -> String {
    let total = availability.len();
    let populated = availability.iter().filter(|r| r.populated).count();
    let no_data: Vec<&str> = availability
        .iter()
        .filter(|r| !r.populated && r.status == SlotStatus::NoData)
        .map(|r| r.field)
        .collect();
    let upstream: Vec<&str> = availability
        .iter()
        .filter(|r| r.status == SlotStatus::UpstreamNotRun)
        .map(|r| r.field)
        .collect();
    let errors: Vec<(&str, String)> = availability
        .iter()
        .filter(|r| r.status == SlotStatus::LoaderError)
        .map(|r| {
            (
                r.field,
                r.reason.clone().unwrap_or_else(|| "unknown error".to_string()),
            )
        })
        .collect();

    let mut out = String::from("---\n\n");
    out.push_str(INTEGRITY_FOOTER_MARKER);
    out.push('\n');

    if populated == total {
        out.push_str(&format!("*Report integrity: all {total} slots populated.*"));
    } else {
        let mut line = format!("*Report integrity: {populated}/{total} slots populated.");
        if !no_data.is_empty() {
            line.push_str(&format!(" No data: {}.", no_data.join(", ")));
        }
        if !upstream.is_empty() {
            line.push_str(&format!(" Upstream not run: {}.", upstream.join(", ")));
        }
        line.push('*');
        out.push_str(&line);
        if !errors.is_empty() {
            let rendered: Vec<String> = errors
                .iter()
                .map(|(field, err)| format!("**{field}: {err}**"))
                .collect();
            out.push_str(&format!(" **LOADER ERRORS:** {}.", rendered.join("; ")));
        }
    }

    let suppressed: Vec<&SectionOutcome> = outcomes.iter().filter(|o| !o.rendered).collect();
    let rendered_count = outcomes.len() - suppressed.len();
    if !suppressed.is_empty() {
        let details: Vec<String> = suppressed
            .iter()
            .map(|o| {
                format!(
                    "{} ({})",
                    o.name,
                    o.suppression_reason.as_deref().unwrap_or("no reason given")
                )
            })
            .collect();
        out.push_str(&format!(
            "\n*Sections: {} rendered, {} auto-suppressed — {}.*",
            rendered_count,
            suppressed.len(),
            details.join("; ")
        ));
    }
    if !staleness.is_empty() {
        let inputs: Vec<String> = staleness
            .iter()
            .map(|w| format!("{} ({})", w.input, w.message.trim_start_matches("⚠ ").trim_start_matches('⚠').trim()))
            .collect();
        out.push_str(&format!("\n*Stale inputs: {}.*", inputs.join("; ")));
    }
    out
}

/// Log a warning for any leaked internal/debug text found in assembled
/// markdown. Non-fatal: surfacing it in CI (via the regression test) and on
/// stderr is enough; we never want to fail an operator's report build over a
/// cosmetic leak.
fn warn_on_leaks(markdown: &str) {
    let findings = crate::report::lint::scan_for_leaks(markdown);
    for f in &findings {
        eprintln!(
            "report-lint: leaked internal text on line {} (matched '{}'): {}",
            f.line, f.pattern, f.excerpt
        );
    }
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

/// Append the unconditional integrity footer to an assembled private body.
fn with_integrity_footer(ctx: &BuildContext, body: String, outcomes: &[SectionOutcome]) -> String {
    let footer = render_integrity_footer(&data_availability(ctx), outcomes, &ctx.staleness);
    if body.trim().is_empty() {
        footer
    } else {
        format!("{body}\n\n{footer}")
    }
}

/// Assemble the private markdown (uses private section plan only — the public
/// analytical core is intentionally not duplicated into the private file;
/// `--mode both` produces TWO separate documents, one per destination).
/// Unconditionally appends the integrity footer AFTER the last section so
/// the composition step edits above it.
pub fn assemble_private(ctx: &BuildContext) -> Result<String> {
    let plan = private_section_plan();
    let (body, outcomes) = assemble_markdown_accounted(ctx, &plan)?;
    Ok(with_integrity_footer(ctx, body, &outcomes))
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
    let (body, outcomes) = assemble_markdown_with_override(ctx, &plan, |name| {
        (name == "private_decisions_pending").then(|| {
            crate::report::sections::private_decisions_pending::render_private_decisions_pending_with_cards(&annotated)
        })
    })?;
    Ok(with_integrity_footer(ctx, body, &outcomes))
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

    // Section accounting for the dry run: render every planned section and
    // record rendered vs auto-suppressed (with the empty-state reason).
    let section_outcomes = assemble_markdown_accounted(ctx, &plan.sections)
        .map(|(_, outcomes)| outcomes)
        .unwrap_or_default();

    DryRunSummary {
        mode,
        report_date: date.to_string(),
        plan: plan.sections,
        data_availability: data_availability(ctx),
        section_outcomes,
        staleness: ctx.staleness.clone(),
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

/// Read `/tmp/pftui-parallels-<date>.json` and decode it into the compact
/// `ParallelsResult` shape. Returns an empty Vec when the file is missing
/// or malformed — the assembler must never abort because the parallels
/// runner did not execute (it's a best-effort enrichment pass run from
/// the report skill, not a hard dependency).
pub fn load_parallels_results(report_date: &str) -> Vec<ParallelsResult> {
    load_parallels_results_classified(report_date).0
}

/// Like [`load_parallels_results`] but classifies WHY the result is empty:
/// a missing file means the Step 4.5 parallels runner did not run for this
/// date (`UpstreamNotRun`); an unreadable/malformed file is a `LoaderError`;
/// a well-formed file with zero matching sets is genuine `NoData`.
pub fn load_parallels_results_classified(
    report_date: &str,
) -> (Vec<ParallelsResult>, Option<SlotIssue>) {
    let path = format!("/tmp/pftui-parallels-{report_date}.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                Vec::new(),
                Some(SlotIssue::UpstreamNotRun(format!(
                    "{path} not found — parallels catalog runner (Step 4.5) did not run for this date"
                ))),
            );
        }
        Err(e) => {
            return (
                Vec::new(),
                Some(SlotIssue::LoaderError(format!("failed to read {path}: {e}"))),
            );
        }
    };
    if serde_json::from_str::<serde_json::Value>(&raw).is_err() {
        return (
            Vec::new(),
            Some(SlotIssue::LoaderError(format!(
                "{path} exists but is not valid JSON"
            ))),
        );
    }
    let results = parse_parallels_json(&raw);
    let issue = results.is_empty().then(|| {
        SlotIssue::NoData(format!(
            "{path} parsed but contained no matching parallel sets"
        ))
    });
    (results, issue)
}

/// Parse the parallels bundle JSON into the compact `ParallelsResult` shape.
/// Defensive: every field is optional and missing keys default to empty/
/// `None` so a partial run still surfaces what landed.
fn parse_parallels_json(raw: &str) -> Vec<ParallelsResult> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(results) = value.get("results").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    results
        .iter()
        .map(|r| {
            // The engine emits a nested `forward_distributions_pct` whose
            // values are objects (`{ n, median, mean, p25, p75, hit_rate_up }`)
            // not bare numbers. The original parser read raw f64s, so every
            // horizon collapsed to None and the table rendered "—" in every
            // cell despite N>0 matches — repro of the 2026-06-05 weekly run.
            let horizon_median = |key: &str, legacy: &str| -> Option<f64> {
                r.get("forward_distributions_pct")
                    .and_then(|fr| fr.get(key))
                    .and_then(|h| h.get("median"))
                    .and_then(|v| v.as_f64())
                    // Backwards-compat: older bundle shapes used the bare
                    // number under `forward_returns` or at the row level
                    // (`median_30d_pct`).
                    .or_else(|| {
                        r.get("forward_returns")
                            .and_then(|fr| fr.get(key))
                            .and_then(|v| v.as_f64())
                    })
                    .or_else(|| r.get(legacy).and_then(|v| v.as_f64()))
            };
            let horizon_hit = |key: &str, legacy: &str| -> Option<f64> {
                r.get("forward_distributions_pct")
                    .and_then(|fr| fr.get(key))
                    .and_then(|h| h.get("hit_rate_up"))
                    .and_then(|v| v.as_f64())
                    .or_else(|| r.get(legacy).and_then(|v| v.as_f64()))
            };
            ParallelsResult {
                id: r
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                // The engine writes the human-readable set label as
                // `condition_set_name`. Keep `name` as a fallback for
                // legacy bundle shapes.
                name: r
                    .get("condition_set_name")
                    .or_else(|| r.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                symbol: r
                    .get("symbol")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                narrative: r
                    .get("narrative")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                match_count: r
                    .get("match_count")
                    .or_else(|| r.get("n_matches"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                median_5d_pct: horizon_median("5d", "median_5d_pct"),
                median_30d_pct: horizon_median("30d", "median_30d_pct"),
                median_90d_pct: horizon_median("90d", "median_90d_pct"),
                median_180d_pct: horizon_median("180d", "median_180d_pct"),
                hit_rate_30d_pct: horizon_hit("30d", "hit_rate_30d_pct"),
                hit_rate_90d_pct: horizon_hit("90d", "hit_rate_90d_pct"),
                error: r
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            }
        })
        .collect()
}

/// Pull `agent_messages` rows for the report date whose `to_agent` is
/// `synthesis` and whose `priority` is `high` or `normal`. Lower
/// priorities are intentionally dropped — the cross-layer signal table
/// is meant to be scannable, not exhaustive.
fn load_cross_layer_signals(
    backend: &BackendConnection,
    report_date: &str,
) -> Result<Vec<CrossLayerSignal>> {
    // agent_messages.created_at is stored with a space separator
    // ("YYYY-MM-DD HH:MM:SS"), so the since-bound must use a space too —
    // a 'T' separator sorts lexically AFTER the space and silently drops
    // every same-day row, leaving the Cross-Layer Signals section empty.
    // The starts_with(report_date) filter below is the real date bound;
    // this just scopes the query.
    let since = format!("{report_date} 00:00:00");
    let messages = crate::db::agent_messages::list_messages_backend(
        backend,
        None,
        Some("synthesis"),
        None,
        false,
        Some(&since),
        None,
        Some(100),
    )?;
    Ok(messages
        .into_iter()
        .filter(|m| {
            let p = m.priority.to_ascii_lowercase();
            p == "high" || p == "normal"
        })
        .filter(|m| m.created_at.starts_with(report_date))
        // Strip messages that belong in their own dedicated sections OR
        // are raw JSON envelopes that would dump as a wall of text. Three
        // gates: (1) category='decision-card' = decision-architect cards;
        // (2) from_agent='analyst-decisions' or starts with 'panel-' (the
        // 2026-06-07 run wrote decision cards with category='signal' so the
        // category gate alone was insufficient); (3) content that starts
        // with '{' is a JSON payload that doesn't belong in a human-
        // readable signals table regardless of who wrote it.
        .filter(|m| {
            let cat = m.category.as_deref().unwrap_or("");
            if cat == "decision-card" {
                return false;
            }
            if m.from_agent == "analyst-decisions" || m.from_agent.starts_with("panel-") {
                return false;
            }
            !m.content.trim_start().starts_with('{')
        })
        .map(|m| CrossLayerSignal {
            from_layer: m.from_agent,
            to_layer: m.to_agent.unwrap_or_else(|| "synthesis".to_string()),
            priority: m.priority,
            category: m.category.unwrap_or_default(),
            summary: first_sentence(&m.content).replace('|', "/"),
        })
        .collect())
}

/// Find the most recent `created_at` DATE strictly before `report_date` among
/// synthesis-bound agent messages whose `from_agent` matches `from_filter`.
/// Used to distinguish `upstream_not_run` (the writing phase ran on earlier
/// days but not today) from genuine `no_data` (never ran at all).
/// Best-effort: returns None on any query error.
fn latest_agent_message_date_before(
    backend: &BackendConnection,
    report_date: &str,
    from_filter: impl Fn(&str) -> bool,
) -> Option<String> {
    let messages = crate::db::agent_messages::list_messages_backend(
        backend,
        None,
        Some("synthesis"),
        None,
        false,
        None,
        None,
        Some(500),
    )
    .ok()?;
    messages
        .iter()
        .filter(|m| from_filter(&m.from_agent))
        .filter_map(|m| m.created_at.get(..10).map(|d| d.to_string()))
        .filter(|d| d.as_str() < report_date)
        .max()
}

/// Find the most recent `daily_notes` date strictly before `report_date`
/// carrying an `analyst-synthesis` note with a recognised `[synthesis-…]`
/// header. Best-effort: returns None on any query error.
fn latest_synthesis_note_date_before(
    conn: &rusqlite::Connection,
    report_date: &str,
) -> Option<String> {
    let notes = crate::db::daily_notes::list_notes(
        conn,
        None,
        None,
        Some(500),
        Some("analyst-synthesis"),
    )
    .ok()?;
    notes
        .iter()
        .filter(|n| parse_synthesis_header(&n.content).is_some())
        .map(|n| n.date.clone())
        .filter(|d| d.as_str() < report_date)
        .max()
}

/// Load the investor-panel persona responses for the report date by
/// reading `agent_messages` rows where `from_agent` starts with `panel-`
/// and parsing the JSON content per the panel schema. Rows that don't
/// parse as valid panel responses are silently dropped — degraded
/// rather than aborting so a single malformed persona response doesn't
/// blank the entire section.
fn load_investor_panel_responses(
    backend: &BackendConnection,
    report_date: &str,
) -> Result<Vec<InvestorPanelResponse>> {
    let since = format!("{report_date} 00:00:00");
    let messages = crate::db::agent_messages::list_messages_backend(
        backend,
        None,
        Some("synthesis"),
        None,
        false,
        Some(&since),
        None,
        Some(64),
    )?;
    Ok(messages
        .into_iter()
        .filter(|m| m.from_agent.starts_with("panel-"))
        .filter(|m| m.created_at.starts_with(report_date))
        .filter_map(|m| parse_panel_response(&m.content))
        .collect())
}

fn parse_panel_response(raw: &str) -> Option<InvestorPanelResponse> {
    let trimmed = raw.trim();
    // Try strict JSON first.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(parsed) = parse_panel_response_json(&value) {
            return Some(parsed);
        }
    }
    // Fallback: persona subagents that returned prose instead of JSON
    // (the 2026-06-07 run produced "Buffett [neutral, conf62]: cash OW
    // …, gold NEUTRAL/UW …" rather than the strict schema). Parse the
    // prose with a tolerant best-effort scanner so the panel section
    // populates instead of saying "no responses landed".
    parse_panel_response_prose(trimmed)
}

fn parse_panel_response_json(value: &serde_json::Value) -> Option<InvestorPanelResponse> {
    let investor = value.get("investor")?.as_str()?.to_string();
    let overall_signal = value
        .get("overall_signal")
        .and_then(|v| v.as_str())
        .unwrap_or("neutral")
        .to_string();
    let confidence = value
        .get("confidence")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(100) as u8;
    let key_insight = value
        .get("key_insight")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let what_would_change_my_mind = value
        .get("what_would_change_my_mind")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut positioning = Vec::new();
    if let Some(map) = value.get("positioning").and_then(|v| v.as_object()) {
        for (asset, entry) in map {
            let signal = entry
                .get("signal")
                .and_then(|v| v.as_str())
                .unwrap_or("neutral")
                .to_string();
            let weight = entry
                .get("weight")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let reasoning = entry
                .get("reasoning")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            positioning.push(InvestorPanelPositioning {
                asset: asset.clone(),
                signal,
                weight,
                reasoning,
            });
        }
    }
    positioning.sort_by(|a, b| a.asset.cmp(&b.asset));
    Some(InvestorPanelResponse {
        investor,
        overall_signal,
        confidence,
        positioning,
        key_insight,
        what_would_change_my_mind,
    })
}

/// Tolerant prose-mode parser for persona responses that didn't honor the
/// JSON schema. Expected shape (e.g. 2026-06-07 weekly):
///   "Buffett [neutral, conf62]: cash OW (paid to wait, positive real
///    yield), gold NEUTRAL/UW (produces nothing…), BTC NEUTRAL/AVOID
///    (rat poison squared…), equities neutral/tactical (want productive
///    businesses…), oil neutral. <key insight prose>. <change-mind prose>."
fn parse_panel_response_prose(raw: &str) -> Option<InvestorPanelResponse> {
    // The first token before '[' is the persona name; the bracket block
    // carries the overall signal + confidence; per-asset signals come as
    // "<ASSET> <SIGNAL>/<WEIGHT> (<reasoning>)" segments.
    let header_split = raw.find('[')?;
    let investor = raw[..header_split].trim().trim_end_matches(':').to_string();
    if investor.is_empty() {
        return None;
    }
    let after_header = &raw[header_split..];
    let bracket_end = after_header.find(']')?;
    let bracket = &after_header[1..bracket_end];
    let after_bracket = after_header[bracket_end + 1..].trim_start_matches(':').trim();

    let mut overall_signal = "neutral".to_string();
    let mut confidence: u8 = 0;
    for piece in bracket.split(',') {
        let t = piece.trim().to_lowercase();
        if t.starts_with("bull") {
            overall_signal = "bullish".to_string();
        } else if t.starts_with("bear") {
            overall_signal = "bearish".to_string();
        } else if t.starts_with("neutral") {
            overall_signal = "neutral".to_string();
        } else if let Some(rest) = t.strip_prefix("conf") {
            confidence = rest.trim().parse::<u8>().unwrap_or(0).min(100);
        }
    }

    // Split the prose into the asset list (before the first ". " that
    // closes a paren-block) and the trailing prose (key insight +
    // what-would-change). Best-effort — preserve the full body in the
    // key_insight if we can't separate.
    let positioning = scan_prose_positioning(after_bracket);
    let key_insight = first_sentence_after_assets(after_bracket).to_string();
    let what_would_change_my_mind = trailing_change_clause(after_bracket).to_string();

    Some(InvestorPanelResponse {
        investor,
        overall_signal,
        confidence,
        positioning,
        key_insight,
        what_would_change_my_mind,
    })
}

fn scan_prose_positioning(body: &str) -> Vec<InvestorPanelPositioning> {
    // Recognise comma-separated segments of the form
    //   "<asset> <signal>/<weight> (reasoning)"
    // or "<asset> <signal>" (no weight, no reasoning). Asset names we
    // accept: cash, gold, btc, equities, oil (canonical panel buckets).
    const ASSET_KEYS: &[&str] = &["cash", "gold", "btc", "equities", "oil"];
    let lower = body.to_lowercase();
    let mut out: Vec<InvestorPanelPositioning> = Vec::new();
    for key in ASSET_KEYS {
        let Some(idx) = lower.find(key) else { continue };
        let after = &body[idx + key.len()..];
        let head = after.trim_start();
        // Read up to the next comma or period to get "<signal>/<weight>"
        let end = head
            .find([',', '.'])
            .unwrap_or_else(|| head.len().min(120));
        let segment = head[..end].trim();
        let mut signal = "neutral".to_string();
        let mut weight = String::new();
        let lower_seg = segment.to_lowercase();
        if lower_seg.contains("bullish") || lower_seg.contains("bull") {
            signal = "bullish".to_string();
        } else if lower_seg.contains("bearish") || lower_seg.contains("bear") {
            signal = "bearish".to_string();
        }
        for piece in segment.split('/') {
            let t = piece.trim().to_lowercase();
            if t.contains("over") {
                weight = "overweight".to_string();
            } else if t.contains("under") || t == "uw" {
                weight = "underweight".to_string();
            } else if t == "ow" {
                weight = "overweight".to_string();
            } else if t.contains("avoid") {
                weight = "zero".to_string();
            } else if t.contains("tactical") {
                weight = "tactical".to_string();
            }
        }
        // The reasoning is the parenthetical clause immediately after,
        // if any.
        let reasoning = head[end..]
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')').map(|(inner, _)| inner.trim().to_string()))
            .unwrap_or_default();
        out.push(InvestorPanelPositioning {
            asset: (*key).to_string(),
            signal,
            weight,
            reasoning,
        });
    }
    out
}

fn first_sentence_after_assets(body: &str) -> &str {
    // Crude: take the last sentence-ending segment as key_insight.
    body.rsplit_once(". ")
        .map(|(prefix, _)| prefix.rsplit_once(". ").map(|(_, tail)| tail).unwrap_or(prefix))
        .unwrap_or(body)
        .trim()
}

fn trailing_change_clause(body: &str) -> &str {
    body.rsplit_once(". ")
        .map(|(_, tail)| tail.trim_end_matches('.'))
        .unwrap_or("")
        .trim()
}

/// Aggregate per-asset bullish/bearish/neutral vote tally across the
/// panel responses. Labels mirror the panel skill's consensus rules:
/// "strong-consensus" when ≥75% of voters agree, "high-divergence"
/// when bullish/bearish counts are within one vote, "mixed" otherwise.
fn aggregate_panel_consensus(
    responses: &[InvestorPanelResponse],
) -> Vec<InvestorPanelConsensus> {
    use std::collections::BTreeMap;
    let mut tally: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
    for r in responses {
        for p in &r.positioning {
            let entry = tally.entry(p.asset.clone()).or_insert((0, 0, 0));
            match p.signal.to_ascii_lowercase().as_str() {
                "bullish" => entry.0 += 1,
                "bearish" => entry.1 += 1,
                _ => entry.2 += 1,
            }
        }
    }
    tally
        .into_iter()
        .map(|(asset, (b, r, n))| {
            let total = b + r + n;
            let label = if total == 0 {
                "no-votes".to_string()
            } else if b as f64 / total as f64 >= 0.75 {
                "strong-consensus-bullish".to_string()
            } else if r as f64 / total as f64 >= 0.75 {
                "strong-consensus-bearish".to_string()
            } else if (b as i32 - r as i32).abs() <= 1 && b + r > 0 {
                "high-divergence".to_string()
            } else if b > r {
                "lean-bullish".to_string()
            } else if r > b {
                "lean-bearish".to_string()
            } else {
                "mixed".to_string()
            };
            InvestorPanelConsensus {
                asset,
                bullish_count: b,
                bearish_count: r,
                neutral_count: n,
                label,
            }
        })
        .collect()
}

/// Load portfolio decision cards for the report date by reading
/// `agent_messages` rows where `from_agent='analyst-decisions'` and
/// `category='decision-card'`, parsed from JSON. Rows that don't parse
/// are silently dropped.
fn load_portfolio_decision_cards(
    backend: &BackendConnection,
    report_date: &str,
) -> Result<Vec<PortfolioDecisionCard>> {
    let since = format!("{report_date} 00:00:00");
    let messages = crate::db::agent_messages::list_messages_backend(
        backend,
        Some("analyst-decisions"),
        Some("synthesis"),
        None,
        false,
        Some(&since),
        None,
        Some(64),
    )?;
    Ok(messages
        .into_iter()
        .filter(|m| m.created_at.starts_with(report_date))
        .filter(|m| m.category.as_deref() == Some("decision-card"))
        .filter_map(|m| parse_decision_card(&m.content))
        .collect())
}

fn parse_decision_card(raw: &str) -> Option<PortfolioDecisionCard> {
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let symbol = value.get("symbol")?.as_str()?.to_string();
    let question = value
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let recommendation = value
        .get("recommendation")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let what_would_change_it = value
        .get("what_would_change_it")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let sizing_math = value
        .get("sizing_math")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let to_str_vec = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| i.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let evidence_for = value
        .get("evidence_for")
        .map(to_str_vec)
        .unwrap_or_default();
    let evidence_against = value
        .get("evidence_against")
        .map(to_str_vec)
        .unwrap_or_default();
    Some(PortfolioDecisionCard {
        symbol,
        question,
        evidence_for,
        evidence_against,
        recommendation,
        what_would_change_it,
        sizing_math,
    })
}

/// Load the per-asset synthesis digest for the report date from
/// `daily_notes` rows authored by `analyst-synthesis`. Each note's content
/// opens with a `[synthesis-<KEY>]` header line, where KEY is either a held
/// symbol (e.g. `BTC`, `GC=F`) or the literal `economy`. The header line is
/// stripped and the remaining body retained verbatim (it carries the
/// BULL CASE / BEAR CASE / WHAT WOULD CHANGE MY MIND / RISK / REWARD
/// sub-sections). Notes without a recognised header are ignored so the
/// section never renders stray content.
fn load_synthesis_notes(
    conn: &rusqlite::Connection,
    report_date: &str,
) -> Result<SynthesisNotes> {
    let notes = crate::db::daily_notes::list_notes(
        conn,
        Some(report_date),
        None,
        None,
        Some("analyst-synthesis"),
    )?;
    let mut out = SynthesisNotes::default();
    for note in notes {
        let Some((key, body)) = parse_synthesis_header(&note.content) else {
            continue;
        };
        if key.eq_ignore_ascii_case("economy") {
            // First economy note wins; later duplicates are ignored.
            if out.economy.is_none() {
                out.economy = Some(body);
            }
        } else if key.to_ascii_lowercase().starts_with("deep-dive") {
            // First deep-dive wins. The header tag may carry a date suffix
            // (e.g. `[synthesis-deep-dive 2026-06-07]`) so match by prefix.
            if out.deep_dive.is_none() {
                out.deep_dive = Some(body);
            }
        } else if key.to_ascii_lowercase().starts_with("macro-outlook") {
            if out.macro_outlook.is_none() {
                out.macro_outlook = Some(body);
            }
        } else if key.to_ascii_lowercase().starts_with("closing") {
            if out.closing.is_none() {
                out.closing = Some(body);
            }
        } else if key.to_ascii_lowercase().starts_with("external-ta") {
            if out.external_ta.is_none() {
                out.external_ta = Some(body);
            }
        } else {
            out.assets.push(SynthesisAssetNote { symbol: key, body });
        }
    }
    Ok(out)
}

/// Parse a `[synthesis-<KEY>]` header from the first non-empty line of a
/// note's content. Returns `(KEY, body)` where body is everything after the
/// header line, trimmed. Returns `None` when the header is absent or empty.
fn parse_synthesis_header(content: &str) -> Option<(String, String)> {
    let trimmed = content.trim_start();
    let first_line_end = trimmed.find('\n').unwrap_or(trimmed.len());
    let first_line = trimmed[..first_line_end].trim();
    let inner = first_line
        .strip_prefix("[synthesis-")
        .and_then(|s| s.strip_suffix(']'))?;
    let key = inner.trim();
    if key.is_empty() {
        return None;
    }
    let body = trimmed[first_line_end..].trim().to_string();
    Some((key.to_string(), body))
}

/// Build a compact `AssetIntelligenceBlob` for a single held symbol,
/// drawing from the same per-asset substrate `pftui analytics asset`
/// surfaces. We intentionally avoid invoking `run_asset_intelligence`
/// directly because it prints to stdout; instead we re-query the
/// underlying tables and assemble the summary fields the renderer needs.
fn load_asset_intelligence_blob(
    backend: &BackendConnection,
    symbol: &str,
) -> Option<AssetIntelligenceBlob> {
    let sym = symbol.to_uppercase();
    let spot =
        crate::db::price_cache::get_cached_price_backend(backend, &sym, "USD")
            .ok()
            .flatten();

    let history = crate::db::price_history::get_history_backend(backend, &sym, 30)
        .unwrap_or_default();
    let daily_change_pct = if history.len() >= 2 {
        let prev = history[history.len() - 2].close;
        let curr = history[history.len() - 1].close;
        if prev > rust_decimal::Decimal::ZERO {
            Some(dec_to_f64(
                ((curr - prev) / prev * rust_decimal::Decimal::from(100)).round_dp(2),
            ))
        } else {
            None
        }
    } else {
        None
    };

    let snap = crate::db::technical_snapshots::get_latest_snapshot_backend(
        backend, &sym, "1d",
    )
    .ok()
    .flatten();

    let rsi_14 = snap.as_ref().and_then(|s| s.rsi_14);
    let rsi_signal = rsi_14.map(|r| {
        if r > 70.0 {
            "overbought".to_string()
        } else if r < 30.0 {
            "oversold".to_string()
        } else {
            "neutral".to_string()
        }
    });
    let trend = trend_signal(backend, &sym);

    let levels =
        crate::db::technical_levels::get_levels_for_symbol_backend(backend, &sym)
            .unwrap_or_default();
    let (nearest_support, nearest_resistance) = if let Some(price) = spot.as_ref().map(|q| q.price)
    {
        if let Ok(p) = price.to_string().parse::<f64>() {
            let pair = crate::analytics::levels::nearest_actionable_levels(&levels, p);
            (
                pair.support.map(|l| format!("${:.2}", l.price)),
                pair.resistance.map(|l| format!("${:.2}", l.price)),
            )
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let range_52w_position = snap.as_ref().and_then(|s| s.range_52w_position);

    let scenarios = crate::db::scenarios::list_scenarios_backend(backend, Some("active"))
        .unwrap_or_default();
    let scenario_count = scenarios
        .iter()
        .filter(|s| {
            let haystack = format!(
                "{} {} {} {}",
                s.name,
                s.asset_impact.as_deref().unwrap_or(""),
                s.description.as_deref().unwrap_or(""),
                s.triggers.as_deref().unwrap_or("")
            )
            .to_uppercase();
            haystack.contains(&sym)
        })
        .count() as u32;

    let open_predictions_count = backend
        .sqlite_native()
        .and_then(|conn| {
            crate::db::user_predictions::list_predictions(conn, Some("pending"), None, None, None)
                .ok()
        })
        .map(|preds| {
            preds
                .into_iter()
                .filter(|p| {
                    p.symbol
                        .as_deref()
                        .map(|s| s.eq_ignore_ascii_case(&sym))
                        .unwrap_or(false)
                })
                .count() as u32
        })
        .unwrap_or(0);

    let structural_context = snap.as_ref().and_then(|s| {
        match (s.above_sma_50, s.above_sma_200) {
            (Some(true), Some(true)) => Some("Above both 50/200 SMA — structural uptrend".to_string()),
            (Some(false), Some(false)) => Some("Below both 50/200 SMA — structural downtrend".to_string()),
            (Some(true), Some(false)) => Some("Above 50 SMA, below 200 SMA — mixed".to_string()),
            (Some(false), Some(true)) => Some("Below 50 SMA, above 200 SMA — fragile".to_string()),
            _ => None,
        }
    });

    if spot.is_none()
        && snap.is_none()
        && levels.is_empty()
        && scenario_count == 0
        && open_predictions_count == 0
    {
        return None;
    }

    // Price-action structure verdicts (daily + weekly), the cycle-clock
    // position line (cycle assets), and the composite Cyber Dots verdict.
    // All auto-skip (None) when the underlying history is too shallow for
    // an honest read.
    let (structure_verdict_daily, structure_verdict_weekly, cycle_clock_verdict, cyber_verdict_daily) =
        load_structure_and_cycle_verdicts(backend, &sym);

    let signal_expectancy = load_signal_expectancy_line(backend, &sym);

    Some(AssetIntelligenceBlob {
        symbol: sym,
        spot_price: spot.as_ref().map(|q| format_price(q.price)),
        daily_change_pct,
        rsi_14,
        rsi_signal,
        trend,
        nearest_support,
        nearest_resistance,
        range_52w_position,
        scenario_count,
        open_predictions_count,
        structural_context,
        structure_verdict_daily,
        structure_verdict_weekly,
        cycle_clock_verdict,
        cyber_verdict_daily,
        signal_expectancy,
    })
}

/// "Signal expectancy" line for the per-asset card: cite the persisted 90d
/// event-study stats (vs baseline) for any registry signal that FIRED for
/// this asset within the last 10 days of its history. Auto-skips (None)
/// when nothing fired recently, history is shallow, or no stats are
/// persisted (the expectancy table is L2 — rebuilt by `pftui research
/// backtest`). Citations are lookahead-free: stats carry their own as_of.
fn load_signal_expectancy_line(backend: &BackendConnection, sym: &str) -> Option<String> {
    use crate::research::registry::{self, AssetContext, SignalEmitter};

    let conn = backend.sqlite_native()?;
    let (series, history) =
        crate::commands::research_harness::load_deep_history_full(backend, sym).ok()?;
    if history.len() < 250 {
        return None;
    }
    let last_date =
        chrono::NaiveDate::parse_from_str(&history.last()?.date, "%Y-%m-%d").ok()?;
    let cutoff = (last_date - chrono::Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let ctx = AssetContext::build(sym, &series, &history)?;
    let persisted =
        crate::db::signal_expectancy::latest_rows(conn, None, Some(&ctx.series)).ok()?;
    if persisted.is_empty() {
        return None;
    }

    let fmt = |v: Option<f64>| {
        v.map(|x| format!("{x:+.1}%"))
            .unwrap_or_else(|| "n/a".to_string())
    };
    let mut parts: Vec<String> = Vec::new();
    for def in registry::registry() {
        let events = def.emit(&ctx);
        let Some(last_event) = events.last() else {
            continue;
        };
        if last_event.date < cutoff {
            continue;
        }
        let Some(row) = persisted
            .iter()
            .find(|r| r.signal_id == def.id() && r.horizon_days == 90)
        else {
            continue;
        };
        if row.n_nonoverlap == 0 {
            continue;
        }
        let fired = chrono::NaiveDate::parse_from_str(&last_event.date, "%Y-%m-%d")
            .map(|d| d.format("%b-%d").to_string())
            .unwrap_or_else(|_| last_event.date.clone());
        let since = events
            .first()
            .map(|e| e.date.chars().take(4).collect::<String>())
            .unwrap_or_default();
        let qualifier = if row.n_nonoverlap < 10 {
            " [anecdotal n<10]"
        } else if row.significant {
            " [significant]"
        } else {
            ""
        };
        parts.push(format!(
            "{} fired {fired}: n={} since {since}, 90d mean {} vs baseline {} (lift {}), MAE mean {}{qualifier}",
            def.id(),
            row.n_nonoverlap,
            fmt(row.mean_pct),
            fmt(row.baseline_mean_pct),
            fmt(row.mean_lift).replace('%', "pp"),
            fmt(row.mae_mean),
        ));
        if parts.len() >= 2 {
            break; // keep the card line compact
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// Compute the market-structure verdicts (daily + weekly bars), the
/// composite Cyber Dots verdict (daily bars), and — for BTC / GC=F — the
/// cycle-clock verdict, for the per-asset report card.
/// Uses the deeper of `SYM` / `SYM-USD` history (the held `BTC` series is
/// shallow; the deep series is `BTC-USD`). Every component degrades to
/// None rather than erroring.
#[allow(clippy::type_complexity)]
fn load_structure_and_cycle_verdicts(
    backend: &BackendConnection,
    sym: &str,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    use crate::analytics::market_structure::{analyze, Timeframe};

    let (series, history) =
        match crate::commands::technicals_structure::load_deep_history(backend, sym) {
            Ok(pair) => pair,
            Err(_) => return (None, None, None, None),
        };
    if history.is_empty() {
        return (None, None, None, None);
    }

    let daily = analyze(&series, Timeframe::Daily, &history).map(|r| r.verdict);
    let weekly = analyze(&series, Timeframe::Weekly, &history).map(|r| r.verdict);
    let cyber = crate::analytics::cyber::analyze(
        &series,
        crate::analytics::cyber::CyberTimeframe::Daily,
        &history,
        3,
    )
    .map(|s| s.verdict);

    // Cycle verdict: prefer the deterministic cycle-theory ENGINE's
    // composite verdict (multi-degree bands/translation/FLD/VTL —
    // `analytics::cycle_engine`); the legacy cycle-clock verdict stays as
    // the fallback when the engine has too little history.
    let cycle = match sym {
        "BTC" | "BTC-USD" | "GC=F" | "SI=F" => {
            let deep_series = if matches!(sym, "BTC" | "BTC-USD") {
                "BTC-USD"
            } else {
                sym
            };
            let deep = crate::db::price_history::get_history_backend(backend, deep_series, 9000)
                .unwrap_or_default();
            let source = if deep.len() > history.len() {
                &deep
            } else {
                &history
            };
            let engine_config = crate::analytics::cycle_engine::default_config(sym, deep_series);
            let engine = crate::analytics::cycle_engine::analyze(&engine_config, source)
                .map(|r| r.composite_verdict);
            engine.or_else(|| match sym {
                "BTC" | "BTC-USD" => {
                    crate::analytics::cycle_clock::btc_cycle_clock("BTC-USD", source)
                        .map(|c| c.verdict)
                }
                "GC=F" => crate::analytics::cycle_clock::gold_cycle_clock("GC=F", source)
                    .map(|c| c.verdict),
                _ => None,
            })
        }
        _ => None,
    };

    (daily, weekly, cycle, cyber)
}

/// Derive a `MorningBriefSummary` from the latest narrative snapshot. The
/// snapshot already carries the `headline` + `subtitle` substrate the
/// morning-brief command surfaces for the Executive Summary lead.
fn load_morning_brief_summary(narrative: &serde_json::Value) -> Option<MorningBriefSummary> {
    let headline = narrative
        .get("headline")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());
    let central_tension = narrative
        .get("subtitle")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());
    if headline.is_none() && central_tension.is_none() {
        return None;
    }
    Some(MorningBriefSummary {
        headline,
        central_tension,
    })
}

#[cfg(test)]
mod assembler_tests {
    use super::*;

    #[test]
    fn scoreboard_lines_for_held_filters_and_aggregates() {
        use crate::db::recommendations::{Scoreboard, ScoreboardCell, ScoreboardRow, WindowQuality};
        let board = Scoreboard {
            rows: vec![
                ScoreboardRow {
                    symbol: "GC=F".to_string(),
                    action: "add".to_string(),
                    n_total: 5,
                    h30: None,
                    h90: Some(ScoreboardCell {
                        n: 4,
                        positive: 1,
                        pct_positive: 25.0,
                        mean_pct: -6.0,
                    }),
                    h180: None,
                },
                ScoreboardRow {
                    symbol: "GC=F".to_string(),
                    action: "wait".to_string(),
                    n_total: 2,
                    h30: None,
                    h90: Some(ScoreboardCell {
                        n: 2,
                        positive: 2,
                        pct_positive: 100.0,
                        mean_pct: 3.0,
                    }),
                    h180: None,
                },
                // Held, but nothing scored at 90d → excluded.
                ScoreboardRow {
                    symbol: "SI=F".to_string(),
                    action: "wait".to_string(),
                    n_total: 3,
                    h30: None,
                    h90: None,
                    h180: None,
                },
                // Scored, but not held → excluded.
                ScoreboardRow {
                    symbol: "QQQ".to_string(),
                    action: "add".to_string(),
                    n_total: 1,
                    h30: None,
                    h90: Some(ScoreboardCell {
                        n: 1,
                        positive: 1,
                        pct_positive: 100.0,
                        mean_pct: 8.0,
                    }),
                    h180: None,
                },
            ],
            window_quality: vec![WindowQuality {
                symbol: "GC=F".to_string(),
                add_n: 4,
                wait_n: 2,
                add_mean_90d_pct: Some(-6.0),
                wait_mean_90d_pct: Some(3.0),
                delta_pct: Some(-9.0),
            }],
            unscored: 3,
        };
        let held = vec!["GC=F".to_string(), "SI=F".to_string(), "BTC".to_string()];
        let lines = scoreboard_lines_for_held(&board, &held);
        assert_eq!(lines.len(), 1);
        let gold = &lines[0];
        assert_eq!(gold.symbol, "GC=F");
        assert_eq!(gold.action_mix, "add×5 wait×2");
        assert_eq!(gold.n_scored_90d, 6);
        assert!((gold.pct_positive_90d.unwrap() - 50.0).abs() < 1e-9);
        assert!((gold.window_quality_delta_pct.unwrap() + 9.0).abs() < 1e-9);
    }

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
        // Decisions pending intentionally surfaced via chat (Step 11),
        // not as a static PDF section. Confirm the section is gone.
        assert!(plan.iter().all(|s| s.name != "private_decisions_pending"));
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
            "private_overview",
            "private_operator_deep_dive",
            "private_bottom_line",
            "private_synthesis",
            "private_portfolio_snapshot",
            "private_macro_news_outlook",
            "private_conviction_trajectory",
            "private_outlook_by_horizon",
            "private_risk_concentration",
            "private_investor_panel",
            "private_external_ta",
            "private_parallels",
            "private_closing",
            "private_epistemic_health",
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
        // Decisions Pending now surfaced in chat (Step 11), not in PDF.
        assert!(!body.contains("## Decisions Pending"));
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
    fn parse_parallels_json_handles_empty_or_malformed() {
        assert!(parse_parallels_json("").is_empty());
        assert!(parse_parallels_json("not json").is_empty());
        assert!(parse_parallels_json("{}").is_empty());
        assert!(parse_parallels_json(r#"{"results": []}"#).is_empty());
    }

    #[test]
    fn parse_parallels_json_extracts_canonical_fields() {
        let raw = r#"{
            "generated_at": "2026-06-05",
            "results": [
                {
                    "id": "btc-200wma",
                    "name": "BTC at 200WMA",
                    "symbol": "BTC",
                    "narrative": "Test narrative",
                    "match_count": 12,
                    "median_5d_pct": 1.4,
                    "median_30d_pct": 8.2,
                    "median_90d_pct": 24.1,
                    "median_180d_pct": 45.0,
                    "hit_rate_30d_pct": 75.0,
                    "hit_rate_90d_pct": 83.3
                }
            ]
        }"#;
        let results = parse_parallels_json(raw);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.id, "btc-200wma");
        assert_eq!(r.symbol, "BTC");
        assert_eq!(r.match_count, 12);
        assert_eq!(r.median_30d_pct, Some(8.2));
        assert_eq!(r.hit_rate_90d_pct, Some(83.3));
    }

    #[test]
    fn parse_parallels_json_supports_forward_returns_nested_shape() {
        let raw = r#"{
            "results": [
                {
                    "id": "spx-rsi-elevated",
                    "name": "SPX RSI Elevated",
                    "symbol": "SPY",
                    "narrative": "",
                    "n_matches": 8,
                    "forward_returns": { "5d": 0.5, "30d": -1.0, "90d": 2.5, "180d": 6.0 }
                }
            ]
        }"#;
        let results = parse_parallels_json(raw);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].match_count, 8);
        assert_eq!(results[0].median_5d_pct, Some(0.5));
        assert_eq!(results[0].median_30d_pct, Some(-1.0));
    }

    #[test]
    fn parse_parallels_json_supports_engine_native_distribution_shape() {
        // Repro of the 2026-06-05 bug: engine emits forward_distributions_pct
        // whose values are objects (n / median / mean / p25 / p75 /
        // hit_rate_up) plus a top-level condition_set_name. The parser must
        // extract median + hit_rate_up so the table renders non-empty.
        let raw = r#"{
            "results": [
                {
                    "id": "btc-mayer-undervalued",
                    "condition_set_name": "BTC Mayer Multiple < 0.85",
                    "symbol": "BTC-USD",
                    "narrative": "Mayer's accumulation zone.",
                    "n_matches": 25,
                    "forward_distributions_pct": {
                        "5d":   { "n": 25, "median":  1.43, "hit_rate_up": 64.0 },
                        "30d":  { "n": 25, "median": -0.36, "hit_rate_up": 48.0 },
                        "90d":  { "n": 24, "median": 12.62, "hit_rate_up": 58.3 },
                        "180d": { "n": 23, "median": 11.15, "hit_rate_up": 60.9 }
                    }
                }
            ]
        }"#;
        let results = parse_parallels_json(raw);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.name, "BTC Mayer Multiple < 0.85");
        assert_eq!(r.match_count, 25);
        assert_eq!(r.median_5d_pct, Some(1.43));
        assert_eq!(r.median_30d_pct, Some(-0.36));
        assert_eq!(r.median_90d_pct, Some(12.62));
        assert_eq!(r.median_180d_pct, Some(11.15));
        assert_eq!(r.hit_rate_30d_pct, Some(48.0));
        assert_eq!(r.hit_rate_90d_pct, Some(58.3));
    }

    #[test]
    fn parse_parallels_json_preserves_engine_error() {
        let raw = r#"{
            "results": [
                {
                    "id": "broken",
                    "name": "Broken Set",
                    "symbol": "BTC",
                    "narrative": "",
                    "error": "predicate parse failed"
                }
            ]
        }"#;
        let results = parse_parallels_json(raw);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].error.as_deref(), Some("predicate parse failed"));
    }

    #[test]
    fn load_parallels_results_returns_empty_when_file_missing() {
        // A nonsense date guarantees the file does not exist; loader must
        // degrade rather than panic or surface an error.
        let out = load_parallels_results("9999-99-99");
        assert!(out.is_empty());
    }

    #[test]
    fn load_morning_brief_summary_from_narrative_value() {
        let v = serde_json::json!({
            "headline": "Risk-on with caveats",
            "subtitle": "Sticky services inflation still hot"
        });
        let summary = load_morning_brief_summary(&v).expect("populated brief");
        assert_eq!(summary.headline.as_deref(), Some("Risk-on with caveats"));
        assert!(summary.central_tension.is_some());
    }

    #[test]
    fn load_morning_brief_summary_none_when_no_fields() {
        let v = serde_json::json!({});
        assert!(load_morning_brief_summary(&v).is_none());
    }

    #[test]
    fn data_availability_includes_new_slots() {
        let ctx = BuildContext::default();
        let rows = data_availability(&ctx);
        let names: Vec<&str> = rows.iter().map(|r| r.field).collect();
        assert!(names.contains(&"parallels_results"));
        assert!(names.contains(&"cross_layer_signals"));
        assert!(names.contains(&"private_asset_intelligence"));
        assert!(names.contains(&"morning_brief"));
        assert!(names.contains(&"epistemic_health"));
        // Slots the pre-integrity availability table missed entirely:
        assert!(names.contains(&"investor_panel"));
        assert!(names.contains(&"portfolio_decision_cards"));
        assert!(names.contains(&"todays_analyst_synthesis"));
        assert!(names.contains(&"private_thesis_chains"));
        assert!(names.contains(&"synthesis_adversary_views"));
    }

    /// Parse the field names of a struct out of this source file. Test-only
    /// reflection substitute: lines `pub <ident>: …` between the struct's
    /// opening brace and the first column-0 closing brace.
    fn parse_struct_fields(src: &str, struct_name: &str) -> Vec<String> {
        let needle = format!("pub struct {struct_name} {{");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("struct {struct_name} not found in source"));
        let mut fields = Vec::new();
        for line in src[start + needle.len()..].lines() {
            if line.starts_with('}') {
                break;
            }
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("pub ") {
                if let Some(colon) = rest.find(':') {
                    let name = rest[..colon].trim();
                    if name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !name.is_empty()
                    {
                        fields.push(name.to_string());
                    }
                }
            }
        }
        fields
    }

    /// THE conformance test (schema-conformance pattern applied to the
    /// report): every data-bearing field on the build context must appear in
    /// `data_availability` output. A new loader/slot that ships without
    /// availability tracking turns this red. Do NOT weaken; add the
    /// vec_slot!/opt_slot! row (or a BUILD_CONTEXT_META_FIELDS entry for
    /// genuine metadata) instead.
    #[test]
    fn every_build_context_slot_is_tracked() {
        let src = include_str!("daily.rs");
        let fields = parse_struct_fields(src, "BuildContext");
        assert!(
            fields.len() > 60,
            "struct parse looks broken: only {} fields found",
            fields.len()
        );
        let rows = data_availability(&BuildContext::default());
        let tracked: Vec<&str> = rows.iter().map(|r| r.field).collect();
        if let Err(msg) = check_slot_conformance(&fields, &tracked, BUILD_CONTEXT_META_FIELDS) {
            panic!("{msg}");
        }
    }

    /// Test the test: a fictional slot added to the struct without a
    /// matching availability row must produce a red, actionable failure.
    #[test]
    fn slot_conformance_flags_untracked_fictional_slot() {
        let mut fields = parse_struct_fields(include_str!("daily.rs"), "BuildContext");
        fields.push("fictional_new_slot".to_string());
        let rows = data_availability(&BuildContext::default());
        let tracked: Vec<&str> = rows.iter().map(|r| r.field).collect();
        let err = check_slot_conformance(&fields, &tracked, BUILD_CONTEXT_META_FIELDS)
            .expect_err("an untracked slot must fail conformance");
        assert!(err.contains("fictional_new_slot"), "error names the slot: {err}");
        assert!(
            err.contains("availability tracking"),
            "error explains the rule: {err}"
        );
    }

    /// Renames/deletions must also be caught: tracking a slot that no longer
    /// exists on the struct is a failure too.
    #[test]
    fn slot_conformance_flags_tracked_but_missing_field() {
        let fields = vec!["regime".to_string()];
        let err = check_slot_conformance(&fields, &["regime", "ghost_slot"], &[])
            .expect_err("tracking a non-existent field must fail");
        assert!(err.contains("ghost_slot"), "{err}");
    }

    #[test]
    fn availability_classifies_loader_error_vs_no_data_vs_upstream() {
        let mut ctx = BuildContext::default();
        ctx.slot_issues.insert(
            "cross_layer_signals",
            SlotIssue::LoaderError("synthetic: table locked".to_string()),
        );
        ctx.slot_issues.insert(
            "investor_panel",
            SlotIssue::UpstreamNotRun("no panel-* messages today; latest 2026-06-01".to_string()),
        );
        ctx.slot_issues.insert(
            "parallels_results",
            SlotIssue::NoData("file parsed but no matching sets".to_string()),
        );
        let rows = data_availability(&ctx);
        let by_name = |name: &str| rows.iter().find(|r| r.field == name).unwrap();

        let err_row = by_name("cross_layer_signals");
        assert_eq!(err_row.status, SlotStatus::LoaderError);
        assert_eq!(err_row.reason.as_deref(), Some("synthetic: table locked"));
        assert!(!err_row.populated);

        let upstream_row = by_name("investor_panel");
        assert_eq!(upstream_row.status, SlotStatus::UpstreamNotRun);

        let nodata_row = by_name("parallels_results");
        assert_eq!(nodata_row.status, SlotStatus::NoData);
        assert!(nodata_row.reason.is_some());

        // A slot with no issue recorded and no data is plain no_data.
        let silent = by_name("equity_indices");
        assert_eq!(silent.status, SlotStatus::NoData);
        assert!(silent.reason.is_none());

        // A populated slot is Populated even if an issue was recorded.
        ctx.regime = Some(RegimeSummary {
            classification: "risk_on".to_string(),
            detail: None,
        });
        let rows = data_availability(&ctx);
        let regime_row = rows.iter().find(|r| r.field == "regime").unwrap();
        assert_eq!(regime_row.status, SlotStatus::Populated);
    }

    #[test]
    fn parallels_classified_missing_file_is_upstream_not_run() {
        let (results, issue) = load_parallels_results_classified("1999-01-01");
        assert!(results.is_empty());
        match issue {
            Some(SlotIssue::UpstreamNotRun(reason)) => {
                assert!(reason.contains("did not run"), "{reason}");
            }
            other => panic!("expected UpstreamNotRun, got {other:?}"),
        }
    }

    #[test]
    fn parallels_classified_malformed_file_is_loader_error() {
        let date = format!("test-malformed-{}", std::process::id());
        let path = format!("/tmp/pftui-parallels-{date}.json");
        std::fs::write(&path, "this is not json").unwrap();
        let (results, issue) = load_parallels_results_classified(&date);
        std::fs::remove_file(&path).ok();
        assert!(results.is_empty());
        match issue {
            Some(SlotIssue::LoaderError(reason)) => {
                assert!(reason.contains("not valid JSON"), "{reason}");
            }
            other => panic!("expected LoaderError, got {other:?}"),
        }
    }

    #[test]
    fn parallels_classified_empty_results_is_no_data() {
        let date = format!("test-empty-{}", std::process::id());
        let path = format!("/tmp/pftui-parallels-{date}.json");
        std::fs::write(&path, r#"{"results": []}"#).unwrap();
        let (results, issue) = load_parallels_results_classified(&date);
        std::fs::remove_file(&path).ok();
        assert!(results.is_empty());
        assert!(matches!(issue, Some(SlotIssue::NoData(_))), "{issue:?}");
    }

    #[test]
    fn integrity_footer_quiet_line_when_all_populated() {
        let rows: Vec<DataAvailabilityRow> = (0..5)
            .map(|i| DataAvailabilityRow {
                field: ["a", "b", "c", "d", "e"][i],
                populated: true,
                status: SlotStatus::Populated,
                reason: None,
            })
            .collect();
        let footer = render_integrity_footer(&rows, &[], &[]);
        assert!(footer.contains(INTEGRITY_FOOTER_MARKER));
        assert!(footer.contains("Report integrity: all 5 slots populated."));
        assert!(!footer.contains("LOADER ERRORS"));
        assert!(!footer.contains("No data"));
    }

    #[test]
    fn integrity_footer_renders_loader_errors_in_bold_with_text() {
        let rows = vec![
            DataAvailabilityRow {
                field: "regime",
                populated: true,
                status: SlotStatus::Populated,
                reason: None,
            },
            DataAvailabilityRow {
                field: "cross_layer_signals",
                populated: false,
                status: SlotStatus::LoaderError,
                reason: Some("synthetic: db locked".to_string()),
            },
            DataAvailabilityRow {
                field: "parallels_results",
                populated: false,
                status: SlotStatus::UpstreamNotRun,
                reason: Some("runner did not run".to_string()),
            },
            DataAvailabilityRow {
                field: "equity_news",
                populated: false,
                status: SlotStatus::NoData,
                reason: None,
            },
        ];
        let outcomes = vec![SectionOutcome {
            name: "private_parallels",
            visibility: SectionVisibility::Private,
            rendered: false,
            suppression_reason: Some("no parallel sets matched".to_string()),
        }];
        let footer = render_integrity_footer(&rows, &outcomes, &[]);
        assert!(footer.contains("1/4 slots populated"));
        assert!(footer.contains("No data: equity_news."));
        assert!(footer.contains("Upstream not run: parallels_results."));
        assert!(
            footer.contains("**LOADER ERRORS:** **cross_layer_signals: synthetic: db locked**"),
            "loader errors must be bold with error text: {footer}"
        );
        assert!(footer.contains("auto-suppressed"));
        assert!(footer.contains("private_parallels (no parallel sets matched)"));
    }

    #[test]
    fn assemble_private_appends_integrity_footer_after_last_section() {
        let ctx = BuildContext::for_date("2026-06-02");
        let body = assemble_private(&ctx).unwrap();
        let marker_pos = body
            .find(INTEGRITY_FOOTER_MARKER)
            .expect("private report must carry the integrity footer");
        // The footer must be the LAST block: no section heading after it.
        assert!(
            !body[marker_pos..].contains("\n## "),
            "no section may render after the integrity footer"
        );
        assert!(body[marker_pos..].contains("Report integrity:"));
    }

    #[test]
    fn assemble_private_footer_carries_loader_error_from_context() {
        let mut ctx = BuildContext::for_date("2026-06-02");
        ctx.slot_issues.insert(
            "cross_layer_signals",
            SlotIssue::LoaderError("synthetic failure for test".to_string()),
        );
        let body = assemble_private(&ctx).unwrap();
        assert!(
            body.contains("**cross_layer_signals: synthetic failure for test**"),
            "loader error must surface in the footer"
        );
    }

    #[test]
    fn staleness_warning_injected_under_section_heading() {
        let mut ctx = BuildContext::for_date("2026-06-02");
        ctx.staleness.push(StalenessWarning {
            input: "analyst_views",
            message: "⚠ analyst views are 3 days old (freshness gate 6h) — run Phase 1 before trusting convergence".to_string(),
            sections: vec!["private_investor_panel"],
        });
        let (body, _) = assemble_markdown_accounted(
            &ctx,
            &[SectionSpec {
                name: "private_investor_panel",
                visibility: SectionVisibility::Private,
            }],
        )
        .unwrap();
        let heading_pos = body.find("## Investor Panel").unwrap();
        let warn_pos = body
            .find("> ⚠ analyst views are 3 days old")
            .expect("staleness annotation must be injected");
        assert!(warn_pos > heading_pos, "annotation goes under the heading");
        // Annotation, not suppression: the section body still renders.
        assert!(body.len() > heading_pos + 100);
    }

    #[test]
    fn staleness_does_not_touch_unrelated_sections() {
        let mut ctx = BuildContext::for_date("2026-06-02");
        ctx.staleness.push(StalenessWarning {
            input: "prices",
            message: "⚠ stale prices".to_string(),
            sections: vec!["public_market_snapshot"],
        });
        let (body, _) = assemble_markdown_accounted(
            &ctx,
            &[SectionSpec {
                name: "private_investor_panel",
                visibility: SectionVisibility::Private,
            }],
        )
        .unwrap();
        assert!(!body.contains("stale prices"));
    }

    #[test]
    fn compute_staleness_flags_old_analyst_views() {
        let backend = in_memory_backend();
        let views = vec![crate::db::analyst_views::AnalystView {
            id: 1,
            analyst: "low".to_string(),
            asset: "BTC".to_string(),
            direction: "bullish".to_string(),
            conviction: 2,
            reasoning_summary: "fixture".to_string(),
            key_evidence: None,
            blind_spots: None,
            allocation_bias: None,
            updated_at: "2026-06-01 08:00:00".to_string(),
        }];
        // Report date 3 days after the only view → stale (gate is 6h).
        let warnings = compute_staleness(&backend, "2026-06-04", &views, &[]);
        let views_warning = warnings
            .iter()
            .find(|w| w.input == "analyst_views")
            .expect("old views must produce a staleness warning");
        assert!(views_warning.message.contains("run Phase 1"));
        assert!(views_warning
            .sections
            .contains(&"private_synthesis"));
    }

    #[test]
    fn compute_staleness_quiet_when_views_fresh() {
        let backend = in_memory_backend();
        let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
        let views = vec![crate::db::analyst_views::AnalystView {
            id: 1,
            analyst: "low".to_string(),
            asset: "BTC".to_string(),
            direction: "bullish".to_string(),
            conviction: 2,
            reasoning_summary: "fixture".to_string(),
            key_evidence: None,
            blind_spots: None,
            allocation_bias: None,
            updated_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        }];
        let warnings = compute_staleness(&backend, &today, &views, &[]);
        assert!(!warnings.iter().any(|w| w.input == "analyst_views"));
    }

    #[test]
    fn compute_staleness_flags_price_cache_older_than_report_date() {
        let backend = in_memory_backend();
        let quote = crate::models::price::PriceQuote {
            symbol: "BTC".to_string(),
            price: rust_decimal_macros::dec!(100000),
            currency: "USD".to_string(),
            source: "fixture".to_string(),
            fetched_at: "2026-06-01T12:00:00Z".to_string(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close: None,
        };
        let warnings = compute_staleness(&backend, "2026-06-04", &[], &[quote]);
        let w = warnings
            .iter()
            .find(|w| w.input == "prices")
            .expect("price cache older than report date must warn");
        assert!(w.sections.contains(&"public_market_snapshot"));
        assert!(w.sections.contains(&"private_portfolio_snapshot"));
    }

    #[test]
    fn load_classifies_synthesis_notes_upstream_not_run() {
        let backend = in_memory_backend();
        {
            let conn = backend.sqlite_native().unwrap();
            crate::db::daily_notes::add_note(
                conn,
                "2026-06-01",
                "synthesis",
                "[synthesis-economy]\nFixture economy paragraph from an earlier run.",
                "analyst-synthesis",
            )
            .unwrap();
        }
        let ctx = BuildContext::load(&backend, "2026-06-04").unwrap();
        match ctx.slot_issues.get("synthesis_notes") {
            Some(SlotIssue::UpstreamNotRun(reason)) => {
                assert!(reason.contains("2026-06-01"), "{reason}");
            }
            other => panic!("expected UpstreamNotRun for synthesis_notes, got {other:?}"),
        }
        // And the staleness pass annotates the prose sections.
        assert!(ctx
            .staleness
            .iter()
            .any(|w| w.input == "synthesis_notes"));
    }

    #[test]
    fn load_leaves_no_issue_when_synthesis_notes_present_today() {
        let backend = in_memory_backend();
        {
            let conn = backend.sqlite_native().unwrap();
            crate::db::daily_notes::add_note(
                conn,
                "2026-06-04",
                "synthesis",
                "[synthesis-economy]\nFixture economy paragraph for today.",
                "analyst-synthesis",
            )
            .unwrap();
        }
        let ctx = BuildContext::load(&backend, "2026-06-04").unwrap();
        assert!(ctx.synthesis_notes.has_content());
        assert!(!ctx.slot_issues.contains_key("synthesis_notes"));
    }

    #[test]
    fn suppressed_marker_roundtrip() {
        let marker = crate::report::sections::suppressed("no fixture data");
        assert_eq!(
            extract_suppression_reason(&marker).as_deref(),
            Some("no fixture data")
        );
        // Real content with an embedded comment is NOT a suppression.
        let body = format!("## Heading\n\n{marker}\n\nProse.");
        assert!(extract_suppression_reason(&body).is_none());
        assert!(extract_suppression_reason("").is_none());
    }

    /// Every section renderer's empty state must go through the
    /// suppression-reason channel: against an empty context, a section either
    /// renders content or returns `sections::suppressed(reason)`. A bare
    /// empty string is a conformance failure — the operator could never
    /// learn WHY the section vanished.
    #[test]
    fn every_section_empty_state_carries_a_suppression_reason() {
        let ctx = BuildContext::for_date("2026-06-02");
        let mut names: Vec<&'static str> = Vec::new();
        names.extend(public_section_plan().iter().map(|s| s.name));
        names.extend(private_section_plan().iter().map(|s| s.name));
        // Sections dropped from the default plan but still renderable by the
        // composition step:
        names.extend_from_slice(&[
            "private_macro_context",
            "private_macro_thesis_chains",
            "private_mismatch_surface",
            "private_news_catalysts",
            "private_upcoming_calendar",
            "private_open_predictions",
            "private_lessons_applied",
            "private_self_retrospective_calibration",
            "private_cross_layer_signals",
            "private_decisions_pending",
            "private_per_asset_convergence",
        ]);
        for name in names {
            let body = render_section(name, &ctx)
                .unwrap_or_else(|e| panic!("section {name} failed to render: {e}"));
            if body.trim().is_empty() {
                panic!(
                    "section {name} returned a bare empty body — use \
                     sections::suppressed(reason) so the suppression is accounted"
                );
            }
            if let Some(reason) = extract_suppression_reason(&body) {
                assert!(
                    !reason.trim().is_empty(),
                    "section {name} suppressed without a reason"
                );
            }
        }
    }

    #[test]
    fn dry_run_accounts_suppressed_sections_with_reasons() {
        let ctx = BuildContext::for_date("2026-06-02");
        let summary = render_dry_run(&ctx, BuildMode::Private, "2026-06-02", None, None);
        let overview = summary
            .section_outcomes
            .iter()
            .find(|o| o.name == "private_overview")
            .expect("overview outcome present");
        assert!(!overview.rendered);
        assert!(overview
            .suppression_reason
            .as_deref()
            .unwrap_or_default()
            .contains("synthesis-economy"));
        // Investor panel renders its own empty-state prose → rendered.
        let panel = summary
            .section_outcomes
            .iter()
            .find(|o| o.name == "private_investor_panel")
            .expect("panel outcome present");
        assert!(panel.rendered);
    }

    #[test]
    fn private_section_plan_contains_new_sections() {
        let plan = private_section_plan();
        let names: Vec<&str> = plan.iter().map(|s| s.name).collect();
        assert!(names.contains(&"private_macro_news_outlook"));
        assert!(names.contains(&"private_closing"));
        assert!(names.contains(&"private_external_ta"));
        assert!(names.contains(&"private_parallels"));
        // private_overview must be first.
        assert_eq!(names[0], "private_overview");
        // private_epistemic_health (meta) must be last, after the closing.
        assert!(names.contains(&"private_epistemic_health"));
        assert_eq!(*names.last().unwrap(), "private_epistemic_health");
        assert_eq!(names[names.len() - 2], "private_closing");
        // Sections intentionally dropped per 2026-06-08 polish:
        assert!(!names.contains(&"private_per_asset_convergence"));
        assert!(!names.contains(&"private_decisions_pending"));
        assert!(!names.contains(&"private_macro_context"));
        assert!(!names.contains(&"private_news_catalysts"));
        assert!(!names.contains(&"private_open_predictions"));
        assert!(!names.contains(&"private_lessons_applied"));
        assert!(!names.contains(&"private_cross_layer_signals"));
        assert!(!names.contains(&"private_upcoming_calendar"));
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

    // ------------------------------------------------------------------
    // Loader / derive_actions unit tests — pure-Rust fixtures, no DB.
    // ------------------------------------------------------------------

    fn fixture_convergence_row(
        symbol: &str,
        target_pct: Option<f64>,
        convictions: &[(&str, i64)],
    ) -> PrivateAssetConvergenceRow {
        PrivateAssetConvergenceRow {
            symbol: symbol.to_string(),
            target_pct,
            views: convictions
                .iter()
                .map(|(analyst, conviction)| PrivateAssetConvergenceView {
                    analyst: (*analyst).to_string(),
                    conviction: *conviction,
                    reasoning_summary: format!("{analyst} reasoning"),
                    probation: false,
                    probation_streak: None,
                })
                .collect(),
        }
    }

    fn fixture_drift(symbol: &str, target_pct: f64, actual_pct: f64, band_pct: f64) -> PrivateDriftRow {
        PrivateDriftRow {
            symbol: symbol.to_string(),
            target_pct,
            actual_pct,
            band_pct,
        }
    }

    #[test]
    fn derive_actions_emits_add_when_convergent_bull_and_underweight() {
        let convergence = vec![fixture_convergence_row(
            "BTC",
            Some(20.0),
            &[("low", 2), ("medium", 2), ("high", 2), ("macro", 1)],
        )];
        let drift = vec![fixture_drift("BTC", 20.0, 15.0, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert_eq!(actions.len(), 1, "expected ADD action");
        assert_eq!(actions[0].asset, "BTC");
        assert_eq!(actions[0].action, "ADD");
        assert_eq!(actions[0].urgency, "normal");
    }

    #[test]
    fn derive_actions_emits_high_urgency_for_strong_convergent_bull() {
        let convergence = vec![fixture_convergence_row(
            "BTC",
            Some(20.0),
            &[("low", 4), ("medium", 4), ("high", 5), ("macro", 5)],
        )];
        let drift = vec![fixture_drift("BTC", 20.0, 10.0, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "ADD");
        assert_eq!(actions[0].urgency, "high");
    }

    #[test]
    fn derive_actions_emits_trim_when_convergent_bear_and_overweight() {
        let convergence = vec![fixture_convergence_row(
            "QQQ",
            Some(10.0),
            &[("low", -2), ("medium", -2), ("high", -1), ("macro", -2)],
        )];
        let drift = vec![fixture_drift("QQQ", 10.0, 18.0, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "TRIM");
        assert_eq!(actions[0].urgency, "normal");
    }

    #[test]
    fn derive_actions_emits_high_urgency_for_strong_convergent_bear() {
        let convergence = vec![fixture_convergence_row(
            "QQQ",
            Some(10.0),
            &[("low", -4), ("medium", -5), ("high", -4), ("macro", -3)],
        )];
        let drift = vec![fixture_drift("QQQ", 10.0, 20.0, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "TRIM");
        assert_eq!(actions[0].urgency, "high");
    }

    #[test]
    fn derive_actions_emits_hold_when_convergent_neutral_within_band() {
        let convergence = vec![fixture_convergence_row(
            "GLD",
            Some(10.0),
            &[("low", 0), ("medium", 0), ("high", 1), ("macro", -1)],
        )];
        let drift = vec![fixture_drift("GLD", 10.0, 10.5, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "HOLD");
        assert_eq!(actions[0].urgency, "low");
    }

    #[test]
    fn derive_actions_skips_insufficient_views() {
        let convergence = vec![fixture_convergence_row("BTC", Some(20.0), &[("low", 3)])];
        let drift = vec![fixture_drift("BTC", 20.0, 10.0, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert!(
            actions.is_empty(),
            "single-view rows must produce no action (insufficient-views)"
        );
    }

    #[test]
    fn derive_actions_skips_when_no_drift_row_present() {
        // No matching drift entry for the convergence asset → no action emitted.
        let convergence = vec![fixture_convergence_row(
            "BTC",
            Some(20.0),
            &[("low", 2), ("medium", 2), ("high", 2)],
        )];
        let drift = vec![fixture_drift("QQQ", 10.0, 9.0, 1.0)];
        let actions = derive_actions(&convergence, &drift);
        assert!(actions.is_empty());
    }

    #[test]
    fn derive_actions_does_not_emit_add_when_already_in_band() {
        let convergence = vec![fixture_convergence_row(
            "BTC",
            Some(20.0),
            &[("low", 2), ("medium", 2), ("high", 2), ("macro", 1)],
        )];
        // actual within target band — bullish convergence but no rebalance need.
        let drift = vec![fixture_drift("BTC", 20.0, 19.5, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert!(actions.is_empty());
    }

    #[test]
    fn derive_actions_drift_match_is_case_insensitive() {
        let convergence = vec![fixture_convergence_row(
            "btc",
            Some(20.0),
            &[("low", 2), ("medium", 2), ("high", 2)],
        )];
        let drift = vec![fixture_drift("BTC", 20.0, 12.0, 2.0)];
        let actions = derive_actions(&convergence, &drift);
        assert_eq!(actions.len(), 1, "case mismatch must not block matching");
        assert_eq!(actions[0].action, "ADD");
    }

    // ------------------------------------------------------------------
    // Loader tests against an in-memory SQLite backend. These exercise
    // the convergence + drift loaders end-to-end using only synthetic
    // fixtures (no live DB access, per CLAUDE.md data-security rule).
    // ------------------------------------------------------------------

    fn in_memory_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory sqlite");
        crate::db::schema::run_migrations(&conn).expect("migrate in-memory db");
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn load_populates_private_asset_convergence_from_analyst_views() {
        use rust_decimal_macros::dec;
        let backend = in_memory_backend();
        // Synthetic per-asset views from each of the four analyst layers.
        for (analyst, conviction) in [("low", 2), ("medium", 2), ("high", 3), ("macro", 2)] {
            crate::db::analyst_views::upsert_view_backend(
                &backend,
                analyst,
                "BTC",
                "bull",
                conviction,
                "Synthetic fixture reasoning.",
                None,
                None,
                None,
            )
            .expect("upsert synthetic view");
        }
        crate::db::allocation_targets::set_target_backend(&backend, "BTC", dec!(20), dec!(2))
            .expect("set synthetic target");

        let ctx = BuildContext::load(&backend, "2026-06-05").expect("load context");
        assert_eq!(ctx.private_asset_convergence.len(), 1);
        let row = &ctx.private_asset_convergence[0];
        assert_eq!(row.symbol, "BTC");
        assert_eq!(row.views.len(), 4, "expected all four layers");
        assert_eq!(row.target_pct, Some(20.0));
    }

    #[test]
    fn load_emits_no_drift_rows_when_allocation_targets_empty() {
        let backend = in_memory_backend();
        // Seed views so positions are knowable, but no allocation_targets rows.
        crate::db::analyst_views::upsert_view_backend(
            &backend, "low", "BTC", "bull", 2, "fixture", None, None, None,
        )
        .expect("upsert");

        let ctx = BuildContext::load(&backend, "2026-06-05").expect("load");
        assert!(
            ctx.private_drift_rows.is_empty(),
            "empty allocation_targets table → no drift rows"
        );
    }

    #[test]
    fn load_derives_actions_from_loaded_convergence_and_drift() {
        use rust_decimal_macros::dec;
        let backend = in_memory_backend();

        // Synthetic four-layer bull convergence on BTC.
        for (analyst, conviction) in [("low", 2), ("medium", 2), ("high", 3), ("macro", 2)] {
            crate::db::analyst_views::upsert_view_backend(
                &backend, analyst, "BTC", "bull", conviction, "fixture", None, None, None,
            )
            .expect("upsert");
        }
        crate::db::allocation_targets::set_target_backend(&backend, "BTC", dec!(20), dec!(2))
            .expect("set target");

        // Bypass the transactions-based positions loader by stamping a
        // synthetic underweight position and re-running the derive step.
        // The DB-driven loader paths still run via `BuildContext::load`.
        let mut ctx = BuildContext::load(&backend, "2026-06-05").expect("load");
        ctx.private_positions = vec![PrivatePositionSnapshotRow {
            symbol: "BTC".to_string(),
            price: Some("100000".to_string()),
            daily_change: None,
            allocation_pct: 12.0,
            unrealized_pnl: None,
        }];
        ctx.private_drift_rows = vec![PrivateDriftRow {
            symbol: "BTC".to_string(),
            target_pct: 20.0,
            actual_pct: 12.0,
            band_pct: 2.0,
        }];
        ctx.private_derived_actions =
            derive_actions(&ctx.private_asset_convergence, &ctx.private_drift_rows);
        assert_eq!(ctx.private_derived_actions.len(), 1);
        assert_eq!(ctx.private_derived_actions[0].action, "ADD");
    }

    // ---------------------------------------------------------------
    // Loader tests for BTC ETF flows, on-chain context, and catalysts
    // (agent W3 — flow + catalyst loaders)
    // ---------------------------------------------------------------

    use crate::db::calendar_cache::CalendarEvent;
    use std::str::FromStr;

    fn evt(date: &str, name: &str, impact: &str, ty: &str) -> CalendarEvent {
        CalendarEvent {
            id: 0,
            date: date.to_string(),
            name: name.to_string(),
            impact: impact.to_string(),
            previous: None,
            forecast: None,
            event_type: ty.to_string(),
            symbol: None,
            fetched_at: "2026-06-05T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn calendar_to_macro_catalysts_keeps_high_and_medium_economic_only() {
        let events = vec![
            evt("2026-06-10", "FOMC Decision", "high", "economic"),
            evt("2026-06-11", "NFP", "high", "economic"),
            evt("2026-06-12", "Building Permits", "low", "economic"),
            evt("2026-06-13", "AAPL Earnings", "high", "earnings"),
            evt("2026-06-14", "CPI", "medium", "economic"),
        ];
        let rows = calendar_to_macro_catalysts(&events, 10);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|r| r.event == "FOMC Decision"));
        assert!(rows.iter().any(|r| r.event == "CPI"));
        // Low impact and earnings excluded.
        assert!(rows.iter().all(|r| r.event != "Building Permits"));
        assert!(rows.iter().all(|r| r.event != "AAPL Earnings"));
    }

    #[test]
    fn calendar_to_binary_catalysts_filters_to_high_impact_within_horizon() {
        let events = vec![
            evt("2026-06-06", "FOMC", "high", "economic"),
            evt("2026-06-07", "CPI", "high", "economic"),
            evt("2026-06-25", "PCE", "high", "economic"), // outside 14d
            // Housing Starts: medium-impact, stays medium under the name
            // heuristic — should be excluded from the binary slate.
            evt("2026-06-08", "Housing Starts", "medium", "economic"),
        ];
        let rows = calendar_to_binary_catalysts(&events, "2026-06-05", 14, 6);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.event == "FOMC"));
        assert!(rows.iter().any(|r| r.event == "CPI"));
        assert!(rows.iter().all(|r| r.event != "PCE"));
        assert!(rows.iter().all(|r| r.event != "Housing Starts"));
    }

    #[test]
    fn effective_impact_upgrades_misclassified_high_impact_events() {
        // A scraped row mis-tagged "low" but the name is "Non Farm Payrolls":
        // the read-time backstop must upgrade it to "high" so the binary
        // catalyst slate doesn't silently drop it. Repro of the 2026-06-05
        // regression where NFP / CPI day rendered as "low importance".
        let nfp = evt("2026-06-06", "Non Farm Payrolls", "low", "economic");
        assert_eq!(effective_impact(&nfp), "high");

        let cpi = evt("2026-06-10", "Core CPI YoY", "low", "economic");
        assert_eq!(effective_impact(&cpi), "high");

        let baker = evt("2026-06-06", "Baker Hughes Oil Rig Count", "low", "economic");
        assert_eq!(effective_impact(&baker), "low"); // genuinely low

        let participation = evt("2026-06-06", "Participation Rate", "low", "economic");
        assert_eq!(effective_impact(&participation), "medium");

        // Stored "high" wins over inferred "low".
        let mut treaty = evt("2026-06-10", "Some Random Treaty Vote", "high", "economic");
        treaty.impact = "high".to_string();
        assert_eq!(effective_impact(&treaty), "high");
    }

    #[test]
    fn calendar_to_binary_catalysts_returns_empty_on_unparseable_date() {
        let events = vec![evt("2026-06-06", "FOMC", "high", "economic")];
        let rows = calendar_to_binary_catalysts(&events, "not-a-date", 14, 6);
        assert!(rows.is_empty());
    }

    #[test]
    fn catalyst_impact_label_includes_forecast_when_present() {
        let mut e = evt("2026-06-10", "CPI", "high", "economic");
        e.forecast = Some("3.1%".to_string());
        e.previous = Some("3.0%".to_string());
        let label = catalyst_impact_label(&e);
        assert!(label.contains("High-impact"));
        assert!(label.contains("3.1%"));
        assert!(label.contains("3.0%"));
    }

    #[test]
    fn format_usd_compact_renders_with_sign_and_unit() {
        use rust_decimal::Decimal;
        assert_eq!(format_usd_compact(Decimal::from_str("245300000").unwrap()), "+$245.3M");
        assert_eq!(format_usd_compact(Decimal::from_str("-1200000000").unwrap()), "-$1.20B");
        assert_eq!(format_usd_compact(Decimal::from_str("0").unwrap()), "+$0");
    }

    #[test]
    fn onchain_flow_from_metadata_extracts_btc_fields() {
        let meta = r#"{"reserve_usd": 12345.0, "flow_7d_btc": -123.4, "flow_30d_btc": -567.8}"#;
        let (f7, f30) = onchain_flow_from_metadata(Some(meta));
        assert!((f7.unwrap() + 123.4).abs() < f64::EPSILON);
        assert!((f30.unwrap() + 567.8).abs() < f64::EPSILON);
    }

    #[test]
    fn onchain_flow_from_metadata_returns_none_on_missing_or_invalid() {
        assert_eq!(onchain_flow_from_metadata(None), (None, None));
        assert_eq!(onchain_flow_from_metadata(Some("not json")), (None, None));
    }

    #[test]
    fn load_bitcoin_etf_flow_summaries_aggregates_creation_redemption_by_window() {
        let backend = in_memory_backend();
        let conn = backend.sqlite_native().expect("sqlite backend");
        // Seed three flow rows: two creations and one redemption within
        // 1d / 7d windows. 30d should pick up all three.
        for (flow_type, amount, period_end) in [
            ("etf_creation", "200000000", "2026-06-04"),  // 1d
            ("etf_redemption", "50000000", "2026-06-01"), // within 7d
            ("etf_creation", "100000000", "2026-05-20"),  // within 30d
        ] {
            crate::db::capital_flows::insert(
                conn,
                &crate::data::flows::CapitalFlow {
                    asset: "BTC".to_string(),
                    flow_type: flow_type.to_string(),
                    amount_usd: rust_decimal::Decimal::from_str(amount).unwrap(),
                    period_start: period_end.to_string(),
                    period_end: period_end.to_string(),
                    source: "fixture".to_string(),
                },
            )
            .expect("insert flow");
        }
        let rows = load_bitcoin_etf_flow_summaries(&backend, "2026-06-05");
        assert_eq!(rows.len(), 3, "1d / 7d / 30d windows");
        assert_eq!(rows[0].period, "1d");
        assert_eq!(rows[1].period, "7d");
        assert_eq!(rows[2].period, "30d");
        // 1d window picks up the +$200M creation only.
        assert!(rows[0].net_flow.as_deref().unwrap().contains("+$200"));
        // 7d window nets the creation against the redemption: +$150M.
        assert!(rows[1].net_flow.as_deref().unwrap().contains("+$150"));
        // 30d window: 200 - 50 + 100 = +$250M.
        assert!(rows[2].net_flow.as_deref().unwrap().contains("+$250"));
    }

    #[test]
    fn load_bitcoin_etf_flow_summaries_returns_empty_when_no_btc_etf_rows() {
        let backend = in_memory_backend();
        let rows = load_bitcoin_etf_flow_summaries(&backend, "2026-06-05");
        assert!(rows.is_empty());
    }

    #[test]
    fn load_bitcoin_onchain_summaries_pulls_network_and_reserve_when_present() {
        let backend = in_memory_backend();
        let conn = backend.sqlite_native().expect("sqlite backend");
        crate::db::onchain_cache::upsert_metric(
            conn,
            &crate::db::onchain_cache::OnchainMetric {
                metric: "network".to_string(),
                date: "2026-06-04".to_string(),
                value: "620.5".to_string(),
                metadata: None,
                fetched_at: "2026-06-04T12:00:00Z".to_string(),
            },
        )
        .expect("upsert");
        crate::db::onchain_cache::upsert_metric(
            conn,
            &crate::db::onchain_cache::OnchainMetric {
                metric: "exchange_reserve_proxy_btc".to_string(),
                date: "2026-06-04".to_string(),
                value: "1850000".to_string(),
                metadata: Some(
                    r#"{"flow_7d_btc": -1200.0, "flow_30d_btc": -3400.0}"#.to_string(),
                ),
                fetched_at: "2026-06-04T12:00:00Z".to_string(),
            },
        )
        .expect("upsert");
        let rows = load_bitcoin_onchain_summaries(&backend);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].value.as_deref().unwrap().contains("620.5 EH/s"));
        assert!(rows[1].interpretation.as_deref().unwrap().contains("7d net flow -1200"));
    }
}

#[cfg(test)]
mod todays_analyst_synthesis_tests {
    use super::*;
    use rusqlite::Connection;

    fn create_tables(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE daily_notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,
                section TEXT NOT NULL DEFAULT 'general',
                content TEXT NOT NULL,
                author TEXT NOT NULL DEFAULT 'system',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                novelty_score REAL
            );
            CREATE TABLE agent_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_agent TEXT NOT NULL,
                to_agent TEXT,
                package_id TEXT,
                package_title TEXT,
                priority TEXT NOT NULL DEFAULT 'normal',
                content TEXT NOT NULL,
                category TEXT,
                layer TEXT,
                acknowledged INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                acknowledged_at TEXT
            );",
        )
        .expect("create tables");
    }

    #[test]
    fn synthesis_loader_surfaces_leading_move_action_and_headlines() {
        let conn = Connection::open_in_memory().expect("memory db");
        create_tables(&conn);

        let date = "2026-06-05";
        // Multiple notes per analyst — loader should pick the longest as the headline.
        crate::db::daily_notes::add_note(
            &conn,
            date,
            "low",
            "BTC -7% to $62,447 cum -14% from May 28; ETF -$671M, COT 92.3 pctile flush — risk-on tape broke under quarter-end positioning unwinds.",
            "analyst-low",
        )
        .unwrap();
        crate::db::daily_notes::add_note(&conn, date, "low", "ping", "analyst-low").unwrap();
        crate::db::daily_notes::add_note(
            &conn,
            date,
            "medium",
            "Weekly outlook: credit spreads continuing to widen, rates pricing eases marginally.",
            "analyst-medium",
        )
        .unwrap();
        crate::db::daily_notes::add_note(
            &conn,
            date,
            "macro",
            "Macro: dollar squeeze through quarter-end is the dominant tape; DXY +0.3% intraday.",
            "analyst-macro",
        )
        .unwrap();

        crate::db::agent_messages::send_message(
            &conn,
            "analyst-low",
            Some("synthesis"),
            Some("high"),
            "Trim BTC exposure into strength; raise stop to $61.5k ahead of CME open.",
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let held = vec!["BTC".to_string(), "GLD".to_string(), "SPY".to_string()];
        let synthesis = load_todays_analyst_synthesis(&conn, date, &held)
            .expect("loader runs")
            .expect("synthesis produced");

        let leading = synthesis.leading_move.as_ref().expect("leading move set");
        assert_eq!(leading.asset, "BTC");
        assert!((leading.move_pct + 7.0).abs() < f64::EPSILON, "{}", leading.move_pct);
        assert_eq!(leading.cumulative_pct, Some(-14.0));

        assert!(synthesis.headline_low.as_deref().unwrap().contains("BTC -7%"));
        assert!(synthesis.headline_medium.as_deref().unwrap().contains("credit spreads"));
        assert!(synthesis.headline_high.is_none());
        assert!(synthesis.headline_macro.as_deref().unwrap().contains("dollar squeeze"));

        let action = synthesis.action_summary.as_deref().unwrap();
        assert!(action.contains("Trim BTC exposure"), "action: {action}");
    }

    #[test]
    fn synthesis_loader_returns_none_when_nothing_today() {
        let conn = Connection::open_in_memory().expect("memory db");
        create_tables(&conn);
        // Insert a note for a DIFFERENT date so the report-date filter excludes it.
        crate::db::daily_notes::add_note(
            &conn,
            "2026-05-30",
            "low",
            "BTC -1.2% intraday",
            "analyst-low",
        )
        .unwrap();
        let result = load_todays_analyst_synthesis(&conn, "2026-06-05", &["BTC".to_string()])
            .expect("loader runs");
        assert!(result.is_none(), "expected no synthesis: {result:?}");
    }

    #[test]
    fn synthesis_loader_ignores_unheld_assets_in_leading_move() {
        let conn = Connection::open_in_memory().expect("memory db");
        create_tables(&conn);
        let date = "2026-06-05";
        crate::db::daily_notes::add_note(
            &conn,
            date,
            "low",
            "TSLA +12% on earnings, BTC -2.0% intraday.",
            "analyst-low",
        )
        .unwrap();
        // BTC is held, TSLA is not. Expect BTC -2.0% even though TSLA |12%| is larger.
        let synthesis = load_todays_analyst_synthesis(&conn, date, &["BTC".to_string()])
            .expect("loader runs")
            .expect("synthesis produced");
        let mv = synthesis.leading_move.expect("leading move set");
        assert_eq!(mv.asset, "BTC");
        assert!((mv.move_pct + 2.0).abs() < f64::EPSILON);
    }

    // ---------------------------------------------------------------
    // Loader tests for `BuildContext::load`
    // (agent W2 — deltas / macro scenarios / macro regime)
    // ---------------------------------------------------------------

    use crate::analytics::situation::SituationInsight;

    #[test]
    fn map_change_radar_bullish_signed_value_yields_bull() {
        let insight = SituationInsight {
            title: "BTC momentum re-priced".to_string(),
            detail: "Bitcoin (BTC)".to_string(),
            value: "+2.50".to_string(),
            severity: "elevated".to_string(),
        };
        let row = map_change_radar_to_delta(insight);
        assert_eq!(row.direction, "bull");
        assert_eq!(row.delta, "+2.50");
        assert_eq!(row.label, "BTC momentum re-priced");
    }

    #[test]
    fn map_change_radar_bearish_signed_value_yields_bear() {
        let insight = SituationInsight {
            title: "Scenario re-ranked: Soft Landing".to_string(),
            detail: "Probability moved from 30% to 22%.".to_string(),
            value: "-8".to_string(),
            severity: "elevated".to_string(),
        };
        let row = map_change_radar_to_delta(insight);
        assert_eq!(row.direction, "bear");
    }

    #[test]
    fn map_change_radar_regime_shift_is_info() {
        let insight = SituationInsight {
            title: "Regime shifted".to_string(),
            detail: "The current regime changed since the baseline snapshot.".to_string(),
            value: "risk off".to_string(),
            severity: "critical".to_string(),
        };
        let row = map_change_radar_to_delta(insight);
        assert_eq!(row.direction, "info");
    }

    #[test]
    fn map_change_radar_correlation_break_is_info() {
        let insight = SituationInsight {
            title: "Correlation shifted: BTC / GLD".to_string(),
            detail: "30d moved from 0.20 to -0.45.".to_string(),
            value: "-0.65".to_string(),
            severity: "elevated".to_string(),
        };
        let row = map_change_radar_to_delta(insight);
        // "Correlation shifted" branch dominates the signed-value branch so
        // correlation breaks always tag as info.
        assert_eq!(row.direction, "info");
    }

    #[test]
    fn map_change_radar_unsigned_value_falls_back_to_info() {
        let insight = SituationInsight {
            title: "Lead signal changed".to_string(),
            detail: "VIX spike".to_string(),
            value: "warning".to_string(),
            severity: "elevated".to_string(),
        };
        let row = map_change_radar_to_delta(insight);
        assert_eq!(row.direction, "info");
    }

    #[test]
    fn regime_to_axes_known_labels() {
        assert_eq!(regime_to_axes("risk_on"), Some((1.0, 0.0)));
        assert_eq!(regime_to_axes("RISK-ON"), Some((1.0, 0.0)));
        assert_eq!(regime_to_axes("lean_risk_on"), Some((0.5, 0.0)));
        assert_eq!(regime_to_axes("neutral"), Some((0.0, 0.0)));
        assert_eq!(regime_to_axes("transitioning"), Some((0.0, 0.0)));
        assert_eq!(regime_to_axes("lean_risk_off"), Some((-0.5, 0.0)));
        assert_eq!(regime_to_axes("risk_off"), Some((-1.0, 0.0)));
    }

    #[test]
    fn regime_to_axes_unknown_label_yields_none() {
        assert!(regime_to_axes("goldilocks_v2").is_none());
        assert!(regime_to_axes("").is_none());
    }

    /// Build an in-memory SQLite backend with the canonical schema for loader
    /// integration tests. Each test owns its own backend so they can't
    /// observe one another's writes.
    fn fresh_backend() -> crate::db::backend::BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory sqlite");
        crate::db::schema::run_migrations(&conn).expect("run migrations");
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn loader_private_macro_scenarios_sorts_by_probability_desc() {
        let backend = fresh_backend();
        {
            let conn = backend.sqlite_native().expect("sqlite backend");
            // Seed three active scenarios with distinct probabilities.
            crate::db::scenarios::add_scenario(
                conn,
                "Soft Landing",
                30.0,
                None,
                None,
                None,
                None,
            )
            .expect("add a");
            crate::db::scenarios::add_scenario(
                conn,
                "Stagflation",
                55.0,
                None,
                None,
                None,
                None,
            )
            .expect("add b");
            crate::db::scenarios::add_scenario(
                conn,
                "Recession",
                15.0,
                None,
                None,
                None,
                None,
            )
            .expect("add c");
        }
        let ctx = BuildContext::load(&backend, "2026-06-05").expect("load context");
        let names: Vec<&str> = ctx
            .private_macro_scenarios
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, vec!["Stagflation", "Soft Landing", "Recession"]);
        // Probability values are surfaced unchanged; prior_7d falls back to
        // the current probability when no history exists yet.
        for row in &ctx.private_macro_scenarios {
            assert!((row.prior_7d - row.probability).abs() < 1e-9);
        }
    }

    #[test]
    fn loader_private_macro_regime_maps_axes_and_collects_trail() {
        let backend = fresh_backend();
        {
            let conn = backend.sqlite_native().expect("sqlite backend");
            // Seed the latest plus two prior snapshots. `store_regime` writes
            // in insert-order; `get_history_backend` orders DESC by
            // `recorded_at`, so we insert oldest-first below.
            // Older snapshots first so the most recent ends up at head.
            crate::db::regime_snapshots::store_regime(
                conn, "risk_off", None, None, None, None, None, None, None, None,
            )
            .expect("seed 1");
            crate::db::regime_snapshots::store_regime(
                conn,
                "lean_risk_on",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("seed 2");
            crate::db::regime_snapshots::store_regime(
                conn, "risk_on", None, None, None, None, None, None, None, None,
            )
            .expect("seed head");
        }
        let ctx = BuildContext::load(&backend, "2026-06-05").expect("load context");
        let quad = ctx
            .private_macro_regime
            .expect("macro regime should be populated");
        // Head is the most-recent insert ("risk_on") because the loader
        // orders DESC by recorded_at. The trail carries the two earlier rows.
        assert!((quad.growth - 1.0).abs() < 1e-9);
        assert!((quad.inflation - 0.0).abs() < 1e-9);
        assert_eq!(quad.trail.len(), 2);
    }

    #[test]
    fn loader_private_what_changed_deltas_degrades_to_empty_on_empty_db() {
        let backend = fresh_backend();
        let ctx = BuildContext::load(&backend, "2026-06-05").expect("load context");
        // An empty situation-snapshot history can't produce material deltas;
        // the loader must degrade to an empty Vec rather than abort.
        // (The "No major deltas" filler row is upstream of map; the deltas
        // loader passes through whatever `change_radar` returns.)
        for row in &ctx.private_what_changed_deltas {
            assert!(["bull", "bear", "info"].contains(&row.direction.as_str()));
        }
    }
}

// ---------------------------------------------------------------------------
// W4 loader unit tests (synthetic fixtures, no real-DB dependency).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod loader_w4_tests {
    use super::*;
    use crate::db::analyst_views::AnalystView;
    use crate::db::news_cache::{NewsEntry, NewsSourceIndependence};

    fn synthetic_news(id: i64, title: &str, symbol_tag: Option<&str>) -> NewsEntry {
        NewsEntry {
            id,
            title: title.to_string(),
            url: format!("https://example.com/{id}"),
            source: "Example".to_string(),
            source_type: "rss".to_string(),
            symbol_tag: symbol_tag.map(|s| s.to_string()),
            source_domain: "example.com".to_string(),
            source_tier: 1,
            source_tier_inferred: false,
            source_independence: NewsSourceIndependence::Independent,
            description: format!("Body for {title}"),
            extra_snippets: Vec::new(),
            category: "news".to_string(),
            topic: "equities".to_string(),
            published_at: 0,
            fetched_at: "2026-06-02T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn private_news_events_match_held_symbol_tag_case_insensitive() {
        let news = vec![
            synthetic_news(1, "BTC rallies on flows", Some("BTC")),
            synthetic_news(2, "Oil pumps", Some("XOM")),
            synthetic_news(3, "Compound symbol_tag list", Some("gld,slv")),
            synthetic_news(4, "No tag", None),
        ];
        let held = vec!["BTC".to_string(), "GLD".to_string()];
        let out = private_news_events_for_held(&news, &held);
        // Two entries match: BTC headline and the GLD,SLV combined tag.
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|c| c.headline == "BTC rallies on flows"));
        assert!(out
            .iter()
            .any(|c| c.headline == "Compound symbol_tag list"
                && c.related_assets.iter().any(|a| a == "GLD")));
        // Domain and tier flow through.
        assert!(out.iter().all(|c| c.domain == "example.com"));
        assert!(out.iter().all(|c| c.source_tier == Some(1)));
    }

    #[test]
    fn private_news_events_empty_when_no_held_assets() {
        let news = vec![synthetic_news(1, "BTC up", Some("BTC"))];
        let out = private_news_events_for_held(&news, &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn private_news_events_alias_match_held_futures_against_etf_symbol_tag() {
        // News feed tags gold rows as "GLD" / "XAU" / "GOLD", but the held
        // symbol is the futures contract "GC=F". Without alias expansion the
        // News & Catalysts section silently renders empty for gold-heavy
        // weeks. Repro of the 2026-06-05 weekly regression.
        let news = vec![
            synthetic_news(1, "Gold breakout on DXY weakness", Some("GLD")),
            synthetic_news(2, "Silver tracks gold higher", Some("XAG")),
            synthetic_news(3, "BTC ETF outflows accelerate", Some("BITCOIN")),
            synthetic_news(4, "Random unrelated", Some("XOM")),
        ];
        let held = vec!["GC=F".to_string(), "SI=F".to_string(), "BTC".to_string()];
        let out = private_news_events_for_held(&news, &held);
        assert_eq!(out.len(), 3, "gold + silver + btc alias matches expected");
        assert!(out.iter().any(|c| c.headline.contains("Gold breakout")));
        assert!(out.iter().any(|c| c.headline.contains("Silver tracks")));
        assert!(out.iter().any(|c| c.headline.contains("BTC ETF")));
    }

    #[test]
    fn symbol_aliases_for_news_returns_expected_set() {
        let gld = symbol_aliases_for_news("GC=F");
        assert!(gld.contains(&"GLD".to_string()));
        assert!(gld.contains(&"XAU".to_string()));
        assert!(gld.contains(&"GC=F".to_string()));

        let btc = symbol_aliases_for_news("BTC");
        assert!(btc.contains(&"BITCOIN".to_string()));
        assert!(btc.contains(&"BTC-USD".to_string()));

        // Unknown symbols just return themselves uppercased.
        let aapl = symbol_aliases_for_news("aapl");
        assert_eq!(aapl, vec!["AAPL".to_string()]);
    }

    fn synthetic_view(analyst: &str, asset: &str, conviction: i64, direction: &str) -> AnalystView {
        AnalystView {
            id: 0,
            analyst: analyst.to_string(),
            asset: asset.to_string(),
            direction: direction.to_string(),
            conviction,
            reasoning_summary: format!("{analyst} view of {asset}"),
            key_evidence: None,
            blind_spots: None,
            allocation_bias: None,
            updated_at: "2026-06-02T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn outlooks_for_held_maps_layers_to_horizons() {
        let views = vec![
            synthetic_view("low", "BTC", 3, "bull"),
            synthetic_view("medium", "BTC", 2, "bull"),
            synthetic_view("high", "BTC", -1, "bear"),
            synthetic_view("macro", "BTC", 4, "bull"),
            // GLD only has a macro view — months falls back to it.
            synthetic_view("macro", "GLD", 3, "bull"),
        ];
        let held = vec!["BTC".to_string(), "GLD".to_string()];
        let rows = outlooks_for_held(&views, &held);
        assert_eq!(rows.len(), 2);
        let btc = rows.iter().find(|r| r.symbol == "BTC").unwrap();
        assert_eq!(btc.days.as_ref().unwrap().direction, "bull");
        assert_eq!(btc.weeks.as_ref().unwrap().direction, "bull");
        // HIGH wins over MACRO for the months slot.
        assert_eq!(btc.months.as_ref().unwrap().direction, "bear");
        let gld = rows.iter().find(|r| r.symbol == "GLD").unwrap();
        assert!(gld.days.is_none());
        assert!(gld.weeks.is_none());
        // MACRO fallback fills months.
        assert_eq!(gld.months.as_ref().unwrap().direction, "bull");
    }

    #[test]
    fn outlooks_for_held_skips_assets_with_no_views() {
        let views = vec![synthetic_view("low", "BTC", 2, "bull")];
        let held = vec!["BTC".to_string(), "ZZZ".to_string()];
        let rows = outlooks_for_held(&views, &held);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "BTC");
    }

    fn open_db() -> rusqlite::Connection {
        let conn = crate::db::open_in_memory();
        // analyst_view_history is created lazily by the analyst_views module;
        // create it directly here so the trajectory loader has a table to read.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS analyst_view_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                analyst TEXT NOT NULL,
                asset TEXT NOT NULL,
                direction TEXT NOT NULL,
                conviction INTEGER NOT NULL,
                reasoning_summary TEXT NOT NULL,
                key_evidence TEXT,
                blind_spots TEXT,
                allocation_bias TEXT,
                recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        // Suppress foreign-key check from prediction_lessons.prediction_id
        // so loader tests can insert lessons without first creating a
        // matching prediction row. Real callers always go through the
        // higher-level CRUD which enforces the link.
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        conn
    }

    #[test]
    fn load_calibration_rows_dedupes_to_latest_per_layer_band() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('low', 'BTC', 'high', 12, 0.6, 0.7, '2026-05-30 00:00:00')",
            [],
        )
        .unwrap();
        // Newer row for the same (layer, band) should win.
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('low', 'BTC', 'high', 20, 0.5, 0.65, '2026-06-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('medium', 'GLD', 'medium', 8, 0.4, 0.5, '2026-05-29 00:00:00')",
            [],
        )
        .unwrap();
        let rows = load_calibration_rows(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        let low_high = rows
            .iter()
            .find(|r| r.layer == "LOW" && r.conviction_band == "high")
            .unwrap();
        assert_eq!(low_high.sample_size, 20);
        assert!((low_high.observed_pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn load_calibration_rows_for_held_filters_topics_to_portfolio() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('low', 'BTC', 'high', 10, 0.6, 0.7, '2026-06-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('medium', 'XOM', 'medium', 12, 0.5, 0.55, '2026-06-01 00:00:00')",
            [],
        )
        .unwrap();
        let held = vec!["BTC".to_string()];
        let rows = load_calibration_rows_for_held(&conn, &held).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].layer, "LOW");
    }

    #[test]
    fn load_open_predictions_calibration_picks_dominant_layer() {
        let conn = open_db();
        // Two pending 'low' predictions, one pending 'high' prediction.
        for _ in 0..2 {
            conn.execute(
                "INSERT INTO user_predictions
                 (claim, symbol, conviction, timeframe, topic, confidence, outcome, lessons_applied, created_at)
                 VALUES ('c', 'BTC', 'high', 'low', 'crypto', 0.7, 'pending', '[]', '2026-06-02 00:00:00')",
                [],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO user_predictions
             (claim, symbol, conviction, timeframe, topic, confidence, outcome, lessons_applied, created_at)
             VALUES ('c', 'BTC', 'high', 'high', 'crypto', 0.6, 'pending', '[]', '2026-06-02 00:00:00')",
            [],
        )
        .unwrap();
        // Calibration rows for both layers; 'low' has the bigger sample.
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('low', 'BTC', 'high', 30, 0.55, 0.6, '2026-06-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO calibration_matrix
             (layer, topic, conviction_band, n, hit_rate, stated_confidence, recorded_at)
             VALUES ('high', 'BTC', 'high', 5, 0.4, 0.7, '2026-06-01 00:00:00')",
            [],
        )
        .unwrap();
        // Synthetic open list so the early-out doesn't fire.
        let open = vec![PrivateOpenPredictionRow {
            id: Some(1),
            symbol: "BTC".to_string(),
            claim: "c".to_string(),
            target_date: "2026-06-30".to_string(),
            days_remaining: 28,
            confidence: Some(0.7),
            conviction: None,
            direction: None,
        }];
        let cal = load_open_predictions_calibration(&conn, &open)
            .unwrap()
            .expect("calibration row");
        assert_eq!(cal.layer.as_deref(), Some("low"));
        assert_eq!(cal.sample_size, 30);
    }

    #[test]
    fn load_conviction_trajectories_collects_history_for_held_assets() {
        let conn = open_db();
        // Two history points for BTC/low, one for BTC/macro, one for GLD/low
        // (not held). Recent dates so the 30d cutoff includes them.
        let recent_ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        for (analyst, asset, conviction) in [
            ("low", "BTC", 2i64),
            ("low", "BTC", 3i64),
            ("macro", "BTC", 4i64),
            ("low", "ZZZ", 1i64),
        ] {
            conn.execute(
                "INSERT INTO analyst_view_history
                 (analyst, asset, direction, conviction, reasoning_summary, recorded_at)
                 VALUES (?1, ?2, 'bull', ?3, '', ?4)",
                rusqlite::params![analyst, asset, conviction, recent_ts],
            )
            .unwrap();
        }
        let held = vec!["BTC".to_string()];
        let traj = load_conviction_trajectories(&conn, &held, 30).unwrap();
        // Two groups: BTC/LOW (2 points) and BTC/MACRO (1 point).
        assert_eq!(traj.len(), 2);
        let btc_low = traj
            .iter()
            .find(|r| r.symbol == "BTC" && r.layer == "LOW")
            .unwrap();
        assert_eq!(btc_low.points.len(), 2);
        let btc_macro = traj
            .iter()
            .find(|r| r.symbol == "BTC" && r.layer == "MACRO")
            .unwrap();
        assert_eq!(btc_macro.points.len(), 1);
    }

    #[test]
    fn load_public_lessons_applied_counts_recent_lesson_references() {
        let conn = open_db();
        // Two recent predictions cite lesson #1; one older prediction also cites it.
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let old = (chrono::Utc::now() - chrono::Duration::days(5))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        conn.execute(
            "INSERT INTO prediction_lessons
             (prediction_id, miss_type, what_predicted, what_happened, why_wrong, signal_misread, created_at)
             VALUES (1, 'timing', 'BTC up', 'BTC down', 'misread liquidity. fwiw.', 'low vol', '2026-05-01 00:00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_predictions
             (claim, symbol, conviction, timeframe, topic, confidence, outcome, lessons_applied, created_at)
             VALUES ('c', 'BTC', 'high', 'low', 'crypto', 0.7, 'pending', '[1]', ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_predictions
             (claim, symbol, conviction, timeframe, topic, confidence, outcome, lessons_applied, created_at)
             VALUES ('c', 'BTC', 'high', 'low', 'crypto', 0.7, 'pending', '[1]', ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        // Older prediction citing lesson 1 — outside 24h window.
        conn.execute(
            "INSERT INTO user_predictions
             (claim, symbol, conviction, timeframe, topic, confidence, outcome, lessons_applied, created_at)
             VALUES ('c', 'BTC', 'high', 'low', 'crypto', 0.7, 'pending', '[1]', ?1)",
            rusqlite::params![old],
        )
        .unwrap();
        let rows = load_public_lessons_applied(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lesson_id, "L1");
        assert!(rows[0].summary.contains("timing"));
        assert!(rows[0].applied_to.as_deref().unwrap().contains('2'));
    }

    #[test]
    fn normalize_pred_layer_maps_common_aliases() {
        assert_eq!(normalize_pred_layer("LOW").as_deref(), Some("low"));
        assert_eq!(normalize_pred_layer("medium").as_deref(), Some("medium"));
        assert_eq!(normalize_pred_layer("macro-checkpoint").as_deref(), Some("macro-checkpoint"));
        assert_eq!(normalize_pred_layer("macro").as_deref(), Some("macro"));
        assert_eq!(normalize_pred_layer("short").as_deref(), Some("low"));
        assert!(normalize_pred_layer("weird").is_none());
    }

    #[test]
    fn round1_rounds_to_one_decimal() {
        assert_eq!(round1(12.34), 12.3);
        assert_eq!(round1(12.36), 12.4);
        // Rust's `round()` rounds half away from zero.
        assert_eq!(round1(-1.25), -1.3);
    }
}
