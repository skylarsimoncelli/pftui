use anyhow::Result;
use chrono::{Datelike, DateTime, NaiveDate, Timelike, Utc};
use serde::Serialize;

/// US market session phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketPhase {
    /// Weekend — Saturday or Sunday ET.
    Weekend,
    /// Pre-market: 4:00 AM – 9:30 AM ET (Mon–Fri).
    PreMarket,
    /// Regular trading hours: 9:30 AM – 4:00 PM ET (Mon–Fri).
    Regular,
    /// After-hours: 4:00 PM – 8:00 PM ET (Mon–Fri).
    AfterHours,
    /// Overnight: 8:00 PM – 4:00 AM ET (Mon–Fri).
    Overnight,
}

impl std::fmt::Display for MarketPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketPhase::Weekend => write!(f, "weekend"),
            MarketPhase::PreMarket => write!(f, "pre-market"),
            MarketPhase::Regular => write!(f, "regular"),
            MarketPhase::AfterHours => write!(f, "after-hours"),
            MarketPhase::Overnight => write!(f, "overnight"),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MarketHoursInfo {
    /// Current UTC timestamp (ISO 8601).
    pub utc_now: String,
    /// Current Eastern Time timestamp (ISO 8601, no TZ suffix).
    pub eastern_now: String,
    /// Whether US equities are in regular trading hours.
    pub is_open: bool,
    /// Current session phase.
    pub phase: MarketPhase,
    /// Human description of the current state.
    pub description: String,
    /// Next market open (UTC, ISO 8601). Null if currently open.
    pub next_open_utc: Option<String>,
    /// Next market close (UTC, ISO 8601). Null if currently closed.
    pub next_close_utc: Option<String>,
    /// Time until next open, human-readable. Null if currently open.
    pub until_open: Option<String>,
    /// Time until close, human-readable. Null if currently closed.
    pub until_close: Option<String>,
    /// Agent guidance: what data sources are most useful right now.
    pub agent_hint: String,
}

/// Determine Eastern Time offset for a given UTC datetime.
fn et_offset_hours(utc: &DateTime<Utc>) -> i64 {
    if is_us_eastern_dst(utc, utc.year()) {
        -4
    } else {
        -5
    }
}

/// Convert UTC datetime to Eastern Time components (as a UTC-typed DateTime
/// shifted by the ET offset — only use the h/m/weekday, not the TZ).
fn to_eastern(utc: &DateTime<Utc>) -> DateTime<Utc> {
    let offset = et_offset_hours(utc);
    let ts = utc.timestamp() + offset * 3600;
    DateTime::from_timestamp(ts, 0).unwrap_or(*utc)
}

/// Approximate US Eastern DST check (mirrors header.rs logic).
fn is_us_eastern_dst(utc: &DateTime<Utc>, year: i32) -> bool {
    let march_start = match NaiveDate::from_ymd_opt(year, 3, 1) {
        Some(d) => d,
        None => return false,
    };
    let march_first_wd = march_start.weekday().num_days_from_sunday();
    let first_sunday_day = if march_first_wd == 0 {
        1
    } else {
        1 + (7 - march_first_wd)
    };
    let second_sunday_march = first_sunday_day + 7;
    let dst_start = NaiveDate::from_ymd_opt(year, 3, second_sunday_march)
        .and_then(|d| d.and_hms_opt(7, 0, 0))
        .map(|dt| dt.and_utc());
    let dst_start = match dst_start {
        Some(t) => t,
        None => return false,
    };

    let nov_start = match NaiveDate::from_ymd_opt(year, 11, 1) {
        Some(d) => d,
        None => return false,
    };
    let nov_first_wd = nov_start.weekday().num_days_from_sunday();
    let first_sunday_nov = if nov_first_wd == 0 {
        1
    } else {
        1 + (7 - nov_first_wd)
    };
    let dst_end = NaiveDate::from_ymd_opt(year, 11, first_sunday_nov)
        .and_then(|d| d.and_hms_opt(6, 0, 0))
        .map(|dt| dt.and_utc());
    let dst_end = match dst_end {
        Some(t) => t,
        None => return false,
    };

    *utc >= dst_start && *utc < dst_end
}

/// Compute the market phase for a given UTC time.
pub fn compute_phase(utc: &DateTime<Utc>) -> MarketPhase {
    let et = to_eastern(utc);
    let weekday = et.weekday();

    if weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun {
        return MarketPhase::Weekend;
    }

    let minutes = et.hour() * 60 + et.minute();
    let pre_market_open = 4 * 60; // 4:00 AM
    let regular_open = 9 * 60 + 30; // 9:30 AM
    let regular_close = 16 * 60; // 4:00 PM
    let after_hours_close = 20 * 60; // 8:00 PM

    if minutes >= regular_open && minutes < regular_close {
        MarketPhase::Regular
    } else if minutes >= pre_market_open && minutes < regular_open {
        MarketPhase::PreMarket
    } else if minutes >= regular_close && minutes < after_hours_close {
        MarketPhase::AfterHours
    } else {
        MarketPhase::Overnight
    }
}

