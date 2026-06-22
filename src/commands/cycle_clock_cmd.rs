//! `pftui analytics cycles clock` — cycle-position read for BTC and gold.
//! Position only; never a price prediction. Engine and anchor verification
//! policy live in `analytics::cycle_clock`.

use anyhow::{bail, Result};
use serde_json::json;

use crate::analytics::cycle_clock::{self, BtcCycleClock, GoldCycleClock};
use crate::commands::technicals_structure::load_deep_history;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Full-depth fetch — the gold series goes back to 2000 (~6,500 rows).
const DEEP_LIMIT: u32 = 8000;

pub fn run(backend: &BackendConnection, asset: Option<&str>, json_output: bool) -> Result<()> {
    let asset_norm = asset.map(|a| a.trim().to_uppercase());
    let (want_btc, want_gold) = match asset_norm.as_deref() {
        None => (true, true),
        Some("BTC") | Some("BTC-USD") => (true, false),
        Some("GC=F") | Some("GOLD") => (false, true),
        Some(other) => bail!(
            "cycle clock supports --asset BTC or --asset GC=F (got '{other}')"
        ),
    };

    let btc = if want_btc { btc_clock(backend)? } else { None };
    let gold = if want_gold { gold_clock(backend)? } else { None };

    if btc.is_none() && gold.is_none() {
        bail!("no usable price history for the requested cycle clock — run `pftui data refresh`");
    }

    if json_output {
        // `as_of` from whichever clock we computed (BTC preferred, else gold).
        let as_of = btc
            .as_ref()
            .map(|b| b.as_of.clone())
            .or_else(|| gold.as_ref().map(|g| g.as_of.clone()))
            .unwrap_or_default();
        // No single resolved symbol — the default read covers BTC + gold.
        let payload = crate::commands::cli_json::envelope(
            json!({
                "btc": btc,
                "gold": gold,
                "note": "cycle POSITION only — the checklist confirms, the calendar does not; no price predictions",
            }),
            "cycles clock",
            &as_of,
            None,
        );
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("Cycle clock (position only — no price predictions)\n");
    if let Some(b) = &btc {
        println!("  {}", b.verdict);
        println!(
            "    halving {} (+{}d); Olson day-900: {} ({}d away)",
            b.halving_date, b.days_since_halving, b.olson_day900_date, b.olson_days_remaining
        );
        if let Some(l) = &b.loukas {
            println!(
                "    Loukas: cycle week {} of ~{} (low band wk {}-{}, in band: {})",
                l.cycle_week, l.cycle_length_weeks, l.band_low_week, l.band_high_week, l.in_band
            );
        }
        if let (Some(date), Some(close)) = (
            b.cycle_low_anchor.verified_date.as_deref(),
            b.cycle_low_anchor.verified_close,
        ) {
            println!(
                "    cycle-low anchor: documented {} — verified {} @ {} (confirms: {})",
                b.cycle_low_anchor.documented,
                date,
                close.round_dp(0),
                b.cycle_low_anchor
                    .confirms_documented
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
        if let Some(t) = &b.major_cycle_test {
            println!(
                "    major-vs-4yr test: prior cycle-high CLOSE {} ({}), now {:+}% — {}",
                t.prior_cycle_high, t.prior_cycle_high_date, t.pct_vs_prior_high, t.note
            );
        }
        let acc = &b.accumulation;
        println!("    ▸ {} (score {:+})", acc.verdict, acc.score);
        for f in &acc.factors {
            println!("        · {f}");
        }
        println!();
    }
    if let Some(g) = &gold {
        println!("  {}", g.verdict);
        for a in &g.anchors {
            match (&a.verified_date, a.verified_close) {
                (Some(date), Some(close)) => println!(
                    "    anchor: documented ~{} — verified {} @ {} (confirms: {})",
                    a.documented,
                    date,
                    close.round_dp(0),
                    a.confirms_documented
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ),
                _ => println!(
                    "    anchor: documented ~{} — history does not cover window",
                    a.documented
                ),
            }
        }
        println!();
    }
    println!("  Note: the checklist confirms, the calendar does not.");
    Ok(())
}

fn btc_clock(backend: &BackendConnection) -> Result<Option<BtcCycleClock>> {
    // Always use the deep BTC-USD series (the shallow `BTC` series only
    // carries ~365 rows and cannot anchor a cycle).
    let history = price_history::get_history_backend(backend, "BTC-USD", DEEP_LIMIT)?;
    if history.len() >= 400 {
        return Ok(cycle_clock::btc_cycle_clock("BTC-USD", &history));
    }
    // Fall back to whatever the deepest BTC series is.
    let (series, history) = load_deep_history(backend, "BTC")?;
    Ok(cycle_clock::btc_cycle_clock(&series, &history))
}

fn gold_clock(backend: &BackendConnection) -> Result<Option<GoldCycleClock>> {
    let history = price_history::get_history_backend(backend, "GC=F", DEEP_LIMIT)?;
    if history.is_empty() {
        return Ok(None);
    }
    Ok(cycle_clock::gold_cycle_clock("GC=F", &history))
}
