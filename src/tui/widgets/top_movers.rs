//! Top movers widget — retained for potential future use. Currently superseded by portfolio_stats.
#![allow(dead_code)]

use std::collections::HashMap;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::app::{is_privacy_view, App};
use crate::models::asset::AssetCategory;
use crate::models::price::HistoryRecord;
use crate::tui::theme;

/// A single asset's daily change data.
#[derive(Debug, Clone)]
struct Mover {
    symbol: String,
    change_pct: Decimal,
}

/// Height of the top movers panel (border top + 1 line per category + border bottom).
/// We show up to 4 categories (crypto, equity, commodity, forex/fund).
/// Fixed at 6 rows: top border + up to 4 category lines + bottom border.
pub const TOP_MOVERS_HEIGHT: u16 = 6;

/// Minimum number of movers required across all categories to show the panel.
const MIN_MOVERS: usize = 2;

/// Returns true if there's enough data to display the top movers panel.
pub fn has_movers(app: &App) -> bool {
    if is_privacy_view(app) {
        return false;
    }
    let movers = compute_movers(app);
    movers.len() >= MIN_MOVERS
}

/// Compute daily % change for each position that has price history.
fn compute_movers(app: &App) -> Vec<(AssetCategory, Mover)> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let mut result = Vec::new();

    for pos in &app.positions {
        if pos.category == AssetCategory::Cash {
            continue;
        }
        let current_price = match pos.current_price {
            Some(p) if p > dec!(0) => p,
            _ => continue,
        };

        if let Some(records) = app.price_history.get(&pos.symbol) {
            if let Some(prev_close) = prev_close_from_history(records, &today, current_price) {
                if prev_close > dec!(0) {
                    let change_pct =
                        ((current_price - prev_close) / prev_close) * dec!(100);
                    result.push((
                        pos.category,
                        Mover {
                            symbol: pos.symbol.clone(),
                            change_pct,
                        },
                    ));
                }
            }
        }
    }

    result
}

/// Get the previous close price from history records.
///
/// Walks backwards through history, skipping today's record and any records
/// whose close matches `current_price` (stale-close duplication from Yahoo).
/// Returns the first genuinely different close, or the oldest candidate if
/// all closes are identical (flat market → 0% change).
fn prev_close_from_history(
    records: &[HistoryRecord],
    today: &str,
    current_price: Decimal,
) -> Option<Decimal> {
    if records.is_empty() {
        return None;
    }

    let mut fallback: Option<Decimal> = None;
    for record in records.iter().rev() {
        // Skip today's entry
        if record.date == today {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(record.close);
        }
        // Return the first close that differs from the cached spot
        if record.close != current_price {
            return Some(record.close);
        }
    }

    // All prior closes equal current_price — return oldest candidate (produces 0%)
    fallback
}

/// Group movers by category and sort each group by |change_pct| descending.
fn group_by_category(movers: &[(AssetCategory, Mover)]) -> Vec<(AssetCategory, Vec<&Mover>)> {
    // Desired display order
    let order = [
        AssetCategory::Crypto,
        AssetCategory::Equity,
        AssetCategory::Commodity,
        AssetCategory::Fund,
        AssetCategory::Forex,
    ];

    let mut map: HashMap<AssetCategory, Vec<&Mover>> = HashMap::new();
    for (cat, mover) in movers {
        map.entry(*cat).or_default().push(mover);
    }

    // Sort each group by absolute change descending
    for group in map.values_mut() {
        group.sort_by(|a, b| {
            b.change_pct
                .abs()
                .cmp(&a.change_pct.abs())
        });
    }

    let mut result = Vec::new();
    for cat in &order {
        if let Some(group) = map.remove(cat) {
            if !group.is_empty() {
                result.push((*cat, group));
            }
        }
    }
    result
}

