//! CyberBands — component A of the Cyber Dots port.
//!
//! Pine mapping:
//! - `F_DEMA` → [`primitives::dema`]
//! - `F_Gaussian` → [`gaussian_filter`] — weighted sum over the last `len`
//!   bars where Pine `src[i]` indexes BACKWARDS (i = 0 is the current bar)
//!   and `weight_i = exp(−0.5·((i − (len−1)/2) / σ)²)`. Pine `/` is float
//!   division, so the centre `(len−1)/2` is 1.5 for the default len 4 and the
//!   weights are symmetric — but we still apply them in Pine's backwards
//!   orientation so non-default even/odd lengths stay faithful.
//! - `F_SMMA` → [`smma_quirk`] — **ported semantics of the Pine quirk**: the
//!   function recomputes per bar; for `bar_index < len−1` it assigns
//!   `SMMA := src`, and from bar `len−1` on `SMMA := prev + (src − prev)/len`
//!   where `prev` is the previous bar's value. Because every earlier bar
//!   already seeded SMMA with `src` (and the script's EMA chain is defined
//!   from bar 0), `SMMA[1]` is never `na` on the first smoothed bar — the
//!   internal `na(SMMA[1]) ? dema : …` branch is dead code. We therefore seed
//!   with `src` and smooth; the DEMA-reseed branch is intentionally not
//!   ported (documented adaptation, observably identical).
//! - Bands: `SMMA ± ta.stdev(SMMA, 30) · mult` (population stdev), mult up
//!   2.5 / down 1.8.
//! - QB state machine: `var QB = 0`; `close > upper ⇒ 1`; `close < lower ⇒
//!   −1`; else hold. (The Pine evaluates `Long_C and not Short_C` then
//!   `Short_C`, so "lower wins" if both were ever true — impossible while
//!   `upper ≥ lower`, but the ordering is preserved.) `QB == 0` therefore
//!   means "caution — neither band breached since series start".
//! - Zone Based mode → [`compute_zone_bands`]: EMA 144/233 scaled by the
//!   inner/outer zone scales (×2 inner, ÷2 outer), `band_th`/`band_lum`/
//!   `multiScaleEMA` logic, and the timeframe-adaptation multiplier from the
//!   Pine table (≤60min → 1.0, daily → 0.7, weekly → 0.4, monthly+ → 0.2).

use serde::Serialize;

use super::primitives::{self, round_level};
use super::CyberTimeframe;

/// Persistent band trend state (`QB` in the Pine source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QbState {
    Bullish,
    Bearish,
    /// Neither band has ever been breached (initial state 0).
    Caution,
}

impl QbState {
    pub fn label(self) -> &'static str {
        match self {
            QbState::Bullish => "bullish",
            QbState::Bearish => "bearish",
            QbState::Caution => "caution-never-breached",
        }
    }
    fn from_i8(v: i8) -> Self {
        match v {
            1 => QbState::Bullish,
            -1 => QbState::Bearish,
            _ => QbState::Caution,
        }
    }
}

/// A dated QB flip (state machine transition).
#[derive(Debug, Clone, Serialize)]
pub struct QbTransition {
    pub date: String,
    pub from: QbState,
    pub to: QbState,
}

/// Gaussian-channel read for the latest bar plus the QB state history.
#[derive(Debug, Clone, Serialize)]
pub struct GaussianBandsRead {
    pub smma: f64,
    pub upper: f64,
    pub lower: f64,
    pub qb: QbState,
    /// Date the current QB state was entered (None while still Caution).
    pub qb_since: Option<String>,
    /// Bars (inclusive of the latest) spent in the current QB state.
    pub qb_bars: usize,
    /// Most recent transitions, oldest first (capped by the caller's window).
    pub transitions: Vec<QbTransition>,
    /// Full per-bar QB series (internal — used by the breakout gate).
    #[serde(skip)]
    pub qb_series: Vec<i8>,
}

