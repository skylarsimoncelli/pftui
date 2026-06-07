//! Private "Overview / Week-in-Review" opening section.
//!
//! Sets the tone of the report with a human-readable, engaging summary of
//! what happened in markets, news, and data over the period. The substrate
//! is the `analyst-synthesis` `daily_notes` row with `section='synthesis-
//! economy'`. Promoting this from a sub-block of the synthesis section to a
//! standalone opening section is the operator's explicit ask: "the report
//! should always open with an overview section. human readable, engaging,
//! high level discussion on what has happened in markets, news, data."

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_overview(ctx: &BuildContext) -> Result<String> {
    let economy = ctx
        .synthesis_notes
        .economy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Suppress entirely when no synthesis-economy note is attached. The
    // operator's tolerance for empty-state filler is low; a missing overview
    // is better than a placeholder that adds no signal.
    let Some(economy) = economy else {
        return Ok(String::new());
    };

    let mut output = String::from("## Overview — Week in Review\n\n");
    output.push_str(
        "_What happened this week in markets, news, and data. Drawn from the macro \
         layer + investor-panel macro consensus + 7d tape delta. Read this first._\n\n",
    );
    output.push_str(economy);
    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    #[test]
    fn suppressed_when_no_economy_note() {
        let ctx = BuildContext::default();
        let out = render_private_overview(&ctx).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn renders_economy_when_present() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: Some("Hot NFP reinforces the dollar bid this week.".to_string()),
                assets: vec![],
            },
            ..BuildContext::default()
        };
        let out = render_private_overview(&ctx).unwrap();
        assert!(out.contains("## Overview — Week in Review"));
        assert!(out.contains("Hot NFP reinforces the dollar bid"));
    }

    #[test]
    fn suppressed_when_economy_is_whitespace() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: Some("   ".to_string()),
                assets: vec![],
            },
            ..BuildContext::default()
        };
        let out = render_private_overview(&ctx).unwrap();
        assert!(out.is_empty());
    }
}
