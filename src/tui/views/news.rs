use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::app::App;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let entries = &app.news_entries;
    let t = &app.theme;

    // Apply filters
    let filtered_entries: Vec<&crate::db::news_cache::NewsEntry> = entries
        .iter()
        .filter(|e| {
            // Source filter
            if let Some(ref source) = app.news_filter_source {
                if !e.source.eq_ignore_ascii_case(source) {
                    return false;
                }
            }
            // Category filter
            if let Some(ref category) = app.news_filter_category {
                if !e.category.eq_ignore_ascii_case(category) {
                    return false;
                }
            }
            // Search query
            if !app.news_search_query.is_empty() {
                let query = app.news_search_query.to_lowercase();
                if !e.title.to_lowercase().contains(&query)
                    && !e.source.to_lowercase().contains(&query)
                {
                    return false;
                }
            }
            true
        })
        .collect();

    if filtered_entries.is_empty() {
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

    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Source"),
        Cell::from("Category"),
        Cell::from("Headline"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = filtered_entries
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

            let style = Style::default().bg(row_bg);

            let marker = if i == app.news_selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };

            // Format timestamp as relative time (e.g., "2h ago")
            let time_str = format_relative_time(entry.published_at);

            let time_line = Line::from(vec![marker, Span::raw(format!(" {}", time_str))]);

            // Category color coding
            let category_color = match entry.category.to_lowercase().as_str() {
                "crypto" => Color::Rgb(255, 165, 0), // orange
                "macro" => Color::Rgb(100, 149, 237), // blue
                "commodities" => Color::Rgb(255, 215, 0), // yellow/gold
                "geopolitics" => Color::Rgb(220, 20, 60), // red
                "markets" => t.text_primary,
                _ => t.text_secondary,
            };

            // Truncate headline if too long
            let headline_truncated = if entry.title.len() > 80 {
                format!("{}...", &entry.title[..77])
            } else {
                entry.title.clone()
            };

            Row::new(vec![
                Cell::from(time_line),
                Cell::from(entry.source.clone()).style(Style::default().fg(t.text_secondary)),
                Cell::from(entry.category.clone()).style(Style::default().fg(category_color)),
                Cell::from(headline_truncated).style(Style::default().fg(t.text_primary)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(10), // Time
        Constraint::Length(15), // Source
        Constraint::Length(13), // Category
        Constraint::Percentage(100), // Headline (takes remaining space)
    ];

    // Build title with active filters
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

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.border_inactive))
                .title(title)
                .title_style(Style::default().fg(t.text_primary).bold()),
        )
        .column_spacing(1);

    frame.render_widget(table, area);
}

/// Format Unix timestamp as relative time (e.g., "2h ago", "1d ago")
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
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}
