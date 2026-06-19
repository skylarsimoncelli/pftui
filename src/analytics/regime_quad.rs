//! Growth × Inflation regime quadrant — the legible, glass-box regime spine
//! (`docs/ENVIRONMENT-ENGINE.md` §3.3), in the lineage of the Hedgeye GIP /
//! 42 Macro Quad models.
//!
//! Each day is classified by the **rate of change** (acceleration) of a growth
//! proxy and an inflation proxy into one of four regimes. This is a price-proxy
//! v1 (the historical macro-print series needed for a true growth/inflation
//! nowcast are not yet ingested as time series): growth = equity 63-day
//! momentum, inflation = commodity (gold+oil) 63-day momentum; the regime is
//! the sign of each proxy's change over the last ~quarter. Transparent and
//! explainable by construction — no opaque score.
//!
//! All values `f64`.

use serde::Serialize;

/// Lookback for the momentum proxy (~one quarter of trading days).
const MOM_WINDOW: usize = 63;
/// Lookback for the rate-of-change of momentum (~one month).
const ROC_WINDOW: usize = 21;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Quad {
    /// Growth accelerating, inflation decelerating — risk-on, growth/momentum.
    Goldilocks,
    /// Growth accelerating, inflation accelerating — reflation, commodities.
    Reflation,
    /// Growth decelerating, inflation accelerating — stagflation, defensives + hard assets.
    Inflation,
    /// Growth decelerating, inflation decelerating — deflation, duration + USD.
    Deflation,
    /// Not enough history to classify.
    Unknown,
}

impl Quad {
    pub fn label(&self) -> &'static str {
        match self {
            Quad::Goldilocks => "Goldilocks (growth↑ inflation↓)",
            Quad::Reflation => "Reflation (growth↑ inflation↑)",
            Quad::Inflation => "Inflation/Stagflation (growth↓ inflation↑)",
            Quad::Deflation => "Deflation (growth↓ inflation↓)",
            Quad::Unknown => "unknown",
        }
    }
    pub fn short(&self) -> &'static str {
        match self {
            Quad::Goldilocks => "goldilocks",
            Quad::Reflation => "reflation",
            Quad::Inflation => "inflation",
            Quad::Deflation => "deflation",
            Quad::Unknown => "unknown",
        }
    }
    pub fn from_short(s: &str) -> Quad {
        match s {
            "goldilocks" => Quad::Goldilocks,
            "reflation" => Quad::Reflation,
            "inflation" => Quad::Inflation,
            "deflation" => Quad::Deflation,
            _ => Quad::Unknown,
        }
    }
}

fn log_ret(v: &[f64], i: usize, w: usize) -> Option<f64> {
    if i < w || v[i - w] <= 0.0 || v[i] <= 0.0 {
        return None;
    }
    Some((v[i] / v[i - w]).ln())
}

/// Classify the quad at index `i` given aligned growth (equity) and the two
/// inflation-proxy series (gold, oil), all on the same master axis.
pub fn classify(equity: &[f64], gold: &[f64], oil: &[f64], i: usize) -> Quad {
    let need = MOM_WINDOW + ROC_WINDOW;
    if i < need {
        return Quad::Unknown;
    }
    // Growth proxy momentum + its change over the last ~month.
    let (Some(g_now), Some(g_prev)) = (log_ret(equity, i, MOM_WINDOW), log_ret(equity, i - ROC_WINDOW, MOM_WINDOW))
    else {
        return Quad::Unknown;
    };
    // Inflation proxy = average of gold + oil momentum.
    let infl = |k: usize| match (log_ret(gold, k, MOM_WINDOW), log_ret(oil, k, MOM_WINDOW)) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        _ => None,
    };
    let (Some(i_now), Some(i_prev)) = (infl(i), infl(i - ROC_WINDOW)) else {
        return Quad::Unknown;
    };
    let growth_accel = g_now - g_prev >= 0.0;
    let infl_accel = i_now - i_prev >= 0.0;
    match (growth_accel, infl_accel) {
        (true, false) => Quad::Goldilocks,
        (true, true) => Quad::Reflation,
        (false, true) => Quad::Inflation,
        (false, false) => Quad::Deflation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Flat until `knee`, then ramps up at `step`/bar — a recent UP-acceleration.
    fn flat_then_ramp(n: usize, knee: usize, step: f64) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + if i < knee { 0.0 } else { step * (i - knee) as f64 })
            .collect()
    }
    // Ramps up until `knee`, then flat — a recent DOWN-deceleration.
    fn ramp_then_flat(n: usize, knee: usize, step: f64) -> Vec<f64> {
        (0..n)
            .map(|i| 100.0 + step * i.min(knee) as f64)
            .collect()
    }

    #[test]
    fn unknown_without_history() {
        let s = flat_then_ramp(50, 10, 1.0);
        assert_eq!(classify(&s, &s, &s, 40), Quad::Unknown);
    }

    #[test]
    fn accelerating_growth_decelerating_inflation_is_goldilocks() {
        let n = 220;
        let equity = flat_then_ramp(n, 150, 2.0); // growth accelerating recently
        let commod = ramp_then_flat(n, 150, 2.0); // inflation decelerating recently
        let q = classify(&equity, &commod, &commod, n - 1);
        assert_eq!(q, Quad::Goldilocks, "got {:?}", q);
    }

    #[test]
    fn accelerating_both_is_reflation() {
        let n = 220;
        let equity = flat_then_ramp(n, 150, 2.0);
        let commod = flat_then_ramp(n, 150, 2.0);
        assert_eq!(classify(&equity, &commod, &commod, n - 1), Quad::Reflation);
    }
}
