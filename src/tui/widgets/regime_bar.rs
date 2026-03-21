//! Regime Health Bar widget — compact risk-on/risk-off gauge for the sidebar.
//!
//! Renders as:
//! ```text
//! ⚡ REGIME: RISK-OFF ████████░░ -6/9
//!    VIX 23.7↑  10Y 3.98↓  DXY 97.9↑  Cu/Au↓
//! ```

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::regime::RegimeScore;
use crate::tui::theme::{self, Theme};

/// Height of the regime bar widget (border top + gauge line + signal line + border bottom).
pub const REGIME_BAR_HEIGHT: u16 = 4;

/// Render the regime health bar into the given area.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let regime = &app.regime_score;

    if !regime.has_data() {
        // Not enough data — render a minimal placeholder
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(theme::BORDER_INACTIVE)
            .border_style(Style::default().fg(t.border_inactive))
            .title(Span::styled(" Regime ", Style::default().fg(t.text_muted)))
            .style(Style::default().bg(t.surface_1));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height > 0 {
            let text = Paragraph::new(Line::from(Span::styled(
                "Waiting for data...",
                Style::default().fg(t.text_muted),
            )));
            frame.render_widget(text, inner);
        }
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Regime ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width < 10 {
        return;
    }

    // Line 1: gauge bar + label + score
    let gauge_line = build_gauge_line(regime, t, inner.width);
    frame.render_widget(
        Paragraph::new(gauge_line),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    // Line 2: signal summary (if room)
    if inner.height >= 2 {
        let signal_line = build_signal_line(regime, t, inner.width);
        frame.render_widget(
            Paragraph::new(signal_line),
            Rect {
                x: inner.x,
                y: inner.y + 1,
                width: inner.width,
                height: 1,
            },
        );
    }
}

/// Build the main gauge line: "⚡ RISK-ON ████████░░ +7/9"
fn build_gauge_line<'a>(regime: &RegimeScore, t: &'a Theme, width: u16) -> Line<'a> {
    let label = regime.label();
    let (label_color, icon) = regime_color_and_icon(regime, t);

    let score_str = format!("{:+}/{}", regime.total, regime.active_count);

    // Compute gauge bar width: total width - icon - label - spaces - score
    let fixed_chars = 2 + label.len() + 1 + 1 + score_str.len(); // "⚡ " + label + " " + gauge + " " + score
    let gauge_width = if width as usize > fixed_chars + 4 {
        (width as usize - fixed_chars - 1).min(18)
    } else {
        6 // minimum
    };

    // Build gauge: filled + empty blocks
    // Map total (-9..+9) to 0..gauge_width
    let max_signals = 9i8;
    let normalized =
        ((regime.total as f32 + max_signals as f32) / (2.0 * max_signals as f32)).clamp(0.0, 1.0);
    let filled = (normalized * gauge_width as f32).round() as usize;
    let empty = gauge_width.saturating_sub(filled);

    let filled_str: String = "█".repeat(filled);
    let empty_str: String = "░".repeat(empty);

    // Gauge color: gradient from loss_red through neutral to gain_green
    let gauge_color = theme::gradient_3(t.loss_red, t.neutral, t.gain_green, normalized);

    Line::from(vec![
        Span::styled(
            format!("{} ", icon),
            Style::default().fg(label_color).bold(),
        ),
        Span::styled(
            format!("{} ", label),
            Style::default().fg(label_color).bold(),
        ),
        Span::styled(filled_str, Style::default().fg(gauge_color)),
        Span::styled(empty_str, Style::default().fg(t.text_muted)),
        Span::styled(
            format!(" {}", score_str),
            Style::default().fg(t.text_secondary),
        ),
    ])
}

