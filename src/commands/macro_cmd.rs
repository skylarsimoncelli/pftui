//! `pftui macro` — Terminal-friendly macro dashboard output.
//!
//! Displays key macroeconomic indicators from cached prices (Yahoo Finance)
//! and FRED economic data. Supports `--json` for structured agent consumption.

use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::config::Config;
use crate::data::fred;
use crate::db::economic_cache;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::{get_all_cached_prices_backend, upsert_price_backend};
use crate::db::price_history::get_history_backend;
use crate::indicators::{compute_macd, compute_rsi, compute_sma};
use crate::price::yahoo;

const MARKET_INDICATORS: &[(&str, &str, &str)] = &[
    ("dxy", "DX-Y.NYB", "index"),
    ("vix", "^VIX", "index"),
    ("yield_2y", "^IRX", "%"),
    ("yield_5y", "^FVX", "%"),
    ("yield_10y", "^TNX", "%"),
    ("yield_30y", "^TYX", "%"),
    ("gold", "GC=F", "USD"),
    ("silver", "SI=F", "USD"),
    ("oil_wti", "CL=F", "USD"),
    ("oil_brent", "BZ=F", "USD"),
    ("copper", "HG=F", "USD"),
    ("nat_gas", "NG=F", "USD"),
    ("wheat", "ZW=F", "USD"),
    ("corn", "ZC=F", "USD"),
    ("soybeans", "ZS=F", "USD"),
    ("coffee", "KC=F", "USD"),
    ("eur_usd", "EURUSD=X", "fx"),
    ("gbp_usd", "GBPUSD=X", "fx"),
    ("usd_jpy", "JPY=X", "fx"),
    ("usd_cny", "CNY=X", "fx"),
];

/// Technical indicators for a macro instrument.
#[derive(Debug, Clone)]
struct Technicals {
    rsi: Option<f64>,
    macd: Option<f64>,
    macd_signal: Option<f64>,
    macd_histogram: Option<f64>,
    sma50: Option<f64>,
}

impl Technicals {
    fn none() -> Self {
        Self {
            rsi: None,
            macd: None,
            macd_signal: None,
            macd_histogram: None,
            sma50: None,
        }
    }
}

/// Compute technicals (RSI, MACD, SMA50) for a symbol.
/// Requires ~90 days of history for SMA50, ~50 for MACD/RSI.
fn compute_technicals(backend: &BackendConnection, symbol: &str) -> Technicals {
    let history = match get_history_backend(backend, symbol, 100) {
        Ok(h) if h.len() >= 30 => h,
        _ => return Technicals::none(),
    };

    let closes: Vec<f64> = history
        .iter()
        .map(|rec| rec.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let rsi_vec = compute_rsi(&closes, 14);
    let macd_vec = compute_macd(&closes, 12, 26, 9);
    let sma50_vec = compute_sma(&closes, 50);

    let rsi = rsi_vec.last().and_then(|x| *x);
    let macd_res = macd_vec.last().and_then(|x| *x);
    let sma50 = sma50_vec.last().and_then(|x| *x);

    Technicals {
        rsi,
        macd: macd_res.map(|m| m.macd),
        macd_signal: macd_res.map(|m| m.signal),
        macd_histogram: macd_res.map(|m| m.histogram),
        sma50,
    }
}

fn missing_market_symbols(price_map: &HashMap<String, Decimal>) -> Vec<&'static str> {
    MARKET_INDICATORS
        .iter()
        .map(|(_, symbol, _)| *symbol)
        .filter(|symbol| !price_map.contains_key(*symbol))
        .collect()
}

fn backfill_market_prices(
    backend: &BackendConnection,
    price_map: &mut HashMap<String, Decimal>,
    symbols: &[&str],
    cached_only: bool,
) -> Result<()> {
    if cached_only {
        return Ok(());
    }
    if symbols.is_empty() {
        return Ok(());
    }

    let rt = tokio::runtime::Runtime::new()?;
    for symbol in symbols {
        if let Ok(quote) = rt.block_on(yahoo::fetch_price(symbol)) {
            upsert_price_backend(backend, &quote)?;
            price_map.insert(symbol.to_string(), quote.price);
        }
    }

    Ok(())
}

