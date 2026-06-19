//! Extreme-Value-Theory tail risk — Peaks-Over-Threshold (POT) with a
//! Generalized Pareto Distribution (GPD) fit.
//!
//! Historical / Gaussian VaR understate crash depth for fat-tailed assets
//! (BTC, gold): the Gaussian assigns almost no mass to a −20% day, yet they
//! happen. The Pickands–Balkema–de Haan theorem says the distribution of
//! exceedances over a high threshold converges to a GPD *regardless of the
//! parent distribution* — so fitting the GPD to the left tail directly gives a
//! principled, tail-aware VaR / Expected-Shortfall and, via the shape
//! parameter ξ, a single number for *how fat* each asset's tail is.
//!
//! Estimator: closed-form **method of moments** (not an opaque optimizer) —
//! transparent and auditable, valid for ξ < ½ (finite variance, which holds
//! for daily financial returns). When the tail is heavier than that, the fit
//! is flagged unreliable rather than silently trusted. All math on `f64`
//! (these are return statistics, not money).

use serde::Serialize;

/// EVT tail-risk fit for a daily-return series. VaR/ES are POSITIVE loss
/// percentages (a 1-day loss you exceed with the stated tail probability).
#[derive(Debug, Clone, Serialize)]
pub struct EvtTailRisk {
    pub n_obs: usize,
    /// Threshold u as a loss percent (positive) — the start of the modeled tail.
    pub threshold_pct: f64,
    pub n_exceedances: usize,
    /// GPD shape ξ (tail index): >0 fat-tailed (power-law), 0 exponential, <0 bounded.
    pub xi: f64,
    /// GPD scale σ, in loss-percent units.
    pub sigma_pct: f64,
    /// EVT Value-at-Risk (1-day loss %) at 95 / 99 / 99.9% confidence.
    pub var_95_pct: f64,
    pub var_99_pct: f64,
    pub var_999_pct: f64,
    /// Expected Shortfall (mean loss beyond VaR) at 99%.
    pub es_99_pct: f64,
    /// Historical (empirical) VaR/ES at 99% for comparison with the EVT fit.
    pub hist_var_99_pct: f64,
    pub hist_es_99_pct: f64,
    /// Qualitative tail class from ξ.
    pub tail_class: String,
    /// False when the fit can't be trusted (too few exceedances, or ξ ≥ ½ so
    /// the method-of-moments variance assumption breaks down).
    pub reliable: bool,
    /// Human note on reliability / interpretation.
    pub note: String,
}

