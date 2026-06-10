//! CyberLine — component B of the Cyber Dots port.
//!
//! Pine mapping:
//! - The "Volatility Weighted" mode is the custom VIDYA recursion
//!   (`cyberLine*` block) → [`vidya_series`]: recursive ±DM smoothing →
//!   directional index → normalized volatility index over `len` →
//!   `trend := (1 − k·VI)·trend[1] + k·VI·close` with `k = 1/len`.
//!   Default sensitivity Medium ⇒ `len = 18` (Fast 9 / Slow 27).
//! - "Donchian Channel" mode → [`donchian_trend_series`]: midline average of
//!   the Donchian midpoints at the conversion length (5) and the Donchian
//!   length (26) — `math.avg(donchian(5), donchian(26))` where
//!   `donchian(len) = avg(ta.lowest(len), ta.highest(len))` (lowest low /
//!   highest high).
//! - "Hybrid" mode: `(1 − w)·volatility + w·donchian`, default weight 0.5.
//!
//! Documented adaptation (div-by-zero guards): Pine produces `na` for the
//! bar when `pdmS+mdmS == 0`, `pdiS+mdiS == 0`, or the volatility-index range
//! is 0, then resumes from `nz(prev) = 0` — which would collapse the trend
//! line to zero after a perfectly flat stretch. We instead CARRY THE PREVIOUS
//! VALUE of the affected recursion for that bar. On real price data the
//! denominators are never exactly zero after warm-up, so the two behaviours
//! agree everywhere it matters; the guard only changes pathological synthetic
//! inputs. Warm-up also differs benignly: Pine's leading-na bars (`nz` → 0
//! seeds) are reproduced by starting every recursion at 0, and the normalized
//! volatility index only engages once `len` bars of the smoothed directional
//! index exist (before that the trend carries, matching Pine's na-window).

use serde::Serialize;

use super::primitives::{self, round_level};

/// Per-bar VIDYA ("Volatility Weighted") trend line. Shared by CyberLine
/// (len 18) and the CyberDots VMA (len 4) — the Pine `cyberDotsVma*` block is
/// the same recursion on close.
pub(super) fn vidya_series(closes: &[f64], len: usize) -> Vec<f64> {
    let n = closes.len();
    let mut trend = vec![0.0; n];
    if n == 0 || len == 0 {
        return trend;
    }
    let k = 1.0 / len as f64;
    let mut pdm_s = 0.0;
    let mut mdm_s = 0.0;
    let mut pdi_s = 0.0;
    let mut mdi_s = 0.0;
    let mut i_s = 0.0;
    // Rolling window of the smoothed directional index for highest/lowest.
    let mut i_hist: Vec<f64> = Vec::with_capacity(n);
    let mut prev_trend = 0.0;
    for t in 1..n {
        let up = (closes[t] - closes[t - 1]).max(0.0);
        let dn = (closes[t - 1] - closes[t]).max(0.0);
        pdm_s = (1.0 - k) * pdm_s + k * up;
        mdm_s = (1.0 - k) * mdm_s + k * dn;
        let sum = pdm_s + mdm_s;
        if sum > 0.0 {
            // Guard: sum == 0 (flat so far) ⇒ carry previous pdi_s/mdi_s.
            let pdi = pdm_s / sum;
            let mdi = mdm_s / sum;
            pdi_s = (1.0 - k) * pdi_s + k * pdi;
            mdi_s = (1.0 - k) * mdi_s + k * mdi;
        }
        let sum1 = pdi_s + mdi_s;
        if sum1 > 0.0 {
            // Guard: sum1 == 0 ⇒ carry previous i_s.
            let diff = (pdi_s - mdi_s).abs();
            i_s = (1.0 - k) * i_s + k * (diff / sum1);
        }
        i_hist.push(i_s);

        let mut next = prev_trend;
        if i_hist.len() >= len {
            let window = &i_hist[i_hist.len() - len..];
            let hh = window.iter().copied().fold(f64::MIN, f64::max);
            let ll = window.iter().copied().fold(f64::MAX, f64::min);
            let range = hh - ll;
            if range > 0.0 {
                let vi = (i_s - ll) / range;
                next = (1.0 - k * vi) * prev_trend + k * vi * closes[t];
            }
            // Guard: range == 0 ⇒ carry previous trend value.
        }
        trend[t] = next;
        prev_trend = next;
    }
    trend
}

/// Donchian midline trend: `avg(donchian(conversion), donchian(base))`,
/// `None` until the longer window fills.
pub(super) fn donchian_trend_series(
    highs: &[f64],
    lows: &[f64],
    conversion_len: usize,
    base_len: usize,
) -> Vec<Option<f64>> {
    let hi_c = primitives::highest(highs, conversion_len);
    let lo_c = primitives::lowest(lows, conversion_len);
    let hi_b = primitives::highest(highs, base_len);
    let lo_b = primitives::lowest(lows, base_len);
    (0..highs.len())
        .map(|i| match (hi_c[i], lo_c[i], hi_b[i], lo_b[i]) {
            (Some(hc), Some(lc), Some(hb), Some(lb)) => {
                let conversion = (hc + lc) / 2.0;
                let base = (hb + lb) / 2.0;
                Some((conversion + base) / 2.0)
            }
            _ => None,
        })
        .collect()
}

/// Slope of the line on the latest bar (Pine colors the line by exact
/// `trend > trend[1]` / `<` comparison; equality ⇒ flat/caution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineSlope {
    Up,
    Down,
    Flat,
}

impl LineSlope {
    pub fn label(self) -> &'static str {
        match self {
            LineSlope::Up => "up",
            LineSlope::Down => "down",
            LineSlope::Flat => "flat",
        }
    }
}

