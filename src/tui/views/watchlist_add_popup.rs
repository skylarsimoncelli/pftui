use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;
use crate::tui::theme;
use crate::tui::views::search_overlay::build_results;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Some(state) = &app.watchlist_add_popup else {
        return;
    };
    let t = &app.theme;

    let width = (area.width * 4 / 5).clamp(40, 80);
    let height = (area.height * 4 / 5).clamp(10, 40);
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
            format!(
                " ◆ Add Watchlist Item (Group {}) ",
                app.watchlist_active_group
            ),
            Style::default().fg(t.text_accent).bold(),
        ))
        .title(
            Line::from(Span::styled(
                " Type symbol/name  Enter:add  Esc:cancel ",
                Style::default().fg(t.text_muted),
            ))
            .alignment(Alignment::Right),
        );
    frame.render_widget(block, popup_area);

    let inner = Rect::new(
        popup_area.x + 1,
        popup_area.y + 1,
        popup_area.width.saturating_sub(2),
        popup_area.height.saturating_sub(2),
    );

    let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let results_area = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );

    let cursor_char = if app.tick_count % 30 < 15 { "▏" } else { " " };
    let input_line = Line::from(vec![
        Span::styled("  + ", Style::default().fg(t.text_accent).bold()),
        Span::styled(state.query.clone(), Style::default().fg(t.text_primary)),
        Span::styled(cursor_char, Style::default().fg(t.text_accent)),
    ]);
    frame.render_widget(
        Paragraph::new(input_line).style(Style::default().bg(t.surface_2)),
        input_area,
    );

    let sep = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(t.border_subtle),
    ));
    frame.render_widget(
        Paragraph::new(sep).style(Style::default().bg(t.surface_2)),
        sep_area,
    );

    let results = build_results(app, &state.query);
    if state.query.trim().is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Type to search for a symbol to add…",
                Style::default().fg(t.text_muted).italic(),
            )))
            .style(Style::default().bg(t.surface_2)),
            results_area,
        );
        return;
    }

    if results.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No matching assets",
                Style::default().fg(t.text_muted),
            )))
            .style(Style::default().bg(t.surface_2)),
            results_area,
        );
        return;
    }

    let selected = state.selected.min(results.len().saturating_sub(1));
    let visible = results_area.height as usize;
    let scroll_offset = if selected >= visible {
        selected - visible + 1
    } else {
        0
    };

    let mut lines = Vec::with_capacity(visible);
    for (idx, result) in results.iter().enumerate().skip(scroll_offset).take(visible) {
        let row_bg = if idx == selected {
            t.surface_3
        } else {
            t.surface_2
        };
        let marker = if idx == selected { "▸" } else { " " };
        let status = if result.in_watchlist {
            "○"
        } else if result.in_portfolio {
            "◆"
        } else {
            " "
        };
        let line = Line::from(vec![
            Span::styled(
                format!("{marker}{status} "),
                Style::default().fg(t.text_accent).bg(row_bg),
            ),
            Span::styled(
                format!("{:<8}", result.symbol),
                Style::default().fg(t.text_primary).bg(row_bg).bold(),
            ),
            Span::styled(
                format!("{:<10}", result.category),
                Style::default().fg(t.text_secondary).bg(row_bg),
            ),
            Span::styled(
                result.name.clone(),
                Style::default().fg(t.text_primary).bg(row_bg),
            ),
        ]);
        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(t.surface_2)),
        results_area,
    );
}
