//! Pearson Correlation Coefficient.
//!
//! Measures linear relationship between two price series.
//! Returns values in range [-1, 1]:
//! - +1: perfect positive correlation
//! -  0: no correlation
//! - -1: perfect negative correlation

/// Compute Pearson correlation coefficient between two price series over a rolling window.
///
/// `prices_a` and `prices_b` must have the same length.
/// `window` is the lookback period (typically 7, 30, or 90 days).
///
/// Returns a `Vec<Option<f64>>` of the same length as the input series.
/// Values before index `window` are `None` (insufficient data).
///
/// Correlation is computed on daily returns (percent change from previous close).
///
/// # Panics
///
/// Does not panic. Returns all-`None` if inputs have different lengths,
/// window is 0, or insufficient data.
#[allow(dead_code)]
pub fn compute_rolling_correlation(
    prices_a: &[f64],
    prices_b: &[f64],
    window: usize,
) -> Vec<Option<f64>> {
    if prices_a.len() != prices_b.len() || window == 0 || prices_a.len() < window + 1 {
        return vec![None; prices_a.len()];
    }

    let mut result = vec![None; prices_a.len()];

    // Compute daily returns
    let returns_a: Vec<f64> = prices_a
        .windows(2)
        .map(|w| {
            let prev = w[0];
            let curr = w[1];
            if prev.abs() < f64::EPSILON {
                0.0
            } else {
                (curr - prev) / prev
            }
        })
        .collect();

    let returns_b: Vec<f64> = prices_b
        .windows(2)
        .map(|w| {
            let prev = w[0];
            let curr = w[1];
            if prev.abs() < f64::EPSILON {
                0.0
            } else {
                (curr - prev) / prev
            }
        })
        .collect();

    // Compute rolling correlation on returns
    for i in window..=returns_a.len() {
        let slice_a = &returns_a[i - window..i];
        let slice_b = &returns_b[i - window..i];

        if let Some(corr) = pearson(slice_a, slice_b) {
            result[i] = Some(corr);
        }
    }

    result
}

/// Compute Pearson correlation for a single pair of return slices.
fn pearson(returns_a: &[f64], returns_b: &[f64]) -> Option<f64> {
    if returns_a.is_empty() || returns_a.len() != returns_b.len() {
        return None;
    }

    let n = returns_a.len() as f64;
    let mean_a: f64 = returns_a.iter().sum::<f64>() / n;
    let mean_b: f64 = returns_b.iter().sum::<f64>() / n;

    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;

    for i in 0..returns_a.len() {
        let da = returns_a[i] - mean_a;
        let db = returns_b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < f64::EPSILON {
        return None;
    }

    Some(cov / denom)
}

/// Correlation break detection.
///
/// Identifies points where correlation changed significantly between two rolling windows.
/// A "break" occurs when `|corr_short - corr_long| > threshold`.
///
/// `short_window` is typically 7 or 30 days.
/// `long_window` is typically 90 days.
/// `threshold` is typically 0.3 (significant change).
///
/// Returns a `Vec<Option<f64>>` where `Some(delta)` indicates a correlation break
/// (delta = short_window_corr - long_window_corr), and `None` means no break or insufficient data.
#[allow(dead_code)]
pub fn detect_correlation_breaks(
    prices_a: &[f64],
    prices_b: &[f64],
    short_window: usize,
    long_window: usize,
    threshold: f64,
) -> Vec<Option<f64>> {
    if prices_a.len() != prices_b.len() {
        return vec![None; prices_a.len()];
    }

    let short_corr = compute_rolling_correlation(prices_a, prices_b, short_window);
    let long_corr = compute_rolling_correlation(prices_a, prices_b, long_window);

    let mut breaks = vec![None; prices_a.len()];

    for i in 0..prices_a.len() {
        if let (Some(sc), Some(lc)) = (short_corr[i], long_corr[i]) {
            let delta = sc - lc;
            if delta.abs() > threshold {
                breaks[i] = Some(delta);
            }
        }
    }

    breaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_perfect_positive() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let b = vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0]; // b = 2*a
        let corr = compute_rolling_correlation(&a, &b, 5);

        assert!(corr[0..5].iter().all(|c| c.is_none()));
        for c in corr.iter().skip(5).flatten() {
            assert!((c - 1.0).abs() < 0.01, "expected ~1.0, got {}", c);
        }
    }

    #[test]
    fn correlation_perfect_negative() {
        // For negative correlation on returns, we need opposite movements
        // a rising when b falling and vice versa
        let a = vec![1.0, 2.0, 1.5, 2.5, 2.0, 3.0, 2.5, 3.5];
        let b = vec![8.0, 7.0, 7.5, 6.5, 7.0, 6.0, 6.5, 5.5];
        let corr = compute_rolling_correlation(&a, &b, 5);

        // With alternating up/down movements, we should see negative correlation
        for c in corr.iter().skip(5).flatten() {
            assert!(c < &0.0, "expected negative correlation, got {}", c);
        }
    }

    #[test]
    fn correlation_uncorrelated() {
        // Flat vs random-ish changes — should be near 0
        let a = vec![5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0];
        let b = vec![1.0, 2.0, 1.5, 2.5, 1.0, 3.0, 1.0, 2.0];
        let corr = compute_rolling_correlation(&a, &b, 5);

        // All returns for `a` are 0, so correlation is undefined (should be None)
        assert!(
            corr.iter().skip(5).all(|c| c.is_none()),
            "expected None for zero-variance series"
        );
    }

    #[test]
    fn correlation_insufficient_data() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0];
        let corr = compute_rolling_correlation(&a, &b, 10);
        assert!(corr.iter().all(|c| c.is_none()));
    }

    #[test]
    fn correlation_mismatched_length() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0];
        let corr = compute_rolling_correlation(&a, &b, 2);
        assert_eq!(corr.len(), 3);
        assert!(corr.iter().all(|c| c.is_none()));
    }

    #[test]
    fn correlation_window_zero() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0];
        let corr = compute_rolling_correlation(&a, &b, 0);
        assert!(corr.iter().all(|c| c.is_none()));
    }

    #[test]
    fn break_detection_basic() {
        // Shift from uncorrelated to strongly correlated
        // First 10 values: a rises steadily, b alternates (uncorrelated)
        // Last 10 values: both rise steadily together (correlated)
        let mut a = vec![];
        let mut b = vec![];

        // First half: a rises, b alternates
        for i in 0..12 {
            a.push(i as f64);
            b.push(if i % 2 == 0 { 10.0 } else { 11.0 });
        }

        // Second half: both rise together
        for i in 12..24 {
            a.push(i as f64);
            b.push((i * 2) as f64);
        }

        let breaks = detect_correlation_breaks(&a, &b, 5, 10, 0.3);

        // Should detect a break somewhere in the transition
        let has_break = breaks.iter().any(|b| b.is_some());
        assert!(has_break, "expected at least one correlation break");
    }

    #[test]
    fn break_detection_no_breaks() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let b = vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0];

        let breaks = detect_correlation_breaks(&a, &b, 3, 5, 0.3);

        // Perfect correlation throughout — no breaks
        assert!(
            breaks.iter().all(|b| b.is_none()),
            "expected no breaks in perfectly correlated series"
        );
    }

    #[test]
    fn pearson_basic() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0];
        let corr = pearson(&a, &b);
        assert!(corr.is_some());
        assert!((corr.unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn pearson_empty() {
        let corr = pearson(&[], &[]);
        assert!(corr.is_none());
    }

    #[test]
    fn pearson_mismatched() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0];
        let corr = pearson(&a, &b);
        assert!(corr.is_none());
    }
}
