//! P5a — constrained **walk-forward** parameter optimization with honest in-run
//! reporting (POSITIONING-MODELS.md §4 "P5" + §5 discipline).
//!
//! This is the most statistically-delicate stage: the whole point is to NOT
//! produce impressive-but-fake results. It searches ONLY the declared numeric
//! `[params]` named on the CLI (frozen topology — no inventing knobs), over a
//! rolling **warmup-aware** train/test fold scheme, selects each fold's best
//! config on the TRAIN segment ONLY, scores it out-of-sample on the TEST segment,
//! and reports the full grid with IS→OOS degradation flags and a conservative
//! verdict label.
//!
//! ## P5b (shipped) — multiple-testing overfit statistics + the ledger
//! P5b builds directly on the per-config OOS outputs this harness produces:
//! - **PBO via CSCV** over a per-config × per-slice objective matrix
//!   ([`overfit::pbo_cscv`]).
//! - **DSR** on the winner's OOS return stream, deflated for `n_configs` trials
//!   ([`overfit::deflated_sharpe`]).
//! - A **LOCKBOX** holdout (the trailing [`LOCKBOX_DAYS`]) that NO fold, CSCV
//!   slice, or DSR stream touches — reserved for a one-time final candidate check.
//! - A persistent **optimization ledger** (`db::model_optimize_runs`) keyed by a
//!   stable `topology_hash`, so cumulative trials across repeated runs are visible
//!   (the meta-overfitting guardrail). The ledger lives at the command layer
//!   (`commands/models_cmd.rs`), which owns the DB.
//!
//! The verdict is now downgraded to `overfit-likely` on `PBO > 0.25`, `DSR < 0.95`,
//! or non-positive OOS; a `robust` verdict additionally requires PBO ≤ 0.25 AND
//! DSR ≥ 0.95. The honest framing is unchanged: this is the best **observed** OOS
//! config under a FROZEN search space, never a proven edge.
//!
//! ## No-leakage architecture (the leakage-critical part)
//! 1. **Warmup is burned.** The first `WARMUP_DAYS` (~4 years) of the panel are a
//!    signal-warmup prefix; folds are constructed entirely from dates AFTER the
//!    warmup cutoff, so warmup-period returns are NEVER scored.
//! 2. **Per fold, params are chosen on TRAIN outcomes only.** The selection
//!    function [`select_best_on_train`] is handed ONLY a slice of per-config TRAIN
//!    objective values — it is type-incapable of seeing TEST data.
//! 3. **Each config's full-panel simulation is causal** (the engine decides after
//!    date T's close and fills at the NEXT tradable close — unchanged here). We
//!    only SLICE the resulting daily curve to score a window; slicing a causally
//!    produced curve cannot introduce lookahead.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use rust_decimal::prelude::ToPrimitive;

use super::engine::{simulate, DailyEquityPoint, PortfolioBacktestReport, RebalanceEvent};
use super::metrics::{self, PortfolioMetrics};
use super::overfit::{self, DsrResult, PboResult};
use super::spec::{resolve, ModelSpec};
use super::PricePanel;
use crate::research::validation::sharpe as period_sharpe;

// ---------------------------------------------------------------------------
// Tunable discipline constants (Codex review).
// ---------------------------------------------------------------------------

/// Signal warmup prefix burned before any scored fold. ~4 years: the 40-week MA
/// needs ~200 weekly bars and the cycle suites ~120 daily bars, so the FIRST
/// scored date must have ~4y of prior history for point-in-time signals.
pub const WARMUP_DAYS: i64 = 1461; // round(4 * 365.25)

/// Default rolling train span (~6 years) when `--folds` is not given.
const DEFAULT_TRAIN_DAYS: i64 = 2192; // round(6 * 365.25)
/// Default rolling test span (~1 year); step == test (non-overlapping OOS).
const DEFAULT_TEST_DAYS: i64 = 365;
/// Train:test ratio used to size folds when `--folds N` is given.
const TRAIN_TEST_RATIO: i64 = 6;

/// Refuse the search outright above these (a clear error, nonzero exit).
const MAX_K_PARAMS: usize = 6;
const MAX_N_CONFIGS: usize = 2000;
/// Warn (but proceed) inside these bands.
const WARN_K_PARAMS_LO: usize = 5;
const WARN_N_CONFIGS_LO: usize = 101;

/// LOCKBOX holdout: the most recent ~18 months of history is reserved and NEVER
/// touched by any walk-forward fold, PBO slice, or DSR stream. It exists for a
/// single, ONE-TIME final candidate check after the search space is frozen — it
/// is NOT scored in the optimize run. Round(1.5 * 365.25).
pub const LOCKBOX_DAYS: i64 = 548;

/// Default number of equal CSCV time-slices over the post-warmup, pre-lockbox
/// span (even; Codex default). Configurable via `--slices`.
pub const DEFAULT_SLICES: usize = 8;

/// PBO above this → the winner's verdict is downgraded to `overfit-likely`.
const PBO_OVERFIT_THRESHOLD: f64 = 0.25;
/// DSR below this → the winner's verdict is downgraded to `overfit-likely`.
const DSR_MIN_FOR_ROBUST: f64 = 0.95;
/// Cumulative trials for a topology above this earn a loud meta-overfit warning
/// (surfaced by the command layer, which owns the ledger).
pub const CUMULATIVE_TRIALS_WARN_THRESHOLD: u64 = 5000;

/// Honesty downgrade thresholds.
const MIN_OOS_FOLDS: usize = 4;
const MIN_OOS_REBALANCES_TOTAL: usize = 30;
/// Per-fold minimum activity (traded rebalances) for a config to be eligible for
/// winner selection and for the verdict to be confident.
const MIN_EVENTS_PER_FOLD: usize = 10;

/// A "fragile/isolated" optimum: the best adjacent grid neighbour's OOS is below
/// this fraction of the winner's OOS.
const FRAGILE_NEIGHBOUR_RATIO: f64 = 0.5;
/// An "overfit-likely" winner: OOS below this fraction of a positive IS.
const OVERFIT_OOS_RATIO: f64 = 0.5;

// ---------------------------------------------------------------------------
// Objective.
// ---------------------------------------------------------------------------

/// The selection/reporting objective. **All four are higher-is-better** and are
/// computed on the NET daily curve (the simulator already debits commission +
/// slippage to the ledger, so the curve IS net — see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Objective {
    Calmar,
    Sortino,
    Sharpe,
    Cagr,
}

impl Objective {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.trim().to_lowercase().as_str() {
            "calmar" => Objective::Calmar,
            "sortino" => Objective::Sortino,
            "sharpe" => Objective::Sharpe,
            "cagr" => Objective::Cagr,
            other => bail!("unknown --objective '{other}': expected calmar|sortino|sharpe|cagr"),
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Objective::Calmar => "calmar",
            Objective::Sortino => "sortino",
            Objective::Sharpe => "sharpe",
            Objective::Cagr => "cagr",
        }
    }

    /// Extract this objective from a computed metric block.
    pub fn of(&self, m: &PortfolioMetrics) -> f64 {
        match self {
            Objective::Calmar => m.calmar,
            Objective::Sortino => m.sortino,
            Objective::Sharpe => m.sharpe,
            Objective::Cagr => m.cagr_pct,
        }
    }
}

// ---------------------------------------------------------------------------
// Grid axes.
// ---------------------------------------------------------------------------

/// One searched grid axis: a param name + its inclusive `min:max:step` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamAxis {
    pub name: String,
    pub values: Vec<f64>,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// Parse `NAME=min:max:step` into a [`ParamAxis`] (inclusive of both endpoints).
