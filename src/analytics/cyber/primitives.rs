//! Pine-faithful series primitives shared by the Cyber Dots components.
//!
//! Semantics ported from PineScript v6 built-ins as used by
//! `docs/reference/cyber-dots.pine`:
//!
//! - [`ema`] — recursive EMA seeded with the first source value, defined from
//!   bar 0 (`sum := na(sum[1]) ? src : alpha*src + (1-alpha)*sum[1]`). The
//!   script's `F_SMMA` quirk analysis (see `bands.rs`) relies on the EMA being
//!   defined from the first bar.
//! - [`sma`] — `ta.sma`: undefined (`None`) until `len` values exist.
//! - [`stdev_pop`] — `ta.stdev` default (biased / population standard
//!   deviation, divide by N), undefined until `len` values exist.
//! - [`rsi`] — `ta.rsi`: Wilder RMA smoothing of gains/losses, RMA seeded
//!   with the simple mean of the first `len` changes, so the first defined
//!   value is at index `len`.
//! - [`true_range`] — `ta.tr`: `max(h-l, |h-c[1]|, |l-c[1]|)`, undefined on
//!   bar 0 (no previous close).
//! - [`highest`] / [`lowest`] — `ta.highest` / `ta.lowest`: rolling window of
//!   `len` bars ending at the current bar, undefined until the window fills.
//! - [`crossover_at`] / [`crossunder_at`] — `ta.crossover(a, b)` =
//!   `a > b && a[1] <= b[1]` (and the mirror), false when either bar is
//!   undefined.
//!
//! All math is `f64` (existing precedent: `analytics::technicals`); these are
//! indicator values, not money.

/// Recursive EMA seeded with the first value (Pine `ta.ema` as relied on by
/// the script — defined from bar 0).
pub fn ema(src: &[f64], len: usize) -> Vec<f64> {
    if src.is_empty() || len == 0 {
        return Vec::new();
    }
    let alpha = 2.0 / (len as f64 + 1.0);
    let mut out = Vec::with_capacity(src.len());
    let mut prev = src[0];
    for (i, &v) in src.iter().enumerate() {
        let next = if i == 0 { v } else { alpha * v + (1.0 - alpha) * prev };
        out.push(next);
        prev = next;
    }
    out
}

/// Double EMA: `2·EMA(src,len) − EMA(EMA(src,len),len)` (Pine `F_DEMA`).
pub fn dema(src: &[f64], len: usize) -> Vec<f64> {
    let e1 = ema(src, len);
    let e2 = ema(&e1, len);
    e1.iter().zip(e2.iter()).map(|(a, b)| 2.0 * a - b).collect()
}

/// Simple moving average — `None` until `len` values exist (Pine `ta.sma`).
pub fn sma(src: &[f64], len: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if len == 0 || src.len() < len {
        return out;
    }
    let mut sum: f64 = src[..len].iter().sum();
    out[len - 1] = Some(sum / len as f64);
    for i in len..src.len() {
        sum += src[i] - src[i - len];
        out[i] = Some(sum / len as f64);
    }
    out
}

/// Population (biased) standard deviation over a rolling `len` window —
/// Pine `ta.stdev` default. `None` until the window fills.
pub fn stdev_pop(src: &[f64], len: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if len < 2 || src.len() < len {
        return out;
    }
    for i in (len - 1)..src.len() {
        let window = &src[i + 1 - len..=i];
        let mean = window.iter().sum::<f64>() / len as f64;
        let var = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / len as f64;
        out[i] = Some(var.sqrt());
    }
    out
}

