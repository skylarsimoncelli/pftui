#![allow(dead_code)]
//! Private "Quantitative Parallels" section.
//!
//! Surfaces the forward-return distributions produced by the parallels
//! catalog runner (`pftui-parallels-run`). Each matching set yields a
//! one-row table of median 5d / 30d / 90d / 180d returns plus 30d / 90d
//! hit rates and the set's narrative.

use anyhow::Result;

use crate::report::build::daily::{BuildContext, ParallelsResult};

pub fn render_private_parallels(ctx: &BuildContext) -> Result<String> {
    // Suppress 0-match rows from the table. The narrative row underneath
    // each set already explains *why* a set returned 0 matches (typically
    // because a referenced data series has too little history), so the
    // em-dash-across-every-column line adds noise. Engine-error rows are
    // kept so the operator notices a broken set.
    let actionable: Vec<_> = ctx
        .parallels_results
        .iter()
        .filter(|r| r.error.is_some() || r.match_count > 0)
        .collect();

    if actionable.is_empty() {
        // Whole section suppressed when nothing landed AND no errors —
        // empty-state filler bloats the PDF.
        return Ok(super::suppressed(
            "no parallel sets matched for the report date and no runner errors to surface",
        ));
    }

    let mut output = String::from("## Quantitative Parallels\n\n");
    output.push_str(
        "Matching historical-analog set forward-return distributions \
        (median per horizon). Hit rates measure the share of analog episodes \
        that closed higher than the entry print at the respective horizon.\n\n",
    );

    output.push_str("| Set | Symbol | n | 5d | 30d | 90d | 180d | hit 30d | hit 90d |\n");
    output.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for r in &actionable {
        if let Some(err) = &r.error {
            output.push_str(&format!(
                "| {} | {} | — | — | — | — | — | — | — |  _engine error: {}_\n",
                escape_cell(&r.name),
                escape_cell(&r.symbol),
                escape_cell(err)
            ));
            continue;
        }
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            escape_cell(&r.name),
            escape_cell(&r.symbol),
            r.match_count,
            fmt_pct(r.median_5d_pct),
            fmt_pct(r.median_30d_pct),
            fmt_pct(r.median_90d_pct),
            fmt_pct(r.median_180d_pct),
            fmt_pct(r.hit_rate_30d_pct),
            fmt_pct(r.hit_rate_90d_pct),
        ));
    }

    // Narratives only render for sets that actually produced matches —
    // a 0-match set's narrative is just the "retained for the future
    // when more history is backfilled" disclosure, which is operator
    // noise rather than signal.
    let with_narrative: Vec<&ParallelsResult> = ctx
        .parallels_results
        .iter()
        .filter(|r| !r.narrative.trim().is_empty() && r.match_count > 0)
        .collect();
    if !with_narrative.is_empty() {
        output.push_str("\n### Narratives\n\n");
        for r in with_narrative {
            output.push_str(&format!(
                "- **{}** ({}): {}\n",
                escape_cell(&r.name),
                escape_cell(&r.symbol),
                escape_cell(r.narrative.trim()),
            ));
        }
    }

    Ok(output.trim_end().to_string())
}

fn fmt_pct(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:+.1}%"),
        None => "—".to_string(),
    }
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ParallelsResult {
        ParallelsResult {
            id: "btc-200wma".to_string(),
            name: "BTC at 200WMA".to_string(),
            symbol: "BTC".to_string(),
            narrative: "Spot trading inside 5% of the 200-week SMA — historically a high-RR floor.".to_string(),
            match_count: 12,
            median_5d_pct: Some(1.4),
            median_30d_pct: Some(8.2),
            median_90d_pct: Some(24.1),
            median_180d_pct: Some(45.0),
            hit_rate_30d_pct: Some(75.0),
            hit_rate_90d_pct: Some(83.3),
            error: None,
        }
    }

    #[test]
    fn suppressed_when_no_results() {
        // Empty results => suppress section entirely. Empty-state filler
        // was operator-noise.
        let ctx = BuildContext::default();
        let out = render_private_parallels(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("no parallel sets matched"), "unexpected reason: {reason}");
    }

    #[test]
    fn renders_table_and_narrative_block() {
        let ctx = BuildContext {
            parallels_results: vec![sample()],
            ..BuildContext::default()
        };
        let out = render_private_parallels(&ctx).unwrap();
        assert!(out.contains("## Quantitative Parallels"));
        assert!(out.contains("BTC at 200WMA"));
        assert!(out.contains("| 12 |"));
        assert!(out.contains("+8.2%"));
        assert!(out.contains("+24.1%"));
        assert!(out.contains("hit 30d"));
        assert!(out.contains("Narratives"));
        assert!(out.contains("200-week SMA"));
    }

    #[test]
    fn surfaces_engine_errors_inline() {
        let mut err = sample();
        err.error = Some("predicate parse failed".to_string());
        let ctx = BuildContext {
            parallels_results: vec![err],
            ..BuildContext::default()
        };
        let out = render_private_parallels(&ctx).unwrap();
        assert!(out.contains("engine error: predicate parse failed"));
    }

    #[test]
    fn missing_horizons_render_dash() {
        let mut r = sample();
        r.median_90d_pct = None;
        r.hit_rate_30d_pct = None;
        let ctx = BuildContext {
            parallels_results: vec![r],
            ..BuildContext::default()
        };
        let out = render_private_parallels(&ctx).unwrap();
        // Two cells should be dashed (90d median + 30d hit rate).
        assert!(out.contains("| — |"));
    }
}
