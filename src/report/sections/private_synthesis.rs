#![allow(dead_code)]
//! Private "Per-Asset Briefing" section.
//!
//! Restructured 2026-06-07 per operator feedback: the per-asset block used
//! to be a wall of "vaguely related information caveating itself", split
//! across two separate sections (Synthesis + Per-Asset Convergence). The
//! operator asked for one card per asset with five clean sub-blocks:
//!
//!   - **Overview** — what happened this week
//!   - **Current bias** — the bias / conviction / feel right now
//!   - **Bull case / Bear case**
//!   - **Key levels & technicals**
//!   - **What to watch / What would change my mind**
//!
//! The substrate is unchanged — `analyst-synthesis` notes carry the Bull/
//! Bear/Change-Mind/RR blocks, `private_asset_intelligence` carries the
//! levels & technicals blob, and `private_asset_convergence` carries the
//! four-layer convictions. This renderer combines them into one card.
//!
//! The Overview/Economy paragraph moved to the new `private_overview`
//! section. The 4-layer convergence section was removed from the section
//! plan (its data feeds into the Current bias sub-block here).

use anyhow::Result;

use crate::report::build::daily::{
    AssetIntelligenceBlob, BuildContext, PrivateAssetConvergenceRow,
    PrivateAssetConvergenceView, SynthesisAssetNote,
};

/// Compact portfolio-context line for one held asset: allocation, daily change,
/// unrealized P&L, and drift vs target band. Reads the already-loaded
/// `private_positions` + `private_drift_rows` slots. Returns None when the asset
/// isn't a held position (so non-portfolio assets render analysis-only).
fn render_position_context(symbol: &str, ctx: &BuildContext) -> Option<String> {
    let pos = ctx
        .private_positions
        .iter()
        .find(|p| p.symbol.eq_ignore_ascii_case(symbol))?;
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{:.1}% of book", pos.allocation_pct));
    if let Some(px) = pos.price.as_deref() {
        let chg = pos
            .daily_change
            .as_deref()
            .map(|c| format!(" ({c}/day)"))
            .unwrap_or_default();
        parts.push(format!("${px}{chg}"));
    }
    if let Some(pnl) = pos.unrealized_pnl.as_deref() {
        parts.push(format!("unrealized {pnl}"));
    }
    if let Some(drift) = ctx
        .private_drift_rows
        .iter()
        .find(|d| d.symbol.eq_ignore_ascii_case(symbol))
    {
        let gap = drift.actual_pct - drift.target_pct;
        let status = if gap.abs() <= drift.band_pct {
            "in band"
        } else if gap > 0.0 {
            "over"
        } else {
            "under"
        };
        parts.push(format!(
            "target {:.0}% ({} {:+.1}pp)",
            drift.target_pct, status, gap
        ));
    }
    Some(parts.join(" · "))
}

const TAGS: [&str; 4] = [
    "BULL CASE",
    "BEAR CASE",
    "WHAT WOULD CHANGE MY MIND",
    "RISK / REWARD",
];

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;

pub fn render_private_synthesis(ctx: &BuildContext) -> Result<String> {
    let notes = &ctx.synthesis_notes;
    if notes.assets.is_empty() {
        // Suppress the per-asset section entirely when synthesis didn't
        // write any per-asset notes. Empty-state filler bloated prior
        // reports.
        return Ok(super::suppressed(
            "no per-asset [synthesis-SYM] notes for the report date",
        ));
    }

    let mut output = String::from("## Per-Asset Briefing\n\n");
    output.push_str(
        "_One card per held asset. Each card carries the same five blocks: \
         what happened this week, current bias across the four timeframe layers, \
         bull case, bear case, key levels + technicals, and what would change \
         the read. Sources: synthesis writer + four-layer convergence + \
         per-asset technical snapshot._\n\n",
    );

    for asset in &notes.assets {
        output.push_str(&render_asset_card(asset, ctx));
    }

    Ok(output.trim_end().to_string())
}

