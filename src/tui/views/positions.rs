use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
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

            let row_bg = if i == app.selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_1_alt
            };

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
            let asset_line =
                Line::from(vec![marker, Span::raw(" "), Span::raw(asset_text)]);

            // Price flash
            let price_style = match app.price_flash_ticks.get(&pos.symbol) {
                Some(&flash_tick)
                    if app.tick_count.saturating_sub(flash_tick) < theme::FLASH_DURATION =>
                {
                    Style::default().fg(t.surface_0).bg(t.text_accent).bold()
                }
                _ => Style::default().fg(t.text_primary),
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
                Cell::from(asset_line).style(Style::default().fg(cat_color)),
                Cell::from(format_qty(pos.quantity))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format_price_opt(pos.current_price)).style(price_style),
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
        Constraint::Length(10),
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

            let row_bg = if i == app.selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_1_alt
            };

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
            let asset_line =
                Line::from(vec![marker, Span::raw(" "), Span::raw(asset_text)]);

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
                Cell::from(asset_line).style(Style::default().fg(cat_color)),
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

    let border_color = if app.detail_open {
        t.border_inactive
    } else {
        t.border_active
    };

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
}
