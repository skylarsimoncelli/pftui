use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset::AssetCategory;
use crate::tui::theme;

/// A single entry in the Markets overview table.
#[derive(Debug, Clone)]
pub struct MarketItem {
    pub symbol: String,
    pub name: String,
    pub category: AssetCategory,
    /// Yahoo Finance symbol for price lookup.
    pub yahoo_symbol: String,
}

/// Returns the fixed list of market overview symbols.
pub fn market_symbols() -> Vec<MarketItem> {
    vec![
        // Indices
        MarketItem { symbol: "SPX".into(), name: "S&P 500".into(), category: AssetCategory::Equity, yahoo_symbol: "^GSPC".into() },
        MarketItem { symbol: "NDX".into(), name: "Nasdaq 100".into(), category: AssetCategory::Equity, yahoo_symbol: "^NDX".into() },
        MarketItem { symbol: "DJI".into(), name: "Dow Jones".into(), category: AssetCategory::Equity, yahoo_symbol: "^DJI".into() },
        MarketItem { symbol: "RUT".into(), name: "Russell 2000".into(), category: AssetCategory::Equity, yahoo_symbol: "^RUT".into() },
        MarketItem { symbol: "VIX".into(), name: "CBOE Volatility".into(), category: AssetCategory::Equity, yahoo_symbol: "^VIX".into() },
        // Commodities
        MarketItem { symbol: "Gold".into(), name: "Gold Futures".into(), category: AssetCategory::Commodity, yahoo_symbol: "GC=F".into() },
        MarketItem { symbol: "Silver".into(), name: "Silver Futures".into(), category: AssetCategory::Commodity, yahoo_symbol: "SI=F".into() },
        MarketItem { symbol: "Oil".into(), name: "Crude Oil (WTI)".into(), category: AssetCategory::Commodity, yahoo_symbol: "CL=F".into() },
        MarketItem { symbol: "NatGas".into(), name: "Natural Gas".into(), category: AssetCategory::Commodity, yahoo_symbol: "NG=F".into() },
        // Crypto
        MarketItem { symbol: "BTC".into(), name: "Bitcoin".into(), category: AssetCategory::Crypto, yahoo_symbol: "BTC-USD".into() },
        MarketItem { symbol: "ETH".into(), name: "Ethereum".into(), category: AssetCategory::Crypto, yahoo_symbol: "ETH-USD".into() },
        MarketItem { symbol: "SOL".into(), name: "Solana".into(), category: AssetCategory::Crypto, yahoo_symbol: "SOL-USD".into() },
        // Forex
        MarketItem { symbol: "DXY".into(), name: "Dollar Index".into(), category: AssetCategory::Forex, yahoo_symbol: "DX-Y.NYB".into() },
        MarketItem { symbol: "EUR".into(), name: "Euro / USD".into(), category: AssetCategory::Forex, yahoo_symbol: "EURUSD=X".into() },
        MarketItem { symbol: "GBP".into(), name: "Pound / USD".into(), category: AssetCategory::Forex, yahoo_symbol: "GBPUSD=X".into() },
        MarketItem { symbol: "JPY".into(), name: "USD / Yen".into(), category: AssetCategory::Forex, yahoo_symbol: "JPY=X".into() },
        // Bonds
        MarketItem { symbol: "10Y".into(), name: "10-Year Treasury".into(), category: AssetCategory::Fund, yahoo_symbol: "^TNX".into() },
        MarketItem { symbol: "2Y".into(), name: "2-Year Treasury".into(), category: AssetCategory::Fund, yahoo_symbol: "^IRX".into() },
    ]
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let items = market_symbols();

    let header = Row::new(vec![
        Cell::from("Symbol"),
        Cell::from("Name"),
        Cell::from("Category"),
        Cell::from("Price"),
        Cell::from("Change %"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    let rows: Vec<Row> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let cat_color = t.category_color(item.category);

            let row_bg = if i == app.markets_selected_index {
                t.surface_3
            } else if i % 2 == 0 {
                t.surface_1
            } else {
                t.surface_0
            };

            // Look up the live price from the app's price map
            let price = app.prices.get(&item.yahoo_symbol).copied();
            let price_str = match price {
                Some(p) => format_price(p),
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
                    format!("{}", item.category),
                    Style::default().fg(cat_color),
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
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(8),
        Constraint::Min(16),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .title(Span::styled(
                    " Markets ",
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
    fn market_symbols_has_expected_count() {
        let items = market_symbols();
        assert_eq!(items.len(), 18);
    }

    #[test]
    fn market_symbols_has_all_categories() {
        let items = market_symbols();
        let has_equity = items.iter().any(|i| i.category == AssetCategory::Equity);
        let has_commodity = items.iter().any(|i| i.category == AssetCategory::Commodity);
        let has_crypto = items.iter().any(|i| i.category == AssetCategory::Crypto);
        let has_forex = items.iter().any(|i| i.category == AssetCategory::Forex);
        assert!(has_equity, "missing equity items");
        assert!(has_commodity, "missing commodity items");
        assert!(has_crypto, "missing crypto items");
        assert!(has_forex, "missing forex items");
    }

    #[test]
    fn market_symbols_yahoo_symbols_unique() {
        let items = market_symbols();
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
    fn market_symbols_spx_is_first() {
        let items = market_symbols();
        assert_eq!(items[0].symbol, "SPX");
        assert_eq!(items[0].yahoo_symbol, "^GSPC");
    }

    #[test]
    fn format_price_large() {
        let p = Decimal::new(5234500, 2); // 52345.00
        assert_eq!(format_price(p), "52345");
    }

    #[test]
    fn format_price_medium() {
        let p = Decimal::new(17523, 2); // 175.23
        assert_eq!(format_price(p), "175.23");
    }

    #[test]
    fn format_price_ones() {
        let p = Decimal::new(523, 2); // 5.23
        assert_eq!(format_price(p), "5.23");
    }

    #[test]
    fn format_price_small() {
        let p = Decimal::new(8321, 4); // 0.8321
        assert_eq!(format_price(p), "0.8321");
    }
}
