//! Drawdown-path-coherent risk metrics — the one dimension the tearsheet's
//! return-based ratios (Sharpe/Sortino) and single-point max-drawdown miss.
//!
//! - **CDaR** (Conditional Drawdown at Risk) — the tail mean of the *drawdown
//!   distribution*, the drawdown analogue of CVaR on returns. Where Calmar
//!   collapses the whole drawdown experience into one worst point, CDaR(β) is
//!   "the average depth of the worst (1−β) fraction of drawdowns I sat
//!   through" — a path-coherent risk budget for a cycle accumulator. It
//!   complements the EVT left-tail of *returns* in `evt.rs` (a different axis).
//! - **Ulcer Index / Martin Ratio** — RMS of percentage drawdowns from the
//!   running peak; penalizes the *duration* of underwater periods, not just the
//!   deepest point. Ideal for a buy-the-low-and-hold strategy.
//! - **Omega ratio** — probability-weighted gains vs losses about a threshold,
//!   capturing the full return-distribution shape with no Gaussian assumption.
//!
//! All pure `f64` math over an equity curve (no I/O, no dates). The CDaR
//! estimator is the coherent discrete form: mean of the worst
//! `ceil((1−β)·n)` drawdown observations (monotone in β by construction).

use serde::Serialize;

/// Drawdown-path risk metrics over one equity curve.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DrawdownMetrics {
    /// RMS percentage drawdown from the running peak (Peter Martin's Ulcer
    /// Index), in percent. 0.0 = never underwater.
    pub ulcer_index_pct: f64,
    /// (annualized_return − risk_free) / Ulcer Index. `None` when the caller
    /// passed no annualized return, or UI is 0 (no drawdown → undefined).
    pub martin_ratio: Option<f64>,
    /// Drawdown at Risk (β=0.90): the 90th-percentile drawdown depth (fraction).
    pub dar_90: f64,
    /// Conditional DaR (β=0.90): mean of the worst-decile drawdowns (fraction).
    pub cdar_90: f64,
    /// Drawdown at Risk (β=0.95) (fraction).
    pub dar_95: f64,
    /// Conditional DaR (β=0.95): mean of the worst-5% drawdowns (fraction).
    pub cdar_95: f64,
    /// Omega ratio about τ=0 over the curve's per-step returns. `None` when
    /// there are no losing steps (ratio → ∞).
    pub omega_ratio: Option<f64>,
}

/// Minimum equity-curve points before the metrics are trusted.
const MIN_POINTS: usize = 20;

/// Compute the drawdown-path metrics for an equity curve (oldest→newest, each
/// value > 0). `annualized_return_pct` feeds the Martin ratio (pass `None` to
/// skip it). `risk_free_pct` is subtracted from the annualized return there.
/// Returns `None` if the curve has fewer than [`MIN_POINTS`] points.
pub fn compute(
    equity: &[f64],
    annualized_return_pct: Option<f64>,
    risk_free_pct: f64,
) -> Option<DrawdownMetrics> {
    if equity.len() < MIN_POINTS {
        return None;
    }
    let dd = drawdown_series(equity);
    let (dar_90, cdar_90) = cdar(&dd, 0.90);
    let (dar_95, cdar_95) = cdar(&dd, 0.95);
    let ui = ulcer_index_pct(equity);
    let martin = annualized_return_pct.and_then(|c| {
        (ui > 0.0).then_some((c - risk_free_pct) / ui)
    });
    let rets = step_returns(equity);
    let omega = omega_ratio(&rets, 0.0);
    Some(DrawdownMetrics {
        ulcer_index_pct: ui,
        martin_ratio: martin,
        dar_90,
        cdar_90,
        dar_95,
        cdar_95,
        omega_ratio: omega,
    })
}

/// Running maximum (high-water mark) of a series.
fn running_max(series: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(series.len());
    let mut peak = f64::MIN;
    for &v in series {
        if v > peak {
            peak = v;
        }
        out.push(peak);
    }
    out
}

/// Drawdown at each point as a non-negative fraction `(peak − value)/peak`.
fn drawdown_series(equity: &[f64]) -> Vec<f64> {
    let peaks = running_max(equity);
    equity
        .iter()
        .zip(peaks.iter())
        .map(|(&e, &m)| if m > 0.0 { ((m - e) / m).max(0.0) } else { 0.0 })
        .collect()
}

/// `(DaR, CDaR)` at confidence β via the coherent discrete estimator: sort
/// drawdowns descending, take the worst `k = ceil((1−β)·n)` (≥1), CDaR = their
/// mean and DaR = the smallest of them (the tail threshold). Monotone in β:
/// a larger β picks a smaller/worse subset, so CDaR(0.95) ≥ CDaR(0.90).
fn cdar(drawdowns: &[f64], beta: f64) -> (f64, f64) {
    let n = drawdowns.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut sorted = drawdowns.to_vec();
    // Descending.
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = (((1.0 - beta) * n as f64).ceil() as usize).clamp(1, n);
    let tail = &sorted[..k];
    let cdar = tail.iter().sum::<f64>() / k as f64;
    let dar = tail[k - 1]; // smallest of the worst-k = the tail threshold
    (dar, cdar)
}

