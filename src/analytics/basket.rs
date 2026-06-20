//! Basket allocation — turn a set of assets' return histories into portfolio
//! weights under three risk-aware schemes, with the per-asset risk
//! contributions and the diversification ratio that justify them.
//!
//! - **equal** — naive 1/N (the baseline every scheme should beat on risk).
//! - **inverse-vol** — `w_i ∝ 1/σ_i`; equalizes *standalone* risk, ignores
//!   correlation. Cheap and robust; the right answer when correlations are
//!   unknown or unstable.
//! - **risk-parity (ERC)** — equal *risk contribution*: each asset adds the
//!   same share of portfolio variance, accounting for the full covariance.
//!   Solved by the Maillard fixed-point `w_i ← b_i / (Σ_j cov_ij w_j)`
//!   (normalized each step), which converges to the unique long-only ERC
//!   portfolio for a positive-semidefinite covariance.
//!
//! All pure `f64`. Inputs are per-asset return series already aligned on a
//! common date axis (same length, same dates) — the caller does the alignment.

use serde::Serialize;

/// One asset's place in the allocation.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AssetWeight {
    pub symbol: String,
    /// Portfolio weight (fraction, sums to 1 across the basket).
    pub weight: f64,
    /// Annualized standalone volatility (percent).
    pub vol_pct: f64,
    /// Share of total portfolio variance this asset contributes (fraction;
    /// sums to 1). For risk-parity these are all ≈ 1/N.
    pub risk_contribution: f64,
}

/// A full basket allocation result.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BasketAllocation {
    pub method: String,
    pub weights: Vec<AssetWeight>,
    /// Annualized portfolio volatility at these weights (percent).
    pub portfolio_vol_pct: f64,
    /// Diversification ratio `(Σ wᵢσᵢ) / σ_portfolio` ≥ 1 — higher means the
    /// weighting captures more diversification benefit (1.0 = none). Always on
    /// the FULL covariance, so it is comparable across methods.
    pub diversification_ratio: f64,
    /// What the `risk_contribution` figures (and the solver) are based on:
    /// `variance` for equal/inverse-vol/risk-parity, `semivariance` for the
    /// downside method (co-downside only). `portfolio_vol`/`diversification`
    /// are always full-variance regardless.
    pub risk_basis: String,
    /// A caveat about the result when set — e.g. a downside-RP basket that
    /// contains an asset with no downside history (zero semivariance), which the
    /// ERC then assigns 0% (its co-downside risk is zero at any weight). `None`
    /// for a clean allocation.
    pub note: Option<String>,
    /// Number of aligned observations the covariance was estimated on.
    pub n_obs: usize,
}

const TRADING_DAYS: f64 = 252.0;

/// Allocation scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Equal,
    InverseVol,
    RiskParity,
    /// Equal risk contribution on the SEMIcovariance (co-downside only) — sizes
    /// for joint-crash risk rather than symmetric volatility.
    DownsideRiskParity,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Equal => "equal",
            Method::InverseVol => "inverse-vol",
            Method::RiskParity => "risk-parity",
            Method::DownsideRiskParity => "downside-risk-parity",
        }
    }
    /// True when the solver and reported risk contributions use the
    /// semicovariance rather than the full covariance.
    pub fn is_downside(self) -> bool {
        matches!(self, Method::DownsideRiskParity)
    }
    pub fn parse(s: &str) -> Option<Method> {
        match s.to_ascii_lowercase().as_str() {
            "equal" | "equal-weight" | "1/n" => Some(Method::Equal),
            "inverse-vol" | "inverse-volatility" | "inv-vol" | "ivp" => Some(Method::InverseVol),
            "risk-parity" | "erc" | "risk_parity" => Some(Method::RiskParity),
            "downside-risk-parity" | "downside" | "semivariance" | "semi-rp" | "drp" => {
                Some(Method::DownsideRiskParity)
            }
            _ => None,
        }
    }
}

