//! Synthesis-time adversary view renderer for the daily report.
//!
//! For any asset whose latest `adversary_synthesis_views` row has
//! `fragility_score >= 3`, emit a compact markdown block that QUOTES
//! the recorded `counter_case_summary` directly (no paraphrase). The
//! adversary's text is the contract; the renderer must not reword it.
//!
//! This renderer is intentionally additive: it returns `None` when no
//! qualifying entry exists for `asset`, so the assembler can call it
//! unconditionally per asset without branching.
//!
//! See:
//!   - `agents/routines/adversary-analyst.md` — the routine that
//!     authors these rows
//!   - `AGENTS.md` — the synthesis-gating contract requiring the
//!     orchestrator to address the counter-case for fragility >= 3
//!   - `commands::adversary_synthesis` — the CLI writer
//!   - `db::adversary_synthesis_views` — the table

use anyhow::Result;

use crate::report::build::daily::BuildContext;

/// Renderer threshold: at or above this fragility score, the daily
/// report MUST surface the adversary's counter-case verbatim. Documented
/// in AGENTS.md as a soft contract for the synthesis agent / human
/// reading the report.
#[allow(dead_code)] // Consumed by the daily-report assembler hook (see AGENTS.md)
pub const ADVERSARY_FRAGILITY_THRESHOLD: i64 = 3;

/// Returns the adversary view markdown block for `asset`, or `None`
/// when no qualifying entry exists.
///
/// Lookup order:
///   1. Latest row from `ctx.synthesis_adversary_views` whose `asset`
///      matches (case-sensitive — symbol convention is upper-case).
///   2. If found and `fragility_score >= ADVERSARY_FRAGILITY_THRESHOLD`,
///      render the block.
///   3. Otherwise return `None`.
#[allow(dead_code)] // Consumed by the daily-report assembler hook (see AGENTS.md)
pub fn render_adversary_view_block(ctx: &BuildContext, asset: &str) -> Result<Option<String>> {
    let Some(view) = ctx
        .synthesis_adversary_views
        .iter()
        .find(|v| v.asset == asset)
    else {
        return Ok(None);
    };
    if view.fragility_score < ADVERSARY_FRAGILITY_THRESHOLD {
        return Ok(None);
    }

    let mut out = String::new();
    out.push_str(&format!(
        "#### Adversary view — {} (fragility {}/5)\n\n",
        view.asset, view.fragility_score
    ));
    out.push_str(&format!(
        "Convergence under challenge: {}\n\n",
        view.current_convergence_summary
    ));
    // Quote the counter-case verbatim — no paraphrase.
    out.push_str("> ");
    out.push_str(&view.counter_case_summary);
    out.push_str("\n\n");
    if !view.counter_case_evidence_points.is_empty() {
        out.push_str("Evidence:\n");
        for ev in &view.counter_case_evidence_points {
            out.push_str(&format!("- {}\n", ev));
        }
        out.push('\n');
    }
    if !view.falsification_triggers.is_empty() {
        out.push_str("Falsification triggers:\n");
        for ft in &view.falsification_triggers {
            out.push_str(&format!("- {}\n", ft));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "_Recorded {}; synthesis MUST address this counter-case._\n",
        view.recorded_at
    ));
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::AdversarySynthesisSummary;

    fn ctx_with(views: Vec<AdversarySynthesisSummary>) -> BuildContext {
        BuildContext {
            synthesis_adversary_views: views,
            ..BuildContext::default()
        }
    }

    fn view(asset: &str, fragility: i64) -> AdversarySynthesisSummary {
        AdversarySynthesisSummary {
            asset: asset.to_string(),
            current_convergence_summary: "Four-layer consensus is bullish.".to_string(),
            counter_case_summary: "But the bull case relies on a single assumption.".to_string(),
            counter_case_evidence_points: vec![
                "supporting datum a".to_string(),
                "supporting datum b".to_string(),
            ],
            falsification_triggers: vec!["triggering condition c".to_string()],
            fragility_score: fragility,
            recorded_at: "2026-06-02T18:00:00Z".to_string(),
        }
    }

    #[test]
    fn returns_none_when_no_entry_for_asset() {
        let ctx = ctx_with(vec![view("GLD", 5)]);
        assert!(render_adversary_view_block(&ctx, "BTC")
            .unwrap()
            .is_none());
    }

    #[test]
    fn returns_none_when_fragility_below_three() {
        let ctx = ctx_with(vec![view("BTC", 2)]);
        assert!(render_adversary_view_block(&ctx, "BTC")
            .unwrap()
            .is_none());
        let ctx2 = ctx_with(vec![view("BTC", 1)]);
        assert!(render_adversary_view_block(&ctx2, "BTC")
            .unwrap()
            .is_none());
    }

    #[test]
    fn returns_some_at_or_above_three_quoting_counter_case_verbatim() {
        for score in [3, 4, 5] {
            let ctx = ctx_with(vec![view("BTC", score)]);
            let block = render_adversary_view_block(&ctx, "BTC").unwrap().unwrap();
            assert!(block.contains("Adversary view — BTC"));
            assert!(block.contains(&format!("fragility {}/5", score)));
            // The contract: the counter_case_summary MUST appear verbatim,
            // prefixed by the markdown quote marker.
            assert!(block
                .contains("> But the bull case relies on a single assumption."));
            assert!(block.contains("supporting datum a"));
            assert!(block.contains("triggering condition c"));
            assert!(block.contains("synthesis MUST address"));
        }
    }

    #[test]
    fn render_omits_evidence_section_when_empty() {
        let mut v = view("BTC", 3);
        v.counter_case_evidence_points.clear();
        let ctx = ctx_with(vec![v]);
        let block = render_adversary_view_block(&ctx, "BTC").unwrap().unwrap();
        assert!(!block.contains("Evidence:\n"));
    }

    #[test]
    fn first_matching_asset_wins() {
        // `BuildContext::load` is expected to insert at most one entry per
        // asset (latest row), but the renderer should be tolerant if two
        // ever appear.
        let ctx = ctx_with(vec![view("BTC", 5), view("BTC", 2)]);
        let block = render_adversary_view_block(&ctx, "BTC").unwrap().unwrap();
        assert!(block.contains("fragility 5/5"));
    }
}
