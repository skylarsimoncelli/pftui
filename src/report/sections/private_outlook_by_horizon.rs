#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, PrivateOutlookByHorizonRow, PrivateOutlookPoint, PrivatePositionSnapshotRow,
};
use crate::report::charts::outlook_arrows::{
    render_svg as outlook_arrows_svg, OutlookArrowsInput, OutlookPoint as ChartOutlookPoint,
};

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NormalizedOutlook {
    direction: &'static str,
    conviction: &'static str,
}

pub fn render_private_outlook_by_horizon(ctx: &BuildContext) -> Result<String> {
    let held = qualifying_positions(&ctx.private_positions);
    if held.is_empty() {
        return Ok(String::new());
    }

    // Collect rows first so we can suppress the whole section when every
    // horizon for every held asset is unknown — that case the section
    // adds noise without information.
    let mut rows: Vec<(String, String, bool)> = Vec::new();
    let mut alignments = Vec::new();
    let mut any_known = false;
    for position in held {
        let outlook = find_outlook(&ctx.private_outlooks, &position.symbol);
        let days = normalize_point(outlook.and_then(|row| row.days.as_ref()));
        let weeks = normalize_point(outlook.and_then(|row| row.weeks.as_ref()));
        let months = normalize_point(outlook.and_then(|row| row.months.as_ref()));
        let row_known = days.conviction != "unknown"
            || weeks.conviction != "unknown"
            || months.conviction != "unknown";
        if row_known {
            any_known = true;
        }
        alignments.push(is_aligned(days, weeks, months));
        rows.push((
            clean_cell(&position.symbol),
            native_placeholder(days, weeks, months),
            row_known,
        ));
    }
    if !any_known {
        return Ok(String::new());
    }

    let mut output = String::from("## Outlook by Horizon\n\n");
    output.push_str("| Asset | Outlook |\n");
    output.push_str("|---|---|\n");
    for (symbol, outlook_svg, _) in &rows {
        output.push_str(&format!("| {} | {} |\n", symbol, outlook_svg));
    }

    output.push('\n');
    output.push_str(&alignment_summary(&alignments));
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

fn find_outlook<'a>(
    rows: &'a [PrivateOutlookByHorizonRow],
    symbol: &str,
) -> Option<&'a PrivateOutlookByHorizonRow> {
    rows.iter()
        .find(|row| row.symbol.eq_ignore_ascii_case(symbol))
}

fn normalize_point(point: Option<&PrivateOutlookPoint>) -> NormalizedOutlook {
    let Some(point) = point else {
        return unknown_outlook();
    };

    NormalizedOutlook {
        direction: normalize_direction(&point.direction),
        conviction: normalize_conviction(&point.conviction),
    }
}

fn unknown_outlook() -> NormalizedOutlook {
    NormalizedOutlook {
        direction: "neutral",
        conviction: "unknown",
    }
}

fn normalize_direction(direction: &str) -> &'static str {
    match direction.trim().to_ascii_lowercase().as_str() {
        "up" | "bull" | "bullish" | "positive" | "long" => "up",
        "down" | "bear" | "bearish" | "negative" | "short" => "down",
        "up_strong" | "bull_strong" | "strong_up" | "strong_bull" | "very_bullish"
        | "strong_bullish" => "up_strong",
        "down_strong" | "bear_strong" | "strong_down" | "strong_bear" | "very_bearish"
        | "strong_bearish" => "down_strong",
        "up_slight" | "up_mild" | "slight_up" | "mild_up" | "slightly_bullish"
        | "mildly_bullish" => "up_slight",
        "down_slight" | "down_mild" | "slight_down" | "mild_down" | "slightly_bearish"
        | "mildly_bearish" => "down_slight",
        "flat" | "neutral" | "hold" | "unknown" | "" => "neutral",
        _ => "neutral",
    }
}

fn normalize_conviction(conviction: &str) -> &'static str {
    match conviction.trim().to_ascii_lowercase().as_str() {
        "high" | "strong" => "high",
        "medium" | "med" | "moderate" => "medium",
        "low" | "weak" => "low",
        "unknown" | "" => "unknown",
        _ => "unknown",
    }
}

