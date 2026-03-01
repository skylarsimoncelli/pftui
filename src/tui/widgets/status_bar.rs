use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::config::PortfolioMode;
use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Pulsing live indicator
    let dot_color = if app.prices_live {
        theme::pulse_color(t.live_green, t.surface_0, app.tick_count, theme::PULSE_PERIOD)
    } else {
        t.stale_yellow
    };

    let live_text = if app.prices_live { "Live" } else { "Stale" };
    let live_indicator = vec![
        Span::styled("● ", Style::default().fg(dot_color)),
        Span::styled(
            live_text,
            Style::default().fg(if app.prices_live {
                t.live_green
            } else {
                t.stale_yellow
            }),
        ),
    ];

    let filter_text = app
        .category_filter
        .map(|c| format!(" [{}]", c))
        .unwrap_or_default();

    let sep = Span::styled(" | ", Style::default().fg(t.text_muted));

    let mut spans = vec![
        Span::styled(" [?]", Style::default().fg(t.key_hint)),
        Span::styled("Help", Style::default().fg(t.text_secondary)),
        sep.clone(),
        Span::styled("[Enter]", Style::default().fg(t.key_hint)),
        Span::styled("Chart", Style::default().fg(t.text_secondary)),
        sep.clone(),
        Span::styled("[r]", Style::default().fg(t.key_hint)),
        Span::styled("Refresh", Style::default().fg(t.text_secondary)),
        sep.clone(),
        Span::styled("[f]", Style::default().fg(t.key_hint)),
        Span::styled("Filter", Style::default().fg(t.text_secondary)),
        Span::styled(filter_text, Style::default().fg(t.text_secondary)),
    ];

    if app.portfolio_mode == PortfolioMode::Full {
        spans.push(sep.clone());
        spans.push(Span::styled("[p]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Privacy", Style::default().fg(t.text_secondary)));
    }

    spans.push(sep);
    spans.push(Span::styled("[t]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled("Theme", Style::default().fg(t.text_secondary)));

    spans.push(Span::raw("  "));
    spans.extend(live_indicator);

    let hints = Line::from(spans);

    let status = Paragraph::new(hints).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(t.border_subtle))
            .style(Style::default().bg(t.surface_2)),
    );

    frame.render_widget(status, area);
}
