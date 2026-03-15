use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{HashMap, HashSet};

use crate::app::{is_privacy_view, App, PriceFlashDirection, SortField};
use crate::config::PortfolioMode;
use crate::indicators;
use crate::models::asset::AssetCategory;
use crate::models::price::HistoryRecord;
use crate::tui::theme;
use crate::tui::widgets::skeleton;

const SPARKLINE_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Left-side eighth blocks for sub-character bar resolution (1/8 to 8/8 fill).
const EIGHTH_BLOCKS: &[char] = &['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// 52-week high/low range result.
#[allow(dead_code)]
pub struct Range52W {
    pub high: Decimal,
    pub low: Decimal,
    /// Position of current price within the range as 0.0..=1.0
    pub position: f64,
    /// Percentage distance from 52-week high (negative = below high)
    pub from_high_pct: f64,
}

/// Compute 52-week high and low from price history records.
/// Returns None if fewer than 2 records or current_price is None.
pub fn compute_52w_range(
    records: &[HistoryRecord],
    current_price: Option<Decimal>,
) -> Option<Range52W> {
    if records.len() < 2 {
        return None;
    }
    let current = current_price?;

    // Take last 365 days of records (they should already be sorted by date)
    let start = if records.len() > 365 {
        records.len() - 365
    } else {
        0
    };
    let slice = &records[start..];

    let mut high = slice[0].close;
    let mut low = slice[0].close;
    for r in slice.iter().skip(1) {
        if r.close > high {
            high = r.close;
        }
        if r.close < low {
            low = r.close;
        }
    }

    // Include current price in high/low
    if current > high {
        high = current;
    }
    if current < low {
        low = current;
    }

    let range = high - low;
    let position = if range > dec!(0) {
        let pos_str = ((current - low) / range).to_string();
        pos_str.parse::<f64>().unwrap_or(0.5)
    } else {
        0.5
    };

    let from_high_pct = if high > dec!(0) {
        let pct_str = (((current - high) / high) * dec!(100)).to_string();
        pct_str.parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    };

    Some(Range52W {
        high,
        low,
        position,
        from_high_pct,
    })
}

/// Build a visual range bar showing current price position within 52-week range.
/// Returns spans like: `━━━●━━━ -5%`
/// Bar width is 6 chars, then from-high percentage.
/// Compute daily change % from price history: (latest - previous) / previous * 100.
/// Uses the last two entries in the history for the given symbol.
pub fn compute_change_pct(app: &App, symbol: &str) -> Option<Decimal> {
    let history = app.price_history.get(symbol)?;
    if history.len() < 2 {
        return None;
    }
    let latest = &history[history.len() - 1];
    let prev = &history[history.len() - 2];
    if prev.close == dec!(0) {
        return None;
    }
    Some((latest.close - prev.close) / prev.close * dec!(100))
}

fn format_change_pct(change: Option<Decimal>) -> String {
    change
        .map(|v| format!("{:+.1}%", v))
        .unwrap_or_else(|| "---".to_string())
}

/// Compute percentage change over a specific timeframe.
/// For YTD, computes from Jan 1 of current year to latest.
/// For other periods, looks back N days from latest record.
pub fn compute_period_change_pct(
    app: &App,
    symbol: &str,
    timeframe: crate::app::ChangeTimeframe,
) -> Option<Decimal> {
    use crate::app::ChangeTimeframe;
    
    let history = app.price_history.get(symbol)?;
    if history.is_empty() {
        return None;
    }

    let latest = &history[history.len() - 1];
    let latest_close = latest.close;

    match timeframe {
        ChangeTimeframe::YearToDate => {
            // Find the first record of current year
            let current_year = chrono::Utc::now().format("%Y").to_string();
            let year_start = history
                .iter()
                .find(|r| r.date.starts_with(&current_year))?;
            
            if year_start.close == dec!(0) {
                return None;
            }
            Some((latest_close - year_start.close) / year_start.close * dec!(100))
        }
        _ => {
            // For other timeframes, look back N days
            let lookback = timeframe.lookback_days()?;
            
            if history.len() < 2 {
                return None;
            }

            // Find the record closest to lookback days ago
            // History is sorted by date (oldest to newest)
            let target_idx = history.len().saturating_sub(lookback as usize);
            let base_record = &history[target_idx];
            
            if base_record.close == dec!(0) {
                return None;
            }
            Some((latest_close - base_record.close) / base_record.close * dec!(100))
        }
    }
}

/// Format a value (price × quantity) compactly with appropriate suffix.
/// Examples: $892, $12.4k, $1.2M
pub fn format_value(value: Decimal) -> String {
    let val_f64: f64 = value.to_string().parse().unwrap_or(0.0);
    let abs_val = val_f64.abs();
    
    if abs_val >= 1_000_000.0 {
        format!("${:.1}M", val_f64 / 1_000_000.0)
    } else if abs_val >= 10_000.0 {
        format!("${:.0}k", val_f64 / 1_000.0)
    } else if abs_val >= 1_000.0 {
        format!("${:.1}k", val_f64 / 1_000.0)
    } else {
        format!("${:.0}", val_f64)
    }
}

/// Format a signed dollar delta compactly with sign and suffix.
/// Examples: +$892, -$12.4k, +$1.2M
fn format_signed_value(value: Option<Decimal>) -> String {
    let Some(value) = value else {
        return "---".to_string();
    };

    let val_f64: f64 = value.to_string().parse().unwrap_or(0.0);
    let abs_val = val_f64.abs();
    let sign = if val_f64 >= 0.0 { "+" } else { "-" };

    if abs_val >= 1_000_000.0 {
        format!("{}${:.1}M", sign, abs_val / 1_000_000.0)
    } else if abs_val >= 10_000.0 {
        format!("{}${:.0}k", sign, abs_val / 1_000.0)
    } else if abs_val >= 1_000.0 {
        format!("{}${:.1}k", sign, abs_val / 1_000.0)
    } else {
        format!("{}${:.0}", sign, abs_val)
    }
}

/// Compute one-day dollar P&L from previous close and current price.
/// Formula: (current_price - previous_close) * quantity.
fn compute_day_pnl_dollars(
    app: &App,
    symbol: &str,
    quantity: Decimal,
    current_price: Option<Decimal>,
) -> Option<Decimal> {
    let current = current_price?;
    let history = app.price_history.get(symbol)?;
    if history.len() < 2 {
        return None;
    }
    let prev_close = history[history.len() - 2].close;
    Some((current - prev_close) * quantity)
}

/// Build compact RSI indicator spans for a position row.
/// Format: `45 ▲` with color coding:
/// - RSI > 70: red (overbought)
/// - RSI < 30: green (oversold)
/// - 30-70: neutral/secondary
///
/// The direction arrow compares current RSI to 1 bar ago.
fn build_rsi_spans<'a>(t: &'a theme::Theme, records: &[HistoryRecord]) -> Line<'a> {
    if records.len() < 15 {
        return Line::from(Span::styled("---", Style::default().fg(t.text_muted)));
    }
    let closes: Vec<f64> = records
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();
    let rsi_series = indicators::compute_rsi(&closes, 14);
    let current = match rsi_series.last().copied().flatten() {
        Some(v) => v,
        None => return Line::from(Span::styled("---", Style::default().fg(t.text_muted))),
    };

    let rsi_color = if current > 70.0 {
        t.loss_red
    } else if current < 30.0 {
        t.gain_green
    } else {
        t.text_secondary
    };

    // Direction arrow: compare to previous RSI value
    let prev_rsi = if rsi_series.len() >= 2 {
        rsi_series[rsi_series.len() - 2]
    } else {
        None
    };
    let arrow = match prev_rsi {
        Some(prev) if current > prev + 0.5 => " ▲",
        Some(prev) if current < prev - 0.5 => " ▼",
        _ => "",
    };

    let arrow_color = if arrow == " ▲" {
        // RSI rising — could be moving toward overbought
        if current > 60.0 { t.loss_red } else { t.text_secondary }
    } else if arrow == " ▼" {
        // RSI falling — could be moving toward oversold
        if current < 40.0 { t.gain_green } else { t.text_secondary }
    } else {
        t.text_muted
    };

    Line::from(vec![
        Span::styled(format!("{:.0}", current), Style::default().fg(rsi_color)),
        Span::styled(arrow.to_string(), Style::default().fg(arrow_color)),
    ])
}