/// Ulcer Index (percent): RMS of percentage drawdowns from the running peak.
fn ulcer_index_pct(equity: &[f64]) -> f64 {
    let peaks = running_max(equity);
    let n = equity.len();
    if n == 0 {
        return 0.0;
    }
    let sum_sq: f64 = equity
        .iter()
        .zip(peaks.iter())
        .map(|(&e, &m)| {
            if m > 0.0 {
                let pd = 100.0 * (e - m) / m; // ≤ 0
                pd * pd
            } else {
                0.0
            }
        })
        .sum();
    (sum_sq / n as f64).sqrt()
}

/// Per-step simple returns of an equity curve.
fn step_returns(equity: &[f64]) -> Vec<f64> {
    equity
        .windows(2)
        .filter_map(|w| (w[0] != 0.0).then_some(w[1] / w[0] - 1.0))
        .collect()
}

/// Omega ratio about threshold τ: Σmax(r−τ,0) / Σmax(τ−r,0). `None` when there
/// is no downside mass (ratio undefined / → ∞).
fn omega_ratio(returns: &[f64], tau: f64) -> Option<f64> {
    if returns.len() < 5 {
        return None;
    }
    let gains: f64 = returns.iter().map(|r| (r - tau).max(0.0)).sum();
    let losses: f64 = returns.iter().map(|r| (tau - r).max(0.0)).sum();
    (losses > 0.0).then_some(gains / losses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_up_curve_has_no_drawdown_risk() {
        let equity: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let m = compute(&equity, Some(10.0), 0.0).unwrap();
        assert_eq!(m.ulcer_index_pct, 0.0);
        assert_eq!(m.cdar_90, 0.0);
        assert_eq!(m.cdar_95, 0.0);
        assert_eq!(m.dar_90, 0.0);
        // UI == 0 → Martin ratio undefined.
        assert!(m.martin_ratio.is_none());
        // No losing steps → Omega undefined.
        assert!(m.omega_ratio.is_none());
    }

    #[test]
    fn cdar_known_answer() {
        // Two drawdown episodes; hand-computed drawdowns (fractions of peak 110):
        // [0,0, 10/110, 20/110, 10/110, 0, 10/110, 15/110, 10/110, 0]
        let dd = vec![
            0.0,
            0.0,
            10.0 / 110.0,
            20.0 / 110.0,
            10.0 / 110.0,
            0.0,
            10.0 / 110.0,
            15.0 / 110.0,
            10.0 / 110.0,
            0.0,
        ];
        // β=0.90, n=10 → k=ceil(1.0)=1 → worst single = 20/110.
        let (dar90, cdar90) = cdar(&dd, 0.90);
        assert!((cdar90 - 20.0 / 110.0).abs() < 1e-12);
        assert!((dar90 - 20.0 / 110.0).abs() < 1e-12);
        // β=0.80, n=10 → k=ceil(2.0)=2 → mean(20/110, 15/110)=35/220.
        let (dar80, cdar80) = cdar(&dd, 0.80);
        assert!((cdar80 - 35.0 / 220.0).abs() < 1e-12);
        assert!((dar80 - 15.0 / 110.0).abs() < 1e-12);
    }

    #[test]
    fn ulcer_index_known_answer() {
        // equity [100,95,90,95,100] → PD [0,-5,-10,-5,0] → sqrt(150/5)=sqrt(30).
        let equity = vec![100.0, 95.0, 90.0, 95.0, 100.0];
        let ui = ulcer_index_pct(&equity);
        assert!((ui - 30.0_f64.sqrt()).abs() < 1e-9, "ui={ui}");
    }

    #[test]
    fn omega_known_answer() {
        // returns [.01,.02,-.01,.03,-.02], τ=0: gains .06 / losses .03 = 2.0.
        let rets = vec![0.01, 0.02, -0.01, 0.03, -0.02];
        let om = omega_ratio(&rets, 0.0).unwrap();
        assert!((om - 2.0).abs() < 1e-12, "omega={om}");
    }

    #[test]
    fn too_few_points_is_none() {
        let equity = vec![100.0; 15];
        assert!(compute(&equity, None, 0.0).is_none());
    }

    #[test]
    fn cdar_is_monotone_in_beta() {
        // A noisy curve: CDaR(0.95) ≥ CDaR(0.90) ≥ CDaR(0.80) by construction.
        let equity: Vec<f64> = (0..200)
            .map(|i| {
                let t = i as f64;
                100.0 + t * 0.1 + 8.0 * (t / 9.0).sin()
            })
            .collect();
        let dd = drawdown_series(&equity);
        let (_, c95) = cdar(&dd, 0.95);
        let (_, c90) = cdar(&dd, 0.90);
        let (_, c80) = cdar(&dd, 0.80);
        assert!(c95 >= c90 - 1e-12 && c90 >= c80 - 1e-12, "c95={c95} c90={c90} c80={c80}");
        // And the full metric bundle resolves on a long enough curve.
        let m = compute(&equity, Some(5.0), 0.0).unwrap();
        assert!(m.cdar_95 >= m.cdar_90 - 1e-12);
        assert!(m.ulcer_index_pct > 0.0);
        assert!(m.martin_ratio.is_some());
    }
}
