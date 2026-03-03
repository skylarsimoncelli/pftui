//! Relative Strength Index (RSI) — Wilder's smoothing method.
//!
//! Standard period: 14. Values range 0–100.
//! - RSI < 30: oversold
//! - RSI > 70: overbought

/// Compute RSI using Wilder's smoothing method.
///
/// `prices` should be closing prices in chronological order.
/// `period` is the lookback window (typically 14).
///
/// Returns a `Vec<Option<f64>>` of the same length as `prices`.
/// The first `period` entries are `None` (need `period` price changes,
/// which requires `period + 1` prices to begin producing values, but the
/// first value appears at index `period`).
///
/// # Panics
///
/// Does not panic. Returns all-`None` if `period` is 0 or `prices` has
/// fewer than `period + 1` elements.
pub fn compute_rsi(prices: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || prices.len() < period + 1 {
        return vec![None; prices.len()];
    }

    let mut result = vec![None; prices.len()];

    // Step 1: compute price changes
    let changes: Vec<f64> = prices.windows(2).map(|w| w[1] - w[0]).collect();

    // Step 2: seed — average gain / average loss over the first `period` changes
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    for &ch in &changes[..period] {
        if ch > 0.0 {
            avg_gain += ch;
        } else {
            avg_loss += ch.abs();
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;

    // First RSI value at index `period` (we have `period` changes → `period + 1` prices)
    result[period] = Some(rsi_from_avgs(avg_gain, avg_loss));

    // Step 3: Wilder's smoothing for remaining changes
    let pf = period as f64;
    for i in period..changes.len() {
        let ch = changes[i];
        let (gain, loss) = if ch > 0.0 {
            (ch, 0.0)
        } else {
            (0.0, ch.abs())
        };
        avg_gain = (avg_gain * (pf - 1.0) + gain) / pf;
        avg_loss = (avg_loss * (pf - 1.0) + loss) / pf;
        result[i + 1] = Some(rsi_from_avgs(avg_gain, avg_loss));
    }

    result
}

/// Convert average gain/loss to RSI value.
fn rsi_from_avgs(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        100.0
    } else {
        let rs = avg_gain / avg_loss;
        100.0 - (100.0 / (1.0 + rs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsi_basic_14() {
        // 15 prices → 14 changes → first RSI at index 14
        let prices: Vec<f64> = (0..15).map(|i| 44.0 + i as f64 * 0.5).collect();
        let rsi = compute_rsi(&prices, 14);
        assert_eq!(rsi.len(), 15);
        assert!(rsi[..14].iter().all(|v| v.is_none()));
        // All prices rising → RSI should be 100
        assert!((rsi[14].unwrap() - 100.0).abs() < 1e-10);
    }

    #[test]
    fn rsi_all_falling() {
        let prices: Vec<f64> = (0..16).map(|i| 50.0 - i as f64).collect();
        let rsi = compute_rsi(&prices, 14);
        // All prices falling → RSI should be 0
        assert!((rsi[14].unwrap() - 0.0).abs() < 1e-10);
        assert!((rsi[15].unwrap() - 0.0).abs() < 1e-10);
    }

    #[test]
    fn rsi_mixed() {
        // Alternating up/down
        let mut prices = vec![50.0];
        for i in 1..20 {
            if i % 2 == 0 {
                prices.push(prices[i - 1] + 1.0);
            } else {
                prices.push(prices[i - 1] - 0.5);
            }
        }
        let rsi = compute_rsi(&prices, 14);
        // Should be between 0 and 100
        for val in rsi.iter().flatten() {
            assert!(*val >= 0.0 && *val <= 100.0, "RSI out of range: {val}");
        }
    }

    #[test]
    fn rsi_too_few_prices() {
        let prices = vec![1.0, 2.0, 3.0];
        let rsi = compute_rsi(&prices, 14);
        assert!(rsi.iter().all(|v| v.is_none()));
    }

    #[test]
    fn rsi_period_zero() {
        let prices = vec![1.0, 2.0, 3.0];
        let rsi = compute_rsi(&prices, 0);
        assert!(rsi.iter().all(|v| v.is_none()));
    }

    #[test]
    fn rsi_flat_prices() {
        // All prices equal → 0 gain, 0 loss → RSI = 100 (by convention: no losses)
        let prices = vec![50.0; 20];
        let rsi = compute_rsi(&prices, 14);
        // avg_gain = 0, avg_loss = 0 → rsi_from_avgs(0, 0) = 100
        assert!((rsi[14].unwrap() - 100.0).abs() < 1e-10);
    }

    #[test]
    fn rsi_wilder_smoothing() {
        // Verify Wilder's smoothing produces different result than simple average
        // 30 prices: first 15 rising, then mixed
        let mut prices = Vec::new();
        for i in 0..15 {
            prices.push(40.0 + i as f64);
        }
        for i in 0..15 {
            prices.push(if i % 2 == 0 { 55.0 - i as f64 * 0.3 } else { 54.0 + i as f64 * 0.2 });
        }
        let rsi = compute_rsi(&prices, 14);

        // After the initial seed, subsequent values use smoothing
        // The RSI at index 14 should be 100 (all gains in seed window)
        assert!((rsi[14].unwrap() - 100.0).abs() < 1e-10);
        // Later values should drop as losses enter
        let later = rsi[20].unwrap();
        assert!(later < 100.0, "Expected RSI to drop from 100, got {later}");
    }
}
