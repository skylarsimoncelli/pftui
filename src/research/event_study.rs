//! Event-study engine — forward-return statistics for one (signal, asset).
//!
//! For each horizon (5/30/90/180 calendar days) and each event:
//!
//! - the forward return is measured from the event bar's close to the close
//!   of the FIRST bar dated on/after `event_date + horizon` days; an event
//!   is **evaluable** only when that bar exists AND the horizon date is
//!   `<= as_of` (walk-forward: no partially-resolved windows ever enter the
//!   stats);
//! - **MAE/MFE**: the maximum adverse (most negative, clamped at 0) and
//!   maximum favorable (most positive, clamped at 0) excursion of any close
//!   inside the window, relative to the event close;
//! - **overlap exclusion**: walking evaluable events oldest-first, an event
//!   within `horizon` days of the previously KEPT event is excluded from
//!   that horizon's stats (`n_nonoverlap`), so one regime cannot
//!   pseudo-replicate into significance;
//! - **baseline + lift**: the unconditional forward distribution over ALL
//!   evaluable bars from the first event date to `as_of` (same windows,
//!   same series) — `mean_lift = signal mean − baseline mean`, `hit_lift =
//!   hit rate − baseline up-rate`. A signal is only interesting vs its
//!   asset's own drift;
//! - **significance**: an exact two-sided binomial test of the kept-event
//!   up-count against the BASELINE up-rate (not 50%). `significant_5pct`
//!   requires p < 0.05 AND n_nonoverlap >= 10; below 10 the stats carry an
//!   `anecdotal` flag;
//! - **era/regime splits**: per-decade counts + mean returns, and an
//!   above/below-200dma-at-event split, so non-stationarity is visible.
//!
//! All returns are percentages. Internals are `f64` — these are statistics
//! over price ratios, not monetary values.

use chrono::{Duration, NaiveDate};
use serde::Serialize;

use super::registry::SignalFiring;

/// Canonical forward-return horizons (calendar days).
pub const HORIZONS: [i64; 4] = [5, 30, 90, 180];

/// Minimum non-overlapping events for stats to escape the anecdotal flag.
pub const ANECDOTAL_N: usize = 10;