/// Run the macro dashboard command.
pub fn run(backend: &BackendConnection, _config: &Config, json: bool, cached_only: bool) -> Result<()> {
    // Build price map from cached data (yahoo symbol -> price)
    let all_prices = get_all_cached_prices_backend(backend)?;
    let mut price_map: HashMap<String, Decimal> = all_prices
        .iter()
        .map(|p| (p.symbol.clone(), p.price))
        .collect();

    // Ensure macro dashboard can display the full market basket even when
    // some symbols were not previously fetched during refresh.
    let missing = missing_market_symbols(&price_map);
    backfill_market_prices(backend, &mut price_map, &missing, cached_only)?;

    // Build FRED data map (series_id -> latest observation)
    let fred_data: HashMap<String, (Decimal, String)> = economic_cache::get_all_latest_backend(backend)?
        .into_iter()
        .map(|obs| (obs.series_id.clone(), (obs.value, obs.date)))
        .collect();

    if json {
        print_json(&price_map, &fred_data, backend)?;
    } else {
        print_terminal(&price_map, &fred_data, backend)?;
    }

    Ok(())
}

// ─── JSON output ────────────────────────────────────────────────────────────

fn print_json(
    prices: &HashMap<String, Decimal>,
    fred: &HashMap<String, (Decimal, String)>,
    backend: &BackendConnection,
) -> Result<()> {
    use serde_json::{json, Map, Value};

    let mut macro_obj = Map::new();

    for (key, symbol, unit) in MARKET_INDICATORS {
        if let Some(price) = prices.get(*symbol) {
            let mut entry = Map::new();
            entry.insert("value".into(), json!(price.to_string().parse::<f64>().unwrap_or(0.0)));
            entry.insert("unit".into(), json!(unit));

            // Add technical indicators
            let tech = compute_technicals(backend, symbol);
            if tech.rsi.is_some() || tech.macd.is_some() || tech.sma50.is_some() {
                let mut tech_obj = Map::new();
                if let Some(rsi) = tech.rsi {
                    tech_obj.insert("rsi".into(), json!(rsi));
                }
                if let Some(macd) = tech.macd {
                    tech_obj.insert("macd".into(), json!(macd));
                }
                if let Some(sig) = tech.macd_signal {
                    tech_obj.insert("macd_signal".into(), json!(sig));
                }
                if let Some(hist) = tech.macd_histogram {
                    tech_obj.insert("macd_histogram".into(), json!(hist));
                }
                if let Some(sma) = tech.sma50 {
                    tech_obj.insert("sma50".into(), json!(sma));
                }
                entry.insert("technicals".into(), Value::Object(tech_obj));
            }

            macro_obj.insert(key.to_string(), Value::Object(entry));
        }
    }

    // FRED indicators
    let fred_indicators: &[(&str, &str)] = &[
        ("fed_funds", "FEDFUNDS"),
        ("cpi", "CPIAUCSL"),
        ("ppi", "PPIACO"),
        ("unemployment", "UNRATE"),
        ("yield_spread_10y2y", "T10Y2Y"),
    ];

    for (key, series_id) in fred_indicators {
        if let Some((value, date)) = fred.get(*series_id) {
            let mut entry = Map::new();
            entry.insert("value".into(), json!(value.to_string().parse::<f64>().unwrap_or(0.0)));
            entry.insert("date".into(), json!(date));
            if let Some(meta) = fred::series_by_id(series_id) {
                entry.insert("unit".into(), json!(meta.unit));
                entry.insert("name".into(), json!(meta.name));
            }
            macro_obj.insert(key.to_string(), Value::Object(entry));
        }
    }

    // Derived metrics
    let mut derived = Map::new();

    // Gold/silver ratio
    if let (Some(gold), Some(silver)) = (prices.get("GC=F"), prices.get("SI=F")) {
        if *silver > dec!(0) {
            let ratio = gold
                .checked_div(*silver)
                .unwrap_or(Decimal::ZERO);
            derived.insert("gold_silver_ratio".into(), json!({
                "value": ratio.round_dp(1).to_string().parse::<f64>().unwrap_or(0.0),
                "context": if ratio > dec!(80) { "gold_strong" } else if ratio < dec!(60) { "silver_strong" } else { "normal" }
            }));
        }
    }

    // Gold/oil ratio
    if let (Some(gold), Some(oil)) = (prices.get("GC=F"), prices.get("CL=F")) {
        if *oil > dec!(0) {
            let ratio = gold.checked_div(*oil).unwrap_or(Decimal::ZERO);
            derived.insert("gold_oil_ratio".into(), json!({
                "value": ratio.round_dp(1).to_string().parse::<f64>().unwrap_or(0.0),
                "context": if ratio > dec!(25) { "risk_off" } else if ratio < dec!(15) { "expansion" } else { "balanced" }
            }));
        }
    }

    // Copper/gold ratio (scaled ×1000)
    if let (Some(copper), Some(gold)) = (prices.get("HG=F"), prices.get("GC=F")) {
        if *gold > dec!(0) {
            let ratio = copper
                .checked_div(*gold)
                .unwrap_or(Decimal::ZERO)
                * dec!(1000);
            derived.insert("copper_gold_ratio".into(), json!({
                "value": ratio.round_dp(2).to_string().parse::<f64>().unwrap_or(0.0),
                "context": if ratio > dec!(2) { "growth" } else if ratio < dec!(1.2) { "caution" } else { "steady" }
            }));
        }
    }

    // Yield curve spread from market data
    if let (Some(y10), Some(y2)) = (prices.get("^TNX"), prices.get("^IRX")) {
        let spread = *y10 - *y2;
        let spread_bps = (spread * dec!(100)).round_dp(0);
        derived.insert("yield_curve".into(), json!({
            "spread_bps": spread_bps.to_string().parse::<f64>().unwrap_or(0.0),
            "status": if spread > dec!(0.05) { "normal" } else if spread < dec!(-0.05) { "inverted" } else { "flat" }
        }));
    }

    // VIX context
    if let Some(vix) = prices.get("^VIX") {
        let v = vix.to_string().parse::<f64>().unwrap_or(0.0);
        derived.insert("vix_regime".into(), json!({
            "value": v,
            "regime": if v > 30.0 { "high_fear" } else if v > 20.0 { "elevated" } else if v > 12.0 { "normal" } else { "complacent" }
        }));
    }

    // WTI-Brent spread
    if let (Some(wti), Some(brent)) = (prices.get("CL=F"), prices.get("BZ=F")) {
        let spread = *wti - *brent;
        let context = if spread > dec!(5) {
            "wti_premium"
        } else if spread < dec!(-5) {
            "brent_premium"
        } else {
            "converged"
        };
        derived.insert("wti_brent_spread".into(), json!({
            "value": spread.round_dp(2).to_string().parse::<f64>().unwrap_or(0.0),
            "context": context
        }));
    }

    if !derived.is_empty() {
        macro_obj.insert("derived".into(), Value::Object(derived));
    }

    let output = serde_json::to_string_pretty(&Value::Object(macro_obj))?;
    println!("{}", output);
    Ok(())
}