/// Find the next market open in UTC. Searches up to 7 days ahead.
fn next_market_open_utc(utc: &DateTime<Utc>) -> Option<DateTime<Utc>> {
    let offset = et_offset_hours(utc);
    // Regular open is 9:30 AM ET.
    // In UTC: 9:30 - offset (offset is negative, so subtract negative = add).
    // e.g. EST=-5 → 14:30 UTC. EDT=-4 → 13:30 UTC.
    let open_hour_utc = (9 - offset) as u32;
    let open_min_utc = 30u32;

    let et = to_eastern(utc);

    for day_offset in 0..8 {
        let candidate_et = et + chrono::Duration::days(day_offset);
        let wd = candidate_et.weekday();
        if wd == chrono::Weekday::Sat || wd == chrono::Weekday::Sun {
            continue;
        }

        // Build candidate open time in UTC for this ET date.
        let candidate_date = candidate_et.date_naive();
        let candidate_open = candidate_date
            .and_hms_opt(open_hour_utc, open_min_utc, 0)
            .map(|dt| dt.and_utc());

        if let Some(open_utc) = candidate_open {
            if open_utc > *utc {
                return Some(open_utc);
            }
        }
    }
    None
}

/// Find the next market close in UTC.
fn next_market_close_utc(utc: &DateTime<Utc>) -> Option<DateTime<Utc>> {
    let offset = et_offset_hours(utc);
    let close_hour_utc = (16 - offset) as u32; // 4:00 PM ET in UTC
    let close_min_utc = 0u32;

    let et = to_eastern(utc);

    for day_offset in 0..8 {
        let candidate_et = et + chrono::Duration::days(day_offset);
        let wd = candidate_et.weekday();
        if wd == chrono::Weekday::Sat || wd == chrono::Weekday::Sun {
            continue;
        }

        let candidate_date = candidate_et.date_naive();
        let candidate_close = candidate_date
            .and_hms_opt(close_hour_utc, close_min_utc, 0)
            .map(|dt| dt.and_utc());

        if let Some(close_utc) = candidate_close {
            if close_utc > *utc {
                return Some(close_utc);
            }
        }
    }
    None
}