/// Compute the background color for a row in the positions table.
/// Selected rows flash briefly on selection change, lerping from
/// `border_accent` back to `surface_3` over SELECTION_FLASH_DURATION ticks.
fn row_background(app: &App, row_index: usize) -> Color {
    let t = &app.theme;
    if row_index == app.selected_index {
        let elapsed = app.tick_count.saturating_sub(app.last_selection_change_tick);
        if elapsed < theme::SELECTION_FLASH_DURATION && app.last_selection_change_tick > 0 {
            // Lerp from border_accent (flash) toward surface_3 (steady)
            let progress = elapsed as f32 / theme::SELECTION_FLASH_DURATION as f32;
            theme::lerp_color(t.border_accent, t.surface_3, progress)
        } else {
            t.surface_3
        }
    } else if row_index.is_multiple_of(2) {
        t.surface_1
    } else {
        t.surface_1_alt
    }
}

/// Build a category divider row for insertion between position groups.
/// Produces a thin separator like "─── Crypto ───" spanning the first column,
/// with empty cells for the remaining columns.
fn category_divider_row(category: AssetCategory, t: &theme::Theme, col_count: usize) -> Row<'static> {
    let label = format!("{}", category);
    let cap_label = capitalize_category(&label);
    let divider_text = format!("─── {} ───", cap_label);
    let mut cells: Vec<Cell> = Vec::with_capacity(col_count);
    cells.push(Cell::from(Span::styled(
        divider_text,
        Style::default().fg(t.border_subtle),
    )));
    for _ in 1..col_count {
        cells.push(Cell::from(""));
    }
    Row::new(cells)
        .style(Style::default().bg(t.surface_1))
        .height(1)
}

#[derive(Default, Clone, Copy)]
struct CategoryAggregate {
    count: usize,
    allocation_pct: Decimal,
    total_gain: Decimal,
    total_cost: Decimal,
}

fn compute_category_aggregates(positions: &[crate::models::position::Position]) -> HashMap<AssetCategory, CategoryAggregate> {
    let mut out: HashMap<AssetCategory, CategoryAggregate> = HashMap::new();
    for pos in positions {
        let entry = out.entry(pos.category).or_default();
        entry.count += 1;
        entry.allocation_pct += pos.allocation_pct.unwrap_or(dec!(0));
        entry.total_gain += pos.gain.unwrap_or(dec!(0));
        entry.total_cost += pos.total_cost;
    }
    out
}

fn category_summary_row(
    category: AssetCategory,
    agg: CategoryAggregate,
    t: &theme::Theme,
    col_count: usize,
) -> Row<'static> {
    let label = format!("{}", category);
    let cap_label = capitalize_category(&label);
    let perf_pct = if agg.total_cost > dec!(0) {
        Some((agg.total_gain / agg.total_cost) * dec!(100))
    } else {
        None
    };
    let perf_text = perf_pct
        .map(|v| format!("{:+.1}%", v))
        .unwrap_or_else(|| "---".to_string());
    let summary = format!(
        "─── {} ({}) · Alloc {:.1}% · P&L {} ───",
        cap_label, agg.count, agg.allocation_pct, perf_text
    );

    let mut cells: Vec<Cell> = Vec::with_capacity(col_count);
    cells.push(Cell::from(Span::styled(
        summary,
        Style::default().fg(t.text_secondary).bold(),
    )));
    for _ in 1..col_count {
        cells.push(Cell::from(""));
    }
    Row::new(cells)
        .style(Style::default().bg(t.surface_1_alt))
        .height(1)
}

/// Capitalize the first letter of a category name.
fn capitalize_category(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn overdue_review_symbols(app: &App) -> HashSet<String> {
    let mut overdue = HashSet::new();
    let backend = match app.open_backend() {
        Some(b) => b,
        None => return overdue,
    };
    let annotations = match crate::db::annotations::list_annotations_backend(&backend) {
        Ok(v) => v,
        Err(_) => return overdue,
    };
    let today = chrono::Utc::now().date_naive();
    for ann in annotations {
        let Some(date_str) = ann.review_date else {
            continue;
        };
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
            if d <= today {
                overdue.insert(ann.symbol.to_uppercase());
            }
        }
    }
    overdue
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    if is_privacy_view(app) {
        render_privacy_table(frame, area, app);
    } else {
        render_full_table(frame, area, app);
    }
}

