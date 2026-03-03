use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset_names::{infer_category, resolve_name};
use crate::tui::theme;
use crate::tui::views::position_detail::format_money;
use crate::tui::views::positions::compute_52w_range;
use crate::tui::widgets::price_chart;

/// State for the asset detail popup opened from search overlay.
#[derive(Debug, Clone)]
pub struct AssetDetailState {
    pub symbol: String,
    /// Scroll offset for the content lines.
    pub scroll: usize,
}

/// Renders a large centered popup with all available info about any asset.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let state = match &app.asset_detail {
        Some(s) => s,
        None => return,
    };

    let t = &app.theme;
    let symbol = &state.symbol;

    let lines = build_lines(symbol, app);
    let total_lines = lines.len();

    // Large popup — 85% width, 85% height
    let width = (area.width * 85 / 100).clamp(50, 100);
    let height = (area.height * 85 / 100).clamp(12, 50);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    // Shadow
    theme::render_popup_shadow(frame, popup_area, area, t);
    frame.render_widget(Clear, popup_area);

    let visible_lines = height.saturating_sub(2) as usize;
    let scroll = state.scroll.min(total_lines.saturating_sub(visible_lines));

    let displayed: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(visible_lines)
        .collect();

    // Title
    let name = lookup_name(symbol);
    let title = if name.is_empty() {
        format!(" ◆ {} ", symbol)
    } else {
        format!(" ◆ {} ({}) ", name, symbol)
    };

    let scroll_hint = if total_lines > visible_lines {
        format!(" {}/{} ", scroll + 1, total_lines.saturating_sub(visible_lines) + 1)
    } else {
        String::new()
    };

    let detail = Paragraph::new(displayed).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(theme::BORDER_POPUP)
            .border_style(Style::default().fg(t.border_accent))
            .style(Style::default().bg(t.surface_2))
            .title(Span::styled(
                title,
                Style::default().fg(t.text_accent).bold(),
            ))
            .title(
                Line::from(vec![
                    Span::styled(
                        scroll_hint,
                        Style::default().fg(t.text_muted),
                    ),
                    Span::styled(
                        " Esc to close ",
                        Style::default().fg(t.text_muted),
                    ),
                ])
                .alignment(Alignment::Right),
            ),
    );

    frame.render_widget(detail, popup_area);
}

fn lookup_name(symbol: &str) -> String {
    resolve_name(symbol)
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 10000.0 {
        format!("{:.0}", f)
    } else if f.abs() >= 100.0 {
        format!("{:.1}", f)
    } else if f.abs() >= 1.0 {
        format!("{:.2}", f)
    } else {
        format!("{:.4}", f)
    }
}

