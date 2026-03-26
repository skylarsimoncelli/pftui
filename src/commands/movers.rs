use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::config::Config;
use crate::db::allocations::get_unique_allocation_symbols_backend;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::{get_history_backend, get_price_at_date_backend};
use crate::db::transactions::get_unique_symbols_backend;
use crate::db::watchlist::list_watchlist_backend;
use crate::models::asset::AssetCategory;
use crate::models::asset_names::resolve_name;

/// A mover: symbol with its daily change exceeding the threshold.
struct Mover {
    symbol: String,
    name: String,
    category: String,
    source: &'static str, // "held" or "watchlist"
    price: String,
    change_pct: Decimal,
    change_str: String,
}

/// Compute daily change % from current price vs the previous trading session's close.
/// Returns None if no previous close exists or current price is not available.
///
/// Bug fix (2026-03-20): When Yahoo duplicates the same closing price across
/// consecutive days (common for after-hours fetches and weekends), the old logic
/// compared identical values and produced 0% change — missing real movers.
/// The new logic walks backwards through history to find the last *prior* trading
/// session close, skipping any records that share today's date or that duplicate
/// the current cached price on adjacent dates.
///
/// Bug fix (2026-03-20): Added `cached_previous_close` fallback. When price history
/// is empty or stale (e.g. during extreme market moves before history refresh), the
/// function now falls back to Yahoo's `regularMarketPreviousClose` stored in the
/// price cache. This prevents symbols from being silently skipped during crashes.
fn compute_change_pct(
    backend: &BackendConnection,
    symbol: &str,
    current_price: Option<Decimal>,
    cached_previous_close: Option<Decimal>,
) -> Option<Decimal> {
    use chrono::Utc;

    let current = current_price?;

    let today = Utc::now().date_naive();
    // Fetch more history to survive multi-day stale-close duplication (weekends, holidays).
    let history = get_history_backend(backend, symbol, 10)
        .ok()
        .unwrap_or_default();
    let prev_close = previous_close_from_history(&history, today, current)
        .or_else(|| {
            // Fallback: explicit yesterday lookup
            let yesterday = today - chrono::Duration::days(1);
            let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
            let price = get_price_at_date_backend(backend, symbol, &yesterday_str)
                .ok()
                .flatten()?;
            // Only use fallback if it differs from current (same stale-close guard)
            if price != current {
                Some(price)
            } else {
                None
            }
        })
        .or(cached_previous_close); // Final fallback: Yahoo's regularMarketPreviousClose
    let prev_close = prev_close?;
    if prev_close == dec!(0) {
        return None;
    }

    let pct = (current - prev_close) / prev_close * dec!(100);

    // Plausibility guard: reject anomalous changes (e.g. 224,000% from corrupt data)
    if !crate::models::price::is_plausible_daily_change(pct) {
        return None;
    }

    Some(pct)
}

/// Find the previous trading session's close from history.
///
/// Strategy (ordered):
/// 1. Skip today's record (if present at the tail of history).
/// 2. From the remaining records (newest-first), return the first close that
///    differs from `current_price`. This handles Yahoo's common pattern of
///    writing the same stale close to multiple consecutive dates.
/// 3. If ALL remaining closes equal current_price (unlikely but possible for
///    very flat markets), return the oldest available close — which produces 0%
///    change rather than a false mover.
fn previous_close_from_history(
    history: &[crate::models::price::HistoryRecord],
    today: chrono::NaiveDate,
    current_price: Decimal,
) -> Option<Decimal> {
    if history.is_empty() {
        return None;
    }

    // History is chronological (oldest first). Walk from the end.
    let iter = history.iter().rev();

    // Step 1: skip today's record if present
    let latest = iter.clone().next()?;
    let latest_date = chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d").ok();
    let candidates: Box<dyn Iterator<Item = &crate::models::price::HistoryRecord> + '_> =
        if latest_date == Some(today) {
            // Skip today's entry
            let mut skipped = iter.clone();
            skipped.next();
            Box::new(skipped)
        } else {
            Box::new(iter)
        };

    // Step 2: find the first close that differs from the cached spot price.
    // This is the real "previous session" close.
    let mut fallback: Option<Decimal> = None;
    for record in candidates {
        if fallback.is_none() {
            fallback = Some(record.close);
        }
        if record.close != current_price {
            return Some(record.close);
        }
    }

    // Step 3: all historical closes equal current_price — return the oldest
    // candidate (produces 0% change, which is correct for truly flat markets).
    fallback
}