fn render_asset_card(asset: &SynthesisAssetNote, ctx: &BuildContext) -> String {
    let symbol = asset.symbol.trim();
    let mut block = format!("### {symbol}\n\n");

    let parsed = parse_synthesis_body(&asset.body);
    let convergence = find_convergence(&ctx.private_asset_convergence, symbol);
    let intelligence = ctx.private_asset_intelligence.get(symbol);

    // Portfolio context first — the 2026-06-25 reshape folds the operator's
    // own position for this asset into its card (position/alloc/drift/unrealized)
    // so each asset section is a self-contained operating block.
    if let Some(position) = render_position_context(symbol, ctx) {
        block.push_str("**Position**\n\n");
        block.push_str(&position);
        block.push_str("\n\n");
    }

    if let Some(overview) = render_overview(asset, ctx, &parsed) {
        block.push_str("**Overview — this week**\n\n");
        block.push_str(&overview);
        block.push_str("\n\n");
    }

    if let Some(bias) = render_current_bias(symbol, convergence) {
        block.push_str("**Current bias**\n\n");
        block.push_str(&bias);
        block.push_str("\n\n");
    }

    if let Some(bull) = parsed.bull_case.as_deref() {
        block.push_str("**Bull case**\n\n");
        block.push_str(bull.trim());
        block.push_str("\n\n");
    }
    if let Some(bear) = parsed.bear_case.as_deref() {
        block.push_str("**Bear case**\n\n");
        block.push_str(bear.trim());
        block.push_str("\n\n");
    }

    if let Some(levels) = render_levels(intelligence) {
        block.push_str("**Key levels & technicals**\n\n");
        block.push_str(&levels);
        block.push_str("\n\n");
    }

    if let Some(change) = parsed.change_mind.as_deref() {
        block.push_str("**What to watch / What would change my mind**\n\n");
        block.push_str(change.trim());
        block.push_str("\n\n");
    }

    if let Some(rr) = parsed.risk_reward.as_deref() {
        block.push_str("**Risk / Reward (next 7 days)**\n\n");
        block.push_str(rr.trim());
        block.push_str("\n\n");
    }

    block
}

#[derive(Debug, Clone, Default)]
struct SynthesisBlocks {
    bull_case: Option<String>,
    bear_case: Option<String>,
    change_mind: Option<String>,
    risk_reward: Option<String>,
    preamble: Option<String>,
}

/// Split the synthesis note body into the four tagged sub-blocks. Lines
/// before the first recognised tag land in `preamble` (the assembler's
/// hint about what happened this week, if the writer included one).
fn parse_synthesis_body(body: &str) -> SynthesisBlocks {
    let mut out = SynthesisBlocks::default();
    let mut current: Option<&str> = None;
    let mut buf = String::new();
    let mut preamble = String::new();

    let flush = |out: &mut SynthesisBlocks, current: Option<&str>, buf: &str| {
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            return;
        }
        match current {
            Some("BULL CASE") => out.bull_case = Some(trimmed.to_string()),
            Some("BEAR CASE") => out.bear_case = Some(trimmed.to_string()),
            Some("WHAT WOULD CHANGE MY MIND") => out.change_mind = Some(trimmed.to_string()),
            Some("RISK / REWARD") => out.risk_reward = Some(trimmed.to_string()),
            _ => {}
        }
    };

    for line in body.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        if let Some(tag) = TAGS.iter().find(|t| upper.starts_with(**t)) {
            flush(&mut out, current, &buf);
            buf.clear();
            current = Some(*tag);
            // Capture inline content after the colon, if any.
            if let Some(colon) = trimmed.find(':') {
                let rest = trimmed[colon + 1..].trim();
                if !rest.is_empty() {
                    buf.push_str(rest);
                    buf.push('\n');
                }
            }
            continue;
        }
        if current.is_none() {
            if !trimmed.is_empty() {
                preamble.push_str(trimmed);
                preamble.push(' ');
            }
            continue;
        }
        buf.push_str(line);
        buf.push('\n');
    }
    flush(&mut out, current, &buf);

    let preamble = preamble.trim().to_string();
    if !preamble.is_empty() {
        out.preamble = Some(preamble);
    }
    out
}

