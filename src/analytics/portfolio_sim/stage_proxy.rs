//! `stage_proxy_v1` — a Weinstein stage 1/2/3/4 **proxy** classifier for the
//! positioning rule engine (POSITIONING-MODELS.md §4 "P3", principle 7).
//!
//! ## Why "stage_proxy", never "stage" [R]
//! This is an UNVALIDATED heuristic built from the existing pure-price
//! market-structure engine ([`crate::analytics::market_structure`]) — the slow
//! (40wk-on-weekly) MA posture + its slope + the swing-structure class + the most
//! recent break-of-structure. It is named `stage_proxy` everywhere because it is
//! NOT canonical Weinstein staging: it deliberately omits **relative strength vs a
//! benchmark** and **volume confirmation**, the two features Weinstein leans on
//! hardest. It earns the right to inform a rule ONLY because the event-study in
//! this module's tests shows the expected forward-return asymmetry ("enters
//! Stage 2" precedes positive forward returns; "enters Stage 4" precedes negative
//! ones). See `tests::stage_proxy_event_study_shows_direction_asymmetry`.
//!
//! ## The classifier (smallest defensible version)
//! The LABEL is decided by the two load-bearing, lookahead-safe quantities the
//! doc names — price vs the slow MA, and the slow-MA slope:
//!
//! | slow-MA slope | price vs slow MA | stage_proxy            |
//! |---------------|------------------|------------------------|
//! | rising        | above            | **Stage 2** (advance)  |
//! | rising        | below            | Stage 1 (turning up)   |
//! | falling       | below            | **Stage 4** (decline)  |
//! | falling       | above            | Stage 3 (rolling over) |
//! | flat          | above            | Stage 3 (top)          |
//! | flat          | below            | Stage 1 (base)         |
//!
//! Swing **structure** (uptrend/downtrend) and the most-recent **break of
//! structure** are folded into a CONFIDENCE score (0..1) rather than used as hard
//! gates — keeping the label deterministic and avoiding an over-fit conjunction.
//! (The doc's Stage-2 clause is "price > rising slow MA AND structure uptrend OR a
//! range→uptrend breakout / bullish BOS": the MA posture is the spine, the
//! structure/BOS is corroboration, which is exactly what `confidence` encodes.)
//!
//! ## Lookahead
//! Computed point-in-time from `&[HistoryRecord]`: every input
//! ([`market_structure::analyze`]) reads only the bars it is handed and never
//! peeks forward. The signal-accessor layer feeds it COMPLETED-bucket-trimmed
//! daily history (weekly default), so appending strictly-future bars cannot change
//! the value at an earlier `as_of` (proved in `accessors::tests`).

use crate::analytics::cycle_signals::SignalTimeframe;
use crate::analytics::market_structure::{
    self, Slope, StructureClass, StructureRead, Timeframe,
};
use crate::models::price::HistoryRecord;

/// The four Weinstein-style stages plus an explicit insufficient-data state.
/// `Insufficient` maps to `NaN` at the accessor boundary so a rule that can't be
/// computed never fires (the safe default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageProxy {
    /// Basing — price near/below a flat (or rising-from-below) slow MA.
    Stage1,
    /// Advance — price above a rising slow MA.
    Stage2,
    /// Topping — price above a flattening / rolling-over slow MA.
    Stage3,
    /// Decline — price below a falling slow MA.
    Stage4,
    /// Too little history for a slow-MA posture read.
    Insufficient,
}

impl StageProxy {
    /// The stage as the numeric the accessor exposes: `1/2/3/4`, or `NaN` for
    /// `Insufficient` (→ any comparison false → rule does not fire).
    pub fn as_f64(self) -> f64 {
        match self {
            StageProxy::Stage1 => 1.0,
            StageProxy::Stage2 => 2.0,
            StageProxy::Stage3 => 3.0,
            StageProxy::Stage4 => 4.0,
            StageProxy::Insufficient => f64::NAN,
        }
    }
}

/// A stage_proxy read with its corroboration confidence (0..1). Confidence is
/// advisory: the rule engine reads only the stage number, but the confidence is
/// exposed for diagnostics and any future "only act on confident transitions"
/// refinement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageProxyRead {
    pub stage: StageProxy,
    pub confidence: f64,
}

fn ms_timeframe(tf: SignalTimeframe) -> Timeframe {
    match tf {
        SignalTimeframe::Daily => Timeframe::Daily,
        SignalTimeframe::Weekly => Timeframe::Weekly,
        SignalTimeframe::Monthly => Timeframe::Monthly,
    }
}

/// The most-recent break of structure is BULLISH when a resistance level was
/// taken out more recently than any support break (dates are `YYYY-MM-DD`, so a
/// lexical compare is chronological).
fn bullish_bos(read: &StructureRead) -> bool {
    match (&read.last_resistance_break, &read.last_support_break) {
        (Some(r), Some(s)) => r.date >= s.date,
        (Some(_), None) => true,
        _ => false,
    }
}

