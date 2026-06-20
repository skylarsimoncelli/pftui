//! Hurst exponent via Rescaled-Range (R/S) analysis — a persistence/regime
//! gauge, distinct from the cycle-COUNTING Hurst in `cycle_engine.rs`.
//!
//! H answers "does this series trend or mean-revert?":
//! - H > 0.5  persistent/trending — an up move makes the next up move more
//!   likely; trend-following has an edge, accumulate dips and ride.
//! - H ≈ 0.5  random walk — no autocorrelation edge; trend signals are noise.
//! - H < 0.5  anti-persistent/mean-reverting — fade extremes, expect chop.
//!
//! Computed on LOG-RETURNS (R/S is defined on a stationary/additive series;
//! raw-price R/S spuriously inflates H toward 1). The naive R/S slope is biased
//! high on finite samples, so we apply the Anis-Lloyd / Peters expected-value
//! correction. All `f64` (a statistical exponent, not money).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HurstResult {
    pub n_obs: usize,
    /// Window sizes used in the R/S regression.
    pub windows: Vec<usize>,
    /// Naive R/S slope (biased high on finite samples).
    pub h_uncorrected: f64,
    /// Anis-Lloyd/Peters-corrected Hurst exponent — the one to read.
    pub h: f64,
    /// DFA-1 scaling exponent α (≈H) — an INDEPENDENT persistence estimate with
    /// a different bias structure than R/S (R/S is biased ~0.48 under the null,
    /// DFA-1 ~0.51). Two independent estimators agreeing is a stronger regime
    /// read than either alone; a genuine divergence (beyond their joint sampling
    /// noise) flags an unstable estimate. (Note: since the input is already
    /// log-returns, neither faces a raw-price trend — DFA-1 is a second view,
    /// not a trend filter.)
    pub dfa_alpha: Option<f64>,
    /// Agreement between the two estimators.
    pub agreement: String,
    /// "trending" | "random-walk" | "mean-reverting".
    pub regime: String,
    pub interpretation: String,
}

/// DFA-1 (Detrended Fluctuation Analysis) scaling exponent over a (stationary)
/// series. Integrates the mean-subtracted series, then for each window size
/// measures the RMS of the linearly-detrended fluctuation; `F(n) ~ n^α` and the
/// log-log slope is α (≈ the Hurst exponent, centered on 0.5 for white noise).
pub fn dfa_alpha(series: &[f64]) -> Option<f64> {
    let n = series.len();
    if n < 64 {
        return None;
    }
    // Integrate the mean-subtracted series (the DFA "profile").
    let mean = series.iter().sum::<f64>() / n as f64;
    let mut y = vec![0.0f64; n];
    let mut acc = 0.0;
    for (i, &x) in series.iter().enumerate() {
        acc += x - mean;
        y[i] = acc;
    }
    let candidate = [8usize, 16, 32, 64, 128, 256];
    let mut pts = Vec::new();
    for &w in &candidate {
        if w > n / 4 {
            break; // want at least ~4 segments for a stable fluctuation
        }
        let f = dfa_fluctuation(&y, w);
        if f > 0.0 {
            pts.push(((w as f64).ln(), f.ln()));
        }
    }
    if pts.len() < 3 {
        return None;
    }
    let a = ols_slope(&pts);
    a.is_finite().then_some(a)
}

/// RMS of the linearly-detrended fluctuation over non-overlapping length-`w`
/// segments of the integrated profile `y`.
fn dfa_fluctuation(y: &[f64], w: usize) -> f64 {
    let segs = y.len() / w;
    if segs == 0 {
        return 0.0;
    }
    let mut sumsq = 0.0;
    let mut cnt = 0usize;
    for s in 0..segs {
        let seg = &y[s * w..(s + 1) * w];
        let (a, b) = linfit(seg);
        for (i, &v) in seg.iter().enumerate() {
            let resid = v - (a + b * i as f64);
            sumsq += resid * resid;
            cnt += 1;
        }
    }
    if cnt == 0 {
        0.0
    } else {
        (sumsq / cnt as f64).sqrt()
    }
}

/// Least-squares line `y = a + b·x` over `seg` at x = 0..len. Returns (a, b).
fn linfit(seg: &[f64]) -> (f64, f64) {
    let n = seg.len() as f64;
    let sx: f64 = (0..seg.len()).map(|i| i as f64).sum();
    let sy: f64 = seg.iter().sum();
    let sxx: f64 = (0..seg.len()).map(|i| (i * i) as f64).sum();
    let sxy: f64 = seg.iter().enumerate().map(|(i, &v)| i as f64 * v).sum();
    let denom = n * sxx - sx * sx;
    let b = if denom.abs() < 1e-12 { 0.0 } else { (n * sxy - sx * sy) / denom };
    let a = (sy - b * sx) / n;
    (a, b)
}

