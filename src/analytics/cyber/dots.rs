//! CyberDots — component C of the Cyber Dots port (micro-trend strength dots).
//!
//! Pine mapping (`cyberDots*` block):
//! - SuperTrend: `hl2` source, ATR = `ta.sma(ta.tr, period)` — **SMA of true
//!   range, not Wilder RMA** — with the exact band-ratchet semantics
//!   (`supUp := close[1] > nz(supUp[1]) ? max(supUp, nz(supUp[1])) : supUp`,
//!   mirror for the lower band). Daily and weekly both map to period 12 /
//!   multiplier 1.3 in the Pine timeframe ternary.
//! - Direction state machine: `dir := dir == −1 and close > nz(supDn[1]) ? 1
//!   : dir == 1 and close < nz(supUp[1]) ? −1 : dir`.
//!   **Documented adaptation:** the Pine seeds with `dir := nz(dir[1])`,
//!   which evaluates to 0 on the first bar (`nz(na) = 0`) — and because the
//!   transitions only fire from the ±1 states, a literal port would pin
//!   `dir ≡ 0` forever, killing the third strength condition. The canonical
//!   SuperTrend this block derives from seeds `nz(dir[1], dir)` (= 1 on bar
//!   0); we seed `dir = 1`, which is what the rest of the script (and the
//!   indicator's published description) assumes. dir ∈ {1, −1} thereafter.
//! - VMA(4): the same VIDYA recursion as CyberLine, on close
//!   ([`super::line::vidya_series`] with len 4).
//! - SMA(18) on close.
//! - Medium sensitivity thresholds: VMA distance ≥ 0.15%, SMA distance ≥
//!   0.20%, minimum strength 2. Strength = count of (beyond-VMA-with-
//!   distance, beyond-SMA-with-distance, supertrend-direction) conditions; a
//!   dot fires when strength ≥ min.

use serde::Serialize;

use super::line::vidya_series;
use super::primitives::{self, round_level};

const SUPER_PERIOD: usize = 12; // Pine: 'D'/'1W' ⇒ 12
const SUPER_MULT: f64 = 1.3; // Pine: 'D'/'1W' ⇒ 1.3
const VMA_LEN: usize = 4;
const MA_LEN: usize = 18;
// Medium sensitivity (defaults).
const VMA_THRESHOLD_PCT: f64 = 0.15;
const MA_THRESHOLD_PCT: f64 = 0.20;
const MIN_STRENGTH: u8 = 2;

/// A bar where a dot was active. Only onsets are emitted as events (a dot
/// "fires" on the first bar of a run — Pine plots a shape on every active
/// bar, which would flood a dated signal list).
#[derive(Debug, Clone, Serialize)]
pub struct DotEvent {
    pub date: String,
    /// "up" or "down".
    pub direction: String,
    /// Strength 1–3 on the onset bar.
    pub strength: u8,
}

/// CyberDots read for the latest bar.
#[derive(Debug, Clone, Serialize)]
pub struct DotsRead {
    /// SuperTrend direction on the latest bar: 1 bullish, −1 bearish.
    pub supertrend_dir: i8,
    /// SuperTrend active stop level (lower band when long, upper when short).
    pub supertrend_stop: Option<f64>,
    pub vma: f64,
    pub sma18: Option<f64>,
    /// Distance of close from the VMA / SMA in percent.
    pub vma_distance_pct: f64,
    pub sma_distance_pct: Option<f64>,
    pub up_strength: u8,
    pub down_strength: u8,
    /// True when a dot is active on the latest bar (strength ≥ 2, Medium).
    pub up_dot: bool,
    pub down_dot: bool,
    /// Recent dot-run onsets, oldest first (capped by the caller).
    pub recent_dots: Vec<DotEvent>,
}

