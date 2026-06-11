#![allow(dead_code)]
//! Private "Cross-Layer Signals" section.
//!
//! Surfaces the inbound signals from every analyst layer to the synthesis
//! layer as a readable bulleted list, grouped by priority. The previous
//! incarnation rendered a 4-column FROM/TO/CATEGORY/SUMMARY table that
//! the operator described as "unreadable and doesn't even seem to be
//! written for human reading" — this rewrite keeps the data but presents
//! it as English the operator can skim in a minute.

use anyhow::Result;
use std::collections::BTreeMap;

use crate::report::build::daily::{BuildContext, CrossLayerSignal};

pub fn render_private_cross_layer_signals(ctx: &BuildContext) -> Result<String> {
    if ctx.cross_layer_signals.is_empty() {
        // Suppress on quiet sessions rather than emitting the "no inbound
        // messages landed" disclaimer that wasted a page in prior runs.
        return Ok(super::suppressed(
            "no synthesis-bound agent messages landed for the report date",
        ));
    }

    let mut output = String::from("## Cross-Layer Signals\n\n");
    output.push_str(
        "What each timeframe layer flagged to synthesis this run. Grouped by priority \
         and source; one bullet per signal, source layer prefixed.\n\n",
    );

    let high: Vec<&CrossLayerSignal> = ctx
        .cross_layer_signals
        .iter()
        .filter(|s| s.priority.eq_ignore_ascii_case("high"))
        .collect();
    let normal: Vec<&CrossLayerSignal> = ctx
        .cross_layer_signals
        .iter()
        .filter(|s| s.priority.eq_ignore_ascii_case("normal"))
        .collect();

    if !high.is_empty() {
        output.push_str("### High priority\n\n");
        push_grouped_bullets(&mut output, &high);
        output.push('\n');
    }
    if !normal.is_empty() {
        output.push_str("### Normal priority\n\n");
        push_grouped_bullets(&mut output, &normal);
    }

    Ok(output.trim_end().to_string())
}

/// Group signals by source layer (analyst-low / analyst-medium / etc) so
/// the operator can scan one layer's worth of flags at a time. Within
/// each group, signals render as bullets prefixed with the source label.
fn push_grouped_bullets(out: &mut String, rows: &[&CrossLayerSignal]) {
    let mut by_source: BTreeMap<String, Vec<&CrossLayerSignal>> = BTreeMap::new();
    for s in rows {
        by_source
            .entry(canonical_source_label(&s.from_layer))
            .or_default()
            .push(*s);
    }
    for (source, signals) in by_source {
        out.push_str(&format!("**{source}**\n\n"));
        for s in signals {
            let body = clean_summary(&s.summary);
            if body.is_empty() {
                continue;
            }
            out.push_str(&format!("- {body}\n"));
        }
        out.push('\n');
    }
}

/// Map raw `from_agent` strings onto display labels the operator can scan.
/// `analyst-low` → "LOW", `analyst-macro-agent` → "MACRO", etc.
fn canonical_source_label(from: &str) -> String {
    let lower = from.to_ascii_lowercase();
    if lower.contains("macro") {
        return "MACRO".to_string();
    }
    if lower.contains("high") {
        return "HIGH".to_string();
    }
    if lower.contains("medium") || lower.contains("med") {
        return "MEDIUM".to_string();
    }
    if lower.contains("low") {
        return "LOW".to_string();
    }
    if lower.contains("synthesis") {
        return "SYNTHESIS".to_string();
    }
    if lower.contains("adversary") {
        return "ADVERSARY".to_string();
    }
    if lower.starts_with("panel-") {
        return "PANEL".to_string();
    }
    from.to_string()
}

fn clean_summary(s: &str) -> String {
    s.replace('|', "/")
        .replace('\n', " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(from: &str, priority: &str, category: &str, summary: &str) -> CrossLayerSignal {
        CrossLayerSignal {
            from_layer: from.to_string(),
            to_layer: "synthesis".to_string(),
            priority: priority.to_string(),
            category: category.to_string(),
            summary: summary.to_string(),
        }
    }

    #[test]
    fn suppressed_when_no_signals() {
        let ctx = BuildContext::default();
        let out = render_private_cross_layer_signals(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("no synthesis-bound agent messages"), "unexpected reason: {reason}");
    }

    #[test]
    fn groups_by_layer_high_first() {
        let ctx = BuildContext {
            cross_layer_signals: vec![
                sig("analyst-low", "high", "alert", "RSI divergence printing"),
                sig("analyst-medium", "normal", "view-shift", "MACRO tilt softening"),
                sig("analyst-high", "high", "risk", "200WMA test underway"),
            ],
            ..BuildContext::default()
        };
        let out = render_private_cross_layer_signals(&ctx).unwrap();
        assert!(out.contains("### High priority"));
        assert!(out.contains("### Normal priority"));
        // High block must precede Normal block.
        let hi = out.find("High priority").unwrap();
        let no = out.find("Normal priority").unwrap();
        assert!(hi < no);
        // Layer labels appear as section headers.
        assert!(out.contains("**LOW**"));
        assert!(out.contains("**HIGH**"));
        assert!(out.contains("**MEDIUM**"));
        // Bullets carry summaries.
        assert!(out.contains("- RSI divergence printing"));
        assert!(out.contains("- 200WMA test underway"));
    }

    #[test]
    fn pipe_chars_are_escaped() {
        let ctx = BuildContext {
            cross_layer_signals: vec![sig(
                "analyst-low",
                "normal",
                "view-shift",
                "BTC | ETH spread widening",
            )],
            ..BuildContext::default()
        };
        let out = render_private_cross_layer_signals(&ctx).unwrap();
        assert!(out.contains("BTC / ETH spread widening"));
        assert!(!out.contains("BTC | ETH spread"));
    }
}
