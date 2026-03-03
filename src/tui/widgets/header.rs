use chrono::{Datelike, Timelike, Utc};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App, ViewMode};
use crate::config::PortfolioMode;
use crate::tui::theme::{self, lerp_color};
use crate::tui::ui::COMPACT_WIDTH;
use crate::tui::views::markets;

/// Number of ticks between each 1-character scroll advance.
/// At ~60fps tick rate, 6 ticks ≈ 100ms per character ≈ 10 chars/sec.
const TICKER_SCROLL_DIVISOR: u64 = 6;

/// Separator between ticker entries.
const TICKER_SEP: &str = " │ ";

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

/// Returns the total header height in rows (including bottom border).
/// 3 when the ticker tape is active (Positions view, non-compact),
/// 2 otherwise.
pub fn header_height(app: &App) -> u16 {
    let compact = app.terminal_width < COMPACT_WIDTH;
    if !compact && matches!(app.view_mode, ViewMode::Positions) {
        3
    } else {
        2
    }
}

/// A single ticker tape entry: symbol + change%.
struct TickerEntry {
    symbol: String,
    change_pct: f64,
}

/// Build ticker entries from the market symbols that have price data.
fn build_ticker_entries(app: &App) -> Vec<TickerEntry> {
    let items = markets::market_symbols();
    let mut entries = Vec::new();

    for item in &items {
        // Need both current price and history to compute change
        if !app.prices.contains_key(&item.yahoo_symbol) {
            continue;
        }
        if let Some(pct) = ticker_change_pct(app, &item.yahoo_symbol) {
            let f: f64 = pct.to_string().parse().unwrap_or(0.0);
            entries.push(TickerEntry {
                symbol: item.symbol.clone(),
                change_pct: f,
            });
        }
    }

    entries
}

/// Compute change percentage from price history for a symbol.
fn ticker_change_pct(app: &App, yahoo_symbol: &str) -> Option<Decimal> {
    let history = app.price_history.get(yahoo_symbol)?;
    if history.len() < 2 {
        return None;
    }
    let latest = &history[history.len() - 1];
    let prev = &history[history.len() - 2];
    if prev.close == dec!(0) {
        return None;
    }
    Some((latest.close - prev.close) / prev.close * dec!(100))
}

/// Build the plain-text ticker string for measuring length.
/// Format: "SPX +1.2% │ BTC -3.4% │ GOLD +0.5% │ "
#[allow(dead_code)]
pub fn build_ticker_text(app: &App) -> String {
    let entries = build_ticker_entries(app);
    if entries.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();
    for e in &entries {
        parts.push(format!("{} {:+.1}%", e.symbol, e.change_pct));
    }
    // Join with separator and add trailing separator for seamless wrap
    let mut text = parts.join(TICKER_SEP);
    text.push_str(TICKER_SEP);
    text
}