/// Zero-target (Hogan-Warren) semicovariance matrix: `SC_ij = (1/n) Σ_t
/// min(r_i,t, 0)·min(r_j,t, 0)` — the co-movement of LOSSES only (returns below
/// the 0 target; daily returns are near-zero-mean so this ≈ the demeaned
/// Estrada form). Symmetric and positive-semidefinite (a Gram matrix of the
/// downside-clipped return vectors), so the ERC solver applies unchanged. The
/// diagonal is each asset's downside semivariance; an asset that NEVER loses
/// has a zero diagonal (see `allocate`'s degeneracy note).
pub fn semicovariance_matrix(series: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let k = series.len();
    let mut sc = vec![vec![0.0; k]; k];
    if k == 0 {
        return sc;
    }
    let n = series[0].len();
    if n == 0 {
        return sc;
    }
    // Downside-clipped series: min(r, 0).
    let down: Vec<Vec<f64>> = series
        .iter()
        .map(|s| s.iter().map(|r| r.min(0.0)).collect())
        .collect();
    for i in 0..k {
        for j in i..k {
            let acc: f64 = down[i].iter().zip(down[j].iter()).map(|(a, b)| a * b).sum();
            let c = acc / n as f64;
            sc[i][j] = c;
            sc[j][i] = c;
        }
    }
    sc
}

/// Population covariance matrix of `series` (one inner Vec per asset, all the
/// same length). `cov[i][j]` over the aligned observations.
pub fn covariance_matrix(series: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let k = series.len();
    let mut cov = vec![vec![0.0; k]; k];
    if k == 0 {
        return cov;
    }
    let n = series[0].len();
    if n == 0 {
        return cov;
    }
    let means: Vec<f64> = series.iter().map(|s| s.iter().sum::<f64>() / n as f64).collect();
    for i in 0..k {
        for j in i..k {
            let acc: f64 = series[i]
                .iter()
                .zip(series[j].iter())
                .map(|(a, b)| (a - means[i]) * (b - means[j]))
                .sum();
            let c = acc / n as f64;
            cov[i][j] = c;
            cov[j][i] = c;
        }
    }
    cov
}

/// Matrix·vector: `(Cov · w)_i`.
fn cov_times(cov: &[Vec<f64>], w: &[f64]) -> Vec<f64> {
    cov.iter()
        .map(|row| row.iter().zip(w).map(|(c, wj)| c * wj).sum())
        .collect()
}

/// Portfolio variance `wᵀ Cov w`.
fn portfolio_variance(cov: &[Vec<f64>], w: &[f64]) -> f64 {
    cov_times(cov, w).iter().zip(w).map(|(cw, wi)| cw * wi).sum()
}

/// Normalize a weight vector to sum 1 (no-op if the sum is ~0).
fn normalize(w: &mut [f64]) {
    let s: f64 = w.iter().sum();
    if s.abs() > 1e-15 {
        for x in w.iter_mut() {
            *x /= s;
        }
    }
}

/// Inverse-volatility weights from per-asset standalone vols.
pub fn inverse_vol_weights(vols: &[f64]) -> Vec<f64> {
    let mut w: Vec<f64> = vols
        .iter()
        .map(|v| if *v > 0.0 { 1.0 / v } else { 0.0 })
        .collect();
    normalize(&mut w);
    w
}

/// Equal-risk-contribution (risk parity) weights via the Maillard fixed point
/// `w_i ← (1/N) / (Cov w)_i`, normalized each iteration. Seeded from
/// inverse-vol. Returns the converged long-only weights.
pub fn risk_parity_weights(cov: &[Vec<f64>]) -> Vec<f64> {
    let k = cov.len();
    if k == 0 {
        return vec![];
    }
    if k == 1 {
        return vec![1.0];
    }
    let b = 1.0 / k as f64;
    // Seed from inverse-vol (diagonal = variances).
    let vols: Vec<f64> = (0..k).map(|i| cov[i][i].max(0.0).sqrt()).collect();
    let mut w = inverse_vol_weights(&vols);
    if w.iter().sum::<f64>() <= 0.0 {
        w = vec![1.0 / k as f64; k];
    }
    for _ in 0..10_000 {
        let m = cov_times(cov, &w); // marginal risk (unnormalized)
        let mut w_new: Vec<f64> = (0..k)
            .map(|i| if m[i] > 1e-18 { b / m[i] } else { w[i] })
            .collect();
        normalize(&mut w_new);
        // Convergence: max weight change below tolerance.
        let delta = w_new
            .iter()
            .zip(&w)
            .map(|(a, c)| (a - c).abs())
            .fold(0.0_f64, f64::max);
        w = w_new;
        if delta < 1e-12 {
            break;
        }
    }
    w
}

