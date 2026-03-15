use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::App;

const STEPS: [(&str, &str); 5] = [
    (
        "Welcome to pftui",
        "This tour highlights the fastest workflow: refresh, brief, scan, and monitor drift.",
    ),
    (
        "Core Views",
        "1 Portfolio, 5 Watchlist, 8 Chart Grid, 3 Markets, 4 Economy. Press ? for full keybindings.",
    ),
    (
        "Command Palette",
        "Press : for command mode. Useful commands: view chartgrid, refresh, scan, layout analyst.",
    ),
    (
        "Daily Routine",
        "Run `pftui refresh` before analysis. Use `pftui brief`, `pftui movers`, and `pftui drift` for checkpoints.",
    ),
    (
        "You Are Ready",
        "Use Enter/Right for next, Left for previous, Esc to finish. Press Shift+O anytime to reopen this tour.",
    ),
];

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let width = 84u16.min(area.width.saturating_sub(4)).max(56);
    let height = 16u16.min(area.height.saturating_sub(2)).max(12);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    crate::tui::theme::render_popup_shadow(frame, popup_area, area, t);
    frame.render_widget(Clear, popup_area);

    let idx = app.onboarding_step.min(STEPS.len().saturating_sub(1));
    let (title, body) = STEPS[idx];
    let progress = format!(" Step {}/{} ", idx + 1, STEPS.len());
    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(
            title,
            Style::default().fg(t.text_accent).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(body, Style::default().fg(t.text_primary))),
        Line::from(""),
        Line::from(Span::styled(
            "←/Backspace previous  Enter/Right next  Esc finish",
            Style::default().fg(t.text_muted),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(crate::tui::theme::BORDER_POPUP)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                " Onboarding Tour ",
                Style::default().fg(t.text_accent).bold(),
            ))
            .title(
                Line::from(Span::styled(progress, Style::default().fg(t.text_muted)))
                    .alignment(Alignment::Right),
            ),
    )
    .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, popup_area);
}