#[derive(Debug, Clone, Serialize)]
pub struct EraSplit {
    /// "2000s", "2010s", "2020s", ...
    pub era: String,
    pub n: usize,
    pub mean_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegimeSplit {
    /// "above-200dma" | "below-200dma" at event time.
    pub regime: String,
    pub n: usize,
    pub mean_pct: f64,
}

/// Full per-horizon statistics for one (signal, asset, as_of).
#[derive(Debug, Clone, Serialize)]
pub struct HorizonStats {
    pub horizon_days: i64,
    /// Events dated <= as_of.
    pub n_total: usize,
    /// Events whose forward window fully resolved by as_of and inside data.
    pub n_evaluable: usize,
    /// Evaluable events surviving overlap exclusion — the stats sample.
    pub n_nonoverlap: usize,
    pub hit_rate: Option<f64>,
    pub baseline_hit_rate: Option<f64>,
    pub hit_lift: Option<f64>,
    pub mean_pct: Option<f64>,
    pub baseline_mean_pct: Option<f64>,
    pub mean_lift: Option<f64>,
    pub median_pct: Option<f64>,
    pub p25: Option<f64>,
    pub p75: Option<f64>,
    /// Mean / worst maximum adverse excursion (<= 0).
    pub mae_mean: Option<f64>,
    pub mae_worst: Option<f64>,
    /// Mean maximum favorable excursion (>= 0).
    pub mfe_mean: Option<f64>,
    /// Exact two-sided binomial p of the up-count vs the baseline up-rate.
    pub p_value: Option<f64>,
    pub significant_5pct: bool,
    /// n_nonoverlap < ANECDOTAL_N — render stats as anecdotes, not evidence.
    pub anecdotal: bool,
    pub era_splits: Vec<EraSplit>,
    pub regime_splits: Vec<RegimeSplit>,
}

/// Per-event outcome at one horizon (the "show me the 12 instances" surface).
#[derive(Debug, Clone, Serialize)]
pub struct EventHorizonOutcome {
    pub horizon_days: i64,
    pub evaluable: bool,
    /// Included in stats after overlap exclusion.
    pub kept: bool,
    pub return_pct: Option<f64>,
    pub mae_pct: Option<f64>,
    pub mfe_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub date: String,
    pub detail: String,
    pub outcomes: Vec<EventHorizonOutcome>,
}

/// The full study output for one (signal, asset, as_of).
#[derive(Debug, Clone, Serialize)]
pub struct EventStudy {
    pub as_of: String,
    pub horizons: Vec<HorizonStats>,
    pub events: Vec<EventRow>,
}

/// Run the event study. `dates`/`closes`/`sma200` are the asset's daily
/// series oldest-first; `events` the signal's dated firings oldest-first;
/// `as_of` a YYYY-MM-DD walk-forward cutoff (events after it are excluded;
/// forward windows must fully resolve by it).
pub fn study(
    dates: &[String],
    closes: &[f64],
    sma200: &[Option<f64>],
    events: &[SignalFiring],
    as_of: &str,
) -> EventStudy {
    let parsed: Vec<Option<NaiveDate>> = dates
        .iter()
        .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();
    let as_of_date = NaiveDate::parse_from_str(as_of, "%Y-%m-%d").ok();

    // Events <= as_of with a resolvable bar index.
    let in_scope: Vec<(&SignalFiring, usize, NaiveDate)> = events
        .iter()
        .filter(|e| e.date.as_str() <= as_of)
        .filter_map(|e| {
            let idx = dates.iter().position(|d| *d == e.date)?;
            let ed = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d").ok()?;
            Some((e, idx, ed))
        })
        .collect();

    let mut event_rows: Vec<EventRow> = in_scope
        .iter()
        .map(|(e, _, _)| EventRow {
            date: e.date.clone(),
            detail: e.detail.clone(),
            outcomes: Vec::new(),
        })
        .collect();

    let mut horizons = Vec::with_capacity(HORIZONS.len());
    for &h in &HORIZONS {
        let mut outcomes: Vec<(usize, Outcome)> = Vec::new(); // (event pos, outcome)
        for (pos, (_, idx, ed)) in in_scope.iter().enumerate() {
            let o = forward_outcome(&parsed, closes, *idx, *ed, h, as_of_date);
            outcomes.push((pos, o));
        }

        // Overlap exclusion among evaluable events.
        let mut kept: Vec<usize> = Vec::new(); // positions into in_scope
        let mut last_kept: Option<NaiveDate> = None;
        for (pos, o) in &outcomes {
            if o.ret.is_none() {
                continue;
            }
            let ed = in_scope[*pos].2;
            let ok = match last_kept {
                Some(prev) => (ed - prev).num_days() >= h,
                None => true,
            };
            if ok {
                kept.push(*pos);
                last_kept = Some(ed);
            }
        }
        let kept_set: std::collections::HashSet<usize> = kept.iter().copied().collect();

        // Fill per-event rows for this horizon.
        for (pos, o) in &outcomes {
            event_rows[*pos].outcomes.push(EventHorizonOutcome {
                horizon_days: h,
                evaluable: o.ret.is_some(),
                kept: kept_set.contains(pos),
                return_pct: o.ret,
                mae_pct: o.mae,
                mfe_pct: o.mfe,
            });
        }

        let n_total = in_scope.len();
        let n_evaluable = outcomes.iter().filter(|(_, o)| o.ret.is_some()).count();

        // Baseline: unconditional forward distribution over all evaluable
        // bars from the first event date to as_of.
        let baseline = in_scope.first().map(|(_, first_idx, _)| {
            let mut rets: Vec<f64> = Vec::new();
            for i in *first_idx..closes.len() {
                if let Some(ed) = parsed[i] {
                    let o = forward_outcome(&parsed, closes, i, ed, h, as_of_date);
                    if let Some(r) = o.ret {
                        rets.push(r);
                    }
                }
            }
            rets
        });
        let (baseline_mean, baseline_up) = match &baseline {
            Some(rets) if !rets.is_empty() => (
                Some(mean(rets)),
                Some(rets.iter().filter(|r| **r > 0.0).count() as f64 / rets.len() as f64),
            ),
            _ => (None, None),
        };

        // Stats over kept events.
        let kept_rets: Vec<f64> = kept
            .iter()
            .filter_map(|pos| outcomes[*pos].1.ret)
            .collect();
        let kept_maes: Vec<f64> = kept
            .iter()
            .filter_map(|pos| outcomes[*pos].1.mae)
            .collect();
        let kept_mfes: Vec<f64> = kept
            .iter()
            .filter_map(|pos| outcomes[*pos].1.mfe)
            .collect();
        let n_nonoverlap = kept_rets.len();

        let hit_rate = (!kept_rets.is_empty()).then(|| {
            kept_rets.iter().filter(|r| **r > 0.0).count() as f64 / kept_rets.len() as f64
        });
        let mean_pct = (!kept_rets.is_empty()).then(|| mean(&kept_rets));
        let median_pct = percentile(&kept_rets, 50.0);
        let p25 = percentile(&kept_rets, 25.0);
        let p75 = percentile(&kept_rets, 75.0);
        let mae_mean = (!kept_maes.is_empty()).then(|| mean(&kept_maes));
        let mae_worst = kept_maes.iter().copied().fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.min(v)))
        });
        let mfe_mean = (!kept_mfes.is_empty()).then(|| mean(&kept_mfes));

        let hits = kept_rets.iter().filter(|r| **r > 0.0).count();
        let p_value = match (baseline_up, n_nonoverlap) {
            (Some(p0), n) if n > 0 => Some(binomial_two_sided_p(n, hits, p0)),
            _ => None,
        };
        let anecdotal = n_nonoverlap < ANECDOTAL_N;
        let significant_5pct =
            !anecdotal && p_value.map(|p| p < 0.05).unwrap_or(false);

        // Era + regime splits over kept events.
        let mut era_map: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
        let mut above: Vec<f64> = Vec::new();
        let mut below: Vec<f64> = Vec::new();
        for pos in &kept {
            let (e, idx, _) = &in_scope[*pos];
            let Some(r) = outcomes[*pos].1.ret else {
                continue;
            };
            if e.date.len() >= 4 {
                let era = format!("{}0s", &e.date[..3]);
                era_map.entry(era).or_default().push(r);
            }
            match sma200.get(*idx).copied().flatten() {
                Some(s) if s > 0.0 => {
                    if closes[*idx] >= s {
                        above.push(r);
                    } else {
                        below.push(r);
                    }
                }
                _ => {}
            }
        }
        let era_splits = era_map
            .into_iter()
            .map(|(era, v)| EraSplit {
                era,
                n: v.len(),
                mean_pct: mean(&v),
            })
            .collect();
        let mut regime_splits = Vec::new();
        if !above.is_empty() {
            regime_splits.push(RegimeSplit {
                regime: "above-200dma".to_string(),
                n: above.len(),
                mean_pct: mean(&above),
            });
        }
        if !below.is_empty() {
            regime_splits.push(RegimeSplit {
                regime: "below-200dma".to_string(),
                n: below.len(),
                mean_pct: mean(&below),
            });
        }

        horizons.push(HorizonStats {
            horizon_days: h,
            n_total,
            n_evaluable,
            n_nonoverlap,
            hit_rate,
            baseline_hit_rate: baseline_up,
            hit_lift: match (hit_rate, baseline_up) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            },
            mean_pct,
            baseline_mean_pct: baseline_mean,
            mean_lift: match (mean_pct, baseline_mean) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            },
            median_pct,
            p25,
            p75,
            mae_mean,
            mae_worst,
            mfe_mean,
            p_value,
            significant_5pct,
            anecdotal,
            era_splits,
            regime_splits,
        });
    }

    EventStudy {
        as_of: as_of.to_string(),
        horizons,
        events: event_rows,
    }
}

