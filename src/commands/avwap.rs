//! `analytics avwap --asset SYM [--anchor cycle-low|halving|ath] [--anchor-date D]`
//! — anchored VWAP from a cycle low (or halving / ATH), with the price's
//! position relative to it as an accumulation/basis read.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use serde_json::json;

use crate::analytics::cycle_clock::{btc_cycle_clock, gold_cycle_clock, BTC_HALVING_2024};
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;
use crate::indicators::anchored_vwap::{anchored_vwap, AvwapQuality};

/// Resolve a named/dated anchor to a bar index in `hist` (oldest→newest).
fn resolve_anchor(
    resolved: &str,
    hist: &[crate::models::price::HistoryRecord],
    anchor: &str,
    anchor_date: Option<&str>,
) -> Result<(usize, String)> {
    // Explicit date wins: first bar on/after it.
    if let Some(d) = anchor_date {
        let idx = hist
            .iter()
            .position(|b| b.date.as_str() >= d)
            .ok_or_else(|| anyhow::anyhow!("no bar on/after anchor-date {d}"))?;
        return Ok((idx, format!("explicit date {d}")));
    }
    let find_date = |target: &str| hist.iter().position(|b| b.date.as_str() >= target);
    match anchor {
        "ath" => {
            let idx = hist
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.close.cmp(&b.1.close))
                .map(|(i, _)| i)
                .ok_or_else(|| anyhow::anyhow!("empty history"))?;
            Ok((idx, "all-time-high close".into()))
        }
        "halving" => {
            if resolved != "BTC-USD" {
                bail!("--anchor halving is BTC-only (got {resolved}); use cycle-low, ath, or --anchor-date");
            }
            let idx = find_date(BTC_HALVING_2024)
                .ok_or_else(|| anyhow::anyhow!("no bar on/after the 2024 halving"))?;
            Ok((idx, format!("2024 halving ({BTC_HALVING_2024})")))
        }
        "cycle-low" => {
            // Prefer the verified cycle-low anchor from the cycle clock; fall
            // back to the lowest close over the trailing ~2 years.
            let verified = if resolved == "BTC-USD" {
                btc_cycle_clock(resolved, hist).and_then(|c| c.cycle_low_anchor.verified_date)
            } else if resolved == "GC=F" {
                gold_cycle_clock(resolved, hist).and_then(|c| c.last_cycle_low_date)
            } else {
                None
            };
            if let Some(d) = verified {
                if let Some(idx) = find_date(&d) {
                    return Ok((idx, format!("verified cycle low {d}")));
                }
            }
            // Generic fallback: lowest close in the trailing 730 bars.
            let start = hist.len().saturating_sub(730);
            let idx = (start..hist.len())
                .min_by(|&a, &b| hist[a].close.cmp(&hist[b].close))
                .ok_or_else(|| anyhow::anyhow!("empty history"))?;
            Ok((idx, format!("trailing-2y low ({})", hist[idx].date)))
        }
        other => bail!("unknown --anchor '{other}' (use cycle-low, halving, ath, or --anchor-date)"),
    }
}

pub fn run(
    backend: &BackendConnection,
    symbol: &str,
    anchor: &str,
    anchor_date: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let resolved = resolve_alias(symbol);
    let hist = price_history::get_history(backend.sqlite(), &resolved, u32::MAX)?;
    if hist.len() < 2 {
        bail!("not enough price history for '{symbol}' (resolved '{resolved}')");
    }
    let (anchor_idx, anchor_desc) = resolve_anchor(&resolved, &hist, anchor, anchor_date)?;
    let av = anchored_vwap(&hist, anchor_idx)?;
    let price = *hist.last().map(|b| &b.close).unwrap();
    let pct_vs = if av.current > Decimal::ZERO {
        ((price - av.current) / av.current * Decimal::from(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };
    let above = price >= av.current;
    let bars_since = hist.len() - anchor_idx;
    let degraded = matches!(av.quality, AvwapQuality::FlatWeightDegraded);

    let interpretation = if above {
        "price is ABOVE the anchored VWAP — the average buyer since the anchor is in profit; basis defended, accumulation structure intact"
    } else {
        "price is BELOW the anchored VWAP — the average buyer since the anchor is underwater; the accumulation leg is in question"
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "avwap",
                "asset": symbol,
                "resolved_symbol": resolved,
                "anchor": anchor,
                "anchor_date": av.anchor_date,
                "anchor_desc": anchor_desc,
                "bars_since_anchor": bars_since,
                "quality": av.quality,
                "avwap": av.current,
                "price": price,
                "pct_vs_avwap": pct_vs,
                "above": above,
                "interpretation": interpretation,
            }))?
        );
        return Ok(());
    }

    println!("═══ Anchored VWAP — {symbol} ({resolved}) ═══");
    println!("Anchor: {anchor_desc} · {bars_since} bars since\n");
    if degraded {
        println!("⚠ DEGRADED: a bar in the window lacked volume → flat-weight anchored AVERAGE price (not a true VWAP).");
    }
    println!(
        "AVWAP {} | price {} | {} {}%",
        av.current.round_dp(2),
        price.round_dp(2),
        if above { "ABOVE by" } else { "BELOW by" },
        pct_vs.abs(),
    );
    println!("\n{interpretation}.");
    Ok(())
}
