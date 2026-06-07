#![allow(dead_code)]
//! Private "Synthesis — Bull / Bear / What Would Change My Mind" section.
//!
//! Renders the synthesis-writer pass's decision-ready digest: a per-asset
//! bull case / bear case / what-would-change-my-mind / risk-reward block,
//! preceded by an "economy this week" paragraph. The substrate is the
//! `analyst-synthesis` `daily_notes` rows parsed into [`SynthesisNotes`] by
//! the assembler; this renderer only formats them.

use anyhow::Result;

use crate::report::build::daily::{BuildContext, SynthesisAssetNote};

/// The four structured tags the synthesis writer emits in each per-asset
/// note body. We bold them so the PDF renders clear sub-headers without the
/// writer having to hand-format markdown.
const TAGS: [&str; 4] = [
    "BULL CASE",
    "BEAR CASE",
    "WHAT WOULD CHANGE MY MIND",
    "RISK / REWARD",
];

pub fn render_private_synthesis(ctx: &BuildContext) -> Result<String> {
    let mut output =
        String::from("## Synthesis — Bull / Bear / What Would Change My Mind\n\n");

    let notes = &ctx.synthesis_notes;
    if notes.economy.is_none() && notes.assets.is_empty() {
        output.push_str(
            "No synthesis digest was written for today. The synthesis-writer pass \
            produces this section from `analyst-synthesis` notes; on runs where it \
            did not execute, the per-asset convergence cards below carry the layer \
            reasoning instead.",
        );
        return Ok(output);
    }

    output.push_str(
        "_Decision-ready digest synthesized from the four timeframe layers, the \
        adversary, and the investor panel. Risk/reward is a 7-day expected-value \
        sketch._\n\n",
    );

    if let Some(economy) = &notes.economy {
        let economy = economy.trim();
        if !economy.is_empty() {
            output.push_str("### Economy this week\n\n");
            output.push_str(economy);
            output.push_str("\n\n");
        }
    }

    for asset in &notes.assets {
        output.push_str(&render_asset_block(asset));
    }

    Ok(output.trim_end().to_string())
}

fn render_asset_block(asset: &SynthesisAssetNote) -> String {
    let mut block = format!("### {}\n\n", asset.symbol.trim());
    block.push_str(&bold_tags(&asset.body));
    block.push_str("\n\n");
    block
}

/// Bold each recognised tag at the start of a line, e.g. turn
/// `BULL CASE:` (or `RISK / REWARD (next 7 days):`) into `**BULL CASE:**`.
/// The synthesis writer typically puts the tag on its own line with the
/// content on the following line(s); we join a lone tag onto its first
/// content line so it renders as an inline bold lead-in. Lines that don't
/// start with a tag pass through unchanged.
fn bold_tags(body: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    // A bolded tag awaiting its content (the tag sat alone on its line).
    let mut pending: Option<String> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some((label, rest)) = match_tag(trimmed) {
            if let Some(p) = pending.take() {
                out.push(p);
            }
            if rest.is_empty() {
                pending = Some(format!("**{label}:**"));
            } else {
                out.push(format!("**{label}:** {rest}"));
            }
            continue;
        }
        if trimmed.is_empty() {
            if let Some(p) = pending.take() {
                out.push(p);
            }
            out.push(String::new());
            continue;
        }
        match pending.take() {
            Some(p) => out.push(format!("{p} {trimmed}")),
            None => out.push(line.to_string()),
        }
    }
    if let Some(p) = pending.take() {
        out.push(p);
    }
    out.join("\n")
}

/// If `line` starts with one of the recognised [`TAGS`] followed by a colon
/// (possibly after a parenthetical like " (next 7 days)"), return the label
/// up to (excluding) the colon and the trimmed remainder after it.
fn match_tag(line: &str) -> Option<(String, String)> {
    let upper = line.to_ascii_uppercase();
    let tag = TAGS.iter().find(|t| upper.starts_with(**t))?;
    let colon = line.find(':')?;
    // Guard against a colon appearing before the tag's own terminator would
    // be unusual; the tags contain no colon so the first colon closes them.
    let _ = tag;
    let label = line[..colon].trim_end().to_string();
    let rest = line[colon + 1..].trim_start().to_string();
    Some((label, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    fn note(symbol: &str, body: &str) -> SynthesisAssetNote {
        SynthesisAssetNote {
            symbol: symbol.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn renders_empty_state_when_no_notes() {
        let ctx = BuildContext::default();
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(out.starts_with("## Synthesis"));
        assert!(out.contains("No synthesis digest"));
    }

    #[test]
    fn renders_economy_and_asset_blocks() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: Some("Hot NFP reinforces the dollar bid.".to_string()),
                assets: vec![note(
                    "BTC",
                    "BULL CASE:\nCapitulation zone, RSI 18.\n\nBEAR CASE:\nFlush not finished.\n\nWHAT WOULD CHANGE MY MIND:\nA net-positive ETF day.\n\nRISK / REWARD (next 7 days):\nEV -0.8%.",
                )],
            },
            ..BuildContext::default()
        };
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(out.contains("### Economy this week"));
        assert!(out.contains("Hot NFP reinforces the dollar bid."));
        assert!(out.contains("### BTC"));
        // Tags are bolded.
        assert!(out.contains("**BULL CASE:** Capitulation zone, RSI 18."));
        assert!(out.contains("**BEAR CASE:** Flush not finished."));
        assert!(out.contains("**WHAT WOULD CHANGE MY MIND:** A net-positive ETF day."));
        // Parenthetical on the RR tag is preserved inside the bold span.
        assert!(out.contains("**RISK / REWARD (next 7 days):** EV -0.8%."));
        // Economy precedes the asset block.
        let econ = out.find("Economy this week").unwrap();
        let btc = out.find("### BTC").unwrap();
        assert!(econ < btc);
    }

    #[test]
    fn renders_asset_only_when_no_economy() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: None,
                assets: vec![note("GC=F", "BULL CASE:\nMonetary bid intact.")],
            },
            ..BuildContext::default()
        };
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(!out.contains("### Economy this week"));
        assert!(out.contains("### GC=F"));
        assert!(out.contains("**BULL CASE:** Monetary bid intact."));
    }

    #[test]
    fn passes_through_untagged_lines() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: None,
                assets: vec![note("USD", "Plain prose with no tag line.")],
            },
            ..BuildContext::default()
        };
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(out.contains("Plain prose with no tag line."));
        assert!(!out.contains("**Plain"));
    }
}
