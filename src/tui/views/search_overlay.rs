use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use rust_decimal::Decimal;

use crate::app::App;
use crate::models::asset_names::{infer_category, search_names};
use crate::tui::theme;

/// A single search result row.
pub struct SearchResult {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub in_portfolio: bool,
    pub in_watchlist: bool,
    pub price: Option<Decimal>,
    pub day_change_pct: Option<Decimal>,
    pub week_52_low: Option<Decimal>,
    pub week_52_high: Option<Decimal>,
}

/// Build search results from the query, enriched with portfolio/price data.
pub fn build_results(app: &App, query: &str) -> Vec<SearchResult> {
    if query.trim().is_empty() {
        return Vec::new();
    }

    let matches = search_names(query);

    matches
        .into_iter()
        .take(20) // Cap at 20 results for performance
        .map(|(symbol, name)| {
            let category = infer_category(symbol);
            let category_str = {
                let s = format!("{category}");
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                }
            };
            let in_portfolio = app.positions.iter().any(|p| p.symbol == symbol);
            let in_watchlist = app
                .watchlist_entries
                .iter()
                .any(|w| w.symbol == symbol);
            let price = app.prices.get(symbol).copied();

            // Compute day change % from price history if available
            let day_change_pct = app.price_history.get(symbol).and_then(|history| {
                if history.len() >= 2 {
                    let latest = price.or_else(|| history.last().map(|h| h.close))?;
                    let prev = history.get(history.len() - 2).map(|h| h.close)?;
                    if prev > Decimal::ZERO {
                        Some(((latest - prev) / prev) * Decimal::from(100))
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            let (week_52_low, week_52_high) = app
                .price_history
                .get(symbol)
                .and_then(|history| {
                    if history.is_empty() {
                        return None;
                    }
                    let window = if history.len() > 365 {
                        &history[history.len() - 365..]
                    } else {
                        history.as_slice()
                    };
                    let low = window.iter().map(|h| h.close).min()?;
                    let high = window.iter().map(|h| h.close).max()?;
                    Some((Some(low), Some(high)))
                })
                .unwrap_or((None, None));

            SearchResult {
                symbol: symbol.to_string(),
                name: name.to_string(),
                category: category_str,
                in_portfolio,
                in_watchlist,
                price,
                day_change_pct,
                week_52_low,
                week_52_high,
            }
        })
        .collect()
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Full-screen overlay (80% width, 80% height, centered)
    let width = (area.width * 4 / 5).clamp(40, 80);
    let height = (area.height * 4 / 5).clamp(10, 40);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    // Shadow
    theme::render_popup_shadow(frame, popup_area, area, t);

    // Clear background
    frame.render_widget(Clear, popup_area);

    // Inner area (inside border)
    let inner = Rect::new(
        popup_area.x + 1,
        popup_area.y + 1,
        popup_area.width.saturating_sub(2),
        popup_area.height.saturating_sub(2),
    );

    // Draw border block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_POPUP)
        .border_style(Style::default().fg(t.border_accent))
        .style(Style::default().bg(t.surface_2))
        .title(Span::styled(
            " ◆ Search Assets ",
            Style::default().fg(t.text_accent).bold(),
        ));
    frame.render_widget(block, popup_area);

    if inner.height < 3 || inner.width < 10 {
        return;
    }

    // Layout: search input (1 line) + separator (1 line) + results list
    let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let results_height = inner.height.saturating_sub(2);
    let results_area = Rect::new(inner.x, inner.y + 2, inner.width, results_height);

    // Render search input
    let cursor_char = if app.tick_count % 30 < 15 { "▏" } else { " " };
    let input_line = Line::from(vec![
        Span::styled("  / ", Style::default().fg(t.text_accent).bold()),
        Span::styled(
            app.search_overlay_query.clone(),
            Style::default().fg(t.text_primary),
        ),
        Span::styled(cursor_char, Style::default().fg(t.text_accent)),
    ]);
    frame.render_widget(
        Paragraph::new(input_line).style(Style::default().bg(t.surface_2)),
        input_area,
    );

    // Separator
    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(t.border_subtle),
    ));
    frame.render_widget(
        Paragraph::new(sep).style(Style::default().bg(t.surface_2)),
        sep_area,
    );

    // Results
    let results = build_results(app, &app.search_overlay_query);
    let result_count = results.len();

    if app.search_overlay_query.trim().is_empty() {
        // Show hint when empty
        let hint = Line::from(Span::styled(
            "  Type to search all assets…",
            Style::default().fg(t.text_muted).italic(),
        ));
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().bg(t.surface_2)),
            results_area,
        );
        return;
    }

    if results.is_empty() {
        let no_match = Line::from(Span::styled(
            "  No matching assets",
            Style::default().fg(t.text_muted),
        ));
        frame.render_widget(
            Paragraph::new(no_match).style(Style::default().bg(t.surface_2)),
            results_area,
        );
        return;
    }

    // Compute scroll offset so selected item is visible
    let visible = results_height as usize;
    let selected = app.search_overlay_selected.min(result_count.saturating_sub(1));
    let scroll_offset = if selected >= visible {
        selected - visible + 1
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::with_capacity(visible);

    for (i, result) in results.iter().enumerate().skip(scroll_offset).take(visible) {
        let is_selected = i == selected;

        // Status indicators
        let status = if result.in_portfolio {
            "◆"
        } else if result.in_watchlist {
            "○"
        } else {
            " "
        };

        let status_color = if result.in_portfolio {
            t.gain_green
        } else if result.in_watchlist {
            t.text_accent
        } else {
            t.text_muted
        };

        // Price display
        let price_str = match result.price {
            Some(p) => format!("{p:.2}"),
            None => "—".to_string(),
        };

        // Day change display
        let (change_str, change_color) = match result.day_change_pct {
            Some(pct) if pct > Decimal::ZERO => {
                (format!("+{pct:.1}%"), t.gain_green)
            }
            Some(pct) if pct < Decimal::ZERO => {
                (format!("{pct:.1}%"), t.loss_red)
            }
            Some(_) => ("0.0%".to_string(), t.text_muted),
            None => ("".to_string(), t.text_muted),
        };

        // Compute column widths within available space
        // Layout: " ◆ SYMBOL  Name   Category  Price  Change  52W range"
        let sym_width = 10;
        let cat_width = 8;
        let price_width = 10;
        let change_width = 8;
        let range_width = 18;
        let name_width = (inner.width as usize)
            .saturating_sub(2 + sym_width + cat_width + price_width + change_width + range_width + 2);

        let row_bg = if is_selected { t.surface_3 } else { t.surface_2 };

        let truncated_name = if result.name.len() > name_width {
            format!("{}…", &result.name[..name_width.saturating_sub(1)])
        } else {
            result.name.clone()
        };
        let range_str = match (result.week_52_low, result.week_52_high) {
            (Some(lo), Some(hi)) => format!("{:.0}-{:.0}", lo, hi),
            _ => "—".to_string(),
        };

        let line = Line::from(vec![
            Span::styled(
                format!(" {status} "),
                Style::default().fg(status_color).bg(row_bg),
            ),
            Span::styled(
                format!("{:<width$}", result.symbol, width = sym_width),
                Style::default().fg(t.text_accent).bg(row_bg).bold(),
            ),
            Span::styled(
                format!("{:<width$}", truncated_name, width = name_width),
                Style::default().fg(t.text_primary).bg(row_bg),
            ),
            Span::styled(
                format!("{:<width$}", result.category, width = cat_width),
                Style::default().fg(t.text_secondary).bg(row_bg),
            ),
            Span::styled(
                format!("{:>width$}", price_str, width = price_width),
                Style::default().fg(t.text_primary).bg(row_bg),
            ),
            Span::styled(
                format!("{:>width$} ", change_str, width = change_width),
                Style::default().fg(change_color).bg(row_bg),
            ),
            Span::styled(
                format!("{:>width$}", range_str, width = range_width),
                Style::default().fg(t.text_secondary).bg(row_bg),
            ),
        ]);

        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.surface_2)),
        results_area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_app() -> App {
        let config = Config::default();
        let db_path = std::path::PathBuf::from(":memory:");
        App::new(&config, db_path)
    }

    #[test]
    fn empty_query_returns_no_results() {
        let app = test_app();
        let results = build_results(&app, "");
        assert!(results.is_empty());
    }

    #[test]
    fn whitespace_query_returns_no_results() {
        let app = test_app();
        let results = build_results(&app, "   ");
        assert!(results.is_empty());
    }

    #[test]
    fn known_symbol_returns_results() {
        let app = test_app();
        let results = build_results(&app, "BTC");
        assert!(!results.is_empty());
        assert_eq!(results[0].symbol, "BTC");
    }

    #[test]
    fn results_capped_at_20() {
        let app = test_app();
        // "A" matches many symbols
        let results = build_results(&app, "A");
        assert!(results.len() <= 20);
    }

    #[test]
    fn result_has_correct_category() {
        let app = test_app();
        let results = build_results(&app, "GC=F");
        assert!(!results.is_empty());
        assert_eq!(results[0].category, "Commodity");
    }

    #[test]
    fn result_detects_portfolio_membership() {
        let mut app = test_app();
        // Add a fake position for BTC
        app.positions.push(crate::models::position::Position {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            category: crate::models::asset::AssetCategory::Crypto,
            quantity: rust_decimal_macros::dec!(1),
            avg_cost: rust_decimal_macros::dec!(50000),
            total_cost: rust_decimal_macros::dec!(50000),
            currency: "USD".to_string(),
            current_price: None,
            current_value: None,
            gain: None,
            gain_pct: None,
            allocation_pct: None,
        });
        let results = build_results(&app, "BTC");
        assert!(!results.is_empty());
        assert!(results[0].in_portfolio);
    }

    #[test]
    fn result_without_position_not_in_portfolio() {
        let app = test_app();
        let results = build_results(&app, "AAPL");
        assert!(!results.is_empty());
        assert!(!results[0].in_portfolio);
    }

    #[test]
    fn name_search_works() {
        let app = test_app();
        let results = build_results(&app, "Bitcoin");
        assert!(!results.is_empty());
        assert_eq!(results[0].symbol, "BTC");
    }

    #[test]
    fn case_insensitive_search() {
        let app = test_app();
        let results_upper = build_results(&app, "BTC");
        let results_lower = build_results(&app, "btc");
        assert_eq!(results_upper.len(), results_lower.len());
        if !results_upper.is_empty() {
            assert_eq!(results_upper[0].symbol, results_lower[0].symbol);
        }
    }

    #[test]
    fn no_match_returns_empty() {
        let app = test_app();
        let results = build_results(&app, "ZZZZZZZZZ");
        assert!(results.is_empty());
    }
}
