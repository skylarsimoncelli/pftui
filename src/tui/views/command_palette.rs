use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::App;

#[derive(Debug, Clone, Copy)]
pub struct CommandPaletteEntry {
    pub command: &'static str,
    pub description: &'static str,
}

const COMMANDS: &[CommandPaletteEntry] = &[
    CommandPaletteEntry { command: "help", description: "Open help overlay" },
    CommandPaletteEntry { command: "refresh", description: "Fetch latest market data" },
    CommandPaletteEntry { command: "theme next", description: "Cycle to next theme" },
    CommandPaletteEntry { command: "split toggle", description: "Toggle split detail pane" },
    CommandPaletteEntry { command: "layout compact", description: "Set compact workspace layout" },
    CommandPaletteEntry { command: "layout split", description: "Set split workspace layout" },
    CommandPaletteEntry { command: "layout analyst", description: "Set analyst workspace layout" },
    CommandPaletteEntry { command: "view positions", description: "Switch to Positions view" },
    CommandPaletteEntry { command: "view transactions", description: "Switch to Transactions view" },
    CommandPaletteEntry { command: "view markets", description: "Switch to Markets view" },
    CommandPaletteEntry { command: "view economy", description: "Switch to Economy view" },
    CommandPaletteEntry { command: "view watchlist", description: "Switch to Watchlist view" },
    CommandPaletteEntry { command: "view analytics", description: "Switch to Analytics view" },
    CommandPaletteEntry { command: "view news", description: "Switch to News view" },
    CommandPaletteEntry { command: "view journal", description: "Switch to Journal view" },
    CommandPaletteEntry { command: "quit", description: "Exit pftui" },
];

pub fn matching_commands(query: &str) -> Vec<&'static CommandPaletteEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return COMMANDS.iter().collect();
    }

    COMMANDS
        .iter()
        .filter(|entry| {
            entry.command.starts_with(&q)
                || entry.command.contains(&q)
                || entry.description.to_lowercase().contains(&q)
        })
        .collect()
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let width = 76u16.min(area.width.saturating_sub(4));
    let height = 14u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    crate::tui::theme::render_popup_shadow(frame, popup_area, area, t);
    frame.render_widget(Clear, popup_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(4), Constraint::Length(1)])
        .split(popup_area);

    let input = Paragraph::new(format!(":{}", app.command_palette_input)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(crate::tui::theme::BORDER_POPUP)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                " Command Palette ",
                Style::default().fg(t.text_accent).bold(),
            )),
    );
    frame.render_widget(input, chunks[0]);

    let matches = matching_commands(&app.command_palette_input);
    let max_items = chunks[1].height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = matches
        .iter()
        .take(max_items)
        .enumerate()
        .map(|(idx, entry)| {
            let selected = idx == app.command_palette_selected;
            let style = if selected {
                Style::default().fg(t.surface_2).bg(t.text_accent).bold()
            } else {
                Style::default().fg(t.text_primary)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<24}", entry.command), style),
                Span::styled(entry.description, style),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .style(Style::default().bg(t.surface_2)),
    );
    frame.render_widget(list, chunks[1]);

    let hint = Paragraph::new("Enter execute  Tab autocomplete  ↑/↓ navigate  Esc close")
        .style(Style::default().fg(t.text_muted))
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .style(Style::default().bg(t.surface_2)),
        );
    frame.render_widget(hint, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_returns_prefix_hits() {
        let results = matching_commands("view mar");
        assert!(results.iter().any(|e| e.command == "view markets"));
    }

    #[test]
    fn matching_returns_all_for_empty_query() {
        let results = matching_commands("");
        assert_eq!(results.len(), COMMANDS.len());
    }
}