/// Lanczos approximation of ln Γ(x) (g=7), hand-rolled, zero deps.
fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_1,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection formula.
        std::f64::consts::PI.ln()
            - (std::f64::consts::PI * x).sin().ln()
            - ln_gamma(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = C[0];
        let t = x + 7.5;
        for (i, &c) in C.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

/// Anis-Lloyd/Peters expected R/S under independence, for the bias correction.
fn expected_rs(n: usize) -> f64 {
    let nf = n as f64;
    let sum: f64 = (1..n).map(|i| ((n - i) as f64 / i as f64).sqrt()).sum();
    let front = if n <= 340 {
        // Γ((n−1)/2) / (√π · Γ(n/2))
        (ln_gamma((nf - 1.0) / 2.0) - 0.5 * std::f64::consts::PI.ln() - ln_gamma(nf / 2.0)).exp()
    } else {
        1.0 / (nf * std::f64::consts::PI / 2.0).sqrt()
    };
    (nf - 0.5) / nf * front * sum
}

/// Average rescaled range (R/S) over the non-overlapping length-`n` sub-series.
fn rescaled_range_avg(x: &[f64], n: usize) -> Option<f64> {
    if n < 2 || x.len() < n {
        return None;
    }
    let num_sub = x.len() / n;
    let mut rs_vals = Vec::with_capacity(num_sub);
    for s in 0..num_sub {
        let sub = &x[s * n..(s + 1) * n];
        let mean = sub.iter().sum::<f64>() / n as f64;
        let mut z = 0.0;
        let mut zmin = 0.0_f64;
        let mut zmax = 0.0_f64;
        let mut var = 0.0;
        for &v in sub {
            let dev = v - mean;
            z += dev;
            zmin = zmin.min(z);
            zmax = zmax.max(z);
            var += dev * dev;
        }
        let r = zmax - zmin;
        let sd = (var / n as f64).sqrt();
        if sd > 0.0 {
            rs_vals.push(r / sd);
        }
    }
    if rs_vals.is_empty() {
        None
    } else {
        Some(rs_vals.iter().sum::<f64>() / rs_vals.len() as f64)
    }
}

/// Least-squares slope of `(x, y)` points.
fn ols_slope(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len() as f64;
    let sx: f64 = pts.iter().map(|p| p.0).sum();
    let sy: f64 = pts.iter().map(|p| p.1).sum();
    let sxx: f64 = pts.iter().map(|p| p.0 * p.0).sum();
    let sxy: f64 = pts.iter().map(|p| p.0 * p.1).sum();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        f64::NAN
    } else {
        (n * sxy - sx * sy) / denom
    }
}

