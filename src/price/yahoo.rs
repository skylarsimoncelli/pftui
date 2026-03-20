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

fn uses_frankfurter_fallback(symbol: &str) -> bool {
    matches!(symbol, "JPY=X" | "CNY=X")
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
    if uses_frankfurter_fallback(symbol) {
        let currency = match symbol.strip_suffix("=X") {
            Some(currency) => currency,
            None => anyhow::bail!(
                "FX fallback symbol invariant violated: expected '=X' suffix for '{}'",
                symbol
            ),
        };
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
                    pre_market_price: None,
                    post_market_price: None,
                    post_market_change_percent: None,
                    previous_close: None,
                    open: None,
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

    // Extract previous_close from Yahoo metadata and open from today's quote
    let mut meta_previous_close = response
        .metadata()
        .ok()
        .and_then(|m| m.previous_close)
        .and_then(|v| Decimal::try_from(v).ok());
    let mut quote_open = Decimal::try_from(quote.open).ok();
    
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
                meta_previous_close = meta_previous_close.map(|p| (p * fx_rate).round_dp(4));
                quote_open = quote_open.map(|p| (p * fx_rate).round_dp(4));
            }
            Err(_) => {
                // If FX fetch fails, return the foreign price with a warning in source
                return Ok(PriceQuote {
                    symbol: symbol.to_string(),
                    price,
                    source: format!("yahoo (unconverted {})", currency),
                    currency,
                    fetched_at: now,
                    pre_market_price: None,
                    post_market_price: None,
                    post_market_change_percent: None,
                    previous_close: meta_previous_close,
                    open: quote_open,
                });
            }
        }
    }

    // Fetch extended hours data for US equities
    let (pre_market_price, post_market_price, post_market_change_percent) = 
        fetch_extended_hours(&yahoo_sym, &provider).await;

    Ok(PriceQuote {
        symbol: symbol.to_string(),
        price,
        currency: "USD".to_string(),
        source: "yahoo".to_string(),
        fetched_at: now,
        pre_market_price,
        post_market_price,
        post_market_change_percent,
        previous_close: meta_previous_close,
        open: quote_open,
    })
}

/// Fetch pre-market and post-market prices using Yahoo Finance v8 quote API.
/// Returns (pre_market, post_market, post_change_pct).
async fn fetch_extended_hours(
    symbol: &str,
    _provider: &yahoo::YahooConnector,
) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    // Only attempt extended hours for US equities (no .TO, no =X, etc.)
    if symbol.contains('.') || symbol.contains('=') {
        return (None, None, None);
    }

    // Use Yahoo Finance v8 quote API which includes extended hours
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?region=US&includePrePost=true&interval=1d&range=1d",
        symbol
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return (None, None, None),
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return (None, None, None),
    };

    if !resp.status().is_success() {
        return (None, None, None);
    }

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return (None, None, None),
    };
    
    // Extract post-market price and change from the meta section
    let meta = match json.get("chart")
        .and_then(|c| c.get("result"))
        .and_then(|r| r.get(0))
        .and_then(|r0| r0.get("meta"))
    {
        Some(m) => m,
        None => return (None, None, None),
    };
    
    let post_market_price = meta.get("postMarketPrice")
        .and_then(|v| v.as_f64())
        .and_then(|p| Decimal::try_from(p).ok())
        .filter(|&p| p > dec!(0));
    
    let post_market_change_percent = meta.get("postMarketChangePercent")
        .and_then(|v| v.as_f64())
        .and_then(|p| Decimal::try_from(p).ok());

    let pre_market_price = meta.get("preMarketPrice")
        .and_then(|v| v.as_f64())
        .and_then(|p| Decimal::try_from(p).ok())
        .filter(|&p| p > dec!(0));

    (pre_market_price, post_market_price, post_market_change_percent)
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
                let open = Decimal::try_from(q.open).ok().map(|mut o| {
                    if currency != "USD" {
                        if let Some(ref rates) = fx_rates {
                            if let Some(&rate) = rates.get(&date) {
                                o = (o * rate).round_dp(4);
                            } else if let Some(rate) = fallback_fx {
                                o = (o * rate).round_dp(4);
                            }
                        } else if let Some(rate) = fallback_fx {
                            o = (o * rate).round_dp(4);
                        }
                    }
                    o
                });
                let high = Decimal::try_from(q.high).ok().map(|mut h| {
                    if currency != "USD" {
                        if let Some(ref rates) = fx_rates {
                            if let Some(&rate) = rates.get(&date) {
                                h = (h * rate).round_dp(4);
                            } else if let Some(rate) = fallback_fx {
                                h = (h * rate).round_dp(4);
                            }
                        } else if let Some(rate) = fallback_fx {
                            h = (h * rate).round_dp(4);
                        }
                    }
                    h
                });
                let low = Decimal::try_from(q.low).ok().map(|mut l| {
                    if currency != "USD" {
                        if let Some(ref rates) = fx_rates {
                            if let Some(&rate) = rates.get(&date) {
                                l = (l * rate).round_dp(4);
                            } else if let Some(rate) = fallback_fx {
                                l = (l * rate).round_dp(4);
                            }
                        } else if let Some(rate) = fallback_fx {
                            l = (l * rate).round_dp(4);
                        }
                    }
                    l
                });
                records.push(HistoryRecord {
                    date,
                    close,
                    volume: Some(q.volume),
                    open,
                    high,
                    low,
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

    #[test]
    fn detects_special_fx_fallback_symbols() {
        assert!(uses_frankfurter_fallback("JPY=X"));
        assert!(uses_frankfurter_fallback("CNY=X"));
        assert!(!uses_frankfurter_fallback("EURUSD=X"));
    }
}
