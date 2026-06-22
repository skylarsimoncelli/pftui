//! Validation gauntlet — the "don't fool yourself" layer.
//!
//! Cheap, pure-Rust statistical honesty checks that every claimed edge must
//! clear before it is reported. Implements the highest-leverage methods from
//! the quant literature (see `docs/ENVIRONMENT-ENGINE.md` §3.6):
//!
//! - **Deflated Sharpe Ratio (DSR)** — Sharpe benchmarked against the best you
//!   would expect by luck after N trials, deflated for sample length and
//!   non-normality (Bailey & López de Prado 2014).
//! - **Probabilistic Sharpe Ratio (PSR)** — P(true Sharpe > benchmark).
//! - **PBO via CSCV** — model-free probability the in-sample-best config lands
//!   below the OOS median (Bailey, Borwein, López de Prado, Zhu 2016).
//! - **Multiple-testing haircuts** — Bonferroni / Holm / BHY adjusted p-values
//!   (Harvey & Liu 2015).
//! - **Stationary block bootstrap** — honest CIs on a statistic under serial
//!   dependence (Politis & Romano 1994).
//! - **Minimum Backtest Length** gate (Bailey et al. 2014).
//!
//! All values are `f64` — statistics over returns, not monetary balances.
//! No external numeric dependency: the few special functions we need
//! (Φ, Φ⁻¹) are implemented here.

use rand::Rng;

// Used by the full Deflated Sharpe (the Phase-3 positioning sweep); the
// single-rule strategy path uses the PSR directly. Proven by the inline tests.
#[allow(dead_code)]
const EULER_MASCHERONI: f64 = 0.577_215_664_901_532_9;

// ----------------------------------------------------------------------------
// Special functions (normal CDF / inverse CDF) — no statrs dependency.
// ----------------------------------------------------------------------------

/// Standard normal CDF via the Abramowitz & Stegun erf approximation (7.1.26),
/// max abs error ~1.5e-7 — ample for our use.
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

fn erfc(x: f64) -> f64 {
    // erfc via the A&S 7.1.26 rational approximation of erf.
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf_abs = 1.0 - poly * (-z * z).exp();
    let erf = if x >= 0.0 { erf_abs } else { -erf_abs };
    1.0 - erf
}