// ─── Terminal output ────────────────────────────────────────────────────────

fn print_terminal(
    prices: &HashMap<String, Decimal>,
    fred: &HashMap<String, (Decimal, String)>,
    backend: &BackendConnection,
) -> Result<()> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    MACRO DASHBOARD                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // ── Key Indicators Strip ──
    print_key_strip(prices, fred);
    println!();

    // ── Yields ──
    println!("── Yields ─────────────────────────────────────────────────────");
    let yields: &[(&str, &str, &str)] = &[
        ("2Y Treasury", "^IRX", "%"),
        ("5Y Treasury", "^FVX", "%"),
        ("10Y Treasury", "^TNX", "%"),
        ("30Y Treasury", "^TYX", "%"),
    ];
    for (name, symbol, unit) in yields {
        print_indicator_row(name, symbol, unit, prices, backend);
    }

    // FRED yields data
    if let Some((val, date)) = fred.get("T10Y2Y") {
        let spread_bps = (val * dec!(100)).round_dp(0);
        println!(
            "  {:<22} {:>10}bps  (FRED, {})",
            "10Y-2Y Spread",
            spread_bps,
            date
        );
    }
    if let Some((val, date)) = fred.get("DGS10") {
        println!(
            "  {:<22} {:>10}%    (FRED, {})",
            "10Y Yield (FRED)",
            val.round_dp(2),
            date
        );
    }
    println!();

    // ── Currency ──
    println!("── Currency ───────────────────────────────────────────────────");
    let currencies: &[(&str, &str, &str)] = &[
        ("US Dollar Index", "DX-Y.NYB", ""),
        ("EUR/USD", "EURUSD=X", ""),
        ("GBP/USD", "GBPUSD=X", ""),
        ("USD/JPY", "JPY=X", ""),
        ("USD/CNY", "CNY=X", ""),
    ];
    for (name, symbol, unit) in currencies {
        print_indicator_row(name, symbol, unit, prices, backend);
    }
    println!();

    // ── Commodities ──
    println!("── Commodities ────────────────────────────────────────────────");
    let commodities: &[(&str, &str, &str)] = &[
        ("Gold", "GC=F", "$"),
        ("Silver", "SI=F", "$"),
        ("Oil (WTI)", "CL=F", "$"),
        ("Oil (Brent)", "BZ=F", "$"),
        ("Copper", "HG=F", "$"),
        ("Natural Gas", "NG=F", "$"),
        ("Wheat", "ZW=F", "$"),
        ("Corn", "ZC=F", "$"),
        ("Soybeans", "ZS=F", "$"),
        ("Coffee", "KC=F", "$"),
    ];
    for (name, symbol, unit) in commodities {
        print_indicator_row(name, symbol, unit, prices, backend);
    }

    // WTI-Brent spread
    if let (Some(wti), Some(brent)) = (prices.get("CL=F"), prices.get("BZ=F")) {
        let spread = *wti - *brent;
        let context = if spread > dec!(5) {
            "🇺🇸 WTI Premium"
        } else if spread < dec!(-5) {
            "🌍 Brent Premium"
        } else {
            "✓ Converged"
        };
        println!(
            "  {:<22} {:>10}  ({})",
            "WTI-Brent Spread",
            format!("${:.2}", spread),
            context
        );
    }
    println!();

    // ── Volatility ──
    println!("── Volatility ─────────────────────────────────────────────────");
    print_indicator_row("VIX", "^VIX", "", prices, backend);
    if let Some(vix) = prices.get("^VIX") {
        let v = vix.to_string().parse::<f64>().unwrap_or(0.0);
        let regime = if v > 30.0 {
            "⚠️  HIGH FEAR"
        } else if v > 20.0 {
            "⚡ Elevated"
        } else if v > 12.0 {
            "✓ Normal"
        } else {
            "😴 Complacent"
        };
        println!("  {:<22} {}", "Regime", regime);
    }
    println!();

    // ── FRED Economic Indicators ──
    if !fred.is_empty() {
        println!("── FRED Economic Data ─────────────────────────────────────────");
        let fred_display: &[(&str, &str, &str)] = &[
            ("Fed Funds Rate", "FEDFUNDS", "%"),
            ("CPI (Index)", "CPIAUCSL", ""),
            ("PPI (Index)", "PPIACO", ""),
            ("Unemployment", "UNRATE", "%"),
        ];
        for (name, series_id, unit) in fred_display {
            if let Some((val, date)) = fred.get(*series_id) {
                println!(
                    "  {:<22} {:>10}{}   ({})",
                    name,
                    val.round_dp(2),
                    unit,
                    date
                );
            }
        }
        println!();
    }

    // ── Derived Metrics ──
    println!("── Derived Metrics ────────────────────────────────────────────");
    print_derived_metrics(prices, fred);

    Ok(())
}

