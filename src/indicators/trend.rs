//! Trend-strength indicator — ADX / DMI (Wilder's Directional Movement).
//!
//! Pure function over price slices. ADX measures trend STRENGTH (not
//! direction); +DI/−DI give the directional bias. Classic reading: ADX > 25 =
//! trending, < 20 = ranging.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdxResult {
    pub adx: f64,
    pub plus_di: f64,
    pub minus_di: f64,
}

/// Wilder ADX/DMI over `period` (typically 14). First valid value appears after
/// ~2·period bars (one smoothing for DI, one for ADX).
pub fn compute_adx(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<AdxResult>> {
    let n = closes.len();
    let mut out = vec![None; n];
    if period == 0 || n < 2 * period + 1 {
        return out;
    }
    // Per-bar TR, +DM, −DM.
    let mut tr = vec![0.0; n];
    let mut plus_dm = vec![0.0; n];
    let mut minus_dm = vec![0.0; n];
    for i in 1..n {
        let up = highs[i] - highs[i - 1];
        let down = lows[i - 1] - lows[i];
        plus_dm[i] = if up > down && up > 0.0 { up } else { 0.0 };
        minus_dm[i] = if down > up && down > 0.0 { down } else { 0.0 };
        let hl = highs[i] - lows[i];
        let hc = (highs[i] - closes[i - 1]).abs();
        let lc = (lows[i] - closes[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }
    // Wilder-smoothed sums seeded by the first `period` sum.
    let pf = period as f64;
    let mut atr = tr[1..=period].iter().sum::<f64>();
    let mut sp = plus_dm[1..=period].iter().sum::<f64>();
    let mut sm = minus_dm[1..=period].iter().sum::<f64>();
    let mut dx_series = vec![None; n];
    for i in (period + 1)..n {
        atr = atr - atr / pf + tr[i];
        sp = sp - sp / pf + plus_dm[i];
        sm = sm - sm / pf + minus_dm[i];
        if atr <= 0.0 {
            continue;
        }
        let plus_di = 100.0 * sp / atr;
        let minus_di = 100.0 * sm / atr;
        let denom = plus_di + minus_di;
        let dx = if denom > 0.0 {
            100.0 * (plus_di - minus_di).abs() / denom
        } else {
            0.0
        };
        dx_series[i] = Some((dx, plus_di, minus_di));
    }
    // ADX = Wilder average of DX. Seed with the mean of the first `period` DX.
    let first_dx: Vec<(usize, f64)> = dx_series
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|(dx, _, _)| (i, dx)))
        .collect();
    if first_dx.len() < period {
        return out;
    }
    let seed_idx = first_dx[period - 1].0;
    let mut adx = first_dx[..period].iter().map(|(_, dx)| dx).sum::<f64>() / pf;
    if let Some((_, p, m)) = dx_series[seed_idx] {
        out[seed_idx] = Some(AdxResult {
            adx,
            plus_di: p,
            minus_di: m,
        });
    }
    for i in (seed_idx + 1)..n {
        if let Some((dx, p, m)) = dx_series[i] {
            adx = (adx * (pf - 1.0) + dx) / pf;
            out[i] = Some(AdxResult {
                adx,
                plus_di: p,
                minus_di: m,
            });
        }
    }
    out
}

/// One Supertrend reading: the trailing-stop `line` and the regime `dir`
/// (+1 = uptrend / line below price, −1 = downtrend / line above price).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SupertrendResult {
    pub line: f64,
    pub dir: i8,
}

