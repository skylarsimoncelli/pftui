//! Simple Moving Average (SMA).
//!
//! Rolling mean over a fixed window. Common periods: 50, 200.

/// Compute the Simple Moving Average for `values` with the given `period`.
///
/// Returns a `Vec<Option<f64>>` of the same length as `values`.
/// The first `period - 1` entries are `None` (insufficient data).
///
/// # Panics
///
/// Does not panic. Returns all-`None` if `period` is 0 or `values` is empty.
pub fn compute_sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || values.is_empty() {
        return vec![None; values.len()];
    }

    let mut result = Vec::with_capacity(values.len());
    let mut window_sum = 0.0;

    for (i, &v) in values.iter().enumerate() {
        window_sum += v;
        if i >= period {
            window_sum -= values[i - period];
        }
        if i + 1 >= period {
            result.push(Some(window_sum / period as f64));
        } else {
            result.push(None);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sma_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sma = compute_sma(&data, 3);
        assert_eq!(sma.len(), 5);
        assert!(sma[0].is_none());
        assert!(sma[1].is_none());
        assert!((sma[2].unwrap() - 2.0).abs() < 1e-10);
        assert!((sma[3].unwrap() - 3.0).abs() < 1e-10);
        assert!((sma[4].unwrap() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn sma_period_one() {
        let data = vec![10.0, 20.0, 30.0];
        let sma = compute_sma(&data, 1);
        for (i, val) in sma.iter().enumerate() {
            assert!((val.unwrap() - data[i]).abs() < 1e-10);
        }
    }

    #[test]
    fn sma_period_zero() {
        let data = vec![1.0, 2.0];
        let sma = compute_sma(&data, 0);
        assert!(sma.iter().all(|v| v.is_none()));
    }

    #[test]
    fn sma_empty_input() {
        let sma = compute_sma(&[], 5);
        assert!(sma.is_empty());
    }

    #[test]
    fn sma_period_equals_length() {
        let data = vec![2.0, 4.0, 6.0];
        let sma = compute_sma(&data, 3);
        assert!(sma[0].is_none());
        assert!(sma[1].is_none());
        assert!((sma[2].unwrap() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn sma_period_exceeds_length() {
        let data = vec![1.0, 2.0];
        let sma = compute_sma(&data, 5);
        assert!(sma.iter().all(|v| v.is_none()));
    }
}
