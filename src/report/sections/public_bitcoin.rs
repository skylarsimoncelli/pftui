#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    AnalystViewSummary, BitcoinCatalystSummary, BitcoinEtfFlowSummary, BitcoinMarketSummary,
    BitcoinOnChainSummary, BitcoinPredictionSignal, BuildContext,
};

pub fn render_public_bitcoin(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Bitcoin\n\n");

    output.push_str("### Current State\n\n");
    output.push_str(&render_current_state(ctx));
    output.push_str("\n\n### Multi-Timeframe View\n\n");
    output.push_str(&render_multi_timeframe_view(&ctx.bitcoin_analyst_views));
    output.push_str("\n\n### What to Watch\n\n");
    output.push_str(&render_what_to_watch(
        &ctx.bitcoin_news,
        &ctx.bitcoin_prediction_signals,
    ));

    Ok(output.trim_end().to_string())
}

fn render_current_state(ctx: &BuildContext) -> String {
    let mut parts = Vec::new();

    match &ctx.bitcoin_market {
        Some(market) => parts.push(render_market_summary(market)),
        None => parts.push("BTC price context is unavailable in the build context. Run the market-data refresh before relying on this section for spot or trend claims.".to_string()),
    }

    if !ctx.bitcoin_etf_flows.is_empty() {
        parts.push(render_etf_flows(&ctx.bitcoin_etf_flows));
    }

    if !ctx.bitcoin_onchain.is_empty() {
        parts.push(render_onchain(&ctx.bitcoin_onchain));
    }

    parts.join("\n\n")
}

fn render_market_summary(market: &BitcoinMarketSummary) -> String {
    let price = market.price.as_deref().unwrap_or("n/a");
    let daily = format_pct(market.daily_change_pct);
    let weekly = format_pct(market.weekly_change_pct);
    let trend = market.trend.as_deref().unwrap_or("trend unavailable");
    let freshness = market
        .freshness
        .as_deref()
        .unwrap_or("freshness unavailable");

    format!(
        "BTC price: {} (1d {}, 7d {}). Trend: {}. Data freshness: {}.",
        clean_text(price),
        daily,
        weekly,
        sentence_fragment(trend),
        sentence_fragment(freshness)
    )
}

fn render_etf_flows(rows: &[BitcoinEtfFlowSummary]) -> String {
    let mut table = String::from(
        "ETF flows:\n\n| Period | Net Flow | Detail | Freshness |\n|---|---:|---|---|\n",
    );
    for row in rows {
        table.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            clean_cell(&row.period),
            clean_cell(row.net_flow.as_deref().unwrap_or("n/a")),
            clean_cell(row.detail.as_deref().unwrap_or("n/a")),
            clean_cell(row.freshness.as_deref().unwrap_or("freshness unavailable")),
        ));
    }
    table.trim_end().to_string()
}

fn render_onchain(rows: &[BitcoinOnChainSummary]) -> String {
    let mut table = String::from(
        "On-chain and exchange-reserve context:\n\n| Metric | Latest | Read | Freshness |\n|---|---:|---|---|\n",
    );
    for row in rows {
        table.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            clean_cell(&row.metric),
            clean_cell(row.value.as_deref().unwrap_or("n/a")),
            clean_cell(row.interpretation.as_deref().unwrap_or("n/a")),
            clean_cell(row.freshness.as_deref().unwrap_or("freshness unavailable")),
        ));
    }
    table.trim_end().to_string()
}

fn render_multi_timeframe_view(views: &[AnalystViewSummary]) -> String {
    if views.is_empty() {
        return "No BTC-specific analyst rows are attached to this report build. Keep the Bitcoin read descriptive until LOW, MEDIUM, HIGH, and MACRO views are refreshed.".to_string();
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
            "{}\n\nMissing BTC analyst layers: {}. Cross-timeframe BTC claims should stay qualified.",
            table.trim_end(),
            missing_layers.join(", ")
        )
    }
}

fn render_what_to_watch(
    news: &[BitcoinCatalystSummary],
    predictions: &[BitcoinPredictionSignal],
) -> String {
    if news.is_empty() && predictions.is_empty() {
        return "No BTC-specific news or prediction-market rows are attached to this build. This does not prove catalyst absence; it only means no sourced Bitcoin watch rows were loaded.".to_string();
    }

    let mut parts = Vec::new();

    if !news.is_empty() {
        let mut bullets = String::from("News catalysts:\n");
        for item in news.iter().take(5) {
            let source = item.source.as_deref().unwrap_or("source unavailable");
            let relevance = item
                .relevance
                .as_deref()
                .unwrap_or("market relevance still being classified");
            bullets.push_str(&format!(
                "- {}. Source: {}. Relevance: {}.\n",
                sentence_fragment(&item.headline),
                clean_text(source),
                sentence_fragment(relevance)
            ));
        }
        parts.push(bullets.trim_end().to_string());
    }

    if !predictions.is_empty() {
        let mut table = String::from(
            "Prediction-market signals:\n\n| Market | Probability | 7d Delta | Relevance |\n|---|---:|---:|---|\n",
        );
        for signal in predictions.iter().take(5) {
            table.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                clean_cell(&signal.market),
                format_probability(signal.probability),
                format_delta(signal.delta_7d),
                clean_cell(signal.relevance.as_deref().unwrap_or("n/a")),
            ));
        }
        parts.push(table.trim_end().to_string());
    }

    parts.join("\n\n")
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

fn format_pct(value: Option<f64>) -> String {
    match value {
        Some(value) if value > 0.0 => format!("+{value:.1}%"),
        Some(value) => format!("{value:.1}%"),
        None => "n/a".to_string(),
    }
}

