//! `pftui data options` — Yahoo Finance options chain viewer + GEX ingestion.
//!
//! Subcommands:
//!   refresh — fetch chain(s) + persist + compute GEX (calls into `data::options`)
//!   show    — read latest cached chain from SQLite
//!   view    — legacy live-fetch viewer (kept for ad-hoc inspection)

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use serde_json::Value;

use crate::data::options::{
    compute_gex, fetch_options_chain, GexSummary, OptionsStrikeRow, DEFAULT_OPTIONS_SYMBOLS,
};
use crate::db::backend::BackendConnection;
use crate::db::{gex_snapshots, options_chain_snapshots};

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

/// Legacy live-fetch viewer (used by `data options view`).
pub fn run_view(symbol: &str, expiry: Option<&str>, limit: usize, json: bool) -> Result<()> {
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

/// `data options refresh` — fetch chain(s) from Yahoo, persist
/// per-strike snapshot rows, compute GEX summary, persist the
/// summary. Returns a JSON result when `json` is true.
pub fn run_refresh(
    backend: &BackendConnection,
    symbol: Option<&str>,
    all: bool,
    json: bool,
) -> Result<()> {
    let symbols: Vec<String> = if all {
        DEFAULT_OPTIONS_SYMBOLS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else if let Some(s) = symbol {
        vec![s.to_uppercase()]
    } else {
        DEFAULT_OPTIONS_SYMBOLS
            .iter()
            .map(|s| s.to_string())
            .collect()
    };

    let rt = tokio::runtime::Runtime::new()?;
    let mut per_symbol: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<(String, String)> = Vec::new();

    for sym in &symbols {
        match rt.block_on(fetch_options_chain(sym)) {
            Ok(snapshot) => {
                let gex = compute_gex(&snapshot);
                if let Some(conn) = backend.sqlite_native() {
                    options_chain_snapshots::insert_chain(
                        conn,
                        &snapshot.rows,
                        &snapshot.fetched_at,
                    )?;
                    gex_snapshots::insert(conn, &gex)?;
                }
                per_symbol.push(serde_json::json!({
                    "symbol": sym,
                    "spot": snapshot.spot,
                    "expiry": snapshot.expiry,
                    "rows": snapshot.rows.len(),
                    "gex_flip_strike": gex.gex_flip_strike,
                    "max_pain": gex.max_pain,
                    "total_gamma_call": gex.total_gamma_call,
                    "total_gamma_put": gex.total_gamma_put,
                }));
            }
            Err(e) => {
                errors.push((sym.clone(), e.to_string()));
            }
        }
    }

    // BTC reminder per scope item (3) in TODO: Yahoo doesn't have BTC
    // options; surface a hint when BTC is in the held universe.
    let btc_held = backend.sqlite_native().is_some_and(|conn| {
        conn.query_row(
            "SELECT 1 FROM transactions WHERE symbol = 'BTC' LIMIT 1",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false)
    });

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "refreshed": per_symbol,
                "errors": errors.iter().map(|(s, e)| serde_json::json!({"symbol": s, "error": e})).collect::<Vec<_>>(),
                "btc_hint": if btc_held { Some("BTC options not on Yahoo; deribit provider TBD") } else { None },
            }))?
        );
    } else {
        println!("Refreshed {} chain(s):", per_symbol.len());
        for entry in &per_symbol {
            println!(
                "  {:<5} spot ${:.2} expiry {} rows={} flip={} max_pain={}",
                entry["symbol"].as_str().unwrap_or("?"),
                entry["spot"].as_f64().unwrap_or(0.0),
                entry["expiry"].as_str().unwrap_or("?"),
                entry["rows"].as_i64().unwrap_or(0),
                entry["gex_flip_strike"]
                    .as_f64()
                    .map(|v| format!("${:.2}", v))
                    .unwrap_or_else(|| "n/a".into()),
                entry["max_pain"]
                    .as_f64()
                    .map(|v| format!("${:.2}", v))
                    .unwrap_or_else(|| "n/a".into()),
            );
        }
        for (sym, err) in &errors {
            eprintln!("  {} failed: {}", sym, err);
        }
        if btc_held {
            println!("Hint: BTC options not available on Yahoo; deribit provider TBD");
        }
    }
    Ok(())
}

