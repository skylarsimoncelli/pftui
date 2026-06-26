//! P5b — multiple-testing overfit statistics for the walk-forward optimizer.
//!
//! Two model-free honesty gauges, computed on the SAME single-run-then-slice
//! outputs P5a already produces (no extra simulations):
//!
//! - **PBO via CSCV** (Probability of Backtest Overfitting via Combinatorially-
//!   Symmetric Cross-Validation; Bailey, Borwein, López de Prado, Zhu 2016). It
//!   answers: across every balanced split of the timeline into in-sample (IS) and
//!   out-of-sample (OOS) halves, how often does the config that looked best IS
//!   land in the WORSE OOS half? ~0 = a persistent edge; ~0.5 = selecting noise.
//! - **DSR** (Deflated Sharpe Ratio; Bailey & López de Prado 2014) on the winner's
//!   OOS return stream, deflated for the number of trials and for non-normality.
//!
//! The special functions (Φ, Φ⁻¹, sample moments, per-period Sharpe) are reused
//! from [`crate::research::validation`] so there is ONE normal-CDF in the tree.
//!
//! ## Rank convention (read before editing the PBO logit)
//! The Codex brief phrased the relative rank as `w = rank/(N+1)` with "rank 1 =
//! best", but its own operative parenthetical is "λ<0 ⟺ the IS winner landed in
//! the worse OOS half". Those two are mutually inconsistent. We implement the
//! **literature-correct** CSCV (and the parenthetical): the OOS relative rank
//! `ω = R/(N+1)` uses `R` = 1 (worst) … N (best), so the OOS-BEST config has
//! ω ≈ 1 → λ = ln(ω/(1−ω)) > 0 → NOT overfit, and the OOS-WORST has ω ≈ 0 →
//! λ < 0 → overfit. This is the only convention under which a clean persistent
//! edge → PBO ≈ 0 and pure noise → PBO ≈ 0.5 (both unit-tested below). Ties take
//! the average rank.

use serde::{Deserialize, Serialize};

use crate::research::validation::{moments, normal_cdf, normal_inv_cdf};

const EULER_MASCHERONI: f64 = 0.577_215_664_901_532_9;

// ---------------------------------------------------------------------------
// PBO via CSCV.
// ---------------------------------------------------------------------------

/// Interpretation band for a PBO value (Codex thresholds).
pub fn pbo_band(pbo: f64) -> &'static str {
    if pbo < 0.10 {
        "low"
    } else if pbo < 0.25 {
        "caution"
    } else if pbo < 0.50 {
        "fragile"
    } else {
        "selecting-noise"
    }
}

/// PBO result + the enumeration provenance that makes it auditable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PboResult {
    /// Probability of Backtest Overfitting in [0, 1].
    pub pbo: f64,
    /// Interpretation band: low | caution | fragile | selecting-noise.
    pub band: String,
    /// Number of equal time-slices S (even).
    pub s_slices: usize,
    /// Number of CSCV splits enumerated = C(S, S/2).
    pub n_splits: usize,
    /// Number of configs ranked.
    pub n_configs: usize,
    /// Splits in which the IS-best config fell into the worse OOS half.
    pub n_overfit_splits: usize,
}

