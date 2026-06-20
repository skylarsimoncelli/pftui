//! CUSUM change-point detection — when did the return regime structurally
//! break? Distinguishes a healthy dip inside an intact trend from "the drift
//! just flipped" (the single most decision-relevant call for a dip-accumulator).
//!
//! Input: daily returns. All callers (`risk-dashboard`, `regime-break`,
//! `environment`) pass SIMPLE returns (`p₁/p₀−1`); the test is σ-normalised
//! (`k`/`h` scaled by the running σ) so the log↔simple choice only shifts the
//! baseline marginally, and SIMPLE is the pinned convention. (Hurst, by
//! contrast, requires LOG returns for range additivity — a different contract.)
//!
//! Page's two-sided cumulative-sum test on daily returns: with reference mean
//! `μ₀`, slack `k` (half the shift to detect) and decision threshold `h`,
//!   S⁺ₜ = max(0, S⁺ₜ₋₁ + (xₜ − μ₀) − k),   alarm when S⁺ > h (drift up-shift)
//!   S⁻ₜ = max(0, S⁻ₜ₋₁ − (xₜ − μ₀) − k),   alarm when S⁻ > h (drift down-shift)
//! On alarm the change-point is the last bar where that CUSUM was zero (the bar
//! just BEFORE the excursion started), and the CUSUM resets. The reference
//! `μ₀`/`σ` are **causal expanding estimates** — computed only from returns
//! STRICTLY BEFORE the current bar — so a later regime shift can never reach
//! back and contaminate the baseline (which would manufacture phantom
//! opposite-sign breaks in an otherwise stable stretch). `k`/`h` are scaled by
//! that running σ so the test is unit-free. All `f64` (return statistics).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ChangePoint {
    pub date: String,
    /// "up-shift" (drift turned more positive) | "down-shift" (turned negative).
    pub direction: String,
    pub bars_ago: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegimeBreak {
    pub n_obs: usize,
    /// Reference mean daily return (percent) and stddev used to scale k/h.
    pub mean_return_pct: f64,
    pub sigma_pct: f64,
    pub k_sigma: f64,
    pub h_sigma: f64,
    /// All detected change-points (chronological).
    pub change_points: Vec<ChangePoint>,
    /// Most recent change-point (None if the regime never broke in-window).
    pub last_change: Option<ChangePoint>,
    /// Current accumulating CUSUM as a fraction of the threshold h (0..1+),
    /// i.e. how close a NEW break is to firing right now.
    pub building_up_pct: f64,
    pub building_down_pct: f64,
    pub interpretation: String,
}

