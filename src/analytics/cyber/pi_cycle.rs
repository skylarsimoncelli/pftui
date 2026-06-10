//! Pi Cycle — component E of the Cyber Dots port.
//!
//! Pine mapping (`pi*` block): always computed on DAILY closes — the Pine
//! requests `"1D"` via `request.security` regardless of the chart timeframe,
//! so weekly runs of the Rust engine also feed daily bars here.
//!
//! - TOP: `ta.crossunder(SMA(close,350)·2, SMA(close,111))` — the 111-day
//!   SMA rising through 2× the 350-day SMA.
//! - BOTTOM: `ta.crossover(SMA(close,471)·0.745, EMA(close,150))` — the
//!   scaled long SMA crossing above the 150-day EMA (i.e. the EMA falling
//!   through it at a capitulation low).
//!
//! Proximity ratios (engine addition for "how close to firing"; both are
//! scaled so 1.0 = trigger):
//! - `top_ratio` = SMA111 / (2·SMA350) — rises toward 1.0 into a top.
//! - `bottom_ratio` = (0.745·SMA471) / EMA150 — rises toward 1.0 into a
//!   bottom.

use serde::Serialize;

use super::primitives::{self, round_level};

const TOP_LONG: usize = 350;
const TOP_SHORT: usize = 111;
const BOTTOM_LONG: usize = 471;
const BOTTOM_SHORT: usize = 150;
const BOTTOM_LONG_SCALE: f64 = 0.745;

/// Pi Cycle read: historical fires + current proximity.
#[derive(Debug, Clone, Serialize)]
pub struct PiCycleRead {
    /// All historical TOP fires in the supplied history, oldest first.
    pub top_fires: Vec<String>,
    /// All historical BOTTOM fires, oldest first.
    pub bottom_fires: Vec<String>,
    pub last_top: Option<String>,
    pub last_bottom: Option<String>,
    /// SMA111 / (2·SMA350) — 1.0 = top trigger.
    pub top_ratio: Option<f64>,
    /// (0.745·SMA471) / EMA150 — 1.0 = bottom trigger.
    pub bottom_ratio: Option<f64>,
    /// Current SMA/EMA values backing the ratios.
    pub sma_111: Option<f64>,
    pub sma_350_x2: Option<f64>,
    pub sma_471_x0745: Option<f64>,
    pub ema_150: Option<f64>,
    /// Daily bars available — fires older than the window can't be seen.
    pub daily_bars: usize,
}

