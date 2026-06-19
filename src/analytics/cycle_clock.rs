//! Cycle-position clock (T1, gold post-mortem fix — the filed-but-never-built
//! cycle-clock TODO from the `cycle-frameworks` thesis section).
//!
//! Emits WHERE we are in documented market cycles — never a price
//! prediction. The checklist confirms; the calendar does not.
//!
//! **BTC** (computed from the deep `BTC-USD` daily series, never the
//! shallow `BTC` series):
//! - days/weeks since the 2024-04-19 halving
//! - Olson day-900 countdown (every prior cycle bottomed near day ~900
//!   post-halving; n≈3 analog)
//! - Loukas 4-yr cycle week position. Cycle anchor = the 2022-11-21 cycle
//!   low (verified against the `price_history` minimum in a ±9-month
//!   window around the documented date). The thesis frames the low band
//!   as "week ~46 (±10%)" of the cycle's 4th year — i.e. total cycle week
//!   ~202 of a ~208-week (4×52) cycle. We emit the band as cycle weeks
//!   187-229 (208 ± 10%), centered Oct-Nov 2026.
//! - midterm-year H2 flag (US midterm years: year % 4 == 2; H2 = Jul-Dec)
//! - Mayer Multiple (price / 200-day MA)
//! - price vs 200-week MA (weekly bars aggregated from daily history)
//!
//! **GC=F** (gold, ~8-year cycle):
//! - documented cycle-low anchors ~2008-10, ~2015-12, ~2022-09, each
//!   VERIFIED against the actual `price_history` minimum in a ±9-month
//!   window (verified prints: 2008-11-13, 2015-12-17, 2022-09-26 on the
//!   local GC=F series); the output carries what was verified
//! - years since the last verified cycle low, average observed cycle
//!   length, half-cycle (~4y) position
//! - extension vs 200-day MA and vs 40-week MA
//!
//! All prices are `rust_decimal::Decimal`.

use chrono::{Datelike, Duration, NaiveDate};
use rust_decimal::Decimal;
use serde::Serialize;

use crate::analytics::market_structure::{aggregate, Timeframe};
use crate::models::price::HistoryRecord;

// ---------------------------------------------------------------------------
// Documented anchors (see module docs for verification policy)
// ---------------------------------------------------------------------------

pub const BTC_HALVING_2024: &str = "2024-04-19";
pub const OLSON_BOTTOM_DAY: i64 = 900;
pub const BTC_DOCUMENTED_CYCLE_LOW: &str = "2022-11-21";
/// Loukas 4-yr cycle: ~208 weeks low-to-low, band ±10%.
pub const LOUKAS_CYCLE_WEEKS: i64 = 208;
pub const LOUKAS_BAND_LOW_WEEK: i64 = 187; // 208 - 10%
pub const LOUKAS_BAND_HIGH_WEEK: i64 = 229; // 208 + 10%

/// Documented gold cycle-low anchors (approximate months; verified at
/// runtime against price_history minima).
pub const GOLD_DOCUMENTED_CYCLE_LOWS: [&str; 3] =
    ["2008-10-15", "2015-12-15", "2022-09-15"];

