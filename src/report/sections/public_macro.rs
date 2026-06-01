#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    AnalystViewSummary, BuildContext, EconomicCalendarEvent, NewsVolumeSignal,
};

pub fn render_public_macro(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Macro\n\n");

    if let Some(callout) = render_news_volume_callout(&ctx.macro_news_volume) {
        output.push_str(&callout);
        output.push_str("\n\n");
    }

    output.push_str("### Current State\n\n");
    output.push_str(&render_current_state(ctx));
    output.push_str("\n\n### Multi-Timeframe View\n\n");
    output.push_str(&render_multi_timeframe_view(&ctx.macro_analyst_views));
    output.push_str("\n\n### What to Watch\n\n");
    output.push_str(&render_what_to_watch(&ctx.economic_calendar));

    Ok(output.trim_end().to_string())
}

fn render_current_state(ctx: &BuildContext) -> String {
    let mut parts = Vec::new();

    if let Some(regime) = &ctx.regime {
        let detail = regime
            .detail
            .as_deref()
            .map(sentence)
            .unwrap_or_else(|| {
                "No source detail is attached to the cached regime row; treat the classification as provisional.".to_string()
            });
        parts.push(format!(
            "Regime state: {}. {}",
            readable(&regime.classification),
            detail
        ));
    } else {
        parts.push("Regime state is unavailable in the build context. Run the macro refresh and regime classifier before relying on this section.".to_string());
    }

    if ctx.macro_indicators.is_empty() {
        parts.push("Macro indicator rows are unavailable, so this section avoids inferring growth, inflation, dollar, or rates direction from incomplete inputs.".to_string());
    } else {
        let mut table =
            String::from("| Indicator | Latest | Direction | Freshness |\n|---|---:|---|---|\n");
        for indicator in &ctx.macro_indicators {
            table.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                clean_cell(&indicator.name),
                clean_cell(indicator.value.as_deref().unwrap_or("n/a")),
                clean_cell(indicator.trend.as_deref().unwrap_or("n/a")),
                clean_cell(
                    indicator
                        .freshness
                        .as_deref()
                        .unwrap_or("freshness unavailable")
                ),
            ));
        }
        parts.push(table.trim_end().to_string());
    }

    parts.join("\n\n")
}

fn render_multi_timeframe_view(views: &[AnalystViewSummary]) -> String {
    if views.is_empty() {
        return "LOW, MEDIUM, HIGH, and MACRO analyst rows are not available for the macro section. Keep the macro read provisional until each layer has a current, sourceable view.".to_string();
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
            "{}\n\nMissing analyst layers: {}. Cross-timeframe claims should stay qualified until those rows exist.",
            table.trim_end(),
            missing_layers.join(", ")
        )
    }
}

fn render_what_to_watch(events: &[EconomicCalendarEvent]) -> String {
    if events.is_empty() {
        return "No economic calendar rows are attached to this report build. The daily report should avoid claiming that no catalysts exist; it only knows that no sourced calendar rows were loaded.".to_string();
    }

    let mut table =
        String::from("| Date | Event | Importance | Market Relevance |\n|---|---|---|---|\n");
    for event in events {
        table.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            clean_cell(&event.date),
            clean_cell(&event.event),
            clean_cell(event.importance.as_deref().unwrap_or("n/a")),
            clean_cell(event.market_relevance.as_deref().unwrap_or("n/a")),
        ));
    }

    table.trim_end().to_string()
}

