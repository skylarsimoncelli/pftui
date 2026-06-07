//! Private "Investor Panel" section.
//!
//! Surfaces the structured responses from the 8 persona subagents the
//! report skill's Phase 2b spawns (`~/pftui/agents/investor-panel/`).
//! Renders three sub-blocks per run:
//!   1. Panel consensus table — per-asset bullish/bearish/neutral votes
//!      with a consensus label so the operator can scan agreement at a
//!      glance.
//!   2. Per-persona table — each persona's overall signal, confidence,
//!      key insight, and what-would-change-my-mind text.
//!   3. Top divergences callout — assets where the panel is most split.
//!
//! Empty section when no `panel-*` agent_messages landed for the run.

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, InvestorPanelConsensus, InvestorPanelResponse,
};

const SECTION_TITLE: &str = "## Investor Panel\n\n";

pub fn render_private_investor_panel(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from(SECTION_TITLE);

    if ctx.investor_panel.is_empty() {
        output.push_str(
            "No investor-panel responses landed for this report. The Phase 2b \
             persona spawn either was skipped or every persona's response failed \
             to parse as the expected JSON shape.",
        );
        return Ok(output);
    }

    output.push_str(
        "Eight investor personas weighed in independently from their own \
         philosophical priors. Aggregated consensus + per-persona detail below.\n\n",
    );

    push_consensus_block(&mut output, &ctx.investor_panel_consensus);
    push_persona_block(&mut output, &ctx.investor_panel);
    push_divergence_block(&mut output, &ctx.investor_panel_consensus);

    Ok(output.trim_end().to_string())
}

fn push_consensus_block(output: &mut String, rows: &[InvestorPanelConsensus]) {
    if rows.is_empty() {
        return;
    }
    output.push_str("### Panel consensus by asset\n\n");
    output.push_str("| Asset | Bullish | Bearish | Neutral | Label |\n");
    output.push_str("|---|---:|---:|---:|---|\n");
    for r in rows {
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean_cell(&r.asset.to_uppercase()),
            r.bullish_count,
            r.bearish_count,
            r.neutral_count,
            consensus_emoji(&r.label),
        ));
    }
    output.push('\n');
}

fn push_persona_block(output: &mut String, rows: &[InvestorPanelResponse]) {
    output.push_str("### Persona views\n\n");
    output.push_str("| Persona | Overall | Conf | Key insight | What would change my mind |\n");
    output.push_str("|---|---|---:|---|---|\n");
    for r in rows {
        output.push_str(&format!(
            "| {} | {} {} | {}% | {} | {} |\n",
            clean_cell(&r.investor),
            signal_glyph(&r.overall_signal),
            clean_cell(&r.overall_signal),
            r.confidence,
            truncate(&clean_cell(&r.key_insight), 180),
            truncate(&clean_cell(&r.what_would_change_my_mind), 180),
        ));
    }
    output.push('\n');
}

fn push_divergence_block(output: &mut String, rows: &[InvestorPanelConsensus]) {
    let mut divergent: Vec<&InvestorPanelConsensus> = rows
        .iter()
        .filter(|r| r.label == "high-divergence" || r.label == "mixed")
        .collect();
    divergent.sort_by(|a, b| {
        let abs_a = (a.bullish_count as i32 - a.bearish_count as i32).abs();
        let abs_b = (b.bullish_count as i32 - b.bearish_count as i32).abs();
        abs_a.cmp(&abs_b)
            .then_with(|| (b.bullish_count + b.bearish_count).cmp(&(a.bullish_count + a.bearish_count)))
    });
    if divergent.is_empty() {
        return;
    }
    output.push_str("### Most-contested calls\n\n");
    for r in divergent.iter().take(3) {
        output.push_str(&format!(
            "- **{}**: {} bullish vs {} bearish vs {} neutral — {}\n",
            r.asset.to_uppercase(),
            r.bullish_count,
            r.bearish_count,
            r.neutral_count,
            r.label,
        ));
    }
}

fn signal_glyph(signal: &str) -> &'static str {
    match signal.to_ascii_lowercase().as_str() {
        "bullish" => "🐂",
        "bearish" => "🐻",
        _ => "⚖",
    }
}

fn consensus_emoji(label: &str) -> String {
    match label {
        "strong-consensus-bullish" => format!("🐂 {label}"),
        "strong-consensus-bearish" => format!("🐻 {label}"),
        "high-divergence" => format!("⚡ {label}"),
        "lean-bullish" => format!("↑ {label}"),
        "lean-bearish" => format!("↓ {label}"),
        _ => label.to_string(),
    }
}

fn clean_cell(s: &str) -> String {
    s.replace('|', "/").replace('\n', " ").trim().to_string()
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{}…", truncated.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{
        InvestorPanelConsensus, InvestorPanelPositioning, InvestorPanelResponse,
    };

    fn sample_response(name: &str, overall: &str, confidence: u8, asset_signals: &[(&str, &str)]) -> InvestorPanelResponse {
        InvestorPanelResponse {
            investor: name.to_string(),
            overall_signal: overall.to_string(),
            confidence,
            positioning: asset_signals
                .iter()
                .map(|(a, s)| InvestorPanelPositioning {
                    asset: a.to_string(),
                    signal: s.to_string(),
                    weight: "tactical".to_string(),
                    reasoning: format!("{name} reasoning on {a}"),
                })
                .collect(),
            key_insight: format!("{name} key insight"),
            what_would_change_my_mind: format!("{name} change trigger"),
        }
    }

    #[test]
    fn renders_empty_state_when_no_panel_responses() {
        let ctx = BuildContext::default();
        let out = render_private_investor_panel(&ctx).unwrap();
        assert!(out.contains("No investor-panel responses landed"));
    }

    #[test]
    fn renders_consensus_persona_and_divergence_blocks() {
        let ctx = BuildContext {
            investor_panel: vec![
                sample_response("Druckenmiller", "bearish", 68, &[("cash", "bullish"), ("btc", "bearish")]),
                sample_response("Dalio", "neutral", 58, &[("cash", "bullish"), ("btc", "bearish")]),
                sample_response("Commodity Bull", "bullish", 64, &[("cash", "bearish"), ("btc", "bullish")]),
            ],
            investor_panel_consensus: vec![
                InvestorPanelConsensus {
                    asset: "cash".to_string(),
                    bullish_count: 2,
                    bearish_count: 1,
                    neutral_count: 0,
                    label: "high-divergence".to_string(),
                },
                InvestorPanelConsensus {
                    asset: "btc".to_string(),
                    bullish_count: 1,
                    bearish_count: 2,
                    neutral_count: 0,
                    label: "high-divergence".to_string(),
                },
            ],
            ..BuildContext::default()
        };
        let out = render_private_investor_panel(&ctx).unwrap();
        assert!(out.contains("Panel consensus by asset"));
        assert!(out.contains("Druckenmiller"));
        assert!(out.contains("Dalio"));
        assert!(out.contains("Commodity Bull"));
        assert!(out.contains("Most-contested calls"));
        // Both cash and btc are high-divergence — both should appear.
        assert!(out.contains("CASH"));
        assert!(out.contains("BTC"));
    }
}