/// Fit the GPD tail to `returns` (daily simple returns, e.g. −0.05 = −5%).
/// `threshold_quantile` (e.g. 0.95) sets the loss threshold u at that quantile
/// of the loss distribution. Returns `None` if there is not enough data.
pub fn fit_evt_tail_risk(returns: &[f64], threshold_quantile: f64) -> Option<EvtTailRisk> {
    let n = returns.len();
    if n < 100 {
        return None; // EVT needs a meaningful sample to populate the tail
    }
    let tq = threshold_quantile.clamp(0.80, 0.99);
    // Losses (positive = a down day). Keep all observations; threshold the tail.
    let mut losses: Vec<f64> = returns.iter().map(|r| -r).filter(|l| l.is_finite()).collect();
    losses.sort_by(f64::total_cmp);
    let m = losses.len();
    let u = quantile_sorted(&losses, tq);
    let exceed: Vec<f64> = losses.iter().filter(|l| **l > u).map(|l| l - u).collect();
    let nu = exceed.len();

    // Empirical (historical) VaR/ES at 99% from the loss distribution.
    let hist_var_99 = quantile_sorted(&losses, 0.99);
    let tail_99: Vec<f64> = losses.iter().filter(|l| **l >= hist_var_99).copied().collect();
    let hist_es_99 = if tail_99.is_empty() {
        hist_var_99
    } else {
        tail_99.iter().sum::<f64>() / tail_99.len() as f64
    };

    // Method-of-moments GPD fit on the exceedances.
    // GPD(ξ,σ): mean = σ/(1−ξ), var = σ²/[(1−ξ)²(1−2ξ)] ⇒ mean²/var = 1−2ξ.
    let (xi, sigma, mom_ok) = if nu >= 10 {
        let mean = exceed.iter().sum::<f64>() / nu as f64;
        let var = exceed.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / (nu as f64 - 1.0);
        if var > 0.0 && mean > 0.0 {
            let xi = 0.5 * (1.0 - mean * mean / var);
            let sigma = mean * (1.0 - xi);
            (xi, sigma.max(1e-9), sigma > 0.0)
        } else {
            (0.0, 0.0, false)
        }
    } else {
        (0.0, 0.0, false)
    };

    // VaR at confidence α via the POT tail estimator. ξ→0 uses the exponential
    // limit. Falls back to historical when the MoM fit is unusable.
    let pot_var = |alpha: f64| -> f64 {
        if !mom_ok {
            return quantile_sorted(&losses, alpha);
        }
        let ratio = (m as f64 / nu as f64) * (1.0 - alpha); // < 1 for α > threshold
        if xi.abs() < 1e-6 {
            u - sigma * ratio.ln()
        } else {
            u + (sigma / xi) * (ratio.powf(-xi) - 1.0)
        }
    };
    let var_95 = pot_var(0.95);
    let var_99 = pot_var(0.99);
    let var_999 = pot_var(0.999);
    // ES_α = VaR_α/(1−ξ) + (σ − ξu)/(1−ξ), for ξ < 1.
    let es_99 = if mom_ok && xi < 1.0 {
        var_99 / (1.0 - xi) + (sigma - xi * u) / (1.0 - xi)
    } else {
        hist_es_99
    };

    let reliable = mom_ok && nu >= 20 && xi < 0.5;
    let tail_class = if !mom_ok {
        "unfitted".to_string()
    } else if xi >= 0.4 {
        "extreme (very heavy tail)".to_string()
    } else if xi >= 0.25 {
        "fat".to_string()
    } else if xi >= 0.1 {
        "moderate".to_string()
    } else if xi >= -0.05 {
        "near-normal".to_string()
    } else {
        "thin (bounded)".to_string()
    };
    let note = if nu < 10 {
        format!("only {nu} exceedances over the threshold — too few to fit a tail; showing historical VaR/ES")
    } else if !mom_ok {
        "degenerate exceedance distribution — falling back to historical VaR/ES".to_string()
    } else if xi >= 0.5 {
        format!("ξ={xi:.2} ≥ 0.5 — tail heavier than the method-of-moments variance assumption; EVT figures are indicative only")
    } else {
        let mult = if hist_var_99 > 0.0 { var_99 / hist_var_99 } else { 1.0 };
        format!("ξ={xi:.2} ({tail_class}); EVT 99% VaR is {mult:.2}× the historical 99% VaR")
    };

    Some(EvtTailRisk {
        n_obs: n,
        threshold_pct: u * 100.0,
        n_exceedances: nu,
        xi,
        sigma_pct: sigma * 100.0,
        var_95_pct: (var_95 * 100.0).max(0.0),
        var_99_pct: (var_99 * 100.0).max(0.0),
        var_999_pct: (var_999 * 100.0).max(0.0),
        es_99_pct: (es_99 * 100.0).max(0.0),
        hist_var_99_pct: (hist_var_99 * 100.0).max(0.0),
        hist_es_99_pct: (hist_es_99 * 100.0).max(0.0),
        tail_class,
        reliable,
        note,
    })
}

/// Linear-interpolated quantile of an ASCENDING-sorted slice (type-7).
fn quantile_sorted(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let f = pos - lo as f64;
        sorted[lo] * (1.0 - f) + sorted[hi] * f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic heavy-tailed loss sample via inverse-CDF of a Pareto, mixed
    /// with small gains, so the tail genuinely follows a power law (ξ > 0).
    fn pareto_returns(n: usize) -> Vec<f64> {
        // Pareto(xm=0.01, α=3) losses on a deterministic quantile grid → ξ≈1/α≈0.33.
        (0..n)
            .map(|i| {
                let p = (i as f64 + 0.5) / n as f64; // (0,1)
                let loss = 0.01 * (1.0 - p).powf(-1.0 / 3.0); // Pareto inverse-CDF
                -loss // as a (negative) return
            })
            .collect()
    }

    #[test]
    fn fits_fat_tail_and_orders_var_levels() {
        let r = pareto_returns(2000);
        let e = fit_evt_tail_risk(&r, 0.95).unwrap();
        // Power-law tail → positive shape.
        assert!(e.xi > 0.1, "expected fat tail, got xi={}", e.xi);
        // VaR monotone in confidence; ES ≥ VaR99.
        assert!(e.var_95_pct <= e.var_99_pct);
        assert!(e.var_99_pct <= e.var_999_pct);
        assert!(e.es_99_pct >= e.var_99_pct);
        assert!(e.reliable);
    }

    #[test]
    fn deterministic() {
        let r = pareto_returns(1500);
        let a = fit_evt_tail_risk(&r, 0.95).unwrap();
        let b = fit_evt_tail_risk(&r, 0.95).unwrap();
        assert_eq!(a.xi, b.xi);
        assert_eq!(a.var_999_pct, b.var_999_pct);
    }

    #[test]
    fn too_little_data_returns_none() {
        assert!(fit_evt_tail_risk(&[0.01; 50], 0.95).is_none());
    }

    #[test]
    fn thin_tail_has_nonpositive_shape() {
        // Uniform losses in [−1%, +1%] → bounded tail → ξ should be ≤ ~0.
        let r: Vec<f64> = (0..2000)
            .map(|i| ((i as f64 / 2000.0) - 0.5) * 0.02)
            .collect();
        let e = fit_evt_tail_risk(&r, 0.95).unwrap();
        assert!(e.xi < 0.1, "bounded tail should not look fat, got xi={}", e.xi);
    }
}