/// Build all the content lines for the asset detail popup.
pub fn build_lines<'a>(symbol: &str, app: &'a App) -> Vec<Line<'a>> {
    let t = &app.theme;
    let category = infer_category(symbol);
    let cat_color = t.category_color(category);
    let name = lookup_name(symbol);

    let mut lines: Vec<Line> = Vec::with_capacity(40);
    lines.push(Line::from(""));

    // ── Asset Info ──
    lines.push(section_header("  Asset", t.text_accent));
    lines.push(sep_line(t.border_subtle, 80));

    lines.push(Line::from(vec![
        Span::styled("  Symbol      ", Style::default().fg(t.text_secondary)),
        Span::styled(
            symbol.to_string(),
            Style::default().fg(t.text_primary).bold(),
        ),
    ]));
    if !name.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Name        ", Style::default().fg(t.text_secondary)),
            Span::styled(name.clone(), Style::default().fg(t.text_primary)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  Category    ", Style::default().fg(t.text_secondary)),
        Span::styled(
            format!("{}", category),
            Style::default().fg(cat_color).bold(),
        ),
    ]));

    // Portfolio/Watchlist status
    let in_portfolio = app.positions.iter().any(|p| p.symbol == symbol);
    let in_watchlist = app.watchlist_entries.iter().any(|w| w.symbol == symbol);
    let status_str = if in_portfolio {
        "◆ In Portfolio"
    } else if in_watchlist {
        "○ In Watchlist"
    } else {
        "  Not in portfolio"
    };
    let status_color = if in_portfolio {
        t.gain_green
    } else if in_watchlist {
        t.text_accent
    } else {
        t.text_muted
    };
    lines.push(Line::from(vec![
        Span::styled("  Status      ", Style::default().fg(t.text_secondary)),
        Span::styled(status_str.to_string(), Style::default().fg(status_color)),
    ]));
    lines.push(Line::from(""));

    // ── Price ──
    lines.push(section_header("  Price", t.text_accent));
    lines.push(sep_line(t.border_subtle, 80));

    let current_price = app.prices.get(symbol).copied();
    let price_str = current_price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string());
    let currency = &app.base_currency;

    lines.push(Line::from(vec![
        Span::styled("  Current     ", Style::default().fg(t.text_secondary)),
        Span::styled(
            format!("{} {}", price_str, currency),
            Style::default().fg(t.text_primary).bold(),
        ),
    ]));

    // Day change from history
    let history = app.price_history.get(symbol);
    if let Some(hist) = history {
        if hist.len() >= 2 {
            let latest = hist.last().map(|h| h.close).unwrap_or(dec!(0));
            let prev = hist.get(hist.len() - 2).map(|h| h.close).unwrap_or(dec!(0));
            if prev > dec!(0) {
                let change = latest - prev;
                let change_pct = (change / prev) * dec!(100);
                let (sign, color) = if change > dec!(0) {
                    ("+", t.gain_green)
                } else if change < dec!(0) {
                    ("", t.loss_red)
                } else {
                    ("", t.text_muted)
                };
                lines.push(Line::from(vec![
                    Span::styled("  24h Change  ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{}{} {} ({}{:.2}%)", sign, format_price(change), currency, sign, change_pct),
                        Style::default().fg(color).bold(),
                    ),
                ]));
            }
        }

        // 7-day change
        if hist.len() >= 7 {
            let latest = hist.last().map(|h| h.close).unwrap_or(dec!(0));
            let prev7 = hist.get(hist.len().saturating_sub(7)).map(|h| h.close).unwrap_or(dec!(0));
            if prev7 > dec!(0) {
                let change_pct = ((latest - prev7) / prev7) * dec!(100);
                let (sign, color) = if change_pct > dec!(0) {
                    ("+", t.gain_green)
                } else if change_pct < dec!(0) {
                    ("", t.loss_red)
                } else {
                    ("", t.text_muted)
                };
                lines.push(Line::from(vec![
                    Span::styled("  7D Change   ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{}{:.2}%", sign, change_pct),
                        Style::default().fg(color),
                    ),
                ]));
            }
        }

        // 30-day change
        if hist.len() >= 30 {
            let latest = hist.last().map(|h| h.close).unwrap_or(dec!(0));
            let prev30 = hist.get(hist.len().saturating_sub(30)).map(|h| h.close).unwrap_or(dec!(0));
            if prev30 > dec!(0) {
                let change_pct = ((latest - prev30) / prev30) * dec!(100);
                let (sign, color) = if change_pct > dec!(0) {
                    ("+", t.gain_green)
                } else if change_pct < dec!(0) {
                    ("", t.loss_red)
                } else {
                    ("", t.text_muted)
                };
                lines.push(Line::from(vec![
                    Span::styled("  30D Change  ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{}{:.2}%", sign, change_pct),
                        Style::default().fg(color),
                    ),
                ]));
            }
        }
    }

    // 52-week range
    if let Some(range) = compute_52w_range(
        history.map(|v| v.as_slice()).unwrap_or(&[]),
        current_price,
    ) {
        let high_str = format_price(range.high);
        let low_str = format_price(range.low);
        lines.push(Line::from(vec![
            Span::styled("  52W Range   ", Style::default().fg(t.text_secondary)),
            Span::styled(
                format!("{} — {}", low_str, high_str),
                Style::default().fg(t.text_primary),
            ),
        ]));
        let pct_text = if range.from_high_pct.abs() < 0.05 {
            "At 52W high".to_string()
        } else {
            format!("{:+.1}% from high", range.from_high_pct)
        };
        let pct_color = if range.from_high_pct.abs() < 0.05 {
            t.gain_green
        } else if range.from_high_pct > -10.0 {
            t.text_secondary
        } else {
            t.loss_red
        };
        lines.push(Line::from(vec![
            Span::styled("              ", Style::default().fg(t.text_secondary)),
            Span::styled(pct_text, Style::default().fg(pct_color)),
        ]));
    }

    lines.push(Line::from(""));

    // ── Chart ──
    if let Some(hist) = history {
        if hist.len() >= 2 {
            lines.push(section_header("  Chart", t.text_accent));
            lines.push(sep_line(t.border_subtle, 80));

            // Use popup width minus border/padding: 2 border + 2 left padding = 4
            // Popup is 85% of screen width, clamped 50-100. Use a reasonable chart width.
            let chart_width = 70_usize; // fits within the popup comfortably
            let chart_height = 8_usize; // 8 rows of braille = 32 dot-rows of resolution

            let chart_lines = price_chart::render_braille_lines(hist, chart_width, chart_height, t);
            if !chart_lines.is_empty() {
                for line in chart_lines {
                    lines.push(line);
                }
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("Insufficient chart data", Style::default().fg(t.text_muted)),
                ]));
            }

            lines.push(Line::from(""));
        }
    }

    // ── Technicals (SMA, Bollinger) ──
    if let Some(hist) = history {
        if hist.len() >= 20 {
            lines.push(section_header("  Technicals", t.text_accent));
            lines.push(sep_line(t.border_subtle, 80));

            let closes: Vec<f64> = hist
                .iter()
                .map(|h| h.close.to_string().parse::<f64>().unwrap_or(0.0))
                .collect();

            // SMA 20
            let sma20 = simple_moving_average(&closes, 20);
            if let Some(sma) = sma20 {
                let current_f: f64 = current_price
                    .map(|p| p.to_string().parse::<f64>().unwrap_or(0.0))
                    .unwrap_or(0.0);
                let above = current_f > sma;
                let indicator = if above { "▲" } else { "▼" };
                let ind_color = if above { t.gain_green } else { t.loss_red };
                lines.push(Line::from(vec![
                    Span::styled("  SMA(20)     ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}", sma),
                        Style::default().fg(t.text_primary),
                    ),
                    Span::styled(
                        format!(" {}", indicator),
                        Style::default().fg(ind_color),
                    ),
                ]));
            }

            // SMA 50
            if closes.len() >= 50 {
                let sma50 = simple_moving_average(&closes, 50);
                if let Some(sma) = sma50 {
                    let current_f: f64 = current_price
                        .map(|p| p.to_string().parse::<f64>().unwrap_or(0.0))
                        .unwrap_or(0.0);
                    let above = current_f > sma;
                    let indicator = if above { "▲" } else { "▼" };
                    let ind_color = if above { t.gain_green } else { t.loss_red };
                    lines.push(Line::from(vec![
                        Span::styled("  SMA(50)     ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:.2}", sma),
                            Style::default().fg(t.text_primary),
                        ),
                        Span::styled(
                            format!(" {}", indicator),
                            Style::default().fg(ind_color),
                        ),
                    ]));
                }
            }

            // Bollinger Band width (20-period, 2 std dev)
            let bb = bollinger_band_width(&closes, 20);
            if let Some((upper, lower, width)) = bb {
                lines.push(Line::from(vec![
                    Span::styled("  BB Upper    ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}", upper),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  BB Lower    ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}", lower),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  BB Width    ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}%", width),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
            }

            // RSI (14-period)
            if closes.len() >= 15 {
                let rsi = compute_rsi(&closes, 14);
                if let Some(rsi_val) = rsi {
                    let rsi_color = if rsi_val > 70.0 {
                        t.loss_red
                    } else if rsi_val < 30.0 {
                        t.gain_green
                    } else {
                        t.text_primary
                    };
                    let label = if rsi_val > 70.0 {
                        " Overbought"
                    } else if rsi_val < 30.0 {
                        " Oversold"
                    } else {
                        ""
                    };
                    lines.push(Line::from(vec![
                        Span::styled("  RSI(14)     ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:.1}", rsi_val),
                            Style::default().fg(rsi_color).bold(),
                        ),
                        Span::styled(
                            label.to_string(),
                            Style::default().fg(rsi_color),
                        ),
                    ]));
                }
            }

            lines.push(Line::from(""));
        }
    }

    // ── Portfolio Context ──
    if in_portfolio {
        if let Some(pos) = app.positions.iter().find(|p| p.symbol == symbol) {
            let privacy = crate::app::is_privacy_view(app);

            lines.push(section_header("  Portfolio", t.text_accent));
            lines.push(sep_line(t.border_subtle, 80));

            if !privacy {
                lines.push(Line::from(vec![
                    Span::styled("  Quantity    ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format_qty(pos.quantity),
                        Style::default().fg(t.text_primary),
                    ),
                ]));

                if pos.avg_cost > dec!(0) {
                    lines.push(Line::from(vec![
                        Span::styled("  Avg Cost    ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{} {}", format_price(pos.avg_cost), currency),
                            Style::default().fg(t.text_primary),
                        ),
                    ]));
                }

                if let Some(val) = pos.current_value {
                    lines.push(Line::from(vec![
                        Span::styled("  Value       ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{} {}", format_money(val), currency),
                            Style::default().fg(t.text_primary).bold(),
                        ),
                    ]));
                }

                if let Some(gain) = pos.gain {
                    let gain_f: f64 = gain.to_string().parse().unwrap_or(0.0);
                    let gain_color = theme::gain_intensity_color(t, gain_f);
                    let sign = if gain >= dec!(0) { "+" } else { "" };
                    lines.push(Line::from(vec![
                        Span::styled("  Gain        ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{}{} {}", sign, format_money(gain), currency),
                            Style::default().fg(gain_color).bold(),
                        ),
                    ]));
                }

                if let Some(gain_pct) = pos.gain_pct {
                    let gain_f: f64 = gain_pct.to_string().parse().unwrap_or(0.0);
                    let gain_color = theme::gain_intensity_color(t, gain_f);
                    lines.push(Line::from(vec![
                        Span::styled("  Gain %      ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:+.2}%", gain_pct),
                            Style::default().fg(gain_color).bold(),
                        ),
                    ]));
                }
            }

            if let Some(alloc) = pos.allocation_pct {
                lines.push(Line::from(vec![
                    Span::styled("  Allocation  ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.1}%", alloc),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
            }

            lines.push(Line::from(""));
        }
    } else if in_watchlist {
        lines.push(section_header("  Watchlist", t.text_accent));
        lines.push(sep_line(t.border_subtle, 80));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().fg(t.text_secondary)),
            Span::styled(
                "○ Watching".to_string(),
                Style::default().fg(t.text_accent),
            ),
        ]));
        lines.push(Line::from(""));
    }

    // ── Footer ──
    lines.push(Line::from(Span::styled(
        "  Esc to close · j/k to scroll",
        Style::default().fg(t.text_muted),
    )));
    lines.push(Line::from(""));

    lines
}