pub fn parse_axis(spec: &str) -> Result<ParamAxis> {
    let (name, range) = spec
        .split_once('=')
        .with_context(|| format!("--param '{spec}' must be NAME=min:max:step"))?;
    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("--param '{spec}' has an empty NAME");
    }
    let parts: Vec<&str> = range.split(':').collect();
    if parts.len() != 3 {
        bail!("--param '{name}' range must be min:max:step (got '{range}')");
    }
    let parse_num = |p: &str, which: &str| -> Result<f64> {
        p.trim()
            .parse::<f64>()
            .with_context(|| format!("--param '{name}' {which} '{p}' is not a number"))
    };
    let min = parse_num(parts[0], "min")?;
    let max = parse_num(parts[1], "max")?;
    let step = parse_num(parts[2], "step")?;
    if step <= 0.0 || step.is_nan() {
        bail!("--param '{name}' step must be > 0 (got {step})");
    }
    if min > max {
        bail!("--param '{name}' min ({min}) is greater than max ({max})");
    }
    // Inclusive expansion, robust to float drift: derive a count then index.
    let count = ((max - min) / step).round() as i64;
    let mut values = Vec::with_capacity((count + 1) as usize);
    for i in 0..=count {
        let v = min + (i as f64) * step;
        // Round to 10 decimals to kill 0.30000000000004-style noise.
        values.push((v * 1e10).round() / 1e10);
    }
    Ok(ParamAxis {
        name,
        values,
        min,
        max,
        step,
    })
}

/// A single grid configuration: a concrete value per axis (sorted by name).
pub type ConfigAssignment = BTreeMap<String, f64>;

/// Cartesian product of the axes → every config to try (deterministic order).
pub fn build_grid(axes: &[ParamAxis]) -> Vec<ConfigAssignment> {
    let mut out: Vec<ConfigAssignment> = vec![BTreeMap::new()];
    for axis in axes {
        let mut next = Vec::with_capacity(out.len() * axis.values.len());
        for base in &out {
            for &v in &axis.values {
                let mut c = base.clone();
                c.insert(axis.name.clone(), v);
                next.push(c);
            }
        }
        out = next;
    }
    out
}

// ---------------------------------------------------------------------------
// Frozen-topology validation: every searched param must EXIST and be REFERENCED.
// ---------------------------------------------------------------------------

/// Is `param` referenced by any rule (in a `when` predicate or a `then` magnitude
/// field)? An unreferenced param is a knob the model ignores — searching it would
/// be theatre, so the caller rejects it.
fn param_referenced(spec: &ModelSpec, param: &str) -> bool {
    for r in &spec.rules {
        // `when` predicate: tokenise on non-identifier chars and look for the name.
        if r.when
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == param)
        {
            return true;
        }
        // `then` magnitude fields: a bare name or `-name`.
        for s in [&r.then.by, &r.then.from, &r.then.to].into_iter().flatten() {
            let body = s.trim().strip_prefix('-').unwrap_or(s.trim());
            if body == param {
                return true;
            }
        }
    }
    false
}

/// Validate the requested axes against the spec's `[params]` (frozen topology):
/// each axis name must be a declared AND referenced param. Errors otherwise.
pub fn validate_axes(spec: &ModelSpec, axes: &[ParamAxis]) -> Result<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for axis in axes {
        if !seen.insert(axis.name.as_str()) {
            bail!("--param '{}' is given more than once", axis.name);
        }
        if !spec.params.contains_key(&axis.name) {
            let known: Vec<&str> = spec.params.keys().map(|s| s.as_str()).collect();
            bail!(
                "--param '{}' is not a declared [params] knob of this model (frozen topology: known params are [{}])",
                axis.name,
                known.join(", ")
            );
        }
        if !param_referenced(spec, &axis.name) {
            bail!(
                "--param '{}' is declared but NOT referenced by any rule — refusing to search a knob the model ignores",
                axis.name
            );
        }
    }
    Ok(())
}

/// Refusal / warning gate on grid size (Codex thresholds). Returns warnings to
/// surface; errors when over the hard caps.
pub fn refusal_gate(k_params: usize, n_configs: usize) -> Result<Vec<String>> {
    if k_params > MAX_K_PARAMS {
        bail!(
            "refusing: {k_params} params exceeds the {MAX_K_PARAMS}-param cap (combinatorial-overfit risk). Search fewer knobs."
        );
    }
    if n_configs > MAX_N_CONFIGS {
        bail!(
            "refusing: {n_configs} configs exceeds the {MAX_N_CONFIGS}-config cap (multiple-testing risk). Coarsen the grid."
        );
    }
    let mut warns = Vec::new();
    if (WARN_K_PARAMS_LO..=MAX_K_PARAMS).contains(&k_params) {
        warns.push(format!(
            "{k_params} params is in the {WARN_K_PARAMS_LO}–{MAX_K_PARAMS} caution band — interaction-overfit risk rises; prefer fewer knobs."
        ));
    }
    if (WARN_N_CONFIGS_LO..=MAX_N_CONFIGS).contains(&n_configs) {
        warns.push(format!(
            "{n_configs} configs is in the {WARN_N_CONFIGS_LO}–{MAX_N_CONFIGS} caution band — the more configs tried, the larger the (uncorrected, P5b) multiple-testing inflation."
        ));
    }
    Ok(warns)
}

// ---------------------------------------------------------------------------
// Walk-forward folds.
// ---------------------------------------------------------------------------

/// One rolling train/test fold. Windows are date half-open intervals
/// `[start, end)` so a fold's TEST begins exactly where its TRAIN ends (no
/// overlap, contiguous). All four dates are strictly after the warmup cutoff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoldDef {
    pub idx: usize,
    pub train_start: NaiveDate,
    pub train_end: NaiveDate,
    pub test_start: NaiveDate,
    pub test_end: NaiveDate,
}

/// The fold scheme actually used (for honest reporting).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldScheme {
    pub warmup_days: i64,
    pub warmup_cutoff: NaiveDate,
    pub post_warmup_start: NaiveDate,
    pub data_end: NaiveDate,
    pub train_days: i64,
    pub test_days: i64,
    pub step_days: i64,
    pub folds: Vec<FoldDef>,
}

/// Build the warmup-aware rolling fold scheme over `[first_date, last_date]`.
/// `folds_opt = Some(N)` derives the train/test sizes from the post-warmup span
/// to yield ~N folds; `None` uses the ~6y train / ~1y test defaults.
pub fn build_folds(
    first_date: NaiveDate,
    last_date: NaiveDate,
    folds_opt: Option<usize>,
) -> FoldScheme {
    let warmup_cutoff = first_date + Duration::days(WARMUP_DAYS);
    let post_start = warmup_cutoff; // first scoreable date
    let post_span = (last_date - post_start).num_days().max(0);

    let (train_days, test_days) = match folds_opt {
        Some(n) if n >= 1 => {
            // train = RATIO*test, step = test; the last fold ends at
            // (n + RATIO)*test <= post_span → test = post_span / (n + RATIO).
            let denom = (n as i64) + TRAIN_TEST_RATIO;
            let test = (post_span / denom).max(0);
            (test * TRAIN_TEST_RATIO, test)
        }
        _ => (DEFAULT_TRAIN_DAYS, DEFAULT_TEST_DAYS),
    };
    let step_days = test_days.max(1);

    let mut folds = Vec::new();
    if test_days >= 1 && train_days >= 1 {
        let mut i: usize = 0;
        loop {
            let train_start = post_start + Duration::days((i as i64) * step_days);
            let train_end = train_start + Duration::days(train_days);
            let test_start = train_end;
            let test_end = test_start + Duration::days(test_days);
            if test_end > last_date + Duration::days(1) {
                break;
            }
            folds.push(FoldDef {
                idx: i,
                train_start,
                train_end,
                test_start,
                test_end,
            });
            i += 1;
            // Hard stop so a degenerate tiny step can't loop forever.
            if i > 100_000 {
                break;
            }
        }
    }

    FoldScheme {
        warmup_days: WARMUP_DAYS,
        warmup_cutoff,
        post_warmup_start: post_start,
        data_end: last_date,
        train_days,
        test_days,
        step_days,
        folds,
    }
}

