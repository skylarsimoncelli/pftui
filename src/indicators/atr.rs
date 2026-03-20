//! Average True Range (ATR) — volatility indicator using OHLCV data.
//!
//! True Range = max(high - low, |high - prev_close|, |low - prev_close|)
//! ATR = smoothed moving average of True Range over `period` bars.
//!
//! Requires high, low, and close data. Falls back to close-only range when
//! OHLCV is unavailable.

/// Compute True Range for each bar given high, low, close arrays.
///
/// Returns `Vec<Option<f64>>` of the same length. First element is
/// `high[0] - low[0]` (no previous close available).
pub fn compute_true_range(
    highs: &[Option<f64>],
    lows: &[Option<f64>],
    closes: &[f64],
) -> Vec<Option<f64>> {
    if closes.is_empty() {
        return vec![];
    }

    let mut tr = Vec::with_capacity(closes.len());

    for i in 0..closes.len() {
        match (highs.get(i).copied().flatten(), lows.get(i).copied().flatten()) {
            (Some(high), Some(low)) => {
                if i == 0 {
                    tr.push(Some(high - low));
                } else {
                    let prev_close = closes[i - 1];
                    let hl = high - low;
                    let hc = (high - prev_close).abs();
                    let lc = (low - prev_close).abs();
                    tr.push(Some(hl.max(hc).max(lc)));
                }
            }
            _ => {
                // No OHLCV — use close-to-close range as fallback
                if i == 0 {
                    tr.push(None);
                } else {
                    tr.push(Some((closes[i] - closes[i - 1]).abs()));
                }
            }
        }
    }

    tr
}

/// Compute ATR (Average True Range) using Wilder's smoothing.
///
/// Returns `Vec<Option<f64>>` of the same length as input. First `period - 1`
/// entries are `None`. Uses Wilder's smoothing: ATR[i] = (ATR[i-1] * (period-1) + TR[i]) / period.
pub fn compute_atr(
    highs: &[Option<f64>],
    lows: &[Option<f64>],
    closes: &[f64],
    period: usize,
) -> Vec<Option<f64>> {
    if period == 0 || closes.len() < period {
        return vec![None; closes.len()];
    }

    let tr = compute_true_range(highs, lows, closes);
    let mut atr = vec![None; closes.len()];

    // Initial ATR = simple average of first `period` true ranges.
    // Tolerate one missing TR (e.g. first bar with no previous close and no OHLCV).
    let initial_trs: Vec<f64> = tr[..period].iter().filter_map(|v| *v).collect();
    let min_required = if period > 1 { period - 1 } else { 1 };
    if initial_trs.len() < min_required {
        return atr;
    }

    let initial_atr = initial_trs.iter().sum::<f64>() / initial_trs.len() as f64;
    atr[period - 1] = Some(initial_atr);

    // Wilder's smoothing for subsequent values
    let mut prev_atr = initial_atr;
    for i in period..closes.len() {
        if let Some(current_tr) = tr[i] {
            let smoothed = (prev_atr * (period as f64 - 1.0) + current_tr) / period as f64;
            atr[i] = Some(smoothed);
            prev_atr = smoothed;
        } else {
            atr[i] = Some(prev_atr); // Carry forward on missing TR
        }
    }

    atr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_range_basic() {
        let highs = vec![Some(105.0), Some(108.0), Some(107.0)];
        let lows = vec![Some(95.0), Some(100.0), Some(99.0)];
        let closes = vec![100.0, 105.0, 103.0];

        let tr = compute_true_range(&highs, &lows, &closes);
        assert_eq!(tr.len(), 3);
        // First bar: high - low = 10.0
        assert!((tr[0].unwrap() - 10.0).abs() < 1e-10);
        // Second bar: max(8, |108-100|, |100-100|) = 8
        assert!((tr[1].unwrap() - 8.0).abs() < 1e-10);
        // Third bar: max(8, |107-105|, |99-105|) = max(8, 2, 6) = 8
        assert!((tr[2].unwrap() - 8.0).abs() < 1e-10);
    }

    #[test]
    fn true_range_gap_up() {
        // Gap up scenario: prev close 100, today high 115, low 110
        let highs = vec![Some(105.0), Some(115.0)];
        let lows = vec![Some(95.0), Some(110.0)];
        let closes = vec![100.0, 112.0];

        let tr = compute_true_range(&highs, &lows, &closes);
        // Second bar: max(5, |115-100|, |110-100|) = max(5, 15, 10) = 15
        assert!((tr[1].unwrap() - 15.0).abs() < 1e-10);
    }

    #[test]
    fn true_range_fallback_no_ohlcv() {
        let highs = vec![None, None, None];
        let lows = vec![None, None, None];
        let closes = vec![100.0, 105.0, 102.0];

        let tr = compute_true_range(&highs, &lows, &closes);
        assert!(tr[0].is_none()); // No previous close, no OHLCV
        assert!((tr[1].unwrap() - 5.0).abs() < 1e-10); // |105 - 100|
        assert!((tr[2].unwrap() - 3.0).abs() < 1e-10); // |102 - 105|
    }

    #[test]
    fn atr_basic() {
        // 20 bars of data with stable OHLCV
        let n = 20;
        let highs: Vec<Option<f64>> = (0..n).map(|i| Some(100.0 + i as f64 + 2.0)).collect();
        let lows: Vec<Option<f64>> = (0..n).map(|i| Some(100.0 + i as f64 - 2.0)).collect();
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();

        let atr = compute_atr(&highs, &lows, &closes, 14);
        assert_eq!(atr.len(), n);
        // First 13 entries should be None
        assert!(atr[..13].iter().all(|v| v.is_none()));
        // Entry 13 (index) should have a value
        assert!(atr[13].is_some());
        // ATR should be around 4.0 (high-low range) for a trending market
        let val = atr[13].unwrap();
        assert!(val > 3.0 && val < 6.0, "Expected ATR ~4.0, got {val}");
    }

    #[test]
    fn atr_empty() {
        let atr = compute_atr(&[], &[], &[], 14);
        assert!(atr.is_empty());
    }

    #[test]
    fn atr_insufficient_data() {
        let closes = vec![100.0; 5];
        let highs = vec![Some(101.0); 5];
        let lows = vec![Some(99.0); 5];
        let atr = compute_atr(&highs, &lows, &closes, 14);
        assert!(atr.iter().all(|v| v.is_none()));
    }

    #[test]
    fn atr_period_zero() {
        let atr = compute_atr(&[Some(105.0)], &[Some(95.0)], &[100.0], 0);
        assert!(atr.iter().all(|v| v.is_none()));
    }

    #[test]
    fn atr_wilder_smoothing() {
        // Verify Wilder's smoothing: ATR[i] = (ATR[i-1] * 13 + TR[i]) / 14
        let n = 20;
        let highs: Vec<Option<f64>> = (0..n).map(|_| Some(105.0)).collect();
        let lows: Vec<Option<f64>> = (0..n).map(|_| Some(95.0)).collect();
        let closes: Vec<f64> = vec![100.0; n];

        let atr = compute_atr(&highs, &lows, &closes, 14);
        // All TR values are 10.0, so ATR should converge to 10.0
        let last = atr.last().unwrap().unwrap();
        assert!((last - 10.0).abs() < 0.01, "Expected ATR ~10.0, got {last}");
    }
}