/// Compute Pi Cycle on daily closes. Returns `None` only when even the TOP
/// pair (351 bars) cannot be computed; the BOTTOM side stays `None` until
/// 472 bars exist.
pub fn compute_pi_cycle(daily_closes: &[f64], daily_dates: &[String]) -> Option<PiCycleRead> {
    let n = daily_closes.len();
    if n < TOP_LONG + 1 {
        return None;
    }
    let sma_short = primitives::sma(daily_closes, TOP_SHORT);
    let sma_long2: Vec<Option<f64>> = primitives::sma(daily_closes, TOP_LONG)
        .into_iter()
        .map(|v| v.map(|x| x * 2.0))
        .collect();
    let mut top_fires = Vec::new();
    for (i, date) in daily_dates.iter().enumerate().skip(1) {
        // piCycleTop = crossunder(2·SMA350, SMA111)
        if primitives::crossunder_at(&sma_long2, &sma_short, i) {
            top_fires.push(date.clone());
        }
    }

    let mut bottom_fires = Vec::new();
    let (mut sma471s, mut ema150s): (Vec<Option<f64>>, Vec<Option<f64>>) =
        (vec![None; n], vec![None; n]);
    if n > BOTTOM_LONG {
        sma471s = primitives::sma(daily_closes, BOTTOM_LONG)
            .into_iter()
            .map(|v| v.map(|x| x * BOTTOM_LONG_SCALE))
            .collect();
        ema150s = primitives::ema(daily_closes, BOTTOM_SHORT)
            .into_iter()
            .map(Some)
            .collect();
        for (i, date) in daily_dates.iter().enumerate().skip(1) {
            // piCycleBottom = crossover(0.745·SMA471, EMA150)
            if primitives::crossover_at(&sma471s, &ema150s, i) {
                bottom_fires.push(date.clone());
            }
        }
    }

    let last = n - 1;
    let sma_111 = sma_short[last];
    let sma_350_x2 = sma_long2[last];
    let sma_471_x0745 = sma471s[last];
    let ema_150 = ema150s[last];
    let top_ratio = match (sma_111, sma_350_x2) {
        (Some(s), Some(l)) if l > 0.0 => Some(((s / l) * 1000.0).round() / 1000.0),
        _ => None,
    };
    let bottom_ratio = match (sma_471_x0745, ema_150) {
        (Some(l), Some(s)) if s > 0.0 => Some(((l / s) * 1000.0).round() / 1000.0),
        _ => None,
    };

    Some(PiCycleRead {
        last_top: top_fires.last().cloned(),
        last_bottom: bottom_fires.last().cloned(),
        top_fires,
        bottom_fires,
        top_ratio,
        bottom_ratio,
        sma_111: sma_111.map(round_level),
        sma_350_x2: sma_350_x2.map(round_level),
        sma_471_x0745: sma_471_x0745.map(round_level),
        ema_150: ema_150.map(round_level),
        daily_bars: n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("d{i:04}")).collect()
    }

    /// Synthetic golden fixture replicating a Pi Cycle TOP crossing: a long
    /// flat base (SMA111 ≈ close, 2·SMA350 ≈ 2·close, short far below long)
    /// followed by a parabolic ramp that drags the fast SMA111 up through
    /// the slow 2·SMA350. Exactly one TOP fire must be detected, after the
    /// ramp begins, and the post-fire ratio must sit above 1.0.
    #[test]
    fn synthetic_top_crossing_fires_once() {
        let mut closes = vec![100.0; 450];
        // Parabolic blow-off: +2%/bar for 150 bars (≈ 100 → 1 970).
        let mut px = 100.0;
        for _ in 0..150 {
            px *= 1.02;
            closes.push(px);
        }
        let d = dates(closes.len());
        let read = compute_pi_cycle(&closes, &d).expect("pi computes");
        assert_eq!(read.top_fires.len(), 1, "fires: {:?}", read.top_fires);
        let fire_idx: usize = read.top_fires[0][1..].parse().unwrap_or(0);
        assert!(fire_idx > 450, "fire must be inside the ramp, got {fire_idx}");
        assert!(read.top_ratio.unwrap_or_default() > 1.0);
        assert_eq!(read.last_top, read.top_fires.last().cloned());
    }

    /// Mirror fixture for a BOTTOM crossing: long elevated base then a deep
    /// decline — the EMA150 collapses below 0.745·SMA471 and a single
    /// crossover (long over short) fires.
    #[test]
    fn synthetic_bottom_crossing_fires() {
        let mut closes = vec![1000.0; 600];
        let mut px = 1000.0;
        for _ in 0..200 {
            px *= 0.985;
            closes.push(px);
        }
        let d = dates(closes.len());
        let read = compute_pi_cycle(&closes, &d).expect("pi computes");
        assert!(
            !read.bottom_fires.is_empty(),
            "expected a bottom fire, ratios: top {:?} bottom {:?}",
            read.top_ratio,
            read.bottom_ratio
        );
        let fire_idx: usize = read.bottom_fires[0][1..].parse().unwrap_or(0);
        assert!(fire_idx > 600, "fire must be inside the decline, got {fire_idx}");
        assert!(read.bottom_ratio.unwrap_or_default() > 1.0);
    }

    #[test]
    fn flat_series_never_fires_and_reports_ratios() {
        let closes = vec![100.0; 700];
        let d = dates(700);
        let read = compute_pi_cycle(&closes, &d).expect("pi computes");
        assert!(read.top_fires.is_empty());
        assert!(read.bottom_fires.is_empty());
        // Flat: SMA111 = 100, 2·SMA350 = 200 ⇒ top ratio 0.5;
        // 0.745·SMA471 = 74.5, EMA150 = 100 ⇒ bottom ratio 0.745.
        assert!((read.top_ratio.unwrap_or_default() - 0.5).abs() < 1e-9);
        assert!((read.bottom_ratio.unwrap_or_default() - 0.745).abs() < 1e-9);
    }

    #[test]
    fn short_history_returns_none_then_top_only() {
        let closes = vec![100.0; 300];
        assert!(compute_pi_cycle(&closes, &dates(300)).is_none());
        let closes = vec![100.0; 400];
        let read = compute_pi_cycle(&closes, &dates(400)).expect("top pair fits");
        assert!(read.top_ratio.is_some());
        assert!(read.bottom_ratio.is_none(), "bottom needs 471 bars");
    }
}
