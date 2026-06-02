//! Pi cycle top/bottom crossover signal.
//!
//! Daily-close-only. Two crossovers:
//!  * **Top**: `(350-period SMA × 2)` crossing UNDER the `111-period SMA`.
//!  * **Bottom**: `(471-period SMA × 0.745)` crossing OVER the `150-period EMA`.
//!
//! Output: for each signal, the index of the most recent crossover bar (within
//! the close series), the bars-since-cross, and the optional ISO date if the
//! caller passes a date slice aligned to closes.
//!
//! The parameters were calibrated on BTC daily but the function is asset-
//! agnostic. Canonical TA terminology only.

use crate::indicators::sma::compute_sma;
use serde::{Deserialize, Serialize};

pub const TOP_SHORT_PERIOD: usize = 111;
pub const TOP_LONG_PERIOD: usize = 350;
pub const TOP_LONG_MULTIPLIER: f64 = 2.0;

pub const BOTTOM_SHORT_PERIOD: usize = 150;
pub const BOTTOM_LONG_PERIOD: usize = 471;
pub const BOTTOM_LONG_MULTIPLIER: f64 = 0.745;

/// A single crossover marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossoverMarker {
    /// Bar index in the input close series where the crossover occurred.
    pub bar_index: usize,
    /// Bars elapsed since the crossover (0 = crossed on the latest bar).
    pub bars_since: usize,
    /// ISO date string for the crossover bar, if the caller supplied dates.
    pub date: Option<String>,
}

/// Pi cycle top + bottom signal result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiCycleResult {
    /// Most recent top crossover (350SMA×2 crossing UNDER 111SMA). `None` if
    /// no crossover ever observed in the series.
    pub top: Option<CrossoverMarker>,
    /// Most recent bottom crossover (471SMA×0.745 crossing OVER 150EMA).
    pub bottom: Option<CrossoverMarker>,
    /// Whether the close series had enough bars to evaluate either signal.
    pub has_sufficient_data: bool,
}

/// Compute the pi cycle top/bottom signals.
///
/// `closes` is the (daily) close series in chronological order.
/// `dates` is an optional same-length slice of ISO-8601 dates aligned to
/// `closes`; pass `&[]` to omit dates from the output.
pub fn compute_pi_cycle(closes: &[f64], dates: &[String]) -> PiCycleResult {
    let n = closes.len();
    if n < BOTTOM_LONG_PERIOD + 2 {
        return PiCycleResult {
            top: None,
            bottom: None,
            has_sufficient_data: false,
        };
    }

    // -- TOP: SMA350 * 2 crossing under SMA111
    let sma_111 = compute_sma(closes, TOP_SHORT_PERIOD);
    let sma_350 = compute_sma(closes, TOP_LONG_PERIOD);
    let top = find_latest_crossover(
        n,
        |i| sma_350.get(i).copied().flatten().map(|v| v * TOP_LONG_MULTIPLIER),
        |i| sma_111.get(i).copied().flatten(),
        CrossDir::Down, // long*mult crossing UNDER short → "down"
        dates,
    );

    // -- BOTTOM: SMA471 * 0.745 crossing over EMA150
    let sma_471 = compute_sma(closes, BOTTOM_LONG_PERIOD);
    let ema_150 = compute_ema(closes, BOTTOM_SHORT_PERIOD);
    let bottom = find_latest_crossover(
        n,
        |i| sma_471.get(i).copied().flatten().map(|v| v * BOTTOM_LONG_MULTIPLIER),
        |i| ema_150.get(i).copied().flatten(),
        CrossDir::Up, // long*mult crossing OVER ema → "up"
        dates,
    );

    PiCycleResult {
        top,
        bottom,
        has_sufficient_data: true,
    }
}

/// Wilder-style EMA (smoothing = 2 / (period + 1)). Standard EMA.
fn compute_ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }
    let mut out = vec![None; values.len()];
    if values.len() < period {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let seed: f64 = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(seed);
    let mut prev = seed;
    for (i, &v) in values.iter().enumerate().skip(period) {
        let ema = alpha * v + (1.0 - alpha) * prev;
        out[i] = Some(ema);
        prev = ema;
    }
    out
}

