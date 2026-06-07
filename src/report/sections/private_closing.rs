//! Private "Closing — Gameplan / Portfolio Reflection / What to Watch" section.
//!
//! Reads the `[synthesis-closing]` daily_note from `analyst-synthesis` and
//! renders the body verbatim. The synthesis writer authors the closing
//! conclusion as 300-500 word prose covering: the gameplan for the coming
//! week, what the current allocation says about the bet on the table, and
//! the top 3-5 falsifiable triggers to watch over the next 5-10 sessions.
//!
//! Final section in the private report — the operator's takeaway.

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_closing(ctx: &BuildContext) -> Result<String> {
    let body = ctx
        .synthesis_notes
        .closing
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(body) = body else {
        return Ok(String::new());
    };
    let mut output = String::from("## Closing — Gameplan, Portfolio, What to Watch\n\n");
    output.push_str(
        "_The week's takeaway: what to do, what the portfolio is currently \
         betting, what would force a rethink. Written last, read first when \
         the operator wants the punchline._\n\n",
    );
    output.push_str(body);
    Ok(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    #[test]
    fn suppressed_when_no_closing_note() {
        let ctx = BuildContext::default();
        let out = render_private_closing(&ctx).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn renders_closing_body_when_present() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                closing: Some(
                    "Gameplan: accumulate gold + BTC in tranches into next week's CPI binary. Spend cash slowly. Watch: BTC daily close below 50k, DXY weekly above 101, soft CPI below 2.6%."
                        .to_string(),
                ),
                ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_closing(&ctx).unwrap();
        assert!(out.starts_with("## Closing"));
        assert!(out.contains("Gameplan"));
        assert!(out.contains("DXY weekly above 101"));
    }
}
