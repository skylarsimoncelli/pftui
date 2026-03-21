//! MACD — Moving Average Convergence Divergence.
//!
//! Standard parameters: fast=12, slow=26, signal=9.
//! Uses Exponential Moving Average (EMA) internally.

/// Result of MACD computation for a single data point.
#[derive(Debug, Clone, Copy)]
pub struct MacdResult {
    /// MACD line (fast EMA - slow EMA).
    pub macd: f64,
    /// Signal line (EMA of MACD line).
    pub signal: f64,
    /// Histogram (MACD - signal).
    pub histogram: f64,
}

/// Compute MACD with the given parameters.
///
/// `prices` should be closing prices in chronological order.
/// Returns `Vec<Option<MacdResult>>` of the same length as `prices`.
///
/// The first `slow_period - 1` entries are always `None` (not enough data for
/// the slow EMA). The signal line requires an additional `signal_period - 1`
/// MACD values to seed, so the first non-`None` entry with a meaningful signal
/// appears at index `slow_period + signal_period - 2`.
///
/// Between those two thresholds the MACD line is valid but signal is
/// approximated from available values, which matches standard charting
/// implementations.
pub fn compute_macd(
    prices: &[f64],
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> Vec<Option<MacdResult>> {
    if fast_period == 0 || slow_period == 0 || signal_period == 0 {
        return vec![None; prices.len()];
    }
    if prices.len() < slow_period {
        return vec![None; prices.len()];
    }

    let fast_ema = compute_ema(prices, fast_period);
    let slow_ema = compute_ema(prices, slow_period);

    // MACD line = fast EMA - slow EMA
    let mut macd_line: Vec<Option<f64>> = Vec::with_capacity(prices.len());
    for i in 0..prices.len() {
        match (fast_ema[i], slow_ema[i]) {
            (Some(f), Some(s)) => macd_line.push(Some(f - s)),
            _ => macd_line.push(None),
        }
    }

    // Signal line = EMA of MACD line values
    let signal_line = ema_of_optional(&macd_line, signal_period);

    // Assemble results
    let mut result = Vec::with_capacity(prices.len());
    for i in 0..prices.len() {
        match (macd_line[i], signal_line[i]) {
            (Some(m), Some(s)) => result.push(Some(MacdResult {
                macd: m,
                signal: s,
                histogram: m - s,
            })),
            _ => result.push(None),
        }
    }

    result
}

/// Compute Exponential Moving Average.
///
/// The first `period - 1` entries are `None`. The value at index `period - 1`
/// is the simple average (SMA) seed. Subsequent values use the EMA formula:
/// `EMA_today = price * k + EMA_prev * (1 - k)` where `k = 2 / (period + 1)`.
fn compute_ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }

    let mut result = vec![None; values.len()];

    if values.len() < period {
        return result;
    }

    // Seed: SMA of first `period` values
    let seed: f64 = values[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = Some(seed);

    let k = 2.0 / (period as f64 + 1.0);
    let mut prev = seed;

    for i in period..values.len() {
        let ema = values[i] * k + prev * (1.0 - k);
        result[i] = Some(ema);
        prev = ema;
    }

    result
}

/// Compute EMA over an `Option<f64>` series (for the signal line).
/// Skips `None` entries; starts the seed window from the first non-`None` value.
fn ema_of_optional(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut result = vec![None; values.len()];
    if period == 0 {
        return result;
    }

    // Collect indices of non-None values for seeding
    let mut non_none_count = 0usize;
    let mut seed_sum = 0.0;
    let mut seeded = false;
    let mut prev = 0.0;
    let k = 2.0 / (period as f64 + 1.0);

    for (i, val) in values.iter().enumerate() {
        if let Some(v) = val {
            non_none_count += 1;
            if !seeded {
                seed_sum += v;
                if non_none_count == period {
                    let seed = seed_sum / period as f64;
                    result[i] = Some(seed);
                    prev = seed;
                    seeded = true;
                }
            } else {
                let ema = v * k + prev * (1.0 - k);
                result[i] = Some(ema);
                prev = ema;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macd_basic() {
        // 50 prices: enough for MACD(12,26,9)
        let prices: Vec<f64> = (0..50).map(|i| 100.0 + (i as f64).sin() * 5.0).collect();
        let macd = compute_macd(&prices, 12, 26, 9);
        assert_eq!(macd.len(), 50);
        // First 25 should be None (slow period = 26, need index 25 for first slow EMA)
        assert!(macd[..25].iter().all(|v| v.is_none()));
        // Later values should be Some
        assert!(macd[40].is_some());
        let r = macd[40].unwrap();
        assert!((r.histogram - (r.macd - r.signal)).abs() < 1e-10);
    }

    #[test]
    fn macd_too_few_prices() {
        let prices = vec![1.0, 2.0, 3.0];
        let macd = compute_macd(&prices, 12, 26, 9);
        assert!(macd.iter().all(|v| v.is_none()));
    }

    #[test]
    fn macd_zero_period() {
        let prices = vec![1.0; 50];
        let macd = compute_macd(&prices, 0, 26, 9);
        assert!(macd.iter().all(|v| v.is_none()));
    }

    #[test]
    fn macd_flat_prices() {
        // Flat prices → MACD, signal, histogram should all be ~0
        let prices = vec![50.0; 50];
        let macd = compute_macd(&prices, 12, 26, 9);
        for val in macd.iter().flatten() {
            assert!(val.macd.abs() < 1e-10, "MACD should be ~0 for flat prices");
            assert!(
                val.signal.abs() < 1e-10,
                "Signal should be ~0 for flat prices"
            );
            assert!(
                val.histogram.abs() < 1e-10,
                "Histogram should be ~0 for flat prices"
            );
        }
    }

    #[test]
    fn macd_trending_up() {
        // Steadily rising prices → fast EMA > slow EMA → positive MACD
        let prices: Vec<f64> = (0..60).map(|i| 100.0 + i as f64).collect();
        let macd = compute_macd(&prices, 12, 26, 9);
        // After the signal line seeds, MACD should be positive
        for val in macd[40..].iter().flatten() {
            assert!(
                val.macd > 0.0,
                "MACD should be positive in uptrend, got {}",
                val.macd
            );
        }
    }

    #[test]
    fn ema_basic() {
        let data = vec![10.0, 11.0, 12.0, 13.0, 14.0];
        let ema = compute_ema(&data, 3);
        assert!(ema[0].is_none());
        assert!(ema[1].is_none());
        // Seed at index 2: (10+11+12)/3 = 11.0
        assert!((ema[2].unwrap() - 11.0).abs() < 1e-10);
        // k = 2/(3+1) = 0.5, EMA[3] = 13*0.5 + 11*0.5 = 12.0
        assert!((ema[3].unwrap() - 12.0).abs() < 1e-10);
    }

    #[test]
    fn histogram_equals_macd_minus_signal() {
        let prices: Vec<f64> = (0..60)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 10.0)
            .collect();
        let macd = compute_macd(&prices, 12, 26, 9);
        for val in macd.iter().flatten() {
            assert!(
                (val.histogram - (val.macd - val.signal)).abs() < 1e-10,
                "Histogram must equal MACD - signal"
            );
        }
    }
}
