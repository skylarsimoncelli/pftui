//! Fractional-Kelly position sizing from a strategy's realized per-trade edge,
//! with two honesty discounts the naive Kelly formula omits:
//!
//! 1. **Estimation-uncertainty haircut** — full Kelly trusts the point estimate
//!    of the edge completely. A sample of `n` trades estimates the mean edge `μ`
//!    only to within a standard error `σ/√n`; we re-run Kelly on the *lower*
//!    one-SE bound of the edge so a noisy or thin track record sizes smaller.
//! 2. **Drawdown-budget cap (via CDaR)** — Kelly maximizes log-growth and is
//!    famously over-levered for real risk tolerance; leverage `L` scales the
//!    drawdown roughly linearly, so we cap `L` so that `L · CDaR-95 ≤` the
//!    operator's drawdown budget. This is the whole reason CDaR exists: it is
//!    the coherent denominator a growth-optimal sizer should respect.
//!
//! The recommended size is **half-Kelly on the uncertainty-adjusted edge,
//! capped by the CDaR budget** — the conservative-by-default composition.
//!
//! Kelly assumes i.i.d. bets; per-trade returns are the bet sequence here. The
//! leverage is a multiple on the per-trade return (comparable to the backtest's
//! `--vol-target` leverage), NOT a fraction of capital. All pure `f64` math.

use serde::Serialize;

/// Default drawdown budget (percent) the CDaR cap targets when the caller does
/// not specify one. 25% is a conservative strategy-overlay risk budget.
pub const DEFAULT_DRAWDOWN_BUDGET_PCT: f64 = 25.0;

/// Minimum trades before a Kelly estimate is trustworthy (matches the
/// drawdown-metrics / Monte-Carlo gate — Kelly on a thin sample is noise).
const MIN_TRADES: usize = 20;

/// Growth-optimal leverage guidance derived from a strategy's per-trade returns.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KellySizing {
    /// Mean per-trade return (the raw edge), percent.
    pub edge_per_trade_pct: f64,
    /// One-standard-error lower bound on the edge (`μ − σ/√n`), percent — the
    /// conservative edge the uncertainty-adjusted Kelly is computed on.
    pub edge_lower_1se_pct: f64,
    /// Full continuous Kelly leverage `μ/σ²` on the point-estimate edge,
    /// floored at 0 (a negative-edge `μ/σ²` would mean "go short" — out of
    /// scope for this long-only sizer; see `binding_constraint = no-edge`).
    pub full_kelly_leverage: f64,
    /// Half-Kelly on the point-estimate edge (the classic robustness fraction),
    /// likewise floored at 0.
    pub half_kelly_leverage: f64,
    /// Full Kelly on the uncertainty-adjusted (lower-1SE) edge — 0 if that edge
    /// is non-positive (a track record indistinguishable from no edge).
    pub uncertainty_adjusted_leverage: f64,
    /// Drawdown budget (percent) the CDaR cap targets.
    pub drawdown_budget_pct: f64,
    /// `drawdown_budget / CDaR-95` — the max leverage keeping expected worst-5%
    /// drawdown within budget. `None` when CDaR-95 is unavailable or ~0.
    pub cdar_cap_leverage: Option<f64>,
    /// The recommended size: `min(0.5 · uncertainty_adjusted, cdar_cap)`,
    /// floored at 0. Half-Kelly on the conservative edge, capped by drawdown.
    pub recommended_leverage: f64,
    /// Which constraint bound the recommendation: `edge`, `drawdown-cap`, or
    /// `no-edge` (recommended 0).
    pub binding_constraint: String,
}

