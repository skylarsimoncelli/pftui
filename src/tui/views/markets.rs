use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::App;
use crate::models::asset::AssetCategory;
use crate::tui::theme;
use crate::tui::views::correlation_grid;
use crate::tui::widgets::skeleton;

/// Braille-style sparkline characters (bottom to top).
const SPARKLINE_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Number of days shown in the mini sparkline.
const SPARKLINE_DAYS: usize = 7;

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
        // Bonds & Credit
        MarketItem { symbol: "10Y".into(), name: "10-Year Treasury".into(), category: AssetCategory::Fund, yahoo_symbol: "^TNX".into() },
        MarketItem { symbol: "2Y".into(), name: "2-Year Treasury".into(), category: AssetCategory::Fund, yahoo_symbol: "^IRX".into() },
        MarketItem { symbol: "HYG".into(), name: "High Yield Bond ETF".into(), category: AssetCategory::Fund, yahoo_symbol: "HYG".into() },
        MarketItem { symbol: "LQD".into(), name: "Inv Grade Bond ETF".into(), category: AssetCategory::Fund, yahoo_symbol: "LQD".into() },
        // Metals & Industrial
        MarketItem { symbol: "Copper".into(), name: "Copper Futures".into(), category: AssetCategory::Commodity, yahoo_symbol: "HG=F".into() },
    ]
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // Split area: markets table, correlation matrix, then predictions.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(20),
            Constraint::Percentage(30),
        ])
        .split(area);
    
    render_markets_table(frame, chunks[0], app);
    correlation_grid::render(frame, chunks[1], app);
    render_predictions_panel(frame, chunks[2], app);
}

