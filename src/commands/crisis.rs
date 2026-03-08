use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::db::news_cache;
use crate::db::price_cache::{get_all_cached_prices, upsert_price};
use crate::db::price_history::get_history;
use crate::price::yahoo;

const REQUIRED: &[&str] = &[
    "CL=F",   // WTI
    "BZ=F",   // Brent
    "^VIX",   // Volatility
    "GC=F",   // Gold
    "DX-Y.NYB", // DXY
    "JPY=X",  // USDJPY
    "ITA",    // Defense ETF
    "LMT",
    "RTX",
    "PLTR",
];

pub fn run(conn: &Connection, json: bool) -> Result<()> {
    let mut prices = get_all_cached_prices(conn)?
        .into_iter()
        .map(|p| (p.symbol, p.price))
        .collect::<std::collections::HashMap<_, _>>();
    backfill_missing(conn, &mut prices)?;

    let spread = match (prices.get("CL=F"), prices.get("BZ=F")) {
        (Some(wti), Some(brent)) => Some(*wti - *brent),
        _ => None,
    };

    let vix = prices.get("^VIX").copied();
    let crisis_regime = match vix {
        Some(v) if v >= Decimal::from(30) => "high_fear",
        Some(v) if v >= Decimal::from(20) => "elevated",
        Some(_) => "normal",
        None => "unknown",
    };

    let defense = vec![
        metric(conn, &prices, "ITA"),
        metric(conn, &prices, "LMT"),
        metric(conn, &prices, "RTX"),
        metric(conn, &prices, "PLTR"),
    ];
    let safe_havens = vec![
        metric(conn, &prices, "GC=F"),
        metric(conn, &prices, "DX-Y.NYB"),
        metric(conn, &prices, "JPY=X"),
    ];

    let headlines = crisis_headlines(conn)?;

    if json {
        let out = serde_json::json!({
            "oil": {
                "wti": prices.get("CL=F").copied().and_then(to_f64),
                "brent": prices.get("BZ=F").copied().and_then(to_f64),
                "spread": spread.and_then(to_f64),
            },
            "vix": vix.and_then(to_f64),
            "regime": crisis_regime,
            "defense": defense,
            "safe_havens": safe_havens,
            "headlines": headlines,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("\nCrisis Dashboard\n");
    println!("Regime: {}", crisis_regime);
    println!(
        "Oil: WTI {} | Brent {} | Spread {}",
        fmt_money(prices.get("CL=F").copied()),
        fmt_money(prices.get("BZ=F").copied()),
        fmt_signed(spread)
    );
    println!("VIX:  {}", fmt_money(vix));
    println!();
    println!("Defense:");
    print_metric_rows(&defense);
    println!("Safe Havens:");
    print_metric_rows(&safe_havens);
    println!("Context:");
    print_topic("Oil/Shipping", &headlines.oil_shipping);
    print_topic("Geopolitics", &headlines.geopolitics);
    print_topic("Defense", &headlines.defense);
    println!();
    Ok(())
}

#[derive(serde::Serialize)]
struct Metric {
    symbol: String,
    price: Option<f64>,
    day_change_pct: Option<f64>,
}

fn metric(
    conn: &Connection,
    prices: &std::collections::HashMap<String, Decimal>,
    symbol: &str,
) -> Metric {
    let price = prices.get(symbol).copied().and_then(to_f64);
    let day_change_pct = day_change_pct(conn, symbol);
    Metric {
        symbol: symbol.to_string(),
        price,
        day_change_pct,
    }
}

fn day_change_pct(conn: &Connection, symbol: &str) -> Option<f64> {
    let history = get_history(conn, symbol, 2).ok()?;
    if history.len() < 2 {
        return None;
    }
    let y = history[history.len() - 2].close.to_string().parse::<f64>().ok()?;
    let p = history[history.len() - 1].close.to_string().parse::<f64>().ok()?;
    if y == 0.0 {
        return None;
    }
    Some(((p - y) / y) * 100.0)
}

fn backfill_missing(
    conn: &Connection,
    prices: &mut std::collections::HashMap<String, Decimal>,
) -> Result<()> {
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|s| !prices.contains_key(*s))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let rt = tokio::runtime::Runtime::new()?;
    for symbol in missing {
        if let Ok(q) = rt.block_on(yahoo::fetch_price(symbol)) {
            upsert_price(conn, &q)?;
            prices.insert(symbol.to_string(), q.price);
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct CrisisHeadlines {
    oil_shipping: Vec<String>,
    geopolitics: Vec<String>,
    defense: Vec<String>,
}

fn crisis_headlines(conn: &Connection) -> Result<CrisisHeadlines> {
    let items = news_cache::get_latest_news(conn, 40, None, None, None, Some(72))?;
    let mut oil_shipping = Vec::new();
    let mut geopolitics = Vec::new();
    let mut defense = Vec::new();
    for n in items {
        let t = n.title.to_lowercase();
        if (t.contains("oil") || t.contains("shipping") || t.contains("freight"))
            && oil_shipping.len() < 4
        {
            oil_shipping.push(n.title.clone());
        }
        if (t.contains("iran")
            || t.contains("russia")
            || t.contains("ukraine")
            || t.contains("middle east")
            || t.contains("war"))
            && geopolitics.len() < 4
        {
            geopolitics.push(n.title.clone());
        }
        if (t.contains("defense")
            || t.contains("lockheed")
            || t.contains("raytheon")
            || t.contains("palantir"))
            && defense.len() < 4
        {
            defense.push(n.title.clone());
        }
    }
    Ok(CrisisHeadlines {
        oil_shipping,
        geopolitics,
        defense,
    })
}

fn print_metric_rows(metrics: &[Metric]) {
    for m in metrics {
        let p = m
            .price
            .map(|v| format!("${:.2}", v))
            .unwrap_or_else(|| "-".to_string());
        let chg = m
            .day_change_pct
            .map(|v| format!("{:+.2}%", v))
            .unwrap_or_else(|| "-".to_string());
        println!("  {:<10} {:>10}  {:>8}", m.symbol, p, chg);
    }
}

fn print_topic(label: &str, items: &[String]) {
    println!("  {}:", label);
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

fn fmt_money(v: Option<Decimal>) -> String {
    v.map(|d| format!("${:.2}", d))
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_signed(v: Option<Decimal>) -> String {
    v.map(|d| {
        let sign = if d >= Decimal::ZERO { "+" } else { "" };
        format!("{}${}", sign, d.round_dp(2))
    })
    .unwrap_or_else(|| "-".to_string())
}
