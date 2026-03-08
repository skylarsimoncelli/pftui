use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::tui::theme;

const GRID_MAX_CARDS: usize = 9;
const MINI_CHARS: [char; 8] = ['⣀', '⣄', '⣆', '⣇', '⣧', '⣷', '⣾', '⣿'];

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let t = &app.theme;
    let symbols: Vec<String> = app
        .display_positions
        .iter()
        .filter(|p| p.category != crate::models::asset::AssetCategory::Cash)
        .take(GRID_MAX_CARDS)
        .map(|p| p.symbol.clone())
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(crate::tui::theme::BORDER_ACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(" Chart Grid ", Style::default().fg(t.text_accent).bold()))
        .title(
            Line::from(Span::styled(
                "Mini trend cards for held positions",
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        )
        .style(Style::default().bg(t.surface_0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if symbols.is_empty() {
        let msg = Paragraph::new("No positions to chart.")
            .style(Style::default().fg(t.text_muted).bg(t.surface_0));
        frame.render_widget(msg, inner);
        return;
    }

    let cols = if inner.width >= 120 { 3 } else { 2 };
    let rows = ((symbols.len() as f32) / cols as f32).ceil() as usize;
    let mut v_constraints = Vec::new();
    for _ in 0..rows {
        v_constraints.push(Constraint::Ratio(1, rows as u32));
    }
    let row_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(v_constraints)
        .split(inner);

    let mut idx = 0usize;
    for row_area in row_chunks.iter().copied() {
        let mut h_constraints = Vec::new();
        for _ in 0..cols {
            h_constraints.push(Constraint::Ratio(1, cols as u32));
        }
        let col_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(h_constraints)
            .split(row_area);

        for card in col_chunks.iter().copied() {
            if idx >= symbols.len() {
                break;
            }
            render_symbol_card(frame, card, app, &symbols[idx]);
            idx += 1;
        }
    }
}

fn render_symbol_card(frame: &mut Frame, area: Rect, app: &App, symbol: &str) {
    let t = &app.theme;
    let history = app.price_history.get(symbol);
    let spark = history
        .map(|h| mini_sparkline(h, 24, t))
        .unwrap_or_else(|| "---".to_string());
    let change = history.and_then(|h| day_change_pct(h)).unwrap_or(0.0);
    let change_color = if change > 0.0 {
        t.gain_green
    } else if change < 0.0 {
        t.loss_red
    } else {
        t.text_muted
    };
    let price = app
        .prices
        .get(symbol)
        .map(|p| format!("${:.2}", p))
        .unwrap_or_else(|| "-".to_string());

    let card = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_subtle))
        .style(Style::default().bg(t.surface_1));
    let inner = card.inner(area);
    frame.render_widget(card, area);

    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{:<10}", symbol), Style::default().fg(t.text_primary).bold()),
            Span::styled(format!("{:>10}", price), Style::default().fg(t.text_secondary)),
        ]),
        Line::from(Span::styled(spark, Style::default().fg(t.text_accent))),
        Line::from(Span::styled(format!("{:+.2}% 1D", change), Style::default().fg(change_color))),
    ];
    let para = Paragraph::new(lines).style(Style::default().bg(t.surface_1));
    frame.render_widget(para, inner);
}

fn mini_sparkline(records: &[crate::models::price::HistoryRecord], width: usize, t: &theme::Theme) -> String {
    if records.len() < 2 || width == 0 {
        return "---".to_string();
    }
    let closes: Vec<f64> = records
        .iter()
        .rev()
        .take(width)
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if closes.is_empty() {
        return "---".to_string();
    }
    let min = closes.iter().fold(f64::INFINITY, |a, b| a.min(*b));
    let max = closes.iter().fold(f64::NEG_INFINITY, |a, b| a.max(*b));
    let range = max - min;
    let mut out = String::new();
    for v in closes {
        let idx = if range > 0.0 {
            (((v - min) / range) * 7.0).round() as usize
        } else {
            3
        };
        out.push(MINI_CHARS[idx.min(7)]);
    }
    // keep a tiny marker tone by appending a neutral spacer char using theme in caller
    let _ = t;
    out
}

fn day_change_pct(records: &[crate::models::price::HistoryRecord]) -> Option<f64> {
    if records.len() < 2 {
        return None;
    }
    let latest = records.last()?.close.to_string().parse::<f64>().ok()?;
    let prev = records.get(records.len().saturating_sub(2))?.close.to_string().parse::<f64>().ok()?;
    if prev == 0.0 {
        return None;
    }
    Some(((latest - prev) / prev) * 100.0)
}