/// Supertrend — an ATR-banded trailing-stop trend follower (Olivier Seban).
///
/// Bands are `hl2 ± multiplier·ATR(period)`; the active band ratchets one way
/// (the upper band only moves down, the lower only moves up) until price closes
/// through it, which flips the regime and swaps the active band. The returned
/// `line` is the side currently acting as the trailing stop; `dir` is +1 while
/// price holds above the lower band, −1 while it sits below the upper band.
///
/// Warmup follows ATR: `None` until the first ATR value (bar `period-1`).
///
/// This is the **Everget v3** variant: Wilder-smoothed ATR and a flip check
/// against the *current* (just-ratcheted) band (`close > final_upper`). Note a
/// SECOND, intentionally distinct Supertrend port lives in
/// [`crate::analytics::cyber::dots`] (CyberDots): SMA-smoothed range and a flip
/// check against the *previous* band (`close > prev_dn`). They can disagree by
/// a bar when ATR expands on the flip bar — by design; don't "reconcile" one to
/// the other. This DSL primitive (`supertrend`/`supertrend_dir`) uses THIS one.
pub fn compute_supertrend(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    multiplier: f64,
) -> Vec<Option<SupertrendResult>> {
    let n = closes.len();
    let mut out = vec![None; n];
    if period == 0 || multiplier <= 0.0 || n < period {
        return out;
    }
    let h_opt: Vec<Option<f64>> = highs.iter().map(|v| Some(*v)).collect();
    let l_opt: Vec<Option<f64>> = lows.iter().map(|v| Some(*v)).collect();
    let atr = crate::indicators::atr::compute_atr(&h_opt, &l_opt, closes, period);

    // Running FINAL bands (the ratcheted versions) and prior regime.
    let mut final_upper = 0.0_f64;
    let mut final_lower = 0.0_f64;
    let mut prev_dir: i8 = 0;
    let mut started = false;
    for i in 0..n {
        let a = match atr[i] {
            Some(a) => a,
            None => continue,
        };
        let hl2 = (highs[i] + lows[i]) / 2.0;
        let basic_upper = hl2 + multiplier * a;
        let basic_lower = hl2 - multiplier * a;
        if !started {
            // Seed: pick the regime from where the close sits relative to the
            // first bands; the inactive band is carried until it ratchets.
            final_upper = basic_upper;
            final_lower = basic_lower;
            let (line, dir) = if closes[i] <= basic_upper {
                (basic_upper, -1)
            } else {
                (basic_lower, 1)
            };
            prev_dir = dir;
            out[i] = Some(SupertrendResult { line, dir });
            started = true;
            continue;
        }
        // Ratchet: the upper band only tightens (moves down) unless the prior
        // close broke above it; symmetric for the lower band.
        let fu = if basic_upper < final_upper || closes[i - 1] > final_upper {
            basic_upper
        } else {
            final_upper
        };
        let fl = if basic_lower > final_lower || closes[i - 1] < final_lower {
            basic_lower
        } else {
            final_lower
        };
        let (line, dir) = if prev_dir < 0 {
            // Was downtrend (line = upper): a close above the upper band flips up.
            if closes[i] > fu {
                (fl, 1)
            } else {
                (fu, -1)
            }
        } else {
            // Was uptrend (line = lower): a close below the lower band flips down.
            if closes[i] < fl {
                (fu, -1)
            } else {
                (fl, 1)
            }
        };
        final_upper = fu;
        final_lower = fl;
        prev_dir = dir;
        out[i] = Some(SupertrendResult { line, dir });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adx_high_in_strong_uptrend() {
        // Steady uptrend -> +DI > −DI and a high ADX.
        let n = 80;
        let highs: Vec<f64> = (0..n).map(|i| 100.0 + 2.0 * i as f64).collect();
        let lows: Vec<f64> = (0..n).map(|i| 98.0 + 2.0 * i as f64).collect();
        let closes: Vec<f64> = (0..n).map(|i| 99.5 + 2.0 * i as f64).collect();
        let a = compute_adx(&highs, &lows, &closes, 14);
        let last = a.last().unwrap().unwrap();
        assert!(last.plus_di > last.minus_di, "+DI should lead in an uptrend");
        assert!(last.adx > 25.0, "strong trend adx={}", last.adx);
    }

    #[test]
    fn adx_none_during_warmup_and_short_series() {
        let v = vec![100.0; 10];
        assert!(compute_adx(&v, &v, &v, 14).iter().all(|x| x.is_none()));
    }

    #[test]
    fn supertrend_uptrend_line_below_price() {
        // Steady uptrend → regime +1 and the trailing line sits BELOW the close.
        let n = 60;
        let highs: Vec<f64> = (0..n).map(|i| 102.0 + i as f64).collect();
        let lows: Vec<f64> = (0..n).map(|i| 98.0 + i as f64).collect();
        let closes: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let st = compute_supertrend(&highs, &lows, &closes, 10, 3.0);
        let last = st.last().unwrap().unwrap();
        assert_eq!(last.dir, 1, "uptrend should read dir=+1");
        assert!(last.line < *closes.last().unwrap(), "uptrend line must be below price");
        // Warmup: first period-1 bars are None.
        assert!(st[..9].iter().all(|x| x.is_none()));
    }

    #[test]
    fn supertrend_flips_on_trend_reversal() {
        // Up then sharply down → the regime must flip from +1 to −1, and once
        // in the downtrend the line sits ABOVE price.
        let mut highs = Vec::new();
        let mut lows = Vec::new();
        let mut closes = Vec::new();
        for i in 0..40 {
            highs.push(102.0 + i as f64);
            lows.push(98.0 + i as f64);
            closes.push(100.0 + i as f64);
        }
        for i in 0..40 {
            let base = 140.0 - 2.0 * i as f64;
            highs.push(base + 2.0);
            lows.push(base - 2.0);
            closes.push(base);
        }
        let st = compute_supertrend(&highs, &lows, &closes, 10, 3.0);
        let dirs: Vec<i8> = st.iter().filter_map(|o| o.map(|r| r.dir)).collect();
        assert!(dirs.contains(&1) && dirs.contains(&-1), "should see both regimes");
        let last = st.last().unwrap().unwrap();
        assert_eq!(last.dir, -1, "ends in a downtrend");
        assert!(last.line > *closes.last().unwrap(), "downtrend line must be above price");
        // Pin the flip-bar INDICES so a one-bar-early/late regression fails
        // (the regime-presence checks above would pass either way). For this
        // exact series: seed downtrend at bar 9, up-flip at 22, down-flip at 47.
        let flips: Vec<(usize, i8)> = st
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.map(|r| (i, r.dir)))
            .scan(0i8, |prev, (i, d)| {
                let changed = d != *prev;
                *prev = d;
                Some((i, d, changed))
            })
            .filter_map(|(i, d, changed)| changed.then_some((i, d)))
            .collect();
        assert_eq!(flips, vec![(9, -1), (22, 1), (47, -1)], "flip bars drifted: {flips:?}");
    }

    #[test]
    fn supertrend_guards_bad_params() {
        let v = vec![100.0; 30];
        // Bad params → all-None.
        assert!(compute_supertrend(&v, &v, &v, 0, 3.0).iter().all(|x| x.is_none()));
        assert!(compute_supertrend(&v, &v, &v, 10, 0.0).iter().all(|x| x.is_none()));
        assert!(compute_supertrend(&v, &v, &v, 10, -1.0).iter().all(|x| x.is_none()));
        // Valid params on a non-trivial series → None strictly before the ATR
        // warmup boundary (bar period-1) and Some from there on (no NaN/inf).
        let highs: Vec<f64> = (0..30).map(|i| 102.0 + i as f64).collect();
        let lows: Vec<f64> = (0..30).map(|i| 98.0 + i as f64).collect();
        let closes: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let st = compute_supertrend(&highs, &lows, &closes, 10, 3.0);
        assert_eq!(st.len(), 30);
        assert!(st[..9].iter().all(|x| x.is_none()), "None before warmup boundary");
        assert!(
            st[9..].iter().all(|x| x.map(|r| r.line.is_finite()).unwrap_or(false)),
            "finite line from the warmup boundary onward"
        );
    }
}
