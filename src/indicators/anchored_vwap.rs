//! Anchored VWAP — the volume-weighted average price from a chosen anchor bar
//! (a cycle low, halving, or ATH) to now.
//!
//! Anchored to the last cycle low, AVWAP is the average cost-basis of everyone
//! who bought since the bottom: while price holds ABOVE it the average post-low
//! buyer is in profit (dips get bought, bull structure intact); a decisive break
//! BELOW means the average buyer since the low is underwater and the
//! accumulation leg is in question. A behavioral support/magnet, not an
//! oscillator.
//!
//! Money → `rust_decimal` (this is a price level). Volume is `Option<u64>`: if
//! ANY bar in the window lacks real volume, true volume-weighting is unsound, so
//! we fall back to a FLAT-weight anchored average price and FLAG it — a degraded
//! line must never be mistaken for a true AVWAP.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::models::price::HistoryRecord;

/// Minimum fraction of bars in the window that must carry real volume before we
/// trust a true volume-weighted AVWAP. Below this (e.g. a ratio-chart series with
/// no volume at all) we fall back to a flat-weight anchored average price.
const VWAP_COVERAGE_MIN: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AvwapQuality {
    /// True volume-weighted AVWAP. Bars without volume (e.g. the close-only
    /// trailing bars the daily feed writes) simply don't contribute to a
    /// volume-weighted average — they are skipped, not degraded.
    VolumeWeighted,
    /// Flat (unit) weights — the series lacks volume for most of the window
    /// (coverage < 50%); this is an anchored AVERAGE PRICE, not a true VWAP.
    FlatWeightDegraded,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnchoredVwap {
    pub anchor_date: String,
    pub anchor_idx: usize,
    pub quality: AvwapQuality,
    /// Fraction of bars in the window that carried real volume (0..1).
    pub volume_coverage: f64,
    /// Bars in the window with no/zero volume (skipped in volume-weighted mode).
    pub null_volume_bars: usize,
    /// AVWAP value at each bar from the anchor (inclusive) to the last bar.
    pub values: Vec<Decimal>,
    /// AVWAP at the latest bar.
    pub current: Decimal,
}

/// Compute the anchored VWAP series from `anchor_idx` (inclusive) to the end of
/// `bars`. Typical price = (high+low+close)/3, falling back to `close` when a
/// bar lacks high/low. Bars without volume are skipped (they contribute nothing
/// to a volume-weighted average) UNLESS volume coverage is poor, in which case
/// the whole window degrades to flat weights and says so.
pub fn anchored_vwap(bars: &[HistoryRecord], anchor_idx: usize) -> Result<AnchoredVwap> {
    if bars.is_empty() {
        bail!("no price history to anchor against");
    }
    if anchor_idx >= bars.len() {
        bail!("anchor index {anchor_idx} is past the end of the {}-bar series", bars.len());
    }
    let window = &bars[anchor_idx..];
    let has_vol = |b: &HistoryRecord| b.volume.map(|v| v > 0).unwrap_or(false);
    let volumed = window.iter().filter(|b| has_vol(b)).count();
    let null_volume_bars = window.len() - volumed;
    let volume_coverage = volumed as f64 / window.len() as f64;
    let quality = if volume_coverage >= VWAP_COVERAGE_MIN {
        AvwapQuality::VolumeWeighted
    } else {
        AvwapQuality::FlatWeightDegraded
    };

    let mut num = Decimal::ZERO; // Σ typical·weight
    let mut den = Decimal::ZERO; // Σ weight
    let mut values = Vec::with_capacity(window.len());
    for b in window {
        let tp = match (b.high, b.low) {
            (Some(h), Some(l)) => (h + l + b.close) / dec!(3),
            _ => b.close,
        };
        let w = match quality {
            // Skip no-volume bars: weight 0 leaves num/den (hence the AVWAP)
            // unchanged, so they carry the prior value rather than corrupting it.
            AvwapQuality::VolumeWeighted if has_vol(b) => Decimal::from(b.volume.unwrap_or(0)),
            AvwapQuality::VolumeWeighted => Decimal::ZERO,
            AvwapQuality::FlatWeightDegraded => Decimal::ONE,
        };
        num += tp * w;
        den += w;
        let v = if den > Decimal::ZERO { num / den } else { tp };
        values.push(v.round_dp(2));
    }
    let current = *values.last().expect("window is non-empty");
    Ok(AnchoredVwap {
        anchor_date: window[0].date.clone(),
        anchor_idx,
        quality,
        volume_coverage,
        null_volume_bars,
        values,
        current,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(date: &str, h: f64, l: f64, c: f64, vol: Option<u64>) -> HistoryRecord {
        HistoryRecord {
            date: date.to_string(),
            close: Decimal::try_from(c).unwrap(),
            volume: vol,
            open: None,
            high: Some(Decimal::try_from(h).unwrap()),
            low: Some(Decimal::try_from(l).unwrap()),
        }
    }

    #[test]
    fn volume_weighted_matches_hand_calc() {
        // 3 bars, TP = (h+l+c)/3, volumes 10/20/30.
        // bar0 TP=10 (10,10,10) v10; bar1 TP=20 v20; bar2 TP=30 v30.
        let bars = vec![
            bar("d0", 10.0, 10.0, 10.0, Some(10)),
            bar("d1", 20.0, 20.0, 20.0, Some(20)),
            bar("d2", 30.0, 30.0, 30.0, Some(30)),
        ];
        let a = anchored_vwap(&bars, 0).unwrap();
        assert_eq!(a.quality, AvwapQuality::VolumeWeighted);
        // running: [10, (10*10+20*20)/30=16.67, (100+400+900)/60=23.33]
        assert_eq!(a.values[0], dec!(10.00));
        assert_eq!(a.values[1], dec!(16.67));
        assert_eq!(a.current, dec!(23.33));
    }

    #[test]
    fn sparse_null_volume_is_skipped_not_degraded() {
        // 1 of 3 bars lacks volume → coverage 67% ≥ 50% → still volume-weighted;
        // the null bar is skipped (contributes nothing), AVWAP carries.
        let bars = vec![
            bar("d0", 10.0, 10.0, 10.0, Some(10)),
            bar("d1", 20.0, 20.0, 20.0, None), // skipped
            bar("d2", 30.0, 30.0, 30.0, Some(30)),
        ];
        let a = anchored_vwap(&bars, 0).unwrap();
        assert_eq!(a.quality, AvwapQuality::VolumeWeighted);
        assert_eq!(a.null_volume_bars, 1);
        // (10*10 + 30*30)/(10+30) = 1000/40 = 25.00 (null bar carries the prior 10 mid-window)
        assert_eq!(a.values[1], dec!(10.00)); // carried (null bar adds nothing)
        assert_eq!(a.current, dec!(25.00));
    }

    #[test]
    fn mostly_missing_volume_degrades_to_flat_weight() {
        // 2 of 3 bars lack volume → coverage 33% < 50% → flat-weight degrade.
        let bars = vec![
            bar("d0", 10.0, 10.0, 10.0, None),
            bar("d1", 20.0, 20.0, 20.0, Some(10)),
            bar("d2", 30.0, 30.0, 30.0, None),
        ];
        let a = anchored_vwap(&bars, 0).unwrap();
        assert_eq!(a.quality, AvwapQuality::FlatWeightDegraded);
        // flat average of TP: [10, 15, 20]
        assert_eq!(a.current, dec!(20.00));
    }

    #[test]
    fn anchor_partway_only_uses_window() {
        let bars = vec![
            bar("d0", 5.0, 5.0, 5.0, Some(100)),
            bar("d1", 10.0, 10.0, 10.0, Some(10)),
            bar("d2", 30.0, 30.0, 30.0, Some(30)),
        ];
        // Anchor at idx 1 → ignore bar0 entirely.
        let a = anchored_vwap(&bars, 1).unwrap();
        assert_eq!(a.anchor_date, "d1");
        // (10*10 + 30*30)/40 = 1000/40 = 25.00
        assert_eq!(a.current, dec!(25.00));
    }

    #[test]
    fn missing_high_low_falls_back_to_close() {
        let mut b = bar("d0", 0.0, 0.0, 42.0, Some(5));
        b.high = None;
        b.low = None;
        let bars = vec![b];
        let a = anchored_vwap(&bars, 0).unwrap();
        assert_eq!(a.current, dec!(42.00));
    }

    #[test]
    fn out_of_range_anchor_errors() {
        let bars = vec![bar("d0", 1.0, 1.0, 1.0, Some(1))];
        assert!(anchored_vwap(&bars, 5).is_err());
        assert!(anchored_vwap(&[], 0).is_err());
    }
}
