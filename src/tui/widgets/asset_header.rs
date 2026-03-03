use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::tui::theme;

/// Renders a compact asset info header for the right pane.
/// Shows: symbol, name, price, gain/loss, quantity, allocation%.
/// Always visible above the chart when a position is selected.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let pos = match app.selected_position() {
        Some(p) => p,
        None => return,
    };

    let privacy = is_privacy_view(app);
    let cat_color = t.category_color(pos.category);

    let header_border_color = if app.prices_live {
        theme::pulse_color(t.border_active, t.border_inactive, app.tick_count, theme::PULSE_PERIOD_BORDER)
    } else {
        t.border_active
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(crate::tui::theme::BORDER_ACTIVE)
        .border_style(Style::default().fg(header_border_color))
        .style(Style::default().bg(t.surface_1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width < 10 {
        return;
    }

    let mut lines: Vec<Line> = Vec::with_capacity(4);

    // Line 1: Symbol + Name + Category dot
    let name_display = if pos.name.is_empty() {
        String::new()
    } else {
        format!("  {}", pos.name)
    };

    lines.push(Line::from(vec![
        Span::styled("●", Style::default().fg(cat_color)),
        Span::styled(
            format!(" {}", pos.symbol),
            Style::default().fg(t.text_primary).bold(),
        ),
        Span::styled(name_display, Style::default().fg(t.text_secondary)),
    ]));

    // Line 2: Price + Gain/Loss
    let price_str = pos
        .current_price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string());

    let mut price_spans = vec![
        Span::styled(
            format!("{} {}", price_str, pos.currency),
            Style::default().fg(t.text_primary).bold(),
        ),
    ];

    if !privacy {
        if let Some(gain_pct) = pos.gain_pct {
            let gain_f: f64 = gain_pct.to_string().parse().unwrap_or(0.0);
            let gain_color = theme::gain_intensity_color(t, gain_f);
            let arrow = if gain_pct > dec!(0) {
                "▲"
            } else if gain_pct < dec!(0) {
                "▼"
            } else {
                "─"
            };
            price_spans.push(Span::raw("  "));
            price_spans.push(Span::styled(
                format!("{} {:+.2}%", arrow, gain_pct),
                Style::default().fg(gain_color).bold(),
            ));

            if let Some(gain) = pos.gain {
                let sign = if gain >= dec!(0) { "+" } else { "" };
                price_spans.push(Span::styled(
                    format!("  {}{}", sign, format_money(gain)),
                    Style::default().fg(gain_color),
                ));
            }
        }
    }

    lines.push(Line::from(price_spans));

    // Line 3: Quantity + Value + Allocation (if not privacy)
    let mut detail_spans: Vec<Span> = Vec::new();

    if !privacy {
        detail_spans.push(Span::styled(
            format!("Qty: {}", format_qty(pos.quantity)),
            Style::default().fg(t.text_secondary),
        ));

        if let Some(val) = pos.current_value {
            detail_spans.push(Span::styled(
                format!("  │  Val: {}", format_money(val)),
                Style::default().fg(t.text_secondary),
            ));
        }
    }

    if let Some(alloc) = pos.allocation_pct {
        if !detail_spans.is_empty() {
            detail_spans.push(Span::styled(
                format!("  │  {:.1}%", alloc),
                Style::default().fg(t.text_accent),
            ));
        } else {
            detail_spans.push(Span::styled(
                format!("{:.1}% of portfolio", alloc),
                Style::default().fg(t.text_accent),
            ));
        }
    }

    if !detail_spans.is_empty() {
        lines.push(Line::from(detail_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Height needed for the asset header (border + content lines).
pub fn height() -> u16 {
    // 2 (border top/bottom) + 3 (symbol, price, details) = 5
    5
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 10000.0 {
        format!("{:.0}", f)
    } else if f.abs() >= 100.0 {
        format!("{:.1}", f)
    } else if f.abs() >= 1.0 {
        format!("{:.2}", f)
    } else if f.abs() >= 0.001 {
        format!("{:.4}", f)
    } else {
        format!("{:.6}", f)
    }
}

fn format_money(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    let abs = f.abs();
    if abs >= 1_000_000.0 {
        format!("{:.2}M", f / 1_000_000.0)
    } else if abs >= 10_000.0 {
        format!("{:.1}k", f / 1000.0)
    } else if abs >= 100.0 {
        format!("{:.0}", f)
    } else {
        format!("{:.2}", f)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_returns_5() {
        assert_eq!(height(), 5);
    }

    #[test]
    fn format_price_large() {
        assert_eq!(format_price(dec!(50000)), "50000");
    }

    #[test]
    fn format_price_medium() {
        assert_eq!(format_price(dec!(175.50)), "175.5");
    }

    #[test]
    fn format_price_small() {
        assert_eq!(format_price(dec!(1.50)), "1.50");
    }

    #[test]
    fn format_price_tiny() {
        assert_eq!(format_price(dec!(0.0050)), "0.0050");
    }

    #[test]
    fn format_money_millions() {
        assert_eq!(format_money(dec!(1500000)), "1.50M");
    }

    #[test]
    fn format_money_thousands() {
        assert_eq!(format_money(dec!(25000)), "25.0k");
    }

    #[test]
    fn format_money_hundreds() {
        assert_eq!(format_money(dec!(500)), "500");
    }

    #[test]
    fn format_money_small_value() {
        assert_eq!(format_money(dec!(42.50)), "42.50");
    }

    #[test]
    fn format_qty_large() {
        assert_eq!(format_qty(dec!(150000)), "150.0k");
    }

    #[test]
    fn format_qty_integer() {
        assert_eq!(format_qty(dec!(10)), "10");
    }

    #[test]
    fn format_qty_fractional() {
        assert_eq!(format_qty(dec!(0.5)), "0.50");
    }
}
