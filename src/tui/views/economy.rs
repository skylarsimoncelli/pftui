use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset::AssetCategory;
use crate::models::price::HistoryRecord;
use crate::tui::theme;

/// Braille sparkline characters for mini-charts (same as markets view).
const SPARKLINE_CHARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Number of days for mini sparkline.
const SPARKLINE_DAYS: usize = 7;

/// A single entry in the Economy dashboard table.
#[derive(Debug, Clone)]
pub struct EconomyItem {
    pub symbol: String,
    pub name: String,
    pub group: EconomyGroup,
    /// Yahoo Finance symbol for price/value lookup.
    pub yahoo_symbol: String,
}

/// Groups for visual organization in the economy table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomyGroup {
    Yields,
    Currency,
    Commodities,
    Volatility,
}

impl std::fmt::Display for EconomyGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EconomyGroup::Yields => write!(f, "Yields"),
            EconomyGroup::Currency => write!(f, "Currency"),
            EconomyGroup::Commodities => write!(f, "Commod"),
            EconomyGroup::Volatility => write!(f, "Volatility"),
        }
    }
}

/// Returns the fixed list of economy/macro symbols.
pub fn economy_symbols() -> Vec<EconomyItem> {
    vec![
        // Treasury Yields
        EconomyItem { symbol: "2Y".into(), name: "2-Year Treasury Yield".into(), group: EconomyGroup::Yields, yahoo_symbol: "^IRX".into() },
        EconomyItem { symbol: "5Y".into(), name: "5-Year Treasury Yield".into(), group: EconomyGroup::Yields, yahoo_symbol: "^FVX".into() },
        EconomyItem { symbol: "10Y".into(), name: "10-Year Treasury Yield".into(), group: EconomyGroup::Yields, yahoo_symbol: "^TNX".into() },
        EconomyItem { symbol: "30Y".into(), name: "30-Year Treasury Yield".into(), group: EconomyGroup::Yields, yahoo_symbol: "^TYX".into() },
        // Currency
        EconomyItem { symbol: "DXY".into(), name: "US Dollar Index".into(), group: EconomyGroup::Currency, yahoo_symbol: "DX-Y.NYB".into() },
        EconomyItem { symbol: "EUR".into(), name: "Euro / USD".into(), group: EconomyGroup::Currency, yahoo_symbol: "EURUSD=X".into() },
        EconomyItem { symbol: "GBP".into(), name: "Pound / USD".into(), group: EconomyGroup::Currency, yahoo_symbol: "GBPUSD=X".into() },
        EconomyItem { symbol: "JPY".into(), name: "USD / Yen".into(), group: EconomyGroup::Currency, yahoo_symbol: "JPY=X".into() },
        EconomyItem { symbol: "CNY".into(), name: "USD / Yuan".into(), group: EconomyGroup::Currency, yahoo_symbol: "CNY=X".into() },
        // Commodities
        EconomyItem { symbol: "Gold".into(), name: "Gold Futures".into(), group: EconomyGroup::Commodities, yahoo_symbol: "GC=F".into() },
        EconomyItem { symbol: "Oil".into(), name: "Crude Oil WTI".into(), group: EconomyGroup::Commodities, yahoo_symbol: "CL=F".into() },
        EconomyItem { symbol: "Copper".into(), name: "Copper Futures".into(), group: EconomyGroup::Commodities, yahoo_symbol: "HG=F".into() },
        EconomyItem { symbol: "NatGas".into(), name: "Natural Gas".into(), group: EconomyGroup::Commodities, yahoo_symbol: "NG=F".into() },
        // Volatility
        EconomyItem { symbol: "VIX".into(), name: "CBOE Volatility Index".into(), group: EconomyGroup::Volatility, yahoo_symbol: "^VIX".into() },
    ]
}