/// Verification window around a documented anchor (±9 months).
const VERIFY_WINDOW_DAYS: i64 = 270;
/// A verified minimum within this distance of the documented date counts
/// as confirming the documented anchor.
const VERIFY_MATCH_DAYS: i64 = 75;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct VerifiedAnchor {
    pub documented: String,
    /// Date of the actual minimum close in the verification window
    /// (None when history doesn't cover the window).
    pub verified_date: Option<String>,
    pub verified_close: Option<Decimal>,
    /// True when the verified minimum lands within 75 days of the
    /// documented anchor.
    pub confirms_documented: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoukasBand {
    pub cycle_week: i64,
    pub cycle_length_weeks: i64,
    pub band_low_week: i64,
    pub band_high_week: i64,
    pub in_band: bool,
    pub weeks_to_band_start: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BtcCycleClock {
    pub series: String,
    pub as_of: String,
    pub last_close: Decimal,
    pub halving_date: String,
    pub days_since_halving: i64,
    pub weeks_since_halving: i64,
    pub olson_day900_date: String,
    pub olson_days_remaining: i64,
    pub cycle_low_anchor: VerifiedAnchor,
    pub loukas: Option<LoukasBand>,
    pub midterm_year: bool,
    pub midterm_h2: bool,
    pub mayer_multiple: Option<Decimal>,
    pub ma_200w: Option<Decimal>,
    pub pct_vs_200wma: Option<Decimal>,
    /// The falsifiable 4-year-intact vs 16-18y-major-top adjudication.
    pub major_cycle_test: Option<MajorCycleTest>,
    pub verdict: String,
}

/// The single falsifiable test that separates "the 4-year cycle is intact"
/// from "the larger (Loukas ~16-18y) cycle has topped": after the next cycle
/// low, does the rally CLEAR the prior cycle high (4yr-intact → new bull) or
/// fail beneath it (major-top → lower high). Until the next low forms the test
/// is pending; the level to clear is reported either way.
#[derive(Debug, Clone, Serialize)]
pub struct MajorCycleTest {
    pub prior_cycle_high: Decimal,
    pub prior_cycle_high_date: String,
    /// Current close vs the prior cycle high (negative = below it).
    pub pct_vs_prior_high: Decimal,
    pub above_prior_high: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoldCycleClock {
    pub series: String,
    pub as_of: String,
    pub last_close: Decimal,
    pub anchors: Vec<VerifiedAnchor>,
    pub last_cycle_low_date: Option<String>,
    pub years_since_cycle_low: Option<Decimal>,
    pub avg_cycle_years: Option<Decimal>,
    pub half_cycle_years: Option<Decimal>,
    pub past_half_cycle: Option<bool>,
    pub cycle_position_pct: Option<Decimal>,
    pub extension_pct_vs_200dma: Option<Decimal>,
    pub extension_pct_vs_40wma: Option<Decimal>,
    pub verdict: String,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Verify a documented cycle-low anchor against the actual minimum close
/// in a ±`VERIFY_WINDOW_DAYS` window of price history.
pub fn verify_anchor(history: &[HistoryRecord], documented: &str) -> VerifiedAnchor {
    let mut out = VerifiedAnchor {
        documented: documented.to_string(),
        verified_date: None,
        verified_close: None,
        confirms_documented: None,
    };
    let Some(doc) = parse_date(documented) else {
        return out;
    };
    let lo = doc - Duration::days(VERIFY_WINDOW_DAYS);
    let hi = doc + Duration::days(VERIFY_WINDOW_DAYS);
    let mut min: Option<(NaiveDate, Decimal)> = None;
    let mut covered = false;
    for row in history {
        let Some(d) = parse_date(&row.date) else {
            continue;
        };
        if d < lo || d > hi {
            continue;
        }
        covered = true;
        if min.map(|(_, c)| row.close < c).unwrap_or(true) {
            min = Some((d, row.close));
        }
    }
    if !covered {
        return out;
    }
    if let Some((d, c)) = min {
        out.confirms_documented = Some((d - doc).num_days().abs() <= VERIFY_MATCH_DAYS);
        out.verified_date = Some(d.format("%Y-%m-%d").to_string());
        out.verified_close = Some(c);
    }
    out
}

fn sma_last(closes: &[Decimal], period: usize) -> Option<Decimal> {
    if closes.len() < period || period == 0 {
        return None;
    }
    let sum: Decimal = closes[closes.len() - period..].iter().copied().sum();
    Some(sum / Decimal::from(period))
}

fn pct_vs(price: Decimal, ma: Decimal) -> Option<Decimal> {
    if ma <= Decimal::ZERO {
        return None;
    }
    Some(((price - ma) / ma * Decimal::from(100)).round_dp(1))
}

// ---------------------------------------------------------------------------
// BTC
// ---------------------------------------------------------------------------

/// Build the BTC cycle clock from the deep daily series (BTC-USD).
/// Returns None when history is empty or undated.
pub fn btc_cycle_clock(series: &str, history: &[HistoryRecord]) -> Option<BtcCycleClock> {
    let last = history.last()?;
    let as_of = parse_date(&last.date)?;
    let last_close = last.close;

    let halving = parse_date(BTC_HALVING_2024)?;
    let days_since_halving = (as_of - halving).num_days();
    let olson_date = halving + Duration::days(OLSON_BOTTOM_DAY);
    let olson_days_remaining = (olson_date - as_of).num_days();

    let cycle_low_anchor = verify_anchor(history, BTC_DOCUMENTED_CYCLE_LOW);
    let anchor_date = cycle_low_anchor
        .verified_date
        .as_deref()
        .and_then(parse_date)
        .or_else(|| parse_date(BTC_DOCUMENTED_CYCLE_LOW));

    let loukas = anchor_date.map(|anchor| {
        let cycle_week = (as_of - anchor).num_days() / 7;
        LoukasBand {
            cycle_week,
            cycle_length_weeks: LOUKAS_CYCLE_WEEKS,
            band_low_week: LOUKAS_BAND_LOW_WEEK,
            band_high_week: LOUKAS_BAND_HIGH_WEEK,
            in_band: (LOUKAS_BAND_LOW_WEEK..=LOUKAS_BAND_HIGH_WEEK).contains(&cycle_week),
            weeks_to_band_start: LOUKAS_BAND_LOW_WEEK - cycle_week,
            note: "thesis frames the low band as wk~46 (±10%) of cycle year 4 \
                   ≈ total cycle weeks 187-229 of a ~208-week cycle"
                .to_string(),
        }
    });

    let midterm_year = as_of.year() % 4 == 2;
    let midterm_h2 = midterm_year && as_of.month() >= 7;

    let closes: Vec<Decimal> = history.iter().map(|r| r.close).collect();
    let mayer_multiple = sma_last(&closes, 200).and_then(|ma| {
        if ma > Decimal::ZERO {
            Some((last_close / ma).round_dp(2))
        } else {
            None
        }
    });

    let weekly = aggregate(history, Timeframe::Weekly);
    let weekly_closes: Vec<Decimal> = weekly.iter().map(|b| b.close).collect();
    let ma_200w = sma_last(&weekly_closes, 200).map(|d| d.round_dp(2));
    let pct_vs_200wma = ma_200w.and_then(|ma| pct_vs(last_close, ma));

    // Major-vs-4yr falsifiable test: the prior cycle high is the all-time-high
    // close; the next cycle's rally either clears it (4yr-intact) or fails it
    // (major top). Reported live with the level to clear.
    let major_cycle_test = history
        .iter()
        .filter_map(|r| parse_date(&r.date).map(|d| (d, r.close)))
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(d, high)| {
            let pct = pct_vs(last_close, high).unwrap_or(Decimal::ZERO);
            let above = last_close >= high;
            let note = if above {
                "price is AT/ABOVE the prior cycle high — the 4-year-intact read \
                 is live; a confirmed cycle low followed by a HIGHER high keeps it intact"
                    .to_string()
            } else {
                "test PENDING the next cycle low: the post-low rally clearing this \
                 level confirms 4-year-intact; failing beneath it confirms the \
                 Loukas major-cycle top (a lower high)"
                    .to_string()
            };
            MajorCycleTest {
                prior_cycle_high: high.round_dp(2),
                prior_cycle_high_date: d.format("%Y-%m-%d").to_string(),
                pct_vs_prior_high: pct,
                above_prior_high: above,
                note,
            }
        });

    let mut parts: Vec<String> = vec![format!(
        "day {days_since_halving} post-halving (Olson day-900 = {})",
        olson_date.format("%Y-%m-%d")
    )];
    if let Some(l) = &loukas {
        parts.push(format!(
            "cycle week {} of ~{} (Loukas low band wk {}-{}: {})",
            l.cycle_week,
            l.cycle_length_weeks,
            l.band_low_week,
            l.band_high_week,
            if l.in_band {
                "IN BAND".to_string()
            } else if l.weeks_to_band_start > 0 {
                format!("{} wks to band", l.weeks_to_band_start)
            } else {
                "past band".to_string()
            }
        ));
    }
    parts.push(format!(
        "midterm-year H2: {}",
        if midterm_h2 {
            "yes"
        } else if midterm_year {
            "not yet (H1)"
        } else {
            "no"
        }
    ));
    if let Some(m) = mayer_multiple {
        parts.push(format!("Mayer {m}"));
    }
    if let Some(p) = pct_vs_200wma {
        parts.push(format!("{p:+}% vs 200w MA"));
    }
    if let Some(t) = &major_cycle_test {
        parts.push(format!(
            "major-vs-4yr: {} prior-high CLOSE {} ({:+}%)",
            if t.above_prior_high { "at/above" } else { "below" },
            t.prior_cycle_high,
            t.pct_vs_prior_high
        ));
    }
    let verdict = format!("BTC: {}", parts.join(", "));

    Some(BtcCycleClock {
        series: series.to_string(),
        as_of: last.date.clone(),
        last_close,
        halving_date: BTC_HALVING_2024.to_string(),
        days_since_halving,
        weeks_since_halving: days_since_halving / 7,
        olson_day900_date: olson_date.format("%Y-%m-%d").to_string(),
        olson_days_remaining,
        cycle_low_anchor,
        loukas,
        midterm_year,
        midterm_h2,
        mayer_multiple,
        ma_200w,
        pct_vs_200wma,
        major_cycle_test,
        verdict,
    })
}

// ---------------------------------------------------------------------------
// Gold
// ---------------------------------------------------------------------------

/// Build the gold cycle clock from the deep daily series (GC=F).
pub fn gold_cycle_clock(series: &str, history: &[HistoryRecord]) -> Option<GoldCycleClock> {
    let last = history.last()?;
    let as_of = parse_date(&last.date)?;
    let last_close = last.close;

    let anchors: Vec<VerifiedAnchor> = GOLD_DOCUMENTED_CYCLE_LOWS
        .iter()
        .map(|d| verify_anchor(history, d))
        .collect();

    // Use verified dates where available, documented otherwise.
    let low_dates: Vec<NaiveDate> = anchors
        .iter()
        .filter_map(|a| {
            a.verified_date
                .as_deref()
                .and_then(parse_date)
                .or_else(|| parse_date(&a.documented))
        })
        .collect();

    let avg_cycle_years = if low_dates.len() >= 2 {
        let total_days: i64 = low_dates
            .windows(2)
            .map(|w| (w[1] - w[0]).num_days())
            .sum();
        let spans = (low_dates.len() - 1) as i64;
        Some(
            (Decimal::from(total_days) / Decimal::from(spans) / Decimal::new(36525, 2))
                .round_dp(1),
        )
    } else {
        None
    };
    let half_cycle_years = avg_cycle_years.map(|a| (a / Decimal::from(2)).round_dp(1));

    let last_low = low_dates.last().copied();
    let years_since_cycle_low = last_low.map(|d| {
        (Decimal::from((as_of - d).num_days()) / Decimal::new(36525, 2)).round_dp(1)
    });
    let past_half_cycle = match (years_since_cycle_low, half_cycle_years) {
        (Some(y), Some(h)) => Some(y >= h),
        _ => None,
    };
    let cycle_position_pct = match (years_since_cycle_low, avg_cycle_years) {
        (Some(y), Some(a)) if a > Decimal::ZERO => {
            Some((y / a * Decimal::from(100)).round_dp(0))
        }
        _ => None,
    };

    let closes: Vec<Decimal> = history.iter().map(|r| r.close).collect();
    let extension_pct_vs_200dma =
        sma_last(&closes, 200).and_then(|ma| pct_vs(last_close, ma));
    let weekly = aggregate(history, Timeframe::Weekly);
    let weekly_closes: Vec<Decimal> = weekly.iter().map(|b| b.close).collect();
    let extension_pct_vs_40wma =
        sma_last(&weekly_closes, 40).and_then(|ma| pct_vs(last_close, ma));

    let mut parts: Vec<String> = Vec::new();
    match (years_since_cycle_low, avg_cycle_years, cycle_position_pct) {
        (Some(y), Some(a), Some(p)) => {
            parts.push(format!("year {y} of ~{a}yr cycle ({p}% through)"));
        }
        (Some(y), _, _) => parts.push(format!("year {y} since last cycle low")),
        _ => {}
    }
    if let (Some(h), Some(past)) = (half_cycle_years, past_half_cycle) {
        parts.push(format!(
            "half-cycle ~{h}yr {}",
            if past { "passed" } else { "not yet reached" }
        ));
    }
    if let Some(d) = last_low {
        parts.push(format!("last verified cycle low {}", d.format("%Y-%m-%d")));
    }
    if let Some(e) = extension_pct_vs_200dma {
        parts.push(format!("extension {e:+}% vs 200d MA"));
    }
    if let Some(e) = extension_pct_vs_40wma {
        parts.push(format!("{e:+}% vs 40wk MA"));
    }
    let verdict = format!("GOLD: {}", parts.join(", "));

    Some(GoldCycleClock {
        series: series.to_string(),
        as_of: last.date.clone(),
        last_close,
        anchors,
        last_cycle_low_date: last_low.map(|d| d.format("%Y-%m-%d").to_string()),
        years_since_cycle_low,
        avg_cycle_years,
        half_cycle_years,
        past_half_cycle,
        cycle_position_pct,
        extension_pct_vs_200dma,
        extension_pct_vs_40wma,
        verdict,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Daily synthetic series from `start` to `end` (calendar days) at a
    /// flat close, with explicit (date, close) overrides for lows.
    fn flat_history(
        start: &str,
        end: &str,
        base: Decimal,
        overrides: &[(&str, Decimal)],
    ) -> Vec<HistoryRecord> {
        let start = parse_date(start).unwrap();
        let end = parse_date(end).unwrap();
        let mut rows = Vec::new();
        let mut d = start;
        while d <= end {
            let ds = d.format("%Y-%m-%d").to_string();
            let close = overrides
                .iter()
                .find(|(od, _)| *od == ds)
                .map(|(_, c)| *c)
                .unwrap_or(base);
            rows.push(HistoryRecord {
                date: ds,
                close,
                volume: None,
                open: None,
                high: None,
                low: None,
            });
            d += Duration::days(1);
        }
        rows
    }

    #[test]
    fn verify_anchor_finds_window_minimum() {
        let history = flat_history(
            "2022-01-01",
            "2023-06-01",
            dec!(20000),
            &[("2022-11-21", dec!(15787))],
        );
        let anchor = verify_anchor(&history, BTC_DOCUMENTED_CYCLE_LOW);
        assert_eq!(anchor.verified_date.as_deref(), Some("2022-11-21"));
        assert_eq!(anchor.verified_close, Some(dec!(15787)));
        assert_eq!(anchor.confirms_documented, Some(true));
    }

    #[test]
    fn verify_anchor_flags_distant_minimum() {
        // Minimum 8 months after the documented date → does not confirm.
        let history = flat_history(
            "2022-01-01",
            "2023-09-01",
            dec!(20000),
            &[("2023-07-15", dec!(14000))],
        );
        let anchor = verify_anchor(&history, BTC_DOCUMENTED_CYCLE_LOW);
        assert_eq!(anchor.confirms_documented, Some(false));
    }

    #[test]
    fn verify_anchor_uncovered_window_returns_none_fields() {
        let history = flat_history("2025-01-01", "2025-06-01", dec!(100), &[]);
        let anchor = verify_anchor(&history, "2008-10-15");
        assert!(anchor.verified_date.is_none());
        assert!(anchor.confirms_documented.is_none());
    }

    #[test]
    fn btc_clock_positions_against_halving_and_band() {
        let history = flat_history(
            "2018-01-01",
            "2026-06-09",
            dec!(50000),
            &[("2022-11-21", dec!(15787))],
        );
        let clock = btc_cycle_clock("BTC-USD", &history).unwrap();
        // 2024-04-19 → 2026-06-09 = 781 days
        assert_eq!(clock.days_since_halving, 781);
        assert_eq!(clock.olson_day900_date, "2026-10-06");
        assert_eq!(clock.olson_days_remaining, 119);
        let loukas = clock.loukas.as_ref().unwrap();
        // 2022-11-21 → 2026-06-09 = 1296 days = week 185
        assert_eq!(loukas.cycle_week, 185);
        assert!(!loukas.in_band);
        assert_eq!(loukas.weeks_to_band_start, 2);
        // 2026 is a midterm year; June = H1.
        assert!(clock.midterm_year);
        assert!(!clock.midterm_h2);
        // Flat series: Mayer ≈ price/MA ≈ 1 (within rounding of the dip).
        let mayer = clock.mayer_multiple.unwrap();
        assert!(mayer > dec!(0.9) && mayer < dec!(1.1), "mayer {mayer}");
        assert!(clock.ma_200w.is_some());
        assert!(clock.verdict.starts_with("BTC: day 781 post-halving"));
        assert!(!clock.verdict.to_lowercase().contains("predict"));
    }

    #[test]
    fn btc_clock_in_band_when_week_within_loukas_window() {
        // as_of 2026-07-01: 2022-11-21 → 1318 days = week 188 (in band),
        // July = midterm H2.
        let history = flat_history(
            "2018-01-01",
            "2026-07-01",
            dec!(50000),
            &[("2022-11-21", dec!(15787))],
        );
        let clock = btc_cycle_clock("BTC-USD", &history).unwrap();
        let loukas = clock.loukas.as_ref().unwrap();
        assert_eq!(loukas.cycle_week, 188);
        assert!(loukas.in_band);
        assert!(clock.midterm_h2);
        assert!(clock.verdict.contains("IN BAND"), "{}", clock.verdict);
    }

    #[test]
    fn btc_clock_major_cycle_test_reports_prior_high() {
        // Base 50k with a 120k ATH spike in 2025-10; last close (flat 50k) is
        // below it -> test pending, level to clear = 120k.
        let history = flat_history(
            "2018-01-01",
            "2026-06-18",
            dec!(50000),
            &[("2022-11-21", dec!(15787)), ("2025-10-06", dec!(120000))],
        );
        let clock = btc_cycle_clock("BTC-USD", &history).unwrap();
        let t = clock.major_cycle_test.as_ref().unwrap();
        assert_eq!(t.prior_cycle_high, dec!(120000));
        assert_eq!(t.prior_cycle_high_date, "2025-10-06");
        assert!(!t.above_prior_high);
        assert!(t.pct_vs_prior_high < dec!(0)); // below the high
        assert!(t.note.contains("PENDING"));
    }

    #[test]
    fn gold_clock_verifies_anchors_and_positions_cycle() {
        let history = flat_history(
            "2007-01-01",
            "2026-06-09",
            dec!(2000),
            &[
                ("2008-11-13", dec!(705)),
                ("2015-12-17", dec!(1051)),
                ("2022-09-26", dec!(1623)),
            ],
        );
        let clock = gold_cycle_clock("GC=F", &history).unwrap();
        assert_eq!(clock.anchors.len(), 3);
        for anchor in &clock.anchors {
            assert_eq!(
                anchor.confirms_documented,
                Some(true),
                "anchor {:?} should confirm",
                anchor.documented
            );
        }
        assert_eq!(clock.last_cycle_low_date.as_deref(), Some("2022-09-26"));
        // 2008-11-13 → 2015-12-17 = 7.1y; 2015-12-17 → 2022-09-26 = 6.8y; avg ≈ 6.9y
        let avg = clock.avg_cycle_years.unwrap();
        assert!(avg > dec!(6.5) && avg < dec!(7.3), "avg {avg}");
        // 2022-09-26 → 2026-06-09 ≈ 3.7y → past the ~3.5y half-cycle.
        let years = clock.years_since_cycle_low.unwrap();
        assert!(years > dec!(3.5) && years < dec!(3.9), "years {years}");
        assert_eq!(clock.past_half_cycle, Some(true));
        let pos = clock.cycle_position_pct.unwrap();
        assert!(pos > dec!(45) && pos < dec!(65), "pos {pos}");
        assert!(clock.extension_pct_vs_200dma.is_some());
        assert!(clock.extension_pct_vs_40wma.is_some());
        assert!(clock.verdict.starts_with("GOLD: year"), "{}", clock.verdict);
    }

    #[test]
    fn empty_history_returns_none() {
        assert!(btc_cycle_clock("BTC-USD", &[]).is_none());
        assert!(gold_cycle_clock("GC=F", &[]).is_none());
    }
}
