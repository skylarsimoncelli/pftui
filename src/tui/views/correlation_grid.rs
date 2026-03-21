use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::app::App;
use crate::indicators::correlation::compute_rolling_correlation;
use crate::tui::theme;
use crate::tui::views::markets;

/// Keep the matrix compact so it remains legible in terminal sizes.
const MAX_SYMBOLS: usize = 8;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let window = app.markets_correlation_window;
    let symbols = build_symbol_set();

    let mut header_cells = vec![Cell::from(Span::styled(
        format!("Corr {}", window.label()),
        Style::default().fg(t.text_secondary).bold(),
    ))];
    header_cells.extend(symbols.iter().map(|(short, _)| {
        Cell::from(Span::styled(
            short.clone(),
            Style::default().fg(t.text_secondary).bold(),
        ))
    }));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = symbols
        .iter()
        .map(|(row_label, row_symbol)| {
            let mut cells = vec![Cell::from(Span::styled(
                row_label.clone(),
                Style::default().fg(t.text_primary).bold(),
            ))];

            for (_, col_symbol) in &symbols {
                if row_symbol == col_symbol {
                    cells.push(Cell::from(Span::styled(
                        "1.00",
                        Style::default().fg(t.text_primary).bold(),
                    )));
                    continue;
                }

                match latest_correlation(app, row_symbol, col_symbol, window.days()) {
                    Some(corr) => {
                        let color = theme::gain_intensity_color(t, corr * 100.0);
                        let bg = correlation_bg(t, corr);
                        cells.push(Cell::from(Span::styled(
                            format!("{:+.2}", corr),
                            Style::default().fg(color).bg(bg),
                        )));
                    }
                    None => cells.push(Cell::from(Span::styled(
                        " ---",
                        Style::default().fg(t.text_muted),
                    ))),
                }
            }

            Row::new(cells)
                .height(1)
                .style(Style::default().bg(t.surface_1))
        })
        .collect();

    let mut widths = vec![Constraint::Length(8)];
    widths.extend((0..symbols.len()).map(|_| Constraint::Length(6)));

    let title = format!(" Correlation Grid (M: 7d/30d/90d, now {}) ", window.label());
    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .title(Span::styled(
                    title,
                    Style::default().fg(t.text_accent).bold(),
                ))
                .style(Style::default().bg(t.surface_0)),
        );

    frame.render_widget(table, area);
}

fn build_symbol_set() -> Vec<(String, String)> {
    let preferred = [
        ("SPX", "^GSPC"),
        ("NDX", "^NDX"),
        ("VIX", "^VIX"),
        ("Gold", "GC=F"),
        ("Oil", "CL=F"),
        ("BTC", "BTC-USD"),
        ("ETH", "ETH-USD"),
        ("DXY", "DX-Y.NYB"),
    ];

    let mut symbols: Vec<(String, String)> = preferred
        .into_iter()
        .map(|(s, y)| (s.to_string(), y.to_string()))
        .collect();
    if symbols.len() < MAX_SYMBOLS {
        for item in markets::market_symbols() {
            if symbols.iter().any(|(_, y)| *y == item.yahoo_symbol) {
                continue;
            }
            symbols.push((item.symbol, item.yahoo_symbol));
            if symbols.len() >= MAX_SYMBOLS {
                break;
            }
        }
    }
    symbols.truncate(MAX_SYMBOLS);
    symbols
}

fn latest_correlation(
    app: &App,
    symbol_a: &str,
    symbol_b: &str,
    window_days: usize,
) -> Option<f64> {
    let history_a = app.price_history.get(symbol_a)?;
    let history_b = app.price_history.get(symbol_b)?;
    let min_len = history_a.len().min(history_b.len());
    if min_len < window_days + 1 {
        return None;
    }

    let prices_a: Vec<f64> = history_a[history_a.len() - min_len..]
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();
    let prices_b: Vec<f64> = history_b[history_b.len() - min_len..]
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let rolling = compute_rolling_correlation(&prices_a, &prices_b, window_days);
    rolling.into_iter().rev().flatten().next()
}

fn correlation_bg(t: &theme::Theme, corr: f64) -> Color {
    if corr.abs() < 0.05 {
        return t.surface_1;
    }
    let intensity = (corr.abs() as f32).min(1.0) * 0.22;
    let target = if corr >= 0.0 {
        t.gain_green
    } else {
        t.loss_red
    };
    theme::lerp_color(t.surface_1, target, intensity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_days_match_labels() {
        assert_eq!(crate::app::MarketCorrelationWindow::SevenDay.days(), 7);
        assert_eq!(crate::app::MarketCorrelationWindow::ThirtyDay.days(), 30);
        assert_eq!(crate::app::MarketCorrelationWindow::NinetyDay.days(), 90);
    }

    #[test]
    fn symbol_set_is_compact() {
        let symbols = build_symbol_set();
        assert!(!symbols.is_empty());
        assert!(symbols.len() <= MAX_SYMBOLS);
    }
}
