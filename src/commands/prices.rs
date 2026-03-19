//! `pftui data prices` command — cached price snapshot for all tracked symbols.
//!
//! Returns the latest cached price for every symbol in `price_cache`.
//! Optionally filter by symbol. Supports `--json` for agent consumption.

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::db::backend::BackendConnection;
use crate::db::price_cache::get_all_cached_prices_backend;
use crate::db::price_history::get_history_backend;

#[derive(Serialize)]
struct PriceEntry {
    symbol: String,
    price: Decimal,
    currency: String,
    source: String,
    fetched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prev_close: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    change: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    change_pct: Option<Decimal>,
}

/// Run `pftui data prices` command.
pub fn run(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json: bool,
) -> Result<()> {
    let all_quotes = get_all_cached_prices_backend(backend)?;

    let mut entries: Vec<PriceEntry> = Vec::new();

    for quote in &all_quotes {
        if let Some(filter) = symbol {
            if !quote.symbol.eq_ignore_ascii_case(filter) {
                continue;
            }
        }

        // Try to get previous close from history (2 most recent bars)
        let (prev_close, change, change_pct) =
            match get_history_backend(backend, &quote.symbol, 2) {
                Ok(history) if history.len() >= 2 => {
                    let prev = history[history.len() - 2].close;
                    if prev != Decimal::ZERO {
                        let chg = quote.price - prev;
                        let pct = (chg * Decimal::from(100)) / prev;
                        (Some(prev), Some(chg), Some(pct.round_dp(2)))
                    } else {
                        (Some(prev), None, None)
                    }
                }
                _ => (None, None, None),
            };

        entries.push(PriceEntry {
            symbol: quote.symbol.clone(),
            price: quote.price,
            currency: quote.currency.clone(),
            source: quote.source.clone(),
            fetched_at: quote.fetched_at.clone(),
            prev_close,
            change,
            change_pct,
        });
    }

    entries.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        if entries.is_empty() {
            println!("No cached prices. Run `pftui data refresh` first.");
            return Ok(());
        }

        let header = format!(
            "{:<12} {:>12} {:>12} {:>8}  {:<10} FETCHED",
            "SYMBOL", "PRICE", "CHANGE", "CHG%", "SOURCE"
        );
        println!("{header}");
        println!("{}", "-".repeat(72));

        for e in &entries {
            let chg_str = e
                .change
                .map(|c| format!("{:.2}", c))
                .unwrap_or_else(|| "—".to_string());
            let pct_str = e
                .change_pct
                .map(|p| format!("{:.2}%", p))
                .unwrap_or_else(|| "—".to_string());
            println!(
                "{:<12} {:>12} {:>12} {:>8}  {:<10} {}",
                e.symbol, e.price, chg_str, pct_str, e.source, e.fetched_at
            );
        }

        println!("\n{} symbols", entries.len());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_entry_serializes_without_optional_nulls() {
        let entry = PriceEntry {
            symbol: "BTC-USD".to_string(),
            price: Decimal::from(50000),
            currency: "USD".to_string(),
            source: "yahoo".to_string(),
            fetched_at: "2026-03-18T12:00:00Z".to_string(),
            prev_close: None,
            change: None,
            change_pct: None,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(!json.contains("prev_close"));
        assert!(!json.contains("change"));
        assert!(json.contains("BTC-USD"));
        assert!(json.contains("50000"));
    }

    #[test]
    fn price_entry_serializes_with_change() {
        let entry = PriceEntry {
            symbol: "AAPL".to_string(),
            price: Decimal::from(150),
            currency: "USD".to_string(),
            source: "yahoo".to_string(),
            fetched_at: "2026-03-18T12:00:00Z".to_string(),
            prev_close: Some(Decimal::from(145)),
            change: Some(Decimal::from(5)),
            change_pct: Some(Decimal::new(345, 2)), // 3.45
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("prev_close"));
        assert!(json.contains("change_pct"));
    }
}