fn render_markets_table(frame: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let items = market_symbols();

    let header = Row::new(vec![
        Cell::from("Symbol"),
        Cell::from("Name"),
        Cell::from("Category"),
        Cell::from("Price"),
        Cell::from("Day %"),
        Cell::from("7D"),
        Cell::from("7D %"),
        Cell::from("COT"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);

    // Show skeleton placeholder rows while waiting for initial price data
    let rows: Vec<Row> = if !app.prices_live {
        let col_widths = [6, 12, 8, 10, 7, 5, 6, 2];
        skeleton::skeleton_rows(t, app.tick_count, &col_widths, 8)
    } else {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let cat_color = t.category_color(item.category);

            // Look up the live price from the app's price map
            let price = app.prices.get(&item.yahoo_symbol).copied();
            let price_str = match price {
                Some(p) => format_price(p),
                None => "---".to_string(),
            };

            // Compute daily change % from history
            let day_change = compute_change_pct(app, &item.yahoo_symbol);
            let day_f = day_change
                .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
                .unwrap_or(0.0);
            let (day_str, day_color) = match day_change {
                Some(pct) => {
                    let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                    let color = theme::gain_intensity_color(t, f);
                    (format!("{:+.2}%", f), color)
                }
                None => ("---".to_string(), t.text_muted),
            };

            // Heat-map row background: tint the row based on daily change magnitude
            let row_bg = if i == app.markets_selected_index {
                t.surface_3
            } else {
                let base = if i % 2 == 0 { t.surface_1 } else { t.surface_0 };
                if day_change.is_some() {
                    heatmap_tint(base, day_f, t)
                } else {
                    base
                }
            };

            // Build 7-day mini sparkline
            let sparkline_cell = build_mini_sparkline(app, &item.yahoo_symbol, t);

            // Compute 7-day momentum %
            let momentum = compute_7d_momentum(app, &item.yahoo_symbol);
            let (mom_str, mom_color) = match momentum {
                Some(pct) => {
                    let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                    let color = theme::gain_intensity_color(t, f);
                    (format!("{:+.1}%", f), color)
                }
                None => ("---".to_string(), t.text_muted),
            };

            // Compute COT signal
            let cot_signal = compute_cot_signal(app, &item.yahoo_symbol);

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
                    day_str,
                    Style::default().fg(day_color),
                )),
                sparkline_cell,
                Cell::from(Span::styled(
                    mom_str,
                    Style::default().fg(mom_color),
                )),
                Cell::from(Span::styled(
                    cot_signal,
                    Style::default().fg(t.text_primary),
                )),
            ])
            .style(Style::default().bg(row_bg))
            .height(1)
        })
        .collect()
    };

    let widths = [
        Constraint::Length(8),   // Symbol
        Constraint::Min(14),     // Name
        Constraint::Length(10),  // Category
        Constraint::Length(12),  // Price
        Constraint::Length(9),   // Day %
        Constraint::Length(7),   // 7D sparkline
        Constraint::Length(8),   // 7D %
        Constraint::Length(4),   // COT
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

/// Build a mini sparkline cell from the last 7 days of price history.
/// Returns a Cell containing colored braille characters showing the trend.
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
/// Takes the last `count` records and maps each to a braille bar character
/// with a gradient color based on relative position within the range.
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

/// Compute daily change % from price history: (latest - prev) / prev × 100.
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

/// Compute COT signal for a given yahoo symbol.
/// Returns an emoji: 🟢 aligned, 🔴 divergence, ⚠️ extreme, or empty string if no COT data.
///
/// Signal logic:
/// - 🟢 Aligned: managed money net position and price trend agree (both up or both down)
/// - 🔴 Divergence: managed money and price trend disagree
/// - ⚠️ Extreme: managed money net is >2 std devs from 52-week mean
fn compute_cot_signal(app: &App, yahoo_symbol: &str) -> String {
    // Map yahoo symbol to CFTC code
    let cftc_code = match crate::data::cot::symbol_to_cftc_code(yahoo_symbol) {
        Some(code) => code,
        None => return String::new(),
    };

    let backend = match app.open_backend() {
        Some(b) => b,
        None => return String::new(),
    };

    // Fetch latest COT report
    let latest = match crate::db::cot_cache::get_latest_backend(&backend, cftc_code) {
        Ok(Some(report)) => report,
        _ => return String::new(),
    };

    // Fetch last 52 weeks of history for extremity check
    let history = match crate::db::cot_cache::get_history_backend(&backend, cftc_code, 52) {
        Ok(h) if h.len() >= 10 => h, // Need at least 10 weeks for meaningful stats
        _ => return String::new(),
    };

    // Check for extreme positioning (>2 std devs from mean)
    let mm_nets: Vec<f64> = history
        .iter()
        .map(|r| r.managed_money_net as f64)
        .collect();
    if mm_nets.is_empty() {
        return String::new();
    }
    let mean = mm_nets.iter().sum::<f64>() / mm_nets.len() as f64;
    let variance = mm_nets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / mm_nets.len() as f64;
    let std_dev = variance.sqrt();
    let current_mm = latest.managed_money_net as f64;
    let z_score = if std_dev > 0.0 {
        (current_mm - mean) / std_dev
    } else {
        0.0
    };

    if z_score.abs() > 2.0 {
        return "⚠️".to_string();
    }

    // Check alignment with price trend
    // Get 7-day price momentum
    let price_momentum = compute_7d_momentum(app, yahoo_symbol);
    
    // Get managed money WoW change
    let mm_change = if history.len() >= 2 {
        let prev = &history[1];
        latest.managed_money_net - prev.managed_money_net
    } else {
        return String::new();
    };

    match (price_momentum, mm_change) {
        (Some(price_pct), mm_delta) => {
            let price_up = price_pct > dec!(0);
            let mm_up = mm_delta > 0;
            
            if price_up == mm_up {
                "🟢".to_string() // Aligned
            } else {
                "🔴".to_string() // Divergence
            }
        }
        _ => String::new(),
    }
}

/// Compute 7-day momentum: (latest - 7d_ago) / 7d_ago × 100.
/// Falls back to whatever history is available if less than 7 days.
fn compute_7d_momentum(app: &App, yahoo_symbol: &str) -> Option<Decimal> {
    let history = app.price_history.get(yahoo_symbol)?;
    if history.len() < 2 {
        return None;
    }
    let latest = &history[history.len() - 1];
    // Go back 7 days or as far as we can
    let lookback = SPARKLINE_DAYS.min(history.len() - 1);
    let baseline = &history[history.len() - 1 - lookback];
    if baseline.close == dec!(0) {
        return None;
    }
    Some((latest.close - baseline.close) / baseline.close * dec!(100))
}

/// Apply a subtle heat-map tint to a row background based on daily change %.
/// Positive changes tint toward green, negative toward red. The tint is very
/// subtle (≤8% blend) so it doesn't overwhelm text readability.
fn heatmap_tint(base: Color, change_pct: f64, theme: &theme::Theme) -> Color {
    if change_pct.abs() < 0.01 {
        return base;
    }
    // Scale: 0% → no tint, ±5%+ → max tint (8% blend)
    let intensity = (change_pct.abs() / 5.0).min(1.0) as f32 * 0.08;
    let tint_color = if change_pct > 0.0 {
        theme.gain_green
    } else {
        theme.loss_red
    };
    theme::lerp_color(base, tint_color, intensity)
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

/// Render the prediction markets panel showing key tracked predictions with 30-day sparklines.
fn render_predictions_panel(frame: &mut Frame, area: Rect, app: &App) {
    use rusqlite::Connection;
    
    let t = &app.theme;
    
    // Use prediction markets already loaded in app state
    let predictions = &app.prediction_markets;
    
    let header = Row::new(vec![
        Cell::from("Question"),
        Cell::from("Prob"),
        Cell::from("Chg"),
        Cell::from("30D Trend"),
        Cell::from("Cat"),
    ])
    .style(Style::default().fg(t.text_secondary).bold())
    .height(1);
    
    let rows: Vec<Row> = if predictions.is_empty() {
        // Show skeleton or empty state
        let col_widths = [40, 6, 6, 12, 8];
        skeleton::skeleton_rows(t, app.tick_count, &col_widths, 5)
    } else {
        predictions
            .iter()
            .take(6) // Limit to 6 rows to fit in the panel
            .map(|pred| {
                // Format probability as percentage
                let prob_pct = pred.probability * 100.0;
                let prob_str = format!("{:.0}%", prob_pct);
                
                // Get 30-day history for sparkline
                let history = if let Ok(conn) = Connection::open(&app.db_path) {
                    crate::db::predictions_history::get_history(&conn, &pred.id, 30)
                        .unwrap_or_default()
                } else {
                    vec![]
                };
                
                // Compute 30-day change
                let change_str = if history.len() >= 2 {
                    let oldest = history.last().unwrap().probability;
                    let newest = history.first().unwrap().probability;
                    let change = (newest - oldest) * 100.0;
                    format!("{:+.0}pp", change) // percentage points
                } else {
                    "---".to_string()
                };
                
                // Build sparkline from history
                let sparkline_cell = build_prediction_sparkline(t, &history);
                
                // Category color
                let cat_color = match pred.category {
                    crate::data::predictions::MarketCategory::Crypto => t.cat_crypto,
                    crate::data::predictions::MarketCategory::Economics => t.cat_fund,
                    crate::data::predictions::MarketCategory::Geopolitics => t.text_accent,
                    crate::data::predictions::MarketCategory::AI => t.cat_equity,
                    crate::data::predictions::MarketCategory::Other => t.text_muted,
                };
                
                // Probability color: green >60%, red <40%, yellow 40-60%
                let prob_color = if prob_pct > 60.0 {
                    t.gain_green
                } else if prob_pct < 40.0 {
                    t.loss_red
                } else {
                    t.text_secondary
                };
                
                Row::new(vec![
                    Cell::from(Span::styled(
                        truncate_question(&pred.question, 40),
                        Style::default().fg(t.text_primary),
                    )),
                    Cell::from(Span::styled(
                        prob_str,
                        Style::default().fg(prob_color).bold(),
                    )),
                    Cell::from(Span::styled(
                        change_str,
                        Style::default().fg(t.text_secondary),
                    )),
                    sparkline_cell,
                    Cell::from(Span::styled(
                        format!("{}", pred.category),
                        Style::default().fg(cat_color),
                    )),
                ])
                .height(1)
            })
            .collect()
    };
    
    let widths = [
        Constraint::Min(40),     // Question
        Constraint::Length(6),   // Probability
        Constraint::Length(6),   // Change
        Constraint::Length(12),  // Sparkline
        Constraint::Length(8),   // Category
    ];
    
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(crate::tui::theme::BORDER_ACTIVE)
                .border_style(Style::default().fg(t.border_inactive))
                .title(Span::styled(
                    " Prediction Markets ",
                    Style::default().fg(t.text_accent).bold(),
                ))
                .style(Style::default().bg(t.surface_0)),
        );
    
    frame.render_widget(table, area);
}

/// Build a sparkline from prediction history (probability over time).
fn build_prediction_sparkline<'a>(
    theme: &'a theme::Theme,
    history: &[crate::db::predictions_history::PredictionHistoryRecord],
) -> Cell<'a> {
    if history.len() < 2 {
        return Cell::from(Span::styled("   ---   ", Style::default().fg(theme.text_muted)));
    }
    
    // Reverse history so it's oldest to newest
    let mut sorted: Vec<_> = history.iter().collect();
    sorted.reverse();
    
    // Extract probabilities
    let probs: Vec<f64> = sorted.iter().map(|r| r.probability).collect();
    
    // Find min/max for normalization
    let min = probs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = probs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    
    let range = max - min;
    if range < 0.01 {
        // Flat line
        let chars: String = "━".repeat(10);
        return Cell::from(Span::styled(chars, Style::default().fg(theme.text_muted)));
    }
    
    // Build sparkline characters
    let chars: String = probs
        .iter()
        .map(|&p| {
            let normalized = (p - min) / range;
            let idx = (normalized * (SPARKLINE_CHARS.len() - 1) as f64).round() as usize;
            SPARKLINE_CHARS[idx.min(SPARKLINE_CHARS.len() - 1)]
        })
        .collect();
    
    // Color based on trend (green if rising, red if falling)
    let trend_color = if probs.last() > probs.first() {
        theme.gain_green
    } else if probs.last() < probs.first() {
        theme.loss_red
    } else {
        theme.text_secondary
    };
    
    Cell::from(Span::styled(chars, Style::default().fg(trend_color)))
}