fn render_news_volume_callout(rows: &[NewsVolumeSignal]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }

    let summary = rows
        .iter()
        .take(3)
        .map(|row| {
            let baseline = row
                .baseline_count
                .map(|value| format!("{value:.1} baseline"))
                .unwrap_or_else(|| "baseline unavailable".to_string());
            let caveat = row
                .caveat
                .as_deref()
                .map(|value| format!(" ({})", sentence_fragment(value)))
                .unwrap_or_default();
            format!(
                "{}: {} current vs {} — {}{}",
                clean_cell(&row.topic),
                row.current_count,
                baseline,
                clean_cell(&row.status),
                caveat
            )
        })
        .collect::<Vec<_>>()
        .join("; ");

    Some(format!("News volume vs baseline: {summary}."))
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

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::{MacroIndicatorSummary, RegimeSummary, SynthesisSnapshot};

    #[test]
    fn public_macro_renders_fixture_subsections() {
        let ctx = BuildContext {
            synthesis: Some(SynthesisSnapshot {
                summary: "Growth is cooling while inflation expectations remain sticky".to_string(),
                central_tension: None,
            }),
            regime: Some(RegimeSummary {
                classification: "stagflation_watch".to_string(),
                detail: Some(
                    "Rates and dollar strength are the dominant macro constraints".to_string(),
                ),
            }),
            macro_indicators: vec![
                indicator("DXY", Some("104.2"), Some("rising"), Some("fresh 1d")),
                indicator("10Y yield", Some("4.28%"), Some("flat"), Some("fresh 1d")),
                indicator("CPI YoY", Some("3.4%"), Some("sticky"), Some("stale 12d")),
            ],
            macro_analyst_views: vec![
                view(
                    "LOW",
                    "Rates",
                    "front-end repricing is pressuring risk appetite",
                ),
                view(
                    "MEDIUM",
                    "Dollar",
                    "dollar strength is the main cross-asset headwind",
                ),
                view(
                    "HIGH",
                    "Growth",
                    "earnings sensitivity rises if PMIs keep fading",
                ),
                view(
                    "MACRO",
                    "Inflation",
                    "structural inflation risks remain two-sided",
                ),
            ],
            economic_calendar: vec![
                event(
                    "2026-06-02",
                    "JOLTS",
                    Some("medium"),
                    Some("labor-market confirmation"),
                ),
                event(
                    "2026-06-05",
                    "Nonfarm payrolls",
                    Some("high"),
                    Some("rates and dollar catalyst"),
                ),
            ],
            macro_news_volume: vec![NewsVolumeSignal {
                topic: "fed-policy".to_string(),
                current_count: 9,
                baseline_count: Some(4.5),
                status: "saturated".to_string(),
                caveat: Some("wire-heavy sample".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_macro(&ctx).unwrap();

        assert!(rendered.starts_with("## Macro\n\n"));
        assert!(rendered.contains("News volume vs baseline: fed-policy: 9 current vs 4.5 baseline"));
        assert!(rendered.contains("### Current State"));
        assert!(rendered.contains("Regime state: stagflation watch"));
        assert!(rendered.contains("| DXY | 104.2 | rising | fresh 1d |"));
        assert!(rendered.contains("| CPI YoY | 3.4% | sticky | stale 12d |"));
        assert!(rendered.contains("### Multi-Timeframe View"));
        assert!(rendered
            .contains("| MACRO | Inflation | structural inflation risks remain two-sided |"));
        assert!(rendered.contains("### What to Watch"));
        assert!(rendered
            .contains("| 2026-06-05 | Nonfarm payrolls | high | rates and dollar catalyst |"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_macro_degrades_with_stale_and_missing_data() {
        let ctx = BuildContext {
            macro_indicators: vec![indicator("GDPNow", Some("n/a"), None, None)],
            macro_analyst_views: vec![view("MACRO", "Regime", "long-cycle view is stale")],
            macro_news_volume: vec![NewsVolumeSignal {
                topic: "inflation".to_string(),
                current_count: 0,
                baseline_count: None,
                status: "silent".to_string(),
                caveat: Some("insufficient weekday baseline".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_macro(&ctx).unwrap();

        assert!(rendered.contains("Regime state is unavailable"));
        assert!(rendered.contains("| GDPNow | n/a | n/a | freshness unavailable |"));
        assert!(rendered.contains("Missing analyst layers: LOW, MEDIUM, HIGH"));
        assert!(rendered.contains("No economic calendar rows are attached"));
        assert!(rendered.contains("baseline unavailable"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_macro_escapes_table_pipes_and_stays_sourceable() {
        let ctx = BuildContext {
            macro_indicators: vec![indicator(
                "PMI|ISM",
                Some("49|8"),
                Some("falling|soft"),
                Some("fresh"),
            )],
            macro_analyst_views: vec![view("LOW", "Rates|FX", "claim is tied to cached rows")],
            economic_calendar: vec![event(
                "2026-06-03",
                "Fed|minutes",
                None,
                Some("policy|path"),
            )],
            ..BuildContext::default()
        };

        let rendered = render_public_macro(&ctx).unwrap();

        assert!(rendered.contains("PMI/ISM"));
        assert!(rendered.contains("49/8"));
        assert!(rendered.contains("Rates/FX"));
        assert!(rendered.contains("Fed/minutes"));
        assert!(rendered.contains("policy/path"));
        assert_public_safe(&rendered);
    }

    fn indicator(
        name: &str,
        value: Option<&str>,
        trend: Option<&str>,
        freshness: Option<&str>,
    ) -> MacroIndicatorSummary {
        MacroIndicatorSummary {
            name: name.to_string(),
            value: value.map(ToString::to_string),
            trend: trend.map(ToString::to_string),
            freshness: freshness.map(ToString::to_string),
        }
    }

    fn event(
        date: &str,
        event: &str,
        importance: Option<&str>,
        market_relevance: Option<&str>,
    ) -> EconomicCalendarEvent {
        EconomicCalendarEvent {
            date: date.to_string(),
            event: event.to_string(),
            importance: importance.map(ToString::to_string),
            market_relevance: market_relevance.map(ToString::to_string),
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
            "allocation percentage",
            "position size",
            "my portfolio",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "public macro leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
