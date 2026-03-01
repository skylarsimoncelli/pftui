use chrono::{Datelike, Timelike, Utc};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App, ViewMode};
use crate::config::PortfolioMode;
use crate::tui::theme;
use crate::tui::ui::COMPACT_WIDTH;

/// Returns true if the US stock market (NYSE/NASDAQ) is currently open.
///
/// Market hours: Monday-Friday, 9:30 AM - 4:00 PM Eastern Time.
/// Uses a fixed UTC offset: ET = UTC-5 (EST) or UTC-4 (EDT).
/// DST approximation: March second Sunday - November first Sunday.
/// Does not account for market holidays (conservative: shows OPEN on holidays).
pub fn is_us_market_open() -> bool {
    is_us_market_open_at(Utc::now())
}

/// Testable version that accepts an arbitrary UTC datetime.
pub fn is_us_market_open_at(utc: chrono::DateTime<Utc>) -> bool {
    // Determine Eastern Time offset (EST = -5, EDT = -4)
    let year = utc.year();
    let is_dst = is_us_eastern_dst(utc, year);
    let et_offset_hours: i64 = if is_dst { -4 } else { -5 };

    // Convert to Eastern Time components
    let et_timestamp = utc.timestamp() + et_offset_hours * 3600;
    let et = chrono::DateTime::from_timestamp(et_timestamp, 0).unwrap_or(utc);

    // Weekday check
    let weekday = et.weekday();
    if weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun {
        return false;
    }

    // Time check: 9:30 AM - 4:00 PM ET
    let hour = et.hour();
    let minute = et.minute();
    let time_minutes = hour * 60 + minute;
    let open = 9 * 60 + 30;  // 9:30 AM = 570
    let close = 16 * 60;     // 4:00 PM = 960

    time_minutes >= open && time_minutes < close
}

/// Approximate US Eastern DST check.
/// DST: second Sunday of March 2:00 AM ET to first Sunday of November 2:00 AM ET.
fn is_us_eastern_dst(utc: chrono::DateTime<Utc>, year: i32) -> bool {
    // Second Sunday of March
    let march_start = match chrono::NaiveDate::from_ymd_opt(year, 3, 1) {
        Some(d) => d,
        None => return false,
    };
    let march_first_wd = march_start.weekday().num_days_from_sunday(); // Sun=0
    let first_sunday_day = if march_first_wd == 0 { 1 } else { 1 + (7 - march_first_wd) };
    let second_sunday_march = first_sunday_day + 7;
    // DST starts at 2:00 AM EST = 7:00 AM UTC
    let dst_start = chrono::NaiveDate::from_ymd_opt(year, 3, second_sunday_march)
        .and_then(|d| d.and_hms_opt(7, 0, 0))
        .map(|dt| dt.and_utc());
    let dst_start = match dst_start {
        Some(t) => t,
        None => return false,
    };

    // First Sunday of November
    let nov_start = match chrono::NaiveDate::from_ymd_opt(year, 11, 1) {
        Some(d) => d,
        None => return false,
    };
    let nov_first_wd = nov_start.weekday().num_days_from_sunday();
    let first_sunday_nov = if nov_first_wd == 0 { 1 } else { 1 + (7 - nov_first_wd) };
    // DST ends at 2:00 AM EDT = 6:00 AM UTC
    let dst_end = chrono::NaiveDate::from_ymd_opt(year, 11, first_sunday_nov)
        .and_then(|d| d.and_hms_opt(6, 0, 0))
        .map(|dt| dt.and_utc());
    let dst_end = match dst_end {
        Some(t) => t,
        None => return false,
    };

    utc >= dst_start && utc < dst_end
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let now = chrono::Utc::now().format("%H:%M UTC");
    let privacy = is_privacy_view(app);
    let pct_mode = app.portfolio_mode == PortfolioMode::Percentage;
    let t = &app.theme;
    let compact = app.terminal_width < COMPACT_WIDTH;

    let pos_style = if matches!(app.view_mode, ViewMode::Positions) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };

    let mut spans = vec![
        Span::styled(" pf", Style::default().fg(t.text_accent).bold()),
        Span::styled("tui", Style::default().fg(t.text_primary).bold()),
        Span::raw("  "),
        Span::styled("[1]", Style::default().fg(t.key_hint)),
        Span::styled("Pos", pos_style),
    ];

    if !pct_mode {
        let tx_style = if matches!(app.view_mode, ViewMode::Transactions) {
            Style::default().fg(t.text_primary).bold().underlined()
        } else {
            Style::default().fg(t.text_muted)
        };
        spans.push(Span::raw(" "));
        spans.push(Span::styled("[2]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Tx", tx_style));
    }

    // Markets tab — always visible
    let mkt_style = if matches!(app.view_mode, ViewMode::Markets) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[3]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled("Mkt", mkt_style));

    // Economy tab — always visible
    let econ_style = if matches!(app.view_mode, ViewMode::Economy) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[4]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled(if compact { "Ec" } else { "Econ" }, econ_style));

    // Watchlist tab — always visible
    let watch_style = if matches!(app.view_mode, ViewMode::Watchlist) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[5]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled(if compact { "Wl" } else { "Watch" }, watch_style));

    if !privacy {
        let total = app.total_value;
        let cost = app.total_cost;
        let gain = total - cost;
        let gain_pct = if cost > dec!(0) {
            (gain / cost) * dec!(100)
        } else {
            dec!(0)
        };
        let gain_color = if gain > dec!(0) {
            t.gain_green
        } else if gain < dec!(0) {
            t.loss_red
        } else {
            t.neutral
        };

        let value_str = format_compact(total);
        let gain_str = format!("{:+.1}%", gain_pct);

        // Flash on value update
        let is_flashing = app.tick_count.saturating_sub(app.last_value_update_tick)
            < theme::FLASH_DURATION
            && app.last_value_update_tick > 0;

        let value_style = if is_flashing {
            Style::default()
                .fg(t.surface_0)
                .bg(t.text_accent)
                .bold()
        } else {
            Style::default().fg(t.text_primary).bold()
        };

        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("{}  ", value_str), value_style));
        spans.push(Span::styled(gain_str, Style::default().fg(gain_color)));
    } else {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("[% view]", Style::default().fg(t.text_muted)));
    }

    // In compact mode, hide the clock, market status, and theme name to save space
    if !compact {
        spans.push(Span::styled(" | ", Style::default().fg(t.text_muted)));
        spans.push(Span::styled(
            format!("{}", now),
            Style::default().fg(t.text_muted),
        ));

        // Market status indicator
        let market_open = is_us_market_open();
        if market_open {
            spans.push(Span::styled("  ◉ ", Style::default().fg(t.gain_green)));
            spans.push(Span::styled("OPEN", Style::default().fg(t.gain_green)));
        } else {
            spans.push(Span::styled("  ◎ ", Style::default().fg(t.text_muted)));
            spans.push(Span::styled("CLOSED", Style::default().fg(t.text_muted)));
        }

        // Theme indicator
        spans.push(Span::styled(
            format!("  {}", app.theme_name),
            Style::default().fg(t.text_muted),
        ));
    }

    let line = Line::from(spans);

    let header = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(t.border_subtle))
            .style(Style::default().bg(t.surface_2)),
    );

    frame.render_widget(header, area);
}