/// Format a duration as human-readable (e.g. "2h 15m", "3d 4h").
fn format_duration(secs: i64) -> String {
    if secs < 0 {
        return "now".to_string();
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

/// Build the full MarketHoursInfo for a given UTC time.
pub fn compute_market_hours(utc: DateTime<Utc>) -> MarketHoursInfo {
    let phase = compute_phase(&utc);
    let is_open = phase == MarketPhase::Regular;
    let et = to_eastern(&utc);

    let description = match phase {
        MarketPhase::Weekend => "US equity markets are closed for the weekend.".to_string(),
        MarketPhase::PreMarket => "Pre-market session active. Limited liquidity, wider spreads.".to_string(),
        MarketPhase::Regular => "US equity markets are open (regular trading hours).".to_string(),
        MarketPhase::AfterHours => "After-hours session active. Limited liquidity, wider spreads.".to_string(),
        MarketPhase::Overnight => "Markets closed. Overnight session (futures/crypto active).".to_string(),
    };

    let agent_hint = match phase {
        MarketPhase::Weekend => "Skip intraday equity data. Focus on: crypto (24/7), positioning review, macro prep, scenario analysis, economic calendar for next week, COT data review.".to_string(),
        MarketPhase::PreMarket => "Pre-market movers available. Focus on: overnight news, futures, European markets, economic releases, positioning for open.".to_string(),
        MarketPhase::Regular => "Full market data available. All data sources active. Real-time prices reliable.".to_string(),
        MarketPhase::AfterHours => "After-hours movers may be significant (earnings). Focus on: earnings reactions, news developments, positioning review, next-day prep.".to_string(),
        MarketPhase::Overnight => "Equity prices stale until pre-market. Focus on: crypto, futures, Asian/European markets, overnight news, macro analysis.".to_string(),
    };

    let next_open = if is_open {
        None
    } else {
        next_market_open_utc(&utc)
    };

    let next_close = if is_open {
        next_market_close_utc(&utc)
    } else {
        None
    };

    let until_open = next_open.map(|o| {
        let secs = (o - utc).num_seconds();
        format_duration(secs)
    });

    let until_close = next_close.map(|c| {
        let secs = (c - utc).num_seconds();
        format_duration(secs)
    });

    let et_str = et.format("%Y-%m-%d %H:%M:%S").to_string();
    let utc_str = utc.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    MarketHoursInfo {
        utc_now: utc_str,
        eastern_now: et_str,
        is_open,
        phase,
        description,
        next_open_utc: next_open.map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        next_close_utc: next_close.map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        until_open,
        until_close,
        agent_hint,
    }
}

/// CLI entry point: `pftui system market-hours [--json]`
pub fn run(json: bool) -> Result<()> {
    let info = compute_market_hours(Utc::now());

    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("US MARKET HOURS");
        println!("───────────────────────────────────────");
        println!("  UTC:      {}", info.utc_now);
        println!("  Eastern:  {}", info.eastern_now);
        println!("  Phase:    {}", info.phase);
        println!("  Status:   {}", if info.is_open { "🟢 OPEN" } else { "🔴 CLOSED" });
        println!();
        println!("  {}", info.description);
        println!();

        if let Some(ref until) = info.until_open {
            println!("  Next open:  {} (in {})", info.next_open_utc.as_deref().unwrap_or("—"), until);
        }
        if let Some(ref until) = info.until_close {
            println!("  Closes at:  {} (in {})", info.next_close_utc.as_deref().unwrap_or("—"), until);
        }

        println!();
        println!("  Agent hint: {}", info.agent_hint);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_regular_hours_wednesday() {
        // Wednesday 2026-03-25 at 15:00 UTC = 11:00 AM EDT (regular hours)
        let utc = Utc.with_ymd_and_hms(2026, 3, 25, 15, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::Regular);
        assert!(info.is_open);
        assert!(info.next_close_utc.is_some());
        assert!(info.next_open_utc.is_none());
    }

    #[test]
    fn test_pre_market_wednesday() {
        // Wednesday 2026-03-25 at 10:00 UTC = 6:00 AM EDT (pre-market)
        let utc = Utc.with_ymd_and_hms(2026, 3, 25, 10, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::PreMarket);
        assert!(!info.is_open);
        assert!(info.next_open_utc.is_some());
    }

    #[test]
    fn test_after_hours_wednesday() {
        // Wednesday 2026-03-25 at 21:00 UTC = 5:00 PM EDT (after-hours)
        let utc = Utc.with_ymd_and_hms(2026, 3, 25, 21, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::AfterHours);
        assert!(!info.is_open);
    }

    #[test]
    fn test_overnight_wednesday() {
        // Thursday 2026-03-26 at 01:00 UTC = 9:00 PM EDT Wed (overnight)
        let utc = Utc.with_ymd_and_hms(2026, 3, 26, 1, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::Overnight);
        assert!(!info.is_open);
    }

    #[test]
    fn test_weekend_saturday() {
        // Saturday 2026-03-28 at 15:00 UTC
        let utc = Utc.with_ymd_and_hms(2026, 3, 28, 15, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::Weekend);
        assert!(!info.is_open);
        assert!(info.next_open_utc.is_some());
        // Next open should be Monday
        let next = info.next_open_utc.unwrap();
        assert!(next.contains("2026-03-30")); // Monday
    }

    #[test]
    fn test_weekend_sunday() {
        // Sunday 2026-03-29 at 12:00 UTC
        let utc = Utc.with_ymd_and_hms(2026, 3, 29, 12, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::Weekend);
        assert!(!info.is_open);
    }

    #[test]
    fn test_exactly_at_open() {
        // Wednesday 2026-03-25 at 13:30 UTC = 9:30 AM EDT (exactly open)
        let utc = Utc.with_ymd_and_hms(2026, 3, 25, 13, 30, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::Regular);
        assert!(info.is_open);
    }

    #[test]
    fn test_exactly_at_close() {
        // Wednesday 2026-03-25 at 20:00 UTC = 4:00 PM EDT (exactly close → after-hours)
        let utc = Utc.with_ymd_and_hms(2026, 3, 25, 20, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::AfterHours);
        assert!(!info.is_open);
    }

    #[test]
    fn test_est_winter_regular_hours() {
        // January 15, 2026 at 16:00 UTC = 11:00 AM EST (regular hours, no DST)
        let utc = Utc.with_ymd_and_hms(2026, 1, 15, 16, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        assert_eq!(info.phase, MarketPhase::Regular);
        assert!(info.is_open);
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(300), "5m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(7500), "2h 5m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(90060), "1d 1h 1m");
    }

    #[test]
    fn test_json_output_structure() {
        let utc = Utc.with_ymd_and_hms(2026, 3, 25, 15, 0, 0).unwrap();
        let info = compute_market_hours(utc);
        let json_str = serde_json::to_string(&info).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.get("phase").is_some());
        assert!(parsed.get("is_open").is_some());
        assert!(parsed.get("agent_hint").is_some());
        assert!(parsed.get("utc_now").is_some());
        assert!(parsed.get("eastern_now").is_some());
    }
}
