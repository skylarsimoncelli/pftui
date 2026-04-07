use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc, Weekday};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Serialize;

use crate::commands::refresh::{self, RefreshPlan};
use crate::config::Config;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::{get_history_backend, get_price_at_date_backend};
use crate::db::transactions::get_unique_symbols_backend;
use crate::db::watchlist::list_watchlist_backend;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;
use crate::tui::views::markets;

/// Threshold in hours beyond which cached prices are considered stale.
const STALE_THRESHOLD_HOURS: i64 = 2;

/// Helper for serde skip_serializing_if on usize fields.
fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// Public version of is_zero for cross-module serde skip_serializing_if.
pub fn is_zero_pub(v: &usize) -> bool {
    *v == 0
}

/// US market holidays for 2025-2027 (NYSE/NASDAQ observed closure dates).
/// Maintained as a static list — add new years as needed.
const US_MARKET_HOLIDAYS: &[&str] = &[
    // 2025
    "2025-01-01", // New Year's Day
    "2025-01-20", // MLK Day
    "2025-02-17", // Presidents' Day
    "2025-04-18", // Good Friday
    "2025-05-26", // Memorial Day
    "2025-06-19", // Juneteenth
    "2025-07-04", // Independence Day
    "2025-09-01", // Labor Day
    "2025-11-27", // Thanksgiving
    "2025-12-25", // Christmas
    // 2026
    "2026-01-01", // New Year's Day
    "2026-01-19", // MLK Day
    "2026-02-16", // Presidents' Day
    "2026-04-03", // Good Friday
    "2026-05-25", // Memorial Day
    "2026-06-19", // Juneteenth
    "2026-07-03", // Independence Day (observed)
    "2026-09-07", // Labor Day
    "2026-11-26", // Thanksgiving
    "2026-12-25", // Christmas
    // 2027
    "2027-01-01", // New Year's Day
    "2027-01-18", // MLK Day
    "2027-02-15", // Presidents' Day
    "2027-03-26", // Good Friday
    "2027-05-31", // Memorial Day
    "2027-06-18", // Juneteenth (observed, falls on Sat)
    "2027-07-05", // Independence Day (observed, falls on Sun)
    "2027-09-06", // Labor Day
    "2027-11-25", // Thanksgiving
    "2027-12-24", // Christmas (observed, falls on Sat)
];

/// Check if a given date is a US market holiday.
fn is_us_market_holiday(date: NaiveDate) -> bool {
    let date_str = date.format("%Y-%m-%d").to_string();
    US_MARKET_HOLIDAYS.contains(&date_str.as_str())
}

/// Determine if the market for a given asset category is currently closed.
///
/// - **Crypto**: trades 24/7 — never considered market-closed
/// - **Forex/Cash**: trades 24/5 — closed on weekends (Sat/Sun UTC)
/// - **Equity/Fund/Commodity**: closed on weekends + US market holidays
///
/// Uses UTC date for simplicity. US market hours are roughly 13:30-20:00 UTC,
/// but for staleness purposes, the full calendar day is sufficient.
pub fn is_market_closed(category: AssetCategory, now: chrono::DateTime<Utc>) -> bool {
    match category {
        AssetCategory::Crypto => false, // 24/7
        AssetCategory::Forex | AssetCategory::Cash => {
            // Closed weekends only
            let weekday = now.weekday();
            weekday == Weekday::Sat || weekday == Weekday::Sun
        }
        AssetCategory::Equity | AssetCategory::Fund | AssetCategory::Commodity => {
            let weekday = now.weekday();
            let date = now.date_naive();
            weekday == Weekday::Sat || weekday == Weekday::Sun || is_us_market_holiday(date)
        }
    }
}

#[derive(Serialize)]
struct PriceOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    staleness_warning: Option<StalenessWarning>,
    prices: Vec<PriceRow>,
}

#[derive(Serialize)]
struct StalenessWarning {
    /// How many hours since the most recent cached price was fetched.
    stale_hours: f64,
    message: String,
    /// Number of symbols with individually stale prices (>2h old or missing).
    stale_count: usize,
    /// Total number of tracked symbols.
    total_count: usize,
    /// List of symbols whose prices are individually stale.
    stale_symbols: Vec<String>,
    /// Number of stale symbols whose market is currently closed (weekend/holiday).
    /// Omitted when zero.
    #[serde(skip_serializing_if = "is_zero")]
    market_closed_count: usize,
    /// List of stale symbols whose staleness is expected due to market closure.
    /// Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    market_closed_symbols: Vec<String>,
}

#[derive(Serialize)]
struct PriceRow {
    symbol: String,
    name: String,
    price: Option<Decimal>,
    change: Option<Decimal>,
    change_pct: Option<Decimal>,
    source: String,
    fetched_at: String,
    /// Machine-readable freshness contract for agents:
    /// - fresh: cached quote is recent enough to use
    /// - stale: cached quote exists but should be treated as degraded/fallback-only
    /// - missing: no cached quote is available
    status: String,
    /// Whether this specific symbol's cached price is stale (>2h old or missing).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stale: bool,
    /// How many hours since this symbol's price was last fetched. Omitted when fresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    age_hours: Option<f64>,
    /// Whether this symbol's market is currently closed (weekend/holiday).
    /// When true AND stale, the staleness is expected — not a data error.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    market_closed: bool,
    /// Asset category (used internally; not serialized in output).
    #[serde(skip)]
    category: AssetCategory,
}

