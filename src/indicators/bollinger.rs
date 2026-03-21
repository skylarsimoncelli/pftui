//! Bollinger Bands — volatility-based envelope around SMA.
//!
//! Standard parameters: period=20, multiplier=2.0.
//! Upper = SMA + k * σ, Lower = SMA - k * σ.

use crate::indicators::sma::compute_sma;

/// Bollinger Bands result for a single data point.
#[derive(Debug, Clone, Copy)]
pub struct BollingerBands {
    /// Middle band (SMA).
    #[allow(dead_code)]
    pub middle: f64,
    /// Upper band (SMA + multiplier * stddev).
    pub upper: f64,
    /// Lower band (SMA - multiplier * stddev).
    pub lower: f64,
    /// Band width as a fraction: (upper - lower) / middle.
    pub width: f64,
}

/// Compute Bollinger Bands for the given price series.
///
/// Returns `Vec<Option<BollingerBands>>` of the same length as `values`.
/// The first `period - 1` entries are `None`.
pub fn compute_bollinger(
    values: &[f64],
    period: usize,
    multiplier: f64,
) -> Vec<Option<BollingerBands>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }

    let sma = compute_sma(values, period);
    let mut result = Vec::with_capacity(values.len());

    for (i, sma_val) in sma.iter().enumerate() {
        match sma_val {
            Some(mean) => {
                let start = i + 1 - period;
                let window = &values[start..=i];
                let variance =
                    window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
                let stddev = variance.sqrt();
                let upper = mean + multiplier * stddev;
                let lower = mean - multiplier * stddev;
                let width = if *mean != 0.0 {
                    (upper - lower) / mean
                } else {
                    0.0
                };
                result.push(Some(BollingerBands {
                    middle: *mean,
                    upper,
                    lower,
                    width,
                }));
            }
            None => result.push(None),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bollinger_basic() {
        let data: Vec<f64> = (1..=25).map(|i| i as f64).collect();
        let bb = compute_bollinger(&data, 20, 2.0);
        assert_eq!(bb.len(), 25);
        assert!(bb[..19].iter().all(|v| v.is_none()));

        let b = bb[19].unwrap();
        // SMA of 1..=20 = 10.5
        assert!((b.middle - 10.5).abs() < 1e-10);
        assert!(b.upper > b.middle);
        assert!(b.lower < b.middle);
        assert!(b.width > 0.0);
    }

    #[test]
    fn bollinger_flat_prices() {
        let data = vec![100.0; 25];
        let bb = compute_bollinger(&data, 20, 2.0);
        let b = bb[19].unwrap();
        assert!((b.middle - 100.0).abs() < 1e-10);
        // Flat → stddev = 0 → upper = lower = middle
        assert!((b.upper - 100.0).abs() < 1e-10);
        assert!((b.lower - 100.0).abs() < 1e-10);
        assert!(b.width.abs() < 1e-10);
    }

    #[test]
    fn bollinger_symmetry() {
        let data: Vec<f64> = (1..=30)
            .map(|i| 50.0 + (i as f64 * 0.5).sin() * 3.0)
            .collect();
        let bb = compute_bollinger(&data, 20, 2.0);
        for b in bb.iter().flatten() {
            let upper_dist = b.upper - b.middle;
            let lower_dist = b.middle - b.lower;
            assert!(
                (upper_dist - lower_dist).abs() < 1e-10,
                "Bands must be symmetric around middle"
            );
        }
    }

    #[test]
    fn bollinger_empty() {
        let bb = compute_bollinger(&[], 20, 2.0);
        assert!(bb.is_empty());
    }

    #[test]
    fn bollinger_period_zero() {
        let bb = compute_bollinger(&[1.0, 2.0], 0, 2.0);
        assert!(bb.iter().all(|v| v.is_none()));
    }

    #[test]
    fn bollinger_width_calculation() {
        let data: Vec<f64> = (1..=25).map(|i| i as f64).collect();
        let bb = compute_bollinger(&data, 20, 2.0);
        let b = bb[19].unwrap();
        let expected_width = (b.upper - b.lower) / b.middle;
        assert!((b.width - expected_width).abs() < 1e-10);
    }
}
