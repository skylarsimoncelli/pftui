//! `pftui sector` — Sector ETF performance tracker.
//!
//! Displays performance of major sector ETFs (XLE, XLF, XLK, etc.) with
//! current prices, daily change, and RSI/MACD indicators. Useful for
//! gauging sector rotation and relative strength.

use std::collections::HashMap;

use anyhow::Result;
use rust_decimal::Decimal;

use crate::config::Config;
use crate::db::backend::BackendConnection;
use crate::db::price_cache::{get_all_cached_prices_backend, upsert_price_backend};
use crate::db::price_history::get_history_backend;
use crate::indicators::{compute_macd, compute_rsi};
use crate::price::yahoo;

/// Sector and defense universe definitions (symbol, name).
pub(crate) const SECTOR_ETFS: &[(&str, &str)] = &[
    ("XLE", "Energy"),
    ("XLF", "Financials"),
    ("XLK", "Technology"),
    ("XLV", "Healthcare"),
    ("XLY", "Consumer Discretionary"),
    ("XLP", "Consumer Staples"),
    ("XLI", "Industrials"),
    ("XLU", "Utilities"),
    ("XLB", "Materials"),
    ("XLRE", "Real Estate"),
    ("XLC", "Communications"),
    ("IGV", "Software & Services"),
    ("SMH", "Semiconductors"),
    ("XBI", "Biotech"),
    ("XRT", "Retail"),
    ("XHB", "Homebuilders"),
    ("ITB", "Building Materials"),
    ("GDX", "Gold Miners"),
    // Defense tracking (feedback-driven)
    ("ITA", "Aerospace & Defense ETF"),
    ("LMT", "Lockheed Martin"),
    ("RTX", "RTX Corp"),
    ("PLTR", "Palantir"),
];

/// Technical indicators for a sector ETF.
#[derive(Debug, Clone)]
struct Technicals {
    rsi: Option<f64>,
    macd_histogram: Option<f64>,
}

impl Technicals {
    fn none() -> Self {
        Self {
            rsi: None,
            macd_histogram: None,
        }
    }
}

/// Compute RSI and MACD histogram for a symbol.
fn compute_technicals(backend: &BackendConnection, symbol: &str) -> Technicals {
    let history = match get_history_backend(backend, symbol, 60) {
        Ok(h) if h.len() >= 30 => h,
        _ => return Technicals::none(),
    };

    let closes: Vec<f64> = history
        .iter()
        .map(|rec| rec.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();

    let rsi_vec = compute_rsi(&closes, 14);
    let macd_vec = compute_macd(&closes, 12, 26, 9);

    let rsi = rsi_vec.last().and_then(|x| *x);
    let macd_histogram = macd_vec.last().and_then(|x| *x).map(|m| m.histogram);

    Technicals {
        rsi,
        macd_histogram,
    }
}

fn missing_sector_symbols(price_map: &HashMap<String, Decimal>) -> Vec<&'static str> {
    SECTOR_ETFS
        .iter()
        .map(|(symbol, _)| *symbol)
        .filter(|symbol| !price_map.contains_key(*symbol))
        .collect()
}

