use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::indicators;
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

    // ── Technicals (SMA, BB, RSI, MACD) ──
    if let Some(hist) = history {
        if hist.len() >= 20 {
            lines.push(section_header("  Technicals", t.text_accent));
            lines.push(sep_line(t.border_subtle, 80));

            let closes: Vec<f64> = hist
                .iter()
                .map(|h| h.close.to_string().parse::<f64>().unwrap_or(0.0))
                .collect();
            let current_f: f64 = current_price
                .map(|p| p.to_string().parse::<f64>().unwrap_or(0.0))
                .unwrap_or(0.0);

            // ── Moving Averages ──
            let sma_periods: &[(usize, &str)] = &[
                (20, "SMA(20)  "),
                (50, "SMA(50)  "),
                (200, "SMA(200) "),
            ];
            for &(period, label) in sma_periods {
                if closes.len() >= period {
                    let sma_series = indicators::compute_sma(&closes, period);
                    if let Some(Some(sma_val)) = sma_series.last() {
                        let above = current_f > *sma_val;
                        let indicator = if above { "▲" } else { "▼" };
                        let ind_color = if above { t.gain_green } else { t.loss_red };
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {}   ", label),
                                Style::default().fg(t.text_secondary),
                            ),
                            Span::styled(
                                format!("{:.2}", sma_val),
                                Style::default().fg(t.text_primary),
                            ),
                            Span::styled(
                                format!(" {}", indicator),
                                Style::default().fg(ind_color),
                            ),
                        ]));
                    }
                }
            }

            // ── Bollinger Bands ──
            let bb_series = indicators::bollinger::compute_bollinger(&closes, 20, 2.0);
            if let Some(Some(bb)) = bb_series.last() {
                lines.push(Line::from(vec![
                    Span::styled("  BB Upper      ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}", bb.upper),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  BB Lower      ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}", bb.lower),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  BB Width      ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.2}%", bb.width * 100.0),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
            }

            // ── RSI with visual gauge ──
            if closes.len() >= 15 {
                let rsi_series = indicators::compute_rsi(&closes, 14);
                if let Some(Some(rsi_val)) = rsi_series.last() {
                    let rsi_color = if *rsi_val > 70.0 {
                        t.loss_red
                    } else if *rsi_val < 30.0 {
                        t.gain_green
                    } else {
                        t.text_primary
                    };
                    let label = if *rsi_val > 70.0 {
                        " Overbought"
                    } else if *rsi_val < 30.0 {
                        " Oversold"
                    } else {
                        ""
                    };
                    lines.push(Line::from(vec![
                        Span::styled("  RSI(14)       ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:.1}", rsi_val),
                            Style::default().fg(rsi_color).bold(),
                        ),
                        Span::styled(
                            label.to_string(),
                            Style::default().fg(rsi_color),
                        ),
                    ]));
                    // Visual RSI gauge: 30 chars wide, color-zoned
                    lines.push(rsi_gauge_line(*rsi_val, t));
                }
            }

            // ── MACD (12, 26, 9) ──
            if closes.len() >= 35 {
                let macd_series = indicators::compute_macd(&closes, 12, 26, 9);
                if let Some(Some(macd)) = macd_series.last() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("  MACD          ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:+.4}", macd.macd),
                            Style::default().fg(t.text_primary),
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("  Signal        ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:+.4}", macd.signal),
                            Style::default().fg(t.text_primary),
                        ),
                    ]));
                    let hist_color = if macd.histogram > 0.0 {
                        t.gain_green
                    } else {
                        t.loss_red
                    };
                    let trend = if macd.histogram > 0.0 { "Bullish" } else { "Bearish" };
                    lines.push(Line::from(vec![
                        Span::styled("  Histogram     ", Style::default().fg(t.text_secondary)),
                        Span::styled(
                            format!("{:+.4}", macd.histogram),
                            Style::default().fg(hist_color).bold(),
                        ),
                        Span::styled(
                            format!(" {}", trend),
                            Style::default().fg(hist_color),
                        ),
                    ]));

                    // Visual MACD histogram bar (last 20 values)
                    let hist_bars = macd_histogram_bars(&macd_series, t);
                    if !hist_bars.spans.is_empty() {
                        lines.push(hist_bars);
                    }
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

            // Show drift band if target is set
            if let Some(target) = app.allocation_targets.get(symbol) {
                use rust_decimal::Decimal;
                let actual_pct = pos.allocation_pct.unwrap_or(dec!(0));
                let drift = actual_pct - target.target_pct;
                let abs_drift = drift.abs();
                let over_band = abs_drift > target.drift_band_pct;
                
                let drift_color = if over_band {
                    if drift > Decimal::ZERO {
                        t.gain_green
                    } else {
                        t.loss_red
                    }
                } else {
                    t.text_muted
                };
                
                let status_text = if over_band {
                    if drift > Decimal::ZERO {
                        format!("OVERWEIGHT (+{:.1}%)", abs_drift)
                    } else {
                        format!("UNDERWEIGHT (-{:.1}%)", abs_drift)
                    }
                } else {
                    "IN RANGE".to_string()
                };
                
                lines.push(Line::from(vec![
                    Span::styled("  Target      ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:.1}% ± {:.1}%", target.target_pct, target.drift_band_pct),
                        Style::default().fg(t.text_primary),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("  Drift       ", Style::default().fg(t.text_secondary)),
                    Span::styled(
                        format!("{:+.1}%  ", drift),
                        Style::default().fg(drift_color),
                    ),
                    Span::styled(
                        status_text,
                        Style::default().fg(drift_color).bold(),
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

/// Render a visual RSI gauge bar with color zones.
///
/// Layout: `  [oversold |   neutral   | overbought]`
/// 30 chars wide, position marker shows current RSI.
/// Green zone: 0-30, neutral: 30-70, red zone: 70-100.
fn rsi_gauge_line<'a>(rsi: f64, t: &crate::tui::theme::Theme) -> Line<'a> {
    let gauge_width: usize = 30;
    let pos = ((rsi / 100.0) * gauge_width as f64).round() as usize;
    let pos = pos.min(gauge_width);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(gauge_width + 4);
    spans.push(Span::styled("  ", Style::default()));

    // Build the gauge character by character
    for i in 0..gauge_width {
        let zone_color = if i < 9 {
            // 0-30% zone (oversold = green/bullish)
            t.gain_green
        } else if i < 21 {
            // 30-70% zone (neutral)
            t.text_muted
        } else {
            // 70-100% zone (overbought = red/bearish)
            t.loss_red
        };

        if i == pos {
            spans.push(Span::styled(
                "◆".to_string(),
                Style::default().fg(t.text_accent).bold(),
            ));
        } else {
            spans.push(Span::styled(
                "─".to_string(),
                Style::default().fg(zone_color),
            ));
        }
    }

    spans.push(Span::styled(
        "  0".to_string(),
        Style::default().fg(t.text_muted),
    ));
    spans.push(Span::styled(
        "·30·70·".to_string(),
        Style::default().fg(t.text_muted),
    ));
    spans.push(Span::styled(
        "100".to_string(),
        Style::default().fg(t.text_muted),
    ));

    Line::from(spans)
}

/// Render a compact MACD histogram bar from the last N values.
///
/// Uses block characters (▁▂▃▄▅▆▇█) scaled to the max absolute histogram value.
/// Green for positive, red for negative.
fn macd_histogram_bars<'a>(
    macd_series: &[Option<indicators::MacdResult>],
    t: &crate::tui::theme::Theme,
) -> Line<'a> {
    let bar_chars: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let display_count = 20;

    // Collect last N histogram values
    let hist_values: Vec<f64> = macd_series
        .iter()
        .rev()
        .take(display_count)
        .filter_map(|v| v.as_ref().map(|m| m.histogram))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    if hist_values.is_empty() {
        return Line::from("");
    }

    let max_abs = hist_values
        .iter()
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max);

    if max_abs < f64::EPSILON {
        return Line::from("");
    }

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(hist_values.len() + 2);
    spans.push(Span::styled("  Hist          ", Style::default().fg(t.text_secondary)));

    for &val in &hist_values {
        let normalized = (val.abs() / max_abs * (bar_chars.len() - 1) as f64).round() as usize;
        let idx = normalized.min(bar_chars.len() - 1);
        let color = if val >= 0.0 { t.gain_green } else { t.loss_red };
        spans.push(Span::styled(
            bar_chars[idx].to_string(),
            Style::default().fg(color),
        ));
    }

    Line::from(spans)
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
    fn rsi_gauge_oversold() {
        let t = crate::tui::theme::midnight();
        let line = rsi_gauge_line(20.0, &t);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('◆'), "Gauge should contain position marker");
    }

    #[test]
    fn rsi_gauge_overbought() {
        let t = crate::tui::theme::midnight();
        let line = rsi_gauge_line(80.0, &t);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('◆'));
    }

    #[test]
    fn macd_histogram_bars_with_data() {
        let t = crate::tui::theme::midnight();
        let macd_data: Vec<Option<indicators::MacdResult>> = (0..30)
            .map(|i| {
                Some(indicators::MacdResult {
                    macd: (i as f64 * 0.1).sin(),
                    signal: (i as f64 * 0.1).cos() * 0.5,
                    histogram: (i as f64 * 0.1).sin() - (i as f64 * 0.1).cos() * 0.5,
                })
            })
            .collect();
        let line = macd_histogram_bars(&macd_data, &t);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.is_empty(), "Should render histogram bars");
    }

    #[test]
    fn build_lines_shows_macd_with_enough_history() {
        let mut app = test_app();
        app.prices.insert("AAPL".to_string(), dec!(175));
        let mut hist = Vec::new();
        for i in 0..50 {
            hist.push(HistoryRecord {
                date: format!("2026-01-{:02}", (i % 28) + 1),
                close: dec!(150) + Decimal::from(i),
                volume: None,
            });
        }
        app.price_history.insert("AAPL".to_string(), hist);
        let lines = build_lines("AAPL", &app);
        let text = lines_to_string(&lines);
        assert!(text.contains("MACD"), "Should show MACD with 50+ data points");
        assert!(text.contains("Signal"), "Should show Signal line");
        assert!(text.contains("Histogram"), "Should show Histogram value");
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
