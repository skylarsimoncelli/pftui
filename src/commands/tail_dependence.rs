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

/// Aligned daily returns for two symbols on their COMMON dates. Intersects
/// price dates first, then differences over consecutive common dates so both
/// series' return on each date spans the SAME calendar interval (the correct
/// co-movement construction — differencing each on its own prior close before
/// intersecting would mismatch intervals on differing trading calendars and
/// dampen measured co-movement). Shared by `tail-dependence` and the
/// `risk-dashboard` co-crash block so they can't drift. `None` if <101 common.
pub fn aligned_common_returns(
    backend: &BackendConnection,
    ra: &str,
    rb: &str,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let a = dated_closes(backend, ra).ok()?;
    let b = dated_closes(backend, rb).ok()?;
    let mut common: Vec<&String> = a.keys().filter(|d| b.contains_key(*d)).collect();
    common.sort();
    if common.len() < 101 {
        return None;
    }
    let x: Vec<f64> = common.windows(2).map(|w| a[w[1]] / a[w[0]] - 1.0).collect();
    let y: Vec<f64> = common.windows(2).map(|w| b[w[1]] / b[w[0]] - 1.0).collect();
    Some((x, y))
}

/// The most recent date present in BOTH price series, for the `as_of` envelope
/// key. `None` if either lookup fails or they share no dates.
fn latest_common_date(backend: &BackendConnection, ra: &str, rb: &str) -> Option<String> {
    let a = dated_closes(backend, ra).ok()?;
    let b = dated_closes(backend, rb).ok()?;
    a.keys()
        .filter(|d| b.contains_key(*d))
        .max()
        .cloned()
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
    let (x, y) = aligned_common_returns(backend, &ra, &rb).ok_or_else(|| {
        anyhow::anyhow!(
            "not enough common price history for {ra} vs {rb} — need ≥101 common dates (check both symbols have data)"
        )
    })?;
    let td = tail_dependence(&x, &y, q / 100.0)
        .ok_or_else(|| anyhow::anyhow!("not enough aligned data to estimate tail dependence"))?;

    if json_output {
        // `as_of` = most recent date common to both series (the last bar the
        // co-movement estimate actually covers).
        let as_of = latest_common_date(backend, &ra, &rb).unwrap_or_default();
        // Standard envelope (additive — keeps `resolved:[ra,rb]` and `vs`).
        // `resolved_symbol` is the primary asset (`--asset`); the counterpart
        // stays in `resolved[1]`/`vs`.
        let payload = crate::commands::cli_json::envelope(
            json!({
                "command": "tail-dependence",
                "asset": asset,
                "vs": vs,
                "resolved": [ra.clone(), rb.clone()],
                "tail_dependence": td,
            }),
            "tail-dependence",
            &as_of,
            Some(&ra),
        );
        println!("{}", serde_json::to_string_pretty(&payload)?);
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
