//! `pftui data futures` — Overnight futures prices for pre-market positioning.
//!
//! Fetches current prices for major index, commodity, and energy futures
//! (ES, NQ, YM, RTY, GC, SI, CL) using Yahoo Finance continuous contracts.
//! Results are cached in the `futures_cache` table for agent consumption.

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::data::futures::{self, FuturesQuote, FUTURES_SYMBOLS};
use crate::db::backend::BackendConnection;
use crate::db::futures_cache;

#[derive(Debug, Serialize)]
struct FuturesReport {
    quotes: Vec<FuturesQuoteJson>,
    fetched: usize,
    failed: usize,
    total: usize,
}

#[derive(Debug, Serialize)]
struct FuturesQuoteJson {
    symbol: String,
    name: String,
    last_price: f64,
    previous_close: Option<f64>,
    change: Option<f64>,
    change_pct: Option<f64>,
    volume: Option<u64>,
    fetched_at: String,
}

impl From<&FuturesQuote> for FuturesQuoteJson {
    fn from(q: &FuturesQuote) -> Self {
        Self {
            symbol: q.symbol.clone(),
            name: q.name.clone(),
            last_price: dec_to_f64(q.last_price),
            previous_close: q.previous_close.map(dec_to_f64),
            change: q.change.map(dec_to_f64),
            change_pct: q.change_pct.map(dec_to_f64),
            volume: q.volume,
            fetched_at: q.fetched_at.clone(),
        }
    }
}

pub fn run(backend: &BackendConnection, json: bool, cached_only: bool) -> Result<()> {
    let quotes = if cached_only {
        // Read from cache only
        futures_cache::get_all_backend(backend).unwrap_or_default()
    } else {
        // Fetch live data
        let rt = tokio::runtime::Runtime::new()?;
        let live = rt.block_on(futures::fetch_all_futures());

        // Cache each quote
        for q in &live {
            let _ = futures_cache::upsert_backend(backend, q);
        }

        live
    };

    let total = FUTURES_SYMBOLS.len();
    let fetched = quotes.len();
    let failed = total - fetched;

    if json {
        let report = FuturesReport {
            quotes: quotes.iter().map(FuturesQuoteJson::from).collect(),
            fetched,
            failed,
            total,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("\nOvernight Futures — Pre-Market Positioning");
    println!("═══════════════════════════════════════════\n");

    if quotes.is_empty() {
        println!("  No futures data available.");
        if cached_only {
            println!("  (cached-only mode — run without --cached-only to fetch live data)");
        }
        println!();
        return Ok(());
    }

    // Print header
    println!(
        "  {:<8} {:<24} {:>12} {:>10} {:>8} {:>12}",
        "Symbol", "Name", "Last", "Change", "Chg%", "Volume"
    );
    println!("  {}", "─".repeat(78));

    for q in &quotes {
        let change_str = q
            .change
            .map(|c| format_signed_decimal(c, 2))
            .unwrap_or_else(|| "—".to_string());
        let pct_str = q
            .change_pct
            .map(|p| format!("{}%", format_signed_decimal(p, 2)))
            .unwrap_or_else(|| "—".to_string());
        let vol_str = q
            .volume
            .map(format_with_commas)
            .unwrap_or_else(|| "—".to_string());

        println!(
            "  {:<8} {:<24} {:>12} {:>10} {:>8} {:>12}",
            q.symbol,
            q.name,
            format_price(q.last_price),
            change_str,
            pct_str,
            vol_str,
        );
    }

    println!();

    if failed > 0 {
        println!(
            "  ⚠ {}/{} symbols failed to fetch (partial data shown)",
            failed, total
        );
        println!();
    }

    // Print fetch timestamp from first quote
    if let Some(first) = quotes.first() {
        println!("  Fetched: {}", first.fetched_at);
        println!();
    }

    Ok(())
}

fn dec_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

fn format_price(d: Decimal) -> String {
    let val = dec_to_f64(d);
    if val >= 10.0 {
        format!("{:.2}", val)
    } else {
        format!("{:.4}", val)
    }
}

fn format_signed_decimal(d: Decimal, dp: u32) -> String {
    let val = dec_to_f64(d.round_dp(dp));
    if val >= 0.0 {
        format!("+{:.prec$}", val, prec = dp as usize)
    } else {
        format!("{:.prec$}", val, prec = dp as usize)
    }
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_with_commas_works() {
        assert_eq!(format_with_commas(0), "0");
        assert_eq!(format_with_commas(999), "999");
        assert_eq!(format_with_commas(1000), "1,000");
        assert_eq!(format_with_commas(1_234_567), "1,234,567");
    }

    #[test]
    fn format_signed_decimal_positive() {
        let d = Decimal::new(123, 2); // 1.23
        assert_eq!(format_signed_decimal(d, 2), "+1.23");
    }

    #[test]
    fn format_signed_decimal_negative() {
        let d = Decimal::new(-456, 2); // -4.56
        assert_eq!(format_signed_decimal(d, 2), "-4.56");
    }

    #[test]
    fn format_price_large() {
        let d = Decimal::new(5_432_100, 2); // 54321.00
        assert_eq!(format_price(d), "54321.00");
    }

    #[test]
    fn format_price_small() {
        let d = Decimal::new(123, 2); // 1.23
        assert_eq!(format_price(d), "1.2300");
    }

    #[test]
    fn futures_quote_json_from() {
        let q = FuturesQuote {
            symbol: "ES=F".to_string(),
            name: "S&P 500 Futures".to_string(),
            last_price: Decimal::new(550_000, 2),
            previous_close: Some(Decimal::new(549_000, 2)),
            change: Some(Decimal::new(10_00, 2)),
            change_pct: Some(Decimal::new(18, 2)),
            volume: Some(1_234_567),
            fetched_at: "2026-03-26T00:00:00Z".to_string(),
        };
        let j = FuturesQuoteJson::from(&q);
        assert_eq!(j.symbol, "ES=F");
        assert!((j.last_price - 5500.0).abs() < 0.01);
        assert!((j.change.unwrap() - 10.0).abs() < 0.01);
        assert_eq!(j.volume, Some(1_234_567));
    }
}