// ---------------------------------------------------------------------------
// Segment scoring (slice a causal full-panel curve to a window).
// ---------------------------------------------------------------------------

/// Metrics + activity for one date window `[start, end)` of a config's full-panel
/// run. Slicing a causally produced curve cannot introduce lookahead.
#[derive(Debug, Clone)]
struct SegmentScore {
    objective: f64,
    /// Rebalance events in-window whose orders actually traded (the activity gate
    /// uses this — "the strategy must be active enough to measure").
    traded_rebalances: usize,
    /// Rebalance events in-window where >=1 rule fired (reported for transparency).
    rule_firings: usize,
    /// Number of daily points in-window (a <2 window can't form a return).
    points: usize,
    turnover_pct_per_yr: f64,
}

/// Score the window `[start, end)` of one full-panel report under `objective`.
fn score_segment(
    report: &PortfolioBacktestReport,
    start: NaiveDate,
    end: NaiveDate,
    objective: Objective,
) -> SegmentScore {
    let curve: Vec<DailyEquityPoint> = report
        .daily_equity_curve
        .iter()
        .filter(|p| p.date >= start && p.date < end)
        .cloned()
        .collect();
    let events: Vec<RebalanceEvent> = report
        .rebalance_events
        .iter()
        .filter(|e| e.date >= start && e.date < end)
        .cloned()
        .collect();
    let total_costs: Decimal = events.iter().map(|e| e.total_cost).sum();
    let traded_rebalances = events.iter().filter(|e| !e.orders.is_empty()).count();
    let rule_firings = events
        .iter()
        .filter(|e| !e.applied_rule_ids.is_empty())
        .count();
    let points = curve.len();

    let m = metrics::compute(&curve, &events, total_costs);
    SegmentScore {
        objective: objective.of(&m),
        traded_rebalances,
        rule_firings,
        points,
        turnover_pct_per_yr: m.avg_turnover_pct_per_yr,
    }
}

// ---------------------------------------------------------------------------
// Per-config / per-fold results.
// ---------------------------------------------------------------------------

/// One config's score on one fold (both IS and OOS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldScore {
    pub fold: usize,
    pub is_objective: f64,
    pub oos_objective: f64,
    pub oos_traded_rebalances: usize,
    pub oos_rule_firings: usize,
}

/// A full per-config result across every fold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub params: ConfigAssignment,
    pub per_fold: Vec<FoldScore>,
    pub mean_is: f64,
    pub mean_oos: f64,
    /// IS − OOS degradation (positive = OOS worse than IS).
    pub gap: f64,
    /// OOS / IS (when IS != 0). A small ratio with a positive IS is a red flag.
    pub ratio: f64,
    pub total_oos_rebalances: usize,
    pub min_oos_events_per_fold: usize,
    pub mean_oos_turnover_pct_per_yr: f64,
    /// Discarded from winner selection: too few traded rebalances in some fold.
    pub below_activity_gate: bool,
    /// "IS >> OOS likely-overfit": positive IS but OOS collapses below it.
    pub overfit_flag: bool,
}

/// Mean of a slice, ignoring NaNs; 0 if all NaN/empty.
fn mean_ignoring_nan(xs: &[f64]) -> f64 {
    let v: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Select the index of the best config on TRAIN. **Leakage boundary:** this
/// function is handed ONLY the per-config TRAIN objective values for one fold —
/// it is structurally incapable of seeing TEST data. Higher objective wins;
/// NaNs are skipped; deterministic first-index tie-break.
pub fn select_best_on_train(train_objectives: &[f64]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, &v) in train_objectives.iter().enumerate() {
        if !v.is_finite() {
            continue;
        }
        if best.map(|(_, bv)| v > bv).unwrap_or(true) {
            best = Some((i, v));
        }
    }
    best.map(|(i, _)| i)
}

// ---------------------------------------------------------------------------
// Verdict.
// ---------------------------------------------------------------------------

/// The conservative verdict label. Ordering of precedence:
/// `insufficient-data` > `overfit-likely` > `fragile` > `robust`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Not enough OOS folds / rebalances / activity to crown a winner confidently.
    InsufficientData,
    /// The winner's edge collapses out-of-sample (IS >> OOS).
    OverfitLikely,
    /// The winner is an isolated grid spike (adjacent neighbours much worse).
    Fragile,
    /// Survives every gate. (Still NOT "proven" — see the standing P5b caveat.)
    Robust,
}

impl Verdict {
    pub fn label(&self) -> &'static str {
        match self {
            Verdict::InsufficientData => "insufficient-data",
            Verdict::OverfitLikely => "overfit-likely",
            Verdict::Fragile => "fragile",
            Verdict::Robust => "robust",
        }
    }
}

/// A single adjacent-grid-point reading around the winner (one axis, ±1 step).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityPoint {
    pub axis: String,
    pub value: f64,
    pub mean_oos: f64,
    /// True when this neighbour exists in the grid.
    pub present: bool,
}

// ---------------------------------------------------------------------------
// The full optimize report.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizeReport {
    pub model_name: String,
    pub model_version: u32,
    pub objective: String,
    pub axes: Vec<ParamAxis>,
    pub k_params: usize,
    pub n_configs: usize,
    pub scheme: FoldScheme,
    pub configs: Vec<ConfigResult>,
    pub winner_idx: Option<usize>,
    /// Per-fold walk-forward: the config picked on TRAIN, and its realized OOS.
    /// This is the HONEST "if you optimized on train each fold" number.
    pub walk_forward: Vec<WalkForwardFold>,
    pub walk_forward_mean_oos: f64,
    /// The rebalanced-base-policy benchmark's mean OOS objective (config-free).
    pub benchmark_rebalanced_oos: f64,
    pub sensitivity: Vec<SensitivityPoint>,
    /// P5b — Probability of Backtest Overfitting (CSCV over `n_slices` slices).
    /// `None` when the pre-lockbox span is too short to slice.
    pub pbo: Option<PboResult>,
    /// P5b — Deflated Sharpe Ratio on the winner's OOS return stream, deflated
    /// for `n_configs` trials + non-normality. `None` when no winner / too short.
    pub dsr: Option<DsrResult>,
    /// P5b — the lockbox holdout reserved (and NEVER scored) by this run.
    pub lockbox: LockboxInfo,
    pub verdict: Verdict,
    pub warnings: Vec<String>,
}

/// The reserved lockbox holdout window (P5b). Folds/slices/DSR never touch it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockboxInfo {
    pub lockbox_days: i64,
    /// First date inside the lockbox (inclusive). Folds end strictly before this.
    pub lockbox_start: NaiveDate,
    /// Last date inside the lockbox (inclusive) = the data end.
    pub lockbox_end: NaiveDate,
    /// Number of equal CSCV slices over the post-warmup, PRE-lockbox span.
    pub n_slices: usize,
}

/// One fold of the walk-forward procedure: train-pick → its OOS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardFold {
    pub fold: usize,
    pub picked_config_idx: usize,
    pub picked_params: ConfigAssignment,
    pub train_objective: f64,
    pub oos_objective: f64,
}

// ---------------------------------------------------------------------------
// The driver.
// ---------------------------------------------------------------------------

