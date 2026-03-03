use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::models::asset::AssetCategory;

const EIGHTH_BLOCKS: &[char] = &[' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// Minimum full-cell width for the percentage label to be rendered inside the bar.
const MIN_LABEL_WIDTH: usize = 5;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Aggregate allocation by category
    let mut cat_allocs: Vec<(AssetCategory, f64)> = Vec::new();
    for cat in AssetCategory::all() {
        let alloc: Decimal = app
            .positions
            .iter()
            .filter(|p| p.category == *cat)
            .filter_map(|p| p.allocation_pct)
            .sum();
        if alloc > dec!(0) {
            cat_allocs.push((*cat, alloc.to_string().parse::<f64>().unwrap_or(0.0)));
        }
    }
    cat_allocs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let inner_width = area.width.saturating_sub(4) as usize;
    let bar_width = inner_width.saturating_sub(10);

    let mut lines = Vec::new();
    for (cat, pct) in &cat_allocs {
        let cat_color = t.category_color(*cat);
        let label = match cat {
            AssetCategory::Equity => "Eqty",
            AssetCategory::Crypto => "Cryp",
            AssetCategory::Forex => "Fex",
            AssetCategory::Cash => "Cash",
            AssetCategory::Commodity => "Comd",
            AssetCategory::Fund => "Fund",
        };

        let ratio = pct / 100.0;
        let bar_spans = fractional_bar_with_label(bar_width, ratio, *pct, cat_color, t.surface_1);

        // Compute allocation change indicator vs previous day
        let change_indicator = allocation_change_span(*cat, *pct, app, t);

        let mut spans = bar_spans;
        spans.push(Span::styled(
            format!(" {} {:>4.0}%", label, pct),
            Style::default().fg(cat_color),
        ));
        spans.push(change_indicator);

        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No data",
            Style::default().fg(t.text_muted),
        )));
    }

    // Total portfolio value line (hidden in privacy mode)
    if !is_privacy_view(app) && app.total_value > dec!(0) {
        let csym = crate::config::currency_symbol(&app.base_currency);
        let total_str = format_compact_value(app.total_value);
        let total_line = Line::from(Span::styled(
            format!("Total: {csym}{total_str}"),
            Style::default().fg(t.text_secondary),
        ));
        lines.push(total_line);
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_inactive))
        .style(Style::default().bg(t.surface_1))
        .title(Span::styled(
            " Allocation ",
            Style::default().fg(t.text_primary).bold(),
        ));

    let widget = Paragraph::new(lines).block(block);
    frame.render_widget(widget, area);
}

/// Compute the allocation change direction and magnitude.
/// Returns `Some((diff, is_up))` if the change is significant (>= 0.1pp),
/// or `None` if no previous data or change is negligible.
fn allocation_change(
    cat: AssetCategory,
    current_pct: f64,
    app: &App,
) -> Option<f64> {
    let prev_dec = app.prev_day_cat_allocations.get(&cat)?;
    let prev: f64 = prev_dec.to_string().parse().unwrap_or(0.0);
    let diff = current_pct - prev;
    if diff.abs() < 0.1 {
        None
    } else {
        Some(diff)
    }
}

/// Build a ▲/▼ change indicator span for a category's allocation percentage.
/// Compares current allocation to the previous day's allocation stored in `app.prev_day_cat_allocations`.
/// Returns a styled span with the arrow and percentage-point change, or an empty span if no data.
fn allocation_change_span<'a>(
    cat: AssetCategory,
    current_pct: f64,
    app: &App,
    t: &crate::tui::theme::Theme,
) -> Span<'a> {
    match allocation_change(cat, current_pct, app) {
        Some(diff) => {
            let (arrow, color) = if diff > 0.0 {
                ("▲", t.gain_green)
            } else {
                ("▼", t.loss_red)
            };
            Span::styled(
                format!("{}{:.1}", arrow, diff.abs()),
                Style::default().fg(color),
            )
        }
        None => Span::raw(""),
    }
}

/// Format a portfolio value compactly: $1.23M, $456.7K, $12,345
pub fn format_compact_value(value: Decimal) -> String {
    let abs = if value < dec!(0) { -value } else { value };
    let f = abs.to_string().parse::<f64>().unwrap_or(0.0);
    if f >= 1_000_000.0 {
        format!("{:.2}M", f / 1_000_000.0)
    } else if f >= 10_000.0 {
        format!("{:.1}K", f / 1_000.0)
    } else if f >= 1_000.0 {
        // Insert comma for thousands
        let whole = f as u64;
        let frac = ((f - whole as f64) * 100.0).round() as u64;
        if frac > 0 {
            format!("{},{:03}.{:02}", whole / 1000, whole % 1000, frac)
        } else {
            format!("{},{:03}", whole / 1000, whole % 1000)
        }
    } else {
        format!("{:.2}", f)
    }
}