struct Outcome {
    ret: Option<f64>,
    mae: Option<f64>,
    mfe: Option<f64>,
}

/// Forward return + MAE/MFE for one bar at one horizon. None when the
/// window is not fully resolved by `as_of` or runs off the data.
fn forward_outcome(
    parsed: &[Option<NaiveDate>],
    closes: &[f64],
    idx: usize,
    event_date: NaiveDate,
    horizon_days: i64,
    as_of: Option<NaiveDate>,
) -> Outcome {
    let none = Outcome {
        ret: None,
        mae: None,
        mfe: None,
    };
    let target = event_date + Duration::days(horizon_days);
    if let Some(cutoff) = as_of {
        if target > cutoff {
            return none;
        }
    }
    // First bar dated on/after the target (binary search — dates ascending).
    let mut lo = idx + 1;
    let mut hi = parsed.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        match parsed[mid] {
            Some(d) if d < target => lo = mid + 1,
            _ => hi = mid,
        }
    }
    let j = lo;
    if j >= parsed.len() || parsed[j].is_none() {
        return none;
    }
    let base = closes[idx];
    if base <= 0.0 {
        return none;
    }
    let ret = (closes[j] / base - 1.0) * 100.0;
    let mut min_exc = 0.0_f64;
    let mut max_exc = 0.0_f64;
    for c in closes.iter().take(j + 1).skip(idx + 1) {
        let exc = (c / base - 1.0) * 100.0;
        min_exc = min_exc.min(exc);
        max_exc = max_exc.max(exc);
    }
    Outcome {
        ret: Some(ret),
        mae: Some(min_exc),
        mfe: Some(max_exc),
    }
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