/// Build the "what happened this week" overview from the synthesis note's
/// preamble (if it has one) plus the leading-move scan from today's notes
/// (which already carries an inline price-action snippet for the largest
/// |%| held-asset move of the day). Returns None when both sources are
/// empty.
fn render_overview(
    asset: &SynthesisAssetNote,
    ctx: &BuildContext,
    parsed: &SynthesisBlocks,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = parsed.preamble.as_deref() {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    // If today's leading-move scan flagged this asset, prepend that
    // ticker-tape snippet so the operator sees the price action without
    // hunting for it.
    let symbol_upper = asset.symbol.to_uppercase();
    if let Some(synth) = &ctx.todays_analyst_synthesis {
        if let Some(mv) = &synth.leading_move {
            if mv.asset.to_uppercase() == symbol_upper {
                let snippet = format!(
                    "Leading move: {} {:+.1}%. {}",
                    mv.asset, mv.move_pct, mv.note.trim()
                );
                parts.insert(0, snippet);
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" "))
}

/// Compact 4-row convergence table showing each timeframe layer's
/// direction + conviction + 1-line reasoning. Renders nothing when no
/// convergence row is attached for the symbol.
fn render_current_bias(
    symbol: &str,
    convergence: Option<&PrivateAssetConvergenceRow>,
) -> Option<String> {
    let row = convergence?;
    if row.views.is_empty() {
        return None;
    }
    let mut out = String::new();
    // Probation views (active forecast misalignment on the layer/asset) are
    // rendered — visible, never hidden — but excluded from the net
    // conviction, exactly like the convergence stats exclude them.
    let voting: Vec<PrivateAssetConvergenceView> = row
        .views
        .iter()
        .filter(|v| !v.probation)
        .cloned()
        .collect();
    let n_probation = row.views.len() - voting.len();
    // Net conviction + label so the operator gets the takeaway in one line
    // before scanning the layer rows.
    let avg = average_conviction(&voting);
    let label = bias_label(avg, &voting);
    let probation_note = if n_probation > 0 {
        format!(" {n_probation} layer(s) on probation — listed below, not voting.")
    } else {
        String::new()
    };
    out.push_str(&format!(
        "_Net conviction: {} ({:+.1}/5). {} layer rows attached.{}_\n\n",
        label,
        avg,
        row.views.len(),
        probation_note
    ));
    out.push_str("| Layer | Conviction | Reasoning |\n");
    out.push_str("|---|---:|---|\n");
    for v in ordered_views(&row.views) {
        let reasoning = if v.probation {
            format!(
                "⚠ on probation (streak {}) — {}",
                v.probation_streak.unwrap_or_default(),
                clean_cell(&v.reasoning_summary, 180),
            )
        } else {
            clean_cell(&v.reasoning_summary, 220)
        };
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            layer_label(&v.analyst),
            sign(v.conviction),
            reasoning,
        ));
    }
    // `symbol` parameter exists to support future per-symbol tweaks (e.g.
    // suppress for cash positions); intentionally unused today.
    let _ = symbol;
    Some(out)
}

