//! Fetch BTC on-chain data from multiple sources.
//!
//! Data sources:
//! - Blockchair API: network metrics
//! - btcetffundflow.com: BTC ETF flows
//! - BitInfoCharts: labeled rich-list exchange wallets, concentration, active addresses,
//!   and 24h aggregate "100 largest transactions" activity

use anyhow::{anyhow, bail, Context, Result};
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

const BITINFOCHARTS_BTC_URL: &str = "https://bitinfocharts.com/bitcoin/";
const BITINFOCHARTS_RICH_LIST_URL: &str =
    "https://bitinfocharts.com/top-100-richest-bitcoin-addresses.html";

fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; pftui/1.0)")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(Into::into)
}

fn fetch_html(url: &str) -> Result<String> {
    let client = build_client()?;
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to fetch {}", url))?;

    if !response.status().is_success() {
        bail!("{} returned HTTP {}", url, response.status());
    }

    response
        .text()
        .with_context(|| format!("failed to read {}", url))
}

fn cached_selector<'a>(slot: &'a OnceLock<Selector>, css: &str) -> Result<&'a Selector> {
    if slot.get().is_none() {
        let parsed =
            Selector::parse(css).map_err(|e| anyhow!("invalid CSS selector '{}': {:?}", css, e))?;
        let _ = slot.set(parsed);
    }
    slot.get()
        .ok_or_else(|| anyhow!("failed to initialize CSS selector '{}'", css))
}

