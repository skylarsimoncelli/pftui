#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{BuildContext, TodaysAnalystSynthesis};

pub fn render_public_executive_summary(ctx: &BuildContext) -> Result<String> {
    let mut paragraphs = Vec::new();

    // Lead with the analyst-written synthesis when today's analysts have
    // produced substantive content; fall back to the legacy regime
    // paragraph so the section is never empty.
    let lead = ctx
        .todays_analyst_synthesis
        .as_ref()
        .and_then(|s| render_synthesis_paragraph(ctx, s))
        .unwrap_or_else(|| render_regime_paragraph(ctx));
    paragraphs.push(lead);

    paragraphs.push(render_analyst_paragraph(ctx));
    paragraphs.push(render_scenario_paragraph(ctx));

    if let Some(catalyst) = ctx.news_catalysts.first() {
        paragraphs.push(render_catalyst_paragraph(catalyst));
    }

    Ok(format!(
        "## Executive Summary\n\n{}",
        paragraphs.join("\n\n")
    ))
}

fn render_synthesis_paragraph(
    ctx: &BuildContext,
    synthesis: &TodaysAnalystSynthesis,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(mv) = &synthesis.leading_move {
        let cum = mv
            .cumulative_pct
            .map(|c| format!(" (cum {c:+.1}% from baseline)"))
            .unwrap_or_default();
        parts.push(format!("Today's leading move is {} {:+.1}%{}.", mv.asset, mv.move_pct, cum));
    }
    if let Some(action) = synthesis.action_summary.as_deref() {
        parts.push(sentence(action));
    }
    // Surface a regime read so the lead still carries the classifier
    // context the operator-facing version had, but framed as the analysts'
    // narrative rather than synthetic boilerplate.
    if let Some(regime) = &ctx.regime {
        parts.push(format!(
            "Regime read: {}.",
            readable(&regime.classification)
        ));
    }
    // Include up to two analyst-layer headlines to give the lead actual
    // multi-timeframe weight.
    let mut headline_count = 0;
    for (label, headline) in [
        ("LOW analyst", synthesis.headline_low.as_deref()),
        ("MEDIUM analyst", synthesis.headline_medium.as_deref()),
        ("HIGH analyst", synthesis.headline_high.as_deref()),
        ("MACRO analyst", synthesis.headline_macro.as_deref()),
    ] {
        if headline_count >= 2 {
            break;
        }
        if let Some(text) = headline.filter(|t| !t.trim().is_empty()) {
            parts.push(format!("{label} reads: {}", sentence_fragment(text)));
            headline_count += 1;
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn render_regime_paragraph(ctx: &BuildContext) -> String {
    match (&ctx.synthesis, &ctx.regime) {
        (Some(synthesis), Some(regime)) => {
            let tension = synthesis
                .central_tension
                .as_deref()
                .unwrap_or("the dominant cross-asset tension is still forming");
            let detail = regime
                .detail
                .as_deref()
                .unwrap_or("no additional regime detail is available");
            format!(
                "pftui classifies the current regime as {}. {} The central tension is {}. {}",
                readable(&regime.classification),
                sentence(&synthesis.summary),
                tension,
                sentence(detail)
            )
        }
        (Some(synthesis), None) => {
            let tension = synthesis
                .central_tension
                .as_deref()
                .unwrap_or("the dominant cross-asset tension is still forming");
            format!(
                "pftui has a synthesis snapshot, but no current regime classification is available. {} The central tension is {}.",
                sentence(&synthesis.summary),
                tension
            )
        }
        (None, Some(regime)) => {
            let detail = regime
                .detail
                .as_deref()
                .unwrap_or("no additional regime detail is available");
            format!(
                "pftui classifies the current regime as {}, but no synthesis snapshot is available. {}",
                readable(&regime.classification),
                sentence(detail)
            )
        }
        (None, None) => "pftui does not yet have enough cached synthesis data to classify the day with confidence. Treat this report as a data-availability snapshot until the analytics refresh and analyst routines have produced current inputs.".to_string(),
    }
}

fn render_analyst_paragraph(ctx: &BuildContext) -> String {
    if ctx.analyst_convergence.is_empty() {
        return "The multi-timeframe analyst layer has not produced current convergence rows for this run. Without LOW, MEDIUM, HIGH, and MACRO agreement data, the executive view should stay provisional rather than overstating consensus.".to_string();
    }

    let highlights = ctx
        .analyst_convergence
        .iter()
        .take(3)
        .map(|row| format!("{}: {}", row.asset, sentence_fragment(&row.summary)))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "The strongest multi-timeframe reads are {}. These rows summarize where the analyst layers agree or diverge, so they should drive the deeper asset sections rather than repeated unsupported claims.",
        highlights
    )
}

fn render_scenario_paragraph(ctx: &BuildContext) -> String {
    if ctx.scenario_deltas.is_empty() {
        return "Scenario probability tracking has no active deltas for the executive summary. The scenario dashboard should still render later in the report, but the lead section should avoid inventing a directional probability story.".to_string();
    }

    let scenario = ctx
        .scenario_deltas
        .iter()
        .max_by(|a, b| {
            scenario_move_abs(a)
                .partial_cmp(&scenario_move_abs(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("checked non-empty scenario list");
    let delta = scenario
        .delta_7d
        .map(format_delta)
        .unwrap_or_else(|| "no 7-day comparison".to_string());
    format!(
        "The most important scenario input is {} at {:.0}% probability with {}. Read this as one bucket in the normalized scenario set, not as an overlapping marginal probability.",
        scenario.name, scenario.probability, delta
    )
}

fn render_catalyst_paragraph(catalyst: &crate::report::build::daily::CatalystSummary) -> String {
    let read = catalyst
        .market_read
        .as_deref()
        .unwrap_or("market impact is still being classified");
    format!(
        "The top catalyst to carry into the rest of the report is {}. {}",
        sentence_fragment(&catalyst.headline),
        sentence(read)
    )
}

fn scenario_move_abs(row: &crate::report::build::daily::ScenarioDeltaSummary) -> f64 {
    row.delta_7d.unwrap_or(0.0).abs()
}

fn format_delta(delta: f64) -> String {
    if delta > 0.0 {
        format!("a +{:.0}pp 7-day move", delta)
    } else if delta < 0.0 {
        format!("a {:.0}pp 7-day move", delta)
    } else {
        "a flat 7-day move".to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{
        AnalystConvergenceSummary, CatalystSummary, MaterialMove, RegimeSummary,
        ScenarioDeltaSummary, SynthesisSnapshot, TodaysAnalystSynthesis,
    };

    #[test]
    fn public_executive_summary_renders_fixture_context() {
        let ctx = BuildContext {
            synthesis: Some(SynthesisSnapshot {
                summary: "Liquidity is defensive while hard-money assets hold relative strength"
                    .to_string(),
                central_tension: Some(
                    "whether falling growth expectations overwhelm inflation hedges".to_string(),
                ),
            }),
            regime: Some(RegimeSummary {
                classification: "defensive_liquidity".to_string(),
                detail: Some(
                    "Cross-asset breadth is narrow and volatility is elevated".to_string(),
                ),
            }),
            analyst_convergence: vec![
                AnalystConvergenceSummary {
                    asset: "Gold".to_string(),
                    summary: "constructive across HIGH and MACRO".to_string(),
                },
                AnalystConvergenceSummary {
                    asset: "BTC".to_string(),
                    summary: "mixed because LOW is fragile".to_string(),
                },
            ],
            scenario_deltas: vec![ScenarioDeltaSummary {
                name: "Inflation Spike".to_string(),
                probability: 45.0,
                delta_7d: Some(6.0),
            }],
            news_catalysts: vec![CatalystSummary {
                headline: "Fed speakers pushed back on early easing".to_string(),
                market_read: Some("Rates stayed firm and equities lost momentum".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_executive_summary(&ctx).unwrap();

        assert!(rendered.starts_with("## Executive Summary\n\n"));
        assert!(rendered.contains("defensive liquidity"));
        assert!(rendered.contains("Gold: constructive across HIGH and MACRO"));
        assert!(rendered.contains("Inflation Spike at 45% probability"));
        assert_eq!(paragraph_count(&rendered), 4);
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_executive_summary_leads_with_analyst_synthesis_when_present() {
        let mut ctx = BuildContext {
            regime: Some(RegimeSummary {
                classification: "risk_off".to_string(),
                detail: Some("Cross-asset breadth is narrow".to_string()),
            }),
            todays_analyst_synthesis: Some(TodaysAnalystSynthesis {
                headline_low: Some(
                    "BTC -7% to $62,447 cum -14% from May 28; ETF -$671M, COT 92.3 pctile flush"
                        .to_string(),
                ),
                headline_medium: Some(
                    "Weekly: credit spreads widen as positioning unwinds".to_string(),
                ),
                headline_high: None,
                headline_macro: None,
                leading_move: Some(MaterialMove {
                    asset: "BTC".to_string(),
                    move_pct: -7.0,
                    cumulative_pct: Some(-14.0),
                    note: "ETF -$671M".to_string(),
                }),
                action_summary: Some(
                    "Position for further BTC weakness into quarter-end".to_string(),
                ),
            }),
            ..BuildContext::default()
        };
        ctx.scenario_deltas = vec![];

        let rendered = render_public_executive_summary(&ctx).unwrap();
        assert!(
            rendered.contains("Today's leading move is BTC -7.0%"),
            "leading move missing from exec summary: {rendered}"
        );
        assert!(
            rendered.contains("Position for further BTC weakness"),
            "action summary missing from exec summary: {rendered}"
        );
        assert!(
            rendered.contains("LOW analyst reads: BTC -7% to $62,447"),
            "LOW analyst headline missing: {rendered}"
        );
        // Must NOT lead with the legacy boilerplate when synthesis is present.
        assert!(
            !rendered.contains("pftui classifies the current regime as risk off"),
            "legacy boilerplate must be replaced when synthesis present: {rendered}"
        );
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_executive_summary_degrades_with_sparse_context() {
        let rendered = render_public_executive_summary(&BuildContext::default()).unwrap();

        assert!(rendered.contains("does not yet have enough cached synthesis data"));
        assert!(rendered.contains("has not produced current convergence rows"));
        assert!(rendered.contains("has no active deltas"));
        assert_eq!(paragraph_count(&rendered), 3);
        assert_public_safe(&rendered);
    }

    fn paragraph_count(markdown: &str) -> usize {
        markdown
            .split("\n\n")
            .filter(|part| !part.starts_with("## ") && !part.trim().is_empty())
            .count()
    }

    fn assert_public_safe(markdown: &str) {
        let lowered = markdown.to_ascii_lowercase();
        for forbidden in [
            "i hold",
            "we own",
            "our position",
            "cost basis",
            "unrealized",
            "transaction",
            "allocation percentage",
            "position size",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public summary leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
