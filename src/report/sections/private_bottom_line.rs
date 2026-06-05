#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BinaryCatalystSummary, BuildContext, DerivedActionSummary, MaterialMove,
    PrivatePortfolioSnapshotSummary,
};
use crate::report::charts::what_changed_strip::{
    render_svg as what_changed_strip_svg, WhatChangedDelta, WhatChangedStripInput,
};

pub fn render_private_bottom_line(ctx: &BuildContext) -> Result<String> {
    let synthesis = ctx.todays_analyst_synthesis.as_ref();
    // First bullet: rich analyst-written leading-move line when present;
    // otherwise the legacy regime bullet so the section never goes silent.
    let first_bullet = synthesis
        .and_then(|s| s.leading_move.as_ref())
        .map(render_leading_move_bullet)
        .unwrap_or_else(|| render_regime_bullet(ctx));

    // Third bullet: synthesis-bound action summary when present; otherwise
    // the legacy derived-actions bullet.
    let third_bullet = synthesis
        .and_then(|s| s.action_summary.as_deref())
        .map(|summary| format!("Action: {}", sentence_fragment(summary)))
        .unwrap_or_else(|| render_actions_bullet(&ctx.private_derived_actions));

    let mut bullets = vec![
        first_bullet,
        render_portfolio_bullet(ctx.private_portfolio_snapshot.as_ref()),
        third_bullet,
        render_catalyst_bullet(&ctx.private_binary_catalysts),
    ];

    // Append per-analyst headline excerpts as separate bullets when the
    // synthesis exposes them. Each is already truncated to ~200 chars by
    // the loader.
    if let Some(s) = synthesis {
        for (label, headline) in [
            ("LOW", s.headline_low.as_deref()),
            ("MEDIUM", s.headline_medium.as_deref()),
            ("HIGH", s.headline_high.as_deref()),
            ("MACRO", s.headline_macro.as_deref()),
        ] {
            if let Some(text) = headline.filter(|t| !t.trim().is_empty()) {
                bullets.push(format!("{label}: {}", sentence_fragment(text)));
            }
        }
    }

    if !ctx.private_what_changed_deltas.is_empty() {
        bullets.push(format!(
            "What changed: {} material delta{} attached for the native strip below.",
            ctx.private_what_changed_deltas.len(),
            if ctx.private_what_changed_deltas.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }

    let mut output = String::from("## Bottom Line\n\n");
    // When the analyst synthesis is present the section can carry up to 4
    // extra headline bullets (one per timeframe layer) on top of the four
    // core bullets — keep them all so the lead doesn't drop substantive
    // analyst content.
    let bullet_cap = if synthesis.is_some() { 9 } else { 5 };
    for bullet in bullets.into_iter().take(bullet_cap) {
        output.push_str(&format!("- {}\n", sentence_fragment(&bullet)));
    }
    let strip_svg = render_what_changed_strip(ctx);
    if !strip_svg.is_empty() {
        output.push_str("\n<!-- what_changed_strip - diff vs prior report -->\n");
        output.push_str(&strip_svg);
    }

    Ok(output.trim_end().to_string())
}

fn render_leading_move_bullet(mv: &MaterialMove) -> String {
    let cum = mv
        .cumulative_pct
        .map(|c| format!(" (cum {c:+.1}% from baseline)"))
        .unwrap_or_default();
    let note = clean_text(&mv.note);
    let note_part = if note.is_empty() {
        String::new()
    } else {
        format!(" {}", sentence(&note))
    };
    format!(
        "**{} {:+.1}%**{}.{}",
        clean_text(&mv.asset),
        mv.move_pct,
        cum,
        note_part
    )
}

fn render_regime_bullet(ctx: &BuildContext) -> String {
    match (&ctx.regime, &ctx.synthesis) {
        (Some(regime), Some(synthesis)) => format!(
            "Regime: {}. {}",
            readable(&regime.classification),
            sentence(&synthesis.summary)
        ),
        (Some(regime), None) => {
            let detail = regime
                .detail
                .as_deref()
                .unwrap_or("no regime detail attached");
            format!(
                "Regime: {}. {}",
                readable(&regime.classification),
                sentence(detail)
            )
        }
        (None, Some(synthesis)) => {
            format!("Regime: unclassified. {}", sentence(&synthesis.summary))
        }
        (None, None) => {
            "Regime: no current regime or synthesis snapshot is attached to this private build"
                .to_string()
        }
    }
}

fn render_portfolio_bullet(snapshot: Option<&PrivatePortfolioSnapshotSummary>) -> String {
    let Some(snapshot) = snapshot else {
        return "Portfolio: no private portfolio snapshot is attached, so P&L context is unavailable"
            .to_string();
    };

    let total = snapshot
        .total_value
        .as_deref()
        .unwrap_or("total unavailable");
    let pnl = snapshot
        .daily_pnl
        .as_deref()
        .unwrap_or("unavailable");
    let pct = snapshot
        .daily_pnl_pct
        .map(|value| format!(" ({value:+.2}%)"))
        .unwrap_or_default();
    let allocation = snapshot
        .allocation_summary
        .as_deref()
        .map(|value| format!(" {}", sentence(value)))
        .unwrap_or_default();
    format!("Portfolio: {total}; day P&L {pnl}{pct}.{allocation}")
}

fn render_actions_bullet(actions: &[DerivedActionSummary]) -> String {
    if actions.is_empty() {
        return "Actions: no derived ADD/TRIM/HOLD rows are attached for today".to_string();
    }

    let joined = actions
        .iter()
        .take(2)
        .map(|action| {
            format!(
                "{} {} [{}]: {}",
                clean_text(&action.action).to_ascii_uppercase(),
                clean_text(&action.asset),
                clean_text(&action.urgency),
                sentence_fragment(&action.rationale)
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("Actions: {joined}")
}

fn render_catalyst_bullet(catalysts: &[BinaryCatalystSummary]) -> String {
    if catalysts.is_empty() {
        return "Catalysts: no binary catalyst row is attached for the next week".to_string();
    }

    let catalyst = &catalysts[0];
    format!(
        "Catalyst: {} on {}. {}",
        clean_text(&catalyst.event),
        clean_text(&catalyst.date),
        sentence(&catalyst.impact)
    )
}

fn readable(value: &str) -> String {
    value.replace(['_', '-'], " ")
}

fn sentence(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

fn sentence_fragment(value: &str) -> String {
    value.trim().trim_end_matches(['.', '!', '?']).to_string()
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn render_what_changed_strip(ctx: &BuildContext) -> String {
    if ctx.private_what_changed_deltas.is_empty() {
        return String::new();
    }
    let deltas = ctx
        .private_what_changed_deltas
        .iter()
        .map(|row| WhatChangedDelta {
            label: row.label.clone(),
            delta_str: row.delta.clone(),
            direction: row.direction.clone(),
        })
        .collect();
    what_changed_strip_svg(&WhatChangedStripInput {
        deltas,
        width: None,
        height: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{
        MaterialMove, RegimeSummary, SynthesisSnapshot, TodaysAnalystSynthesis,
        WhatChangedDeltaSummary,
    };

    #[test]
    fn private_bottom_line_bullets_cover_regime_action_and_catalyst() {
        let ctx = fixture_context();

        let rendered = render_private_bottom_line(&ctx).unwrap();

        assert!(rendered.starts_with("## Bottom Line\n\n"));
        assert!(rendered.contains("- Regime: defensive liquidity."));
        assert!(rendered.contains("- Portfolio: total 100,000 USD; day P&L +1,250 USD (+1.25%)."));
        assert!(rendered.contains("- Actions: ADD BTC [today]: allocation drift is below target band; TRIM QQQ [watch]: risk budget is crowded"));
        assert!(rendered.contains("- Catalyst: FOMC decision on 2026-06-03."));
        assert_eq!(
            rendered
                .lines()
                .filter(|line| line.starts_with("- "))
                .count(),
            5
        );
    }

    #[test]
    fn private_bottom_line_embeds_native_what_changed_helper() {
        let rendered = render_private_bottom_line(&fixture_context()).unwrap();

        assert!(rendered.contains("<!-- what_changed_strip - diff vs prior report -->"));
        // Renderer now emits inline SVG, not the {what_changed_strip(deltas)} token.
        assert!(
            !rendered.contains("{what_changed_strip"),
            "must not leak the token placeholder: {rendered}"
        );
        assert!(
            rendered.contains("<svg"),
            "expected inline SVG for the what-changed strip: {rendered}"
        );
    }

    #[test]
    fn private_bottom_line_omits_strip_when_no_deltas() {
        let mut ctx = fixture_context();
        ctx.private_what_changed_deltas = vec![];
        let rendered = render_private_bottom_line(&ctx).unwrap();
        assert!(!rendered.contains("<!-- what_changed_strip"));
        assert!(!rendered.contains("<svg"));
    }

    #[test]
    fn private_bottom_line_surfaces_todays_analyst_synthesis() {
        let mut ctx = fixture_context();
        ctx.todays_analyst_synthesis = Some(TodaysAnalystSynthesis {
            headline_low: Some(
                "BTC -7% to $62,447 cum -14% from May 28; ETF -$671M, COT 92.3 pctile flush"
                    .to_string(),
            ),
            headline_medium: Some("Weekly: rates pricing eases but credit spreads widen".to_string()),
            headline_high: None,
            headline_macro: Some("Macro: dollar squeeze through quarter-end is the dominant tape".to_string()),
            leading_move: Some(MaterialMove {
                asset: "BTC".to_string(),
                move_pct: -7.0,
                cumulative_pct: Some(-14.0),
                note: "ETF -$671M, COT 92.3 pctile flush".to_string(),
            }),
            action_summary: Some("Trim BTC exposure into strength; raise stop to $61.5k".to_string()),
        });

        let rendered = render_private_bottom_line(&ctx).unwrap();

        assert!(
            rendered.contains("**BTC -7.0%**"),
            "leading move bullet missing: {rendered}"
        );
        assert!(
            rendered.contains("cum -14.0% from baseline"),
            "cumulative framing missing: {rendered}"
        );
        assert!(
            rendered.contains("- Action: Trim BTC exposure into strength"),
            "action summary missing: {rendered}"
        );
        assert!(
            rendered.contains("- LOW: BTC -7% to $62,447"),
            "LOW headline excerpt missing: {rendered}"
        );
        assert!(
            rendered.contains("- MEDIUM: Weekly: rates pricing eases"),
            "MEDIUM headline excerpt missing: {rendered}"
        );
        assert!(
            rendered.contains("- MACRO: Macro: dollar squeeze"),
            "MACRO headline excerpt missing: {rendered}"
        );
        // Synthesis must NOT clobber the catalyst/portfolio fallback bullets.
        assert!(rendered.contains("- Portfolio: total 100,000 USD"));
        assert!(rendered.contains("- Catalyst: FOMC decision"));
    }

    #[test]
    fn private_bottom_line_falls_back_when_synthesis_absent() {
        let mut ctx = fixture_context();
        ctx.todays_analyst_synthesis = None;
        let rendered = render_private_bottom_line(&ctx).unwrap();
        // Legacy regime + actions bullets still present.
        assert!(rendered.contains("- Regime: defensive liquidity."));
        assert!(rendered.contains("- Actions: ADD BTC [today]"));
    }

    #[test]
    fn private_bottom_line_is_not_public_mode_content() {
        let rendered = render_private_bottom_line(&fixture_context()).unwrap();

        assert!(rendered.contains("## Bottom Line"));
        assert!(!rendered.contains("## Executive Summary"));
        assert!(!rendered.contains("## Methodology"));
        assert!(!rendered.contains("PFTUI Intelligence Report | pftui.dev"));
        assert!(!rendered.contains("for informational purposes only"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            regime: Some(RegimeSummary {
                classification: "defensive_liquidity".to_string(),
                detail: Some("volatility is elevated".to_string()),
            }),
            synthesis: Some(SynthesisSnapshot {
                summary: "cash and hard-asset hedges are leading risk assets".to_string(),
                central_tension: None,
            }),
            private_portfolio_snapshot: Some(PrivatePortfolioSnapshotSummary {
                total_value: Some("total 100,000 USD".to_string()),
                daily_pnl: Some("+1,250 USD".to_string()),
                daily_pnl_pct: Some(1.25),
                allocation_summary: Some("cash buffer is still above floor".to_string()),
            }),
            private_derived_actions: vec![
                action(
                    "BTC",
                    "ADD",
                    "today",
                    "allocation drift is below target band",
                ),
                action("QQQ", "TRIM", "watch", "risk budget is crowded"),
            ],
            private_binary_catalysts: vec![BinaryCatalystSummary {
                date: "2026-06-03".to_string(),
                event: "FOMC decision".to_string(),
                impact: "Rates repricing could flip the equity risk signal".to_string(),
            }],
            private_what_changed_deltas: vec![WhatChangedDeltaSummary {
                label: "BTC".to_string(),
                delta: "+3.2%".to_string(),
                direction: "bull".to_string(),
            }],
            ..BuildContext::default()
        }
    }

    fn action(asset: &str, action: &str, urgency: &str, rationale: &str) -> DerivedActionSummary {
        DerivedActionSummary {
            asset: asset.to_string(),
            action: action.to_string(),
            urgency: urgency.to_string(),
            rationale: rationale.to_string(),
        }
    }
}
