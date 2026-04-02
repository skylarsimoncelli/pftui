//! Unified market snapshot: prices + sentiment + regime flows in one call.
//!
//! Consolidates `data prices --market`, `analytics news-sentiment`, and
//! `analytics regime-flows` into a single JSON payload. Designed for agent
//! consumption — one call replaces three.

use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::news_cache::get_latest_news_backend;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::{get_history_backend, get_price_at_date_backend};
use crate::db::regime_snapshots;
use crate::db::transactions::get_unique_symbols_backend;
use crate::db::watchlist::list_watchlist_backend;
use crate::commands::news_sentiment::{score_all, aggregate_by_category, SentimentLabel};
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::tui::views::markets;

// ── Output structs ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MarketSnapshot {
    pub generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_warning: Option<StalenessInfo>,
    pub prices: PricesSection,
    pub sentiment: SentimentSection,
    pub regime: RegimeSection,
}

#[derive(Debug, Serialize)]
pub struct StalenessInfo {
    /// How many hours since the most recent cached price was fetched.
    pub stale_hours: f64,
    pub message: String,
}

// -- Prices --

#[derive(Debug, Serialize)]
pub struct PricesSection {
    pub portfolio_count: usize,
    pub market_count: usize,
    pub portfolio: Vec<PriceEntry>,
    pub market: Vec<PriceEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PriceEntry {
    pub symbol: String,
    pub name: String,
    pub price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_pct: Option<Decimal>,
    pub source: String,
    pub fetched_at: String,
}

// -- Sentiment --

#[derive(Debug, Serialize)]
pub struct SentimentSection {
    pub articles_scored: usize,
    pub overall_score: i32,
    pub overall_label: String,
    pub by_category: Vec<CategorySentiment>,
}

#[derive(Debug, Serialize)]
pub struct CategorySentiment {
    pub category: String,
    pub score: i32,
    pub label: String,
    pub articles: usize,
}

// -- Regime --

#[derive(Debug, Serialize)]
pub struct RegimeSection {
    pub current_regime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drivers: Option<String>,
    pub key_levels: RegimeKeyLevels,
    pub recorded_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegimeKeyLevels {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vix: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dxy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yield_10y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oil: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub btc: Option<f64>,
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Threshold in hours beyond which cached prices are considered stale.
const STALE_THRESHOLD_HOURS: i64 = 2;

/// Check if price entries are stale (>2h since most recent fetch).
fn check_staleness(entries: &[PriceEntry]) -> Option<StalenessInfo> {
    let newest = entries
        .iter()
        .filter(|e| !e.fetched_at.is_empty())
        .filter_map(|e| parse_fetched_at(&e.fetched_at))
        .max()?;

    let age = Utc::now().signed_duration_since(newest);
    let stale_hours = age.num_minutes() as f64 / 60.0;

    if age.num_hours() >= STALE_THRESHOLD_HOURS {
        let hours_display = stale_hours.round() as i64;
        Some(StalenessInfo {
            stale_hours: (stale_hours * 10.0).round() / 10.0,
            message: format!(
                "Cached prices are {}h old. Run `pftui data refresh` for live data.",
                hours_display
            ),
        })
    } else {
        None
    }
}

/// Parse the fetched_at timestamp string into a chrono DateTime.
fn parse_fetched_at(s: &str) -> Option<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z")
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%#z"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%#z"))
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Compute daily change for a symbol from price history.
fn compute_change(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
    current: Decimal,
) -> (Option<Decimal>, Option<Decimal>) {
    let prev = prev_close(backend, symbol)
        .or_else(|| {
            if category == AssetCategory::Crypto && !symbol.ends_with("-USD") {
                prev_close(backend, &format!("{}-USD", symbol))
            } else {
                None
            }
        });

    match prev {
        Some(prev_price) => {
            let change = current - prev_price;
            let pct = if prev_price == dec!(0) {
                None
            } else {
                Some(change / prev_price * dec!(100))
            };
            (Some(change), pct)
        }
        None => (None, None),
    }
}

fn prev_close(backend: &BackendConnection, symbol: &str) -> Option<Decimal> {
    let today = chrono::Utc::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();

    get_price_at_date_backend(backend, symbol, &yesterday_str)
        .ok()
        .flatten()
        .or_else(|| {
            let history = get_history_backend(backend, symbol, 3).ok()?;
            if history.len() >= 2 {
                Some(history[history.len() - 2].close)
            } else {
                None
            }
        })
}

/// Build a price entry from a cached quote.
fn build_price_entry(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
    display_name: &str,
    price_map: &HashMap<String, (Decimal, String, String)>,
) -> PriceEntry {
    // Try canonical symbol, then Yahoo-mapped crypto
    let quote = price_map.get(symbol).or_else(|| {
        if category == AssetCategory::Crypto && !symbol.ends_with("-USD") {
            price_map.get(&format!("{}-USD", symbol))
        } else {
            None
        }
    });

    let (price, source, fetched_at) = match quote {
        Some((p, src, ts)) => (Some(*p), src.clone(), ts.clone()),
        None => (None, String::new(), String::new()),
    };

    let (change, change_pct) = match price {
        Some(current) => compute_change(backend, symbol, category, current),
        None => (None, None),
    };

    PriceEntry {
        symbol: symbol.to_string(),
        name: display_name.to_string(),
        price,
        change,
        change_pct,
        source,
        fetched_at,
    }
}

// ── Main entry point ─────────────────────────────────────────────────

pub fn run(backend: &BackendConnection, json_output: bool) -> Result<()> {
    let snapshot = build_snapshot(backend)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        print_terminal(&snapshot);
    }

    Ok(())
}

/// Build the full market snapshot (public for testing and composition).
pub fn build_snapshot(backend: &BackendConnection) -> Result<MarketSnapshot> {
    // ── Prices ─────────────────────────────────────────────────────
    let cached = get_all_cached_prices_backend(backend)?;
    let price_map: HashMap<String, (Decimal, String, String)> = cached
        .into_iter()
        .map(|q| (q.symbol, (q.price, q.source, q.fetched_at)))
        .collect();

    // Portfolio + watchlist symbols
    let mut portfolio_symbols: Vec<(String, AssetCategory)> = Vec::new();
    let held = get_unique_symbols_backend(backend)?;
    for (sym, cat) in &held {
        portfolio_symbols.push((sym.clone(), *cat));
    }
    let watched = list_watchlist_backend(backend)?;
    for entry in &watched {
        let cat: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
        if !portfolio_symbols.iter().any(|(s, _)| s == &entry.symbol) {
            portfolio_symbols.push((entry.symbol.clone(), cat));
        }
    }

    // Market overview name map
    let market_items = markets::market_symbols();
    let market_name_map: HashMap<String, String> = market_items
        .iter()
        .map(|i| (i.yahoo_symbol.clone(), i.name.clone()))
        .collect();

    let mut portfolio_prices: Vec<PriceEntry> = Vec::new();
    for (sym, cat) in &portfolio_symbols {
        let name = resolve_name(sym);
        let display_name = if name.is_empty() {
            market_name_map.get(sym).cloned().unwrap_or_else(|| sym.clone())
        } else {
            name
        };
        portfolio_prices.push(build_price_entry(backend, sym, *cat, &display_name, &price_map));
    }

    let mut market_prices: Vec<PriceEntry> = Vec::new();
    for item in &market_items {
        // Skip if already in portfolio to avoid duplication
        if portfolio_symbols.iter().any(|(s, _)| s == &item.yahoo_symbol) {
            continue;
        }
        market_prices.push(build_price_entry(
            backend,
            &item.yahoo_symbol,
            item.category,
            &item.name,
            &price_map,
        ));
    }

    let prices = PricesSection {
        portfolio_count: portfolio_prices.len(),
        market_count: market_prices.len(),
        portfolio: portfolio_prices,
        market: market_prices,
    };

    // ── Sentiment ──────────────────────────────────────────────────
    let news_entries = get_latest_news_backend(backend, 50, None, None, None, Some(24))?;
    let sentiment = if news_entries.is_empty() {
        SentimentSection {
            articles_scored: 0,
            overall_score: 0,
            overall_label: "neutral".to_string(),
            by_category: Vec::new(),
        }
    } else {
        let scored = score_all(&news_entries);
        let total_score: i32 = scored.iter().map(|s| s.score).sum();
        let overall_label = if total_score > 2 {
            SentimentLabel::Bullish
        } else if total_score < -2 {
            SentimentLabel::Bearish
        } else {
            SentimentLabel::Neutral
        };

        let by_cat = aggregate_by_category(&scored);
        let by_category: Vec<CategorySentiment> = by_cat
            .iter()
            .map(|agg| CategorySentiment {
                category: agg.group.clone(),
                score: agg.avg_score as i32,
                label: agg.label.as_str().to_string(),
                articles: agg.count,
            })
            .collect();

        SentimentSection {
            articles_scored: scored.len(),
            overall_score: total_score,
            overall_label: overall_label.as_str().to_string(),
            by_category,
        }
    };

    // ── Regime ─────────────────────────────────────────────────────
    let regime_snap = regime_snapshots::get_current_backend(backend)?;
    let regime = match regime_snap {
        Some(snap) => RegimeSection {
            current_regime: snap.regime,
            confidence: snap.confidence,
            drivers: snap.drivers,
            key_levels: RegimeKeyLevels {
                vix: snap.vix,
                dxy: snap.dxy,
                yield_10y: snap.yield_10y,
                oil: snap.oil,
                gold: snap.gold,
                btc: snap.btc,
            },
            recorded_at: Some(snap.recorded_at),
        },
        None => RegimeSection {
            current_regime: "unknown".to_string(),
            confidence: None,
            drivers: None,
            key_levels: RegimeKeyLevels {
                vix: None,
                dxy: None,
                yield_10y: None,
                oil: None,
                gold: None,
                btc: None,
            },
            recorded_at: None,
        },
    };

    let generated_at = chrono::Utc::now().to_rfc3339();

    // Check staleness across all price entries (portfolio + market)
    let all_entries: Vec<&PriceEntry> = prices
        .portfolio
        .iter()
        .chain(prices.market.iter())
        .collect();
    let staleness_warning = check_staleness(
        &all_entries
            .iter()
            .map(|e| (*e).clone())
            .collect::<Vec<_>>(),
    );

    Ok(MarketSnapshot {
        generated_at,
        staleness_warning,
        prices,
        sentiment,
        regime,
    })
}

// ── Terminal output ──────────────────────────────────────────────────

fn print_terminal(snap: &MarketSnapshot) {
    println!("═══ Market Snapshot ═══\n");

    // Staleness warning
    if let Some(ref warning) = snap.staleness_warning {
        println!("  ⚠ {}\n", warning.message);
    }

    // Regime
    println!(
        "Regime: {} (confidence: {})",
        snap.regime.current_regime,
        snap.regime
            .confidence
            .map(|c| format!("{:.0}%", c * 100.0))
            .unwrap_or_else(|| "N/A".to_string())
    );
    if let Some(ref drivers) = snap.regime.drivers {
        println!("Drivers: {}", drivers);
    }
    let kl = &snap.regime.key_levels;
    let mut levels = Vec::new();
    if let Some(v) = kl.vix {
        levels.push(format!("VIX {:.1}", v));
    }
    if let Some(v) = kl.dxy {
        levels.push(format!("DXY {:.1}", v));
    }
    if let Some(v) = kl.yield_10y {
        levels.push(format!("10Y {:.2}%", v));
    }
    if let Some(v) = kl.oil {
        levels.push(format!("Oil ${:.1}", v));
    }
    if let Some(v) = kl.gold {
        levels.push(format!("Gold ${:.0}", v));
    }
    if let Some(v) = kl.btc {
        levels.push(format!("BTC ${:.0}", v));
    }
    if !levels.is_empty() {
        println!("Key: {}", levels.join(" | "));
    }
    println!();

    // Sentiment
    println!(
        "Sentiment: {} (score: {}, {} articles)",
        snap.sentiment.overall_label,
        snap.sentiment.overall_score,
        snap.sentiment.articles_scored,
    );
    if !snap.sentiment.by_category.is_empty() {
        let cats: Vec<String> = snap
            .sentiment
            .by_category
            .iter()
            .map(|c| format!("{}:{}", c.category, c.label))
            .collect();
        println!("  {}", cats.join(" | "));
    }
    println!();

    // Portfolio prices
    if !snap.prices.portfolio.is_empty() {
        println!("Portfolio ({} symbols):", snap.prices.portfolio_count);
        print_price_table(&snap.prices.portfolio);
        println!();
    }

    // Market prices
    if !snap.prices.market.is_empty() {
        println!("Market ({} symbols):", snap.prices.market_count);
        print_price_table(&snap.prices.market);
        println!();
    }

    println!("Generated: {}", snap.generated_at);
}

fn print_price_table(entries: &[PriceEntry]) {
    let sym_w = entries
        .iter()
        .map(|e| e.symbol.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let name_w = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(4)
        .clamp(4, 20);

    for e in entries {
        let price_str = match e.price {
            Some(p) => {
                let dp = if p.abs() >= dec!(1) { 2 } else { 4 };
                format!("{:.prec$}", p.round_dp(dp), prec = dp as usize)
            }
            None => "N/A".to_string(),
        };
        let pct_str = match e.change_pct {
            Some(p) => {
                let f: f64 = p.to_string().parse().unwrap_or(0.0);
                format!("{:+.2}%", f)
            }
            None => "---".to_string(),
        };
        let name_display: String = e.name.chars().take(20).collect();
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:>12}  {:>8}",
            e.symbol, name_display, price_str, pct_str,
        );
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn snapshot_empty_db() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let snap = build_snapshot(&backend).unwrap();
        assert_eq!(snap.prices.portfolio_count, 0);
        assert_eq!(snap.sentiment.articles_scored, 0);
        assert_eq!(snap.regime.current_regime, "unknown");
    }

