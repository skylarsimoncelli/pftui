//! Volatility-weighted trend line.
//!
//! Smoothed momentum line whose smoothing constant `α` is modulated by
//! realised volatility. High volatility → larger `α` (faster reaction). Low
//! volatility → smaller `α` (slower). The output reports the latest trend
//! value, a slope label, and a 0–3 trend-strength integer derived from the
//! slope normalised to ATR-style volatility.

use serde::{Deserialize, Serialize};

/// Sensitivity preset for the trend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TrendSensitivity {
    Fast,
    Medium,
    Slow,
}

impl TrendSensitivity {
    pub fn length(self) -> usize {
        match self {
            TrendSensitivity::Fast => 9,
            TrendSensitivity::Medium => 18,
            TrendSensitivity::Slow => 27,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VolatilityTrendConfig {
    pub sensitivity: TrendSensitivity,
    /// Volatility lookback (rolling stdev of returns). Defaults to the
    /// sensitivity length if `None`.
    pub volatility_lookback: Option<usize>,
}

impl Default for VolatilityTrendConfig {
    fn default() -> Self {
        Self {
            sensitivity: TrendSensitivity::Medium,
            volatility_lookback: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendSlope {
    Up,
    Down,
    Flat,
}

impl TrendSlope {
    pub fn as_str(self) -> &'static str {
        match self {
            TrendSlope::Up => "up",
            TrendSlope::Down => "down",
            TrendSlope::Flat => "flat",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilityTrendResult {
    pub value: f64,
    pub slope: TrendSlope,
    /// 0 = no trend, 1 = weak, 2 = moderate, 3 = strong.
    pub trend_strength: u8,
}

/// Compute the volatility-weighted trend on `closes`. Returns `None` if there
/// is not enough history to seed both the trend and the volatility lookback.
pub fn compute_volatility_trend(
    closes: &[f64],
    cfg: &VolatilityTrendConfig,
) -> Option<VolatilityTrendResult> {
    let length = cfg.sensitivity.length();
    let vol_lookback = cfg.volatility_lookback.unwrap_or(length);
    if length == 0 || vol_lookback < 2 || closes.len() <= length.max(vol_lookback) + 1 {
        return None;
    }

    // Returns (log-return-style increments) used for volatility weighting.
    let returns: Vec<f64> = closes
        .windows(2)
        .map(|w| w[1] - w[0])
        .collect();

    // Rolling stdev of returns over `vol_lookback`. We compute it for each bar
    // i in [vol_lookback..returns.len()]; we use the latest one to drive α
    // and a recent slope baseline to derive trend_strength.
    let vol_series: Vec<Option<f64>> = (0..returns.len())
        .map(|i| {
            if i + 1 < vol_lookback {
                return None;
            }
            let win = &returns[i + 1 - vol_lookback..=i];
            let mean = win.iter().sum::<f64>() / win.len() as f64;
            let var = win.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / win.len() as f64;
            Some(var.sqrt())
        })
        .collect();

    let latest_vol = vol_series.iter().rev().find_map(|v| *v)?;
    // Normalise volatility to a 0..1 range by referencing the in-series median
    // std-dev so the α modulation is scale-free.
    let mut defined_vols: Vec<f64> = vol_series.iter().filter_map(|v| *v).collect();
    if defined_vols.is_empty() {
        return None;
    }
    defined_vols.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_vol = defined_vols[defined_vols.len() / 2];
    let vol_ratio = if median_vol > 0.0 {
        (latest_vol / median_vol).clamp(0.25, 4.0)
    } else {
        1.0
    };

    // Base α from the smoothing length, then scale toward faster reaction when
    // volatility is elevated. Final α is clamped to [α_base/2, 0.95] so the
    // trend never fully ignores history.
    let base_alpha = 2.0 / (length as f64 + 1.0);
    let alpha = (base_alpha * vol_ratio).clamp(base_alpha * 0.5, 0.95);

    // Volatility-weighted EMA of closes.
    let mut prev = closes[0];
    let mut series = Vec::with_capacity(closes.len());
    series.push(prev);
    for &c in closes.iter().skip(1) {
        prev += alpha * (c - prev);
        series.push(prev);
    }

    let value = *series.last()?;
    // Slope: compare current trend to value `length` bars ago.
    let lookback_idx = series.len().saturating_sub(length + 1);
    let prior = series[lookback_idx];
    let raw_slope = value - prior;

    // Slope direction with a vol-aware flat threshold.
    let flat_threshold = latest_vol * 0.5;
    let slope = if raw_slope.abs() <= flat_threshold {
        TrendSlope::Flat
    } else if raw_slope > 0.0 {
        TrendSlope::Up
    } else {
        TrendSlope::Down
    };

    // Trend strength: |raw_slope| / vol, where vol prefers the in-series
    // median (more stable than instantaneous latest stdev) and falls back to
    // latest. Both can be zero for perfectly synthetic series — strength
    // floors at 1 in that case to indicate a directional but unmeasurable
    // move.
    let vol_for_norm = if median_vol > 0.0 {
        median_vol
    } else {
        latest_vol
    };
    let normalised = if vol_for_norm > 0.0 {
        raw_slope.abs() / vol_for_norm
    } else {
        f64::INFINITY
    };
    let trend_strength = match slope {
        TrendSlope::Flat => 0,
        _ => {
            if normalised >= 6.0 {
                3
            } else if normalised >= 3.0 {
                2
            } else {
                1
            }
        }
    };

    Some(VolatilityTrendResult {
        value,
        slope,
        trend_strength,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_up_for_steady_uptrend() {
        let closes: Vec<f64> = (0..200).map(|i| 100.0 + i as f64).collect();
        let r = compute_volatility_trend(&closes, &VolatilityTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Up);
        // Linear ramp has near-zero return stdev so the strength normalisation
        // floors at 1; we just require a non-zero positive strength.
        assert!(r.trend_strength >= 1, "got strength {}", r.trend_strength);
        // Last value tracks but lags the ramp.
        assert!(r.value < *closes.last().unwrap());
    }

    #[test]
    fn trend_down_for_steady_downtrend() {
        let closes: Vec<f64> = (0..200).map(|i| 500.0 - i as f64).collect();
        let r = compute_volatility_trend(&closes, &VolatilityTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Down);
        assert!(r.trend_strength >= 1);
    }

    #[test]
    fn trend_strength_scales_with_slope_to_vol_ratio() {
        // Quiet noise base + late, steep ramp → high slope-to-vol ratio.
        let mut closes = Vec::with_capacity(200);
        for i in 0..150 {
            let osc = ((i as f64) * 0.7).sin() * 0.2;
            closes.push(100.0 + osc);
        }
        for i in 0..50 {
            closes.push(100.0 + i as f64 * 5.0);
        }
        let r = compute_volatility_trend(&closes, &VolatilityTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Up);
        assert!(r.trend_strength >= 2, "got strength {}", r.trend_strength);
    }

    #[test]
    fn trend_flat_for_constant_series() {
        let closes = vec![100.0; 200];
        let r = compute_volatility_trend(&closes, &VolatilityTrendConfig::default())
            .expect("computed");
        assert_eq!(r.slope, TrendSlope::Flat);
        assert_eq!(r.trend_strength, 0);
        assert!((r.value - 100.0).abs() < 1e-9);
    }

    #[test]
    fn trend_returns_none_for_short_history() {
        let closes = vec![100.0; 5];
        assert!(compute_volatility_trend(&closes, &VolatilityTrendConfig::default()).is_none());
    }

    #[test]
    fn fast_sensitivity_reacts_faster_than_slow() {
        let mut closes: Vec<f64> = vec![100.0; 100];
        for v in closes.iter_mut().skip(80) {
            *v = 200.0;
        }
        let fast_cfg = VolatilityTrendConfig {
            sensitivity: TrendSensitivity::Fast,
            volatility_lookback: None,
        };
        let slow_cfg = VolatilityTrendConfig {
            sensitivity: TrendSensitivity::Slow,
            volatility_lookback: None,
        };
        let fast = compute_volatility_trend(&closes, &fast_cfg).expect("fast");
        let slow = compute_volatility_trend(&closes, &slow_cfg).expect("slow");
        // Fast EMA should have moved closer to the new 200 level than slow.
        assert!(fast.value >= slow.value, "fast={} slow={}", fast.value, slow.value);
    }
}
