use anyhow::Result;
use rust_decimal::Decimal;
use yahoo_finance_api as yahoo;

use crate::models::price::{HistoryRecord, PriceQuote};

/// Normalize a symbol to Yahoo Finance format.
///
/// Handles special cases like Toronto Stock Exchange trust units:
/// - `U.UN` → `U-UN.TO` (Sprott Uranium Trust)
/// - `X.UN` → `X-UN.TO` (generic TSX trust units)
///
/// Returns the original symbol if no normalization is needed.
pub fn normalize_yahoo_symbol(symbol: &str) -> String {
    let upper = symbol.to_uppercase();

    // TSX trust units: *.UN → *-UN.TO
    // Yahoo Finance uses dash instead of dot and appends .TO for Toronto
    if upper.ends_with(".UN") && !upper.ends_with(".TO") {
        let prefix = &upper[..upper.len() - 3]; // strip ".UN"
        return format!("{}-UN.TO", prefix);
    }

    // TSX stocks with .TO suffix: already correct for Yahoo
    // Return as-is for everything else
    symbol.to_string()
}

pub async fn fetch_price(symbol: &str) -> Result<PriceQuote> {
    let yahoo_sym = normalize_yahoo_symbol(symbol);
    let provider = yahoo::YahooConnector::new()?;
    let response = provider.get_latest_quotes(&yahoo_sym, "1d").await?;
    let quote = response.last_quote()?;

    let price = Decimal::try_from(quote.close)?;
    let now = chrono::Utc::now().to_rfc3339();

    Ok(PriceQuote {
        symbol: symbol.to_string(),
        price,
        currency: "USD".to_string(),
        source: "yahoo".to_string(),
        fetched_at: now,
    })
}

pub async fn fetch_history(symbol: &str, days: u32) -> Result<Vec<HistoryRecord>> {
    let yahoo_sym = normalize_yahoo_symbol(symbol);
    let provider = yahoo::YahooConnector::new()?;
    let now = time::OffsetDateTime::now_utc();
    let start = now - time::Duration::days(days as i64);

    let response = provider
        .get_quote_history(&yahoo_sym, start, now)
        .await?;
    let quotes = response.quotes()?;

    let mut records = Vec::new();
    for q in &quotes {
        let ts = chrono::DateTime::from_timestamp(q.timestamp as i64, 0);
        if let Some(dt) = ts {
            let date = dt.format("%Y-%m-%d").to_string();
            if let Ok(close) = Decimal::try_from(q.close) {
                records.push(HistoryRecord {
                    date,
                    close,
                    volume: Some(q.volume),
                });
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tsx_trust_unit() {
        assert_eq!(normalize_yahoo_symbol("U.UN"), "U-UN.TO");
        assert_eq!(normalize_yahoo_symbol("u.un"), "U-UN.TO");
    }

    #[test]
    fn test_normalize_tsx_trust_unit_multi_char_prefix() {
        assert_eq!(normalize_yahoo_symbol("HR.UN"), "HR-UN.TO");
        assert_eq!(normalize_yahoo_symbol("REI.UN"), "REI-UN.TO");
    }

    #[test]
    fn test_normalize_regular_symbol_unchanged() {
        assert_eq!(normalize_yahoo_symbol("AAPL"), "AAPL");
        assert_eq!(normalize_yahoo_symbol("GC=F"), "GC=F");
        assert_eq!(normalize_yahoo_symbol("BTC-USD"), "BTC-USD");
    }

    #[test]
    fn test_normalize_already_to_suffix_unchanged() {
        // Symbols already ending in .TO should not be double-suffixed
        assert_eq!(normalize_yahoo_symbol("RY.TO"), "RY.TO");
    }

    #[test]
    fn test_normalize_preserves_original_for_non_tsx() {
        assert_eq!(normalize_yahoo_symbol("^GSPC"), "^GSPC");
        assert_eq!(normalize_yahoo_symbol("DX-Y.NYB"), "DX-Y.NYB");
        assert_eq!(normalize_yahoo_symbol("GBPUSD=X"), "GBPUSD=X");
    }
}