/// Linear-interpolation percentile (0-100) of an unsorted sample.
fn percentile(v: &[f64], pct: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if s.len() == 1 {
        return Some(s[0]);
    }
    let rank = pct / 100.0 * (s.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    Some(s[lo] + (s[hi] - s[lo]) * frac)
}

/// Exact two-sided binomial test: P over all outcomes x in 0..=n whose
/// point probability under Binomial(n, p0) does not exceed the observed
/// outcome's point probability (standard small-sample two-sided definition).
pub fn binomial_two_sided_p(n: usize, k: usize, p0: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    let p0 = p0.clamp(1e-9, 1.0 - 1e-9);
    // ln factorials 0..=n.
    let mut ln_fact = Vec::with_capacity(n + 1);
    ln_fact.push(0.0_f64);
    for i in 1..=n {
        let prev = ln_fact[i - 1];
        ln_fact.push(prev + (i as f64).ln());
    }
    let ln_pmf = |x: usize| -> f64 {
        ln_fact[n] - ln_fact[x] - ln_fact[n - x]
            + x as f64 * p0.ln()
            + (n - x) as f64 * (1.0 - p0).ln()
    };
    let obs = ln_pmf(k);
    let tol = 1e-7; // relative tolerance for pmf ties
    let mut p = 0.0;
    for x in 0..=n {
        if ln_pmf(x) <= obs + tol {
            p += ln_pmf(x).exp();
        }
    }
    p.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Days;

    fn dates(n: usize) -> Vec<String> {
        let start = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap_or_default();
        (0..n)
            .map(|i| (start + Days::new(i as u64)).format("%Y-%m-%d").to_string())
            .collect()
    }

    fn firing(date: &str) -> SignalFiring {
        SignalFiring {
            date: date.to_string(),
            detail: "test".to_string(),
        }
    }

    #[test]
    fn known_returns_produce_exact_stats() {
        // Daily series doubling slope after day 100: event at day 50, close
        // 150; 5d later (day 55) close 155 → +3.333..%.
        let d = dates(400);
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + i as f64).collect();
        let sma = vec![None; 400];
        let events = vec![firing(&d[50])];
        let s = study(&d, &closes, &sma, &events, &d[399]);
        let h5 = &s.horizons[0];
        assert_eq!(h5.horizon_days, 5);
        assert_eq!(h5.n_total, 1);
        assert_eq!(h5.n_evaluable, 1);
        assert_eq!(h5.n_nonoverlap, 1);
        let expected = (155.0 / 150.0 - 1.0) * 100.0;
        assert!((h5.mean_pct.unwrap_or(0.0) - expected).abs() < 1e-9);
        assert_eq!(h5.hit_rate, Some(1.0));
        assert!(h5.anecdotal, "n=1 must be anecdotal");
        assert!(!h5.significant_5pct, "anecdotal stats can never be significant");
        // Monotonic series: MAE 0 (never below entry), MFE = the return.
        assert_eq!(h5.mae_mean, Some(0.0));
        assert!((h5.mfe_mean.unwrap_or(0.0) - expected).abs() < 1e-9);
    }

    #[test]
    fn mae_mfe_capture_excursions_inside_the_window() {
        // Flat 100, dip to 90 mid-window, recover to 105 at the horizon bar.
        let d = dates(50);
        let mut closes = vec![100.0; 50];
        closes[12] = 90.0; // adverse excursion -10%
        for c in closes.iter_mut().take(50).skip(20) {
            *c = 105.0;
        }
        let sma = vec![None; 50];
        let events = vec![firing(&d[10])];
        let s = study(&d, &closes, &sma, &events, &d[49]);
        let h5 = &s.horizons[0]; // 5d window covers bars 11..=15 (dip at 12)
        assert!((h5.mae_mean.unwrap_or(9.9) - (-10.0)).abs() < 1e-6);
        assert!((h5.mae_worst.unwrap_or(9.9) - (-10.0)).abs() < 1e-6);
        let h30 = &s.horizons[1]; // 30d window covers the recovery too
        assert!((h30.mfe_mean.unwrap_or(0.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn overlap_exclusion_drops_clustered_events() {
        let d = dates(400);
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + i as f64).collect();
        let sma = vec![None; 400];
        // Three events 2 days apart: at h=5 only the 1st and 3rd survive
        // (3rd is 4 days after the 1st — still inside 5d → excluded too).
        let events = vec![firing(&d[50]), firing(&d[52]), firing(&d[54])];
        let s = study(&d, &closes, &sma, &events, &d[399]);
        let h5 = &s.horizons[0];
        assert_eq!(h5.n_total, 3);
        assert_eq!(h5.n_evaluable, 3);
        assert_eq!(h5.n_nonoverlap, 1, "events 2 and 3 overlap event 1's window");
        // A 4th event 6 days out would survive.
        let events = vec![firing(&d[50]), firing(&d[52]), firing(&d[56])];
        let s = study(&d, &closes, &sma, &events, &d[399]);
        assert_eq!(s.horizons[0].n_nonoverlap, 2);
    }

    #[test]
    fn as_of_excludes_unresolved_windows_and_future_events() {
        let d = dates(400);
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + i as f64).collect();
        let sma = vec![None; 400];
        // Event at day 300; as_of day 303 → no horizon resolved.
        let events = vec![firing(&d[300]), firing(&d[350])];
        let s = study(&d, &closes, &sma, &events, &d[303]);
        let h5 = &s.horizons[0];
        assert_eq!(h5.n_total, 1, "future event (day 350) excluded entirely");
        assert_eq!(h5.n_evaluable, 0, "5d window not resolved by as_of");
        // as_of at day 306: the 5d window (target day 305) is resolved.
        let s = study(&d, &closes, &sma, &events, &d[306]);
        assert_eq!(s.horizons[0].n_evaluable, 1);
        // 30d horizon still unresolved.
        assert_eq!(s.horizons[1].n_evaluable, 0);
    }

    #[test]
    fn baseline_reflects_unconditional_drift() {
        // +1/day forever: every 5d forward return is positive → baseline
        // up-rate 1.0, and a signal with the same distribution has 0 lift.
        let d = dates(300);
        let closes: Vec<f64> = (0..300).map(|i| 100.0 + i as f64).collect();
        let sma = vec![None; 300];
        let events = vec![firing(&d[50]), firing(&d[100]), firing(&d[150])];
        let s = study(&d, &closes, &sma, &events, &d[299]);
        let h5 = &s.horizons[0];
        assert_eq!(h5.baseline_hit_rate, Some(1.0));
        assert_eq!(h5.hit_lift, Some(0.0));
        assert!(h5.baseline_mean_pct.unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn binomial_p_matches_hand_computed_values() {
        // n=10, k=8, p0=0.5 → two-sided p = 2*(45+10+1)/1024 = 0.109375.
        let p = binomial_two_sided_p(10, 8, 0.5);
        assert!((p - 0.109375).abs() < 1e-9, "got {p}");
        // n=10, k=10, p0=0.5 → 2/1024 = 0.001953125.
        let p = binomial_two_sided_p(10, 10, 0.5);
        assert!((p - 0.001953125).abs() < 1e-9, "got {p}");
        // k exactly at the mode → p ~ 1.
        let p = binomial_two_sided_p(10, 5, 0.5);
        assert!(p > 0.99);
    }

    #[test]
    fn significance_requires_n_and_low_p() {
        // 30 alternating up/down steps with events placed only on bars
        // followed by an up-move — hit rate 1.0 vs baseline ~0.5.
        let n = 600;
        let d = dates(n);
        let mut closes = vec![100.0; n];
        for i in 1..n {
            // Sawtooth: up 2 on even bars, down 1 on odd → long-term drift up
            // but plenty of negative 5d windows.
            closes[i] = closes[i - 1] + if i % 2 == 0 { 2.0 } else { -1.0 };
        }
        // Events every 12 days starting day 24 — all even bars.
        let events: Vec<SignalFiring> = (2..40).map(|k| firing(&d[k * 12])).collect();
        let s = study(&d, &closes, &sma_none(n), &events, &d[n - 1]);
        let h5 = &s.horizons[0];
        assert!(h5.n_nonoverlap >= ANECDOTAL_N);
        assert!(!h5.anecdotal);
        // Deterministic: p-value exists and stats line up with baseline.
        assert!(h5.p_value.is_some());
        assert!(h5.baseline_hit_rate.is_some());
    }

    fn sma_none(n: usize) -> Vec<Option<f64>> {
        vec![None; n]
    }

    #[test]
    fn era_and_regime_splits_partition_kept_events() {
        let n = 900;
        let d = dates(n); // 2020-01-01 .. 2022-06
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64) * 0.1).collect();
        // SMA: defined, below price for first half, above for second half.
        let sma: Vec<Option<f64>> = (0..n)
            .map(|i| {
                if i < 450 {
                    Some(50.0)
                } else {
                    Some(1e6)
                }
            })
            .collect();
        let events = vec![firing(&d[100]), firing(&d[200]), firing(&d[500])];
        let s = study(&d, &closes, &sma, &events, &d[n - 1]);
        let h5 = &s.horizons[0];
        let era_n: usize = h5.era_splits.iter().map(|e| e.n).sum();
        assert_eq!(era_n, h5.n_nonoverlap);
        let reg_n: usize = h5.regime_splits.iter().map(|r| r.n).sum();
        assert_eq!(reg_n, h5.n_nonoverlap);
        assert!(h5
            .regime_splits
            .iter()
            .any(|r| r.regime == "below-200dma" && r.n == 1));
    }

    #[test]
    fn determinism_same_inputs_same_json() {
        let d = dates(500);
        let closes: Vec<f64> =
            (0..500).map(|i| 100.0 + 10.0 * ((i as f64) / 9.0).sin()).collect();
        let sma = vec![Some(100.0); 500];
        let events: Vec<SignalFiring> = (1..20).map(|k| firing(&d[k * 23])).collect();
        let a = study(&d, &closes, &sma, &events, &d[499]);
        let b = study(&d, &closes, &sma, &events, &d[499]);
        assert_eq!(
            serde_json::to_string(&a).unwrap_or_default(),
            serde_json::to_string(&b).unwrap_or_default()
        );
    }
}
