//! Futures data source — fetch current prices for major overnight futures contracts.
//!
//! Uses Yahoo Finance continuous contract symbols (e.g. ES=F, NQ=F) to provide
//! pre-market positioning data for index, commodity, and energy futures.

use anyhow::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::price::yahoo;

/// A single futures contract quote with change data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesQuote {
    pub symbol: String,
    pub name: String,
    pub last_price: Decimal,
    pub previous_close: Option<Decimal>,
    pub change: Option<Decimal>,
    pub change_pct: Option<Decimal>,
    pub volume: Option<u64>,
    pub fetched_at: String,
}

/// Standard futures symbols tracked for overnight positioning analysis.
pub const FUTURES_SYMBOLS: &[(&str, &str)] = &[
    ("ES=F", "S&P 500 Futures"),
    ("NQ=F", "Nasdaq 100 Futures"),
    ("YM=F", "Dow Futures"),
    ("RTY=F", "Russell 2000 Futures"),
    ("GC=F", "Gold Futures"),
    ("SI=F", "Silver Futures"),
    ("CL=F", "Crude Oil WTI Futures"),
];

/// Fetch a single futures quote from Yahoo Finance, computing change fields
/// from the previous close when available.
pub async fn fetch_futures_quote(symbol: &str, name: &str) -> Result<FuturesQuote> {
    let quote = yahoo::fetch_price(symbol).await?;

    let change = quote
        .previous_close
        .map(|prev| quote.price - prev);
    let change_pct = quote.previous_close.and_then(|prev| {
        if prev == Decimal::ZERO {
            None
        } else {
            Some((quote.price - prev) * Decimal::from(100) / prev)
        }
    });

    // Attempt to get volume from the chart API via a lightweight history fetch
    let volume = fetch_latest_volume(symbol).await.ok().flatten();

    Ok(FuturesQuote {
        symbol: symbol.to_string(),
        name: name.to_string(),
        last_price: quote.price,
        previous_close: quote.previous_close,
        change,
        change_pct,
        volume,
        fetched_at: quote.fetched_at,
    })
}

/// Fetch the latest volume for a symbol from Yahoo Finance 1-day history.
async fn fetch_latest_volume(symbol: &str) -> Result<Option<u64>> {
    let history = yahoo::fetch_history(symbol, 2).await?;
    Ok(history.last().and_then(|r| r.volume))
}

/// Fetch all standard futures quotes, returning partial results if some fail.
pub async fn fetch_all_futures() -> Vec<FuturesQuote> {
    let mut results = Vec::new();
    for &(symbol, name) in FUTURES_SYMBOLS {
        match fetch_futures_quote(symbol, name).await {
            Ok(q) => results.push(q),
            Err(_) => {
                // Skip failed symbols — partial results are better than none
            }
        }
    }
    results
}
