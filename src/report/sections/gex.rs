//! Per-asset GEX one-liner for the daily report.
//!
//! Reads the most recent `gex_snapshots` row for `asset` via
//! `db::gex_snapshots::latest`. Returns `Ok(None)` when no snapshot
//! has been ingested — callers treat that as "skip silently".
//!
//! Emits a single line:
//!   "GEX flip: $X · Max pain: $Y · Net gamma: ±Z (asof YYYY-MM-DD)"

#![allow(dead_code)]

use anyhow::Result;

use crate::data::options::GexSummary;
use crate::db::gex_snapshots;
use crate::report::build::daily::BuildContext;

/// Render the per-asset GEX block, if the snapshot exists.
///
/// `ctx` is currently unused: snapshot lookup goes straight to the
/// SQLite cache. The signature carries `ctx` for future BuildContext
/// pre-population (e.g. when the assembler hoists the lookup into a
/// single batched query).
pub fn render_gex_block(ctx: &BuildContext, asset: &str) -> Result<Option<String>> {
    let _ = ctx;
    let backend = match crate::db::backend::open_from_config(
        &crate::config::Config::default(),
        &crate::db::default_db_path(),
    ) {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    let Some(conn) = backend.sqlite_native() else {
        return Ok(None);
    };
    let gex = gex_snapshots::latest(conn, asset)?;
    Ok(gex.map(|g| render_from_summary(&g)))
}

/// Pure render helper used by tests and the BuildContext path.
pub fn render_from_summary(g: &GexSummary) -> String {
    let flip = g
        .gex_flip_strike
        .map(|v| format!("${:.2}", v))
        .unwrap_or_else(|| "n/a".to_string());
    let mp = g
        .max_pain
        .map(|v| format!("${:.2}", v))
        .unwrap_or_else(|| "n/a".to_string());
    let net = g.total_gamma_call - g.total_gamma_put;
    let net_str = format!("{:+.0}", net);
    let asof = g.fetched_at.split('T').next().unwrap_or(&g.fetched_at);
    format!(
        "GEX flip: {} · Max pain: {} · Net gamma: {} (asof {})",
        flip, mp, net_str, asof
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_full_summary() {
        let g = GexSummary {
            symbol: "SPY".into(),
            gex_flip_strike: Some(550.0),
            total_gamma_call: 10000.0,
            total_gamma_put: 4000.0,
            max_pain: Some(548.0),
            fetched_at: "2026-06-02T12:34:56Z".into(),
        };
        let line = render_from_summary(&g);
        assert!(line.contains("$550.00"), "line={}", line);
        assert!(line.contains("$548.00"), "line={}", line);
        assert!(line.contains("+6000"), "line={}", line);
        assert!(line.contains("asof 2026-06-02"), "line={}", line);
    }

    #[test]
    fn render_with_missing_fields() {
        let g = GexSummary {
            symbol: "GLD".into(),
            gex_flip_strike: None,
            total_gamma_call: 0.0,
            total_gamma_put: 0.0,
            max_pain: None,
            fetched_at: "2026-06-02T00:00:00Z".into(),
        };
        let line = render_from_summary(&g);
        assert!(line.contains("n/a"), "line={}", line);
        assert!(line.contains("+0"), "line={}", line);
    }
}
