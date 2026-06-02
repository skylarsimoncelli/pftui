//! Macro-section helper that summarizes the real-yields curve for the daily
//! report. Emits a single compact block with the 10Y TIPS level, week-over-
//! week change in basis points, breakeven inflation, and US-vs-Germany 10Y
//! spread plus a one-line interpretation hint.
//!
//! The renderer pulls data from a `MacroBlockSnapshot` so it can be unit-
//! tested without a live database. The hook into the assembler currently
//! lives behind the call-site documented in `AGENTS.md` — invoke
//! `pftui analytics real-rates differentials --json` from any analyst routine
//! that needs the underlying data, then pass the resulting snapshot through
//! `render_real_rates_block`.

#![allow(dead_code)]

use anyhow::Result;

use crate::commands::real_yields::MacroBlockSnapshot;
use crate::report::build::daily::BuildContext;

/// Render the Macro-section real-rates block.
///
/// Returns `Ok(String::new())` when there isn't enough cached data to make a
/// useful block — callers should treat that as "skip this section silently".
pub fn render_real_rates_block(ctx: &BuildContext) -> Result<String> {
    // The BuildContext currently exposes a narrative-only `real_yield_context`
    // (filled by the assembler from heuristic text). The structured snapshot
    // built from `real_yields_history` lives in `real_rates_snapshot` and is
    // optional — if not populated we still try the narrative summary so the
    // report keeps emitting something useful.
    let snapshot = ctx.real_rates_snapshot.clone();
    Ok(render_from_snapshot(snapshot.as_ref()))
}

/// Pure render helper, used by both the BuildContext path and the unit tests.
pub fn render_from_snapshot(snap: Option<&MacroBlockSnapshot>) -> String {
    let Some(snap) = snap else {
        return String::new();
    };
    if snap.us_nominal_10y.is_none()
        && snap.us_tips_10y.is_none()
        && snap.us_breakeven_10y.is_none()
        && snap.us_minus_de_bp.is_none()
    {
        return String::new();
    }

    let tips_part = match (snap.us_tips_10y, snap.tips_week_change_bp) {
        (Some(v), Some(c)) => format!("10Y TIPS {:.2}% (week change {:+.1} bp)", v, c),
        (Some(v), None) => format!("10Y TIPS {:.2}%", v),
        _ => "10Y TIPS n/a".to_string(),
    };
    let be_part = snap
        .us_breakeven_10y
        .map(|v| format!("Breakeven {:.2}%", v))
        .unwrap_or_else(|| "Breakeven n/a".to_string());
    let spread_part = snap
        .us_minus_de_bp
        .map(|v| format!("US-DE 10Y spread {:+.0} bp", v))
        .unwrap_or_else(|| "US-DE 10Y spread n/a".to_string());

    let mut out = String::new();
    out.push_str("Real rates: ");
    out.push_str(&tips_part);
    out.push_str(" | ");
    out.push_str(&be_part);
    out.push_str(" | ");
    out.push_str(&spread_part);
    out.push('\n');

    let hint = interpretation_hint(snap);
    if !hint.is_empty() {
        out.push_str("Interpretation: ");
        out.push_str(&hint);
        out.push('\n');
    }
    out
}

fn interpretation_hint(snap: &MacroBlockSnapshot) -> String {
    // Lightweight, deterministic heuristics — the analyst routines remain the
    // primary interpreters; this hint only flags the dominant signal so the
    // report doesn't read as a wall of numbers.
    let change = snap.tips_week_change_bp.unwrap_or(0.0);
    let spread = snap.us_minus_de_bp.unwrap_or(0.0);
    let mut parts: Vec<String> = Vec::new();
    if change >= 10.0 {
        parts.push("real yields rising — headwind for gold and long duration".to_string());
    } else if change <= -10.0 {
        parts.push("real yields falling — tailwind for gold and long duration".to_string());
    }
    if spread.abs() >= 200.0 {
        let dir = if spread > 0.0 { "wide US premium" } else { "US discount" };
        parts.push(format!("US-Bund spread {} — DXY-supportive when widening", dir));
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_returns_empty_when_snapshot_is_none() {
        assert!(render_from_snapshot(None).is_empty());
    }

    #[test]
    fn render_emits_real_rates_line_and_interpretation() {
        let snap = MacroBlockSnapshot {
            us_nominal_10y: Some(4.30),
            us_tips_10y: Some(2.15),
            us_breakeven_10y: Some(2.40),
            us_minus_de_bp: Some(210.0),
            tips_week_change_bp: Some(15.0),
        };
        let out = render_from_snapshot(Some(&snap));
        assert!(out.contains("Real rates:"));
        assert!(out.contains("10Y TIPS 2.15%"));
        assert!(out.contains("week change +15.0 bp"));
        assert!(out.contains("Breakeven 2.40%"));
        assert!(out.contains("US-DE 10Y spread +210 bp"));
        assert!(out.contains("Interpretation:"));
        // 15bp rise should trigger the rising-real-yields hint
        assert!(out.contains("real yields rising"));
    }

    #[test]
    fn render_handles_partial_data_without_panicking() {
        let snap = MacroBlockSnapshot {
            us_nominal_10y: Some(4.30),
            us_tips_10y: None,
            us_breakeven_10y: Some(2.40),
            us_minus_de_bp: None,
            tips_week_change_bp: None,
        };
        let out = render_from_snapshot(Some(&snap));
        assert!(out.contains("10Y TIPS n/a"));
        assert!(out.contains("Breakeven 2.40%"));
        assert!(out.contains("US-DE 10Y spread n/a"));
    }
}
