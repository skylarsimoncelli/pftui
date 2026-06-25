//! Reliability backtest for the cycle-bottom signal suite.
//!
//! For a given asset over its full available daily history, this measures —
//! for each of the 7 composite criteria AND the N/7 confluence at one or more
//! thresholds — how reliably the signal LEADS a verified cycle low.
//!
//! ## Method (no lookahead)
//!
//! The signal engine ([`crate::analytics::cycle_signals::cycle_bottom_signals`])
//! computes the read on the LATEST bar of whatever history it is given. We
//! exploit that to get a strictly point-in-time, no-lookahead evaluation: at
//! each evaluated bar `i` we feed the engine ONLY `history[..=i]` and read the
//! `met` flag for each criterion on that bar. Because the engine never sees a
//! bar after `i`, the read at `i` cannot change when future bars are appended.
//!
//! A criterion "fires" on the first bar where it newly becomes true after
//! having been false on the previous evaluated bar (a rising edge). Each
//! firing is then matched to the nearest VERIFIED cycle-low anchor; if that
//! match falls within ±`window_bars` it is a hit (and contributes a signed
//! lead/lag distance), otherwise it is a false positive.
//!
//! ## Honesty about small N
//!
//! There are only ~3 documented cycle lows per asset. A 3-sample hit-rate is
//! NOT robust and the payload says so via `small_n` / the `caveat` field. The
//! point of this read is to tell the operator how much to trust each signal,
//! not to manufacture confidence.
//!
//! Compute-only: nothing is persisted.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::analytics::cycle_engine::pivot_lows;
use crate::analytics::cycle_signals::{self, SignalTimeframe};
use crate::models::price::HistoryRecord;

/// Below this many verified anchors the result is flagged small-n: a hit-rate
/// computed on fewer samples than this must not be read as robust.
pub const SMALL_N_THRESHOLD: usize = 5;

/// Forward-return horizons (calendar days) reported by the expectancy backtest.
/// A signal's edge is "what does the asset do 1mo / 1q / 6mo / 1y after it
/// fires", measured against the same-horizon unconditioned baseline.
pub const FORWARD_HORIZONS_DAYS: [i64; 4] = [30, 90, 180, 365];

/// Rolling-window half-width (in DAILY bars) for the asset-agnostic
/// price-structure swing-low detector. ~90 bars ≈ a quarter on each side, so a
/// retained pivot is the lowest low in roughly a half-year window — significant
/// enough to stand in for a cycle low when no doctrine anchor exists.
pub const PRICE_LOW_PIVOT_WINDOW: usize = 90;

/// Minimum post-low recovery (percent) for a detected swing low to be retained
/// as a price-structure anchor. A genuine cycle low is followed by a
/// meaningful rally; a 20% bounce filters out shelves and minor pullbacks.
/// NOTE (epistemics): price-derived anchors are WEAKER ground truth than the
/// doctrine anchors (BTC 4y / gold ~6.9y) — they are mechanically the lowest
/// low in a window with a rally out, not an externally verified cycle low. They
/// exist so the expectancy read works for an ARBITRARY symbol with enough
/// history; treat their hit-rates as directional, not authoritative.
pub const PRICE_LOW_PROMINENCE_PCT: i64 = 20;

/// Default match window (in DAILY bars) around a verified low within which a
/// firing counts as a hit. ±90 calendar days ≈ one quarter — a cycle-bottom
/// confirmation that lands within a quarter of the verified low is "on it".
pub const DEFAULT_WINDOW_BARS: i64 = 90;

/// Default confluence thresholds to report (N-of-7).
pub const DEFAULT_CONFLUENCE_THRESHOLDS: [usize; 3] = [3, 4, 5];

/// Trailing window (calendar days) for the local log-price drift estimate used
/// by the `--detrend` expectancy mode.
///
/// WHY 365: one year is long enough to average a sub-annual cycle's swings into
/// a representative secular slope, yet short enough to stay TIME-LOCAL — it
/// tracks the *contemporaneous* drift regime, so a 2013-era firing is detrended
/// against 2013-era drift and a 2024-era firing against 2024-era drift, instead
/// of subtracting one global all-history mean (which the existing
/// `lift_vs_baseline_pct` already does and which the evaluation flagged as
/// insufficient for a NON-stationary trend). A bar lacking a full year of
/// trailing history has no drift estimate and is dropped from the detrended
/// path (this reduces the detrended sample size vs raw — by design, documented).
pub const DETREND_TRAILING_DAYS: i64 = 365;

/// Decimal places the per-bar drift return % is rounded to BEFORE it is
/// subtracted from the raw return.
///
/// DETERMINISM: the drift estimate is the ONLY place `ln`/`exp` (libm f64
/// transcendentals, which are NOT IEEE-754 correctly-rounded and may differ in
/// the last ulp across libm versions/platforms) enter the pipeline. Their
/// cross-platform variation is sub-ulp (~1e-15 relative). Rounding the drift %
/// to 6 dp (1e-6) — far finer than any quoted percentage — absorbs that noise
/// so the stored EXCESS return (raw − drift, both Decimal) is reproducible given
/// the same rounded drift. Residual risk: a value landing exactly on a 6-dp
/// half-way boundary could round either way across libm versions; that is
/// astronomically unlikely on real return data and is noted honestly rather than
/// hidden. All downstream means/medians/lift/stdev stay pure Decimal.
const DRIFT_DP: u32 = 6;

/// Minimum firing count behind an expectancy row for its forward-return /
/// lift stats to be read as anything more than directional. Below this the row
/// is flagged `low_firings` and the renderer appends a "too few firings" marker
/// so a seductive lift value (e.g. "365d lift +281.9" on 11 firings) cannot be
/// mistaken for a probability. 20 is a deliberately conservative floor: it is
/// the smallest sample at which a per-horizon mean / positive-rate begins to
/// have any standard-error worth quoting (a binomial rate on n<20 has a
/// half-width wider than ±10 points), and it is keyed to the *per-row firing
/// count* — independent of the verified-ANCHOR count that drives `small_n`,
/// because a row can have plenty of anchors yet only a handful of firings.
pub const MIN_SIGNIFICANT_FIRINGS: usize = 20;

/// Evaluation cadence in DAILY bars. Daily signals must be sampled every bar
/// so one-day rising edges cannot disappear between evaluations. Weekly and
/// monthly signals can use a weekly cadence because their underlying bars are
/// aggregated from daily history and the backtest is already matching broad
/// cycle-low windows.
pub fn eval_stride_days(timeframe: SignalTimeframe) -> usize {
    match timeframe {
        SignalTimeframe::Daily => 1,
        SignalTimeframe::Weekly | SignalTimeframe::Monthly => 7,
    }
}

/// One matched firing of a criterion (or confluence threshold).
#[derive(Debug, Clone, Serialize)]
pub struct Firing {
    /// Date of the bar on which the criterion newly became true.
    pub fired_on: String,
    /// Matched verified-low date, when a low fell within the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_low: Option<String>,
    /// Signed bar (calendar-day) distance fired→low. Negative = the signal
    /// LED the low (fired before it); positive = lagged. `None` when no match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_lag_days: Option<i64>,
    /// Whether this firing landed within the window of a verified low.
    pub hit: bool,
}

/// Reliability statistics for one criterion (or confluence threshold).
#[derive(Debug, Clone, Serialize)]
pub struct CriterionReliability {
    /// Stable machine key (criterion key, or e.g. `confluence_ge_3`).
    pub key: String,
    /// Numeric confluence threshold (N of 7) for confluence rows; `None` for
    /// per-criterion rows. Lets agents read the threshold without string-parsing
    /// the `confluence_ge_<N>` key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<usize>,
    /// Human label (no practitioner names).
    pub label: String,
    /// Total firings (rising edges) over history.
    pub firings: usize,
    /// Firings that landed within the window of a verified low.
    pub hits: usize,
    /// Firings with no nearby verified low.
    pub false_positives: usize,
    /// hits / firings — fraction of firings that were near a real low.
    pub precision: Option<f64>,
    /// Distinct verified lows this criterion flagged in-window.
    pub lows_covered: usize,
    /// lows_covered / total_anchors — fraction of known lows caught.
    pub coverage: Option<f64>,
    /// Median signed lead/lag over the HITS, in days (negative = leads).
    pub median_lead_lag_days: Option<i64>,
    /// Min / max signed lead/lag over the hits (distribution edges).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_lag_min_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead_lag_max_days: Option<i64>,
    /// Plain-language one-liner.
    pub summary: String,
    /// Every matched firing (ordered by date).
    pub firing_detail: Vec<Firing>,
}

/// Full reliability backtest result.
#[derive(Debug, Clone, Serialize)]
pub struct CycleSignalBacktest {
    pub symbol: String,
    pub series: String,
    pub timeframe: SignalTimeframe,
    pub as_of: String,
    /// Daily history depth used.
    pub bars: usize,
    /// Match window in days (±).
    pub window_days: i64,
    /// Evaluation cadence in daily bars. Daily timeframe evaluates every bar;
    /// weekly/monthly evaluate weekly to keep the historical point-in-time
    /// backtest bounded.
    pub eval_stride_days: usize,
    /// Verified cycle-low anchor dates used as ground truth (price-minimum
    /// resolved within the documented window).
    pub anchors: Vec<String>,
    /// Documented anchor dates that could not be verified against the series.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unverified_anchors: Vec<String>,
    /// True when the verified-anchor count is below `SMALL_N_THRESHOLD`.
    pub small_n: bool,
    /// Per-criterion reliability (7 criteria), then confluence thresholds.
    pub criteria: Vec<CriterionReliability>,
    /// Confluence-threshold reliability rows (N-of-7).
    pub confluence: Vec<CriterionReliability>,
    /// Headline: most reliable leading criteria + what confluence buys.
    pub headline: String,
    /// Small-n / trust caveat (always present).
    pub caveat: String,
    /// Forward-return expectancy block. Populated only when the backtest is run
    /// with expectancy enabled (the `--expectancy` CLI flag). `None` keeps the
    /// legacy reliability-only payload byte-for-byte unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expectancy: Option<CycleSignalExpectancy>,
}

/// Forward-return summary at one horizon for one signal (or the baseline).
#[derive(Debug, Clone, Serialize)]
pub struct HorizonReturn {
    /// Horizon in calendar days (30/90/180/365).
    pub horizon_days: i64,
    /// Number of firings that had at least `horizon_days` of future history.
    pub samples: usize,
    /// Mean forward return over the samples, in percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_return_pct: Option<Decimal>,
    /// Median forward return over the samples, in percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_return_pct: Option<Decimal>,
    /// Fraction of samples with a strictly positive forward return, in percent
    /// (the hit-rate of the trade thesis at this horizon).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positive_rate_pct: Option<Decimal>,
    /// Fraction of samples with a strictly NEGATIVE forward return, in percent.
    /// Only populated for the cycle-TOP backtest, where a good top signal
    /// precedes a DECLINE — so `negative_rate_pct` is the top thesis hit-rate
    /// (the mirror of `positive_rate_pct` for the bottom backtest). `None` (and
    /// omitted from JSON) on the bottom path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_rate_pct: Option<Decimal>,
    /// Unconditioned baseline mean forward return at this horizon (every
    /// evaluated bar), in percent — the bar to beat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_mean_return_pct: Option<Decimal>,
    /// Expectancy LIFT: `mean_return_pct - baseline_mean_return_pct`, in percent.
    /// Positive means firing on this signal beat buying a random bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift_vs_baseline_pct: Option<Decimal>,
    /// POPULATION standard deviation of this signal's forward returns over
    /// `samples`, in percent — the noise behind `mean_return_pct`. `None` when
    /// `samples < 2` (dispersion undefined). See [`stdev_pct`] for the exact
    /// definition and the deterministic sqrt note. Additive honesty field,
    /// omitted from JSON when absent so the legacy payload is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdev_return_pct: Option<Decimal>,
    /// ROUGH standardized effect size: `lift_vs_baseline_pct` divided by the
    /// SAME-horizon BASELINE `stdev_return_pct` — i.e. how many baseline-return
    /// standard deviations the signal shifts the forward-return mean. Drift- and
    /// scale-aware: a +67 lift against a 180%-std baseline is ~0.37σ (modest),
    /// exposing that the raw lift is small relative to noise. `None` when the
    /// baseline stdev is missing or zero. DIRECTIONAL context only — this is NOT
    /// a significance test and implies no p-value. Additive field, omitted when
    /// absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect_size: Option<Decimal>,
}

/// Unconditioned baseline forward-return at one horizon (every evaluated bar).
#[derive(Debug, Clone, Serialize)]
pub struct HorizonBaseline {
    pub horizon_days: i64,
    pub samples: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_return_pct: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_return_pct: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positive_rate_pct: Option<Decimal>,
    /// POPULATION standard deviation of the unconditioned baseline forward
    /// returns over `samples`, in percent — the dispersion scale against which
    /// signal `effect_size` is measured. `None` when `samples < 2`. Additive
    /// field, omitted from JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdev_return_pct: Option<Decimal>,
}

/// Closeness of a signal's firings to the nearest price-structure low: how near
/// the actual extreme, in BOTH days and price-percent.
#[derive(Debug, Clone, Serialize)]
pub struct ClosenessStats {
    /// Firings that matched a price-structure low within the match window.
    pub matched_firings: usize,
    /// Total firings (denominator for `confidence`).
    pub firings: usize,
    /// Median signed lead/lag in days over the matched firings (negative =
    /// fired BEFORE the low).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_lead_lag_days: Option<i64>,
    /// Median signed price-percent gap between the firing price and the matched
    /// low's price: `(fire_price - low_price) / low_price * 100`. Positive =
    /// fired ABOVE the low.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_price_gap_pct: Option<Decimal>,
    /// matched_firings / firings, in percent — the firing's hit-rate / accuracy
    /// against price-structure lows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_pct: Option<Decimal>,
}