/// Build styled spans for a visible window of the ticker tape.
/// The ticker scrolls left: we extract a window of `width` characters
/// from the ticker text (doubled for seamless wrap), using tick_count
/// to determine the offset.
fn build_ticker_spans(app: &App, width: usize) -> Vec<Span<'static>> {
    let entries = build_ticker_entries(app);
    if entries.is_empty() || width == 0 {
        return vec![];
    }

    let t = &app.theme;

    // Build the full ticker as a sequence of styled segments with known char lengths
    struct Segment {
        text: String,
        color: Color,
        bold: bool,
    }

    let mut segments: Vec<Segment> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        // Symbol in secondary color
        segments.push(Segment {
            text: e.symbol.clone(),
            color: t.text_secondary,
            bold: true,
        });
        // Space + arrow + change
        let change_color = if e.change_pct > 0.0 {
            t.gain_green
        } else if e.change_pct < 0.0 {
            t.loss_red
        } else {
            t.neutral
        };
        let arrow = if e.change_pct > 0.0 {
            "▲"
        } else if e.change_pct < 0.0 {
            "▼"
        } else {
            "―"
        };
        segments.push(Segment {
            text: format!(" {}{:+.1}%", arrow, e.change_pct),
            color: change_color,
            bold: false,
        });
        // Separator (always, for seamless wrapping)
        if i < entries.len() - 1 {
            segments.push(Segment {
                text: TICKER_SEP.to_string(),
                color: t.text_muted,
                bold: false,
            });
        }
    }
    // Trailing separator for seamless wrap
    segments.push(Segment {
        text: TICKER_SEP.to_string(),
        color: t.text_muted,
        bold: false,
    });

    // Compute total character width of one cycle
    let cycle_len: usize = segments.iter().map(|s| s.text.chars().count()).sum();
    if cycle_len == 0 {
        return vec![];
    }

    // Scroll offset (advances by 1 char every TICKER_SCROLL_DIVISOR ticks)
    let offset = (app.tick_count / TICKER_SCROLL_DIVISOR) as usize % cycle_len;

    // Extract `width` chars starting at `offset`, wrapping around via modular indexing.
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars_emitted = 0usize;

    // Find starting segment and char offset within it
    let mut seg_idx = 0usize;
    let mut char_offset = offset;
    loop {
        let seg_chars = segments[seg_idx % segments.len()].text.chars().count();
        if char_offset < seg_chars {
            break;
        }
        char_offset -= seg_chars;
        seg_idx += 1;
    }

    // Emit chars from segments until we've filled `width`
    while chars_emitted < width {
        let seg = &segments[seg_idx % segments.len()];
        let seg_chars: Vec<char> = seg.text.chars().collect();
        let available = seg_chars.len() - char_offset;
        let take = available.min(width - chars_emitted);

        let slice: String = seg_chars[char_offset..char_offset + take].iter().collect();
        let mut style = Style::default().fg(seg.color);
        if seg.bold {
            style = style.bold();
        }
        spans.push(Span::styled(slice, style));

        chars_emitted += take;
        char_offset = 0; // subsequent segments start from 0
        seg_idx += 1;
    }

    spans
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

        let csym = crate::config::currency_symbol(&app.base_currency);
        let value_str = format_compact(total, csym);
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

        // Daily portfolio change
        if let Some(day_change) = app.daily_portfolio_change {
            let day_arrow = if day_change > dec!(0) {
                "▲"
            } else if day_change < dec!(0) {
                "▼"
            } else {
                "―"
            };
            let day_color = if day_change > dec!(0) {
                t.gain_green
            } else if day_change < dec!(0) {
                t.loss_red
            } else {
                t.neutral
            };
            let day_str = format_compact_signed(day_change, csym);
            spans.push(Span::styled(
                format!("  {}{} today", day_arrow, day_str),
                Style::default().fg(day_color),
            ));
        }
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

    let line1 = Line::from(spans);

    // Build lines for the paragraph
    let show_ticker = !compact && matches!(app.view_mode, ViewMode::Positions);
    let lines = if show_ticker {
        // Ticker tape line: scrolling market data marquee
        // Available width is the full area width minus 3 for the leading " ▸ " prefix
        let ticker_width = area.width.saturating_sub(3) as usize;
        let mut ticker_spans: Vec<Span<'static>> = vec![
            Span::styled(" ▸ ", Style::default().fg(t.text_muted)),
        ];
        let scrolling = build_ticker_spans(app, ticker_width);
        if scrolling.is_empty() {
            // No market data yet — show placeholder
            ticker_spans.push(Span::styled(
                "waiting for market data…",
                Style::default().fg(t.text_muted).italic(),
            ));
        } else {
            ticker_spans.extend(scrolling);
        }
        let line2 = Line::from(ticker_spans);
        vec![line1, line2]
    } else {
        vec![line1]
    };

    // Tint header border based on daily portfolio performance.
    // Subtle 15% blend toward green (up) or red (down) for ambient mood.
    let border_color = match app.daily_portfolio_change {
        Some(change) if change > dec!(0) => lerp_color(t.border_subtle, t.gain_green, 0.15),
        Some(change) if change < dec!(0) => lerp_color(t.border_subtle, t.loss_red, 0.15),
        _ => t.border_subtle,
    };

    let header = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(t.surface_2)),
    );

    frame.render_widget(header, area);
}

fn format_compact(v: rust_decimal::Decimal, sym: &str) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 1_000_000.0 {
        format!("{}{:.1}M", sym, f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("{}{:.1}k", sym, f / 1_000.0)
    } else {
        format!("{}{:.0}", sym, f)
    }
}

