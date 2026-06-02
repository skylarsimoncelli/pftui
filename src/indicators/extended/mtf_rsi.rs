//! Multi-timeframe RSI alignment.
//!
//! Computes RSI on the current timeframe and four higher timeframes derived by
//! aggregating the current-TF close series into N-bar buckets (last-close of
//! each bucket = synthetic HTF close). Reports whether all four higher
//! timeframes plus the current TF are aligned overbought (>70) or aligned
//! oversold (<30).
//!
//! The bucket sizes are picked per `default_htf_periods_for(timeframe)` —
//! e.g. `5min` → [3, 6, 12, 48] (which approximates 15m / 30m / 1h / 4h
//! windows). The caller can pass their own bucket sizes for non-default
//! timeframes via `compute_mtf_rsi`.
//!
//! Canonical TA terminology only.

use crate::indicators::rsi::compute_rsi;
use serde::{Deserialize, Serialize};

/// Aligned-overbought RSI threshold (per spec).
pub const OVERBOUGHT_THRESHOLD: f64 = 70.0;
/// Aligned-oversold RSI threshold (per spec).
pub const OVERSOLD_THRESHOLD: f64 = 30.0;
/// Default RSI lookback period.
pub const DEFAULT_RSI_PERIOD: usize = 14;

/// Result of an MTF RSI computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtfRsiResult {
    /// RSI on the input timeframe (last bar).
    pub current_rsi: Option<f64>,
    /// HTF bucket sizes (in current-TF bars) used to derive each HTF series.
    pub htf_bucket_sizes: Vec<usize>,
    /// RSI values on each derived HTF (latest bucket), aligned with
    /// `htf_bucket_sizes`. `None` if the HTF series had insufficient bars.
    pub htf_rsi_values: Vec<Option<f64>>,
    /// True iff `current_rsi > 70` AND every HTF RSI > 70.
    pub aligned_overbought: bool,
    /// True iff `current_rsi < 30` AND every HTF RSI < 30.
    pub aligned_oversold: bool,
}

/// Reasonable HTF bucket-size defaults for common timeframes.
///
/// Each tuple element is the number of CURRENT-TF bars to aggregate into one
/// HTF bar. Twelve `5min` bars ≈ one hour; 288 ≈ one day, etc.
pub fn default_htf_periods_for(timeframe: &str) -> Vec<usize> {
    match timeframe.to_lowercase().as_str() {
        // intraday
        "1min" | "1m" => vec![5, 15, 60, 240],
        "5min" | "5m" => vec![3, 6, 12, 48],
        "15min" | "15m" => vec![2, 4, 16, 96],
        "30min" | "30m" => vec![2, 8, 48, 240],
        "60min" | "1h" | "60m" => vec![4, 24, 120, 480],
        "4h" | "240min" | "4hr" => vec![6, 30, 130, 365],
        // daily and above
        "1d" | "daily" | "d" => vec![5, 21, 63, 252],   // ≈ week / month / quarter / year
        "1w" | "weekly" | "w" => vec![4, 13, 26, 52],
        "1m_cal" | "monthly" => vec![3, 12, 24, 60],
        _ => vec![5, 21, 63, 252],
    }
}

/// Compute multi-timeframe RSI alignment.
///
/// `closes` is the current-timeframe close series (oldest → newest).
/// `htf_bucket_sizes` lists how many CURRENT-TF bars each HTF aggregates.
/// Pass `&[]` to use `default_htf_periods_for(timeframe)`.
pub fn compute_mtf_rsi(
    closes: &[f64],
    timeframe: &str,
    htf_bucket_sizes: &[usize],
    rsi_period: usize,
) -> MtfRsiResult {
    let buckets: Vec<usize> = if htf_bucket_sizes.is_empty() {
        default_htf_periods_for(timeframe)
    } else {
        htf_bucket_sizes.to_vec()
    };

    let current_rsi = compute_rsi(closes, rsi_period)
        .iter()
        .rev()
        .find_map(|v| *v);

    let htf_rsi_values: Vec<Option<f64>> = buckets
        .iter()
        .map(|&bucket| {
            if bucket == 0 {
                return None;
            }
            let htf_closes = aggregate_closes(closes, bucket);
            if htf_closes.len() < rsi_period + 1 {
                return None;
            }
            compute_rsi(&htf_closes, rsi_period)
                .iter()
                .rev()
                .find_map(|v| *v)
        })
        .collect();

    let all_htf_ob = !htf_rsi_values.is_empty()
        && htf_rsi_values
            .iter()
            .all(|v| matches!(v, Some(x) if *x > OVERBOUGHT_THRESHOLD));
    let all_htf_os = !htf_rsi_values.is_empty()
        && htf_rsi_values
            .iter()
            .all(|v| matches!(v, Some(x) if *x < OVERSOLD_THRESHOLD));

    let aligned_overbought = all_htf_ob
        && matches!(current_rsi, Some(x) if x > OVERBOUGHT_THRESHOLD);
    let aligned_oversold = all_htf_os
        && matches!(current_rsi, Some(x) if x < OVERSOLD_THRESHOLD);

    MtfRsiResult {
        current_rsi,
        htf_bucket_sizes: buckets,
        htf_rsi_values,
        aligned_overbought,
        aligned_oversold,
    }
}

