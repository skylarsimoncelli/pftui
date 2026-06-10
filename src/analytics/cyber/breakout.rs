//! Hybrid breakout arrows — component G of the Cyber Dots port.
//!
//! Pine mapping (`bullish3Strike`/`bearish3Strike`, `bindex`/`sindex`
//! momentum-exhaustion counters, `bullBreakout`/`bearBreakout` block):
//!
//! - 3-line strike: three consecutive opposite candles then an engulfing
//!   close beyond the PREVIOUS bar's open —
//!   `close[3]<open[3] and close[2]<open[2] and close[1]<open[1] and
//!   close>open[1]` (bullish; mirror for bearish).
//! - Momentum exhaustion: persistent counters `bindex += close > close[4]`,
//!   `sindex += close < close[4]` (both start 0, only reset on fire);
//!   `momExhaustionHigh = bindex > 5 and close < open and
//!   high >= ta.highest(high, 25)` (mirror with `lowest(low, 25)`); the
//!   fired counter resets to 0.
//! - RSI zone-exit signals come from component F (`rsi_UpSignal` /
//!   `rsi_DownSignal`).
//! - Arrows: `signalCount >= 1` (default `requireMultipleSignals = false`),
//!   gated by the CyberBands QB trend state (bull arrows need
//!   `trendState >= 0`, bear arrows `<= 0`), with a 5-bar cooldown per side
//!   (`var lastBullArrow = 0` ⇒ Pine suppresses arrows on the first 5 bars;
//!   preserved). Strength 1–3 = the arrow's own contributing-signal count.

use serde::Serialize;

const EXHAUSTION_BARS: usize = 5; // counter must EXCEED this
const EXHAUSTION_LOOKBACK: usize = 25;
const MIN_COOLDOWN_BARS: usize = 5;
const MIN_SIGNALS_REQUIRED: u8 = 1;

/// A dated breakout arrow.
#[derive(Debug, Clone, Serialize)]
pub struct BreakoutEvent {
    pub date: String,
    /// "bull" or "bear".
    pub direction: String,
    /// Number of contributing signals (1–3).
    pub strength: u8,
    /// Contributing signal names ("rsi-zone-exit", "3-line-strike",
    /// "momentum-exhaustion").
    pub signals: Vec<String>,
}

/// Breakout read for the latest bar + recent arrows.
#[derive(Debug, Clone, Serialize)]
pub struct BreakoutRead {
    /// Momentum counters on the latest bar (after any reset).
    pub bull_counter: u32,
    pub bear_counter: u32,
    /// Arrow active on the latest bar, if any ("bull" / "bear").
    pub latest_arrow: Option<String>,
    /// Recent arrows, oldest first (capped by the caller).
    pub recent: Vec<BreakoutEvent>,
}

