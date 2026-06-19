//! Volume indicators — OBV (On-Balance Volume) and MFI (Money Flow Index).
//!
//! Pure functions over price/volume slices.

/// On-Balance Volume — a running total that adds the bar's volume on an up
/// close and subtracts it on a down close. The cumulative LEVEL is arbitrary;
/// its trend/divergence vs price is the signal.
pub fn compute_obv(closes: &[f64], volumes: &[f64]) -> Vec<Option<f64>> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![None; closes.len()];
    if n == 0 {
        return out;
    }
    let mut obv = 0.0;
    out[0] = Some(0.0);
    for i in 1..n {
        if closes[i] > closes[i - 1] {
            obv += volumes[i];
        } else if closes[i] < closes[i - 1] {
            obv -= volumes[i];
        }
        out[i] = Some(obv);
    }
    out
}

/// Money Flow Index — a volume-weighted RSI over `period`. 0–100; >80
/// overbought, <20 oversold. Typical price = (high + low + close) / 3.
#[allow(clippy::needless_range_loop)] // multi-array windowed indexing
pub fn compute_mfi(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    period: usize,
) -> Vec<Option<f64>> {
    let n = closes.len().min(volumes.len());
    let mut out = vec![None; closes.len()];
    if period == 0 || n < period + 1 {
        return out;
    }
    let tp: Vec<f64> = (0..n).map(|i| (highs[i] + lows[i] + closes[i]) / 3.0).collect();
    let raw_mf: Vec<f64> = (0..n).map(|i| tp[i] * volumes[i]).collect();
    for i in period..n {
        let mut pos = 0.0;
        let mut neg = 0.0;
        for j in (i + 1 - period)..=i {
            if tp[j] > tp[j - 1] {
                pos += raw_mf[j];
            } else if tp[j] < tp[j - 1] {
                neg += raw_mf[j];
            }
        }
        out[i] = Some(if neg == 0.0 {
            100.0
        } else {
            let ratio = pos / neg;
            100.0 - 100.0 / (1.0 + ratio)
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obv_rises_with_up_closes() {
        let closes = vec![10.0, 11.0, 12.0, 11.5, 13.0];
        let vols = vec![100.0, 100.0, 100.0, 100.0, 100.0];
        let o = compute_obv(&closes, &vols);
        // up,up,down,up -> 0,+100,+200,+100,+200
        assert_eq!(o[1], Some(100.0));
        assert_eq!(o[2], Some(200.0));
        assert_eq!(o[3], Some(100.0));
        assert_eq!(o[4], Some(200.0));
    }

    #[test]
    fn mfi_high_when_rising_on_volume() {
        let n = 30;
        let highs: Vec<f64> = (0..n).map(|i| 101.0 + i as f64).collect();
        let lows: Vec<f64> = (0..n).map(|i| 99.0 + i as f64).collect();
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let vols = vec![1000.0; n];
        let m = compute_mfi(&highs, &lows, &closes, &vols, 14);
        assert!(m.last().unwrap().unwrap() > 80.0);
    }
}
