use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset::AssetCategory;
use crate::models::price::HistoryRecord;
use crate::tui::theme;
use crate::tui::widgets::skeleton;

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
        EconomyItem {
            symbol: "2Y".into(),
            name: "2-Year Treasury Yield".into(),
            group: EconomyGroup::Yields,
            yahoo_symbol: "^IRX".into(),
        },
        EconomyItem {
            symbol: "5Y".into(),
            name: "5-Year Treasury Yield".into(),
            group: EconomyGroup::Yields,
            yahoo_symbol: "^FVX".into(),
        },
        EconomyItem {
            symbol: "10Y".into(),
            name: "10-Year Treasury Yield".into(),
            group: EconomyGroup::Yields,
            yahoo_symbol: "^TNX".into(),
        },
        EconomyItem {
            symbol: "30Y".into(),
            name: "30-Year Treasury Yield".into(),
            group: EconomyGroup::Yields,
            yahoo_symbol: "^TYX".into(),
        },
        // Currency
        EconomyItem {
            symbol: "DXY".into(),
            name: "US Dollar Index".into(),
            group: EconomyGroup::Currency,
            yahoo_symbol: "DX-Y.NYB".into(),
        },
        EconomyItem {
            symbol: "EUR".into(),
            name: "Euro / USD".into(),
            group: EconomyGroup::Currency,
            yahoo_symbol: "EURUSD=X".into(),
        },
        EconomyItem {
            symbol: "GBP".into(),
            name: "Pound / USD".into(),
            group: EconomyGroup::Currency,
            yahoo_symbol: "GBPUSD=X".into(),
        },
        EconomyItem {
            symbol: "JPY".into(),
            name: "USD / Yen".into(),
            group: EconomyGroup::Currency,
            yahoo_symbol: "JPY=X".into(),
        },
        EconomyItem {
            symbol: "CNY".into(),
            name: "USD / Yuan".into(),
            group: EconomyGroup::Currency,
            yahoo_symbol: "CNY=X".into(),
        },
        // Commodities
        EconomyItem {
            symbol: "Gold".into(),
            name: "Gold Futures".into(),
            group: EconomyGroup::Commodities,
            yahoo_symbol: "GC=F".into(),
        },
        EconomyItem {
            symbol: "Silver".into(),
            name: "Silver Futures".into(),
            group: EconomyGroup::Commodities,
            yahoo_symbol: "SI=F".into(),
        },
        EconomyItem {
            symbol: "Oil".into(),
            name: "Crude Oil WTI".into(),
            group: EconomyGroup::Commodities,
            yahoo_symbol: "CL=F".into(),
        },
        EconomyItem {
            symbol: "Brent".into(),
            name: "Crude Oil Brent".into(),
            group: EconomyGroup::Commodities,
            yahoo_symbol: "BZ=F".into(),
        },
        EconomyItem {
            symbol: "Copper".into(),
            name: "Copper Futures".into(),
            group: EconomyGroup::Commodities,
            yahoo_symbol: "HG=F".into(),
        },
        EconomyItem {
            symbol: "NatGas".into(),
            name: "Natural Gas".into(),
            group: EconomyGroup::Commodities,
            yahoo_symbol: "NG=F".into(),
        },
        // Volatility
        EconomyItem {
            symbol: "VIX".into(),
            name: "CBOE Volatility Index".into(),
            group: EconomyGroup::Volatility,
            yahoo_symbol: "^VIX".into(),
        },
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

pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    // Split into: top strip (3 rows) + body (rest)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10)])
        .split(area);

    render_top_strip(frame, outer[0], app);

    // Body: left table (~65%) + right panel (yield curve + derived metrics, ~35%)
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(outer[1]);

    // Left side: macro table (top) + global macro panel (bottom ~10 rows)
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(10)])
        .split(body[0]);

    render_macro_table(frame, left[0], app);
    render_global_macro_panel(frame, left[1], app);

    // Right panel: BLS indicators (top) + yield curve chart + sentiment + calendar + predictions
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // selected indicator
            Constraint::Length(9), // BLS indicators panel
            Constraint::Percentage(25),
            Constraint::Length(7),
            Constraint::Length(11), // Calendar panel: 7 days + header + borders
            Constraint::Min(8),
        ])
        .split(body[1]);

    render_selected_indicator_panel(frame, right[0], app);
    render_bls_indicators(frame, right[1], app);
    render_yield_curve_chart(frame, right[2], app);
    render_sentiment_panel(frame, right[3], app);
    render_calendar_panel(frame, right[4], app);
    render_predictions_panel(frame, right[5], app);
}

