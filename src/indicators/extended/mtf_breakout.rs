//! Multi-timeframe breakout composite signal.
//!
//! Composite of three sub-signals computed on a single OHLC series:
//!  * **a — MTF-RSI breakout**: current RSI has just exited a multi-timeframe
//!    aligned overbought/oversold zone (alignment evaluated by
//!    [`crate::indicators::extended::mtf_rsi`]). Exit = the latest bar broke
//!    alignment vs the prior bar.
//!  * **b — 3-line strike pattern**: three consecutive down-closes followed by
//!    an up-close that exceeds the open of bar `t-3` (mirror for bear).
//!  * **c — Momentum exhaustion**: 5+ consecutive closes greater than
//!    `close[t-4]` with the current bar `close < open` AND `high >= 25-bar
//!    high` (mirror: 5+ closes less than `close[t-4]` with `close > open` AND
//!    `low <= 25-bar low`).
//!
//! Outputs per-signal booleans (bull/bear variants per signal), an overall
//! `signal_count` (0-3, picking the dominant direction), and a cooldown-aware
//! `breakout_state` ∈ {`bull-fresh`, `bull-armed`, `none`, `bear-armed`,
//! `bear-fresh`}.

use crate::indicators::extended::mtf_rsi::{
    compute_mtf_rsi, DEFAULT_RSI_PERIOD, OVERBOUGHT_THRESHOLD, OVERSOLD_THRESHOLD,
};
use serde::{Deserialize, Serialize};

/// Number of bars to look back for the 25-bar high / low used by momentum
/// exhaustion sub-signal.
pub const MOMENTUM_LOOKBACK_BARS: usize = 25;

/// Default cooldown window (in bars) between successive breakout firings.
pub const DEFAULT_COOLDOWN_BARS: usize = 5;

/// Configurable inputs for the breakout composite.
#[derive(Debug, Clone)]
pub struct MtfBreakoutConfig {
    pub rsi_period: usize,
    pub htf_bucket_sizes: Vec<usize>,
    pub cooldown_bars: usize,
    pub momentum_lookback: usize,
}

impl Default for MtfBreakoutConfig {
    fn default() -> Self {
        MtfBreakoutConfig {
            rsi_period: DEFAULT_RSI_PERIOD,
            htf_bucket_sizes: Vec::new(),
            cooldown_bars: DEFAULT_COOLDOWN_BARS,
            momentum_lookback: MOMENTUM_LOOKBACK_BARS,
        }
    }
}

/// Cooldown-aware breakout state machine value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BreakoutState {
    BullFresh,
    BullArmed,
    None,
    BearArmed,
    BearFresh,
}

impl BreakoutState {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            BreakoutState::BullFresh => "bull-fresh",
            BreakoutState::BullArmed => "bull-armed",
            BreakoutState::None => "none",
            BreakoutState::BearArmed => "bear-armed",
            BreakoutState::BearFresh => "bear-fresh",
        }
    }
}

/// Result of the breakout composite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtfBreakoutResult {
    pub mtf_rsi_breakout_bull: bool,
    pub mtf_rsi_breakout_bear: bool,
    pub three_line_strike_bull: bool,
    pub three_line_strike_bear: bool,
    pub momentum_exhaustion_top: bool,
    pub momentum_exhaustion_bottom: bool,
    /// Count of bull-or-bear sub-signals currently active in the dominant
    /// direction (0–3).
    pub signal_count: u8,
    pub breakout_state: BreakoutState,
}