/// Per-bar SuperTrend direction series with the exact Pine ratchet.
/// Returns (dir per bar, ratcheted upper-trail, ratcheted lower-trail).
fn supertrend_series(
    closes: &[f64],
    highs: &[f64],
    lows: &[f64],
) -> (Vec<i8>, Vec<Option<f64>>, Vec<Option<f64>>) {
    let n = closes.len();
    let tr = primitives::true_range(highs, lows, closes);
    // ta.sma over a series with a leading na: the window only fills once
    // `period` defined TR values exist (i.e. from bar `period`).
    let tr_dense: Vec<f64> = tr.iter().copied().flatten().collect();
    let atr_dense = primitives::sma(&tr_dense, SUPER_PERIOD);
    let offset = n - tr_dense.len();
    let mut atr = vec![None; n];
    for (i, v) in atr_dense.iter().enumerate() {
        atr[offset + i] = *v;
    }

    let mut sup_up: Vec<Option<f64>> = vec![None; n];
    let mut sup_dn: Vec<Option<f64>> = vec![None; n];
    // Documented adaptation: seed dir = 1 (see module docs).
    let mut dir: i8 = 1;
    let mut dirs = vec![1i8; n];
    for t in 0..n {
        let hl2 = (highs[t] + lows[t]) / 2.0;
        if let Some(a) = atr[t] {
            let raw_up = hl2 - SUPER_MULT * a;
            let raw_dn = hl2 + SUPER_MULT * a;
            // Pine nz(x[1]) ⇒ 0 when the previous value is na.
            let prev_up = if t > 0 { sup_up[t - 1].unwrap_or(0.0) } else { 0.0 };
            let prev_dn = if t > 0 { sup_dn[t - 1].unwrap_or(0.0) } else { 0.0 };
            let prev_close = if t > 0 { closes[t - 1] } else { closes[t] };
            sup_up[t] = Some(if prev_close > prev_up { raw_up.max(prev_up) } else { raw_up });
            sup_dn[t] = Some(if prev_close < prev_dn { raw_dn.min(prev_dn) } else { raw_dn });
            if dir == -1 && closes[t] > prev_dn {
                dir = 1;
            } else if dir == 1 && closes[t] < prev_up {
                dir = -1;
            }
        }
        dirs[t] = dir;
    }
    (dirs, sup_up, sup_dn)
}