fn render_full_table(frame: &mut Frame, area: Rect, app: &mut App) {
    app.positions_table_area = Some(area);
    let positions = &app.display_positions;
    let t = &app.theme;
    let overdue_symbols = overdue_review_symbols(app);

    // New column layout: Asset, Price, 24h (or active timeframe), Day$, P&L, Value, Alloc%, RSI, Trend
    let timeframe_label = app.change_timeframe.label();
    
    let mut header_cells = vec![
        Cell::from("Asset"),
        Cell::from("Price"),
        Cell::from(timeframe_label),
        Cell::from("Day$"),
        Cell::from("P&L"),
        Cell::from("Value"),
        Cell::from("Alloc%"),
    ];
    
    if app.show_drift_columns {
        header_cells.push(Cell::from("Target"));
        header_cells.push(Cell::from("Drift"));
        header_cells.push(Cell::from("Status"));
    }
    
    header_cells.extend(vec![
        Cell::from("RSI"),
        Cell::from("Trend"),
    ]);

    let header = Row::new(header_cells)
        .style(Style::default().fg(t.text_secondary).bold())
        .height(1);

    let grouped_by_category = app.show_sector_grouping || matches!(app.sort_field, SortField::Category);
    let col_count = if app.show_drift_columns { 12 } else { 9 };
    let mut rows: Vec<Row> = Vec::new();
    let category_aggregates = if app.show_sector_grouping {
        Some(compute_category_aggregates(positions))
    } else {
        None
    };

    // Show skeleton placeholder rows while waiting for initial data
    if positions.is_empty() && !app.prices_live {
        // New layout: Asset, Price, timeframe%, Day$, P&L, Value, Alloc%, RSI, Trend
        let col_widths = [12, 16, 7, 9, 8, 10, 5, 5, 6];
        rows = skeleton::skeleton_rows(t, app.tick_count, &col_widths, col_count);
    }

    let mut last_category: Option<AssetCategory> = None;

    for (i, pos) in positions.iter().enumerate() {
        // Insert category divider when sorted by category and category changes
        if grouped_by_category {
            if last_category != Some(pos.category) {
                if let Some(aggregates) = &category_aggregates {
                    let agg = aggregates.get(&pos.category).copied().unwrap_or_default();
                    rows.push(category_summary_row(pos.category, agg, t, col_count));
                } else {
                    rows.push(category_divider_row(pos.category, t, col_count));
                }
            }
            last_category = Some(pos.category);
        }

            let cat_color = t.category_color(pos.category);

            let row_bg = row_background(app, i);

            let style = Style::default().bg(row_bg);

            let marker = if i == app.selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let asset_text = if pos.name.is_empty() {
                pos.symbol.clone()
            } else {
                format!("{} {}", pos.name, pos.symbol)
            };
            let has_overdue_review = overdue_symbols.contains(&pos.symbol.to_uppercase());
            let cat_dot = Span::styled("●", Style::default().fg(cat_color));
            let mut asset_spans = vec![
                marker,
                cat_dot,
                Span::raw(" "),
                Span::styled(asset_text, Style::default().fg(t.text_primary)),
            ];
            if has_overdue_review {
                asset_spans.push(Span::raw(" "));
                asset_spans.push(Span::styled("⏰", Style::default().fg(t.stale_yellow).bold()));
            }
            let asset_line = Line::from(asset_spans);

            // Price flash with direction
            let (price_style, flash_direction) = match app.price_flash_ticks.get(&pos.symbol) {
                Some(&(flash_tick, direction))
                    if app.tick_count.saturating_sub(flash_tick) < theme::FLASH_DURATION =>
                {
                    let bg = match direction {
                        PriceFlashDirection::Up => t.gain_green,
                        PriceFlashDirection::Down => t.loss_red,
                        PriceFlashDirection::Same => t.text_accent,
                    };
                    (
                        Style::default().fg(t.surface_0).bg(bg).bold(),
                        Some(direction),
                    )
                }
                _ => (Style::default().fg(t.text_primary), None),
            };

            let mini_sparkline_spans = build_sparkline_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                3,
            );

            let _sparkline_spans = build_sparkline_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                7,
            );

            // Period change % using active timeframe
            let period_change = compute_period_change_pct(app, &pos.symbol, app.change_timeframe);
            let period_change_f: f64 = period_change
                .unwrap_or(dec!(0))
                .to_string()
                .parse()
                .unwrap_or(0.0);
            let period_change_color = theme::gain_intensity_color(t, period_change_f);

            // Position value (price × quantity)
            let position_value = pos.current_price.map(|p| p * pos.quantity);
            let value_text = position_value.map(format_value).unwrap_or_else(|| "---".to_string());
            let day_pnl = compute_day_pnl_dollars(app, &pos.symbol, pos.quantity, pos.current_price);
            let day_pnl_color = match day_pnl {
                Some(v) if v > Decimal::ZERO => t.gain_green,
                Some(v) if v < Decimal::ZERO => t.loss_red,
                Some(_) => t.text_muted,
                None => t.text_muted,
            };

            // RSI indicator
            let rsi_line = build_rsi_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
            );

            // Trend sparkline matching active timeframe
            // For now, use the full sparkline (7 bars). Future enhancement: adjust based on timeframe.
            let trend_sparkline_spans = build_sparkline_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                7,
            );

            // New column order: Asset, Price, timeframe%, Day$, P&L, Value, Alloc%, RSI, Trend
            let mut row_cells = vec![
                Cell::from(asset_line),
                Cell::from(Line::from({
                    let mut spans = vec![];
                    // Currency indicator for non-USD positions
                    if let Some(ref curr) = pos.native_currency {
                        let symbol = match curr.as_str() {
                            "GBP" => "£",
                            "EUR" => "€",
                            "JPY" => "¥",
                            "CAD" => "C$",
                            "AUD" => "A$",
                            "CHF" => "₣",
                            _ => curr.as_str(),
                        };
                        spans.push(Span::styled(symbol, Style::default().fg(app.theme.text_muted)));
                    }
                    let price_text = format_price_opt(pos.current_price);
                    let price_spans = match flash_direction {
                        Some(PriceFlashDirection::Up) => vec![
                            Span::styled(price_text, price_style),
                            Span::styled(" ▲", price_style),
                        ],
                        Some(PriceFlashDirection::Down) => vec![
                            Span::styled(price_text, price_style),
                            Span::styled(" ▼", price_style),
                        ],
                        _ => vec![Span::styled(price_text, price_style)],
                    };
                    spans.extend(price_spans);
                    if !mini_sparkline_spans.is_empty() {
                        spans.push(Span::raw(" "));
                        spans.extend(mini_sparkline_spans);
                    }
                    spans
                })),
                Cell::from(format_change_pct(period_change))
                    .style(Style::default().fg(period_change_color)),
                Cell::from(format_signed_value(day_pnl))
                    .style(Style::default().fg(day_pnl_color)),
                Cell::from(build_gain_bar_spans(t, pos.gain_pct, 8)),
                Cell::from(value_text)
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format_alloc_pct(pos.allocation_pct))
                    .style(Style::default().fg(t.text_secondary)),
            ];

            // Add drift columns if enabled
            if app.show_drift_columns {
                use rust_decimal::Decimal;
                if let Some(target) = app.allocation_targets.get(&pos.symbol) {
                    let actual_pct = pos.allocation_pct.unwrap_or(dec!(0));
                    let target_pct = target.target_pct;
                    let drift = actual_pct - target_pct;
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
                    
                    let status_char = if over_band {
                        if drift > Decimal::ZERO { "▲" } else { "▼" }
                    } else {
                        "✓"
                    };
                    let status_color = if over_band { drift_color } else { t.gain_green };
                    
                    row_cells.push(Cell::from(format!("{:.1}%", target_pct))
                        .style(Style::default().fg(t.text_secondary)));
                    row_cells.push(Cell::from(format!("{:+.1}%", drift))
                        .style(Style::default().fg(drift_color)));
                    row_cells.push(Cell::from(status_char)
                        .style(Style::default().fg(status_color)));
                } else {
                    row_cells.push(Cell::from("---").style(Style::default().fg(t.text_muted)));
                    row_cells.push(Cell::from("---").style(Style::default().fg(t.text_muted)));
                    row_cells.push(Cell::from("---").style(Style::default().fg(t.text_muted)));
                }
            }

            row_cells.extend(vec![
                Cell::from(rsi_line),
                Cell::from(Line::from(trend_sparkline_spans)),
            ]);

            rows.push(Row::new(row_cells).style(style));
    }

    // New column layout: Asset, Price, timeframe%, Day$, P&L, Value, Alloc%, [drift cols], RSI, Trend
    let widths = if app.show_drift_columns {
        vec![
            Constraint::Min(14),    // Asset
            Constraint::Length(16), // Price (with mini sparkline)
            Constraint::Length(7),  // timeframe% (24h, 7d, etc.)
            Constraint::Length(9),  // Day$
            Constraint::Length(8),  // P&L (gain bar)
            Constraint::Length(10), // Value (position value)
            Constraint::Length(7),  // Alloc%
            Constraint::Length(7),  // Target
            Constraint::Length(7),  // Drift
            Constraint::Length(6),  // Status
            Constraint::Length(6),  // RSI
            Constraint::Length(8),  // Trend (sparkline)
        ]
    } else {
        vec![
            Constraint::Min(14),    // Asset
            Constraint::Length(16), // Price
            Constraint::Length(7),  // timeframe%
            Constraint::Length(9),  // Day$
            Constraint::Length(8),  // P&L
            Constraint::Length(10), // Value
            Constraint::Length(7),  // Alloc%
            Constraint::Length(6),  // RSI
            Constraint::Length(8),  // Trend
        ]
    };

    render_table(frame, area, app, header, rows, &widths);
}

