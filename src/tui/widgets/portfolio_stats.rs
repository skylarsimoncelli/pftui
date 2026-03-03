use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::models::asset::AssetCategory;
use crate::tui::views::positions::compute_change_pct;

/// Height of the portfolio stats section (no border — inline content).
pub const STATS_HEIGHT: u16 = 3;

/// Render key portfolio stats: total positions, top performer, worst performer.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    if area.height == 0 || area.width < 10 {
        return;
    }

    let mut lines = Vec::new();

    // Line 1: position count + category breakdown
    let position_count = app
        .positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .count();
    let cat_count = app
        .positions
        .iter()
        .filter(|p| p.category != AssetCategory::Cash)
        .map(|p| p.category)
        .collect::<std::collections::HashSet<_>>()
        .len();

    lines.push(Line::from(vec![
        Span::styled(
            format!(" {} positions", position_count),
            Style::default().fg(t.text_secondary),
        ),
        Span::styled(" · ", Style::default().fg(t.border_subtle)),
        Span::styled(
            format!("{} categories", cat_count),
            Style::default().fg(t.text_muted),
        ),
    ]));

    if !is_privacy_view(app) {
        let (top, worst) = find_top_worst_performers(app);

        if let Some((sym, pct)) = top {
            lines.push(build_performer_line(" ▲ Top", &sym, pct, t, true));
        } else {
            lines.push(Line::from(Span::styled(
                " ▲ Top: —",
                Style::default().fg(t.text_muted),
            )));
        }

        if let Some((sym, pct)) = worst {
            lines.push(build_performer_line(" ▼ Bot", &sym, pct, t, false));
        } else {
            lines.push(Line::from(Span::styled(
                " ▼ Bot: —",
                Style::default().fg(t.text_muted),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            " ▲ Top: ••••",
            Style::default().fg(t.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            " ▼ Bot: ••••",
            Style::default().fg(t.text_muted),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Build a styled performer line: " ▲ Top: BTC +3.5%"
fn build_performer_line<'a>(
    prefix: &str,
    symbol: &str,
    pct: Decimal,
    t: &crate::tui::theme::Theme,
    is_top: bool,
) -> Line<'a> {
    let color = if is_top { t.gain_green } else { t.loss_red };
    let sign = if pct > dec!(0) { "+" } else { "" };
    let pct_str = format!("{}{:.1}%", sign, pct.round_dp(1));

    Line::from(vec![
        Span::styled(
            format!("{}: ", prefix),
            Style::default().fg(t.text_muted),
        ),
        Span::styled(
            format!("{} ", symbol),
            Style::default().fg(t.text_primary).bold(),
        ),
        Span::styled(pct_str, Style::default().fg(color)),
    ])
}

/// A symbol with its day change percentage.
pub type Performer = Option<(String, Decimal)>;

/// Find the top (best day %) and worst (worst day %) performers from portfolio positions.
/// Returns (top, worst) where each is Option<(symbol, day_change_pct)>.
pub fn find_top_worst_performers(app: &App) -> (Performer, Performer) {
    let mut best: Option<(String, Decimal)> = None;
    let mut worst: Option<(String, Decimal)> = None;

    for pos in &app.positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }

        let day_pct = match compute_change_pct(app, &pos.symbol) {
            Some(p) => p,
            None => continue,
        };

        match &best {
            Some((_, best_pct)) => {
                if day_pct > *best_pct {
                    best = Some((pos.symbol.clone(), day_pct));
                }
            }
            None => best = Some((pos.symbol.clone(), day_pct)),
        }

        match &worst {
            Some((_, worst_pct)) => {
                if day_pct < *worst_pct {
                    worst = Some((pos.symbol.clone(), day_pct));
                }
            }
            None => worst = Some((pos.symbol.clone(), day_pct)),
        }
    }

    (best, worst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_height_constant() {
        assert_eq!(STATS_HEIGHT, 3);
    }

    #[test]
    fn build_performer_line_top() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let line = build_performer_line(" ▲ Top", "BTC", dec!(3.5), &t, true);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("BTC"));
        assert!(text.contains("+3.5%"));
    }

    #[test]
    fn build_performer_line_worst() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let line = build_performer_line(" ▼ Bot", "ETH", dec!(-2.1), &t, false);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("ETH"));
        assert!(text.contains("-2.1%"));
    }

    #[test]
    fn build_performer_line_zero() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let line = build_performer_line(" ▲ Top", "SOL", dec!(0), &t, true);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("SOL"));
        assert!(text.contains("0.0%"));
    }
}
