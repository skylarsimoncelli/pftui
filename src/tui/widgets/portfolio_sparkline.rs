use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::tui::theme;

const BRAILLE_ROWS: usize = 4;

/// All available timeframe periods for gain/loss display (label, approximate days back).
const ALL_TIMEFRAME_PERIODS: &[(&str, usize)] = &[
    ("1D", 1),
    ("1W", 7),
    ("1M", 30),
    ("3M", 90),
    ("6M", 180),
    ("1Y", 365),
];

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    // Split area: 1 line for timeframe selector + rest for chart
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Timeframe selector bar
            Constraint::Min(8),    // Chart area
        ])
        .split(area);

    // Render timeframe selector bar and store click targets
    render_timeframe_selector(frame, chunks[0], app);

    // Now borrow theme and other fields (after mutable borrow above is done)
    let t = &app.theme;

    // Render chart in the remaining space
    let chart_area = chunks[1];
    let timeframe_days = app.sparkline_timeframe.days() as usize;

    // Filter history to the selected sparkline timeframe
    let full_history = &app.portfolio_value_history;
    let history: &[(String, Decimal)] = if full_history.len() > timeframe_days {
        &full_history[full_history.len() - timeframe_days..]
    } else {
        full_history
    };

    // Only show gain/loss periods that fit within the selected timeframe
    let timeframe_periods: Vec<(&str, usize)> = ALL_TIMEFRAME_PERIODS
        .iter()
        .filter(|(_, days)| *days < timeframe_days)
        .copied()
        .collect();

    // Dynamic title: show timeframe label and current portfolio value if available
    let tf_label = app.sparkline_timeframe.label();
    let csym = crate::config::currency_symbol(&app.base_currency);
    let title = if let Some((_, latest)) = history.last() {
        format!(
            " Portfolio {}  {} ",
            tf_label,
            format_compact_value(*latest, csym)
        )
    } else {
        format!(" Portfolio {} ", tf_label)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(crate::tui::theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .style(Style::default().bg(t.surface_1))
        .title(Span::styled(
            title,
            Style::default().fg(t.text_primary).bold(),
        ));

    let inner = block.inner(chart_area);
    frame.render_widget(block, chart_area);

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

    // Compute timeframe gains for display below the chart
    let timeframe_gains = compute_timeframe_gains(history, &timeframe_periods);
    // Determine which timeframe to highlight based on active change_timeframe
    let active_label = match app.change_timeframe {
        crate::app::ChangeTimeframe::OneHour => "1D", // 1h uses 1D display
        crate::app::ChangeTimeframe::TwentyFourHour => "1D",
        crate::app::ChangeTimeframe::SevenDay => "1W",
        crate::app::ChangeTimeframe::ThirtyDay => "1M",
        crate::app::ChangeTimeframe::YearToDate => "1Y",
    };
    // Reserve lines: 1 separator + gain rows (up to 2 lines for timeframes)
    let gain_lines = build_gain_lines(
        &timeframe_gains,
        inner.width as usize,
        t,
        csym,
        active_label,
    );
    let reserved_lines = 1 + gain_lines.len(); // separator + gain display

    let chart_height = inner.height.saturating_sub(reserved_lines as u16) as usize;
    let chart_width = inner.width as usize;

    if chart_height == 0 || chart_width == 0 {
        // Not enough space for chart, just show gains
        let mut lines = Vec::new();
        lines.extend(gain_lines);
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
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

    // Timeframe gain/loss lines
    lines.extend(gain_lines);

    let chart_area = Rect::new(inner.x, inner.y, inner.width, inner.height);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, chart_area);
}

/// Represents a computed gain for a specific timeframe.
struct TimeframeGain<'a> {
    label: &'a str,
    change: Decimal,
    pct: Decimal,
}