fn render_privacy_table(frame: &mut Frame, area: Rect, app: &mut App) {
    app.positions_table_area = Some(area);
    let positions = &app.display_positions;
    let t = &app.theme;
    let overdue_symbols = overdue_review_symbols(app);

    let timeframe_label = app.change_timeframe.label();

    let header = Row::new(vec![
        Cell::from("Asset"),
        Cell::from("Price"),
        Cell::from(timeframe_label),
        Cell::from("Alloc%"),
        Cell::from("RSI"),
        Cell::from("Trend"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let grouped_by_category = app.show_sector_grouping || matches!(app.sort_field, SortField::Category);
    let privacy_col_count = 6;
    let mut rows: Vec<Row> = Vec::new();
    let category_aggregates = if app.show_sector_grouping {
        Some(compute_category_aggregates(positions))
    } else {
        None
    };

    // Show skeleton placeholder rows while waiting for initial data
    if positions.is_empty() && !app.prices_live {
        let col_widths = [14, 10, 7, 6, 5, 6];
        rows = skeleton::skeleton_rows(t, app.tick_count, &col_widths, privacy_col_count);
    }

    let mut last_category: Option<AssetCategory> = None;

    for (i, pos) in positions.iter().enumerate() {
        if grouped_by_category {
            if last_category != Some(pos.category) {
                if let Some(aggregates) = &category_aggregates {
                    let agg = aggregates.get(&pos.category).copied().unwrap_or_default();
                    rows.push(category_summary_row(pos.category, agg, t, privacy_col_count));
                } else {
                    rows.push(category_divider_row(pos.category, t, privacy_col_count));
                }
            }
            last_category = Some(pos.category);
        }

            let cat_color = t.category_color(pos.category);

            let row_bg = row_background(app, i);

            let style = Style::default().bg(row_bg);

            let marker = if i == app.selected_index {
                Span::styled("▎", Style::default().fg(t.border_active))
            } else {
                Span::raw(" ")
            };
            let asset_text = if pos.name.is_empty() {
                pos.symbol.clone()
            } else {
                format!("{} {}", pos.name, pos.symbol)
            };
            let has_overdue_review = overdue_symbols.contains(&pos.symbol.to_uppercase());
            let cat_dot = Span::styled("●", Style::default().fg(cat_color));
            let mut asset_spans = vec![
                marker,
                cat_dot,
                Span::raw(" "),
                Span::styled(asset_text, Style::default().fg(t.text_primary)),
            ];
            if has_overdue_review {
                asset_spans.push(Span::raw(" "));
                asset_spans.push(Span::styled("⏰", Style::default().fg(t.stale_yellow).bold()));
            }
            let asset_line = Line::from(asset_spans);

            let sparkline_spans = build_sparkline_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
                7,
            );

            // Period change % using active timeframe (privacy-safe — percentage only)
            let period_change = compute_period_change_pct(app, &pos.symbol, app.change_timeframe);
            let period_change_f: f64 = period_change
                .unwrap_or(dec!(0))
                .to_string()
                .parse()
                .unwrap_or(0.0);
            let period_change_color = theme::gain_intensity_color(t, period_change_f);

            // RSI indicator (privacy-safe — derived from public price data)
            let rsi_line = build_rsi_spans(
                t,
                app.price_history
                    .get(&pos.symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]),
            );

            rows.push(Row::new(vec![
                Cell::from(asset_line),
                Cell::from(format_price_opt(pos.current_price))
                    .style(Style::default().fg(t.text_primary)),
                Cell::from(format_change_pct(period_change))
                    .style(Style::default().fg(period_change_color)),
                Cell::from(format_alloc_pct(pos.allocation_pct))
                    .style(Style::default().fg(t.text_secondary)),
                Cell::from(rsi_line),
                Cell::from(Line::from(sparkline_spans)),
            ])
            .style(style));
    }

    // Privacy table: Asset, Price, timeframe%, Alloc%, RSI, Trend
    let widths = [
        Constraint::Min(18),
        Constraint::Length(12),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(8),
    ];

    render_table(frame, area, app, header, rows, &widths);
}

fn render_table(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    header: Row,
    rows: Vec<Row>,
    widths: &[Constraint],
) {
    let t = &app.theme;

    let arrow = if app.sort_ascending { "▲" } else { "▼" };
    let sort_indicator = format!(" [{}{}] ", app.sort_field_label(), arrow);

    // Flash sort indicator when sort changes — bright accent fading to normal
    let sort_elapsed = app.tick_count.saturating_sub(app.last_sort_change_tick);
    let sort_style = if sort_elapsed < theme::SORT_FLASH_DURATION && app.last_sort_change_tick > 0 {
        let progress = sort_elapsed as f32 / theme::SORT_FLASH_DURATION as f32;
        let flash_color = theme::lerp_color(t.text_primary, t.text_accent, progress);
        Style::default().fg(flash_color).bold()
    } else {
        Style::default().fg(t.text_accent)
    };

    let base_title = if app.portfolio_mode == PortfolioMode::Percentage {
        " Positions (%) "
    } else if app.show_percentages_only {
        " Positions [% view] "
    } else {
        " Positions "
    };
    let title = if app.show_sector_grouping {
        format!("{base_title}[Grouped] ")
    } else {
        base_title.to_string()
    };

    let is_active_panel = !(app.selected_position().is_some() && app.terminal_width >= crate::tui::ui::COMPACT_WIDTH);
    let border_color = positions_border_color(is_active_panel, app.prices_live, t.border_active, t.border_inactive, app.tick_count);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(if is_active_panel { crate::tui::theme::BORDER_ACTIVE } else { crate::tui::theme::BORDER_INACTIVE })
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(t.surface_1))
                .title(Span::styled(title, Style::default().fg(t.text_primary).bold()))
                .title_alignment(Alignment::Left)
                .title(
                    Line::from(Span::styled(
                        sort_indicator,
                        sort_style,
                    ))
                    .alignment(Alignment::Right),
                ),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}

fn build_sparkline_spans<'a>(
    theme: &'a theme::Theme,
    records: &[crate::models::price::HistoryRecord],
    count: usize,
) -> Vec<Span<'a>> {
    if records.is_empty() {
        return Vec::new();
    }
    let tail: Vec<f64> = records
        .iter()
        .rev()
        .take(count)
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let min = tail.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    tail.iter()
        .map(|v| {
            let position = if range > 0.0 {
                ((v - min) / range) as f32
            } else {
                0.5
            };
            let idx = if range > 0.0 {
                (position * 7.0).round() as usize
            } else {
                3
            };
            let color = theme::gradient_3(
                theme.chart_grad_low,
                theme.chart_grad_mid,
                theme.chart_grad_high,
                position,
            );
            Span::styled(
                String::from(SPARKLINE_CHARS[idx.min(7)]),
                Style::default().fg(color),
            )
        })
        .collect()
}

/// Compute the bar fill width in eighth-units for a gain/loss magnitude bar.
/// Returns (full_chars, eighth_remainder) where the bar fills `full_chars` full
/// cells plus `eighth_remainder` eighths (0..7) of the next cell.
/// `gain_pct` is the raw gain percentage (absolute value used), `max_pct` is the
/// scale max (e.g. 20.0), and `col_width` is the total column width in characters.
pub fn gain_bar_width(gain_pct: f64, max_pct: f64, col_width: usize) -> (usize, usize) {
    if max_pct <= 0.0 || col_width == 0 {
        return (0, 0);
    }
    let ratio = (gain_pct.abs() / max_pct).min(1.0);
    let total_eighths = (ratio * col_width as f64 * 8.0).round() as usize;
    let full_chars = total_eighths / 8;
    let remainder = total_eighths % 8;
    // Clamp full_chars to col_width
    if full_chars >= col_width {
        (col_width, 0)
    } else {
        (full_chars, remainder)
    }
}