/// Most recent close/line cross (Pine `ta.crossover/crossunder(close, trend)`
/// alert conditions).
#[derive(Debug, Clone, Serialize)]
pub struct LineCross {
    pub date: String,
    /// "above" — price crossed above the line; "below" — below.
    pub direction: String,
}

/// CyberLine read for the latest bar (all three modes emitted; the headline
/// value is the default Volatility Weighted / Medium line).
#[derive(Debug, Clone, Serialize)]
pub struct LineRead {
    /// Volatility Weighted (default mode, Medium sensitivity len 18).
    pub value: f64,
    pub slope: LineSlope,
    /// True when the latest close is above the line.
    pub price_above: bool,
    /// Most recent price/line cross with date (scans the full series).
    pub last_cross: Option<LineCross>,
    /// Donchian Channel mode value (conversion 5 / base 26), if computable.
    pub donchian_value: Option<f64>,
    /// Hybrid mode (weight 0.5) value, if Donchian is computable.
    pub hybrid_value: Option<f64>,
}

const SENSITIVITY_MEDIUM_LEN: usize = 18;
const CONVERSION_LEN: usize = 5;
const DONCHIAN_LEN: usize = 26;
const HYBRID_WEIGHT: f64 = 0.5;

/// Compute the CyberLine read over the full series.
pub fn compute_line(
    closes: &[f64],
    highs: &[f64],
    lows: &[f64],
    dates: &[String],
) -> Option<LineRead> {
    // Need the VIDYA warm-up (≈2·len) plus the volatility-index window.
    if closes.len() < SENSITIVITY_MEDIUM_LEN * 3 {
        return None;
    }
    let trend = vidya_series(closes, SENSITIVITY_MEDIUM_LEN);
    let n = closes.len();
    let last = n - 1;

    let slope = if trend[last] > trend[last - 1] {
        LineSlope::Up
    } else if trend[last] < trend[last - 1] {
        LineSlope::Down
    } else {
        LineSlope::Flat
    };

    // Last price/line cross — Pine crossover/crossunder on (close, trend).
    let closes_opt: Vec<Option<f64>> = closes.iter().map(|v| Some(*v)).collect();
    let trend_opt: Vec<Option<f64>> = trend.iter().map(|v| Some(*v)).collect();
    let mut last_cross = None;
    for i in (1..n).rev() {
        if primitives::crossover_at(&closes_opt, &trend_opt, i) {
            last_cross = Some(LineCross {
                date: dates[i].clone(),
                direction: "above".to_string(),
            });
            break;
        }
        if primitives::crossunder_at(&closes_opt, &trend_opt, i) {
            last_cross = Some(LineCross {
                date: dates[i].clone(),
                direction: "below".to_string(),
            });
            break;
        }
    }

    let donchian = donchian_trend_series(highs, lows, CONVERSION_LEN, DONCHIAN_LEN);
    let donchian_value = donchian[last];
    let hybrid_value =
        donchian_value.map(|d| (1.0 - HYBRID_WEIGHT) * trend[last] + HYBRID_WEIGHT * d);

    Some(LineRead {
        value: round_level(trend[last]),
        slope,
        price_above: closes[last] > trend[last],
        last_cross,
        donchian_value: donchian_value.map(round_level),
        hybrid_value: hybrid_value.map(round_level),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("2025-{:02}-{:02}", 1 + i / 28, 1 + i % 28)).collect()
    }

    #[test]
    fn vidya_flat_series_carries_zero_without_nan() {
        // Perfectly flat input: every denominator is zero — the guards must
        // carry values (here the initial 0) and never emit NaN.
        let trend = vidya_series(&[100.0; 60], 18);
        for v in &trend {
            assert!(v.is_finite());
        }
        assert_eq!(trend[59], 0.0);
    }

    #[test]
    fn vidya_uptrend_rises_and_tracks_below_price() {
        let closes: Vec<f64> = (0..200).map(|i| 100.0 + i as f64).collect();
        let trend = vidya_series(&closes, 18);
        let last = trend[199];
        let prev = trend[198];
        assert!(last > prev, "trend should rise in an uptrend");
        assert!(last > 0.0 && last < closes[199], "lags below price, got {last}");
    }

    #[test]
    fn donchian_midline_hand_calc() {
        // Constant high=10/low=6 → conversion = base = 8 → midline 8.
        let highs = vec![10.0; 30];
        let lows = vec![6.0; 30];
        let d = donchian_trend_series(&highs, &lows, 5, 26);
        assert_eq!(d[24], None); // base window not filled yet
        assert!((d[25].unwrap_or_default() - 8.0).abs() < 1e-12);
    }

    #[test]
    fn line_read_reports_slope_cross_and_hybrid() {
        // Ramp up then a sharp drop below the (lagging) line at the end —
        // slope context + a "below" cross on the final bars.
        let mut closes: Vec<f64> = (0..120).map(|i| 100.0 + i as f64).collect();
        closes.extend([150.0, 140.0, 130.0]);
        let highs: Vec<f64> = closes.iter().map(|c| c + 1.0).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 1.0).collect();
        let read = compute_line(&closes, &highs, &lows, &dates(closes.len())).expect("line");
        let cross = read.last_cross.expect("cross exists");
        assert_eq!(cross.direction, "below");
        assert!(!read.price_above);
        let don = read.donchian_value.expect("donchian");
        let hyb = read.hybrid_value.expect("hybrid");
        assert!((hyb - (0.5 * read.value + 0.5 * don)).abs() < 0.01);
    }
}