/// Returns the AssetCategory for price fetching based on economy group.
pub fn category_for_group(group: EconomyGroup) -> AssetCategory {
    match group {
        EconomyGroup::Yields => AssetCategory::Fund,
        EconomyGroup::Currency => AssetCategory::Forex,
        EconomyGroup::Commodities => AssetCategory::Commodity,
        EconomyGroup::Volatility => AssetCategory::Equity,
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let items = economy_symbols();

    let header = Row::new(vec![
        Cell::from("Symbol"),
        Cell::from("Name"),
        Cell::from("Group"),
        Cell::from("Value"),
        Cell::from("Day %"),
        Cell::from("7D"),
        Cell::from("7D %"),
        Cell::from("Trend"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let mut rows: Vec<Row> = Vec::with_capacity(items.len() + 4);
    let mut prev_group: Option<EconomyGroup> = None;
    let yield_curve = yield_curve_status(app);

    for (i, item) in items.iter().enumerate() {
        // Insert yield curve status row after yields group ends
        if prev_group == Some(EconomyGroup::Yields) && item.group != EconomyGroup::Yields {
            let (curve_label, curve_color) = match yield_curve {
                YieldCurveState::Normal(spread) => (
                    format!("  Yield Curve: NORMAL  2Y-10Y spread {:+.2}bps", spread),
                    t.gain_green,
                ),
                YieldCurveState::Inverted(spread) => (
                    format!("  Yield Curve: INVERTED  2Y-10Y spread {:.2}bps", spread),
                    t.loss_red,
                ),
                YieldCurveState::Flat => (
                    "  Yield Curve: FLAT  2Y-10Y spread ~0bps".to_string(),
                    t.text_accent,
                ),
                YieldCurveState::Unknown => (
                    "  Yield Curve: ---".to_string(),
                    t.text_muted,
                ),
            };
            rows.push(
                Row::new(vec![Cell::from(Span::styled(
                    curve_label,
                    Style::default().fg(curve_color).italic(),
                ))])
                .style(Style::default().bg(t.surface_0))
                .height(1),
            );
        }

        // Add a group separator row when group changes
        if prev_group.is_some() && prev_group != Some(item.group) {
            rows.push(
                Row::new(vec![Cell::from("")])
                    .style(Style::default().bg(t.surface_0))
                    .height(1),
            );
        }
        prev_group = Some(item.group);

        let group_color = match item.group {
            EconomyGroup::Yields => t.cat_fund,
            EconomyGroup::Currency => t.cat_forex,
            EconomyGroup::Commodities => t.cat_commodity,
            EconomyGroup::Volatility => t.cat_crypto,
        };

        let row_bg = if i == app.economy_selected_index {
            t.surface_3
        } else if i % 2 == 0 {
            t.surface_1
        } else {
            t.surface_0
        };

        // Look up the live price from the app's price map
        let price = app.prices.get(&item.yahoo_symbol).copied();
        let price_str = match price {
            Some(p) => format_value(p, item.group),
            None => "---".to_string(),
        };

        // Compute daily change % from history
        let change_pct = compute_change_pct(app, &item.yahoo_symbol);
        let (change_str, change_color) = match change_pct {
            Some(pct) => {
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                let color = theme::gain_intensity_color(t, f);
                (format!("{:+.2}%", f), color)
            }
            None => ("---".to_string(), t.text_muted),
        };

        // 7D mini sparkline
        let sparkline_cell = build_mini_sparkline(app, &item.yahoo_symbol, t);

        // 7D momentum
        let momentum = compute_7d_momentum(app, &item.yahoo_symbol);
        let (momentum_str, momentum_color) = match momentum {
            Some(pct) => {
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                let color = theme::gain_intensity_color(t, f);
                (format!("{:+.2}%", f), color)
            }
            None => ("---".to_string(), t.text_muted),
        };

        // Trend arrow based on 7D momentum
        let (arrow, arrow_color) = trend_arrow(momentum, t);

        rows.push(
            Row::new(vec![
                Cell::from(Span::styled(
                    item.symbol.clone(),
                    Style::default().fg(t.text_primary).bold(),
                )),
                Cell::from(Span::styled(
                    item.name.clone(),
                    Style::default().fg(t.text_secondary),
                )),
                Cell::from(Span::styled(
                    format!("{}", item.group),
                    Style::default().fg(group_color),
                )),
                Cell::from(Span::styled(
                    price_str,
                    Style::default().fg(t.text_primary),
                )),
                Cell::from(Span::styled(
                    change_str,
                    Style::default().fg(change_color),
                )),
                sparkline_cell,
                Cell::from(Span::styled(
                    momentum_str,
                    Style::default().fg(momentum_color),
                )),
                Cell::from(Span::styled(
                    arrow,
                    Style::default().fg(arrow_color),
                )),
            ])
            .style(Style::default().bg(row_bg))
            .height(1),
        );
    }

    // If the last group is Yields (edge case), append yield curve status at the end
    if prev_group == Some(EconomyGroup::Yields) {
        let (curve_label, curve_color) = match yield_curve {
            YieldCurveState::Normal(spread) => (
                format!("  Yield Curve: NORMAL  2Y-10Y spread {:+.2}bps", spread),
                t.gain_green,
            ),
            YieldCurveState::Inverted(spread) => (
                format!("  Yield Curve: INVERTED  2Y-10Y spread {:.2}bps", spread),
                t.loss_red,
            ),
            YieldCurveState::Flat => (
                "  Yield Curve: FLAT  2Y-10Y spread ~0bps".to_string(),
                t.text_accent,
            ),
            YieldCurveState::Unknown => (
                "  Yield Curve: ---".to_string(),
                t.text_muted,
            ),
        };
        rows.push(
            Row::new(vec![Cell::from(Span::styled(
                curve_label,
                Style::default().fg(curve_color).italic(),
            ))])
            .style(Style::default().bg(t.surface_0))
            .height(1),
        );
    }

    let widths = [
        Constraint::Length(8),
        Constraint::Min(14),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(9),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .title(Span::styled(
                    " Economy ",
                    Style::default().fg(t.text_accent).bold(),
                ))
                .style(Style::default().bg(t.surface_0)),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}

/// Yield curve state derived from 2Y and 10Y treasury yields.
#[derive(Debug, Clone, PartialEq)]
enum YieldCurveState {
    /// Normal: 10Y > 2Y, spread in basis points
    Normal(f64),
    /// Inverted: 2Y > 10Y, spread in basis points (negative)
    Inverted(f64),
    /// Flat: spread within ±5 bps
    Flat,
    /// Data unavailable
    Unknown,
}

/// Compute yield curve status from 2Y (^IRX) and 10Y (^TNX) prices.
/// Yahoo Finance reports these as e.g. 4.325 meaning 4.325%.
/// Spread in basis points = (10Y - 2Y) × 100.
fn yield_curve_status(app: &App) -> YieldCurveState {
    let yield_2y = match app.prices.get("^IRX") {
        Some(p) => p.to_string().parse::<f64>().unwrap_or(0.0),
        None => return YieldCurveState::Unknown,
    };
    let yield_10y = match app.prices.get("^TNX") {
        Some(p) => p.to_string().parse::<f64>().unwrap_or(0.0),
        None => return YieldCurveState::Unknown,
    };
    let spread_bps = (yield_10y - yield_2y) * 100.0;
    if spread_bps.abs() < 5.0 {
        YieldCurveState::Flat
    } else if spread_bps > 0.0 {
        YieldCurveState::Normal(spread_bps)
    } else {
        YieldCurveState::Inverted(spread_bps)
    }
}

/// Return a trend arrow and color based on 7D momentum.
/// ↑ green for >0.5%, ↓ red for <-0.5%, → muted for flat.
fn trend_arrow(momentum: Option<Decimal>, t: &theme::Theme) -> (String, Color) {
    match momentum {
        Some(pct) => {
            let f: f64 = pct.to_string().parse().unwrap_or(0.0);
            if f > 0.5 {
                ("↑".to_string(), t.gain_green)
            } else if f < -0.5 {
                ("↓".to_string(), t.loss_red)
            } else {
                ("→".to_string(), t.text_muted)
            }
        }
        None => ("—".to_string(), t.text_muted),
    }
}

/// Build a mini sparkline cell from the last N days of price history.
fn build_mini_sparkline<'a>(
    app: &App,
    yahoo_symbol: &str,
    theme: &'a theme::Theme,
) -> Cell<'a> {
    let history = match app.price_history.get(yahoo_symbol) {
        Some(h) if h.len() >= 2 => h,
        _ => return Cell::from(Span::styled("  ---  ", Style::default().fg(theme.text_muted))),
    };

    let spans = build_sparkline_spans(theme, history, SPARKLINE_DAYS);
    if spans.is_empty() {
        Cell::from(Span::styled("  ---  ", Style::default().fg(theme.text_muted)))
    } else {
        Cell::from(Line::from(spans))
    }
}

/// Build sparkline character spans from price history records.
fn build_sparkline_spans<'a>(
    theme: &'a theme::Theme,
    records: &[HistoryRecord],
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

/// Compute 7-day momentum: (latest - 7d_ago) / 7d_ago × 100.
fn compute_7d_momentum(app: &App, yahoo_symbol: &str) -> Option<Decimal> {
    let history = app.price_history.get(yahoo_symbol)?;
    if history.len() < 2 {
        return None;
    }
    let latest = &history[history.len() - 1];
    let lookback = SPARKLINE_DAYS.min(history.len() - 1);
    let baseline = &history[history.len() - 1 - lookback];
    if baseline.close == dec!(0) {
        return None;
    }
    Some((latest.close - baseline.close) / baseline.close * dec!(100))
}

/// Compute daily change % from price history: (latest_close - prev_close) / prev_close * 100
fn compute_change_pct(app: &App, yahoo_symbol: &str) -> Option<Decimal> {
    let history = app.price_history.get(yahoo_symbol)?;
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

/// Format a value appropriately based on economy group.
/// Yields display as percentages, currencies/commodities as prices.
fn format_value(p: Decimal, group: EconomyGroup) -> String {
    let f: f64 = p.to_string().parse().unwrap_or(0.0);
    match group {
        EconomyGroup::Yields => format!("{:.3}%", f),
        _ => {
            if f.abs() >= 10_000.0 {
                format!("{:.0}", f)
            } else if f.abs() >= 1.0 {
                format!("{:.2}", f)
            } else {
                format!("{:.4}", f)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::PathBuf;

    fn test_app() -> App {
        let config = Config::default();
        App::new(&config, PathBuf::from(":memory:"))
    }

    #[test]
    fn economy_symbols_has_expected_count() {
        let items = economy_symbols();
        assert_eq!(items.len(), 14);
    }

    #[test]
    fn economy_symbols_has_all_groups() {
        let items = economy_symbols();
        let has_yields = items.iter().any(|i| i.group == EconomyGroup::Yields);
        let has_currency = items.iter().any(|i| i.group == EconomyGroup::Currency);
        let has_commodities = items.iter().any(|i| i.group == EconomyGroup::Commodities);
        let has_volatility = items.iter().any(|i| i.group == EconomyGroup::Volatility);
        assert!(has_yields, "missing yields items");
        assert!(has_currency, "missing currency items");
        assert!(has_commodities, "missing commodities items");
        assert!(has_volatility, "missing volatility items");
    }

    #[test]
    fn economy_symbols_yahoo_symbols_unique() {
        let items = economy_symbols();
        let mut seen = std::collections::HashSet::new();
        for item in &items {
            assert!(
                seen.insert(&item.yahoo_symbol),
                "duplicate yahoo_symbol: {}",
                item.yahoo_symbol
            );
        }
    }

    #[test]
    fn economy_symbols_yields_first() {
        let items = economy_symbols();
        assert_eq!(items[0].symbol, "2Y");
        assert_eq!(items[0].group, EconomyGroup::Yields);
    }

    #[test]
    fn format_value_yields_shows_percent() {
        let p = Decimal::new(4325, 3); // 4.325
        assert_eq!(format_value(p, EconomyGroup::Yields), "4.325%");
    }

    #[test]
    fn format_value_currency_large() {
        let p = Decimal::new(10452, 2); // 104.52
        assert_eq!(format_value(p, EconomyGroup::Currency), "104.52");
    }

    #[test]
    fn format_value_commodity_large() {
        let p = Decimal::new(5234500, 2); // 52345.00
        assert_eq!(format_value(p, EconomyGroup::Commodities), "52345");
    }

    #[test]
    fn format_value_currency_small() {
        let p = Decimal::new(8321, 4); // 0.8321
        assert_eq!(format_value(p, EconomyGroup::Currency), "0.8321");
    }

    #[test]
    fn category_for_group_mapping() {
        assert_eq!(category_for_group(EconomyGroup::Yields), AssetCategory::Fund);
        assert_eq!(category_for_group(EconomyGroup::Currency), AssetCategory::Forex);
        assert_eq!(category_for_group(EconomyGroup::Commodities), AssetCategory::Commodity);
        assert_eq!(category_for_group(EconomyGroup::Volatility), AssetCategory::Equity);
    }

    // --- Yield curve tests ---

    #[test]
    fn yield_curve_normal() {
        let mut app = test_app();
        app.prices.insert("^IRX".to_string(), dec!(4.000)); // 2Y = 4.0%
        app.prices.insert("^TNX".to_string(), dec!(4.500)); // 10Y = 4.5%
        match yield_curve_status(&app) {
            YieldCurveState::Normal(spread) => {
                assert!((spread - 50.0).abs() < 0.1, "expected ~50bps, got {spread}");
            }
            other => panic!("expected Normal, got {other:?}"),
        }
    }

    #[test]
    fn yield_curve_inverted() {
        let mut app = test_app();
        app.prices.insert("^IRX".to_string(), dec!(5.000)); // 2Y = 5.0%
        app.prices.insert("^TNX".to_string(), dec!(4.200)); // 10Y = 4.2%
        match yield_curve_status(&app) {
            YieldCurveState::Inverted(spread) => {
                assert!(spread < 0.0, "expected negative spread, got {spread}");
            }
            other => panic!("expected Inverted, got {other:?}"),
        }
    }

    #[test]
    fn yield_curve_flat() {
        let mut app = test_app();
        app.prices.insert("^IRX".to_string(), dec!(4.300));
        app.prices.insert("^TNX".to_string(), dec!(4.320)); // 2bps spread
        assert_eq!(yield_curve_status(&app), YieldCurveState::Flat);
    }

    #[test]
    fn yield_curve_unknown_missing_data() {
        let app = test_app();
        assert_eq!(yield_curve_status(&app), YieldCurveState::Unknown);
    }

    // --- Trend arrow tests ---

    #[test]
    fn trend_arrow_up() {
        let t = theme::midnight();
        let (arrow, color) = trend_arrow(Some(dec!(2.5)), &t);
        assert_eq!(arrow, "↑");
        assert_eq!(color, t.gain_green);
    }

    #[test]
    fn trend_arrow_down() {
        let t = theme::midnight();
        let (arrow, color) = trend_arrow(Some(dec!(-1.8)), &t);
        assert_eq!(arrow, "↓");
        assert_eq!(color, t.loss_red);
    }

    #[test]
    fn trend_arrow_flat() {
        let t = theme::midnight();
        let (arrow, color) = trend_arrow(Some(dec!(0.3)), &t);
        assert_eq!(arrow, "→");
        assert_eq!(color, t.text_muted);
    }

    #[test]
    fn trend_arrow_none() {
        let t = theme::midnight();
        let (arrow, color) = trend_arrow(None, &t);
        assert_eq!(arrow, "—");
        assert_eq!(color, t.text_muted);
    }

    // --- Sparkline tests ---

    #[test]
    fn sparkline_spans_ascending() {
        let t = theme::midnight();
        let records: Vec<HistoryRecord> = (1..=7)
            .map(|i| HistoryRecord {
                date: format!("2026-01-{:02}", i),
                close: Decimal::new(i * 100, 0),
                volume: None,
            })
            .collect();
        let spans = build_sparkline_spans(&t, &records, 7);
        assert_eq!(spans.len(), 7);
        // First should be lowest bar, last should be highest
        assert_eq!(spans[0].content.as_ref(), "▁");
        assert_eq!(spans[6].content.as_ref(), "█");
    }

    #[test]
    fn sparkline_spans_empty() {
        let t = theme::midnight();
        let spans = build_sparkline_spans(&t, &[], 7);
        assert!(spans.is_empty());
    }

    #[test]
    fn sparkline_spans_flat() {
        let t = theme::midnight();
        let records: Vec<HistoryRecord> = (1..=5)
            .map(|i| HistoryRecord {
                date: format!("2026-01-{:02}", i),
                close: dec!(100),
                volume: None,
            })
            .collect();
        let spans = build_sparkline_spans(&t, &records, 7);
        assert_eq!(spans.len(), 5);
        // All should be middle bar when flat
        for span in &spans {
            assert_eq!(span.content.as_ref(), "▄");
        }
    }

    // --- 7D momentum tests ---

    #[test]
    fn momentum_7d_basic() {
        let mut app = test_app();
        let records: Vec<HistoryRecord> = (0..=7)
            .map(|i| HistoryRecord {
                date: format!("2026-01-{:02}", i + 1),
                close: Decimal::new(100 + i * 10, 0), // 100, 110, ..., 170
                volume: None,
            })
            .collect();
        app.price_history.insert("^TNX".to_string(), records);
        let m = compute_7d_momentum(&app, "^TNX");
        assert!(m.is_some());
        let pct: f64 = m.unwrap().to_string().parse().unwrap();
        assert!(pct > 0.0, "expected positive momentum, got {pct}");
    }

    #[test]
    fn momentum_7d_insufficient_data() {
        let mut app = test_app();
        app.price_history.insert(
            "^TNX".to_string(),
            vec![HistoryRecord {
                date: "2026-01-01".to_string(),
                close: dec!(100),
                volume: None,
            }],
        );
        assert!(compute_7d_momentum(&app, "^TNX").is_none());
    }

    #[test]
    fn sparkline_chars_count() {
        assert_eq!(SPARKLINE_CHARS.len(), 8);
    }
}
