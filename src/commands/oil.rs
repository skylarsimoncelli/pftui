use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::db::news_cache;
use crate::db::price_cache::{get_all_cached_prices, upsert_price};
use crate::db::price_history::get_history;
use crate::indicators::compute_rsi;
use crate::price::yahoo;

pub fn run(conn: &Connection, json: bool) -> Result<()> {
    let mut prices = get_all_cached_prices(conn)?
        .into_iter()
        .map(|p| (p.symbol, p.price))
        .collect::<std::collections::HashMap<_, _>>();

    ensure_symbol(conn, &mut prices, "CL=F")?;
    ensure_symbol(conn, &mut prices, "BZ=F")?;

    let wti = prices.get("CL=F").copied();
    let brent = prices.get("BZ=F").copied();
    let spread = match (wti, brent) {
        (Some(w), Some(b)) => Some(w - b),
        _ => None,
    };

    let rsi_wti = compute_symbol_rsi(conn, "CL=F");
    let rsi_brent = compute_symbol_rsi(conn, "BZ=F");
    let headlines = oil_headlines(conn)?;

    if json {
        let out = serde_json::json!({
            "wti": wti.and_then(to_f64),
            "brent": brent.and_then(to_f64),
            "wti_brent_spread": spread.and_then(to_f64),
            "rsi": {
                "wti": rsi_wti,
                "brent": rsi_brent,
            },
            "context": {
                "opec_headlines": headlines.opec,
                "hormuz_headlines": headlines.hormuz,
                "geopolitical_headlines": headlines.geopolitics,
            }
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("\nOil Dashboard\n");
    println!("  WTI (CL=F):   {}", fmt_opt_money(wti));
    println!("  Brent (BZ=F): {}", fmt_opt_money(brent));
    println!("  Spread:       {}", fmt_opt_signed(spread, "$"));
    println!("  RSI(14):      WTI {} | Brent {}", fmt_opt_num(rsi_wti), fmt_opt_num(rsi_brent));
    println!();
    println!("  OPEC+ context:");
    print_headline_list(&headlines.opec);
    println!("  Hormuz context:");
    print_headline_list(&headlines.hormuz);
    println!("  Geopolitical context:");
    print_headline_list(&headlines.geopolitics);
    println!();
    Ok(())
}

fn ensure_symbol(
    conn: &Connection,
    prices: &mut std::collections::HashMap<String, Decimal>,
    symbol: &str,
) -> Result<()> {
    if prices.contains_key(symbol) {
        return Ok(());
    }
    let rt = tokio::runtime::Runtime::new()?;
    if let Ok(quote) = rt.block_on(yahoo::fetch_price(symbol)) {
        upsert_price(conn, &quote)?;
        prices.insert(symbol.to_string(), quote.price);
    }
    Ok(())
}

fn compute_symbol_rsi(conn: &Connection, symbol: &str) -> Option<f64> {
    let history = get_history(conn, symbol, 40).ok()?;
    if history.len() < 15 {
        return None;
    }
    let closes: Vec<f64> = history
        .iter()
        .map(|r| r.close.to_string().parse::<f64>().unwrap_or(0.0))
        .collect();
    let values = compute_rsi(&closes, 14);
    values.last().and_then(|v| *v)
}

struct OilHeadlines {
    opec: Vec<String>,
    hormuz: Vec<String>,
    geopolitics: Vec<String>,
}

fn oil_headlines(conn: &Connection) -> Result<OilHeadlines> {
    let items = news_cache::get_latest_news(conn, 30, None, None, None, Some(72))?;
    let mut opec = Vec::new();
    let mut hormuz = Vec::new();
    let mut geopolitics = Vec::new();

    for n in items {
        let title = n.title.to_lowercase();
        if (title.contains("opec") || title.contains("saudi")) && opec.len() < 3 {
            opec.push(n.title.clone());
        }
        if (title.contains("hormuz") || title.contains("strait")) && hormuz.len() < 3 {
            hormuz.push(n.title.clone());
        }
        if (title.contains("iran")
            || title.contains("russia")
            || title.contains("ukraine")
            || title.contains("middle east"))
            && geopolitics.len() < 3
        {
            geopolitics.push(n.title.clone());
        }
    }
    Ok(OilHeadlines {
        opec,
        hormuz,
        geopolitics,
    })
}

fn print_headline_list(items: &[String]) {
    if items.is_empty() {
        println!("    - none in cache");
        return;
    }
    for h in items {
        println!("    - {}", h);
    }
}

fn to_f64(v: Decimal) -> Option<f64> {
    v.to_string().parse::<f64>().ok()
}

fn fmt_opt_money(v: Option<Decimal>) -> String {
    v.map(|d| format!("${:.2}", d))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_opt_signed(v: Option<Decimal>, prefix: &str) -> String {
    v.map(|d| {
        let sign = if d >= Decimal::ZERO { "+" } else { "" };
        format!("{}{}{}", sign, prefix, d.round_dp(2))
    })
    .unwrap_or_else(|| "-".to_string())
}

fn fmt_opt_num(v: Option<f64>) -> String {
    v.map(|x| format!("{:.1}", x))
        .unwrap_or_else(|| "-".to_string())
}
