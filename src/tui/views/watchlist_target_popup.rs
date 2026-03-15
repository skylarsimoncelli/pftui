use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;
use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Some(state) = &app.watchlist_target_popup else {
        return;
    };
    let t = &app.theme;
    let width = (area.width * 3 / 5).clamp(44, 72);
    let height = 10;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);
    theme::render_popup_shadow(frame, popup, area, t);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_POPUP)
        .border_style(Style::default().fg(t.border_accent))
        .style(Style::default().bg(t.surface_2))
        .title(Span::styled(
            format!(" Set Watch Target · {} ", state.symbol),
            Style::default().fg(t.text_accent).bold(),
        ))
        .title(
            Line::from(Span::styled(
                "Tab toggle dir  Enter save  c clear  Esc cancel",
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        );
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let direction = match state.direction {
        crate::app::WatchlistTargetDirection::Above => "above",
        crate::app::WatchlistTargetDirection::Below => "below",
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Direction: ", Style::default().fg(t.text_secondary)),
            Span::styled(direction, Style::default().fg(t.text_primary).bold()),
        ]),
        Line::from(vec![
            Span::styled("Target:    ", Style::default().fg(t.text_secondary)),
            Span::styled(&state.price_input, Style::default().fg(t.text_primary)),
            Span::styled("█", Style::default().fg(t.text_accent)),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            state.message.clone().unwrap_or_else(|| " ".to_string()),
            Style::default().fg(t.loss_red),
        )),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}