/// Run the full walk-forward optimization. `base_spec` is the parsed (not yet
/// resolved) model; each config overrides the named params and re-resolves so the
/// param values fold into the compiled rules.
pub fn run_optimize(
    base_spec: &ModelSpec,
    panel: &PricePanel,
    axes: &[ParamAxis],
    objective: Objective,
    folds_opt: Option<usize>,
    slices_opt: Option<usize>,
) -> Result<OptimizeReport> {
    validate_axes(base_spec, axes)?;
    let grid = build_grid(axes);
    let k_params = axes.len();
    let n_configs = grid.len();
    let mut warnings = refusal_gate(k_params, n_configs)?;

    let calendar = panel.calendar();
    let first_date = *calendar
        .first()
        .context("empty price panel: no calendar dates to optimize over")?;
    let last_date = *calendar.last().unwrap();

    // LOCKBOX: reserve the most recent ~18 months. Folds + CSCV slices run ONLY
    // on the post-warmup span BEFORE the lockbox; the lockbox is never scored.
    let lockbox_start = last_date - Duration::days(LOCKBOX_DAYS - 1);
    // The last date folds/slices may use: strictly before the lockbox.
    let fold_last = lockbox_start - Duration::days(1);
    let scheme = build_folds(first_date, fold_last, folds_opt);

    // CSCV slices: S even equal contiguous date-windows over the post-warmup,
    // PRE-lockbox span [post_warmup_start, lockbox_start).
    let n_slices = {
        let s = slices_opt.unwrap_or(DEFAULT_SLICES);
        if s >= 2 { s } else { DEFAULT_SLICES }
    };
    let n_slices = if n_slices % 2 == 0 { n_slices } else { n_slices + 1 };
    let pre_lockbox_span = (lockbox_start - scheme.post_warmup_start).num_days().max(0);
    let slice_bounds: Vec<NaiveDate> = (0..=n_slices)
        .map(|k| {
            scheme.post_warmup_start
                + Duration::days((k as i64 * pre_lockbox_span) / n_slices as i64)
        })
        .collect();
    let slices_usable = pre_lockbox_span >= 2 * n_slices as i64;

    // Resolve the model name/version once (from a no-override resolve).
    let base_resolved =
        resolve(base_spec.clone()).context("base model spec failed to resolve")?;
    let model_name = base_resolved.name.clone();
    let model_version = base_resolved.version;

    // Simulate every config once over the full panel (causal), then slice.
    let mut configs: Vec<ConfigResult> = Vec::with_capacity(n_configs);
    // Per (config, fold): IS objective and OOS objective — the matrices the
    // walk-forward selection + reporting consume.
    let mut is_matrix: Vec<Vec<f64>> = Vec::with_capacity(n_configs); // [config][fold]
    let mut oos_matrix: Vec<Vec<f64>> = Vec::with_capacity(n_configs);
    let mut benchmark_oos_per_fold: Option<Vec<f64>> = None;
    // P5b: per-config × per-slice objective (the CSCV input) and per-config
    // concatenated OOS daily return stream (the DSR input).
    let mut slice_scores: Vec<Vec<f64>> = Vec::with_capacity(n_configs);
    let mut oos_return_streams: Vec<Vec<f64>> = Vec::with_capacity(n_configs);

    for assignment in &grid {
        let mut spec = base_spec.clone();
        for (k, v) in assignment {
            spec.params.insert(k.clone(), *v);
        }
        let rm = resolve(spec).with_context(|| {
            format!("model failed to resolve for config {assignment:?}")
        })?;
        let report = simulate(&rm.model, panel)
            .with_context(|| format!("simulate failed for config {assignment:?}"))?;

        // Benchmark (rebalanced base policy) is config-independent — compute once.
        if benchmark_oos_per_fold.is_none() {
            let bench = &report.benchmarks.rebalanced_base_policy;
            let bench_report = PortfolioBacktestReport {
                daily_equity_curve: bench.daily_equity_curve.clone(),
                rebalance_events: vec![],
                ..report.clone()
            };
            let mut bvals = Vec::with_capacity(scheme.folds.len());
            for f in &scheme.folds {
                let s = score_segment(&bench_report, f.test_start, f.test_end, objective);
                bvals.push(if s.points >= 2 { s.objective } else { f64::NAN });
            }
            benchmark_oos_per_fold = Some(bvals);
        }

        let mut per_fold = Vec::with_capacity(scheme.folds.len());
        let mut oos_turnovers = Vec::new();
        for f in &scheme.folds {
            let is = score_segment(&report, f.train_start, f.train_end, objective);
            let oos = score_segment(&report, f.test_start, f.test_end, objective);
            oos_turnovers.push(oos.turnover_pct_per_yr);
            per_fold.push(FoldScore {
                fold: f.idx,
                is_objective: if is.points >= 2 { is.objective } else { f64::NAN },
                oos_objective: if oos.points >= 2 { oos.objective } else { f64::NAN },
                oos_traded_rebalances: oos.traded_rebalances,
                oos_rule_firings: oos.rule_firings,
            });
        }
        // P5b: this config's objective on each pre-lockbox CSCV slice.
        let mut row = Vec::with_capacity(n_slices);
        if slices_usable {
            for k in 0..n_slices {
                let seg = score_segment(&report, slice_bounds[k], slice_bounds[k + 1], objective);
                row.push(if seg.points >= 2 { seg.objective } else { f64::NAN });
            }
        }
        slice_scores.push(row);

        // P5b: this config's concatenated OOS daily return stream (per fold,
        // never bridging across folds), for DSR.
        oos_return_streams.push(oos_return_stream(&report, &scheme.folds));

        let cr = aggregate_config(assignment.clone(), per_fold, &oos_turnovers);
        is_matrix.push(cr.per_fold.iter().map(|p| p.is_objective).collect());
        oos_matrix.push(cr.per_fold.iter().map(|p| p.oos_objective).collect());
        configs.push(cr);
    }

    // Walk-forward: per fold pick the best config on TRAIN ONLY, record its OOS.
    let mut walk_forward = Vec::with_capacity(scheme.folds.len());
    for (fi, f) in scheme.folds.iter().enumerate() {
        let train_objs: Vec<f64> = is_matrix.iter().map(|row| row[fi]).collect();
        if let Some(pick) = select_best_on_train(&train_objs) {
            walk_forward.push(WalkForwardFold {
                fold: f.idx,
                picked_config_idx: pick,
                picked_params: configs[pick].params.clone(),
                train_objective: is_matrix[pick][fi],
                oos_objective: oos_matrix[pick][fi],
            });
        }
    }
    let walk_forward_mean_oos =
        mean_ignoring_nan(&walk_forward.iter().map(|w| w.oos_objective).collect::<Vec<_>>());

    let benchmark_rebalanced_oos =
        mean_ignoring_nan(&benchmark_oos_per_fold.unwrap_or_default());

    // Winner = best mean-OOS among configs that pass the activity gate.
    let winner_idx = pick_winner(&configs);

    // Sensitivity around the winner (each axis, ±1 step neighbours).
    let sensitivity = winner_idx
        .map(|wi| sensitivity_around(&configs, &configs[wi].params, axes))
        .unwrap_or_default();

    // P5b — PBO via CSCV over the per-config × per-slice objective matrix.
    let pbo = if slices_usable {
        overfit::pbo_cscv(&slice_scores)
    } else {
        None
    };

    // P5b — DSR on the winner's OOS return stream, deflated for `n_configs`
    // trials. The trial-Sharpe spread is each config's per-period OOS Sharpe.
    let trial_sharpes: Vec<f64> = oos_return_streams
        .iter()
        .map(|s| period_sharpe(s).unwrap_or(f64::NAN))
        .collect();
    let dsr = winner_idx.and_then(|wi| {
        overfit::deflated_sharpe(&oos_return_streams[wi], &trial_sharpes, n_configs)
    });

    let verdict = decide_verdict(
        &scheme,
        &configs,
        winner_idx,
        &sensitivity,
        axes,
        pbo.as_ref(),
        dsr.as_ref(),
    );

    let lockbox = LockboxInfo {
        lockbox_days: LOCKBOX_DAYS,
        lockbox_start,
        lockbox_end: last_date,
        n_slices: if slices_usable { n_slices } else { 0 },
    };

    Ok(OptimizeReport {
        model_name,
        model_version,
        objective: objective.label().to_string(),
        axes: axes.to_vec(),
        k_params,
        n_configs,
        scheme,
        configs,
        winner_idx,
        walk_forward,
        walk_forward_mean_oos,
        benchmark_rebalanced_oos,
        sensitivity,
        pbo,
        dsr,
        lockbox,
        verdict,
        warnings: std::mem::take(&mut warnings),
    })
}

