//! DSS Bressert — Double Smoothed Stochastic (William Blau / Walter Bressert).
//! Faithful port of `docs/reference/dss-bressert.pine`.
//!
//! ```text
//! stoch(c,h,l,n) = 100 * (c - lowest(l,n)) / (highest(h,n) - lowest(l,n))
//! xPreCalc = ema(stoch(close, high, low, PDS), EMAlen)
//! xDSS     = ema(stoch(xPreCalc, xPreCalc, xPreCalc, PDS), EMAlen)   // double-smoothed
//! xTrigger = ema(xDSS, TriggerLen)
//! ```
//!
//! The second `stoch` deliberately uses `xPreCalc` as ALL THREE of c/h/l — a
//! self-referential stochastic of the pre-calc line (replicated exactly).
//! Bottom = `xDSS` turns up AND crosses above `xTrigger`, usually from <20.
//!
//! Defaults: PDS=10, EMAlen=9, TriggerLen=5, Overbought=80, Oversold=20.
//! All math is `f64`; values are oscillator readings bounded to [0, 100].

/// Pine `ema(src, len)`: SMA-seeded EMA, leading `None` until the seed bar.
/// Re-implemented locally (rather than importing `indicators::ema`) so the
/// warm-up semantics here are explicit and the recurrence is self-contained.
fn ema_opt(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    // Pine's `ema` runs on a continuous series; our `stoch` may emit leading
    // `None` during its own warm-up. We seed the EMA on the FIRST run of
    // `period` consecutive finite values (mirroring Pine, where the stoch is
    // finite from bar 0 because lowest/highest collapse to the bar itself).
    let n = values.len();
    let mut out = vec![None; n];
    if period == 0 {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    // Find first index where a window of `period` finite values ends.
    let mut prev: Option<f64> = None;
    let mut run = 0usize;
    let mut sum = 0.0;
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(x) => {
                if prev.is_none() {
                    run += 1;
                    sum += *x;
                    if run == period {
                        let seed = sum / period as f64;
                        out[i] = Some(seed);
                        prev = Some(seed);
                    }
                } else {
                    let p = prev.unwrap();
                    let cur = (*x - p) * k + p;
                    out[i] = Some(cur);
                    prev = Some(cur);
                }
            }
            None => {
                // Reset the warm-up run if we hit a gap before seeding.
                if prev.is_none() {
                    run = 0;
                    sum = 0.0;
                }
            }
        }
    }
    out
}

/// `stoch(c, h, l, n)` = 100*(c - lowest(l,n)) / (highest(h,n) - lowest(l,n)).
/// `lowest`/`highest` are over the trailing `n` bars INCLUDING the current
/// (Pine semantics). During warm-up (`i < n-1`) the window shrinks to `i+1`
/// bars (Pine's `ta.lowest` does the same on early bars). Flat windows
/// (high==low) yield 50.0 (neutral) — Pine would emit `na`/0; 50 is the
/// stable neutral that keeps the EMA chain finite on pathological flats.
fn stoch(c: &[f64], h: &[f64], l: &[f64], n: usize) -> Vec<Option<f64>> {
    let len = c.len();
    let mut out = vec![None; len];
    if n == 0 {
        return out;
    }
    for i in 0..len {
        let start = i.saturating_sub(n - 1);
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for j in start..=i {
            if l[j] < lo {
                lo = l[j];
            }
            if h[j] > hi {
                hi = h[j];
            }
        }
        let denom = hi - lo;
        let val = if denom.abs() < f64::EPSILON {
            50.0
        } else {
            (100.0 * (c[i] - lo) / denom).clamp(0.0, 100.0)
        };
        out[i] = Some(val);
    }
    out
}

/// Computed DSS read: the two series of interest (xDSS and the trigger),
/// each `Option<f64>` per bar mirroring Pine warm-up.
#[derive(Debug, Clone)]
pub struct DssSeries {
    pub dss: Vec<Option<f64>>,
    pub trigger: Vec<Option<f64>>,
}