/// Wilder RSI (Pine `ta.rsi`): RMA-smoothed gains/losses, RMA seeded with the
/// simple mean of the first `len` changes. First defined value at index `len`.
pub fn rsi(src: &[f64], len: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if len == 0 || src.len() <= len {
        return out;
    }
    let mut gain_seed = 0.0;
    let mut loss_seed = 0.0;
    for i in 1..=len {
        let d = src[i] - src[i - 1];
        if d > 0.0 {
            gain_seed += d;
        } else {
            loss_seed += -d;
        }
    }
    let mut avg_gain = gain_seed / len as f64;
    let mut avg_loss = loss_seed / len as f64;
    out[len] = Some(rsi_value(avg_gain, avg_loss));
    let alpha = 1.0 / len as f64;
    for i in (len + 1)..src.len() {
        let d = src[i] - src[i - 1];
        let gain = if d > 0.0 { d } else { 0.0 };
        let loss = if d < 0.0 { -d } else { 0.0 };
        avg_gain = (1.0 - alpha) * avg_gain + alpha * gain;
        avg_loss = (1.0 - alpha) * avg_loss + alpha * loss;
        out[i] = Some(rsi_value(avg_gain, avg_loss));
    }
    out
}

fn rsi_value(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        if avg_gain == 0.0 {
            // Flat series: Pine yields na (0/0); we emit the neutral 50 so
            // downstream zone checks (strict > / <) stay false, matching
            // Pine's na-comparison-is-false behaviour.
            50.0
        } else {
            100.0
        }
    } else {
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    }
}

/// Compute only the final RSI value of a series (used by the MTF ladder where
/// the higher-timeframe series is rebuilt per current-timeframe bar).
pub fn rsi_last(src: &[f64], len: usize) -> Option<f64> {
    rsi(src, len).last().copied().flatten()
}

/// Pine `ta.tr`: true range, undefined on bar 0.
pub fn true_range(high: &[f64], low: &[f64], close: &[f64]) -> Vec<Option<f64>> {
    let n = close.len();
    let mut out = vec![None; n];
    for i in 1..n {
        let pc = close[i - 1];
        let tr = (high[i] - low[i])
            .max((high[i] - pc).abs())
            .max((low[i] - pc).abs());
        out[i] = Some(tr);
    }
    out
}

/// Pine `ta.highest(src, len)` — rolling max, `None` until the window fills.
pub fn highest(src: &[f64], len: usize) -> Vec<Option<f64>> {
    rolling(src, len, f64::max)
}

/// Pine `ta.lowest(src, len)` — rolling min, `None` until the window fills.
pub fn lowest(src: &[f64], len: usize) -> Vec<Option<f64>> {
    rolling(src, len, f64::min)
}

fn rolling(src: &[f64], len: usize, fold: fn(f64, f64) -> f64) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if len == 0 || src.len() < len {
        return out;
    }
    for i in (len - 1)..src.len() {
        let window = &src[i + 1 - len..=i];
        let mut acc = window[0];
        for &v in &window[1..] {
            acc = fold(acc, v);
        }
        out[i] = Some(acc);
    }
    out
}

/// Pine `ta.crossover(a, b)` at index `i`: `a[i] > b[i] && a[i-1] <= b[i-1]`.
/// Undefined inputs (None) make the result false, mirroring Pine na handling.
pub fn crossover_at(a: &[Option<f64>], b: &[Option<f64>], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    match (a[i], b[i], a[i - 1], b[i - 1]) {
        (Some(a1), Some(b1), Some(a0), Some(b0)) => a1 > b1 && a0 <= b0,
        _ => false,
    }
}

/// Pine `ta.crossunder(a, b)` at index `i`: `a[i] < b[i] && a[i-1] >= b[i-1]`.
pub fn crossunder_at(a: &[Option<f64>], b: &[Option<f64>], i: usize) -> bool {
    if i == 0 {
        return false;
    }
    match (a[i], b[i], a[i - 1], b[i - 1]) {
        (Some(a1), Some(b1), Some(a0), Some(b0)) => a1 < b1 && a0 >= b0,
        _ => false,
    }
}