/// Truncate a prediction question to fit in the table.
fn truncate_question(question: &str, max_len: usize) -> String {
    if question.len() <= max_len {
        question.to_string()
    } else {
        format!("{}...", &question[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::price::HistoryRecord;

    #[test]
    fn market_symbols_has_expected_count() {
        let items = market_symbols();
        assert_eq!(items.len(), 21);
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

    #[test]
    fn sparkline_chars_count() {
        assert_eq!(SPARKLINE_CHARS.len(), 8);
    }

    #[test]
    fn sparkline_days_reasonable() {
        const { assert!(SPARKLINE_DAYS >= 3 && SPARKLINE_DAYS <= 14) };
    }

    #[test]
    fn build_sparkline_spans_empty_history() {
        let t = theme::midnight();
        let spans = build_sparkline_spans(&t, &[], 7);
        assert!(spans.is_empty());
    }

    #[test]
    fn build_sparkline_spans_single_record() {
        let t = theme::midnight();
        let records = vec![HistoryRecord {
            date: "2026-03-01".into(),
            close: dec!(100),
            volume: None,
                open: None,
                high: None,
                low: None,
            }];
        let spans = build_sparkline_spans(&t, &records, 7);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn build_sparkline_spans_ascending() {
        let t = theme::midnight();
        let records: Vec<HistoryRecord> = (1..=5)
            .map(|i| HistoryRecord {
                date: format!("2026-03-0{}", i),
                close: Decimal::new(i * 100, 0),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        let spans = build_sparkline_spans(&t, &records, 7);
        assert_eq!(spans.len(), 5);
    }

    #[test]
    fn build_sparkline_spans_truncates_to_count() {
        let t = theme::midnight();
        let records: Vec<HistoryRecord> = (1..=20)
            .map(|i| HistoryRecord {
                date: format!("2026-01-{:02}", i),
                close: Decimal::new(i * 10, 0),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        let spans = build_sparkline_spans(&t, &records, 7);
        assert_eq!(spans.len(), 7);
    }

    #[test]
    fn build_sparkline_spans_flat_uses_middle_char() {
        let t = theme::midnight();
        let records: Vec<HistoryRecord> = (1..=5)
            .map(|i| HistoryRecord {
                date: format!("2026-03-0{}", i),
                close: dec!(50),
                volume: None,
                open: None,
                high: None,
                low: None,
            })
            .collect();
        let spans = build_sparkline_spans(&t, &records, 7);
        // Flat data → all same mid-level character (index 3 = '▄')
        for span in &spans {
            assert_eq!(span.content.as_ref(), "▄");
        }
    }

    #[test]
    fn heatmap_tint_zero_change() {
        let t = theme::midnight();
        let base = Color::Rgb(30, 30, 40);
        let result = heatmap_tint(base, 0.0, &t);
        assert_eq!(result, base);
    }

    #[test]
    fn heatmap_tint_positive_shifts_toward_green() {
        let t = theme::midnight();
        let base = Color::Rgb(30, 30, 40);
        let result = heatmap_tint(base, 3.0, &t);
        // Should be different from base (shifted toward green)
        assert_ne!(result, base);
        if let Color::Rgb(r, g, _b) = result {
            // Green channel should be >= base green
            assert!(g >= 30);
            // Tint is subtle so red shouldn't change dramatically
            assert!(r.abs_diff(30) < 15);
        } else {
            panic!("expected Rgb color");
        }
    }

    #[test]
    fn heatmap_tint_negative_shifts_toward_red() {
        let t = theme::midnight();
        let base = Color::Rgb(30, 30, 40);
        let result = heatmap_tint(base, -3.0, &t);
        assert_ne!(result, base);
        if let Color::Rgb(r, _g, _b) = result {
            // Red channel should be >= base red
            assert!(r >= 30);
        } else {
            panic!("expected Rgb color");
        }
    }

    #[test]
    fn heatmap_tint_saturates_at_5pct() {
        let t = theme::midnight();
        let base = Color::Rgb(30, 30, 40);
        let at_5 = heatmap_tint(base, 5.0, &t);
        let at_20 = heatmap_tint(base, 20.0, &t);
        // Both should be the same since tint saturates at 5%
        assert_eq!(at_5, at_20);
    }
}