fn format_compact(v: rust_decimal::Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 1_000_000.0 {
        format!("${:.1}M", f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("${:.1}k", f / 1_000.0)
    } else {
        format!("${:.0}", f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_market_open_weekday_during_hours() {
        // Wednesday 2026-03-04 at 15:00 UTC = 10:00 AM EST (market open)
        let dt = Utc.with_ymd_and_hms(2026, 3, 4, 15, 0, 0).unwrap();
        assert!(is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_closed_weekday_before_open() {
        // Wednesday 2026-03-04 at 13:00 UTC = 8:00 AM EST (before 9:30)
        let dt = Utc.with_ymd_and_hms(2026, 3, 4, 13, 0, 0).unwrap();
        assert!(!is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_closed_weekday_after_close() {
        // Wednesday 2026-03-04 at 21:30 UTC = 4:30 PM EST (after 4:00 PM)
        let dt = Utc.with_ymd_and_hms(2026, 3, 4, 21, 30, 0).unwrap();
        assert!(!is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_closed_weekend_saturday() {
        // Saturday 2026-03-07 at 15:00 UTC
        let dt = Utc.with_ymd_and_hms(2026, 3, 7, 15, 0, 0).unwrap();
        assert!(!is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_closed_weekend_sunday() {
        // Sunday 2026-03-08 at 15:00 UTC
        let dt = Utc.with_ymd_and_hms(2026, 3, 8, 15, 0, 0).unwrap();
        assert!(!is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_open_exactly_at_open() {
        // Wednesday 2026-03-04 at 14:30 UTC = 9:30 AM EST (exactly market open)
        let dt = Utc.with_ymd_and_hms(2026, 3, 4, 14, 30, 0).unwrap();
        assert!(is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_closed_exactly_at_close() {
        // Wednesday 2026-03-04 at 21:00 UTC = 4:00 PM EST (exactly market close)
        let dt = Utc.with_ymd_and_hms(2026, 3, 4, 21, 0, 0).unwrap();
        assert!(!is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_open_during_dst() {
        // Wednesday 2026-07-15 at 14:00 UTC = 10:00 AM EDT (DST active)
        let dt = Utc.with_ymd_and_hms(2026, 7, 15, 14, 0, 0).unwrap();
        assert!(is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_closed_dst_before_open() {
        // Wednesday 2026-07-15 at 13:00 UTC = 9:00 AM EDT (before 9:30)
        let dt = Utc.with_ymd_and_hms(2026, 7, 15, 13, 0, 0).unwrap();
        assert!(!is_us_market_open_at(dt));
    }

    #[test]
    fn test_market_open_friday_afternoon() {
        // Friday 2026-03-06 at 19:00 UTC = 2:00 PM EST (open)
        let dt = Utc.with_ymd_and_hms(2026, 3, 6, 19, 0, 0).unwrap();
        assert!(is_us_market_open_at(dt));
    }
}