fn compute_row_status(row: &PriceRow) -> &'static str {
    if row.price.is_none() {
        "missing"
    } else if row.stale {
        "stale"
    } else {
        "fresh"
    }
}

/// Annotate each price row with per-symbol staleness and market closure info.
/// Sets `stale`, `age_hours`, and `market_closed` on each row.
fn annotate_per_symbol_staleness(rows: &mut [PriceRow]) {
    let now = Utc::now();
    for row in rows.iter_mut() {
        // Check market closure for this asset's category
        row.market_closed = is_market_closed(row.category, now);

        if row.fetched_at.is_empty() || row.price.is_none() {
            // No cached price at all — treat as stale
            row.stale = true;
            row.age_hours = None;
        } else if let Some(ts) = parse_fetched_at(&row.fetched_at) {
            let age = now.signed_duration_since(ts);
            let hours = age.num_minutes() as f64 / 60.0;
            let rounded = (hours * 10.0).round() / 10.0;
            if age.num_hours() >= STALE_THRESHOLD_HOURS {
                row.stale = true;
                row.age_hours = Some(rounded);
            }
            // fresh: stale stays false, age_hours stays None
        } else {
            // Unparseable timestamp — treat as stale
            row.stale = true;
            row.age_hours = None;
        }
        row.status = compute_row_status(row).to_string();
    }
}

/// Check if cached prices are stale (>2h since most recent fetch).
/// Returns a warning if stale, None if fresh or no prices exist.
/// Includes per-symbol breakdown: which symbols are stale and how many.
/// Distinguishes between staleness due to market closure vs potential data errors.
fn check_staleness(rows: &[PriceRow]) -> Option<StalenessWarning> {
    let newest = rows
        .iter()
        .filter(|r| !r.fetched_at.is_empty())
        .filter_map(|r| parse_fetched_at(&r.fetched_at))
        .max()?;

    let age = Utc::now().signed_duration_since(newest);
    let stale_hours = age.num_minutes() as f64 / 60.0;

    // Collect per-symbol stale info
    let stale_symbols: Vec<String> = rows.iter().filter(|r| r.stale).map(|r| r.symbol.clone()).collect();
    let stale_count = stale_symbols.len();
    let total_count = rows.len();

    // Separate stale symbols into market-closed (expected) vs potentially errored
    let market_closed_symbols: Vec<String> = rows
        .iter()
        .filter(|r| r.stale && r.market_closed)
        .map(|r| r.symbol.clone())
        .collect();
    let market_closed_count = market_closed_symbols.len();
    let error_count = stale_count.saturating_sub(market_closed_count);

    if age.num_hours() >= STALE_THRESHOLD_HOURS {
        let hours_display = stale_hours.round() as i64;
        let message = if market_closed_count > 0 && error_count == 0 {
            // All stale symbols are market-closed — this is expected
            format!(
                "Cached prices are {}h old ({}/{} symbols stale — all markets closed). No action needed.",
                hours_display, stale_count, total_count
            )
        } else if market_closed_count > 0 {
            format!(
                "Cached prices are {}h old ({}/{} symbols stale: {} market closed, {} may need refresh). Run `pftui data refresh` for live data.",
                hours_display, stale_count, total_count, market_closed_count, error_count
            )
        } else {
            format!(
                "Cached prices are {}h old ({}/{} symbols stale). Run `pftui data refresh` for live data.",
                hours_display, stale_count, total_count
            )
        };
        Some(StalenessWarning {
            stale_hours: (stale_hours * 10.0).round() / 10.0,
            message,
            stale_count,
            total_count,
            stale_symbols,
            market_closed_count,
            market_closed_symbols,
        })
    } else if stale_count > 0 {
        // Global cache is fresh but some individual symbols are stale/missing
        let message = if market_closed_count > 0 && error_count == 0 {
            format!(
                "{}/{} symbols have stale prices (all markets closed). No action needed.",
                stale_count, total_count
            )
        } else if market_closed_count > 0 {
            format!(
                "{}/{} symbols have stale or missing prices ({} market closed, {} may need refresh). Run `pftui data refresh` to update.",
                stale_count, total_count, market_closed_count, error_count
            )
        } else {
            format!(
                "{}/{} symbols have stale or missing prices. Run `pftui data refresh` to update.",
                stale_count, total_count
            )
        };
        Some(StalenessWarning {
            stale_hours: (stale_hours * 10.0).round() / 10.0,
            message,
            stale_count,
            total_count,
            stale_symbols,
            market_closed_count,
            market_closed_symbols,
        })
    } else {
        None
    }
}