/// Probability of Backtest Overfitting via CSCV operating on a **per-config ×
/// per-slice objective matrix**: `slice_scores[config][slice]` is the config's
/// objective (higher = better) on time-slice `slice`. `S = slice_scores[0].len()`
/// must be even and ≥ 2; there must be ≥ 2 configs.
///
/// For every balanced split that assigns S/2 slices to IS and the complement to
/// OOS: pick the IS-best config (max mean IS objective), compute its OOS relative
/// rank ω (see the module rank-convention note), and count the split as overfit
/// iff its logit λ = ln(ω/(1−ω)) < 0. `PBO = #overfit / #splits`.
///
/// NaN slice scores are treated as the worst possible value for that config on
/// that slice (a config that could not be scored on a slice cannot win it).
pub fn pbo_cscv(slice_scores: &[Vec<f64>]) -> Option<PboResult> {
    let n_configs = slice_scores.len();
    if n_configs < 2 {
        return None;
    }
    let s = slice_scores[0].len();
    if s < 2 || !s.is_multiple_of(2) {
        return None;
    }
    // Every config must expose the same number of slices.
    if slice_scores.iter().any(|r| r.len() != s) {
        return None;
    }

    let half = s / 2;
    let combos = combinations(s, half);
    if combos.is_empty() {
        return None;
    }

    // Replace NaN with a sentinel below every real score so it can never win.
    let finite_min = slice_scores
        .iter()
        .flat_map(|r| r.iter())
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::INFINITY, f64::min);
    let sentinel = if finite_min.is_finite() {
        finite_min - 1.0
    } else {
        0.0
    };
    let at = |c: usize, slice: usize| -> f64 {
        let v = slice_scores[c][slice];
        if v.is_finite() {
            v
        } else {
            sentinel
        }
    };

    let mean_over = |c: usize, slices: &[usize]| -> f64 {
        let sum: f64 = slices.iter().map(|&sl| at(c, sl)).sum();
        sum / slices.len() as f64
    };

    let mut n_overfit = 0usize;
    let mut total = 0usize;
    for is_slices in &combos {
        let is_set: Vec<usize> = is_slices.clone();
        let oos_set: Vec<usize> = (0..s).filter(|sl| !is_set.contains(sl)).collect();

        // IS-best config (deterministic first-index tie-break).
        let mut best = 0usize;
        let mut best_val = f64::NEG_INFINITY;
        for c in 0..n_configs {
            let v = mean_over(c, &is_set);
            if v > best_val {
                best_val = v;
                best = c;
            }
        }

        // OOS performance of every config, and the average rank of `best`.
        let oos: Vec<f64> = (0..n_configs).map(|c| mean_over(c, &oos_set)).collect();
        let best_oos = oos[best];
        // Average rank with R = 1 (worst) … N (best): rank = 1 + #strictly-less +
        // 0.5 * #ties (excluding self).
        let mut less = 0usize;
        let mut ties = 0usize;
        for (c, &v) in oos.iter().enumerate() {
            if c == best {
                continue;
            }
            if v < best_oos {
                less += 1;
            } else if v == best_oos {
                ties += 1;
            }
        }
        let rank = 1.0 + less as f64 + 0.5 * ties as f64;
        let omega = rank / (n_configs as f64 + 1.0);
        // logit; ω∈(0,1) by construction so this is finite.
        let lambda = (omega / (1.0 - omega)).ln();
        if lambda < 0.0 {
            n_overfit += 1;
        }
        total += 1;
    }

    let pbo = n_overfit as f64 / total as f64;
    Some(PboResult {
        pbo,
        band: pbo_band(pbo).to_string(),
        s_slices: s,
        n_splits: total,
        n_configs,
        n_overfit_splits: n_overfit,
    })
}

/// All k-subsets of {0..n} as ascending index vectors. Guarded against blow-up
/// (caller keeps S small — C(12,6)=924; C(16,8)=12870 is the practical ceiling).
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    if k > n {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.clone());
        if out.len() > 20000 {
            break;
        }
        let mut i = k as isize - 1;
        while i >= 0 && idx[i as usize] == n - k + i as usize {
            i -= 1;
        }
        if i < 0 {
            break;
        }
        idx[i as usize] += 1;
        for j in (i as usize + 1)..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Deflated Sharpe Ratio.
// ---------------------------------------------------------------------------

/// Expected maximum of N i.i.d. standard normals (Bailey & López de Prado 2014):
/// `E[max_N Z] ≈ (1−γE)·Φ⁻¹(1−1/N) + γE·Φ⁻¹(1−1/(N·e))`. 0 for N < 2.
pub fn expected_max_z(n_trials: usize) -> f64 {
    if n_trials < 2 {
        return 0.0;
    }
    let n = n_trials as f64;
    let g = EULER_MASCHERONI;
    let a = normal_inv_cdf(1.0 - 1.0 / n);
    let b = normal_inv_cdf(1.0 - 1.0 / (n * std::f64::consts::E));
    (1.0 - g) * a + g * b
}

/// Deflated Sharpe Ratio result + the inputs that produced it (auditable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DsrResult {
    /// Observed per-period Sharpe of the winner's OOS return stream.
    pub sharpe_oos: f64,
    /// Number of trials the deflation accounts for (configs searched).
    pub n_trials: usize,
    /// Number of OOS return observations T.
    pub t_obs: usize,
    /// Fisher skewness of the OOS returns.
    pub skew: f64,
    /// Pearson kurtosis (3 = normal) of the OOS returns.
    pub kurtosis: f64,
    /// Expected-max-by-luck benchmark Sharpe SR* (per period).
    pub sr_star: f64,
    /// Estimated variance of the Sharpe estimator.
    pub var_sr: f64,
    /// DSR = Φ((SR − SR*)/sqrt(Var(SR))) in [0, 1].
    pub dsr: f64,
    /// True iff DSR ≥ 0.95.
    pub passes: bool,
}

