use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{App, ChartKind, ChartVariant};
use crate::models::price::HistoryRecord;
use crate::tui::theme;

const BRAILLE_ROWS: usize = 4;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let pos = match app.selected_position() {
        Some(p) => p,
        None => return,
    };

    let variants = App::chart_variants_for_position(pos);
    let variant_count = variants.len();
    let idx = app.chart_index % variant_count.max(1);
    let variant = match variants.into_iter().nth(idx) {
        Some(v) => v,
        None => return,
    };

    // Navigation hint
    let nav_hint = if variant_count > 1 {
        format!(" [{}/{}] J/K ", idx + 1, variant_count)
    } else {
        String::new()
    };
    let title = format!(" {} 90d ", variant.label);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_active))
        .style(Style::default().bg(t.surface_1))
        .title(Span::styled(
            title,
            Style::default().fg(t.text_primary).bold(),
        ))
        .title(
            Line::from(Span::styled(
                nav_hint,
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        );

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match &variant.kind {
        ChartKind::All => {
            // Get individual variants (skip index 0 which is "All")
            let all_variants = App::chart_variants_for_position(pos);
            let individuals: Vec<&ChartVariant> = all_variants.iter().skip(1).collect();
            render_multi_panel(frame, inner, &individuals, app);
        }
        ChartKind::Single { symbol, .. } => {
            render_single_chart(frame, inner, symbol, &variant.label, app);
        }
        ChartKind::Ratio {
            num_symbol,
            den_symbol,
            ..
        } => {
            render_ratio_chart(frame, inner, num_symbol, den_symbol, &variant.label, app);
        }
    }
}

/// Renders a multi-panel stacked view of all individual charts
fn render_multi_panel(
    frame: &mut Frame,
    area: Rect,
    variants: &[&ChartVariant],
    app: &App,
) {
    let t = &app.theme;
    if variants.is_empty() || area.height < 4 {
        return;
    }

    let panel_count = variants.len();
    let panel_height = area.height / panel_count as u16;
    if panel_height < 3 {
        // Too small for multi-panel; just show first
        if let Some(v) = variants.first() {
            match &v.kind {
                ChartKind::Single { symbol, .. } => {
                    render_single_chart(frame, area, symbol, &v.label, app);
                }
                ChartKind::Ratio {
                    num_symbol,
                    den_symbol,
                    ..
                } => {
                    render_ratio_chart(frame, area, num_symbol, den_symbol, &v.label, app);
                }
                _ => {}
            }
        }
        return;
    }

    for (i, v) in variants.iter().enumerate() {
        let y = area.y + (i as u16 * panel_height);
        let h = if i == panel_count - 1 {
            area.height - (i as u16 * panel_height)
        } else {
            panel_height
        };
        let panel_area = Rect::new(area.x, y, area.width, h);

        // Label on first line of each panel
        let label_line = Line::from(Span::styled(
            format!(" {} ", v.label),
            Style::default().fg(t.text_accent).bold(),
        ));
        let label_area = Rect::new(panel_area.x, panel_area.y, panel_area.width, 1);
        frame.render_widget(Paragraph::new(label_line), label_area);

        let chart_area = Rect::new(
            panel_area.x,
            panel_area.y + 1,
            panel_area.width,
            panel_area.height.saturating_sub(1),
        );

        match &v.kind {
            ChartKind::Single { symbol, .. } => {
                render_single_mini(frame, chart_area, symbol, app);
            }
            ChartKind::Ratio {
                num_symbol,
                den_symbol,
                ..
            } => {
                render_ratio_mini(frame, chart_area, num_symbol, den_symbol, app);
            }
            _ => {}
        }
    }
}

/// Render a single-symbol chart (full size with stats)
fn render_single_chart(
    frame: &mut Frame,
    area: Rect,
    symbol: &str,
    _label: &str,
    app: &App,
) {
    let t = &app.theme;

    let records = match app.price_history.get(symbol) {
        Some(r) if r.len() >= 2 => r,
        _ => {
            let msg = Paragraph::new(Span::styled(
                format!("Loading {}...", symbol),
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };

    let first_close = records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    render_braille_chart(frame, area, records, Some(last_close), gain_pct, t);
}

/// Render a ratio chart (numerator / denominator)
fn render_ratio_chart(
    frame: &mut Frame,
    area: Rect,
    num_symbol: &str,
    den_symbol: &str,
    _label: &str,
    app: &App,
) {
    let t = &app.theme;

    let num_records = match app.price_history.get(num_symbol) {
        Some(r) if r.len() >= 2 => r,
        _ => {
            let msg = Paragraph::new(Span::styled(
                format!("Loading {}...", num_symbol),
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };
    let den_records = match app.price_history.get(den_symbol) {
        Some(r) if r.len() >= 2 => r,
        _ => {
            let msg = Paragraph::new(Span::styled(
                format!("Loading {}...", den_symbol),
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };

    let ratio_records = compute_ratio(num_records, den_records);
    if ratio_records.len() < 2 {
        let msg = Paragraph::new(Span::styled(
            "Insufficient data for ratio",
            Style::default().fg(t.text_muted),
        ));
        frame.render_widget(msg, area);
        return;
    }

    let first_close = ratio_records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = ratio_records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    render_braille_chart(frame, area, &ratio_records, Some(last_close), gain_pct, t);
}

/// Compact single chart for multi-panel (no stats line, just braille)
fn render_single_mini(
    frame: &mut Frame,
    area: Rect,
    symbol: &str,
    app: &App,
) {
    let t = &app.theme;
    let records = match app.price_history.get(symbol) {
        Some(r) if r.len() >= 2 => r,
        _ => {
            let msg = Paragraph::new(Span::styled(
                "...",
                Style::default().fg(t.text_muted),
            ));
            frame.render_widget(msg, area);
            return;
        }
    };

    let first_close = records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    render_braille_mini(frame, area, records, Some(last_close), gain_pct, t);
}

/// Compact ratio chart for multi-panel
fn render_ratio_mini(
    frame: &mut Frame,
    area: Rect,
    num_symbol: &str,
    den_symbol: &str,
    app: &App,
) {
    let t = &app.theme;
    let num_records = match app.price_history.get(num_symbol) {
        Some(r) if r.len() >= 2 => r,
        _ => {
            let msg = Paragraph::new(Span::styled("...", Style::default().fg(t.text_muted)));
            frame.render_widget(msg, area);
            return;
        }
    };
    let den_records = match app.price_history.get(den_symbol) {
        Some(r) if r.len() >= 2 => r,
        _ => {
            let msg = Paragraph::new(Span::styled("...", Style::default().fg(t.text_muted)));
            frame.render_widget(msg, area);
            return;
        }
    };

    let ratio_records = compute_ratio(num_records, den_records);
    if ratio_records.len() < 2 {
        return;
    }

    let first_close = ratio_records.first().map(|r| r.close).unwrap_or(dec!(0));
    let last_close = ratio_records.last().map(|r| r.close).unwrap_or(dec!(0));
    let gain_pct = if first_close > dec!(0) {
        Some(((last_close - first_close) / first_close) * dec!(100))
    } else {
        None
    };

    render_braille_mini(frame, area, &ratio_records, Some(last_close), gain_pct, t);
}

/// Compute ratio records by aligning two histories on date and dividing
fn compute_ratio(
    numerator: &[HistoryRecord],
    denominator: &[HistoryRecord],
) -> Vec<HistoryRecord> {
    use std::collections::HashMap;

    let den_map: HashMap<&str, Decimal> = denominator
        .iter()
        .map(|r| (r.date.as_str(), r.close))
        .collect();

    numerator
        .iter()
        .filter_map(|nr| {
            let den_close = den_map.get(nr.date.as_str())?;
            if *den_close > dec!(0) {
                Some(HistoryRecord {
                    date: nr.date.clone(),
                    close: nr.close / *den_close,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Full braille chart with stats line (price, gain%, H/L)
fn render_braille_chart(
    frame: &mut Frame,
    area: Rect,
    records: &[HistoryRecord],
    current_price: Option<Decimal>,
    gain_pct: Option<Decimal>,
    t: &theme::Theme,
) {
    if area.width < 4 || area.height < 4 {
        return;
    }

    let values: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let chart_height = area.height.saturating_sub(3) as usize;
    let chart_width = area.width as usize;

    if chart_height == 0 || chart_width == 0 {
        return;
    }

    let sample_count = chart_width * 2;
    let resampled = resample(&values, sample_count);

    let min_val = resampled.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = resampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_val - min_val;
    let dot_rows = chart_height * BRAILLE_ROWS;

    let normalized: Vec<usize> = resampled
        .iter()
        .map(|v| {
            if range > 0.0 {
                (((v - min_val) / range) * (dot_rows.saturating_sub(1)) as f64).round() as usize
            } else {
                dot_rows / 2
            }
        })
        .collect();

    let gain = gain_pct.unwrap_or(dec!(0));
    let gain_f: f64 = gain.to_string().parse().unwrap_or(0.0);
    let (grad_low, grad_mid, grad_high) = gain_gradient(gain_f, t);

    let mut lines: Vec<Line> = Vec::new();
    for row in (0..chart_height).rev() {
        let position = if chart_height > 1 {
            row as f32 / (chart_height - 1) as f32
        } else {
            0.5
        };
        let row_color = theme::gradient_3(grad_low, grad_mid, grad_high, position);

        let mut spans = Vec::new();
        for col in 0..chart_width {
            let idx0 = col * 2;
            let idx1 = idx0 + 1;
            let v0 = normalized.get(idx0).copied().unwrap_or(0);
            let v1 = normalized.get(idx1).copied().unwrap_or(0);
            let ch = braille_char(v0, v1, row, BRAILLE_ROWS);
            spans.push(Span::styled(
                String::from(ch),
                Style::default().fg(row_color),
            ));
        }

        // Y-axis labels
        let label_width = 6;
        if row == chart_height - 1 && chart_width > label_width + 2 {
            overlay_label(&mut spans, format_compact_short(max_val), t);
        }
        if row == 0 && chart_width > label_width + 2 {
            overlay_label(&mut spans, format_compact_short(min_val), t);
        }

        lines.push(Line::from(spans));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(t.border_subtle),
    )));

    // Price + gain line
    let price_str = current_price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string());
    let gain_color = if gain > dec!(0) {
        t.gain_green
    } else if gain < dec!(0) {
        t.loss_red
    } else {
        t.neutral
    };

    lines.push(Line::from(vec![
        Span::styled(price_str, Style::default().fg(t.text_primary).bold()),
        Span::raw(" "),
        Span::styled(
            format!("({:+.1}%)", gain),
            Style::default().fg(gain_color),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "H:{} L:{}",
                format_price_f64(max_val),
                format_price_f64(min_val)
            ),
            Style::default().fg(t.text_muted),
        ),
    ]));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Compact braille chart for multi-panel (1-line stats)
fn render_braille_mini(
    frame: &mut Frame,
    area: Rect,
    records: &[HistoryRecord],
    current_price: Option<Decimal>,
    gain_pct: Option<Decimal>,
    t: &theme::Theme,
) {
    if area.width < 4 || area.height < 2 {
        return;
    }

    let values: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    // Reserve 1 line for stats
    let chart_height = area.height.saturating_sub(1) as usize;
    let chart_width = area.width as usize;

    if chart_height == 0 || chart_width == 0 {
        return;
    }

    let sample_count = chart_width * 2;
    let resampled = resample(&values, sample_count);

    let min_val = resampled.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = resampled.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_val - min_val;
    let dot_rows = chart_height * BRAILLE_ROWS;

    let normalized: Vec<usize> = resampled
        .iter()
        .map(|v| {
            if range > 0.0 {
                (((v - min_val) / range) * (dot_rows.saturating_sub(1)) as f64).round() as usize
            } else {
                dot_rows / 2
            }
        })
        .collect();

    let gain = gain_pct.unwrap_or(dec!(0));
    let gain_f: f64 = gain.to_string().parse().unwrap_or(0.0);
    let (grad_low, grad_mid, grad_high) = gain_gradient(gain_f, t);

    let mut lines: Vec<Line> = Vec::new();
    for row in (0..chart_height).rev() {
        let position = if chart_height > 1 {
            row as f32 / (chart_height - 1) as f32
        } else {
            0.5
        };
        let row_color = theme::gradient_3(grad_low, grad_mid, grad_high, position);

        let mut spans = Vec::new();
        for col in 0..chart_width {
            let idx0 = col * 2;
            let idx1 = idx0 + 1;
            let v0 = normalized.get(idx0).copied().unwrap_or(0);
            let v1 = normalized.get(idx1).copied().unwrap_or(0);
            let ch = braille_char(v0, v1, row, BRAILLE_ROWS);
            spans.push(Span::styled(
                String::from(ch),
                Style::default().fg(row_color),
            ));
        }
        lines.push(Line::from(spans));
    }

    // Compact stats line
    let price_str = current_price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string());
    let gain_color = if gain > dec!(0) {
        t.gain_green
    } else if gain < dec!(0) {
        t.loss_red
    } else {
        t.neutral
    };
    lines.push(Line::from(vec![
        Span::styled(price_str, Style::default().fg(t.text_secondary)),
        Span::raw(" "),
        Span::styled(
            format!("{:+.1}%", gain),
            Style::default().fg(gain_color),
        ),
    ]));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn gain_gradient(gain_f: f64, t: &theme::Theme) -> (Color, Color, Color) {
    if gain_f > 0.0 {
        (
            Color::Rgb(60, 80, 60),
            Color::Rgb(100, 190, 80),
            t.gain_green,
        )
    } else if gain_f < 0.0 {
        (
            t.loss_red,
            Color::Rgb(190, 100, 80),
            Color::Rgb(120, 60, 60),
        )
    } else {
        (t.chart_grad_low, t.chart_grad_mid, t.chart_grad_high)
    }
}

fn overlay_label(spans: &mut [Span], label: String, t: &theme::Theme) {
    for (j, c) in label.chars().enumerate() {
        if j < spans.len() {
            spans[j] = Span::styled(String::from(c), Style::default().fg(t.text_muted));
        }
    }
}

fn resample(values: &[f64], target_len: usize) -> Vec<f64> {
    if values.is_empty() || target_len == 0 {
        return vec![0.0; target_len];
    }
    if values.len() == target_len {
        return values.to_vec();
    }
    let mut result = Vec::with_capacity(target_len);
    for i in 0..target_len {
        let src_idx = (i as f64 / target_len as f64) * (values.len() - 1) as f64;
        let lo = src_idx.floor() as usize;
        let hi = (lo + 1).min(values.len() - 1);
        let frac = src_idx - lo as f64;
        result.push(values[lo] * (1.0 - frac) + values[hi] * frac);
    }
    result
}

fn braille_char(v0: usize, v1: usize, row: usize, dots_per_row: usize) -> char {
    let row_base = row * dots_per_row;
    let mut bits: u8 = 0;

    let col0_bits = [0u8, 1, 2, 6];
    for (dot_idx, &bit) in col0_bits.iter().enumerate() {
        let y = row_base + (dots_per_row - 1 - dot_idx);
        if v0 >= y && y < row_base + dots_per_row {
            bits |= 1 << bit;
        }
    }

    let col1_bits = [3u8, 4, 5, 7];
    for (dot_idx, &bit) in col1_bits.iter().enumerate() {
        let y = row_base + (dots_per_row - 1 - dot_idx);
        if v1 >= y && y < row_base + dots_per_row {
            bits |= 1 << bit;
        }
    }

    char::from_u32(0x2800 + bits as u32).unwrap_or(' ')
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    format_price_f64(f)
}

fn format_price_f64(f: f64) -> String {
    if f >= 10000.0 {
        format!("{:.0}", f)
    } else if f >= 100.0 {
        format!("{:.1}", f)
    } else if f >= 1.0 {
        format!("{:.2}", f)
    } else if f >= 0.001 {
        format!("{:.4}", f)
    } else {
        format!("{:.6}", f)
    }
}

fn format_compact_short(f: f64) -> String {
    if f.abs() >= 1_000_000.0 {
        format!("{:.1}M", f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("{:.0}k", f / 1_000.0)
    } else if f.abs() >= 1.0 {
        format!("{:.0}", f)
    } else {
        format!("{:.3}", f)
    }
}