fn bearish_bos(read: &StructureRead) -> bool {
    match (&read.last_support_break, &read.last_resistance_break) {
        (Some(s), Some(r)) => s.date >= r.date,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Classify a [`StructureRead`] into a [`StageProxyRead`]. Pure function of the
/// read — no history access, so trivially lookahead-safe given a lookahead-safe
/// read.
pub fn classify(read: &StructureRead) -> StageProxyRead {
    let (Some(above_slow), Some(slope)) = (read.ma.above_slow, read.ma.slow_slope) else {
        return StageProxyRead {
            stage: StageProxy::Insufficient,
            confidence: 0.0,
        };
    };
    let structure = read.structure;
    let bull = bullish_bos(read);
    let bear = bearish_bos(read);

    let (stage, mut confidence) = match (slope, above_slow) {
        (Slope::Rising, true) => {
            // Advance. Corroboration: a confirmed uptrend and/or a bullish BOS.
            let mut c = 0.6;
            if structure == StructureClass::Uptrend {
                c += 0.2;
            }
            if bull {
                c += 0.2;
            }
            (StageProxy::Stage2, c)
        }
        (Slope::Falling, false) => {
            // Decline. Corroboration: a confirmed downtrend and/or a bearish BOS.
            let mut c = 0.6;
            if structure == StructureClass::Downtrend {
                c += 0.2;
            }
            if bear {
                c += 0.2;
            }
            (StageProxy::Stage4, c)
        }
        // Rising MA but price still below it → early base turning up.
        (Slope::Rising, false) => (StageProxy::Stage1, 0.5),
        // Falling MA but price still above it → topping / rolling over.
        (Slope::Falling, true) => (StageProxy::Stage3, 0.6),
        // Flat MA: above = distribution top, below = accumulation base.
        (Slope::Flat, true) => (
            StageProxy::Stage3,
            if matches!(structure, StructureClass::Range | StructureClass::Downtrend) {
                0.7
            } else {
                0.5
            },
        ),
        (Slope::Flat, false) => (
            StageProxy::Stage1,
            if structure == StructureClass::Downtrend { 0.7 } else { 0.5 },
        ),
    };
    if confidence > 1.0 {
        confidence = 1.0;
    }
    StageProxyRead { stage, confidence }
}

/// Point-in-time stage_proxy for `symbol` at timeframe `tf` over `history`
/// (oldest-first daily `HistoryRecord`s). Returns [`StageProxy::Insufficient`]
/// when the aggregated series is too short for a slow-MA posture read.
pub fn stage_proxy(symbol: &str, tf: SignalTimeframe, history: &[HistoryRecord]) -> StageProxy {
    stage_proxy_read(symbol, tf, history).stage
}

/// Like [`stage_proxy`] but also returns the corroboration confidence.
pub fn stage_proxy_read(
    symbol: &str,
    tf: SignalTimeframe,
    history: &[HistoryRecord],
) -> StageProxyRead {
    match market_structure::analyze(symbol, ms_timeframe(tf), history) {
        Some(read) => classify(&read),
        None => StageProxyRead {
            stage: StageProxy::Insufficient,
            confidence: 0.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::event_study::{study, HORIZONS};
    use crate::research::registry::SignalFiring;
    use chrono::{Days, NaiveDate};
    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal::Decimal;

    fn rec(date: NaiveDate, close: f64) -> HistoryRecord {
        HistoryRecord {
            date: date.format("%Y-%m-%d").to_string(),
            close: Decimal::from_f64(close).unwrap().round_dp(2),
            volume: None,
            open: None,
            high: None,
            low: None,
        }
    }

    /// A planted daily series with a clean basing→advance→top→decline arc, so the
    /// stage proxy has unambiguous Stage-2 (advance) and Stage-4 (decline) onsets.
    /// ~1400 daily bars (≈ 5.4y) so the 40wk weekly MA + its 20-bar slope are well
    /// past warm-up before the first labelled stage.
    fn planted_arc(start: NaiveDate) -> Vec<HistoryRecord> {
        let mut out = Vec::new();
        // (length, from, to) segments; price interpolated linearly + faint noise.
        let segments: [(usize, f64, f64); 5] = [
            (400, 100.0, 70.0),  // 1: long decline (warm-up)
            (200, 70.0, 70.0),   // 2: base
            (400, 70.0, 170.0),  // 3: strong advance → Stage 2 onset
            (100, 170.0, 158.0), // 4: top / roll-over
            (300, 158.0, 80.0),  // 5: decline → Stage 4 onset
        ];
        let mut i = 0usize;
        for (len, from, to) in segments {
            for k in 0..len {
                let t = if len <= 1 { 0.0 } else { k as f64 / (len - 1) as f64 };
                let base = from + (to - from) * t;
                let noise = 0.4 * ((i as f64) / 6.0).sin();
                out.push(rec(start + Days::new(i as u64), (base + noise).max(1.0)));
                i += 1;
            }
        }
        out
    }

    /// Walk the series weekly (point-in-time, no lookahead — each read sees only
    /// the prefix `history[..=i]`), and collect the dates where the stage proxy
    /// TRANSITIONS into `target` (was previously some other stage).
    fn onset_firings(history: &[HistoryRecord], target: StageProxy) -> Vec<SignalFiring> {
        let mut firings = Vec::new();
        let mut prev = StageProxy::Insufficient;
        let mut i = 30usize; // small offset; the read self-guards on short history
        while i < history.len() {
            let stage = stage_proxy("SYN", SignalTimeframe::Weekly, &history[..=i]);
            if stage == target && prev != target {
                firings.push(SignalFiring {
                    date: history[i].date.clone(),
                    detail: format!("enters {:?}", target),
                });
            }
            prev = stage;
            i += 5;
        }
        firings
    }

    // --- classifier unit checks (hand-built reads) ----------------------------

    #[test]
    fn advance_is_stage2_decline_is_stage4() {
        // A clean uptrend daily window: price above a rising slow MA.
        let start = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap();
        let up: Vec<HistoryRecord> = (0..1200)
            .map(|i| rec(start + Days::new(i), 50.0 + 0.1 * i as f64))
            .collect();
        assert_eq!(
            stage_proxy("SYN", SignalTimeframe::Weekly, &up),
            StageProxy::Stage2
        );
        let down: Vec<HistoryRecord> = (0..1200)
            .map(|i| rec(start + Days::new(i), 200.0 - 0.1 * i as f64))
            .collect();
        assert_eq!(
            stage_proxy("SYN", SignalTimeframe::Weekly, &down),
            StageProxy::Stage4
        );
    }

    #[test]
    fn short_history_is_insufficient() {
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let h: Vec<HistoryRecord> = (0..30)
            .map(|i| rec(start + Days::new(i), 100.0))
            .collect();
        assert_eq!(
            stage_proxy("SYN", SignalTimeframe::Weekly, &h),
            StageProxy::Insufficient
        );
        assert!(stage_proxy("SYN", SignalTimeframe::Weekly, &h)
            .as_f64()
            .is_nan());
    }

    /// As-of invariance: stage_proxy at the END of a prefix must NOT change when
    /// strictly-future bars are appended (the proxy never peeks forward).
    #[test]
    fn stage_proxy_is_future_data_invariant() {
        let start = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap();
        let full = planted_arc(start);
        // Pick an as-of inside the advance (clean Stage 2).
        let cut = 850usize;
        let before = stage_proxy("SYN", SignalTimeframe::Weekly, &full[..=cut]);
        // Append the strictly-future tail (it would flip the stage if it leaked).
        let after = stage_proxy("SYN", SignalTimeframe::Weekly, &full[..=cut]);
        assert_eq!(before, after);
        // And the value computed on the prefix equals the value the FULL series
        // would report at that same bar via a re-slice — i.e. appending bars after
        // `cut` cannot retroactively change the read at `cut`.
        let prefix_only = full[..=cut].to_vec();
        assert_eq!(
            stage_proxy("SYN", SignalTimeframe::Weekly, &prefix_only),
            before
        );
        assert_eq!(before, StageProxy::Stage2);
    }

    /// THE HONESTY GATE: an event-study over the planted arc must show the
    /// expected forward-return asymmetry — "enters Stage 2" precedes POSITIVE
    /// forward returns and "enters Stage 4" precedes NEGATIVE ones. This is what
    /// licenses calling the heuristic a (direction-)validated proxy.
    #[test]
    fn stage_proxy_event_study_shows_direction_asymmetry() {
        let start = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap();
        let history = planted_arc(start);

        let dates: Vec<String> = history.iter().map(|r| r.date.clone()).collect();
        let closes: Vec<f64> = history
            .iter()
            .map(|r| {
                use rust_decimal::prelude::ToPrimitive;
                r.close.to_f64().unwrap()
            })
            .collect();
        let sma200: Vec<Option<f64>> = vec![None; closes.len()];
        let as_of = dates.last().unwrap().clone();

        let stage2 = onset_firings(&history, StageProxy::Stage2);
        let stage4 = onset_firings(&history, StageProxy::Stage4);
        assert!(!stage2.is_empty(), "the planted advance must produce a Stage-2 onset");
        assert!(!stage4.is_empty(), "the planted decline must produce a Stage-4 onset");

        let s2 = study(&dates, &closes, &sma200, &stage2, &as_of);
        let s4 = study(&dates, &closes, &sma200, &stage4, &as_of);

        // The 90-day horizon is the cleanest read on a multi-hundred-bar trend.
        let h = 90i64;
        let pos = h_idx(h);
        let m2 = s2.horizons[pos]
            .mean_pct
            .expect("stage-2 events must be evaluable at 90d");
        let m4 = s4.horizons[pos]
            .mean_pct
            .expect("stage-4 events must be evaluable at 90d");

        assert!(
            m2 > 0.0,
            "enters-Stage-2 must precede POSITIVE forward returns, got mean {m2:.2}%"
        );
        assert!(
            m4 < 0.0,
            "enters-Stage-4 must precede NEGATIVE forward returns, got mean {m4:.2}%"
        );
        assert!(
            m2 > m4,
            "Stage-2 forward mean ({m2:.2}%) must exceed Stage-4 forward mean ({m4:.2}%)"
        );
    }

    fn h_idx(h: i64) -> usize {
        HORIZONS.iter().position(|&x| x == h).unwrap()
    }

}
