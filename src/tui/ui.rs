use ratatui::prelude::*;
use ratatui::widgets::Block;

use crate::app::{App, ViewMode};
use crate::tui::views;
use crate::tui::widgets;

pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Fill entire background with deepest surface
    let bg = Block::default().style(Style::default().bg(app.theme.surface_0));
    frame.render_widget(bg, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // header
            Constraint::Min(5),    // main content
            Constraint::Length(2), // status bar
        ])
        .split(size);

    widgets::header::render(frame, chunks[0], app);

    match app.view_mode {
        ViewMode::Positions => render_positions_layout(frame, chunks[1], app),
        ViewMode::Transactions => {
            if app.portfolio_mode == crate::config::PortfolioMode::Percentage {
                render_positions_layout(frame, chunks[1], app);
            } else {
                views::transactions::render(frame, chunks[1], app);
            }
        }
        ViewMode::Markets => views::markets::render(frame, chunks[1], app),
    }

    widgets::status_bar::render(frame, chunks[2], app);

    if app.show_help {
        views::help::render(frame, size, app);
    }
}

fn render_positions_layout(frame: &mut Frame, area: Rect, app: &App) {
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(57),
            Constraint::Percentage(43),
        ])
        .split(area);

    views::positions::render(frame, h_chunks[0], app);

    if app.detail_open {
        widgets::price_chart::render(frame, h_chunks[1], app);
    } else {
        widgets::sidebar::render(frame, h_chunks[1], app);
    }
}
