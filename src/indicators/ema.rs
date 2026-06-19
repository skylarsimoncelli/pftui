//! Exponential Moving Average — first-class (previously inlined in macd/resolver).

/// EMA with an SMA seed at index `period-1`. First `period-1` entries are
/// `None`. Multiplier k = 2 / (period + 1).
pub fn compute_ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 || n < period {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let seed: f64 = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(seed);
    let mut prev = seed;
    for i in period..n {
        let cur = (values[i] - prev) * k + prev;
        out[i] = Some(cur);
        prev = cur;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_warmup_then_tracks() {
        let v: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        let e = compute_ema(&v, 3);
        assert!(e[0].is_none() && e[1].is_none());
        assert_eq!(e[2], Some(2.0)); // SMA(1,2,3) seed
        assert!(e[9].unwrap() > e[2].unwrap()); // tracks the uptrend
    }

    #[test]
    fn ema_empty_for_short_series() {
        assert!(compute_ema(&[1.0, 2.0], 5).iter().all(|x| x.is_none()));
    }
}
