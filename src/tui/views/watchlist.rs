use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::indicators;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::tui::theme;

/// Compute proximity to target as a percentage. Returns (distance_pct, hit).
/// distance_pct > 0 means not yet reached. distance_pct <= 0 means target hit.
fn compute_proximity(current: Decimal, target: Decimal, direction: &str) -> (Decimal, bool) {
    if target.is_zero() {
        return (dec!(0), false);
    }
    let dist_pct = match direction {
        "below" => (current - target) / target * dec!(100),
        "above" => (target - current) / current * dec!(100),
        _ => dec!(0),
    };
    let hit = dist_pct <= dec!(0);
    (dist_pct, hit)
}

/// Render a proximity bar as colored spans.
fn proximity_spans(dist_pct: Decimal, hit: bool, t: &theme::Theme) -> Vec<Span<'static>> {
    if hit {
        vec![Span::styled("🎯 HIT".to_string(), Style::default().fg(t.gain_green).bold())]
    } else {
        let f: f64 = dist_pct.to_string().parse().unwrap_or(0.0);
        let color = if f < 3.0 {
            t.loss_red // very close — red urgency
        } else if f < 10.0 {
            Color::Yellow // approaching
        } else {
            t.gain_green // far away — green (safe)
        };
        vec![Span::styled(format!("{:.1}%", f), Style::default().fg(color))]
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let entries = &app.watchlist_entries;

    if entries.is_empty() {
        let empty_msg = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No symbols in watchlist",
                Style::default().fg(t.text_muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Add symbols with: pftui watch <SYMBOL>",
                Style::default().fg(t.text_secondary),
            )),
            Line::from(Span::styled(
                "  Example: pftui watch AAPL",
                Style::default().fg(t.text_secondary),
            )),
        ];
        let paragraph = ratatui::widgets::Paragraph::new(empty_msg).block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_INACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .style(Style::default().bg(t.surface_0)),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    // Check if any entry has a target price set
    let has_targets = entries
        .iter()
        .any(|e| e.target_price.is_some());

    let mut header_cells = vec![
        Cell::from("Symbol"),
        Cell::from("Name"),
        Cell::from("Category"),
        Cell::from("Price"),
        Cell::from("Change %"),
        Cell::from("RSI"),
        Cell::from("SMA50"),
    ];
    if has_targets {
        header_cells.push(Cell::from("Target"));
        header_cells.push(Cell::from("Prox"));
    }

    let header = Row::new(header_cells)
        .style(Style::default().fg(t.text_secondary).bold())
        .height(1);

    let rows: Vec<Row> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let cat: AssetCategory = entry
                .category
                .parse()
                .unwrap_or(AssetCategory::Equity);
            let cat_color = t.category_color(cat);

            let row_bg = if i == app.watchlist_selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_0
            };

            let name = resolve_name(&entry.symbol);
            let display_name = if name.is_empty() {
                entry.symbol.clone()
            } else {
                name
            };

            // Look up price via yahoo symbol
            let yahoo_sym = yahoo_symbol_for(&entry.symbol, cat);
            let price = app.prices.get(&yahoo_sym).copied();
            let price_str = match price {
                Some(p) => format_price(p),
                None => "---".to_string(),
            };

            // Compute change % from history
            let change_pct = compute_change_pct(app, &yahoo_sym);
            let (change_str, change_color) = match change_pct {
                Some(pct) => {
                    let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                    let color = theme::gain_intensity_color(t, f);
                    (format!("{:+.2}%", f), color)
                }
                None => ("---".to_string(), t.text_muted),
            };

            // Compute RSI from price history
            let rsi_cell = {
                let history = app.price_history.get(&yahoo_sym);
                match history {
                    Some(records) if records.len() >= 15 => {
                        let closes: Vec<f64> = records
                            .iter()
                            .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
                            .collect();
                        let rsi_series = indicators::compute_rsi(&closes, 14);
                        match rsi_series.last().copied().flatten() {
                            Some(rsi_val) => {
                                let rsi_color = if rsi_val > 70.0 {
                                    t.loss_red
                                } else if rsi_val < 30.0 {
                                    t.gain_green
                                } else {
                                    t.text_secondary
                                };
                                // Direction arrow
                                let prev_rsi = if rsi_series.len() >= 2 {
                                    rsi_series[rsi_series.len() - 2]
                                } else {
                                    None
                                };
                                let arrow = match prev_rsi {
                                    Some(prev) if rsi_val > prev + 0.5 => " ▲",
                                    Some(prev) if rsi_val < prev - 0.5 => " ▼",
                                    _ => "",
                                };
                                let arrow_color = if arrow == " ▲" {
                                    if rsi_val > 60.0 { t.loss_red } else { t.text_secondary }
                                } else if arrow == " ▼" {
                                    if rsi_val < 40.0 { t.gain_green } else { t.text_secondary }
                                } else {
                                    t.text_muted
                                };
                                Cell::from(Line::from(vec![
                                    Span::styled(
                                        format!("{:.0}", rsi_val),
                                        Style::default().fg(rsi_color),
                                    ),
                                    Span::styled(
                                        arrow.to_string(),
                                        Style::default().fg(arrow_color),
                                    ),
                                ]))
                            }
                            None => Cell::from(Span::styled(
                                "---",
                                Style::default().fg(t.text_muted),
                            )),
                        }
                    }
                    _ => Cell::from(Span::styled(
                        "---",
                        Style::default().fg(t.text_muted),
                    )),
                }
            };

            // Compute SMA50 from price history
            let sma50_cell = {
                let history = app.price_history.get(&yahoo_sym);
                match history {
                    Some(records) if records.len() >= 50 => {
                        let closes: Vec<f64> = records
                            .iter()
                            .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
                            .collect();
                        let sma50_series = indicators::compute_sma(&closes, 50);
                        match sma50_series.last().copied().flatten() {
                            Some(sma50_val) => {
                                // Compare current price to SMA50
                                let sma50_color = if let Some(p) = price {
                                    let price_f64: f64 = p.to_string().parse().unwrap_or(0.0);
                                    let pct_diff = ((price_f64 - sma50_val) / sma50_val) * 100.0;
                                    if pct_diff > 5.0 {
                                        t.gain_green // >5% above SMA50 — bullish
                                    } else if pct_diff < -5.0 {
                                        t.loss_red // >5% below SMA50 — bearish
                                    } else {
                                        t.text_secondary // near SMA50 — neutral
                                    }
                                } else {
                                    t.text_muted
                                };
                                Cell::from(Span::styled(
                                    format!("{:.2}", sma50_val),
                                    Style::default().fg(sma50_color),
                                ))
                            }
                            None => Cell::from(Span::styled(
                                "---",
                                Style::default().fg(t.text_muted),
                            )),
                        }
                    }
                    _ => Cell::from(Span::styled(
                        "---",
                        Style::default().fg(t.text_muted),
                    )),
                }
            };

            let mut cells = vec![
                Cell::from(Span::styled(
                    entry.symbol.clone(),
                    Style::default().fg(t.text_primary).bold(),
                )),
                Cell::from(Span::styled(
                    display_name,
                    Style::default().fg(t.text_secondary),
                )),
                Cell::from(Span::styled(
                    format!("{}", cat),
                    Style::default().fg(cat_color),
                )),
                Cell::from(Span::styled(
                    price_str.clone(),
                    Style::default().fg(t.text_primary),
                )),
                Cell::from(Span::styled(
                    change_str,
                    Style::default().fg(change_color),
                )),
                rsi_cell,
                sma50_cell,
            ];

            if has_targets {
                match (&entry.target_price, &entry.target_direction) {
                    (Some(tp), Some(dir)) => {
                        if let Ok(target_dec) = tp.parse::<Decimal>() {
                            // Target cell
                            let dir_symbol = if dir == "below" { "↓" } else { "↑" };
                            let target_str = format!("{}{:.2}", dir_symbol, target_dec);
                            cells.push(Cell::from(Span::styled(
                                target_str,
                                Style::default().fg(t.text_secondary),
                            )));

                            // Proximity cell
                            if let Some(cur) = price {
                                let (dist_pct, hit) = compute_proximity(cur, target_dec, dir);
                                let spans = proximity_spans(dist_pct, hit, t);
                                cells.push(Cell::from(Line::from(spans)));
                            } else {
                                cells.push(Cell::from(Span::styled(
                                    "---",
                                    Style::default().fg(t.text_muted),
                                )));
                            }
                        } else {
                            cells.push(Cell::from(Span::styled("---", Style::default().fg(t.text_muted))));
                            cells.push(Cell::from(Span::styled("---", Style::default().fg(t.text_muted))));
                        }
                    }
                    _ => {
                        cells.push(Cell::from(Span::styled("---", Style::default().fg(t.text_muted))));
                        cells.push(Cell::from(Span::styled("---", Style::default().fg(t.text_muted))));
                    }
                }
            }

            Row::new(cells)
            .style(Style::default().bg(row_bg))
            .height(1)
        })
        .collect();

    let widths: Vec<Constraint> = if has_targets {
        vec![
            Constraint::Length(8),
            Constraint::Min(12),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(10),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Min(16),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(8),
        ]
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_INACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .style(Style::default().bg(t.surface_0)),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}

