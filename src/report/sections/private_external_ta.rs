//! Private "External TA" section.
//!
//! Renders the Phase 2c external-research agent's [synthesis-external-ta]
//! note body verbatim. The agent web-searches outside pftui's news
//! pipeline for technical analysis takes (TradingView ideas, sell-side
//! desk notes, on-chain trackers, sentiment indices, retail subs) and
//! writes a per-asset comparison of those external reads against our
//! own LOW / MEDIUM / HIGH / MACRO convergence.
//!
//! Suppressed when no external-ta note was attached this run (i.e. the
//! Phase 2c agent didn't run, or the run produced nothing).

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_external_ta(ctx: &BuildContext) -> Result<String> {
    let body = ctx
        .synthesis_notes
        .external_ta
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(body) = body else {
        return Ok(super::suppressed(
            "no [synthesis-external-ta] note attached — Phase 2c external-TA research did not run",
        ));
    };
    let mut output = String::from("## External TA & Comparison\n\n");
    output.push_str(
        "_What the outside read says — pulled from TradingView idea streams, \
         sell-side desk notes, on-chain trackers, sentiment indices, and \
         retail TA streams the Phase 2c research agent sampled this run. \
         The comparison block under each asset names where pftui's convergence \
         aligns with or diverges from the external consensus._\n\n",
    );
    output.push_str(body);
    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    #[test]
    fn suppressed_when_no_external_ta_note() {
        let ctx = BuildContext::default();
        let out = render_private_external_ta(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("synthesis-external-ta"), "unexpected reason: {reason}");
    }

    #[test]
    fn renders_body_when_present() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                external_ta: Some(
                    "## External TA — captured takes\n### BTC\n- TradingView idea (2026-06-05): bullish 65k → 80k 30d horizon".to_string(),
                ),
                ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_external_ta(&ctx).unwrap();
        assert!(out.starts_with("## External TA & Comparison"));
        assert!(out.contains("TradingView"));
    }
}