/// Compact key-indicators strip at the top.
fn print_key_strip(
    prices: &HashMap<String, Decimal>,
    fred: &HashMap<String, (Decimal, String)>,
) {
    let mut parts: Vec<String> = Vec::new();

    if let Some(p) = prices.get("DX-Y.NYB") {
        parts.push(format!("DXY {:.2}", p));
    }
    if let Some(p) = prices.get("^VIX") {
        let v = p.to_string().parse::<f64>().unwrap_or(0.0);
        let flag = if v > 25.0 { " ⚠️" } else { "" };
        parts.push(format!("VIX {:.1}{}", p, flag));
    }
    if let Some(p) = prices.get("^TNX") {
        parts.push(format!("10Y {:.2}%", p));
    }
    if let Some((val, _)) = fred.get("FEDFUNDS") {
        parts.push(format!("FFR {:.2}%", val));
    }
    if let Some(p) = prices.get("GC=F") {
        parts.push(format!("Gold ${}", fmt_commas(*p, 0)));
    }
    if let Some(p) = prices.get("CL=F") {
        parts.push(format!("Oil ${:.2}", p));
    }

    if !parts.is_empty() {
        println!("  {}", parts.join(" │ "));
    }
}

/// Print a single indicator row with 1-day change and technical indicators.
fn print_indicator_row(
    name: &str,
    yahoo_symbol: &str,
    unit_prefix: &str,
    prices: &HashMap<String, Decimal>,
    backend: &BackendConnection,
) {
    let Some(price) = prices.get(yahoo_symbol) else {
        println!("  {:<22} {:>10}", name, "---");
        return;
    };

    // Get 1-day change from price history
    let change_str = match get_history_backend(backend, yahoo_symbol, 2) {
        Ok(hist) if hist.len() >= 2 => {
            let prev = hist[hist.len() - 2].close;
            if prev != Decimal::ZERO {
                let change_pct = ((*price - prev) / prev * dec!(100)).round_dp(2);
                
                // Sanity check: reject obviously corrupt data (>100% daily moves)
                if change_pct.abs() > dec!(100) {
                    String::new()
                } else {
                    let arrow = if change_pct > Decimal::ZERO {
                        "↑"
                    } else if change_pct < Decimal::ZERO {
                        "↓"
                    } else {
                        "→"
                    };
                    format!("{} {:.2}%", arrow, change_pct)
                }
            } else {
                String::new()
            }
        }
        _ => String::new(),
    };

    // Format value
    let formatted = if unit_prefix == "$" {
        format!("${}", fmt_commas(*price, 2))
    } else if unit_prefix == "%" {
        format!("{:.3}%", price)
    } else {
        format!("{:.4}", price)
    };

    // Compute technicals
    let tech = compute_technicals(backend, yahoo_symbol);

    // Build technical indicators string
    let mut tech_parts = Vec::new();
    if let Some(rsi) = tech.rsi {
        tech_parts.push(format!("RSI {:.1}", rsi));
    }
    if let Some(macd) = tech.macd {
        if let Some(sig) = tech.macd_signal {
            let cross = if macd > sig { "↑" } else { "↓" };
            tech_parts.push(format!("MACD {:.2}/{:.2} {}", macd, sig, cross));
        }
    }
    if let Some(sma) = tech.sma50 {
        let current = price.to_string().parse::<f64>().unwrap_or(0.0);
        let vs_sma = if current > sma { "above" } else { "below" };
        tech_parts.push(format!("SMA50 {:.2} ({})", sma, vs_sma));
    }

    let tech_str = if !tech_parts.is_empty() {
        format!(" │ {}", tech_parts.join(" │ "))
    } else {
        String::new()
    };

    println!(
        "  {:<22} {:>12}  {}{}",
        name, formatted, change_str, tech_str
    );
}