/// Forward-return expectancy + closeness for one signal (criterion or
/// confluence threshold).
#[derive(Debug, Clone, Serialize)]
pub struct ExpectancyRow {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<usize>,
    pub label: String,
    pub firings: usize,
    /// True when this row's firing count is below [`MIN_SIGNIFICANT_FIRINGS`] —
    /// its forward-return / lift numbers are then directional only, NOT a
    /// probability. Additive honesty flag; keyed to the per-row firing count,
    /// independent of the anchor-count-driven `small_n`. Omitted from JSON when
    /// false so the legacy payload is unchanged for well-sampled rows.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub low_firings: bool,
    /// Forward-return expectancy at each horizon.
    pub horizons: Vec<HorizonReturn>,
    /// Closeness to the nearest price-structure low.
    pub closeness: ClosenessStats,
}

/// Asset-agnostic forward-return expectancy backtest.
///
/// Conditions forward returns on confluence (and on each criterion) using
/// price-structure swing lows as anchors — so it works for ANY symbol with
/// enough history, not just BTC/gold. Doctrine anchors, when they exist, are
/// merged into the anchor set (stronger ground truth) but are not required.
#[derive(Debug, Clone, Serialize)]
pub struct CycleSignalExpectancy {
    /// Price-structure swing anchor dates (asset-agnostic, prominence-filtered).
    /// Polarity-neutral: carries swing LOWS on the cycle-bottom path and swing
    /// HIGHS on the cycle-top path (the render labels them per-polarity).
    pub price_structure_anchors: Vec<String>,
    /// Pivot half-width (daily bars) used for the swing-low scan.
    pub price_low_pivot_window: usize,
    /// Minimum post-low recovery (percent) required to retain a swing low.
    pub price_low_prominence_pct: Decimal,
    /// Whether doctrine anchors (BTC/gold) were merged into the anchor set.
    pub doctrine_anchors_used: bool,
    /// Total anchors used for closeness matching (price-structure ∪ doctrine).
    pub anchors_used: usize,
    /// True when no anchors at all could be derived (closeness unmeasurable).
    pub insufficient_anchors: bool,
    /// True when the anchor count is below `SMALL_N_THRESHOLD`.
    pub small_n: bool,
    /// True when forward returns in this block are DRIFT-DETRENDED: every
    /// per-firing AND baseline return is reported as EXCESS over the asset's
    /// contemporaneous trailing-window local drift (`--detrend` mode), not the
    /// raw price change. The per-horizon stat field NAMES are unchanged — this
    /// flag tells the reader how to interpret them. Additive honesty flag,
    /// omitted from JSON when false so the default (raw) payload is byte-for-byte
    /// unchanged.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub detrended: bool,
    /// Trailing window (calendar days) used for the local-drift estimate when
    /// `detrended` is true ([`DETREND_TRAILING_DAYS`]). `None` (omitted) in raw
    /// mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detrend_trailing_days: Option<i64>,
    /// Unconditioned baseline forward returns (every evaluated bar) per horizon.
    pub baseline: Vec<HorizonBaseline>,
    /// Per-criterion expectancy (7 rows).
    pub criteria: Vec<ExpectancyRow>,
    /// Per-confluence-threshold expectancy.
    pub confluence: Vec<ExpectancyRow>,
    /// Honest trust caveat.
    pub caveat: String,
}

/// Resolve a documented anchor date to the verified price-minimum date within
/// the documented window. Mirrors `cycle_clock::verify_anchor`'s policy.
fn verify_anchor_date(
    history: &[HistoryRecord],
    documented: &str,
    window_days: i64,
) -> Option<String> {
    let doc = NaiveDate::parse_from_str(documented, "%Y-%m-%d").ok()?;
    let lo = doc - chrono::Duration::days(window_days);
    let hi = doc + chrono::Duration::days(window_days);
    let mut min: Option<(NaiveDate, rust_decimal::Decimal)> = None;
    for r in history {
        let Ok(d) = NaiveDate::parse_from_str(&r.date, "%Y-%m-%d") else {
            continue;
        };
        if d < lo || d > hi {
            continue;
        }
        if min.map(|(_, c)| r.close < c).unwrap_or(true) {
            min = Some((d, r.close));
        }
    }
    min.map(|(d, _)| d.format("%Y-%m-%d").to_string())
}

/// Documented cycle-low anchors for a series. BTC and gold/silver have
/// runtime-verified doctrine anchors; other assets have none (and the backtest
/// degrades to `insufficient_anchors`).
fn documented_anchors(series: &str) -> Vec<&'static str> {
    let s = series.to_uppercase();
    if s.starts_with("BTC") {
        crate::analytics::cycle_engine::BTC_DOCUMENTED_4Y_LOWS.to_vec()
    } else if s.starts_with("GC=F") || s.starts_with("GOLD") || s.starts_with("SI=F") {
        crate::analytics::cycle_clock::GOLD_DOCUMENTED_CYCLE_LOWS.to_vec()
    } else {
        Vec::new()
    }
}

fn parse(d: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
}

/// Match a firing date to the nearest verified-low date, returning
/// `(matched_low, signed_days)` when within `window_days`.
fn match_firing(fired_on: &str, anchors: &[NaiveDate], window_days: i64) -> Option<(String, i64)> {
    let f = parse(fired_on)?;
    let mut best: Option<(NaiveDate, i64)> = None;
    for &a in anchors {
        let signed = (f - a).num_days(); // negative = fired before low (leads)
        if signed.abs() <= window_days && best.map(|(_, b)| signed.abs() < b.abs()).unwrap_or(true)
        {
            best = Some((a, signed));
        }
    }
    best.map(|(a, s)| (a.format("%Y-%m-%d").to_string(), s))
}

fn median(mut v: Vec<i64>) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        Some(v[mid])
    } else {
        // average of the two middle values, rounded toward zero
        Some((v[mid - 1] + v[mid]) / 2)
    }
}

fn friendly_days(d: i64) -> String {
    let mag = d.abs();
    let when = if d < 0 {
        "before"
    } else if d > 0 {
        "after"
    } else {
        "at"
    };
    if mag == 0 {
        "at the low".to_string()
    } else {
        format!("{mag}d {when} the low")
    }
}

/// Build per-firing stats into a [`CriterionReliability`] row.
fn build_reliability(
    key: &str,
    label: &str,
    firings: Vec<Firing>,
    total_anchors: usize,
) -> CriterionReliability {
    let n = firings.len();

    // No verified anchors to grade against (e.g. the cycle-TOP path, which has
    // no doctrine top-anchors, or any non-doctrine series). The firing COUNT is
    // still real, but hits / false_positives / precision are UNMEASURABLE: with
    // an empty anchor set every firing trivially "misses", which would report
    // e.g. `false_positives: 29` and read as a real failure rate. Null those
    // fields out instead and say so in the summary; the loud `insufficient_anchors`
    // caveat carries the rest. (The bottom path with doctrine anchors —
    // total_anchors > 0 — is unchanged.)
    if total_anchors == 0 {
        let summary = if n == 0 {
            "never fired over the available history".to_string()
        } else {
            format!("{n} firings · reliability unmeasurable (no verified anchors)")
        };
        return CriterionReliability {
            key: key.to_string(),
            threshold: None,
            label: label.to_string(),
            firings: n,
            hits: 0,
            false_positives: 0,
            precision: None,
            lows_covered: 0,
            coverage: None,
            median_lead_lag_days: None,
            lead_lag_min_days: None,
            lead_lag_max_days: None,
            summary,
            firing_detail: firings,
        };
    }

    let hits = firings.iter().filter(|f| f.hit).count();
    let false_positives = n - hits;
    let precision = if n > 0 {
        Some(hits as f64 / n as f64)
    } else {
        None
    };
    let lows: BTreeSet<String> = firings
        .iter()
        .filter_map(|f| f.matched_low.clone())
        .collect();
    let lows_covered = lows.len();
    let coverage = if total_anchors > 0 {
        Some(lows_covered as f64 / total_anchors as f64)
    } else {
        None
    };
    let lead_lags: Vec<i64> = firings.iter().filter_map(|f| f.lead_lag_days).collect();
    let median_lead_lag_days = median(lead_lags.clone());
    let lead_lag_min_days = lead_lags.iter().copied().min();
    let lead_lag_max_days = lead_lags.iter().copied().max();

    let summary = if n == 0 {
        "never fired over the available history".to_string()
    } else {
        let prec = precision
            .map(|p| format!("{:.0}%", p * 100.0))
            .unwrap_or_default();
        let cov = coverage
            .map(|c| format!("{:.0}%", c * 100.0))
            .unwrap_or_default();
        let lead = match median_lead_lag_days {
            Some(d) => format!("median {}", friendly_days(d)),
            None => "no in-window hits".to_string(),
        };
        format!(
            "{n} firings · {hits} hit / {false_positives} false · precision {prec} · coverage {cov} · {lead}"
        )
    };

    CriterionReliability {
        key: key.to_string(),
        threshold: None,
        label: label.to_string(),
        firings: n,
        hits,
        false_positives,
        precision,
        lows_covered,
        coverage,
        median_lead_lag_days,
        lead_lag_min_days,
        lead_lag_max_days,
        summary,
        firing_detail: firings,
    }
}

/// Run the reliability backtest. `window_days` is the ± match window; `None`
/// uses [`DEFAULT_WINDOW_BARS`]. Returns `None` only when history is too
/// shallow for even a single engine read.
#[allow(clippy::too_many_arguments)]
pub fn run_backtest(
    symbol: &str,
    series: &str,
    history: &[HistoryRecord],
    timeframe: SignalTimeframe,
    window_days: Option<i64>,
    thresholds: &[usize],
    with_expectancy: bool,
    detrend: bool,
) -> Option<CycleSignalBacktest> {
    let window_days = window_days.unwrap_or(DEFAULT_WINDOW_BARS).max(1);
    if history.len() < cycle_signals::min_daily_bars() {
        return None;
    }
    let as_of = history.last()?.date.clone();
    let eval_stride_days = eval_stride_days(timeframe);

    // --- Resolve verified anchors (ground truth) ---
    let documented = documented_anchors(series);
    let mut anchors: Vec<String> = Vec::new();
    let mut unverified: Vec<String> = Vec::new();
    for d in &documented {
        match verify_anchor_date(history, d, 270) {
            Some(v) => {
                if !anchors.contains(&v) {
                    anchors.push(v);
                }
            }
            None => unverified.push((*d).to_string()),
        }
    }
    anchors.sort();
    let anchor_dates: Vec<NaiveDate> = anchors.iter().filter_map(|d| parse(d)).collect();
    let total_anchors = anchor_dates.len();
    let small_n = total_anchors < SMALL_N_THRESHOLD;

    // --- Rolling, point-in-time evaluation (no lookahead) ---
    // For each evaluated bar i we read the engine on history[..=i] only.
    // We track per-criterion previous state to detect rising edges (firings),
    // plus the confluence count for the threshold rows.
    let n_criteria = 7usize;
    let mut prev_met: Vec<bool> = vec![false; n_criteria];
    let mut prev_count: usize = 0;
    let mut have_prev = false;
    // criterion key/label captured from the first successful read.
    let mut keys_labels: Vec<(String, String)> = Vec::new();
    let mut crit_firings: Vec<Vec<Firing>> = vec![Vec::new(); n_criteria];
    let mut conf_firings: Vec<Vec<Firing>> = thresholds.iter().map(|_| Vec::new()).collect();
    // Expectancy bookkeeping (only used when `with_expectancy`): the daily-bar
    // INDEX of every firing per signal, plus every evaluated bar (baseline).
    let mut crit_fire_idx: Vec<Vec<usize>> = vec![Vec::new(); n_criteria];
    let mut conf_fire_idx: Vec<Vec<usize>> = thresholds.iter().map(|_| Vec::new()).collect();
    let mut eval_idx: Vec<usize> = Vec::new();

    let start = cycle_signals::min_daily_bars().saturating_sub(1);
    let mut i = start;
    while i < history.len() {
        if let Some(read) = cycle_signals::cycle_bottom_signals(symbol, &history[..=i], timeframe) {
            if keys_labels.is_empty() {
                keys_labels = read
                    .criteria
                    .iter()
                    .map(|c| (c.key.clone(), c.label.clone()))
                    .collect();
            }
            let fired_on = read.as_of.clone();
            let cur_met: Vec<bool> = read.criteria.iter().map(|c| c.met).collect();
            let cur_count = read.met_count;
            if with_expectancy {
                eval_idx.push(i);
            }

            if have_prev {
                for (ci, &met) in cur_met.iter().enumerate().take(n_criteria) {
                    if met && !prev_met[ci] {
                        let m = match_firing(&fired_on, &anchor_dates, window_days);
                        let hit = m.is_some();
                        crit_firings[ci].push(Firing {
                            fired_on: fired_on.clone(),
                            matched_low: m.as_ref().map(|(l, _)| l.clone()),
                            lead_lag_days: m.as_ref().map(|(_, d)| *d),
                            hit,
                        });
                        if with_expectancy {
                            crit_fire_idx[ci].push(i);
                        }
                    }
                }
                for (ti, &thr) in thresholds.iter().enumerate() {
                    let now_at = cur_count >= thr;
                    let was_at = prev_count >= thr;
                    if now_at && !was_at {
                        let m = match_firing(&fired_on, &anchor_dates, window_days);
                        let hit = m.is_some();
                        conf_firings[ti].push(Firing {
                            fired_on: fired_on.clone(),
                            matched_low: m.as_ref().map(|(l, _)| l.clone()),
                            lead_lag_days: m.as_ref().map(|(_, d)| *d),
                            hit,
                        });
                        if with_expectancy {
                            conf_fire_idx[ti].push(i);
                        }
                    }
                }
            }
            prev_met = cur_met;
            prev_count = cur_count;
            have_prev = true;
        }
        if i + eval_stride_days >= history.len() && i + 1 < history.len() {
            // ensure the final bar is always evaluated
            i = history.len() - 1;
        } else {
            i += eval_stride_days;
        }
    }

    if keys_labels.is_empty() {
        return None;
    }

    // --- Assemble per-criterion + confluence reliability rows ---
    let criteria: Vec<CriterionReliability> = keys_labels
        .iter()
        .enumerate()
        .map(|(ci, (k, l))| {
            build_reliability(k, l, std::mem::take(&mut crit_firings[ci]), total_anchors)
        })
        .collect();

    let confluence: Vec<CriterionReliability> = thresholds
        .iter()
        .enumerate()
        .map(|(ti, &thr)| {
            let mut row = build_reliability(
                &format!("confluence_ge_{thr}"),
                &format!("Confluence ≥{thr}/7 criteria firing"),
                std::mem::take(&mut conf_firings[ti]),
                total_anchors,
            );
            row.threshold = Some(thr);
            row
        })
        .collect();

    let headline = build_headline(&criteria, &confluence, total_anchors);
    let caveat = build_caveat(total_anchors, small_n, window_days);

    // --- Forward-return expectancy (asset-agnostic) ---
    let expectancy = if with_expectancy {
        // Price-structure anchors: derived purely from OHLC (no circularity).
        let price_lows =
            price_structure_lows(history, PRICE_LOW_PIVOT_WINDOW, PRICE_LOW_PROMINENCE_PCT);
        // Doctrine anchors carried over from the verified set (date, close).
        let doctrine_anchors: Vec<(NaiveDate, Decimal)> = anchor_dates
            .iter()
            .filter_map(|&d| price_at_date(history, d).map(|p| (d, p)))
            .collect();
        let crit_idx: Vec<(String, String, Vec<usize>)> = keys_labels
            .iter()
            .enumerate()
            .map(|(ci, (k, l))| (k.clone(), l.clone(), std::mem::take(&mut crit_fire_idx[ci])))
            .collect();
        let conf_idx: Vec<(usize, Vec<usize>)> = thresholds
            .iter()
            .enumerate()
            .map(|(ti, &thr)| (thr, std::mem::take(&mut conf_fire_idx[ti])))
            .collect();
        Some(build_expectancy(
            history,
            &eval_idx,
            &crit_idx,
            &conf_idx,
            &price_lows,
            &doctrine_anchors,
            window_days,
            detrend,
        ))
    } else {
        None
    };

    Some(CycleSignalBacktest {
        symbol: symbol.to_string(),
        series: series.to_string(),
        timeframe,
        as_of,
        bars: history.len(),
        window_days,
        eval_stride_days,
        anchors,
        unverified_anchors: unverified,
        small_n,
        criteria,
        confluence,
        headline,
        caveat,
        expectancy,
    })
}

