use ratatui::prelude::*;
use ratatui::widgets::Block;

use crate::app::{App, ViewMode};
use crate::tui::views;
use crate::tui::widgets;

/// Width threshold below which the sidebar is hidden and positions get full width.
pub const COMPACT_WIDTH: u16 = 100;

/// Minimum height for the portfolio overview panel (allocation bars + sparkline).
const MIN_OVERVIEW_HEIGHT: u16 = 14;

pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Fill entire background with deepest surface
    let bg = Block::default().style(Style::default().bg(app.theme.surface_0));
    frame.render_widget(bg, size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(widgets::header::header_height(app)),  // header (dynamic: 3 with ticker, 2 without)
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
        ViewMode::Economy => views::economy::render(frame, chunks[1], app),
        ViewMode::Watchlist => render_watchlist_layout(frame, chunks[1], app),
        ViewMode::News => views::news::render(frame, chunks[1], app),
        ViewMode::Journal => views::journal::render(frame, chunks[1], app),
    }

    widgets::status_bar::render(frame, chunks[2], app);

    if app.detail_popup_open && matches!(app.view_mode, ViewMode::Positions) {
        views::position_detail::render(frame, size, app);
    }

    if app.context_menu.is_some() {
        views::context_menu::render(frame, size, app);
    }

    if app.search_overlay_open {
        views::search_overlay::render(frame, size, app);
    }

    if app.asset_detail.is_some() {
        views::asset_detail_popup::render(frame, size, app);
    }

    if app.show_help {
        views::help::render(frame, size, app);
    }

    if app.alerts_open {
        views::alerts_popup::render(frame, app);
    }
}

fn render_positions_layout(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::tui::theme;

    let width = app.terminal_width;

    if width < COMPACT_WIDTH {
        // Compact: full width, no right pane
        views::positions::render(frame, area, app);
    } else {
        // Standard two-column layout:
        //   Left (57%):  table (top) + portfolio overview (bottom)
        //   Right (43%): asset section header + asset header + price chart
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(57),
                Constraint::Percentage(43),
            ])
            .split(area);

        // Left pane: section header + table + portfolio overview
        let left_height = h_chunks[0].height;
        if left_height > MIN_OVERVIEW_HEIGHT + 5 + theme::SECTION_HEADER_HEIGHT {
            // Enough room: split left pane vertically with section header + overview
            let overview_height = compute_overview_height(app, left_height);
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(theme::SECTION_HEADER_HEIGHT), // section header
                    Constraint::Min(5),                               // positions table
                    Constraint::Length(overview_height),               // portfolio overview
                ])
                .split(h_chunks[0]);

            theme::render_section_header(frame, left_chunks[0], "PORTFOLIO OVERVIEW", &app.theme);
            views::positions::render(frame, left_chunks[1], app);
            widgets::sidebar::render(frame, left_chunks[2], app);
        } else if left_height > 5 + theme::SECTION_HEADER_HEIGHT {
            // Enough for header + table, but no overview
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(theme::SECTION_HEADER_HEIGHT),
                    Constraint::Min(5),
                ])
                .split(h_chunks[0]);

            theme::render_section_header(frame, left_chunks[0], "PORTFOLIO OVERVIEW", &app.theme);
            views::positions::render(frame, left_chunks[1], app);
        } else {
            // Too short: table only
            views::positions::render(frame, h_chunks[0], app);
        }

        // Right pane: section header + asset header + price chart
        if app.selected_position().is_some() {
            let header_h = widgets::asset_header::height();
            if h_chunks[1].height > header_h + 6 + theme::SECTION_HEADER_HEIGHT {
                // Enough room for section header + asset header + chart
                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(theme::SECTION_HEADER_HEIGHT), // section header
                        Constraint::Length(header_h),                     // asset info header
                        Constraint::Min(4),                               // price chart
                    ])
                    .split(h_chunks[1]);

                theme::render_section_header(frame, right_chunks[0], "ASSET OVERVIEW", &app.theme);
                widgets::asset_header::render(frame, right_chunks[1], app);
                widgets::price_chart::render(frame, right_chunks[2], app);
            } else if h_chunks[1].height > 6 + theme::SECTION_HEADER_HEIGHT {
                // Enough for section header + chart
                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(theme::SECTION_HEADER_HEIGHT),
                        Constraint::Min(4),
                    ])
                    .split(h_chunks[1]);

                theme::render_section_header(frame, right_chunks[0], "ASSET OVERVIEW", &app.theme);
                widgets::price_chart::render(frame, right_chunks[1], app);
            } else {
                // Too short: just show chart
                widgets::price_chart::render(frame, h_chunks[1], app);
            }
        } else {
            // No position selected — show section header + empty state
            if h_chunks[1].height > theme::SECTION_HEADER_HEIGHT {
                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(theme::SECTION_HEADER_HEIGHT),
                        Constraint::Min(1),
                    ])
                    .split(h_chunks[1]);

                theme::render_section_header(frame, right_chunks[0], "ASSET OVERVIEW", &app.theme);
            }
        }
    }
}

fn render_watchlist_layout(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::theme;

    let width = app.terminal_width;

    if width < COMPACT_WIDTH {
        views::watchlist::render(frame, area, app);
    } else {
        // Section header + watchlist table (full width)
        let left_height = area.height;
        if left_height > 5 + theme::SECTION_HEADER_HEIGHT {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(theme::SECTION_HEADER_HEIGHT),
                    Constraint::Min(5),
                ])
                .split(area);

            theme::render_section_header(frame, chunks[0], "WATCHLIST", &app.theme);
            views::watchlist::render(frame, chunks[1], app);
        } else {
            views::watchlist::render(frame, area, app);
        }
    }
}

/// Compute the ideal height for the portfolio overview panel.
/// Based on the number of asset categories + sparkline space.
fn compute_overview_height(app: &App, max_height: u16) -> u16 {
    use rust_decimal_macros::dec;

    let cat_count = app
        .positions
        .iter()
        .filter(|p| p.allocation_pct.is_some_and(|a| a > dec!(0)))
        .map(|p| p.category)
        .collect::<std::collections::HashSet<_>>()
        .len() as u16;

    // Allocation bars: cat_count + 2 (border) + 1 (total value line)
    let alloc_height = (cat_count + 3).max(4);
    // Sparkline: minimum 12 rows for meaningful chart + timeframe gains
    let sparkline_height = 12u16;
    // Total: allocation + sparkline
    let ideal = alloc_height + sparkline_height;

    // Cap at 40% of available height to leave room for positions
    let cap = (max_height * 2) / 5;
    ideal.min(cap).max(MIN_OVERVIEW_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_width_threshold_is_100() {
        assert_eq!(COMPACT_WIDTH, 100);
    }
}
