//! Cross-module input-space CONTRACT tests for the analytics suite.
//!
//! Each numerical module assumes a specific input space — EVT/copula want
//! SIMPLE daily returns, Hurst/survival want LOG returns, drawdown-metrics wants
//! a PRICE/equity curve. Every module unit-tests its own internal math, but
//! nothing guards those input contracts at the boundary: a future caller could
//! pass log returns to EVT or a return series to drawdown-metrics and every
//! existing test would still pass. QA has already caught two bugs of exactly
//! this class (the survival log-vs-arithmetic confusion and the CDaR `n%20`
//! FP off-by-one), so these tripwires + invariants exist to catch the next one.
//!
//! `#[cfg(test)]`-only; synthetic data, no DB, no network.

use super::{drawdown_metrics, evt, hurst_rs, kelly, survival};

/// Deterministic price path: geometric drift `g`/bar with a sine wobble of
/// fractional amplitude `amp`, starting at 100. No RNG.
fn price_path(g: f64, amp: f64, n: usize) -> Vec<f64> {
    let mut p = 100.0;
    let mut out = Vec::with_capacity(n);
    for t in 0..n {
        p *= 1.0 + g + amp * (t as f64 * 0.6).sin();
        out.push(p);
    }
    out
}

fn simple_returns(prices: &[f64]) -> Vec<f64> {
    prices.windows(2).map(|w| w[1] / w[0] - 1.0).collect()
}
fn log_returns(prices: &[f64]) -> Vec<f64> {
    prices.windows(2).map(|w| (w[1] / w[0]).ln()).collect()
}

// ─────────────────────────── EVT (simple returns) ───────────────────────────

#[test]
fn evt_var_ordering_holds_under_extreme_inputs() {
    // Contract: fit_evt_tail_risk takes SIMPLE daily returns. Even with a few
    // BTC-scale shocks (±25%/day), the VaR quantiles must stay monotone:
    // 99.9% ≥ 99% ≥ 95%, and ξ must be finite. A broken GPD fit or a space
    // mix-up tends to break this ordering.
    let mut rets = simple_returns(&price_path(0.0005, 0.03, 600));
    rets[100] = -0.25;
    rets[300] = -0.30;
    rets[450] = 0.22;
    let e = evt::fit_evt_tail_risk(&rets, 0.95).expect("EVT should fit on 600 returns");
    assert!(e.xi.is_finite(), "ξ must be finite, got {}", e.xi);
    assert!(
        e.var_999_pct >= e.var_99_pct && e.var_99_pct >= e.var_95_pct,
        "VaR must be monotone: 99.9%={} 99%={} 95%={}",
        e.var_999_pct,
        e.var_99_pct,
        e.var_95_pct
    );
    assert!(e.es_99_pct >= e.var_99_pct, "ES99 must be ≥ VaR99");
}

#[test]
fn evt_is_sensitive_to_return_space_on_fat_tails() {
    // Tripwire: feeding LOG returns where SIMPLE is expected is NOT a no-op on a
    // fat-tailed series — the two produce a different ξ. This documents that the
    // space matters (so a caller passing the wrong one is a real bug, not noise).
    let prices = price_path(0.0005, 0.04, 800);
    let simple = simple_returns(&prices);
    let logr = log_returns(&prices);
    let xi_simple = evt::fit_evt_tail_risk(&simple, 0.95).unwrap().xi;
    let xi_log = evt::fit_evt_tail_risk(&logr, 0.95).unwrap().xi;
    assert!(
        (xi_simple - xi_log).abs() > 1e-6,
        "log vs simple should differ on fat tails (caller must pass SIMPLE): {xi_simple} vs {xi_log}"
    );
}

// ─────────────────────────── Hurst (log returns) ────────────────────────────

#[test]
fn hurst_in_unit_range_and_dfa_guards_thin_series() {
    let h = hurst_rs::hurst(&log_returns(&price_path(0.0008, 0.02, 700))).expect("hurst fits");
    assert!((0.0..=1.0).contains(&h.h), "H must be in [0,1], got {}", h.h);
    // DFA needs ≥64 points — a thin series returns None cleanly, never panics.
    assert!(hurst_rs::dfa_alpha(&[0.01; 40]).is_none(), "thin DFA must be None");
    assert!(hurst_rs::dfa_alpha(&log_returns(&price_path(0.0008, 0.02, 300))).is_some());
}

// ─────────────────────── drawdown_metrics (PRICE curve) ──────────────────────