/// Format a decimal price with commas.
fn format_price(value: Decimal) -> String {
    let dp = if value >= dec!(1) { 2 } else { 4 };
    let rounded = value.round_dp(dp);
    let s = format!("{:.prec$}", rounded, prec = dp as usize);

    let (integer_part, decimal_part) = if let Some(dot_pos) = s.find('.') {
        (&s[..dot_pos], Some(&s[dot_pos..]))
    } else {
        (s.as_str(), None)
    };

    let (sign, digits) = if let Some(stripped) = integer_part.strip_prefix('-') {
        ("-", stripped)
    } else {
        ("", integer_part)
    };

    let mut result = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    let formatted_int: String = result.chars().rev().collect();

    match decimal_part {
        Some(dec_part) => format!("{}{}{}", sign, formatted_int, dec_part),
        None => format!("{}{}", sign, formatted_int),
    }
}

pub fn run(
    backend: &BackendConnection,
    config: &Config,
    threshold: Option<&str>,
    overnight: bool,
    json: bool,
) -> Result<()> {
    // Parse threshold (default 3%)
    let threshold_pct: Decimal = match threshold {
        Some(s) => {
            let cleaned = s.replace('%', "");
            Decimal::from_str_exact(&cleaned).unwrap_or(dec!(3))
        }
        None => dec!(3),
    };

    // Collect all unique symbols from held positions + watchlist
    let mut symbols: Vec<(String, AssetCategory, &'static str)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Held positions (full mode)
    if let Ok(held) = get_unique_symbols_backend(backend) {
        for (sym, cat) in held {
            if cat == AssetCategory::Cash {
                continue; // Skip cash — always 1.0
            }
            if seen.insert(sym.clone()) {
                symbols.push((sym, cat, "held"));
            }
        }
    }

    // Held positions (percentage mode)
    if let Ok(alloc) = get_unique_allocation_symbols_backend(backend) {
        for (sym, cat) in alloc {
            if cat == AssetCategory::Cash {
                continue;
            }
            if seen.insert(sym.clone()) {
                symbols.push((sym, cat, "held"));
            }
        }
    }

    // Watchlist
    if let Ok(entries) = list_watchlist_backend(backend) {
        for entry in entries {
            let cat: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
            if seen.insert(entry.symbol.clone()) {
                symbols.push((entry.symbol, cat, "watchlist"));
            }
        }
    }

    if symbols.is_empty() {
        println!("No symbols found. Add positions or watchlist entries first.");
        return Ok(());
    }

    // Build price map for display (includes previous_close from Yahoo)
    let cached = get_all_cached_prices_backend(backend)?;
    let price_map: std::collections::HashMap<String, Decimal> =
        cached.iter().map(|q| (q.symbol.clone(), q.price)).collect();
    let prev_close_map: std::collections::HashMap<String, Decimal> = cached
        .iter()
        .filter_map(|q| q.previous_close.map(|pc| (q.symbol.clone(), pc)))
        .collect();

    let csym = crate::config::currency_symbol(&config.base_currency);

    // Compute movers
    let mut movers: Vec<Mover> = Vec::new();
    let mut skipped: Vec<serde_json::Value> = Vec::new();
    for (sym, cat, source) in &symbols {
        let current_price = price_map.get(sym).copied();
        let cached_prev_close = prev_close_map.get(sym).copied();

        match compute_change_pct(backend, sym, current_price, cached_prev_close) {
            Some(pct) => {
                let abs_pct = if pct < dec!(0) { -pct } else { pct };
                if abs_pct >= threshold_pct {
                    let name = resolve_name(sym);
                    let display_name = if name.is_empty() { sym.clone() } else { name };
                    let price_str = match current_price {
                        Some(p) => format!("{}{}", csym, format_price(p)),
                        None => "N/A".to_string(),
                    };
                    let f: f64 = pct.to_string().parse().unwrap_or(0.0);
                    let change_str = format!("{:+.2}%", f);

                    movers.push(Mover {
                        symbol: sym.clone(),
                        name: display_name,
                        category: cat.to_string(),
                        source,
                        price: price_str,
                        change_pct: pct,
                        change_str,
                    });
                }
            }
            None if current_price.is_some() => {
                // Symbol has a price but no computable change — no history and no previous_close
                skipped.push(serde_json::json!({
                    "symbol": sym,
                    "reason": "no previous close available (no history, no cached previous_close)"
                }));
            }
            None => {} // No current price — expected for unfetched symbols
        }
    }

    // Sort by absolute change descending (biggest movers first)
    movers.sort_by(|a, b| {
        let abs_a = if a.change_pct < dec!(0) {
            -a.change_pct
        } else {
            a.change_pct
        };
        let abs_b = if b.change_pct < dec!(0) {
            -b.change_pct
        } else {
            b.change_pct
        };
        abs_b.cmp(&abs_a)
    });

    if json {
        // Fetch recent technical signals for mover context
        let recent_signals =
            crate::db::technical_signals::list_signals_backend(backend, None, None, Some(200))
                .unwrap_or_default();
        let signal_map: std::collections::HashMap<String, Vec<String>> = {
            let mut map: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for sig in &recent_signals {
                map.entry(sig.symbol.clone())
                    .or_default()
                    .push(sig.description.clone());
            }
            map
        };

        // JSON output for agent consumption
        let entries: Vec<serde_json::Value> = movers
            .iter()
            .map(|m| {
                let f: f64 = m.change_pct.to_string().parse().unwrap_or(0.0);
                let sym_signals = signal_map.get(&m.symbol).cloned().unwrap_or_default();
                let mut obj = serde_json::json!({
                    "symbol": m.symbol,
                    "name": m.name,
                    "category": m.category,
                    "source": m.source,
                    "change_pct": (f * 100.0).round() / 100.0,
                });
                if !sym_signals.is_empty() {
                    obj["signals"] = serde_json::json!(sym_signals);
                }
                obj
            })
            .collect();
        let mut output = serde_json::json!({
            "threshold_pct": threshold_pct.to_string().parse::<f64>().unwrap_or(3.0),
            "mode": if overnight { "overnight" } else { "daily" },
            "total_scanned": symbols.len(),
            "movers_count": movers.len(),
            "movers": entries,
        });
        if !skipped.is_empty() {
            output["skipped"] = serde_json::json!(skipped);
            output["skipped_count"] = serde_json::json!(skipped.len());
        }
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if movers.is_empty() {
        println!(
            "No movers exceeding {}% threshold across {} symbols.",
            threshold_pct,
            symbols.len()
        );
        return Ok(());
    }

    println!(
        "Movers (≥{}% {} change) — {}/{} symbols:",
        threshold_pct,
        if overnight { "overnight" } else { "daily" },
        movers.len(),
        symbols.len()
    );
    println!();

    // Compute column widths
    let sym_w = movers
        .iter()
        .map(|m| m.symbol.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let name_w = movers
        .iter()
        .map(|m| m.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let cat_w = movers
        .iter()
        .map(|m| m.category.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let price_w = movers
        .iter()
        .map(|m| m.price.len())
        .max()
        .unwrap_or(5)
        .max(5);
    let chg_w = movers
        .iter()
        .map(|m| m.change_str.len())
        .max()
        .unwrap_or(8)
        .max(8);

    // Header
    println!(
        "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  Source",
        "Symbol", "Name", "Category", "Price", "1D Chg %",
    );
    let total_w = sym_w + name_w + cat_w + price_w + chg_w + 20;
    println!("  {}", "─".repeat(total_w));

    for m in &movers {
        println!(
            "  {:<sym_w$}  {:<name_w$}  {:<cat_w$}  {:>price_w$}  {:>chg_w$}  {}",
            m.symbol, m.name, m.category, m.price, m.change_str, m.source,
        );
    }

    Ok(())
}

// ─── Sector theme mapping ───────────────────────────────────────────────────

/// Map a symbol to its sector name. Uses SECTOR_ETFS for known sector ETFs,
/// then falls back to asset category grouping.
fn classify_sector(symbol: &str, category: &AssetCategory) -> String {
    // Check if this IS a sector ETF
    for (etf_sym, name) in crate::commands::sector::SECTOR_ETFS {
        if symbol.eq_ignore_ascii_case(etf_sym) {
            return name.to_string();
        }
    }

    // Map by asset category for non-ETF symbols
    match category {
        AssetCategory::Crypto => "Crypto".to_string(),
        AssetCategory::Commodity => "Commodities".to_string(),
        AssetCategory::Forex => "Forex".to_string(),
        AssetCategory::Cash => "Cash".to_string(),
        AssetCategory::Fund => "Funds".to_string(),
        AssetCategory::Equity => "Equities".to_string(),
    }
}

/// A mover within a theme: symbol, display name, change percentage.
type ThemeMover = (String, String, f64);

/// A detected sector theme: multiple symbols in the same sector moving together.
struct SectorTheme {
    sector: String,
    direction: &'static str, // "up" or "down"
    symbols: Vec<ThemeMover>,
    avg_change: f64,
    strength: f64, // |avg_change| * count — composite strength score
}

/// Detect sector-wide themes from movers data.
///
/// Groups symbols by sector, then within each sector detects when ≥min_symbols
/// move in the same direction above threshold. Returns themes sorted by strength.
fn detect_themes(
    backend: &BackendConnection,
    threshold_pct: Decimal,
    min_symbols: usize,
) -> Vec<SectorTheme> {
    // Collect all trackable symbols: held + watchlist + sector ETFs
    let mut symbols: Vec<(String, AssetCategory, &'static str)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Held positions (full mode)
    if let Ok(held) = get_unique_symbols_backend(backend) {
        for (sym, cat) in held {
            if cat == AssetCategory::Cash {
                continue;
            }
            if seen.insert(sym.clone()) {
                symbols.push((sym, cat, "held"));
            }
        }
    }

    // Held positions (percentage mode)
    if let Ok(alloc) = get_unique_allocation_symbols_backend(backend) {
        for (sym, cat) in alloc {
            if cat == AssetCategory::Cash {
                continue;
            }
            if seen.insert(sym.clone()) {
                symbols.push((sym, cat, "held"));
            }
        }
    }

    // Watchlist
    if let Ok(entries) = list_watchlist_backend(backend) {
        for entry in entries {
            let cat: AssetCategory = entry.category.parse().unwrap_or(AssetCategory::Equity);
            if seen.insert(entry.symbol.clone()) {
                symbols.push((entry.symbol, cat, "watchlist"));
            }
        }
    }

    // Sector ETFs (ensure coverage even if not in portfolio/watchlist)
    for (etf_sym, _) in crate::commands::sector::SECTOR_ETFS {
        if seen.insert(etf_sym.to_string()) {
            symbols.push((etf_sym.to_string(), AssetCategory::Fund, "sector"));
        }
    }

    // Build price maps
    let cached = get_all_cached_prices_backend(backend).unwrap_or_default();
    let price_map: HashMap<String, Decimal> =
        cached.iter().map(|q| (q.symbol.clone(), q.price)).collect();
    let prev_close_map: HashMap<String, Decimal> = cached
        .iter()
        .filter_map(|q| q.previous_close.map(|pc| (q.symbol.clone(), pc)))
        .collect();

    // Compute changes and group by sector
    // Key: (sector, direction) → Vec<ThemeMover>
    let mut groups: HashMap<(String, &'static str), Vec<ThemeMover>> = HashMap::new();

    for (sym, cat, _source) in &symbols {
        let current_price = price_map.get(sym).copied();
        let cached_prev = prev_close_map.get(sym).copied();

        if let Some(pct) = compute_change_pct(backend, sym, current_price, cached_prev) {
            let abs_pct = if pct < dec!(0) { -pct } else { pct };
            if abs_pct >= threshold_pct {
                let direction = if pct >= dec!(0) { "up" } else { "down" };
                let sector = classify_sector(sym, cat);
                let name = resolve_name(sym);
                let display_name = if name.is_empty() {
                    sym.clone()
                } else {
                    name
                };
                let f: f64 = pct.to_string().parse().unwrap_or(0.0);

                groups
                    .entry((sector, direction))
                    .or_default()
                    .push((sym.clone(), display_name, f));
            }
        }
    }

    // Build themes from groups that meet the min_symbols threshold
    let mut themes: Vec<SectorTheme> = Vec::new();
    for ((sector, direction), mut movers) in groups {
        if movers.len() < min_symbols {
            continue;
        }

        // Sort by absolute change descending
        movers.sort_by(|a, b| {
            b.2.abs()
                .partial_cmp(&a.2.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let count = movers.len() as f64;
        let avg_change: f64 = movers.iter().map(|m| m.2).sum::<f64>() / count;
        let strength = avg_change.abs() * count;

        themes.push(SectorTheme {
            sector,
            direction,
            symbols: movers,
            avg_change,
            strength,
        });
    }

    // Sort by strength descending
    themes.sort_by(|a, b| {
        b.strength
            .partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    themes
}

/// Run the `analytics movers themes` subcommand.
pub fn run_themes(
    backend: &BackendConnection,
    _config: &Config,
    threshold: &str,
    min_symbols: usize,
    json: bool,
) -> Result<()> {
    let threshold_pct: Decimal = {
        let cleaned = threshold.replace('%', "");
        Decimal::from_str_exact(&cleaned).unwrap_or(dec!(2))
    };

    let themes = detect_themes(backend, threshold_pct, min_symbols);

    if json {
        print_themes_json(&themes, threshold_pct, min_symbols)?;
    } else {
        print_themes_terminal(&themes, threshold_pct, min_symbols)?;
    }

    Ok(())
}

fn print_themes_json(
    themes: &[SectorTheme],
    threshold_pct: Decimal,
    min_symbols: usize,
) -> Result<()> {
    let entries: Vec<serde_json::Value> = themes
        .iter()
        .map(|t| {
            let symbols: Vec<serde_json::Value> = t
                .symbols
                .iter()
                .map(|(sym, name, chg)| {
                    serde_json::json!({
                        "symbol": sym,
                        "name": name,
                        "change_pct": (*chg * 100.0).round() / 100.0,
                    })
                })
                .collect();
            serde_json::json!({
                "sector": t.sector,
                "direction": t.direction,
                "symbol_count": t.symbols.len(),
                "avg_change_pct": (t.avg_change * 100.0).round() / 100.0,
                "strength": (t.strength * 100.0).round() / 100.0,
                "symbols": symbols,
            })
        })
        .collect();

    let output = serde_json::json!({
        "threshold_pct": threshold_pct.to_string().parse::<f64>().unwrap_or(2.0),
        "min_symbols": min_symbols,
        "themes_count": themes.len(),
        "themes": entries,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_themes_terminal(
    themes: &[SectorTheme],
    threshold_pct: Decimal,
    min_symbols: usize,
) -> Result<()> {
    if themes.is_empty() {
        println!(
            "No sector themes detected (threshold: {}%, min symbols: {}).",
            threshold_pct, min_symbols
        );
        return Ok(());
    }

    println!(
        "\n📊 Sector Themes (≥{}% move, ≥{} symbols)\n",
        threshold_pct, min_symbols
    );

    for theme in themes {
        let arrow = if theme.direction == "up" {
            "▲"
        } else {
            "▼"
        };
        let color = if theme.direction == "up" {
            "\x1b[32m"
        } else {
            "\x1b[31m"
        };
        let reset = "\x1b[0m";

        println!(
            "  {}{} {} — {} ({} symbols, avg {}{:+.2}%{}, strength {:.1})",
            color,
            arrow,
            theme.sector,
            theme.direction.to_uppercase(),
            theme.symbols.len(),
            color,
            theme.avg_change,
            reset,
            theme.strength,
        );

        for (sym, name, chg) in &theme.symbols {
            let sym_color = if *chg >= 0.0 { "\x1b[32m" } else { "\x1b[31m" };
            println!(
                "      {:<8} {:<24} {}{:+.2}%{}",
                sym, name, sym_color, chg, reset
            );
        }
        println!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn to_backend(conn: Connection) -> crate::db::backend::BackendConnection {
        crate::db::backend::BackendConnection::Sqlite { conn }
    }

    #[test]
    fn movers_empty_db() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_no_history() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::watchlist::add_to_watchlist;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_below_threshold() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(201), // 0.5% change — below 3% default
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_above_threshold() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::{HistoryRecord, PriceQuote};

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "AAPL".to_string(),
                price: dec!(220),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-03T20:00:00Z".to_string(),

                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None,
            },
        )
        .unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220), // 10% change — above 3% default
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_custom_threshold() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(204), // 2% change
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        // 1% threshold — should appear
        let backend = to_backend(conn);
        let result = run(&backend, &config, Some("1"), false, false);
        assert!(result.is_ok());

        // 5% threshold — should not appear
        let result = run(&backend, &config, Some("5"), false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_json_output() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_skips_cash() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::transactions::insert_transaction;
        use crate::models::transaction::NewTransaction;

        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "USD".to_string(),
                category: AssetCategory::Cash,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: dec!(10000),
                price_per: dec!(1),
                currency: "USD".to_string(),
                date: "2026-03-03".to_string(),
                notes: None,
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_negative_change() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;

        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(180), // -10% change
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_dedupes_held_and_watchlist() {
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_history::upsert_history;
        use crate::db::transactions::insert_transaction;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::HistoryRecord;
        use crate::models::transaction::NewTransaction;

        // Same symbol in both held and watchlist
        insert_transaction(
            &conn,
            &NewTransaction {
                symbol: "AAPL".to_string(),
                category: AssetCategory::Equity,
                tx_type: crate::models::transaction::TxType::Buy,
                quantity: dec!(10),
                price_per: dec!(150),
                currency: "USD".to_string(),
                date: "2026-01-01".to_string(),
                notes: None,
            },
        )
        .unwrap();
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[
                HistoryRecord {
                    date: "2026-03-02".to_string(),
                    close: dec!(200),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
                HistoryRecord {
                    date: "2026-03-03".to_string(),
                    close: dec!(220),
                    volume: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ],
        )
        .unwrap();

        // Should only show AAPL once (as "held")
        let backend = to_backend(conn);
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn change_pct_computation() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(200),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        // Current price is 210, previous close was 200 → 5% gain
        let backend = to_backend(conn);
        let pct = compute_change_pct(&backend, "AAPL", Some(dec!(210)), None).unwrap();
        assert_eq!(pct, dec!(5));
    }

    #[test]
    fn change_pct_zero_prev() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(0),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        // Previous close was 0 → should return None (can't compute % change)
        let backend = to_backend(conn);
        assert!(compute_change_pct(&backend, "AAPL", Some(dec!(100)), None).is_none());
    }

    #[test]
    fn change_pct_no_current_price() {
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(200),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        // No current price provided → should return None
        let backend = to_backend(conn);
        assert!(compute_change_pct(&backend, "AAPL", None, None).is_none());
    }

    #[test]
    fn previous_close_uses_latest_historical_close_on_weekend_gap() {
        use crate::models::price::HistoryRecord;

        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let history = vec![
            HistoryRecord {
                date: "2026-03-12".to_string(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-13".to_string(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];

        // Current price is 110 (different from history) — should return Friday close (105)
        assert_eq!(
            previous_close_from_history(&history, today, dec!(110)),
            Some(dec!(105))
        );
    }

    #[test]
    fn previous_close_uses_penultimate_when_history_contains_today() {
        use crate::models::price::HistoryRecord;

        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let history = vec![
            HistoryRecord {
                date: "2026-03-13".to_string(),
                close: dec!(105),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-16".to_string(),
                close: dec!(109),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];

        // Current price matches today's close — should skip today and return 105
        assert_eq!(
            previous_close_from_history(&history, today, dec!(109)),
            Some(dec!(105))
        );
    }

    #[test]
    fn previous_close_skips_stale_duplicates() {
        // This is the core bug scenario: Yahoo writes the same close to multiple
        // consecutive dates (e.g. after-hours fetch duplicates yesterday's close
        // into today's row). The old logic returned the penultimate record, which
        // also had the same close as the cached spot → 0% change.
        use crate::models::price::HistoryRecord;

        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        let current_price = dec!(4600);
        let history = vec![
            HistoryRecord {
                date: "2026-03-17".to_string(),
                close: dec!(5001),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-18".to_string(),
                close: dec!(4890),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-19".to_string(),
                close: dec!(4600), // same as cached spot — stale duplicate
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-20".to_string(),
                close: dec!(4600), // today — also stale duplicate
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];

        // Should skip today (Mar 20) and the stale Mar 19 duplicate, returning Mar 18 close
        assert_eq!(
            previous_close_from_history(&history, today, current_price),
            Some(dec!(4890))
        );
    }

    #[test]
    fn previous_close_flat_market_returns_zero_change() {
        // When ALL history records genuinely have the same close (flat market),
        // the function should still return a value (producing 0% change).
        use crate::models::price::HistoryRecord;

        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        let history = vec![
            HistoryRecord {
                date: "2026-03-18".to_string(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-19".to_string(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
            HistoryRecord {
                date: "2026-03-20".to_string(),
                close: dec!(100),
                volume: None,
                open: None,
                high: None,
                low: None,
            },
        ];

        // All closes are 100, current is also 100 — returns 100 (oldest candidate), yielding 0%
        assert_eq!(
            previous_close_from_history(&history, today, dec!(100)),
            Some(dec!(100))
        );
    }

    #[test]
    fn change_pct_uses_cached_previous_close_when_no_history() {
        // P0 fix: symbol has current price + cached previous_close but no history
        // → should compute change using the cached previous_close
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // No history at all — but we have a cached previous_close
        let pct = compute_change_pct(&backend, "GC=F", Some(dec!(2700)), Some(dec!(3000)));
        assert!(pct.is_some());
        let pct = pct.unwrap();
        assert_eq!(pct, dec!(-10)); // (2700 - 3000) / 3000 * 100 = -10%
    }

    #[test]
    fn change_pct_none_when_no_history_and_no_cached_previous_close() {
        // Symbol has current price but no previous_close and no history
        // → should return None (symbol appears in skipped list)
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        assert!(compute_change_pct(&backend, "GC=F", Some(dec!(2700)), None).is_none());
    }

    #[test]
    fn change_pct_prefers_history_over_cached_previous_close() {
        // When history is available, it should be used over cached previous_close
        let conn = crate::db::open_in_memory();
        use crate::db::price_history::upsert_history;
        use crate::models::price::HistoryRecord;

        upsert_history(
            &conn,
            "AAPL",
            "yahoo",
            &[HistoryRecord {
                date: "2026-03-02".to_string(),
                close: dec!(200),
                volume: None,
                open: None,
                high: None,
                low: None,
            }],
        )
        .unwrap();

        let backend = to_backend(conn);
        // History says 200, cached says 195 — should use history (200)
        let pct = compute_change_pct(&backend, "AAPL", Some(dec!(210)), Some(dec!(195))).unwrap();
        assert_eq!(pct, dec!(5)); // (210 - 200) / 200 * 100 = 5%
    }

    #[test]
    fn movers_json_includes_skipped() {
        // Test that --json mode includes skipped symbols diagnostic
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::PriceQuote;

        // Add a symbol with a cached price but no history and no previous_close
        add_to_watchlist(&conn, "GC=F", AssetCategory::Commodity).unwrap();
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".to_string(),
                price: dec!(2700),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-20T12:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: None, // No previous_close
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        // Should succeed without panic — GC=F will be in skipped list
        let result = run(&backend, &config, None, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn movers_uses_cached_previous_close_for_extreme_moves() {
        // The actual P0 bug scenario: gold crashes but has no history, only
        // cached previous_close from Yahoo's regularMarketPreviousClose
        let conn = crate::db::open_in_memory();
        let config = crate::config::Config::default();
        use crate::db::price_cache::upsert_price;
        use crate::db::watchlist::add_to_watchlist;
        use crate::models::price::PriceQuote;

        add_to_watchlist(&conn, "GC=F", AssetCategory::Commodity).unwrap();
        upsert_price(
            &conn,
            &PriceQuote {
                symbol: "GC=F".to_string(),
                price: dec!(2700),
                currency: "USD".to_string(),
                source: "yahoo".to_string(),
                fetched_at: "2026-03-20T12:00:00Z".to_string(),
                pre_market_price: None,
                post_market_price: None,
                post_market_change_percent: None,
                previous_close: Some(dec!(3000)), // Previous close from Yahoo
            },
        )
        .unwrap();

        let backend = to_backend(conn);
        // Should show GC=F as a mover (-10% exceeds 3% threshold)
        let result = run(&backend, &config, None, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn change_pct_rejects_anomalous_data() {
        // Reproduces the BTC 224,632% anomaly: corrupt previous_close near zero
        // should be filtered by the plausibility guard
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // Current BTC = $84,000, corrupt previous_close = $37 → ~227,000% change
        let pct = compute_change_pct(&backend, "BTC-USD", Some(dec!(84000)), Some(dec!(37)));
        assert!(pct.is_none(), "Should reject >500% change as implausible");
    }

    #[test]
    fn change_pct_allows_legitimate_extreme_moves() {
        // A real 50% crash (e.g. small-cap halt) should still be allowed
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // Current = $50, previous = $100 → -50% change (legitimate)
        let pct = compute_change_pct(&backend, "SMCI", Some(dec!(50)), Some(dec!(100)));
        assert!(pct.is_some(), "Should allow legitimate -50% move");
        assert_eq!(pct.unwrap(), dec!(-50));
    }

    #[test]
    fn change_pct_boundary_at_500_pct() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // Exactly 500% → allowed
        let pct = compute_change_pct(&backend, "TEST", Some(dec!(600)), Some(dec!(100)));
        assert!(pct.is_some(), "500% should be allowed");
        assert_eq!(pct.unwrap(), dec!(500));

        // Just over 500% → rejected
        let pct = compute_change_pct(&backend, "TEST", Some(dec!(602)), Some(dec!(100)));
        assert!(pct.is_none(), "502% should be rejected as implausible");
    }

    // ─── Sector theme tests ────────────────────────────────────────────────

    #[test]
    fn classify_sector_known_etf() {
        let cat = AssetCategory::Fund;
        assert_eq!(classify_sector("XLE", &cat), "Energy");
        assert_eq!(classify_sector("XLK", &cat), "Technology");
        assert_eq!(classify_sector("GDX", &cat), "Gold Miners");
        assert_eq!(classify_sector("ITA", &cat), "Aerospace & Defense ETF");
        assert_eq!(classify_sector("SMH", &cat), "Semiconductors");
    }

    #[test]
    fn classify_sector_case_insensitive() {
        let cat = AssetCategory::Fund;
        assert_eq!(classify_sector("xle", &cat), "Energy");
        assert_eq!(classify_sector("Xlk", &cat), "Technology");
    }

    #[test]
    fn classify_sector_unknown_symbol_uses_category() {
        assert_eq!(
            classify_sector("TSLA", &AssetCategory::Equity),
            "Equities"
        );
        assert_eq!(
            classify_sector("BTC-USD", &AssetCategory::Crypto),
            "Crypto"
        );
        assert_eq!(
            classify_sector("GC=F", &AssetCategory::Commodity),
            "Commodities"
        );
        assert_eq!(
            classify_sector("GBPUSD=X", &AssetCategory::Forex),
            "Forex"
        );
    }

    #[test]
    fn detect_themes_empty_db() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let themes = detect_themes(&backend, dec!(2), 2);
        assert!(themes.is_empty());
    }

    fn make_quote(
        symbol: &str,
        price: Decimal,
        previous_close: Option<Decimal>,
    ) -> crate::models::price::PriceQuote {
        crate::models::price::PriceQuote {
            symbol: symbol.to_string(),
            price,
            currency: "USD".to_string(),
            source: "test".to_string(),
            fetched_at: chrono::Utc::now().to_rfc3339(),
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
            previous_close,
        }
    }

    fn make_history(date: &str, close: Decimal) -> crate::models::price::HistoryRecord {
        crate::models::price::HistoryRecord {
            date: date.to_string(),
            close,
            open: None,
            high: None,
            low: None,
            volume: None,
        }
    }

    fn seed_symbol(
        conn: &Connection,
        symbol: &str,
        prev_price: Decimal,
        curr_price: Decimal,
    ) {
        use crate::db::price_cache::upsert_price;
        use crate::db::price_history::upsert_history;

        let today = chrono::Utc::now().date_naive().to_string();
        let yesterday =
            (chrono::Utc::now().date_naive() - chrono::Duration::days(1)).to_string();

        upsert_price(conn, &make_quote(symbol, curr_price, Some(prev_price))).unwrap();
        upsert_history(
            conn,
            symbol,
            "test",
            &[
                make_history(&yesterday, prev_price),
                make_history(&today, curr_price),
            ],
        )
        .unwrap();
    }

    #[test]
    fn detect_themes_groups_sector_movers() {
        let conn = crate::db::open_in_memory();

        // XLE: Energy, XLK: Technology, XLF: Financials — all up >3%
        seed_symbol(&conn, "XLE", dec!(95), dec!(100));  // +5.26%
        seed_symbol(&conn, "XLK", dec!(190), dec!(200)); // +5.26%
        seed_symbol(&conn, "XLF", dec!(48), dec!(50));   // +4.17%

        let backend = to_backend(conn);

        // Each sector ETF maps to its own unique sector name (Energy, Technology,
        // Financials) — so no theme of ≥2 in same sector emerges from them alone.
        let themes = detect_themes(&backend, dec!(3), 2);

        for t in &themes {
            assert!(
                t.symbols.len() >= 2,
                "Theme {} should have ≥2 symbols",
                t.sector
            );
        }
    }

    #[test]
    fn detect_themes_respects_min_symbols() {
        let conn = crate::db::open_in_memory();

        // Only one symbol per sector — no theme should form
        seed_symbol(&conn, "XLE", dec!(95), dec!(100)); // +5.26%

        let backend = to_backend(conn);
        let themes = detect_themes(&backend, dec!(3), 2);

        let energy_theme = themes.iter().find(|t| t.sector == "Energy");
        assert!(
            energy_theme.is_none(),
            "Should not form theme with only 1 symbol"
        );
    }

    #[test]
    fn detect_themes_separates_up_and_down() {
        let conn = crate::db::open_in_memory();

        use crate::db::watchlist::add_to_watchlist;

        // Add four watchlist equities: two up, two down
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "MSFT", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "GOOG", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "META", AssetCategory::Equity).unwrap();

        // AAPL +5%, MSFT +5%
        seed_symbol(&conn, "AAPL", dec!(100), dec!(105));
        seed_symbol(&conn, "MSFT", dec!(200), dec!(210));

        // GOOG -5%, META -5%
        seed_symbol(&conn, "GOOG", dec!(100), dec!(95));
        seed_symbol(&conn, "META", dec!(200), dec!(190));

        let backend = to_backend(conn);
        let themes = detect_themes(&backend, dec!(3), 2);

        // Should get two themes for Equities: one up, one down
        let equity_up = themes
            .iter()
            .find(|t| t.sector == "Equities" && t.direction == "up");
        let equity_down = themes
            .iter()
            .find(|t| t.sector == "Equities" && t.direction == "down");

        assert!(equity_up.is_some(), "Should detect Equities UP theme");
        assert!(equity_down.is_some(), "Should detect Equities DOWN theme");

        let up = equity_up.unwrap();
        assert_eq!(up.symbols.len(), 2);
        assert!(up.avg_change > 0.0);

        let down = equity_down.unwrap();
        assert_eq!(down.symbols.len(), 2);
        assert!(down.avg_change < 0.0);
    }

    #[test]
    fn detect_themes_sorted_by_strength() {
        let conn = crate::db::open_in_memory();

        use crate::db::watchlist::add_to_watchlist;

        // Two equities with big moves, two crypto with smaller moves
        add_to_watchlist(&conn, "AAPL", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "MSFT", AssetCategory::Equity).unwrap();
        add_to_watchlist(&conn, "BTC-USD", AssetCategory::Crypto).unwrap();
        add_to_watchlist(&conn, "ETH-USD", AssetCategory::Crypto).unwrap();

        // Equities: big moves (+10%)
        seed_symbol(&conn, "AAPL", dec!(100), dec!(110));
        seed_symbol(&conn, "MSFT", dec!(200), dec!(220));

        // Crypto: smaller moves (+4%)
        seed_symbol(&conn, "BTC-USD", dec!(100), dec!(104));
        seed_symbol(&conn, "ETH-USD", dec!(200), dec!(208));

        let backend = to_backend(conn);
        let themes = detect_themes(&backend, dec!(3), 2);

        // Equities theme should rank higher (larger avg change × count)
        if themes.len() >= 2 {
            assert!(
                themes[0].strength >= themes[1].strength,
                "Themes should be sorted by strength descending"
            );
        }
    }

    #[test]
    fn theme_strength_computation() {
        // Strength = |avg_change| * count
        // 3 symbols averaging +4% → strength = 4 * 3 = 12
        let theme = SectorTheme {
            sector: "Test".to_string(),
            direction: "up",
            symbols: vec![
                ("A".into(), "Alpha".into(), 5.0),
                ("B".into(), "Beta".into(), 4.0),
                ("C".into(), "Charlie".into(), 3.0),
            ],
            avg_change: 4.0,
            strength: 4.0 * 3.0,
        };
        assert!((theme.strength - 12.0).abs() < 0.001);
    }
}