/// Per-asset fractional risk contribution `RC_i = w_i·(Cov w)_i / (wᵀCov w)`
/// (sums to 1).
pub fn risk_contributions(cov: &[Vec<f64>], w: &[f64]) -> Vec<f64> {
    let pv = portfolio_variance(cov, w);
    let m = cov_times(cov, w);
    if pv <= 0.0 {
        return vec![0.0; w.len()];
    }
    w.iter().zip(&m).map(|(wi, mi)| wi * mi / pv).collect()
}

/// Assemble a full [`BasketAllocation`] for `method` over aligned daily return
/// `series` (one per symbol, same order as `symbols`). Returns `None` if there
/// are fewer than two assets or fewer than 20 aligned observations.
pub fn allocate(symbols: &[String], series: &[Vec<f64>], method: Method) -> Option<BasketAllocation> {
    let k = symbols.len();
    if k < 2 || series.len() != k {
        return None;
    }
    let n = series[0].len();
    if n < 20 || series.iter().any(|s| s.len() != n) {
        return None;
    }
    let cov = covariance_matrix(series);
    // Daily standalone vols → annualized percent (always full-variance).
    let daily_vols: Vec<f64> = (0..k).map(|i| cov[i][i].max(0.0).sqrt()).collect();
    let vols_ann_pct: Vec<f64> = daily_vols.iter().map(|v| v * TRADING_DAYS.sqrt() * 100.0).collect();

    // The downside method drives its ERC solver and risk contributions off the
    // SEMIcovariance; every other method uses the full covariance. Portfolio
    // vol + diversification stay full-variance for cross-method comparability.
    let risk_mat = if method.is_downside() {
        semicovariance_matrix(series)
    } else {
        cov.clone()
    };
    let w = match method {
        Method::Equal => vec![1.0 / k as f64; k],
        Method::InverseVol => inverse_vol_weights(&daily_vols),
        Method::RiskParity | Method::DownsideRiskParity => risk_parity_weights(&risk_mat),
    };
    let rc = risk_contributions(&risk_mat, &w);

    // Degeneracy note: under the downside method an asset with NO losing days
    // has a zero semivariance diagonal, so ERC freezes it at 0% (its
    // co-downside risk is zero at any weight — the equal-risk problem is
    // ill-posed for it). Mathematically correct but worth flagging.
    let note = if method.is_downside() {
        let zero_downside: Vec<&str> = (0..k)
            .filter(|&i| risk_mat[i][i] <= 1e-18)
            .map(|i| symbols[i].as_str())
            .collect();
        (!zero_downside.is_empty()).then(|| {
            format!(
                "{} had no downside (losing) days in the window — downside-RP assigns them 0% (zero co-downside risk at any weight)",
                zero_downside.join(", ")
            )
        })
    } else {
        None
    };

    let port_var = portfolio_variance(&cov, &w);
    let port_vol_daily = port_var.max(0.0).sqrt();
    let port_vol_ann_pct = port_vol_daily * TRADING_DAYS.sqrt() * 100.0;
    // Diversification ratio = weighted avg standalone vol / portfolio vol.
    let weighted_vol: f64 = w.iter().zip(&daily_vols).map(|(wi, v)| wi * v).sum();
    let dr = if port_vol_daily > 1e-18 { weighted_vol / port_vol_daily } else { 1.0 };

    let weights = (0..k)
        .map(|i| AssetWeight {
            symbol: symbols[i].clone(),
            weight: w[i],
            vol_pct: vols_ann_pct[i],
            risk_contribution: rc[i],
        })
        .collect();
    Some(BasketAllocation {
        method: method.as_str().to_string(),
        weights,
        portfolio_vol_pct: port_vol_ann_pct,
        diversification_ratio: dr,
        risk_basis: if method.is_downside() { "semivariance" } else { "variance" }.to_string(),
        note,
        n_obs: n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build two synthetic return series with target daily vols and a target
    /// correlation, deterministically (no RNG): asset A = base sine, asset B =
    /// rho·A + sqrt(1-rho²)·orthogonal, each scaled to its vol.
    fn two_assets(vol_a: f64, vol_b: f64, rho: f64, n: usize) -> Vec<Vec<f64>> {
        let mut a = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        for t in 0..n {
            let x = (t as f64 * 0.7).sin();
            let y = (t as f64 * 0.39 + 1.3).sin(); // ~orthogonal to x over many samples
            a.push(vol_a * x);
            b.push(vol_b * (rho * x + (1.0 - rho * rho).sqrt() * y));
        }
        vec![a, b]
    }

    #[test]
    fn inverse_vol_is_proportional_to_one_over_sigma() {
        let w = inverse_vol_weights(&[0.1, 0.2, 0.4]);
        // 1/σ ∝ [10, 5, 2.5] → /17.5.
        assert!((w[0] - 10.0 / 17.5).abs() < 1e-12);
        assert!((w[1] - 5.0 / 17.5).abs() < 1e-12);
        assert!((w[2] - 2.5 / 17.5).abs() < 1e-12);
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn risk_parity_equalizes_risk_contributions() {
        // Correlated, unequal-vol pair → ERC weights must equalize the per-asset
        // risk contributions (the defining property), even though the weights
        // are NOT inverse-vol once correlation ≠ 0.
        let series = two_assets(0.01, 0.03, 0.4, 400);
        let cov = covariance_matrix(&series);
        let w = risk_parity_weights(&cov);
        let rc = risk_contributions(&cov, &w);
        assert!((rc[0] - rc[1]).abs() < 1e-6, "risk contributions not equal: {rc:?}");
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(w.iter().all(|x| *x > 0.0), "long-only weights expected");
    }

    #[test]
    fn risk_parity_equals_inverse_vol_for_two_assets_any_correlation() {
        // For k=2 the ERC condition RC₁=RC₂ reduces to w₁/w₂ = σ₂/σ₁ for ANY
        // correlation (the off-diagonal term cancels), so ERC == inverse-vol
        // exactly — not just in the uncorrelated case. Tight tolerance: the
        // k=2 solver reaches the inverse-vol seed's answer essentially exactly.
        for rho in [-0.5, 0.0, 0.4, 0.7] {
            let series = two_assets(0.01, 0.02, rho, 600);
            let cov = covariance_matrix(&series);
            let w_rp = risk_parity_weights(&cov);
            let vols: Vec<f64> = (0..2).map(|i| cov[i][i].sqrt()).collect();
            let w_iv = inverse_vol_weights(&vols);
            assert!((w_rp[0] - w_iv[0]).abs() < 1e-10, "rho={rho}: rp {w_rp:?} vs iv {w_iv:?}");
        }
    }

    #[test]
    fn diversification_ratio_at_least_one_and_higher_when_decorrelated() {
        let symbols = vec!["A".to_string(), "B".to_string()];
        let uncorr = allocate(&symbols, &two_assets(0.01, 0.02, 0.0, 600), Method::InverseVol).unwrap();
        let corr = allocate(&symbols, &two_assets(0.01, 0.02, 0.95, 600), Method::InverseVol).unwrap();
        assert!(uncorr.diversification_ratio >= 1.0 - 1e-9);
        // Less correlation → more diversification benefit.
        assert!(uncorr.diversification_ratio > corr.diversification_ratio);
    }

    #[test]
    fn allocate_guards_small_inputs() {
        let one = vec!["A".to_string()];
        assert!(allocate(&one, &[vec![0.0; 50]], Method::Equal).is_none()); // < 2 assets
        let two = vec!["A".to_string(), "B".to_string()];
        assert!(allocate(&two, &two_assets(0.01, 0.02, 0.0, 10), Method::Equal).is_none()); // < 20 obs
    }

    #[test]
    fn downside_risk_parity_equalizes_semivariance_contributions() {
        // Build a pair whose DOWNSIDE co-moves more than its full covariance:
        // asset B tracks A on down-days (shared crash factor) but is noisy on
        // up-days. Downside-RP must equalize the SEMIcovariance risk
        // contributions (RC on semicov ≈ 1/2), which differ from the symmetric
        // risk-parity weights.
        let symbols = vec!["A".to_string(), "B".to_string()];
        let n = 500;
        let mut a = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        for t in 0..n {
            let x = (t as f64 * 0.7).sin();
            let up_noise = (t as f64 * 0.41 + 0.9).sin();
            a.push(0.02 * x);
            // On down-moves B follows A; on up-moves B is its own noise.
            let bv = if x < 0.0 { 0.03 * x } else { 0.03 * up_noise };
            b.push(bv);
        }
        let series = vec![a, b];
        let drp = allocate(&symbols, &series, Method::DownsideRiskParity).unwrap();
        assert_eq!(drp.risk_basis, "semivariance");
        let rc: Vec<f64> = drp.weights.iter().map(|w| w.risk_contribution).collect();
        assert!((rc[0] - rc[1]).abs() < 1e-6, "semivariance RC not equal: {rc:?}");
        assert!((drp.weights.iter().map(|w| w.weight).sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(drp.weights.iter().all(|w| w.weight > 0.0));
        // The downside weights differ from the symmetric risk-parity weights
        // (the asymmetry is real, not a no-op).
        let rp = allocate(&symbols, &series, Method::RiskParity).unwrap();
        let dw0 = drp.weights[0].weight;
        let rw0 = rp.weights[0].weight;
        assert!((dw0 - rw0).abs() > 1e-3, "downside vs symmetric weights identical: {dw0} {rw0}");
        // portfolio_vol + diversification stay full-variance for both.
        assert_eq!(rp.risk_basis, "variance");
    }

    #[test]
    fn downside_rp_flags_and_zeroes_a_never_losing_asset() {
        // Asset B never has a down day (all returns ≥ 0) → zero semivariance.
        // Downside-RP must assign it 0% (ill-posed ERC) AND set the note, not
        // crash or emit NaN. Documents the known degenerate behavior (QA #973).
        let symbols = vec!["RISKY".to_string(), "NEVERLOSE".to_string()];
        let n = 200;
        let risky: Vec<f64> = (0..n).map(|t| 0.02 * (t as f64 * 0.7).sin()).collect();
        let neverlose: Vec<f64> = (0..n).map(|t| 0.01 * (t as f64 * 0.5).sin().abs()).collect();
        let series = vec![risky, neverlose];
        let a = allocate(&symbols, &series, Method::DownsideRiskParity).unwrap();
        assert!(a.note.is_some(), "expected a degeneracy note");
        assert!(a.note.as_ref().unwrap().contains("NEVERLOSE"));
        let nl = a.weights.iter().find(|w| w.symbol == "NEVERLOSE").unwrap();
        assert!(nl.weight < 1e-9, "never-losing asset should get ~0 weight, got {}", nl.weight);
        assert!(a.weights.iter().all(|w| w.weight.is_finite()));
        assert!((a.weights.iter().map(|w| w.weight).sum::<f64>() - 1.0).abs() < 1e-9);
        // A clean (all-assets-have-downside) basket sets no note.
        let clean = allocate(
            &symbols,
            &two_assets(0.01, 0.02, 0.3, 300),
            Method::DownsideRiskParity,
        )
        .unwrap();
        assert!(clean.note.is_none());
    }

    #[test]
    fn semicovariance_is_symmetric_and_downside_only() {
        // All-positive returns → no downside → semicovariance is all-zero.
        let series = vec![vec![0.01_f64; 50], vec![0.02_f64; 50]];
        let sc = semicovariance_matrix(&series);
        assert!(sc.iter().flatten().all(|x| *x == 0.0), "no losses → zero semicov");
        // A series with losses → symmetric, non-negative diagonal.
        let s2 = vec![
            (0..50).map(|t| if t % 2 == 0 { -0.02 } else { 0.01 }).collect::<Vec<f64>>(),
            (0..50).map(|t| if t % 2 == 0 { -0.03 } else { 0.02 }).collect::<Vec<f64>>(),
        ];
        let sc2 = semicovariance_matrix(&s2);
        assert!((sc2[0][1] - sc2[1][0]).abs() < 1e-15, "must be symmetric");
        assert!(sc2[0][0] > 0.0 && sc2[1][1] > 0.0, "downside semivariance positive");
    }

    #[test]
    fn equal_weight_is_uniform() {
        let symbols = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let series = vec![vec![0.01, -0.01].repeat(30), vec![0.02, -0.02].repeat(30), vec![0.015, -0.005].repeat(30)];
        let a = allocate(&symbols, &series, Method::Equal).unwrap();
        for aw in &a.weights {
            assert!((aw.weight - 1.0 / 3.0).abs() < 1e-12);
        }
    }
}