/// Map a watchlist symbol to its Yahoo Finance ticker.
pub fn yahoo_symbol_for(symbol: &str, category: AssetCategory) -> String {
    match category {
        AssetCategory::Crypto => {
            if symbol.ends_with("-USD") {
                symbol.to_string()
            } else {
                format!("{}-USD", symbol)
            }
        }
        _ => symbol.to_string(),
    }
}

/// Compute daily change % from price history.
fn compute_change_pct(app: &App, yahoo_symbol: &str) -> Option<Decimal> {
    // Get current price from cache
    let current_price = app.prices.get(yahoo_symbol)?;
    
    // Get yesterday's close from history
    let history = app.price_history.get(yahoo_symbol)?;
    if history.is_empty() {
        return None;
    }
    
    let prev_close = history[0].close;
    if prev_close == dec!(0) {
        return None;
    }
    
    Some((current_price - prev_close) / prev_close * dec!(100))
}

fn format_price(p: Decimal) -> String {
    let f: f64 = p.to_string().parse().unwrap_or(0.0);
    if f.abs() >= 10_000.0 {
        format!("{:.0}", f)
    } else if f.abs() >= 1.0 {
        format!("{:.2}", f)
    } else {
        format!("{:.4}", f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yahoo_symbol_for_crypto() {
        assert_eq!(
            yahoo_symbol_for("BTC", AssetCategory::Crypto),
            "BTC-USD"
        );
    }

    #[test]
    fn yahoo_symbol_for_crypto_already_suffixed() {
        assert_eq!(
            yahoo_symbol_for("BTC-USD", AssetCategory::Crypto),
            "BTC-USD"
        );
    }

    #[test]
    fn yahoo_symbol_for_equity() {
        assert_eq!(
            yahoo_symbol_for("AAPL", AssetCategory::Equity),
            "AAPL"
        );
    }

    #[test]
    fn yahoo_symbol_for_commodity() {
        assert_eq!(
            yahoo_symbol_for("GC=F", AssetCategory::Commodity),
            "GC=F"
        );
    }

    #[test]
    fn format_price_large() {
        let p = Decimal::new(5234500, 2);
        assert_eq!(format_price(p), "52345");
    }

    #[test]
    fn format_price_medium() {
        let p = Decimal::new(17523, 2);
        assert_eq!(format_price(p), "175.23");
    }

    #[test]
    fn format_price_small() {
        let p = Decimal::new(8321, 4);
        assert_eq!(format_price(p), "0.8321");
    }
}