/// A config's OOS return stream for DSR, **sampled at the strategy's own
/// decision clock** (its rebalance dates) inside each fold's OOS test window
/// `[test_start, test_end)`.
///
/// Why rebalance-period, not daily: a weekly/monthly strategy only acts on its
/// cadence; between decisions the equity drifts on a held book. Per-DAY Sharpe of
/// such a curve is dominated by intra-period noise/flat days and badly understates
/// a genuine periodic edge. Sampling equity at consecutive rebalance dates yields
/// the strategy's realised per-decision returns — the financially correct period
/// for a Sharpe/DSR. Returns are computed within a fold only (never bridging the
/// gap between folds). Falls back to the daily curve when a fold has no rebalance
/// activity, so the stream is never empty for an active strategy.
fn oos_return_stream(report: &PortfolioBacktestReport, folds: &[FoldDef]) -> Vec<f64> {
    use std::collections::BTreeMap;
    let mut out = Vec::new();
    for f in folds {
        // Equity by date inside the OOS window.
        let by_date: BTreeMap<NaiveDate, f64> = report
            .daily_equity_curve
            .iter()
            .filter(|p| p.date >= f.test_start && p.date < f.test_end)
            .map(|p| (p.date, p.equity.to_f64().unwrap_or(0.0)))
            .collect();
        // Sampling dates = rebalance dates in-window (the strategy's clock).
        let mut sample_dates: Vec<NaiveDate> = report
            .rebalance_events
            .iter()
            .filter(|e| e.date >= f.test_start && e.date < f.test_end)
            .map(|e| e.date)
            .collect();
        sample_dates.sort();
        sample_dates.dedup();

        // Equity at each sampling date (nearest on-or-before date in-window).
        let equity_at = |d: NaiveDate| -> Option<f64> {
            by_date
                .range(..=d)
                .next_back()
                .map(|(_, &v)| v)
                .filter(|v| *v > 0.0)
        };
        let sampled: Vec<f64> = if sample_dates.len() >= 2 {
            sample_dates.iter().filter_map(|&d| equity_at(d)).collect()
        } else {
            // No rebalances in-window: fall back to the daily equity series.
            by_date.values().copied().filter(|v| *v > 0.0).collect()
        };
        for w in sampled.windows(2) {
            if w[0] > 0.0 {
                out.push(w[1] / w[0] - 1.0);
            }
        }
    }
    out
}

/// Aggregate one config's per-fold scores into a [`ConfigResult`]: means, the
/// IS→OOS degradation gap/ratio, the activity gate, and the overfit flag. Shared
/// by the driver and the honesty tests so the gate/flag logic is tested directly.
fn aggregate_config(
    params: ConfigAssignment,
    per_fold: Vec<FoldScore>,
    oos_turnovers: &[f64],
) -> ConfigResult {
    let is_row: Vec<f64> = per_fold.iter().map(|p| p.is_objective).collect();
    let oos_row: Vec<f64> = per_fold.iter().map(|p| p.oos_objective).collect();
    let total_oos_rebalances: usize = per_fold.iter().map(|p| p.oos_traded_rebalances).sum();
    let min_oos_events = per_fold
        .iter()
        .map(|p| p.oos_traded_rebalances)
        .min()
        .unwrap_or(0);
    let mean_is = mean_ignoring_nan(&is_row);
    let mean_oos = mean_ignoring_nan(&oos_row);
    let gap = mean_is - mean_oos;
    let ratio = if mean_is.abs() > f64::EPSILON {
        mean_oos / mean_is
    } else {
        f64::NAN
    };
    let below_activity_gate = per_fold.is_empty() || min_oos_events < MIN_EVENTS_PER_FOLD;
    // Overfit: a genuinely positive IS edge that collapses OOS.
    let overfit_flag = mean_is > 0.0 && mean_oos < OVERFIT_OOS_RATIO * mean_is;
    ConfigResult {
        params,
        per_fold,
        mean_is,
        mean_oos,
        gap,
        ratio,
        total_oos_rebalances,
        min_oos_events_per_fold: min_oos_events,
        mean_oos_turnover_pct_per_yr: mean_ignoring_nan(oos_turnovers),
        below_activity_gate,
        overfit_flag,
    }
}

/// Best mean-OOS config among those passing the activity gate (deterministic
/// first-index tie-break). None if no config passes.
fn pick_winner(configs: &[ConfigResult]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, c) in configs.iter().enumerate() {
        if c.below_activity_gate || !c.mean_oos.is_finite() {
            continue;
        }
        if best.map(|(_, bv)| c.mean_oos > bv).unwrap_or(true) {
            best = Some((i, c.mean_oos));
        }
    }
    best.map(|(i, _)| i)
}

/// Find the config whose params exactly match `target` (used to look up grid
/// neighbours).
fn find_config(configs: &[ConfigResult], target: &ConfigAssignment) -> Option<usize> {
    configs.iter().position(|c| {
        c.params.len() == target.len()
            && c.params
                .iter()
                .all(|(k, v)| target.get(k).map(|t| (t - v).abs() < 1e-9).unwrap_or(false))
    })
}

/// Read the ±1-step grid neighbours of the winner along each axis.
fn sensitivity_around(
    configs: &[ConfigResult],
    winner_params: &ConfigAssignment,
    axes: &[ParamAxis],
) -> Vec<SensitivityPoint> {
    let mut out = Vec::new();
    for axis in axes {
        let cur = match winner_params.get(&axis.name) {
            Some(v) => *v,
            None => continue,
        };
        // Index of the winner's value on this axis.
        let cur_idx = axis
            .values
            .iter()
            .position(|v| (v - cur).abs() < 1e-9);
        for delta in [-1i64, 1] {
            let (value, present) = match cur_idx {
                Some(ci) => {
                    let ni = ci as i64 + delta;
                    if ni >= 0 && (ni as usize) < axis.values.len() {
                        (axis.values[ni as usize], true)
                    } else {
                        (cur + delta as f64 * axis.step, false)
                    }
                }
                None => (cur + delta as f64 * axis.step, false),
            };
            let mean_oos = if present {
                let mut neighbour = winner_params.clone();
                neighbour.insert(axis.name.clone(), value);
                find_config(configs, &neighbour)
                    .map(|i| configs[i].mean_oos)
                    .unwrap_or(f64::NAN)
            } else {
                f64::NAN
            };
            out.push(SensitivityPoint {
                axis: axis.name.clone(),
                value,
                mean_oos,
                present,
            });
        }
    }
    out
}

