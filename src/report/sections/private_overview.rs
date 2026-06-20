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
        if let Some(fallback) = deterministic_overview(ctx) {
            return Ok(fallback);
        }
        return Ok(super::suppressed(
            "no [synthesis-economy] note for the report date",
        ));
    };

    let mut output = String::from("## Overview — Week in Review\n\n");
    output.push_str(
        "_What happened this week in markets, news, and data. Drawn from the macro \
         layer + investor-panel macro consensus + 7d tape delta. Read this first._\n\n",
    );
    output.push_str(economy);
    Ok(output.trim_end().to_string())
}

fn deterministic_overview(ctx: &BuildContext) -> Option<String> {
    if ctx.market_snapshot.is_empty()
        && ctx.scenario_deltas.is_empty()
        && ctx.news_catalysts.is_empty()
    {
        return None;
    }

    let mut output = String::from("## Overview — State of Market\n\n");
    output.push_str(
        "_Deterministic fallback from cached market, scenario, and catalyst data; no same-day synthesis note was attached._\n\n",
    );

    let mut moves = ctx
        .market_snapshot
        .iter()
        .filter_map(|row| {
            let change = row.daily_change_pct.or(row.weekly_change_pct)?;
            Some((change.abs(), row, change))
        })
        .collect::<Vec<_>>();
    moves.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if !moves.is_empty() {
        output.push_str("Market tape: ");
        let parts = moves
            .iter()
            .take(4)
            .map(|(_, row, change)| {
                let price = row.price.as_deref().unwrap_or("n/a");
                let signal = row.signal.as_deref().unwrap_or("no cached signal");
                format!("{} {} ({:+.1}%, {})", row.asset, price, change, signal)
            })
            .collect::<Vec<_>>();
        output.push_str(&parts.join("; "));
        output.push_str(".\n\n");
    }

    let mut deltas = ctx
        .scenario_deltas
        .iter()
        .filter_map(|row| row.delta_7d.map(|delta| (delta.abs(), row, delta)))
        .collect::<Vec<_>>();
    deltas.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if !deltas.is_empty() {
        output.push_str("Scenario movement: ");
        let parts = deltas
            .iter()
            .take(3)
            .map(|(_, row, delta)| {
                format!("{} {:.0}% ({:+.1}pp 7d)", row.name, row.probability, delta)
            })
            .collect::<Vec<_>>();
        output.push_str(&parts.join("; "));
        output.push_str(".\n\n");
    }

    if !ctx.news_catalysts.is_empty() {
        output.push_str("Catalysts: ");
        let parts = ctx
            .news_catalysts
            .iter()
            .take(3)
            .map(|row| match row.market_read.as_deref() {
                Some(read) if !read.trim().is_empty() => {
                    format!("{} ({})", row.headline, read.trim())
                }
                _ => row.headline.clone(),
            })
            .collect::<Vec<_>>();
        output.push_str(&parts.join("; "));
        output.push('.');
    }

    Some(output.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::SynthesisNotes;

    #[test]
    fn suppressed_when_no_economy_note() {
        let ctx = BuildContext::default();
        let out = render_private_overview(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("synthesis-economy"), "unexpected reason: {reason}");
    }

    #[test]
    fn renders_economy_when_present() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: Some("Hot NFP reinforces the dollar bid this week.".to_string()),
                assets: vec![],
            ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_overview(&ctx).unwrap();
        assert!(out.contains("## Overview — Week in Review"));
        assert!(out.contains("Hot NFP reinforces the dollar bid"));
    }

    #[test]
    fn fallback_renders_when_cached_market_context_exists() {
        let ctx = BuildContext {
            market_snapshot: vec![crate::report::build::daily::MarketSnapshotRow {
                asset: "BTC".to_string(),
                price: Some("$100,000".to_string()),
                daily_change_pct: Some(2.4),
                weekly_change_pct: None,
                signal: Some("risk bid".to_string()),
            }],
            ..BuildContext::default()
        };
        let out = render_private_overview(&ctx).unwrap();
        assert!(out.contains("## Overview — State of Market"));
        assert!(out.contains("BTC"));
    }

    #[test]
    fn suppressed_when_economy_is_whitespace() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: Some("   ".to_string()),
                assets: vec![],
            ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        let out = render_private_overview(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("synthesis-economy"), "unexpected reason: {reason}");
    }
}