/// Rank criteria by reliability as leading indicators and describe what
/// confluence buys over the best single criterion.
fn build_headline(
    criteria: &[CriterionReliability],
    confluence: &[CriterionReliability],
    total_anchors: usize,
) -> String {
    if total_anchors == 0 {
        return "no verified cycle-low anchors for this series — reliability cannot be measured"
            .to_string();
    }
    // Rank by precision (then coverage) among criteria that fired at least once
    // and lead the low (median lead/lag <= window, i.e. any hit).
    let mut ranked: Vec<&CriterionReliability> = criteria.iter().filter(|c| c.hits > 0).collect();
    ranked.sort_by(|a, b| {
        let pa = a.precision.unwrap_or(0.0);
        let pb = b.precision.unwrap_or(0.0);
        pb.partial_cmp(&pa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.coverage
                    .unwrap_or(0.0)
                    .partial_cmp(&a.coverage.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let best = ranked.first();
    let best_conf = confluence.iter().filter(|c| c.hits > 0).max_by(|a, b| {
        a.precision
            .unwrap_or(0.0)
            .partial_cmp(&b.precision.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let lead = |c: &CriterionReliability| -> String {
        match c.median_lead_lag_days {
            Some(d) => friendly_days(d),
            None => "timing n/a".to_string(),
        }
    };

    match (best, best_conf) {
        (Some(b), Some(cf)) => format!(
            "Most reliable single criterion: {} (precision {:.0}%, coverage {:.0}%, {}). \
             Best confluence: {} (precision {:.0}%, coverage {:.0}%) — confluence trades firings for \
             precision vs any single criterion.",
            b.label,
            b.precision.unwrap_or(0.0) * 100.0,
            b.coverage.unwrap_or(0.0) * 100.0,
            lead(b),
            cf.label,
            cf.precision.unwrap_or(0.0) * 100.0,
            cf.coverage.unwrap_or(0.0) * 100.0,
        ),
        (Some(b), None) => format!(
            "Most reliable single criterion: {} (precision {:.0}%, coverage {:.0}%, {}). \
             No confluence threshold fired near a verified low.",
            b.label,
            b.precision.unwrap_or(0.0) * 100.0,
            b.coverage.unwrap_or(0.0) * 100.0,
            lead(b),
        ),
        _ => "no criterion fired within the match window of a verified cycle low over the \
              available history".to_string(),
    }
}

fn build_caveat(total_anchors: usize, small_n: bool, window_days: i64) -> String {
    if total_anchors == 0 {
        return "insufficient_anchors: this series has no documented cycle-low anchors to \
                measure against — treat every number here as unverified."
            .to_string();
    }
    let base = format!(
        "Hit-rates are measured against {total_anchors} verified cycle low(s) with a ±{window_days}-day \
         match window."
    );
    if small_n {
        format!(
            "{base} small_n: with only {total_anchors} anchor(s) these rates are NOT statistically \
             robust — read them as directional, not as probabilities. A single coincidence moves \
             a 3-sample hit-rate by 33 points."
        )
    } else {
        base
    }
}

/// Run the cycle-TOP reliability + (optional) forward-return expectancy
/// backtest — the symmetric mirror of [`run_backtest`]. The signal read at each
/// point-in-time bar is [`cycle_signals::cycle_top_signals`] over
/// `history[..=i]` (no lookahead). There are NO documented doctrine TOP anchors
/// (the BTC/gold doctrine anchors are cycle LOWS), so the reliability section's
/// verified-anchor set is always empty and the real grading happens in the
/// expectancy block against asset-agnostic price-structure swing HIGHS.
#[allow(clippy::too_many_arguments)]
pub fn run_top_backtest(
    symbol: &str,
    series: &str,
    history: &[HistoryRecord],
    timeframe: SignalTimeframe,
    window_days: Option<i64>,
    thresholds: &[usize],
    with_expectancy: bool,
    detrend: bool,
) -> Option<CycleSignalBacktest> {
    let window_days = window_days.unwrap_or(DEFAULT_WINDOW_BARS).max(1);
    if history.len() < cycle_signals::min_daily_bars() {
        return None;
    }
    let as_of = history.last()?.date.clone();
    let eval_stride_days = eval_stride_days(timeframe);

    // No documented doctrine TOP anchors exist — tops are price-structure-only.
    let anchors: Vec<String> = Vec::new();
    let unverified: Vec<String> = Vec::new();
    let anchor_dates: Vec<NaiveDate> = Vec::new();
    let total_anchors = 0usize;
    let small_n = total_anchors < SMALL_N_THRESHOLD;

    let n_criteria = 7usize;
    let mut prev_met: Vec<bool> = vec![false; n_criteria];
    let mut prev_count: usize = 0;
    let mut have_prev = false;
    let mut keys_labels: Vec<(String, String)> = Vec::new();
    let mut crit_firings: Vec<Vec<Firing>> = vec![Vec::new(); n_criteria];
    let mut conf_firings: Vec<Vec<Firing>> = thresholds.iter().map(|_| Vec::new()).collect();
    let mut crit_fire_idx: Vec<Vec<usize>> = vec![Vec::new(); n_criteria];
    let mut conf_fire_idx: Vec<Vec<usize>> = thresholds.iter().map(|_| Vec::new()).collect();
    let mut eval_idx: Vec<usize> = Vec::new();

    let start = cycle_signals::min_daily_bars().saturating_sub(1);
    let mut i = start;
    while i < history.len() {
        if let Some(read) = cycle_signals::cycle_top_signals(symbol, &history[..=i], timeframe) {
            if keys_labels.is_empty() {
                keys_labels = read
                    .criteria
                    .iter()
                    .map(|c| (c.key.clone(), c.label.clone()))
                    .collect();
            }
            let fired_on = read.as_of.clone();
            let cur_met: Vec<bool> = read.criteria.iter().map(|c| c.met).collect();
            let cur_count = read.met_count;
            if with_expectancy {
                eval_idx.push(i);
            }
            if have_prev {
                for (ci, &met) in cur_met.iter().enumerate().take(n_criteria) {
                    if met && !prev_met[ci] {
                        let m = match_firing(&fired_on, &anchor_dates, window_days);
                        let hit = m.is_some();
                        crit_firings[ci].push(Firing {
                            fired_on: fired_on.clone(),
                            matched_low: m.as_ref().map(|(l, _)| l.clone()),
                            lead_lag_days: m.as_ref().map(|(_, d)| *d),
                            hit,
                        });
                        if with_expectancy {
                            crit_fire_idx[ci].push(i);
                        }
                    }
                }
                for (ti, &thr) in thresholds.iter().enumerate() {
                    let now_at = cur_count >= thr;
                    let was_at = prev_count >= thr;
                    if now_at && !was_at {
                        let m = match_firing(&fired_on, &anchor_dates, window_days);
                        let hit = m.is_some();
                        conf_firings[ti].push(Firing {
                            fired_on: fired_on.clone(),
                            matched_low: m.as_ref().map(|(l, _)| l.clone()),
                            lead_lag_days: m.as_ref().map(|(_, d)| *d),
                            hit,
                        });
                        if with_expectancy {
                            conf_fire_idx[ti].push(i);
                        }
                    }
                }
            }
            prev_met = cur_met;
            prev_count = cur_count;
            have_prev = true;
        }
        if i + eval_stride_days >= history.len() && i + 1 < history.len() {
            i = history.len() - 1;
        } else {
            i += eval_stride_days;
        }
    }

    if keys_labels.is_empty() {
        return None;
    }

    let criteria: Vec<CriterionReliability> = keys_labels
        .iter()
        .enumerate()
        .map(|(ci, (k, l))| {
            build_reliability(k, l, std::mem::take(&mut crit_firings[ci]), total_anchors)
        })
        .collect();
    let confluence: Vec<CriterionReliability> = thresholds
        .iter()
        .enumerate()
        .map(|(ti, &thr)| {
            let mut row = build_reliability(
                &format!("confluence_ge_{thr}"),
                &format!("Confluence ≥{thr}/7 criteria firing"),
                std::mem::take(&mut conf_firings[ti]),
                total_anchors,
            );
            row.threshold = Some(thr);
            row
        })
        .collect();

    let headline =
        "Cycle-TOP suite has no doctrine anchors (doctrine anchors are cycle LOWS); reliability \
         is measured against price-structure swing highs in the expectancy block."
            .to_string();
    let caveat =
        "insufficient_anchors: cycle TOPS have no documented doctrine anchors — the verified-low \
         reliability section is empty by construction; use the expectancy block (price-structure \
         swing highs) for the forward-return read."
            .to_string();

    let expectancy = if with_expectancy {
        let price_highs =
            price_structure_highs(history, PRICE_LOW_PIVOT_WINDOW, PRICE_LOW_PROMINENCE_PCT);
        let crit_idx: Vec<(String, String, Vec<usize>)> = keys_labels
            .iter()
            .enumerate()
            .map(|(ci, (k, l))| (k.clone(), l.clone(), std::mem::take(&mut crit_fire_idx[ci])))
            .collect();
        let conf_idx: Vec<(usize, Vec<usize>)> = thresholds
            .iter()
            .enumerate()
            .map(|(ti, &thr)| (thr, std::mem::take(&mut conf_fire_idx[ti])))
            .collect();
        Some(build_top_expectancy(
            history,
            &eval_idx,
            &crit_idx,
            &conf_idx,
            &price_highs,
            window_days,
            detrend,
        ))
    } else {
        None
    };

    Some(CycleSignalBacktest {
        symbol: symbol.to_string(),
        series: series.to_string(),
        timeframe,
        as_of,
        bars: history.len(),
        window_days,
        eval_stride_days,
        anchors,
        unverified_anchors: unverified,
        small_n,
        criteria,
        confluence,
        headline,
        caveat,
        expectancy,
    })
}

// ---------------------------------------------------------------------------
// Forward-return expectancy (asset-agnostic)
// ---------------------------------------------------------------------------

/// Mean of a slice of decimals.
fn dec_mean(v: &[Decimal]) -> Option<Decimal> {
    if v.is_empty() {
        return None;
    }
    let sum: Decimal = v.iter().copied().sum();
    Some(sum / Decimal::from(v.len()))
}

/// Median of a slice of decimals (average of the two middle values for even n).
fn dec_median(mut v: Vec<Decimal>) -> Option<Decimal> {
    if v.is_empty() {
        return None;
    }
    v.sort();
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        Some(v[mid])
    } else {
        Some((v[mid - 1] + v[mid]) / dec!(2))
    }
}

/// Fraction (percent) of strictly-positive values.
fn positive_rate_pct(v: &[Decimal]) -> Option<Decimal> {
    if v.is_empty() {
        return None;
    }
    let pos = v.iter().filter(|x| x.is_sign_positive() && !x.is_zero()).count();
    Some(Decimal::from(pos) / Decimal::from(v.len()) * dec!(100))
}

/// Decimal places the deterministic square root is rounded to. Six places is
/// far finer than any quoted percentage and keeps the rounded result identical
/// across platforms.
const STDEV_DP: u32 = 6;

/// POPULATION standard deviation (percent) of a forward-return distribution.
///
/// DEFINITION: **population** standard deviation — the variance divides by `n`
/// (NOT `n-1`). These are full enumerations of a row's firings (or every
/// evaluated baseline bar), not inferential subsamples, so the population form
/// is the honest one and it avoids the `n-1` edge case. `None` when `samples < 2`
/// (dispersion is undefined on a single point).
///
/// DETERMINISM: the mean and the variance are computed entirely in `Decimal`.
/// ONLY the final square-root step leaves `Decimal`: the variance is converted
/// to `f64`, `f64::sqrt` is taken, and the result is converted back to `Decimal`
/// rounded to [`STDEV_DP`] places. `f64::sqrt` is an IEEE-754 *correctly-rounded*
/// operation — it returns the same bits on every conforming platform for the
/// same input (unlike libm transcendentals such as `ln`/`exp`) — and the trailing
/// `round_dp` discards any sub-ulp noise, so the stored value is fully
/// reproducible. There is no randomness anywhere.
fn stdev_pct(v: &[Decimal]) -> Option<Decimal> {
    if v.len() < 2 {
        return None;
    }
    let n = Decimal::from(v.len());
    let mean = v.iter().copied().sum::<Decimal>() / n;
    let var = v
        .iter()
        .map(|x| {
            let d = *x - mean;
            d * d
        })
        .sum::<Decimal>()
        / n;
    if var <= Decimal::ZERO {
        // Degenerate distribution (all samples identical) — zero dispersion.
        return Some(Decimal::ZERO);
    }
    let root = var.to_f64()?.sqrt();
    Decimal::from_f64(root).map(|d| d.round_dp(STDEV_DP))
}

/// ROUGH standardized effect size: `lift / baseline_stdev`, rounded to 2 dp —
/// how many baseline-return standard deviations the signal shifts the
/// forward-return mean. `None` when the lift or the baseline dispersion is
/// missing, or the baseline stdev is zero. DIRECTIONAL scale context only, NOT a
/// significance test: no p-value is claimed.
fn effect_size(lift: Option<Decimal>, base_stdev: Option<Decimal>) -> Option<Decimal> {
    match (lift, base_stdev) {
        (Some(l), Some(s)) if !s.is_zero() => Some((l / s).round_dp(2)),
        _ => None,
    }
}

/// Forward return (percent) from bar `i` to the first bar whose date is on or
/// after `date(i) + horizon_days`. `None` when bar `i` has no qualifying future
/// bar (not enough forward history) or a zero/negative base price.
///
/// This is the OUTCOME measurement, not the signal read: it deliberately looks
/// forward. The no-lookahead discipline governs the *signal* evaluation
/// (`history[..=i]`), never the realized forward return being graded.
fn forward_return_pct(history: &[HistoryRecord], i: usize, horizon_days: i64) -> Option<Decimal> {
    let d0 = parse(&history[i].date)?;
    let target = d0 + chrono::Duration::days(horizon_days);
    let p0 = history[i].close;
    if p0 <= Decimal::ZERO {
        return None;
    }
    // History is date-sorted ascending, so binary-search the first bar whose
    // date is on/after the horizon target instead of scanning forward linearly
    // (O(log n) vs O(n) — this runs once per firing AND once per baseline bar,
    // so the linear form was O(n²) on long histories). `partition_point`
    // returns the first index where the predicate `date < target` is false,
    // i.e. the first bar on/after the target — identical to the bar the linear
    // scan selected. Unparseable dates (never present in real bar data) sort
    // with the past so they are skipped exactly as the linear scan skipped them.
    let tail = &history[i + 1..];
    let off = tail.partition_point(|r| parse(&r.date).map(|d| d < target).unwrap_or(true));
    let r = tail.get(off)?;
    if parse(&r.date)? >= target {
        Some((r.close - p0) / p0 * dec!(100))
    } else {
        None
    }
}

/// Expected forward DRIFT return (percent, Decimal) over `horizon_days` from
/// bar `i`, estimated from the asset's TRAILING-WINDOW local log-price slope —
/// the "what the secular/time-local trend alone would deliver" baseline that the
/// `--detrend` mode subtracts from each raw return.
///
/// METHOD: locate the most recent bar whose date is on/before
/// `date(i) − DETREND_TRAILING_DAYS` (the trailing reference). Per-day log drift
/// `g = (ln P_i − ln P_trail) / Δdays`, where `Δdays` is the ACTUAL calendar gap
/// to that bar (≥ the nominal window on a gappy series — using the real gap is
/// the honest per-day rate). Expected horizon drift return = `exp(g · h) − 1`,
/// in percent.
///
/// `None` — and the bar is dropped from the detrended sample — when bar `i`
/// lacks a full `DETREND_TRAILING_DAYS` of trailing history, on a nonpositive
/// price, or on a degenerate zero/negative elapsed gap. Dropping early bars is
/// the documented sample-size cost of detrending.
///
/// DETERMINISM: `ln`/`exp` are taken in f64 (isolated transcendental step) and
/// the resulting drift % is immediately rounded to [`DRIFT_DP`] before being
/// returned as a Decimal, so the subtraction downstream is stable Decimal math
/// (see [`DRIFT_DP`] for the full ulp argument).
fn expected_drift_return_pct(
    history: &[HistoryRecord],
    i: usize,
    horizon_days: i64,
) -> Option<Decimal> {
    let d_i = parse(&history[i].date)?;
    let p_i = history[i].close;
    if p_i <= Decimal::ZERO {
        return None;
    }
    let trail_target = d_i - chrono::Duration::days(DETREND_TRAILING_DAYS);
    // Most recent bar on/before the trailing target. `partition_point` returns
    // the count of leading bars with `date <= trail_target` (ascending dates),
    // i.e. the index of the first bar PAST the target; the trailing reference is
    // the bar just before it. Unparseable dates (never present in real bar data)
    // sort with the past, matching `forward_return_pct`'s convention.
    let head = &history[..i];
    let off = head.partition_point(|r| parse(&r.date).map(|d| d <= trail_target).unwrap_or(true));
    if off == 0 {
        // No bar is a full window old → insufficient trailing history.
        return None;
    }
    let trail = &head[off - 1];
    let d_trail = parse(&trail.date)?;
    let p_trail = trail.close;
    if p_trail <= Decimal::ZERO {
        return None;
    }
    let elapsed = (d_i - d_trail).num_days();
    if elapsed <= 0 {
        return None;
    }
    // --- Transcendental step ISOLATED to f64 (ln/exp are not IEEE-correctly-
    // rounded); round the % to DRIFT_DP before leaving f64 so downstream Decimal
    // math is reproducible. ---
    let ln_pi = p_i.to_f64()?.ln();
    let ln_pt = p_trail.to_f64()?.ln();
    let g = (ln_pi - ln_pt) / elapsed as f64;
    let drift_factor = (g * horizon_days as f64).exp();
    let drift_pct = (drift_factor - 1.0) * 100.0;
    Decimal::from_f64(drift_pct).map(|d| d.round_dp(DRIFT_DP))
}

/// Forward return (percent) from bar `i` over `horizon_days`, RAW or
/// DRIFT-DETRENDED depending on `detrend`.
///
/// - Raw (`detrend == false`): `(P_{i+h} − P_i)/P_i` — identical to
///   [`forward_return_pct`], so default output is byte-for-byte unchanged.
/// - Detrended (`detrend == true`): `raw − expected_drift_return` — the EXCESS
///   over the asset's contemporaneous trailing-window local drift. Returns
///   `None` whenever either the raw return OR the trailing-drift estimate is
///   unavailable, so a bar without a full trailing window is dropped from the
///   detrended sample.
fn measured_return_pct(
    history: &[HistoryRecord],
    i: usize,
    horizon_days: i64,
    detrend: bool,
) -> Option<Decimal> {
    let raw = forward_return_pct(history, i, horizon_days)?;
    if !detrend {
        return Some(raw);
    }
    let drift = expected_drift_return_pct(history, i, horizon_days)?;
    Some(raw - drift)
}

/// Asset-agnostic price-structure swing lows: prominence-filtered pivot lows
/// over the FULL price history, independent of the cycle-signal suite (no
/// circularity — only OHLC is consulted). Returns `(index, date, low_price)`.
///
/// Method: rolling-window pivot lows (`cycle_engine::pivot_lows`) on the daily
/// low (falling back to close), then keep only pivots followed by a recovery of
/// at least `prominence_pct` before the next pivot (or series end). The
/// recovery filter both removes minor shelves AND drops the most recent,
/// not-yet-recovered low — which conveniently avoids leaning on an unconfirmed
/// bottom.
fn price_structure_lows(
    history: &[HistoryRecord],
    pivot_window: usize,
    prominence_pct: i64,
) -> Vec<(usize, NaiveDate, Decimal)> {
    if history.is_empty() {
        return Vec::new();
    }
    let lows: Vec<Decimal> = history
        .iter()
        .map(|r| r.low.unwrap_or(r.close))
        .collect();
    let pivots = pivot_lows(&lows, pivot_window);
    let prominence = Decimal::from(prominence_pct);
    let mut out = Vec::new();
    for (k, &pi) in pivots.iter().enumerate() {
        let low_price = lows[pi];
        if low_price <= Decimal::ZERO {
            continue;
        }
        // Highest high between this pivot and the next (or the series end).
        let seg_end = pivots.get(k + 1).copied().unwrap_or(history.len());
        let seg_hi = history[pi..seg_end.min(history.len())]
            .iter()
            .map(|r| r.high.unwrap_or(r.close))
            .max()
            .unwrap_or(low_price);
        let recovery_pct = (seg_hi - low_price) / low_price * dec!(100);
        if recovery_pct >= prominence {
            if let Some(d) = parse(&history[pi].date) {
                out.push((pi, d, low_price));
            }
        }
    }
    out
}

/// Rolling-window pivot HIGHS — the mirror of [`pivot_lows`]. Left window
/// clamped at the series start; right window must be complete (finality).
/// Tie-break mirrors `pivot_lows`: equal highs resolve to the LATER bar
/// (left non-strict, right strict).
fn pivot_highs(high: &[Decimal], w: usize) -> Vec<usize> {
    let n = high.len();
    if n == 0 || w == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..n {
        let Some(right_end) = i.checked_add(w) else {
            continue;
        };
        if right_end >= n {
            break;
        }
        let l0 = i.saturating_sub(w);
        let left_ok = high[l0..i].iter().all(|&v| high[i] >= v);
        let right_ok = high[i + 1..=right_end].iter().all(|&v| high[i] > v);
        if left_ok && right_ok {
            out.push(i);
        }
    }
    out
}

/// Asset-agnostic price-structure swing HIGHS — the mirror of
/// [`price_structure_lows`]. Prominence-filtered pivot highs over the FULL
/// history, independent of the cycle-signal suite (only OHLC consulted).
/// Returns `(index, date, high_price)`.
///
/// Method: rolling-window pivot highs on the daily high (falling back to
/// close), then keep only pivots followed by a DECLINE of at least
/// `prominence_pct` before the next pivot (or series end). The decline filter
/// removes minor pullback peaks AND drops the most recent, not-yet-declined
/// high — which conveniently avoids leaning on an unconfirmed top.
///
/// NOTE (epistemics): unlike the bottom backtest there are NO doctrine TOP
/// anchors (the documented BTC 4y / gold ~6.9y anchors are cycle LOWS), so the
/// top backtest is PRICE-STRUCTURE-ONLY. Treat these hit-rates as directional,
/// never authoritative.
fn price_structure_highs(
    history: &[HistoryRecord],
    pivot_window: usize,
    prominence_pct: i64,
) -> Vec<(usize, NaiveDate, Decimal)> {
    if history.is_empty() {
        return Vec::new();
    }
    let highs: Vec<Decimal> = history.iter().map(|r| r.high.unwrap_or(r.close)).collect();
    let pivots = pivot_highs(&highs, pivot_window);
    let prominence = Decimal::from(prominence_pct);
    let mut out = Vec::new();
    for (k, &pi) in pivots.iter().enumerate() {
        let high_price = highs[pi];
        if high_price <= Decimal::ZERO {
            continue;
        }
        // Lowest low between this pivot and the next (or the series end).
        let seg_end = pivots.get(k + 1).copied().unwrap_or(history.len());
        let seg_lo = history[pi..seg_end.min(history.len())]
            .iter()
            .map(|r| r.low.unwrap_or(r.close))
            .min()
            .unwrap_or(high_price);
        let decline_pct = (high_price - seg_lo) / high_price * dec!(100);
        if decline_pct >= prominence {
            if let Some(d) = parse(&history[pi].date) {
                out.push((pi, d, high_price));
            }
        }
    }
    out
}

/// Match firing bar `i` to the nearest price anchor within `window_days`,
/// returning `(signed_lead_lag_days, signed_price_gap_pct)`. Price gap is
/// `(fire_price - low_price) / low_price * 100` (positive = fired above the low).
fn match_price_anchor(
    history: &[HistoryRecord],
    i: usize,
    anchors: &[(NaiveDate, Decimal)],
    window_days: i64,
) -> Option<(i64, Decimal)> {
    let f = parse(&history[i].date)?;
    let fire_price = history[i].close;
    let mut best: Option<(i64, Decimal)> = None;
    for &(a, low_price) in anchors {
        if low_price <= Decimal::ZERO {
            continue;
        }
        let signed = (f - a).num_days();
        if signed.abs() <= window_days
            && best.map(|(b, _)| signed.abs() < b.abs()).unwrap_or(true)
        {
            let gap = (fire_price - low_price) / low_price * dec!(100);
            best = Some((signed, gap));
        }
    }
    best
}

/// Build the per-horizon forward-return rows for one signal's firing indices.
fn horizon_returns(
    history: &[HistoryRecord],
    firing_idx: &[usize],
    baseline: &[HorizonBaseline],
    detrend: bool,
) -> Vec<HorizonReturn> {
    FORWARD_HORIZONS_DAYS
        .iter()
        .map(|&h| {
            let rets: Vec<Decimal> = firing_idx
                .iter()
                .filter_map(|&i| measured_return_pct(history, i, h, detrend))
                .collect();
            let mean = dec_mean(&rets);
            let base_mean = baseline
                .iter()
                .find(|b| b.horizon_days == h)
                .and_then(|b| b.mean_return_pct);
            let lift = match (mean, base_mean) {
                (Some(m), Some(b)) => Some(m - b),
                _ => None,
            };
            let base_stdev = baseline
                .iter()
                .find(|b| b.horizon_days == h)
                .and_then(|b| b.stdev_return_pct);
            HorizonReturn {
                horizon_days: h,
                samples: rets.len(),
                mean_return_pct: mean,
                median_return_pct: dec_median(rets.clone()),
                positive_rate_pct: positive_rate_pct(&rets),
                negative_rate_pct: None,
                baseline_mean_return_pct: base_mean,
                lift_vs_baseline_pct: lift,
                stdev_return_pct: stdev_pct(&rets),
                effect_size: effect_size(lift, base_stdev),
            }
        })
        .collect()
}

/// Cycle-TOP variant of [`horizon_returns`]: identical forward-return machinery,
/// but also populates `negative_rate_pct` (the fraction of forward returns that
/// were strictly negative — the top thesis hit-rate). Lift is still
/// `mean - baseline`; for a good top signal this is NEGATIVE (price
/// underperformed the unconditioned baseline after the signal fired).
fn top_horizon_returns(
    history: &[HistoryRecord],
    firing_idx: &[usize],
    baseline: &[HorizonBaseline],
    detrend: bool,
) -> Vec<HorizonReturn> {
    FORWARD_HORIZONS_DAYS
        .iter()
        .map(|&h| {
            let rets: Vec<Decimal> = firing_idx
                .iter()
                .filter_map(|&i| measured_return_pct(history, i, h, detrend))
                .collect();
            let mean = dec_mean(&rets);
            let base_mean = baseline
                .iter()
                .find(|b| b.horizon_days == h)
                .and_then(|b| b.mean_return_pct);
            let lift = match (mean, base_mean) {
                (Some(m), Some(b)) => Some(m - b),
                _ => None,
            };
            let base_stdev = baseline
                .iter()
                .find(|b| b.horizon_days == h)
                .and_then(|b| b.stdev_return_pct);
            HorizonReturn {
                horizon_days: h,
                samples: rets.len(),
                mean_return_pct: mean,
                median_return_pct: dec_median(rets.clone()),
                positive_rate_pct: positive_rate_pct(&rets),
                negative_rate_pct: negative_rate_pct(&rets),
                baseline_mean_return_pct: base_mean,
                lift_vs_baseline_pct: lift,
                stdev_return_pct: stdev_pct(&rets),
                effect_size: effect_size(lift, base_stdev),
            }
        })
        .collect()
}

/// Fraction (percent) of strictly-negative values — the cycle-TOP mirror of
/// [`positive_rate_pct`].
fn negative_rate_pct(v: &[Decimal]) -> Option<Decimal> {
    if v.is_empty() {
        return None;
    }
    let neg = v.iter().filter(|x| x.is_sign_negative() && !x.is_zero()).count();
    Some(Decimal::from(neg) / Decimal::from(v.len()) * dec!(100))
}

/// Aggregate closeness of a signal's firings to the nearest price-structure low.
fn closeness_stats(
    history: &[HistoryRecord],
    firing_idx: &[usize],
    anchors: &[(NaiveDate, Decimal)],
    window_days: i64,
) -> ClosenessStats {
    let firings = firing_idx.len();
    let mut lead_lags: Vec<i64> = Vec::new();
    let mut gaps: Vec<Decimal> = Vec::new();
    for &i in firing_idx {
        if let Some((days, gap)) = match_price_anchor(history, i, anchors, window_days) {
            lead_lags.push(days);
            gaps.push(gap);
        }
    }
    let matched = lead_lags.len();
    let confidence_pct = if firings > 0 {
        Some(Decimal::from(matched) / Decimal::from(firings) * dec!(100))
    } else {
        None
    };
    ClosenessStats {
        matched_firings: matched,
        firings,
        median_lead_lag_days: median(lead_lags),
        median_price_gap_pct: dec_median(gaps),
        confidence_pct,
    }
}

/// Assemble the full expectancy block from per-signal firing indices, the set
/// of evaluated bars (for the baseline), and the price/doctrine anchor set.
#[allow(clippy::too_many_arguments)]
fn build_expectancy(
    history: &[HistoryRecord],
    eval_idx: &[usize],
    crit_idx: &[(String, String, Vec<usize>)],
    conf_idx: &[(usize, Vec<usize>)],
    price_lows: &[(usize, NaiveDate, Decimal)],
    doctrine_anchors: &[(NaiveDate, Decimal)],
    window_days: i64,
    detrend: bool,
) -> CycleSignalExpectancy {
    // Anchor set for closeness = price-structure lows ∪ doctrine anchors,
    // deduplicated by date (doctrine price wins on a tie since it is stronger
    // ground truth).
    let mut anchor_map: std::collections::BTreeMap<NaiveDate, Decimal> = std::collections::BTreeMap::new();
    for &(_, d, p) in price_lows {
        anchor_map.entry(d).or_insert(p);
    }
    for &(d, p) in doctrine_anchors {
        anchor_map.insert(d, p);
    }
    let anchors: Vec<(NaiveDate, Decimal)> = anchor_map.iter().map(|(&d, &p)| (d, p)).collect();
    let anchors_used = anchors.len();
    let doctrine_anchors_used = !doctrine_anchors.is_empty();
    let insufficient_anchors = anchors.is_empty();
    let small_n = anchors_used < SMALL_N_THRESHOLD;

    // Baseline: forward returns over every evaluated bar at each horizon
    // (raw, or excess-over-local-drift when `detrend` — the SAME treatment as
    // the per-firing returns, so lift stays a like-for-like comparison).
    let baseline: Vec<HorizonBaseline> = FORWARD_HORIZONS_DAYS
        .iter()
        .map(|&h| {
            let rets: Vec<Decimal> = eval_idx
                .iter()
                .filter_map(|&i| measured_return_pct(history, i, h, detrend))
                .collect();
            HorizonBaseline {
                horizon_days: h,
                samples: rets.len(),
                mean_return_pct: dec_mean(&rets),
                median_return_pct: dec_median(rets.clone()),
                positive_rate_pct: positive_rate_pct(&rets),
                stdev_return_pct: stdev_pct(&rets),
            }
        })
        .collect();

    let criteria: Vec<ExpectancyRow> = crit_idx
        .iter()
        .map(|(k, l, idx)| ExpectancyRow {
            key: k.clone(),
            threshold: None,
            label: l.clone(),
            firings: idx.len(),
            low_firings: idx.len() < MIN_SIGNIFICANT_FIRINGS,
            horizons: horizon_returns(history, idx, &baseline, detrend),
            closeness: closeness_stats(history, idx, &anchors, window_days),
        })
        .collect();

    let confluence: Vec<ExpectancyRow> = conf_idx
        .iter()
        .map(|(thr, idx)| ExpectancyRow {
            key: format!("confluence_ge_{thr}"),
            threshold: Some(*thr),
            label: format!("Confluence ≥{thr}/7 criteria firing"),
            firings: idx.len(),
            low_firings: idx.len() < MIN_SIGNIFICANT_FIRINGS,
            horizons: horizon_returns(history, idx, &baseline, detrend),
            closeness: closeness_stats(history, idx, &anchors, window_days),
        })
        .collect();

    let caveat = if insufficient_anchors {
        "insufficient_anchors: no price-structure or doctrine cycle lows could be derived from \
         this history — forward returns are reported but closeness is unmeasurable."
            .to_string()
    } else if small_n {
        format!(
            "small_n: expectancy is conditioned on only {anchors_used} cycle-low anchor(s) \
             ({} doctrine). Price-structure anchors are mechanically derived (lowest low in a \
             window with a ≥{PRICE_LOW_PROMINENCE_PCT}% recovery) and are WEAKER ground truth than \
             doctrine anchors — read lift/closeness as directional, not as probabilities.",
            doctrine_anchors.len()
        )
    } else {
        format!(
            "Expectancy conditioned on {anchors_used} cycle-low anchor(s); forward-return lift is \
             measured against the unconditioned same-horizon baseline."
        )
    };
    let caveat = if detrend {
        format!(
            "{caveat} DRIFT-DETRENDED: every return (signal AND baseline) is EXCESS over the \
             asset's trailing-{DETREND_TRAILING_DAYS}d local log-drift, isolating the signal's \
             edge from secular/time-local trend. Caveats: this assumes log-LINEAR local drift over \
             the window, and bars without a full trailing year are dropped (smaller sample than \
             raw mode)."
        )
    } else {
        caveat
    };

    CycleSignalExpectancy {
        price_structure_anchors: price_lows
            .iter()
            .map(|(_, d, _)| d.format("%Y-%m-%d").to_string())
            .collect(),
        price_low_pivot_window: PRICE_LOW_PIVOT_WINDOW,
        price_low_prominence_pct: Decimal::from(PRICE_LOW_PROMINENCE_PCT),
        doctrine_anchors_used,
        anchors_used,
        insufficient_anchors,
        small_n,
        detrended: detrend,
        detrend_trailing_days: detrend.then_some(DETREND_TRAILING_DAYS),
        baseline,
        criteria,
        confluence,
        caveat,
    }
}

/// Assemble the cycle-TOP expectancy block — the mirror of [`build_expectancy`].
/// Anchors are asset-agnostic price-structure swing HIGHS only (no doctrine top
/// anchors exist). Forward returns are graded with [`top_horizon_returns`] so
/// each horizon also carries `negative_rate_pct` (the top thesis hit-rate).
fn build_top_expectancy(
    history: &[HistoryRecord],
    eval_idx: &[usize],
    crit_idx: &[(String, String, Vec<usize>)],
    conf_idx: &[(usize, Vec<usize>)],
    price_highs: &[(usize, NaiveDate, Decimal)],
    window_days: i64,
    detrend: bool,
) -> CycleSignalExpectancy {
    let anchors: Vec<(NaiveDate, Decimal)> =
        price_highs.iter().map(|&(_, d, p)| (d, p)).collect();
    let anchors_used = anchors.len();
    let insufficient_anchors = anchors.is_empty();
    let small_n = anchors_used < SMALL_N_THRESHOLD;

    let baseline: Vec<HorizonBaseline> = FORWARD_HORIZONS_DAYS
        .iter()
        .map(|&h| {
            let rets: Vec<Decimal> = eval_idx
                .iter()
                .filter_map(|&i| measured_return_pct(history, i, h, detrend))
                .collect();
            HorizonBaseline {
                horizon_days: h,
                samples: rets.len(),
                mean_return_pct: dec_mean(&rets),
                median_return_pct: dec_median(rets.clone()),
                positive_rate_pct: positive_rate_pct(&rets),
                stdev_return_pct: stdev_pct(&rets),
            }
        })
        .collect();

    let criteria: Vec<ExpectancyRow> = crit_idx
        .iter()
        .map(|(k, l, idx)| ExpectancyRow {
            key: k.clone(),
            threshold: None,
            label: l.clone(),
            firings: idx.len(),
            low_firings: idx.len() < MIN_SIGNIFICANT_FIRINGS,
            horizons: top_horizon_returns(history, idx, &baseline, detrend),
            closeness: closeness_stats(history, idx, &anchors, window_days),
        })
        .collect();
    let confluence: Vec<ExpectancyRow> = conf_idx
        .iter()
        .map(|(thr, idx)| ExpectancyRow {
            key: format!("confluence_ge_{thr}"),
            threshold: Some(*thr),
            label: format!("Confluence ≥{thr}/7 criteria firing"),
            firings: idx.len(),
            low_firings: idx.len() < MIN_SIGNIFICANT_FIRINGS,
            horizons: top_horizon_returns(history, idx, &baseline, detrend),
            closeness: closeness_stats(history, idx, &anchors, window_days),
        })
        .collect();

    let caveat = if insufficient_anchors {
        "insufficient_anchors: no price-structure swing highs could be derived from this history \
         — forward returns are reported but closeness is unmeasurable."
            .to_string()
    } else {
        format!(
            "small_n / price-structure-only: top expectancy is conditioned on {anchors_used} \
             price-structure swing high(s) (lowest-low decline of ≥{PRICE_LOW_PROMINENCE_PCT}% out \
             of the peak). Cycle TOPS have NO doctrine anchors, so these are WEAKER ground truth \
             than the bottom backtest's doctrine lows — read negative-rate / lift / closeness as \
             directional, not as probabilities. A good top signal precedes a DECLINE: expect \
             negative mean forward return and negative lift vs baseline."
        )
    };
    let caveat = if detrend {
        format!(
            "{caveat} DRIFT-DETRENDED: every return (signal AND baseline) is EXCESS over the \
             asset's trailing-{DETREND_TRAILING_DAYS}d local log-drift, isolating the signal's \
             edge from secular/time-local trend. Caveats: assumes log-LINEAR local drift over the \
             window, and bars without a full trailing year are dropped (smaller sample than raw)."
        )
    } else {
        caveat
    };

    CycleSignalExpectancy {
        price_structure_anchors: price_highs
            .iter()
            .map(|(_, d, _)| d.format("%Y-%m-%d").to_string())
            .collect(),
        price_low_pivot_window: PRICE_LOW_PIVOT_WINDOW,
        price_low_prominence_pct: Decimal::from(PRICE_LOW_PROMINENCE_PCT),
        doctrine_anchors_used: false,
        anchors_used,
        insufficient_anchors,
        small_n,
        detrended: detrend,
        detrend_trailing_days: detrend.then_some(DETREND_TRAILING_DAYS),
        baseline,
        criteria,
        confluence,
        caveat,
    }
}

/// Look up the close price on an exact date (anchors are real bar dates).
fn price_at_date(history: &[HistoryRecord], date: NaiveDate) -> Option<Decimal> {
    let target = date.format("%Y-%m-%d").to_string();
    history.iter().find(|r| r.date == target).map(|r| r.close)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn record(date: &str, close: f64) -> HistoryRecord {
        HistoryRecord {
            date: date.to_string(),
            close: Decimal::from_str(&format!("{close:.4}")).unwrap_or_default(),
            volume: None,
            open: None,
            high: None,
            low: None,
        }
    }

    /// Deep decline into a V-bottom then rally, planted to bottom near a known
    /// date. Mirrors the engine's own fixture shape so criteria actually fire.
    fn planted_v_bottom(start: NaiveDate, n_decline: usize, n_rally: usize) -> Vec<HistoryRecord> {
        let mut out = Vec::new();
        let mut day = 0u64;
        let mut price = 1000.0;
        for i in 0..n_decline {
            price = 1000.0 - i as f64 * (700.0 / n_decline as f64);
            let noise = 8.0 * (i as f64 / 11.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, (price + noise).max(50.0)));
            day += 1;
        }
        let base = price;
        for j in 1..=n_rally {
            let p = base + j as f64 * (600.0 / n_rally as f64);
            let noise = 6.0 * (j as f64 / 9.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, p + noise));
            day += 1;
        }
        out
    }

    #[test]
    fn matches_firing_within_window_signed() {
        let anchors = vec![NaiveDate::from_ymd_opt(2022, 11, 21).unwrap()];
        // fired 10 days before the low -> leads, negative.
        let m = match_firing("2022-11-11", &anchors, 90).unwrap();
        assert_eq!(m.0, "2022-11-21");
        assert_eq!(m.1, -10);
        // fired 30 days after -> lags, positive.
        let m2 = match_firing("2022-12-21", &anchors, 90).unwrap();
        assert_eq!(m2.1, 30);
        // outside window -> no match.
        assert!(match_firing("2021-01-01", &anchors, 90).is_none());
    }

    #[test]
    fn median_basic() {
        assert_eq!(median(vec![]), None);
        assert_eq!(median(vec![5]), Some(5));
        assert_eq!(median(vec![-10, 0, 10]), Some(0));
        assert_eq!(median(vec![-10, -2, 2, 10]), Some(0));
    }

    #[test]
    fn eval_stride_matches_timeframe_granularity() {
        assert_eq!(eval_stride_days(SignalTimeframe::Daily), 1);
        assert_eq!(eval_stride_days(SignalTimeframe::Weekly), 7);
        assert_eq!(eval_stride_days(SignalTimeframe::Monthly), 7);
    }

    #[test]
    fn no_lookahead_invariant() {
        // Evaluate at bar i over history[..=i]; appending future bars must NOT
        // change the criteria read at bar i. This is the no-lookahead guarantee.
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let full = planted_v_bottom(start, 700, 250);
        let i = 800usize.min(full.len() - 1);
        let read_truncated =
            cycle_signals::cycle_bottom_signals("TEST", &full[..=i], SignalTimeframe::Monthly)
                .expect("read at i");
        // Append more future bars (the rest of `full` is already future to i):
        let mut extended = full[..=i].to_vec();
        for k in 1..=50 {
            let date = (start + chrono::Days::new((i + k) as u64))
                .format("%Y-%m-%d")
                .to_string();
            extended.push(record(&date, 5000.0 + k as f64));
        }
        let read_extended_at_i =
            cycle_signals::cycle_bottom_signals("TEST", &extended[..=i], SignalTimeframe::Monthly)
                .expect("read at i (extended)");
        let a: Vec<bool> = read_truncated.criteria.iter().map(|c| c.met).collect();
        let b: Vec<bool> = read_extended_at_i.criteria.iter().map(|c| c.met).collect();
        assert_eq!(a, b, "criteria at bar i must not depend on bars after i");
        assert_eq!(read_truncated.as_of, read_extended_at_i.as_of);
    }

    #[test]
    fn backtest_planted_low_measures_lead_and_hit() {
        // Plant a V-bottom whose verified low we control by labeling the series
        // BTC and overriding... we instead test the matching plumbing directly
        // with a synthetic anchor via run-level coverage below. Here assert the
        // engine produces firings and stats on a deep series with no anchors
        // (series "TEST" => no documented anchors => insufficient).
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h = planted_v_bottom(start, 700, 250);
        let bt = run_backtest(
            "TEST",
            "TEST",
            &h,
            SignalTimeframe::Monthly,
            Some(90),
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            false,
            false,
        )
        .expect("backtest");
        assert!(bt.expectancy.is_none(), "expectancy off by default");
        assert_eq!(bt.eval_stride_days, 7);
        assert_eq!(bt.criteria.len(), 7);
        assert_eq!(bt.confluence.len(), 3);
        // Confluence rows carry the numeric threshold (matching DEFAULT_CONFLUENCE_THRESHOLDS)
        // so agents don't string-parse the `confluence_ge_<N>` key.
        let thresholds: Vec<usize> = bt.confluence.iter().filter_map(|c| c.threshold).collect();
        assert_eq!(thresholds, DEFAULT_CONFLUENCE_THRESHOLDS.to_vec());
        for c in &bt.confluence {
            assert_eq!(
                c.threshold,
                c.key
                    .strip_prefix("confluence_ge_")
                    .and_then(|n| n.parse().ok()),
                "threshold field must match the key for {}",
                c.key
            );
        }
        // Per-criterion rows have no threshold (omitted from JSON).
        assert!(bt.criteria.iter().all(|c| c.threshold.is_none()));
        // No anchors for TEST => small_n + insufficient caveat.
        assert!(bt.anchors.is_empty());
        assert!(bt.small_n);
        assert!(bt.caveat.contains("insufficient_anchors"));
        // Criteria still fired over the rally (firings counted even w/o anchors).
        let total_firings: usize = bt.criteria.iter().map(|c| c.firings).sum();
        assert!(
            total_firings > 0,
            "expected some firings on a deep V-bottom"
        );
    }

    #[test]
    fn backtest_hits_a_planted_verified_low() {
        // Use BTC anchors: plant a deep series whose price minimum lands inside
        // the documented 2022-11-21 window, so verify_anchor resolves it and a
        // criterion firing near that minimum scores a hit with a sane lead/lag.
        // Build ~5 years of daily data declining into a low near 2022-11 then
        // rallying, dated so the minimum is within ±270d of 2022-11-21.
        let start = NaiveDate::from_ymd_opt(2020, 11, 1).unwrap(); // ~2y decline -> low ~2022-11
        let h = planted_v_bottom(start, 750, 250);
        let bt = run_backtest(
            "BTC",
            "BTC-USD",
            &h,
            SignalTimeframe::Monthly,
            Some(120),
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            false,
            false,
        )
        .expect("backtest");
        // At least the 2022-11-21 anchor should verify against the planted low.
        assert!(
            bt.anchors.iter().any(|a| a.starts_with("2022")),
            "expected the 2022 low to verify; got {:?}",
            bt.anchors
        );
        // Some criterion fired in-window of that low (a hit) with a lead/lag.
        let any_hit =
            bt.criteria.iter().any(|c| c.hits > 0) || bt.confluence.iter().any(|c| c.hits > 0);
        assert!(
            any_hit,
            "expected at least one firing to hit the verified low"
        );
    }

    #[test]
    fn shallow_history_returns_none() {
        let start = NaiveDate::from_ymd_opt(2022, 1, 1).unwrap();
        let h = planted_v_bottom(start, 40, 10);
        assert!(run_backtest(
            "BTC",
            "BTC-USD",
            &h,
            SignalTimeframe::Monthly,
            None,
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            true,
            false,
        )
        .is_none());
    }

    #[test]
    fn determinism_identical_output() {
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h = planted_v_bottom(start, 600, 200);
        let a = run_backtest(
            "BTC",
            "BTC-USD",
            &h,
            SignalTimeframe::Monthly,
            None,
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            true,
            false,
        )
        .unwrap();
        let b = run_backtest(
            "BTC",
            "BTC-USD",
            &h,
            SignalTimeframe::Monthly,
            None,
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            true,
            false,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        // Determinism must hold WITH the expectancy block populated.
        assert!(a.expectancy.is_some());
    }

    // ---- Expectancy unit tests -------------------------------------------

    /// Forward-return math on a synthetic series with a KNOWN post-signal rally:
    /// 100 -> 110 over 30 days is a clean +10%; assert mean/median numerically.
    #[test]
    fn forward_return_math_is_exact() {
        // 31 daily bars: bar 0 at 100.0, then linear ramp so bar 30 == 110.0.
        let start = NaiveDate::from_ymd_opt(2021, 1, 1).unwrap();
        let mut h = Vec::new();
        for d in 0..=30u64 {
            let price = 100.0 + (d as f64) * (10.0 / 30.0);
            let date = (start + chrono::Days::new(d)).format("%Y-%m-%d").to_string();
            h.push(record(&date, price));
        }
        let r = forward_return_pct(&h, 0, 30).expect("30d forward return");
        // (110 - 100)/100 * 100 = 10.00 (allow tiny ramp rounding).
        assert!((r - dec!(10)).abs() < dec!(0.01), "got {r}");
        // Two identical firings -> mean == median == the same return.
        let rets = vec![r, r];
        assert_eq!(dec_mean(&rets), Some(r));
        assert_eq!(dec_median(rets.clone()), Some(r));
        assert_eq!(positive_rate_pct(&rets), Some(dec!(100)));
    }

    /// Hand-computed dispersion + effect size on a KNOWN distribution.
    ///
    /// Returns [10, 20, 30, 40] (percent):
    ///   mean = (10+20+30+40)/4 = 25
    ///   deviations = [-15, -5, 5, 15]; squares = [225, 25, 25, 225], sum = 500
    ///   POPULATION variance = 500/4 = 125
    ///   stdev = sqrt(125) = 11.180339887… → 11.180340 at 6 dp
    /// Effect size: lift 67.5 against baseline stdev 180 = 67.5/180 = 0.375 → 0.38 (2 dp).
    #[test]
    fn stdev_and_effect_size_hand_computed() {
        // Population stdev of [10,20,30,40] = sqrt(125) = 11.180340 (6 dp).
        let v = vec![dec!(10), dec!(20), dec!(30), dec!(40)];
        let sd = stdev_pct(&v).expect("stdev for n>=2");
        assert!(
            (sd - dec!(11.180340)).abs() < dec!(0.000001),
            "stdev got {sd}, want 11.180340"
        );

        // n < 2 → dispersion undefined.
        assert_eq!(stdev_pct(&[dec!(5)]), None);
        assert_eq!(stdev_pct(&[]), None);
        // Degenerate (all identical) → exactly zero dispersion.
        assert_eq!(stdev_pct(&[dec!(7), dec!(7), dec!(7)]), Some(dec!(0)));

        // Effect size = lift / baseline_stdev, rounded to 2 dp.
        assert_eq!(effect_size(Some(dec!(67.5)), Some(dec!(180))), Some(dec!(0.38)));
        // Zero / degenerate baseline dispersion → None (no scale to divide by).
        assert_eq!(effect_size(Some(dec!(67.5)), Some(dec!(0))), None);
        // Missing baseline dispersion or missing lift → None.
        assert_eq!(effect_size(Some(dec!(67.5)), None), None);
        assert_eq!(effect_size(None, Some(dec!(180))), None);
    }

    /// Price-% closeness: a firing planted exactly 25% above a planted low must
    /// report a +25.00% price gap and a 0-day lead/lag at the low.
    /// The binary-search forward-return hot path must select the SAME bar as a
    /// naive linear scan on an irregularly-spaced (gappy) date series — across
    /// several start bars and all reported horizons.
    #[test]
    fn forward_return_binary_matches_linear() {
        // Reference linear scan (the prior implementation) for cross-checking.
        fn linear(history: &[HistoryRecord], i: usize, horizon_days: i64) -> Option<Decimal> {
            let d0 = parse(&history[i].date)?;
            let target = d0 + chrono::Duration::days(horizon_days);
            let p0 = history[i].close;
            if p0 <= Decimal::ZERO {
                return None;
            }
            for r in &history[i + 1..] {
                if let Some(d) = parse(&r.date) {
                    if d >= target {
                        return Some((r.close - p0) / p0 * dec!(100));
                    }
                }
            }
            None
        }
        // Gappy series: skip some days so target dates fall BETWEEN bars (the
        // case where ">= target" must pick the first bar strictly past a gap).
        let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let mut h = Vec::new();
        let mut day = 0u64;
        for step in 0..400u64 {
            let date = (start + chrono::Days::new(day)).format("%Y-%m-%d").to_string();
            h.push(record(&date, 100.0 + step as f64 * 0.7));
            // irregular cadence: 1,2,3-day gaps repeating
            day += 1 + (step % 3);
        }
        for &horizon in &FORWARD_HORIZONS_DAYS {
            for i in (0..h.len()).step_by(7) {
                assert_eq!(
                    forward_return_pct(&h, i, horizon),
                    linear(&h, i, horizon),
                    "binary-search != linear at i={i}, horizon={horizon}"
                );
            }
        }
        // And a known exact value: bar 0 priced 100.0, find the 30-day-forward bar.
        let exact = forward_return_pct(&h, 0, 30).expect("30d return");
        let exact_linear = linear(&h, 0, 30).expect("30d return (linear)");
        assert_eq!(exact, exact_linear);
    }

    /// A zero-anchor backtest (no doctrine anchors, e.g. the cycle-TOP path or a
    /// non-doctrine series) must NOT report every firing as a false positive.
    /// Firing COUNTS stay real; hits/false_positives/precision/coverage null out.
    #[test]
    fn zero_anchor_backtest_no_misleading_miss_counts() {
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h = planted_inverted_v(start, 700, 250);
        let bt = run_top_backtest(
            "TEST",
            "TEST",
            &h,
            SignalTimeframe::Monthly,
            Some(90),
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            false,
            false,
        )
        .expect("backtest");
        assert!(bt.anchors.is_empty(), "top path has no doctrine anchors");
        let total_firings: usize = bt.criteria.iter().map(|c| c.firings).sum();
        assert!(total_firings > 0, "expected firings to still be counted");
        for row in bt.criteria.iter().chain(bt.confluence.iter()) {
            assert_eq!(
                row.false_positives, 0,
                "{} must not count firings as false positives without anchors",
                row.key
            );
            assert_eq!(row.hits, 0, "{} cannot hit a nonexistent anchor", row.key);
            assert!(
                row.precision.is_none(),
                "{} precision must be null (unmeasurable), not 0%",
                row.key
            );
            assert!(row.coverage.is_none(), "{} coverage must be null", row.key);
            if row.firings > 0 {
                assert!(
                    row.summary.contains("unmeasurable"),
                    "{} summary should flag unmeasurable reliability, got {:?}",
                    row.key,
                    row.summary
                );
            }
        }
    }

    #[test]
    fn price_gap_closeness_is_exact() {
        let start = NaiveDate::from_ymd_opt(2021, 6, 1).unwrap();
        // The low bar is at index 0 priced 80; the firing bar at index 0 too is
        // the anchor itself — instead place the anchor at 80 and the firing 25%
        // higher (100) on the SAME date offset window.
        let mut h = Vec::new();
        for d in 0..10u64 {
            let date = (start + chrono::Days::new(d)).format("%Y-%m-%d").to_string();
            h.push(record(&date, 100.0)); // firing price 100
        }
        let low_date = NaiveDate::from_ymd_opt(2021, 6, 5).unwrap();
        let anchors = vec![(low_date, dec!(80))]; // low price 80; 100 is +25%
        // Firing at index 0 (date 2021-06-01), 4 days before the low.
        let (days, gap) = match_price_anchor(&h, 0, &anchors, 90).expect("matched");
        assert_eq!(days, -4, "fired 4 days before the low");
        assert_eq!(gap, dec!(25), "(100-80)/80*100 == 25%");
    }

    /// Asset-agnostic path: a synthetic NON-BTC/NON-gold symbol with planted
    /// swing lows must yield price-structure anchors and a populated expectancy
    /// result (NOT insufficient_anchors).
    #[test]
    fn asset_agnostic_expectancy_has_anchors() {
        // Two deep V-bottoms back to back so the prominence-filtered pivot scan
        // retains at least one price-structure low (rally out > 20%).
        let start = NaiveDate::from_ymd_opt(2016, 1, 1).unwrap();
        let mut h = planted_v_bottom(start, 500, 400);
        let next_start = NaiveDate::from_ymd_opt(2018, 6, 1).unwrap();
        let mut h2 = planted_v_bottom(next_start, 500, 400);
        h.append(&mut h2);
        // A symbol with NO doctrine anchors.
        let bt = run_backtest(
            "ACME",
            "ACME",
            &h,
            SignalTimeframe::Monthly,
            Some(120),
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            true,
            false,
        )
        .expect("backtest");
        let exp = bt.expectancy.expect("expectancy populated");
        assert!(
            !exp.doctrine_anchors_used,
            "ACME has no doctrine anchors"
        );
        assert!(
            !exp.price_structure_anchors.is_empty(),
            "expected price-structure swing lows on a double V-bottom"
        );
        assert!(
            !exp.insufficient_anchors,
            "asset-agnostic anchors should be present"
        );
        assert!(exp.anchors_used > 0);
        // Baseline + per-horizon rows are present for all 4 horizons.
        assert_eq!(exp.baseline.len(), FORWARD_HORIZONS_DAYS.len());
        assert_eq!(exp.criteria.len(), 7);
        for row in exp.criteria.iter().chain(exp.confluence.iter()) {
            assert_eq!(row.horizons.len(), FORWARD_HORIZONS_DAYS.len());
        }
    }

    // ---- Cycle-TOP backtest (symmetric mirror) ---------------------------

    /// Deep advance into an inverted-V top then selloff, planted to peak near a
    /// known date. Mirrors the top engine fixture so criteria actually fire.
    fn planted_inverted_v(start: NaiveDate, n_rally: usize, n_decline: usize) -> Vec<HistoryRecord> {
        let mut out = Vec::new();
        let mut day = 0u64;
        let mut price = 300.0;
        for i in 0..n_rally {
            price = 300.0 + i as f64 * (700.0 / n_rally as f64);
            let noise = 8.0 * (i as f64 / 11.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, (price + noise).max(50.0)));
            day += 1;
        }
        let base = price;
        for j in 1..=n_decline {
            let p = (base - j as f64 * (600.0 / n_decline as f64)).max(50.0);
            let noise = 6.0 * (j as f64 / 9.0).sin();
            let date = (start + chrono::Days::new(day))
                .format("%Y-%m-%d")
                .to_string();
            out.push(record(&date, (p + noise).max(50.0)));
            day += 1;
        }
        out
    }

    #[test]
    fn top_backtest_shallow_history_returns_none() {
        let start = NaiveDate::from_ymd_opt(2022, 1, 1).unwrap();
        let h = planted_inverted_v(start, 40, 10);
        assert!(run_top_backtest(
            "ACME",
            "ACME",
            &h,
            SignalTimeframe::Monthly,
            None,
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            true,
            false,
        )
        .is_none());
    }

    #[test]
    fn top_backtest_no_doctrine_anchors_but_fires() {
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h = planted_inverted_v(start, 700, 250);
        let bt = run_top_backtest(
            "TEST",
            "TEST",
            &h,
            SignalTimeframe::Monthly,
            Some(90),
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            false,
            false,
        )
        .expect("backtest");
        assert!(bt.expectancy.is_none(), "expectancy off by default");
        assert_eq!(bt.criteria.len(), 7);
        assert_eq!(bt.confluence.len(), 3);
        // No doctrine top anchors ever.
        assert!(bt.anchors.is_empty());
        assert!(bt.small_n);
        assert!(bt.caveat.contains("insufficient_anchors"));
        // Top criteria still fired over the selloff.
        let total_firings: usize = bt.criteria.iter().map(|c| c.firings).sum();
        assert!(total_firings > 0, "expected some top firings on a selloff");
    }

    #[test]
    fn top_expectancy_has_price_highs_and_negative_return() {
        // Double inverted-V so the prominence-filtered pivot-high scan retains
        // at least one swing high (decline out > 20%).
        let start = NaiveDate::from_ymd_opt(2016, 1, 1).unwrap();
        let mut h = planted_inverted_v(start, 500, 400);
        let next_start = NaiveDate::from_ymd_opt(2018, 6, 1).unwrap();
        let mut h2 = planted_inverted_v(next_start, 500, 400);
        h.append(&mut h2);
        let bt = run_top_backtest(
            "ACME",
            "ACME",
            &h,
            SignalTimeframe::Monthly,
            Some(120),
            &DEFAULT_CONFLUENCE_THRESHOLDS,
            true,
            false,
        )
        .expect("backtest");
        let exp = bt.expectancy.expect("expectancy populated");
        assert!(!exp.doctrine_anchors_used, "tops have no doctrine anchors");
        assert!(
            !exp.price_structure_anchors.is_empty(),
            "expected price-structure swing highs (stored in the shared field)"
        );
        assert!(!exp.insufficient_anchors, "swing highs should be present");
        assert!(exp.anchors_used > 0);
        assert_eq!(exp.baseline.len(), FORWARD_HORIZONS_DAYS.len());
        assert_eq!(exp.criteria.len(), 7);
        // Every horizon row on the top path carries negative_rate_pct.
        for row in exp.criteria.iter().chain(exp.confluence.iter()) {
            assert_eq!(row.horizons.len(), FORWARD_HORIZONS_DAYS.len());
            for h in &row.horizons {
                if h.samples > 0 {
                    assert!(
                        h.negative_rate_pct.is_some(),
                        "top horizons must report negative_rate_pct"
                    );
                }
            }
        }
        // The high-confluence firings should precede a DECLINE: at the longest
        // horizon, the >=3 confluence mean forward return should be negative for
        // a series engineered to sell off after each top. Find the row.
        let conf3 = exp
            .confluence
            .iter()
            .find(|r| r.threshold == Some(3))
            .expect("confluence>=3 row");
        if let Some(h365) = conf3.horizons.iter().find(|h| h.horizon_days == 365) {
            if let Some(mean) = h365.mean_return_pct {
                assert!(
                    mean < Decimal::ZERO,
                    "top confluence should precede a 1y decline, got {mean}"
                );
            }
        }
    }

    #[test]
    fn price_structure_highs_detects_planted_peak() {
        let start = NaiveDate::from_ymd_opt(2016, 1, 1).unwrap();
        let mut h = planted_inverted_v(start, 500, 400);
        let next_start = NaiveDate::from_ymd_opt(2018, 6, 1).unwrap();
        let mut h2 = planted_inverted_v(next_start, 500, 400);
        h.append(&mut h2);
        let highs = price_structure_highs(&h, PRICE_LOW_PIVOT_WINDOW, PRICE_LOW_PROMINENCE_PCT);
        assert!(!highs.is_empty(), "should detect at least one swing high");
        // Each retained high must be a real bar with a positive price.
        for (idx, _d, p) in &highs {
            assert!(*idx < h.len());
            assert!(*p > Decimal::ZERO);
        }
    }

    #[test]
    fn negative_rate_pct_is_exact() {
        // Three returns: -5, -1, +3 -> 2/3 negative = 66.67%.
        let v = vec![dec!(-5), dec!(-1), dec!(3)];
        let r = negative_rate_pct(&v).unwrap();
        // 2/3 * 100 = 66.6666...
        assert!((r - dec!(66.6666)).abs() < dec!(0.01), "got {r}");
        let pos = positive_rate_pct(&v).unwrap();
        assert!((pos - dec!(33.3333)).abs() < dec!(0.01), "got {pos}");
        assert_eq!(negative_rate_pct(&[]), None);
    }

    #[test]
    fn top_no_lookahead_signal_stable() {
        let start = NaiveDate::from_ymd_opt(2017, 1, 1).unwrap();
        let full = planted_inverted_v(start, 700, 300);
        let i = 820usize.min(full.len() - 1);
        let read_a = cycle_signals::cycle_top_signals("ACME", &full[..=i], SignalTimeframe::Monthly)
            .expect("read at i");
        let mut extended = full[..=i].to_vec();
        for k in 1..=60 {
            let date = (start + chrono::Days::new((i + k) as u64))
                .format("%Y-%m-%d")
                .to_string();
            extended.push(record(&date, 50.0 + k as f64));
        }
        let read_b =
            cycle_signals::cycle_top_signals("ACME", &extended[..=i], SignalTimeframe::Monthly)
                .expect("read at i (extended)");
        let a: Vec<bool> = read_a.criteria.iter().map(|c| c.met).collect();
        let b: Vec<bool> = read_b.criteria.iter().map(|c| c.met).collect();
        assert_eq!(a, b, "top firing-driving criteria at bar i must be lookahead-free");
        assert_eq!(read_a.met_count, read_b.met_count);
    }

    /// No-lookahead invariant for the EXPECTANCY path: the signal read at bar i
    /// over history[..=i] is unchanged after appending future bars, so the
    /// firing INDEX set (which drives expectancy) cannot shift retroactively.
    /// (Forward returns inherently consume future bars — that is the outcome,
    /// not the signal — so we assert the SIGNAL stability that expectancy rests on.)
    #[test]
    fn expectancy_no_lookahead_signal_stable() {
        let start = NaiveDate::from_ymd_opt(2017, 1, 1).unwrap();
        let full = planted_v_bottom(start, 700, 300);
        let i = 820usize.min(full.len() - 1);
        let read_a =
            cycle_signals::cycle_bottom_signals("ACME", &full[..=i], SignalTimeframe::Monthly)
                .expect("read at i");
        let mut extended = full[..=i].to_vec();
        for k in 1..=60 {
            let date = (start + chrono::Days::new((i + k) as u64))
                .format("%Y-%m-%d")
                .to_string();
            extended.push(record(&date, 9000.0 + k as f64));
        }
        let read_b =
            cycle_signals::cycle_bottom_signals("ACME", &extended[..=i], SignalTimeframe::Monthly)
                .expect("read at i (extended)");
        let a: Vec<bool> = read_a.criteria.iter().map(|c| c.met).collect();
        let b: Vec<bool> = read_b.criteria.iter().map(|c| c.met).collect();
        assert_eq!(a, b, "firing-driving criteria at bar i must be lookahead-free");
        assert_eq!(read_a.met_count, read_b.met_count);
    }

    /// Chain `cycles` back-to-back V-bottoms (decline→rally) into one continuous,
    /// strictly date-ascending series. Each bottom is followed by a large rally,
    /// so the expectancy engine has a KNOWN bullish edge to recover.
    fn chained_v_bottoms(
        start: NaiveDate,
        cycles: usize,
        n_decline: usize,
        n_rally: usize,
    ) -> Vec<HistoryRecord> {
        let mut out: Vec<HistoryRecord> = Vec::new();
        let mut cursor = start;
        for _ in 0..cycles {
            let cyc = planted_v_bottom(cursor, n_decline, n_rally);
            // Next cycle starts the day after this cycle's last bar.
            if let Some(last) = cyc.last() {
                let last_date = NaiveDate::parse_from_str(&last.date, "%Y-%m-%d").unwrap();
                cursor = last_date + chrono::Days::new(1);
            }
            out.extend(cyc);
        }
        out
    }

    fn chained_inverted_v(
        start: NaiveDate,
        cycles: usize,
        n_rally: usize,
        n_decline: usize,
    ) -> Vec<HistoryRecord> {
        let mut out: Vec<HistoryRecord> = Vec::new();
        let mut cursor = start;
        for _ in 0..cycles {
            let cyc = planted_inverted_v(cursor, n_rally, n_decline);
            if let Some(last) = cyc.last() {
                let last_date = NaiveDate::parse_from_str(&last.date, "%Y-%m-%d").unwrap();
                cursor = last_date + chrono::Days::new(1);
            }
            out.extend(cyc);
        }
        out
    }

    /// Helper: the lift_vs_baseline_pct of a confluence row at one horizon.
    fn lift_at(row: &ExpectancyRow, horizon: i64) -> Decimal {
        row.horizons
            .iter()
            .find(|h| h.horizon_days == horizon)
            .and_then(|h| h.lift_vs_baseline_pct)
            .unwrap_or_else(|| panic!("no lift at {horizon}d for {}", row.key))
    }

    /// GROUND-TRUTH VALIDATION. Plant a synthetic series with a KNOWN edge and
    /// assert the expectancy engine recovers it — turning "trust the lift number"
    /// into "the methodology is validated against ground truth."
    ///
    /// Construction: 14 back-to-back V-cycles, each a 150-bar decline into a deep
    /// low followed by a 250-bar (~+200%) rally. So the cycle-BOTTOM confluence
    /// fires near each low, shortly BEFORE a large up-move; the cycle-TOP
    /// confluence (on the mirror series) fires near each peak, shortly before a
    /// large DOWN-move. We plant 14 cycles specifically so the headline ≥4/7 row
    /// clears MIN_SIGNIFICANT_FIRINGS (it gets 35 firings) and is therefore NOT
    /// flagged low_firings — the edge is well-sampled, not a 1-2 firing mirage.
    ///
    /// Assertions:
    ///   * BOTTOM ≥4/7: lift_vs_baseline is clearly POSITIVE at the 30/90/180d
    ///     horizons (the planted rally), with double-digit magnitude — the engine
    ///     recovers the planted bullish edge.
    ///   * TOP ≥4/7 (mirror series): lift_vs_baseline is clearly NEGATIVE at the
    ///     same horizons — the engine recovers the planted bearish edge.
    ///   * Both headline rows have ≥20 firings and low_firings == false.
    ///   * low_firings is consistently wired: it equals (firings < MIN) on EVERY
    ///     row of both blocks.
    ///
    /// (365d is deliberately NOT asserted: a 1-year horizon overshoots the ~13mo
    /// cycle and lands in the next decline, which is correct behaviour, not edge.)
    #[test]
    fn synthetic_known_edge_is_recovered_by_expectancy() {
        let bottoms = chained_v_bottoms(NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(), 14, 150, 250);
        let bt = run_backtest(
            "ACME", "ACME", &bottoms, SignalTimeframe::Monthly, Some(120),
            &DEFAULT_CONFLUENCE_THRESHOLDS, true, false,
        )
        .expect("bottom backtest");
        let exp = bt.expectancy.expect("bottom expectancy");

        // low_firings is consistently keyed to the per-row firing count.
        for r in exp.criteria.iter().chain(exp.confluence.iter()) {
            assert_eq!(
                r.low_firings,
                r.firings < MIN_SIGNIFICANT_FIRINGS,
                "bottom row {} low_firings flag must equal firings<{MIN_SIGNIFICANT_FIRINGS}",
                r.key
            );
        }

        let bot4 = exp
            .confluence
            .iter()
            .find(|r| r.threshold == Some(4))
            .expect("bottom ≥4 row");
        // Enough firings planted → NOT flagged low_firings.
        assert!(
            bot4.firings >= MIN_SIGNIFICANT_FIRINGS,
            "planted enough firings, got {}",
            bot4.firings
        );
        assert!(!bot4.low_firings, "well-sampled row must not be low_firings");
        // The planted bullish edge is recovered: positive lift, double-digit.
        for h in [30, 90, 180] {
            let lift = lift_at(bot4, h);
            assert!(
                lift > dec!(5),
                "bottom ≥4 should recover a clear positive edge at {h}d, got lift {lift}"
            );
        }

        // Mirror: planted bearish edge on inverted-V cycles.
        let tops = chained_inverted_v(NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(), 14, 150, 250);
        let btt = run_top_backtest(
            "ACME", "ACME", &tops, SignalTimeframe::Monthly, Some(120),
            &DEFAULT_CONFLUENCE_THRESHOLDS, true, false,
        )
        .expect("top backtest");
        let expt = btt.expectancy.expect("top expectancy");
        for r in expt.criteria.iter().chain(expt.confluence.iter()) {
            assert_eq!(
                r.low_firings,
                r.firings < MIN_SIGNIFICANT_FIRINGS,
                "top row {} low_firings flag must equal firings<{MIN_SIGNIFICANT_FIRINGS}",
                r.key
            );
        }
        let top4 = expt
            .confluence
            .iter()
            .find(|r| r.threshold == Some(4))
            .expect("top ≥4 row");
        assert!(
            top4.firings >= MIN_SIGNIFICANT_FIRINGS,
            "planted enough top firings, got {}",
            top4.firings
        );
        assert!(!top4.low_firings, "well-sampled top row must not be low_firings");
        for h in [30, 90, 180] {
            let lift = lift_at(top4, h);
            assert!(
                lift < dec!(-5),
                "top ≥4 should recover a clear negative edge at {h}d, got lift {lift}"
            );
        }
    }

    // ---- DRIFT-DETRENDED expectancy (the money test) ---------------------

    /// Build a contiguous daily series directly from a log-price path, so the
    /// drift and any planted dips are EXACTLY controllable. Price = exp(logp),
    /// floored to stay positive; dates are one calendar day apart from `start`.
    fn from_log_prices(start: NaiveDate, logp: &[f64]) -> Vec<HistoryRecord> {
        logp.iter()
            .enumerate()
            .map(|(t, &lp)| {
                let date = (start + chrono::Days::new(t as u64))
                    .format("%Y-%m-%d")
                    .to_string();
                record(&date, lp.exp().max(0.01))
            })
            .collect()
    }

    /// Mean forward return (percent) of an expectancy row at one horizon.
    fn mean_at(row: &ExpectancyRow, horizon: i64) -> Decimal {
        row.horizons
            .iter()
            .find(|h| h.horizon_days == horizon)
            .and_then(|h| h.mean_return_pct)
            .unwrap_or_else(|| panic!("no mean at {horizon}d for {}", row.key))
    }

    /// THE MONEY TEST. Drift-detrending must strip an asset's time-local drift
    /// from forward-return expectancy, leaving only the signal's genuine excess
    /// edge — fixing the statistical-honesty gap that a global-mean baseline
    /// cannot close on a NON-stationary trend.
    ///
    /// Construction (all controlled directly, no confluence engine in the loop —
    /// the test calls `build_expectancy` with explicit firing indices, exactly
    /// the production path minus signal detection):
    ///
    ///   EDGE series: two eras. Era 1 (bars 0..900) has a strong constant
    ///   exponential drift (≈ +82% per 30d); era 2 (900..5900) is FLAT. "Firings"
    ///   are clustered in era 1 at the bottoms of small planted V-dips (a +5.13%
    ///   recovery bump on top of the drift). The baseline (every 10th bar, minus
    ///   the regime-transition zone) spans BOTH eras, so its mean drift is far
    ///   lower than era-1 drift — this is the non-stationarity that inflates raw
    ///   lift even though the firings carry only a small real edge.
    ///
    ///   NO-EDGE series: a single era of the SAME pure exponential drift, NO
    ///   dips. Firings are arbitrary mid-history bars with zero real edge.
    ///
    /// Asserts (horizon 30d; measured values in brackets):
    ///   (a) RAW lift is large and dominated by drift — era-1 firings vs a
    ///       both-eras baseline → [+78.4%], almost all the clustering/drift
    ///       artifact (the real edge is only ~+10%).
    ///   (b) DETRENDED lift ≈ the planted excess and is MUCH smaller than raw
    ///       lift. In simple-return space the detrended excess is
    ///       drift_factor·(bump_factor−1) ≈ 1.82·0.0513 ≈ +9.3% [measured +10.07%,
    ///       baseline detrends to ~0] — ~7.8× smaller than the +78% raw lift, with
    ///       ~+68pts of drift inflation removed.
    ///   (c) On the NO-EDGE pure-drift series, detrended lift ≈ 0 [+2.4%] and the
    ///       detrended MEAN return ≈ 0 [~0.00000004%] while the raw mean forward
    ///       return is large [+82.2%] — proving the drift is actually stripped,
    ///       not merely rescaled.
    ///   (+) Detrending reduces the baseline sample size [544 → 507] (early bars
    ///       lack a full trailing year and are dropped) — the documented honesty
    ///       cost.
    #[test]
    fn detrend_strips_local_drift_isolating_planted_edge() {
        let start = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();

        // --- EDGE series: era1 high drift + dips, era2 long & flat. ---
        // Era 2 is made LONG so the ~365-bar regime-transition zone (where a flat
        // bar's trailing window still reaches into the steep era 1 and its local
        // drift is mis-estimated) is only a small fraction of the baseline — that
        // transition mis-estimate is a real, honest artifact of local detrending,
        // diluted here rather than hidden.
        let era1 = 900usize;
        let total = 5900usize;
        let drift_hi = 0.020_f64; // ≈ exp(0.020*30)=1.822 → +82.2% per 30 calendar days
        let mut logp = vec![0.0_f64; total];
        logp[0] = 100.0_f64.ln();
        for t in 1..total {
            let g = if t < era1 { drift_hi } else { 0.0 };
            logp[t] = logp[t - 1] + g;
        }
        // Planted V-dips (the real edge): bottom at each center, recovering to the
        // trend within ±hw bars. depth 0.05 log → exp(0.05)−1 = +5.13% recovery.
        let hw = 10usize;
        let depth = 0.05_f64;
        let centers: Vec<usize> = (450..=850).step_by(50).collect(); // 9 dips, all with ≥365d trailing
        for &c in &centers {
            for off in 0..=hw {
                let sub = depth * (1.0 - off as f64 / hw as f64);
                logp[c + off] -= sub;
                if off > 0 && c >= off {
                    logp[c - off] -= sub; // skip off==0 so the bottom isn't subtracted twice
                }
            }
        }
        let edge = from_log_prices(start, &logp);
        let firing_idx = centers.clone();
        // Baseline = every 10th bar EXCEPT the regime-transition zone, where a
        // bar's trailing year straddles both drift regimes and its local drift is
        // genuinely ill-defined (the documented limitation of any local-drift
        // estimator at a regime change). Excluding it consistently from BOTH raw
        // and detrended baselines keeps the comparison like-for-like; the firings
        // themselves sit safely in clean era-1 interior (full single-regime
        // trailing window, forward window inside era 1).
        let eval_idx: Vec<usize> = (0..total)
            .step_by(10)
            .filter(|&i| i + 35 < era1 || i >= era1 + 400)
            .collect();
        let crit_idx: Vec<(String, String, Vec<usize>)> = Vec::new();
        let conf_idx: Vec<(usize, Vec<usize>)> = vec![(4usize, firing_idx.clone())];

        let raw = build_expectancy(
            &edge, &eval_idx, &crit_idx, &conf_idx, &[], &[], 90, /*detrend=*/ false,
        );
        let det = build_expectancy(
            &edge, &eval_idx, &crit_idx, &conf_idx, &[], &[], 90, /*detrend=*/ true,
        );
        assert!(!raw.detrended);
        assert!(det.detrended);
        assert_eq!(det.detrend_trailing_days, Some(DETREND_TRAILING_DAYS));

        let raw_row = raw.confluence.iter().find(|r| r.threshold == Some(4)).unwrap();
        let det_row = det.confluence.iter().find(|r| r.threshold == Some(4)).unwrap();
        let raw_lift = lift_at(raw_row, 30);
        let det_lift = lift_at(det_row, 30);

        // (a) Raw lift is large and dominated by the (clustered) drift artifact.
        assert!(
            raw_lift > dec!(35),
            "raw lift should be large (drift-inflated), got {raw_lift}"
        );
        // (b) Detrended lift is much smaller than raw lift, and near the planted
        //     excess drift_factor·(bump_factor−1) ≈ +9.3% (band generous for the
        //     simple-return cross-term + minor transition contamination).
        assert!(
            det_lift > dec!(3) && det_lift < dec!(22),
            "detrended lift should isolate the small planted excess (~9%), got {det_lift}"
        );
        assert!(
            det_lift * dec!(2) < raw_lift,
            "detrended lift {det_lift} must be MUCH smaller than raw lift {raw_lift}"
        );
        assert!(
            (raw_lift - det_lift) > dec!(25),
            "detrending must remove the bulk of the drift (~40pts), removed {}",
            raw_lift - det_lift
        );

        // (+) Detrending drops early bars (no full trailing year) → fewer samples.
        let raw_base = raw.baseline.iter().find(|b| b.horizon_days == 30).unwrap();
        let det_base = det.baseline.iter().find(|b| b.horizon_days == 30).unwrap();
        assert!(
            det_base.samples < raw_base.samples,
            "detrended baseline ({}) must drop early no-trailing-history bars vs raw ({})",
            det_base.samples,
            raw_base.samples
        );

        // --- NO-EDGE series: single era of the SAME pure drift, no dips. ---
        let mut logp2 = vec![0.0_f64; total];
        logp2[0] = 100.0_f64.ln();
        for t in 1..total {
            logp2[t] = logp2[t - 1] + drift_hi;
        }
        let flat = from_log_prices(start, &logp2);
        // Firings: arbitrary mid-history bars (≥365d trailing), zero real edge.
        let firing2: Vec<usize> = (500..=850).step_by(50).collect();
        let conf2: Vec<(usize, Vec<usize>)> = vec![(4usize, firing2.clone())];

        let raw2 = build_expectancy(
            &flat, &eval_idx, &crit_idx, &conf2, &[], &[], 90, /*detrend=*/ false,
        );
        let det2 = build_expectancy(
            &flat, &eval_idx, &crit_idx, &conf2, &[], &[], 90, /*detrend=*/ true,
        );
        let raw2_row = raw2.confluence.iter().find(|r| r.threshold == Some(4)).unwrap();
        let det2_row = det2.confluence.iter().find(|r| r.threshold == Some(4)).unwrap();

        // (c) Raw mean forward return is large (pure drift), but detrended lift
        //     AND detrended mean are ≈ 0 — the drift is genuinely stripped.
        assert!(
            mean_at(raw2_row, 30) > dec!(50),
            "no-edge raw 30d mean should be large pure drift, got {}",
            mean_at(raw2_row, 30)
        );
        let det2_lift = lift_at(det2_row, 30);
        assert!(
            det2_lift.abs() < dec!(5),
            "no-edge detrended lift must be ≈0 (drift removed), got {det2_lift}"
        );
        assert!(
            mean_at(det2_row, 30).abs() < dec!(5),
            "no-edge detrended mean must be ≈0 (excess over its own drift), got {}",
            mean_at(det2_row, 30)
        );
        let det2_base = det2.baseline.iter().find(|b| b.horizon_days == 30).unwrap();
        assert!(
            det2_base.mean_return_pct.map(|m| m.abs() < dec!(5)).unwrap_or(false),
            "no-edge detrended baseline mean must be ≈0, got {:?}",
            det2_base.mean_return_pct
        );
    }

    /// Default (no `--detrend`) output must be byte-for-byte unchanged: the
    /// `detrended` / `detrend_trailing_days` fields are omitted from raw JSON.
    #[test]
    fn detrend_off_is_byte_identical_and_fields_omitted() {
        let start = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        let h = planted_v_bottom(start, 600, 200);
        let raw = run_backtest(
            "BTC", "BTC-USD", &h, SignalTimeframe::Monthly, None,
            &DEFAULT_CONFLUENCE_THRESHOLDS, true, false,
        )
        .unwrap();
        let json = serde_json::to_string(&raw).unwrap();
        assert!(
            !json.contains("detrended") && !json.contains("detrend_trailing_days"),
            "raw-mode JSON must not carry the detrend flags"
        );
        // And detrended mode DOES surface them.
        let det = run_backtest(
            "BTC", "BTC-USD", &h, SignalTimeframe::Monthly, None,
            &DEFAULT_CONFLUENCE_THRESHOLDS, true, true,
        )
        .unwrap();
        let djson = serde_json::to_string(&det).unwrap();
        assert!(djson.contains("\"detrended\":true"));
        assert!(djson.contains("\"detrend_trailing_days\":365"));
    }
}