/// Compute the hybrid breakout arrows over the full series.
///
/// `qb_series` is the per-bar CyberBands QB state (component A);
/// `rsi_up`/`rsi_dn` are the per-bar zone-exit signals (component F). Either
/// gate degrades gracefully: an empty slice disables that input (QB defaults
/// to 0 = both sides allowed, matching the Pine before the first breach).
#[allow(clippy::too_many_arguments)]
pub fn compute_breakouts(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    dates: &[String],
    qb_series: &[i8],
    rsi_up: &[bool],
    rsi_dn: &[bool],
    max_events: usize,
) -> Option<BreakoutRead> {
    let n = closes.len();
    if n < EXHAUSTION_LOOKBACK + 5 {
        return None;
    }
    let highest25 = super::primitives::highest(highs, EXHAUSTION_LOOKBACK);
    let lowest25 = super::primitives::lowest(lows, EXHAUSTION_LOOKBACK);

    let mut bindex: u32 = 0;
    let mut sindex: u32 = 0;
    // Pine: `var lastBullArrow = 0` — bar_index distance gate.
    let mut last_bull_arrow: i64 = 0;
    let mut last_bear_arrow: i64 = 0;
    let mut events: Vec<BreakoutEvent> = Vec::new();
    let mut latest_arrow: Option<String> = None;

    for t in 0..n {
        if t >= 4 {
            if closes[t] > closes[t - 4] {
                bindex += 1;
            }
            if closes[t] < closes[t - 4] {
                sindex += 1;
            }
        }

        let bullish_strike = t >= 3
            && closes[t - 3] < opens[t - 3]
            && closes[t - 2] < opens[t - 2]
            && closes[t - 1] < opens[t - 1]
            && closes[t] > opens[t - 1];
        let bearish_strike = t >= 3
            && closes[t - 3] > opens[t - 3]
            && closes[t - 2] > opens[t - 2]
            && closes[t - 1] > opens[t - 1]
            && closes[t] < opens[t - 1];

        let exhaustion_high = bindex > EXHAUSTION_BARS as u32
            && closes[t] < opens[t]
            && highest25[t].map(|h| highs[t] >= h).unwrap_or(false);
        let exhaustion_low = sindex > EXHAUSTION_BARS as u32
            && closes[t] > opens[t]
            && lowest25[t].map(|l| lows[t] <= l).unwrap_or(false);
        if exhaustion_high {
            bindex = 0;
        }
        if exhaustion_low {
            sindex = 0;
        }

        let rsi_up_t = rsi_up.get(t).copied().unwrap_or(false);
        let rsi_dn_t = rsi_dn.get(t).copied().unwrap_or(false);
        let mut bull_signals: Vec<String> = Vec::new();
        if rsi_up_t {
            bull_signals.push("rsi-zone-exit".to_string());
        }
        if bullish_strike {
            bull_signals.push("3-line-strike".to_string());
        }
        if exhaustion_low {
            bull_signals.push("momentum-exhaustion".to_string());
        }
        let mut bear_signals: Vec<String> = Vec::new();
        if rsi_dn_t {
            bear_signals.push("rsi-zone-exit".to_string());
        }
        if bearish_strike {
            bear_signals.push("3-line-strike".to_string());
        }
        if exhaustion_high {
            bear_signals.push("momentum-exhaustion".to_string());
        }

        let trend_state = qb_series.get(t).copied().unwrap_or(0);
        let bull_breakout = bull_signals.len() as u8 >= MIN_SIGNALS_REQUIRED
            && trend_state >= 0
            && (t as i64 - last_bull_arrow) >= MIN_COOLDOWN_BARS as i64;
        let bear_breakout = bear_signals.len() as u8 >= MIN_SIGNALS_REQUIRED
            && trend_state <= 0
            && (t as i64 - last_bear_arrow) >= MIN_COOLDOWN_BARS as i64;

        if bull_breakout {
            last_bull_arrow = t as i64;
            events.push(BreakoutEvent {
                date: dates[t].clone(),
                direction: "bull".to_string(),
                strength: bull_signals.len() as u8,
                signals: bull_signals,
            });
            if t == n - 1 {
                latest_arrow = Some("bull".to_string());
            }
        }
        if bear_breakout {
            last_bear_arrow = t as i64;
            events.push(BreakoutEvent {
                date: dates[t].clone(),
                direction: "bear".to_string(),
                strength: bear_signals.len() as u8,
                signals: bear_signals,
            });
            if t == n - 1 {
                latest_arrow = Some("bear".to_string());
            }
        }

        if t == n - 1 {
            if events.len() > max_events {
                events.drain(..events.len() - max_events);
            }
            return Some(BreakoutRead {
                bull_counter: bindex,
                bear_counter: sindex,
                latest_arrow,
                recent: events,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("d{i:04}")).collect()
    }

    /// Build (opens, highs, lows, closes) from (open, close) candles with a
    /// small wick either side.
    fn candles(oc: &[(f64, f64)]) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let opens: Vec<f64> = oc.iter().map(|c| c.0).collect();
        let closes: Vec<f64> = oc.iter().map(|c| c.1).collect();
        let highs: Vec<f64> = oc.iter().map(|c| c.0.max(c.1) + 0.5).collect();
        let lows: Vec<f64> = oc.iter().map(|c| c.0.min(c.1) - 0.5).collect();
        (opens, highs, lows, closes)
    }

    #[test]
    fn bullish_three_line_strike_fires_arrow() {
        // 30 quiet bars, then 3 red candles, then a green engulf closing
        // above the previous bar's open.
        let mut oc: Vec<(f64, f64)> = (0..30).map(|_| (100.0, 100.2)).collect();
        oc.push((100.0, 98.0)); // red
        oc.push((98.0, 96.0)); // red
        oc.push((96.0, 94.0)); // red, open[1] of strike bar = 96
        oc.push((94.0, 97.0)); // close 97 > 96 ⇒ bullish strike
        let (o, h, l, c) = candles(&oc);
        let n = c.len();
        let qb = vec![0i8; n];
        let read = compute_breakouts(&o, &h, &l, &c, &dates(n), &qb, &[], &[], 10)
            .expect("breakouts");
        let arrow = read
            .recent
            .iter()
            .find(|e| e.direction == "bull")
            .expect("bull arrow");
        assert_eq!(arrow.date, format!("d{:04}", n - 1));
        assert!(arrow.signals.contains(&"3-line-strike".to_string()));
        assert_eq!(read.latest_arrow.as_deref(), Some("bull"));
    }

    #[test]
    fn engulf_must_beat_previous_open_not_its_own() {
        // Same as above but the last close stops below open[1] (96): no strike.
        let mut oc: Vec<(f64, f64)> = (0..30).map(|_| (100.0, 100.2)).collect();
        oc.push((100.0, 98.0));
        oc.push((98.0, 96.0));
        oc.push((96.0, 94.0));
        oc.push((94.0, 95.5)); // green but 95.5 < 96
        let (o, h, l, c) = candles(&oc);
        let n = c.len();
        let qb = vec![0i8; n];
        let read = compute_breakouts(&o, &h, &l, &c, &dates(n), &qb, &[], &[], 10)
            .expect("breakouts");
        assert!(read.recent.iter().all(|e| e.direction != "bull"));
    }

    #[test]
    fn momentum_exhaustion_high_fires_and_resets_counter() {
        // 30 rising green bars (bindex climbs well past 5), then a red bar
        // tagging the 25-bar high ⇒ momExhaustionHigh + counter reset.
        let mut oc: Vec<(f64, f64)> = (0..30)
            .map(|i| (100.0 + i as f64, 101.0 + i as f64))
            .collect();
        oc.push((131.5, 130.0)); // red close; high = 131.5+0.5 ≥ 25-bar high
        let (o, h, l, c) = candles(&oc);
        let n = c.len();
        let qb = vec![-1i8; n]; // bear gate open
        let read = compute_breakouts(&o, &h, &l, &c, &dates(n), &qb, &[], &[], 10)
            .expect("breakouts");
        let arrow = read
            .recent
            .iter()
            .find(|e| e.direction == "bear")
            .expect("bear arrow");
        assert!(arrow.signals.contains(&"momentum-exhaustion".to_string()));
        assert_eq!(read.bull_counter, 0, "bindex resets on fire");
    }

    #[test]
    fn qb_gate_blocks_counter_trend_arrows() {
        // Same bearish exhaustion setup but QB = +1 (bull): bear arrows are
        // gated off (`trendState <= 0` fails).
        let mut oc: Vec<(f64, f64)> = (0..30)
            .map(|i| (100.0 + i as f64, 101.0 + i as f64))
            .collect();
        oc.push((131.5, 130.0));
        let (o, h, l, c) = candles(&oc);
        let n = c.len();
        let qb = vec![1i8; n];
        let read = compute_breakouts(&o, &h, &l, &c, &dates(n), &qb, &[], &[], 10)
            .expect("breakouts");
        assert!(read.recent.iter().all(|e| e.direction != "bear"));
    }

    #[test]
    fn cooldown_suppresses_back_to_back_arrows() {
        // Two bullish strikes 3 bars apart: only the first arrow fires
        // (5-bar cooldown), and nothing fires in the first 5 bars (Pine
        // lastArrow=0 artifact preserved).
        let mut oc: Vec<(f64, f64)> = (0..30).map(|_| (100.0, 100.2)).collect();
        oc.push((100.0, 98.0));
        oc.push((98.0, 96.0));
        oc.push((96.0, 94.0));
        oc.push((94.0, 97.0)); // strike 1 → arrow
        oc.push((97.0, 95.0));
        oc.push((95.0, 93.0));
        oc.push((93.0, 91.0));
        oc.push((91.0, 94.0)); // strike 2, 4 bars later → cooled down
        let (o, h, l, c) = candles(&oc);
        let n = c.len();
        let qb = vec![0i8; n];
        let read = compute_breakouts(&o, &h, &l, &c, &dates(n), &qb, &[], &[], 10)
            .expect("breakouts");
        let bulls: Vec<_> = read.recent.iter().filter(|e| e.direction == "bull").collect();
        assert_eq!(bulls.len(), 1, "cooldown must suppress the second arrow");
    }
}