#[derive(Copy, Clone)]
enum CrossDir {
    Up,
    Down,
}

/// Walks bar indices newest-first; returns the most recent bar where the two
/// series crossed in the requested direction.
fn find_latest_crossover<F, G>(
    n: usize,
    a: F,
    b: G,
    dir: CrossDir,
    dates: &[String],
) -> Option<CrossoverMarker>
where
    F: Fn(usize) -> Option<f64>,
    G: Fn(usize) -> Option<f64>,
{
    for i in (1..n).rev() {
        let (Some(a_now), Some(b_now), Some(a_prev), Some(b_prev)) =
            (a(i), b(i), a(i - 1), b(i - 1))
        else {
            continue;
        };
        let crossed = match dir {
            // a crossing UNDER b: prev a >= prev b, now a < b
            CrossDir::Down => a_prev >= b_prev && a_now < b_now,
            // a crossing OVER b: prev a <= prev b, now a > b
            CrossDir::Up => a_prev <= b_prev && a_now > b_now,
        };
        if crossed {
            return Some(CrossoverMarker {
                bar_index: i,
                bars_since: n.saturating_sub(1).saturating_sub(i),
                date: dates.get(i).cloned(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_top_crossover_series() -> Vec<f64> {
        // We need (350-SMA × 2) to CROSS UNDER 111-SMA: prev (a≥b), now (a<b),
        // where a = SMA350 × 2 and b = SMA111. Equivalently: SMA111 catches up
        // to double the SMA350. Strategy: hold flat at $100 for a long time
        // (pinning SMA350 ≈ $100), then run a steady aggressive rally so SMA111
        // climbs through $200.
        // 500 bars at $100 to keep SMA350 anchored near 100
        let mut closes: Vec<f64> = vec![100.0; 500];
        // 200 bars of fast linear rally — SMA111 will quickly surpass $200
        for i in 1..=200 {
            closes.push(100.0 + (i as f64) * 5.0);
        }
        closes
    }


    #[test]
    fn pi_cycle_top_crossover_detected_in_synthetic_series() {
        let closes = synthetic_top_crossover_series();
        let result = compute_pi_cycle(&closes, &[]);
        assert!(result.has_sufficient_data);
        let top = result
            .top
            .expect("synthetic series should produce a top crossover");
        assert!(top.bar_index >= 400, "crossover should be in the post-ramp region");
        assert!(top.bars_since < closes.len(), "bars_since within range");
    }

    #[test]
    fn pi_cycle_returns_no_crossovers_for_short_series() {
        let closes: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        let result = compute_pi_cycle(&closes, &[]);
        assert!(!result.has_sufficient_data);
        assert!(result.top.is_none());
        assert!(result.bottom.is_none());
    }

    #[test]
    fn pi_cycle_dates_propagate_when_provided() {
        let closes = synthetic_top_crossover_series();
        let dates: Vec<String> = (0..closes.len())
            .map(|i| format!("2024-{:02}-{:02}", (i / 28) % 12 + 1, (i % 28) + 1))
            .collect();
        let result = compute_pi_cycle(&closes, &dates);
        let top = result.top.expect("should find crossover");
        assert!(top.date.is_some(), "date must propagate when supplied");
    }

    #[test]
    fn pi_cycle_no_top_for_pure_ramp() {
        // A pure linear ramp never produces a SMA350*2 < SMA111 crossing.
        let closes: Vec<f64> = (0..600).map(|i| 100.0 + (i as f64) * 0.5).collect();
        let result = compute_pi_cycle(&closes, &[]);
        assert!(result.has_sufficient_data);
        assert!(result.top.is_none(), "linear ramp must not trip top crossover");
    }

    #[test]
    fn ema_matches_first_value_at_seed() {
        let v = vec![10.0; 30];
        let ema = compute_ema(&v, 5);
        assert!((ema[4].unwrap() - 10.0).abs() < 1e-12);
        // Flat → all EMAs identical.
        for val in ema.iter().skip(4) {
            assert!((val.unwrap() - 10.0).abs() < 1e-12);
        }
    }
}