fn backfill_sector_prices(
    backend: &BackendConnection,
    price_map: &mut HashMap<String, Decimal>,
    symbols: &[&str],
) -> Result<()> {
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

/// Run the sector command.
pub fn run(backend: &BackendConnection, _config: &Config, json: bool) -> Result<()> {
    let all_prices = get_all_cached_prices_backend(backend)?;
    let mut price_map: HashMap<String, Decimal> = all_prices
        .iter()
        .map(|p| (p.symbol.clone(), p.price))
        .collect();

    // Ensure sector command has complete coverage even if prices weren't preloaded
    // by other flows (portfolio/watchlist/refresh subsets).
    let missing = missing_sector_symbols(&price_map);
    backfill_sector_prices(backend, &mut price_map, &missing)?;

    // Get history for day change calculation
    let mut sector_data: Vec<(String, String, Decimal, Option<Decimal>, Technicals)> = Vec::new();

    for (symbol, name) in SECTOR_ETFS {
        let price = match price_map.get(*symbol) {
            Some(p) => *p,
            None => continue, // Skip if no price cached
        };

        // Get yesterday's close for daily change
        let history = get_history_backend(backend, symbol, 2)?;
        let day_change_pct = if history.len() >= 2 {
            let yesterday = history[history.len() - 2].close;
            if yesterday > Decimal::ZERO {
                Some((price - yesterday) / yesterday * Decimal::from(100))
            } else {
                None
            }
        } else {
            None
        };

        let tech = compute_technicals(backend, symbol);
        sector_data.push((
            symbol.to_string(),
            name.to_string(),
            price,
            day_change_pct,
            tech,
        ));
    }

    // Sort by day change descending (strongest first)
    sector_data.sort_by(|a, b| match (a.3, b.3) {
        (Some(ac), Some(bc)) => bc.partial_cmp(&ac).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    if json {
        print_json(&sector_data)?;
    } else {
        print_terminal(&sector_data)?;
    }

    Ok(())
}

// ─── JSON output ────────────────────────────────────────────────────────────

fn print_json(data: &[(String, String, Decimal, Option<Decimal>, Technicals)]) -> Result<()> {
    use serde_json::{json, Map, Value};

    let mut sectors = Vec::new();

    for (symbol, name, price, day_chg, tech) in data {
        let mut entry = Map::new();
        entry.insert("symbol".into(), json!(symbol));
        entry.insert("name".into(), json!(name));
        entry.insert(
            "price".into(),
            json!(price.to_string().parse::<f64>().unwrap_or(0.0)),
        );

        if let Some(chg) = day_chg {
            entry.insert(
                "day_change_pct".into(),
                json!(chg.to_string().parse::<f64>().unwrap_or(0.0)),
            );
        }

        if tech.rsi.is_some() || tech.macd_histogram.is_some() {
            let mut tech_obj = Map::new();
            if let Some(rsi) = tech.rsi {
                tech_obj.insert("rsi".into(), json!(rsi));
            }
            if let Some(macd) = tech.macd_histogram {
                tech_obj.insert("macd_histogram".into(), json!(macd));
            }
            entry.insert("technicals".into(), Value::Object(tech_obj));
        }

        sectors.push(Value::Object(entry));
    }

    let output = json!({
        "sectors": sectors,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

// ─── Terminal output ────────────────────────────────────────────────────────

fn print_terminal(data: &[(String, String, Decimal, Option<Decimal>, Technicals)]) -> Result<()> {
    println!("\n┌─────────────────────────────────────────────────────────────────┐");
    println!("│                 SECTOR + DEFENSE PERFORMANCE                   │");
    println!("├──────┬─────────────────────────┬──────────┬─────────┬────┬──────┤");
    println!("│ Sym  │ Sector                  │  Price   │  Day %  │RSI │ MACD │");
    println!("├──────┼─────────────────────────┼──────────┼─────────┼────┼──────┤");

    for (symbol, name, price, day_chg, tech) in data {
        let price_str = format!("${:.2}", price.to_string().parse::<f64>().unwrap_or(0.0));

        let day_chg_str = if let Some(chg) = day_chg {
            let val = chg.to_string().parse::<f64>().unwrap_or(0.0);
            if val >= 0.0 {
                format!("\x1b[32m+{:.2}%\x1b[0m", val)
            } else {
                format!("\x1b[31m{:.2}%\x1b[0m", val)
            }
        } else {
            "  ---  ".to_string()
        };

        let rsi_str = if let Some(rsi) = tech.rsi {
            format!("{:>3.0}", rsi)
        } else {
            " --".to_string()
        };

        let macd_str = if let Some(macd) = tech.macd_histogram {
            if macd > 0.0 {
                format!("\x1b[32m+{:.1}\x1b[0m", macd)
            } else {
                format!("\x1b[31m{:.1}\x1b[0m", macd)
            }
        } else {
            "  -- ".to_string()
        };

        println!(
            "│ {:<4} │ {:<23} │ {:>8} │ {:>9} │{:>3} │ {:>6} │",
            symbol, name, price_str, day_chg_str, rsi_str, macd_str
        );
    }

    println!("└──────┴─────────────────────────┴──────────┴─────────┴────┴──────┘");
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn identifies_missing_sector_symbols_from_cache() {
        let mut price_map = HashMap::new();
        price_map.insert("XLE".to_string(), dec!(100.0));
        price_map.insert("XLK".to_string(), dec!(200.0));

        let missing = missing_sector_symbols(&price_map);

        assert_eq!(missing.len(), SECTOR_ETFS.len() - 2);
        assert!(!missing.contains(&"XLE"));
        assert!(!missing.contains(&"XLK"));
        assert!(missing.contains(&"XLF"));
        assert!(missing.contains(&"GDX"));
        assert!(missing.contains(&"ITA"));
        assert!(missing.contains(&"LMT"));
        assert!(missing.contains(&"RTX"));
        assert!(missing.contains(&"PLTR"));
    }
}
