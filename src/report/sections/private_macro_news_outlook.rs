//! Private "Macro & News Outlook" section.
//!
//! Reads the `[synthesis-macro-outlook]` daily_note from `analyst-synthesis`
//! and renders the body verbatim. Replaces the previous architecture of a
//! standalone atomic-data Macro Context block + a separate News & Catalysts
//! table. Per operator: "less unformatted data and walls of text" — the
//! synthesis writer combines the macro tape, the next-week calendar, the
//! connected news themes, the adversary's read, and the panel's macro
//! consensus into one 300-500 word prose block the operator can read
//! quickly.
//!
//! Suppressed entirely when no macro-outlook note exists for the run.

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_macro_news_outlook(ctx: &BuildContext) -> Result<String> {
    let body = ctx
        .synthesis_notes
        .macro_outlook
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(body) = body else {
        return Ok(super::suppressed(
            "no [synthesis-macro-outlook] note for the report date",
        ));
    };
    let mut output = String::from("## Macro & News Outlook\n\n");
    output.push_str(
        "_What the tape, the calendar, and the connected news are saying — \
         synthesized from the macro layer + panel macro consensus + connected \
         news themes. The next-week catalyst slate is named inline; binary \
         dates are in bold._\n\n",
    );
    output.push_str(body);
    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    #[test]
    fn suppressed_when_no_macro_outlook_note() {
        let ctx = BuildContext::default();
        let out = render_private_macro_news_outlook(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("synthesis-macro-outlook"), "unexpected reason: {reason}");
    }

    #[test]
    fn renders_macro_outlook_body_when_present() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                macro_outlook: Some(
                    "The macro tape this week was dollar-up squeeze layered on an AI/tech de-rate. CPI Jun 10 and FOMC Jun 16-17 are the binary catalysts."
                        .to_string(),
                ),
                ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_macro_news_outlook(&ctx).unwrap();
        assert!(out.starts_with("## Macro & News Outlook"));
        assert!(out.contains("dollar-up squeeze"));
    }
}
