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

/// Per-asset daily returns keyed by date (return into each bar from its own
/// prior close).
fn dated_returns(backend: &BackendConnection, resolved: &str) -> Result<HashMap<String, f64>> {
    let hist = price_history::get_history(backend.sqlite(), resolved, u32::MAX)?;
    let mut out = HashMap::new();
    for w in hist.windows(2) {
        let prev = w[0].close.to_f64().unwrap_or(0.0);
        let cur = w[1].close.to_f64().unwrap_or(0.0);
        if prev > 0.0 {
            out.insert(w[1].date.clone(), cur / prev - 1.0);
        }
    }
    Ok(out)
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
    let map_a = dated_returns(backend, &ra)?;
    let map_b = dated_returns(backend, &rb)?;
    if map_a.is_empty() {
        bail!("no price history for '{asset}' (resolved '{ra}')");
    }
    if map_b.is_empty() {
        bail!("no price history for '{vs}' (resolved '{rb}')");
    }
    // Align on common dates (sorted for determinism).
    let mut dates: Vec<&String> = map_a.keys().filter(|d| map_b.contains_key(*d)).collect();
    dates.sort();
    if dates.len() < 100 {
        bail!(
            "only {} common dates for {ra} vs {rb} — need ≥100 to estimate tail dependence",
            dates.len()
        );
    }
    let x: Vec<f64> = dates.iter().map(|d| map_a[*d]).collect();
    let y: Vec<f64> = dates.iter().map(|d| map_b[*d]).collect();
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
    println!(
        "Lower tail (co-crash):  empirical λ_L {:.2}  |  Clayton λ_L {:.2}{}",
        td.emp_lower_tail_dep,
        td.clayton_lower_tail_dep,
        td.clayton_alpha
            .map(|a| format!("  (α={a:.2})"))
            .unwrap_or_else(|| "  (τ≤0 → no Clayton dependence)".into()),
    );
    println!("Upper tail (co-rally):  empirical λ_U {:.2}", td.emp_upper_tail_dep);
    println!();
    println!("{}", td.interpretation);
    Ok(())
}