/// Print derived metrics (ratios, yield curve, etc.)
fn print_derived_metrics(
    prices: &HashMap<String, Decimal>,
    _fred: &HashMap<String, (Decimal, String)>,
) {
    // Gold/Silver ratio
    if let (Some(gold), Some(silver)) = (prices.get("GC=F"), prices.get("SI=F")) {
        if *silver > dec!(0) {
            let ratio = gold.checked_div(*silver).unwrap_or(Decimal::ZERO);
            let context = if ratio > dec!(80) {
                "Gold strong"
            } else if ratio < dec!(60) {
                "Silver strong"
            } else {
                "Normal range"
            };
            println!(
                "  {:<22} {:>10.1}    {}",
                "Au/Ag Ratio",
                ratio.round_dp(1),
                context
            );
        }
    }

    // Gold/Oil ratio
    if let (Some(gold), Some(oil)) = (prices.get("GC=F"), prices.get("CL=F")) {
        if *oil > dec!(0) {
            let ratio = gold.checked_div(*oil).unwrap_or(Decimal::ZERO);
            let context = if ratio > dec!(25) {
                "Risk-off"
            } else if ratio < dec!(15) {
                "Expansion"
            } else {
                "Balanced"
            };
            println!(
                "  {:<22} {:>10.1}    {}",
                "Au/Oil Ratio",
                ratio.round_dp(1),
                context
            );
        }
    }

    // Copper/Gold ratio (×1000)
    if let (Some(copper), Some(gold)) = (prices.get("HG=F"), prices.get("GC=F")) {
        if *gold > dec!(0) {
            let ratio = copper
                .checked_div(*gold)
                .unwrap_or(Decimal::ZERO)
                * dec!(1000);
            let context = if ratio > dec!(2) {
                "Growth"
            } else if ratio < dec!(1.2) {
                "Caution"
            } else {
                "Steady"
            };
            println!(
                "  {:<22} {:>10.2}    {}",
                "Cu/Au Ratio (×1000)",
                ratio.round_dp(2),
                context
            );
        }
    }

    // Yield curve (from market data)
    if let (Some(y10), Some(y2)) = (prices.get("^TNX"), prices.get("^IRX")) {
        let spread = *y10 - *y2;
        let spread_bps = (spread * dec!(100)).round_dp(0);
        let status = if spread > dec!(0.05) {
            "Normal"
        } else if spread < dec!(-0.05) {
            "Inverted"
        } else {
            "Flat"
        };
        println!(
            "  {:<22} {:>8}bps    Yield Curve: {}",
            "10Y-2Y Spread",
            spread_bps,
            status
        );
    }

    println!();
}