/// Build the signal detail line: compact summary of individual signals.
fn build_signal_line<'a>(regime: &RegimeScore, t: &'a Theme, max_width: u16) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut current_width: usize = 0;
    let sep = "  ";

    for (i, signal) in regime.signals.iter().enumerate() {
        if signal.score == 0 {
            continue; // Skip signals with no data
        }

        let label = &signal.label;
        let needed = if i > 0 && !spans.is_empty() {
            sep.len() + label.len()
        } else {
            label.len()
        };

        if current_width + needed > max_width as usize {
            break; // Don't overflow
        }

        if !spans.is_empty() {
            spans.push(Span::styled(sep, Style::default().fg(t.text_muted)));
            current_width += sep.len();
        }

        let color = if signal.score > 0 {
            t.gain_green
        } else {
            t.loss_red
        };

        spans.push(Span::styled(label.to_string(), Style::default().fg(color)));
        current_width += label.len();
    }

    Line::from(spans)
}

/// Returns the color and icon for the regime label.
fn regime_color_and_icon(regime: &RegimeScore, t: &Theme) -> (Color, &'static str) {
    match regime.total {
        5..=9 => (t.gain_green, "⚡"),
        2..=4 => (t.gain_green, "↑"),
        -1..=1 => (t.neutral, "→"),
        -4..=-2 => (t.loss_red, "↓"),
        _ => (t.loss_red, "⚡"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regime::{RegimeScore, RegimeSignal};

    fn make_regime(total: i8, active: u8) -> RegimeScore {
        RegimeScore {
            signals: vec![
                RegimeSignal {
                    name: "VIX level",
                    label: "VIX 15.0✓".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "VIX dir",
                    label: "VIX 15.0↓".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "10Y dir",
                    label: "10Y 4.5↑".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "2Y-10Y",
                    label: "2s10s +0.50".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "DXY dir",
                    label: "DXY 97.9↓".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "Au/SPX",
                    label: "Au/SPX↓".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "BTC/SPX",
                    label: "BTC/SPX 0.85".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "HY sprd",
                    label: "HY sprd↑".into(),
                    score: 1,
                },
                RegimeSignal {
                    name: "Cu/Au",
                    label: "Cu/Au↑".into(),
                    score: 1,
                },
            ],
            total,
            active_count: active,
        }
    }

    #[test]
    fn regime_bar_height_is_four() {
        assert_eq!(REGIME_BAR_HEIGHT, 4);
    }

    #[test]
    fn gauge_line_contains_label() {
        let t = theme::midnight();
        let regime = make_regime(5, 9);
        let line = build_gauge_line(&regime, &t, 60);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            text.contains("RISK-ON"),
            "gauge should show RISK-ON: {}",
            text
        );
    }

    #[test]
    fn gauge_line_risk_off() {
        let t = theme::midnight();
        let regime = make_regime(-6, 9);
        let _line = build_gauge_line(&regime.clone(), &t, 60);
        // The label method returns RISK-OFF for -6
        assert_eq!(regime.label(), "RISK-OFF");
    }

    #[test]
    fn signal_line_shows_active_signals() {
        let t = theme::midnight();
        let regime = make_regime(7, 9);
        let line = build_signal_line(&regime, &t, 80);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            text.contains("VIX"),
            "signal line should contain VIX: {}",
            text
        );
    }

    #[test]
    fn signal_line_respects_width() {
        let t = theme::midnight();
        let regime = make_regime(7, 9);
        let line = build_signal_line(&regime, &t, 20);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        // Should be truncated to fit within 20 chars
        assert!(text.len() <= 22); // Allow slight overshoot from last span added
    }

    #[test]
    fn regime_color_risk_on() {
        let t = theme::midnight();
        let regime = make_regime(7, 9);
        let (color, icon) = regime_color_and_icon(&regime, &t);
        assert_eq!(color, t.gain_green);
        assert_eq!(icon, "⚡");
    }

    #[test]
    fn regime_color_neutral() {
        let t = theme::midnight();
        let regime = make_regime(0, 5);
        let (color, icon) = regime_color_and_icon(&regime, &t);
        assert_eq!(color, t.neutral);
        assert_eq!(icon, "→");
    }
}
