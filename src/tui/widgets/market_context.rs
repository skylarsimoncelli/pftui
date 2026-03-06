//! Market context panel — shows live market data relevant to the selected position.
//!
//! Displays in the middle column of ultra-wide (160+ col) layouts:
//! - Top movers today (top 3 gainers/losers from portfolio + watchlist)
//! - Related macro indicators (context-aware based on selected asset)
//! - Sector summary (if equity selected)
//! - Fear & Greed gauge (compact)
//! - Next economic event countdown
//! - Active alerts for selected symbol

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::models::asset::AssetCategory;
use crate::models::position::Position;

/// Render the market context panel.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if is_privacy_view(app) {
        render_privacy_placeholder(frame, area, app);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_inactive))
        .style(Style::default().bg(app.theme.surface_1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 6 {
        return; // Not enough space
    }

    // Split into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Top movers
            Constraint::Length(4), // Macro indicators
            Constraint::Length(3), // Fear & Greed
            Constraint::Length(3), // Next event
            Constraint::Min(1),    // Active alerts
        ])
        .split(inner);

    render_top_movers(frame, chunks[0], app);
    render_macro_indicators(frame, chunks[1], app);
    render_fear_greed(frame, chunks[2], app);
    render_next_event(frame, chunks[3], app);
    render_active_alerts(frame, chunks[4], app);
}

fn render_privacy_placeholder(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border_inactive))
        .style(Style::default().bg(app.theme.surface_1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new("MARKET CONTEXT\n[Privacy mode]")
        .alignment(Alignment::Center)
        .style(Style::default().fg(app.theme.text_muted));

    frame.render_widget(text, inner);
}

/// Render top 3 movers (gainers/losers) from portfolio + watchlist.
fn render_top_movers(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let mut movers = compute_movers(app);
    movers.sort_by(|a, b| b.change_pct.abs().cmp(&a.change_pct.abs()));
    movers.truncate(3);

    let mut lines = vec![Line::from(vec![
        Span::styled("TOP MOVERS ", Style::default().fg(app.theme.text_secondary)),
    ])];

    for mover in movers {
        let color = if mover.change_pct > dec!(0) {
            app.theme.gain_green
        } else {
            app.theme.loss_red
        };

        let change_str = format!("{:+.2}%", mover.change_pct);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:8}", mover.symbol),
                Style::default().fg(app.theme.text_primary),
            ),
            Span::raw(" "),
            Span::styled(change_str, Style::default().fg(color).bold()),
        ]));
    }

    if lines.len() == 1 {
        lines.push(Line::from(Span::styled(
            "No data",
            Style::default().fg(app.theme.text_muted),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.surface_1));
    frame.render_widget(para, area);
}

#[derive(Debug, Clone)]
struct Mover {
    symbol: String,
    change_pct: Decimal,
}

/// Compute daily % change for positions and watchlist entries.
fn compute_movers(app: &App) -> Vec<Mover> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut result = Vec::new();

    // Positions
    for pos in &app.positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        if let Some(change_pct) = compute_daily_change(pos, &today, app) {
            result.push(Mover {
                symbol: pos.symbol.clone(),
                change_pct,
            });
        }
    }

    // Watchlist
    for entry in &app.watchlist_entries {
        if let Some(current) = app.prices.get(&entry.symbol) {
            if let Some(change_pct) = compute_watchlist_change(&entry.symbol, *current, &today, app) {
                result.push(Mover {
                    symbol: entry.symbol.clone(),
                    change_pct,
                });
            }
        }
    }

    result
}

fn compute_daily_change(pos: &Position, today: &str, app: &App) -> Option<Decimal> {
    let current = pos.current_price?;
    if current <= dec!(0) {
        return None;
    }

    let history = app.price_history.get(&pos.symbol)?;
    let yesterday = history.iter().rev().find(|rec| rec.date.as_str() < today)?;

    let close = yesterday.close;
    if close <= dec!(0) {
        return None;
    }

    Some(((current - close) / close) * dec!(100))
}