/// Parse the fetched_at timestamp string into a chrono DateTime.
/// Handles formats: "2026-04-02 08:09:25.198868+00" and ISO 8601 variants.
fn parse_fetched_at(s: &str) -> Option<chrono::DateTime<Utc>> {
    // Try standard formats
    chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z")
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%#z"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z"))
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%#z"))
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Compute daily change using price history.
fn compute_prev_close(
    backend: &BackendConnection,
    symbol: &str,
    category: AssetCategory,
    current: Decimal,
) -> Option<Decimal> {
    // Try canonical symbol first
    if let Some(prev) = prev_close_for(backend, symbol) {
        return Some(current - prev);
    }
    // Try Yahoo-mapped symbol for crypto
    let yahoo_sym = match category {
        AssetCategory::Crypto => {
            if symbol.ends_with("-USD") {
                return None;
            }
            format!("{}-USD", symbol)
        }
        _ => return None,
    };
    prev_close_for(backend, &yahoo_sym).map(|prev| current - prev)
}

fn prev_close_for(backend: &BackendConnection, symbol: &str) -> Option<Decimal> {
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

/// Check if the price cache is stale (>2h since most recent fetch).
/// Returns true if stale or empty.
pub fn is_cache_stale(backend: &BackendConnection) -> bool {
    let cached = match get_all_cached_prices_backend(backend) {
        Ok(c) => c,
        Err(_) => return true,
    };
    if cached.is_empty() {
        return true;
    }
    let newest = cached
        .iter()
        .filter(|q| !q.fetched_at.is_empty())
        .filter_map(|q| parse_fetched_at(&q.fetched_at))
        .max();
    match newest {
        Some(ts) => {
            let age = Utc::now().signed_duration_since(ts);
            age.num_hours() >= STALE_THRESHOLD_HOURS
        }
        None => true,
    }
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    market: bool,
    json: bool,
    auto_refresh: bool,
) -> Result<()> {
    // Auto-refresh: if flag is set and cache is stale, do a prices-only refresh first
    if auto_refresh && is_cache_stale(backend) {
        if !json {
            println!("  ⟳ Cache stale — auto-refreshing prices...");
        }
        let plan = RefreshPlan::prices_only();
        let _ = refresh::run_quiet_with_plan(backend, config, false, &plan);
        if !json {
            println!("  ✓ Prices refreshed.\n");
        }
    }

    // Collect all tracked symbols: portfolio holdings + watchlist
    let mut symbols: BTreeMap<String, AssetCategory> = BTreeMap::new();

    // Portfolio holdings
    let held = get_unique_symbols_backend(backend)?;
    for (sym, cat) in &held {
        symbols.insert(sym.clone(), *cat);
    }

    // Watchlist
    let watched = list_watchlist_backend(backend)?;
    for entry in &watched {
        let cat: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
        symbols.entry(entry.symbol.clone()).or_insert(cat);
    }

    // Market overview symbols (indices, commodities, crypto, forex, bonds)
    // Build a name override map for Yahoo symbols that resolve_name() may not know
    let mut market_name_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if market {
        for item in markets::market_symbols() {
            symbols.entry(item.yahoo_symbol.clone()).or_insert(item.category);
            market_name_map.insert(item.yahoo_symbol, item.name);
        }
    }

    if symbols.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No portfolio holdings or watchlist symbols found.");
        }
        return Ok(());
    }

    // Load all cached prices
    let cached = get_all_cached_prices_backend(backend)?;
    let price_map: std::collections::HashMap<String, (Decimal, String, String)> = cached
        .into_iter()
        .map(|q| (q.symbol, (q.price, q.source, q.fetched_at)))
        .collect();

    // Also build a set of Yahoo-mapped crypto symbols for lookup
    let mut yahoo_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (sym, cat) in &symbols {
        if *cat == AssetCategory::Crypto && !sym.ends_with("-USD") {
            yahoo_map.insert(format!("{}-USD", sym), sym.clone());
        }
    }

    let mut rows: Vec<PriceRow> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (sym, cat) in &symbols {
        if !seen.insert(sym.clone()) {
            continue;
        }

        let name = resolve_name(sym);
        let display_name = if name.is_empty() {
            // Fall back to market name map for Yahoo symbols like ^GSPC
            market_name_map.get(sym).cloned().unwrap_or_else(|| sym.clone())
        } else {
            name
        };

        // Look up price: try canonical symbol first, then Yahoo-mapped crypto
        let quote = price_map.get(sym).or_else(|| {
            if *cat == AssetCategory::Crypto && !sym.ends_with("-USD") {
                price_map.get(&format!("{}-USD", sym))
            } else {
                None
            }
        });

        let (price, source, fetched_at) = match quote {
            Some((p, src, ts)) => (Some(*p), src.clone(), ts.clone()),
            None => (None, String::new(), String::new()),
        };

        let (change, change_pct) = match price {
            Some(current) => {
                let chg = compute_prev_close(backend, sym, *cat, current);
                let pct = chg.and_then(|c| {
                    let prev = current - c;
                    if prev == dec!(0) {
                        None
                    } else {
                        Some(c / prev * dec!(100))
                    }
                });
                (chg, pct)
            }
            None => (None, None),
        };

        rows.push(PriceRow {
            symbol: sym.clone(),
            name: display_name,
            price,
            change,
            change_pct,
            source,
            fetched_at,
            status: "missing".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: *cat,
        });
    }

    // Annotate per-symbol staleness before computing global staleness
    annotate_per_symbol_staleness(&mut rows);

    // Check staleness of cached prices (now uses per-symbol annotations)
    let staleness = check_staleness(&rows);

    if json {
        let output = PriceOutput {
            staleness_warning: staleness,
            prices: rows,
        };
        let json_str = serde_json::to_string_pretty(&output)?;
        println!("{}", json_str);
        return Ok(());
    }

    // Table output
    if rows.is_empty() {
        println!("No tracked symbols found.");
        return Ok(());
    }

    // Show staleness warning before table
    if let Some(ref warning) = staleness {
        println!("  ⚠ {}", warning.message);
        println!();
    }

    let sym_w = rows
        .iter()
        .map(|r| r.symbol.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let name_w = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let price_w = rows
        .iter()
        .map(|r| format_decimal_opt(r.price).len())
        .max()
        .unwrap_or(5)
        .max(5);
    let chg_w = rows
        .iter()
        .map(|r| format_change_opt(r.change).len())
        .max()
        .unwrap_or(6)
        .max(6);
    let pct_w = rows
        .iter()
        .map(|r| format_pct_opt(r.change_pct).len())
        .max()
        .unwrap_or(8)
        .max(8);

    println!(
        "  {:<sym_w$}  {:<name_w$}  {:>price_w$}  {:>chg_w$}  {:>pct_w$}",
        "Symbol", "Name", "Price", "Change", "Chg %",
    );
    let total_w = sym_w + name_w + price_w + chg_w + pct_w + 16;
    println!("  {}", "\u{2500}".repeat(total_w));

    for r in &rows {
        let stale_marker = if r.stale && r.market_closed {
            " 🌙" // Market closed — staleness expected
        } else if r.stale {
            " ⚠" // Stale — possible data error
        } else {
            ""
        };
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:>price_w$}  {:>chg_w$}  {:>pct_w$}{}",
            r.symbol,
            r.name,
            format_decimal_opt(r.price),
            format_change_opt(r.change),
            format_pct_opt(r.change_pct),
            stale_marker,
        );
    }

    let priced = rows.iter().filter(|r| r.price.is_some()).count();
    let total = rows.len();
    println!();
    println!("  {}/{} symbols with cached prices.", priced, total);
    if priced < total {
        println!("  Run `pftui data refresh` to update missing prices.");
    }

    Ok(())
}

