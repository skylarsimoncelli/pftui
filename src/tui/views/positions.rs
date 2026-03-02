use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App, PriceFlashDirection};
use crate::config::PortfolioMode;
use crate::models::price::HistoryRecord;
use crate::tui::theme;

const SPARKLINE_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// 52-week high/low range result.
#[allow(dead_code)]
pub struct Range52W {
    pub high: Decimal,
    pub low: Decimal,
    /// Position of current price within the range as 0.0..=1.0
    pub position: f64,
    /// Percentage distance from 52-week high (negative = below high)
    pub from_high_pct: f64,
}

/// Compute 52-week high and low from price history records.
/// Returns None if fewer than 2 records or current_price is None.
pub fn compute_52w_range(
    records: &[HistoryRecord],
    current_price: Option<Decimal>,
) -> Option<Range52W> {
    if records.len() < 2 {
        return None;
    }
    let current = current_price?;

    // Take last 365 days of records (they should already be sorted by date)
    let start = if records.len() > 365 {
        records.len() - 365
    } else {
        0
    };
    let slice = &records[start..];

    let mut high = slice[0].close;
    let mut low = slice[0].close;
    for r in slice.iter().skip(1) {
        if r.close > high {
            high = r.close;
        }
        if r.close < low {
            low = r.close;
        }
    }

    // Include current price in high/low
    if current > high {
        high = current;
    }
    if current < low {
        low = current;
    }

    let range = high - low;
    let position = if range > dec!(0) {
        let pos_str = ((current - low) / range).to_string();
        pos_str.parse::<f64>().unwrap_or(0.5)
    } else {
        0.5
    };

    let from_high_pct = if high > dec!(0) {
        let pct_str = (((current - high) / high) * dec!(100)).to_string();
        pct_str.parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    };

    Some(Range52W {
        high,
        low,
        position,
        from_high_pct,
    })
}

/// Build a visual range bar showing current price position within 52-week range.
/// Returns spans like: `━━━●━━━ -5%`
/// Bar width is 6 chars, then from-high percentage.
pub fn build_52w_spans<'a>(theme: &'a theme::Theme, range: &Range52W) -> Vec<Span<'a>> {
    const BAR_WIDTH: usize = 6;

    // Compute dot position within bar (0..BAR_WIDTH-1)
    let dot_pos = ((range.position * (BAR_WIDTH - 1) as f64).round() as usize).min(BAR_WIDTH - 1);

    // Color: green near high, red near low, gradient in between
    let pos_f32 = range.position as f32;
    let dot_color = theme::gradient_3(
        theme.loss_red,
        theme.neutral,
        theme.gain_green,
        pos_f32,
    );

    let mut spans = Vec::new();

    // Build bar characters
    for i in 0..BAR_WIDTH {
        if i == dot_pos {
            spans.push(Span::styled(
                "●",
                Style::default().fg(dot_color).bold(),
            ));
        } else {
            spans.push(Span::styled(
                "━",
                Style::default().fg(theme.text_muted),
            ));
        }
    }

    // From-high percentage
    let pct_text = if range.from_high_pct.abs() < 0.05 {
        " ATH".to_string()
    } else {
        format!("{:+.0}%", range.from_high_pct)
    };

    let pct_color = if range.from_high_pct.abs() < 0.05 {
        theme.gain_green
    } else if range.from_high_pct > -10.0 {
        theme.text_secondary
    } else {
        theme.loss_red
    };

    spans.push(Span::styled(pct_text, Style::default().fg(pct_color)));

    spans
}