fn compute_watchlist_change(symbol: &str, current: Decimal, today: &str, app: &App) -> Option<Decimal> {
    if current <= dec!(0) {
        return None;
    }

    let history = app.price_history.get(symbol)?;
    let yesterday = history.iter().rev().find(|rec| rec.date.as_str() < today)?;

    let close = yesterday.close;
    if close <= dec!(0) {
        return None;
    }

    Some(((current - close) / close) * dec!(100))
}

/// Render related macro indicators based on selected position category.
fn render_macro_indicators(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let selected_pos = app.selected_position();
    let lines = if let Some(pos) = selected_pos {
        build_contextual_macro_lines(pos, app)
    } else {
        vec![
            Line::from(Span::styled(
                "MACRO CONTEXT",
                Style::default().fg(app.theme.text_secondary),
            )),
            Line::from(Span::styled(
                "Select an asset",
                Style::default().fg(app.theme.text_muted),
            )),
        ]
    };

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.surface_1));
    frame.render_widget(para, area);
}

fn build_contextual_macro_lines(pos: &Position, app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        "MACRO CONTEXT",
        Style::default().fg(app.theme.text_secondary),
    ))];

    match pos.category {
        AssetCategory::Commodity => {
            // Gold → DXY, real yields, gold/silver ratio
            if pos.symbol.starts_with("GC") || pos.symbol.contains("GOLD") {
                if let Some(dxy_price) = app.prices.get("DXY") {
                    lines.push(Line::from(vec![
                        Span::styled("DXY ", Style::default().fg(app.theme.text_muted)),
                        Span::styled(
                            format!("{:.2}", dxy_price),
                            Style::default().fg(app.theme.text_primary),
                        ),
                    ]));
                }
                // Gold/Silver ratio
                if let Some(gc) = app.prices.get("GC=F") {
                    if let Some(si) = app.prices.get("SI=F") {
                        if *si > dec!(0) {
                            let ratio = gc / si;
                            lines.push(Line::from(vec![
                                Span::styled("Au/Ag ", Style::default().fg(app.theme.text_muted)),
                                Span::styled(
                                    format!("{:.1}", ratio),
                                    Style::default().fg(app.theme.text_primary),
                                ),
                            ]));
                        }
                    }
                }
            }
        }
        AssetCategory::Equity => {
            // Equities → VIX, sector ETF
            if let Some(vix) = app.prices.get("^VIX") {
                let vix_color = if *vix > dec!(20) {
                    app.theme.loss_red
                } else {
                    app.theme.gain_green
                };
                lines.push(Line::from(vec![
                    Span::styled("VIX ", Style::default().fg(app.theme.text_muted)),
                    Span::styled(format!("{:.2}", vix), Style::default().fg(vix_color)),
                ]));
            }
            if let Some(spy) = app.prices.get("SPY") {
                lines.push(Line::from(vec![
                    Span::styled("SPY ", Style::default().fg(app.theme.text_muted)),
                    Span::styled(format!("{:.2}", spy), Style::default().fg(app.theme.text_primary)),
                ]));
            }
        }
        AssetCategory::Crypto => {
            // Crypto → BTC dominance proxy
            if let Some(btc) = app.prices.get("BTC-USD") {
                lines.push(Line::from(vec![
                    Span::styled("BTC ", Style::default().fg(app.theme.text_muted)),
                    Span::styled(
                        format!("{:.0}", btc),
                        Style::default().fg(app.theme.text_primary),
                    ),
                ]));
            }
        }
        _ => {
            lines.push(Line::from(Span::styled(
                "No context",
                Style::default().fg(app.theme.text_muted),
            )));
        }
    }

    if lines.len() == 1 {
        lines.push(Line::from(Span::styled(
            "No data",
            Style::default().fg(app.theme.text_muted),
        )));
    }

    lines
}