/// Apply the verdict precedence rule. Documented in [`Verdict`].
///
/// P5b downgrade: a `robust` verdict now ADDITIONALLY requires PBO ≤ 0.25 and
/// DSR ≥ 0.95. A high PBO (selecting noise) or a low DSR (the winner's OOS edge
/// is within luck after the multiple-testing deflation) downgrades the winner to
/// `overfit-likely` even when the in-run P5a gates (gap/ratio/sensitivity)
/// looked fine.
#[allow(clippy::too_many_arguments)]
fn decide_verdict(
    scheme: &FoldScheme,
    configs: &[ConfigResult],
    winner_idx: Option<usize>,
    sensitivity: &[SensitivityPoint],
    _axes: &[ParamAxis],
    pbo: Option<&PboResult>,
    dsr: Option<&DsrResult>,
) -> Verdict {
    let n_folds = scheme.folds.len();
    let winner = match winner_idx {
        Some(i) => &configs[i],
        None => return Verdict::InsufficientData,
    };
    // 1. insufficient-data
    if n_folds < MIN_OOS_FOLDS
        || winner.total_oos_rebalances < MIN_OOS_REBALANCES_TOTAL
        || winner.min_oos_events_per_fold < MIN_EVENTS_PER_FOLD
        || !winner.mean_oos.is_finite()
    {
        return Verdict::InsufficientData;
    }
    // 2. overfit-likely: OOS non-positive, IS≫OOS collapse, OR a P5b multiple-
    //    testing failure (high PBO / low DSR). PBO/DSR only downgrade when they
    //    are COMPUTABLE — a None (too-short span / ill-conditioned stream) never
    //    fabricates a downgrade, but it does block `robust` below.
    let pbo_fail = pbo.map(|p| p.pbo > PBO_OVERFIT_THRESHOLD).unwrap_or(false);
    let dsr_fail = dsr.map(|d| d.dsr < DSR_MIN_FOR_ROBUST).unwrap_or(false);
    if winner.mean_oos <= 0.0 || winner.overfit_flag || pbo_fail || dsr_fail {
        return Verdict::OverfitLikely;
    }
    // 3. fragile/isolated: best present neighbour's OOS far below the winner's.
    let present: Vec<f64> = sensitivity
        .iter()
        .filter(|s| s.present && s.mean_oos.is_finite())
        .map(|s| s.mean_oos)
        .collect();
    if !present.is_empty() {
        let best_neighbour = present.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if best_neighbour < FRAGILE_NEIGHBOUR_RATIO * winner.mean_oos {
            return Verdict::Fragile;
        }
    }
    // 4. robust — but now ONLY if the multiple-testing clearance is actually
    //    present AND passing. A None PBO/DSR (too-short pre-lockbox span or an
    //    ill-conditioned OOS stream) cannot certify robustness, so it degrades to
    //    `fragile` rather than claiming a robust edge we never deflated.
    let pbo_ok = pbo.map(|p| p.pbo <= PBO_OVERFIT_THRESHOLD).unwrap_or(false);
    let dsr_ok = dsr.map(|d| d.dsr >= DSR_MIN_FOR_ROBUST).unwrap_or(false);
    if pbo_ok && dsr_ok {
        Verdict::Robust
    } else {
        Verdict::Fragile
    }
}

