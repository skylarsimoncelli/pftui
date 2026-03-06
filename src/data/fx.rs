use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use yahoo_finance_api as yahoo;

/// Supported currencies for FX conversion.
/// All rates are to USD (e.g., GBPUSD=X means GBP → USD).
pub const SUPPORTED_CURRENCIES: &[&str] = &["GBP", "EUR", "CAD", "AUD", "JPY", "CHF"];

/// Fetch live FX rates for all supported currencies to USD.
/// Returns a map of currency code → rate (multiply foreign price by rate to get USD).
/// Example: GBP rate of 1.27 means £1 = $1.27 USD.
pub async fn fetch_all_fx_rates() -> Result<HashMap<String, Decimal>> {
    let mut rates = HashMap::new();
    
    // USD to USD is always 1.0
    rates.insert("USD".to_string(), dec!(1));

    for &currency in SUPPORTED_CURRENCIES {
        match fetch_fx_rate(currency).await {
            Ok(rate) => {
                rates.insert(currency.to_string(), rate);
            }
            Err(e) => {
                eprintln!("Warning: Failed to fetch FX rate for {}: {}", currency, e);
                // Continue with other currencies
            }
        }
    }

    Ok(rates)
}

/// Fetch the FX rate to convert from `from_currency` to USD.
/// Uses Yahoo Finance FX pairs (e.g., GBPUSD=X, EURUSD=X, CADUSD=X).
/// Returns the multiplier: price_in_foreign * rate = price_in_usd.
async fn fetch_fx_rate(from_currency: &str) -> Result<Decimal> {
    if from_currency == "USD" {
        return Ok(dec!(1));
    }

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

/// Fetch historical FX rates for a given currency to USD.
/// Returns a map of date string (YYYY-MM-DD) → FX rate.
#[allow(dead_code)]
pub async fn fetch_fx_history(
    from_currency: &str,
    days: u32,
) -> Result<HashMap<String, Decimal>> {
    if from_currency == "USD" {
        // No conversion needed for USD
        let mut rates = HashMap::new();
        rates.insert("USD".to_string(), dec!(1));
        return Ok(rates);
    }

    let pair = format!("{}USD=X", from_currency);
    let provider = yahoo::YahooConnector::new()?;
    let now = time::OffsetDateTime::now_utc();
    let start = now - time::Duration::days(days as i64);
    let response = provider.get_quote_history(&pair, start, now).await?;
    let quotes = response.quotes()?;

    let mut rates = HashMap::new();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_currencies() {
        assert_eq!(SUPPORTED_CURRENCIES.len(), 6);
        assert!(SUPPORTED_CURRENCIES.contains(&"GBP"));
        assert!(SUPPORTED_CURRENCIES.contains(&"EUR"));
        assert!(SUPPORTED_CURRENCIES.contains(&"CAD"));
    }
}
