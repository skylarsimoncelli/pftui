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
}
