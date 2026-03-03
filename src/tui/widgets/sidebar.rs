use ratatui::prelude::*;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::tui::widgets::{allocation_bars, portfolio_sparkline, portfolio_stats, regime_assets, regime_bar};

/// Renders the portfolio overview panel: value summary, allocation bars,
/// portfolio sparkline chart, key portfolio stats, and regime health bar.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if is_privacy_view(app) {
        // In privacy view, allocation bars get full height (no sparkline/stats/regime)
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

        let stats_height = portfolio_stats::STATS_HEIGHT;
        let regime_height = regime_bar::REGIME_BAR_HEIGHT;
        let show_regime = app.regime_score.has_data();

        let assets_height = regime_assets::compute_height(app);
        let show_assets = show_regime && assets_height > 0;

        if show_regime && show_assets && area.height > alloc_height + stats_height + regime_height + assets_height + 10 {
            // Full layout with regime + asset suggestions
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(alloc_height),
                    Constraint::Min(10),
                    Constraint::Length(stats_height),
                    Constraint::Length(regime_height),
                    Constraint::Length(assets_height),
                ])
                .split(area);

            allocation_bars::render(frame, chunks[0], app);
            portfolio_sparkline::render(frame, chunks[1], app);
            portfolio_stats::render(frame, chunks[2], app);
            regime_bar::render(frame, chunks[3], app);
            regime_assets::render(frame, chunks[4], app);
        } else if show_regime && area.height > alloc_height + stats_height + regime_height + 10 {
            // Full layout with regime bar only (no room for asset suggestions)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(alloc_height),
                    Constraint::Min(10),
                    Constraint::Length(stats_height),
                    Constraint::Length(regime_height),
                ])
                .split(area);

            allocation_bars::render(frame, chunks[0], app);
            portfolio_sparkline::render(frame, chunks[1], app);
            portfolio_stats::render(frame, chunks[2], app);
            regime_bar::render(frame, chunks[3], app);
        } else if area.height > alloc_height + stats_height + 10 {
            // Full layout without regime: alloc bars + sparkline + stats
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(alloc_height),
                    Constraint::Min(10),
                    Constraint::Length(stats_height),
                ])
                .split(area);

            allocation_bars::render(frame, chunks[0], app);
            portfolio_sparkline::render(frame, chunks[1], app);
            portfolio_stats::render(frame, chunks[2], app);
        } else {
            // Tight layout: alloc bars + sparkline (not enough room for stats/regime)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(alloc_height),
                    Constraint::Min(10),
                ])
                .split(area);

            allocation_bars::render(frame, chunks[0], app);
            portfolio_sparkline::render(frame, chunks[1], app);
        }
    }
}