/// Compute DSS + trigger over OHLC closes/highs/lows (oldest-first).
/// Returns `None` if the series is shorter than the stochastic period.
///
/// Defaults: PDS=10, EMAlen=9, TriggerLen=5.
pub fn compute_dss(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    pds: usize,
    ema_len: usize,
    trigger_len: usize,
) -> Option<DssSeries> {
    let n = close.len();
    if n < pds || pds == 0 || ema_len == 0 || trigger_len == 0 {
        return None;
    }
    if high.len() != n || low.len() != n {
        return None;
    }
    // xPreCalc = ema(stoch(close, high, low, PDS), EMAlen)
    let s1 = stoch(close, high, low, pds);
    let pre = ema_opt(&s1, ema_len);
    // Materialise pre as a plain f64 series for the second stoch, carrying
    // None forward (Pine's pre is finite from its seed bar onward).
    let pre_vals: Vec<f64> = pre.iter().map(|v| v.unwrap_or(0.0)).collect();
    // xDSS = ema(stoch(xPreCalc, xPreCalc, xPreCalc, PDS), EMAlen)
    // Self-referential: pre is used as c, h AND l. Only meaningful once pre
    // is seeded; mask the stoch with None where pre is None.
    let mut s2 = stoch(&pre_vals, &pre_vals, &pre_vals, pds);
    for (i, p) in pre.iter().enumerate() {
        if p.is_none() {
            s2[i] = None;
        }
    }
    let dss = ema_opt(&s2, ema_len);
    // xTrigger = ema(xDSS, TriggerLen)
    let trigger = ema_opt(&dss, trigger_len);
    Some(DssSeries { dss, trigger })
}

/// Compute with the Pine defaults (PDS=10, EMAlen=9, TriggerLen=5).
pub fn compute_dss_default(close: &[f64], high: &[f64], low: &[f64]) -> Option<DssSeries> {
    compute_dss(close, high, low, 10, 9, 5)
}

/// Latest DSS value.
pub fn current_dss(s: &DssSeries) -> Option<f64> {
    s.dss.last().copied().flatten()
}

/// Latest trigger value.
pub fn current_trigger(s: &DssSeries) -> Option<f64> {
    s.trigger.last().copied().flatten()
}

/// True when xDSS ticked up on the latest bar (`dss[0] > dss[1]`).
pub fn turned_up(s: &DssSeries) -> Option<bool> {
    last_two(&s.dss).map(|(prev, cur)| cur > prev)
}

/// True when xDSS ticked down on the latest bar (`dss[0] < dss[1]`).
pub fn turned_down(s: &DssSeries) -> Option<bool> {
    last_two(&s.dss).map(|(prev, cur)| cur < prev)
}

/// True when xDSS crossed ABOVE its trigger on the latest bar
/// (`dss[1] <= trigger[1]` and `dss[0] > trigger[0]`).
pub fn crossed_above_trigger(s: &DssSeries) -> Option<bool> {
    let (d_prev, d_cur) = last_two(&s.dss)?;
    let (t_prev, t_cur) = last_two(&s.trigger)?;
    Some(d_prev <= t_prev && d_cur > t_cur)
}

/// True when xDSS crossed BELOW its trigger on the latest bar
/// (`dss[1] >= trigger[1]` and `dss[0] < trigger[0]`).
pub fn crossed_below_trigger(s: &DssSeries) -> Option<bool> {
    let (d_prev, d_cur) = last_two(&s.dss)?;
    let (t_prev, t_cur) = last_two(&s.trigger)?;
    Some(d_prev >= t_prev && d_cur < t_cur)
}

/// True when the latest xDSS is oversold (< `oversold`, default 20).
pub fn is_oversold(s: &DssSeries, oversold: f64) -> Option<bool> {
    current_dss(s).map(|v| v < oversold)
}

/// True when the latest xDSS is overbought (> `overbought`, default 80).
pub fn is_overbought(s: &DssSeries, overbought: f64) -> Option<bool> {
    current_dss(s).map(|v| v > overbought)
}

