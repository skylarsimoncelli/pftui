use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::indicators::rsi::compute_rsi;
use crate::models::asset_names::resolve_name;
use crate::tui::theme;
use crate::tui::views::positions::compute_52w_range;
use crate::tui::widgets::price_chart;

#[derive(Debug, Clone)]
pub struct SearchChartPopupState {
    pub symbol: String,
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Some(state) = &app.search_chart_popup else { return };
    let t = &app.theme;
    let symbol = &state.symbol;

    let width = (area.width * 9 / 10).clamp(60, 130);
    let height = (area.height * 9 / 10).clamp(18, 56);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);
    theme::render_popup_shadow(frame, popup_area, area, t);
    frame.render_widget(Clear, popup_area);

    let title = match resolve_name(symbol).as_str() {
        "" => format!(" ◆ {} ", symbol),
        name => format!(" ◆ {} ({}) ", name, symbol),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_POPUP)
        .border_style(Style::default().fg(t.border_accent))
        .style(Style::default().bg(t.surface_2))
        .title(Span::styled(
            title,
            Style::default().fg(t.text_accent).bold(),
        ))
        .title(
            Line::from(Span::styled(
                " w:watch  a:add-tx  Esc:back ",
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        );
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height < 8 || inner.width < 30 {
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(6)])
        .split(inner);

    let history = app.price_history.get(symbol).map(|h| h.as_slice()).unwrap_or(&[]);
    let current_price = app.prices.get(symbol).copied();

    let summary = build_summary_lines(symbol, current_price, history, app);
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().bg(t.surface_2)),
        layout[0],
    );

    if history.len() < 2 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("Loading chart data for {}...", symbol),
                Style::default().fg(t.text_muted),
            ))),
            layout[1],
        );
        return;
    }

    let chart_width = layout[1].width.saturating_sub(2) as usize;
    let chart_height = layout[1].height.saturating_sub(2) as usize;
    let mut lines = price_chart::render_braille_lines(history, chart_width, chart_height, t);
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Insufficient data for chart",
            Style::default().fg(t.text_muted),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.surface_2)),
        layout[1],
    );
}

fn build_summary_lines<'a>(
    symbol: &str,
    current_price: Option<Decimal>,
    history: &[crate::models::price::HistoryRecord],
    app: &'a App,
) -> Vec<Line<'a>> {
    let t = &app.theme;
    let mut out = Vec::new();

    let price_str = current_price
        .map(|p| format!("{:.2}", p))
        .unwrap_or_else(|| "---".to_string());
    out.push(Line::from(vec![
        Span::styled("Price: ", Style::default().fg(t.text_secondary)),
        Span::styled(price_str, Style::default().fg(t.text_primary).bold()),
    ]));

    if history.len() >= 2 {
        let latest = current_price.unwrap_or_else(|| history.last().map(|h| h.close).unwrap_or(dec!(0)));
        let prev = history.get(history.len() - 2).map(|h| h.close).unwrap_or(dec!(0));
        if prev > dec!(0) {
            let pct = ((latest - prev) / prev) * dec!(100);
            let color = if pct > dec!(0) {
                t.gain_green
            } else if pct < dec!(0) {
                t.loss_red
            } else {
                t.text_muted
            };
            out.push(Line::from(vec![
                Span::styled("1D:   ", Style::default().fg(t.text_secondary)),
                Span::styled(format!("{:+.2}%", pct), Style::default().fg(color)),
            ]));
        }
    }

    if let Some(range) = compute_52w_range(history, current_price) {
        out.push(Line::from(vec![
            Span::styled("52W:  ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!("{:.0} - {:.0}", range.low, range.high),
                Style::default().fg(t.text_primary),
            ),
        ]));
    }

    if history.len() >= 15 {
        let closes: Vec<f64> = history
            .iter()
            .map(|h| h.close.to_string().parse::<f64>().unwrap_or(0.0))
            .collect();
        let rsi = compute_rsi(&closes, 14);
        if let Some(val) = rsi.last().and_then(|v| *v) {
            out.push(Line::from(vec![
                Span::styled("RSI14:", Style::default().fg(t.text_secondary)),
                Span::styled(format!(" {:.1}", val), Style::default().fg(t.text_primary)),
                Span::styled(format!("  {}", symbol), Style::default().fg(t.text_muted)),
            ]));
        }
    }

    if let Some(vol) = history.last().and_then(|h| h.volume) {
        out.push(Line::from(vec![
            Span::styled("Vol:  ", Style::default().fg(t.text_secondary)),
            Span::styled(format!("{vol}"), Style::default().fg(t.text_primary)),
        ]));
    }

    out
}
