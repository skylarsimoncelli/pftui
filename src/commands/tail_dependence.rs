//! `analytics tail-dependence --asset X --vs Y` — do two assets co-crash?
//! Reports Kendall τ, Pearson, and lower/upper tail-dependence (empirical +
//! Clayton-copula), answering whether diversification survives a crisis.

use std::collections::HashMap;

use anyhow::{bail, Result};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;

use crate::analytics::copula::tail_dependence;
use crate::analytics::strategy::resolver::resolve_alias;
use crate::db::backend::BackendConnection;
use crate::db::price_history;

/// Per-asset close keyed by date.
fn dated_closes(backend: &BackendConnection, resolved: &str) -> Result<HashMap<String, f64>> {
    let hist = price_history::get_history(backend.sqlite(), resolved, u32::MAX)?;
    Ok(hist
        .into_iter()
        .filter_map(|r| r.close.to_f64().map(|c| (r.date, c)))
        .filter(|(_, c)| *c > 0.0)
        .collect())
}

pub fn run(
    backend: &BackendConnection,
    asset: &str,
    vs: &str,
    q: f64,
    json_output: bool,
) -> Result<()> {
    let ra = resolve_alias(asset);
    let rb = resolve_alias(vs);
    if ra == rb {
        bail!("--asset and --vs resolve to the same symbol ({ra})");
    }
    if !(1.0..=20.0).contains(&q) {
        bail!("--q is a tail percent and must be between 1 and 20 (got {q})");
    }
    let closes_a = dated_closes(backend, &ra)?;
    let closes_b = dated_closes(backend, &rb)?;
    if closes_a.is_empty() {
        bail!("no price history for '{asset}' (resolved '{ra}')");
    }
    if closes_b.is_empty() {
        bail!("no price history for '{vs}' (resolved '{rb}')");
    }
    // Intersect PRICE dates first, then difference over consecutive common
    // dates — so both assets' return on each date spans the SAME calendar
    // interval. (Differencing each asset on its own prior close before
    // intersecting would mismatch intervals on differing trading calendars —
    // e.g. BTC's Sun→Mon vs gold's Fri→Mon — and dampen measured co-movement.)
    let mut common: Vec<&String> = closes_a.keys().filter(|d| closes_b.contains_key(*d)).collect();
    common.sort();
    if common.len() < 101 {
        bail!(
            "only {} common dates for {ra} vs {rb} — need ≥101 to estimate tail dependence",
            common.len()
        );
    }
    let x: Vec<f64> = common
        .windows(2)
        .map(|w| closes_a[w[1]] / closes_a[w[0]] - 1.0)
        .collect();
    let y: Vec<f64> = common
        .windows(2)
        .map(|w| closes_b[w[1]] / closes_b[w[0]] - 1.0)
        .collect();
    let td = tail_dependence(&x, &y, q / 100.0)
        .ok_or_else(|| anyhow::anyhow!("not enough aligned data to estimate tail dependence"))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "command": "tail-dependence",
                "asset": asset,
                "vs": vs,
                "resolved": [ra, rb],
                "tail_dependence": td,
            }))?
        );
        return Ok(());
    }

    println!("═══ Tail Dependence — {asset} ({ra}) vs {vs} ({rb}) ═══");
    println!("{} common daily returns · tail q = {:.0}%\n", td.n, td.q * 100.0);
    println!(
        "Correlation:  Pearson {:+.3}  |  Kendall τ {:+.3}",
        td.pearson, td.kendall_tau
    );
    // Distinguish the three Clayton cases: τ≤0 (no dependence), a fitted α, and
    // τ→1 (comonotonic, λ_L→1 with α→∞ so clayton_alpha is None).
    let clayton_suffix = if td.kendall_tau <= 0.0 {
        "  (τ≤0 → no lower-tail dependence)".to_string()
    } else if let Some(a) = td.clayton_alpha {
        format!("  (α={a:.2})")
    } else {
        "  (τ→1 → comonotonic, λ_L→1)".to_string()
    };
    println!(
        "Lower tail (co-crash):  empirical λ_L {:.2}  |  Clayton λ_L {:.2}{}",
        td.emp_lower_tail_dep, td.clayton_lower_tail_dep, clayton_suffix,
    );
    println!("Upper tail (co-rally):  empirical λ_U {:.2}", td.emp_upper_tail_dep);
    println!();
    println!("{}", td.interpretation);
    Ok(())
}