fn render_selected_indicator_panel(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let items = economy_symbols();
    let Some(item) = items.get(app.economy_selected_index) else {
        return;
    };
    let price = app.prices.get(&item.yahoo_symbol).copied();
    let change = app
        .price_history
        .get(&item.yahoo_symbol)
        .and_then(|history| {
            if history.len() < 2 {
                return None;
            }
            let latest = history.last()?.close;
            let prev = history.get(history.len().saturating_sub(2))?.close;
            if prev == dec!(0) {
                return None;
            }
            Some((latest - prev) / prev * dec!(100))
        });

    let lines = vec![
        Line::from(vec![
            Span::styled("Selected ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(&item.symbol, Style::default().fg(t.text_primary).bold()),
        ]),
        Line::from(item.name.clone()),
        Line::from(vec![
            Span::styled("Group: ", Style::default().fg(t.text_secondary)),
            Span::styled(item.group.to_string(), Style::default().fg(t.text_primary)),
        ]),
        Line::from(vec![
            Span::styled("Value: ", Style::default().fg(t.text_secondary)),
            Span::styled(
                price
                    .map(|v| format!("{:.2}", v))
                    .unwrap_or_else(|| "---".to_string()),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(vec![
            Span::styled("1D:   ", Style::default().fg(t.text_secondary)),
            Span::styled(
                change
                    .map(|v| format!("{:+.2}%", v))
                    .unwrap_or_else(|| "---".to_string()),
                Style::default().fg(t.text_primary),
            ),
        ]),
        Line::from(Span::styled(
            "c chart  n news  j/k navigate indicators",
            Style::default().fg(t.text_muted),
        )),
    ];

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_set(theme::BORDER_ACTIVE)
            .border_style(Style::default().fg(t.border_inactive))
            .title(Span::styled(
                " Selected Indicator ",
                Style::default().fg(t.text_accent).bold(),
            ))
            .style(Style::default().bg(t.surface_0)),
    );
    frame.render_widget(paragraph, area);
}

/// BLS Economic Indicators panel — CPI, unemployment, NFP, hourly earnings.
fn render_bls_indicators(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let econ = &app.economic_data;

    // Get latest BLS data points
    let cpi = app.bls_data.get(crate::data::bls::SERIES_CPI_U);
    let unemployment = app.bls_data.get(crate::data::bls::SERIES_UNEMPLOYMENT);
    let nfp = app.bls_data.get(crate::data::bls::SERIES_NFP);
    let earnings = app.bls_data.get(crate::data::bls::SERIES_HOURLY_EARNINGS);

    let mut lines = Vec::new();

    // CPI
    if let Some(cpi_econ) = econ.get("cpi") {
        let cpi_str = format!("CPI: {:.2}%", cpi_econ.value);
        let date_str = format!(" ({})", format_fetched_date(&cpi_econ.fetched_at));
        lines.push(Line::from(vec![
            Span::styled("CPI ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(cpi_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else if let Some(cpi_latest) = cpi {
        let cpi_str = format!("CPI: {:.1}", cpi_latest.value);
        let date_str = format!(" ({})", cpi_latest.date.format("%b %Y"));
        lines.push(Line::from(vec![
            Span::styled("CPI ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(cpi_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "CPI: ---",
            Style::default().fg(t.text_muted),
        )));
    }

    // Unemployment Rate
    if let Some(unemp_econ) = econ.get("unemployment_rate") {
        let unemp_str = format!("{:.2}%", unemp_econ.value);
        let date_str = format!(" ({})", format_fetched_date(&unemp_econ.fetched_at));
        lines.push(Line::from(vec![
            Span::styled("Unemp Rate ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(unemp_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else if let Some(unemp) = unemployment {
        let unemp_str = format!("{:.1}%", unemp.value);
        let date_str = format!(" ({})", unemp.date.format("%b %Y"));
        lines.push(Line::from(vec![
            Span::styled("Unemp Rate ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(unemp_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "Unemp Rate: ---",
            Style::default().fg(t.text_muted),
        )));
    }

    // NFP (Nonfarm Payrolls)
    if let Some(nfp_econ) = econ.get("nfp") {
        let nfp_k = nfp_econ.value / rust_decimal_macros::dec!(1000);
        let nfp_str = format!("{:.0}k", nfp_k);
        let date_str = format!(" ({})", format_fetched_date(&nfp_econ.fetched_at));
        lines.push(Line::from(vec![
            Span::styled("NFP ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(nfp_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else if let Some(nfp_data) = nfp {
        let nfp_k = nfp_data.value / rust_decimal_macros::dec!(1000);
        let nfp_str = format!("{:.0}k", nfp_k);
        let date_str = format!(" ({})", nfp_data.date.format("%b %Y"));
        lines.push(Line::from(vec![
            Span::styled("NFP ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(nfp_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "NFP: ---",
            Style::default().fg(t.text_muted),
        )));
    }

    // Average Hourly Earnings / PPI
    if let Some(ppi_econ) = econ.get("ppi") {
        let ppi_str = format!("PPI: {:.2}%", ppi_econ.value);
        let date_str = format!(" ({})", format_fetched_date(&ppi_econ.fetched_at));
        lines.push(Line::from(vec![
            Span::styled("PPI ", Style::default().fg(t.text_secondary).bold()),
            Span::styled(ppi_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else if let Some(earn) = earnings {
        let earn_str = format!("${:.2}", earn.value);
        let date_str = format!(" ({})", earn.date.format("%b %Y"));
        lines.push(Line::from(vec![
            Span::styled(
                "Avg Hrly Earnings ",
                Style::default().fg(t.text_secondary).bold(),
            ),
            Span::styled(earn_str, Style::default().fg(t.text_primary)),
            Span::styled(date_str, Style::default().fg(t.text_muted)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "Avg Hrly Earnings: ---",
            Style::default().fg(t.text_muted),
        )));
    }

    // Note about data refresh
    lines.push(Line::from(""));
    if let Some(fed) = econ.get("fed_funds_rate") {
        lines.push(Line::from(Span::styled(
            format!("Fed Funds: {:.2}%  ({})", fed.value, fed.source),
            Style::default().fg(t.text_muted).italic(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Monthly data from BLS",
            Style::default().fg(t.text_muted).italic(),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_ACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Economic Indicators ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_0));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn format_fetched_date(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|d| d.format("%b %d").to_string())
        .unwrap_or_else(|_| "recent".to_string())
}

/// Top strip: key macro numbers at a glance — DXY, VIX, 10Y, Gold, Oil, BTC.
fn render_top_strip(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    // Key indicators to show in the strip
    let indicators: &[(&str, &str)] = &[
        ("DXY", "DX-Y.NYB"),
        ("VIX", "^VIX"),
        ("10Y", "^TNX"),
        ("Gold", "GC=F"),
        ("Oil", "CL=F"),
        ("Silver", "SI=F"),
    ];

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" ", Style::default()));

    for (i, (label, yahoo_sym)) in indicators.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                "  │  ",
                Style::default().fg(t.border_inactive),
            ));
        }
        spans.push(Span::styled(
            format!("{label} "),
            Style::default().fg(t.text_secondary).bold(),
        ));

        let price = app.prices.get(*yahoo_sym).copied();
        let group = if *label == "10Y" {
            EconomyGroup::Yields
        } else {
            EconomyGroup::Commodities
        };
        let val_str = match price {
            Some(p) => format_value(p, group),
            None => "---".into(),
        };
        spans.push(Span::styled(val_str, Style::default().fg(t.text_primary)));

        // Day change
        let change = compute_change_pct(app, yahoo_sym);
        let (chg_str, chg_color) = match change {
            Some(pct) => {
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                (format!(" {:+.1}%", f), theme::gain_intensity_color(t, f))
            }
            None => (" ---".into(), t.text_muted),
        };
        spans.push(Span::styled(chg_str, Style::default().fg(chg_color)));
    }

    let line = Line::from(spans);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_ACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Key Numbers ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_0));

    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

/// Main macro indicators table (left panel).
fn render_macro_table(frame: &mut Frame, area: Rect, app: &mut App) {
    let t = &app.theme;
    app.page_table_area = Some(area);
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

    // Show skeleton placeholder rows while waiting for initial price data
    let show_skeleton = !app.prices_live;
    if show_skeleton {
        let col_widths = [6, 12, 10, 10, 7, 5, 7, 4];
        rows = skeleton::skeleton_rows(t, app.tick_count, &col_widths, 8);
    }

    let mut prev_group: Option<EconomyGroup> = None;
    let yield_curve = yield_curve_status(app);

    for (i, item) in items.iter().enumerate() {
        if show_skeleton {
            break;
        }
        // Insert yield curve status row after yields group ends
        if prev_group == Some(EconomyGroup::Yields) && item.group != EconomyGroup::Yields {
            let (curve_label, curve_color) = yield_curve_label(&yield_curve, t);
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
                Cell::from(Span::styled(price_str, Style::default().fg(t.text_primary))),
                Cell::from(Span::styled(change_str, Style::default().fg(change_color))),
                sparkline_cell,
                Cell::from(Span::styled(
                    momentum_str,
                    Style::default().fg(momentum_color),
                )),
                Cell::from(Span::styled(arrow, Style::default().fg(arrow_color))),
            ])
            .style(Style::default().bg(row_bg))
            .height(1),
        );
    }

    // If the last group is Yields (edge case), append yield curve status at the end
    if prev_group == Some(EconomyGroup::Yields) {
        let (curve_label, curve_color) = yield_curve_label(&yield_curve, t);
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
                .border_set(theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .title(Span::styled(
                    " Macro Indicators ",
                    Style::default().fg(t.text_accent).bold(),
                ))
                .style(Style::default().bg(t.surface_0)),
        )
        .row_highlight_style(Style::default().bg(t.surface_3));

    frame.render_widget(table, area);
}

/// Format yield curve state into a label and color.
fn yield_curve_label(state: &YieldCurveState, t: &theme::Theme) -> (String, Color) {
    match state {
        YieldCurveState::Normal(spread) => (
            format!("  Yield Curve: NORMAL  2Y-10Y {:+.0}bps", spread),
            t.gain_green,
        ),
        YieldCurveState::Inverted(spread) => (
            format!("  Yield Curve: INVERTED  2Y-10Y {:.0}bps", spread),
            t.loss_red,
        ),
        YieldCurveState::Flat => (
            "  Yield Curve: FLAT  2Y-10Y ~0bps".to_string(),
            t.text_accent,
        ),
        YieldCurveState::Unknown => ("  Yield Curve: ---".to_string(), t.text_muted),
    }
}

/// Braille characters for the yield curve chart (2 rows of dots per character row).
const BRAILLE_BASE: u32 = 0x2800;
/// Braille dot positions: col0 = [0x01,0x02,0x04,0x40], col1 = [0x08,0x10,0x20,0x80]
const BRAILLE_COL0: [u32; 4] = [0x01, 0x02, 0x04, 0x40];
const BRAILLE_COL1: [u32; 4] = [0x08, 0x10, 0x20, 0x80];

/// Render a braille yield curve showing 2Y/5Y/10Y/30Y maturities.
fn render_yield_curve_chart(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Yield Curve ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    // Gather yield data: 2Y (^IRX), 5Y (^FVX), 10Y (^TNX), 30Y (^TYX)
    let maturity_syms = ["^IRX", "^FVX", "^TNX", "^TYX"];
    let maturity_labels = ["2Y", "5Y", "10Y", "30Y"];
    let yields: Vec<Option<f64>> = maturity_syms
        .iter()
        .map(|s| {
            app.prices
                .get(*s)
                .and_then(|p| p.to_string().parse::<f64>().ok())
        })
        .collect();

    // Check if we have at least 2 data points
    let available: Vec<(usize, f64)> = yields
        .iter()
        .enumerate()
        .filter_map(|(i, y)| y.map(|v| (i, v)))
        .collect();

    if available.len() < 2 {
        let msg = "Waiting for yield data...";
        let msg_line = Line::from(Span::styled(msg, Style::default().fg(t.text_muted)));
        frame.render_widget(Paragraph::new(msg_line).alignment(Alignment::Center), inner);
        return;
    }

    // Reserve bottom row for maturity labels
    let chart_height = (inner.height - 1) as usize;
    let chart_width = inner.width as usize;

    if chart_height < 2 {
        return;
    }

    // Build interpolated yield values across chart_width columns
    // Map 4 maturities to x positions: evenly spaced
    let x_positions: Vec<f64> = (0..4)
        .map(|i| i as f64 * (chart_width.saturating_sub(1) as f64) / 3.0)
        .collect();

    // Linear interpolation between available points
    let mut curve_values: Vec<f64> = Vec::with_capacity(chart_width);
    for col in 0..chart_width {
        let x = col as f64;
        // Find the two surrounding known points
        let mut val = available[0].1; // default to first
        for w in 0..available.len().saturating_sub(1) {
            let (i0, v0) = available[w];
            let (i1, v1) = available[w + 1];
            let x0 = x_positions[i0];
            let x1 = x_positions[i1];
            if x >= x0 && x <= x1 {
                let frac = if (x1 - x0).abs() > 0.001 {
                    (x - x0) / (x1 - x0)
                } else {
                    0.0
                };
                val = v0 + frac * (v1 - v0);
                break;
            } else if x > x1 {
                val = v1; // beyond last known, use last
            }
        }
        curve_values.push(val);
    }

    // Determine Y range with some padding
    let y_min = curve_values.iter().cloned().fold(f64::INFINITY, f64::min) - 0.1;
    let y_max = curve_values
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        + 0.1;
    let y_range = y_max - y_min;

    // Each braille character covers 2 columns (dots) and 4 rows (dots)
    // But we map: chart_height character rows × chart_width character cols
    // Each char row = 4 dot rows, each char col = 2 dot cols
    let dot_rows = chart_height * 4;

    // Render braille: one character per (char_col, char_row)
    // char_col maps to 2 data columns, char_row maps to 4 dot rows
    let char_cols = chart_width.div_ceil(2);
    let mut braille_grid: Vec<Vec<u32>> = vec![vec![0u32; char_cols]; chart_height];

    for (col_idx, val) in curve_values.iter().enumerate() {
        if y_range < 0.001 {
            continue;
        }
        let norm = ((val - y_min) / y_range).clamp(0.0, 1.0);
        let dot_row = ((1.0 - norm) * (dot_rows as f64 - 1.0)).round() as usize;
        let dot_row = dot_row.min(dot_rows - 1);

        let char_row = dot_row / 4;
        let sub_row = dot_row % 4;
        let char_col = col_idx / 2;
        let sub_col = col_idx % 2;

        if char_row < chart_height && char_col < char_cols {
            let dot_bit = if sub_col == 0 {
                BRAILLE_COL0[sub_row]
            } else {
                BRAILLE_COL1[sub_row]
            };
            braille_grid[char_row][char_col] |= dot_bit;
        }
    }

    // Determine curve color based on yield curve state
    let curve_state = yield_curve_status(app);
    let curve_color = match curve_state {
        YieldCurveState::Normal(_) => t.gain_green,
        YieldCurveState::Inverted(_) => t.loss_red,
        YieldCurveState::Flat => t.text_accent,
        YieldCurveState::Unknown => t.text_muted,
    };

    // Render braille rows
    for (row_idx, char_row) in braille_grid.iter().enumerate() {
        let text: String = char_row
            .iter()
            .map(|bits| char::from_u32(BRAILLE_BASE | bits).unwrap_or(' '))
            .collect();
        let span = Span::styled(text, Style::default().fg(curve_color));
        let y = inner.y + row_idx as u16;
        if y < inner.y + inner.height - 1 {
            frame.render_widget(
                Paragraph::new(Line::from(span)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
    }

    // Render maturity labels at bottom
    let label_y = inner.y + inner.height - 1;
    let mut label_spans: Vec<Span> = Vec::new();
    for (i, label) in maturity_labels.iter().enumerate() {
        let x_pos = x_positions[i] as usize;
        // Pad to reach the x position
        let current_len: usize = label_spans.iter().map(|s| s.content.len()).sum();
        if x_pos > current_len {
            label_spans.push(Span::styled(
                " ".repeat(x_pos - current_len),
                Style::default(),
            ));
        }
        let val_str = match yields[i] {
            Some(v) => format!("{label} {v:.2}%"),
            None => format!("{label} ---"),
        };
        label_spans.push(Span::styled(val_str, Style::default().fg(t.text_secondary)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(label_spans)),
        Rect::new(inner.x, label_y, inner.width, 1),
    );
}

/// Render sentiment gauges: Crypto F&G and Traditional F&G with 30-day sparklines.
fn render_sentiment_panel(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Sentiment ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 40 {
        return;
    }

    let backend = match app.open_backend() {
        Some(b) => b,
        None => {
            let msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No sentiment data cached",
                    Style::default().fg(t.text_muted).italic(),
                )),
            ])
            .alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(msg, inner);
            return;
        }
    };

    let crypto_sentiment = crate::db::sentiment_cache::get_latest_backend(&backend, "crypto")
        .ok()
        .flatten();
    let trad_sentiment = crate::db::sentiment_cache::get_latest_backend(&backend, "traditional")
        .ok()
        .flatten();

    // Fetch 30-day history for sparklines
    let crypto_history =
        crate::db::sentiment_cache::get_history_backend(&backend, "crypto", 30).unwrap_or_default();
    let trad_history = crate::db::sentiment_cache::get_history_backend(&backend, "traditional", 30)
        .unwrap_or_default();

    let mut lines: Vec<Line> = Vec::new();

    // Crypto F&G
    lines.push(Line::from(""));
    let mut crypto_line = vec![Span::styled(
        "  Crypto F&G: ",
        Style::default().fg(t.text_secondary),
    )];

    if let Some(sentiment) = crypto_sentiment {
        let classification_color = sentiment_color(&sentiment.classification, t);
        crypto_line.push(Span::styled(
            format!("{} ", sentiment.value),
            Style::default().fg(classification_color).bold(),
        ));
        crypto_line.push(Span::styled(
            sentiment.classification.clone(),
            Style::default().fg(classification_color).italic(),
        ));

        // Add sparkline
        if !crypto_history.is_empty() {
            crypto_line.push(Span::styled("  ", Style::default()));
            let sparkline = build_sentiment_sparkline(&crypto_history, t);
            crypto_line.extend(sparkline);
        }
    } else {
        crypto_line.push(Span::styled(
            "---",
            Style::default().fg(t.text_muted).italic(),
        ));
    }
    lines.push(Line::from(crypto_line));

    // Traditional F&G
    lines.push(Line::from(""));
    let mut trad_line = vec![Span::styled(
        "  TradFi F&G: ",
        Style::default().fg(t.text_secondary),
    )];

    if let Some(sentiment) = trad_sentiment {
        let classification_color = sentiment_color(&sentiment.classification, t);
        trad_line.push(Span::styled(
            format!("{} ", sentiment.value),
            Style::default().fg(classification_color).bold(),
        ));
        trad_line.push(Span::styled(
            sentiment.classification.clone(),
            Style::default().fg(classification_color).italic(),
        ));

        // Add sparkline
        if !trad_history.is_empty() {
            trad_line.push(Span::styled("  ", Style::default()));
            let sparkline = build_sentiment_sparkline(&trad_history, t);
            trad_line.extend(sparkline);
        }
    } else {
        trad_line.push(Span::styled(
            "---",
            Style::default().fg(t.text_muted).italic(),
        ));
    }
    lines.push(Line::from(trad_line));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render derived metrics panel: gold/silver ratio, real rate, yield curve spread.
/// Calendar panel: 7-day economic event calendar with countdown timers and impact ratings.
fn render_calendar_panel(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Economic Calendar (7D) ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 40 {
        return;
    }

    if app.calendar_events.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "No calendar events loaded",
                Style::default().fg(t.text_muted).italic(),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let max_visible = (inner.height as usize).saturating_sub(1);

    for (i, event) in app.calendar_events.iter().take(max_visible).enumerate() {
        if i >= max_visible {
            break;
        }

        // Impact color coding: high=red, medium=yellow, low=white/muted
        let impact_symbol = match event.impact.as_str() {
            "high" => "🔴",
            "medium" => "🟡",
            _ => "⚪",
        };

        // Parse date for countdown
        let countdown =
            if let Ok(event_date) = chrono::NaiveDate::parse_from_str(&event.date, "%Y-%m-%d") {
                let now = chrono::Utc::now().date_naive();
                let days_until = (event_date - now).num_days();
                if days_until == 0 {
                    "Today".to_string()
                } else if days_until == 1 {
                    "1d".to_string()
                } else {
                    format!("{}d", days_until)
                }
            } else {
                "---".to_string()
            };

        // Truncate event name to fit width (leave ~15 chars for date/countdown/impact)
        let max_name_len = (inner.width as usize).saturating_sub(20);
        let name = if event.name.len() > max_name_len {
            format!("{}...", &event.name[..max_name_len.saturating_sub(3)])
        } else {
            event.name.clone()
        };

        let line = Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(impact_symbol, Style::default()),
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("{:<4}", countdown),
                Style::default().fg(t.text_secondary),
            ),
            Span::styled(" ", Style::default()),
            Span::styled(name, Style::default().fg(t.text_primary)),
        ]);

        lines.push(line);
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_predictions_panel(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(t.border_inactive))
        .title(Span::styled(
            " Prediction Markets ",
            Style::default().fg(t.text_accent).bold(),
        ))
        .style(Style::default().bg(t.surface_0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 30 {
        return;
    }

    if app.prediction_markets.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "No prediction data cached",
                Style::default().fg(t.text_muted).italic(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Run `pftui refresh --predictions`",
                Style::default().fg(t.text_secondary),
            )),
        ])
        .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(msg, inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Show top 10 prediction markets (sorted by volume)
    let max_visible = (inner.height as usize).saturating_sub(1);
    for (i, market) in app.prediction_markets.iter().take(max_visible).enumerate() {
        if i > 0 {
            lines.push(Line::from("")); // Blank line separator
        }

        // Probability color: green >60%, red <40%, yellow middle
        let prob_pct = (market.probability * 100.0) as u8;
        let prob_color = if market.probability > 0.6 {
            t.gain_green
        } else if market.probability < 0.4 {
            t.loss_red
        } else {
            t.cat_commodity
        };

        // Category color
        let cat_color = match market.category {
            crate::data::predictions::MarketCategory::Crypto => t.cat_crypto,
            crate::data::predictions::MarketCategory::Economics => t.cat_commodity,
            crate::data::predictions::MarketCategory::Geopolitics => t.loss_red,
            crate::data::predictions::MarketCategory::AI => t.cat_equity,
            crate::data::predictions::MarketCategory::Other => t.text_secondary,
        };

        // Truncate question to fit width
        let max_q_len = (inner.width as usize).saturating_sub(20);
        let question = if market.question.len() > max_q_len {
            format!("{}...", &market.question[..max_q_len.saturating_sub(3)])
        } else {
            market.question.clone()
        };

        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("[{}]", market.category),
                Style::default().fg(cat_color).bold(),
            ),
            Span::styled(" ", Style::default()),
            Span::styled(question, Style::default().fg(t.text_primary)),
        ]));

        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("{prob_pct}%"),
                Style::default().fg(prob_color).bold(),
            ),
            Span::styled(
                format!(" (vol: ${:.0}k)", market.volume_24h / 1000.0),
                Style::default().fg(t.text_muted).italic(),
            ),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
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
fn build_mini_sparkline<'a>(app: &App, yahoo_symbol: &str, theme: &'a theme::Theme) -> Cell<'a> {
    let history = match app.price_history.get(yahoo_symbol) {
        Some(h) if h.len() >= 2 => h,
        _ => {
            return Cell::from(Span::styled(
                "  ---  ",
                Style::default().fg(theme.text_muted),
            ))
        }
    };

    let spans = build_sparkline_spans(theme, history, SPARKLINE_DAYS);
    if spans.is_empty() {
        Cell::from(Span::styled(
            "  ---  ",
            Style::default().fg(theme.text_muted),
        ))
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

/// Map sentiment classification to color.
fn sentiment_color(classification: &str, theme: &theme::Theme) -> Color {
    match classification {
        "Extreme Fear" => theme.loss_red,
        "Fear" => theme.cat_commodity, // Orange/yellow
        "Neutral" => theme.text_secondary,
        "Greed" => theme.gain_green,
        "Extreme Greed" => theme.gain_green,
        _ => theme.text_muted,
    }
}

/// Build a 30-day sparkline from sentiment history (date, value) tuples.
fn build_sentiment_sparkline<'a>(
    history: &[(String, u8)],
    theme: &'a theme::Theme,
) -> Vec<Span<'a>> {
    if history.is_empty() {
        return Vec::new();
    }

    // Take last 30 days, reverse to chronological order
    let values: Vec<f64> = history
        .iter()
        .rev()
        .take(30)
        .map(|(_, v)| *v as f64)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    if values.is_empty() {
        return Vec::new();
    }

    // Normalize to 0-100 range (sentiment is already 0-100, but we normalize for gradient)
    let min = 0.0;
    let max = 100.0;
    let range = max - min;

    values
        .iter()
        .map(|v| {
            let position = if range > 0.0 {
                ((v - min) / range) as f32
            } else {
                0.5
            };
            let idx = (position * 7.0).round() as usize;

            // Color: red (fear) -> yellow (neutral) -> green (greed)
            let color = if *v < 25.0 {
                theme.loss_red
            } else if *v < 40.0 {
                theme.cat_commodity // Orange
            } else if *v < 60.0 {
                theme.text_secondary // Neutral gray
            } else {
                theme.gain_green
            };

            Span::styled(
                String::from(SPARKLINE_CHARS[idx.min(7)]),
                Style::default().fg(color),
            )
        })
        .collect()
}

/// Global Macro Panel — World Bank structural data for major economies.
/// Shows: Country, GDP Growth, Debt/GDP, Reserves trend for BRICS + US.
fn render_global_macro_panel(frame: &mut Frame, area: Rect, app: &App) {
    use crate::data::worldbank::{
        COUNTRY_BRAZIL, COUNTRY_CHINA, COUNTRY_INDIA, COUNTRY_RUSSIA, COUNTRY_US,
        INDICATOR_DEBT_GDP, INDICATOR_GDP_GROWTH, INDICATOR_RESERVES,
    };
    use ratatui::widgets::{Block, Borders, Row, Table};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let t = &app.theme;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.border_inactive))
        .title(" Global Macro ")
        .title_style(Style::default().fg(t.text_accent).bold());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Define countries to display (BRICS + US)
    let countries = [
        (COUNTRY_US, "US"),
        (COUNTRY_CHINA, "China"),
        (COUNTRY_INDIA, "India"),
        (COUNTRY_RUSSIA, "Russia"),
        (COUNTRY_BRAZIL, "Brazil"),
    ];

    let mut rows = Vec::new();

    for (country_code, country_name) in &countries {
        let gdp_key = (country_code.to_string(), INDICATOR_GDP_GROWTH.to_string());
        let debt_key = (country_code.to_string(), INDICATOR_DEBT_GDP.to_string());
        let reserves_key = (country_code.to_string(), INDICATOR_RESERVES.to_string());

        let gdp_growth = app.worldbank_data.get(&gdp_key);
        let debt_gdp = app.worldbank_data.get(&debt_key);
        let reserves = app.worldbank_data.get(&reserves_key);

        // Format GDP Growth with color coding
        let gdp_str = if let Some(data) = gdp_growth {
            if let Some(value) = data.value {
                let color = if value > Decimal::ZERO {
                    t.gain_green
                } else if value < Decimal::ZERO {
                    t.loss_red
                } else {
                    t.text_muted
                };
                let formatted = format!("{:+.1}%", value);
                Span::styled(formatted, Style::default().fg(color))
            } else {
                Span::styled("---", Style::default().fg(t.text_muted))
            }
        } else {
            Span::styled("---", Style::default().fg(t.text_muted))
        };

        // Format Debt/GDP
        let debt_str = if let Some(data) = debt_gdp {
            if let Some(value) = data.value {
                let formatted = format!("{:.0}%", value);
                // Color code: >100% red, 60-100% yellow, <60% green
                let color = if value > Decimal::from_str("100").unwrap() {
                    t.loss_red
                } else if value > Decimal::from_str("60").unwrap() {
                    t.stale_yellow
                } else {
                    t.gain_green
                };
                Span::styled(formatted, Style::default().fg(color))
            } else {
                Span::styled("---", Style::default().fg(t.text_muted))
            }
        } else {
            Span::styled("---", Style::default().fg(t.text_muted))
        };

        // Format Reserves (show in trillions with trend indicator)
        // For now just show the value — trend requires historical data
        let reserves_str = if let Some(data) = reserves {
            if let Some(value) = data.value {
                let trillions = value / Decimal::from_str("1000000000000").unwrap();
                let formatted = format!("${:.2}T", trillions);
                Span::styled(formatted, Style::default().fg(t.text_primary))
            } else {
                Span::styled("---", Style::default().fg(t.text_muted))
            }
        } else {
            Span::styled("---", Style::default().fg(t.text_muted))
        };

        rows.push(Row::new(vec![
            Span::styled(*country_name, Style::default().fg(t.text_secondary)),
            gdp_str,
            debt_str,
            reserves_str,
        ]));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),  // Country
            Constraint::Length(8),  // GDP Growth
            Constraint::Length(8),  // Debt/GDP
            Constraint::Length(10), // Reserves
        ],
    )
    .header(
        Row::new(vec!["Country", "GDP Grow", "Debt/GDP", "Reserves"])
            .style(Style::default().fg(t.text_accent).bold())
            .bottom_margin(0),
    )
    .style(Style::default().fg(t.text_primary));

    frame.render_widget(table, inner);
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
        assert_eq!(items.len(), 16);
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
        assert_eq!(
            category_for_group(EconomyGroup::Yields),
            AssetCategory::Fund
        );
        assert_eq!(
            category_for_group(EconomyGroup::Currency),
            AssetCategory::Forex
        );
        assert_eq!(
            category_for_group(EconomyGroup::Commodities),
            AssetCategory::Commodity
        );
        assert_eq!(
            category_for_group(EconomyGroup::Volatility),
            AssetCategory::Equity
        );
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
                open: None,
                high: None,
                low: None,
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
                open: None,
                high: None,
                low: None,
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
                open: None,
                high: None,
                low: None,
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
                open: None,
                high: None,
                low: None,
            }],
        );
        assert!(compute_7d_momentum(&app, "^TNX").is_none());
    }

    #[test]
    fn sparkline_chars_count() {
        assert_eq!(SPARKLINE_CHARS.len(), 8);
    }

    // --- Silver added to economy symbols ---

    #[test]
    fn economy_symbols_includes_silver() {
        let items = economy_symbols();
        let silver = items.iter().find(|i| i.symbol == "Silver");
        assert!(silver.is_some(), "silver should be in economy symbols");
        let silver = silver.unwrap();
        assert_eq!(silver.yahoo_symbol, "SI=F");
        assert_eq!(silver.group, EconomyGroup::Commodities);
    }

    #[test]
    fn economy_symbols_includes_brent() {
        let items = economy_symbols();
        let brent = items.iter().find(|i| i.symbol == "Brent");
        assert!(brent.is_some(), "brent should be in economy symbols");
        let brent = brent.unwrap();
        assert_eq!(brent.yahoo_symbol, "BZ=F");
        assert_eq!(brent.group, EconomyGroup::Commodities);
    }

    // --- Yield curve label tests ---

    #[test]
    fn yield_curve_label_normal() {
        let t = theme::midnight();
        let (label, color) = yield_curve_label(&YieldCurveState::Normal(50.0), &t);
        assert!(label.contains("NORMAL"));
        assert!(label.contains("50"));
        assert_eq!(color, t.gain_green);
    }

    #[test]
    fn yield_curve_label_inverted() {
        let t = theme::midnight();
        let (label, color) = yield_curve_label(&YieldCurveState::Inverted(-30.0), &t);
        assert!(label.contains("INVERTED"));
        assert_eq!(color, t.loss_red);
    }

    #[test]
    fn yield_curve_label_flat() {
        let t = theme::midnight();
        let (label, color) = yield_curve_label(&YieldCurveState::Flat, &t);
        assert!(label.contains("FLAT"));
        assert_eq!(color, t.text_accent);
    }

    #[test]
    fn yield_curve_label_unknown() {
        let t = theme::midnight();
        let (label, color) = yield_curve_label(&YieldCurveState::Unknown, &t);
        assert!(label.contains("---"));
        assert_eq!(color, t.text_muted);
    }
}