#[test]
fn drawdown_metrics_takes_prices_not_returns() {
    // Contract tripwire: compute() wants a PRICE/equity curve. Feeding a return
    // series (small values straddling 0) is a contract violation that must NOT
    // silently look like a valid result: a return series has tiny "drawdowns"
    // off its running max and produces a wildly different CDaR than the real
    // price path it came from. Both must stay finite (no NaN/inf).
    let prices = price_path(0.0006, 0.03, 600);
    let on_prices = drawdown_metrics::compute(&prices, None, 0.0).expect("prices → metrics");
    let on_returns = drawdown_metrics::compute(&simple_returns(&prices), None, 0.0);
    assert!(on_prices.cdar_95.is_finite() && on_prices.cdar_95 > 0.0);
    if let Some(r) = on_returns {
        assert!(r.cdar_95.is_finite(), "no NaN even on misuse");
        assert!(
            (r.cdar_95 - on_prices.cdar_95).abs() > 0.01,
            "returns-as-prices must NOT coincidentally match the price-curve CDaR"
        );
    }
    // Monotonicity invariant: CDaR-95 ≥ CDaR-90 always.
    assert!(on_prices.cdar_95 >= on_prices.cdar_90 - 1e-12);
}

#[test]
fn cdar_k_count_is_fp_robust_for_n_multiple_of_20() {
    // Cross-module regression for the merged IEEE-754 fix. CDaR operates on the
    // EQUITY-CURVE length, so use EXACTLY 20 points to land on the `(1−0.95)·20`
    // boundary: the worst-5% tail must hold k=1 (the single deepest drawdown),
    // not k=2. Nineteen strictly-rising points (0 drawdown each) + one
    // catastrophic final drop ⇒ exactly one nonzero drawdown.
    let mut prices: Vec<f64> = (0..19).map(|i| 100.0 + i as f64).collect();
    prices.push(60.0);
    assert_eq!(prices.len(), 20);
    let m = drawdown_metrics::compute(&prices, None, 0.0).expect("metrics");
    // Correct (k=1): CDaR-95 = the single worst drawdown. CDaR-90 (k=2) averages
    // it with a zero ⇒ ≈ half. The buggy k=2 would make CDaR-95 == CDaR-90, so a
    // ~2× ratio is the tripwire that fails if the off-by-one ever regresses.
    assert!(
        m.cdar_95 > 1.9 * m.cdar_90,
        "n=20: CDaR-95 (k=1) must be ~2× CDaR-90 (k=2), got {} vs {}",
        m.cdar_95,
        m.cdar_90
    );
}

// ─────────────────────────── survival (log returns) ──────────────────────────

#[test]
fn survival_flags_non_positive_drift_through_the_contract() {
    // The QA #974 class of bug: a negative-drift window must yield reliable=false
    // with model figures None and certain ruin — never a misleading finite TuW.
    let down = log_returns(&price_path(-0.001, 0.02, 400));
    let s = survival::compute(&down, Some(0.4), 25.0, 0.95).expect("survival computes");
    assert!(!s.reliable, "μ≤0 must be flagged unreliable");
    assert!(s.max_dd_iid.is_none() && s.max_tuw_iid_days.is_none());
    assert_eq!(s.ruin_prob, 1.0);

    // Positive drift → reliable, with an ARITHMETIC max-DD in [0,1) (QA fix: the
    // reported depth is 1−e^−x, comparable to CDaR — not a raw log drop, which
    // could exceed 1).
    let up = log_returns(&price_path(0.0012, 0.02, 400));
    let s2 = survival::compute(&up, Some(0.3), 25.0, 0.95).expect("survival");
    assert!(s2.reliable);
    let dd = s2.max_dd_iid.expect("reliable → Some");
    assert!((0.0..1.0).contains(&dd), "arithmetic max-DD must be in [0,1), got {dd}");
}

// ─────────────────────────── kelly + basket guards ───────────────────────────

#[test]
fn kelly_floors_at_zero_for_a_losing_edge() {
    // A net-negative per-trade record must never report a long leverage.
    let mut rets = vec![0.02_f64; 10];
    rets.extend(vec![-0.04; 10]); // μ < 0
    let k = kelly::compute(&rets, Some(0.3), 25.0).expect("≥20 trades");
    assert_eq!(k.full_kelly_leverage, 0.0);
    assert_eq!(k.recommended_leverage, 0.0);
    assert_eq!(k.binding_constraint, "no-edge");
}

#[test]
fn basket_single_asset_returns_none_not_panic() {
    use super::basket::{allocate, Method};
    let one = vec!["A".to_string()];
    let series = vec![simple_returns(&price_path(0.0005, 0.02, 100))];
    assert!(allocate(&one, &series, Method::RiskParity).is_none());
    assert!(allocate(&one, &series, Method::DownsideRiskParity).is_none());
}
