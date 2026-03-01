use anyhow::Result;
use rust_decimal::Decimal;
use yahoo_finance_api as yahoo;

use crate::models::price::{HistoryRecord, PriceQuote};

pub async fn fetch_price(symbol: &str) -> Result<PriceQuote> {
    let provider = yahoo::YahooConnector::new()?;
    let response = provider.get_latest_quotes(symbol, "1d").await?;
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
    let provider = yahoo::YahooConnector::new()?;
    let now = time::OffsetDateTime::now_utc();
    let start = now - time::Duration::days(days as i64);

    let response = provider
        .get_quote_history(symbol, start, now)
        .await?;
    let quotes = response.quotes()?;

    let mut records = Vec::new();
    for q in &quotes {
        let ts = chrono::DateTime::from_timestamp(q.timestamp as i64, 0);
        if let Some(dt) = ts {
            let date = dt.format("%Y-%m-%d").to_string();
            if let Ok(close) = Decimal::try_from(q.close) {
                records.push(HistoryRecord { date, close });
            }
        }
    }
    Ok(records)
}