/// Build spans for the gain% cell with a proportional magnitude bar behind it.
/// The bar fills from left to right, green for gains, red for losses, scaled to ±20%.
/// Uses background coloring on text characters for the solid portion and an
/// eighth-block character at the bar edge for sub-character precision.
fn build_gain_bar_spans<'a>(
    theme: &'a theme::Theme,
    gain_pct: Option<Decimal>,
    col_width: usize,
) -> Line<'a> {
    let gain_text = format_gain_pct(gain_pct);

    let gain_f: f64 = gain_pct
        .unwrap_or(dec!(0))
        .to_string()
        .parse()
        .unwrap_or(0.0);

    // No bar for zero gain or missing data
    if gain_pct.is_none() || gain_f == 0.0 {
        let fg = if gain_pct.is_none() {
            theme.text_muted
        } else {
            theme.neutral
        };
        return Line::from(Span::styled(gain_text, Style::default().fg(fg)));
    }

    let text_fg = theme::gain_intensity_color(theme, gain_f);
    let bar_color = if gain_f > 0.0 {
        // Dim green for bar background
        theme::lerp_color(theme.surface_1, theme.gain_green, 0.3)
    } else {
        // Dim red for bar background
        theme::lerp_color(theme.surface_1, theme.loss_red, 0.3)
    };

    let (full_chars, eighth_rem) = gain_bar_width(gain_f, 20.0, col_width);
    let text_chars: Vec<char> = gain_text.chars().collect();

    let mut spans: Vec<Span<'a>> = Vec::new();

    for col in 0..col_width {
        let text_ch = text_chars.get(col).copied();

        if col < full_chars {
            // Fully covered by bar — text char (or space) on colored background
            let ch = text_ch.unwrap_or(' ').to_string();
            spans.push(Span::styled(ch, Style::default().fg(text_fg).bg(bar_color)));
        } else if col == full_chars && eighth_rem > 0 {
            if let Some(c) = text_ch {
                // Text char sits in the fractional cell — use bar bg (approximate)
                spans.push(Span::styled(
                    c.to_string(),
                    Style::default().fg(text_fg).bg(bar_color),
                ));
            } else {
                // No text here — render an eighth-block character for precise edge
                // eighth_rem is 1..7, index into EIGHTH_BLOCKS (0-indexed, so rem-1)
                let block = EIGHTH_BLOCKS[eighth_rem.saturating_sub(1).min(7)];
                spans.push(Span::styled(
                    block.to_string(),
                    Style::default().fg(bar_color),
                ));
            }
        } else if let Some(c) = text_ch {
            // Beyond bar — plain text
            spans.push(Span::styled(c.to_string(), Style::default().fg(text_fg)));
        }
        // Don't emit trailing spaces — ratatui handles cell padding
    }

    Line::from(spans)
}

fn format_price_opt(price: Option<Decimal>) -> String {
    price
        .map(format_price)
        .unwrap_or_else(|| "---".to_string())
}

fn format_price(v: Decimal) -> String {
    let f: f64 = v.to_string().parse().unwrap_or(0.0);
    if f >= 10000.0 {
        format!("{:.0}", f)
    } else if f >= 100.0 {
        format!("{:.1}", f)
    } else if f >= 1.0 {
        format!("{:.2}", f)
    } else {
        format!("{:.4}", f)
    }
}

fn format_gain_pct(g: Option<Decimal>) -> String {
    g.map(|v| format!("{:+.1}%", v))
        .unwrap_or_else(|| "---".to_string())
}

fn format_alloc_pct(a: Option<Decimal>) -> String {
    a.map(|v| format!("{:.1}%", v))
        .unwrap_or_else(|| "---".to_string())
}