/// Pine `F_Gaussian` — backwards-indexed Gaussian-weighted mean over the last
/// `len` bars. Undefined (`None`) until `len` bars exist (Pine: `src[i]` is na
/// before that, and na poisons the weighted sum).
fn gaussian_filter(src: &[f64], len: usize, sigma: f64) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if len == 0 || src.len() < len {
        return out;
    }
    let sigma = sigma.max(1e-9);
    let mu = (len as f64 - 1.0) / 2.0; // Pine float division
    let weights: Vec<f64> = (0..len)
        .map(|i| (-0.5 * ((i as f64 - mu) / sigma).powi(2)).exp())
        .collect();
    let wsum: f64 = weights.iter().sum();
    for t in (len - 1)..src.len() {
        // Pine src[i]: i = 0 is the current bar t, i = len-1 is bar t-len+1.
        let acc: f64 = weights
            .iter()
            .enumerate()
            .map(|(i, w)| src[t - i] * w)
            .sum();
        out[t] = Some(acc / wsum);
    }
    out
}

/// Pine `F_SMMA` quirk semantics (see module docs): `src` for the first
/// `len−1` bars, then `prev + (src − prev)/len`. Input may have an undefined
/// prefix (the Gaussian warm-up); the output starts where the input starts.
fn smma_quirk(src: &[Option<f64>], len: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; src.len()];
    if len == 0 {
        return out;
    }
    let mut prev: Option<f64> = None;
    for (i, v) in src.iter().enumerate() {
        let Some(v) = *v else {
            continue;
        };
        let next = if i < len - 1 {
            // bar_index < len−1 branch: SMMA := src.
            v
        } else {
            match prev {
                Some(p) => p + (v - p) / len as f64,
                // First defined bar lands at/after len−1 (deep Gaussian
                // warm-up): seed with src — the Pine DEMA-reseed branch is
                // dead in practice (module docs).
                None => v,
            }
        };
        out[i] = Some(next);
        prev = Some(next);
    }
    out
}

pub(super) struct GaussianParams {
    pub dema_len: usize,
    pub gaussian_len: usize,
    pub gaussian_sigma: f64,
    pub smma_len: usize,
    pub sd_len: usize,
    pub mult_up: f64,
    pub mult_dn: f64,
}

impl Default for GaussianParams {
    fn default() -> Self {
        Self {
            dema_len: 7,
            gaussian_len: 4,
            gaussian_sigma: 2.0,
            smma_len: 12,
            sd_len: 30,
            mult_up: 2.5,
            mult_dn: 1.8,
        }
    }
}

/// Compute the Gaussian-channel bands + QB state machine over the full series.
pub fn compute_gaussian_bands(
    closes: &[f64],
    dates: &[String],
    max_transitions: usize,
) -> Option<GaussianBandsRead> {
    compute_gaussian_bands_with(closes, dates, &GaussianParams::default(), max_transitions)
}

pub(super) fn compute_gaussian_bands_with(
    closes: &[f64],
    dates: &[String],
    p: &GaussianParams,
    max_transitions: usize,
) -> Option<GaussianBandsRead> {
    if closes.len() < p.sd_len + p.gaussian_len + p.smma_len {
        return None;
    }
    let dema = primitives::dema(closes, p.dema_len);
    let gauss = gaussian_filter(&dema, p.gaussian_len, p.gaussian_sigma);
    let smma = smma_quirk(&gauss, p.smma_len);

    // stdev over the SMMA series — Pine sees the same line with a leading na
    // prefix; the window only fills where `sd_len` consecutive values exist.
    let smma_dense: Vec<f64> = smma.iter().copied().flatten().collect();
    let offset = smma.len() - smma_dense.len();
    let sd_dense = primitives::stdev_pop(&smma_dense, p.sd_len);

    let n = closes.len();
    let mut upper = vec![None; n];
    let mut lower = vec![None; n];
    for i in 0..smma_dense.len() {
        if let Some(sd) = sd_dense[i] {
            upper[offset + i] = Some(smma_dense[i] + sd * p.mult_up);
            lower[offset + i] = Some(smma_dense[i] - sd * p.mult_dn);
        }
    }

    // QB state machine — starts 0; Pine ordering: bull set only when not
    // simultaneously short, short always wins.
    let mut qb: i8 = 0;
    let mut qb_series = Vec::with_capacity(n);
    let mut transitions: Vec<QbTransition> = Vec::new();
    for i in 0..n {
        let prev = qb;
        if let (Some(u), Some(l)) = (upper[i], lower[i]) {
            let long_c = closes[i] > u;
            let short_c = closes[i] < l;
            if long_c && !short_c {
                qb = 1;
            }
            if short_c {
                qb = -1;
            }
        }
        if qb != prev {
            transitions.push(QbTransition {
                date: dates[i].clone(),
                from: QbState::from_i8(prev),
                to: QbState::from_i8(qb),
            });
        }
        qb_series.push(qb);
    }

    let last = n - 1;
    let (u, l, m) = (upper[last]?, lower[last]?, smma[last]?);
    let mut qb_bars = 1usize;
    while qb_bars < n && qb_series[last - qb_bars] == qb_series[last] {
        qb_bars += 1;
    }
    let qb_since = if qb_series[last] == 0 {
        None
    } else {
        Some(dates[last + 1 - qb_bars].clone())
    };
    if transitions.len() > max_transitions {
        transitions.drain(..transitions.len() - max_transitions);
    }

    Some(GaussianBandsRead {
        smma: round_level(m),
        upper: round_level(u),
        lower: round_level(l),
        qb: QbState::from_i8(qb_series[last]),
        qb_since,
        qb_bars,
        transitions,
        qb_series,
    })
}