/// Synthetic, money-safe fixtures shared by this module's tests AND the
/// `models_cmd` render test. The "RISK" asset is a deterministic Shannon's-demon
/// oscillator that plants a genuine, persistent volatility-harvesting edge whose
/// optimum is `tilt_size = 0.4` both in- and out-of-sample.
#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;
    use chrono::Datelike;
    use rust_decimal::prelude::FromPrimitive;

    /// Weekday-only daily dates for `n_days` starting at `start` (skips Sat/Sun so
    /// the engine's ISO-week cadence sees a clean 5-bar week).
    fn weekdays(start: NaiveDate, n_days: usize) -> Vec<NaiveDate> {
        let mut out = Vec::with_capacity(n_days);
        let mut d = start;
        while out.len() < n_days {
            if d.weekday().num_days_from_monday() < 5 {
                out.push(d);
            }
            d += Duration::days(1);
        }
        out
    }

    /// A synthetic "RISK" asset that oscillates between `base` and `base*(1+a)`
    /// on **ISO-week** parity — perfectly mean-reverting (multiplicatively
    /// symmetric: up ×(1+a), down ×1/(1+a), so buy-and-hold is net flat), constant
    /// within each ISO week. This is Shannon's demon: weekly rebalancing to a fixed
    /// risk weight `w` HARVESTS the oscillation, with geometric growth peaking at
    /// `w* = 0.5` for ANY amplitude `a`. With base risk target 0.1 and tilt grid
    /// {0..0.8}, the OOS-optimal tilt is 0.4 (risk weight 0.5). The edge is genuine
    /// and PERSISTENT (identical process every fold) so it recovers OUT-OF-SAMPLE.
    /// Aligned to ISO weeks (Monday epoch) so the engine's weekly rebalance lands
    /// exactly on the oscillation turns.
    fn planted_risk_series(start: NaiveDate, n_days: usize, a: f64) -> Vec<(NaiveDate, Decimal)> {
        // Monday on/before 2008-01-01 (a Tuesday) → 2007-12-31.
        let epoch = NaiveDate::from_ymd_opt(2007, 12, 31).unwrap();
        let base = 100.0_f64;
        let dates = weekdays(start, n_days);
        let mut out = Vec::with_capacity(dates.len());
        for d in dates {
            let wk = (d - epoch).num_days().div_euclid(7);
            let px = if wk.rem_euclid(2) == 1 { base * (1.0 + a) } else { base };
            out.push((d, Decimal::from_f64(px).unwrap().round_dp(6)));
        }
        out
    }

    /// The planted-edge model TOML: cash + RISK, weekly to_target, a single
    /// always-firing rule that tilts `tilt_size` from cash into RISK. `tilt_size`
    /// is the only knob and it is referenced by the rule.
    pub(crate) fn planted_model_toml(commission: f64) -> String {
        format!(
            r#"
[model]
name = "planted-edge"
version = 1
base_currency = "USD"
initial_capital = 100000

[universe]
assets = [ {{ symbol = "RISK", class = "risk" }} ]
cash_class = "cash"

[base_policy]
targets = [ {{ class = "cash", target = 0.9, floor = 0.0, ceiling = 1.0 }},
            {{ class = "risk", target = 0.1, floor = 0.0, ceiling = 1.0 }} ]
within_class = "equal"

[constraints]
rebalance_cadence = "weekly"
rebalance_band_mode = "to_target"
fill = "next_close"
commission_pct = {commission}

[[rules]]
id = "tilt-into-risk"
when = "always"
then = {{ kind = "tilt", class = "risk", by = "tilt_size", from = "cash" }}
priority = 10

[params]
tilt_size = 0.1
"#
        )
    }

    pub(crate) fn planted_panel(years: f64) -> PricePanel {
        let start = NaiveDate::from_ymd_opt(2008, 1, 1).unwrap();
        let n = (years * 252.0) as usize;
        let mut panel = PricePanel::new();
        panel.insert_series("RISK", planted_risk_series(start, n, 0.20));
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::{planted_model_toml, planted_panel};
    use super::*;
    use super::super::spec::parse_str;

    /// HONESTY ORACLE 1 — recovers a planted, persistent edge OUT-OF-SAMPLE, and
    /// the P5b statistics tell the *honest* story about it.
    ///
    /// The Shannon's-demon asset is a real, persistent volatility-harvesting edge:
    /// the OOS winner is a high-tilt config that beats the rebalanced-base
    /// benchmark, and PBO is LOW (the IS winner is consistently good OOS — NOT
    /// noise). BUT the edge is a CAGR/leverage win, not a *risk-adjusted* one:
    /// every tilt level shares essentially the same (modest) per-decision Sharpe,
    /// so the Deflated Sharpe of the CAGR-winner does NOT clear 0.95. P5b
    /// therefore correctly REFUSES to stamp it `robust` — exactly the
    /// meta-honesty the stage exists to enforce (CAGR up ≠ proven Sharpe edge).
    #[test]
    fn recovers_planted_edge_out_of_sample() {
        let panel = planted_panel(11.0);
        let spec = parse_str(&planted_model_toml(0.0)).unwrap();
        let axes = vec![parse_axis("tilt_size=0.0:0.8:0.1").unwrap()];
        let rep = run_optimize(&spec, &panel, &axes, Objective::Cagr, Some(4), None).unwrap();

        assert!(rep.scheme.folds.len() >= MIN_OOS_FOLDS, "need >= {MIN_OOS_FOLDS} folds");
        let wi = rep.winner_idx.expect("a winner must be crowned");
        let win_tilt = rep.configs[wi].params["tilt_size"];
        // The harvesting edge favours a high risk weight (a large tilt out of cash).
        assert!(
            win_tilt >= 0.4 - 1e-9,
            "OOS winner must be a high-tilt harvesting config, got {win_tilt}"
        );
        // The EDGE is recovered: winner + walk-forward both beat the benchmark.
        assert!(
            rep.configs[wi].mean_oos > rep.benchmark_rebalanced_oos,
            "winner OOS must beat the rebalanced-base-policy benchmark"
        );
        assert!(rep.walk_forward_mean_oos > rep.benchmark_rebalanced_oos);

        // P5b: PBO is LOW — the IS winner is consistently strong OOS (persistent,
        // NOT selecting noise).
        let pbo = rep.pbo.as_ref().expect("PBO must be computed");
        assert!(pbo.pbo <= 0.25, "persistent edge → low PBO, got {}", pbo.pbo);

        // P5b: DSR is computed but does NOT clear 0.95 (the win is leverage, not a
        // better Sharpe), so the verdict is downgraded away from `robust`.
        let dsr = rep.dsr.as_ref().expect("DSR must be computed");
        assert!(!dsr.passes, "leverage-only edge must not pass DSR≥0.95");
        assert_ne!(rep.verdict, Verdict::InsufficientData);
        assert_ne!(rep.verdict, Verdict::Robust, "DSR<0.95 must block a robust verdict");
        assert_eq!(rep.verdict, Verdict::OverfitLikely, "DSR<0.95 → overfit-likely");
    }

    /// HONESTY ORACLE 2 — the optimizer must NOT be fooled by in-sample luck.
    /// A config that wins the TRAIN selection in EVERY fold (highest IS) but whose
    /// edge COLLAPSES out-of-sample must (a) NOT be crowned the OOS winner, and
    /// (b) trip the IS→OOS degradation (overfit) flag. Built on the real
    /// [`aggregate_config`] / [`pick_winner`] / [`select_best_on_train`] logic.
    #[test]
    fn rejects_in_sample_overfit_config() {
        // helper: a config from explicit IS/OOS per-fold vectors (4 folds), with
        // enough activity to clear the gate.
        let mk = |tilt: f64, is: [f64; 4], oos: [f64; 4]| -> ConfigResult {
            let per_fold: Vec<FoldScore> = (0..4)
                .map(|f| FoldScore {
                    fold: f,
                    is_objective: is[f],
                    oos_objective: oos[f],
                    oos_traded_rebalances: 40, // clears MIN_EVENTS_PER_FOLD
                    oos_rule_firings: 40,
                })
                .collect();
            let mut p = BTreeMap::new();
            p.insert("tilt_size".to_string(), tilt);
            aggregate_config(p, per_fold, &[10.0; 4])
        };
        // STEADY: persistent, modest OOS edge.
        let steady = mk(0.2, [5.0, 5.0, 5.0, 5.0], [5.0, 5.0, 5.0, 5.0]);
        // OVERFIT: best IS every fold, but OOS collapses negative.
        let overfit = mk(0.6, [9.0, 9.0, 9.0, 9.0], [-3.0, -3.0, -3.0, -3.0]);
        // MILD: a third reference.
        let mild = mk(0.1, [3.0, 3.0, 3.0, 3.0], [3.0, 3.0, 3.0, 3.0]);
        let configs = vec![steady, overfit, mild];

        // (a) The OOS winner is NOT the overfit config — it is STEADY (best OOS).
        let wi = pick_winner(&configs).expect("a gate-passing winner exists");
        assert_eq!(wi, 0, "winner must be STEADY, not the IS-overfit config");
        assert_ne!(wi, 1, "the overfit config must NOT be crowned");

        // (b) The overfit config trips the IS→OOS degradation flag…
        assert!(configs[1].overfit_flag, "overfit config must trip the flag");
        assert!(!configs[0].overfit_flag, "the steady winner must not");
        assert!(configs[1].gap > 0.0, "IS >> OOS gap is positive for the overfit config");

        // …and a TRAIN-greedy walk-forward WOULD have been fooled every fold:
        // select_best_on_train (which sees ONLY in-sample objectives) picks the
        // overfit config, whose realised OOS is bad — proving why the report
        // crowns best-OOS (STEADY) and flags the overfit config instead.
        for f in 0..4 {
            let train_objs: Vec<f64> = configs.iter().map(|c| c.per_fold[f].is_objective).collect();
            assert_eq!(
                select_best_on_train(&train_objs),
                Some(1),
                "train-greedy selection is fooled into the overfit config"
            );
        }
    }

    /// LEAKAGE STRUCTURE — the train-selection function is only ever passed
    /// TRAIN-segment objectives. Here the TEST-optimal config differs from the
    /// TRAIN-optimal one; `select_best_on_train` (given only the train row) must
    /// return the TRAIN-optimal index, proving it cannot peek at test data.
    #[test]
    fn train_selection_cannot_see_test_data() {
        // config 0: train best; config 1: test best. The selector sees train only.
        let train_objs = vec![9.0, 1.0];
        let _test_objs = [1.0, 9.0]; // deliberately the opposite ranking
        assert_eq!(
            select_best_on_train(&train_objs),
            Some(0),
            "selection must use TRAIN ranking, never the (opposite) test ranking"
        );
    }

    /// INSUFFICIENT-DATA — a window too short to yield >= MIN_OOS_FOLDS folds
    /// after the 4y warmup must produce the "insufficient-data" verdict and refuse
    /// to crown a confident winner.
    #[test]
    fn short_window_verdict_is_insufficient_data() {
        // 4y warmup + ~1.5y → < 4 OOS folds.
        let panel = planted_panel(5.5);
        let spec = parse_str(&planted_model_toml(0.0)).unwrap();
        let axes = vec![parse_axis("tilt_size=0.0:0.8:0.1").unwrap()];
        let rep = run_optimize(&spec, &panel, &axes, Objective::Cagr, None, None).unwrap();
        assert!(rep.scheme.folds.len() < MIN_OOS_FOLDS);
        assert_eq!(rep.verdict, Verdict::InsufficientData);
    }

    /// Net-vs-gross: a positive commission must REDUCE the winner's OOS objective
    /// vs the zero-cost run (the daily curve is net of costs — selection is on net).
    #[test]
    fn costs_reduce_net_objective() {
        let panel = planted_panel(11.0);
        let axes = vec![parse_axis("tilt_size=0.0:0.8:0.1").unwrap()];
        let gross = run_optimize(
            &parse_str(&planted_model_toml(0.0)).unwrap(),
            &panel,
            &axes,
            Objective::Cagr,
            Some(4),
            None,
        )
        .unwrap();
        let net = run_optimize(
            &parse_str(&planted_model_toml(0.002)).unwrap(),
            &panel,
            &axes,
            Objective::Cagr,
            Some(4),
            None,
        )
        .unwrap();
        // Compare the same tilt=0.4 config's mean OOS under cost vs no cost.
        let oos_at = |r: &OptimizeReport, t: f64| {
            r.configs
                .iter()
                .find(|c| (c.params["tilt_size"] - t).abs() < 1e-9)
                .unwrap()
                .mean_oos
        };
        assert!(
            oos_at(&net, 0.4) < oos_at(&gross, 0.4),
            "commission must lower the NET objective"
        );
    }

    #[test]
    fn axis_inclusive_expansion() {
        let a = parse_axis("tilt_size=0.0:0.8:0.1").unwrap();
        assert_eq!(a.values.len(), 9);
        assert!((a.values[0] - 0.0).abs() < 1e-12);
        assert!((a.values[4] - 0.4).abs() < 1e-12);
        assert!((a.values[8] - 0.8).abs() < 1e-12);
    }

    #[test]
    fn axis_rejects_bad_range() {
        assert!(parse_axis("x=1:2").is_err());
        assert!(parse_axis("x=5:1:1").is_err()); // min>max
        assert!(parse_axis("x=0:1:0").is_err()); // step 0
        assert!(parse_axis("=0:1:0.5").is_err()); // empty name
    }

    #[test]
    fn grid_is_cartesian_product() {
        let axes = vec![
            parse_axis("a=0:1:1").unwrap(),  // {0,1}
            parse_axis("b=0:2:1").unwrap(),  // {0,1,2}
        ];
        let g = build_grid(&axes);
        assert_eq!(g.len(), 6);
    }

    #[test]
    fn refusal_thresholds() {
        // k>6 → error
        assert!(refusal_gate(7, 10).is_err());
        // N>2000 → error
        assert!(refusal_gate(3, 2001).is_err());
        // within caps but in warn bands → warnings, ok
        let w = refusal_gate(5, 500).unwrap();
        assert!(w.iter().any(|s| s.contains("params")));
        assert!(w.iter().any(|s| s.contains("configs")));
        // tiny → no warnings
        assert!(refusal_gate(2, 20).unwrap().is_empty());
    }

    #[test]
    fn select_best_on_train_ignores_test_and_picks_max() {
        // The leakage boundary: only TRAIN objectives are passed in.
        let train = vec![0.1, 0.9, 0.5, f64::NAN];
        assert_eq!(select_best_on_train(&train), Some(1));
    }

    #[test]
    fn folds_burn_warmup() {
        let first = NaiveDate::from_ymd_opt(2010, 1, 1).unwrap();
        let last = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let scheme = build_folds(first, last, None);
        assert!(!scheme.folds.is_empty());
        // every scored date is strictly after the ~4y warmup cutoff
        let cutoff = first + Duration::days(WARMUP_DAYS);
        for f in &scheme.folds {
            assert!(f.train_start >= cutoff, "train_start must be post-warmup");
            assert!(f.test_start >= cutoff);
            // contiguous, non-overlapping: test begins where train ends
            assert_eq!(f.test_start, f.train_end);
        }
    }

    #[test]
    fn short_window_yields_few_or_no_folds() {
        // Only ~5y of data: after a 4y warmup there is barely 1y left → far fewer
        // than MIN_OOS_FOLDS folds.
        let first = NaiveDate::from_ymd_opt(2018, 1, 1).unwrap();
        let last = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        let scheme = build_folds(first, last, None);
        assert!(scheme.folds.len() < MIN_OOS_FOLDS);
    }

    /// LOCKBOX HOLDOUT — no fold's train OR test window may touch the reserved
    /// lockbox. The lockbox is exactly the last `LOCKBOX_DAYS` of history, and
    /// every fold's `test_end` (exclusive) is ≤ `lockbox_start`.
    #[test]
    fn folds_never_touch_the_lockbox() {
        let panel = planted_panel(12.0);
        let spec = parse_str(&planted_model_toml(0.0)).unwrap();
        let axes = vec![parse_axis("tilt_size=0.0:0.8:0.1").unwrap()];
        let rep = run_optimize(&spec, &panel, &axes, Objective::Cagr, Some(4), None).unwrap();

        // Lockbox is the trailing LOCKBOX_DAYS window ending at the data end.
        assert_eq!(rep.lockbox.lockbox_days, LOCKBOX_DAYS);
        assert_eq!(
            rep.lockbox.lockbox_end - rep.lockbox.lockbox_start,
            Duration::days(LOCKBOX_DAYS - 1)
        );
        // The data end equals the lockbox end (the lockbox sits at the very end).
        let cal = panel.calendar();
        assert_eq!(rep.lockbox.lockbox_end, *cal.last().unwrap());

        assert!(!rep.scheme.folds.is_empty(), "expected scored folds before the lockbox");
        for f in &rep.scheme.folds {
            assert!(
                f.test_end <= rep.lockbox.lockbox_start,
                "fold {} test_end {} must not enter the lockbox starting {}",
                f.idx,
                f.test_end,
                rep.lockbox.lockbox_start
            );
            assert!(f.train_start < rep.lockbox.lockbox_start);
        }
        // The fold scheme's scored span also ends strictly before the lockbox.
        assert!(rep.scheme.data_end < rep.lockbox.lockbox_start);
    }

    // -- verdict downgrade (PBO/DSR) --------------------------------------

    /// Build a clean, gate-passing winner config + a scheme with enough folds so
    /// `decide_verdict` reaches the PBO/DSR stage (used by the ladder tests).
    fn passing_winner_scheme() -> (FoldScheme, Vec<ConfigResult>) {
        let scheme = build_folds(
            NaiveDate::from_ymd_opt(2010, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            None,
        );
        assert!(scheme.folds.len() >= MIN_OOS_FOLDS);
        // A winner with a modest, persistent, positive OOS edge and ample activity.
        let n = scheme.folds.len();
        let per_fold: Vec<FoldScore> = (0..n)
            .map(|f| FoldScore {
                fold: f,
                is_objective: 5.0,
                oos_objective: 5.0,
                oos_traded_rebalances: 40,
                oos_rule_firings: 40,
            })
            .collect();
        let mut p = BTreeMap::new();
        p.insert("tilt_size".to_string(), 0.3);
        let winner = aggregate_config(p, per_fold, &vec![10.0; n]);
        assert!(!winner.below_activity_gate);
        assert!(!winner.overfit_flag);
        (scheme, vec![winner])
    }

    fn mk_pbo(pbo: f64) -> PboResult {
        PboResult {
            pbo,
            band: overfit::pbo_band(pbo).to_string(),
            s_slices: 8,
            n_splits: 70,
            n_configs: 9,
            n_overfit_splits: (pbo * 70.0) as usize,
        }
    }
    fn mk_dsr(dsr: f64) -> DsrResult {
        DsrResult {
            sharpe_oos: 0.2,
            n_trials: 9,
            t_obs: 200,
            skew: 0.0,
            kurtosis: 3.0,
            sr_star: 0.1,
            var_sr: 0.01,
            dsr,
            passes: dsr >= 0.95,
        }
    }

    /// Clean winner + low PBO + passing DSR → robust.
    #[test]
    fn verdict_robust_when_pbo_low_and_dsr_passes() {
        let (scheme, configs) = passing_winner_scheme();
        let v = decide_verdict(
            &scheme,
            &configs,
            Some(0),
            &[],
            &[],
            Some(&mk_pbo(0.05)),
            Some(&mk_dsr(0.99)),
        );
        assert_eq!(v, Verdict::Robust);
    }

    /// HIGH PBO (selecting noise) → overfit-likely, even though the in-run winner
    /// passed every P5a gate (positive persistent OOS, ample activity, no IS≫OOS
    /// collapse, no fragile neighbour).
    #[test]
    fn verdict_downgrades_on_high_pbo() {
        let (scheme, configs) = passing_winner_scheme();
        let v = decide_verdict(
            &scheme,
            &configs,
            Some(0),
            &[],
            &[],
            Some(&mk_pbo(0.40)), // > 0.25
            Some(&mk_dsr(0.99)), // DSR fine — PBO alone must downgrade
        );
        assert_eq!(v, Verdict::OverfitLikely, "PBO>0.25 → overfit-likely");
    }

    /// LOW DSR → overfit-likely (the winner's OOS edge is within luck after the
    /// multiple-testing + non-normality deflation), even with a low PBO.
    #[test]
    fn verdict_downgrades_on_low_dsr() {
        let (scheme, configs) = passing_winner_scheme();
        let v = decide_verdict(
            &scheme,
            &configs,
            Some(0),
            &[],
            &[],
            Some(&mk_pbo(0.05)),
            Some(&mk_dsr(0.50)), // < 0.95
        );
        assert_eq!(v, Verdict::OverfitLikely, "DSR<0.95 → overfit-likely");
    }

    /// Missing PBO/DSR (too-short span / ill-conditioned stream) cannot CERTIFY
    /// robustness — the verdict degrades to fragile rather than claiming a robust
    /// edge that was never deflated.
    #[test]
    fn verdict_fragile_when_stats_unavailable() {
        let (scheme, configs) = passing_winner_scheme();
        let v = decide_verdict(&scheme, &configs, Some(0), &[], &[], None, None);
        assert_eq!(v, Verdict::Fragile);
    }
}
