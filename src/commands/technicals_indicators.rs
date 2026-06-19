//! `analytics technicals indicators <SYM>` — a full standard-indicator panel
//! computed on the fly (momentum / trend / volume / volatility), with an
//! at-a-glance bull/bear scorecard. Complements the cached `technicals`
//! snapshot with the broader TA-Lib-standard set.

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;
use crate::indicators::{
    bollinger, compute_adx, compute_cci, compute_macd, compute_mfi, compute_obv, compute_rsi,
    compute_roc, compute_sma, compute_stochastic, compute_williams_r,
};
use crate::indicators::atr::compute_atr;

fn last<T: Copy>(v: &[Option<T>]) -> Option<T> {
    v.iter().rev().find_map(|x| *x)
}

pub fn run(backend: &BackendConnection, symbol: &str, json_output: bool) -> Result<()> {
    let resolved = resolve_alias(symbol);
    let hist = price_history::get_history(backend.sqlite(), &resolved, u32::MAX)?;
    if hist.len() < 30 {
        bail!("not enough price history for '{symbol}' (resolved '{resolved}') — need ≥30 bars");
    }
    let as_of = hist.last().map(|h| h.date.clone()).unwrap_or_default();
    // f64 OHLCV (close substitutes for a missing high/low; 0 for missing volume).
    let closes: Vec<f64> = hist.iter().map(|h| h.close.to_f64().unwrap_or(0.0)).collect();
    let highs: Vec<f64> = hist
        .iter()
        .map(|h| h.high.and_then(|d| d.to_f64()).unwrap_or_else(|| h.close.to_f64().unwrap_or(0.0)))
        .collect();
    let lows: Vec<f64> = hist
        .iter()
        .map(|h| h.low.and_then(|d| d.to_f64()).unwrap_or_else(|| h.close.to_f64().unwrap_or(0.0)))
        .collect();
    let highs_opt: Vec<Option<f64>> = highs.iter().map(|v| Some(*v)).collect();
    let lows_opt: Vec<Option<f64>> = lows.iter().map(|v| Some(*v)).collect();
    let vols: Vec<f64> = hist.iter().map(|h| h.volume.map(|v| v as f64).unwrap_or(0.0)).collect();
    let has_volume = vols.iter().any(|v| *v > 0.0);

    // Compute.
    let rsi = last(&compute_rsi(&closes, 14));
    let stoch = last(&compute_stochastic(&highs, &lows, &closes, 14, 3));
    let willr = last(&compute_williams_r(&highs, &lows, &closes, 14));
    let cci = last(&compute_cci(&highs, &lows, &closes, 20));
    let roc = last(&compute_roc(&closes, 10));
    let adx = last(&compute_adx(&highs, &lows, &closes, 14));
    let macd = last(&compute_macd(&closes, 12, 26, 9));
    let sma50 = last(&compute_sma(&closes, 50));
    let sma200 = last(&compute_sma(&closes, 200));
    let atr = last(&compute_atr(&highs_opt, &lows_opt, &closes, 14));
    let bb = last(&bollinger::compute_bollinger(&closes, 20, 2.0));
    let price = *closes.last().unwrap();
    let obv_series = compute_obv(&closes, &vols);
    // OBV trend: last vs 20 bars ago.
    let obv_trend = if has_volume && obv_series.len() > 20 {
        match (last(&obv_series), obv_series[obv_series.len() - 21]) {
            (Some(now), Some(prev)) => Some(now - prev),
            _ => None,
        }
    } else {
        None
    };
    let mfi = if has_volume {
        last(&compute_mfi(&highs, &lows, &closes, &vols, 14))
    } else {
        None
    };

    // Bull/bear scorecard from the canonical thresholds.
    let mut bull = 0i32;
    let mut bear = 0i32;
    let mut tally = |cond_bull: bool, cond_bear: bool| {
        if cond_bull {
            bull += 1;
        }
        if cond_bear {
            bear += 1;
        }
    };
    if let Some(r) = rsi {
        tally(r < 30.0, r > 70.0);
    }
    if let Some(s) = stoch {
        tally(s.k < 20.0, s.k > 80.0);
    }
    if let Some(w) = willr {
        tally(w < -80.0, w > -20.0);
    }
    if let Some(c) = cci {
        tally(c < -100.0, c > 100.0);
    }
    if let Some(a) = adx {
        // Directional bias only counts when the trend is real (ADX > 20).
        tally(a.adx > 20.0 && a.plus_di > a.minus_di, a.adx > 20.0 && a.minus_di > a.plus_di);
    }
    if let Some(m) = &macd {
        tally(m.histogram > 0.0, m.histogram < 0.0);
    }
    if let (Some(s50), Some(s200)) = (sma50, sma200) {
        tally(price > s50 && s50 > s200, price < s50 && s50 < s200);
    }
    if let Some(f) = mfi {
        tally(f < 20.0, f > 80.0);
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "technicals indicators",
                "asset": symbol,
                "resolved_symbol": resolved,
                "as_of": as_of,
                "price": price,
                "momentum": {
                    "rsi_14": rsi,
                    "stoch_k": stoch.map(|s| s.k),
                    "stoch_d": stoch.map(|s| s.d),
                    "williams_r_14": willr,
                    "cci_20": cci,
                    "roc_10_pct": roc,
                },
                "trend": {
                    "adx_14": adx.map(|a| a.adx),
                    "plus_di": adx.map(|a| a.plus_di),
                    "minus_di": adx.map(|a| a.minus_di),
                    "macd_hist": macd.as_ref().map(|m| m.histogram),
                    "sma_50": sma50,
                    "sma_200": sma200,
                },
                "volume": {
                    "obv_20bar_change": obv_trend,
                    "mfi_14": mfi,
                    "has_volume": has_volume,
                },
                "volatility": {
                    "atr_14": atr,
                    "bb_pct_b": bb.map(|b| if b.upper > b.lower { (price - b.lower) / (b.upper - b.lower) } else { 0.5 }),
                },
                "scorecard": { "bullish": bull, "bearish": bear },
            }))?
        );
        return Ok(());
    }

    let f = |o: Option<f64>, dp: usize| o.map(|v| format!("{v:.*}", dp)).unwrap_or_else(|| "—".into());
    let tag = |v: Option<f64>, lo: f64, hi: f64| match v {
        Some(x) if x < lo => " (oversold)",
        Some(x) if x > hi => " (overbought)",
        _ => "",
    };
    println!("═══ Indicator Panel — {} ({}) ═══", symbol, resolved);
    println!("As of {as_of} · price {price:.2}\n");
    println!(
        "Momentum:   RSI(14) {}{} | Stoch %K {} %D {} | Williams%R {} | CCI(20) {} | ROC(10) {}%",
        f(rsi, 1),
        tag(rsi, 30.0, 70.0),
        f(stoch.map(|s| s.k), 1),
        f(stoch.map(|s| s.d), 1),
        f(willr, 0),
        f(cci, 0),
        f(roc, 1),
    );
    let adx_str = adx
        .map(|a| {
            let strength = if a.adx > 25.0 { "strong" } else if a.adx > 20.0 { "trending" } else { "ranging" };
            let dir = if a.plus_di > a.minus_di { "bull" } else { "bear" };
            format!("ADX {:.0} ({strength}) | +DI {:.0}/-DI {:.0} ({dir})", a.adx, a.plus_di, a.minus_di)
        })
        .unwrap_or_else(|| "ADX —".into());
    println!(
        "Trend:      {} | MACD hist {} | SMA50 {} / SMA200 {}",
        adx_str,
        f(macd.as_ref().map(|m| m.histogram), 1),
        f(sma50, 0),
        f(sma200, 0),
    );
    if has_volume {
        let obv_dir = obv_trend.map(|d| if d > 0.0 { "↑ rising" } else if d < 0.0 { "↓ falling" } else { "flat" }).unwrap_or("—");
        println!("Volume:     OBV(20b) {} | MFI(14) {}{}", obv_dir, f(mfi, 0), tag(mfi, 20.0, 80.0));
    } else {
        println!("Volume:     (no volume data for this series)");
    }
    let bbpct = bb.map(|b| if b.upper > b.lower { (price - b.lower) / (b.upper - b.lower) * 100.0 } else { 50.0 });
    println!("Volatility: ATR(14) {} | Bollinger %b {}", f(atr, 2), f(bbpct, 0));
    println!();
    let verdict = if bull > bear + 1 {
        "net BULLISH"
    } else if bear > bull + 1 {
        "net BEARISH"
    } else {
        "mixed / neutral"
    };
    println!("Scorecard:  {bull} bullish · {bear} bearish → {verdict}");
    Ok(())
}
