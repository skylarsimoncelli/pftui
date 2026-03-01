use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let sep_line = Line::from(Span::styled(
        "─".repeat(48),
        Style::default().fg(t.border_subtle),
    ));

    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Keybindings",
            Style::default().bold().fg(t.text_accent),
        )),
        sep_line.clone(),
        Line::from(vec![
            Span::styled("q / Ctrl+C  ", Style::default().fg(t.key_hint)),
            Span::styled("Quit", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("?           ", Style::default().fg(t.key_hint)),
            Span::styled("Toggle this help", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("1           ", Style::default().fg(t.key_hint)),
            Span::styled("Positions view", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("2           ", Style::default().fg(t.key_hint)),
            Span::styled("Transactions view", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("j/\u{2193}  k/\u{2191}    ", Style::default().fg(t.key_hint)),
            Span::styled("Navigate up/down", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("gg          ", Style::default().fg(t.key_hint)),
            Span::styled("Jump to top", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("G           ", Style::default().fg(t.key_hint)),
            Span::styled("Jump to bottom", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("Enter       ", Style::default().fg(t.key_hint)),
            Span::styled("Toggle price chart", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("J/K         ", Style::default().fg(t.key_hint)),
            Span::styled("Cycle chart variant", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("Esc         ", Style::default().fg(t.key_hint)),
            Span::styled("Close chart / help", Style::default().fg(t.text_primary)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Sorting (Positions)",
            Style::default().bold().fg(t.text_accent),
        )),
        sep_line.clone(),
        Line::from(vec![
            Span::styled("a           ", Style::default().fg(t.key_hint)),
            Span::styled("Sort by allocation %", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("%           ", Style::default().fg(t.key_hint)),
            Span::styled("Sort by gain %", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("$           ", Style::default().fg(t.key_hint)),
            Span::styled("Sort by total gain", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("n           ", Style::default().fg(t.key_hint)),
            Span::styled("Sort by name", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("c           ", Style::default().fg(t.key_hint)),
            Span::styled("Sort by category", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("Tab         ", Style::default().fg(t.key_hint)),
            Span::styled("Toggle ascending/descending", Style::default().fg(t.text_primary)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Other",
            Style::default().bold().fg(t.text_accent),
        )),
        sep_line,
        Line::from(vec![
            Span::styled("d           ", Style::default().fg(t.key_hint)),
            Span::styled("Sort by date (tx view)", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("f           ", Style::default().fg(t.key_hint)),
            Span::styled("Cycle category filter", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("r           ", Style::default().fg(t.key_hint)),
            Span::styled("Force refresh prices", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("p           ", Style::default().fg(t.key_hint)),
            Span::styled("Toggle privacy view (full mode)", Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("t           ", Style::default().fg(t.key_hint)),
            Span::styled("Cycle color theme", Style::default().fg(t.text_primary)),
        ]),
        Line::from(""),
    ];

    // Center the help overlay
    let width = 55u16.min(area.width.saturating_sub(4));
    let height = (help_text.len() as u16 + 2).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    let help = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                " \u{25C6} Help ",
                Style::default().fg(t.text_accent).bold(),
            )),
    );

    frame.render_widget(help, popup_area);
}
