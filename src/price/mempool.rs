//! mempool.space — BTC spot-price fallback (free, no key).
//!
//! Used as the LAST resort in the BTC spot chain
//! (coingecko → yahoo → mempool.space) during `data refresh`. Spot only —
//! no OHLCV, no history. Also exposes the current block height as a cheap
//! bonus field for provenance logging when the fallback fires.
//!
//! Endpoint: `GET https://mempool.space/api/v1/prices`
//! Response: `{"time": 1781190309, "USD": 62631, "EUR": 54346, ...}`
//! (prices are whole-unit JSON numbers).
//!
//! Politeness: one request per refresh (only when the primaries failed),
//! proper User-Agent, 10s timeout.

use anyhow::{bail, Context, Result};
use rust_decimal::Decimal;

const PRICES_URL: &str = "https://mempool.space/api/v1/prices";
const TIP_HEIGHT_URL: &str = "https://mempool.space/api/blocks/tip/height";

/// Build a reqwest client with a proper User-Agent and a tight timeout.
fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(Into::into)
}

#[derive(serde::Deserialize)]
struct PricesResponse {
    #[serde(rename = "USD")]
    usd: f64,
}

/// Parse the `/api/v1/prices` JSON body into a USD spot price.
fn parse_prices_response(body: &str) -> Result<Decimal> {
    let parsed: PricesResponse =
        serde_json::from_str(body).context("mempool.space prices response did not parse")?;
    if parsed.usd <= 0.0 {
        bail!("mempool.space returned non-positive USD price {}", parsed.usd);
    }
    Decimal::try_from(parsed.usd).context("mempool.space USD price not representable as Decimal")
}

/// Fetch the current BTC/USD spot price from mempool.space.
pub async fn fetch_btc_spot_usd() -> Result<Decimal> {
    let client = build_client()?;
    let resp = client.get(PRICES_URL).send().await?;
    if !resp.status().is_success() {
        bail!("mempool.space prices returned status {}", resp.status());
    }
    let body = resp.text().await?;
    parse_prices_response(&body)
}

/// Fetch the current Bitcoin block height (plain-integer body).
/// Best-effort bonus field — callers should tolerate failure.
pub async fn fetch_block_height() -> Result<u64> {
    let client = build_client()?;
    let resp = client.get(TIP_HEIGHT_URL).send().await?;
    if !resp.status().is_success() {
        bail!("mempool.space tip height returned status {}", resp.status());
    }
    let body = resp.text().await?;
    parse_block_height(&body)
}

/// Parse the plain-integer block-height body.
fn parse_block_height(body: &str) -> Result<u64> {
    body.trim()
        .parse::<u64>()
        .with_context(|| format!("mempool.space tip height not an integer: {:?}", body.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Canned from a live `GET /api/v1/prices` response (2026-06-11).
    const PRICES_FIXTURE: &str = r#"{"time":1781190309,"USD":62631,"EUR":54346,"GBP":47076,"CAD":87798,"CHF":50148,"AUD":89572,"JPY":10059437}"#;

    #[test]
    fn parses_live_shaped_prices_response() {
        let price = parse_prices_response(PRICES_FIXTURE).unwrap();
        assert_eq!(price, dec!(62631));
    }

    #[test]
    fn rejects_missing_usd_field() {
        assert!(parse_prices_response(r#"{"time":1,"EUR":54346}"#).is_err());
    }

    #[test]
    fn rejects_non_positive_price() {
        assert!(parse_prices_response(r#"{"time":1,"USD":0}"#).is_err());
        assert!(parse_prices_response(r#"{"time":1,"USD":-5}"#).is_err());
    }

    #[test]
    fn rejects_garbage_body() {
        assert!(parse_prices_response("<html>rate limited</html>").is_err());
    }

    #[test]
    fn parses_block_height_plain_integer() {
        assert_eq!(parse_block_height("953254").unwrap(), 953254);
        assert_eq!(parse_block_height("953254\n").unwrap(), 953254);
        assert!(parse_block_height("not-a-number").is_err());
    }

    /// LIVE smoke — run explicitly with `cargo test -- --ignored mempool_live`.
    #[tokio::test]
    #[ignore = "hits the real mempool.space API"]
    async fn mempool_live_smoke() {
        let price = fetch_btc_spot_usd().await.expect("live BTC spot fetch");
        assert!(price > dec!(1000), "implausible BTC spot: {price}");
        let height = fetch_block_height().await.expect("live block height");
        assert!(height > 800_000, "implausible block height: {height}");
        eprintln!("LIVE mempool.space: BTC=${price} block={height}");
    }
}
