//! GeckoTerminal — on-chain XAUT/USDT pool price as a GOLD spot fallback
//! (free, no key).
//!
//! Used as the only fallback in the gold spot chain (yahoo → geckoterminal)
//! during `data refresh`. Spot only — no OHLCV, no history.
//!
//! XAUT (Tether Gold, 1 token = 1 troy oz) is an on-chain PROXY for spot
//! gold: it typically tracks XAU within ~0.5–1% but can dislocate under
//! market stress or thin DEX liquidity, and GC=F (front-month futures, the
//! canonical pftui gold series) itself carries basis vs spot. That is why
//! every price from this source passes the 5% divergence guard against the
//! last stored GC=F close before it is allowed into price_history.
//!
//! Canonical pool: Uniswap V3 XAUt/USDT 0.05% on Ethereum — the deepest
//! XAUT pool (~$13M reserve, ~$11M daily volume as of 2026-06).
//! Endpoint:
//!   `GET https://api.geckoterminal.com/api/v2/networks/eth/pools/
//!        0x6546055f46e866a4b9a4a13e81273e3152bae5da`
//! Field: `data.attributes.base_token_price_usd` (decimal string; base
//! token of this pool is XAUt).
//!
//! Politeness: GeckoTerminal free tier allows 30 calls/min; we make a
//! single request per refresh, and only when Yahoo failed for the gold
//! symbol. Proper User-Agent, 10s timeout.

use anyhow::{bail, Context, Result};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Uniswap V3 XAUt/USDT 0.05% pool on Ethereum (base token = XAUt).
const XAUT_POOL_URL: &str = "https://api.geckoterminal.com/api/v2/networks/eth/pools/0x6546055f46e866a4b9a4a13e81273e3152bae5da";

/// Build a reqwest client with a proper User-Agent and a tight timeout.
fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("pftui/1.0 (https://github.com/skylarsimoncelli/pftui)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(Into::into)
}

#[derive(serde::Deserialize)]
struct PoolResponse {
    data: PoolData,
}

#[derive(serde::Deserialize)]
struct PoolData {
    attributes: PoolAttributes,
}

#[derive(serde::Deserialize)]
struct PoolAttributes {
    /// Decimal string, e.g. "4054.3640907979" — base token (XAUt) in USD.
    base_token_price_usd: String,
}

/// Parse the pool response body into the XAUt USD price.
fn parse_pool_response(body: &str) -> Result<Decimal> {
    let parsed: PoolResponse =
        serde_json::from_str(body).context("GeckoTerminal pool response did not parse")?;
    let raw = parsed.data.attributes.base_token_price_usd;
    let price = Decimal::from_str(&raw)
        .with_context(|| format!("GeckoTerminal base_token_price_usd not a decimal: {raw:?}"))?;
    if price <= Decimal::ZERO {
        bail!("GeckoTerminal returned non-positive XAUT price {}", price);
    }
    Ok(price)
}

/// Fetch the XAUt/USD price from the canonical GeckoTerminal pool.
pub async fn fetch_xaut_usd() -> Result<Decimal> {
    let client = build_client()?;
    let resp = client
        .get(XAUT_POOL_URL)
        .header("Accept", "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("GeckoTerminal pool returned status {}", resp.status());
    }
    let body = resp.text().await?;
    parse_pool_response(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Canned (trimmed) from a live pool response (2026-06-11).
    const POOL_FIXTURE: &str = r#"{"data":{"id":"eth_0x6546055f46e866a4b9a4a13e81273e3152bae5da","type":"pool","attributes":{"base_token_price_usd":"4054.3640907979","quote_token_price_usd":"0.996835762149867","name":"XAUt / USDT 0.05%","token_price_usd":"4054.3640907979","reserve_in_usd":"12924043.2916"},"relationships":{"base_token":{"data":{"id":"eth_0x68749665ff8d2d112fa859aa293f07a622782f38","type":"token"}}}}}"#;

    #[test]
    fn parses_live_shaped_pool_response() {
        let price = parse_pool_response(POOL_FIXTURE).unwrap();
        assert_eq!(price, dec!(4054.3640907979));
    }

    #[test]
    fn rejects_missing_price_field() {
        assert!(parse_pool_response(r#"{"data":{"attributes":{"name":"XAUt / USDT"}}}"#).is_err());
    }

    #[test]
    fn rejects_non_decimal_price() {
        let body = r#"{"data":{"attributes":{"base_token_price_usd":"n/a"}}}"#;
        assert!(parse_pool_response(body).is_err());
    }

    #[test]
    fn rejects_non_positive_price() {
        let body = r#"{"data":{"attributes":{"base_token_price_usd":"0"}}}"#;
        assert!(parse_pool_response(body).is_err());
    }

    #[test]
    fn rejects_garbage_body() {
        assert!(parse_pool_response("<html>429</html>").is_err());
    }

    /// LIVE smoke — run explicitly with `cargo test -- --ignored geckoterminal_live`.
    #[tokio::test]
    #[ignore = "hits the real GeckoTerminal API"]
    async fn geckoterminal_live_smoke() {
        let price = fetch_xaut_usd().await.expect("live XAUT fetch");
        assert!(price > dec!(100), "implausible XAUT price: {price}");
        eprintln!("LIVE GeckoTerminal XAUt/USDT: ${price}");
    }
}