/// Helper: the last two finite values of an `Option` series, as (prev, cur).
fn last_two(series: &[Option<f64>]) -> Option<(f64, f64)> {
    let n = series.len();
    if n < 2 {
        return None;
    }
    let cur = series[n - 1]?;
    let prev = series[n - 2]?;
    Some((prev, cur))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stoch_basic_bounds_and_value() {
        // close=high=low single-value windows are flat -> neutral 50.
        let c = vec![10.0, 10.0, 10.0];
        let s = stoch(&c, &c, &c, 3);
        assert_eq!(s[2], Some(50.0));
        // A rising series: last close at the top of its window -> ~100.
        let close = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let s = stoch(&close, &close, &close, 5);
        assert_eq!(s[4], Some(100.0)); // 100*(5-1)/(5-1)
                                       // A mid value.
        let close = vec![1.0, 5.0, 3.0];
        let s = stoch(&close, &close, &close, 3);
        // window [1,5,3]: lo=1, hi=5, c=3 -> 100*(3-1)/(5-1) = 50
        assert_eq!(s[2], Some(50.0));
    }

    #[test]
    fn dss_in_zero_to_hundred() {
        let n = 200usize;
        let close: Vec<f64> = (0..n)
            .map(|i| 100.0 + 10.0 * (i as f64 / 9.0).sin())
            .collect();
        let s = compute_dss_default(&close, &close, &close).expect("dss");
        for v in s.dss.iter().flatten() {
            assert!((0.0..=100.0).contains(v), "dss out of range: {v}");
        }
        for v in s.trigger.iter().flatten() {
            assert!((0.0..=100.0).contains(v), "trigger out of range: {v}");
        }
    }

    #[test]
    fn too_short_returns_none() {
        let c = vec![1.0, 2.0, 3.0];
        assert!(compute_dss(&c, &c, &c, 10, 9, 5).is_none());
    }

    #[test]
    fn v_bottom_fires_turn_up_and_cross_from_oversold() {
        // Long decline drives DSS to oversold, then a sharp reversal makes it
        // turn up and cross above its (lagging) trigger.
        let mut close: Vec<f64> = (0..120).map(|i| 200.0 - i as f64 * 1.2).collect();
        let base = *close.last().unwrap();
        for j in 1..=30 {
            close.push(base + j as f64 * 2.4);
        }
        let s = compute_dss_default(&close, &close, &close).expect("dss");
        // After the rally the DSS must be rising.
        assert_eq!(
            turned_up(&s),
            Some(true),
            "DSS should turn up after V-bottom"
        );
        // And it should be above the trigger now (it crossed during the rally).
        let d = current_dss(&s).unwrap();
        let t = current_trigger(&s).unwrap();
        assert!(d > t, "DSS {d} should be above trigger {t} post-reversal");
    }

    #[test]
    fn blowoff_top_fires_turn_down_and_cross_from_overbought() {
        let mut close: Vec<f64> = (0..120).map(|i| 100.0 + i as f64 * 1.2).collect();
        let peak = *close.last().unwrap();
        for j in 1..=40 {
            close.push(peak - j as f64 * 2.0);
        }
        let s = compute_dss_default(&close, &close, &close).expect("dss");
        assert!(
            s.dss.iter().flatten().any(|v| *v > 80.0),
            "fixture should visit overbought"
        );
        let mut down = false;
        let mut crossed = false;
        for end in 120..=close.len() {
            if let Some(sub) = compute_dss_default(&close[..end], &close[..end], &close[..end]) {
                down |= turned_down(&sub) == Some(true);
                crossed |= crossed_below_trigger(&sub) == Some(true);
            }
        }
        assert!(down, "DSS should turn down after a top");
        assert!(crossed, "DSS should cross below trigger after a top");
    }

    #[test]
    fn oversold_flag_at_bottom() {
        // Pure decline: DSS pinned near 0.
        let close: Vec<f64> = (0..150).map(|i| 500.0 - i as f64 * 2.0).collect();
        let s = compute_dss_default(&close, &close, &close).expect("dss");
        assert_eq!(
            is_oversold(&s, 20.0),
            Some(true),
            "DSS should be oversold in a downtrend"
        );
    }

    #[test]
    fn default_dss_golden_tail_on_wave_trend_fixture() {
        let close: Vec<f64> = (0..180)
            .map(|i| {
                let x = i as f64;
                100.0 + 0.07 * x + 6.0 * (x / 5.0).sin() + 2.0 * (x / 17.0).cos()
            })
            .collect();
        let s = compute_dss_default(&close, &close, &close).expect("dss");
        let expected_dss = [
            20.685811380499,
            16.548649104399,
            13.238919283520,
            10.591135426816,
            8.472908341452,
        ];
        let expected_trigger = [
            35.315531411708,
            29.059903975938,
            23.786242411799,
            19.387873416804,
            15.749551725020,
        ];
        let dss_tail = &s.dss[s.dss.len() - expected_dss.len()..];
        let trigger_tail = &s.trigger[s.trigger.len() - expected_trigger.len()..];
        for (got, expected) in dss_tail.iter().zip(expected_dss) {
            let got = got.expect("dss value");
            assert!(
                (got - expected).abs() < 1e-9,
                "golden DSS tail changed: got {got}, expected {expected}"
            );
        }
        for (got, expected) in trigger_tail.iter().zip(expected_trigger) {
            let got = got.expect("trigger value");
            assert!(
                (got - expected).abs() < 1e-9,
                "golden DSS trigger tail changed: got {got}, expected {expected}"
            );
        }
    }
}
