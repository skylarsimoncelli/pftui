use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, JournalEntryPopupField};
use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Some(state) = &app.journal_entry_popup else {
        return;
    };
    let t = &app.theme;

    let width = (area.width * 4 / 5).clamp(50, 90);
    let height = (area.height * 3 / 5).clamp(14, 20);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);
    theme::render_popup_shadow(frame, popup_area, area, t);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_POPUP)
        .border_style(Style::default().fg(t.border_accent))
        .style(Style::default().bg(t.surface_2))
        .title(Span::styled(
            " ◆ Add Journal Entry ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .title(
            Line::from(Span::styled(
                " Tab:next  Shift+Tab:prev  Enter:save  Esc:cancel ",
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        );
    frame.render_widget(block.clone(), popup_area);

    let inner = block.inner(popup_area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(inner);

    render_field(
        frame,
        sections[0],
        "Content",
        &state.content,
        state.active_field == JournalEntryPopupField::Content,
        true,
        t,
    );
    render_field(
        frame,
        sections[1],
        "Tag (optional)",
        &state.tag,
        state.active_field == JournalEntryPopupField::Tag,
        false,
        t,
    );
    render_field(
        frame,
        sections[2],
        "Symbol (optional)",
        &state.symbol,
        state.active_field == JournalEntryPopupField::Symbol,
        false,
        t,
    );

    let footer = if let Some(message) = &state.message {
        Line::from(Span::styled(
            message.clone(),
            Style::default().fg(t.loss_red),
        ))
    } else {
        Line::from(vec![
            Span::styled("Tip: ", Style::default().fg(t.text_muted)),
            Span::styled(
                "leave tag and symbol blank for a quick freeform note",
                Style::default().fg(t.text_secondary),
            ),
        ])
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().bg(t.surface_2)),
        sections[3],
    );
}

fn render_field(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    value: &str,
    active: bool,
    wrap: bool,
    t: &theme::Theme,
) {
    let border = if active {
        t.border_accent
    } else {
        t.border_inactive
    };
    let cursor = if active { "▏" } else { "" };
    let text = if value.is_empty() && !active {
        Line::from(Span::styled(" ", Style::default().fg(t.text_muted)))
    } else {
        Line::from(Span::styled(
            format!("{value}{cursor}"),
            Style::default().fg(t.text_primary),
        ))
    };

    let mut paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .style(Style::default().bg(t.surface_1))
            .title(Span::styled(
                format!(" {} ", title),
                Style::default()
                    .fg(if active {
                        t.text_accent
                    } else {
                        t.text_secondary
                    })
                    .bold(),
            )),
    );
    if wrap {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }
    frame.render_widget(paragraph, area);
}
