//! Momentum indicators — Stochastic, Williams %R, CCI, ROC.
//!
//! Pure functions over price slices (`&[f64]`); no I/O. Highs/lows/closes are
//! given as `&[f64]` (callers substitute close where OHLC is unavailable).
//! Each returns `Vec<Option<f64>>` (or a small struct) the same length as the
//! input, with `None` during the warmup.

/// Stochastic Oscillator result: fast %K and its %D smoothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StochResult {
    pub k: f64,
    pub d: f64,
}

/// Stochastic Oscillator. %K = 100·(close − lowest_low) / (highest_high −
/// lowest_low) over `k_period`; %D = SMA of %K over `d_period`. Values 0–100.
pub fn compute_stochastic(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    k_period: usize,
    d_period: usize,
) -> Vec<Option<StochResult>> {
    let n = closes.len();
    let mut out = vec![None; n];
    if k_period == 0 || d_period == 0 || n < k_period {
        return out;
    }
    // raw %K series first.
    let mut k_series = vec![None; n];
    for i in (k_period - 1)..n {
        let hh = highs[i + 1 - k_period..=i].iter().cloned().fold(f64::MIN, f64::max);
        let ll = lows[i + 1 - k_period..=i].iter().cloned().fold(f64::MAX, f64::min);
        let range = hh - ll;
        let k = if range > 0.0 {
            100.0 * (closes[i] - ll) / range
        } else {
            50.0
        };
        k_series[i] = Some(k);
    }
    // %D = SMA of %K.
    for i in 0..n {
        if i + 1 < k_period - 1 + d_period {
            continue;
        }
        let window: Vec<f64> = (i + 1 - d_period..=i).filter_map(|j| k_series[j]).collect();
        if window.len() == d_period {
            let d = window.iter().sum::<f64>() / d_period as f64;
            if let Some(k) = k_series[i] {
                out[i] = Some(StochResult { k, d });
            }
        }
    }
    out
}

/// Williams %R = −100·(highest_high − close) / (highest_high − lowest_low) over
/// `period`. Range −100..0 (−20 overbought, −80 oversold).
pub fn compute_williams_r(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = closes.len();
    let mut out = vec![None; n];
    if period == 0 || n < period {
        return out;
    }
    for i in (period - 1)..n {
        let hh = highs[i + 1 - period..=i].iter().cloned().fold(f64::MIN, f64::max);
        let ll = lows[i + 1 - period..=i].iter().cloned().fold(f64::MAX, f64::min);
        let range = hh - ll;
        out[i] = Some(if range > 0.0 {
            -100.0 * (hh - closes[i]) / range
        } else {
            -50.0
        });
    }
    out
}

/// Commodity Channel Index = (typical − SMA(typical)) / (0.015 · mean_deviation)
/// over `period`, where typical = (high + low + close) / 3.
pub fn compute_cci(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = closes.len();
    let mut out = vec![None; n];
    if period == 0 || n < period {
        return out;
    }
    let tp: Vec<f64> = (0..n).map(|i| (highs[i] + lows[i] + closes[i]) / 3.0).collect();
    for i in (period - 1)..n {
        let window = &tp[i + 1 - period..=i];
        let sma = window.iter().sum::<f64>() / period as f64;
        let mean_dev = window.iter().map(|v| (v - sma).abs()).sum::<f64>() / period as f64;
        out[i] = Some(if mean_dev > 0.0 {
            (tp[i] - sma) / (0.015 * mean_dev)
        } else {
            0.0
        });
    }
    out
}

/// Rate of Change = 100·(value − value[period bars ago]) / value[period ago].
pub fn compute_roc(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    for i in period..n {
        let prev = values[i - period];
        if prev != 0.0 {
            out[i] = Some(100.0 * (values[i] - prev) / prev);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stochastic_at_top_of_range_is_high() {
        // Close pinned at the high of a rising range -> %K near 100.
        let highs: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let lows: Vec<f64> = (0..30).map(|i| 90.0 + i as f64).collect();
        let closes = highs.clone(); // close = high
        let s = compute_stochastic(&highs, &lows, &closes, 14, 3);
        let last = s.last().unwrap().unwrap();
        assert!(last.k > 90.0, "k={}", last.k);
    }

    #[test]
    fn williams_r_in_range() {
        let highs: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let lows: Vec<f64> = (0..30).map(|i| 90.0 + i as f64).collect();
        let closes: Vec<f64> = (0..30).map(|i| 95.0 + i as f64).collect();
        let w = compute_williams_r(&highs, &lows, &closes, 14);
        let last = w.last().unwrap().unwrap();
        assert!((-100.0..=0.0).contains(&last));
    }

    #[test]
    fn roc_of_steady_growth_positive() {
        let v: Vec<f64> = (0..30).map(|i| 100.0 * (1.0 + 0.01 * i as f64)).collect();
        let r = compute_roc(&v, 10);
        assert!(r[20].unwrap() > 0.0);
        assert!(r[5].is_none()); // warmup
    }

    #[test]
    fn cci_zero_for_flat_series() {
        let v = vec![100.0; 40];
        let c = compute_cci(&v, &v, &v, 20);
        assert_eq!(c[30], Some(0.0));
    }
}
