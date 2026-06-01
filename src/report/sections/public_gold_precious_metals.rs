#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    AnalystViewSummary, BuildContext, PreciousMetalMarketRow, PreciousMetalsNewsSignal,
    PreciousMetalsSupplyRow, RealYieldSummary, SovereignHoldingSummary,
};

pub fn render_public_gold_precious_metals(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Gold (and Precious Metals)\n\n");

    output.push_str("### Current State\n\n");
    output.push_str(&render_current_state(ctx));
    output.push_str("\n\n### Multi-Timeframe View\n\n");
    output.push_str(&render_multi_timeframe_view(
        &ctx.precious_metals_analyst_views,
    ));
    output.push_str("\n\n### What to Watch\n\n");
    output.push_str(&render_what_to_watch(&ctx.precious_metals_news));

    Ok(output.trim_end().to_string())
}

fn render_current_state(ctx: &BuildContext) -> String {
    let mut parts = Vec::new();

    if ctx.precious_metals_market.is_empty() {
        parts.push("Gold and silver price rows are unavailable in the build context. Run the market-data refresh before relying on this section for spot or trend claims.".to_string());
    } else {
        parts.push(render_market_table(&ctx.precious_metals_market));
    }

    match &ctx.real_yield_context {
        Some(row) => parts.push(render_real_yields(row)),
        None => parts.push("Real-yield context is unavailable, so this section does not infer whether rates are helping or hurting precious metals.".to_string()),
    }

    if ctx.precious_metals_supply.is_empty() {
        parts.push("COMEX, COT, and supply rows are unavailable. Treat physical-market and positioning claims as unverified until those caches refresh.".to_string());
    } else {
        parts.push(render_supply_table(&ctx.precious_metals_supply));
    }

    if !ctx.sovereign_gold_holdings.is_empty() {
        parts.push(render_sovereign_table(&ctx.sovereign_gold_holdings));
    }

    parts.join("\n\n")
}

fn render_market_table(rows: &[PreciousMetalMarketRow]) -> String {
    let mut table = String::from(
        "| Asset | Symbol | Price | Daily Chg | Weekly Chg | Trend | Freshness |\n|---|---|---:|---:|---:|---|---|\n",
    );
    for row in rows {
        table.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            clean_cell(&row.asset),
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

fn render_real_yields(row: &RealYieldSummary) -> String {
    format!(
        "Real-yield context: {} with {} direction. {} Data freshness: {}.",
        clean_text(row.value.as_deref().unwrap_or("n/a")),
        sentence_fragment(row.direction.as_deref().unwrap_or("unknown")),
        sentence(row.interpretation.as_deref().unwrap_or(
            "No sourced interpretation is attached, so the rate impulse stays provisional"
        )),
        sentence_fragment(row.freshness.as_deref().unwrap_or("freshness unavailable"))
    )
}

fn render_supply_table(rows: &[PreciousMetalsSupplyRow]) -> String {
    let mut table =
        String::from("| Asset | Metric | Latest | Read | Freshness |\n|---|---|---:|---|---|\n");
    for row in rows {
        table.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean_cell(&row.asset),
            clean_cell(&row.metric),
            clean_cell(row.value.as_deref().unwrap_or("n/a")),
            clean_cell(row.interpretation.as_deref().unwrap_or("n/a")),
            clean_cell(row.freshness.as_deref().unwrap_or("freshness unavailable")),
        ));
    }
    table.trim_end().to_string()
}

fn render_sovereign_table(rows: &[SovereignHoldingSummary]) -> String {
    let mut table = String::from(
        "Sovereign holdings:\n\n| Holder | Latest | Change | Freshness |\n|---|---:|---:|---|\n",
    );
    for row in rows {
        table.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            clean_cell(&row.holder),
            clean_cell(row.latest.as_deref().unwrap_or("n/a")),
            clean_cell(row.change.as_deref().unwrap_or("n/a")),
            clean_cell(row.freshness.as_deref().unwrap_or("freshness unavailable")),
        ));
    }
    table.trim_end().to_string()
}

fn render_multi_timeframe_view(views: &[AnalystViewSummary]) -> String {
    if views.is_empty() {
        return "No gold or silver analyst rows are attached to this report build. Keep the precious-metals read descriptive until LOW, MEDIUM, HIGH, and MACRO views refresh.".to_string();
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
            "{}\n\nMissing precious-metals analyst layers: {}. Cross-timeframe claims should stay qualified.",
            table.trim_end(),
            missing_layers.join(", ")
        )
    }
}