/// Render the top movers panel.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if is_privacy_view(app) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(theme::BORDER_INACTIVE)
            .border_style(Style::default().fg(app.theme.border_inactive))
            .style(Style::default().bg(app.theme.surface_1));
        frame.render_widget(block, area);
        return;
    }

    let movers_data = compute_movers(app);
    let groups = group_by_category(&movers_data);

    let block = Block::default()
        .title(Span::styled(
            " TOP MOVERS ",
            Style::default()
                .fg(app.theme.text_accent)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(theme::BORDER_INACTIVE)
        .border_style(Style::default().fg(app.theme.border_inactive))
        .style(Style::default().bg(app.theme.surface_1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if groups.is_empty() {
        let msg = Paragraph::new("Waiting for price data...")
            .style(Style::default().fg(app.theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    }

    // Render up to 4 category lines
    let max_lines = inner.height as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (cat, group) in groups.iter().take(max_lines) {
        let cat_label = category_short_label(*cat);
        let cat_color = app.theme.category_color(*cat);

        let mut spans = vec![Span::styled(
            format!("{:<9}", cat_label),
            Style::default().fg(cat_color).add_modifier(Modifier::BOLD),
        )];

        // Show up to 3 movers per category, space permitting
        let available_width = inner.width.saturating_sub(9) as usize;
        let max_movers = 3.min(group.len());
        let mut used_width = 0;

        for mover in group.iter().take(max_movers) {
            let formatted = format_mover(&mover.symbol, mover.change_pct);
            let entry_width = formatted.len() + 2; // +2 for spacing

            if used_width + entry_width > available_width {
                break;
            }

            let color = if mover.change_pct > dec!(0) {
                app.theme.gain_green
            } else if mover.change_pct < dec!(0) {
                app.theme.loss_red
            } else {
                app.theme.neutral
            };

            spans.push(Span::styled(
                format!("  {}", formatted),
                Style::default().fg(color),
            ));
            used_width += entry_width;
        }

        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Short label for each category (padded to consistent width).
fn category_short_label(cat: AssetCategory) -> &'static str {
    match cat {
        AssetCategory::Crypto => "Crypto",
        AssetCategory::Equity => "Equity",
        AssetCategory::Commodity => "Cmdty",
        AssetCategory::Fund => "Fund",
        AssetCategory::Forex => "Forex",
        AssetCategory::Cash => "Cash",
    }
}

/// Format a single mover entry: "BTC +3.5%" or "ETH -2.1%"
fn format_mover(symbol: &str, change_pct: Decimal) -> String {
    let sign = if change_pct > dec!(0) { "+" } else { "" };
    // Truncate to 1 decimal place
    let pct = change_pct.round_dp(1);
    format!("{} {}{:.1}%", symbol, sign, pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_mover_positive() {
        let s = format_mover("BTC", dec!(3.52));
        assert!(s.contains("BTC"));
        assert!(s.contains("+3.5%"));
    }

    #[test]
    fn format_mover_negative() {
        let s = format_mover("ETH", dec!(-2.18));
        assert!(s.contains("ETH"));
        assert!(s.contains("-2.2%"));
    }

    #[test]
    fn format_mover_zero() {
        let s = format_mover("SOL", dec!(0));
        assert!(s.contains("SOL"));
        assert!(s.contains("0.0%"));
    }

    #[test]
    fn category_short_label_coverage() {
        assert_eq!(category_short_label(AssetCategory::Crypto), "Crypto");
        assert_eq!(category_short_label(AssetCategory::Equity), "Equity");
        assert_eq!(category_short_label(AssetCategory::Commodity), "Cmdty");
        assert_eq!(category_short_label(AssetCategory::Fund), "Fund");
        assert_eq!(category_short_label(AssetCategory::Forex), "Forex");
        assert_eq!(category_short_label(AssetCategory::Cash), "Cash");
    }

    #[test]
    fn prev_close_today_uses_second_to_last() {
        let records = vec![
            HistoryRecord {
                date: "2026-03-02".into(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-03".into(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];
        // Current price matches today's close — should return previous day's close
        let prev = prev_close_from_history(&records, "2026-03-03", dec!(105));
        assert_eq!(prev, Some(dec!(100)));
    }

    #[test]
    fn prev_close_not_today_uses_last() {
        let records = vec![
            HistoryRecord {
                date: "2026-03-01".into(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-02".into(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];
        // Current price is 110 (different from history) — should return last close
        let prev = prev_close_from_history(&records, "2026-03-03", dec!(110));
        assert_eq!(prev, Some(dec!(105)));
    }

    #[test]
    fn prev_close_empty_returns_none() {
        let prev = prev_close_from_history(&[], "2026-03-03", dec!(100));
        assert_eq!(prev, None);
    }

    #[test]
    fn prev_close_single_record_today_returns_none() {
        let records = vec![HistoryRecord {
            date: "2026-03-03".into(),
            close: dec!(100),
            volume: None,
                open: None,
                high: None,
                low: None,
            }];
        // Single record that IS today — no previous close available
        let prev = prev_close_from_history(&records, "2026-03-03", dec!(100));
        assert_eq!(prev, None);
    }

    #[test]
    fn prev_close_skips_stale_duplicates_in_tui() {
        let records = vec![
            HistoryRecord {
                date: "2026-03-17".into(),
                close: dec!(5001),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-18".into(),
                close: dec!(4890),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-19".into(),
                close: dec!(4600),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-20".into(),
                close: dec!(4600),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];
        // Should skip today and the stale Mar 19 dup, returning Mar 18
        let prev = prev_close_from_history(&records, "2026-03-20", dec!(4600));
        assert_eq!(prev, Some(dec!(4890)));
    }

    #[test]
    fn group_by_category_orders_correctly() {
        let movers = vec![
            (
                AssetCategory::Equity,
                Mover {
                    symbol: "AAPL".into(),
                    change_pct: dec!(1.5),
                },
            ),
            (
                AssetCategory::Crypto,
                Mover {
                    symbol: "BTC".into(),
                    change_pct: dec!(3.2),
                },
            ),
            (
                AssetCategory::Equity,
                Mover {
                    symbol: "NVDA".into(),
                    change_pct: dec!(-4.1),
                },
            ),
        ];

        let groups = group_by_category(&movers);
        // Crypto should come first, then Equity
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, AssetCategory::Crypto);
        assert_eq!(groups[1].0, AssetCategory::Equity);
        // Within Equity, NVDA (-4.1%) has higher abs value than AAPL (1.5%)
        assert_eq!(groups[1].1[0].symbol, "NVDA");
        assert_eq!(groups[1].1[1].symbol, "AAPL");
    }

    #[test]
    fn group_by_category_empty() {
        let movers: Vec<(AssetCategory, Mover)> = vec![];
        let groups = group_by_category(&movers);
        assert!(groups.is_empty());
    }

    #[test]
    fn min_movers_constant() {
        assert_eq!(MIN_MOVERS, 2);
    }

    #[test]
    fn top_movers_height_constant() {
        assert_eq!(TOP_MOVERS_HEIGHT, 6);
    }
}