fn element_text(element: &scraper::ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// Exchange Flow Data (Blockchair alternative approach)
// ============================================================================

#[derive(Debug, Clone)]
pub struct ExchangeFlow {
    pub date: String,
    pub net_flow: f64,
    pub inflow: f64,
    pub outflow: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeReserveSnapshot {
    pub date: String,
    pub reserve_btc: f64,
    pub reserve_usd: f64,
    pub tracked_wallets: usize,
    pub exchange_labels: usize,
    pub net_flow_7d_btc: f64,
    pub net_flow_30d_btc: f64,
    pub top_exchanges: Vec<ExchangeReserveEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeReserveEntry {
    pub label: String,
    pub balance_btc: f64,
    pub balance_usd: f64,
    pub wallets: usize,
    pub flow_7d_btc: f64,
    pub flow_30d_btc: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnchainMarketStats {
    pub date: String,
    pub top_100_richest_btc: f64,
    pub top_100_share_pct: f64,
    pub top_10_share_pct: f64,
    pub top_1000_share_pct: f64,
    pub top_10000_share_pct: f64,
    pub largest_transactions_24h_btc: f64,
    pub largest_transactions_24h_usd: f64,
    pub largest_transactions_24h_share_pct: f64,
    pub active_addresses_24h: u64,
}

/// Fetch BTC exchange flows.
pub fn fetch_exchange_flows() -> Result<Vec<ExchangeFlow>> {
    let snapshot = fetch_exchange_reserve_snapshot()?;
    Ok(vec![ExchangeFlow {
        date: snapshot.date,
        net_flow: snapshot.net_flow_7d_btc,
        inflow: snapshot.net_flow_7d_btc.max(0.0),
        outflow: (-snapshot.net_flow_7d_btc).max(0.0),
    }])
}

pub fn fetch_exchange_reserve_snapshot() -> Result<ExchangeReserveSnapshot> {
    let mut rows = Vec::new();
    for page in 1..=5 {
        let url = if page == 1 {
            BITINFOCHARTS_RICH_LIST_URL.to_string()
        } else {
            format!(
                "https://bitinfocharts.com/top-100-richest-bitcoin-addresses-{}.html",
                page
            )
        };

        let html = fetch_html(&url)?;
        let page_rows = parse_exchange_wallet_rows(&html)?;
        if page_rows.is_empty() {
            break;
        }
        rows.extend(page_rows);
    }

    if rows.is_empty() {
        bail!("no labeled exchange wallets found in BitInfoCharts rich list");
    }

    let mut by_exchange: HashMap<String, ExchangeReserveEntry> = HashMap::new();
    let mut tracked_wallets = 0usize;

    for row in rows {
        tracked_wallets += 1;
        let entry = by_exchange
            .entry(row.label.clone())
            .or_insert(ExchangeReserveEntry {
                label: row.label,
                balance_btc: 0.0,
                balance_usd: 0.0,
                wallets: 0,
                flow_7d_btc: 0.0,
                flow_30d_btc: 0.0,
            });
        entry.balance_btc += row.balance_btc;
        entry.balance_usd += row.balance_usd;
        entry.wallets += 1;
        entry.flow_7d_btc += row.flow_7d_btc.unwrap_or(0.0);
        entry.flow_30d_btc += row.flow_30d_btc.unwrap_or(0.0);
    }

    let mut top_exchanges: Vec<_> = by_exchange.into_values().collect();
    top_exchanges.sort_by(|a, b| {
        b.balance_btc
            .partial_cmp(&a.balance_btc)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let reserve_btc = top_exchanges.iter().map(|entry| entry.balance_btc).sum();
    let reserve_usd = top_exchanges.iter().map(|entry| entry.balance_usd).sum();
    let net_flow_7d_btc = top_exchanges.iter().map(|entry| entry.flow_7d_btc).sum();
    let net_flow_30d_btc = top_exchanges.iter().map(|entry| entry.flow_30d_btc).sum();

    Ok(ExchangeReserveSnapshot {
        date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        reserve_btc,
        reserve_usd,
        tracked_wallets,
        exchange_labels: top_exchanges.len(),
        net_flow_7d_btc,
        net_flow_30d_btc,
        top_exchanges: top_exchanges.into_iter().take(10).collect(),
    })
}

// ============================================================================
// BTC ETF Flow Data (CoinGlass scraping)
// ============================================================================

#[derive(Debug, Clone)]
pub struct EtfFlow {
    pub date: String,      // YYYY-MM-DD
    pub fund: String,      // e.g., "IBIT", "FBTC", "GBTC"
    pub net_flow_btc: f64, // Net BTC inflow (positive = inflow)
    pub net_flow_usd: f64, // Net USD value
}

/// Fetch BTC ETF flows from btcetffundflow.com embedded data.
///
/// The site embeds flow data in JS arrays in the HTML. This implementation
/// parses the embedded JSON structure from the page source.
///
/// Data update frequency: D+1 09:00 GMT (next day, 9 AM)
/// Source: btcetffundflow.com (managed by SmashFi)
pub fn fetch_etf_flows() -> Result<Vec<EtfFlow>> {
    let url = "https://btcetffundflow.com/us";

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; pftui/1.0)")
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = client.get(url).send()?;

    if !response.status().is_success() {
        bail!("Failed to fetch ETF flow page: HTTP {}", response.status());
    }

    let html = response.text()?;

    // Parse embedded JSON data from the HTML
    // The page embeds data in a structure like: "flows2":[{"provider":"0","value":"131.0"}, ...]
    // This represents the latest daily flows per provider (provider index maps to ETF fund)

    parse_btcetffundflow_html(&html)
}

/// Parse btcetffundflow.com HTML for embedded ETF flow JSON data.
///
/// The page embeds flow data in a JSON structure within a Next.js <script> tag.
/// We extract the "flows2" array which contains the most recent daily flows.
fn parse_btcetffundflow_html(html: &str) -> Result<Vec<EtfFlow>> {
    // ETF provider index mapping (from site's embedded data)
    let provider_names = [
        "TOTAL", "GBTC", "BITB", "IBIT", "HODL", "EZBC", "BTCO", "BRRR", "FBTC", "DEFI", "ARKB",
        "BTCW", "BTC",
    ];

    // Find the embedded JSON data in the Next.js script tag
    // Pattern: "__NEXT_DATA__" type="application/json">{"props": ... "flows2":

    let start_marker = "__NEXT_DATA__\" type=\"application/json\">";
    let start_idx = html
        .find(start_marker)
        .ok_or_else(|| anyhow::anyhow!("Could not find embedded data marker"))?;

    let json_start = start_idx + start_marker.len();
    let json_end = html[json_start..]
        .find("</script>")
        .ok_or_else(|| anyhow::anyhow!("Could not find end of JSON data"))?;

    let json_str = &html[json_start..json_start + json_end];

    // Parse the JSON to extract flows2 array
    let json_value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse embedded JSON: {}", e))?;

    // Navigate to: props.pageProps.dehydratedState.queries[0].state.data.data.flows2
    let flows2 = json_value
        .get("props")
        .and_then(|p| p.get("pageProps"))
        .and_then(|pp| pp.get("dehydratedState"))
        .and_then(|ds| ds.get("queries"))
        .and_then(|q| q.get(0))
        .and_then(|q0| q0.get("state"))
        .and_then(|s| s.get("data"))
        .and_then(|d| d.get("data"))
        .and_then(|d2| d2.get("flows2"))
        .and_then(|f| f.as_array())
        .ok_or_else(|| anyhow::anyhow!("Could not navigate to flows2 array in JSON structure"))?;

    // Also get the timestamp from the first query's dataUpdatedAt
    let timestamp_ms = json_value
        .get("props")
        .and_then(|p| p.get("pageProps"))
        .and_then(|pp| pp.get("dehydratedState"))
        .and_then(|ds| ds.get("queries"))
        .and_then(|q| q.get(0))
        .and_then(|q0| q0.get("state"))
        .and_then(|s| s.get("dataUpdatedAt"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);

    // Convert timestamp to date string (YYYY-MM-DD)
    let date = if timestamp_ms > 0 {
        let datetime = chrono::DateTime::from_timestamp(timestamp_ms / 1000, 0)
            .unwrap_or_else(chrono::Utc::now);
        datetime.format("%Y-%m-%d").to_string()
    } else {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    };

    let mut results = Vec::new();

    // Parse each flow entry
    for (idx, flow_obj) in flows2.iter().enumerate() {
        if let (Some(provider_idx), Some(value_str)) = (
            flow_obj.get("provider").and_then(|p| p.as_str()),
            flow_obj.get("value").and_then(|v| v.as_str()),
        ) {
            // Skip TOTAL (provider "0")
            if provider_idx == "0" {
                continue;
            }

            let provider_num: usize = provider_idx.parse().unwrap_or(idx);

            if provider_num < provider_names.len() {
                let flow_btc: f64 = value_str.parse().unwrap_or(0.0);

                // Estimate USD value (we don't have real-time BTC price in this context)
                // Use a placeholder - actual implementation should fetch current BTC price
                let btc_price = 100_000.0; // Placeholder
                let flow_usd = flow_btc * btc_price;

                results.push(EtfFlow {
                    date: date.clone(),
                    fund: provider_names[provider_num].to_string(),
                    net_flow_btc: flow_btc,
                    net_flow_usd: flow_usd,
                });
            }
        }
    }

    if results.is_empty() {
        bail!("No ETF flow data extracted from page");
    }

    Ok(results)
}

// ============================================================================
// Whale Alert (Large Transactions)
// ============================================================================

#[derive(Debug, Clone)]
pub struct WhaleTransaction {
    pub timestamp: i64,
    pub amount_btc: f64,
    pub amount_usd: f64,
    pub from_owner: String,
    pub to_owner: String,
    pub tx_hash: String,
}

/// Fetch aggregate large-transaction activity from BitInfoCharts.
pub fn fetch_whale_transactions() -> Result<Vec<WhaleTransaction>> {
    let stats = fetch_market_stats()?;
    Ok(vec![WhaleTransaction {
        timestamp: chrono::Utc::now().timestamp(),
        amount_btc: stats.largest_transactions_24h_btc,
        amount_usd: stats.largest_transactions_24h_usd,
        from_owner: "100 largest BTC transactions (24h)".to_string(),
        to_owner: format!("active addresses: {}", stats.active_addresses_24h),
        tx_hash: "aggregate-24h-largest-100".to_string(),
    }])
}

pub fn fetch_market_stats() -> Result<OnchainMarketStats> {
    let html = fetch_html(BITINFOCHARTS_BTC_URL)?;
    parse_market_stats(&html)
}

// ============================================================================
// Network Metrics (Blockchair - Working)
// ============================================================================

#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    pub mempool_size: u64,
    pub hash_rate: f64, // H/s
    pub difficulty: f64,
    pub avg_fee_sat_b: f64, // Sat/byte average fee
    pub blocks_24h: u64,
}

/// Fetch current BTC network metrics from Blockchair.
///
/// This endpoint works and requires no API key.
pub fn fetch_network_metrics() -> Result<NetworkMetrics> {
    let url = "https://api.blockchair.com/bitcoin/stats";
    let client = build_client()?;

    let resp = client.get(url).send()?;

    if !resp.status().is_success() {
        bail!("Blockchair API returned {}", resp.status());
    }

    let body: BlockchairStatsResponse = resp.json()?;

    Ok(NetworkMetrics {
        mempool_size: body.data.mempool_transactions,
        hash_rate: body.data.hashrate_24h,
        difficulty: body.data.difficulty,
        avg_fee_sat_b: body.data.suggested_transaction_fee_per_byte_sat,
        blocks_24h: body.data.blocks_24h,
    })
}

#[derive(Debug, Deserialize)]
struct BlockchairStatsResponse {
    data: BlockchairStats,
}

#[derive(Debug, Deserialize)]
struct BlockchairStats {
    mempool_transactions: u64,
    hashrate_24h: f64,
    difficulty: f64,
    suggested_transaction_fee_per_byte_sat: f64,
    blocks_24h: u64,
}

#[derive(Debug, Clone)]
struct ExchangeWalletRow {
    label: String,
    balance_btc: f64,
    balance_usd: f64,
    flow_7d_btc: Option<f64>,
    flow_30d_btc: Option<f64>,
}

fn parse_exchange_wallet_rows(html: &str) -> Result<Vec<ExchangeWalletRow>> {
    static TABLE_SEL: OnceLock<Selector> = OnceLock::new();
    static ROW_SEL: OnceLock<Selector> = OnceLock::new();
    static CELL_SEL: OnceLock<Selector> = OnceLock::new();

    let doc = Html::parse_document(html);
    let table_sel = cached_selector(&TABLE_SEL, "table[id^='tblOne'], table[id^='tblTwo']")?;
    let row_sel = cached_selector(&ROW_SEL, "tr")?;
    let cell_sel = cached_selector(&CELL_SEL, "td")?;

    let mut rows = Vec::new();
    for table in doc.select(table_sel) {
        for row in table.select(row_sel) {
            let cells: Vec<String> = row
                .select(cell_sel)
                .map(|cell| element_text(&cell))
                .filter(|text| !text.is_empty())
                .collect();
            if cells.len() < 3 {
                continue;
            }
            if cells[0].parse::<usize>().is_err() {
                continue;
            }

            let address_info = &cells[1];
            let Some(label) = extract_exchange_label(address_info) else {
                continue;
            };

            rows.push(ExchangeWalletRow {
                label,
                balance_btc: extract_btc_value(&cells[2]).context("missing balance BTC")?,
                balance_usd: extract_usd_value(&cells[2]).unwrap_or(0.0),
                flow_7d_btc: extract_signed_btc_after(address_info, "7d:"),
                flow_30d_btc: extract_signed_btc_after(address_info, "30d:"),
            });
        }
    }

    Ok(rows)
}

fn parse_market_stats(html: &str) -> Result<OnchainMarketStats> {
    let top_100_richest = extract_cell_text(html, "tdid17")?;
    let wealth = extract_cell_text(html, "tdid18")?;
    let active_addresses = extract_cell_text(html, "tdid20")?;
    let largest_transactions = extract_cell_text(html, "tdid21")?;
    let wealth_pcts = extract_percentages(&wealth);
    if wealth_pcts.len() < 4 {
        bail!("failed to parse wealth distribution percentages");
    }

    Ok(OnchainMarketStats {
        date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        top_100_richest_btc: extract_btc_value(&top_100_richest)
            .context("missing top 100 richest BTC value")?,
        top_100_share_pct: extract_percentages(&top_100_richest)
            .into_iter()
            .next()
            .context("missing top 100 share percentage")?,
        top_10_share_pct: wealth_pcts[0],
        top_1000_share_pct: wealth_pcts[2],
        top_10000_share_pct: wealth_pcts[3],
        largest_transactions_24h_btc: extract_btc_value(&largest_transactions)
            .context("missing largest transactions BTC value")?,
        largest_transactions_24h_usd: extract_usd_value(&largest_transactions)
            .context("missing largest transactions USD value")?,
        largest_transactions_24h_share_pct: extract_percentages(&largest_transactions)
            .into_iter()
            .next()
            .context("missing largest transactions share percentage")?,
        active_addresses_24h: extract_u64_value(&active_addresses)
            .context("missing active addresses value")?,
    })
}

fn extract_cell_text(html: &str, id: &str) -> Result<String> {
    let marker = format!("id=\"{}\"", id);
    let idx = html
        .find(&marker)
        .ok_or_else(|| anyhow!("missing {}", id))?;
    let after_id = &html[idx..];
    let start = after_id
        .find('>')
        .ok_or_else(|| anyhow!("missing start tag for {}", id))?;
    let after_start = &after_id[start + 1..];
    let end = after_start
        .find("</td>")
        .ok_or_else(|| anyhow!("missing closing tag for {}", id))?;
    let fragment = &after_start[..end];
    Ok(strip_html(fragment))
}

fn strip_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }

    output
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_exchange_label(text: &str) -> Option<String> {
    let raw_label = text
        .split("wallet:")
        .nth(1)
        .map(|rest| rest.split("Balance:").next().unwrap_or(rest).trim())?;
    let label = raw_label
        .split("7d:")
        .next()
        .unwrap_or(raw_label)
        .split("30d:")
        .next()
        .unwrap_or(raw_label)
        .trim();
    let lowered = label.to_ascii_lowercase();
    const EXCHANGE_KEYWORDS: &[&str] = &[
        "binance",
        "coinbase",
        "robinhood",
        "bitfinex",
        "kraken",
        "okx",
        "huobi",
        "bybit",
        "kucoin",
        "gate",
        "bitstamp",
        "coincheck",
        "upbit",
        "mexc",
        "gemini",
        "bittrex",
        "poloniex",
        "bitflyer",
        "deribit",
        "coldwallet",
        "exchange",
    ];
    if EXCHANGE_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
    {
        Some(label.to_string())
    } else {
        None
    }
}

fn extract_btc_value(text: &str) -> Option<f64> {
    extract_number_before(text, "BTC")
}

fn extract_usd_value(text: &str) -> Option<f64> {
    let start = text.find('$')?;
    let rest = &text[start + 1..];
    let end = rest
        .find(')')
        .or_else(|| rest.find(' '))
        .unwrap_or(rest.len());
    normalize_number(&rest[..end])
}

fn extract_signed_btc_after(text: &str, marker: &str) -> Option<f64> {
    let idx = text.find(marker)?;
    let rest = &text[idx + marker.len()..];
    extract_number_before(rest, "BTC")
}

fn extract_number_before(text: &str, suffix: &str) -> Option<f64> {
    let idx = text.find(suffix)?;
    let prefix = &text[..idx];
    let candidate = prefix
        .split_whitespace()
        .rev()
        .find(|token| token.chars().any(|ch| ch.is_ascii_digit()))?;
    normalize_number(candidate)
}

fn extract_percentages(text: &str) -> Vec<f64> {
    let mut values = Vec::new();
    for segment in text.split('%') {
        if let Some(token) = segment
            .split_whitespace()
            .rev()
            .find(|token| token.chars().any(|ch| ch.is_ascii_digit()))
        {
            if let Some(value) = normalize_number(token) {
                values.push(value);
            }
        }
    }
    values
}

fn extract_u64_value(text: &str) -> Option<u64> {
    let token = text
        .split_whitespace()
        .find(|part| part.chars().any(|ch| ch.is_ascii_digit()))?;
    let normalized = token.replace(',', "");
    normalized.parse().ok()
}

fn normalize_number(token: &str) -> Option<f64> {
    let cleaned = token
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim_end_matches('/')
        .replace(',', "");
    cleaned.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_market_stats_snippet() {
        let html = r#"
        <table>
          <tr><td id="tdid17"><span class="text-success">3,041,532 BTC</span> <span class="text-info">15.21% Total</span></td></tr>
          <tr><td id="tdid18">5.93% / 15.21% / 29.72% / 53.98% Total</td></tr>
          <tr><td id="tdid20">99,641</td></tr>
          <tr><td id="tdid21">last 24h: <span class="text-success">360,862 BTC</span> <span class="text-error">($26,715,631,071)</span> <span class="text-info">224.26% Total</span></td></tr>
        </table>
        "#;

        let stats = parse_market_stats(html).unwrap();
        assert_eq!(stats.top_100_richest_btc, 3_041_532.0);
        assert_eq!(stats.top_100_share_pct, 15.21);
        assert_eq!(stats.top_10_share_pct, 5.93);
        assert_eq!(stats.top_1000_share_pct, 29.72);
        assert_eq!(stats.active_addresses_24h, 99_641);
        assert_eq!(stats.largest_transactions_24h_btc, 360_862.0);
    }

    #[test]
    fn parses_exchange_wallet_tables() {
        let html = r#"
        <table id="tblOne">
          <tr><td>#</td><td>Address</td><td>Balance</td></tr>
          <tr>
            <td>1</td>
            <td>34xp wallet: Binance-coldwallet Balance: 248,598 BTC ($18,366,910,520) Ins: 5497 Outs: 451</td>
            <td>248,598 BTC ($18,366,910,520)</td>
          </tr>
          <tr>
            <td>2</td>
            <td>3M219 wallet: Robinhood-coldwallet 7d: -15703 BTC / 30d: -1332 BTC Balance: 156,027 BTC ($11,527,568,385)</td>
            <td>156,027 BTC ($11,527,568,385)</td>
          </tr>
        </table>
        <table id="tblOne2">
          <tr><td>20</td><td>bc1 wallet: Coincheck 7d: -21.65 BTC / 30d: +45.78 BTC Balance: 42,333 BTC ($3,127,672,862)</td><td>42,333 BTC ($3,127,672,862)</td></tr>
        </table>
        "#;

        let rows = parse_exchange_wallet_rows(html).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].label, "Binance-coldwallet");
        assert_eq!(rows[1].flow_7d_btc, Some(-15_703.0));
        assert_eq!(rows[2].label, "Coincheck");
        assert_eq!(rows[2].flow_30d_btc, Some(45.78));
    }

    #[test]
    fn test_network_metrics_live() {
        // This test hits the real Blockchair API
        // Skip in CI to avoid rate limits
        if std::env::var("CI").is_ok() {
            return;
        }

        let result = fetch_network_metrics();
        if let Ok(metrics) = result {
            assert!(metrics.hash_rate > 0.0);
            assert!(metrics.difficulty > 0.0);
            assert!(metrics.blocks_24h > 0);
        }
        // Don't fail test if API is down
    }

    #[test]
    fn test_extract_exchange_label_filters_non_exchanges() {
        assert_eq!(
            extract_exchange_label("wallet: Binance-coldwallet Balance: 1 BTC"),
            Some("Binance-coldwallet".to_string())
        );
        assert_eq!(
            extract_exchange_label("wallet: UK-Gov-Confiscated Balance: 1 BTC"),
            None
        );
    }

    #[test]
    fn test_etf_flows_placeholder() {
        // Skip network test in CI
        if std::env::var("CI").is_ok() {
            return;
        }

        // This test may succeed (returning empty vec) or fail depending on
        // whether CoinGlass is accessible and page structure matches expectations
        let _result = fetch_etf_flows();
        // Don't assert error - page may load successfully but return empty data
    }

    #[test]
    fn test_whale_transactions_aggregate_shape() {
        let html = r#"
        <table>
          <tr><td id="tdid17">3,041,532 BTC 15.21% Total</td></tr>
          <tr><td id="tdid18">5.93% / 15.21% / 29.72% / 53.98% Total</td></tr>
          <tr><td id="tdid20">99,641</td></tr>
          <tr><td id="tdid21">last 24h: 360,862 BTC ($26,715,631,071) 224.26% Total</td></tr>
        </table>
        "#;
        let stats = parse_market_stats(html).unwrap();
        let tx = WhaleTransaction {
            timestamp: 0,
            amount_btc: stats.largest_transactions_24h_btc,
            amount_usd: stats.largest_transactions_24h_usd,
            from_owner: "100 largest BTC transactions (24h)".to_string(),
            to_owner: format!("active addresses: {}", stats.active_addresses_24h),
            tx_hash: "aggregate-24h-largest-100".to_string(),
        };
        assert_eq!(tx.amount_btc, 360_862.0);
        assert!(tx.tx_hash.starts_with("aggregate"));
    }
}