/// Format a decimal with commas as thousands separators.
fn fmt_commas(value: Decimal, dp: u32) -> String {
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
        Some(dec) => format!("{}{}{}", sign, formatted_int, dec),
        None => format!("{}{}", sign, formatted_int),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use rusqlite::Connection;
    use rust_decimal_macros::dec;

    fn to_backend(conn: Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    fn seed_prices(conn: &Connection) {
        use crate::db::price_cache::upsert_price;
        use crate::models::price::PriceQuote;

        let quotes = vec![
            ("GC=F", dec!(2950.50)),
            ("SI=F", dec!(33.20)),
            ("CL=F", dec!(68.50)),
            ("HG=F", dec!(4.35)),
            ("NG=F", dec!(3.85)),
            ("^VIX", dec!(22.50)),
            ("^TNX", dec!(4.250)),
            ("^IRX", dec!(4.050)),
            ("^FVX", dec!(4.150)),
            ("^TYX", dec!(4.450)),
            ("DX-Y.NYB", dec!(104.25)),
            ("EURUSD=X", dec!(1.0425)),
            ("GBPUSD=X", dec!(1.2650)),
            ("JPY=X", dec!(149.85)),
            ("CNY=X", dec!(7.2350)),
        ];
        for (sym, price) in quotes {
            upsert_price(
                conn,
                &PriceQuote {
                    symbol: sym.to_string(),
                    price,
                    currency: "USD".to_string(),
                    source: "test".to_string(),
                    fetched_at: "2026-03-04T00:00:00Z".to_string(),
                
            pre_market_price: None,
            post_market_price: None,
            post_market_change_percent: None,
        },
            )
            .unwrap();
        }
    }

    fn seed_fred(conn: &Connection) {
        use crate::db::economic_cache::{upsert_observation, EconomicObservation};

        let observations = vec![
            ("DGS10", "2026-03-03", dec!(4.07)),
            ("FEDFUNDS", "2026-02-01", dec!(4.33)),
            ("CPIAUCSL", "2026-01-01", dec!(315.5)),
            ("PPIACO", "2026-01-01", dec!(152.17)),
            ("UNRATE", "2026-01-01", dec!(4.0)),
            ("T10Y2Y", "2026-03-03", dec!(0.20)),
        ];
        for (series, date, value) in observations {
            upsert_observation(
                conn,
                &EconomicObservation {
                    series_id: series.to_string(),
                    date: date.to_string(),
                    value,
                    fetched_at: "2026-03-04T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn test_run_terminal_output_no_panic() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let config = Config::default();
        // Empty DB — should print gracefully with "---" placeholders
        assert!(run(&backend, &config, false, true).is_ok());
    }

    #[test]
    fn test_run_json_output_no_panic() {
        let conn = open_in_memory();
        let backend = to_backend(conn);
        let config = Config::default();
        assert!(run(&backend, &config, true, true).is_ok());
    }

    #[test]
    fn test_run_terminal_with_data() {
        let conn = open_in_memory();
        seed_prices(&conn);
        seed_fred(&conn);
        let backend = to_backend(conn);
        let config = Config::default();
        assert!(run(&backend, &config, false, true).is_ok());
    }

    #[test]
    fn test_run_json_with_data() {
        let conn = open_in_memory();
        seed_prices(&conn);
        seed_fred(&conn);
        let backend = to_backend(conn);
        let config = Config::default();
        assert!(run(&backend, &config, true, true).is_ok());
    }

    #[test]
    fn test_fmt_commas() {
        assert_eq!(fmt_commas(dec!(1234567.89), 2), "1,234,567.89");
        assert_eq!(fmt_commas(dec!(999), 0), "999");
        assert_eq!(fmt_commas(dec!(1000), 0), "1,000");
        assert_eq!(fmt_commas(dec!(-1234.5), 2), "-1,234.50");
        assert_eq!(fmt_commas(dec!(0), 2), "0.00");
    }

    #[test]
    fn test_derived_gold_silver_ratio() {
        let mut prices = HashMap::new();
        prices.insert("GC=F".to_string(), dec!(2950));
        prices.insert("SI=F".to_string(), dec!(33));
        // ratio = 2950/33 ≈ 89.4 → "Gold strong"

        let fred = HashMap::new();
        // Just verify it doesn't panic
        print_derived_metrics(&prices, &fred);
    }

    #[test]
    fn test_derived_zero_denominator() {
        let mut prices = HashMap::new();
        prices.insert("GC=F".to_string(), dec!(2950));
        prices.insert("SI=F".to_string(), dec!(0));
        prices.insert("CL=F".to_string(), dec!(0));
        prices.insert("HG=F".to_string(), dec!(4));

        let fred = HashMap::new();
        // Should not divide by zero or panic
        print_derived_metrics(&prices, &fred);
    }

    #[test]
    fn test_key_strip_partial_data() {
        let mut prices = HashMap::new();
        prices.insert("DX-Y.NYB".to_string(), dec!(104.25));
        // Missing VIX, 10Y, Gold, Oil — should print what's available
        let fred = HashMap::new();
        print_key_strip(&prices, &fred);
    }

    #[test]
    fn market_indicators_include_agricultural_futures() {
        let symbols: Vec<&str> = MARKET_INDICATORS
            .iter()
            .map(|(_, symbol, _)| *symbol)
            .collect();
        assert!(symbols.contains(&"ZW=F"));
        assert!(symbols.contains(&"ZC=F"));
        assert!(symbols.contains(&"ZS=F"));
        assert!(symbols.contains(&"KC=F"));
    }
}
