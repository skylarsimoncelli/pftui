#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    AnalystViewSummary, BuildContext, EquityBreadthSummary, EquityEarningsSummary, EquityMarketRow,
    EquityNewsSignal,
};

pub fn render_public_equities(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Equities\n\n");

    output.push_str("### Current State\n\n");
    output.push_str(&render_current_state(ctx));
    output.push_str("\n\n### Multi-Timeframe View\n\n");
    output.push_str(&render_multi_timeframe_view(&ctx.equity_analyst_views));
    output.push_str("\n\n### What to Watch\n\n");
    output.push_str(&render_what_to_watch(&ctx.equity_news));

    Ok(output.trim_end().to_string())
}

fn render_current_state(ctx: &BuildContext) -> String {
    let mut parts = Vec::new();

    if ctx.equity_indices.is_empty() {
        parts.push("Broad-index rows are unavailable in the build context. Run the market-data refresh before relying on this section for SPX/SPY or NDX/QQQ price context.".to_string());
    } else {
        parts.push(render_market_table("Broad indices", &ctx.equity_indices));
    }

    if ctx.equity_sectors.is_empty() {
        parts.push("Sector ETF rows are unavailable, so this section does not infer equity leadership or rotation.".to_string());
    } else {
        parts.push(render_market_table("Sector ETFs", &ctx.equity_sectors));
    }

    match &ctx.equity_breadth {
        Some(summary) => parts.push(render_breadth(summary)),
        None => parts.push("Breadth data is unavailable. Treat index-level strength or weakness as price-only until advance/decline or participation data refreshes.".to_string()),
    }

    match &ctx.equity_earnings {
        Some(summary) => parts.push(render_earnings(summary)),
        None => parts.push("Earnings-calendar context is unavailable, so this section does not make earnings-season or revision-breadth claims.".to_string()),
    }

    parts.join("\n\n")
}

fn render_market_table(label: &str, rows: &[EquityMarketRow]) -> String {
    let mut table = format!(
        "{label}:\n\n| Name | Symbol | Price | Daily Chg | Weekly Chg | Trend | Freshness |\n|---|---|---:|---:|---:|---|---|\n"
    );
    for row in rows {
        table.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            clean_cell(&row.name),
            clean_cell(&row.symbol),
            clean_cell(row.price.as_deref().unwrap_or("n/a")),
            format_pct(row.daily_change_pct),
            format_pct(row.weekly_change_pct),
            clean_cell(row.trend.as_deref().unwrap_or("n/a")),
            clean_cell(row.freshness.as_deref().unwrap_or("freshness unavailable")),
        ));
    }
    table.trim_end().to_string()
}

fn render_breadth(summary: &EquityBreadthSummary) -> String {
    format!(
        "Breadth: {} at {}. {} Data freshness: {}.",
        clean_text(&summary.label),
        clean_text(summary.value.as_deref().unwrap_or("n/a")),
        sentence(summary.interpretation.as_deref().unwrap_or(
            "No sourced interpretation is attached, so participation claims stay provisional"
        )),
        sentence_fragment(
            summary
                .freshness
                .as_deref()
                .unwrap_or("freshness unavailable")
        ),
    )
}

fn render_earnings(summary: &EquityEarningsSummary) -> String {
    format!(
        "Earnings: {} at {}. {} Data freshness: {}.",
        clean_text(&summary.label),
        clean_text(summary.value.as_deref().unwrap_or("n/a")),
        sentence(summary.interpretation.as_deref().unwrap_or(
            "No sourced interpretation is attached, so earnings claims stay provisional"
        )),
        sentence_fragment(
            summary
                .freshness
                .as_deref()
                .unwrap_or("freshness unavailable")
        ),
    )
}