/// Fit the Hurst exponent over a log-return series. Returns `None` if there is
/// not enough data for at least 3 window sizes.
pub fn hurst(returns: &[f64]) -> Option<HurstResult> {
    let n_obs = returns.len();
    if n_obs < 64 {
        return None;
    }
    let candidate = [8usize, 16, 32, 64, 128, 256, 512];
    let mut windows = Vec::new();
    let mut raw_pts = Vec::new(); // (ln n, ln rs)
    let mut corr_pts = Vec::new(); // (ln n, ln rs − ln E)
    for &w in &candidate {
        if w > n_obs / 2 {
            break; // need ≥2 sub-series
        }
        if let Some(rs) = rescaled_range_avg(returns, w) {
            if rs > 0.0 {
                let ln_n = (w as f64).ln();
                windows.push(w);
                raw_pts.push((ln_n, rs.ln()));
                corr_pts.push((ln_n, rs.ln() - expected_rs(w).ln()));
            }
        }
    }
    if raw_pts.len() < 3 {
        return None;
    }
    let h_uncorrected = ols_slope(&raw_pts);
    let h = ols_slope(&corr_pts) + 0.5;
    if !h.is_finite() {
        return None;
    }
    // Regime band centered on the EMPIRICAL null, not 0.5: the Anis-Lloyd
    // asymptotic correction over-corrects slightly at these window sizes, so a
    // true random walk lands at ≈0.48 (verified by Monte-Carlo on iid noise),
    // not exactly 0.50. The band is shifted down accordingly so a genuine
    // random walk (e.g. SPY ≈0.45) reads "random-walk", not "mean-reverting".
    let (regime, interpretation) = if h > 0.55 {
        (
            "trending",
            "persistent/trending — moves tend to continue; trend-following has an edge (accumulate dips and ride)",
        )
    } else if h >= 0.44 {
        (
            "random-walk",
            "≈ random walk — little autocorrelation edge; trend/mean-reversion signals here are largely noise (empirical null ≈0.48)",
        )
    } else {
        (
            "mean-reverting",
            "anti-persistent/mean-reverting — moves tend to reverse; fade extremes rather than chase",
        )
    };
    // Cross-validate with DFA (independent estimator, both center ≈0.5).
    let dfa = dfa_alpha(returns);
    let agreement = match dfa {
        Some(a) => {
            // Threshold ≈1.5× the joint sampling SE of the two estimators
            // (each ~0.06–0.07), so same-regime series aren't false-flagged as
            // divergent from estimator noise alone. Δ is also offset by the
            // ~0.03 systematic R/S(~0.48)-vs-DFA(~0.51) null gap.
            let d = (a - h).abs();
            if d < 0.13 {
                format!("R/S and DFA agree (|Δ|={d:.3}) — two independent estimators confirm the regime")
            } else {
                format!("R/S and DFA DIVERGE (|Δ|={d:.3}, beyond joint sampling noise) — the persistence estimate is unstable; treat the regime read with caution")
            }
        }
        None => "DFA not computed (insufficient data)".to_string(),
    };

    Some(HurstResult {
        n_obs,
        windows,
        h_uncorrected,
        h,
        dfa_alpha: dfa,
        agreement,
        regime: regime.to_string(),
        interpretation: interpretation.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescaled_range_matches_hand_calc() {
        // X = [1,3,2,4], n=4 → mean 2.5, Z = [-1.5,-1,-1.5,0], R=1.5,
        // S = sqrt(1.25) ≈ 1.118034 → R/S ≈ 1.341641.
        let rs = rescaled_range_avg(&[1.0, 3.0, 2.0, 4.0], 4).unwrap();
        assert!((rs - 1.341641).abs() < 1e-5, "rs={rs}");
    }

    #[test]
    fn expected_rs_anis_lloyd_known_value() {
        // E[(R/S)_4] = (3.5/4)·[Γ(1.5)/(√π·Γ(2))]·Σ√((4−i)/i)
        //            = 0.875·0.5·(√3+1+√(1/3)) ≈ 1.44786.
        let e = expected_rs(4);
        assert!((e - 1.44786).abs() < 1e-3, "E={e}");
    }

    #[test]
    fn ln_gamma_known_values() {
        // Γ(5)=24 → ln 24 ≈ 3.178054; Γ(0.5)=√π → ln√π ≈ 0.572365.
        assert!((ln_gamma(5.0) - 24f64.ln()).abs() < 1e-9);
        assert!((ln_gamma(0.5) - std::f64::consts::PI.sqrt().ln()).abs() < 1e-9);
    }

    #[test]
    fn trending_series_has_high_hurst() {
        // A strongly persistent series: cumulative same-sign drift with small
        // noise → H well above 0.5.
        let mut v = Vec::new();
        let mut acc = 0.0;
        for i in 0..600 {
            // deterministic pseudo-random sign-persistent increments
            let step = if (i / 20) % 2 == 0 { 1.0 } else { -1.0 };
            acc += step * 0.01;
            v.push(acc);
        }
        // returns of a trending walk
        let rets: Vec<f64> = v.windows(2).map(|w| w[1] - w[0]).collect();
        let h = hurst(&rets).unwrap();
        assert!(h.h > 0.5, "expected trending H>0.5, got {}", h.h);
    }

    #[test]
    fn white_noise_centers_near_the_empirical_null() {
        // Deterministic LCG → uniform-ish iid noise (a random walk's returns).
        // The Anis-Lloyd correction centers this near ≈0.48 (not exactly 0.5);
        // assert it lands in the random-walk band and is classified correctly.
        let mut s: u64 = 0x9E3779B97F4A7C15;
        let mut rets = Vec::with_capacity(2048);
        for _ in 0..2048 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u = (s >> 11) as f64 / (1u64 << 53) as f64; // [0,1)
            rets.push(u - 0.5);
        }
        let h = hurst(&rets).unwrap();
        assert!(
            (0.44..=0.53).contains(&h.h),
            "white noise should center near the ≈0.48 null, got {}",
            h.h
        );
        assert_eq!(h.regime, "random-walk");
    }

    #[test]
    fn dfa_centers_white_noise_near_half() {
        // Deterministic LCG iid noise → DFA α ≈ 0.5 (random walk null).
        let mut s: u64 = 0xDEADBEEFCAFEBABE;
        let rets: Vec<f64> = (0..2048)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 11) as f64 / (1u64 << 53) as f64) - 0.5
            })
            .collect();
        let a = dfa_alpha(&rets).unwrap();
        assert!((0.40..=0.60).contains(&a), "white-noise DFA α should be ~0.5, got {a}");
    }

    #[test]
    fn dfa_higher_for_persistent_series() {
        // A trending (persistent) increment series → DFA α > 0.5.
        let rets: Vec<f64> = (0..600).map(|i| if (i / 25) % 2 == 0 { 0.02 } else { -0.005 }).collect();
        let a = dfa_alpha(&rets).unwrap();
        assert!(a > 0.55, "persistent series DFA α should exceed 0.5, got {a}");
    }

    #[test]
    fn too_little_data_none() {
        assert!(hurst(&vec![0.01; 40]).is_none());
    }
}
