use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::models::asset::AssetCategory;
use crate::tui::views::positions::compute_change_pct;

/// Height of the portfolio stats section (no border — inline content).
pub const STATS_HEIGHT: u16 = 5;

/// Render key portfolio stats: total positions, top performer, worst performer, and performance metrics.
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

        // Line 4: Performance metrics (1D, 1W, 1M, YTD)
        lines.push(build_performance_line(app, t));

        // Line 5: Sparkline of portfolio value (last 30 days)
        lines.push(build_sparkline(app, t));
    } else {
        lines.push(Line::from(Span::styled(
            " ▲ Top: ••••",
            Style::default().fg(t.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            " ▼ Bot: ••••",
            Style::default().fg(t.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            " Performance: ••••",
            Style::default().fg(t.text_muted),
        )));
        lines.push(Line::from(Span::styled(
            " ••••••••",
            Style::default().fg(t.text_muted),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Build performance metrics line: " 1D: +2.1%  1W: +5.3%  1M: -1.2%  YTD: +8.5%"
fn build_performance_line<'a>(app: &App, t: &crate::tui::theme::Theme) -> Line<'a> {
    let perf = compute_performance_metrics(app);
    let mut spans = vec![Span::styled(" ", Style::default())];

    for (label, pct_opt) in [
        ("1D", perf.day_1),
        ("1W", perf.week_1),
        ("1M", perf.month_1),
        ("YTD", perf.ytd),
    ] {
        if let Some(pct) = pct_opt {
            let color = if pct > dec!(0) {
                t.gain_green
            } else if pct < dec!(0) {
                t.loss_red
            } else {
                t.text_muted
            };
            let sign = if pct > dec!(0) { "+" } else { "" };
            let pct_str = format!("{}{:.1}%", sign, pct.round_dp(1));
            spans.push(Span::styled(
                format!("{}: ", label),
                Style::default().fg(t.text_muted),
            ));
            spans.push(Span::styled(pct_str, Style::default().fg(color)));
            spans.push(Span::styled("  ", Style::default()));
        } else {
            spans.push(Span::styled(
                format!("{}: —  ", label),
                Style::default().fg(t.text_muted),
            ));
        }
    }

    Line::from(spans)
}

/// Build a braille sparkline of portfolio value (last 30 days).
fn build_sparkline<'a>(app: &App, t: &crate::tui::theme::Theme) -> Line<'a> {
    let history = &app.portfolio_value_history;
    if history.is_empty() {
        return Line::from(Span::styled(
            " Portfolio history: no data",
            Style::default().fg(t.text_muted),
        ));
    }

    // Take last 30 points
    let points: Vec<f64> = history
        .iter()
        .rev()
        .take(30)
        .map(|(_, val)| {
            val.to_string()
                .parse::<f64>()
                .unwrap_or(0.0)
        })
        .rev()
        .collect();

    if points.is_empty() {
        return Line::from(Span::styled(
            " Portfolio history: no data",
            Style::default().fg(t.text_muted),
        ));
    }

    let sparkline_str = render_braille_sparkline(&points);
    let color = match history.first().zip(history.last()) {
        Some(((_, first), (_, last))) if last > first => t.gain_green,
        Some(((_, first), (_, last))) if last < first => t.loss_red,
        _ => t.text_muted,
    };

    Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(sparkline_str, Style::default().fg(color)),
    ])
}

/// Render a braille sparkline from f64 data points.
/// Uses Unicode braille characters (U+2800..U+28FF) to pack 2x4 pixel grid per char.
fn render_braille_sparkline(points: &[f64]) -> String {
    if points.is_empty() {
        return String::new();
    }

    let min = points.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = points.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = if max - min < 0.01 { 1.0 } else { max - min };

    // Normalize to 0-7 (8 vertical levels using braille)
    let normalized: Vec<u8> = points
        .iter()
        .map(|&v| {
            let norm = ((v - min) / range * 7.0).round();
            norm.clamp(0.0, 7.0) as u8
        })
        .collect();

    // Map each value to a braille character
    // Braille vertical dots: ⠁⠂⠄⡀⡁⡂⡄⣀ (heights 0-7)
    let braille_chars = ['⠀', '⠁', '⠃', '⠇', '⡇', '⡏', '⡟', '⣿'];
    normalized
        .iter()
        .map(|&h| braille_chars[h as usize])
        .collect()
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

/// Performance metrics computed from portfolio value history.
#[derive(Debug, Clone, Default)]
struct PerformanceMetrics {
    day_1: Option<Decimal>,
    week_1: Option<Decimal>,
    month_1: Option<Decimal>,
    ytd: Option<Decimal>,
}

/// Compute performance metrics from portfolio value history.
fn compute_performance_metrics(app: &App) -> PerformanceMetrics {
    let history = &app.portfolio_value_history;
    if history.is_empty() {
        return PerformanceMetrics::default();
    }

    let current = match history.last() {
        Some((_, v)) => *v,
        None => return PerformanceMetrics::default(),
    };

    let today_str = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let year_start = format!("{}-01-01", &today_str[..4]);

    // Helper: find value on or before a given date
    let value_at = |target_date: &str| -> Option<Decimal> {
        history
            .iter()
            .rev()
            .find(|(date, _)| date.as_str() <= target_date)
            .map(|(_, v)| *v)
    };

    // Helper: compute return percentage
    let pct_return = |old: Option<Decimal>| -> Option<Decimal> {
        old.and_then(|o| {
            if o > dec!(0) {
                Some(((current - o) / o) * dec!(100))
            } else {
                None
            }
        })
    };

    // 1D: value 1 day ago
    let day_1 = if history.len() >= 2 {
        let yesterday = history.get(history.len().saturating_sub(2)).map(|(_, v)| *v);
        pct_return(yesterday)
    } else {
        None
    };

    // 1W: value 7 days ago
    let week_1 = {
        let target = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(7))
            .map(|d| d.format("%Y-%m-%d").to_string());
        target.and_then(|t| pct_return(value_at(&t)))
    };

    // 1M: value 30 days ago
    let month_1 = {
        let target = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(30))
            .map(|d| d.format("%Y-%m-%d").to_string());
        target.and_then(|t| pct_return(value_at(&t)))
    };

    // YTD: value at year start
    let ytd = pct_return(value_at(&year_start));

    PerformanceMetrics {
        day_1,
        week_1,
        month_1,
        ytd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_height_constant() {
        assert_eq!(STATS_HEIGHT, 5);
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

    #[test]
    fn render_braille_sparkline_basic() {
        let points = vec![1.0, 2.0, 3.0, 2.0, 1.0];
        let sparkline = render_braille_sparkline(&points);
        assert!(!sparkline.is_empty());
        assert_eq!(sparkline.chars().count(), 5);
    }

    #[test]
    fn render_braille_sparkline_flat() {
        let points = vec![5.0, 5.0, 5.0];
        let sparkline = render_braille_sparkline(&points);
        assert_eq!(sparkline.chars().count(), 3);
        // When all values are the same, they should all render at the same height
        let chars: Vec<char> = sparkline.chars().collect();
        assert_eq!(chars[0], chars[1]);
        assert_eq!(chars[1], chars[2]);
    }

    #[test]
    fn render_braille_sparkline_empty() {
        let points: Vec<f64> = vec![];
        let sparkline = render_braille_sparkline(&points);
        assert!(sparkline.is_empty());
    }
}
