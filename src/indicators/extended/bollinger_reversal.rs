//! Bollinger band cross reversal signals.
//!
//! Top reversal: close crosses UNDER the upper band on bar `t` (close[t-1] >=
//! upper[t-1] AND close[t] < upper[t]). Bottom reversal: close crosses OVER
//! the lower band on bar `t` (close[t-1] <= lower[t-1] AND close[t] >
//! lower[t]).
//!
//! Multi-bar confirmation:
//!  * `confirmation_1`: the bar immediately AFTER the reversal bar trades
//!    entirely below (top) or above (bottom) the reversal-bar low/high.
//!  * `confirmation_2`: the two bars immediately after both confirm.
//!
//! Outputs the most recent signal of each direction with the bar index where
//! it fired and a confirmation count (0, 1, or 2). Pure / no I/O.

use crate::indicators::bollinger::compute_bollinger;
use serde::{Deserialize, Serialize};

/// Default Bollinger period / multiplier (canonical).
pub const DEFAULT_PERIOD: usize = 20;
pub const DEFAULT_MULTIPLIER: f64 = 2.0;

/// Result of a Bollinger reversal scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BollingerReversalResult {
    /// Most recent top reversal signal (cross-under upper band), if any.
    pub top_reversal_signal: Option<ReversalMarker>,
    /// Most recent bottom reversal signal (cross-over lower band), if any.
    pub bottom_reversal_signal: Option<ReversalMarker>,
}

/// One reversal occurrence: bar index, bars-since, and how many subsequent
/// bars confirmed (0, 1, or 2 — `confirmation_1` / `confirmation_2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReversalMarker {
    pub bar_index: usize,
    pub bars_since: usize,
    /// 0 = signal only, 1 = 1-bar confirmation passed, 2 = 2-bar confirmation
    /// passed.
    pub confirmation_count: u8,
    pub confirmation_1: bool,
    pub confirmation_2: bool,
}

/// Compute Bollinger reversal signals (and confirmations).
pub fn compute_bollinger_reversal(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
    multiplier: f64,
) -> BollingerReversalResult {
    let n = closes.len();
    let bb = compute_bollinger(closes, period, multiplier);
    if highs.len() != n || lows.len() != n || n < period + 2 {
        return BollingerReversalResult {
            top_reversal_signal: None,
            bottom_reversal_signal: None,
        };
    }

    // Walk newest → oldest to find the latest top + bottom signal.
    let mut top: Option<ReversalMarker> = None;
    let mut bot: Option<ReversalMarker> = None;

    for i in (1..n).rev() {
        let (Some(cur), Some(prev)) = (bb[i], bb[i - 1]) else {
            continue;
        };

        // top: close crosses UNDER upper band
        if top.is_none()
            && closes[i - 1] >= prev.upper
            && closes[i] < cur.upper
        {
            let conf = evaluate_confirmation_top(highs, lows, i, n);
            top = Some(ReversalMarker {
                bar_index: i,
                bars_since: n - 1 - i,
                confirmation_count: conf.0,
                confirmation_1: conf.1,
                confirmation_2: conf.2,
            });
        }
        // bottom: close crosses OVER lower band
        if bot.is_none()
            && closes[i - 1] <= prev.lower
            && closes[i] > cur.lower
        {
            let conf = evaluate_confirmation_bottom(highs, lows, i, n);
            bot = Some(ReversalMarker {
                bar_index: i,
                bars_since: n - 1 - i,
                confirmation_count: conf.0,
                confirmation_1: conf.1,
                confirmation_2: conf.2,
            });
        }
        if top.is_some() && bot.is_some() {
            break;
        }
    }

    BollingerReversalResult {
        top_reversal_signal: top,
        bottom_reversal_signal: bot,
    }
}

fn evaluate_confirmation_top(highs: &[f64], lows: &[f64], i: usize, n: usize) -> (u8, bool, bool) {
    // Reversal-bar low is the reference.
    let ref_low = lows[i];
    let c1 = i + 1 < n && highs[i + 1] < ref_low;
    let c2 = c1 && i + 2 < n && highs[i + 2] < ref_low;
    let count = if c2 { 2 } else if c1 { 1 } else { 0 };
    (count, c1, c2)
}