fn format_decimal_opt(v: Option<Decimal>) -> String {
    match v {
        Some(d) => {
            let dp = if d.abs() >= dec!(1) { 2 } else { 4 };
            format!("{:.prec$}", d.round_dp(dp), prec = dp as usize)
        }
        None => "N/A".to_string(),
    }
}

fn format_change_opt(v: Option<Decimal>) -> String {
    match v {
        Some(d) => {
            let dp = if d.abs() >= dec!(1) { 2 } else { 4 };
            format!("{:+.prec$}", d.round_dp(dp), prec = dp as usize)
        }
        None => "---".to_string(),
    }
}

fn format_pct_opt(v: Option<Decimal>) -> String {
    match v {
        Some(d) => {
            let f: f64 = d.to_string().parse().unwrap_or(0.0);
            format!("{:+.2}%", f)
        }
        None => "---".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::open_in_memory;

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn prices_empty_db() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, &Config::default(), false, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn prices_empty_db_json() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, &Config::default(), false, true, false);
        assert!(result.is_ok());
    }

    #[test]
    fn prices_with_watchlist_and_holdings() {
        let conn = open_in_memory();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::PriceQuote;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "BTC", AssetCategory::Crypto).unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(195.50),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-18T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC-USD".to_string(),
                price: dec!(84000),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-18T20:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);

        // Table output
        let result = run(&backend, &Config::default(), false, false, false);
        assert!(result.is_ok());

        // JSON output
        let result = run(&backend, &Config::default(), false, true, false);
        assert!(result.is_ok());
    }

    #[test]
    fn format_decimal_opt_large() {
        assert_eq!(format_decimal_opt(Some(dec!(1234.56))), "1234.56");
    }

    #[test]
    fn format_decimal_opt_small() {
        assert_eq!(format_decimal_opt(Some(dec!(0.0045))), "0.0045");
    }

    #[test]
    fn format_decimal_opt_none() {
        assert_eq!(format_decimal_opt(None), "N/A");
    }

    #[test]
    fn format_change_opt_positive() {
        assert_eq!(format_change_opt(Some(dec!(5.25))), "+5.25");
    }

    #[test]
    fn format_change_opt_negative() {
        assert_eq!(format_change_opt(Some(dec!(-3.10))), "-3.10");
    }

    #[test]
    fn format_pct_opt_positive() {
        assert_eq!(format_pct_opt(Some(dec!(2.5))), "+2.50%");
    }

    #[test]
    fn format_pct_opt_none() {
        assert_eq!(format_pct_opt(None), "---");
    }

    #[test]
    fn prices_market_flag_includes_market_symbols() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        // --market flag should not error even with empty db (no cached prices)
        let result = run(&backend, &Config::default(), true, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn prices_market_flag_json() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let result = run(&backend, &Config::default(), true, true, false);
        assert!(result.is_ok());
    }

    #[test]
    fn market_symbols_include_uranium() {
        use crate::tui::views::markets;
        let items = markets::market_symbols();
        assert!(
            items.iter().any(|i| i.yahoo_symbol == "URA"),
            "market_symbols should include URA (Uranium ETF)"
        );
    }

    #[test]
    fn market_symbols_include_copper() {
        use crate::tui::views::markets;
        let items = markets::market_symbols();
        assert!(
            items.iter().any(|i| i.yahoo_symbol == "HG=F"),
            "market_symbols should include HG=F (Copper Futures)"
        );
    }

    #[test]
    fn staleness_none_when_empty() {
        let rows: Vec<PriceRow> = vec![];
        assert!(check_staleness(&rows).is_none());
    }

    #[test]
    fn staleness_none_when_fresh() {
        let now = Utc::now();
        let fetched = now - chrono::Duration::minutes(30);
        let rows = vec![PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Equity,
        }];
        assert!(check_staleness(&rows).is_none());
    }

    #[test]
    fn staleness_warning_when_stale() {
        let now = Utc::now();
        let fetched = now - chrono::Duration::hours(3);
        let rows = vec![PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
            status: "stale".to_string(),
            stale: true,
            age_hours: Some(3.0),
            market_closed: false,
            category: AssetCategory::Equity,
        }];
        let warning = check_staleness(&rows);
        assert!(warning.is_some());
        let w = warning.unwrap();
        assert!(w.stale_hours >= 2.9);
        assert!(w.message.contains("data refresh"));
        assert_eq!(w.stale_count, 1);
        assert_eq!(w.total_count, 1);
        assert_eq!(w.stale_symbols, vec!["BTC"]);
    }

    #[test]
    fn staleness_uses_newest_timestamp() {
        let now = Utc::now();
        let old = now - chrono::Duration::hours(5);
        let fresh = now - chrono::Duration::minutes(30);
        let rows = vec![
            PriceRow {
                symbol: "OLD".to_string(),
                name: "Old Asset".to_string(),
                price: Some(dec!(100)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: old.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(5.0),
                market_closed: false,
                category: AssetCategory::Equity,
            },
            PriceRow {
                symbol: "FRESH".to_string(),
                name: "Fresh Asset".to_string(),
                price: Some(dec!(200)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fresh.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "fresh".to_string(),
                stale: false,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Equity,
            },
        ];
        // Global cache is fresh (newest is <2h old) but OLD is individually stale
        let warning = check_staleness(&rows);
        assert!(warning.is_some(), "should warn because OLD symbol is stale");
        let w = warning.unwrap();
        assert_eq!(w.stale_count, 1);
        assert_eq!(w.stale_symbols, vec!["OLD"]);
    }

    #[test]
    fn staleness_skips_empty_fetched_at() {
        let now = Utc::now();
        let fresh = now - chrono::Duration::minutes(10);
        let rows = vec![
            PriceRow {
                symbol: "MISSING".to_string(),
                name: "No Timestamp".to_string(),
                price: None,
                change: None,
                change_pct: None,
                source: String::new(),
                fetched_at: String::new(),
                status: "missing".to_string(),
                stale: true,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Equity,
            },
            PriceRow {
                symbol: "OK".to_string(),
                name: "Has Timestamp".to_string(),
                price: Some(dec!(100)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fresh.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "fresh".to_string(),
                stale: false,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Equity,
            },
        ];
        // Global is fresh but MISSING is individually stale (no price)
        let warning = check_staleness(&rows);
        assert!(warning.is_some());
        let w = warning.unwrap();
        assert_eq!(w.stale_count, 1);
        assert_eq!(w.stale_symbols, vec!["MISSING"]);
    }

    #[test]
    fn parse_fetched_at_postgres_format() {
        use chrono::Datelike;
        let dt = parse_fetched_at("2026-04-02 08:09:25.198868+00");
        assert!(dt.is_some());
        assert_eq!(dt.unwrap().year(), 2026);
    }

    #[test]
    fn parse_fetched_at_iso_format() {
        let dt = parse_fetched_at("2026-04-02T08:09:25.198868+00");
        assert!(dt.is_some());
    }

    #[test]
    fn parse_fetched_at_no_fractional() {
        let dt = parse_fetched_at("2026-04-02 08:09:25+00");
        assert!(dt.is_some());
    }

    #[test]
    fn parse_fetched_at_invalid() {
        assert!(parse_fetched_at("not-a-date").is_none());
        assert!(parse_fetched_at("").is_none());
    }

    #[test]
    fn staleness_json_output_includes_warning() {
        let warning = StalenessWarning {
            stale_hours: 3.5,
            message: "Cached prices are 4h old (2/3 symbols stale). Run `pftui data refresh` for live data.".to_string(),
            stale_count: 2,
            total_count: 3,
            stale_symbols: vec!["BTC".to_string(), "AAPL".to_string()],
            market_closed_count: 0,
            market_closed_symbols: vec![],
        };
        let output = PriceOutput {
            staleness_warning: Some(warning),
            prices: vec![],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(json_str.contains("staleness_warning"));
        assert!(json_str.contains("stale_hours"));
        assert!(json_str.contains("data refresh"));
        assert!(json_str.contains("stale_count"));
        assert!(json_str.contains("stale_symbols"));
        assert!(json_str.contains("BTC"));
    }

    #[test]
    fn staleness_json_output_omits_when_none() {
        let output = PriceOutput {
            staleness_warning: None,
            prices: vec![],
        };
        let json_str = serde_json::to_string(&output).unwrap();
        assert!(!json_str.contains("staleness_warning"));
    }

    #[test]
    fn per_symbol_staleness_fresh_symbol() {
        let now = Utc::now();
        let fetched = now - chrono::Duration::minutes(30);
        let mut rows = vec![PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Equity,
        }];
        annotate_per_symbol_staleness(&mut rows);
        assert!(!rows[0].stale);
        assert!(rows[0].age_hours.is_none());
    }

    #[test]
    fn per_symbol_staleness_old_symbol() {
        let now = Utc::now();
        let fetched = now - chrono::Duration::hours(5);
        let mut rows = vec![PriceRow {
            symbol: "AAPL".to_string(),
            name: "Apple".to_string(),
            price: Some(dec!(195)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Equity,
        }];
        annotate_per_symbol_staleness(&mut rows);
        assert!(rows[0].stale);
        assert!(rows[0].age_hours.unwrap() >= 4.9);
    }

    #[test]
    fn per_symbol_staleness_missing_price() {
        let mut rows = vec![PriceRow {
            symbol: "MISSING".to_string(),
            name: "No Price".to_string(),
            price: None,
            change: None,
            change_pct: None,
            source: String::new(),
            fetched_at: String::new(),
            status: "missing".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Equity,
        }];
        annotate_per_symbol_staleness(&mut rows);
        assert!(rows[0].stale);
        assert!(rows[0].age_hours.is_none());
    }

    #[test]
    fn per_symbol_staleness_mixed_freshness() {
        let now = Utc::now();
        let old = now - chrono::Duration::hours(4);
        let fresh = now - chrono::Duration::minutes(15);
        let mut rows = vec![
            PriceRow {
                symbol: "OLD".to_string(),
                name: "Old Asset".to_string(),
                price: Some(dec!(100)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: old.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "fresh".to_string(),
                stale: false,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Equity,
            },
            PriceRow {
                symbol: "FRESH".to_string(),
                name: "Fresh Asset".to_string(),
                price: Some(dec!(200)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fresh.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "fresh".to_string(),
                stale: false,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Equity,
            },
        ];
        annotate_per_symbol_staleness(&mut rows);
        assert!(rows[0].stale, "OLD should be stale");
        assert!(rows[0].age_hours.unwrap() >= 3.9);
        assert!(!rows[1].stale, "FRESH should not be stale");
        assert!(rows[1].age_hours.is_none());
    }

    #[test]
    fn per_symbol_staleness_json_omits_when_fresh() {
        let row = PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: "2026-04-02T12:00:00+00".to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Equity,
        };
        let json_str = serde_json::to_string(&row).unwrap();
        assert!(!json_str.contains("stale"), "fresh row should not contain stale field");
        assert!(!json_str.contains("age_hours"), "fresh row should not contain age_hours field");
    }

    #[test]
    fn per_symbol_staleness_json_includes_when_stale() {
        let row = PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: "2026-04-02T08:00:00+00".to_string(),
            status: "stale".to_string(),
            stale: true,
            age_hours: Some(4.2),
            market_closed: false,
            category: AssetCategory::Equity,
        };
        let json_str = serde_json::to_string(&row).unwrap();
        assert!(json_str.contains("\"stale\":true"), "stale row should contain stale:true");
        assert!(json_str.contains("\"age_hours\":4.2"), "stale row should contain age_hours");
    }

    #[test]
    fn per_symbol_status_serializes_for_agent_fallbacks() {
        let row = PriceRow {
            symbol: "SI=F".to_string(),
            name: "Silver".to_string(),
            price: Some(dec!(81)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: "2026-03-16T21:31:08+00".to_string(),
            status: "stale".to_string(),
            stale: true,
            age_hours: Some(509.5),
            market_closed: false,
            category: AssetCategory::Commodity,
        };
        let json_str = serde_json::to_string(&row).unwrap();
        assert!(json_str.contains("\"status\":\"stale\""));
    }

    #[test]
    fn annotate_updates_status_for_missing_and_stale_rows() {
        let now = Utc::now();
        let old = now - chrono::Duration::hours(5);
        let mut rows = vec![
            PriceRow {
                symbol: "SI=F".to_string(),
                name: "Silver".to_string(),
                price: Some(dec!(81)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: old.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "fresh".to_string(),
                stale: false,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Commodity,
            },
            PriceRow {
                symbol: "MISSING".to_string(),
                name: "Missing".to_string(),
                price: None,
                change: None,
                change_pct: None,
                source: String::new(),
                fetched_at: String::new(),
                status: "fresh".to_string(),
                stale: false,
                age_hours: None,
                market_closed: false,
                category: AssetCategory::Equity,
            },
        ];
        annotate_per_symbol_staleness(&mut rows);
        assert_eq!(rows[0].status, "stale");
        assert_eq!(rows[1].status, "missing");
    }

    #[test]
    fn staleness_warning_includes_per_symbol_breakdown() {
        let now = Utc::now();
        let old = now - chrono::Duration::hours(3);
        let rows = vec![
            PriceRow {
                symbol: "BTC".to_string(),
                name: "Bitcoin".to_string(),
                price: Some(dec!(84000)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: old.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(3.0),
                market_closed: false,
                category: AssetCategory::Equity,
            },
            PriceRow {
                symbol: "GOLD".to_string(),
                name: "Gold".to_string(),
                price: Some(dec!(2100)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: old.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(3.0),
                market_closed: false,
                category: AssetCategory::Equity,
            },
        ];
        let warning = check_staleness(&rows).unwrap();
        assert_eq!(warning.stale_count, 2);
        assert_eq!(warning.total_count, 2);
        assert!(warning.stale_symbols.contains(&"BTC".to_string()));
        assert!(warning.stale_symbols.contains(&"GOLD".to_string()));
        assert!(warning.message.contains("2/2"));
    }

    #[test]
    fn is_cache_stale_empty_db() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        assert!(is_cache_stale(&backend));
    }

    #[test]
    fn is_cache_stale_fresh_data() {
        let conn = open_in_memory();
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        let now = Utc::now();
        let fetched = now - chrono::Duration::minutes(30);

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC-USD".to_string(),
                price: dec!(84000),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: fetched
                    .format("%Y-%m-%d %H:%M:%S%.6f+00")
                    .to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        assert!(!is_cache_stale(&backend));
    }

    #[test]
    fn is_cache_stale_old_data() {
        let conn = open_in_memory();
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        let now = Utc::now();
        let fetched = now - chrono::Duration::hours(3);

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "BTC-USD".to_string(),
                price: dec!(84000),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: fetched
                    .format("%Y-%m-%d %H:%M:%S%.6f+00")
                    .to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        assert!(is_cache_stale(&backend));
    }

    #[test]
    fn auto_refresh_not_triggered_when_fresh() {
        let conn = open_in_memory();
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        let now = Utc::now();
        let fetched = now - chrono::Duration::minutes(10);

        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(195),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: fetched
                    .format("%Y-%m-%d %H:%M:%S%.6f+00")
                    .to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let config = Config::default();
        // auto_refresh=true but cache is fresh → should NOT attempt refresh, just return data
        let result = run(&backend, &config, false, false, true);
        assert!(result.is_ok());
    }

    // --- Market closure tests ---

    #[test]
    fn crypto_never_market_closed() {
        use chrono::TimeZone;
        // Saturday at noon UTC
        let saturday = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap();
        assert!(!is_market_closed(AssetCategory::Crypto, saturday));
        // Sunday
        let sunday = Utc.with_ymd_and_hms(2026, 4, 5, 12, 0, 0).unwrap();
        assert!(!is_market_closed(AssetCategory::Crypto, sunday));
        // US holiday (Good Friday 2026)
        let good_friday = Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap();
        assert!(!is_market_closed(AssetCategory::Crypto, good_friday));
    }

    #[test]
    fn equity_closed_on_weekends() {
        use chrono::TimeZone;
        let saturday = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Equity, saturday));
        let sunday = Utc.with_ymd_and_hms(2026, 4, 5, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Equity, sunday));
        // Weekday — open
        let monday = Utc.with_ymd_and_hms(2026, 4, 6, 15, 0, 0).unwrap();
        assert!(!is_market_closed(AssetCategory::Equity, monday));
    }

    #[test]
    fn equity_closed_on_us_holidays() {
        use chrono::TimeZone;
        // Good Friday 2026 (observed)
        let good_friday = Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Equity, good_friday));
        // Christmas 2026
        let christmas = Utc.with_ymd_and_hms(2026, 12, 25, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Equity, christmas));
        // Regular Wednesday — open
        let wed = Utc.with_ymd_and_hms(2026, 4, 8, 12, 0, 0).unwrap();
        assert!(!is_market_closed(AssetCategory::Equity, wed));
    }

    #[test]
    fn forex_closed_on_weekends_only() {
        use chrono::TimeZone;
        let saturday = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Forex, saturday));
        // US holiday (Good Friday) — forex still open
        let good_friday = Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap();
        assert!(!is_market_closed(AssetCategory::Forex, good_friday));
    }

    #[test]
    fn commodity_closed_on_weekends_and_holidays() {
        use chrono::TimeZone;
        let saturday = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Commodity, saturday));
        let good_friday = Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Commodity, good_friday));
    }

    #[test]
    fn fund_closed_on_weekends_and_holidays() {
        use chrono::TimeZone;
        let saturday = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Fund, saturday));
        let good_friday = Utc.with_ymd_and_hms(2026, 4, 3, 12, 0, 0).unwrap();
        assert!(is_market_closed(AssetCategory::Fund, good_friday));
    }

    #[test]
    fn is_us_market_holiday_known_dates() {
        // 2026 holidays
        assert!(is_us_market_holiday(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()));
        assert!(is_us_market_holiday(NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()));
        assert!(is_us_market_holiday(NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()));
        // Regular day — not a holiday
        assert!(!is_us_market_holiday(NaiveDate::from_ymd_opt(2026, 4, 8).unwrap()));
    }

    #[test]
    fn market_closed_json_omits_when_false() {
        let row = PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: "2026-04-02T12:00:00+00".to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Crypto,
        };
        let json_str = serde_json::to_string(&row).unwrap();
        assert!(!json_str.contains("market_closed"), "market_closed should be omitted when false");
    }

    #[test]
    fn market_closed_json_includes_when_true() {
        let row = PriceRow {
            symbol: "AAPL".to_string(),
            name: "Apple".to_string(),
            price: Some(dec!(195)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: "2026-04-04T08:00:00+00".to_string(),
            status: "stale".to_string(),
            stale: true,
            age_hours: Some(16.0),
            market_closed: true,
            category: AssetCategory::Equity,
        };
        let json_str = serde_json::to_string(&row).unwrap();
        assert!(json_str.contains("\"market_closed\":true"), "market_closed should be present when true");
    }

    #[test]
    fn staleness_warning_all_market_closed() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(); // Saturday
        let fetched = now - chrono::Duration::hours(16);
        let rows = vec![
            PriceRow {
                symbol: "AAPL".to_string(),
                name: "Apple".to_string(),
                price: Some(dec!(195)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(16.0),
                market_closed: true,
                category: AssetCategory::Equity,
            },
            PriceRow {
                symbol: "GC=F".to_string(),
                name: "Gold Futures".to_string(),
                price: Some(dec!(3100)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(16.0),
                market_closed: true,
                category: AssetCategory::Commodity,
            },
        ];
        let warning = check_staleness(&rows).unwrap();
        assert_eq!(warning.market_closed_count, 2);
        assert_eq!(warning.market_closed_symbols.len(), 2);
        assert!(warning.message.contains("all markets closed"));
        assert!(warning.message.contains("No action needed"));
    }

    #[test]
    fn staleness_warning_mixed_market_closed_and_error() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 4, 4, 12, 0, 0).unwrap(); // Saturday
        let fetched = now - chrono::Duration::hours(16);
        let rows = vec![
            PriceRow {
                symbol: "AAPL".to_string(),
                name: "Apple".to_string(),
                price: Some(dec!(195)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(16.0),
                market_closed: true,
                category: AssetCategory::Equity,
            },
            PriceRow {
                symbol: "BTC".to_string(),
                name: "Bitcoin".to_string(),
                price: Some(dec!(84000)),
                change: None,
                change_pct: None,
                source: "yahoo".to_string(),
                fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
                status: "stale".to_string(),
                stale: true,
                age_hours: Some(16.0),
                market_closed: false, // Crypto is 24/7 — this IS an error
                category: AssetCategory::Crypto,
            },
        ];
        let warning = check_staleness(&rows).unwrap();
        assert_eq!(warning.market_closed_count, 1);
        assert_eq!(warning.stale_count, 2);
        assert!(warning.message.contains("market closed"));
        assert!(warning.message.contains("may need refresh"));
    }

    #[test]
    fn staleness_warning_json_omits_market_closed_when_zero() {
        let warning = StalenessWarning {
            stale_hours: 3.5,
            message: "test".to_string(),
            stale_count: 1,
            total_count: 2,
            stale_symbols: vec!["BTC".to_string()],
            market_closed_count: 0,
            market_closed_symbols: vec![],
        };
        let json_str = serde_json::to_string(&warning).unwrap();
        assert!(!json_str.contains("market_closed_count"), "should omit market_closed_count when zero");
        assert!(!json_str.contains("market_closed_symbols"), "should omit market_closed_symbols when empty");
    }

    #[test]
    fn staleness_warning_json_includes_market_closed_when_nonzero() {
        let warning = StalenessWarning {
            stale_hours: 16.0,
            message: "test".to_string(),
            stale_count: 2,
            total_count: 3,
            stale_symbols: vec!["AAPL".to_string(), "GC=F".to_string()],
            market_closed_count: 2,
            market_closed_symbols: vec!["AAPL".to_string(), "GC=F".to_string()],
        };
        let json_str = serde_json::to_string(&warning).unwrap();
        assert!(json_str.contains("\"market_closed_count\":2"), "should include market_closed_count");
        assert!(json_str.contains("market_closed_symbols"), "should include market_closed_symbols");
    }

    #[test]
    fn annotate_sets_market_closed_for_equity_on_weekend() {
        // This test only asserts market_closed is set during annotation.
        // It may pass/fail depending on the current day being a weekend or not,
        // so we construct the expected value dynamically.
        let now = Utc::now();
        let fetched = now - chrono::Duration::hours(3);
        let expected_closed = is_market_closed(AssetCategory::Equity, now);
        let mut rows = vec![PriceRow {
            symbol: "AAPL".to_string(),
            name: "Apple".to_string(),
            price: Some(dec!(195)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Equity,
        }];
        annotate_per_symbol_staleness(&mut rows);
        assert_eq!(rows[0].market_closed, expected_closed);
    }

    #[test]
    fn annotate_never_sets_market_closed_for_crypto() {
        let now = Utc::now();
        let fetched = now - chrono::Duration::hours(3);
        let mut rows = vec![PriceRow {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            price: Some(dec!(84000)),
            change: None,
            change_pct: None,
            source: "yahoo".to_string(),
            fetched_at: fetched.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string(),
            status: "fresh".to_string(),
            stale: false,
            age_hours: None,
            market_closed: false,
            category: AssetCategory::Crypto,
        }];
        annotate_per_symbol_staleness(&mut rows);
        assert!(!rows[0].market_closed, "crypto should never be market_closed");
    }
}
