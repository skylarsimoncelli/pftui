use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset::AssetCategory;

const EIGHTH_BLOCKS: &[char] = &[' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Aggregate allocation by category
    let mut cat_allocs: Vec<(AssetCategory, f64)> = Vec::new();
    for cat in AssetCategory::all() {
        let alloc: rust_decimal::Decimal = app
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
        let bar_spans = fractional_bar(bar_width, ratio, cat_color, t.surface_1);

        let mut spans = bar_spans;
        spans.push(Span::styled(
            format!(" {} {:>4.0}%", label, pct),
            Style::default().fg(cat_color),
        ));

        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No data",
            Style::default().fg(t.text_muted),
        )));
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

fn fractional_bar(width: usize, ratio: f64, fg: Color, bg: Color) -> Vec<Span<'static>> {
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

    let mut spans = Vec::new();
    if full_cells > 0 {
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
