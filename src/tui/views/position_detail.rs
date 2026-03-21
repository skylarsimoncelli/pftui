use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::config::PortfolioMode;
use crate::models::position::Position;
use crate::models::transaction::TxType;
use crate::tui::theme;
use crate::tui::views::positions::compute_52w_range;

/// Renders a full-screen popup with detailed info about the selected position.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let pos = match app.selected_position() {
        Some(p) => p.clone(),
        None => return,
    };

    let t = &app.theme;
    let privacy = is_privacy_view(app);

    let lines = build_detail_lines(&pos, app, privacy);
    let total_lines = lines.len();

    // Popup sizing — use most of the screen
    let width = 64u16.min(area.width.saturating_sub(4));
    let height = ((total_lines as u16) + 2).min(area.height.saturating_sub(2));

    // Center the popup
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    // Draw shadow behind popup (before Clear so shadow is visible around edges)
    crate::tui::theme::render_popup_shadow(frame, popup_area, area, t);

    frame.render_widget(Clear, popup_area);

    let visible_lines = height.saturating_sub(2) as usize;
    let displayed: Vec<Line> = lines.into_iter().take(visible_lines).collect();

    let title = format!(" ◆ {} ({}) ", pos.name_or_symbol(), pos.symbol);

    let detail = Paragraph::new(displayed).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(crate::tui::theme::BORDER_POPUP)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                title,
                Style::default().fg(t.text_accent).bold(),
            ))
            .title(
                Line::from(Span::styled(
                    " Esc to close ",
                    Style::default().fg(t.text_muted),
                ))
                .alignment(Alignment::Right),
            ),
    );

    frame.render_widget(detail, popup_area);
}

