//! Tail dependence — does the diversification survive a crisis, or do the
//! assets crash together?
//!
//! Correlation hides the failure mode an operator most cares about: two assets
//! can have modest Pearson correlation yet plunge *together* in a crash. The
//! lower-tail-dependence coefficient λ_L = lim_{q→0} P(Y in its bottom q | X in
//! its bottom q) measures exactly that — the probability one asset is crashing
//! given the other is. We report two estimates:
//!
//! - **Empirical** λ_L / λ_U at a finite quantile q (model-free): the share of
//!   days both assets sit in their joint lower (or upper) q-tail, normalized.
//! - **Clayton-copula** λ_L via Kendall's τ inversion (parametric, smooth): the
//!   Clayton copula has lower-tail dependence λ_L = 2^(−1/α) with α = 2τ/(1−τ).
//!   Clayton is the natural choice here because it models asymmetric LOWER-tail
//!   dependence (joint crashes) specifically.
//!
//! All `f64` — these are return co-movement statistics, not money.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TailDependence {
    /// Number of common-date return pairs.
    pub n: usize,
    /// Tail quantile used for the empirical estimates (e.g. 0.05).
    pub q: f64,
    /// Pearson linear correlation (for contrast with the tail measures).
    pub pearson: f64,
    /// Kendall's τ rank correlation.
    pub kendall_tau: f64,
    /// Clayton α = 2τ/(1−τ) (None when τ ≤ 0 — no positive dependence to model).
    pub clayton_alpha: Option<f64>,
    /// Clayton lower-tail dependence λ_L = 2^(−1/α) (0 when τ ≤ 0).
    pub clayton_lower_tail_dep: f64,
    /// Empirical lower-tail dependence at q: P(both in bottom q)/q.
    pub emp_lower_tail_dep: f64,
    /// Empirical upper-tail dependence at q: P(both in top q)/q.
    pub emp_upper_tail_dep: f64,
    /// Plain-language read of the lower-tail (co-crash) result.
    pub interpretation: String,
}

/// Compute tail dependence between two ALIGNED return series (same dates, same
/// length). `q` is the tail quantile for the empirical estimate (0.01–0.20).
pub fn tail_dependence(x: &[f64], y: &[f64], q: f64) -> Option<TailDependence> {
    let n = x.len().min(y.len());
    if n < 100 {
        return None;
    }
    let q = q.clamp(0.01, 0.20);
    let x = &x[..n];
    let y = &y[..n];

    let pearson = pearson(x, y);
    let kendall_tau = kendall_tau(x, y);

    let (clayton_alpha, clayton_lower) = if kendall_tau <= 0.0 {
        (None, 0.0)
    } else if 1.0 - kendall_tau < 1e-9 {
        // τ → 1: α → ∞, λ_L = 2^(−1/α) → 1 (perfect comonotonic tails).
        (None, 1.0)
    } else {
        let alpha = 2.0 * kendall_tau / (1.0 - kendall_tau);
        (Some(alpha), 2f64.powf(-1.0 / alpha))
    };

    // Empirical tail dependence at q.
    let lx = quantile(x, q);
    let ly = quantile(y, q);
    let ux = quantile(x, 1.0 - q);
    let uy = quantile(y, 1.0 - q);
    let both_low = (0..n).filter(|&i| x[i] <= lx && y[i] <= ly).count();
    let both_high = (0..n).filter(|&i| x[i] >= ux && y[i] >= uy).count();
    let denom = q * n as f64;
    let emp_lower_tail_dep = (both_low as f64 / denom).min(1.0);
    let emp_upper_tail_dep = (both_high as f64 / denom).min(1.0);

    let interpretation = if emp_lower_tail_dep >= 0.40 {
        format!(
            "STRONG co-crash dependence (λ_L≈{:.2}): they tend to fall together — diversification largely fails in a crash",
            emp_lower_tail_dep
        )
    } else if emp_lower_tail_dep >= 0.20 {
        format!(
            "MODERATE co-crash dependence (λ_L≈{:.2}): partial joint downside — diversification weakens but doesn't vanish in stress",
            emp_lower_tail_dep
        )
    } else {
        format!(
            "WEAK co-crash dependence (λ_L≈{:.2}): tails are largely independent — the diversification holds up in crises",
            emp_lower_tail_dep
        )
    };

    Some(TailDependence {
        n,
        q,
        pearson,
        kendall_tau,
        clayton_alpha,
        clayton_lower_tail_dep: clayton_lower,
        emp_lower_tail_dep,
        emp_upper_tail_dep,
        interpretation,
    })
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..x.len() {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx > 0.0 && vy > 0.0 {
        cov / (vx.sqrt() * vy.sqrt())
    } else {
        0.0
    }
}

/// Kendall's τ over non-tied pairs (O(n²); fine for a one-shot CLI over a few
/// thousand days). τ = (concordant − discordant) / (concordant + discordant).
fn kendall_tau(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len();
    let mut conc: i64 = 0;
    let mut disc: i64 = 0;
    for i in 0..n {
        for j in (i + 1)..n {
            let s = (x[i] - x[j]) * (y[i] - y[j]);
            if s > 0.0 {
                conc += 1;
            } else if s < 0.0 {
                disc += 1;
            }
        }
    }
    let denom = conc + disc;
    if denom == 0 {
        0.0
    } else {
        (conc - disc) as f64 / denom as f64
    }
}

/// Linear-interpolated quantile (type-7) of an unsorted slice.
fn quantile(v: &[f64], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let pos = q.clamp(0.0, 1.0) * (s.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        s[lo]
    } else {
        let f = pos - lo as f64;
        s[lo] * (1.0 - f) + s[hi] * f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_series_have_full_dependence() {
        let x: Vec<f64> = (0..500).map(|i| ((i * 7 % 11) as f64 - 5.0) / 100.0).collect();
        let d = tail_dependence(&x, &x, 0.05).unwrap();
        assert!(d.kendall_tau > 0.99, "tau={}", d.kendall_tau);
        assert!(d.pearson > 0.99);
        assert!(d.emp_lower_tail_dep > 0.9, "lower={}", d.emp_lower_tail_dep);
        assert!(d.clayton_lower_tail_dep > 0.5);
    }

    #[test]
    fn independent_series_have_low_tail_dependence() {
        // Two unrelated deterministic sequences (coprime strides → low rank corr).
        let x: Vec<f64> = (0..1000).map(|i| ((i * 13 % 97) as f64 - 48.0) / 100.0).collect();
        let y: Vec<f64> = (0..1000).map(|i| ((i * 31 % 89) as f64 - 44.0) / 100.0).collect();
        let d = tail_dependence(&x, &y, 0.05).unwrap();
        assert!(d.kendall_tau.abs() < 0.2, "tau={}", d.kendall_tau);
        assert!(d.emp_lower_tail_dep < 0.4, "lower={}", d.emp_lower_tail_dep);
    }

    #[test]
    fn negative_dependence_gives_no_clayton() {
        let x: Vec<f64> = (0..400).map(|i| i as f64 / 100.0).collect();
        let y: Vec<f64> = (0..400).map(|i| -(i as f64) / 100.0).collect();
        let d = tail_dependence(&x, &y, 0.05).unwrap();
        assert!(d.kendall_tau < -0.9);
        assert!(d.clayton_alpha.is_none());
        assert_eq!(d.clayton_lower_tail_dep, 0.0);
    }

    #[test]
    fn too_little_data_none() {
        assert!(tail_dependence(&[0.1; 50], &[0.1; 50], 0.05).is_none());
    }
}
