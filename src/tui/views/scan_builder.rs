use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::{App, ScanBuilderMode};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let width = 92u16.min(area.width.saturating_sub(4));
    let height = 22u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    crate::tui::theme::render_popup_shadow(frame, popup_area, area, t);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(popup_area);

    let title = match app.scan_builder_mode {
        ScanBuilderMode::Edit => " Scan Builder ",
        ScanBuilderMode::SaveName => " Scan Builder: Save Query ",
        ScanBuilderMode::LoadName => " Scan Builder: Load Query ",
    };

    let expression = if app.scan_builder_clauses.is_empty() {
        "(empty)".to_string()
    } else {
        app.scan_builder_clauses.join(" and ")
    };
    let expr_widget = Paragraph::new(expression).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(crate::tui::theme::BORDER_POPUP)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                title,
                Style::default().fg(t.text_accent).bold(),
            )),
    );
    frame.render_widget(expr_widget, chunks[0]);

    let max_items = chunks[1].height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .scan_builder_clauses
        .iter()
        .take(max_items)
        .enumerate()
        .map(|(idx, clause)| {
            let selected = idx == app.scan_builder_selected;
            let style = if selected {
                Style::default().fg(t.surface_2).bg(t.text_accent).bold()
            } else {
                Style::default().fg(t.text_primary)
            };
            ListItem::new(Line::from(vec![Span::styled(
                format!("{:>2}. {}", idx + 1, clause),
                style,
            )]))
        })
        .collect();
    let clauses = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_subtle))
            .style(Style::default().bg(t.surface_2))
            .title(" Clauses "),
    );
    frame.render_widget(clauses, chunks[1]);

    let input_label = match app.scan_builder_mode {
        ScanBuilderMode::Edit => "Clause Input (field op value)",
        ScanBuilderMode::SaveName => "Save Name",
        ScanBuilderMode::LoadName => "Load Name",
    };
    let input_value = match app.scan_builder_mode {
        ScanBuilderMode::Edit => app.scan_builder_clause_input.clone(),
        ScanBuilderMode::SaveName | ScanBuilderMode::LoadName => {
            app.scan_builder_name_input.clone()
        }
    };
    let input = Paragraph::new(input_value).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_subtle))
            .style(Style::default().bg(t.surface_1))
            .title(format!(" {} ", input_label)),
    );
    frame.render_widget(input, chunks[2]);

    let message = app
        .scan_builder_message
        .as_deref()
        .unwrap_or("Build query clauses and save/load named scans.");
    let msg = Paragraph::new(message).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .style(Style::default().bg(t.surface_2).fg(t.text_muted)),
    );
    frame.render_widget(msg, chunks[3]);

    let hints = match app.scan_builder_mode {
        ScanBuilderMode::Edit => {
            "a/Enter add  r remove  s save  l load  c clear  ↑/↓ select  Esc close"
        }
        ScanBuilderMode::SaveName => "Enter save  Esc cancel",
        ScanBuilderMode::LoadName => "Enter load  Esc cancel",
    };
    let hint = Paragraph::new(hints).block(
        Block::default()
            .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
            .style(Style::default().bg(t.surface_2).fg(t.text_muted)),
    );
    frame.render_widget(hint, chunks[4]);
}
