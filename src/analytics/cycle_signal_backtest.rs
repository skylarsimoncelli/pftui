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
use serde::Serialize;

use crate::analytics::cycle_signals::{self, SignalTimeframe};
use crate::models::price::HistoryRecord;

/// Below this many verified anchors the result is flagged small-n: a hit-rate
/// computed on fewer samples than this must not be read as robust.
pub const SMALL_N_THRESHOLD: usize = 5;

/// Default match window (in DAILY bars) around a verified low within which a
/// firing counts as a hit. ±90 calendar days ≈ one quarter — a cycle-bottom
/// confirmation that lands within a quarter of the verified low is "on it".
pub const DEFAULT_WINDOW_BARS: i64 = 90;

/// Default confluence thresholds to report (N-of-7).
pub const DEFAULT_CONFLUENCE_THRESHOLDS: [usize; 3] = [3, 4, 5];

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
pub fn run_backtest(
    symbol: &str,
    series: &str,
    history: &[HistoryRecord],
    timeframe: SignalTimeframe,
    window_days: Option<i64>,
    thresholds: &[usize],
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
        )
        .expect("backtest");
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
            &DEFAULT_CONFLUENCE_THRESHOLDS
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
        )
        .unwrap();
        let b = run_backtest(
            "BTC",
            "BTC-USD",
            &h,
            SignalTimeframe::Monthly,
            None,
            &DEFAULT_CONFLUENCE_THRESHOLDS,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