/// `data options show` — read the most-recent cached chain from
/// SQLite (no network).
pub fn run_show(
    backend: &BackendConnection,
    symbol: &str,
    limit: usize,
    json: bool,
) -> Result<()> {
    let Some(conn) = backend.sqlite_native() else {
        bail!("`data options show` requires the SQLite backend");
    };
    let upper = symbol.to_uppercase();
    let rows = options_chain_snapshots::latest_chain(conn, &upper)?;
    let fetched_at = options_chain_snapshots::latest_fetched_at(conn, &upper)?;
    let gex = gex_snapshots::latest(conn, &upper)?;

    if rows.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "symbol": upper,
                    "rows": [],
                    "note": "no cached chain — run `pftui data options refresh --symbol <s>`"
                }))?
            );
        } else {
            println!(
                "No cached options chain for {}. Run `pftui data options refresh --symbol {}`.",
                upper, upper
            );
        }
        return Ok(());
    }

    let trimmed = trim_around_atm(&rows, gex.as_ref(), limit);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "symbol": upper,
                "fetched_at": fetched_at,
                "gex": gex.as_ref().map(gex_to_value),
                "rows": trimmed.iter().map(strike_to_value).collect::<Vec<_>>(),
            }))?
        );
    } else {
        println!(
            "OPTIONS CHAIN (cached) {}  fetched {}",
            upper,
            fetched_at.as_deref().unwrap_or("unknown")
        );
        if let Some(g) = &gex {
            println!(
                "  GEX flip {}  max pain {}  net gamma call {:.0} put {:.0}",
                g.gex_flip_strike
                    .map(|v| format!("${:.2}", v))
                    .unwrap_or_else(|| "n/a".into()),
                g.max_pain
                    .map(|v| format!("${:.2}", v))
                    .unwrap_or_else(|| "n/a".into()),
                g.total_gamma_call,
                g.total_gamma_put,
            );
        }
        println!(
            "  strike  oi_calls   oi_puts  vol_calls  vol_puts  iv_atm    expiry  dte"
        );
        for r in &trimmed {
            println!(
                "  {:>6.2}  {:>8}  {:>8}  {:>9}  {:>8}  {:>6.3}  {}  {}",
                r.strike,
                r.oi_calls,
                r.oi_puts,
                r.vol_calls,
                r.vol_puts,
                r.iv_call.unwrap_or(0.0),
                r.expiry,
                r.dte,
            );
        }
    }
    Ok(())
}

fn trim_around_atm(
    rows: &[OptionsStrikeRow],
    gex: Option<&GexSummary>,
    limit: usize,
) -> Vec<OptionsStrikeRow> {
    if rows.is_empty() || limit == 0 {
        return rows.to_vec();
    }
    // Center on flip strike if known; otherwise use median strike.
    let center = gex
        .and_then(|g| g.gex_flip_strike)
        .unwrap_or_else(|| rows[rows.len() / 2].strike);
    let mut sorted: Vec<OptionsStrikeRow> = rows.to_vec();
    sorted.sort_by(|a, b| {
        (a.strike - center)
            .abs()
            .partial_cmp(&(b.strike - center).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.truncate(limit.max(1) * 2);
    sorted.sort_by(|a, b| {
        a.strike
            .partial_cmp(&b.strike)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
}

fn strike_to_value(r: &OptionsStrikeRow) -> serde_json::Value {
    serde_json::json!({
        "symbol": r.symbol,
        "strike": r.strike,
        "expiry": r.expiry,
        "dte": r.dte,
        "oi_calls": r.oi_calls,
        "oi_puts": r.oi_puts,
        "vol_calls": r.vol_calls,
        "vol_puts": r.vol_puts,
        "iv_call": r.iv_call,
    })
}

fn gex_to_value(g: &GexSummary) -> serde_json::Value {
    serde_json::json!({
        "symbol": g.symbol,
        "gex_flip_strike": g.gex_flip_strike,
        "total_gamma_call": g.total_gamma_call,
        "total_gamma_put": g.total_gamma_put,
        "max_pain": g.max_pain,
        "fetched_at": g.fetched_at,
        "gamma_neutral_zone": g.gamma_neutral_zone().map(|(lo, hi)| serde_json::json!([lo, hi])),
    })
}

/// `analytics gex` — read the most-recent GEX summary from SQLite.
pub fn run_analytics_gex(backend: &BackendConnection, symbol: &str, json: bool) -> Result<()> {
    let Some(conn) = backend.sqlite_native() else {
        bail!("`analytics gex` requires the SQLite backend");
    };
    let upper = symbol.to_uppercase();
    let gex = gex_snapshots::latest(conn, &upper)?;

    let Some(g) = gex else {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "symbol": upper,
                    "available": false,
                    "note": "no cached GEX — run `pftui data options refresh --symbol <s>`"
                }))?
            );
        } else {
            println!(
                "No cached GEX for {}. Run `pftui data options refresh --symbol {}`.",
                upper, upper
            );
        }
        return Ok(());
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&gex_to_value(&g))?);
    } else {
        let zone = g.gamma_neutral_zone();
        println!("GEX snapshot for {} (asof {})", g.symbol, g.fetched_at);
        println!(
            "  flip strike: {}",
            g.gex_flip_strike
                .map(|v| format!("${:.2}", v))
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  max pain:    {}",
            g.max_pain
                .map(|v| format!("${:.2}", v))
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  total gamma: call {:.0}  put {:.0}  net {:.0}",
            g.total_gamma_call,
            g.total_gamma_put,
            g.total_gamma_call - g.total_gamma_put
        );
        if let Some((lo, hi)) = zone {
            println!("  gamma-neutral zone (5%): ${:.2} – ${:.2}", lo, hi);
        } else {
            println!("  gamma-neutral zone (5%): n/a");
        }
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