/// Compute gains for each timeframe period by looking back N entries in history.
/// Each entry is roughly one trading day.
fn compute_timeframe_gains<'a>(
    history: &[(String, Decimal)],
    periods: &[(&'a str, usize)],
) -> Vec<TimeframeGain<'a>> {
    if history.is_empty() {
        return Vec::new();
    }

    let latest = history.last().map(|(_, v)| *v).unwrap_or(dec!(0));
    let len = history.len();
    let mut gains = Vec::new();

    for &(label, days_back) in periods {
        if len <= days_back {
            continue;
        }
        let idx = len.saturating_sub(days_back + 1);
        let past_value = history[idx].1;
        if past_value > dec!(0) {
            let change = latest - past_value;
            let pct = (change / past_value) * dec!(100);
            gains.push(TimeframeGain { label, change, pct });
        }
    }

    gains
}

/// Build styled gain/loss lines that fit within the given width.
/// Tries to fit all timeframes on one line; wraps to two if needed.
/// The active_label timeframe is highlighted with bold text.
fn build_gain_lines<'a>(
    gains: &[TimeframeGain<'_>],
    width: usize,
    t: &crate::tui::theme::Theme,
    csym: &str,
    active_label: &str,
) -> Vec<Line<'a>> {
    if gains.is_empty() {
        return vec![Line::from(Span::styled(
            "No period data yet",
            Style::default().fg(t.text_muted),
        ))];
    }

    let mut items: Vec<Vec<Span<'a>>> = Vec::new();
    for g in gains {
        let is_active = g.label == active_label;

        let change_color = if g.pct > dec!(0) {
            t.gain_green
        } else if g.pct < dec!(0) {
            t.loss_red
        } else {
            t.neutral
        };

        let arrow = if g.pct > dec!(0) {
            "▲"
        } else if g.pct < dec!(0) {
            "▼"
        } else {
            "─"
        };
        let change_str = format_compact_change(g.change, csym);
        let pct_str = format!("{:+.1}%", g.pct);

        // Highlight active timeframe with bold and brighter colors
        let label_style = if is_active {
            Style::default().fg(t.text_primary).bold()
        } else {
            Style::default().fg(t.text_secondary)
        };

        let value_style = if is_active {
            Style::default().fg(change_color).bold()
        } else {
            Style::default().fg(change_color)
        };

        let pct_style = if is_active {
            Style::default().fg(change_color).bold()
        } else {
            Style::default().fg(change_color).dim()
        };

        items.push(vec![
            Span::styled(format!("{} ", g.label), label_style),
            Span::styled(format!("{}{} ", arrow, change_str), value_style),
            Span::styled(pct_str, pct_style),
        ]);
    }

    // Try to fit all on one line with " │ " separators
    let mut single_line_spans: Vec<Span<'a>> = Vec::new();
    let mut single_line_width: usize = 0;

    for (i, item_spans) in items.iter().enumerate() {
        if i > 0 {
            single_line_spans.push(Span::styled(" │ ", Style::default().fg(t.border_subtle)));
            single_line_width += 3;
        }
        for s in item_spans {
            single_line_width += s.width();
            single_line_spans.push(s.clone());
        }
    }

    if single_line_width <= width {
        return vec![Line::from(single_line_spans)];
    }

    // Wrap to two lines: split roughly in half
    let mid = items.len() / 2;
    let mut line1_spans: Vec<Span<'a>> = Vec::new();
    for (i, item_spans) in items[..mid].iter().enumerate() {
        if i > 0 {
            line1_spans.push(Span::styled(" │ ", Style::default().fg(t.border_subtle)));
        }
        for s in item_spans {
            line1_spans.push(s.clone());
        }
    }

    let mut line2_spans: Vec<Span<'a>> = Vec::new();
    for (i, item_spans) in items[mid..].iter().enumerate() {
        if i > 0 {
            line2_spans.push(Span::styled(" │ ", Style::default().fg(t.border_subtle)));
        }
        for s in item_spans {
            line2_spans.push(s.clone());
        }
    }

    vec![Line::from(line1_spans), Line::from(line2_spans)]
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

fn format_compact_value(v: Decimal, sym: &str) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 1_000_000.0 {
        format!("{}{:.1}M", sym, f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("{}{:.1}k", sym, f / 1_000.0)
    } else {
        format!("{}{:.0}", sym, f)
    }
}

