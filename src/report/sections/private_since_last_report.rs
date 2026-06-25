//! Private "Since Last Report" section — the opener (Reporting Loop).
//!
//! Two parts:
//!   1. A deterministic price-action table for each held asset over the period
//!      since the most recent prior archived private report (from
//!      `ctx.since_last_report`, computed in `BuildContext::load` against the new
//!      `report_archive` ledger).
//!   2. The synthesis writer's reflection prose (`[synthesis-since-last-report]`
//!      note) — what the desk said last time, what the tape did, and where it
//!      was right or wrong.
//!
//! On the very first run (no prior report archived AND no reflection note) the
//! section suppresses cleanly; the per-asset briefing becomes the opener until
//! the archive has history.

use anyhow::Result;

use crate::report::build::daily::{BuildContext, SinceLastReport};

fn fmt_price(v: f64) -> String {
    if v.abs() >= 100.0 {
        // Whole-number with thousands separators.
        let n = v.round() as i64;
        let s = n.abs().to_string();
        let mut grouped = String::new();
        for (i, ch) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        let body: String = grouped.chars().rev().collect();
        format!("{}{}", if n < 0 { "-" } else { "" }, body)
    } else {
        format!("{v:.2}")
    }
}

fn render_price_table(slr: &SinceLastReport) -> String {
    if slr.rows.is_empty() {
        return String::new();
    }
    let mut t = String::from("| Asset | Then | Now | Change |\n|---|---:|---:|---:|\n");
    for r in &slr.rows {
        t.push_str(&format!(
            "| {} | {} | {} | {:+.1}% |\n",
            r.symbol,
            fmt_price(r.then_price),
            fmt_price(r.now_price),
            r.pct
        ));
    }
    t
}

pub fn render_private_since_last_report(ctx: &BuildContext) -> Result<String> {
    let reflection = ctx
        .synthesis_notes
        .since_last_report
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let slr = ctx.since_last_report.as_ref();
    let has_prior = slr.map(|s| s.prior_date.is_some()).unwrap_or(false);

    // Nothing to anchor the section: no prior report AND no reflection prose.
    if !has_prior && reflection.is_none() {
        return Ok(super::suppressed(
            "no prior archived report and no [synthesis-since-last-report] note",
        ));
    }

    let mut out = String::from("## Since Last Report\n\n");

    if let Some(slr) = slr {
        if let Some(prior) = slr.prior_date.as_deref() {
            out.push_str(&format!(
                "_Price action over the {} day(s) since the last report ({})._\n\n",
                slr.days, prior
            ));
            let table = render_price_table(slr);
            if !table.is_empty() {
                out.push_str(&table);
                out.push('\n');
            }
        }
    }

    if let Some(body) = reflection {
        out.push_str(body);
    } else {
        out.push_str(
            "_(No reflection note this run — the period's price action is above; \
             the desk's written accountability resumes next run.)_",
        );
    }

    Ok(out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{SinceLastReportRow, SynthesisNotes};

    #[test]
    fn suppressed_on_first_run_with_no_prior_and_no_note() {
        let ctx = BuildContext::default();
        let out = render_private_since_last_report(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("first-run empty state must use the suppression channel");
        assert!(reason.contains("no prior archived report"), "got: {reason}");
    }

    #[test]
    fn renders_table_and_reflection() {
        let ctx = BuildContext {
            since_last_report: Some(SinceLastReport {
                prior_date: Some("2026-06-18".to_string()),
                days: 7,
                rows: vec![SinceLastReportRow {
                    symbol: "BTC".to_string(),
                    then_price: 65000.0,
                    now_price: 59198.0,
                    pct: -8.93,
                }],
            }),
            synthesis_notes: SynthesisNotes {
                since_last_report: Some(
                    "Last report we flagged BTC's bottom-suite at 1/7; it stayed unconfirmed and price fell another 9% — the patience call held.".to_string(),
                ),
                ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_since_last_report(&ctx).unwrap();
        assert!(out.starts_with("## Since Last Report"));
        assert!(out.contains("2026-06-18"));
        assert!(out.contains("| BTC | 65,000 | 59,198 | -8.9% |"));
        assert!(out.contains("patience call held"));
    }

    #[test]
    fn renders_reflection_only_when_no_prior_table() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                since_last_report: Some("First reflection.".to_string()),
                ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_since_last_report(&ctx).unwrap();
        assert!(out.starts_with("## Since Last Report"));
        assert!(out.contains("First reflection."));
    }
}
