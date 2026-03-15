use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    app.page_table_area = Some(if area.width >= 118 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area)[0]
    } else {
        area
    });
    let entries = filtered_entries(app);
    if area.width >= 118 && !entries.is_empty() {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area);
        render_table(frame, chunks[0], app, &entries);
        render_detail_panel(frame, chunks[1], app, &entries);
    } else {
        render_table(frame, area, app, &entries);
    }
}

fn render_table(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    entries: &[&crate::db::journal::JournalEntry],
) {
    let t = &app.theme;
    if entries.is_empty() {
        let empty_msg = if app.journal_search_query.is_empty() {
            "No journal entries yet. Press 'a' to add one."
        } else {
            "No entries match your search."
        };
        let empty = Paragraph::new(empty_msg)
            .style(Style::default().fg(t.text_secondary))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.border_inactive))
                    .title(" Journal · a:add ")
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

    let rows: Vec<Row> = entries
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
            let marker = if i == app.journal_selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let date = entry.timestamp.split('T').next().unwrap_or(&entry.timestamp);
            let status_color = match entry.status.as_str() {
                "active" => t.gain_green,
                "closed" => t.text_secondary,
                "invalidated" => t.loss_red,
                _ => t.text_primary,
            };
            let content = if entry.content.len() > 60 {
                format!("{}...", &entry.content[..57])
            } else {
                entry.content.clone()
            };

            Row::new(vec![
                Cell::from(Line::from(vec![marker, Span::raw(format!(" {}", date))])),
                Cell::from(entry.tag.as_deref().unwrap_or("-"))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(entry.symbol.as_deref().unwrap_or("-"))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(entry.status.clone()).style(Style::default().fg(status_color)),
                Cell::from(content).style(Style::default().fg(t.text_primary)),
            ])
            .style(Style::default().bg(row_bg))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Percentage(100),
        ],
    )
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
            .title(
                Line::from(Span::styled(
                    "a:add  c:close  x:invalidate",
                    Style::default().fg(t.text_muted),
                ))
                .alignment(Alignment::Right),
            )
            .title_style(Style::default().fg(t.text_primary).bold()),
    )
    .column_spacing(1);

    frame.render_widget(table, area);
}

fn render_detail_panel(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    entries: &[&crate::db::journal::JournalEntry],
) {
    let t = &app.theme;
    let entry = entries
        .get(app.journal_selected_index)
        .copied()
        .unwrap_or(entries[0]);
    let active = app.journal_entries.iter().filter(|item| item.status == "active").count();
    let tagged = app.journal_entries.iter().filter(|item| item.tag.is_some()).count();
    let with_symbols = app
        .journal_entries
        .iter()
        .filter(|item| item.symbol.is_some())
        .count();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(" Entry Detail ", Style::default().fg(t.text_accent).bold()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::styled("Selected entry", Style::default().fg(t.text_secondary).bold()),
        Line::raw(entry.timestamp.clone()),
        Line::raw(entry.content.clone()),
        Line::raw(""),
        Line::from(format!(
            "Tag: {}",
            entry.tag.clone().unwrap_or_else(|| "-".to_string())
        )),
        Line::from(format!(
            "Symbol: {}",
            entry.symbol.clone().unwrap_or_else(|| "-".to_string())
        )),
        Line::from(format!("Status: {}", entry.status)),
        Line::raw(""),
        Line::styled("Journal stats", Style::default().fg(t.text_secondary).bold()),
        Line::from(format!("Active entries: {active}")),
        Line::from(format!("Tagged entries: {tagged}")),
        Line::from(format!("Symbol-linked: {with_symbols}")),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(t.text_primary)),
        inner,
    );
}

fn filtered_entries(app: &App) -> Vec<&crate::db::journal::JournalEntry> {
    if app.journal_search_query.is_empty() {
        app.journal_entries.iter().collect()
    } else {
        let query = app.journal_search_query.to_lowercase();
        app.journal_entries
            .iter()
            .filter(|entry| {
                entry.content.to_lowercase().contains(&query)
                    || entry
                        .tag
                        .as_ref()
                        .is_some_and(|tag| tag.to_lowercase().contains(&query))
                    || entry
                        .symbol
                        .as_ref()
                        .is_some_and(|symbol| symbol.to_lowercase().contains(&query))
            })
            .collect()
    }
}