/// Render a fractional bar with an optional percentage label inside the filled portion.
///
/// When the filled portion is wide enough (>= MIN_LABEL_WIDTH chars), the percentage
/// is overlaid inside the bar (e.g. "42%") using contrasting text color on the bar color bg.
fn fractional_bar_with_label(
    width: usize,
    ratio: f64,
    pct: f64,
    fg: Color,
    bg: Color,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let ratio = ratio.clamp(0.0, 1.0);
    let total_eighths = (ratio * width as f64 * 8.0).round() as usize;
    let full_cells = total_eighths / 8;
    let remainder = total_eighths % 8;
    let empty_cells = width
        .saturating_sub(full_cells)
        .saturating_sub(if remainder > 0 { 1 } else { 0 });

    // Try to place percentage label inside the filled portion
    let pct_label = format!("{:.0}%", pct);
    let label_len = pct_label.len();
    let show_label = full_cells >= MIN_LABEL_WIDTH && label_len < full_cells;

    let mut spans = Vec::new();

    if show_label {
        // Render bar with embedded label: [███NN%███]
        // Center the label within the full cells
        let pad_total = full_cells - label_len;
        let pad_left = pad_total / 2;
        let pad_right = pad_total - pad_left;

        // Contrasting text color for label on colored background
        let label_style = Style::default().fg(Color::Rgb(0, 0, 0)).bg(fg).bold();

        if pad_left > 0 {
            spans.push(Span::styled("█".repeat(pad_left), Style::default().fg(fg)));
        }
        spans.push(Span::styled(pct_label, label_style));
        if pad_right > 0 {
            spans.push(Span::styled(
                "█".repeat(pad_right),
                Style::default().fg(fg),
            ));
        }
    } else if full_cells > 0 {
        spans.push(Span::styled(
            "█".repeat(full_cells),
            Style::default().fg(fg),
        ));
    }

    if remainder > 0 {
        spans.push(Span::styled(
            String::from(EIGHTH_BLOCKS[remainder]),
            Style::default().fg(fg).bg(bg),
        ));
    }
    if empty_cells > 0 {
        spans.push(Span::styled(
            " ".repeat(empty_cells),
            Style::default().bg(bg),
        ));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_compact_value_millions() {
        let v = Decimal::from(2_500_000i64);
        assert_eq!(format_compact_value(v), "2.50M");
    }

    #[test]
    fn format_compact_value_hundred_thousands() {
        let v = Decimal::from(456_700i64);
        assert_eq!(format_compact_value(v), "456.7K");
    }

    #[test]
    fn format_compact_value_thousands() {
        let v = Decimal::from(12_345i64);
        assert_eq!(format_compact_value(v), "12.3K");
    }

    #[test]
    fn format_compact_value_small() {
        let v = Decimal::from(999i64);
        assert_eq!(format_compact_value(v), "999.00");
    }

    #[test]
    fn fractional_bar_label_shown_when_wide() {
        // 50% of 20 chars = 10 full cells, label "50%" is 3 chars, fits
        let spans = fractional_bar_with_label(20, 0.5, 50.0, Color::Green, Color::Black);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("50%"), "Label should appear in bar: {}", text);
    }

    #[test]
    fn fractional_bar_label_hidden_when_narrow() {
        // 10% of 20 chars = 2 full cells, too narrow for label
        let spans = fractional_bar_with_label(20, 0.1, 10.0, Color::Green, Color::Black);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            !text.contains("10%"),
            "Label should NOT appear in narrow bar: {}",
            text
        );
    }

    #[test]
    fn fractional_bar_zero_width() {
        let spans = fractional_bar_with_label(0, 0.5, 50.0, Color::Green, Color::Black);
        assert!(spans.is_empty());
    }

    #[test]
    fn fractional_bar_full_width() {
        // 100% should fill entire bar
        let spans = fractional_bar_with_label(10, 1.0, 100.0, Color::Green, Color::Black);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text.chars().count(), 10);
        assert!(text.contains("100%"));
    }

    #[test]
    fn fractional_bar_preserves_total_width() {
        // Bar should always sum to the requested width in chars
        for pct in [5.0, 25.0, 50.0, 75.0, 95.0] {
            let ratio = pct / 100.0;
            let spans = fractional_bar_with_label(30, ratio, pct, Color::Green, Color::Black);
            let total_chars: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            assert_eq!(total_chars, 30, "Width mismatch at {}%", pct);
        }
    }
}
