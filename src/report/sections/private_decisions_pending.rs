#![allow(dead_code)]

//! `## Decisions Pending — Your Reply Requested` section renderer.
//!
//! Synthesises native `{decision_card(...)}` placeholders for the operator-facing
//! "what do I need to reply to" surface of the private daily report. Every card
//! is derived from existing context rows so no imperative trade action is
//! emitted without an attached evidence reference (convergence, drift band,
//! mismatch row, or binary catalyst).
//!
//! Ordering: urgency (high → normal → low), then gap size (largest first), then
//! symbol for determinism.

use anyhow::Result;

use crate::db::analyst_views::classify_convergence;
use crate::report::build::daily::{
    BinaryCatalystSummary, BuildContext, PortfolioDecisionCard, PrivateAssetConvergenceRow,
    PrivateAssetConvergenceView, PrivateDriftRow, PrivateJournalViewRow,
    PrivatePositionSnapshotRow,
};

pub const SECTION_PRIVACY: &str = "private";

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;
const MISMATCH_THRESHOLD: f64 = 3.0;
const MIN_VIEWS_FOR_ACTION: usize = 2;
const JOURNAL_AUTHOR: &str = "skylar";

/// Allowed response tokens. Short, machine-friendly, agent-readable.
const RESPONSE_FORMAT: &[&str] = &["yes", "yes-if", "no", "wait", "other"];

/// Allowed urgency values.
const URGENCY_HIGH: &str = "high";
const URGENCY_NORMAL: &str = "normal";
const URGENCY_LOW: &str = "low";

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionCard {
    pub symbol: String,
    pub question: String,
    pub context_lines: Vec<String>,
    pub recommendation: String,
    pub reference: String,
    pub urgency: String,
    /// Magnitude used for ordering ties at the same urgency.
    pub gap: f64,
    /// Classified recommendation type: "add" | "trim" | "hold" | "catalyst" |
    /// "outlook-refine" (target refresh stale). Populated alongside the card
    /// so the recommendations table can persist a deterministic type without
    /// re-parsing the rendered markdown.
    pub recommendation_type: String,
    /// Optional rec_id assigned after persistence — when set, render_card
    /// emits a `<!-- rec_id: N -->` marker so downstream readers can resolve
    /// the card to a row in `recommendations`.
    pub rec_id: Option<i64>,
}

pub fn render_private_decisions_pending(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Decisions Pending — Your Reply Requested\n\n");

    let cards = build_cards(ctx);
    if cards.is_empty() {
        output.push_str(
            "No pending decisions: derived actions, drift bands, mismatches, and catalysts are all within the no-reply-required envelope.",
        );
        return Ok(output);
    }

    for card in &cards {
        output.push_str(&render_card(card));
        output.push('\n');
    }

    Ok(output.trim_end().to_string())
}

