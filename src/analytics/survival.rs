//! Drawdown survival & recovery — the TIME and SOLVENCY axis the rest of the
//! risk suite is missing. EVT (`evt.rs`), CDaR/Ulcer (`drawdown_metrics.rs`),
//! and co-crash λ_L (`copula.rs`) all answer "how deep?"; none answers "how
//! LONG underwater, and will I be forced out before the cycle turns?". For a
//! high-timeframe accumulator holding through a multi-year cycle, time-under-
//! water and risk-of-ruin are the binding constraints, not single-day depth.
//!
//! Three closed-form pieces (Bailey & López de Prado, "Stop-Outs Under Serial
//! Correlation"):
//! - **Recovery cliff** — the gain needed to erase a drawdown D: `D/(1−D)`
//!   (convex; a 50% loss needs +100%, an 80% loss needs +400%).
//! - **Triple Penance** — under Gaussian returns the expected max drawdown,
//!   the time to reach it, and the (≈3×-longer) recovery, at confidence α.
//! - **Risk of ruin** — the probability of ever breaching a drawdown budget B
//!   given the measured drift/vol, `exp(−2μ·b/σ²)` with `b = −ln(1−B)`.
//!
//! An AR(1) serial-correlation correction inflates the variance to the long-run
//! `σ²(1+φ)/(1−φ)` — trending cycles (φ>0) under-state underwater time by a lot
//! on the i.i.d. assumption. `μ ≤ 0` (the common case for an asset sitting at a
//! cycle low) makes recovery unbounded in expectation and ruin certain; that is
//! flagged loudly (`reliable=false`) rather than returning a misleading finite
//! number.

use serde::Serialize;

use crate::research::validation::normal_inv_cdf;

/// Survival/recovery readout for one asset.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Survival {
    /// Mean per-period (daily) log return, percent.
    pub mu_pct: f64,
    /// Per-period (daily) log-return volatility, percent.
    pub sigma_pct: f64,
    /// Lag-1 autocorrelation used for the AR(1) correction (clamped ±0.95).
    pub phi: f64,
    /// Expected max drawdown at confidence α — i.i.d. Gaussian, as an
    /// ARITHMETIC fraction (`1−e^−x` of the log-space Triple-Penance output, so
    /// it is comparable to CDaR/EVT and reads as the "price fell by X%" a human
    /// expects — NOT the raw log drop).
    pub max_dd_iid: Option<f64>,
    /// Expected max drawdown with the AR(1) variance correction (arithmetic).
    pub max_dd_ar1: Option<f64>,
    /// Time to reach the expected max drawdown (trading days), i.i.d.
    pub time_to_dd_days: Option<f64>,
    /// Time-to-trough with the AR(1) serial-correlation correction (days).
    pub time_to_dd_ar1_days: Option<f64>,
    /// Max time-under-water (drawdown + recovery ≈ 4·T_DD), i.i.d. (days).
    pub max_tuw_iid_days: Option<f64>,
    /// Max time-under-water with the AR(1) correction (days).
    pub max_tuw_ar1_days: Option<f64>,
    /// Gain needed to recover the measured CDaR-95 drawdown: `D/(1−D)` (fraction).
    pub recovery_required_at_cdar95: Option<f64>,
    /// The CDaR-95 drawdown that recovery figure is based on (fraction).
    pub cdar95: Option<f64>,
    /// P(ever breaching the drawdown budget) given drift/vol — `[0,1]`.
    pub ruin_prob: f64,
    /// Drawdown budget the ruin probability is measured against (percent).
    pub budget_pct: f64,
    /// Confidence α used for the Triple-Penance figures.
    pub confidence: f64,
    /// One-line regime read of the drift sign / reliability.
    pub regime: String,
    /// False when `μ ≤ 0` makes the recovery/ruin math degenerate (recovery
    /// unbounded, ruin certain) — the Triple-Penance numbers are then `None`.
    pub reliable: bool,
}

/// The gain (fraction) needed to recover a drawdown of depth `d` (fraction):
/// `d/(1−d)`. `d ≥ 1` (total ruin) → `None` (infinite).
pub fn recovery_required(d: f64) -> Option<f64> {
    if d >= 1.0 {
        None
    } else if d <= 0.0 {
        Some(0.0)
    } else {
        Some(d / (1.0 - d))
    }
}