/// Compute Kelly sizing guidance from per-trade returns (as fractions, e.g.
/// 0.02 = +2%) and the strategy's CDaR-95 (a fraction, e.g. 0.30 = 30% tail
/// drawdown). `drawdown_budget_pct` is the leverage cap's target (percent).
/// Returns `None` below [`MIN_TRADES`] or when the returns have no dispersion.
pub fn compute(
    trade_returns: &[f64],
    cdar_95: Option<f64>,
    drawdown_budget_pct: f64,
) -> Option<KellySizing> {
    let n = trade_returns.len();
    if n < MIN_TRADES {
        return None;
    }
    let mean = trade_returns.iter().sum::<f64>() / n as f64;
    // Sample variance (n−1) → both the Kelly denominator and the SE of the mean.
    let var = trade_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    // Reject a (near-)constant series: a real trade-return variance is ~1e-3
    // (≈3% std); anything below 1e-12 is FP noise off an effectively constant
    // record, where μ/σ² explodes into a meaningless leverage. `> 0.0` is not
    // enough — summing a constant accumulates a tiny positive residual variance.
    if var <= 1e-12 {
        return None;
    }
    let std = var.sqrt();
    let se = std / (n as f64).sqrt();
    let edge_lcb = mean - se;

    let full_kelly = mean / var;
    let half_kelly = 0.5 * full_kelly;
    // Uncertainty-adjusted: full Kelly on the conservative edge, floored at 0.
    let unc_adj = (edge_lcb.max(0.0)) / var;

    // CDaR drawdown-budget cap: L such that L·CDaR-95 ≤ budget.
    let budget = drawdown_budget_pct / 100.0;
    let cdar_cap = cdar_95.filter(|c| *c > 1e-9).map(|c| budget / c);

    // Recommendation: half of the uncertainty-adjusted Kelly, capped by CDaR.
    let half_unc = 0.5 * unc_adj;
    let (recommended, binding) = if unc_adj <= 0.0 {
        (0.0, "no-edge")
    } else if let Some(cap) = cdar_cap {
        // `<=` so an exact tie labels the cap as binding (it is, equally).
        if cap <= half_unc {
            (cap.max(0.0), "drawdown-cap")
        } else {
            (half_unc, "edge")
        }
    } else {
        (half_unc, "edge")
    };

    Some(KellySizing {
        edge_per_trade_pct: mean * 100.0,
        edge_lower_1se_pct: edge_lcb * 100.0,
        // Floor the headline Kelly leverages at 0: a negative-edge strategy has
        // a negative μ/σ² (= "go short"), but this is a long-only sizing tool,
        // so every reported leverage is a non-negative LONG multiple. The
        // negative edge is already signalled by `binding_constraint = no-edge`.
        full_kelly_leverage: full_kelly.max(0.0),
        half_kelly_leverage: half_kelly.max(0.0),
        uncertainty_adjusted_leverage: unc_adj,
        drawdown_budget_pct,
        cdar_cap_leverage: cdar_cap,
        recommended_leverage: recommended,
        binding_constraint: binding.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 20-trade record: ten +3% wins, ten −1% losses. μ=1%, and the variance
    /// is exact, so the leverages are hand-checkable.
    fn synthetic_edge() -> Vec<f64> {
        let mut v = vec![0.03; 10];
        v.extend(vec![-0.01; 10]);
        v
    }

    #[test]
    fn full_kelly_matches_hand_computation() {
        let rets = synthetic_edge();
        let k = compute(&rets, None, DEFAULT_DRAWDOWN_BUDGET_PCT).unwrap();
        // μ=0.01; Σ(r−μ)² = 20·(0.02)² = 0.008; var = 0.008/19; μ/var = 0.19/0.008.
        let expected_full = 0.01 / (0.008 / 19.0);
        assert!((k.full_kelly_leverage - expected_full).abs() < 1e-9, "{}", k.full_kelly_leverage);
        assert!((k.full_kelly_leverage - 23.75).abs() < 1e-9);
        assert!((k.half_kelly_leverage - 0.5 * expected_full).abs() < 1e-9);
        assert!((k.edge_per_trade_pct - 1.0).abs() < 1e-9);
    }

    #[test]
    fn uncertainty_haircut_shrinks_below_full() {
        let rets = synthetic_edge();
        let k = compute(&rets, None, DEFAULT_DRAWDOWN_BUDGET_PCT).unwrap();
        // The lower-1SE edge is below the raw edge, so the adjusted Kelly is
        // strictly below full Kelly (but still positive for this clean edge).
        assert!(k.edge_lower_1se_pct < k.edge_per_trade_pct);
        assert!(k.uncertainty_adjusted_leverage > 0.0);
        assert!(k.uncertainty_adjusted_leverage < k.full_kelly_leverage);
        // No CDaR passed → edge-bound recommendation = half the adjusted Kelly.
        assert_eq!(k.binding_constraint, "edge");
        assert!((k.recommended_leverage - 0.5 * k.uncertainty_adjusted_leverage).abs() < 1e-12);
    }

    #[test]
    fn cdar_cap_binds_when_drawdowns_are_severe() {
        let rets = synthetic_edge();
        // A brutal 80% worst-5% drawdown with a 25% budget caps leverage at
        // 0.25/0.80 = 0.3125×, well below the edge-implied half-Kelly (~6×).
        let k = compute(&rets, Some(0.80), 25.0).unwrap();
        assert_eq!(k.binding_constraint, "drawdown-cap");
        assert!((k.recommended_leverage - 0.25 / 0.80).abs() < 1e-12);
        assert!((k.cdar_cap_leverage.unwrap() - 0.3125).abs() < 1e-12);
    }

    #[test]
    fn no_edge_recommends_zero() {
        // Symmetric returns with a slightly NEGATIVE mean → no edge.
        let mut rets = vec![0.02; 10];
        rets.extend(vec![-0.03; 10]); // μ = −0.5%
        let k = compute(&rets, Some(0.30), 25.0).unwrap();
        assert!(k.edge_per_trade_pct < 0.0);
        assert_eq!(k.uncertainty_adjusted_leverage, 0.0);
        assert_eq!(k.binding_constraint, "no-edge");
        assert_eq!(k.recommended_leverage, 0.0);
        // Every reported leverage is a non-negative LONG multiple, even though
        // the raw μ/σ² is negative for a losing edge (QA #971: no negative
        // leverage should leak to JSON consumers).
        assert_eq!(k.full_kelly_leverage, 0.0);
        assert_eq!(k.half_kelly_leverage, 0.0);
    }

    #[test]
    fn guards_thin_and_degenerate_samples() {
        assert!(compute(&vec![0.01; 19], None, 25.0).is_none()); // < 20 trades
        assert!(compute(&vec![0.01; 30], None, 25.0).is_none()); // zero variance
    }
}
