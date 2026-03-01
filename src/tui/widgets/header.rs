use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App, ViewMode};
use crate::config::PortfolioMode;
use crate::tui::theme;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let now = chrono::Utc::now().format("%H:%M UTC");
    let privacy = is_privacy_view(app);
    let pct_mode = app.portfolio_mode == PortfolioMode::Percentage;
    let t = &app.theme;

    let pos_style = if matches!(app.view_mode, ViewMode::Positions) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };

    let mut spans = vec![
        Span::styled(" pf", Style::default().fg(t.text_accent).bold()),
        Span::styled("tui", Style::default().fg(t.text_primary).bold()),
        Span::raw("  "),
        Span::styled("[1]", Style::default().fg(t.key_hint)),
        Span::styled("Pos", pos_style),
    ];

    if !pct_mode {
        let tx_style = if matches!(app.view_mode, ViewMode::Transactions) {
            Style::default().fg(t.text_primary).bold().underlined()
        } else {
            Style::default().fg(t.text_muted)
        };
        spans.push(Span::raw(" "));
        spans.push(Span::styled("[2]", Style::default().fg(t.key_hint)));
        spans.push(Span::styled("Tx", tx_style));

    // Markets tab — always visible
    let mkt_style = if matches!(app.view_mode, ViewMode::Markets) {
        Style::default().fg(t.text_primary).bold().underlined()
    } else {
        Style::default().fg(t.text_muted)
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled("[3]", Style::default().fg(t.key_hint)));
    spans.push(Span::styled("Mkt", mkt_style));
    }

    if !privacy {
        let total = app.total_value;
        let cost = app.total_cost;
        let gain = total - cost;
        let gain_pct = if cost > dec!(0) {
            (gain / cost) * dec!(100)
        } else {
            dec!(0)
        };
        let gain_color = if gain > dec!(0) {
            t.gain_green
        } else if gain < dec!(0) {
            t.loss_red
        } else {
            t.neutral
        };

        let value_str = format_compact(total);
        let gain_str = format!("{:+.1}%", gain_pct);

        // Flash on value update
        let is_flashing = app.tick_count.saturating_sub(app.last_value_update_tick)
            < theme::FLASH_DURATION
            && app.last_value_update_tick > 0;

        let value_style = if is_flashing {
            Style::default()
                .fg(t.surface_0)
                .bg(t.text_accent)
                .bold()
        } else {
            Style::default().fg(t.text_primary).bold()
        };

        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("{}  ", value_str), value_style));
        spans.push(Span::styled(gain_str, Style::default().fg(gain_color)));
    } else {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("[% view]", Style::default().fg(t.text_muted)));
    }

    spans.push(Span::styled(" | ", Style::default().fg(t.text_muted)));
    spans.push(Span::styled(
        format!("{}", now),
        Style::default().fg(t.text_muted),
    ));

    // Theme indicator
    spans.push(Span::styled(
        format!("  {}", app.theme_name),
        Style::default().fg(t.text_muted),
    ));

    let line = Line::from(spans);

    let header = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(t.border_subtle))
            .style(Style::default().bg(t.surface_2)),
    );

    frame.render_widget(header, area);
}

fn format_compact(v: rust_decimal::Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 1_000_000.0 {
        format!("${:.1}M", f / 1_000_000.0)
    } else if f.abs() >= 1_000.0 {
        format!("${:.1}k", f / 1_000.0)
    } else {
        format!("${:.0}", f)
    }
}
