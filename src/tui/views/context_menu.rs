use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;
use crate::tui::theme;

/// Renders the right-click context menu as a small floating popup at the
/// stored screen position. The menu lists available actions with the
/// currently highlighted item shown in accent color.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let menu = match &app.context_menu {
        Some(m) => m,
        None => return,
    };

    let t = &app.theme;
    let action_count = menu.actions.len() as u16;

    // Menu dimensions: fixed width, height = actions + 2 (borders)
    let menu_width: u16 = 22;
    let menu_height: u16 = action_count + 2;

    // Position the menu near the click, clamping to screen bounds.
    // Prefer placing it to the right and below the click point.
    let x = if menu.col + menu_width < area.width {
        menu.col
    } else {
        area.width.saturating_sub(menu_width)
    };
    let y = if menu.row + menu_height < area.height {
        menu.row
    } else {
        area.height.saturating_sub(menu_height)
    };

    let menu_area = Rect::new(x, y, menu_width, menu_height);

    // Draw shadow behind menu
    theme::render_popup_shadow(frame, menu_area, area, t);

    // Clear the area and draw the menu
    frame.render_widget(Clear, menu_area);

    let mut lines: Vec<Line> = Vec::with_capacity(menu.actions.len());
    for (i, action) in menu.actions.iter().enumerate() {
        let label = action.label();
        let is_selected = i == menu.selected;
        let style = if is_selected {
            Style::default().fg(t.surface_0).bg(t.text_accent).bold()
        } else {
            Style::default().fg(t.text_primary).bg(t.surface_2)
        };

        // Pad label to fill the menu width (minus borders)
        let inner_width = (menu_width - 2) as usize;
        let prefix = if is_selected { " ▸ " } else { "   " };
        let padded = format!("{prefix}{label:<width$}", width = inner_width - 3);
        // Truncate if needed
        let display: String = padded.chars().take(inner_width).collect();

        lines.push(Line::from(Span::styled(display, style)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_POPUP)
        .border_style(Style::default().fg(t.border_accent))
        .style(Style::default().bg(t.surface_2))
        .title(Span::styled(
            format!(" {} ", menu.symbol),
            Style::default().fg(t.text_accent).bold(),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, menu_area);
}

#[cfg(test)]
mod tests {
    use crate::app::{ContextMenuAction, ContextMenuState};

    #[test]
    fn context_menu_actions_full_mode() {
        let actions = ContextMenuAction::for_positions(false);
        assert_eq!(actions.len(), 4);
        assert_eq!(actions[0], ContextMenuAction::ViewDetail);
        assert_eq!(actions[1], ContextMenuAction::AddTransaction);
        assert_eq!(actions[2], ContextMenuAction::Delete);
        assert_eq!(actions[3], ContextMenuAction::CopySymbol);
    }

    #[test]
    fn context_menu_actions_percentage_mode() {
        let actions = ContextMenuAction::for_positions(true);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], ContextMenuAction::ViewDetail);
        assert_eq!(actions[1], ContextMenuAction::CopySymbol);
    }

    #[test]
    fn action_labels_are_nonempty() {
        let actions = ContextMenuAction::for_positions(false);
        for action in &actions {
            assert!(!action.label().is_empty());
        }
    }

    #[test]
    fn context_menu_state_defaults() {
        let state = ContextMenuState {
            col: 10,
            row: 5,
            selected: 0,
            actions: ContextMenuAction::for_positions(false),
            symbol: "AAPL".to_string(),
        };
        assert_eq!(state.selected, 0);
        assert_eq!(state.symbol, "AAPL");
        assert_eq!(state.actions.len(), 4);
    }
}