/// Deflated Sharpe Ratio on the winner's OOS return stream.
///
/// `winner_oos_returns` are the winner's per-period (daily) OOS returns; their
/// length is T. `trial_sharpes` are the per-period OOS Sharpe ratios of EVERY
/// config tried (the search width), used to estimate `mean(SR_trials)` and
/// `std(SR_trials)` for the expected-max benchmark. `n_trials` is the count of
/// configs searched (= `trial_sharpes.len()` when all configs scored).
///
/// Formula (Codex brief):
/// - `Var(SR) ≈ (1 − γ3·SR + ((γ4−1)/4)·SR²)/(T−1)`
/// - `SR* ≈ mean(SR_trials) + std(SR_trials)·E[max_N Z]`
/// - `DSR = Φ((SR − SR*)/sqrt(Var(SR)))`
///
/// Returns `None` when the stream is too short, has zero variance, or the
/// Sharpe-variance bracket goes non-positive (undefined under extreme non-
/// normality — we refuse to emit a spurious ~1.0 rather than clamp).
pub fn deflated_sharpe(
    winner_oos_returns: &[f64],
    trial_sharpes: &[f64],
    n_trials: usize,
) -> Option<DsrResult> {
    let m = moments(winner_oos_returns)?;
    if m.std == 0.0 {
        return None;
    }
    let sr = m.mean / m.std;
    let t = m.n;
    if t < 2 {
        return None;
    }
    let bracket = 1.0 - m.skew * sr + (m.kurtosis - 1.0) / 4.0 * sr * sr;
    if bracket <= 0.0 {
        return None;
    }
    let var_sr = bracket / (t as f64 - 1.0);

    // Trial-Sharpe location/scale for the expected-max benchmark.
    let finite_trials: Vec<f64> = trial_sharpes.iter().copied().filter(|v| v.is_finite()).collect();
    let (mean_tr, std_tr) = match moments(&finite_trials) {
        Some(tm) => (tm.mean, tm.std),
        None => (finite_trials.first().copied().unwrap_or(0.0), 0.0),
    };
    let emax = expected_max_z(n_trials.max(1));
    let sr_star = mean_tr + std_tr * emax;

    let dsr = normal_cdf((sr - sr_star) / var_sr.sqrt());
    Some(DsrResult {
        sharpe_oos: sr,
        n_trials: n_trials.max(1),
        t_obs: t,
        skew: m.skew,
        kurtosis: m.kurtosis,
        sr_star,
        var_sr,
        dsr,
        passes: dsr >= 0.95,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- PBO --------------------------------------------------------------

    /// CLEAN persistent edge: one config dominates IS AND OOS on every slice →
    /// it is the IS-best on every split and also OOS-best → never in the worse
    /// half → PBO == 0. Also pins the tiny-S enumeration count C(4,2)=6.
    #[test]
    fn pbo_clean_edge_is_zero() {
        // 3 configs, S=4. Config 0 dominates every slice.
        let scores = vec![
            vec![10.0, 11.0, 12.0, 13.0], // dominant
            vec![1.0, 2.0, 1.5, 0.5],
            vec![0.0, 0.1, -1.0, 0.2],
        ];
        let r = pbo_cscv(&scores).unwrap();
        assert_eq!(r.s_slices, 4);
        assert_eq!(r.n_splits, 6, "C(4,2) must enumerate exactly 6 splits");
        assert_eq!(r.pbo, 0.0, "a dominant config can never land in the worse half");
        assert_eq!(r.band, "low");
    }

    /// TINY-S EXACT HAND-CHECK (S=4 → 6 splits, 2 configs). Slice scores are
    /// chosen tie-free; the 6 splits are enumerated by hand in the comment and
    /// 4 of them are overfit → PBO = 4/6.
    ///
    /// A = [20,15, 2, 1]   B = [1, 2,18, 9]   (slices 0..3, equal counts so
    /// pair *sum* order == mean order):
    ///   IS{0,1}: A35>B3  →A; OOS{2,3}: A3 < B27          → overfit
    ///   IS{0,2}: A22>B19 →A; OOS{1,3}: A16 > B11         → ok
    ///   IS{0,3}: A21>B10 →A; OOS{1,2}: A17 < B20         → overfit
    ///   IS{1,2}: A17<B20 →B; OOS{0,3}: B10 < A21         → overfit
    ///   IS{1,3}: A16>B11 →A; OOS{0,2}: A22 > B19         → ok
    ///   IS{2,3}: A3 <B27 →B; OOS{0,1}: B3  < A35         → overfit
    /// 4 overfit / 6 splits = 0.6667.
    #[test]
    fn pbo_tiny_s_hand_enumeration() {
        let scores = vec![vec![20.0, 15.0, 2.0, 1.0], vec![1.0, 2.0, 18.0, 9.0]];
        let r = pbo_cscv(&scores).unwrap();
        assert_eq!(r.n_splits, 6);
        assert_eq!(r.n_overfit_splits, 4);
        assert!((r.pbo - 4.0 / 6.0).abs() < 1e-12, "PBO must be exactly 4/6, got {}", r.pbo);
    }

    /// PURE NOISE: many configs whose per-slice ranks are essentially random →
    /// the IS-best is no better than chance OOS → PBO ≈ 0.5. Deterministically
    /// seeded so it never flakes.
    #[test]
    fn pbo_pure_noise_is_about_half() {
        use rand::rngs::StdRng;
        use rand::{Rng, SeedableRng};
        let n_configs = 12;
        let s = 10;
        let mut rng = StdRng::seed_from_u64(20260626);
        let scores: Vec<Vec<f64>> = (0..n_configs)
            .map(|_| (0..s).map(|_| rng.gen::<f64>()).collect())
            .collect();
        let r = pbo_cscv(&scores).unwrap();
        assert_eq!(r.n_splits, 252, "C(10,5)=252");
        assert!(
            (r.pbo - 0.5).abs() < 0.2,
            "pure noise PBO must be ~0.5, got {}",
            r.pbo
        );
    }

    #[test]
    fn pbo_rejects_bad_shape() {
        assert!(pbo_cscv(&[vec![1.0, 2.0]]).is_none(), "needs >= 2 configs");
        // odd S
        assert!(pbo_cscv(&[vec![1.0, 2.0, 3.0], vec![3.0, 2.0, 1.0]]).is_none());
        // ragged
        assert!(pbo_cscv(&[vec![1.0, 2.0], vec![3.0, 2.0, 1.0, 0.0]]).is_none());
    }

    // -- DSR --------------------------------------------------------------

    /// E[max_N Z] grows with N and matches known ballpark values.
    #[test]
    fn expected_max_z_grows_and_is_sane() {
        assert_eq!(expected_max_z(1), 0.0);
        let e2 = expected_max_z(2);
        let e10 = expected_max_z(10);
        let e100 = expected_max_z(100);
        assert!(e2 < e10 && e10 < e100, "monotone in N");
        // N=2 true E[max] = 1/sqrt(pi) ≈ 0.5642; the approximation is close.
        assert!((e2 - 0.564).abs() < 0.1, "E[max_2] ~ 0.56, got {e2}");
        // N=10 true E[max] ≈ 1.539; allow approximation slack.
        assert!((e10 - 1.54).abs() < 0.1, "E[max_10] ~ 1.54, got {e10}");
    }

    /// HAND-CHECK: a symmetric stream deflated against a degenerate trial set
    /// (mean 0, std 0 → SR*=0) → DSR = Φ(SR/sqrt(Var(SR))). With Var(SR) =
    /// (1 − γ3·SR + ((γ4−1)/4)·SR²)/(T−1), we recompute the expected Φ-argument
    /// independently and assert the DSR matches.
    #[test]
    fn dsr_hand_checked_single_trial() {
        // Returns: 100 copies of {+0.2,-0.2} (mean 0, sym) plus a small +shift so
        // SR is a clean positive. Build mean≈0.02, std≈0.2 → SR≈0.1.
        let mut rets = Vec::new();
        for _ in 0..50 {
            rets.push(0.22);
            rets.push(-0.18);
        }
        // T = 100 here; mean = 0.02, var = E[x^2]-mean^2.
        let m = moments(&rets).unwrap();
        let sr = m.mean / m.std;
        let var_sr = (1.0 - m.skew * sr + (m.kurtosis - 1.0) / 4.0 * sr * sr) / (m.n as f64 - 1.0);
        let expected = normal_cdf((sr - 0.0) / var_sr.sqrt());
        // Degenerate trial set (all zeros) → mean(SR_trials)=0, std=0 → SR*=0.
        let d = deflated_sharpe(&rets, &[0.0, 0.0], 1).unwrap();
        assert!((d.sr_star - 0.0).abs() < 1e-12, "zero-centered trials → SR*=0");
        assert!((d.dsr - expected).abs() < 1e-9, "DSR must equal Φ(SR/σ_SR)");
        assert_eq!(d.t_obs, 100);
    }

    /// N-trials deflation LOWERS DSR as N grows (wider search → higher luck bar).
    #[test]
    fn dsr_decreases_with_more_trials() {
        let rets: Vec<f64> = (0..250)
            .map(|i| 0.01 + 0.02 * (((i % 7) as f64) - 3.0))
            .collect();
        // A non-degenerate trial-Sharpe spread so std(SR_trials) > 0.
        let trials: Vec<f64> = (0..40).map(|i| 0.02 + 0.05 * (((i % 9) as f64) - 4.0) / 4.0).collect();
        let few = deflated_sharpe(&rets, &trials, 4).unwrap();
        let many = deflated_sharpe(&rets, &trials, 400).unwrap();
        assert!(many.dsr <= few.dsr, "more trials must not raise DSR");
        assert!(many.sr_star > few.sr_star, "expected-max bar rises with N");
    }
}