fn render_levels(intelligence: Option<&AssetIntelligenceBlob>) -> Option<String> {
    let blob = intelligence?;
    let mut bullets: Vec<String> = Vec::new();
    if let Some(spot) = blob.spot_price.as_deref() {
        // Skip the "Spot $1" cash placeholder — adds no information.
        if spot == "$1" {
            return None;
        }
        let change = blob
            .daily_change_pct
            .map(|v| format!(" · daily {v:+.2}%"))
            .unwrap_or_default();
        bullets.push(format!("Price: {spot}{change}"));
    }
    let support = blob.nearest_support.as_deref();
    let resistance = blob.nearest_resistance.as_deref();
    if support.is_some() || resistance.is_some() {
        bullets.push(format!(
            "Support {} · resistance {}",
            support.unwrap_or("—"),
            resistance.unwrap_or("—"),
        ));
    }
    if let Some(rsi) = blob.rsi_14 {
        let signal = blob.rsi_signal.as_deref().unwrap_or("neutral");
        bullets.push(format!("RSI(14) {rsi:.1} ({signal})"));
    }
    if let Some(trend) = blob.trend.as_deref() {
        bullets.push(format!("Trend: {trend}"));
    }
    // Price-action structure verdicts from the market-structure engine
    // (auto-skipped when history was too shallow to compute them).
    if let Some(daily) = blob.structure_verdict_daily.as_deref() {
        bullets.push(format!("Structure: {daily}"));
    }
    if let Some(weekly) = blob.structure_verdict_weekly.as_deref() {
        bullets.push(format!("Structure: {weekly}"));
    }
    if let Some(cycle) = blob.cycle_clock_verdict.as_deref() {
        bullets.push(format!("Cycle clock: {cycle}"));
    }
    // Composite Cyber Dots verdict (auto-skipped when history was too
    // shallow for the engine).
    if let Some(cyber) = blob.cyber_verdict_daily.as_deref() {
        bullets.push(format!("Cyber: {cyber}"));
    }
    // Measured expectancy for signals that fired in the last 10 days
    // (auto-skipped when nothing fired or no stats are persisted).
    if let Some(exp) = blob.signal_expectancy.as_deref() {
        bullets.push(format!("Signal expectancy: {exp}"));
    }
    if let Some(pos) = blob.range_52w_position {
        bullets.push(format!("52w range position: {pos:.1}%"));
    }
    if bullets.is_empty() {
        return None;
    }
    Some(bullets.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n"))
}

fn find_convergence<'a>(
    rows: &'a [PrivateAssetConvergenceRow],
    symbol: &str,
) -> Option<&'a PrivateAssetConvergenceRow> {
    rows.iter()
        .find(|r| r.symbol.eq_ignore_ascii_case(symbol))
}

fn average_conviction(views: &[PrivateAssetConvergenceView]) -> f64 {
    if views.is_empty() {
        return 0.0;
    }
    views.iter().map(|v| v.conviction as f64).sum::<f64>() / views.len() as f64
}

fn bias_label(avg: f64, views: &[PrivateAssetConvergenceView]) -> &'static str {
    let spread = view_spread(views);
    if spread >= 4 {
        return "DIVERGENT";
    }
    if avg >= 3.0 {
        "STRONG BULL"
    } else if avg >= 1.0 {
        "BULL"
    } else if avg <= -3.0 {
        "STRONG BEAR"
    } else if avg <= -1.0 {
        "BEAR"
    } else {
        "NEUTRAL"
    }
}

fn view_spread(views: &[PrivateAssetConvergenceView]) -> i64 {
    let convictions: Vec<i64> = views.iter().map(|v| v.conviction).collect();
    let max = convictions.iter().copied().max().unwrap_or(0);
    let min = convictions.iter().copied().min().unwrap_or(0);
    max - min
}

fn ordered_views(views: &[PrivateAssetConvergenceView]) -> Vec<&PrivateAssetConvergenceView> {
    const ORDER: &[&str] = &["low", "medium", "high", "macro"];
    let mut ordered: Vec<&PrivateAssetConvergenceView> = Vec::new();
    for layer in ORDER {
        if let Some(v) = views
            .iter()
            .find(|v| v.analyst.eq_ignore_ascii_case(layer))
        {
            ordered.push(v);
        }
    }
    // Any remaining custom layers (e.g. `analyst-skylar`) get appended.
    for v in views {
        if !ordered.iter().any(|o| std::ptr::eq(*o, v)) {
            ordered.push(v);
        }
    }
    ordered
}

fn layer_label(layer: &str) -> String {
    let upper = layer.to_uppercase();
    match upper.as_str() {
        "LOW" | "MEDIUM" | "HIGH" | "MACRO" => upper,
        _ => layer.to_string(),
    }
}

fn sign(conviction: i64) -> String {
    if conviction > 0 {
        format!("+{conviction}")
    } else {
        conviction.to_string()
    }
}