/// Aggregate a close series into N-bar buckets, taking the last close of each
/// completed bucket. Incomplete trailing buckets are dropped.
fn aggregate_closes(closes: &[f64], bucket: usize) -> Vec<f64> {
    if bucket <= 1 {
        return closes.to_vec();
    }
    let mut out = Vec::with_capacity(closes.len() / bucket);
    let mut i = bucket;
    while i <= closes.len() {
        out.push(closes[i - 1]);
        i += bucket;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_overbought_when_all_series_strongly_rising() {
        // Strongly rising series → RSI ≈ 100 on every bucket. Need enough
        // bars for the largest default bucket (252) × (period+1)=15 → ≥ 3780.
        let closes: Vec<f64> = (0..4000).map(|i| 100.0 + i as f64).collect();
        let result = compute_mtf_rsi(&closes, "1d", &[], DEFAULT_RSI_PERIOD);
        assert!(result.current_rsi.unwrap() > 90.0);
        assert!(result.aligned_overbought, "should be aligned overbought");
        assert!(!result.aligned_oversold);
    }

    #[test]
    fn aligned_oversold_when_all_series_strongly_falling() {
        let closes: Vec<f64> = (0..4000).map(|i| 10_000.0 - i as f64).collect();
        let result = compute_mtf_rsi(&closes, "1d", &[], DEFAULT_RSI_PERIOD);
        assert!(result.aligned_oversold, "should be aligned oversold");
        assert!(!result.aligned_overbought);
    }

    #[test]
    fn neither_alignment_when_choppy() {
        let closes: Vec<f64> = (0..4000)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 5.0)
            .collect();
        let result = compute_mtf_rsi(&closes, "1d", &[], DEFAULT_RSI_PERIOD);
        assert!(!result.aligned_overbought);
        assert!(!result.aligned_oversold);
    }

    #[test]
    fn returns_none_when_insufficient_data() {
        let closes: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let result = compute_mtf_rsi(&closes, "1d", &[], DEFAULT_RSI_PERIOD);
        assert!(result.current_rsi.is_none());
        // Even though defaults are populated, HTF RSI must be None when
        // insufficient.
        assert!(result.htf_rsi_values.iter().all(|v| v.is_none()));
    }

    #[test]
    fn custom_htf_buckets_take_precedence() {
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + i as f64).collect();
        let result = compute_mtf_rsi(&closes, "1d", &[2, 4, 8, 16], DEFAULT_RSI_PERIOD);
        assert_eq!(result.htf_bucket_sizes, vec![2, 4, 8, 16]);
        assert_eq!(result.htf_rsi_values.len(), 4);
    }

    #[test]
    fn alignment_known_value_with_custom_buckets() {
        // Use small bucket sizes so a 400-bar rising series produces an
        // aligned-overbought outcome with deterministic RSI ≈ 100 on the
        // current bar.
        let closes: Vec<f64> = (0..400).map(|i| 100.0 + i as f64).collect();
        let result = compute_mtf_rsi(&closes, "1d", &[2, 4, 8, 16], DEFAULT_RSI_PERIOD);
        assert!(result.aligned_overbought);
        assert!(result.current_rsi.unwrap() > 95.0);
    }

    #[test]
    fn default_periods_known_timeframes() {
        // Per-spec mapping for 5min and 1h cases.
        assert_eq!(default_htf_periods_for("5min").len(), 4);
        assert_eq!(default_htf_periods_for("1h").len(), 4);
        assert_eq!(default_htf_periods_for("1d").len(), 4);
        // unknown timeframe falls back to daily mapping
        assert_eq!(default_htf_periods_for("bogus"), default_htf_periods_for("1d"));
    }

    #[test]
    fn aggregate_closes_buckets_correctly() {
        let closes = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        // bucket=3 → last of [1,2,3], last of [4,5,6] → [3,6], [7] is dropped
        assert_eq!(aggregate_closes(&closes, 3), vec![3.0, 6.0]);
        // bucket=1 → identity
        assert_eq!(aggregate_closes(&closes, 1), closes);
    }
}