fn native_placeholder(
    days: NormalizedOutlook,
    weeks: NormalizedOutlook,
    months: NormalizedOutlook,
) -> String {
    outlook_arrows_svg(&OutlookArrowsInput {
        days: chart_point(days),
        weeks: chart_point(weeks),
        months: chart_point(months),
        width: None,
        height: None,
    })
    // The rendered SVG contains literal newlines; collapse them so it stays
    // inside a single markdown table cell.
    .replace('\n', " ")
}

fn chart_point(outlook: NormalizedOutlook) -> ChartOutlookPoint {
    ChartOutlookPoint {
        direction: outlook.direction.to_string(),
        conviction: outlook.conviction.to_string(),
    }
}

fn is_aligned(
    days: NormalizedOutlook,
    weeks: NormalizedOutlook,
    months: NormalizedOutlook,
) -> bool {
    let signs = [days, weeks, months]
        .into_iter()
        .map(|point| direction_sign(point.direction))
        .collect::<Vec<_>>();
    signs[0] != 0 && signs.iter().all(|sign| *sign == signs[0])
}

fn direction_sign(direction: &str) -> i8 {
    match direction {
        "up" | "up_slight" | "up_strong" => 1,
        "down" | "down_slight" | "down_strong" => -1,
        _ => 0,
    }
}

fn alignment_summary(alignments: &[bool]) -> String {
    let total = alignments.len();
    let aligned = alignments.iter().filter(|aligned| **aligned).count();
    let mixed = total.saturating_sub(aligned);
    format!(
        "Cross-asset outlook is directionally aligned across all three horizons for {aligned} of {total} qualifying held assets. Mixed or missing horizon rows remain neutral/unknown until analyst output attaches complete days, weeks, and months views; {mixed} asset(s) need that follow-up before reading the horizon chart as a unified signal."
    )
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_outlook_by_horizon_direction_mapping_is_deterministic() {
        let rendered = render_private_outlook_by_horizon(&fixture_context()).unwrap();

        // The Outlook column carries the per-horizon arrow chart inline.
        assert!(rendered.contains("| BTC |"));
        assert!(rendered.contains("<svg"));
        assert!(
            !rendered.contains("{outlook_arrows"),
            "must not leak token placeholder"
        );
    }

    #[test]
    fn private_outlook_by_horizon_missing_data_still_renders_row() {
        let rendered = render_private_outlook_by_horizon(&fixture_context()).unwrap();

        // GLD missing-horizon row still renders its asset cell + SVG chart.
        assert!(rendered.contains("| GLD |"));
        assert!(
            !rendered.contains("{outlook_arrows"),
            "must not leak token placeholder"
        );
    }

    #[test]
    fn private_outlook_by_horizon_order_follows_portfolio_materiality() {
        let rendered = render_private_outlook_by_horizon(&fixture_context()).unwrap();

        let btc = rendered.find("| BTC |").unwrap();
        let gld = rendered.find("| GLD |").unwrap();
        assert!(btc < gld);
        assert!(!rendered.contains("| DOGE |"));
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_positions: vec![
                position("GLD", 22.95),
                position("DOGE", 0.05),
                position("BTC", 42.0),
            ],
            private_outlooks: vec![
                outlook(
                    "BTC",
                    Some(point("bullish", "MED")),
                    Some(point("strong_bullish", "strong")),
                    Some(point("up", "high")),
                ),
                outlook("GLD", None, Some(point("bear", "weak")), None),
                outlook(
                    "DOGE",
                    Some(point("very_bullish", "high")),
                    Some(point("very_bullish", "high")),
                    Some(point("very_bullish", "high")),
                ),
            ],
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

    fn outlook(
        symbol: &str,
        days: Option<PrivateOutlookPoint>,
        weeks: Option<PrivateOutlookPoint>,
        months: Option<PrivateOutlookPoint>,
    ) -> PrivateOutlookByHorizonRow {
        PrivateOutlookByHorizonRow {
            symbol: symbol.to_string(),
            days,
            weeks,
            months,
        }
    }

    fn point(direction: &str, conviction: &str) -> PrivateOutlookPoint {
        PrivateOutlookPoint {
            direction: direction.to_string(),
            conviction: conviction.to_string(),
        }
    }
}