    #[test]
    fn snapshot_empty_db_json() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, true);
        assert!(result.is_ok());
    }

    #[test]
    fn snapshot_empty_db_terminal() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, false);
        assert!(result.is_ok());
    }

    #[test]
    fn snapshot_with_watchlist() {
        let conn = open_in_memory();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::PriceQuote;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(195.50),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-04-02T04:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let snap = build_snapshot(&backend).unwrap();
        assert_eq!(snap.prices.portfolio_count, 1);
        assert_eq!(snap.prices.portfolio[0].symbol, "AAPL");
        assert_eq!(snap.prices.portfolio[0].price, Some(dec!(195.50)));
    }

    #[test]
    fn snapshot_market_includes_indices() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let snap = build_snapshot(&backend).unwrap();
        // Market section should have entries even with empty portfolio
        assert!(snap.prices.market_count > 0);
        // Should include S&P 500
        assert!(snap.prices.market.iter().any(|e| e.name == "S&P 500"));
    }

    #[test]
    fn sentiment_neutral_when_no_news() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let snap = build_snapshot(&backend).unwrap();
        assert_eq!(snap.sentiment.overall_label, "neutral");
        assert_eq!(snap.sentiment.overall_score, 0);
    }

    #[test]
    fn price_entry_change_with_no_history() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let price_map: HashMap<String, (Decimal, String, String)> = HashMap::new();
        let entry = build_price_entry(
            &backend,
            "TEST",
            AssetCategory::Equity,
            "Test Stock",
            &price_map,
        );
        assert_eq!(entry.symbol, "TEST");
        assert!(entry.price.is_none());
        assert!(entry.change.is_none());
    }

    #[test]
    fn regime_unknown_when_no_snapshots() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let snap = build_snapshot(&backend).unwrap();
        assert_eq!(snap.regime.current_regime, "unknown");
        assert!(snap.regime.confidence.is_none());
        assert!(snap.regime.recorded_at.is_none());
    }
}
