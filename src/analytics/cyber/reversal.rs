//! Reversal signals — component D of the Cyber Dots port.
//!
//! Pine mapping (`bb_*`, `topRev`/`botRev`, `topConf*`/`botConf*` block):
//! - Bollinger bands: SMA(close, 20) ± 2.0 · `ta.stdev`(close, 20)
//!   (population stdev).
//! - `topRev` = `ta.crossunder(close, bb_upper)`; `botRev` =
//!   `ta.crossover(close, bb_lower)`.
//! - First confirmation: `topConf1 = close < valuewhen(topRev, low, 0) and
//!   barssince(topRev) == 1` — the NEXT bar closes beyond the signal bar's
//!   low (mirror with high for bottoms).
//! - Second confirmation: `topConf2 = close < valuewhen(topRev, low, 0) and
//!   barssince(topRev) == 2 and barssince(topConf1) == 1` — the bar after
//!   conf1 continues beyond the same level. Exact `barssince`/`valuewhen`
//!   semantics: `valuewhen(cond, src, 0)` is the source at the most recent
//!   bar where cond was true (inclusive), `barssince` counts bars since the
//!   last true (na — never — compares false).

use serde::Serialize;

use super::primitives::{self, round_level};

const BB_LEN: usize = 20;
const BB_MULT: f64 = 2.0;

/// One reversal signal with its confirmation ladder.
#[derive(Debug, Clone, Serialize)]
pub struct ReversalEvent {
    pub date: String,
    /// "top" (T) or "bottom" (B).
    pub kind: String,
    /// The signal bar's low (tops) / high (bottoms) — the confirmation level.
    pub trigger_level: f64,
    /// Date of the first confirmation bar, when it fired.
    pub conf1_date: Option<String>,
    /// Date of the second confirmation bar, when it fired.
    pub conf2_date: Option<String>,
    /// "unconfirmed" | "confirmed-1" | "confirmed-2" | "pending" (signal is
    /// on the latest 1–2 bars, confirmation window still open).
    pub status: String,
}

/// Reversal read: recent T/B signals with confirmation status.
#[derive(Debug, Clone, Serialize)]
pub struct ReversalRead {
    pub bb_upper: f64,
    pub bb_lower: f64,
    /// Recent signals, oldest first (capped by the caller).
    pub recent: Vec<ReversalEvent>,
}

