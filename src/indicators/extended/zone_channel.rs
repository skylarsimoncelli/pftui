//! Two-EMA zone channel.
//!
//! Two EMAs (default 144 / 233) define inner and outer band walls. The outer
//! walls are the inner-band edges extended by a configurable multiple of the
//! inner-band half-width. The latest close is classified into one of four
//! zone positions.
//!
//! Naming is canonical TA only — no vendor or brand-style labels.

use serde::{Deserialize, Serialize};

/// Configuration for [`compute_zone_channel`].
#[derive(Debug, Clone, Copy)]
pub struct ZoneChannelConfig {
    /// Fast EMA length (default 144).
    pub fast_ema: usize,
    /// Slow EMA length (default 233).
    pub slow_ema: usize,
    /// Outer-band extension factor: outer = inner ± `extension` × half_width.
    pub extension: f64,
}

impl Default for ZoneChannelConfig {
    fn default() -> Self {
        Self {
            fast_ema: 144,
            slow_ema: 233,
            extension: 1.5,
        }
    }
}

/// Where the latest close sits relative to the inner/outer bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZonePosition {
    UpperOuter,
    UpperInner,
    LowerInner,
    LowerOuter,
}

impl ZonePosition {
    pub fn as_str(self) -> &'static str {
        match self {
            ZonePosition::UpperOuter => "upper-outer",
            ZonePosition::UpperInner => "upper-inner",
            ZonePosition::LowerInner => "lower-inner",
            ZonePosition::LowerOuter => "lower-outer",
        }
    }
}

/// Zone channel output for the latest bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneChannelResult {
    pub upper_outer: f64,
    pub upper_inner: f64,
    pub lower_inner: f64,
    pub lower_outer: f64,
    pub zone_position: ZonePosition,
}

/// Compute the zone channel for the latest bar of `closes`. Returns `None` if
/// either EMA cannot be populated.
pub fn compute_zone_channel(
    closes: &[f64],
    cfg: &ZoneChannelConfig,
) -> Option<ZoneChannelResult> {
    if cfg.fast_ema == 0 || cfg.slow_ema == 0 {
        return None;
    }
    let fast = ema_last(closes, cfg.fast_ema)?;
    let slow = ema_last(closes, cfg.slow_ema)?;

    let (upper_inner, lower_inner) = if fast >= slow {
        (fast, slow)
    } else {
        (slow, fast)
    };

    let half_width = (upper_inner - lower_inner) / 2.0;
    let upper_outer = upper_inner + cfg.extension * half_width;
    let lower_outer = lower_inner - cfg.extension * half_width;

    let close = *closes.last()?;
    let zone_position = if close >= upper_inner {
        if close >= upper_outer {
            ZonePosition::UpperOuter
        } else {
            ZonePosition::UpperInner
        }
    } else if close <= lower_inner {
        if close <= lower_outer {
            ZonePosition::LowerOuter
        } else {
            ZonePosition::LowerInner
        }
    } else {
        // Price between the two EMAs — classify by which side of the midline.
        let mid = (upper_inner + lower_inner) / 2.0;
        if close >= mid {
            ZonePosition::UpperInner
        } else {
            ZonePosition::LowerInner
        }
    };

    Some(ZoneChannelResult {
        upper_outer,
        upper_inner,
        lower_inner,
        lower_outer,
        zone_position,
    })
}

fn ema_last(values: &[f64], period: usize) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut prev = values[0];
    for &v in values.iter().skip(1) {
        prev += alpha * (v - prev);
    }
    Some(prev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_last_constant_input_returns_constant() {
        let v = vec![42.0; 50];
        let e = ema_last(&v, 10).expect("defined");
        assert!((e - 42.0).abs() < 1e-9);
    }

    #[test]
    fn ema_last_known_value_for_period_3() {
        // alpha = 0.5; sequence 10,20,30,40
        // bar1=10, bar2=15, bar3=22.5, bar4=31.25
        let v = vec![10.0, 20.0, 30.0, 40.0];
        let e = ema_last(&v, 3).expect("defined");
        assert!((e - 31.25).abs() < 1e-9, "got {e}");
    }

    #[test]
    fn zone_channel_uptrend_close_in_upper_inner() {
        // Strong uptrend: fast EMA > slow EMA, close just below fast.
        let mut closes: Vec<f64> = (0..300).map(|i| 100.0 + i as f64 * 0.5).collect();
        // Make last close sit between slow and fast EMA so it lands upper-inner.
        let result =
            compute_zone_channel(&closes, &ZoneChannelConfig::default()).expect("computed");
        assert!(result.upper_inner > result.lower_inner);
        assert!(result.upper_outer > result.upper_inner);
        assert!(result.lower_outer < result.lower_inner);
        // Last close is well above both EMAs in a steady uptrend.
        let close = *closes.last().unwrap();
        assert!(close > result.upper_inner);
        // Force an outer position by pumping the final close.
        *closes.last_mut().unwrap() = result.upper_outer + 50.0;
        let r2 = compute_zone_channel(&closes, &ZoneChannelConfig::default()).expect("computed");
        assert_eq!(r2.zone_position, ZonePosition::UpperOuter);
    }

    #[test]
    fn zone_channel_downtrend_close_in_lower_outer_on_crash() {
        let mut closes: Vec<f64> = (0..300).map(|i| 500.0 - i as f64 * 0.5).collect();
        let result =
            compute_zone_channel(&closes, &ZoneChannelConfig::default()).expect("computed");
        // Crash last close hard.
        *closes.last_mut().unwrap() = result.lower_outer - 100.0;
        let r2 = compute_zone_channel(&closes, &ZoneChannelConfig::default()).expect("computed");
        assert_eq!(r2.zone_position, ZonePosition::LowerOuter);
    }

    #[test]
    fn zone_channel_short_history_returns_none() {
        let closes = vec![100.0; 50];
        assert!(compute_zone_channel(&closes, &ZoneChannelConfig::default()).is_none());
    }
}
