//! Gaussian-filtered channel with σ-bands.
//!
//! Chain: DEMA → Gaussian-weighted filter → SMMA. Bands are derived from a
//! rolling standard deviation of the smoothed line and asymmetric upper/lower
//! multipliers. The latest bar is classified into a `band_state` enum.
//!
//! All math runs on `&[f64]` closes — `f64` is appropriate here because the
//! outputs are indicator floats (not money / quantities).

use serde::{Deserialize, Serialize};

/// Configuration for [`compute_gaussian_channel`].
#[derive(Debug, Clone, Copy)]
pub struct GaussianChannelConfig {
    /// DEMA smoothing length (default 7).
    pub dema_length: usize,
    /// Gaussian filter window length (default 4).
    pub gaussian_length: usize,
    /// Gaussian σ (default 2.0).
    pub gaussian_sigma: f64,
    /// SMMA (Wilder smoothing) length (default 12).
    pub smma_length: usize,
    /// Standard-deviation lookback length (default 30).
    pub sd_length: usize,
    /// Upper σ multiplier (default 2.5).
    pub upper_sd_mult: f64,
    /// Lower σ multiplier (default 1.8).
    pub lower_sd_mult: f64,
}

impl Default for GaussianChannelConfig {
    fn default() -> Self {
        Self {
            dema_length: 7,
            gaussian_length: 4,
            gaussian_sigma: 2.0,
            smma_length: 12,
            sd_length: 30,
            upper_sd_mult: 2.5,
            lower_sd_mult: 1.8,
        }
    }
}

/// Classification of the latest close relative to the upper/lower band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GaussianChannelState {
    AboveUpper,
    InBand,
    BelowLower,
}

impl GaussianChannelState {
    pub fn as_str(self) -> &'static str {
        match self {
            GaussianChannelState::AboveUpper => "above_upper",
            GaussianChannelState::InBand => "in_band",
            GaussianChannelState::BelowLower => "below_lower",
        }
    }
}

/// Result of the Gaussian channel computation for the latest bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianChannelResult {
    pub middle: f64,
    pub upper: f64,
    pub lower: f64,
    pub band_state: GaussianChannelState,
}

/// Compute the Gaussian channel (middle + upper/lower σ-bands + state) on the
/// latest bar of `closes`.
///
/// Returns `None` when there is not enough history to populate the full chain.
pub fn compute_gaussian_channel(
    closes: &[f64],
    cfg: &GaussianChannelConfig,
) -> Option<GaussianChannelResult> {
    if cfg.dema_length == 0
        || cfg.gaussian_length == 0
        || cfg.smma_length == 0
        || cfg.sd_length < 2
    {
        return None;
    }

    let dema_series = dema(closes, cfg.dema_length);
    let gauss_series = gaussian_filter(&dema_series, cfg.gaussian_length, cfg.gaussian_sigma);
    let smma_series = smma(&gauss_series, cfg.smma_length);

    let smma_defined: Vec<f64> = smma_series.iter().filter_map(|v| *v).collect();
    if smma_defined.len() < cfg.sd_length {
        return None;
    }

    let middle = *smma_defined.last()?;

    // Rolling SD over the last `sd_length` defined SMMA values.
    let window = &smma_defined[smma_defined.len() - cfg.sd_length..];
    let mean = window.iter().sum::<f64>() / window.len() as f64;
    let variance =
        window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / window.len() as f64;
    let sd = variance.sqrt();

    let upper = middle + cfg.upper_sd_mult * sd;
    let lower = middle - cfg.lower_sd_mult * sd;

    let latest_close = *closes.last()?;
    let band_state = if latest_close > upper {
        GaussianChannelState::AboveUpper
    } else if latest_close < lower {
        GaussianChannelState::BelowLower
    } else {
        GaussianChannelState::InBand
    };

    Some(GaussianChannelResult {
        middle,
        upper,
        lower,
        band_state,
    })
}

/// Exponential moving average. Seeded with the first value, then recursively
/// smoothed. Returns `None` until the index reaches `period - 1`.
fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut out: Vec<Option<f64>> = Vec::with_capacity(values.len());
    let mut prev: Option<f64> = None;
    for (i, &v) in values.iter().enumerate() {
        let next = match prev {
            Some(p) => p + alpha * (v - p),
            None => v,
        };
        prev = Some(next);
        if i + 1 >= period {
            out.push(Some(next));
        } else {
            out.push(None);
        }
    }
    out
}

/// Double-EMA: 2·EMA − EMA(EMA). Returns one value per input bar; entries
/// before both EMAs are populated are filled by repeating the first defined
/// DEMA (so downstream filters always have a contiguous f64 input).
fn dema(values: &[f64], period: usize) -> Vec<f64> {
    let ema1 = ema(values, period);
    let ema1_filled: Vec<f64> = ema1
        .iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let ema2 = ema(&ema1_filled, period);

    let mut out = Vec::with_capacity(values.len());
    let mut first_defined: Option<f64> = None;
    for i in 0..values.len() {
        match (ema1[i], ema2[i]) {
            (Some(e1), Some(e2)) => {
                let d = 2.0 * e1 - e2;
                if first_defined.is_none() {
                    first_defined = Some(d);
                }
                out.push(d);
            }
            _ => out.push(0.0), // placeholder; replaced after the loop
        }
    }
    if let Some(seed) = first_defined {
        for v in out.iter_mut() {
            if *v == 0.0 {
                *v = seed;
            }
        }
    } else {
        // Fall back to the raw inputs when DEMA cannot be evaluated.
        return values.to_vec();
    }
    out
}

