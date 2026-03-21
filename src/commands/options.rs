//! `pftui options` — Yahoo Finance options chain viewer.

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use serde_json::Value;

#[derive(Debug, Clone)]
struct OptionContract {
    strike: f64,
    last: f64,
    bid: f64,
    ask: f64,
    volume: i64,
    open_interest: i64,
    implied_vol: f64,
    in_the_money: bool,
}

#[derive(Debug, Clone)]
struct OptionsChain {
    symbol: String,
    underlying_price: f64,
    selected_expiry_ts: i64,
    expirations: Vec<i64>,
    calls: Vec<OptionContract>,
    puts: Vec<OptionContract>,
}

fn parse_expiry_to_timestamp(input: &str) -> Result<i64> {
    let date = NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid expiry '{}'. Use YYYY-MM-DD.", input))?;
    let dt = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow!("Invalid expiry '{}'.", input))?;
    Ok(dt.and_utc().timestamp())
}

fn parse_contracts(raw: Option<&Value>) -> Vec<OptionContract> {
    raw.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|c| OptionContract {
                    strike: c.get("strike").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    last: c.get("lastPrice").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    bid: c.get("bid").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    ask: c.get("ask").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    volume: c.get("volume").and_then(|v| v.as_i64()).unwrap_or(0),
                    open_interest: c.get("openInterest").and_then(|v| v.as_i64()).unwrap_or(0),
                    implied_vol: c
                        .get("impliedVolatility")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0)
                        * 100.0,
                    in_the_money: c
                        .get("inTheMoney")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_chain(symbol: &str, json: &Value) -> Result<OptionsChain> {
    let result = json
        .get("optionChain")
        .and_then(|v| v.get("result"))
        .and_then(|v| v.get(0))
        .ok_or_else(|| anyhow!("No options chain returned for {}", symbol))?;

    let quote = result
        .get("quote")
        .ok_or_else(|| anyhow!("Missing quote payload for {}", symbol))?;

    let underlying_price = quote
        .get("regularMarketPrice")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let expirations: Vec<i64> = result
        .get("expirationDates")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let options_obj = result
        .get("options")
        .and_then(|v| v.get(0))
        .ok_or_else(|| anyhow!("Missing option contracts for {}", symbol))?;

    let selected_expiry_ts = options_obj
        .get("expirationDate")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| expirations.first().copied().unwrap_or(0));

    Ok(OptionsChain {
        symbol: symbol.to_uppercase(),
        underlying_price,
        selected_expiry_ts,
        expirations,
        calls: parse_contracts(options_obj.get("calls")),
        puts: parse_contracts(options_obj.get("puts")),
    })
}