pub fn build_detail_lines<'a>(pos: &Position, app: &'a App, privacy: bool) -> Vec<Line<'a>> {
    let t = &app.theme;
    let cat_color = t.category_color(pos.category);

    let mut lines: Vec<Line> = Vec::with_capacity(32);
    lines.push(Line::from(""));

    // ── Asset Info ──
    let name_display = if pos.name.is_empty() {
        pos.symbol.clone()
    } else {
        pos.name.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("  Symbol    ", Style::default().fg(t.text_secondary)),
        Span::styled(
            pos.symbol.clone(),
            Style::default().fg(t.text_primary).bold(),
        ),
    ]));
    if !pos.name.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Name      ", Style::default().fg(t.text_secondary)),
            Span::styled(name_display, Style::default().fg(t.text_primary)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  Category  ", Style::default().fg(t.text_secondary)),
        Span::styled(
            format!("{}", pos.category),
            Style::default().fg(cat_color).bold(),
        ),
    ]));
    lines.push(Line::from(""));

    // ── Price ──
    lines.push(section_header("  Price", t.text_accent));
    lines.push(sep_line(t.border_subtle, 58));

    let price_str = pos
        .current_price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string());
    lines.push(Line::from(vec![
        Span::styled("  Current   ", Style::default().fg(t.text_secondary)),
        Span::styled(
            format!("{} {}", price_str, pos.currency),
            Style::default().fg(t.text_primary).bold(),
        ),
    ]));

    if !privacy {
        // Quantity
        lines.push(Line::from(vec![
            Span::styled("  Quantity  ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format_qty(pos.quantity),
                Style::default().fg(t.text_primary),
            ),
        ]));

        // Avg cost
        if pos.avg_cost > dec!(0) {
            lines.push(Line::from(vec![
                Span::styled("  Avg Cost  ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!("{} {}", format_price(pos.avg_cost), pos.currency),
                    Style::default().fg(t.text_primary),
                ),
            ]));
        }

        // Total cost
        if pos.total_cost > dec!(0) {
            lines.push(Line::from(vec![
                Span::styled("  Cost Basis", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!(" {} {}", format_money(pos.total_cost), pos.currency),
                    Style::default().fg(t.text_primary),
                ),
            ]));
        }

        // Current value
        if let Some(val) = pos.current_value {
            lines.push(Line::from(vec![
                Span::styled("  Value     ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!("{} {}", format_money(val), pos.currency),
                    Style::default().fg(t.text_primary).bold(),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));

    // ── Performance ──
    lines.push(section_header("  Performance", t.text_accent));
    lines.push(sep_line(t.border_subtle, 58));

    if !privacy {
        if let Some(gain) = pos.gain {
            let gain_f: f64 = gain.to_string().parse().unwrap_or(0.0);
            let gain_color = theme::gain_intensity_color(t, gain_f);
            let sign = if gain >= dec!(0) { "+" } else { "" };
            lines.push(Line::from(vec![
                Span::styled("  Gain      ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!("{}{} {}", sign, format_money(gain), pos.currency),
                    Style::default().fg(gain_color).bold(),
                ),
            ]));
        }

        if let Some(gain_pct) = pos.gain_pct {
            let gain_f: f64 = gain_pct.to_string().parse().unwrap_or(0.0);
            let gain_color = theme::gain_intensity_color(t, gain_f);
            lines.push(Line::from(vec![
                Span::styled("  Gain %    ", Style::default().fg(t.text_secondary)),
                Span::styled(
                    format!("{:+.2}%", gain_pct),
                    Style::default().fg(gain_color).bold(),
                ),
            ]));
        }
    }

    if let Some(alloc) = pos.allocation_pct {
        lines.push(Line::from(vec![
            Span::styled("  Allocation", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!(" {:.1}%", alloc),
                Style::default().fg(t.text_primary),
            ),
        ]));
    }

    // 52-week range
    if let Some(range) = compute_52w_range(
        app.price_history
            .get(&pos.symbol)
            .map(|v| v.as_slice())
            .unwrap_or(&[]),
        pos.current_price,
    ) {
        let high_str = format_price(range.high);
        let low_str = format_price(range.low);
        lines.push(Line::from(vec![
            Span::styled("  52W Range ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!(" {} — {}", low_str, high_str),
                Style::default().fg(t.text_primary),
            ),
        ]));
        let pct_text = if range.from_high_pct.abs() < 0.05 {
            "At 52W high".to_string()
        } else {
            format!("{:+.1}% from high", range.from_high_pct)
        };
        let pct_color = if range.from_high_pct.abs() < 0.05 {
            t.gain_green
        } else if range.from_high_pct > -10.0 {
            t.text_secondary
        } else {
            t.loss_red
        };
        lines.push(Line::from(vec![
            Span::styled("            ", Style::default().fg(t.text_secondary)),
            Span::styled(format!(" {}", pct_text), Style::default().fg(pct_color)),
        ]));
    }

    lines.push(Line::from(""));

    // ── Transaction History ──
    if !privacy && app.portfolio_mode == PortfolioMode::Full {
        let symbol_txs: Vec<_> = app
            .transactions
            .iter()
            .filter(|tx| tx.symbol == pos.symbol)
            .collect();

        if !symbol_txs.is_empty() {
            lines.push(section_header("  Transactions", t.text_accent));
            lines.push(sep_line(t.border_subtle, 58));

            // Header row
            lines.push(Line::from(vec![
                Span::styled("  Date        ", Style::default().fg(t.text_muted)),
                Span::styled("Type  ", Style::default().fg(t.text_muted)),
                Span::styled("Qty       ", Style::default().fg(t.text_muted)),
                Span::styled("Price", Style::default().fg(t.text_muted)),
            ]));

            // Show up to 10 most recent transactions (newest first)
            let mut recent: Vec<_> = symbol_txs;
            recent.sort_by(|a, b| b.date.cmp(&a.date));
            let showing = recent.len().min(10);
            for tx in recent.iter().take(showing) {
                let type_color = match tx.tx_type {
                    TxType::Buy => t.gain_green,
                    TxType::Sell => t.loss_red,
                };
                let type_str = match tx.tx_type {
                    TxType::Buy => "BUY ",
                    TxType::Sell => "SELL",
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}  ", tx.date),
                        Style::default().fg(t.text_secondary),
                    ),
                    Span::styled(
                        format!("{}  ", type_str),
                        Style::default().fg(type_color).bold(),
                    ),
                    Span::styled(
                        format!("{:<10}", format_qty(tx.quantity)),
                        Style::default().fg(t.text_primary),
                    ),
                    Span::styled(
                        format!("@ {}", format_price(tx.price_per)),
                        Style::default().fg(t.text_secondary),
                    ),
                ]));
            }
            if recent.len() > 10 {
                lines.push(Line::from(Span::styled(
                    format!("  ... and {} more", recent.len() - 10),
                    Style::default().fg(t.text_muted),
                )));
            }
            lines.push(Line::from(""));
        }
    }

    // ── Footer ──
    lines.push(Line::from(Span::styled(
        "  Esc to close · Enter to toggle chart",
        Style::default().fg(t.text_muted),
    )));
    lines.push(Line::from(""));

    lines
}