/// Gaussian-weighted moving average.
///
/// Weights for offset i (0..length) are `exp(-((i - mu)^2) / (2 σ^2))`, with
/// `mu = (length - 1) / 2`. Trailing window — applied at each bar `t`, the
/// window spans indices `t - length + 1 ..= t` of `values`.
fn gaussian_filter(values: &[f64], length: usize, sigma: f64) -> Vec<f64> {
    if length == 0 || values.is_empty() {
        return values.to_vec();
    }
    let sigma = sigma.max(1e-9);
    let mu = (length as f64 - 1.0) / 2.0;
    let weights: Vec<f64> = (0..length)
        .map(|i| {
            let x = i as f64 - mu;
            (-(x * x) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let weight_sum: f64 = weights.iter().sum();

    let mut out = Vec::with_capacity(values.len());
    for t in 0..values.len() {
        let window_start = t.saturating_sub(length - 1);
        let window = &values[window_start..=t];
        // Align the weights with the available window so early bars still get a
        // sensible weighted mean (using the rightmost `window.len()` weights).
        let w_slice = &weights[length - window.len()..];
        let w_sum: f64 = w_slice.iter().sum();
        let denom = if w_sum > 0.0 { w_sum } else { weight_sum };
        let numer: f64 = window.iter().zip(w_slice.iter()).map(|(v, w)| v * w).sum();
        out.push(numer / denom);
    }
    out
}

/// Wilder-style smoothed moving average. Seeded with a simple mean of the
/// first `period` values, then recursively smoothed with weight `1 / period`.
fn smma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }
    let mut out: Vec<Option<f64>> = Vec::with_capacity(values.len());
    let mut prev: Option<f64> = None;
    for (i, &v) in values.iter().enumerate() {
        if i + 1 < period {
            out.push(None);
            continue;
        }
        if i + 1 == period {
            let seed = values[..period].iter().sum::<f64>() / period as f64;
            prev = Some(seed);
            out.push(Some(seed));
            continue;
        }
        let p = prev.unwrap_or(v);
        let next = (p * (period as f64 - 1.0) + v) / period as f64;
        prev = Some(next);
        out.push(Some(next));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_closes(n: usize) -> Vec<f64> {
        // Smooth sine wave around 100 to exercise the chain — sufficient
        // signal to populate all stages and produce a well-defined band state.
        (0..n)
            .map(|i| 100.0 + 10.0 * (i as f64 / 8.0).sin())
            .collect()
    }

    #[test]
    fn ema_matches_hand_calc_for_period_3() {
        // alpha = 2/4 = 0.5; seed = 10.
        // bar1: 10
        // bar2: 10 + 0.5*(20-10) = 15
        // bar3: 15 + 0.5*(30-15) = 22.5 -> first defined
        let series = ema(&[10.0, 20.0, 30.0, 40.0], 3);
        assert!(series[0].is_none());
        assert!(series[1].is_none());
        let v2 = series[2].expect("ema bar3 defined");
        let v3 = series[3].expect("ema bar4 defined");
        assert!((v2 - 22.5).abs() < 1e-9, "got {v2}");
        // bar4: 22.5 + 0.5*(40-22.5) = 31.25
        assert!((v3 - 31.25).abs() < 1e-9, "got {v3}");
    }

    #[test]
    fn smma_matches_hand_calc_for_period_3() {
        // seed at index 2 = mean(10,20,30) = 20
        // bar4: (20*2 + 40)/3 = 80/3
        let series = smma(&[10.0, 20.0, 30.0, 40.0], 3);
        assert!(series[0].is_none());
        assert!(series[1].is_none());
        assert!((series[2].expect("seed") - 20.0).abs() < 1e-9);
        let expected = (20.0 * 2.0 + 40.0) / 3.0;
        let got = series[3].expect("bar4 defined");
        assert!((got - expected).abs() < 1e-9, "got {got}");
    }

    #[test]
    fn gaussian_filter_constant_input_returns_constant() {
        let v = vec![5.0; 50];
        let out = gaussian_filter(&v, 4, 2.0);
        for x in out {
            assert!((x - 5.0).abs() < 1e-9);
        }
    }

    #[test]
    fn gaussian_channel_produces_band_state_for_synthetic_series() {
        let closes = synthetic_closes(80);
        let result = compute_gaussian_channel(&closes, &GaussianChannelConfig::default())
            .expect("channel should compute");
        assert!(result.upper > result.middle);
        assert!(result.lower < result.middle);
        // Synthetic sine — last close should sit somewhere within or just
        // outside the band; whatever state is returned must be one of the
        // three legal variants.
        let s = result.band_state.as_str();
        assert!(matches!(s, "above_upper" | "in_band" | "below_lower"));
    }

    #[test]
    fn gaussian_channel_classifies_above_upper_on_step_up() {
        // Build a flat series, then a big spike at the end to force
        // band_state = above_upper.
        let mut closes = vec![100.0; 80];
        *closes.last_mut().unwrap() = 1_000.0;
        let result = compute_gaussian_channel(&closes, &GaussianChannelConfig::default())
            .expect("channel should compute");
        // Flat history gives sd ≈ 0; the spike pushes close >> upper.
        // But sd from flat → 0 means upper == middle; close > middle ⇒ above_upper.
        assert_eq!(result.band_state, GaussianChannelState::AboveUpper);
    }

    #[test]
    fn gaussian_channel_returns_none_for_short_history() {
        let closes = vec![100.0; 10];
        assert!(compute_gaussian_channel(&closes, &GaussianChannelConfig::default()).is_none());
    }
}
