#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BinaryCatalystSummary, BuildContext, DerivedActionSummary, PrivatePortfolioSnapshotSummary,
};

pub fn render_private_bottom_line(ctx: &BuildContext) -> Result<String> {
    let mut bullets = vec![
        render_regime_bullet(ctx),
        render_portfolio_bullet(ctx.private_portfolio_snapshot.as_ref()),
        render_actions_bullet(&ctx.private_derived_actions),
        render_catalyst_bullet(&ctx.private_binary_catalysts),
    ];

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
    for bullet in bullets.into_iter().take(5) {
        output.push_str(&format!("- {}\n", sentence_fragment(&bullet)));
    }
    output.push_str("\n<!-- what_changed_strip - diff vs prior report -->\n");
    output.push_str("{what_changed_strip(deltas)}");

    Ok(output.trim_end().to_string())
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
        .unwrap_or("daily P&L unavailable");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{RegimeSummary, SynthesisSnapshot, WhatChangedDeltaSummary};

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
        assert!(rendered.contains("{what_changed_strip(deltas)}"));
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