/// Render Fear & Greed gauge (compact).
fn render_fear_greed(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 2 {
        return;
    }

    let mut lines = vec![Line::from(Span::styled(
        "FEAR & GREED",
        Style::default().fg(app.theme.text_secondary),
    ))];

    // Show crypto FNG if available
    if let Some((value, classification)) = &app.crypto_fng {
        let color = fng_color(*value, app);
        lines.push(Line::from(vec![
            Span::styled("Crypto ", Style::default().fg(app.theme.text_muted)),
            Span::styled(format!("{} ", value), Style::default().fg(color)),
            Span::styled(classification, Style::default().fg(app.theme.text_secondary)),
        ]));
    }

    // Show traditional FNG if available
    if let Some((value, classification)) = &app.traditional_fng {
        let color = fng_color(*value, app);
        lines.push(Line::from(vec![
            Span::styled("Stocks ", Style::default().fg(app.theme.text_muted)),
            Span::styled(format!("{} ", value), Style::default().fg(color)),
            Span::styled(classification, Style::default().fg(app.theme.text_secondary)),
        ]));
    }

    if lines.len() == 1 {
        lines.push(Line::from(Span::styled(
            "No data",
            Style::default().fg(app.theme.text_muted),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.surface_1));
    frame.render_widget(para, area);
}

fn fng_color(value: u8, app: &App) -> Color {
    match value {
        0..=24 => app.theme.loss_red,     // Extreme fear
        25..=44 => app.theme.stale_yellow, // Fear
        45..=55 => app.theme.text_muted, // Neutral
        56..=74 => app.theme.stale_yellow, // Greed
        75..=100 => app.theme.gain_green, // Extreme greed
        _ => app.theme.text_muted,
    }
}

/// Render next economic event countdown.
fn render_next_event(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 2 {
        return;
    }

    let mut lines = vec![Line::from(Span::styled(
        "NEXT EVENT",
        Style::default().fg(app.theme.text_secondary),
    ))];

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let next_event = app
        .calendar_events
        .iter()
        .find(|e| e.date >= today && e.impact == "high");

    if let Some(event) = next_event {
        let days_until = days_until(&event.date, &today);
        let time_str = if days_until == 0 {
            "Today".to_string()
        } else if days_until == 1 {
            "Tomorrow".to_string()
        } else {
            format!("{}d", days_until)
        };

        let name = truncate(&event.name, (area.width as usize).saturating_sub(10));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", time_str), Style::default().fg(app.theme.text_accent)),
            Span::styled(name, Style::default().fg(app.theme.text_primary)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No high-impact events",
            Style::default().fg(app.theme.text_muted),
        )));
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.surface_1));
    frame.render_widget(para, area);
}

fn days_until(date: &str, today: &str) -> i64 {
    let target = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or_default();
    let now = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").unwrap_or_default();
    (target - now).num_days()
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Render active alerts for the selected symbol.
fn render_active_alerts(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 2 {
        return;
    }

    let selected_pos = app.selected_position();
    let symbol = selected_pos.map(|p| p.symbol.as_str()).unwrap_or("");

    let mut lines = vec![Line::from(Span::styled(
        "ACTIVE ALERTS",
        Style::default().fg(app.theme.text_secondary),
    ))];

    if symbol.is_empty() {
        lines.push(Line::from(Span::styled(
            "No position selected",
            Style::default().fg(app.theme.text_muted),
        )));
    } else {
        // Check if there are alerts for this symbol
        // For now, show placeholder (alert data is available in the app but needs query)
        let alert_count = app.triggered_alert_count;
        if alert_count > 0 {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} alert", alert_count),
                    Style::default().fg(app.theme.stale_yellow),
                ),
                Span::styled(
                    if alert_count == 1 { "" } else { "s" },
                    Style::default().fg(app.theme.stale_yellow),
                ),
                Span::styled(" triggered", Style::default().fg(app.theme.text_muted)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "No active alerts",
                Style::default().fg(app.theme.text_muted),
            )));
        }
    }

    let para = Paragraph::new(lines).style(Style::default().bg(app.theme.surface_1));
    frame.render_widget(para, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_days_until() {
        assert_eq!(days_until("2025-01-15", "2025-01-14"), 1);
        assert_eq!(days_until("2025-01-14", "2025-01-14"), 0);
        assert_eq!(days_until("2025-01-20", "2025-01-14"), 6);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world test", 10), "hello w...");
        assert_eq!(truncate("hi", 10), "hi");
    }
}