fn section_header(title: &str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default().bold().fg(color),
    ))
}

fn sep_line(color: Color, width: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {}", "─".repeat(width.saturating_sub(2))),
        Style::default().fg(color),
    ))
}

fn format_qty(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f >= 100_000.0 {
        format!("{:.1}k", f / 1000.0)
    } else if f >= 1000.0 || f == f.floor() {
        format!("{:.0}", f)
    } else {
        format!("{:.2}", f)
    }
}

/// Compute simple moving average of the last `period` values.
fn simple_moving_average(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period {
        return None;
    }
    let slice = &values[values.len() - period..];
    let sum: f64 = slice.iter().sum();
    Some(sum / period as f64)
}

/// Compute Bollinger Band upper, lower, and width percentage.
fn bollinger_band_width(values: &[f64], period: usize) -> Option<(f64, f64, f64)> {
    if values.len() < period {
        return None;
    }
    let slice = &values[values.len() - period..];
    let mean: f64 = slice.iter().sum::<f64>() / period as f64;
    if mean.abs() < f64::EPSILON {
        return None;
    }
    let variance: f64 = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
    let std_dev = variance.sqrt();
    let upper = mean + 2.0 * std_dev;
    let lower = mean - 2.0 * std_dev;
    let width = ((upper - lower) / mean) * 100.0;
    Some((upper, lower, width))
}