/// Compute Bollinger reversal signals + confirmations over the full series.
pub fn compute_reversals(
    closes: &[f64],
    highs: &[f64],
    lows: &[f64],
    dates: &[String],
    max_events: usize,
) -> Option<ReversalRead> {
    let n = closes.len();
    if n < BB_LEN + 2 {
        return None;
    }
    let basis = primitives::sma(closes, BB_LEN);
    let dev = primitives::stdev_pop(closes, BB_LEN);
    let upper: Vec<Option<f64>> = basis
        .iter()
        .zip(dev.iter())
        .map(|(b, d)| match (b, d) {
            (Some(b), Some(d)) => Some(b + BB_MULT * d),
            _ => None,
        })
        .collect();
    let lower: Vec<Option<f64>> = basis
        .iter()
        .zip(dev.iter())
        .map(|(b, d)| match (b, d) {
            (Some(b), Some(d)) => Some(b - BB_MULT * d),
            _ => None,
        })
        .collect();
    let closes_opt: Vec<Option<f64>> = closes.iter().map(|v| Some(*v)).collect();

    // Pass 1: signal bars.
    let top_rev: Vec<bool> = (0..n)
        .map(|i| primitives::crossunder_at(&closes_opt, &upper, i))
        .collect();
    let bot_rev: Vec<bool> = (0..n)
        .map(|i| primitives::crossover_at(&closes_opt, &lower, i))
        .collect();

    // Pass 2: confirmations with exact barssince/valuewhen semantics.
    let mut events: Vec<ReversalEvent> = Vec::new();
    let mut last_top: Option<usize> = None; // most recent topRev index
    let mut last_top_conf1: Option<usize> = None;
    let mut last_bot: Option<usize> = None;
    let mut last_bot_conf1: Option<usize> = None;
    // Index into `events` of the event a confirmation should attach to.
    let mut top_event_idx: Option<usize> = None;
    let mut bot_event_idx: Option<usize> = None;

    for i in 0..n {
        if top_rev[i] {
            events.push(ReversalEvent {
                date: dates[i].clone(),
                kind: "top".to_string(),
                trigger_level: round_level(lows[i]),
                conf1_date: None,
                conf2_date: None,
                status: "unconfirmed".to_string(),
            });
            last_top = Some(i);
            top_event_idx = Some(events.len() - 1);
        }
        if bot_rev[i] {
            events.push(ReversalEvent {
                date: dates[i].clone(),
                kind: "bottom".to_string(),
                trigger_level: round_level(highs[i]),
                conf1_date: None,
                conf2_date: None,
                status: "unconfirmed".to_string(),
            });
            last_bot = Some(i);
            bot_event_idx = Some(events.len() - 1);
        }

        // topConf1 / topConf2
        if let (Some(t), Some(idx)) = (last_top, top_event_idx) {
            let level = lows[t]; // valuewhen(topRev, low, 0)
            let since = i - t; // barssince(topRev)
            if since == 1 && closes[i] < level {
                events[idx].conf1_date = Some(dates[i].clone());
                events[idx].status = "confirmed-1".to_string();
                last_top_conf1 = Some(i);
            }
            if since == 2
                && closes[i] < level
                && last_top_conf1.map(|c| i - c == 1).unwrap_or(false)
            {
                events[idx].conf2_date = Some(dates[i].clone());
                events[idx].status = "confirmed-2".to_string();
            }
        }
        // botConf1 / botConf2
        if let (Some(b), Some(idx)) = (last_bot, bot_event_idx) {
            let level = highs[b]; // valuewhen(botRev, high, 0)
            let since = i - b;
            if since == 1 && closes[i] > level {
                events[idx].conf1_date = Some(dates[i].clone());
                events[idx].status = "confirmed-1".to_string();
                last_bot_conf1 = Some(i);
            }
            if since == 2
                && closes[i] > level
                && last_bot_conf1.map(|c| i - c == 1).unwrap_or(false)
            {
                events[idx].conf2_date = Some(dates[i].clone());
                events[idx].status = "confirmed-2".to_string();
            }
        }
    }

    // Signals on the last 1–2 bars whose confirmation window is still open.
    let last = n - 1;
    for ev in events.iter_mut() {
        let still_open = match ev.status.as_str() {
            "unconfirmed" => {
                // window open if the signal bar is within the last bar
                // (conf1 pending) — conf2 requires conf1, so a missed conf1
                // closes the ladder.
                ev.date == dates[last]
            }
            "confirmed-1" => ev
                .conf1_date
                .as_deref()
                .map(|d| d == dates[last])
                .unwrap_or(false),
            _ => false,
        };
        if still_open {
            ev.status = format!("{}-pending", ev.status);
        }
    }

    if events.len() > max_events {
        events.drain(..events.len() - max_events);
    }

    Some(ReversalRead {
        bb_upper: round_level(upper[last]?),
        bb_lower: round_level(lower[last]?),
        recent: events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("d{i:04}")).collect()
    }

    /// Flat 100s, a spike close above the band, then three closes stepping
    /// down through the signal bar's low — topRev + conf1 + conf2.
    #[test]
    fn top_reversal_with_two_confirmations() {
        let mut closes = vec![100.0; 30];
        closes.push(130.0); // bar 30: close above upper (≈100, sd≈0)
        closes.push(100.0); // bar 31: crossunder ⇒ topRev (low = 99)
        closes.push(95.0); // bar 32: close < 99 ⇒ conf1
        closes.push(90.0); // bar 33: close < 99 ⇒ conf2
        closes.push(91.0); // bar 34: trailing bar so the ladder is closed
        let highs: Vec<f64> = closes.iter().map(|c| c + 1.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 1.0).collect();
        let d = dates(closes.len());
        let read = compute_reversals(&closes, &highs, &lows, &d, 10).expect("reversals");
        let top = read
            .recent
            .iter()
            .find(|e| e.kind == "top")
            .expect("top signal");
        assert_eq!(top.date, d[31]);
        assert!((top.trigger_level - 99.0).abs() < 1e-9);
        assert_eq!(top.conf1_date.as_deref(), Some(d[32].as_str()));
        assert_eq!(top.conf2_date.as_deref(), Some(d[33].as_str()));
        assert_eq!(top.status, "confirmed-2");
    }

    /// Mirror case: a flush below the lower band, then a recovery close back
    /// above it (botRev), then two closes above the signal bar's high.
    #[test]
    fn bottom_reversal_with_confirmations() {
        let mut closes = vec![100.0; 30];
        closes.push(70.0); // below lower band
        closes.push(100.0); // crossover ⇒ botRev (high = 101)
        closes.push(103.0); // conf1 (> 101)
        closes.push(105.0); // conf2
        closes.push(104.0);
        let highs: Vec<f64> = closes.iter().map(|c| c + 1.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 1.0).collect();
        let d = dates(closes.len());
        let read = compute_reversals(&closes, &highs, &lows, &d, 10).expect("reversals");
        let bot = read
            .recent
            .iter()
            .find(|e| e.kind == "bottom")
            .expect("bottom signal");
        assert_eq!(bot.date, d[31]);
        assert_eq!(bot.status, "confirmed-2");
    }

    /// conf2 requires conf1 on the immediately preceding bar — a bounce on
    /// the conf1 bar kills the second confirmation even if bar+2 is weak.
    #[test]
    fn conf2_requires_conf1_chain() {
        let mut closes = vec![100.0; 30];
        closes.push(130.0);
        closes.push(100.0); // topRev, low 99
        closes.push(100.5); // NOT below 99 ⇒ no conf1
        closes.push(90.0); // below 99 but barssince==2 with no conf1 ⇒ no conf2
        closes.push(91.0);
        let highs: Vec<f64> = closes.iter().map(|c| c + 1.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 1.0).collect();
        let d = dates(closes.len());
        let read = compute_reversals(&closes, &highs, &lows, &d, 10).expect("reversals");
        let top = read
            .recent
            .iter()
            .find(|e| e.kind == "top")
            .expect("top signal");
        assert_eq!(top.status, "unconfirmed");
        assert!(top.conf1_date.is_none() && top.conf2_date.is_none());
    }

    #[test]
    fn pending_status_on_latest_bar_signal() {
        let mut closes = vec![100.0; 30];
        closes.push(130.0);
        closes.push(100.0); // topRev on the LAST bar — window still open
        let highs: Vec<f64> = closes.iter().map(|c| c + 1.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 1.0).collect();
        let d = dates(closes.len());
        let read = compute_reversals(&closes, &highs, &lows, &d, 10).expect("reversals");
        let top = read.recent.iter().find(|e| e.kind == "top").expect("top");
        assert_eq!(top.status, "unconfirmed-pending");
    }
}