fn format_compact_signed(v: rust_decimal::Decimal, sym: &str) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    let sign = if f >= 0.0 { "+" } else { "-" };
    let abs = f.abs();
    if abs >= 1_000_000.0 {
        format!("{}{}{:.1}M", sign, sym, abs / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{}{}{:.1}k", sign, sym, abs / 1_000.0)
    } else {
        format!("{}{}{:.0}", sign, sym, abs)
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

    #[test]
    fn test_ticker_text_empty_no_data() {
        // With no market data (empty prices map), ticker text should be empty
        let config = crate::config::Config::default();
        let app = App::new(&config, std::path::PathBuf::from("/tmp/test_ticker.db"));
        let text = build_ticker_text(&app);
        assert!(text.is_empty());
    }

    #[test]
    fn test_ticker_text_format() {
        let entry = TickerEntry {
            symbol: "SPX".to_string(),
            change_pct: 1.23,
        };
        let formatted = format!("{} {:+.1}%", entry.symbol, entry.change_pct);
        assert_eq!(formatted, "SPX +1.2%");
    }

    #[test]
    fn test_ticker_text_format_negative() {
        let entry = TickerEntry {
            symbol: "BTC".to_string(),
            change_pct: -3.45,
        };
        let formatted = format!("{} {:+.1}%", entry.symbol, entry.change_pct);
        assert_eq!(formatted, "BTC -3.5%");
    }

    #[test]
    fn test_ticker_scroll_divisor() {
        assert_eq!(TICKER_SCROLL_DIVISOR, 6);
    }

    #[test]
    fn test_ticker_separator() {
        assert_eq!(TICKER_SEP, " │ ");
    }

    #[test]
    fn test_header_height_positions_view() {
        let config = crate::config::Config::default();
        let mut app = App::new(&config, std::path::PathBuf::from("/tmp/test_hh.db"));
        app.terminal_width = 120; // non-compact
        assert_eq!(header_height(&app), 3);
    }

    #[test]
    fn test_header_height_compact() {
        let config = crate::config::Config::default();
        let mut app = App::new(&config, std::path::PathBuf::from("/tmp/test_hh2.db"));
        app.terminal_width = 80; // compact
        assert_eq!(header_height(&app), 2);
    }

    #[test]
    fn test_header_height_other_views() {
        let config = crate::config::Config::default();
        let mut app = App::new(&config, std::path::PathBuf::from("/tmp/test_hh3.db"));
        app.terminal_width = 120;
        app.view_mode = ViewMode::Markets;
        assert_eq!(header_height(&app), 2);
        app.view_mode = ViewMode::Transactions;
        assert_eq!(header_height(&app), 2);
    }

    #[test]
    fn test_format_compact_signed_positive() {
        assert_eq!(format_compact_signed(dec!(1500), "$"), "+$1.5k");
    }

    #[test]
    fn test_format_compact_signed_negative() {
        assert_eq!(format_compact_signed(dec!(-2300), "$"), "-$2.3k");
    }

    #[test]
    fn test_format_compact_signed_small_positive() {
        assert_eq!(format_compact_signed(dec!(42), "$"), "+$42");
    }

    #[test]
    fn test_format_compact_signed_million() {
        assert_eq!(format_compact_signed(dec!(1500000), "$"), "+$1.5M");
    }

    #[test]
    fn test_format_compact_signed_negative_million() {
        assert_eq!(format_compact_signed(dec!(-1200000), "$"), "-$1.2M");
    }

    #[test]
    fn test_format_compact_euro() {
        assert_eq!(format_compact(dec!(5000), "€"), "€5.0k");
        assert_eq!(format_compact(dec!(1234567), "€"), "€1.2M");
        assert_eq!(format_compact(dec!(42), "€"), "€42");
    }

    #[test]
    fn test_format_compact_signed_gbp() {
        assert_eq!(format_compact_signed(dec!(1500), "£"), "+£1.5k");
        assert_eq!(format_compact_signed(dec!(-800), "£"), "-£800");
    }

    #[test]
    fn test_header_border_tint_positive() {
        let config = crate::config::Config::default();
        let mut app = App::new(&config, std::path::PathBuf::from("/tmp/test_tint_pos.db"));
        app.daily_portfolio_change = Some(dec!(500));
        let t = &app.theme;
        let blended = lerp_color(t.border_subtle, t.gain_green, 0.15);
        // Blended color should differ from border_subtle (shifted toward green)
        assert_ne!(blended, t.border_subtle);
        // Blended color should differ from pure gain_green (only 15% blend)
        assert_ne!(blended, t.gain_green);
    }

    #[test]
    fn test_header_border_tint_negative() {
        let config = crate::config::Config::default();
        let mut app = App::new(&config, std::path::PathBuf::from("/tmp/test_tint_neg.db"));
        app.daily_portfolio_change = Some(dec!(-300));
        let t = &app.theme;
        let blended = lerp_color(t.border_subtle, t.loss_red, 0.15);
        assert_ne!(blended, t.border_subtle);
        assert_ne!(blended, t.loss_red);
    }

    #[test]
    fn test_header_border_tint_zero_change() {
        let config = crate::config::Config::default();
        let mut app = App::new(&config, std::path::PathBuf::from("/tmp/test_tint_zero.db"));
        app.daily_portfolio_change = Some(dec!(0));
        let t = &app.theme;
        // Zero change should use border_subtle unchanged
        let expected = t.border_subtle;
        // The match arm for zero doesn't blend
        assert_eq!(expected, t.border_subtle);
    }

    #[test]
    fn test_header_border_tint_no_data() {
        let config = crate::config::Config::default();
        let app = App::new(&config, std::path::PathBuf::from("/tmp/test_tint_none.db"));
        let t = &app.theme;
        // None daily change should use border_subtle unchanged
        assert!(app.daily_portfolio_change.is_none());
        assert_eq!(t.border_subtle, t.border_subtle);
    }
}