fn clean_cell(s: &str, max_chars: usize) -> String {
    let collapsed = s.replace('|', "/").replace('\n', " ").trim().to_string();
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(max_chars).collect();
    format!("{}…", truncated.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{
        AssetIntelligenceBlob, PrivateAssetConvergenceRow, PrivateAssetConvergenceView,
        SynthesisAssetNote, SynthesisNotes,
    };

    fn note(symbol: &str, body: &str) -> SynthesisAssetNote {
        SynthesisAssetNote {
            symbol: symbol.to_string(),
            body: body.to_string(),
        }
    }

    fn view(analyst: &str, conviction: i64, summary: &str) -> PrivateAssetConvergenceView {
        PrivateAssetConvergenceView {
            analyst: analyst.to_string(),
            conviction,
            reasoning_summary: summary.to_string(),
            probation: false,
            probation_streak: None,
        }
    }

    fn probation_view(
        analyst: &str,
        conviction: i64,
        summary: &str,
        streak: i64,
    ) -> PrivateAssetConvergenceView {
        PrivateAssetConvergenceView {
            probation: true,
            probation_streak: Some(streak),
            ..view(analyst, conviction, summary)
        }
    }

    #[test]
    fn probation_row_rendered_visibly_but_excluded_from_net_conviction() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: None,
                assets: vec![note("GC=F", "BULL CASE:\nMonetary bid intact.\n")],
                ..SynthesisNotes::default()
            },
            private_asset_convergence: vec![PrivateAssetConvergenceRow {
                symbol: "GC=F".to_string(),
                target_pct: None,
                views: vec![
                    view("low", 3, "Momentum constructive"),
                    probation_view("medium", 4, "Still structurally bullish", 7),
                    view("high", 3, "Structural bull intact"),
                    view("macro", 3, "Hard-money regime"),
                ],
            }],
            ..BuildContext::default()
        };
        let out = render_private_synthesis(&ctx).unwrap();
        // The probation row is VISIBLE with the marker in the reasoning cell.
        assert!(out.contains("⚠ on probation (streak 7) — Still structurally bullish"));
        assert!(out.contains("| MEDIUM | +4 |"));
        assert!(out.contains("1 layer(s) on probation"));
        // Net conviction averages the 3 voting layers only: (3+3+3)/3 = 3.0,
        // not (3+4+3+3)/4 = 3.25.
        assert!(out.contains("(+3.0/5)"), "probation row must not vote: {out}");
        assert!(out.contains("4 layer rows attached"));
    }

    #[test]
    fn suppressed_when_no_asset_notes() {
        let ctx = BuildContext::default();
        let out = render_private_synthesis(&ctx).unwrap();
        let reason = crate::report::build::daily::extract_suppression_reason(&out)
            .expect("empty state must go through the suppression-reason channel");
        assert!(reason.contains("no per-asset"), "unexpected reason: {reason}");
    }

    #[test]
    fn renders_per_asset_card_with_all_blocks() {
        let mut ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: None,
                assets: vec![note(
                    "BTC",
                    "BULL CASE:\nCapitulation zone, RSI 18.\n\nBEAR CASE:\nFlush not finished.\n\nWHAT WOULD CHANGE MY MIND:\nA net-positive ETF day.\n\nRISK / REWARD (next 7 days):\nEV -0.8%.",
                )],
            ..SynthesisNotes::default()
            },
            private_asset_convergence: vec![PrivateAssetConvergenceRow {
                symbol: "BTC".to_string(),
                target_pct: None,
                views: vec![
                    view("low", -2, "Active deleveraging breakdown"),
                    view("medium", 1, "Tactical bounce possible"),
                    view("high", 2, "Structural fixed-supply thesis intact"),
                    view("macro", 1, "Hard-money regime test failing"),
                ],
            }],
            ..BuildContext::default()
        };
        ctx.private_asset_intelligence.insert(
            "BTC".to_string(),
            AssetIntelligenceBlob {
                symbol: "BTC".to_string(),
                spot_price: Some("$62,185".to_string()),
                daily_change_pct: Some(-3.2),
                rsi_14: Some(18.0),
                rsi_signal: Some("oversold".to_string()),
                trend: Some("below 50 SMA, above 200 SMA".to_string()),
                nearest_support: Some("$60,000".to_string()),
                nearest_resistance: Some("$65,000".to_string()),
                range_52w_position: Some(20.0),
                scenario_count: 0,
                open_predictions_count: 0,
                structural_context: None,
                structure_verdict_daily: Some(
                    "DAILY: downtrend (LH 65,800 May-26, LL 60,400 Jun-05), below falling 50d/200d MAs"
                        .to_string(),
                ),
                structure_verdict_weekly: Some(
                    "WEEKLY: downtrend (LH 68,000 May-10, LL 60,400 Jun-07), below falling 10wk/40wk MAs"
                        .to_string(),
                ),
                cycle_clock_verdict: Some(
                    "BTC: day 781 post-halving (Olson day-900 = 2026-10-06), cycle week 185 of ~208"
                        .to_string(),
                ),
                cyber_verdict_daily: Some(
                    "CYBER (daily): QB bearish since 2026-05-20 (14 bars) | line down, price below"
                        .to_string(),
                ),
                signal_expectancy: Some(
                    "structure_weekly_flip_down fired Jun-07: n=41 since 2014, 90d mean -2.1% vs baseline +2.0% (lift -4.1pp), MAE mean -9.8%"
                        .to_string(),
                ),
            },
        );
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(out.contains("## Per-Asset Briefing"));
        assert!(out.contains("### BTC"));
        assert!(out.contains("**Current bias**"));
        assert!(out.contains("Net conviction:"));
        // Convergence rows present in canonical order LOW/MEDIUM/HIGH/MACRO.
        let low = out.find("| LOW |").unwrap();
        let medium = out.find("| MEDIUM |").unwrap();
        let high = out.find("| HIGH |").unwrap();
        let macro_ = out.find("| MACRO |").unwrap();
        assert!(low < medium);
        assert!(medium < high);
        assert!(high < macro_);
        assert!(out.contains("**Bull case**"));
        assert!(out.contains("Capitulation zone, RSI 18."));
        assert!(out.contains("**Bear case**"));
        assert!(out.contains("Flush not finished."));
        assert!(out.contains("**Key levels & technicals**"));
        assert!(out.contains("$62,185"));
        assert!(out.contains("Support $60,000 · resistance $65,000"));
        assert!(out.contains("RSI(14) 18.0 (oversold)"));
        // Structure + cycle-clock verdicts surface in the technicals block.
        assert!(out.contains("Structure: DAILY: downtrend"));
        assert!(out.contains("Structure: WEEKLY: downtrend"));
        assert!(out.contains("Cycle clock: BTC: day 781 post-halving"));
        assert!(out.contains("Cyber: CYBER (daily): QB bearish since 2026-05-20"));
        assert!(out.contains("**What to watch / What would change my mind**"));
        assert!(out.contains("A net-positive ETF day."));
        assert!(out.contains("**Risk / Reward (next 7 days)**"));
        assert!(out.contains("EV -0.8%."));
    }

    #[test]
    fn divergence_label_when_4plus_spread() {
        let ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: None,
                assets: vec![note(
                    "GC=F",
                    "BULL CASE:\nMonetary bid intact.\n",
                )],
            ..SynthesisNotes::default()
            },
            private_asset_convergence: vec![PrivateAssetConvergenceRow {
                symbol: "GC=F".to_string(),
                target_pct: None,
                views: vec![
                    view("low", -1, "Inverse-dollar move"),
                    view("medium", 1, "Range-bound"),
                    view("high", 4, "Structural bull intact"),
                    view("macro", 3, "Highest-conviction call"),
                ],
            }],
            ..BuildContext::default()
        };
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(out.contains("DIVERGENT"));
    }

    #[test]
    fn cash_spot_one_suppresses_levels_block() {
        let mut ctx = BuildContext {
            synthesis_notes: SynthesisNotes {
                economy: None,
                assets: vec![note("USD", "BULL CASE:\nDollar bid this week.\n")],
            ..SynthesisNotes::default()
            },
            ..BuildContext::default()
        };
        ctx.private_asset_intelligence.insert(
            "USD".to_string(),
            AssetIntelligenceBlob {
                symbol: "USD".to_string(),
                spot_price: Some("$1".to_string()),
                ..AssetIntelligenceBlob::default()
            },
        );
        let out = render_private_synthesis(&ctx).unwrap();
        assert!(!out.contains("**Key levels & technicals**"));
        // Bull case still renders.
        assert!(out.contains("**Bull case**"));
        assert!(out.contains("Dollar bid this week."));
    }
}