fn render_what_to_watch(news: &[PreciousMetalsNewsSignal]) -> String {
    if news.is_empty() {
        return "No gold or silver news rows are attached to this build. This does not prove catalyst absence; it only means no sourced precious-metals watch rows were loaded.".to_string();
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
    fn public_gold_precious_metals_renders_gold_and_silver() {
        let ctx = BuildContext {
            precious_metals_market: vec![
                metal(
                    "Gold",
                    "GC=F",
                    "$3,420",
                    Some(0.8),
                    Some(2.4),
                    "above 50d range",
                    "fresh 1d",
                ),
                metal(
                    "Silver",
                    "SI=F",
                    "$37.20",
                    Some(1.3),
                    Some(4.1),
                    "confirming gold",
                    "fresh 1d",
                ),
            ],
            real_yield_context: Some(RealYieldSummary {
                value: Some("1.85%".to_string()),
                direction: Some("falling".to_string()),
                interpretation: Some(
                    "Lower real yields are supportive for duration-like precious metals"
                        .to_string(),
                ),
                freshness: Some("fresh 1d".to_string()),
            }),
            precious_metals_supply: vec![
                supply(
                    "Gold",
                    "COMEX registered",
                    "18.4m oz",
                    "inventory stable",
                    "fresh 1d",
                ),
                supply(
                    "Silver",
                    "COT managed money",
                    "+12k contracts",
                    "positioning extended",
                    "fresh 5d",
                ),
            ],
            sovereign_gold_holdings: vec![SovereignHoldingSummary {
                holder: "Central banks".to_string(),
                latest: Some("net buyers".to_string()),
                change: Some("+18t m/m".to_string()),
                freshness: Some("fresh 30d".to_string()),
            }],
            precious_metals_analyst_views: vec![
                view("LOW", "Gold", "spot trend remains constructive"),
                view(
                    "MEDIUM",
                    "Silver",
                    "silver beta is confirming gold strength",
                ),
                view("HIGH", "Gold", "real-yield impulse supports the breakout"),
                view(
                    "MACRO",
                    "Gold",
                    "reserve-diversification thesis remains intact",
                ),
            ],
            precious_metals_news: vec![PreciousMetalsNewsSignal {
                headline: "Central-bank gold demand stayed firm".to_string(),
                domain: "example.com".to_string(),
                source_tier: Some(2),
                independence: Some("independent".to_string()),
                topic: Some("gold-sovereign-demand".to_string()),
                relevance: Some("supports strategic demand narrative".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_gold_precious_metals(&ctx).unwrap();

        assert!(rendered.starts_with("## Gold (and Precious Metals)\n\n"));
        assert!(rendered
            .contains("| Gold | GC=F | $3,420 | +0.8% | +2.4% | above 50d range | fresh 1d |"));
        assert!(rendered
            .contains("| Silver | SI=F | $37.20 | +1.3% | +4.1% | confirming gold | fresh 1d |"));
        assert!(rendered.contains("Real-yield context: 1.85% with falling direction"));
        assert!(rendered.contains("Sovereign holdings:"));
        assert!(
            rendered.contains("| MACRO | Gold | reserve-diversification thesis remains intact |")
        );
        assert!(rendered.contains("Source: example.com (Tier 2, independent)"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_gold_precious_metals_degrades_when_comex_cot_stale() {
        let ctx = BuildContext {
            precious_metals_market: vec![metal(
                "Gold",
                "GC=F",
                "$3,400",
                None,
                None,
                "trend unavailable",
                "freshness unavailable",
            )],
            precious_metals_supply: vec![
                supply("Gold", "COMEX registered", "n/a", "cache stale", "stale 9d"),
                supply(
                    "Silver",
                    "COT managed money",
                    "n/a",
                    "cache stale",
                    "stale 12d",
                ),
            ],
            precious_metals_analyst_views: vec![view(
                "HIGH",
                "Gold",
                "medium-term view is constructive",
            )],
            ..BuildContext::default()
        };

        let rendered = render_public_gold_precious_metals(&ctx).unwrap();

        assert!(rendered.contains("Real-yield context is unavailable"));
        assert!(rendered.contains("| Gold | COMEX registered | n/a | cache stale | stale 9d |"));
        assert!(rendered.contains("| Silver | COT managed money | n/a | cache stale | stale 12d |"));
        assert!(rendered.contains("Missing precious-metals analyst layers: LOW, MEDIUM, MACRO"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_gold_precious_metals_includes_inferred_source_tier_metadata() {
        let ctx = BuildContext {
            precious_metals_news: vec![PreciousMetalsNewsSignal {
                headline: "Silver inventories moved sharply".to_string(),
                domain: "unknown-source.test".to_string(),
                source_tier: None,
                independence: None,
                topic: None,
                relevance: None,
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_gold_precious_metals(&ctx).unwrap();

        assert!(rendered.contains(
            "Source: unknown-source.test (Tier unknown, inferred, independence unknown)"
        ));
        assert!(rendered.contains("Topic: unclassified"));
        assert!(rendered.contains("COMEX, COT, and supply rows are unavailable"));
        assert_public_safe(&rendered);
    }

    fn metal(
        asset: &str,
        symbol: &str,
        price: &str,
        daily_change_pct: Option<f64>,
        weekly_change_pct: Option<f64>,
        trend: &str,
        freshness: &str,
    ) -> PreciousMetalMarketRow {
        PreciousMetalMarketRow {
            asset: asset.to_string(),
            symbol: symbol.to_string(),
            price: Some(price.to_string()),
            daily_change_pct,
            weekly_change_pct,
            trend: Some(trend.to_string()),
            freshness: Some(freshness.to_string()),
        }
    }

    fn supply(
        asset: &str,
        metric: &str,
        value: &str,
        interpretation: &str,
        freshness: &str,
    ) -> PreciousMetalsSupplyRow {
        PreciousMetalsSupplyRow {
            asset: asset.to_string(),
            metric: metric.to_string(),
            value: Some(value.to_string()),
            interpretation: Some(interpretation.to_string()),
            freshness: Some(freshness.to_string()),
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
            "my gold",
            "my silver",
            "cost basis",
            "unrealized",
            "transaction",
            "allocation percentage",
            "position size",
            "my portfolio",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public gold leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