/// Build the ordered, deduplicated list of decision cards for the given
/// context. Exposed so the report assembler can persist each card to the
/// `recommendations` table before final markdown render.
pub fn build_cards(ctx: &BuildContext) -> Vec<DecisionCard> {
    let held = qualifying_positions(&ctx.private_positions);
    let mut cards: Vec<DecisionCard> = Vec::new();

    // 1. Allocation / convergence-derived ADD / TRIM / HOLD.
    for position in &held {
        if let Some(card) = build_allocation_card(
            position,
            &ctx.private_asset_convergence,
            &ctx.private_drift_rows,
        ) {
            cards.push(card);
        }
    }

    // 2. Stale targets (held asset, no convergence views or insufficient views attached).
    for position in &held {
        if let Some(card) = build_stale_target_card(position, &ctx.private_asset_convergence) {
            cards.push(card);
        }
    }

    // 3. Mismatch surface (Skylar journal vs analyst convergence).
    for position in &held {
        if let Some(card) = build_mismatch_card(
            position,
            &ctx.private_journal_views,
            &ctx.private_asset_convergence,
        ) {
            cards.push(card);
        }
    }

    // 4. Catalyst urgency.
    for catalyst in &ctx.private_binary_catalysts {
        if let Some(card) = build_catalyst_card(catalyst) {
            cards.push(card);
        }
    }

    // 5. Portfolio decision cards — written by the Phase 4 decision
    //    architect with full evidence-for / evidence-against blocks plus
    //    sizing math. Promoted ahead of the generic allocation card when
    //    a card for the same symbol exists (it carries strictly richer
    //    detail).
    let symbols_with_architect_card: std::collections::HashSet<String> = ctx
        .portfolio_decision_cards
        .iter()
        .map(|c| c.symbol.to_uppercase())
        .collect();
    if !symbols_with_architect_card.is_empty() {
        cards.retain(|c| {
            c.recommendation_type == "catalyst"
                || !symbols_with_architect_card.contains(&c.symbol.to_uppercase())
        });
    }
    for architect in &ctx.portfolio_decision_cards {
        cards.push(decision_card_from_architect(architect));
    }

    cards.sort_by(|a, b| {
        urgency_rank(&a.urgency)
            .cmp(&urgency_rank(&b.urgency))
            .then_with(|| {
                b.gap
                    .partial_cmp(&a.gap)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.symbol.cmp(&b.symbol))
    });

    cards
}

fn qualifying_positions(rows: &[PrivatePositionSnapshotRow]) -> Vec<&PrivatePositionSnapshotRow> {
    let mut held = rows
        .iter()
        .filter(|row| row.allocation_pct >= HELD_ASSET_THRESHOLD_PCT)
        .collect::<Vec<_>>();
    held.sort_by(|a, b| {
        b.allocation_pct
            .partial_cmp(&a.allocation_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    held
}

fn build_allocation_card(
    position: &PrivatePositionSnapshotRow,
    convergence_rows: &[PrivateAssetConvergenceRow],
    drift_rows: &[PrivateDriftRow],
) -> Option<DecisionCard> {
    let convergence = find_convergence(convergence_rows, &position.symbol)?;
    let views = convergence.views.as_slice();
    if views.len() < MIN_VIEWS_FOR_ACTION {
        return None;
    }
    let avg_conviction = average_conviction(views)?;
    let max_divergence = conviction_divergence(views);
    let summary = classify_convergence(views.len(), avg_conviction, max_divergence);

    // Derive action via convergence-summary + drift band.
    let target = convergence.target_pct;
    let band = find_drift_band(drift_rows, &position.symbol);
    let (action, gap) = derive_action(position.allocation_pct, target, summary, band);
    let action = action?;

    let urgency = action_urgency(action, gap, band);
    let target_str = target
        .map(|t| format!("{t:.2}%"))
        .unwrap_or_else(|| "no target attached".to_string());
    let analyst_summary = views_summary(views);

    let question = match action {
        Action::Add => format!(
            "Add to {} now (current allocation {:.2}%, target {})?",
            clean_text(&position.symbol),
            position.allocation_pct,
            target_str
        ),
        Action::Trim => format!(
            "Trim {} now (current allocation {:.2}%, target {})?",
            clean_text(&position.symbol),
            position.allocation_pct,
            target_str
        ),
        Action::Hold => format!(
            "Hold {} at {:.2}% (target {})?",
            clean_text(&position.symbol),
            position.allocation_pct,
            target_str
        ),
    };

    let mut context_lines = vec![
        format!(
            "Analyst convergence: {} (avg {:+.2}, max divergence {} across {} layer{})",
            readable(summary),
            avg_conviction,
            max_divergence,
            views.len(),
            if views.len() == 1 { "" } else { "s" }
        ),
        format!("Layer views: {analyst_summary}"),
    ];
    if let Some(band) = band {
        context_lines.push(format!(
            "Drift: actual {:.2}% vs target {:.2}% (band ±{:.2}%)",
            band.actual_pct, band.target_pct, band.band_pct
        ));
    }

    let recommendation = match action {
        Action::Add => format!(
            "{} per convergence formula and drift band.",
            action.imperative()
        ),
        Action::Trim => format!(
            "{} per convergence formula and drift band.",
            action.imperative()
        ),
        Action::Hold => "Hold — convergence and drift band agree no change is required.".to_string(),
    };

    let reference = format!(
        "See Per-Asset Convergence card for {} (summary={}, layers={}).",
        clean_text(&position.symbol),
        summary,
        views.len()
    );

    let recommendation_type = match action {
        Action::Add => "add".to_string(),
        Action::Trim => "trim".to_string(),
        Action::Hold => "hold".to_string(),
    };
    Some(DecisionCard {
        symbol: position.symbol.clone(),
        question,
        context_lines,
        recommendation,
        reference,
        urgency,
        gap,
        recommendation_type,
        rec_id: None,
    })
}

fn build_stale_target_card(
    position: &PrivatePositionSnapshotRow,
    convergence_rows: &[PrivateAssetConvergenceRow],
) -> Option<DecisionCard> {
    let convergence = find_convergence(convergence_rows, &position.symbol);
    let view_count = convergence
        .map(|row| row.views.len())
        .unwrap_or_default();
    if view_count >= MIN_VIEWS_FOR_ACTION {
        return None;
    }
    let missing = match convergence {
        Some(row) => missing_layers(&row.views),
        None => vec!["LOW", "MEDIUM", "HIGH", "MACRO"],
    };

    let question = format!(
        "Refresh the allocation target for {}: only {} analyst layer{} attached?",
        clean_text(&position.symbol),
        view_count,
        if view_count == 1 { "" } else { "s" }
    );
    let context_lines = vec![
        format!(
            "Current allocation {:.2}% but the convergence formula requires at least {} layers to derive an action.",
            position.allocation_pct,
            MIN_VIEWS_FOR_ACTION
        ),
        format!("Missing analyst layers: {}", missing.join(", ")),
    ];
    let recommendation =
        "Refresh missing analyst layers before the next report so a convergence-derived action can fire.".to_string();
    let reference = format!(
        "See Per-Asset Convergence card for {} (insufficient-views state).",
        clean_text(&position.symbol)
    );

    Some(DecisionCard {
        symbol: position.symbol.clone(),
        question,
        context_lines,
        recommendation,
        reference,
        urgency: URGENCY_LOW.to_string(),
        gap: (MIN_VIEWS_FOR_ACTION as f64 - view_count as f64).max(0.0),
        recommendation_type: "outlook-refine".to_string(),
        rec_id: None,
    })
}

fn build_mismatch_card(
    position: &PrivatePositionSnapshotRow,
    journal_rows: &[PrivateJournalViewRow],
    convergence_rows: &[PrivateAssetConvergenceRow],
) -> Option<DecisionCard> {
    let journal = journal_rows.iter().find(|row| {
        row.symbol.eq_ignore_ascii_case(&position.symbol)
            && row.author.eq_ignore_ascii_case(JOURNAL_AUTHOR)
    })?;
    let convergence = find_convergence(convergence_rows, &position.symbol)?;
    let avg = average_conviction(&convergence.views)?;
    let gap = (journal.conviction as f64 - avg).abs();
    if gap < MISMATCH_THRESHOLD {
        return None;
    }
    let urgency = if gap >= 5.0 {
        URGENCY_HIGH
    } else {
        URGENCY_NORMAL
    };
    let question = format!(
        "Resolve the {:.1}-point Skylar-vs-analyst gap on {}?",
        gap,
        clean_text(&position.symbol)
    );
    let context_lines = vec![
        format!(
            "Skylar conviction {:+}: {}",
            journal.conviction.clamp(-5, 5),
            clean_text(&journal.summary)
        ),
        format!(
            "Analyst convergence: avg {:+.2} across {} layer{}",
            avg,
            convergence.views.len(),
            if convergence.views.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
    ];
    let recommendation = "Acknowledge the gap and either update the journal view or note why the analyst convergence is wrong.".to_string();
    let reference = format!(
        "See Mismatch Surface card for {} (gap={:.2}).",
        clean_text(&position.symbol),
        gap
    );

    Some(DecisionCard {
        symbol: position.symbol.clone(),
        question,
        context_lines,
        recommendation,
        reference,
        urgency: urgency.to_string(),
        gap,
        recommendation_type: "meta".to_string(),
        rec_id: None,
    })
}

/// Convert a Phase-4 decision-architect JSON envelope into the section's
/// internal `DecisionCard` shape. The architect's card carries strictly
/// more detail than the auto-derived allocation card: evidence FOR and
/// AGAINST blocks, an explicit recommendation, the change-mind trigger,
/// and sizing math — all of which we surface verbatim.
fn decision_card_from_architect(card: &PortfolioDecisionCard) -> DecisionCard {
    let mut context_lines: Vec<String> = Vec::new();
    if !card.evidence_for.is_empty() {
        context_lines.push("Evidence FOR:".to_string());
        for line in &card.evidence_for {
            context_lines.push(format!("• {}", clean_text(line)));
        }
    }
    if !card.evidence_against.is_empty() {
        context_lines.push("Evidence AGAINST:".to_string());
        for line in &card.evidence_against {
            context_lines.push(format!("• {}", clean_text(line)));
        }
    }
    if !card.what_would_change_it.is_empty() {
        context_lines.push(format!(
            "What would change it: {}",
            clean_text(&card.what_would_change_it)
        ));
    }
    if !card.sizing_math.is_empty() {
        context_lines.push(format!("Sizing math: {}", clean_text(&card.sizing_math)));
    }
    let recommendation = if card.recommendation.is_empty() {
        "WAIT — Phase-4 decision architect did not specify a recommendation.".to_string()
    } else {
        clean_text(&card.recommendation)
    };
    let question = if card.question.is_empty() {
        format!("Decision pending on {}.", card.symbol)
    } else {
        clean_text(&card.question)
    };
    let recommendation_type = ledger_type_from_recommendation(&recommendation).to_string();
    DecisionCard {
        symbol: card.symbol.to_uppercase(),
        question,
        context_lines,
        recommendation,
        reference: format!(
            "Phase-4 decision architect card for {}.",
            card.symbol.to_uppercase()
        ),
        urgency: URGENCY_HIGH.to_string(),
        gap: f64::INFINITY,
        recommendation_type,
        rec_id: None,
    }
}

/// Map a Phase-4 architect card's free-text recommendation onto the
/// `recommendations` ledger vocabulary (`VALID_RECOMMENDATION_TYPES`).
/// The architect writes action-led prose such as "WINDOW-OPEN-SCALE-IN,
/// +1pp now", "WAIT-FOR-NAMED-GATE → scale-in", "HOLD dry powder" — the
/// leading action wins, so WAIT/TRIM/AVOID are matched before ADD even when
/// a gated scale-in is named later in the sentence. Persisting a literal
/// "architect" type made `upsert_recommendation` reject the row and abort
/// the whole private assembly.
fn ledger_type_from_recommendation(recommendation: &str) -> &'static str {
    let rec = recommendation.to_uppercase();
    if rec.contains("TRIM") || rec.contains("TAKE-PROFIT") || rec.contains("TAKE PROFIT") {
        "trim"
    } else if rec.contains("AVOID") || rec.contains("DO-NOT-INITIATE") || rec.contains("DO NOT INITIATE") {
        "avoid"
    } else if rec.contains("EXIT") {
        "exit"
    } else if rec.contains("WAIT") || rec.contains("DEFER") {
        "wait"
    } else if rec.contains("ADD")
        || rec.contains("SCALE-IN")
        || rec.contains("SCALE IN")
        || rec.contains("ACCUMULATE")
    {
        "add"
    } else if rec.contains("HOLD") {
        "hold"
    } else {
        "meta"
    }
}

fn build_catalyst_card(catalyst: &BinaryCatalystSummary) -> Option<DecisionCard> {
    let event = clean_text(&catalyst.event);
    let date = clean_text(&catalyst.date);
    if event.is_empty() || date.is_empty() {
        return None;
    }
    let question = format!("Pre-position for {} on {}?", event, date);
    let context_lines = vec![format!("Catalyst impact: {}", clean_text(&catalyst.impact))];
    let recommendation =
        "Decide before the event prints — choose pre-position, wait-and-react, or no-action.".to_string();
    let reference = format!(
        "See Macro Context catalyst row for {} on {}.",
        event, date
    );

    Some(DecisionCard {
        symbol: event.clone(),
        question,
        context_lines,
        recommendation,
        reference,
        urgency: URGENCY_HIGH.to_string(),
        // Catalyst urgency dominates within the high tier.
        gap: f64::INFINITY,
        recommendation_type: "catalyst".to_string(),
        rec_id: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Add,
    Trim,
    Hold,
}

impl Action {
    fn imperative(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Trim => "Trim",
            Self::Hold => "Hold",
        }
    }
}

/// Derive the recommendation purely from `classify_convergence` + drift band.
///
/// - strong-convergent-bull / convergent-bull with allocation below floor → ADD
/// - strong-convergent-bear / convergent-bear with allocation above ceiling → TRIM
/// - convergent-neutral within band → HOLD (only fires if a band is attached so
///   it carries an evidence reference).
/// - divergent / neutral-with-divergence / insufficient-views → no card here
///   (stale-target or mismatch flows handle them).
fn derive_action(
    allocation_pct: f64,
    target_pct: Option<f64>,
    summary: &str,
    band: Option<&PrivateDriftRow>,
) -> (Option<Action>, f64) {
    let floor_breached = band
        .map(|b| allocation_pct < b.target_pct - b.band_pct)
        .unwrap_or(false);
    let ceiling_breached = band
        .map(|b| allocation_pct > b.target_pct + b.band_pct)
        .unwrap_or(false);
    let in_band = band.is_some() && !(floor_breached || ceiling_breached);

    let target_gap = target_pct
        .map(|t| (allocation_pct - t).abs())
        .unwrap_or(0.0);

    match summary {
        "strong-convergent-bull" | "convergent-bull" if floor_breached => {
            (Some(Action::Add), target_gap.max(1.0))
        }
        "strong-convergent-bear" | "convergent-bear" if ceiling_breached => {
            (Some(Action::Trim), target_gap.max(1.0))
        }
        "convergent-neutral" if in_band => (Some(Action::Hold), 0.0),
        _ => (None, 0.0),
    }
}

fn action_urgency(action: Action, gap: f64, band: Option<&PrivateDriftRow>) -> String {
    match action {
        Action::Hold => URGENCY_LOW.to_string(),
        Action::Add | Action::Trim => {
            let breach_size = band
                .map(|b| {
                    let lower = b.target_pct - b.band_pct;
                    let upper = b.target_pct + b.band_pct;
                    if gap == 0.0 {
                        0.0
                    } else if action == Action::Add {
                        (lower - (lower - gap)).abs().max(0.0)
                    } else {
                        (upper - (upper + gap)).abs().max(0.0)
                    }
                })
                .unwrap_or(0.0);
            if gap >= 3.0 || breach_size >= 3.0 {
                URGENCY_HIGH.to_string()
            } else {
                URGENCY_NORMAL.to_string()
            }
        }
    }
}

fn find_convergence<'a>(
    rows: &'a [PrivateAssetConvergenceRow],
    symbol: &str,
) -> Option<&'a PrivateAssetConvergenceRow> {
    rows.iter()
        .find(|row| row.symbol.eq_ignore_ascii_case(symbol))
}

fn find_drift_band<'a>(
    rows: &'a [PrivateDriftRow],
    symbol: &str,
) -> Option<&'a PrivateDriftRow> {
    rows.iter()
        .find(|row| row.symbol.eq_ignore_ascii_case(symbol))
}

fn average_conviction(views: &[PrivateAssetConvergenceView]) -> Option<f64> {
    if views.is_empty() {
        return None;
    }
    Some(views.iter().map(|view| view.conviction).sum::<i64>() as f64 / views.len() as f64)
}

fn conviction_divergence(views: &[PrivateAssetConvergenceView]) -> i64 {
    let Some(min) = views.iter().map(|view| view.conviction).min() else {
        return 0;
    };
    let max = views
        .iter()
        .map(|view| view.conviction)
        .max()
        .unwrap_or(min);
    max - min
}

fn views_summary(views: &[PrivateAssetConvergenceView]) -> String {
    let mut sorted = views.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.analyst.cmp(&b.analyst));
    sorted
        .into_iter()
        .map(|v| format!("{}:{:+}", clean_text(&v.analyst), v.conviction.clamp(-5, 5)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn missing_layers(views: &[PrivateAssetConvergenceView]) -> Vec<&'static str> {
    ["LOW", "MEDIUM", "HIGH", "MACRO"]
        .into_iter()
        .filter(|layer| {
            !views
                .iter()
                .any(|view| view.analyst.eq_ignore_ascii_case(layer))
        })
        .collect()
}

fn urgency_rank(urgency: &str) -> u8 {
    match urgency {
        URGENCY_HIGH => 0,
        URGENCY_NORMAL => 1,
        URGENCY_LOW => 2,
        _ => 3,
    }
}

fn render_card(card: &DecisionCard) -> String {
    let body = crate::report::charts::decision_card::render_html(
        &crate::report::charts::decision_card::DecisionCardInput {
            question: card.question.clone(),
            context_lines: card.context_lines.clone(),
            recommendation: if card.recommendation.is_empty() {
                None
            } else {
                Some(card.recommendation.clone())
            },
            response_format: Some(RESPONSE_FORMAT.iter().map(|s| s.to_string()).collect()),
            reference: if card.reference.is_empty() {
                None
            } else {
                Some(card.reference.clone())
            },
            urgency: card.urgency.clone(),
            width: None,
        },
    );
    if let Some(id) = card.rec_id {
        format!("<!-- rec_id: {id} -->\n{body}")
    } else {
        body
    }
}

/// Render the section using a pre-built ordered list of cards (typically
/// produced by `build_cards` and then annotated with `rec_id`s from the
/// `recommendations` table). Used by the assembler when persistence is on.
pub fn render_private_decisions_pending_with_cards(cards: &[DecisionCard]) -> String {
    let mut output = String::from("## Decisions Pending — Your Reply Requested\n\n");
    if cards.is_empty() {
        output.push_str(
            "No pending decisions: derived actions, drift bands, mismatches, and catalysts are all within the no-reply-required envelope.",
        );
        return output;
    }
    for card in cards {
        output.push_str(&render_card(card));
        output.push('\n');
    }
    output.trim_end().to_string()
}

fn readable(value: &str) -> String {
    value.replace(['_', '-'], " ")
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

fn clean_arg(value: &str) -> String {
    clean_text(value).replace([',', '[', ']', '{', '}', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{
        BinaryCatalystSummary, PrivateAssetConvergenceRow, PrivateAssetConvergenceView,
        PrivateDriftRow, PrivateJournalViewRow, PrivatePositionSnapshotRow,
    };

    #[test]
    fn private_decisions_pending_renders_section_header() {
        let rendered = render_private_decisions_pending(&empty_fixture()).unwrap();
        assert!(rendered.starts_with("## Decisions Pending — Your Reply Requested\n\n"));
    }

    #[test]
    fn private_decisions_pending_empty_fixture_emits_explicit_no_pending_line() {
        let rendered = render_private_decisions_pending(&empty_fixture()).unwrap();
        assert!(rendered.contains("No pending decisions"));
        assert!(!rendered.contains("{decision_card("));
    }

    // After the chart-substitution rewrite, decision cards render as HTML
    // (not the `{decision_card(...)}` token). Urgency is encoded as a
    // border-left accent color rather than a literal "urgency=" string:
    //   high   -> #f38ba8 (bear)
    //   normal -> #89dceb (cyan)
    //   low    -> #6e7681 (muted)
    const URGENCY_HIGH_ACCENT: &str = "#f38ba8";
    const URGENCY_LOW_ACCENT: &str = "#6e7681";

    #[test]
    fn private_decisions_pending_action_derives_from_convergence_formula() {
        // BTC convergent-bull below band -> ADD. Card must cite the convergence summary.
        let rendered = render_private_decisions_pending(&add_fixture()).unwrap();
        assert!(rendered.contains("Add to BTC now"));
        assert!(rendered.contains("Analyst convergence: strong convergent bull"));
        assert!(rendered.contains(URGENCY_HIGH_ACCENT));
        assert!(rendered.contains("Per-Asset Convergence card for BTC"));
        assert!(
            !rendered.contains("{decision_card("),
            "must not leak token placeholder"
        );
    }

    #[test]
    fn private_decisions_pending_trim_when_convergent_bear_above_band() {
        let rendered = render_private_decisions_pending(&trim_fixture()).unwrap();
        assert!(rendered.contains("Trim QQQ now"));
        assert!(rendered.contains("Analyst convergence: convergent bear"));
    }

    #[test]
    fn private_decisions_pending_response_format_tokens_are_short() {
        let rendered = render_private_decisions_pending(&add_fixture()).unwrap();
        // Each response token now appears as its own chip in the HTML.
        for token in RESPONSE_FORMAT {
            assert!(token.len() <= 6, "response token too long: {token}");
            assert!(
                rendered.contains(token),
                "expected response chip '{token}' in: {rendered}"
            );
        }
    }

    #[test]
    fn private_decisions_pending_no_imperative_without_evidence_reference() {
        let rendered = render_private_decisions_pending(&full_fixture()).unwrap();
        // Every rendered card includes a reference line marked by the "↑ "
        // prefix that the chart helper emits. Every reference points at a
        // "See X card …" evidence pointer.
        let ref_chunks: Vec<&str> = rendered.split("↑ ").skip(1).collect();
        assert!(!ref_chunks.is_empty(), "expected at least one card");
        for chunk in ref_chunks {
            // Each reference line should cite evidence ("See …").
            assert!(
                chunk.starts_with("See "),
                "decision card reference missing evidence pointer: {chunk}"
            );
        }
    }

    #[test]
    fn private_decisions_pending_orders_by_urgency_then_gap() {
        let rendered = render_private_decisions_pending(&ordering_fixture()).unwrap();
        // Cards are emitted in order; the first should carry the high-urgency
        // accent color and the last should carry the low-urgency accent color.
        let first_high = rendered.find(URGENCY_HIGH_ACCENT);
        let last_low = rendered.rfind(URGENCY_LOW_ACCENT);
        assert!(first_high.is_some(), "expected high-urgency card to render");
        assert!(last_low.is_some(), "expected low-urgency card to render");
        assert!(
            first_high.unwrap() < last_low.unwrap(),
            "expected high-urgency card before low-urgency: {rendered}"
        );
    }

    #[test]
    fn private_decisions_pending_stale_target_low_urgency_with_reference() {
        let rendered = render_private_decisions_pending(&stale_fixture()).unwrap();
        assert!(rendered.contains("Refresh the allocation target for GLD"));
        assert!(rendered.contains(URGENCY_LOW_ACCENT));
        assert!(rendered.contains("Per-Asset Convergence card for GLD"));
    }

    #[test]
    fn private_decisions_pending_catalyst_renders_high_urgency_card() {
        let rendered = render_private_decisions_pending(&catalyst_only_fixture()).unwrap();
        assert!(rendered.contains("Pre-position for FOMC decision on 2026-06-03"));
        assert!(rendered.contains(URGENCY_HIGH_ACCENT));
        assert!(rendered.contains("Macro Context catalyst row"));
    }

    #[test]
    fn private_decisions_pending_mismatch_renders_when_gap_exceeds_threshold() {
        let rendered = render_private_decisions_pending(&mismatch_fixture()).unwrap();
        assert!(rendered.contains("Resolve the 4.0-point Skylar-vs-analyst gap on BTC"));
        assert!(rendered.contains("Mismatch Surface card"));
    }

    #[test]
    fn private_decisions_pending_is_private_only() {
        assert_eq!(SECTION_PRIVACY, "private");
    }

    #[test]
    fn render_with_cards_emits_rec_id_marker() {
        let mut cards = build_cards(&add_fixture());
        assert!(!cards.is_empty());
        for (i, card) in cards.iter_mut().enumerate() {
            card.rec_id = Some(100 + i as i64);
        }
        let rendered = render_private_decisions_pending_with_cards(&cards);
        assert!(rendered.contains("<!-- rec_id: 100 -->"));
        assert!(rendered.contains("Add to BTC now"));
        assert!(
            !rendered.contains("{decision_card("),
            "must not leak token placeholder"
        );
    }

    #[test]
    fn build_cards_assigns_recommendation_type() {
        let cards = build_cards(&add_fixture());
        assert!(cards.iter().any(|c| c.recommendation_type == "add"));
        let cards2 = build_cards(&trim_fixture());
        assert!(cards2.iter().any(|c| c.recommendation_type == "trim"));
        let cards3 = build_cards(&catalyst_only_fixture());
        assert!(cards3.iter().any(|c| c.recommendation_type == "catalyst"));
        let cards4 = build_cards(&stale_fixture());
        assert!(cards4.iter().any(|c| c.recommendation_type == "outlook-refine"));
    }

    #[test]
    fn architect_cards_map_to_ledger_valid_recommendation_types() {
        // Persisting a literal "architect" type aborted the whole private
        // assembly (upsert_recommendation validates against
        // VALID_RECOMMENDATION_TYPES). The architect's free-text
        // recommendation must map onto the ledger vocabulary instead.
        let mk = |rec: &str| PortfolioDecisionCard {
            symbol: "BTC".to_string(),
            question: "q?".to_string(),
            evidence_for: vec![],
            evidence_against: vec![],
            recommendation: rec.to_string(),
            what_would_change_it: String::new(),
            sizing_math: String::new(),
        };
        for (rec, expected) in [
            ("WINDOW-OPEN-SCALE-IN, +1pp now", "add"),
            ("WAIT-FOR-NAMED-GATE → scale-in (no add at 4,098)", "wait"),
            ("HOLD dry powder (43% cash = staged ammunition)", "hold"),
            ("TRIM 2pp into the extension", "trim"),
            ("do-not-initiate", "avoid"),
            ("something unparseable", "meta"),
        ] {
            let card = decision_card_from_architect(&mk(rec));
            assert_eq!(
                card.recommendation_type, expected,
                "recommendation {rec:?} should map to {expected:?}"
            );
            assert!(
                crate::db::recommendations::VALID_RECOMMENDATION_TYPES
                    .contains(&card.recommendation_type.as_str()),
                "mapped type {:?} must be ledger-valid",
                card.recommendation_type
            );
        }
        // Empty recommendation falls back to the WAIT placeholder.
        let card = decision_card_from_architect(&mk(""));
        assert_eq!(card.recommendation_type, "wait");
    }

    // ---- Fixtures -------------------------------------------------------

    fn empty_fixture() -> BuildContext {
        BuildContext::default()
    }

    fn add_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("BTC", 30.0)],
            private_drift_rows: vec![drift("BTC", 40.0, 30.0, 2.0)],
            private_asset_convergence: vec![convergence(
                "BTC",
                Some(40.0),
                vec![
                    view("LOW", 4, "spot momentum"),
                    view("MEDIUM", 3, "ETF flows"),
                    view("HIGH", 4, "halving cycle"),
                    view("MACRO", 4, "debasement"),
                ],
            )],
            ..BuildContext::default()
        }
    }

    fn trim_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("QQQ", 35.0)],
            private_drift_rows: vec![drift("QQQ", 20.0, 25.0, 2.0)],
            private_asset_convergence: vec![convergence(
                "QQQ",
                Some(25.0),
                vec![view("LOW", -2, "rate risk"), view("MEDIUM", -2, "valuation")],
            )],
            ..BuildContext::default()
        }
    }

    fn stale_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("GLD", 22.0)],
            private_asset_convergence: vec![convergence(
                "GLD",
                Some(25.0),
                vec![view("LOW", -3, "real yields")],
            )],
            ..BuildContext::default()
        }
    }

    fn catalyst_only_fixture() -> BuildContext {
        BuildContext {
            private_binary_catalysts: vec![BinaryCatalystSummary {
                date: "2026-06-03".to_string(),
                event: "FOMC decision".to_string(),
                impact: "Rates repricing flips equity signal".to_string(),
            }],
            ..BuildContext::default()
        }
    }

    fn mismatch_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("BTC", 42.0)],
            private_journal_views: vec![journal(
                "BTC",
                "skylar",
                5,
                "Skylar sees asymmetric upside",
            )],
            private_asset_convergence: vec![convergence(
                "BTC",
                Some(40.0),
                vec![view("LOW", 0, "neutral"), view("HIGH", 2, "constructive")],
            )],
            ..BuildContext::default()
        }
    }

    fn full_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("BTC", 30.0), position("GLD", 22.0)],
            private_drift_rows: vec![drift("BTC", 40.0, 30.0, 2.0)],
            private_journal_views: vec![journal(
                "BTC",
                "skylar",
                5,
                "Skylar sees asymmetric upside",
            )],
            private_asset_convergence: vec![
                convergence(
                    "BTC",
                    Some(40.0),
                    vec![
                        view("LOW", 4, "spot momentum"),
                        view("MEDIUM", 3, "ETF flows"),
                        view("HIGH", 4, "halving cycle"),
                        view("MACRO", 4, "debasement"),
                    ],
                ),
                convergence("GLD", Some(25.0), vec![view("LOW", -3, "real yields")]),
            ],
            private_binary_catalysts: vec![BinaryCatalystSummary {
                date: "2026-06-03".to_string(),
                event: "FOMC decision".to_string(),
                impact: "Rates repricing flips equity signal".to_string(),
            }],
            ..BuildContext::default()
        }
    }

    fn ordering_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("GLD", 22.0)],
            private_asset_convergence: vec![convergence(
                "GLD",
                Some(25.0),
                vec![view("LOW", -3, "real yields")],
            )],
            private_binary_catalysts: vec![BinaryCatalystSummary {
                date: "2026-06-03".to_string(),
                event: "FOMC decision".to_string(),
                impact: "Rates repricing flips equity signal".to_string(),
            }],
            ..BuildContext::default()
        }
    }

    fn position(symbol: &str, allocation_pct: f64) -> PrivatePositionSnapshotRow {
        PrivatePositionSnapshotRow {
            symbol: symbol.to_string(),
            price: None,
            daily_change: None,
            allocation_pct,
            unrealized_pnl: None,
        }
    }

    fn drift(symbol: &str, target_pct: f64, actual_pct: f64, band_pct: f64) -> PrivateDriftRow {
        PrivateDriftRow {
            symbol: symbol.to_string(),
            target_pct,
            actual_pct,
            band_pct,
        }
    }

    fn convergence(
        symbol: &str,
        target_pct: Option<f64>,
        views: Vec<PrivateAssetConvergenceView>,
    ) -> PrivateAssetConvergenceRow {
        PrivateAssetConvergenceRow {
            symbol: symbol.to_string(),
            target_pct,
            views,
        }
    }

    fn view(analyst: &str, conviction: i64, reasoning: &str) -> PrivateAssetConvergenceView {
        PrivateAssetConvergenceView {
            analyst: analyst.to_string(),
            conviction,
            reasoning_summary: reasoning.to_string(),
            probation: false,
            probation_streak: None,
        }
    }

    fn journal(symbol: &str, author: &str, conviction: i64, summary: &str) -> PrivateJournalViewRow {
        PrivateJournalViewRow {
            symbol: symbol.to_string(),
            author: author.to_string(),
            conviction,
            summary: summary.to_string(),
        }
    }
}
