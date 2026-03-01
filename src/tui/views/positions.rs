use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::config::PortfolioMode;
use crate::tui::theme;

const SPARKLINE_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

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
        Cell::from("Gain%"),
        Cell::from("Alloc%"),
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

            Row::new(vec![
                Cell::from(asset_line).style(Style::default().fg(cat_color)),
                Cell::from(format_qty(pos.quantity))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format_price_opt(pos.current_price)).style(price_style),
                Cell::from(format_gain_pct(pos.gain_pct))
                    .style(Style::default().fg(gain_color)),
                Cell::from(format_alloc_pct(pos.allocation_pct))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(Line::from(sparkline_spans)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Min(16),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(7),
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
        Cell::from("Alloc%"),
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

            Row::new(vec![
                Cell::from(asset_line).style(Style::default().fg(cat_color)),
                Cell::from(format_price_opt(pos.current_price))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format_alloc_pct(pos.allocation_pct))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(Line::from(sparkline_spans)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(8),
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