fn section_header(title: &str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default().bold().fg(color),
    ))
}

fn sep_line(color: Color, width: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}", "─".repeat(width.saturating_sub(2))),
        Style::default().fg(color),
    ))
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 10000.0 {
        format!("{:.0}", f)
    } else if f.abs() >= 100.0 {
        format!("{:.1}", f)
    } else if f.abs() >= 1.0 {
        format!("{:.2}", f)
    } else {
        format!("{:.4}", f)
    }
}

pub fn format_money(v: Decimal) -> String {
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

/// Helper trait extension for Position
trait PositionExt {
    fn name_or_symbol(&self) -> &str;
}

impl PositionExt for Position {
    fn name_or_symbol(&self) -> &str {
        if self.name.is_empty() {
            &self.symbol
        } else {
            &self.name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::asset::AssetCategory;

    fn test_app() -> App {
        let config = Config::default();
        let db_path = std::path::PathBuf::from(":memory:");
        App::new(&config, db_path)
    }

    fn make_position(symbol: &str, name: &str, category: AssetCategory) -> Position {
        Position {
            symbol: symbol.to_string(),
            name: name.to_string(),
            category,
            quantity: dec!(10),
            avg_cost: dec!(150),
            total_cost: dec!(1500),
            currency: "USD".to_string(),
            current_price: Some(dec!(175)),
            current_value: Some(dec!(1750)),
            gain: Some(dec!(250)),
            gain_pct: Some(dec!(16.67)),
            allocation_pct: Some(dec!(25)),
            native_currency: None,
            fx_rate: None,
        }
    }

    #[test]
    fn detail_lines_contain_symbol() {
        let mut app = test_app();
        let pos = make_position("AAPL", "Apple Inc", AssetCategory::Equity);
        app.display_positions = vec![pos.clone()];
        app.selected_index = 0;

        let lines = build_detail_lines(&pos, &app, false);
        let text = lines_to_string(&lines);
        assert!(text.contains("AAPL"), "should contain symbol");
        assert!(text.contains("Apple Inc"), "should contain name");
    }

    #[test]
    fn detail_lines_contain_price_info() {
        let mut app = test_app();
        let pos = make_position("AAPL", "Apple Inc", AssetCategory::Equity);
        app.display_positions = vec![pos.clone()];

        let lines = build_detail_lines(&pos, &app, false);
        let text = lines_to_string(&lines);
        assert!(text.contains("175"), "should contain current price");
        assert!(text.contains("150"), "should contain avg cost");
        assert!(text.contains("Quantity"), "should contain quantity label");
    }

    #[test]
    fn detail_lines_contain_gain_info() {
        let mut app = test_app();
        let pos = make_position("AAPL", "Apple Inc", AssetCategory::Equity);
        app.display_positions = vec![pos.clone()];

        let lines = build_detail_lines(&pos, &app, false);
        let text = lines_to_string(&lines);
        assert!(text.contains("Gain"), "should contain gain section");
        assert!(text.contains("16.67"), "should contain gain percentage");
    }

    #[test]
    fn detail_lines_privacy_hides_values() {
        let mut app = test_app();
        let pos = make_position("AAPL", "Apple Inc", AssetCategory::Equity);
        app.display_positions = vec![pos.clone()];

        let lines = build_detail_lines(&pos, &app, true);
        let text = lines_to_string(&lines);
        // Should NOT contain quantity, gain, cost values
        assert!(!text.contains("Quantity"), "privacy should hide quantity");
        assert!(!text.contains("Avg Cost"), "privacy should hide avg cost");
        assert!(
            !text.contains("Cost Basis"),
            "privacy should hide cost basis"
        );
        // Should still contain price and allocation
        assert!(text.contains("175"), "privacy should still show price");
        assert!(
            text.contains("25.0%"),
            "privacy should still show allocation"
        );
    }

    #[test]
    fn detail_lines_contain_category() {
        let mut app = test_app();
        let pos = make_position("BTC", "Bitcoin", AssetCategory::Crypto);
        app.display_positions = vec![pos.clone()];

        let lines = build_detail_lines(&pos, &app, false);
        let text = lines_to_string(&lines);
        assert!(text.contains("crypto"), "should contain category");
    }

    #[test]
    fn detail_lines_show_transactions() {
        let mut app = test_app();
        let pos = make_position("AAPL", "Apple Inc", AssetCategory::Equity);
        app.display_positions = vec![pos.clone()];

        // Add some transactions
        app.transactions = vec![
            crate::models::transaction::Transaction {
                id: 1,
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(5),
                price_per: dec!(140),
                currency: "USD".to_string(),
                date: "2025-01-15".to_string(),
                notes: None,
                created_at: "2025-01-15".to_string(),
            },
            crate::models::transaction::Transaction {
                id: 2,
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: TxType::Buy,
                quantity: dec!(5),
                price_per: dec!(160),
                currency: "USD".to_string(),
                date: "2025-06-01".to_string(),
                notes: None,
                created_at: "2025-06-01".to_string(),
            },
        ];

        let lines = build_detail_lines(&pos, &app, false);
        let text = lines_to_string(&lines);
        assert!(
            text.contains("Transactions"),
            "should contain transactions section"
        );
        assert!(text.contains("BUY"), "should show buy type");
        assert!(text.contains("2025-01-15"), "should show transaction date");
        assert!(
            text.contains("2025-06-01"),
            "should show second transaction date"
        );
    }

    #[test]
    fn detail_lines_privacy_hides_transactions() {
        let mut app = test_app();
        let pos = make_position("AAPL", "Apple Inc", AssetCategory::Equity);
        app.display_positions = vec![pos.clone()];
        app.transactions = vec![crate::models::transaction::Transaction {
            id: 1,
            symbol: "AAPL".to_string(),
            category: AssetCategory::Equity,
            tx_type: TxType::Buy,
            quantity: dec!(5),
            price_per: dec!(140),
            currency: "USD".to_string(),
            date: "2025-01-15".to_string(),
            notes: None,
            created_at: "2025-01-15".to_string(),
        }];

        let lines = build_detail_lines(&pos, &app, true);
        let text = lines_to_string(&lines);
        assert!(
            !text.contains("Transactions"),
            "privacy should hide transactions"
        );
    }

    #[test]
    fn format_money_large() {
        assert_eq!(format_money(dec!(1500000)), "1.50M");
    }

    #[test]
    fn format_money_medium() {
        assert_eq!(format_money(dec!(25000)), "25.0k");
    }

    #[test]
    fn format_money_small() {
        assert_eq!(format_money(dec!(99.50)), "99.50");
    }

    fn lines_to_string(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
