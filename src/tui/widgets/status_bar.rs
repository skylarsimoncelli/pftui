use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::config::PortfolioMode;
use crate::tui::theme;
use crate::tui::ui::COMPACT_WIDTH;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let compact = app.terminal_width < COMPACT_WIDTH;

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

    // Search mode: show search input instead of normal hints
    if app.search_mode {
        let search_spans = vec![
            Span::styled(" /", Style::default().fg(t.text_accent).bold()),
            Span::styled(&app.search_query, Style::default().fg(t.text_primary)),
            Span::styled("█", Style::default().fg(t.text_accent)),
            Span::styled("  (Enter to confirm, Esc to cancel)", Style::default().fg(t.text_muted)),
        ];
        let search_line = Line::from(search_spans);
        let search_bar = Paragraph::new(search_line).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(t.border_accent))
                .style(Style::default().bg(t.surface_2)),
        );
        frame.render_widget(search_bar, area);
        return;
    }

    // Show active search filter indicator
    let search_filter_text = if !app.search_query.is_empty() {
        format!(" [/{}/]", app.search_query)
    } else {
        String::new()
    };

    let filter_text = app
        .category_filter
        .map(|c| format!(" [{}]", c))
        .unwrap_or_default();

    let sep = Span::styled(" | ", Style::default().fg(t.text_muted));

    let mut spans: Vec<Span> = Vec::new();

    if compact {
        // Compact: show only essential hints
        spans.push(Span::styled(" [?]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Help", Style::default().fg(t.text_secondary)));
        spans.push(sep.clone());
        spans.push(Span::styled("[/]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Search", Style::default().fg(t.text_secondary)));
        spans.push(Span::styled(filter_text, Style::default().fg(t.text_secondary)));
        spans.push(Span::styled(search_filter_text, Style::default().fg(t.text_accent)));
    } else {
        // Full: show all hints
        spans.push(Span::styled(" [?]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Help", Style::default().fg(t.text_secondary)));
        spans.push(sep.clone());
        spans.push(Span::styled("[Enter]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Chart", Style::default().fg(t.text_secondary)));
        spans.push(sep.clone());
        spans.push(Span::styled("[r]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Refresh", Style::default().fg(t.text_secondary)));
        spans.push(sep.clone());
        spans.push(Span::styled("[/]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Search", Style::default().fg(t.text_secondary)));
        spans.push(sep.clone());
        spans.push(Span::styled("[f]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Filter", Style::default().fg(t.text_secondary)));
        spans.push(Span::styled(filter_text, Style::default().fg(t.text_secondary)));
        spans.push(Span::styled(search_filter_text, Style::default().fg(t.text_accent)));

        if app.portfolio_mode == PortfolioMode::Full {
            spans.push(sep.clone());
            spans.push(Span::styled("[p]", Style::default().fg(t.key_hint)));
            spans.push(Span::styled("Privacy", Style::default().fg(t.text_secondary)));
        }

        spans.push(sep);
        spans.push(Span::styled("[t]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Theme", Style::default().fg(t.text_secondary)));
    }

    // Show recent price error (fades after ~5 seconds = 300 ticks at 60fps)
    if let Some(ref err) = app.last_price_error {
        let age = app.tick_count.saturating_sub(app.last_price_error_tick);
        if age < 300 {
            spans.push(Span::styled(" ⚠ ", Style::default().fg(t.stale_yellow)));
            // Truncate long error messages for the status bar
            let display_err = if err.len() > 50 { &err[..50] } else { err.as_str() };
            spans.push(Span::styled(display_err, Style::default().fg(t.stale_yellow)));
        }
    }


    // Keystroke echo — flash last key for ~0.3s (18 ticks at 60fps)
    if !app.last_key_display.is_empty() {
        let key_age = app.tick_count.saturating_sub(app.last_key_tick);
        if key_age < 18 {
            // Fade from text_secondary to text_muted over the display period
            let fade_color = if key_age < 9 { t.text_secondary } else { t.text_muted };
            spans.push(Span::styled(
                format!(" [{}]", app.last_key_display),
                Style::default().fg(fade_color),
            ));
        }
    }
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
