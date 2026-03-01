use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset::AssetCategory;
use crate::tui::theme;

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
        Cell::from("Change %"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let mut rows: Vec<Row> = Vec::with_capacity(items.len());
    let mut prev_group: Option<EconomyGroup> = None;

    for (i, item) in items.iter().enumerate() {
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

        // Compute change % from history if available
        let change_pct = compute_change_pct(app, &item.yahoo_symbol);
        let (change_str, change_color) = match change_pct {
            Some(pct) => {
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                let color = theme::gain_intensity_color(t, f);
                (format!("{:+.2}%", f), color)
            }
            None => ("---".to_string(), t.text_muted),
        };

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
            ])
            .style(Style::default().bg(row_bg))
            .height(1),
        );
    }

    let widths = [
        Constraint::Length(8),
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
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
}