/// Risk of ruin: probability of ever drawing down past budget `b_frac`
/// (fraction) given per-period drift `mu`/variance `var`. `exp(−2μ·barrier/σ²)`
/// with `barrier = −ln(1−b_frac)`, clamped to `[0,1]`. Non-positive drift → 1.0.
pub fn risk_of_ruin(mu: f64, var: f64, b_frac: f64) -> f64 {
    if mu <= 0.0 {
        return 1.0;
    }
    if var <= 0.0 || b_frac <= 0.0 {
        return 0.0;
    }
    if b_frac >= 1.0 {
        // Budget of a total loss — only reached at ruin; use the barrier limit.
        return 0.0;
    }
    let barrier = -(1.0 - b_frac).ln();
    (-2.0 * mu * barrier / var).exp().clamp(0.0, 1.0)
}

const MIN_OBS: usize = 30;

/// Compute the survival readout from per-period (daily) log returns, the
/// already-measured CDaR-95 (fraction, from `drawdown_metrics`), a drawdown
/// budget (percent), and confidence α. `None` below [`MIN_OBS`] returns or when
/// the series is (near-)constant.
pub fn compute(
    log_returns: &[f64],
    cdar95: Option<f64>,
    budget_pct: f64,
    confidence: f64,
) -> Option<Survival> {
    let n = log_returns.len();
    if n < MIN_OBS {
        return None;
    }
    let mu = log_returns.iter().sum::<f64>() / n as f64;
    let var = log_returns.iter().map(|r| (r - mu).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    if var <= 1e-12 {
        return None; // constant series → no risk dynamics
    }
    let sigma = var.sqrt();
    // Lag-1 autocorrelation φ for the AR(1) long-run variance correction.
    let phi = {
        let num: f64 = (1..n).map(|t| (log_returns[t] - mu) * (log_returns[t - 1] - mu)).sum();
        let den: f64 = log_returns.iter().map(|r| (r - mu).powi(2)).sum();
        if den > 0.0 { (num / den).clamp(-0.95, 0.95) } else { 0.0 }
    };
    let var_lr = var * (1.0 + phi) / (1.0 - phi);

    let z = normal_inv_cdf(confidence.clamp(0.5 + 1e-9, 1.0 - 1e-9));
    let budget = budget_pct / 100.0;
    let ruin_prob = risk_of_ruin(mu, var, budget);

    // CDaR-95 recovery cliff (uses the measured path drawdown, not the model).
    let recovery_required_at_cdar95 = cdar95.and_then(recovery_required);

    if mu <= 0.0 {
        // Degenerate: no positive drift ⇒ recovery unbounded, ruin certain.
        // Return the measured pieces but flag the model figures as unavailable.
        return Some(Survival {
            mu_pct: mu * 100.0,
            sigma_pct: sigma * 100.0,
            phi,
            max_dd_iid: None,
            max_dd_ar1: None,
            time_to_dd_days: None,
            time_to_dd_ar1_days: None,
            max_tuw_iid_days: None,
            max_tuw_ar1_days: None,
            recovery_required_at_cdar95,
            cdar95,
            ruin_prob,
            budget_pct,
            confidence,
            regime: "no positive drift (μ≤0) — recovery unbounded in expectation, ruin certain; depth still measured by EVT/CDaR".to_string(),
            reliable: false,
        });
    }

    // Triple Penance in LOG-wealth space: T_DD = (z·σ/(2μ))²; the max LOG
    // drawdown = z²σ²/(4μ); recovery ≈ 3·T_DD so total time-under-water ≈ 4·T_DD.
    let t_dd = (z * sigma / (2.0 * mu)).powi(2);
    let t_dd_ar1 = t_dd * (1.0 + phi) / (1.0 - phi);
    let max_dd_log_iid = z * z * var / (4.0 * mu);
    let max_dd_log_ar1 = z * z * var_lr / (4.0 * mu);
    // Convert the log drop to an ARITHMETIC drawdown fraction (1−e^−x), so it is
    // comparable to the arithmetic CDaR/EVT figures shown alongside it and reads
    // as the actual percentage price fall (log 0.27 → arithmetic 0.237).
    let max_dd_iid = 1.0 - (-max_dd_log_iid).exp();
    let max_dd_ar1 = 1.0 - (-max_dd_log_ar1).exp();
    let max_tuw_iid = 4.0 * t_dd;
    let max_tuw_ar1 = 4.0 * t_dd_ar1;

    let regime = if phi > 0.05 {
        format!("positive drift; trending (φ={phi:.2}) — AR(1) figures are the honest ones, i.i.d. understates underwater time")
    } else {
        "positive drift; near-i.i.d. returns".to_string()
    };

    Some(Survival {
        mu_pct: mu * 100.0,
        sigma_pct: sigma * 100.0,
        phi,
        max_dd_iid: Some(max_dd_iid),
        max_dd_ar1: Some(max_dd_ar1),
        time_to_dd_days: Some(t_dd),
        time_to_dd_ar1_days: Some(t_dd_ar1),
        max_tuw_iid_days: Some(max_tuw_iid),
        max_tuw_ar1_days: Some(max_tuw_ar1),
        recovery_required_at_cdar95,
        cdar95,
        ruin_prob,
        budget_pct,
        confidence,
        regime,
        reliable: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_cliff_known_values() {
        assert_eq!(recovery_required(0.50), Some(1.00));
        assert!((recovery_required(0.20).unwrap() - 0.25).abs() < 1e-12);
        assert!((recovery_required(0.80).unwrap() - 4.00).abs() < 1e-12);
        assert_eq!(recovery_required(1.0), None); // total ruin
        assert_eq!(recovery_required(-0.1), Some(0.0)); // not underwater
    }

    #[test]
    fn risk_of_ruin_known_value() {
        // μ=0.001, σ²=0.0004, B=0.25 → b=−ln(0.75)=0.287682;
        // P=exp(−2·0.001·0.287682/0.0004)=exp(−1.438410)=0.23729.
        let p = risk_of_ruin(0.001, 0.0004, 0.25);
        assert!((p - 0.237289).abs() < 1e-4, "ruin p={p}");
        // Non-positive drift → certain ruin.
        assert_eq!(risk_of_ruin(-0.0005, 0.0004, 0.25), 1.0);
        assert_eq!(risk_of_ruin(0.0, 0.0004, 0.25), 1.0);
    }

    /// Synthetic near-i.i.d. log returns with a target μ and σ (deterministic):
    /// alternate ±σ around μ so mean=μ, sample std≈σ, autocorrelation≈−1...
    /// instead use a low-autocorr pattern: a length-4 repeating cycle.
    fn series_mu_sigma(mu: f64, sigma: f64, n: usize) -> Vec<f64> {
        // pattern sums to 0 and has rms = sigma, with modest autocorrelation.
        let pat = [sigma, -sigma, sigma, -sigma];
        (0..n).map(|t| mu + pat[t % 4]).collect()
    }

    #[test]
    fn triple_penance_matches_hand_computation() {
        // Check the Triple-Penance FORMULAS directly on exact μ=0.001, σ=0.02
        // (the ±σ test series gives a slightly different sample var). MaxDD here
        // is the LOG-space drop; the reported field is its arithmetic conversion.
        let mu = 0.001_f64;
        let sigma = 0.02_f64;
        let z = normal_inv_cdf(0.95);
        let t_dd = (z * sigma / (2.0 * mu)).powi(2);
        let max_dd_log = z * z * (sigma * sigma) / (4.0 * mu);
        let max_tuw = 4.0 * t_dd;
        assert!((t_dd - 270.55).abs() < 0.1, "T_DD={t_dd}");
        assert!((max_dd_log - 0.270554).abs() < 1e-4, "MaxDD(log)={max_dd_log}");
        assert!((max_tuw - 1082.2).abs() < 0.5, "MaxTuW={max_tuw}");
        // And compute() wires it together without panicking on a real series.
        let s = compute(&series_mu_sigma(mu, sigma, 400), Some(0.3), 25.0, 0.95).unwrap();
        assert!(s.reliable);
        assert!(s.max_dd_iid.unwrap() > 0.0);
        assert_eq!(s.recovery_required_at_cdar95, recovery_required(0.3));
    }

    #[test]
    fn max_dd_is_reported_in_arithmetic_space() {
        // QA #974: the Triple-Penance MaxDD is a LOG drop; the reported field
        // must be the arithmetic `1−e^−x` so it is comparable to CDaR/EVT.
        let series = series_mu_sigma(0.001, 0.02, 400);
        let n = series.len() as f64;
        let mu = series.iter().sum::<f64>() / n;
        let var = series.iter().map(|r| (r - mu).powi(2)).sum::<f64>() / (n - 1.0);
        let z = normal_inv_cdf(0.95);
        let log_dd = z * z * var / (4.0 * mu);
        let expected_arith = 1.0 - (-log_dd).exp();
        let s = compute(&series, None, 25.0, 0.95).unwrap();
        assert!(
            (s.max_dd_iid.unwrap() - expected_arith).abs() < 1e-9,
            "max_dd_iid should be arithmetic 1−e^−log: got {} want {expected_arith}",
            s.max_dd_iid.unwrap()
        );
        // The conversion strictly shrinks a positive drop: arithmetic < log.
        assert!(s.max_dd_iid.unwrap() < log_dd);
    }

    /// Positive-lag-1-autocorrelation series (a slow sinusoid, φ≈cos(0.6)≈0.82,
    /// unclamped) — the trending case the AR(1) correction exists for.
    fn series_positive_autocorr(mu: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n).map(|t| mu + amp * (t as f64 * 0.6).sin()).collect()
    }

    #[test]
    fn ar1_inflates_underwater_time_for_a_trending_series() {
        let s = compute(&series_positive_autocorr(0.001, 0.03, 400), None, 25.0, 0.95).unwrap();
        assert!(s.phi > 0.05, "expected positive autocorrelation, got φ={}", s.phi);
        // φ>0 → long-run variance inflated → AR(1) depth AND time exceed i.i.d.
        assert!(s.max_dd_ar1.unwrap() > s.max_dd_iid.unwrap());
        assert!(s.max_tuw_ar1_days.unwrap() > s.max_tuw_iid_days.unwrap());
        assert!(s.time_to_dd_ar1_days.unwrap() > s.time_to_dd_days.unwrap());
    }

    #[test]
    fn ar1_correction_inflates_variance() {
        // φ=0.20 → σ²_LR = 1.5·σ² → MaxDD_ar1 = 1.5·MaxDD_iid. Construct a series
        // with positive lag-1 autocorrelation and check the ratio direction.
        let s = compute(&series_mu_sigma(0.001, 0.02, 400), None, 25.0, 0.95).unwrap();
        // The ±σ-alternating pattern is NEGATIVELY autocorrelated, so AR(1)
        // should DEFLATE here — assert the correction moves in φ's direction.
        if s.phi > 0.0 {
            assert!(s.max_dd_ar1.unwrap() >= s.max_dd_iid.unwrap());
        } else {
            assert!(s.max_dd_ar1.unwrap() <= s.max_dd_iid.unwrap());
        }
    }

    #[test]
    fn non_positive_drift_is_flagged_not_silent() {
        // μ ≤ 0 → reliable=false, model figures None, ruin certain.
        let s = compute(&series_mu_sigma(-0.0005, 0.02, 400), Some(0.4), 25.0, 0.95).unwrap();
        assert!(!s.reliable);
        assert!(s.max_dd_iid.is_none());
        assert_eq!(s.ruin_prob, 1.0);
        assert!(s.regime.contains("μ≤0") || s.regime.contains("no positive drift"));
        // The measured CDaR recovery cliff is still reported.
        assert_eq!(s.recovery_required_at_cdar95, recovery_required(0.4));
    }

    #[test]
    fn guards_small_and_constant() {
        assert!(compute(&vec![0.001; 29], None, 25.0, 0.95).is_none()); // < 30
        assert!(compute(&vec![0.001; 50], None, 25.0, 0.95).is_none()); // constant
    }
}