/// Compute the CyberDots read over the full series.
pub fn compute_dots(
    closes: &[f64],
    highs: &[f64],
    lows: &[f64],
    dates: &[String],
    max_events: usize,
) -> Option<DotsRead> {
    let n = closes.len();
    if n < MA_LEN + SUPER_PERIOD + 2 {
        return None;
    }
    let (dirs, sup_up, sup_dn) = supertrend_series(closes, highs, lows);
    let vma = vidya_series(closes, VMA_LEN);
    let ma = primitives::sma(closes, MA_LEN);

    let strength_at = |t: usize| -> (u8, u8) {
        let c = closes[t];
        let vma_dist = if vma[t] != 0.0 {
            (c - vma[t]).abs() / vma[t] * 100.0
        } else {
            0.0
        };
        let (up2, dn2) = match ma[t] {
            Some(m) if m != 0.0 => {
                let dist = (c - m).abs() / m * 100.0;
                (
                    c > m && dist >= MA_THRESHOLD_PCT,
                    c < m && dist >= MA_THRESHOLD_PCT,
                )
            }
            _ => (false, false),
        };
        let up1 = c > vma[t] && vma_dist >= VMA_THRESHOLD_PCT;
        let dn1 = c < vma[t] && vma_dist >= VMA_THRESHOLD_PCT;
        let up3 = dirs[t] == 1;
        let dn3 = dirs[t] == -1;
        (
            u8::from(up1) + u8::from(up2) + u8::from(up3),
            u8::from(dn1) + u8::from(dn2) + u8::from(dn3),
        )
    };

    // Dot-run onsets across the series.
    let mut events: Vec<DotEvent> = Vec::new();
    let mut prev_up = false;
    let mut prev_dn = false;
    for (t, date) in dates.iter().enumerate().take(n) {
        let (us, ds) = strength_at(t);
        let up_dot = us >= MIN_STRENGTH;
        let dn_dot = ds >= MIN_STRENGTH;
        if up_dot && !prev_up {
            events.push(DotEvent {
                date: date.clone(),
                direction: "up".to_string(),
                strength: us,
            });
        }
        if dn_dot && !prev_dn {
            events.push(DotEvent {
                date: date.clone(),
                direction: "down".to_string(),
                strength: ds,
            });
        }
        prev_up = up_dot;
        prev_dn = dn_dot;
    }
    if events.len() > max_events {
        events.drain(..events.len() - max_events);
    }

    let last = n - 1;
    let (up_strength, down_strength) = strength_at(last);
    let c = closes[last];
    let vma_distance_pct = if vma[last] != 0.0 {
        (c - vma[last]).abs() / vma[last] * 100.0
    } else {
        0.0
    };
    let sma_distance_pct = ma[last].map(|m| (c - m).abs() / m * 100.0);
    let supertrend_stop = if dirs[last] == 1 { sup_up[last] } else { sup_dn[last] };

    Some(DotsRead {
        supertrend_dir: dirs[last],
        supertrend_stop: supertrend_stop.map(round_level),
        vma: round_level(vma[last]),
        sma18: ma[last].map(round_level),
        vma_distance_pct: round_level(vma_distance_pct),
        sma_distance_pct: sma_distance_pct.map(round_level),
        up_strength,
        down_strength,
        up_dot: up_strength >= MIN_STRENGTH,
        down_dot: down_strength >= MIN_STRENGTH,
        recent_dots: events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("d{i:04}")).collect()
    }

    fn ohlc_from_closes(closes: &[f64], range: f64) -> (Vec<f64>, Vec<f64>) {
        let highs: Vec<f64> = closes.iter().map(|c| c + range).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - range).collect();
        (highs, lows)
    }

    #[test]
    fn steady_uptrend_full_up_strength_and_dot() {
        // +1%/bar compounding: close stays above VMA (lags), above SMA18,
        // SuperTrend long ⇒ strength 3, dot active.
        let closes: Vec<f64> = (0..120).map(|i| 100.0 * 1.01f64.powi(i)).collect();
        let (highs, lows) = ohlc_from_closes(&closes, 0.5);
        let read = compute_dots(&closes, &highs, &lows, &dates(120), 10).expect("dots");
        assert_eq!(read.supertrend_dir, 1);
        assert_eq!(read.up_strength, 3, "{read:?}");
        assert!(read.up_dot);
        assert_eq!(read.down_strength, 0);
        assert!(!read.recent_dots.is_empty());
        assert_eq!(read.recent_dots.last().map(|e| e.direction.as_str()), Some("up"));
    }

    #[test]
    fn breakdown_flips_supertrend_and_fires_down_dot() {
        // Uptrend then a hard breakdown: close drops far below the ratcheted
        // upper trail ⇒ dir flips to −1; close below VMA + SMA ⇒ strength 3.
        let mut closes: Vec<f64> = (0..100).map(|i| 100.0 + i as f64 * 0.5).collect();
        for i in 0..20 {
            closes.push(150.0 - 5.0 * (i + 1) as f64);
        }
        let (highs, lows) = ohlc_from_closes(&closes, 0.5);
        let read = compute_dots(&closes, &highs, &lows, &dates(closes.len()), 10).expect("dots");
        assert_eq!(read.supertrend_dir, -1);
        assert_eq!(read.down_strength, 3, "{read:?}");
        assert!(read.down_dot);
        assert!(read
            .recent_dots
            .iter()
            .any(|e| e.direction == "down"));
    }

    #[test]
    fn supertrend_ratchet_holds_trail_through_pullback() {
        // Rising closes ratchet the long trail monotonically while the prior
        // close stays above it; a small pullback must not lower the trail.
        let mut closes: Vec<f64> = (0..40).map(|i| 100.0 + i as f64).collect();
        closes.push(138.5); // shallow dip, still above the trail
        let (highs, lows) = ohlc_from_closes(&closes, 1.0);
        let (dirs, sup_up, _) = supertrend_series(&closes, &highs, &lows);
        let n = closes.len();
        assert_eq!(dirs[n - 1], 1);
        let prev = sup_up[n - 2].expect("trail defined");
        let lastv = sup_up[n - 1].expect("trail defined");
        assert!(lastv >= prev, "ratchet must not lower the long trail");
    }

    #[test]
    fn flat_tape_below_thresholds_no_dot() {
        // Microscopic oscillation: distances stay below 0.15%/0.20% so only
        // the SuperTrend-direction condition can be true ⇒ strength ≤ 1.
        // (Long series so the VMA, which warms up from 0, fully converges.)
        let closes: Vec<f64> = (0..300)
            .map(|i| 100.0 + if i % 2 == 0 { 0.01 } else { -0.01 })
            .collect();
        let (highs, lows) = ohlc_from_closes(&closes, 0.05);
        let read = compute_dots(&closes, &highs, &lows, &dates(300), 10).expect("dots");
        assert!(read.up_strength <= 1 && read.down_strength <= 1, "{read:?}");
        assert!(!read.up_dot && !read.down_dot);
    }
}
