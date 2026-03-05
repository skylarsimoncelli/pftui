use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let entries = &app.journal_entries;
    let t = &app.theme;

    // Filter entries if search query is active
    let filtered_entries: Vec<&crate::db::journal::JournalEntry> = if app.journal_search_query.is_empty() {
        entries.iter().collect()
    } else {
        entries
            .iter()
            .filter(|e| {
                e.content.to_lowercase().contains(&app.journal_search_query.to_lowercase())
                    || e.tag.as_ref().is_some_and(|t| t.to_lowercase().contains(&app.journal_search_query.to_lowercase()))
                    || e.symbol.as_ref().is_some_and(|s| s.to_lowercase().contains(&app.journal_search_query.to_lowercase()))
            })
            .collect()
    };

    if filtered_entries.is_empty() {
        let empty_msg = if app.journal_search_query.is_empty() {
            "No journal entries yet. Use 'pftui journal add' to create entries."
        } else {
            "No entries match your search."
        };
        let empty = Paragraph::new(empty_msg)
            .style(Style::default().fg(t.text_secondary))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.border_inactive))
                    .title(" Journal ")
                    .title_style(Style::default().fg(t.text_primary).bold()),
            );
        frame.render_widget(empty, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Date"),
        Cell::from("Tag"),
        Cell::from("Symbol"),
        Cell::from("Status"),
        Cell::from("Content"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = filtered_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let row_bg = if i == app.journal_selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_1_alt
            };

            let style = Style::default().bg(row_bg);

            let marker = if i == app.journal_selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };

            // Parse timestamp to show just the date (YYYY-MM-DD HH:MM)
            let date_str = entry.timestamp
                .split('T')
                .next()
                .unwrap_or(&entry.timestamp)
                .to_string();
            let time_str = entry.timestamp
                .split('T')
                .nth(1)
                .and_then(|t| t.split(':').take(2).collect::<Vec<_>>().join(":").into())
                .unwrap_or_default();
            let datetime = if !time_str.is_empty() {
                format!("{} {}", date_str, time_str)
            } else {
                date_str
            };

            let date_line = Line::from(vec![marker, Span::raw(format!(" {}", datetime))]);

            // Truncate content to fit
            let content_truncated = if entry.content.len() > 60 {
                format!("{}...", &entry.content[..57])
            } else {
                entry.content.clone()
            };

            // Status color coding
            let status_color = match entry.status.as_str() {
                "active" => t.gain_green,
                "closed" => t.text_secondary,
                "invalidated" => t.loss_red,
                _ => t.text_primary,
            };

            Row::new(vec![
                Cell::from(date_line),
                Cell::from(entry.tag.as_deref().unwrap_or("-"))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(entry.symbol.as_deref().unwrap_or("-"))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(entry.status.clone())
                    .style(Style::default().fg(status_color)),
                Cell::from(content_truncated)
                    .style(Style::default().fg(t.text_primary)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(17), // Date
        Constraint::Length(12), // Tag
        Constraint::Length(10), // Symbol
        Constraint::Length(12), // Status
        Constraint::Percentage(100), // Content (takes remaining space)
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_inactive))
                .title(if app.journal_search_query.is_empty() {
                    " Journal "
                } else {
                    " Journal (filtered) "
                })
                .title_style(Style::default().fg(t.text_primary).bold()),
        )
        .column_spacing(1);

    frame.render_widget(table, area);
}