// ---------------------------------------------------------------------------
// Zone Based mode
// ---------------------------------------------------------------------------

/// Zone classification of the latest bar (Pine `is_uuz`/`is_ulz`/`is_luz`/
/// `is_llz`). `Between` covers the configurations the Pine flags don't match
/// (e.g. outer band sitting inside the inner zone span).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZoneState {
    UpperOuter,
    UpperInner,
    LowerInner,
    LowerOuter,
    Between,
}

impl ZoneState {
    pub fn label(self) -> &'static str {
        match self {
            ZoneState::UpperOuter => "upper-outer",
            ZoneState::UpperInner => "upper-inner",
            ZoneState::LowerInner => "lower-inner",
            ZoneState::LowerOuter => "lower-outer",
            ZoneState::Between => "between",
        }
    }
}

/// Zone-Based-mode read for the latest bar.
#[derive(Debug, Clone, Serialize)]
pub struct ZoneBandsRead {
    /// Timeframe-adapted MA lengths (Pine `adaptedMA1`/`adaptedMA2`).
    pub adapted_ma1: usize,
    pub adapted_ma2: usize,
    /// Timeframe adaptation multiplier applied (0.7 daily / 0.4 weekly /
    /// 0.2 monthly).
    pub tf_multiplier: f64,
    pub inner_upper: f64,
    pub inner_lower: f64,
    pub outer_upper: f64,
    pub outer_lower: f64,
    /// Inner-pair EMA bias (`ema1 > ema2`).
    pub ema_bias_bullish: bool,
    pub zone: ZoneState,
}

/// Pine `band_th`.
fn band_th(e1: f64, e2: f64, t1: f64, t2: f64) -> f64 {
    let dif = (e1 - e2).abs();
    if e1 > e2 {
        e1 + t1 * dif
    } else {
        e1 - t2 * dif
    }
}

/// Pine `multiScaleEMA` — returns (ema1, ema2, ema1>ema2, band_th upper) for
/// the latest bar. (`l1`/`m1` from `band_lum` are computed by the Pine but
/// only the upper threshold feeds the zone logic and plots we port.)
fn multi_scale_ema(closes: &[f64], r: f64, b: f64, ma1: f64, ma2: f64) -> (f64, f64, bool, f64) {
    let len1 = (ma1.round() as usize).max(1);
    let len2 = (ma2.round() as usize).max(1);
    let e1 = *primitives::ema(closes, len1).last().unwrap_or(&0.0);
    let e2 = *primitives::ema(closes, len2).last().unwrap_or(&0.0);
    let u = band_th(e1, e2, r, b);
    (e1, e2, e1 > e2, u)
}