/// Compute RSI (Relative Strength Index) from closing prices.
fn compute_rsi(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period + 1 {
        return None;
    }

    let mut gains = 0.0f64;
    let mut losses = 0.0f64;

    // Initial average gain/loss over first `period` changes
    for i in (values.len() - period)..values.len() {
        let change = values[i] - values[i - 1];
        if change > 0.0 {
            gains += change;
        } else {
            losses += change.abs();
        }
    }

    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;

    if avg_loss.abs() < f64::EPSILON {
        return Some(100.0);
    }

    let rs = avg_gain / avg_loss;
    Some(100.0 - (100.0 / (1.0 + rs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::asset::AssetCategory;
    use crate::models::position::Position;
    use crate::models::price::HistoryRecord;

    fn test_app() -> App {
        let config = Config::default();
        let db_path = std::path::PathBuf::from(":memory:");
        App::new(&config, db_path)
    }

    #[test]
    fn build_lines_contains_symbol() {
        let app = test_app();
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("AAPL"));
    }

    #[test]
    fn build_lines_contains_category() {
        let app = test_app();
        let lines = build_lines("BTC", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("crypto") || text.contains("Crypto"));
    }

    #[test]
    fn build_lines_shows_not_in_portfolio() {
        let app = test_app();
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("Not in portfolio"));
    }

    #[test]
    fn build_lines_shows_in_portfolio() {
        let mut app = test_app();
        app.positions.push(Position {
            symbol: "AAPL".to_string(),
            name: "Apple Inc".to_string(),
            category: AssetCategory::Equity,
            quantity: dec!(10),
            avg_cost: dec!(150),
            total_cost: dec!(1500),
            currency: "USD".to_string(),
            current_price: Some(dec!(175)),
            current_value: Some(dec!(1750)),
            gain: Some(dec!(250)),
            gain_pct: Some(dec!(16.67)),
            allocation_pct: Some(dec!(25)),
        });
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("In Portfolio"));
        assert!(text.contains("Portfolio"));
    }

    #[test]
    fn build_lines_shows_price_when_available() {
        let mut app = test_app();
        app.prices.insert("AAPL".to_string(), dec!(175.50));
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        // format_price renders >= 100 with 1 decimal place
        assert!(text.contains("175.5"));
    }

    #[test]
    fn build_lines_shows_no_price_placeholder() {
        let app = test_app();
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("---"));
    }

    #[test]
    fn build_lines_shows_day_change() {
        let mut app = test_app();
        app.prices.insert("AAPL".to_string(), dec!(175));
        app.price_history.insert(
            "AAPL".to_string(),
            vec![
                HistoryRecord {
                    date: "2026-03-01".to_string(),
                    close: dec!(170),
                    volume: None,
                },
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(175),
                    volume: None,
                },
            ],
        );
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("24h Change"));
    }

    #[test]
    fn build_lines_shows_technicals_with_enough_history() {
        let mut app = test_app();
        app.prices.insert("AAPL".to_string(), dec!(175));
        // Need 20+ data points for SMA(20)
        let mut hist = Vec::new();
        for i in 0..25 {
            hist.push(HistoryRecord {
                date: format!("2026-02-{:02}", (i % 28) + 1),
                close: dec!(150) + Decimal::from(i),
                volume: None,
            });
        }
        app.price_history.insert("AAPL".to_string(), hist);
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("SMA(20)"));
    }

    #[test]
    fn sma_basic() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(simple_moving_average(&values, 5), Some(3.0));
    }

    #[test]
    fn sma_insufficient_data() {
        let values = vec![1.0, 2.0];
        assert_eq!(simple_moving_average(&values, 5), None);
    }

    #[test]
    fn rsi_all_gains() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
        let rsi = compute_rsi(&values, 14);
        assert!(rsi.is_some());
        assert!((rsi.unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn rsi_mixed() {
        let values: Vec<f64> = (0..20).map(|i| 100.0 + (i as f64 * 0.5).sin() * 5.0).collect();
        let rsi = compute_rsi(&values, 14);
        assert!(rsi.is_some());
        let v = rsi.unwrap();
        assert!(v >= 0.0 && v <= 100.0);
    }

    #[test]
    fn bollinger_basic() {
        let values: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        let bb = bollinger_band_width(&values, 20);
        assert!(bb.is_some());
        let (upper, lower, width) = bb.unwrap();
        assert!(upper > lower);
        assert!(width > 0.0);
    }

    #[test]
    fn scroll_state_default() {
        let state = AssetDetailState {
            symbol: "BTC".to_string(),
            scroll: 0,
        };
        assert_eq!(state.scroll, 0);
        assert_eq!(state.symbol, "BTC");
    }

    #[test]
    fn build_lines_shows_chart_with_enough_history() {
        let mut app = test_app();
        app.prices.insert("AAPL".to_string(), dec!(175));
        let mut hist = Vec::new();
        for i in 0..30 {
            hist.push(HistoryRecord {
                date: format!("2026-02-{:02}", (i % 28) + 1),
                close: dec!(150) + Decimal::from(i),
                volume: None,
            });
        }
        app.price_history.insert("AAPL".to_string(), hist);
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("Chart"), "Should contain Chart section header when history is available");
    }

    #[test]
    fn build_lines_no_chart_without_history() {
        let app = test_app();
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(!text.contains("Chart"), "Should not contain Chart section without history data");
    }

    #[test]
    fn build_lines_no_chart_with_single_record() {
        let mut app = test_app();
        app.prices.insert("AAPL".to_string(), dec!(175));
        app.price_history.insert(
            "AAPL".to_string(),
            vec![HistoryRecord {
                date: "2026-03-01".to_string(),
                close: dec!(170),
                volume: None,
            }],
        );
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(!text.contains("Chart"), "Should not show Chart section with only 1 record");
    }

    fn lines_to_string(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