/// Round an indicator price level for presentation: 2 dp for values ≥ 100,
/// 3 dp for ≥ 1, 6 dp below (sub-dollar assets keep enough precision).
pub fn round_level(v: f64) -> f64 {
    let dp = if v.abs() >= 100.0 {
        100.0
    } else if v.abs() >= 1.0 {
        1_000.0
    } else {
        1_000_000.0
    };
    (v * dp).round() / dp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_seeds_with_first_value_and_recurses() {
        // len 3 → alpha = 0.5. Seed 10; bar1 = 0.5*20 + 0.5*10 = 15;
        // bar2 = 0.5*30 + 0.5*15 = 22.5; bar3 = 0.5*40 + 0.5*22.5 = 31.25.
        let e = ema(&[10.0, 20.0, 30.0, 40.0], 3);
        assert_eq!(e[0], 10.0);
        assert!((e[1] - 15.0).abs() < 1e-12);
        assert!((e[2] - 22.5).abs() < 1e-12);
        assert!((e[3] - 31.25).abs() < 1e-12);
    }

    #[test]
    fn dema_is_flat_on_constant_input() {
        let d = dema(&[7.0; 20], 7);
        for v in d {
            assert!((v - 7.0).abs() < 1e-12);
        }
    }

    #[test]
    fn sma_hand_calc() {
        let s = sma(&[1.0, 2.0, 3.0, 4.0], 3);
        assert_eq!(s[0], None);
        assert_eq!(s[1], None);
        assert!((s[2].unwrap_or_default() - 2.0).abs() < 1e-12);
        assert!((s[3].unwrap_or_default() - 3.0).abs() < 1e-12);
    }

    #[test]
    fn stdev_pop_hand_calc() {
        // Window [2, 4]: mean 3, population variance ((1)+(1))/2 = 1 → sd 1.
        let s = stdev_pop(&[2.0, 4.0], 2);
        assert!((s[1].unwrap_or_default() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rsi_monotonic_up_is_100() {
        let src: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let r = rsi(&src, 6);
        assert_eq!(r[5], None);
        assert!((r[6].unwrap_or_default() - 100.0).abs() < 1e-9);
        assert!((r[29].unwrap_or_default() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn rsi_alternating_hand_check_stays_bounded() {
        let src: Vec<f64> = (0..40)
            .map(|i| if i % 2 == 0 { 100.0 } else { 101.0 })
            .collect();
        let r = rsi(&src, 14);
        let last = r.last().copied().flatten().unwrap_or_default();
        assert!(last > 40.0 && last < 60.0, "got {last}");
    }

    #[test]
    fn true_range_uses_gaps() {
        // Bar1 gaps below: high 90, low 85, prev close 100 → tr = 15.
        let tr = true_range(&[100.0, 90.0], &[95.0, 85.0], &[100.0, 88.0]);
        assert_eq!(tr[0], None);
        assert!((tr[1].unwrap_or_default() - 15.0).abs() < 1e-12);
    }

    #[test]
    fn highest_lowest_window() {
        let h = highest(&[1.0, 5.0, 3.0, 2.0], 3);
        let l = lowest(&[1.0, 5.0, 3.0, 2.0], 3);
        assert_eq!(h[1], None);
        assert!((h[2].unwrap_or_default() - 5.0).abs() < 1e-12);
        assert!((h[3].unwrap_or_default() - 5.0).abs() < 1e-12);
        assert!((l[2].unwrap_or_default() - 1.0).abs() < 1e-12);
        assert!((l[3].unwrap_or_default() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn cross_helpers_match_pine_definition() {
        let a: Vec<Option<f64>> = vec![Some(1.0), Some(3.0), Some(2.0)];
        let b: Vec<Option<f64>> = vec![Some(2.0), Some(2.0), Some(2.5)];
        assert!(crossover_at(&a, &b, 1)); // 1<=2 then 3>2
        assert!(!crossover_at(&a, &b, 2));
        assert!(crossunder_at(&a, &b, 2)); // 3>=2 then 2<2.5
    }
}