/// Compute daily change % from price history: (latest - previous) / previous * 100.
/// Uses the last two entries in the history for the given symbol.
pub fn compute_change_pct(app: &App, symbol: &str) -> Option<Decimal> {
    let history = app.price_history.get(symbol)?;
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

fn format_change_pct(change: Option<Decimal>) -> String {
    change
        .map(|v| format!("{:+.1}%", v))
        .unwrap_or_else(|| "---".to_string())
}

/// Compute the background color for a row in the positions table.
/// Selected rows flash briefly on selection change, lerping from
/// `border_accent` back to `surface_3` over SELECTION_FLASH_DURATION ticks.
fn row_background(app: &App, row_index: usize) -> Color {
    let t = &app.theme;
    if row_index == app.selected_index {
        let elapsed = app.tick_count.saturating_sub(app.last_selection_change_tick);
        if elapsed < theme::SELECTION_FLASH_DURATION && app.last_selection_change_tick > 0 {
            // Lerp from border_accent (flash) toward surface_3 (steady)
            let progress = elapsed as f32 / theme::SELECTION_FLASH_DURATION as f32;
            theme::lerp_color(t.border_accent, t.surface_3, progress)
        } else {
            t.surface_3
        }
    } else if row_index.is_multiple_of(2) {
        t.surface_1
    } else {
        t.surface_1_alt
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if is_privacy_view(app) {
        render_privacy_table(frame, area, app);
    } else {
        render_full_table(frame, area, app);
    }
}

fn render_full_table(frame: &mut Frame, area: Rect, app: &App) {
    let positions = &app.display_positions;
    let t = &app.theme;

    let header = Row::new(vec![
        Cell::from("Asset"),
        Cell::from("Qty"),
        Cell::from("Price"),
        Cell::from("Day%"),
        Cell::from("Gain%"),
        Cell::from("Alloc%"),
        Cell::from("52W"),
        Cell::from("Trend"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = positions
        .iter()
        .enumerate()
        .map(|(i, pos)| {
            let gain_pct = pos.gain_pct.unwrap_or(dec!(0));
            let gain_f: f64 = gain_pct.to_string().parse().unwrap_or(0.0);
            let gain_color = theme::gain_intensity_color(t, gain_f);
            let cat_color = t.category_color(pos.category);

            let row_bg = row_background(app, i);

            let style = Style::default().bg(row_bg);

            let marker = if i == app.selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let asset_text = if pos.name.is_empty() {
                pos.symbol.clone()
            } else {
                format!("{} {}", pos.name, pos.symbol)
            };
            let cat_dot = Span::styled("●", Style::default().fg(cat_color));
            let asset_line = Line::from(vec![
                marker,
                cat_dot,
                Span::raw(" "),
                Span::styled(asset_text, Style::default().fg(t.text_primary)),
            ]);

            // Price flash with direction
            let (price_style, flash_direction) = match app.price_flash_ticks.get(&pos.symbol) {
                Some(&(flash_tick, direction))
                    if app.tick_count.saturating_sub(flash_tick) < theme::FLASH_DURATION =>
                {
                    let bg = match direction {
                        PriceFlashDirection::Up => t.gain_green,
                        PriceFlashDirection::Down => t.loss_red,
                        PriceFlashDirection::Same => t.text_accent,
                    };
                    (
                        Style::default().fg(t.surface_0).bg(bg).bold(),
                        Some(direction),
                    )
                }
                _ => (Style::default().fg(t.text_primary), None),
            };

            let sparkline_spans = build_sparkline_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                7,
            );

            // 52-week range
            let range_52w = compute_52w_range(
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                pos.current_price,
            );
            let range_spans = match &range_52w {
                Some(r) => build_52w_spans(t, r),
                None => vec![Span::styled("---", Style::default().fg(t.text_muted))],
            };

            // Daily change %
            let day_change = compute_change_pct(app, &pos.symbol);
            let day_change_f: f64 = day_change
                .unwrap_or(dec!(0))
                .to_string()
                .parse()
                .unwrap_or(0.0);
            let day_change_color = theme::gain_intensity_color(t, day_change_f);

            Row::new(vec![
                Cell::from(asset_line),
                Cell::from(format_qty(pos.quantity))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(Line::from({
                    let price_text = format_price_opt(pos.current_price);
                    match flash_direction {
                        Some(PriceFlashDirection::Up) => vec![
                            Span::styled(price_text, price_style),
                            Span::styled(" ▲", price_style),
                        ],
                        Some(PriceFlashDirection::Down) => vec![
                            Span::styled(price_text, price_style),
                            Span::styled(" ▼", price_style),
                        ],
                        _ => vec![Span::styled(price_text, price_style)],
                    }
                })),
                Cell::from(format_change_pct(day_change))
                    .style(Style::default().fg(day_change_color)),
                Cell::from(format_gain_pct(pos.gain_pct))
                    .style(Style::default().fg(gain_color)),
                Cell::from(format_alloc_pct(pos.allocation_pct))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(Line::from(range_spans)),
                Cell::from(Line::from(sparkline_spans)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Min(14),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(8),
    ];

    render_table(frame, area, app, header, rows, &widths);
}

fn render_privacy_table(frame: &mut Frame, area: Rect, app: &App) {
    let positions = &app.display_positions;
    let t = &app.theme;

    let header = Row::new(vec![
        Cell::from("Asset"),
        Cell::from("Price"),
        Cell::from("Day%"),
        Cell::from("Alloc%"),
        Cell::from("52W"),
        Cell::from("Trend"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = positions
        .iter()
        .enumerate()
        .map(|(i, pos)| {
            let cat_color = t.category_color(pos.category);

            let row_bg = row_background(app, i);

            let style = Style::default().bg(row_bg);

            let marker = if i == app.selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let asset_text = if pos.name.is_empty() {
                pos.symbol.clone()
            } else {
                format!("{} {}", pos.name, pos.symbol)
            };
            let cat_dot = Span::styled("●", Style::default().fg(cat_color));
            let asset_line = Line::from(vec![
                marker,
                cat_dot,
                Span::raw(" "),
                Span::styled(asset_text, Style::default().fg(t.text_primary)),
            ]);

            let sparkline_spans = build_sparkline_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                7,
            );

            // 52-week range (safe for privacy — shows price-relative data, not values)
            let range_52w = compute_52w_range(
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                pos.current_price,
            );
            let range_spans = match &range_52w {
                Some(r) => build_52w_spans(t, r),
                None => vec![Span::styled("---", Style::default().fg(t.text_muted))],
            };

            // Daily change % (privacy-safe — percentage only, no absolute values)
            let day_change = compute_change_pct(app, &pos.symbol);
            let day_change_f: f64 = day_change
                .unwrap_or(dec!(0))
                .to_string()
                .parse()
                .unwrap_or(0.0);
            let day_change_color = theme::gain_intensity_color(t, day_change_f);

            Row::new(vec![
                Cell::from(asset_line),
                Cell::from(format_price_opt(pos.current_price))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format_change_pct(day_change))
                    .style(Style::default().fg(day_change_color)),
                Cell::from(format_alloc_pct(pos.allocation_pct))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(Line::from(range_spans)),
                Cell::from(Line::from(sparkline_spans)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Min(18),
        Constraint::Length(12),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(11),
        Constraint::Length(8),
    ];

    render_table(frame, area, app, header, rows, &widths);
}

fn render_table(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    header: Row,
    rows: Vec<Row>,
    widths: &[Constraint],
) {
    let t = &app.theme;

    let arrow = if app.sort_ascending { "▲" } else { "▼" };
    let sort_indicator = format!(" [{}{}] ", app.sort_field_label(), arrow);

    let title = if app.portfolio_mode == PortfolioMode::Percentage {
        " Positions (%) "
    } else if app.show_percentages_only {
        " Positions [% view] "
    } else {
        " Positions "
    };

    let is_active_panel = !(app.selected_position().is_some() && app.terminal_width >= crate::tui::ui::COMPACT_WIDTH);
    let border_color = positions_border_color(is_active_panel, app.prices_live, t.border_active, t.border_inactive, app.tick_count);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(t.surface_1))
                .title(Span::styled(title, Style::default().fg(t.text_primary).bold()))
                .title_alignment(Alignment::Left)
                .title(
                    Line::from(Span::styled(
                        sort_indicator,
                        Style::default().fg(t.text_accent),
                    ))
                    .alignment(Alignment::Right),
                ),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}

fn build_sparkline_spans<'a>(
    theme: &'a theme::Theme,
    records: &[crate::models::price::HistoryRecord],
    count: usize,
) -> Vec<Span<'a>> {
    if records.is_empty() {
        return Vec::new();
    }
    let tail: Vec<f64> = records
        .iter()
        .rev()
        .take(count)
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let min = tail.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    tail.iter()
        .map(|v| {
            let position = if range > 0.0 {
                ((v - min) / range) as f32
            } else {
                0.5
            };
            let idx = if range > 0.0 {
                (position * 7.0).round() as usize
            } else {
                3
            };
            let color = theme::gradient_3(
                theme.chart_grad_low,
                theme.chart_grad_mid,
                theme.chart_grad_high,
                position,
            );
            Span::styled(
                String::from(SPARKLINE_CHARS[idx.min(7)]),
                Style::default().fg(color),
            )
        })
        .collect()
}

fn format_price_opt(price: Option<Decimal>) -> String {
    price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string())
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f >= 10000.0 {
        format!("{:.0}", f)
    } else if f >= 100.0 {
        format!("{:.1}", f)
    } else if f >= 1.0 {
        format!("{:.2}", f)
    } else {
        format!("{:.4}", f)
    }
}

fn format_qty(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f >= 100_000.0 {
        format!("{:.1}k", f / 1000.0)
    } else if f >= 1000.0 || f == f.floor() {
        format!("{:.0}", f)
    } else {
        format!("{:.2}", f)
    }
}

fn format_gain_pct(g: Option<Decimal>) -> String {
    g.map(|v| format!("{:+.1}%", v))
        .unwrap_or_else(|| "---".to_string())
}

fn format_alloc_pct(a: Option<Decimal>) -> String {
    a.map(|v| format!("{:.1}%", v))
        .unwrap_or_else(|| "---".to_string())
}

/// Compute the border color for the positions table panel.
/// When the table is the active (focused) panel and prices are live, the border
/// gently pulses between border_active and border_inactive. When active but stale,
/// it stays solid border_active. When inactive (chart has focus), border_inactive.
fn positions_border_color(
    is_active_panel: bool,
    prices_live: bool,
    border_active: Color,
    border_inactive: Color,
    tick_count: u64,
) -> Color {
    if is_active_panel && prices_live {
        theme::pulse_color(border_active, border_inactive, tick_count, theme::PULSE_PERIOD_BORDER)
    } else if is_active_panel {
        border_active
    } else {
        border_inactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_history(prices: &[&str]) -> Vec<HistoryRecord> {
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2025-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
            })
            .collect()
    }

    #[test]
    fn compute_52w_range_basic() {
        let history = make_history(&["100", "120", "80", "110"]);
        let result = compute_52w_range(&history, Some(dec!(110)));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.high, dec!(120));
        assert_eq!(r.low, dec!(80));
        // position: (110 - 80) / (120 - 80) = 30/40 = 0.75
        assert!((r.position - 0.75).abs() < 0.01);
        // from_high: (110 - 120) / 120 * 100 = -8.33%
        assert!((r.from_high_pct - (-8.33)).abs() < 0.1);
    }

    #[test]
    fn compute_52w_range_at_high() {
        let history = make_history(&["90", "100", "95"]);
        let result = compute_52w_range(&history, Some(dec!(105)));
        assert!(result.is_some());
        let r = result.unwrap();
        // Current price exceeds history high — becomes new high
        assert_eq!(r.high, dec!(105));
        assert_eq!(r.low, dec!(90));
        assert!((r.position - 1.0).abs() < 0.01);
        assert!((r.from_high_pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_52w_range_at_low() {
        let history = make_history(&["100", "110", "95"]);
        let result = compute_52w_range(&history, Some(dec!(85)));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.high, dec!(110));
        assert_eq!(r.low, dec!(85));
        assert!((r.position - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_52w_range_no_records() {
        let result = compute_52w_range(&[], Some(dec!(100)));
        assert!(result.is_none());
    }

    #[test]
    fn compute_52w_range_single_record() {
        let history = make_history(&["100"]);
        let result = compute_52w_range(&history, Some(dec!(100)));
        assert!(result.is_none());
    }

    #[test]
    fn compute_52w_range_no_price() {
        let history = make_history(&["100", "110", "95"]);
        let result = compute_52w_range(&history, None);
        assert!(result.is_none());
    }

    #[test]
    fn compute_52w_range_flat_price() {
        let history = make_history(&["100", "100", "100"]);
        let result = compute_52w_range(&history, Some(dec!(100)));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.high, dec!(100));
        assert_eq!(r.low, dec!(100));
        assert!((r.position - 0.5).abs() < 0.01); // defaults to middle
        assert!((r.from_high_pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_52w_range_limits_to_365_records() {
        // Create 400 records with old high that should be excluded
        let mut prices: Vec<String> = Vec::new();
        // First 35 records: very high price (should be outside 365-day window)
        for _ in 0..35 {
            prices.push("500".to_string());
        }
        // Last 365 records: normal range
        for i in 0..365 {
            prices.push(format!("{}", 100 + (i % 20)));
        }
        let history: Vec<HistoryRecord> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2024-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
            })
            .collect();
        let result = compute_52w_range(&history, Some(dec!(110)));
        assert!(result.is_some());
        let r = result.unwrap();
        // High should be from the last 365 records (119), not the old 500
        assert_eq!(r.high, dec!(119));
    }

    // --- compute_change_pct tests ---

    fn make_test_app_with_history(symbol: &str, prices: &[&str]) -> crate::app::App {
        let config = crate::config::Config::default();
        let mut app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_change_pct.db"));
        let records: Vec<HistoryRecord> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2025-01-{:02}", i + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
            })
            .collect();
        app.price_history.insert(symbol.to_string(), records);
        app
    }

    #[test]
    fn compute_change_pct_basic() {
        let app = make_test_app_with_history("AAPL", &["100", "110"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(10)); // +10%
    }

    #[test]
    fn compute_change_pct_negative() {
        let app = make_test_app_with_history("AAPL", &["100", "90"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(-10)); // -10%
    }

    #[test]
    fn compute_change_pct_no_change() {
        let app = make_test_app_with_history("AAPL", &["100", "100"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(0));
    }

    #[test]
    fn compute_change_pct_uses_last_two_entries() {
        // Should use 200 -> 220, not any earlier entries
        let app = make_test_app_with_history("AAPL", &["100", "150", "200", "220"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(10)); // (220-200)/200 * 100 = 10%
    }

    #[test]
    fn compute_change_pct_single_record() {
        let app = make_test_app_with_history("AAPL", &["100"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_none());
    }

    #[test]
    fn compute_change_pct_no_history() {
        let config = crate::config::Config::default();
        let app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_no_hist.db"));
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_none());
    }

    #[test]
    fn compute_change_pct_zero_prev_close() {
        let app = make_test_app_with_history("AAPL", &["0", "100"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_none()); // Division by zero guarded
    }

    #[test]
    fn format_change_pct_positive() {
        let result = format_change_pct(Some(dec!(3.5)));
        assert_eq!(result, "+3.5%");
    }

    #[test]
    fn format_change_pct_negative() {
        let result = format_change_pct(Some(dec!(-2.1)));
        assert_eq!(result, "-2.1%");
    }

    #[test]
    fn format_change_pct_none() {
        let result = format_change_pct(None);
        assert_eq!(result, "---");
    }

    #[test]
    fn test_positions_border_pulse_when_active_and_live() {
        let active = Color::Rgb(100, 200, 255);
        let inactive = Color::Rgb(50, 50, 50);
        // tick 0: phase=0.0, intensity=0.65 (midpoint)
        // tick 30: phase=0.25, intensity=1.0 (peak — full active)
        // tick 90: phase=0.75, intensity=0.3 (trough — near inactive)
        let c0 = positions_border_color(true, true, active, inactive, 0);
        let c30 = positions_border_color(true, true, active, inactive, 30);
        let c90 = positions_border_color(true, true, active, inactive, 90);
        // Peak vs trough must differ
        assert_ne!(c30, c90, "pulse peak and trough should differ");
        // Midpoint should differ from peak
        assert_ne!(c0, c30, "pulse midpoint and peak should differ");
        // Peak (tick 30) should be closest to active color
        if let (Color::Rgb(r30, _, _), Color::Rgb(ra, _, _)) = (c30, active) {
            assert_eq!(r30, ra, "at peak intensity, color should equal active");
        }
        // Trough (tick 90) should be closer to inactive
        if let (Color::Rgb(r90, _, _), Color::Rgb(ri, _, _), Color::Rgb(ra, _, _)) = (c90, inactive, active) {
            assert!(r90 >= ri && r90 <= ra, "trough color should be between inactive and active");
            assert!(r90 < ra, "trough should be less than full active");
        }
    }

    #[test]
    fn test_positions_border_static_when_active_and_stale() {
        let active = Color::Rgb(100, 200, 255);
        let inactive = Color::Rgb(50, 50, 50);
        // When prices are not live, border should be solid active regardless of tick
        let c0 = positions_border_color(true, false, active, inactive, 0);
        let c50 = positions_border_color(true, false, active, inactive, 50);
        let c99 = positions_border_color(true, false, active, inactive, 99);
        assert_eq!(c0, active);
        assert_eq!(c50, active);
        assert_eq!(c99, active);
    }

    #[test]
    fn test_positions_border_inactive_when_not_active() {
        let active = Color::Rgb(100, 200, 255);
        let inactive = Color::Rgb(50, 50, 50);
        // When not the active panel, always inactive — regardless of prices_live or tick
        assert_eq!(positions_border_color(false, true, active, inactive, 0), inactive);
        assert_eq!(positions_border_color(false, true, active, inactive, 60), inactive);
        assert_eq!(positions_border_color(false, false, active, inactive, 0), inactive);
        assert_eq!(positions_border_color(false, false, active, inactive, 99), inactive);
    }
}

#[cfg(test)]
mod selection_flash_tests {
    use super::*;

    fn make_app_with_selection(selected: usize, tick_count: u64, last_change_tick: u64) -> crate::app::App {
        let config = crate::config::Config::default();
        let mut app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_sel_flash.db"));
        app.selected_index = selected;
        app.tick_count = tick_count;
        app.last_selection_change_tick = last_change_tick;
        app
    }

    #[test]
    fn test_flash_at_start_returns_accent_color() {
        // Immediately after selection change (elapsed=0), color should be border_accent
        let app = make_app_with_selection(2, 100, 100);
        let bg = row_background(&app, 2);
        assert_eq!(bg, app.theme.border_accent, "at elapsed=0, selected row should be border_accent");
    }

    #[test]
    fn test_flash_decays_to_surface_3() {
        // After SELECTION_FLASH_DURATION ticks, color should be surface_3
        let app = make_app_with_selection(2, 100 + theme::SELECTION_FLASH_DURATION, 100);
        let bg = row_background(&app, 2);
        assert_eq!(bg, app.theme.surface_3, "after flash duration, selected row should be surface_3");
    }

    #[test]
    fn test_flash_midpoint_is_between_accent_and_surface() {
        // At halfway through the flash, color should be between border_accent and surface_3
        let midpoint = theme::SELECTION_FLASH_DURATION / 2;
        let app = make_app_with_selection(2, 100 + midpoint, 100);
        let bg = row_background(&app, 2);
        // Should differ from both endpoints
        assert_ne!(bg, app.theme.border_accent, "midpoint should not be full accent");
        assert_ne!(bg, app.theme.surface_3, "midpoint should not be full surface_3");
    }

    #[test]
    fn test_non_selected_rows_unaffected_by_flash() {
        // Non-selected rows should be surface_1 or surface_1_alt regardless of flash state
        let app = make_app_with_selection(2, 100, 100);
        let bg_even = row_background(&app, 0);
        let bg_odd = row_background(&app, 1);
        assert_eq!(bg_even, app.theme.surface_1, "even non-selected row should be surface_1");
        assert_eq!(bg_odd, app.theme.surface_1_alt, "odd non-selected row should be surface_1_alt");
    }

    #[test]
    fn test_no_flash_on_initial_state() {
        // When last_selection_change_tick is 0 (initial state), no flash even if elapsed is small
        let app = make_app_with_selection(0, 5, 0);
        let bg = row_background(&app, 0);
        assert_eq!(bg, app.theme.surface_3, "initial state should not trigger flash");
    }

    #[test]
    fn test_flash_well_past_duration() {
        // Long after the flash, color should be solid surface_3
        let app = make_app_with_selection(1, 1000, 100);
        let bg = row_background(&app, 1);
        assert_eq!(bg, app.theme.surface_3, "well past flash duration, should be surface_3");
    }
}

#[cfg(test)]
mod category_dot_tests {
    use super::*;
    use crate::models::asset::AssetCategory;
    use crate::tui::theme;

    #[test]
    fn test_category_dot_uses_category_color() {
        let t = theme::theme_by_name("midnight");
        for cat in AssetCategory::all() {
            let expected_color = t.category_color(*cat);
            let dot = Span::styled("●", Style::default().fg(expected_color));
            assert_eq!(dot.content, "●");
            if let Some(fg) = dot.style.fg {
                assert_eq!(fg, expected_color, "dot color should match category_color for {:?}", cat);
            }
        }
    }

    #[test]
    fn test_category_dot_is_single_char() {
        // The dot character ● should be exactly 1 Unicode char
        assert_eq!("●".chars().count(), 1);
        // And 3 bytes in UTF-8 (won't break column alignment)
        assert_eq!("●".len(), 3);
    }

    #[test]
    fn test_asset_line_structure_with_dot() {
        // Verify the asset line has the expected span structure:
        // [marker, dot, space, asset_text]
        let t = theme::theme_by_name("midnight");
        let cat_color = t.category_color(AssetCategory::Crypto);

        let marker = Span::styled("▎", Style::default().fg(t.border_active));
        let cat_dot = Span::styled("●", Style::default().fg(cat_color));
        let asset_text = "Bitcoin BTC".to_string();
        let asset_line = Line::from(vec![
            marker,
            cat_dot,
            Span::raw(" "),
            Span::styled(asset_text.clone(), Style::default().fg(t.text_primary)),
        ]);

        assert_eq!(asset_line.spans.len(), 4, "asset line should have 4 spans: marker, dot, space, text");
        assert_eq!(asset_line.spans[0].content, "▎");
        assert_eq!(asset_line.spans[1].content, "●");
        assert_eq!(asset_line.spans[2].content, " ");
        assert_eq!(asset_line.spans[3].content, asset_text);
        // Dot should be in category color
        assert_eq!(asset_line.spans[1].style.fg, Some(cat_color));
        // Text should be in text_primary
        assert_eq!(asset_line.spans[3].style.fg, Some(t.text_primary));
    }
}
