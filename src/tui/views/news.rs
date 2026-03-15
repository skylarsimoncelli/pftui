use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    app.page_table_area = Some(if area.width >= 120 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area)[0]
    } else {
        area
    });
    let entries = filtered_entries(app);
    let t = &app.theme;

    if entries.is_empty() {
        let empty_msg = if !app.news_search_query.is_empty()
            || app.news_filter_source.is_some()
            || app.news_filter_category.is_some()
        {
            "No news entries match your filters."
        } else {
            "No news entries available. News data may not have been fetched yet."
        };
        let empty = Paragraph::new(empty_msg)
            .style(Style::default().fg(t.text_secondary))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(t.border_inactive))
                    .title(" News ")
                    .title_style(Style::default().fg(t.text_primary).bold()),
            );
        frame.render_widget(empty, area);
        return;
    }

    if area.width >= 120 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area);
        render_table(frame, chunks[0], app, &entries);
        render_context_panel(frame, chunks[1], app, &entries);
    } else {
        render_table(frame, area, app, &entries);
    }
}

fn render_table(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    entries: &[&crate::db::news_cache::NewsEntry],
) {
    let t = &app.theme;
    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Source"),
        Cell::from("Category"),
        Cell::from("Headline"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let row_bg = if i == app.news_selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_1_alt
            };
            let marker = if i == app.news_selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let time_line = Line::from(vec![
                marker,
                Span::raw(format!(" {}", format_relative_time(entry.published_at))),
            ]);
            let headline = if entry.title.len() > 80 {
                format!("{}...", &entry.title[..77])
            } else {
                entry.title.clone()
            };

            Row::new(vec![
                Cell::from(time_line),
                Cell::from(entry.source.clone()).style(Style::default().fg(t.text_secondary)),
                Cell::from(entry.category.clone()).style(Style::default().fg(category_color(entry, t))),
                Cell::from(headline).style(Style::default().fg(t.text_primary)),
            ])
            .style(Style::default().bg(row_bg))
        })
        .collect();

    let mut title = String::from(" News ");
    if let Some(ref source) = app.news_filter_source {
        title.push_str(&format!("[source: {}] ", source));
    }
    if let Some(ref category) = app.news_filter_category {
        title.push_str(&format!("[category: {}] ", category));
    }
    if !app.news_search_query.is_empty() {
        title.push_str(&format!("[search: {}] ", app.news_search_query));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(15),
            Constraint::Length(13),
            Constraint::Percentage(100),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_inactive))
            .title(title)
            .title(
                Line::from(Span::styled(
                    "Enter:preview  J:journal  A:watch  o:open",
                    Style::default().fg(t.text_muted),
                ))
                .alignment(Alignment::Right),
            )
            .title_style(Style::default().fg(t.text_primary).bold()),
    )
    .column_spacing(1);

    frame.render_widget(table, area);
}

fn render_context_panel(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    entries: &[&crate::db::news_cache::NewsEntry],
) {
    let t = &app.theme;
    let selected = entries
        .get(app.news_selected_index)
        .copied()
        .unwrap_or(entries[0]);
    let symbols = app.selected_news_detected_symbols();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(" News Context ", Style::default().fg(t.text_accent).bold()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::styled("Headline", Style::default().fg(t.text_secondary).bold()),
        Line::raw(selected.title.clone()),
        Line::raw(""),
        Line::styled("Summary", Style::default().fg(t.text_secondary).bold()),
        Line::raw(if selected.description.trim().is_empty() {
            "No summary available.".to_string()
        } else {
            selected.description.clone()
        }),
        Line::raw(""),
        Line::styled("Related symbols", Style::default().fg(t.text_secondary).bold()),
    ];
    if symbols.is_empty() {
        lines.push(Line::styled("None detected", Style::default().fg(t.text_muted)));
    } else {
        lines.push(Line::raw(symbols.join(", ")));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Workflow", Style::default().fg(t.text_secondary).bold()));
    lines.push(Line::raw("J create journal entry from article"));
    lines.push(Line::raw("A add first detected symbol to watchlist"));
    lines.push(Line::raw("o open original URL"));

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(t.text_primary));
    frame.render_widget(paragraph, inner);
}

fn filtered_entries(app: &App) -> Vec<&crate::db::news_cache::NewsEntry> {
    app.news_entries
        .iter()
        .filter(|entry| {
            if let Some(ref source) = app.news_filter_source {
                if !entry.source.eq_ignore_ascii_case(source) {
                    return false;
                }
            }
            if let Some(ref category) = app.news_filter_category {
                if !entry.category.eq_ignore_ascii_case(category) {
                    return false;
                }
            }
            if !app.news_search_query.is_empty() {
                let query = app.news_search_query.to_lowercase();
                if !entry.title.to_lowercase().contains(&query)
                    && !entry.source.to_lowercase().contains(&query)
                {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn category_color(entry: &crate::db::news_cache::NewsEntry, t: &crate::tui::theme::Theme) -> Color {
    match entry.category.to_lowercase().as_str() {
        "crypto" => Color::Rgb(255, 165, 0),
        "macro" => Color::Rgb(100, 149, 237),
        "commodities" => Color::Rgb(255, 215, 0),
        "geopolitics" => Color::Rgb(220, 20, 60),
        "markets" => t.text_primary,
        _ => t.text_secondary,
    }
}

fn format_relative_time(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let diff = now - timestamp;

    if diff < 60 {
        "now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604_800 {
        format!("{}d ago", diff / 86_400)
    } else {
        format!("{}w ago", diff / 604_800)
    }
}