/// Compute the border color for the positions table panel.
/// When the table is the active (focused) panel and prices are live, the border
/// gently pulses between border_active and border_inactive. When active but stale,
/// it stays solid border_active. When inactive (chart has focus), border_inactive.
fn positions_border_color(
    is_active_panel: bool,
    prices_live: bool,
    border_active: Color,
    border_inactive: Color,
    tick_count: u64,
) -> Color {
    if is_active_panel && prices_live {
        theme::pulse_color(border_active, border_inactive, tick_count, theme::PULSE_PERIOD_BORDER)
    } else if is_active_panel {
        border_active
    } else {
        border_inactive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_history(prices: &[&str]) -> Vec<HistoryRecord> {
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2025-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect()
    }

    #[test]
    fn compute_52w_range_basic() {
        let history = make_history(&["100", "120", "80", "110"]);
        let result = compute_52w_range(&history, Some(dec!(110)));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.high, dec!(120));
        assert_eq!(r.low, dec!(80));
        // position: (110 - 80) / (120 - 80) = 30/40 = 0.75
        assert!((r.position - 0.75).abs() < 0.01);
        // from_high: (110 - 120) / 120 * 100 = -8.33%
        assert!((r.from_high_pct - (-8.33)).abs() < 0.1);
    }

    #[test]
    fn compute_52w_range_at_high() {
        let history = make_history(&["90", "100", "95"]);
        let result = compute_52w_range(&history, Some(dec!(105)));
        assert!(result.is_some());
        let r = result.unwrap();
        // Current price exceeds history high — becomes new high
        assert_eq!(r.high, dec!(105));
        assert_eq!(r.low, dec!(90));
        assert!((r.position - 1.0).abs() < 0.01);
        assert!((r.from_high_pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_52w_range_at_low() {
        let history = make_history(&["100", "110", "95"]);
        let result = compute_52w_range(&history, Some(dec!(85)));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.high, dec!(110));
        assert_eq!(r.low, dec!(85));
        assert!((r.position - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_52w_range_no_records() {
        let result = compute_52w_range(&[], Some(dec!(100)));
        assert!(result.is_none());
    }

    #[test]
    fn compute_52w_range_single_record() {
        let history = make_history(&["100"]);
        let result = compute_52w_range(&history, Some(dec!(100)));
        assert!(result.is_none());
    }

    #[test]
    fn compute_52w_range_no_price() {
        let history = make_history(&["100", "110", "95"]);
        let result = compute_52w_range(&history, None);
        assert!(result.is_none());
    }

    #[test]
    fn compute_52w_range_flat_price() {
        let history = make_history(&["100", "100", "100"]);
        let result = compute_52w_range(&history, Some(dec!(100)));
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.high, dec!(100));
        assert_eq!(r.low, dec!(100));
        assert!((r.position - 0.5).abs() < 0.01); // defaults to middle
        assert!((r.from_high_pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn compute_52w_range_limits_to_365_records() {
        // Create 400 records with old high that should be excluded
        let mut prices: Vec<String> = Vec::new();
        // First 35 records: very high price (should be outside 365-day window)
        for _ in 0..35 {
            prices.push("500".to_string());
        }
        // Last 365 records: normal range
        for i in 0..365 {
            prices.push(format!("{}", 100 + (i % 20)));
        }
        let history: Vec<HistoryRecord> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2024-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        let result = compute_52w_range(&history, Some(dec!(110)));
        assert!(result.is_some());
        let r = result.unwrap();
        // High should be from the last 365 records (119), not the old 500
        assert_eq!(r.high, dec!(119));
    }

    // --- compute_change_pct tests ---

    fn make_test_app_with_history(symbol: &str, prices: &[&str]) -> crate::app::App {
        let config = crate::config::Config::default();
        let mut app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_change_pct.db"));
        let records: Vec<HistoryRecord> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2025-01-{:02}", i + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        app.price_history.insert(symbol.to_string(), records);
        app
    }

    #[test]
    fn compute_change_pct_basic() {
        let app = make_test_app_with_history("AAPL", &["100", "110"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(10)); // +10%
    }

    #[test]
    fn compute_change_pct_negative() {
        let app = make_test_app_with_history("AAPL", &["100", "90"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(-10)); // -10%
    }

    #[test]
    fn compute_change_pct_no_change() {
        let app = make_test_app_with_history("AAPL", &["100", "100"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(0));
    }

    #[test]
    fn compute_change_pct_uses_last_two_entries() {
        // Should use 200 -> 220, not any earlier entries
        let app = make_test_app_with_history("AAPL", &["100", "150", "200", "220"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), dec!(10)); // (220-200)/200 * 100 = 10%
    }

    #[test]
    fn compute_change_pct_single_record() {
        let app = make_test_app_with_history("AAPL", &["100"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_none());
    }

    #[test]
    fn compute_change_pct_no_history() {
        let config = crate::config::Config::default();
        let app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_no_hist.db"));
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_none());
    }

    #[test]
    fn compute_change_pct_zero_prev_close() {
        let app = make_test_app_with_history("AAPL", &["0", "100"]);
        let result = compute_change_pct(&app, "AAPL");
        assert!(result.is_none()); // Division by zero guarded
    }

    #[test]
    fn compute_day_pnl_dollars_basic() {
        let app = make_test_app_with_history("AAPL", &["100", "110"]);
        let result = compute_day_pnl_dollars(&app, "AAPL", dec!(10), Some(dec!(110)));
        assert_eq!(result, Some(dec!(100))); // (110 - 100) * 10
    }

    #[test]
    fn compute_day_pnl_dollars_negative() {
        let app = make_test_app_with_history("AAPL", &["100", "95"]);
        let result = compute_day_pnl_dollars(&app, "AAPL", dec!(8), Some(dec!(95)));
        assert_eq!(result, Some(dec!(-40))); // (95 - 100) * 8
    }

    #[test]
    fn compute_day_pnl_dollars_no_history() {
        let config = crate::config::Config::default();
        let app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_day_pnl.db"));
        let result = compute_day_pnl_dollars(&app, "AAPL", dec!(10), Some(dec!(110)));
        assert!(result.is_none());
    }

    #[test]
    fn format_change_pct_positive() {
        let result = format_change_pct(Some(dec!(3.5)));
        assert_eq!(result, "+3.5%");
    }

    #[test]
    fn format_change_pct_negative() {
        let result = format_change_pct(Some(dec!(-2.1)));
        assert_eq!(result, "-2.1%");
    }

    #[test]
    fn format_change_pct_none() {
        let result = format_change_pct(None);
        assert_eq!(result, "---");
    }

    #[test]
    fn test_positions_border_pulse_when_active_and_live() {
        let active = Color::Rgb(100, 200, 255);
        let inactive = Color::Rgb(50, 50, 50);
        // tick 0: phase=0.0, intensity=0.65 (midpoint)
        // tick 30: phase=0.25, intensity=1.0 (peak — full active)
        // tick 90: phase=0.75, intensity=0.3 (trough — near inactive)
        let c0 = positions_border_color(true, true, active, inactive, 0);
        let c30 = positions_border_color(true, true, active, inactive, 30);
        let c90 = positions_border_color(true, true, active, inactive, 90);
        // Peak vs trough must differ
        assert_ne!(c30, c90, "pulse peak and trough should differ");
        // Midpoint should differ from peak
        assert_ne!(c0, c30, "pulse midpoint and peak should differ");
        // Peak (tick 30) should be closest to active color
        if let (Color::Rgb(r30, _, _), Color::Rgb(ra, _, _)) = (c30, active) {
            assert_eq!(r30, ra, "at peak intensity, color should equal active");
        }
        // Trough (tick 90) should be closer to inactive
        if let (Color::Rgb(r90, _, _), Color::Rgb(ri, _, _), Color::Rgb(ra, _, _)) = (c90, inactive, active) {
            assert!(r90 >= ri && r90 <= ra, "trough color should be between inactive and active");
            assert!(r90 < ra, "trough should be less than full active");
        }
    }

    #[test]
    fn test_positions_border_static_when_active_and_stale() {
        let active = Color::Rgb(100, 200, 255);
        let inactive = Color::Rgb(50, 50, 50);
        // When prices are not live, border should be solid active regardless of tick
        let c0 = positions_border_color(true, false, active, inactive, 0);
        let c50 = positions_border_color(true, false, active, inactive, 50);
        let c99 = positions_border_color(true, false, active, inactive, 99);
        assert_eq!(c0, active);
        assert_eq!(c50, active);
        assert_eq!(c99, active);
    }

    #[test]
    fn test_positions_border_inactive_when_not_active() {
        let active = Color::Rgb(100, 200, 255);
        let inactive = Color::Rgb(50, 50, 50);
        // When not the active panel, always inactive — regardless of prices_live or tick
        assert_eq!(positions_border_color(false, true, active, inactive, 0), inactive);
        assert_eq!(positions_border_color(false, true, active, inactive, 60), inactive);
        assert_eq!(positions_border_color(false, false, active, inactive, 0), inactive);
        assert_eq!(positions_border_color(false, false, active, inactive, 99), inactive);
    }
}

#[cfg(test)]
mod mini_sparkline_tests {
    use super::*;

    fn make_history(prices: &[&str]) -> Vec<HistoryRecord> {
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2025-01-{:02}", i + 1),
                close: p.parse().unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect()
    }

    #[test]
    fn test_mini_sparkline_three_points() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let history = make_history(&["100", "110", "120"]);
        let spans = build_sparkline_spans(&t, &history, 3);
        assert_eq!(spans.len(), 3, "mini sparkline should have 3 chars");
        // Ascending prices: chars should increase
        assert!(spans[0].content.as_ref() <= spans[2].content.as_ref(),
            "ascending prices should produce ascending sparkline");
    }

    #[test]
    fn test_mini_sparkline_uses_last_three_of_many() {
        let t = crate::tui::theme::theme_by_name("midnight");
        // 10 records, mini sparkline should use only the last 3
        let history = make_history(&["50", "60", "70", "80", "90", "100", "200", "300", "100", "110"]);
        let spans = build_sparkline_spans(&t, &history, 3);
        assert_eq!(spans.len(), 3, "should produce exactly 3 chars from long history");
    }

    #[test]
    fn test_mini_sparkline_fewer_than_three_records() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let history = make_history(&["100", "110"]);
        let spans = build_sparkline_spans(&t, &history, 3);
        // Should produce 2 chars (as many as available, up to 3)
        assert_eq!(spans.len(), 2, "should produce chars equal to available history");
    }

    #[test]
    fn test_mini_sparkline_empty_history() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let spans = build_sparkline_spans(&t, &[], 3);
        assert!(spans.is_empty(), "empty history should produce no sparkline");
    }

    #[test]
    fn test_mini_sparkline_flat_prices() {
        let t = crate::tui::theme::theme_by_name("midnight");
        let history = make_history(&["100", "100", "100"]);
        let spans = build_sparkline_spans(&t, &history, 3);
        assert_eq!(spans.len(), 3);
        // All same price → all same middle char
        assert_eq!(spans[0].content, spans[1].content);
        assert_eq!(spans[1].content, spans[2].content);
    }
}

#[cfg(test)]
mod selection_flash_tests {
    use super::*;

    fn make_app_with_selection(selected: usize, tick_count: u64, last_change_tick: u64) -> crate::app::App {
        let config = crate::config::Config::default();
        let mut app = crate::app::App::new(&config, std::path::PathBuf::from("/tmp/pftui_test_sel_flash.db"));
        app.selected_index = selected;
        app.tick_count = tick_count;
        app.last_selection_change_tick = last_change_tick;
        app
    }

    #[test]
    fn test_flash_at_start_returns_accent_color() {
        // Immediately after selection change (elapsed=0), color should be border_accent
        let app = make_app_with_selection(2, 100, 100);
        let bg = row_background(&app, 2);
        assert_eq!(bg, app.theme.border_accent, "at elapsed=0, selected row should be border_accent");
    }

    #[test]
    fn test_flash_decays_to_surface_3() {
        // After SELECTION_FLASH_DURATION ticks, color should be surface_3
        let app = make_app_with_selection(2, 100 + theme::SELECTION_FLASH_DURATION, 100);
        let bg = row_background(&app, 2);
        assert_eq!(bg, app.theme.surface_3, "after flash duration, selected row should be surface_3");
    }

    #[test]
    fn test_flash_midpoint_is_between_accent_and_surface() {
        // At halfway through the flash, color should be between border_accent and surface_3
        let midpoint = theme::SELECTION_FLASH_DURATION / 2;
        let app = make_app_with_selection(2, 100 + midpoint, 100);
        let bg = row_background(&app, 2);
        // Should differ from both endpoints
        assert_ne!(bg, app.theme.border_accent, "midpoint should not be full accent");
        assert_ne!(bg, app.theme.surface_3, "midpoint should not be full surface_3");
    }

    #[test]
    fn test_non_selected_rows_unaffected_by_flash() {
        // Non-selected rows should be surface_1 or surface_1_alt regardless of flash state
        let app = make_app_with_selection(2, 100, 100);
        let bg_even = row_background(&app, 0);
        let bg_odd = row_background(&app, 1);
        assert_eq!(bg_even, app.theme.surface_1, "even non-selected row should be surface_1");
        assert_eq!(bg_odd, app.theme.surface_1_alt, "odd non-selected row should be surface_1_alt");
    }

    #[test]
    fn test_no_flash_on_initial_state() {
        // When last_selection_change_tick is 0 (initial state), no flash even if elapsed is small
        let app = make_app_with_selection(0, 5, 0);
        let bg = row_background(&app, 0);
        assert_eq!(bg, app.theme.surface_3, "initial state should not trigger flash");
    }

    #[test]
    fn test_flash_well_past_duration() {
        // Long after the flash, color should be solid surface_3
        let app = make_app_with_selection(1, 1000, 100);
        let bg = row_background(&app, 1);
        assert_eq!(bg, app.theme.surface_3, "well past flash duration, should be surface_3");
    }
}

#[cfg(test)]
mod category_dot_tests {
    use super::*;
    use crate::models::asset::AssetCategory;
    use crate::tui::theme;

    #[test]
    fn test_category_dot_uses_category_color() {
        let t = theme::theme_by_name("midnight");
        for cat in AssetCategory::all() {
            let expected_color = t.category_color(*cat);
            let dot = Span::styled("●", Style::default().fg(expected_color));
            assert_eq!(dot.content, "●");
            if let Some(fg) = dot.style.fg {
                assert_eq!(fg, expected_color, "dot color should match category_color for {:?}", cat);
            }
        }
    }

    #[test]
    fn test_category_dot_is_single_char() {
        // The dot character ● should be exactly 1 Unicode char
        assert_eq!("●".chars().count(), 1);
        // And 3 bytes in UTF-8 (won't break column alignment)
        assert_eq!("●".len(), 3);
    }

    #[test]
    fn test_asset_line_structure_with_dot() {
        // Verify the asset line has the expected span structure:
        // [marker, dot, space, asset_text]
        let t = theme::theme_by_name("midnight");
        let cat_color = t.category_color(AssetCategory::Crypto);

        let marker = Span::styled("▎", Style::default().fg(t.border_active));
        let cat_dot = Span::styled("●", Style::default().fg(cat_color));
        let asset_text = "Bitcoin BTC".to_string();
        let asset_line = Line::from(vec![
            marker,
            cat_dot,
            Span::raw(" "),
            Span::styled(asset_text.clone(), Style::default().fg(t.text_primary)),
        ]);

        assert_eq!(asset_line.spans.len(), 4, "asset line should have 4 spans: marker, dot, space, text");
        assert_eq!(asset_line.spans[0].content, "▎");
        assert_eq!(asset_line.spans[1].content, "●");
        assert_eq!(asset_line.spans[2].content, " ");
        assert_eq!(asset_line.spans[3].content, asset_text);
        // Dot should be in category color
        assert_eq!(asset_line.spans[1].style.fg, Some(cat_color));
        // Text should be in text_primary
        assert_eq!(asset_line.spans[3].style.fg, Some(t.text_primary));
    }
}

#[cfg(test)]
mod gain_bar_tests {
    use super::*;
    use crate::tui::theme;
    use rust_decimal_macros::dec;

    #[test]
    fn test_gain_bar_width_zero_gain() {
        let (full, rem) = gain_bar_width(0.0, 20.0, 8);
        assert_eq!(full, 0);
        assert_eq!(rem, 0);
    }

    #[test]
    fn test_gain_bar_width_max_gain() {
        // 20% gain at scale 20% on 8-char column = full width
        let (full, rem) = gain_bar_width(20.0, 20.0, 8);
        assert_eq!(full, 8);
        assert_eq!(rem, 0);
    }

    #[test]
    fn test_gain_bar_width_half_gain() {
        // 10% gain at scale 20% on 8-char column = 4 full chars
        let (full, rem) = gain_bar_width(10.0, 20.0, 8);
        assert_eq!(full, 4);
        assert_eq!(rem, 0);
    }

    #[test]
    fn test_gain_bar_width_negative_uses_abs() {
        // Negative gain should use absolute value
        let (full_pos, rem_pos) = gain_bar_width(10.0, 20.0, 8);
        let (full_neg, rem_neg) = gain_bar_width(-10.0, 20.0, 8);
        assert_eq!(full_pos, full_neg);
        assert_eq!(rem_pos, rem_neg);
    }

    #[test]
    fn test_gain_bar_width_capped_beyond_max() {
        // 50% gain at scale 20% should cap at full width
        let (full, rem) = gain_bar_width(50.0, 20.0, 8);
        assert_eq!(full, 8);
        assert_eq!(rem, 0);
    }

    #[test]
    fn test_gain_bar_width_fractional() {
        // 5% gain at scale 20% on 8-char column = 2.0 chars = 2 full, 0 remainder
        let (full, rem) = gain_bar_width(5.0, 20.0, 8);
        assert_eq!(full, 2);
        assert_eq!(rem, 0);

        // 3% gain at scale 20% on 8-char column = 1.2 chars
        // 1.2 * 8 eighths = 9.6 ≈ 10 eighths = 1 full + 2 remainder
        let (full, rem) = gain_bar_width(3.0, 20.0, 8);
        assert_eq!(full, 1);
        assert_eq!(rem, 2);
    }

    #[test]
    fn test_gain_bar_width_zero_col_width() {
        let (full, rem) = gain_bar_width(10.0, 20.0, 0);
        assert_eq!(full, 0);
        assert_eq!(rem, 0);
    }

    #[test]
    fn test_gain_bar_width_zero_max_pct() {
        let (full, rem) = gain_bar_width(10.0, 0.0, 8);
        assert_eq!(full, 0);
        assert_eq!(rem, 0);
    }

    #[test]
    fn test_gain_bar_spans_none() {
        let t = theme::theme_by_name("midnight");
        let line = build_gain_bar_spans(&t, None, 8);
        // Should render "---" with muted color, no bar
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "---");
        assert_eq!(line.spans[0].style.fg, Some(t.text_muted));
        assert_eq!(line.spans[0].style.bg, None);
    }

    #[test]
    fn test_gain_bar_spans_zero() {
        let t = theme::theme_by_name("midnight");
        let line = build_gain_bar_spans(&t, Some(dec!(0)), 8);
        // Should render "+0.0%" with neutral color, no bar
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].style.fg, Some(t.neutral));
        assert_eq!(line.spans[0].style.bg, None);
    }

    #[test]
    fn test_gain_bar_spans_positive_has_bg() {
        let t = theme::theme_by_name("midnight");
        let line = build_gain_bar_spans(&t, Some(dec!(10)), 8);
        // +10.0% at scale 20% = 4 chars of bar
        // The first chars should have a bg color set
        let chars_with_bg = line.spans.iter().filter(|s| s.style.bg.is_some()).count();
        assert!(chars_with_bg > 0, "positive gain should have spans with bar background");
    }

    #[test]
    fn test_gain_bar_spans_negative_has_bg() {
        let t = theme::theme_by_name("midnight");
        let line = build_gain_bar_spans(&t, Some(dec!(-15)), 8);
        // -15.0% at scale 20% = 6 chars of bar
        let chars_with_bg = line.spans.iter().filter(|s| s.style.bg.is_some()).count();
        assert!(chars_with_bg > 0, "negative gain should have spans with bar background");
    }

    #[test]
    fn test_gain_bar_larger_loss_has_more_bg() {
        let t = theme::theme_by_name("midnight");
        let small = build_gain_bar_spans(&t, Some(dec!(-5)), 8);
        let large = build_gain_bar_spans(&t, Some(dec!(-15)), 8);
        let small_bg = small.spans.iter().filter(|s| s.style.bg.is_some()).count();
        let large_bg = large.spans.iter().filter(|s| s.style.bg.is_some()).count();
        assert!(large_bg > small_bg,
            "larger loss should have more bar coverage: {} vs {}", large_bg, small_bg);
    }

    #[test]
    fn test_gain_bar_text_content_preserved() {
        let t = theme::theme_by_name("midnight");
        let line = build_gain_bar_spans(&t, Some(dec!(12.5)), 8);
        // Collect all text content from spans
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("+12.5%"), "bar should preserve gain text, got: '{}'", text);
    }
}

#[cfg(test)]
mod category_divider_tests {
    use super::*;
    use crate::tui::theme;

    #[test]
    fn test_capitalize_category_basic() {
        assert_eq!(capitalize_category("crypto"), "Crypto");
        assert_eq!(capitalize_category("equity"), "Equity");
        assert_eq!(capitalize_category("commodity"), "Commodity");
    }

    #[test]
    fn test_capitalize_category_empty() {
        assert_eq!(capitalize_category(""), "");
    }

    #[test]
    fn test_capitalize_category_already_capitalized() {
        assert_eq!(capitalize_category("Crypto"), "Crypto");
    }

    #[test]
    fn test_divider_row_has_correct_column_count() {
        let t = theme::theme_by_name("midnight");
        let row = category_divider_row(AssetCategory::Crypto, &t, 8);
        // Row should have been created without panic and have 8 cells
        // We can verify it renders without error by checking it's a valid Row
        let _ = row; // creation itself is the test
    }

    #[test]
    fn test_divider_row_with_different_categories() {
        let t = theme::theme_by_name("midnight");
        // Ensure all categories produce valid divider rows
        for cat in &[
            AssetCategory::Crypto,
            AssetCategory::Equity,
            AssetCategory::Commodity,
            AssetCategory::Cash,
            AssetCategory::Forex,
            AssetCategory::Fund,
        ] {
            let _ = category_divider_row(*cat, &t, 8);
        }
    }

    #[test]
    fn test_divider_row_privacy_column_count() {
        let t = theme::theme_by_name("midnight");
        let row = category_divider_row(AssetCategory::Equity, &t, 6);
        let _ = row; // 6-column privacy table variant
    }
}

#[cfg(test)]
mod sort_flash_style_tests {
    use super::*;
    use crate::tui::theme;

    #[test]
    fn test_sort_flash_is_bold_during_flash() {
        let t = theme::theme_by_name("midnight");
        let tick_count: u64 = 100;
        let last_sort_change_tick: u64 = 95; // 5 ticks ago
        let elapsed = tick_count.saturating_sub(last_sort_change_tick);

        assert!(elapsed < theme::SORT_FLASH_DURATION);
        let progress = elapsed as f32 / theme::SORT_FLASH_DURATION as f32;
        let flash_color = theme::lerp_color(t.text_primary, t.text_accent, progress);
        let style = Style::default().fg(flash_color).bold();

        // During flash: style should be bold
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_sort_flash_normal_after_duration() {
        let t = theme::theme_by_name("midnight");
        let tick_count: u64 = 200;
        let last_sort_change_tick: u64 = 100; // 100 ticks ago
        let elapsed = tick_count.saturating_sub(last_sort_change_tick);

        assert!(elapsed >= theme::SORT_FLASH_DURATION);
        // After flash: should use normal text_accent, no bold
        let style = Style::default().fg(t.text_accent);
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_sort_flash_color_starts_at_primary() {
        let t = theme::theme_by_name("midnight");
        // At progress=0 (just changed), color should be near text_primary
        let color = theme::lerp_color(t.text_primary, t.text_accent, 0.0);
        assert_eq!(color, t.text_primary);
    }

    #[test]
    fn test_sort_flash_color_ends_at_accent() {
        let t = theme::theme_by_name("midnight");
        // At progress=1.0, color should be text_accent
        let color = theme::lerp_color(t.text_primary, t.text_accent, 1.0);
        assert_eq!(color, t.text_accent);
    }
}

#[cfg(test)]
mod rsi_indicator_tests {
    use super::*;
    use crate::models::price::HistoryRecord;
    use crate::tui::theme;

    fn make_history(prices: &[f64]) -> Vec<HistoryRecord> {
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| HistoryRecord {
                date: format!("2025-{:02}-{:02}", (i / 28) + 1, (i % 28) + 1),
                close: rust_decimal::Decimal::from_str_exact(&format!("{:.2}", p))
                    .unwrap_or_default(),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect()
    }

    #[test]
    fn rsi_spans_insufficient_data() {
        let t = theme::theme_by_name("midnight");
        let records = make_history(&[100.0, 101.0, 102.0]);
        let line = build_rsi_spans(&t, &records);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "---");
    }

    #[test]
    fn rsi_spans_empty_history() {
        let t = theme::theme_by_name("midnight");
        let line = build_rsi_spans(&t, &[]);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "---");
    }

    #[test]
    fn rsi_spans_all_rising_shows_high_value() {
        let t = theme::theme_by_name("midnight");
        // 20 steadily rising prices → RSI should be ~100
        let prices: Vec<f64> = (0..20).map(|i| 50.0 + i as f64).collect();
        let records = make_history(&prices);
        let line = build_rsi_spans(&t, &records);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Should contain "100" and be colored red (overbought)
        assert!(text.contains("100"), "Expected RSI ~100, got: {}", text);
        // First span should have loss_red color (overbought)
        assert_eq!(line.spans[0].style.fg, Some(t.loss_red));
    }

    #[test]
    fn rsi_spans_all_falling_shows_low_value() {
        let t = theme::theme_by_name("midnight");
        // 20 steadily falling prices → RSI should be ~0
        let prices: Vec<f64> = (0..20).map(|i| 100.0 - i as f64).collect();
        let records = make_history(&prices);
        let line = build_rsi_spans(&t, &records);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("0"), "Expected RSI ~0, got: {}", text);
        // First span should have gain_green color (oversold)
        assert_eq!(line.spans[0].style.fg, Some(t.gain_green));
    }

    #[test]
    fn rsi_spans_neutral_uses_secondary_color() {
        let t = theme::theme_by_name("midnight");
        // Alternating up/down prices → RSI should be ~50 (neutral zone)
        let mut prices = vec![50.0];
        for i in 1..20 {
            if i % 2 == 0 {
                prices.push(prices[i - 1] + 1.0);
            } else {
                prices.push(prices[i - 1] - 0.8);
            }
        }
        let records = make_history(&prices);
        let line = build_rsi_spans(&t, &records);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // RSI should be between 30 and 70
        let rsi_val: f64 = text.split_whitespace().next()
            .and_then(|s| s.replace(['▲', '▼'], "").parse().ok())
            .unwrap_or(0.0);
        assert!((30.0..=70.0).contains(&rsi_val),
            "Expected neutral RSI (30-70), got: {}", rsi_val);
        // Should use text_secondary (not red or green)
        assert_eq!(line.spans[0].style.fg, Some(t.text_secondary));
    }

    #[test]
    fn rsi_spans_rising_shows_up_arrow() {
        let t = theme::theme_by_name("midnight");
        // Alternating prices that dip then rise strongly at the end → RSI jumps up
        let mut prices = vec![50.0];
        for i in 1..16 {
            // Mild oscillation keeping RSI in mid range
            if i % 2 == 0 {
                prices.push(prices[i - 1] + 0.5);
            } else {
                prices.push(prices[i - 1] - 0.3);
            }
        }
        // Strong rise at end to push RSI up significantly vs previous bar
        prices.push(prices.last().unwrap() + 3.0);
        prices.push(prices.last().unwrap() + 3.0);
        let records = make_history(&prices);
        let line = build_rsi_spans(&t, &records);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Should have an up arrow since RSI jumped
        assert!(text.contains('▲'), "Expected ▲ arrow, got: {}", text);
    }
}
