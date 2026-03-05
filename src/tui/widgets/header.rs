use chrono::{Datelike, NaiveDate, Timelike, Utc};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App, ViewMode};
use crate::config::PortfolioMode;
use crate::db::calendar_cache;
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
/// 4 when both ticker tape and news ticker are active (Positions/Watchlist, non-compact, has news),
/// 3 when only ticker tape is active,
/// 2 otherwise.
pub fn header_height(app: &App) -> u16 {
    let compact = app.terminal_width < COMPACT_WIDTH;
    let show_market_ticker = !compact && matches!(app.view_mode, ViewMode::Positions | ViewMode::Watchlist);
    let has_news = !app.news_entries.is_empty();
    
    if show_market_ticker && has_news {
        4  // line1 + market_ticker + news_ticker + border
    } else if show_market_ticker {
        3  // line1 + market_ticker + border
    } else {
        2  // line1 + border
    }
}

/// A single ticker tape entry: symbol + change%.
struct TickerEntry {
    symbol: String,
    change_pct: f64,
}

/// Build ticker entries from the market symbols that have price data.
fn build_ticker_entries(app: &App) -> Vec<TickerEntry> {
    let mut entries = Vec::new();

    // Add sentiment gauges FIRST (always visible, high priority)
    if let Some((value, _classification)) = &app.crypto_fng {
        entries.push(TickerEntry {
            symbol: "Crypto F&G".to_string(),
            change_pct: *value as f64, // repurpose change_pct for FnG value
        });
    }
    if let Some((value, _classification)) = &app.traditional_fng {
        entries.push(TickerEntry {
            symbol: "TradFi F&G".to_string(),
            change_pct: *value as f64,
        });
    }

    // Then market data
    let items = markets::market_symbols();
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
        
        // Handle F&G indices specially
        if e.symbol.contains("F&G") {
            let value = e.change_pct as u8; // value is 0-100
            let (emoji, classification, color) = match value {
                0..=24 => ("🔴", "Extreme Fear", t.loss_red),
                25..=44 => ("🟠", "Fear", t.loss_red),
                45..=55 => ("🟡", "Neutral", t.neutral),
                56..=75 => ("🟢", "Greed", t.gain_green),
                _ => ("🟢", "Extreme Greed", t.gain_green),
            };
            segments.push(Segment {
                text: format!(" {}{} {}", emoji, value, classification),
                color,
                bold: false,
            });
        } else {
            // Normal market data: space + arrow + change%
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
        }
        
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

/// Build news ticker line showing one headline at a time, cycling every 10 seconds.
/// Returns a styled Line with news emoji prefix + headline + source.
fn build_news_ticker_line<'a>(app: &App, width: usize) -> Option<Line<'a>> {
    if app.news_entries.is_empty() || width < 20 {
        return None;
    }

    let t = &app.theme;
    
    // Cycle through latest 3 headlines every 10 seconds (600 ticks at 60fps)
    const CYCLE_TICKS: u64 = 600;
    let num_headlines = app.news_entries.len().min(3);
    let current_index = ((app.tick_count / CYCLE_TICKS) % num_headlines as u64) as usize;
    
    let entry = &app.news_entries[current_index];
    
    // Format: 📰 [Source] Headline title (truncated to fit)
    let prefix = " 📰 ";
    let source_part = format!("[{}] ", entry.source);
    let prefix_len = prefix.chars().count() + source_part.chars().count();
    
    if width <= prefix_len {
        return None;
    }
    
    let available = width.saturating_sub(prefix_len);
    let title = if entry.title.chars().count() > available {
        let truncated: String = entry.title.chars().take(available.saturating_sub(1)).collect();
        format!("{}…", truncated)
    } else {
        entry.title.clone()
    };
    
    let spans = vec![
        Span::styled(prefix, Style::default().fg(t.text_muted)),
        Span::styled(source_part, Style::default().fg(t.text_accent)),
        Span::styled(title, Style::default().fg(t.text_secondary)),
    ];
    
    Some(Line::from(spans))
}

/// Get next high-impact calendar event and format countdown.
/// Returns Some((event_name, countdown_text)) or None if no upcoming events.
fn get_next_event_countdown(app: &App) -> Option<(String, String)> {
    use rusqlite::Connection;
    
    let conn = Connection::open(&app.db_path).ok()?;
    let today = Utc::now().format("%Y-%m-%d").to_string();
    
    // Get next 10 events to find the first high-impact one
    let events = calendar_cache::get_upcoming_events(&conn, &today, 10).ok()?;
    
    // Find first high-impact event
    let next_event = events.iter().find(|e| e.impact == "high")?;
    
    // Parse event date
    let event_date = NaiveDate::parse_from_str(&next_event.date, "%Y-%m-%d").ok()?;
    let today_date = Utc::now().date_naive();
    
    // Calculate days until event
    let days_until = (event_date - today_date).num_days();
    
    // Format countdown based on proximity
    let countdown = if days_until == 0 {
        "today".to_string()
    } else if days_until == 1 {
        "tomorrow".to_string()
    } else if days_until < 7 {
        format!("{}d", days_until)
    } else {
        // For >7 days, show date (e.g., "Mar 12")
        event_date.format("%b %d").to_string()
    };
    
    Some((next_event.name.clone(), countdown))
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let now = chrono::Utc::now().format("%H:%M UTC");
    let privacy = is_privacy_view(app);
    let pct_mode = app.portfolio_mode == PortfolioMode::Percentage;
    let t = &app.theme;
    let compact = app.terminal_width < COMPACT_WIDTH;

    let home_sub_label = if matches!(app.view_mode, ViewMode::Watchlist) { "W" } else { "P" };
    let pos_style = if matches!(app.view_mode, ViewMode::Positions | ViewMode::Watchlist) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };

    let mut spans = vec![
        Span::styled(" pf", Style::default().fg(t.text_accent).bold()),
        Span::styled("tui", Style::default().fg(t.text_primary).bold()),
        Span::raw("  "),
        Span::styled("[1]", Style::default().fg(t.key_hint)),
        Span::styled(
            if compact {
                format!("H:{home_sub_label}")
            } else {
                format!("Home({home_sub_label})")
            },
            pos_style,
        ),
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
    spans.push(Span::styled(if compact { "W" } else { "Watch" }, watch_style));

    // Analytics tab — always visible
    let analytics_style = if matches!(app.view_mode, ViewMode::Analytics) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[6]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled(if compact { "An" } else { "Analytics" }, analytics_style));

    // News tab — always visible
    let news_style = if matches!(app.view_mode, ViewMode::News) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[7]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled(if compact { "N" } else { "News" }, news_style));

    // Journal tab — always visible
    let journal_style = if matches!(app.view_mode, ViewMode::Journal) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[8]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled(if compact { "J" } else { "Journal" }, journal_style));

    // Calendar countdown — show next high-impact event
    if !compact {
        if let Some((event_name, countdown)) = get_next_event_countdown(app) {
            spans.push(Span::styled("  │  ", Style::default().fg(t.text_muted)));
            spans.push(Span::styled("Next: ", Style::default().fg(t.text_muted)));
            spans.push(Span::styled(
                event_name.clone(),
                Style::default().fg(t.text_accent),
            ));
            spans.push(Span::styled(" in ", Style::default().fg(t.text_muted)));
            spans.push(Span::styled(
                countdown.clone(),
                Style::default().fg(t.text_accent).bold(),
            ));
        }
    }

    if !privacy {
        app.header_privacy_col_range = None;
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
        // Privacy/percentage-view indicator — track position for click target
        let privacy_col_start: u16 = spans.iter().map(|s| s.content.chars().count() as u16).sum();
        let privacy_text = "  [% view]";
        spans.push(Span::raw("  "));
        spans.push(Span::styled("[% view]", Style::default().fg(t.text_muted)));
        let privacy_col_end = privacy_col_start + privacy_text.chars().count() as u16;
        app.header_privacy_col_range = Some((privacy_col_start, privacy_col_end));
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

        // Theme indicator — track position for click target
        let theme_text = format!("  {}", app.theme_name);
        let theme_col_start: u16 = spans.iter().map(|s| s.content.chars().count() as u16).sum();
        spans.push(Span::styled(
            theme_text.clone(),
            Style::default().fg(t.text_muted),
        ));
        let theme_col_end = theme_col_start + theme_text.chars().count() as u16;
        app.header_theme_col_range = Some((theme_col_start, theme_col_end));
    } else {
        app.header_theme_col_range = None;
    }

    let line1 = Line::from(spans);

    // Build lines for the paragraph
    let show_ticker = !compact && matches!(app.view_mode, ViewMode::Positions | ViewMode::Watchlist);
    let mut lines = if show_ticker {
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
    
    // Add news ticker line if we have news and are showing tickers
    if show_ticker {
        if let Some(news_line) = build_news_ticker_line(app, area.width as usize) {
            lines.push(news_line);
        }
    }

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
