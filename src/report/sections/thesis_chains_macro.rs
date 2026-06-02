//! Thesis-chain renderer for the Macro section of the daily report.
//!
//! Surfaces "active confirmed chains" and "newly disconfirmed chains" from
//! the `thesis_dependencies` graph so the Macro narrative can cite the
//! current state of the substrate's cross-asset if-then chains.
//!
//! This renderer is intentionally additive: the assembler wires it in only
//! when chains are present in the `BuildContext`. Otherwise no string is
//! emitted, preserving prior section output byte-for-byte.

use anyhow::Result;

use crate::db::thesis_dependencies::ThesisDependency;

/// Render a compact markdown block listing confirmed and disconfirmed chains.
/// Returns an empty string when no chains qualify so callers can `push_str`
/// the result unconditionally.
///
/// Wired into the private daily-report assembler as the
/// `private_macro_thesis_chains` section (sits immediately after
/// `private_macro_context`). Public mode never invokes this renderer because
/// the chain text can carry portfolio-framed antecedents — the section is
/// excluded from `public_section_plan` to preserve the privacy guard.
pub fn render_thesis_chains_block(chains: &[ThesisDependency]) -> Result<String> {
    let confirmed: Vec<&ThesisDependency> = chains
        .iter()
        .filter(|c| c.current_state == "confirmed")
        .collect();
    let disconfirmed: Vec<&ThesisDependency> = chains
        .iter()
        .filter(|c| c.current_state == "disconfirmed")
        .collect();

    if confirmed.is_empty() && disconfirmed.is_empty() {
        return Ok(String::new());
    }

    let mut out = String::new();
    out.push_str("### Cross-Asset Thesis Chains\n\n");
    if !confirmed.is_empty() {
        out.push_str("Active confirmed chains:\n");
        for c in &confirmed {
            out.push_str(&format!(
                "- #{}: `{}` → `{}` ({})\n",
                c.id, c.antecedent_text, c.consequent_text, c.relation,
            ));
        }
        out.push('\n');
    }
    if !disconfirmed.is_empty() {
        out.push_str("Newly disconfirmed chains:\n");
        for c in &disconfirmed {
            out.push_str(&format!(
                "- #{}: `{}` → `{}` ({})\n",
                c.id, c.antecedent_text, c.consequent_text, c.relation,
            ));
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(id: i64, state: &str, ant: &str, cons: &str) -> ThesisDependency {
        ThesisDependency {
            id,
            antecedent_id: None,
            antecedent_text: ant.to_string(),
            relation: "implies".to_string(),
            consequent_id: None,
            consequent_text: cons.to_string(),
            evidence_count: 1,
            conviction: None,
            source_lesson_ids: None,
            source_thesis_sections: None,
            current_state: state.to_string(),
            last_validated_at: None,
            created_at: "2026-06-02T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn empty_input_emits_empty_string() {
        let out = render_thesis_chains_block(&[]).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn only_open_chains_emits_empty_string() {
        let chains = vec![fake(1, "open", "X > 1", "Y > 2")];
        let out = render_thesis_chains_block(&chains).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn confirmed_and_disconfirmed_emit_separate_blocks() {
        let chains = vec![
            fake(1, "confirmed", "XAU > 4500", "BTC > 100000"),
            fake(2, "disconfirmed", "DXY > 104", "GOLD > 4500"),
            fake(3, "open", "ignored > 1", "skip > 2"),
        ];
        let out = render_thesis_chains_block(&chains).unwrap();
        assert!(out.contains("Active confirmed chains"));
        assert!(out.contains("Newly disconfirmed chains"));
        assert!(out.contains("#1"));
        assert!(out.contains("#2"));
        assert!(!out.contains("#3"));
    }
}
