#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateAssetConvergenceRow, PrivateJournalViewRow, PrivatePositionSnapshotRow,
};

pub const SECTION_PRIVACY: &str = "private";

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;
const MEANINGFUL_MISMATCH_THRESHOLD: f64 = 3.0;
const JOURNAL_AUTHOR: &str = "skylar";

#[derive(Debug, Clone, PartialEq)]
struct MismatchCard {
    symbol: String,
    allocation_pct: f64,
    skylar_conviction: i64,
    analyst_conviction: f64,
    gap: f64,
    journal_summary: String,
    convergence_summary: String,
}

pub fn render_private_mismatch_surface(ctx: &BuildContext) -> Result<String> {
    let held = qualifying_positions(&ctx.private_positions);
    if held.is_empty() {
        return Ok(String::new());
    }

    let cards = mismatch_cards(
        &held,
        &ctx.private_journal_views,
        &ctx.private_asset_convergence,
    );
    if cards.is_empty() {
        // Alignment is the default state — suppress when nothing diverges.
        return Ok(String::new());
    }

    let mut output = String::from("## Mismatch Surface - Skylar's view vs analyst convergence\n\n");

    for card in cards {
        output.push_str(&render_card(&card));
        output.push('\n');
    }

    Ok(output.trim_end().to_string())
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

fn mismatch_cards(
    positions: &[&PrivatePositionSnapshotRow],
    journal_rows: &[PrivateJournalViewRow],
    convergence_rows: &[PrivateAssetConvergenceRow],
) -> Vec<MismatchCard> {
    let mut cards = Vec::new();
    for position in positions {
        let Some(journal) = journal_view(journal_rows, &position.symbol) else {
            continue;
        };
        let Some(convergence) = convergence_view(convergence_rows, &position.symbol) else {
            continue;
        };
        let Some(analyst_conviction) = average_conviction(convergence) else {
            continue;
        };
        let gap = (journal.conviction as f64 - analyst_conviction).abs();
        if gap < MEANINGFUL_MISMATCH_THRESHOLD {
            continue;
        }
        cards.push(MismatchCard {
            symbol: position.symbol.clone(),
            allocation_pct: position.allocation_pct,
            skylar_conviction: journal.conviction.clamp(-5, 5),
            analyst_conviction,
            gap,
            journal_summary: journal.summary.clone(),
            convergence_summary: analyst_summary(convergence),
        });
    }

    cards.sort_by(|a, b| {
        b.gap
            .partial_cmp(&a.gap)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.allocation_pct
                    .partial_cmp(&a.allocation_pct)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    cards
}

fn journal_view<'a>(
    rows: &'a [PrivateJournalViewRow],
    symbol: &str,
) -> Option<&'a PrivateJournalViewRow> {
    rows.iter().find(|row| {
        row.symbol.eq_ignore_ascii_case(symbol) && row.author.eq_ignore_ascii_case(JOURNAL_AUTHOR)
    })
}

fn convergence_view<'a>(
    rows: &'a [PrivateAssetConvergenceRow],
    symbol: &str,
) -> Option<&'a PrivateAssetConvergenceRow> {
    rows.iter()
        .find(|row| row.symbol.eq_ignore_ascii_case(symbol))
}

fn average_conviction(row: &PrivateAssetConvergenceRow) -> Option<f64> {
    if row.views.is_empty() {
        return None;
    }
    Some(row.views.iter().map(|view| view.conviction).sum::<i64>() as f64 / row.views.len() as f64)
}

fn analyst_summary(row: &PrivateAssetConvergenceRow) -> String {
    let mut summaries = row
        .views
        .iter()
        .map(|view| {
            format!(
                "{}:{:+}",
                clean_arg(&view.analyst),
                view.conviction.clamp(-5, 5)
            )
        })
        .collect::<Vec<_>>();
    summaries.sort();
    summaries.join("; ")
}

fn render_card(card: &MismatchCard) -> String {
    format!(
        "{{mismatch_card({}, skylar={:+}, analysts={}, gap={}, journal={}, convergence={})}}",
        clean_arg(&card.symbol),
        card.skylar_conviction,
        format_number(card.analyst_conviction),
        format_number(card.gap),
        clean_arg(&card.journal_summary),
        clean_arg(&card.convergence_summary),
    )
}

fn format_number(value: f64) -> String {
    format!("{value:.2}")
}

fn clean_arg(value: &str) -> String {
    value
        .replace(['|', ',', '[', ']', '{', '}', '\n'], " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::PrivateAssetConvergenceView;

    #[test]
    fn private_mismatch_surface_creates_card_for_meaningful_divergence() {
        let rendered = render_private_mismatch_surface(&divergent_fixture()).unwrap();

        assert!(
            rendered.starts_with("## Mismatch Surface - Skylar's view vs analyst convergence\n\n")
        );
        assert!(rendered.contains("{mismatch_card(BTC, skylar=+5, analysts=1.00, gap=4.00"));
        assert!(rendered.contains("journal=Skylar sees asymmetric upside"));
        assert!(rendered.contains("convergence=HIGH:+2; LOW:+0"));
    }

    #[test]
    fn private_mismatch_surface_aligned_fixture_suppresses_section() {
        // Alignment is the default state — when journal view matches
        // analyst convergence for every held asset, the section emits
        // nothing rather than a placeholder.
        let rendered = render_private_mismatch_surface(&aligned_fixture()).unwrap();
        assert!(rendered.is_empty());
    }

    #[test]
    fn private_mismatch_surface_is_marked_private_only() {
        assert_eq!(SECTION_PRIVACY, "private");
    }

    fn divergent_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("BTC", 42.0), position("GLD", 22.0)],
            private_journal_views: vec![
                journal("BTC", "skylar", 5, "Skylar sees asymmetric upside"),
                journal("GLD", "other", -5, "Non-owner note should not render"),
            ],
            private_asset_convergence: vec![
                convergence("BTC", vec![view("LOW", 0), view("HIGH", 2)]),
                convergence("GLD", vec![view("LOW", -5)]),
            ],
            ..BuildContext::default()
        }
    }

    fn aligned_fixture() -> BuildContext {
        BuildContext {
            private_positions: vec![position("BTC", 42.0)],
            private_journal_views: vec![journal("BTC", "skylar", 3, "Aligned positive read")],
            private_asset_convergence: vec![convergence(
                "BTC",
                vec![view("LOW", 2), view("HIGH", 3), view("MACRO", 4)],
            )],
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

    fn journal(
        symbol: &str,
        author: &str,
        conviction: i64,
        summary: &str,
    ) -> PrivateJournalViewRow {
        PrivateJournalViewRow {
            symbol: symbol.to_string(),
            author: author.to_string(),
            conviction,
            summary: summary.to_string(),
        }
    }

    fn convergence(
        symbol: &str,
        views: Vec<PrivateAssetConvergenceView>,
    ) -> PrivateAssetConvergenceRow {
        PrivateAssetConvergenceRow {
            symbol: symbol.to_string(),
            target_pct: None,
            views,
        }
    }

    fn view(analyst: &str, conviction: i64) -> PrivateAssetConvergenceView {
        PrivateAssetConvergenceView {
            analyst: analyst.to_string(),
            conviction,
            reasoning_summary: "synthetic view".to_string(),
        }
    }
}