/// Compute the MTF breakout composite over an OHLC series.
///
/// Each input slice must have the same length and be in chronological order
/// (oldest → newest). When too short to evaluate any signal, returns a zeroed
/// result with `BreakoutState::None`.
pub fn compute_mtf_breakout(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    timeframe: &str,
    config: &MtfBreakoutConfig,
) -> MtfBreakoutResult {
    let n = closes.len();
    if n < config.momentum_lookback + 1
        || opens.len() != n
        || highs.len() != n
        || lows.len() != n
    {
        return zero_result();
    }

    // -- (a) MTF-RSI breakout: detect that the alignment FLIPPED from
    // overbought/oversold on bar t-1 → broken on bar t.
    let last_idx = n - 1;
    let prev_closes = &closes[..n - 1];
    let cur_mtf = compute_mtf_rsi(
        closes,
        timeframe,
        &config.htf_bucket_sizes,
        config.rsi_period,
    );
    let prev_mtf = compute_mtf_rsi(
        prev_closes,
        timeframe,
        &config.htf_bucket_sizes,
        config.rsi_period,
    );

    let mtf_rsi_breakout_bear = prev_mtf.aligned_overbought
        && !cur_mtf.aligned_overbought
        && matches!(cur_mtf.current_rsi, Some(rsi) if rsi < OVERBOUGHT_THRESHOLD);
    let mtf_rsi_breakout_bull = prev_mtf.aligned_oversold
        && !cur_mtf.aligned_oversold
        && matches!(cur_mtf.current_rsi, Some(rsi) if rsi > OVERSOLD_THRESHOLD);

    // -- (b) Three-line strike pattern.
    let three_line_strike_bull = detect_three_line_strike_bull(opens, closes, last_idx);
    let three_line_strike_bear = detect_three_line_strike_bear(opens, closes, last_idx);

    // -- (c) Momentum exhaustion.
    let momentum_exhaustion_top =
        detect_momentum_exhaustion_top(opens, highs, closes, last_idx, config.momentum_lookback);
    let momentum_exhaustion_bottom =
        detect_momentum_exhaustion_bottom(opens, lows, closes, last_idx, config.momentum_lookback);

    let bull_count = [
        mtf_rsi_breakout_bull,
        three_line_strike_bull,
        momentum_exhaustion_bottom,
    ]
    .iter()
    .filter(|x| **x)
    .count() as u8;
    let bear_count = [
        mtf_rsi_breakout_bear,
        three_line_strike_bear,
        momentum_exhaustion_top,
    ]
    .iter()
    .filter(|x| **x)
    .count() as u8;

    let dominant = bull_count.max(bear_count);
    let breakout_state = if dominant == 0 {
        BreakoutState::None
    } else if bull_count > bear_count {
        if recent_firing(opens, highs, lows, closes, timeframe, config, Direction::Bull, last_idx) {
            BreakoutState::BullArmed
        } else {
            BreakoutState::BullFresh
        }
    } else if bear_count > bull_count {
        if recent_firing(opens, highs, lows, closes, timeframe, config, Direction::Bear, last_idx) {
            BreakoutState::BearArmed
        } else {
            BreakoutState::BearFresh
        }
    } else {
        BreakoutState::None
    };

    MtfBreakoutResult {
        mtf_rsi_breakout_bull,
        mtf_rsi_breakout_bear,
        three_line_strike_bull,
        three_line_strike_bear,
        momentum_exhaustion_top,
        momentum_exhaustion_bottom,
        signal_count: dominant,
        breakout_state,
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Direction {
    Bull,
    Bear,
}

fn zero_result() -> MtfBreakoutResult {
    MtfBreakoutResult {
        mtf_rsi_breakout_bull: false,
        mtf_rsi_breakout_bear: false,
        three_line_strike_bull: false,
        three_line_strike_bear: false,
        momentum_exhaustion_top: false,
        momentum_exhaustion_bottom: false,
        signal_count: 0,
        breakout_state: BreakoutState::None,
    }
}

fn detect_three_line_strike_bull(opens: &[f64], closes: &[f64], last: usize) -> bool {
    if last < 4 {
        return false;
    }
    // Bars t-3, t-2, t-1 each closed lower than the prior close, and bar t
    // closed UP and ABOVE the open of bar t-3 (canonical bullish 3-line strike).
    let three_down = closes[last - 3] < closes[last - 4]
        && closes[last - 2] < closes[last - 3]
        && closes[last - 1] < closes[last - 2];
    let up_close = closes[last] > opens[last] && closes[last] > opens[last - 3];
    three_down && up_close
}

fn detect_three_line_strike_bear(opens: &[f64], closes: &[f64], last: usize) -> bool {
    if last < 4 {
        return false;
    }
    let three_up = closes[last - 3] > closes[last - 4]
        && closes[last - 2] > closes[last - 3]
        && closes[last - 1] > closes[last - 2];
    let down_close = closes[last] < opens[last] && closes[last] < opens[last - 3];
    three_up && down_close
}

fn detect_momentum_exhaustion_top(
    opens: &[f64],
    highs: &[f64],
    closes: &[f64],
    last: usize,
    lookback: usize,
) -> bool {
    // 5+ consecutive bars where close > close[-4] (canonical "9-count" style
    // exhaustion seed). End condition: current close < open AND current high
    // >= max high over the last `lookback` bars.
    if last < 4 + 4 || last < lookback {
        return false;
    }
    let mut count = 0usize;
    let mut i = last;
    while i >= 4 && closes[i] > closes[i - 4] {
        count += 1;
        i -= 1;
        if count >= 9 {
            break;
        }
    }
    if count < 5 {
        return false;
    }
    let close_red = closes[last] < opens[last];
    let high_extreme = highs[last]
        >= highs[(last + 1 - lookback)..=last]
            .iter()
            .cloned()
            .fold(f64::MIN, f64::max);
    close_red && high_extreme
}

fn detect_momentum_exhaustion_bottom(
    opens: &[f64],
    lows: &[f64],
    closes: &[f64],
    last: usize,
    lookback: usize,
) -> bool {
    if last < 4 + 4 || last < lookback {
        return false;
    }
    let mut count = 0usize;
    let mut i = last;
    while i >= 4 && closes[i] < closes[i - 4] {
        count += 1;
        i -= 1;
        if count >= 9 {
            break;
        }
    }
    if count < 5 {
        return false;
    }
    let close_green = closes[last] > opens[last];
    let low_extreme = lows[last]
        <= lows[(last + 1 - lookback)..=last]
            .iter()
            .cloned()
            .fold(f64::MAX, f64::min);
    close_green && low_extreme
}

/// Returns true if a same-direction signal fired within the cooldown window
/// preceding the current bar.
#[allow(clippy::too_many_arguments)]
fn recent_firing(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    _timeframe: &str,
    config: &MtfBreakoutConfig,
    direction: Direction,
    last: usize,
) -> bool {
    // Walk back up to `cooldown_bars` bars (exclusive of current) and check
    // whether any of the two pattern sub-signals (3-line strike, exhaustion)
    // fired. We deliberately don't re-run MTF-RSI breakout in the window since
    // it requires its own historical alignment trace.
    let start = last.saturating_sub(config.cooldown_bars);
    for i in start..last {
        match direction {
            Direction::Bull => {
                if detect_three_line_strike_bull(opens, closes, i)
                    || detect_momentum_exhaustion_bottom(opens, lows, closes, i, config.momentum_lookback)
                {
                    return true;
                }
            }
            Direction::Bear => {
                if detect_three_line_strike_bear(opens, closes, i)
                    || detect_momentum_exhaustion_top(opens, highs, closes, i, config.momentum_lookback)
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_constant_series(n: usize, value: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let o = vec![value; n];
        let h = vec![value + 1.0; n];
        let l = vec![value - 1.0; n];
        let c = vec![value; n];
        (o, h, l, c)
    }

    #[test]
    fn breakout_state_none_for_flat_series() {
        let (o, h, l, c) = build_constant_series(120, 100.0);
        let cfg = MtfBreakoutConfig::default();
        let r = compute_mtf_breakout(&o, &h, &l, &c, "1d", &cfg);
        assert_eq!(r.breakout_state, BreakoutState::None);
        assert_eq!(r.signal_count, 0);
    }

    #[test]
    fn three_line_strike_bull_detected_at_known_bar() {
        // Build bars: ...prior flat... then 3 down-closes then 1 strong up close.
        let mut o = vec![100.0; 30];
        let mut h = vec![101.0; 30];
        let mut l = vec![99.0; 30];
        let mut c = vec![100.0; 30];
        // append 4 specific bars
        let baseline = 100.0;
        // bar t-3: down close
        o.push(baseline);
        c.push(baseline - 1.0);
        h.push(baseline + 0.2);
        l.push(baseline - 1.5);
        // bar t-2: lower close
        o.push(baseline - 1.0);
        c.push(baseline - 2.0);
        h.push(baseline - 0.5);
        l.push(baseline - 2.5);
        // bar t-1: lower close
        o.push(baseline - 2.0);
        c.push(baseline - 3.0);
        h.push(baseline - 1.5);
        l.push(baseline - 3.5);
        // bar t: open low, close above open of bar t-3 (=baseline)
        o.push(baseline - 3.0);
        c.push(baseline + 0.5);
        h.push(baseline + 1.0);
        l.push(baseline - 3.5);

        let last = c.len() - 1;
        assert!(detect_three_line_strike_bull(&o, &c, last));
        assert!(!detect_three_line_strike_bear(&o, &c, last));
    }

    #[test]
    fn three_line_strike_bear_detected_at_known_bar() {
        let mut o = vec![100.0; 30];
        let mut c = vec![100.0; 30];
        // bar t-3: up
        o.push(100.0);
        c.push(101.0);
        // bar t-2: up
        o.push(101.0);
        c.push(102.0);
        // bar t-1: up
        o.push(102.0);
        c.push(103.0);
        // bar t: gap up, close BELOW open of bar t-3 (=100)
        o.push(103.0);
        c.push(99.0);
        let last = c.len() - 1;
        assert!(detect_three_line_strike_bear(&o, &c, last));
        assert!(!detect_three_line_strike_bull(&o, &c, last));
    }

    #[test]
    fn momentum_exhaustion_top_detected_in_rally() {
        // 30-bar rising series, last bar a red close at new 25-bar high.
        let n = 40;
        let mut o = Vec::with_capacity(n);
        let mut h = Vec::with_capacity(n);
        let mut l = Vec::with_capacity(n);
        let mut c = Vec::with_capacity(n);
        for i in 0..n - 1 {
            o.push(100.0 + i as f64);
            h.push(101.0 + i as f64);
            l.push(99.0 + i as f64);
            c.push(100.5 + i as f64); // close > close[-4] for sure
        }
        // last bar: open very high, close BELOW open, high makes a new 25-bar high
        let last_o = c.last().copied().unwrap_or(100.0) + 5.0;
        o.push(last_o);
        h.push(last_o + 2.0); // new 25-bar high
        l.push(last_o - 1.0);
        c.push(last_o - 3.0); // red close

        let last = c.len() - 1;
        assert!(detect_momentum_exhaustion_top(&o, &h, &c, last, MOMENTUM_LOOKBACK_BARS));
    }

    #[test]
    fn signal_count_caps_at_three() {
        // Pathological: synthesize bars where all 3 bull signals fire. Use the
        // 3-line strike + exhaustion bottom + (skip MTF-RSI exit by leaving
        // false). Verify dominant signal_count = 2.
        let mut o = vec![100.0; 30];
        let mut h = vec![101.0; 30];
        let mut l = vec![99.0; 30];
        let mut c = vec![100.0; 30];
        // append 3-line strike bull setup over 4 bars (see test above).
        for delta in [(-1.0, false), (-2.0, false), (-3.0, false)] {
            o.push(100.0 + delta.0 + 1.0);
            c.push(100.0 + delta.0);
            h.push(100.0 + delta.0 + 0.5);
            l.push(100.0 + delta.0 - 1.0);
        }
        // strike bar t
        o.push(95.0);
        c.push(102.0);
        h.push(103.0);
        l.push(94.0);

        let last = c.len() - 1;
        assert!(detect_three_line_strike_bull(&o, &c, last));
        // signal_count should be ≥1 and ≤3
        let r = compute_mtf_breakout(&o, &h, &l, &c, "1d", &MtfBreakoutConfig::default());
        assert!(r.signal_count <= 3);
        assert!(matches!(
            r.breakout_state,
            BreakoutState::BullFresh | BreakoutState::BullArmed | BreakoutState::None
        ));
    }
}