fn render_multi_timeframe_view(views: &[AnalystViewSummary]) -> String {
    if views.is_empty() {
        return "No equity-specific analyst rows are attached to this report build. Keep the equity read descriptive until LOW, MEDIUM, HIGH, and MACRO views refresh.".to_string();
    }

    let mut table = String::from("| Layer | Focus | View |\n|---|---|---|\n");
    for view in views {
        table.push_str(&format!(
            "| {} | {} | {} |\n",
            clean_cell(&view.layer),
            clean_cell(&view.asset),
            clean_cell(&sentence_fragment(&view.summary)),
        ));
    }

    let missing_layers = missing_layers(views);
    if missing_layers.is_empty() {
        table.trim_end().to_string()
    } else {
        format!(
            "{}\n\nMissing equity analyst layers: {}. Cross-timeframe equity claims should stay qualified.",
            table.trim_end(),
            missing_layers.join(", ")
        )
    }
}

fn render_what_to_watch(news: &[EquityNewsSignal]) -> String {
    if news.is_empty() {
        return "No equity news rows are attached to this build. This does not prove catalyst absence; it only means no sourced equity watch rows were loaded.".to_string();
    }

    news.iter()
        .take(5)
        .map(|item| {
            format!(
                "- {}. Source: {} ({}{}, {}) | Topic: {} | Relevance: {}.",
                sentence_fragment(&item.headline),
                clean_text(&item.domain),
                tier_label(item.source_tier),
                inferred_label(item.source_tier),
                clean_text(
                    item.independence
                        .as_deref()
                        .unwrap_or("independence unknown")
                ),
                clean_text(item.topic.as_deref().unwrap_or("unclassified")),
                sentence_fragment(
                    item.relevance
                        .as_deref()
                        .unwrap_or("market relevance still being classified")
                )
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn missing_layers(views: &[AnalystViewSummary]) -> Vec<&'static str> {
    ["LOW", "MEDIUM", "HIGH", "MACRO"]
        .into_iter()
        .filter(|layer| {
            !views
                .iter()
                .any(|view| view.layer.eq_ignore_ascii_case(layer))
        })
        .collect()
}

fn tier_label(tier: Option<u8>) -> String {
    tier.map(|tier| format!("Tier {tier}"))
        .unwrap_or_else(|| "Tier unknown".to_string())
}

fn inferred_label(tier: Option<u8>) -> &'static str {
    if tier.is_some() {
        ""
    } else {
        ", inferred"
    }
}

fn format_pct(value: Option<f64>) -> String {
    match value {
        Some(value) if value > 0.0 => format!("+{value:.1}%"),
        Some(value) => format!("{value:.1}%"),
        None => "n/a".to_string(),
    }
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
    value.trim().to_string()
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_equities_renders_broad_indices_and_sector_rows() {
        let ctx = BuildContext {
            equity_indices: vec![
                market("S&P 500", "SPX", "6,250", Some(0.7), Some(1.4), "risk bid"),
                market(
                    "Nasdaq 100",
                    "NDX",
                    "22,100",
                    Some(1.1),
                    Some(2.6),
                    "growth leadership",
                ),
                market(
                    "SPDR S&P 500 ETF",
                    "SPY",
                    "$625.00",
                    Some(0.6),
                    Some(1.3),
                    "tracks SPX",
                ),
                market(
                    "Invesco QQQ Trust",
                    "QQQ",
                    "$540.00",
                    Some(1.0),
                    Some(2.5),
                    "tracks NDX",
                ),
            ],
            equity_sectors: vec![
                market(
                    "Technology Select Sector",
                    "XLK",
                    "$238.10",
                    Some(1.4),
                    Some(3.1),
                    "leading",
                ),
                market(
                    "Financial Select Sector",
                    "XLF",
                    "$47.20",
                    Some(-0.2),
                    Some(0.8),
                    "lagging",
                ),
                market(
                    "Energy Select Sector",
                    "XLE",
                    "$92.40",
                    Some(0.4),
                    Some(-1.1),
                    "range-bound",
                ),
            ],
            equity_breadth: Some(EquityBreadthSummary {
                label: "NYSE advance/decline".to_string(),
                value: Some("58% advancing".to_string()),
                interpretation: Some("Participation confirms the index move".to_string()),
                freshness: Some("fresh 1d".to_string()),
            }),
            equity_earnings: Some(EquityEarningsSummary {
                label: "Earnings calendar".to_string(),
                value: Some("12 S&P 500 reports this week".to_string()),
                interpretation: Some(
                    "Single-name catalysts are concentrated in megacap tech".to_string(),
                ),
                freshness: Some("fresh 1d".to_string()),
            }),
            equity_analyst_views: vec![
                view("LOW", "SPY", "Positive tape while breadth holds"),
                view("MEDIUM", "QQQ", "AI beta remains the swing factor"),
                view(
                    "HIGH",
                    "Equities",
                    "Multiple risk stays tied to real yields",
                ),
                view(
                    "MACRO",
                    "US equities",
                    "Liquidity still constrains durable upside",
                ),
            ],
            equity_news: vec![EquityNewsSignal {
                headline: "Large-cap earnings guidance resets sector leadership".to_string(),
                domain: "example.com".to_string(),
                source_tier: Some(2),
                independence: Some("independent".to_string()),
                topic: Some("earnings".to_string()),
                relevance: Some("Watch whether index gains broaden beyond megacaps".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_equities(&ctx).unwrap();

        assert!(rendered.starts_with("## Equities\n\n"));
        assert!(rendered.contains("### Current State"));
        assert!(
            rendered.contains("| S&P 500 | SPX | 6,250 | +0.7% | +1.4% | risk bid | fresh 1d |")
        );
        assert!(rendered.contains(
            "| Technology Select Sector | XLK | $238.10 | +1.4% | +3.1% | leading | fresh 1d |"
        ));
        assert!(rendered.contains("Breadth: NYSE advance/decline at 58% advancing."));
        assert!(rendered.contains("Earnings: Earnings calendar at 12 S&P 500 reports this week."));
        assert!(rendered
            .contains("| MACRO | US equities | Liquidity still constrains durable upside |"));
        assert!(rendered.contains("Source: example.com (Tier 2, independent) | Topic: earnings"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_equities_falls_back_when_breadth_and_earnings_are_absent() {
        let ctx = BuildContext {
            equity_indices: vec![market("S&P 500", "SPX", "6,250", Some(0.2), None, "mixed")],
            equity_sectors: vec![market(
                "Utilities Select Sector",
                "XLU",
                "$72.00",
                None,
                None,
                "defensive",
            )],
            ..BuildContext::default()
        };

        let rendered = render_public_equities(&ctx).unwrap();

        assert!(rendered.contains("Breadth data is unavailable"));
        assert!(rendered.contains("Earnings-calendar context is unavailable"));
        assert!(rendered.contains("No equity-specific analyst rows are attached"));
        assert!(rendered.contains("No equity news rows are attached"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_equities_avoids_unsupported_market_cap_claims() {
        let rendered = render_public_equities(&BuildContext::default()).unwrap();
        let lowered = rendered.to_ascii_lowercase();

        for forbidden in [
            "market cap expanded",
            "market capitalization",
            "trillions of value",
            "mega-cap weight",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "unsupported market-cap claim leaked: {forbidden}"
            );
        }
        assert_public_safe(&rendered);
    }

    fn market(
        name: &str,
        symbol: &str,
        price: &str,
        daily_change_pct: Option<f64>,
        weekly_change_pct: Option<f64>,
        trend: &str,
    ) -> EquityMarketRow {
        EquityMarketRow {
            name: name.to_string(),
            symbol: symbol.to_string(),
            price: Some(price.to_string()),
            daily_change_pct,
            weekly_change_pct,
            trend: Some(trend.to_string()),
            freshness: Some("fresh 1d".to_string()),
        }
    }

    fn view(layer: &str, asset: &str, summary: &str) -> AnalystViewSummary {
        AnalystViewSummary {
            layer: layer.to_string(),
            asset: asset.to_string(),
            summary: summary.to_string(),
        }
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
            "allocation",
            "position size",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public equities leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
