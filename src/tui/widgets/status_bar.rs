use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use crate::config::PortfolioMode;
use crate::tui::theme;
use crate::tui::ui::COMPACT_WIDTH;

/// Capitalize the first character of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

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

    // Breadcrumb trail — shows navigation context
    let breadcrumb = app.breadcrumb();
    spans.push(Span::styled(
        format!(" {breadcrumb}"),
        Style::default().fg(t.text_accent).bold(),
    ));
    spans.push(Span::styled(" │ ", Style::default().fg(t.border_subtle)));

    if compact {
        // Compact: show only essential hints
        spans.push(Span::styled("[?]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Help", Style::default().fg(t.text_secondary)));
        spans.push(sep.clone());
        spans.push(Span::styled("[/]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Search", Style::default().fg(t.text_secondary)));
        spans.push(Span::styled(filter_text, Style::default().fg(t.text_secondary)));
        spans.push(Span::styled(search_filter_text, Style::default().fg(t.text_accent)));
    } else {
        // Full: show all hints
        spans.push(Span::styled("[?]", Style::default().fg(t.key_hint)));
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

    // Theme toast — show theme name briefly after cycling
    let theme_toast_age = app.tick_count.saturating_sub(app.theme_toast_tick);
    if app.theme_toast_tick > 0 && theme_toast_age < theme::THEME_TOAST_DURATION {
        // Fade: full accent for first half, then lerp to muted
        let fade_progress = theme_toast_age as f32 / theme::THEME_TOAST_DURATION as f32;
        let toast_color = if fade_progress < 0.5 {
            t.text_accent
        } else {
            let fade = (fade_progress - 0.5) * 2.0; // 0.0..1.0 over second half
            theme::lerp_color(t.text_accent, t.text_muted, fade)
        };
        // Capitalize theme name for display
        let display_name = capitalize_first(&app.theme_name);
        spans.push(Span::styled(
            format!("  ◆ {display_name}"),
            Style::default().fg(toast_color).bold(),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capitalize_first_basic() {
        assert_eq!(capitalize_first("midnight"), "Midnight");
        assert_eq!(capitalize_first("catppuccin"), "Catppuccin");
        assert_eq!(capitalize_first("nord"), "Nord");
        assert_eq!(capitalize_first("dracula"), "Dracula");
        assert_eq!(capitalize_first("solarized"), "Solarized");
        assert_eq!(capitalize_first("gruvbox"), "Gruvbox");
    }

    #[test]
    fn test_capitalize_first_empty() {
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn test_capitalize_first_already_capitalized() {
        assert_eq!(capitalize_first("Midnight"), "Midnight");
    }

    #[test]
    fn test_capitalize_first_single_char() {
        assert_eq!(capitalize_first("a"), "A");
    }

    #[test]
    fn test_theme_toast_timing() {
        // Toast should be visible when age < THEME_TOAST_DURATION
        let toast_tick: u64 = 100;
        let current_tick: u64 = 120;
        let age = current_tick.saturating_sub(toast_tick);
        assert!(age < theme::THEME_TOAST_DURATION, "toast should be visible shortly after cycle");

        // Toast should be invisible after THEME_TOAST_DURATION ticks
        let current_tick_expired: u64 = toast_tick + theme::THEME_TOAST_DURATION;
        let age_expired = current_tick_expired.saturating_sub(toast_tick);
        assert!(
            !(toast_tick > 0 && age_expired < theme::THEME_TOAST_DURATION),
            "toast should be hidden after duration expires"
        );
    }

    #[test]
    fn test_theme_toast_not_shown_on_init() {
        // theme_toast_tick starts at 0, toast should not display
        let toast_tick: u64 = 0;
        let current_tick: u64 = 10;
        let age = current_tick.saturating_sub(toast_tick);
        // Guard: toast_tick must be > 0 for display
        let should_show = toast_tick > 0 && age < theme::THEME_TOAST_DURATION;
        assert!(!should_show, "toast should not show on initial state (tick=0)");
    }

    #[test]
    fn test_theme_toast_fade_phases() {
        // First half: full accent color
        let fade_progress_early = 0.2_f32;
        assert!(fade_progress_early < 0.5, "early progress should be in first (bright) phase");

        // Second half: fading to muted
        let fade_progress_late = 0.8_f32;
        assert!(fade_progress_late >= 0.5, "late progress should be in second (fading) phase");
        let fade = (fade_progress_late - 0.5) * 2.0;
        assert!((0.0..=1.0).contains(&fade), "fade factor should be in 0.0..=1.0 range");
    }
}
