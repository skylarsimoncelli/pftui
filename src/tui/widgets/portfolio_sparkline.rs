use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::tui::theme;

const BRAILLE_ROWS: usize = 4;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let history = &app.portfolio_value_history;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_inactive))
        .style(Style::default().bg(t.surface_1))
        .title(Span::styled(
            " Portfolio 90d ",
            Style::default().fg(t.text_primary).bold(),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if history.len() < 2 || inner.width < 4 || inner.height < 2 {
        let msg = Paragraph::new(Span::styled(
            "Waiting for data...",
            Style::default().fg(t.text_muted),
        ));
        frame.render_widget(msg, inner);
        return;
    }

    let values: Vec<f64> = history
        .iter()
        .map(|(_, v)| v.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let chart_height = inner.height.saturating_sub(3) as usize; // reserve 3: separator + summary + blank
    let chart_width = inner.width as usize;

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

    // Build braille grid with vertical gradient
    let mut lines: Vec<Line> = Vec::new();
    for row in (0..chart_height).rev() {
        let position = if chart_height > 1 {
            row as f32 / (chart_height - 1) as f32
        } else {
            0.5
        };
        let row_color = theme::gradient_3(
            t.chart_grad_low,
            t.chart_grad_mid,
            t.chart_grad_high,
            position,
        );

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

        // Y-axis labels: max on top row, min on bottom row
        let label_width = 6;
        if row == chart_height - 1 && chart_width > label_width + 2 {
            let label = format_compact_short(max_val);
            let label_spans: Vec<Span> = label
                .chars()
                .map(|c| Span::styled(String::from(c), Style::default().fg(t.text_muted)))
                .collect();
            for (j, s) in label_spans.into_iter().enumerate() {
                if j < spans.len() {
                    spans[j] = s;
                }
            }
        }
        if row == 0 && chart_width > label_width + 2 {
            let label = format_compact_short(min_val);
            let label_spans: Vec<Span> = label
                .chars()
                .map(|c| Span::styled(String::from(c), Style::default().fg(t.text_muted)))
                .collect();
            for (j, s) in label_spans.into_iter().enumerate() {
                if j < spans.len() {
                    spans[j] = s;
                }
            }
        }

        lines.push(Line::from(spans));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(t.border_subtle),
    )));

    // Summary line
    let latest = history.last().map(|(_, v)| *v).unwrap_or(dec!(0));
    let first = history.first().map(|(_, v)| *v).unwrap_or(dec!(0));
    let change_pct = if first > dec!(0) {
        ((latest - first) / first) * dec!(100)
    } else {
        dec!(0)
    };
    let change_color = if change_pct > dec!(0) {
        t.gain_green
    } else if change_pct < dec!(0) {
        t.loss_red
    } else {
        t.neutral
    };

    let summary = Line::from(vec![
        Span::styled(
            format_compact_value(latest),
            Style::default().fg(t.text_primary).bold(),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:+.1}%", change_pct),
            Style::default().fg(change_color),
        ),
    ]);
    lines.push(summary);

    let chart_area = Rect::new(inner.x, inner.y, inner.width, inner.height);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, chart_area);
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

fn format_compact_value(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 1_000_000.0 {
        format!("${:.1}M", f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("${:.1}k", f / 1_000.0)
    } else {
        format!("${:.0}", f)
    }
}

fn format_compact_short(f: f64) -> String {
    if f.abs() >= 1_000_000.0 {
        format!("{:.1}M", f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("{:.0}k", f / 1_000.0)
    } else {
        format!("{:.0}", f)
    }
}
