//! RSI extreme highlighting (derived flag).
//!
//! `rsi_extreme_high` fires when ALL of:
//!  * current-TF RSI > 85
//!  * MTF RSI alignment is `aligned_overbought` (see `mtf_rsi` module)
//!  * current bar is a new 14-bar high
//!
//! Mirror for `rsi_extreme_low` (RSI < 15 AND aligned_oversold AND new 14-bar
//! low). Pure / no I/O.

use crate::indicators::extended::mtf_rsi::{compute_mtf_rsi, DEFAULT_RSI_PERIOD};
use serde::{Deserialize, Serialize};

pub const RSI_EXTREME_HIGH_THRESHOLD: f64 = 85.0;
pub const RSI_EXTREME_LOW_THRESHOLD: f64 = 15.0;
pub const HIGH_LOW_LOOKBACK: usize = 14;

/// Result of the RSI extreme flag computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsiExtremeResult {
    pub current_rsi: Option<f64>,
    pub aligned_overbought: bool,
    pub aligned_oversold: bool,
    pub new_14_bar_high: bool,
    pub new_14_bar_low: bool,
    pub rsi_extreme_high: bool,
    pub rsi_extreme_low: bool,
}

/// Compute the RSI extreme flag from a close series + matched highs / lows.
///
/// `highs` and `lows` must be the same length as `closes`. If they are
/// missing (length 0) the function falls back to using `closes` for both
/// (treats every bar as a single point).
pub fn compute_rsi_extreme(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    timeframe: &str,
    htf_bucket_sizes: &[usize],
) -> RsiExtremeResult {
    let n = closes.len();
    if n == 0 {
        return RsiExtremeResult {
            current_rsi: None,
            aligned_overbought: false,
            aligned_oversold: false,
            new_14_bar_high: false,
            new_14_bar_low: false,
            rsi_extreme_high: false,
            rsi_extreme_low: false,
        };
    }

    let highs_ref: Vec<f64> = if highs.len() == n {
        highs.to_vec()
    } else {
        closes.to_vec()
    };
    let lows_ref: Vec<f64> = if lows.len() == n {
        lows.to_vec()
    } else {
        closes.to_vec()
    };

    let mtf = compute_mtf_rsi(closes, timeframe, htf_bucket_sizes, DEFAULT_RSI_PERIOD);
    let current_rsi = mtf.current_rsi;

    let lookback_start = n.saturating_sub(HIGH_LOW_LOOKBACK);
    let recent_high = highs_ref[lookback_start..]
        .iter()
        .cloned()
        .fold(f64::MIN, f64::max);
    let recent_low = lows_ref[lookback_start..]
        .iter()
        .cloned()
        .fold(f64::MAX, f64::min);

    let new_14_bar_high = highs_ref[n - 1] >= recent_high;
    let new_14_bar_low = lows_ref[n - 1] <= recent_low;

    let rsi_extreme_high = mtf.aligned_overbought
        && new_14_bar_high
        && matches!(current_rsi, Some(rsi) if rsi > RSI_EXTREME_HIGH_THRESHOLD);
    let rsi_extreme_low = mtf.aligned_oversold
        && new_14_bar_low
        && matches!(current_rsi, Some(rsi) if rsi < RSI_EXTREME_LOW_THRESHOLD);

    RsiExtremeResult {
        current_rsi,
        aligned_overbought: mtf.aligned_overbought,
        aligned_oversold: mtf.aligned_oversold,
        new_14_bar_high,
        new_14_bar_low,
        rsi_extreme_high,
        rsi_extreme_low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_high_on_strongly_rising_series() {
        // Strongly rising series → RSI ≈ 100, aligned_overbought,
        // last bar is the 14-bar high. Use small custom buckets so a 400-bar
        // series produces alignment.
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + i as f64).collect();
        let highs = closes.iter().map(|c| c + 1.0).collect::<Vec<_>>();
        let lows = closes.iter().map(|c| c - 1.0).collect::<Vec<_>>();
        let r = compute_rsi_extreme(&highs, &lows, &closes, "1d", &[2, 4, 8, 16]);
        assert!(r.rsi_extreme_high, "should fire on persistent rally");
        assert!(!r.rsi_extreme_low);
    }

    #[test]
    fn extreme_low_on_strongly_falling_series() {
        let closes: Vec<f64> = (0..400).map(|i| 1000.0 - i as f64).collect();
        let highs = closes.iter().map(|c| c + 1.0).collect::<Vec<_>>();
        let lows = closes.iter().map(|c| c - 1.0).collect::<Vec<_>>();
        let r = compute_rsi_extreme(&highs, &lows, &closes, "1d", &[2, 4, 8, 16]);
        assert!(r.rsi_extreme_low, "should fire on persistent dump");
        assert!(!r.rsi_extreme_high);
    }

    #[test]
    fn no_extreme_when_alignment_missing() {
        // Choppy series: RSI never reaches >85 AND no MTF alignment.
        let closes: Vec<f64> = (0..400)
            .map(|i| 100.0 + (i as f64 * 0.5).sin() * 3.0)
            .collect();
        let r = compute_rsi_extreme(&[], &[], &closes, "1d", &[]);
        assert!(!r.rsi_extreme_high);
        assert!(!r.rsi_extreme_low);
    }

    #[test]
    fn empty_series_returns_safe_defaults() {
        let r = compute_rsi_extreme(&[], &[], &[], "1d", &[]);
        assert!(!r.rsi_extreme_high);
        assert!(!r.rsi_extreme_low);
        assert!(r.current_rsi.is_none());
    }
}