/// Compute the Zone Based bands + zone state for the latest bar.
///
/// Defaults: MA 144/233, inner/outer scale 2.0, R/B extension 3, timeframe
/// adaptation enabled (daily ⇒ ×0.7, weekly ⇒ ×0.4, monthly+ ⇒ ×0.2 per
/// the Pine table).
pub fn compute_zone_bands(closes: &[f64], timeframe: CyberTimeframe) -> Option<ZoneBandsRead> {
    const ZONE_MA1: f64 = 144.0;
    const ZONE_MA2: f64 = 233.0;
    const SCALE_INNER: f64 = 2.0;
    const SCALE_OUTER: f64 = 2.0;
    const R_EXT: f64 = 3.0;
    const B_EXT: f64 = 3.0;

    let tf_multiplier = match timeframe {
        CyberTimeframe::Daily => 0.7,
        CyberTimeframe::Weekly => 0.4,
        CyberTimeframe::Monthly => 0.2,
    };
    let adapted_ma1 = ((ZONE_MA1 * tf_multiplier).round() as usize).max(5);
    let adapted_ma2 = ((ZONE_MA2 * tf_multiplier).round() as usize).max(8);

    // Need enough bars for the longest EMA (inner pair, ×2 scale) to settle.
    let longest = ((adapted_ma2 as f64) * SCALE_INNER).round() as usize;
    if closes.len() < longest {
        return None;
    }

    let (_e1i, iz_i, is_1_gt_2, iz_o) = multi_scale_ema(
        closes,
        R_EXT,
        B_EXT,
        adapted_ma1 as f64 * SCALE_INNER,
        adapted_ma2 as f64 * SCALE_INNER,
    );
    let (_e1o, _e2o, _outer_bias, o2) = multi_scale_ema(
        closes,
        R_EXT,
        B_EXT,
        adapted_ma1 as f64 / SCALE_OUTER,
        adapted_ma2 as f64 / SCALE_OUTER,
    );

    let is_uuz = is_1_gt_2 && o2 > iz_o;
    let is_ulz = is_1_gt_2 && o2 < iz_i;
    let is_llz = !is_1_gt_2 && o2 < iz_o;
    let is_luz = !is_1_gt_2 && o2 > iz_i;

    let uiz_o = o2 > iz_o && o2 > iz_i;
    let liz_o = o2 < iz_o && o2 < iz_i;
    let oz_i = if is_uuz {
        iz_o
    } else if is_ulz {
        iz_i
    } else if is_llz {
        iz_o
    } else if is_luz {
        iz_i
    } else {
        iz_o
    };
    let oz_o = if uiz_o || liz_o { o2 } else { iz_o };

    let zone = if is_uuz {
        ZoneState::UpperOuter
    } else if is_ulz {
        ZoneState::UpperInner
    } else if is_luz {
        ZoneState::LowerInner
    } else if is_llz {
        ZoneState::LowerOuter
    } else {
        ZoneState::Between
    };

    Some(ZoneBandsRead {
        adapted_ma1,
        adapted_ma2,
        tf_multiplier,
        inner_upper: round_level(iz_o),
        inner_lower: round_level(iz_i),
        outer_upper: round_level(oz_o),
        outer_lower: round_level(oz_i),
        ema_bias_bullish: is_1_gt_2,
        zone,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("2025-{:02}-{:02}", 1 + i / 28, 1 + i % 28))
            .collect()
    }

    #[test]
    fn smma_quirk_equals_src_then_smooths() {
        // src = 1..=20, len 12: SMMA[t<11] = src; SMMA[11] = 11 + (12−11)/12.
        let src: Vec<Option<f64>> = (1..=20).map(|i| Some(i as f64)).collect();
        let s = smma_quirk(&src, 12);
        assert!((s[10].unwrap_or_default() - 11.0).abs() < 1e-12);
        let expect_11 = 11.0 + (12.0 - 11.0) / 12.0;
        assert!((s[11].unwrap_or_default() - expect_11).abs() < 1e-12);
        let expect_12 = expect_11 + (13.0 - expect_11) / 12.0;
        assert!((s[12].unwrap_or_default() - expect_12).abs() < 1e-12);
    }

    #[test]
    fn gaussian_filter_is_backwards_indexed_and_centered_at_half() {
        // len 4 σ 2 → weights w(i) = exp(−0.5·((i−1.5)/2)²) on src[t−i].
        let src = vec![1.0, 2.0, 3.0, 4.0];
        let g = gaussian_filter(&src, 4, 2.0);
        assert_eq!(g[2], None);
        let w: Vec<f64> = (0..4)
            .map(|i| (-0.5f64 * ((i as f64 - 1.5) / 2.0).powi(2)).exp())
            .collect();
        let expect = (4.0 * w[0] + 3.0 * w[1] + 2.0 * w[2] + 1.0 * w[3]) / w.iter().sum::<f64>();
        assert!((g[3].unwrap_or_default() - expect).abs() < 1e-12);
    }

    /// Gentle linear ramp: the close's lag-offset above the SMMA (~8 bars ×
    /// slope) stays well inside the 2.5σ/1.8σ band envelope, so neither band
    /// is ever breached and QB holds its initial 0. (A perfectly flat series
    /// is degenerate — σ = 0 makes the bands zero-width and f64 noise decides
    /// the breach — the same way Pine would behave on constant input.)
    #[test]
    fn gentle_ramp_never_breaches_qb_stays_caution() {
        let closes: Vec<f64> = (0..120).map(|i| 100.0 + 0.001 * i as f64).collect();
        let read = compute_gaussian_bands(&closes, &dates(120), 10).expect("computes");
        assert_eq!(read.qb, QbState::Caution);
        assert_eq!(read.qb_since, None);
        assert!(read.transitions.is_empty());
        assert!(read.upper > read.smma && read.lower < read.smma);
    }

    #[test]
    fn qb_square_wave_transitions_bull_then_bear() {
        // Gentle ramp (caution) → jump to 110 closes above the upper band
        // ⇒ QB 1; a later drop to 80 closes below the lower band ⇒ QB −1.
        let mut closes: Vec<f64> = (0..60).map(|i| 100.0 + 0.001 * i as f64).collect();
        closes.extend(vec![110.0; 5]);
        closes.extend(vec![80.0; 5]);
        let d = dates(closes.len());
        let read = compute_gaussian_bands(&closes, &d, 10).expect("computes");
        assert_eq!(read.qb, QbState::Bearish);
        // First transition must be Caution→Bullish at the 110 jump, then
        // Bullish→Bearish at the 80 drop.
        assert!(read.transitions.len() >= 2, "{:?}", read.transitions);
        assert_eq!(read.transitions[0].from, QbState::Caution);
        assert_eq!(read.transitions[0].to, QbState::Bullish);
        assert_eq!(read.transitions[0].date, d[60]);
        let flip = read
            .transitions
            .iter()
            .find(|t| t.to == QbState::Bearish)
            .expect("bear flip");
        assert_eq!(flip.date, d[65]);
        assert_eq!(read.qb_since, Some(d[65].clone()));
    }

    #[test]
    fn zone_bands_adapted_lengths_daily_weekly_and_monthly() {
        let closes: Vec<f64> = (0..700).map(|i| 100.0 + (i as f64) * 0.1).collect();
        let daily = compute_zone_bands(&closes, CyberTimeframe::Daily).expect("daily");
        assert_eq!(daily.adapted_ma1, 101); // round(144·0.7)
        assert_eq!(daily.adapted_ma2, 163); // round(233·0.7)
        assert!((daily.tf_multiplier - 0.7).abs() < 1e-12);
        let weekly = compute_zone_bands(&closes, CyberTimeframe::Weekly).expect("weekly");
        assert_eq!(weekly.adapted_ma1, 58); // round(144·0.4)
        assert_eq!(weekly.adapted_ma2, 93); // round(233·0.4)
        let monthly = compute_zone_bands(&closes, CyberTimeframe::Monthly).expect("monthly");
        assert_eq!(monthly.adapted_ma1, 29); // round(144·0.2)
        assert_eq!(monthly.adapted_ma2, 47); // round(233·0.2)
                                             // Steady uptrend: fast EMA above slow on both pairs ⇒ bullish bias,
                                             // outer band above the inner upper threshold? At minimum the zone is
                                             // a legal upper-side / between value and bias is bullish.
        assert!(daily.ema_bias_bullish);
        assert!(matches!(
            daily.zone,
            ZoneState::UpperOuter | ZoneState::UpperInner | ZoneState::Between
        ));
    }

    #[test]
    fn zone_bands_bearish_on_downtrend() {
        let closes: Vec<f64> = (0..700).map(|i| 500.0 - (i as f64) * 0.3).collect();
        let z = compute_zone_bands(&closes, CyberTimeframe::Daily).expect("zone");
        assert!(!z.ema_bias_bullish);
        assert!(matches!(
            z.zone,
            ZoneState::LowerOuter | ZoneState::LowerInner | ZoneState::Between
        ));
    }
}
