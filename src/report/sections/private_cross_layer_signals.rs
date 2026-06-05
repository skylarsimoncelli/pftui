#![allow(dead_code)]
//! Private "Cross-Layer Signals" section.
//!
//! Surfaces the unacknowledged-since-this-morning inbound signals from
//! every analyst layer to the synthesis layer. Grouped by priority so
//! `high` items lead.

use anyhow::Result;

use crate::report::build::daily::{BuildContext, CrossLayerSignal};

pub fn render_private_cross_layer_signals(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Cross-Layer Signals\n\n");
    if ctx.cross_layer_signals.is_empty() {
        output.push_str(
            "No inbound layer→synthesis messages landed today at high or normal priority. \
            This is normal on quiet sessions; check `pftui agent message list --to synthesis` \
            for the full unfiltered queue.",
        );
        return Ok(output);
    }

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
        push_table(&mut output, &high);
        output.push('\n');
    }
    if !normal.is_empty() {
        output.push_str("### Normal priority\n\n");
        push_table(&mut output, &normal);
    }

    Ok(output.trim_end().to_string())
}

fn push_table(out: &mut String, rows: &[&CrossLayerSignal]) {
    out.push_str("| From | To | Category | Summary |\n");
    out.push_str("|---|---|---|---|\n");
    for s in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            escape_cell(&s.from_layer),
            escape_cell(&s.to_layer),
            escape_cell(&s.category),
            escape_cell(&s.summary),
        ));
    }
}

fn escape_cell(s: &str) -> String {
    let trimmed = s.replace('|', "/").trim().to_string();
    if trimmed.is_empty() {
        "—".to_string()
    } else {
        trimmed
    }
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
    fn renders_empty_state_when_no_signals() {
        let ctx = BuildContext::default();
        let out = render_private_cross_layer_signals(&ctx).unwrap();
        assert!(out.starts_with("## Cross-Layer Signals"));
        assert!(out.contains("No inbound layer"));
    }

    #[test]
    fn groups_by_priority_high_first() {
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
        assert!(out.contains("RSI divergence printing"));
        assert!(out.contains("MACRO tilt softening"));
    }

    #[test]
    fn escapes_pipe_characters_in_summary() {
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
        // Must not break the markdown table.
        assert!(out.contains("BTC / ETH spread widening"));
        assert!(!out.contains("BTC | ETH spread"));
    }

    #[test]
    fn empty_category_renders_dash() {
        let ctx = BuildContext {
            cross_layer_signals: vec![sig("analyst-macro", "normal", "", "Long-cycle rebalance")],
            ..BuildContext::default()
        };
        let out = render_private_cross_layer_signals(&ctx).unwrap();
        assert!(out.contains("| — |"));
    }
}
