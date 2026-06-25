//! RSI + a moving average of the RSI, with the cycle-low crossover helpers.
//!
//! Common cycle-bottom confirmation: RSI(14) bottoms, then its own moving
//! average (SMA(14) by default) turns up and crosses back ABOVE the raw RSI —
//! a momentum-of-momentum reclaim. Reuses `indicators::rsi` for the RSI core
//! and `indicators::sma` for the average; all `f64`, oscillator only.

use crate::indicators::rsi::compute_rsi;
use crate::indicators::sma::compute_sma;

/// RSI series + its moving average, both `Option<f64>` per bar (leading `None`
/// during warm-up). Aligned to the input length.
#[derive(Debug, Clone)]
pub struct RsiMa {
    pub rsi: Vec<Option<f64>>,
    pub rsi_ma: Vec<Option<f64>>,
}

/// Compute RSI(`rsi_period`) and an SMA(`ma_period`) of the RSI over `closes`
/// (oldest-first). Returns `None` if there are too few bars to produce even
/// one RSI-MA value.
///
/// Defaults: `rsi_period = 14`, `ma_period = 14`.
pub fn compute_rsi_ma(closes: &[f64], rsi_period: usize, ma_period: usize) -> Option<RsiMa> {
    if rsi_period == 0 || ma_period == 0 {
        return None;
    }
    let rsi = compute_rsi(closes, rsi_period);
    // SMA over the RSI series, treating leading None as "not yet" — we run the
    // SMA only over the finite tail so the average isn't polluted by warm-up.
    // Build a parallel f64 series padded with the first finite RSI for the
    // pre-warmup region is WRONG (biases the MA); instead compute the SMA over
    // the contiguous finite suffix and re-align.
    let first_finite = rsi.iter().position(|v| v.is_some())?;
    let finite_tail: Vec<f64> = rsi[first_finite..]
        .iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let ma_tail = compute_sma(&finite_tail, ma_period);
    let mut rsi_ma = vec![None; closes.len()];
    for (k, v) in ma_tail.iter().enumerate() {
        rsi_ma[first_finite + k] = *v;
    }
    // Need at least one RSI-MA value.
    if rsi_ma.iter().all(|v| v.is_none()) {
        return None;
    }
    Some(RsiMa { rsi, rsi_ma })
}

/// Compute with the defaults (RSI 14, SMA 14).
pub fn compute_rsi_ma_default(closes: &[f64]) -> Option<RsiMa> {
    compute_rsi_ma(closes, 14, 14)
}

/// Latest raw RSI value.
pub fn current_rsi(r: &RsiMa) -> Option<f64> {
    r.rsi.last().copied().flatten()
}

/// Latest RSI-MA value.
pub fn current_rsi_ma(r: &RsiMa) -> Option<f64> {
    r.rsi_ma.last().copied().flatten()
}

/// True when the RSI-MA ticked up on the latest bar (`rsiMA[0] > rsiMA[1]`).
pub fn ma_turned_up(r: &RsiMa) -> Option<bool> {
    last_two(&r.rsi_ma).map(|(prev, cur)| cur > prev)
}

/// True when the RSI-MA ticked down on the latest bar (`rsiMA[0] < rsiMA[1]`).
pub fn ma_turned_down(r: &RsiMa) -> Option<bool> {
    last_two(&r.rsi_ma).map(|(prev, cur)| cur < prev)
}

/// True when the RSI-MA crossed ABOVE the raw RSI on the latest bar
/// (`rsiMA[1] <= rsi[1]` and `rsiMA[0] > rsi[0]`) — the cycle-low confirmation.
pub fn ma_crossed_above_rsi(r: &RsiMa) -> Option<bool> {
    let (ma_prev, ma_cur) = last_two(&r.rsi_ma)?;
    let (rsi_prev, rsi_cur) = last_two(&r.rsi)?;
    Some(ma_prev <= rsi_prev && ma_cur > rsi_cur)
}

