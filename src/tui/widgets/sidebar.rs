use ratatui::prelude::*;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::tui::widgets::{allocation_bars, portfolio_sparkline};

/// Renders the sidebar: allocation bars on top, portfolio sparkline on bottom.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if is_privacy_view(app) {
        // In privacy view, allocation bars get full height (no portfolio sparkline)
        allocation_bars::render(frame, area, app);
    } else {
        // Dynamic allocation height based on category count
        let cat_count = app
            .positions
            .iter()
            .filter(|p| p.allocation_pct.is_some_and(|a| a > dec!(0)))
            .map(|p| p.category)
            .collect::<std::collections::HashSet<_>>()
            .len();
        // +2 for border, +1 for total value line
        let has_total = app.total_value > dec!(0);
        let extra = if has_total { 1 } else { 0 };
        let alloc_height = (cat_count as u16 + 2 + extra).max(4);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(alloc_height),
                Constraint::Min(6),
            ])
            .split(area);

        allocation_bars::render(frame, chunks[0], app);
        portfolio_sparkline::render(frame, chunks[1], app);
    }
}