fn evaluate_confirmation_bottom(
    highs: &[f64],
    lows: &[f64],
    i: usize,
    n: usize,
) -> (u8, bool, bool) {
    // Reversal-bar high is the reference.
    let ref_high = highs[i];
    let c1 = i + 1 < n && lows[i + 1] > ref_high;
    let c2 = c1 && i + 2 < n && lows[i + 2] > ref_high;
    let count = if c2 { 2 } else if c1 { 1 } else { 0 };
    (count, c1, c2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_top_reversal_series() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        // 50 flat-ish bars to seed Bollinger band, then one bar that breaks
        // ABOVE the upper band, then a reversal bar that closes BACK INSIDE
        // the band → cross-under. Then 3 confirmation bars trade entirely
        // below the reversal-bar low.
        let mut h = vec![101.0; 50];
        let mut l = vec![99.0; 50];
        let mut c = vec![100.0; 50];
        // Push close to ~150 (well above any Bollinger upper band)
        h.push(155.0);
        l.push(148.0);
        c.push(150.0);
        // Reversal bar: close drops back to ~100 — guaranteed inside band
        h.push(125.0);
        l.push(95.0);
        c.push(100.0);
        // Confirmation bars: high MUST stay below the reversal-bar low (95)
        for _ in 0..3 {
            h.push(94.0);
            l.push(85.0);
            c.push(90.0);
        }
        (h, l, c)
    }

    fn synth_bottom_reversal_series() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        // Mirror: 50 flat bars, capitulation below lower band, reversal back
        // inside, confirmation bars entirely above reversal-bar high.
        let mut h = vec![101.0; 50];
        let mut l = vec![99.0; 50];
        let mut c = vec![100.0; 50];
        // Capitulation: close to 50 (well below lower band)
        h.push(55.0);
        l.push(48.0);
        c.push(50.0);
        // Reversal bar: close back to 100
        h.push(105.0);
        l.push(75.0);
        c.push(100.0);
        // Confirmation bars: low MUST stay above the reversal-bar high (105)
        for _ in 0..3 {
            h.push(120.0);
            l.push(106.0);
            c.push(115.0);
        }
        (h, l, c)
    }

    #[test]
    fn detects_top_reversal_with_confirmations() {
        let (h, l, c) = synth_top_reversal_series();
        let r = compute_bollinger_reversal(&h, &l, &c, DEFAULT_PERIOD, DEFAULT_MULTIPLIER);
        let marker = r.top_reversal_signal.expect("top reversal expected");
        assert!(marker.confirmation_1, "1-bar confirmation should pass");
        assert!(marker.confirmation_2, "2-bar confirmation should pass");
        assert_eq!(marker.confirmation_count, 2);
    }

    #[test]
    fn detects_bottom_reversal_with_confirmations() {
        let (h, l, c) = synth_bottom_reversal_series();
        let r = compute_bollinger_reversal(&h, &l, &c, DEFAULT_PERIOD, DEFAULT_MULTIPLIER);
        let marker = r.bottom_reversal_signal.expect("bottom reversal expected");
        assert!(marker.confirmation_1, "1-bar confirmation should pass");
        assert!(marker.confirmation_2, "2-bar confirmation should pass");
        assert_eq!(marker.confirmation_count, 2);
    }

    #[test]
    fn no_signals_on_flat_series() {
        let v = vec![100.0; 50];
        let r = compute_bollinger_reversal(&v, &v, &v, DEFAULT_PERIOD, DEFAULT_MULTIPLIER);
        assert!(r.top_reversal_signal.is_none());
        assert!(r.bottom_reversal_signal.is_none());
    }

    #[test]
    fn returns_empty_when_too_short() {
        let v = vec![1.0, 2.0, 3.0];
        let r = compute_bollinger_reversal(&v, &v, &v, DEFAULT_PERIOD, DEFAULT_MULTIPLIER);
        assert!(r.top_reversal_signal.is_none());
        assert!(r.bottom_reversal_signal.is_none());
    }
}