async fn fetch_chain(symbol: &str, expiry_ts: Option<i64>) -> Result<OptionsChain> {
    let url = if let Some(ts) = expiry_ts {
        format!(
            "https://query2.finance.yahoo.com/v7/finance/options/{}?date={}",
            symbol, ts
        )
    } else {
        format!(
            "https://query2.finance.yahoo.com/v7/finance/options/{}",
            symbol
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("Yahoo options endpoint returned {}", resp.status());
    }
    let json: Value = resp.json().await?;
    parse_chain(symbol, &json)
}

fn fmt_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn print_side(title: &str, items: &[OptionContract], limit: usize, spot: f64) {
    let mut selected: Vec<OptionContract> = items.to_vec();
    selected.sort_by(|a, b| {
        let da = (a.strike - spot).abs();
        let db = (b.strike - spot).abs();
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    selected.truncate(limit);
    selected.sort_by(|a, b| {
        a.strike
            .partial_cmp(&b.strike)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("{}", title);
    println!("┌────────┬────────┬────────┬────────┬──────────┬──────────┬────────┬──────┐");
    println!("│ Strike │  Last  │  Bid   │  Ask   │   Vol    │    OI    │  IV%   │ ITM? │");
    println!("├────────┼────────┼────────┼────────┼──────────┼──────────┼────────┼──────┤");
    for c in selected {
        println!(
            "│ {:>6.2} │ {:>6.2} │ {:>6.2} │ {:>6.2} │ {:>8} │ {:>8} │ {:>6.1} │ {:<4} │",
            c.strike,
            c.last,
            c.bid,
            c.ask,
            c.volume,
            c.open_interest,
            c.implied_vol,
            if c.in_the_money { "yes" } else { "no" }
        );
    }
    println!("└────────┴────────┴────────┴────────┴──────────┴──────────┴────────┴──────┘\n");
}

fn print_terminal(chain: &OptionsChain, limit: usize) {
    println!();
    println!(
        "OPTIONS CHAIN {}  spot ${:.2}  expiry {}",
        chain.symbol,
        chain.underlying_price,
        fmt_ts(chain.selected_expiry_ts)
    );
    println!(
        "Available expiries: {}",
        chain
            .expirations
            .iter()
            .take(8)
            .map(|ts| fmt_ts(*ts))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if chain.expirations.len() > 8 {
        println!("(showing first 8; use --expiry YYYY-MM-DD to select another)");
    }
    println!();

    print_side(
        "Calls (closest strikes to spot)",
        &chain.calls,
        limit,
        chain.underlying_price,
    );
    print_side(
        "Puts  (closest strikes to spot)",
        &chain.puts,
        limit,
        chain.underlying_price,
    );
}

fn print_json(chain: &OptionsChain, limit: usize) -> Result<()> {
    let mut calls = chain.calls.clone();
    calls.sort_by(|a, b| {
        (a.strike - chain.underlying_price)
            .abs()
            .partial_cmp(&(b.strike - chain.underlying_price).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    calls.truncate(limit);

    let mut puts = chain.puts.clone();
    puts.sort_by(|a, b| {
        (a.strike - chain.underlying_price)
            .abs()
            .partial_cmp(&(b.strike - chain.underlying_price).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    puts.truncate(limit);

    let json = serde_json::json!({
        "symbol": chain.symbol,
        "underlying_price": chain.underlying_price,
        "selected_expiry": fmt_ts(chain.selected_expiry_ts),
        "available_expiries": chain.expirations.iter().map(|ts| fmt_ts(*ts)).collect::<Vec<_>>(),
        "calls": calls.iter().map(|c| serde_json::json!({
            "strike": c.strike,
            "last": c.last,
            "bid": c.bid,
            "ask": c.ask,
            "volume": c.volume,
            "open_interest": c.open_interest,
            "implied_vol_pct": c.implied_vol,
            "in_the_money": c.in_the_money,
        })).collect::<Vec<_>>(),
        "puts": puts.iter().map(|c| serde_json::json!({
            "strike": c.strike,
            "last": c.last,
            "bid": c.bid,
            "ask": c.ask,
            "volume": c.volume,
            "open_interest": c.open_interest,
            "implied_vol_pct": c.implied_vol,
            "in_the_money": c.in_the_money,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

pub fn run(symbol: &str, expiry: Option<&str>, limit: usize, json: bool) -> Result<()> {
    let expiry_ts = match expiry {
        Some(v) => Some(parse_expiry_to_timestamp(v)?),
        None => None,
    };

    let rt = tokio::runtime::Runtime::new()?;
    let chain = rt.block_on(fetch_chain(symbol, expiry_ts))?;

    if json {
        print_json(&chain, limit)?;
    } else {
        print_terminal(&chain, limit.max(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_expiry_input() {
        assert_eq!(parse_expiry_to_timestamp("2026-12-18").unwrap(), 1797552000);
        assert!(parse_expiry_to_timestamp("12/18/2026").is_err());
    }

    #[test]
    fn parse_chain_extracts_core_fields() {
        let payload = serde_json::json!({
          "optionChain": {
            "result": [{
              "quote": {"regularMarketPrice": 215.5},
              "expirationDates": [1797552000],
              "options": [{
                "expirationDate": 1797552000,
                "calls": [{"strike": 215.0, "lastPrice": 4.1, "bid": 4.0, "ask": 4.2, "volume": 100, "openInterest": 200, "impliedVolatility": 0.25, "inTheMoney": true}],
                "puts": [{"strike": 215.0, "lastPrice": 3.8, "bid": 3.7, "ask": 3.9, "volume": 90, "openInterest": 180, "impliedVolatility": 0.27, "inTheMoney": false}]
              }]
            }]
          }
        });

        let chain = parse_chain("AAPL", &payload).unwrap();
        assert_eq!(chain.symbol, "AAPL");
        assert_eq!(chain.underlying_price, 215.5);
        assert_eq!(chain.calls.len(), 1);
        assert_eq!(chain.puts.len(), 1);
        assert_eq!(chain.calls[0].implied_vol, 25.0);
    }
}