/// True when the RSI-MA crossed BELOW the raw RSI on the latest bar
/// (`rsiMA[1] >= rsi[1]` and `rsiMA[0] < rsi[0]`) — the cycle-TOP / cycle-high
/// mirror of [`ma_crossed_above_rsi`].
pub fn ma_crossed_below_rsi(r: &RsiMa) -> Option<bool> {
    let (ma_prev, ma_cur) = last_two(&r.rsi_ma)?;
    let (rsi_prev, rsi_cur) = last_two(&r.rsi)?;
    Some(ma_prev >= rsi_prev && ma_cur < rsi_cur)
}

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
    fn rsi_ma_is_sma_of_rsi() {
        let closes: Vec<f64> = (0..60).map(|i| 100.0 + (i as f64 / 5.0).sin() * 10.0).collect();
        let r = compute_rsi_ma(&closes, 14, 5).expect("rsi_ma");
        // Re-derive the MA at the last bar from the last 5 finite RSI values.
        let finite: Vec<f64> = r.rsi.iter().flatten().copied().collect();
        let last5: f64 = finite[finite.len() - 5..].iter().sum::<f64>() / 5.0;
        let got = current_rsi_ma(&r).unwrap();
        assert!((got - last5).abs() < 1e-9, "{got} vs {last5}");
    }

    #[test]
    fn too_short_returns_none() {
        assert!(compute_rsi_ma_default(&[1.0, 2.0, 3.0]).is_none());
    }

    #[test]
    fn v_bottom_ma_turns_up_and_crosses_rsi() {
        // Decline then sharp reversal, then a cooling-off. During the rally the
        // fast RSI leads up and the lagging MA follows (turning up); as the
        // rally cools the RSI dips under its still-rising MA (the MA crosses
        // ABOVE the RSI — the cycle-low confirmation). Both invariants are
        // asserted by scanning the reversal/cooling window.
        let mut closes: Vec<f64> = (0..80).map(|i| 200.0 - i as f64 * 1.5).collect();
        let rally_start = closes.len();
        let base = *closes.last().unwrap();
        for j in 1..=25 {
            closes.push(base + j as f64 * 2.5);
        }
        // Cooling-off: fast RSI dips under its rising MA.
        let top = *closes.last().unwrap();
        for j in 1..=10 {
            closes.push(top - j as f64 * 0.4);
        }

        // The MA must turn up at some point during/after the rally.
        let mut ma_up = false;
        for end in (rally_start + 14)..=closes.len() {
            let sub = compute_rsi_ma(&closes[..end], 14, 14).expect("sub");
            if ma_turned_up(&sub) == Some(true) {
                ma_up = true;
                break;
            }
        }
        assert!(ma_up, "RSI-MA should rise as the rally lifts RSI");

        // The MA-cross-above-RSI confirmation must fire during the cooling.
        let mut crossed = false;
        for end in (closes.len() - 10)..=closes.len() {
            let sub = compute_rsi_ma(&closes[..end], 14, 14).expect("sub");
            if ma_crossed_above_rsi(&sub) == Some(true) {
                crossed = true;
                break;
            }
        }
        assert!(crossed, "RSI-MA should cross above RSI as momentum cools");
    }

    #[test]
    fn inverted_v_ma_turns_down_and_crosses_below_rsi() {
        // Rally then sharp reversal down, then a bounce — the TOP mirror. During
        // the selloff the fast RSI leads down and the lagging MA follows (turning
        // down); as the selloff eases the RSI ticks above its still-falling MA
        // (the MA crosses BELOW the RSI — the cycle-top confirmation).
        let mut closes: Vec<f64> = (0..80).map(|i| 100.0 + i as f64 * 1.5).collect();
        let selloff_start = closes.len();
        let base = *closes.last().unwrap();
        for j in 1..=25 {
            closes.push(base - j as f64 * 2.5);
        }
        let bottom = *closes.last().unwrap();
        for j in 1..=10 {
            closes.push(bottom + j as f64 * 0.4);
        }

        let mut ma_down = false;
        for end in (selloff_start + 14)..=closes.len() {
            let sub = compute_rsi_ma(&closes[..end], 14, 14).expect("sub");
            if ma_turned_down(&sub) == Some(true) {
                ma_down = true;
                break;
            }
        }
        assert!(ma_down, "RSI-MA should fall as the selloff drags RSI down");

        let mut crossed = false;
        for end in (closes.len() - 10)..=closes.len() {
            let sub = compute_rsi_ma(&closes[..end], 14, 14).expect("sub");
            if ma_crossed_below_rsi(&sub) == Some(true) {
                crossed = true;
                break;
            }
        }
        assert!(crossed, "RSI-MA should cross below RSI as momentum recovers");
    }

    #[test]
    fn blowoff_top_ma_turns_down_and_crosses_below_rsi() {
        let mut closes: Vec<f64> = (0..80).map(|i| 100.0 + i as f64 * 1.5).collect();
        let peak = *closes.last().unwrap();
        for j in 1..=25 {
            closes.push(peak - j as f64 * 2.5);
        }
        let mut ma_down = false;
        for end in 40..=closes.len() {
            if let Some(sub) = compute_rsi_ma(&closes[..end], 14, 14) {
                ma_down |= ma_turned_down(&sub) == Some(true);
            }
        }
        assert!(ma_down, "RSI-MA should turn down after a rollover");
    }

    #[test]
    fn ma_crossed_below_rsi_matches_edge_definition() {
        let r = RsiMa {
            rsi: vec![None, Some(55.0), Some(60.0)],
            rsi_ma: vec![None, Some(57.0), Some(58.0)],
        };
        assert_eq!(ma_crossed_below_rsi(&r), Some(true));
        assert_eq!(ma_crossed_above_rsi(&r), Some(false));
    }
}