fn format_probability(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.0}%"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn format_delta(value: Option<f64>) -> String {
    match value {
        Some(value) if value > 0.0 => format!("+{value:.0}pp"),
        Some(value) => format!("{value:.0}pp"),
        None => "n/a".to_string(),
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
    fn public_bitcoin_renders_with_btc_price_only() {
        let ctx = BuildContext {
            bitcoin_market: Some(BitcoinMarketSummary {
                price: Some("$108,500".to_string()),
                daily_change_pct: Some(1.2),
                weekly_change_pct: None,
                trend: Some("holding above the 50-day range".to_string()),
                freshness: Some("fresh 1d".to_string()),
            }),
            ..BuildContext::default()
        };

        let rendered = render_public_bitcoin(&ctx).unwrap();

        assert!(rendered.starts_with("## Bitcoin\n\n"));
        assert!(rendered.contains("### Current State"));
        assert!(rendered.contains("BTC price: $108,500 (1d +1.2%, 7d n/a)"));
        assert!(!rendered.contains("ETF flows:"));
        assert!(!rendered.contains("On-chain and exchange-reserve context:"));
        assert!(rendered.contains("No BTC-specific analyst rows"));
        assert!(rendered.contains("No BTC-specific news or prediction-market rows"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_bitcoin_conditionally_includes_etf_onchain_and_signals() {
        let ctx = BuildContext {
            bitcoin_market: Some(BitcoinMarketSummary {
                price: Some("$109,250".to_string()),
                daily_change_pct: Some(-0.6),
                weekly_change_pct: Some(3.4),
                trend: Some("spot demand is firm but momentum is slowing".to_string()),
                freshness: Some("fresh 1d".to_string()),
            }),
            bitcoin_etf_flows: vec![BitcoinEtfFlowSummary {
                period: "Last session".to_string(),
                net_flow: Some("+$420m".to_string()),
                detail: Some("broad issuer inflow".to_string()),
                freshness: Some("fresh 1d".to_string()),
            }],
            bitcoin_onchain: vec![BitcoinOnChainSummary {
                metric: "Exchange reserves".to_string(),
                value: Some("2.1m BTC".to_string()),
                interpretation: Some("reserve drawdown supports supply-tightness read".to_string()),
                freshness: Some("fresh 2d".to_string()),
            }],
            bitcoin_analyst_views: vec![
                view(
                    "LOW",
                    "BTC spot",
                    "range breakout is constructive but volume is thin",
                ),
                view("MEDIUM", "ETF demand", "flow trend supports dips"),
                view("HIGH", "Cycle", "cycle structure remains constructive"),
                view(
                    "MACRO",
                    "Monetary hedge",
                    "liquidity regime is the key constraint",
                ),
            ],
            bitcoin_news: vec![BitcoinCatalystSummary {
                headline: "Spot ETF inflows accelerated into the close".to_string(),
                source: Some("example.com".to_string()),
                relevance: Some("confirms institutional demand impulse".to_string()),
            }],
            bitcoin_prediction_signals: vec![BitcoinPredictionSignal {
                market: "BTC above $120k by quarter-end".to_string(),
                probability: Some(38.0),
                delta_7d: Some(5.0),
                relevance: Some("upside skew improved".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_bitcoin(&ctx).unwrap();

        assert!(rendered.contains("ETF flows:"));
        assert!(rendered.contains("| Last session | +$420m | broad issuer inflow | fresh 1d |"));
        assert!(rendered.contains("On-chain and exchange-reserve context:"));
        assert!(rendered.contains("| Exchange reserves | 2.1m BTC | reserve drawdown supports supply-tightness read | fresh 2d |"));
        assert!(rendered
            .contains("| MACRO | Monetary hedge | liquidity regime is the key constraint |"));
        assert!(rendered.contains("News catalysts:"));
        assert!(rendered.contains("Prediction-market signals:"));
        assert!(rendered
            .contains("| BTC above $120k by quarter-end | 38% | +5pp | upside skew improved |"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_bitcoin_degrades_with_missing_context_and_partial_layers() {
        let ctx = BuildContext {
            bitcoin_analyst_views: vec![view("LOW", "BTC spot", "intraday structure is mixed")],
            bitcoin_prediction_signals: vec![BitcoinPredictionSignal {
                market: "BTC volatility spike".to_string(),
                probability: None,
                delta_7d: None,
                relevance: None,
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_bitcoin(&ctx).unwrap();

        assert!(rendered.contains("BTC price context is unavailable"));
        assert!(rendered.contains("Missing BTC analyst layers: MEDIUM, HIGH, MACRO"));
        assert!(rendered.contains("| BTC volatility spike | n/a | n/a | n/a |"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_bitcoin_escapes_table_pipes() {
        let ctx = BuildContext {
            bitcoin_etf_flows: vec![BitcoinEtfFlowSummary {
                period: "Spot|ETF".to_string(),
                net_flow: Some("+$1|000m".to_string()),
                detail: Some("issuer|wide".to_string()),
                freshness: None,
            }],
            bitcoin_analyst_views: vec![view("LOW", "BTC|USD", "range|bound")],
            bitcoin_prediction_signals: vec![BitcoinPredictionSignal {
                market: "BTC|market".to_string(),
                probability: Some(50.0),
                delta_7d: Some(0.0),
                relevance: Some("signal|test".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_bitcoin(&ctx).unwrap();

        assert!(rendered.contains("Spot/ETF"));
        assert!(rendered.contains("+$1/000m"));
        assert!(rendered.contains("BTC/USD"));
        assert!(rendered.contains("BTC/market"));
        assert!(rendered.contains("signal/test"));
        assert_public_safe(&rendered);
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
            "my btc",
            "my bitcoin",
            "cost basis",
            "unrealized",
            "transaction",
            "allocation percentage",
            "position size",
            "my portfolio",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public bitcoin leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