fn format_compact_change(v: Decimal, sym: &str) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 1_000_000.0 {
        format!("{}{:+.1}M", sym, f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("{}{:+.1}k", sym, f / 1_000.0)
    } else {
        format!("{}{:+.0}", sym, f)
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

/// Render a clickable timeframe selector bar: [ 1h ] [ 24h ] [ 7d ] [ 30d ] [ YTD ]
/// Stores click target coordinates in app.timeframe_selector_buttons.
fn render_timeframe_selector(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::app::ChangeTimeframe;

    let t = &app.theme;

    // Store the row for click detection
    app.timeframe_selector_row = Some(area.y);
    app.timeframe_selector_buttons.clear();

    // All available timeframes
    let timeframes = [
        ChangeTimeframe::OneHour,
        ChangeTimeframe::TwentyFourHour,
        ChangeTimeframe::SevenDay,
        ChangeTimeframe::ThirtyDay,
        ChangeTimeframe::YearToDate,
    ];

    let mut spans = Vec::new();
    let mut col = area.x;

    // Add spacing before first button
    spans.push(Span::raw("  "));
    col += 2;

    for (i, &tf) in timeframes.iter().enumerate() {
        let is_active = app.change_timeframe == tf;
        let label = tf.label();

        // Button format: [ 24h ] with spacing
        let button_text = format!("[ {} ]", label);
        let button_width = button_text.len() as u16;

        // Store click target (column range for this button)
        let col_start = col;
        let col_end = col + button_width - 1;
        app.timeframe_selector_buttons
            .push((tf, (col_start, col_end)));

        // Style: active = bold + accent color, inactive = secondary
        let style = if is_active {
            Style::default().fg(t.text_accent).bold()
        } else {
            Style::default().fg(t.text_secondary)
        };

        spans.push(Span::styled(button_text, style));
        col += button_width;

        // Add spacing between buttons (except after last)
        if i < timeframes.len() - 1 {
            spans.push(Span::raw(" "));
            col += 1;
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(t.surface_1));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_identity() {
        let vals = vec![1.0, 2.0, 3.0];
        assert_eq!(resample(&vals, 3), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_resample_empty() {
        let vals: Vec<f64> = vec![];
        assert_eq!(resample(&vals, 4), vec![0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_resample_upsample() {
        let vals = vec![0.0, 10.0];
        let result = resample(&vals, 5);
        assert_eq!(result.len(), 5);
        assert!((result[0] - 0.0).abs() < 0.01);
        // result[4] = lerp at src_idx = (4/5)*1 = 0.8, so 8.0
        assert!((result[4] - 8.0).abs() < 0.01);
        // Values should be monotonically increasing
        for i in 1..result.len() {
            assert!(result[i] >= result[i - 1]);
        }
    }

    #[test]
    fn test_format_compact_value_thousands() {
        assert_eq!(format_compact_value(dec!(5432), "$"), "$5.4k");
    }

    #[test]
    fn test_format_compact_value_millions() {
        assert_eq!(format_compact_value(dec!(1234567), "$"), "$1.2M");
    }

    #[test]
    fn test_format_compact_value_small() {
        assert_eq!(format_compact_value(dec!(42), "$"), "$42");
    }

    #[test]
    fn test_format_compact_change_positive() {
        assert_eq!(format_compact_change(dec!(1500), "$"), "$+1.5k");
    }

    #[test]
    fn test_format_compact_change_negative() {
        assert_eq!(format_compact_change(dec!(-250), "$"), "$-250");
    }

    #[test]
    fn test_format_compact_change_millions() {
        assert_eq!(format_compact_change(dec!(2500000), "$"), "$+2.5M");
    }

    #[test]
    fn test_format_compact_value_euro() {
        assert_eq!(format_compact_value(dec!(5432), "€"), "€5.4k");
        assert_eq!(format_compact_value(dec!(42), "€"), "€42");
    }

    #[test]
    fn test_format_compact_change_gbp() {
        assert_eq!(format_compact_change(dec!(1500), "£"), "£+1.5k");
        assert_eq!(format_compact_change(dec!(-800), "£"), "£-800");
    }

    /// Default periods for 3M sparkline timeframe (1D, 1W, 1M fit within 90 days).
    const TEST_PERIODS_3M: &[(&str, usize)] = &[("1D", 1), ("1W", 7), ("1M", 30)];

    #[test]
    fn test_compute_timeframe_gains_empty() {
        let history: Vec<(String, Decimal)> = Vec::new();
        assert!(compute_timeframe_gains(&history, TEST_PERIODS_3M).is_empty());
    }

    #[test]
    fn test_compute_timeframe_gains_too_short() {
        // Only 1 entry — not enough for any timeframe
        let history = vec![("2026-03-02".to_string(), dec!(1000))];
        assert!(compute_timeframe_gains(&history, TEST_PERIODS_3M).is_empty());
    }

    #[test]
    fn test_compute_timeframe_gains_1d() {
        // 2 entries: enough for 1D
        let history = vec![
            ("2026-03-01".to_string(), dec!(1000)),
            ("2026-03-02".to_string(), dec!(1050)),
        ];
        let gains = compute_timeframe_gains(&history, TEST_PERIODS_3M);
        assert_eq!(gains.len(), 1);
        assert_eq!(gains[0].label, "1D");
        assert_eq!(gains[0].change, dec!(50));
        assert_eq!(gains[0].pct, dec!(5));
    }

    #[test]
    fn test_compute_timeframe_gains_multiple_periods() {
        // 31 entries: enough for 1D, 1W, 1M
        let mut history = Vec::new();
        for i in 0..31 {
            let val = dec!(10000) + Decimal::from(i) * dec!(100);
            history.push((format!("2026-02-{:02}", i + 1), val));
        }
        let gains = compute_timeframe_gains(&history, TEST_PERIODS_3M);
        assert_eq!(gains.len(), 3); // 1D, 1W, 1M
        assert_eq!(gains[0].label, "1D");
        assert_eq!(gains[1].label, "1W");
        assert_eq!(gains[2].label, "1M");
    }

    #[test]
    fn test_compute_timeframe_gains_negative() {
        let history = vec![
            ("2026-03-01".to_string(), dec!(1000)),
            ("2026-03-02".to_string(), dec!(900)),
        ];
        let gains = compute_timeframe_gains(&history, TEST_PERIODS_3M);
        assert_eq!(gains[0].change, dec!(-100));
        assert_eq!(gains[0].pct, dec!(-10));
    }

    #[test]
    fn test_compute_timeframe_gains_with_larger_periods() {
        // Test that 6M and 1Y periods work when there's enough data
        let periods_1y: &[(&str, usize)] =
            &[("1D", 1), ("1W", 7), ("1M", 30), ("3M", 90), ("6M", 180)];
        let mut history = Vec::new();
        for i in 0..200 {
            let val = dec!(10000) + Decimal::from(i) * dec!(50);
            history.push((format!("day-{:04}", i), val));
        }
        let gains = compute_timeframe_gains(&history, periods_1y);
        assert_eq!(gains.len(), 5); // All 5 periods fit
        assert_eq!(gains[0].label, "1D");
        assert_eq!(gains[4].label, "6M");
    }

    #[test]
    fn test_compute_timeframe_gains_1w_periods_only() {
        // With 1W timeframe, only 1D period should be shown
        let periods_1w: &[(&str, usize)] = &[("1D", 1)];
        let history = vec![
            ("2026-02-25".to_string(), dec!(1000)),
            ("2026-02-26".to_string(), dec!(1010)),
            ("2026-02-27".to_string(), dec!(1020)),
        ];
        let gains = compute_timeframe_gains(&history, periods_1w);
        assert_eq!(gains.len(), 1);
        assert_eq!(gains[0].label, "1D");
    }

    #[test]
    fn test_braille_char_empty_row() {
        // When both values are 0 and row is 1 (above row 0), should produce blank braille
        let ch = braille_char(0, 0, 1, BRAILLE_ROWS);
        // Row 1 base = 4, checking y positions 7,6,5,4 - none should have dots since values are 0
        assert_eq!(ch, '\u{2800}'); // empty braille
    }

    #[test]
    fn test_format_compact_short() {
        assert_eq!(format_compact_short(1500.0), "2k"); // rounds
        assert_eq!(format_compact_short(500.0), "500");
        assert_eq!(format_compact_short(2_500_000.0), "2.5M");
    }
}
