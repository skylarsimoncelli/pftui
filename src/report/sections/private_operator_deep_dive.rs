//! Private "Operator Deep Dive" section.
//!
//! Renders the long-form essay the Phase 3b synthesis-deep-dive writer
//! produces in response to the operator's `{OPERATOR_FOCUS}` prompt.
//! Substrate: a `daily_notes` row authored by `analyst-synthesis` whose
//! body opens with `[synthesis-deep-dive` (the date suffix is part of the
//! tag so the writer doesn't have to coordinate with the renderer on a
//! single canonical header).
//!
//! Slotted in the section plan right after `private_overview` so the
//! operator's headline read leads. Suppressed when no deep-dive note
//! exists (i.e. balanced-weekly runs with no substantive focus).

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_operator_deep_dive(ctx: &BuildContext) -> Result<String> {
    let body = ctx
        .synthesis_notes
        .deep_dive
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let Some(body) = body else {
        return Ok(String::new());
    };

    let mut output = String::from("## Deep Dive\n\n");
    output.push_str(
        "_Long-form synthesis tailored to the operator's focus for this run. \
         Drawn from every Phase 1+2+3 write — not a survey, an argued take._\n\n",
    );
    output.push_str(body);
    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    #[test]
    fn suppressed_when_no_deep_dive_note() {
        let ctx = BuildContext::default();
        let out = render_private_operator_deep_dive(&ctx).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn renders_deep_dive_body_when_present() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                deep_dive: Some(
                    "BTC sits in the textbook cycle-bottom accumulation zone. Mayer multiple under 0.85, RSI 18, 25 historical precedents averaging +12.6% over 90 days. The case for waiting is the ETF outflow regime — until that breaks, the marginal bid is structural CB-on-the-bid plus passive ETF distribution. Accumulation framework: ladder buys at 65k / 60k / 55k, conviction-weighted toward the lower band. Stop the ladder if a daily close prints below 50k."
                        .to_string(),
                ),
                ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_operator_deep_dive(&ctx).unwrap();
        assert!(out.starts_with("## Deep Dive"));
        assert!(out.contains("Long-form synthesis"));
        assert!(out.contains("cycle-bottom accumulation"));
    }
}
