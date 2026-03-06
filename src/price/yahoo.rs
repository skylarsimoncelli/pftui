use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
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

/// Fetch the FX rate to convert from `from_currency` to USD.
/// Uses Yahoo Finance FX pairs (e.g., CADUSD=X).
/// Returns the multiplier: price_in_foreign * rate = price_in_usd.
async fn fetch_fx_rate(from_currency: &str) -> Result<Decimal> {
    let pair = format!("{}USD=X", from_currency);
    let provider = yahoo::YahooConnector::new()?;
    let response = provider.get_latest_quotes(&pair, "1d").await?;
    let quote = response.last_quote()?;
    let rate = Decimal::try_from(quote.close)?;
    if rate <= dec!(0) {
        anyhow::bail!("Invalid FX rate for {}: {}", pair, rate);
    }
    Ok(rate)
}

/// Fetch the daily FX rate history to convert from `from_currency` to USD.
/// Returns a map of date string → FX rate.
async fn fetch_fx_history(
    from_currency: &str,
    days: u32,
) -> Result<std::collections::HashMap<String, Decimal>> {
    let pair = format!("{}USD=X", from_currency);
    let provider = yahoo::YahooConnector::new()?;
    let now = time::OffsetDateTime::now_utc();
    let start = now - time::Duration::days(days as i64);
    let response = provider.get_quote_history(&pair, start, now).await?;
    let quotes = response.quotes()?;

    let mut rates = std::collections::HashMap::new();
    for q in &quotes {
        let ts = chrono::DateTime::from_timestamp(q.timestamp, 0);
        if let Some(dt) = ts {
            let date = dt.format("%Y-%m-%d").to_string();
            if let Ok(rate) = Decimal::try_from(q.close) {
                if rate > dec!(0) {
                    rates.insert(date, rate);
                }
            }
        }
    }
    Ok(rates)
}

/// Fetch FX rate from frankfurter.app API (free, no key required).
/// Returns rate to convert from_currency → USD.
async fn fetch_fx_rate_frankfurter(from_currency: &str) -> Result<Decimal> {
    let url = format!("https://api.frankfurter.app/latest?from={}&to=USD", from_currency);
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Frankfurter API returned {}", resp.status());
    }
    
    let json: serde_json::Value = resp.json().await?;
    let rate_val = json["rates"]["USD"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("Frankfurter API missing USD rate"))?;
    
    let rate = Decimal::try_from(rate_val)?;
    if rate <= dec!(0) {
        anyhow::bail!("Invalid FX rate from Frankfurter for {}: {}", from_currency, rate);
    }
    
    Ok(rate)
}

pub async fn fetch_price(symbol: &str) -> Result<PriceQuote> {
    let yahoo_sym = normalize_yahoo_symbol(symbol);
    
    // Special handling for FX pairs that Yahoo often gets wrong
    if symbol == "JPY=X" || symbol == "CNY=X" {
        let currency = symbol.strip_suffix("=X").unwrap();
        match fetch_fx_rate_frankfurter(currency).await {
            Ok(rate) => {
                // Frankfurter gives us FROM→USD, but JPY=X quote should be USD→JPY
                // So we need the inverse
                let inverse_rate = if rate > dec!(0) {
                    dec!(1.0) / rate
                } else {
                    anyhow::bail!("Invalid FX rate")
                };
                
                return Ok(PriceQuote {
                    symbol: symbol.to_string(),
                    price: inverse_rate,
                    currency: "USD".to_string(),
                    source: "frankfurter".to_string(),
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                });
            }
            Err(_) => {
                // Fall through to Yahoo
            }
        }
    }
    
    let provider = yahoo::YahooConnector::new()?;
    let response = provider.get_latest_quotes(&yahoo_sym, "1d").await?;
    let quote = response.last_quote()?;

    let mut price = Decimal::try_from(quote.close)?;
    
    // Check if Yahoo returned a bogus 1.0 for FX pairs
    if (symbol.ends_with("=X") || symbol.contains("USD")) && (price - dec!(1.0)).abs() < dec!(0.001) {
        // Try fallback
        if let Some(currency) = symbol.strip_suffix("=X") {
            if let Ok(rate) = fetch_fx_rate_frankfurter(currency).await {
                price = dec!(1.0) / rate; // Inverse for USD→currency quote
            }
        }
    }
    
    let now = chrono::Utc::now().to_rfc3339();

    // Check if the security trades in a non-USD currency and convert
    let currency = response
        .metadata()
        .ok()
        .and_then(|m| m.currency)
        .unwrap_or_else(|| "USD".to_string());

    if currency != "USD" {
        // Convert to USD using live FX rate
        match fetch_fx_rate(&currency).await {
            Ok(fx_rate) => {
                price = (price * fx_rate).round_dp(4);
            }
            Err(_) => {
                // If FX fetch fails, return the foreign price with a warning in source
                return Ok(PriceQuote {
                    symbol: symbol.to_string(),
                    price,
                    source: format!("yahoo (unconverted {})", currency),
                    currency,
                    fetched_at: now,
                });
            }
        }
    }

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

    // Check if the security trades in a non-USD currency
    let currency = response
        .metadata()
        .ok()
        .and_then(|m| m.currency)
        .unwrap_or_else(|| "USD".to_string());

    let fx_rates = if currency != "USD" {
        fetch_fx_history(&currency, days).await.ok()
    } else {
        None
    };

    // If we need FX conversion but have no rates, fetch a single spot rate as fallback
    let fallback_fx = if currency != "USD" && fx_rates.is_none() {
        fetch_fx_rate(&currency).await.ok()
    } else {
        None
    };

    let quotes = response.quotes()?;

    let mut records = Vec::new();
    for q in &quotes {
        let ts = chrono::DateTime::from_timestamp(q.timestamp, 0);
        if let Some(dt) = ts {
            let date = dt.format("%Y-%m-%d").to_string();
            if let Ok(mut close) = Decimal::try_from(q.close) {
                // Apply FX conversion if needed
                if currency != "USD" {
                    if let Some(ref rates) = fx_rates {
                        if let Some(&rate) = rates.get(&date) {
                            close = (close * rate).round_dp(4);
                        } else if let Some(rate) = fallback_fx {
                            // Use fallback spot rate for dates without FX data
                            close = (close * rate).round_dp(4);
                        }
                        // If no rate at all, skip this record to avoid
                        // mixing currencies
                        else {
                            continue;
                        }
                    } else if let Some(rate) = fallback_fx {
                        close = (close * rate).round_dp(4);
                    }
                    // No FX data at all — skip to avoid mixing currencies
                    else {
                        continue;
                    }
                }
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