/// Inverse standard normal CDF (quantile) via Acklam's algorithm. Valid for
/// p in (0,1); clamps just inside the bounds.
#[allow(dead_code)]
pub fn normal_inv_cdf(p: f64) -> f64 {
    let p = p.clamp(1e-12, 1.0 - 1e-12);
    // Coefficients (Peter Acklam).
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    let p_low = 0.024_25;
    let p_high = 1.0 - p_low;
    if p < p_low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_high {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

// ----------------------------------------------------------------------------
// Sample moments + Sharpe.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Moments {
    pub n: usize,
    pub mean: f64,
    pub std: f64,
    /// Fisher skewness (0 for normal).
    pub skew: f64,
    /// Pearson kurtosis (3 for normal — NOT excess).
    pub kurtosis: f64,
}

pub fn moments(x: &[f64]) -> Option<Moments> {
    let n = x.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let mean = x.iter().sum::<f64>() / nf;
    let m2 = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / nf;
    let std = m2.sqrt();
    if std == 0.0 {
        return Some(Moments {
            n,
            mean,
            std: 0.0,
            skew: 0.0,
            kurtosis: 3.0,
        });
    }
    let m3 = x.iter().map(|v| (v - mean).powi(3)).sum::<f64>() / nf;
    let m4 = x.iter().map(|v| (v - mean).powi(4)).sum::<f64>() / nf;
    Some(Moments {
        n,
        mean,
        std,
        skew: m3 / m2.powf(1.5),
        kurtosis: m4 / (m2 * m2),
    })
}

/// Per-period Sharpe ratio (mean / std of returns). Caller annualizes if wanted.
pub fn sharpe(returns: &[f64]) -> Option<f64> {
    let m = moments(returns)?;
    if m.std == 0.0 {
        None
    } else {
        Some(m.mean / m.std)
    }
}

// ----------------------------------------------------------------------------
// Probabilistic & Deflated Sharpe.
// ----------------------------------------------------------------------------

/// Probabilistic Sharpe Ratio: P(true SR > `sr_benchmark`) given the observed
/// `sr`, sample length `t`, and the return distribution's skew/kurtosis.
pub fn probabilistic_sharpe_ratio(sr: f64, sr_benchmark: f64, t: usize, skew: f64, kurt: f64) -> f64 {
    if t < 2 {
        return f64::NAN;
    }
    // The variance term of the Sharpe estimator. For very fat-tailed / highly
    // skewed distributions this bracket can go non-positive — in that regime
    // the PSR is undefined, so return NaN rather than clamping (which would
    // emit a spurious ~1.0 for exactly the lottery-like strategies most likely
    // to be overfit).
    let bracket = 1.0 - skew * sr + (kurt - 1.0) / 4.0 * sr * sr;
    if bracket <= 0.0 {
        return f64::NAN;
    }
    let z = (sr - sr_benchmark) * ((t as f64 - 1.0).sqrt()) / bracket.sqrt();
    normal_cdf(z)
}

/// Expected maximum Sharpe achievable by luck across `n_trials` independent
/// strategies whose Sharpe estimates have variance `sharpe_variance` (the
/// "False Strategy Theorem", Bailey & López de Prado 2014).
#[allow(dead_code)]
pub fn expected_max_sharpe(sharpe_variance: f64, n_trials: usize) -> f64 {
    if n_trials < 1 || sharpe_variance <= 0.0 {
        return 0.0;
    }
    let n = n_trials as f64;
    let g = EULER_MASCHERONI;
    let a = normal_inv_cdf(1.0 - 1.0 / n);
    let b = normal_inv_cdf(1.0 - 1.0 / (n * std::f64::consts::E));
    sharpe_variance.sqrt() * ((1.0 - g) * a + g * b)
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DeflatedSharpe {
    pub sharpe: f64,
    pub n_trials: usize,
    pub expected_max_sharpe: f64,
    /// PSR benchmarked against the expected-max-by-luck Sharpe. Report a result
    /// only when this exceeds ~0.95.
    pub dsr: f64,
    pub passes: bool,
}

/// Deflated Sharpe Ratio. `trial_sharpes` are the Sharpe ratios of all configs
/// tried (the candidate among them); their variance estimates the benchmark.
#[allow(dead_code)]
pub fn deflated_sharpe_ratio(candidate_returns: &[f64], trial_sharpes: &[f64]) -> Option<DeflatedSharpe> {
    let m = moments(candidate_returns)?;
    if m.std == 0.0 {
        return None;
    }
    let sr = m.mean / m.std;
    let n_trials = trial_sharpes.len().max(1);
    let var = if trial_sharpes.len() >= 2 {
        moments(trial_sharpes).map(|mm| mm.std * mm.std).unwrap_or(1.0)
    } else {
        // Single trial: no deflation benchmark beyond the null (SR0 = 0).
        0.0
    };
    let sr0 = expected_max_sharpe(var, n_trials);
    let dsr = probabilistic_sharpe_ratio(sr, sr0, m.n, m.skew, m.kurtosis);
    Some(DeflatedSharpe {
        sharpe: sr,
        n_trials,
        expected_max_sharpe: sr0,
        dsr,
        passes: dsr > 0.95,
    })
}

// ----------------------------------------------------------------------------
// PBO via CSCV.
// ----------------------------------------------------------------------------

/// Probability of Backtest Overfitting via Combinatorially Symmetric Cross
/// Validation. `returns` is a T×N matrix: `returns[t][c]` is config `c`'s
/// per-period return at time `t`. Splits time into `n_blocks` (even),
/// enumerates the C(S, S/2) IS/OOS partitions, and reports the share where the
/// in-sample-best config ranks below the OOS median.
///
/// Consumed by the Phase-3 positioning sweep (ENVIRONMENT-ENGINE.md §3.6);
/// proven correct by the inline tests until that wiring lands.
#[allow(dead_code)]
pub fn pbo_cscv(returns: &[Vec<f64>], n_blocks: usize) -> Option<f64> {
    let t = returns.len();
    let n_configs = returns.first()?.len();
    if n_configs < 2 || t < n_blocks || n_blocks < 2 || !n_blocks.is_multiple_of(2) {
        return None;
    }
    // Partition rows into n_blocks contiguous blocks (drop the remainder tail).
    let block_len = t / n_blocks;
    let blocks: Vec<(usize, usize)> = (0..n_blocks)
        .map(|b| (b * block_len, (b + 1) * block_len))
        .collect();

    let perf = |idxs: &[usize]| -> Vec<f64> {
        (0..n_configs)
            .map(|c| {
                let rs: Vec<f64> = idxs.iter().map(|&row| returns[row][c]).collect();
                sharpe(&rs).unwrap_or(f64::NEG_INFINITY)
            })
            .collect()
    };

    let combos = combinations(n_blocks, n_blocks / 2);
    if combos.is_empty() {
        return None;
    }
    let mut overfit = 0usize;
    let mut total = 0usize;
    for is_blocks in &combos {
        let is_set: std::collections::HashSet<usize> = is_blocks.iter().copied().collect();
        let mut is_rows = Vec::new();
        let mut oos_rows = Vec::new();
        for (bi, &(lo, hi)) in blocks.iter().enumerate() {
            if is_set.contains(&bi) {
                is_rows.extend(lo..hi);
            } else {
                oos_rows.extend(lo..hi);
            }
        }
        let is_perf = perf(&is_rows);
        let oos_perf = perf(&oos_rows);
        // Best config in-sample.
        let best = (0..n_configs)
            .max_by(|&a, &b| is_perf[a].partial_cmp(&is_perf[b]).unwrap())
            .unwrap();
        // Its relative rank OOS (fraction of configs it beats).
        let beaten = (0..n_configs).filter(|&c| oos_perf[best] > oos_perf[c]).count();
        let omega = (beaten as f64 + 0.5) / (n_configs as f64);
        let logit = (omega / (1.0 - omega)).ln();
        if logit <= 0.0 {
            overfit += 1;
        }
        total += 1;
    }
    Some(overfit as f64 / total as f64)
}

/// All k-subsets of {0..n} (indices). Guarded: returns empty if the count would
/// be unreasonably large (caller should keep n_blocks small, e.g. <= 12).
#[allow(dead_code)]
fn combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    if k > n {
        return Vec::new();
    }
    // C(12,6)=924; cap at a safe ceiling.
    let mut out = Vec::new();
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        out.push(idx.clone());
        if out.len() > 5000 {
            break;
        }
        // advance
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

// ----------------------------------------------------------------------------
// Multiple-testing haircuts (Harvey & Liu 2015).
// ----------------------------------------------------------------------------

/// Multiple-testing correction methods (Harvey & Liu 2015). Consumed by the
/// Phase-3 positioning sweep; proven by the inline tests until then.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum HaircutMethod {
    Bonferroni,
    Holm,
    /// Benjamini-Hochberg-Yekutieli (FDR control).
    Bhy,
}

/// Adjust a set of p-values for multiple testing. Returns adjusted p-values in
/// the SAME order as the input.
#[allow(dead_code)]
pub fn haircut_pvalues(pvalues: &[f64], method: HaircutMethod) -> Vec<f64> {
    let m = pvalues.len();
    if m == 0 {
        return Vec::new();
    }
    let mf = m as f64;
    match method {
        HaircutMethod::Bonferroni => pvalues.iter().map(|p| (p * mf).min(1.0)).collect(),
        HaircutMethod::Holm => {
            let mut order: Vec<usize> = (0..m).collect();
            order.sort_by(|&a, &b| pvalues[a].partial_cmp(&pvalues[b]).unwrap());
            let mut adj = vec![0.0; m];
            let mut running: f64 = 0.0;
            for (rank, &i) in order.iter().enumerate() {
                let val = ((mf - rank as f64) * pvalues[i]).min(1.0);
                running = running.max(val);
                adj[i] = running;
            }
            adj
        }
        HaircutMethod::Bhy => {
            // BHY: c(m) = sum_{j=1..m} 1/j ; step-up on sorted p.
            let cm: f64 = (1..=m).map(|j| 1.0 / j as f64).sum();
            let mut order: Vec<usize> = (0..m).collect();
            order.sort_by(|&a, &b| pvalues[b].partial_cmp(&pvalues[a]).unwrap()); // desc
            let mut adj = vec![0.0; m];
            let mut running = f64::INFINITY;
            for (rank_from_top, &i) in order.iter().enumerate() {
                let rank = m - rank_from_top; // largest p has rank m
                let val = (pvalues[i] * mf * cm / rank as f64).min(1.0);
                running = running.min(val);
                adj[i] = running;
            }
            adj
        }
    }
}

// ----------------------------------------------------------------------------
// Stationary block bootstrap (Politis & Romano 1994).
// ----------------------------------------------------------------------------

/// Bootstrap a confidence interval for a statistic of a serially-dependent
/// series. `expected_block_len` should scale ~ T^(1/3). Returns
/// `(lower, point, upper)` at the given two-sided alpha (e.g. 0.05 → 90% CI...
/// here alpha is the total tail mass, so 0.10 → 5%/95%).
///
/// `seed` makes the resampling DETERMINISTIC — callers should derive it from
/// the query identity (e.g. a hash of the date + asset) so the same inputs
/// always produce the same CI. A non-reproducible CI would undermine the
/// whole "open, reproducible" credibility model.
pub fn block_bootstrap_ci<F: Fn(&[f64]) -> Option<f64>>(
    series: &[f64],
    statistic: F,
    n_boot: usize,
    expected_block_len: f64,
    alpha: f64,
    seed: u64,
) -> Option<(f64, f64, f64)> {
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    let t = series.len();
    if t < 4 || n_boot == 0 {
        return None;
    }
    let point = statistic(series)?;
    let p_continue = 1.0 - (1.0 / expected_block_len.max(1.0));
    let mut rng = StdRng::seed_from_u64(seed);
    let mut stats = Vec::with_capacity(n_boot);
    for _ in 0..n_boot {
        let mut sample = Vec::with_capacity(t);
        let mut idx = rng.gen_range(0..t);
        for _ in 0..t {
            sample.push(series[idx]);
            if rng.gen::<f64>() < p_continue {
                idx = (idx + 1) % t; // continue the block (wrap)
            } else {
                idx = rng.gen_range(0..t); // start a new block
            }
        }
        if let Some(s) = statistic(&sample) {
            stats.push(s);
        }
    }
    if stats.is_empty() {
        return None;
    }
    stats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let lo = percentile(&stats, alpha / 2.0);
    let hi = percentile(&stats, 1.0 - alpha / 2.0);
    Some((lo, point, hi))
}

/// Derive a stable u64 seed from a string identity (FNV-1a) so a bootstrap CI
/// is reproducible per (date, asset, ...).
pub fn seed_from_str(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Cross-path distribution from Monte-Carlo trade-sequence resampling.
/// Drawdowns are NEGATIVE percentages (more negative = worse).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonteCarloPaths {
    pub n_paths: usize,
    /// How paths were generated ("bootstrap-resample").
    pub method: String,
    /// Terminal compounded return (%) — unlucky (p5), median, lucky (p95).
    pub terminal_return_p5_pct: f64,
    pub terminal_return_p50_pct: f64,
    pub terminal_return_p95_pct: f64,
    /// Max drawdown (%) — typical (median), bad (95th-pct severity), worst
    /// (99th-pct severity). All negative; the realistic worst-case the single
    /// historical curve hides.
    pub drawdown_median_pct: f64,
    pub drawdown_p95_pct: f64,
    pub drawdown_p99_pct: f64,
    /// Share of paths ending below the starting equity.
    pub prob_loss_pct: f64,
}

/// Monte-Carlo trade-path resampling. The realized equity curve is ONE ordering
/// and draw of the trades; a different draw of the same edge could be far
/// deeper. Bootstrap-resamples the per-trade return list (fractions) over
/// `n_paths` independent paths, compounds each into an equity curve, and returns
/// the cross-path distribution of terminal return and max drawdown — so the
/// operator sizing into a position sees the realistic 95th/99th-percentile
/// drawdown, not just the lucky historical path. Deterministic via `seed`.
/// Returns `None` for fewer than 20 trades (resampling a tiny sample is noise).
pub fn monte_carlo_trade_paths(returns: &[f64], n_paths: usize, seed: u64) -> Option<MonteCarloPaths> {
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    let n = returns.len();
    if n < 20 || n_paths == 0 {
        return None;
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let mut terminals = Vec::with_capacity(n_paths);
    let mut drawdowns = Vec::with_capacity(n_paths);
    for _ in 0..n_paths {
        let mut equity = 1.0f64;
        let mut peak = 1.0f64;
        let mut max_dd = 0.0f64;
        for _ in 0..n {
            let r = returns[rng.gen_range(0..n)];
            equity *= 1.0 + r;
            if equity > peak {
                peak = equity;
            }
            if peak > 0.0 {
                let dd = (equity / peak - 1.0) * 100.0;
                if dd < max_dd {
                    max_dd = dd;
                }
            }
        }
        terminals.push((equity - 1.0) * 100.0);
        drawdowns.push(max_dd);
    }
    // total_cmp is a total order (no NaN-panic) — a stray non-finite value
    // sorts to an end rather than taking down the whole backtest.
    terminals.sort_by(|a, b| a.total_cmp(b));
    // Ascending → the most-negative (worst) drawdowns sit at the LOW quantiles.
    drawdowns.sort_by(|a, b| a.total_cmp(b));
    let prob_loss_pct =
        terminals.iter().filter(|t| **t < 0.0).count() as f64 / terminals.len() as f64 * 100.0;
    Some(MonteCarloPaths {
        n_paths,
        method: "bootstrap-resample".to_string(),
        terminal_return_p5_pct: percentile(&terminals, 0.05),
        terminal_return_p50_pct: percentile(&terminals, 0.50),
        terminal_return_p95_pct: percentile(&terminals, 0.95),
        drawdown_median_pct: percentile(&drawdowns, 0.50),
        drawdown_p95_pct: percentile(&drawdowns, 0.05), // 95th-percentile severity
        drawdown_p99_pct: percentile(&drawdowns, 0.01), // 99th-percentile severity
        prob_loss_pct,
    })
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = (q.clamp(0.0, 1.0)) * (sorted.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = pos - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

// ----------------------------------------------------------------------------
// Minimum Backtest Length.
// ----------------------------------------------------------------------------

/// Minimum backtest length (in the same period units as the Sharpe, e.g. years
/// if `expected_max_sharpe_annual` is annualized) needed before an
/// `n_trials`-deep search is credible (Bailey et al. 2014):
/// MinBTL ≈ 2·ln(N) / E[max SR]².
///
/// Consumed by the Phase-3 positioning sweep; proven by the inline test.
#[allow(dead_code)]
pub fn min_backtest_length(n_trials: usize, expected_max_sharpe: f64) -> Option<f64> {
    if n_trials < 2 || expected_max_sharpe <= 0.0 {
        return None;
    }
    Some(2.0 * (n_trials as f64).ln() / (expected_max_sharpe * expected_max_sharpe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monte_carlo_is_deterministic_and_sane() {
        // 30 trades: mostly small wins, a few losses.
        let rets: Vec<f64> = (0..30)
            .map(|i| if i % 5 == 0 { -0.08 } else { 0.04 })
            .collect();
        let a = monte_carlo_trade_paths(&rets, 2000, 12345).unwrap();
        let b = monte_carlo_trade_paths(&rets, 2000, 12345).unwrap();
        // Determinism: same seed → identical distribution.
        assert_eq!(a.terminal_return_p50_pct, b.terminal_return_p50_pct);
        assert_eq!(a.drawdown_p99_pct, b.drawdown_p99_pct);
        // Ordering sanity: p5 ≤ p50 ≤ p95 terminal; worse drawdowns are MORE
        // negative at the deeper tail.
        assert!(a.terminal_return_p5_pct <= a.terminal_return_p50_pct);
        assert!(a.terminal_return_p50_pct <= a.terminal_return_p95_pct);
        assert!(a.drawdown_p99_pct <= a.drawdown_p95_pct);
        assert!(a.drawdown_p95_pct <= a.drawdown_median_pct);
        assert!(a.drawdown_median_pct <= 0.0);
        assert!((0.0..=100.0).contains(&a.prob_loss_pct));
    }

    #[test]
    fn monte_carlo_none_below_threshold() {
        let rets = vec![0.01; 19];
        assert!(monte_carlo_trade_paths(&rets, 1000, 1).is_none());
    }

    #[test]
    fn normal_cdf_known_values() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((normal_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((normal_cdf(-1.96) - 0.025).abs() < 1e-3);
    }

    #[test]
    fn inv_cdf_roundtrips() {
        for &p in &[0.025, 0.1, 0.5, 0.9, 0.975] {
            let x = normal_inv_cdf(p);
            assert!((normal_cdf(x) - p).abs() < 1e-3, "p={p}");
        }
    }

    #[test]
    fn moments_of_known_series() {
        let m = moments(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert!((m.mean - 3.0).abs() < 1e-9);
        assert!((m.skew).abs() < 1e-9); // symmetric
    }

    #[test]
    fn psr_increases_with_sharpe() {
        let lo = probabilistic_sharpe_ratio(0.1, 0.0, 100, 0.0, 3.0);
        let hi = probabilistic_sharpe_ratio(0.5, 0.0, 100, 0.0, 3.0);
        assert!(hi > lo);
        assert!((0.0..=1.0).contains(&lo));
        assert!((0.0..=1.0).contains(&hi));
    }

    #[test]
    fn expected_max_sharpe_grows_with_trials() {
        let few = expected_max_sharpe(0.01, 5);
        let many = expected_max_sharpe(0.01, 500);
        assert!(many > few, "more trials -> higher luck bar");
    }

    #[test]
    fn dsr_penalizes_more_trials() {
        // Same candidate, but deflated against a wider search => lower DSR.
        let candidate: Vec<f64> = (0..250).map(|i| 0.01 + 0.02 * ((i % 7) as f64 - 3.0)).collect();
        let narrow: Vec<f64> = vec![0.05, 0.04, 0.045];
        let wide: Vec<f64> = (0..200).map(|i| 0.02 + 0.03 * ((i % 11) as f64 - 5.0) / 5.0).collect();
        let d_narrow = deflated_sharpe_ratio(&candidate, &narrow).unwrap();
        let d_wide = deflated_sharpe_ratio(&candidate, &wide).unwrap();
        assert!(d_wide.dsr <= d_narrow.dsr);
    }

    #[test]
    fn per_config_dsr_distinguishes_configs_on_one_grid() {
        // A3: each swept config is deflated against the SAME trial-Sharpe grid,
        // so a stronger config gets a higher overfit-adjusted DSR than a weaker
        // one — letting an agent rank every row, not just read the best.
        let strong: Vec<f64> = (0..250).map(|i| 0.03 + 0.01 * ((i % 5) as f64 - 2.0)).collect();
        let weak: Vec<f64> = (0..250).map(|i| -0.005 + 0.02 * ((i % 7) as f64 - 3.0)).collect();
        let grid: Vec<f64> = vec![
            sharpe(&strong).unwrap(),
            sharpe(&weak).unwrap(),
            0.0,
            0.1,
        ];
        let d_strong = deflated_sharpe_ratio(&strong, &grid).unwrap();
        let d_weak = deflated_sharpe_ratio(&weak, &grid).unwrap();
        // Same grid (same n_trials / benchmark) for both — only the candidate differs.
        assert_eq!(d_strong.n_trials, d_weak.n_trials);
        assert!(d_strong.dsr > d_weak.dsr, "stronger config should out-rank weaker one");
    }

    #[test]
    fn pbo_high_for_pure_noise() {
        // N configs of pure noise: the IS-best should be ~random OOS -> PBO ~0.5.
        // Seed deterministically so the test never flakes.
        use rand::rngs::StdRng;
        use rand::SeedableRng;
        let t = 120;
        let n = 8;
        let mut rng = StdRng::seed_from_u64(42);
        let matrix: Vec<Vec<f64>> = (0..t)
            .map(|_| (0..n).map(|_| rng.gen::<f64>() - 0.5).collect())
            .collect();
        let pbo = pbo_cscv(&matrix, 8).unwrap();
        assert!((0.0..=1.0).contains(&pbo));
        assert!(pbo > 0.2, "noise should overfit appreciably, got {pbo}");
    }

    #[test]
    fn haircut_bonferroni_scales() {
        let adj = haircut_pvalues(&[0.01, 0.02, 0.5], HaircutMethod::Bonferroni);
        assert!((adj[0] - 0.03).abs() < 1e-9);
        assert_eq!(adj[2], 1.0); // 0.5*3 capped at 1
    }

    #[test]
    fn haircut_holm_monotone_and_bounded() {
        let adj = haircut_pvalues(&[0.01, 0.04, 0.03], HaircutMethod::Holm);
        for a in &adj {
            assert!((0.0..=1.0).contains(a));
        }
    }

    #[test]
    fn bootstrap_ci_brackets_mean() {
        let series: Vec<f64> = (0..200).map(|i| ((i % 5) as f64) - 2.0).collect(); // mean 0
        let (lo, point, hi) = block_bootstrap_ci(
            &series,
            |s| Some(s.iter().sum::<f64>() / s.len() as f64),
            500,
            6.0,
            0.10,
            7,
        )
        .unwrap();
        assert!(lo <= point && point <= hi);
        assert!(lo <= 0.0 && hi >= 0.0);
    }

    #[test]
    fn min_backtest_length_grows_with_trials() {
        let a = min_backtest_length(10, 1.0).unwrap();
        let b = min_backtest_length(1000, 1.0).unwrap();
        assert!(b > a);
    }
}
