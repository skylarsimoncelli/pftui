//! Donchian channel midline trend.
//!
//! Two Donchian channels (conversion length and baseline length) — the midline
//! of each is the average of the rolling high and rolling low over the window.
//! The trend value is the mean of the two midlines; the slope is the sign of
//! the change vs `lookback` bars ago. Hybrid mode blends this with the
//! volatility-weighted trend using a configurable weight.

use serde::{Deserialize, Serialize};

use super::volatility_trend::{
    compute_volatility_trend, TrendSlope, VolatilityTrendConfig, VolatilityTrendResult,
};

#[derive(Debug, Clone, Copy)]
pub struct DonchianTrendConfig {
    /// Conversion-length Donchian window (default 5).
    pub conversion_length: usize,
    /// Baseline-length Donchian window (default 26).
    pub baseline_length: usize,
    /// Slope lookback in bars (default 5).
    pub slope_lookback: usize,
    /// Slope flat-threshold expressed as a fraction of the trend value
    /// (default 0.001 = 0.1%).
    pub flat_threshold_pct: f64,
}

impl Default for DonchianTrendConfig {
    fn default() -> Self {
        Self {
            conversion_length: 5,
            baseline_length: 26,
            slope_lookback: 5,
            flat_threshold_pct: 0.001,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DonchianTrendResult {
    pub value: f64,
    pub slope: TrendSlope,
}

/// Result of [`hybrid_trend_blend`]: weighted blend of volatility-weighted and
/// Donchian trends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridTrendResult {
    pub value: f64,
    pub slope: TrendSlope,
    pub volatility_weight: f64,
    pub donchian_weight: f64,
}

/// Compute the Donchian midline trend.
///
/// `highs` / `lows` mirror the layout of `closes`: same length, optional per
/// bar. Missing high/low at a bar falls back to that bar's close so the
/// channel is still well-defined for close-only history.
pub fn compute_donchian_trend(
    closes: &[f64],
    highs: &[Option<f64>],
    lows: &[Option<f64>],
    cfg: &DonchianTrendConfig,
) -> Option<DonchianTrendResult> {
    if cfg.conversion_length == 0 || cfg.baseline_length == 0 || cfg.slope_lookback == 0 {
        return None;
    }
    let needed = cfg.baseline_length.max(cfg.conversion_length) + cfg.slope_lookback;
    if closes.len() < needed {
        return None;
    }
    if highs.len() != closes.len() || lows.len() != closes.len() {
        return None;
    }

    let midline_at = |end: usize, length: usize| -> Option<f64> {
        if end + 1 < length {
            return None;
        }
        let start = end + 1 - length;
        let mut hi = f64::MIN;
        let mut lo = f64::MAX;
        for i in start..=end {
            let h = highs[i].unwrap_or(closes[i]);
            let l = lows[i].unwrap_or(closes[i]);
            if h > hi {
                hi = h;
            }
            if l < lo {
                lo = l;
            }
        }
        Some((hi + lo) / 2.0)
    };

    let last = closes.len() - 1;
    let prior = last - cfg.slope_lookback;

    let conv_now = midline_at(last, cfg.conversion_length)?;
    let base_now = midline_at(last, cfg.baseline_length)?;
    let conv_prior = midline_at(prior, cfg.conversion_length)?;
    let base_prior = midline_at(prior, cfg.baseline_length)?;

    let value = (conv_now + base_now) / 2.0;
    let prior_value = (conv_prior + base_prior) / 2.0;
    let delta = value - prior_value;

    let threshold = value.abs() * cfg.flat_threshold_pct;
    let slope = if delta.abs() <= threshold {
        TrendSlope::Flat
    } else if delta > 0.0 {
        TrendSlope::Up
    } else {
        TrendSlope::Down
    };

    Some(DonchianTrendResult { value, slope })
}

/// Blend the volatility-weighted and Donchian trends.
///
/// `volatility_weight` is clamped to [0, 1]; Donchian weight is `1 - w`.
/// Returns `None` if either underlying computation fails.
pub fn hybrid_trend_blend(
    closes: &[f64],
    highs: &[Option<f64>],
    lows: &[Option<f64>],
    vol_cfg: &VolatilityTrendConfig,
    donchian_cfg: &DonchianTrendConfig,
    volatility_weight: f64,
) -> Option<HybridTrendResult> {
    let w_vol = volatility_weight.clamp(0.0, 1.0);
    let w_don = 1.0 - w_vol;
    let vol: VolatilityTrendResult = compute_volatility_trend(closes, vol_cfg)?;
    let don: DonchianTrendResult = compute_donchian_trend(closes, highs, lows, donchian_cfg)?;
    let value = w_vol * vol.value + w_don * don.value;

    // Slope precedence: if both agree (and not both flat), that's the slope.
    // If they disagree, the heavier weight wins. If weights are equal and they
    // disagree, the result is flat.
    let slope = if vol.slope == don.slope || w_vol > w_don {
        vol.slope
    } else if w_don > w_vol {
        don.slope
    } else {
        TrendSlope::Flat
    };

    Some(HybridTrendResult {
        value,
        slope,
        volatility_weight: w_vol,
        donchian_weight: w_don,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicators::extended::volatility_trend::{
        TrendSensitivity, VolatilityTrendConfig,
    };

    fn fake_ohlc(closes: &[f64]) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
        let highs: Vec<Option<f64>> = closes.iter().map(|c| Some(c + 1.0)).collect();
        let lows: Vec<Option<f64>> = closes.iter().map(|c| Some(c - 1.0)).collect();
        (highs, lows)
    }

    #[test]
    fn donchian_uptrend_slope_up() {
        let closes: Vec<f64> = (0..100).map(|i| 50.0 + i as f64).collect();
        let (highs, lows) = fake_ohlc(&closes);
        let r = compute_donchian_trend(&closes, &highs, &lows, &DonchianTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Up);
        // value sits between the conversion and baseline midlines.
        assert!(r.value > 50.0 && r.value < 150.0);
    }

    #[test]
    fn donchian_downtrend_slope_down() {
        let closes: Vec<f64> = (0..100).map(|i| 500.0 - i as f64).collect();
        let (highs, lows) = fake_ohlc(&closes);
        let r = compute_donchian_trend(&closes, &highs, &lows, &DonchianTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Down);
    }

    #[test]
    fn donchian_flat_for_constant_series() {
        let closes = vec![100.0; 60];
        let (highs, lows) = fake_ohlc(&closes);
        let r = compute_donchian_trend(&closes, &highs, &lows, &DonchianTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Flat);
        // Conversion midline ≈ baseline midline ≈ 100.
        assert!((r.value - 100.0).abs() < 1e-6);
    }

    #[test]
    fn donchian_known_midline_for_window_of_5() {
        // Closes that make the last conversion-window high/low predictable.
        let mut closes = vec![100.0; 60];
        // Bars [55..60): 110, 105, 120, 95, 108. high=120 low=95 mid=107.5.
        closes[55] = 110.0;
        closes[56] = 105.0;
        closes[57] = 120.0;
        closes[58] = 95.0;
        closes[59] = 108.0;
        let highs: Vec<Option<f64>> = closes.iter().map(|c| Some(*c)).collect();
        let lows: Vec<Option<f64>> = closes.iter().map(|c| Some(*c)).collect();
        let cfg = DonchianTrendConfig {
            conversion_length: 5,
            baseline_length: 5, // so value = mid_conv = mid_base
            slope_lookback: 5,
            flat_threshold_pct: 0.001,
        };
        let r = compute_donchian_trend(&closes, &highs, &lows, &cfg).expect("computed");
        // Both midlines equal 107.5; mean = 107.5.
        assert!((r.value - 107.5).abs() < 1e-9, "got {}", r.value);
    }

    #[test]
    fn donchian_missing_history_returns_none() {
        let closes = vec![100.0; 10];
        let (highs, lows) = fake_ohlc(&closes);
        assert!(compute_donchian_trend(
            &closes,
            &highs,
            &lows,
            &DonchianTrendConfig::default()
        )
        .is_none());
    }

    #[test]
    fn hybrid_blend_weighted_value() {
        let closes: Vec<f64> = (0..200).map(|i| 100.0 + i as f64 * 0.5).collect();
        let (highs, lows) = fake_ohlc(&closes);
        let vol_cfg = VolatilityTrendConfig {
            sensitivity: TrendSensitivity::Medium,
            volatility_lookback: None,
        };
        let don_cfg = DonchianTrendConfig::default();
        let hybrid =
            hybrid_trend_blend(&closes, &highs, &lows, &vol_cfg, &don_cfg, 0.5).expect("computed");
        assert!((hybrid.volatility_weight - 0.5).abs() < 1e-9);
        assert!((hybrid.donchian_weight - 0.5).abs() < 1e-9);
        // Both components are positive trends → slope up.
        assert_eq!(hybrid.slope, TrendSlope::Up);
    }
}