/// Detect return-regime change-points via a two-sided CUSUM. `k_sigma` is the
/// slack (default 0.5 — detects ~1σ drift shifts), `h_sigma` the alarm
/// threshold (default 5). Returns `None` with too little data.
pub fn detect_regime_breaks(
    dates: &[String],
    returns: &[f64],
    k_sigma: f64,
    h_sigma: f64,
) -> Option<RegimeBreak> {
    let n = returns.len();
    if n < 30 || dates.len() < n {
        return None;
    }
    // Whole-window σ used only as a FLOOR for the early expanding estimate (so a
    // freak-quiet warm-up can't make k/h absurdly tiny). The reference used in
    // the recursion is the causal expanding one below.
    let gmean = returns.iter().sum::<f64>() / n as f64;
    let gvar = returns.iter().map(|r| (r - gmean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    let gsigma = gvar.sqrt();
    if gsigma <= 0.0 {
        return None;
    }
    // Reference window: a TRAILING, causal window of recent returns (≈6 months)
    // — adaptive to the prevailing trend (so a regime break is measured vs the
    // RECENT baseline, not the whole-history average) yet contains no future
    // data. Warm up before raising alarms.
    let w_ref = 120usize.min(n / 2).max(30);
    let warmup = 30usize.min(n / 2).max(2);

    let mut sp = 0.0; // S+
    let mut sn = 0.0; // S-
    let mut sp_zero_idx = 0usize; // last bar S+ was 0
    let mut sn_zero_idx = 0usize;
    let mut change_points = Vec::new();
    // Rolling sum / sum-of-squares over the trailing reference window [i−w_ref, i).
    let mut rsum = 0.0;
    let mut rsq = 0.0;
    let mut cnt = 0usize;
    let mut last_h = h_sigma * gsigma; // for the trailing building% normalization
    #[allow(clippy::needless_range_loop)] // index tracks the change-point bar (sp_zero_idx)
    for i in 0..n {
        // Causal reference from the trailing window of data before bar i.
        let mu = if cnt > 0 { rsum / cnt as f64 } else { returns[i] };
        let sigma = if cnt > 1 {
            ((rsq / cnt as f64) - mu * mu).max(0.0).sqrt().max(gsigma * 0.25)
        } else {
            gsigma
        };
        let k = k_sigma * sigma;
        let h = h_sigma * sigma;
        last_h = h;
        let dev = returns[i] - mu;
        sp = (sp + dev - k).max(0.0);
        sn = (sn - dev - k).max(0.0);
        if sp == 0.0 {
            sp_zero_idx = i;
        }
        if sn == 0.0 {
            sn_zero_idx = i;
        }
        // Only raise alarms once the reference is warmed up.
        if cnt >= warmup {
            if sp > h {
                change_points.push(ChangePoint {
                    date: dates[sp_zero_idx].clone(),
                    direction: "up-shift".to_string(),
                    bars_ago: n - 1 - sp_zero_idx,
                });
                sp = 0.0;
                sp_zero_idx = i;
            }
            if sn > h {
                change_points.push(ChangePoint {
                    date: dates[sn_zero_idx].clone(),
                    direction: "down-shift".to_string(),
                    bars_ago: n - 1 - sn_zero_idx,
                });
                sn = 0.0;
                sn_zero_idx = i;
            }
        }
        // Slide the trailing window forward: include bar i (now "before" the
        // next bar), and drop the oldest once the window is full — keeps μ₀/σ
        // causal AND adaptive to the recent trend.
        rsum += returns[i];
        rsq += returns[i] * returns[i];
        cnt += 1;
        if cnt > w_ref {
            let old = returns[i - w_ref];
            rsum -= old;
            rsq -= old * old;
            cnt -= 1;
        }
    }
    let last_change = change_points.last().cloned();
    let h = last_h;
    let sigma = gsigma;
    let building_up_pct = (sp / h * 100.0).min(999.0);
    let building_down_pct = (sn / h * 100.0).min(999.0);

    let interpretation = match &last_change {
        None => "no structural drift break detected in-window — the return regime has been stable".to_string(),
        Some(cp) => {
            let dir = if cp.direction == "up-shift" {
                "drift last turned UP (trend strengthening)"
            } else {
                "drift last turned DOWN (trend weakening/breaking)"
            };
            let forming = if building_up_pct >= 60.0 {
                " — a fresh UP-shift is forming"
            } else if building_down_pct >= 60.0 {
                " — a fresh DOWN-shift is forming (a dip may be becoming a regime change)"
            } else {
                ""
            };
            format!(
                "last regime break {} bars ago ({}): {}{}",
                cp.bars_ago, cp.date, dir, forming
            )
        }
    };

    Some(RegimeBreak {
        n_obs: n,
        mean_return_pct: gmean * 100.0,
        sigma_pct: sigma * 100.0,
        k_sigma,
        h_sigma,
        change_points,
        last_change,
        building_up_pct,
        building_down_pct,
        interpretation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| {
                let base = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
                (base + chrono::Duration::days(i as i64))
                    .format("%Y-%m-%d")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn detects_a_clear_mean_shift() {
        // First half flat near zero, second half strong positive drift → an
        // up-shift change-point near the midpoint.
        let mut rets = vec![0.0001f64; 100];
        for r in rets.iter_mut().skip(100 / 2) {
            *r = 0.03; // big positive drift
        }
        // add tiny alternating noise so sigma > 0
        for (i, r) in rets.iter_mut().enumerate() {
            *r += if i % 2 == 0 { 0.0005 } else { -0.0005 };
        }
        let d = dates(rets.len());
        let rb = detect_regime_breaks(&d, &rets, 0.5, 5.0).unwrap();
        assert!(!rb.change_points.is_empty(), "should detect a shift");
        assert!(rb.change_points.iter().any(|c| c.direction == "up-shift"));
    }

    #[test]
    fn stable_series_has_no_break() {
        // Pure low-amplitude alternating noise, zero drift → no change-point.
        let rets: Vec<f64> = (0..200).map(|i| if i % 2 == 0 { 0.002 } else { -0.002 }).collect();
        let d = dates(rets.len());
        let rb = detect_regime_breaks(&d, &rets, 0.5, 5.0).unwrap();
        assert!(rb.change_points.is_empty(), "no break expected, got {:?}", rb.change_points.len());
        assert!(rb.last_change.is_none());
    }

    #[test]
    fn too_little_data_none() {
        let d = dates(10);
        assert!(detect_regime_breaks(&d, &[0.01; 10], 0.5, 5.0).is_none());
    }
}
