#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, EconomicCalendarEvent, NewsVolumeSignal, PublicNewsEvent,
};

pub fn render_public_news_catalysts(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## News & Catalysts\n\n");

    output.push_str(&render_events(&ctx.public_news_events));
    output.push_str("\n\n");
    output.push_str(&render_silence_signals(&ctx.public_news_silence));
    output.push_str("\n\n### Tomorrow's Calendar\n\n");
    output.push_str(&render_tomorrows_calendar(&ctx.economic_calendar));

    Ok(output.trim_end().to_string())
}

fn render_events(events: &[PublicNewsEvent]) -> String {
    if events.is_empty() {
        return "No ranked public news events are attached to this build. This does not prove catalyst absence; it only means no sourceable last-24h event rows were loaded.".to_string();
    }

    let mut ranked = events.to_vec();
    ranked.sort_by(|left, right| {
        right
            .impact_score
            .total_cmp(&left.impact_score)
            .then_with(|| left.headline.cmp(&right.headline))
    });

    ranked
        .iter()
        .take(5)
        .enumerate()
        .map(|(index, event)| {
            let mut block = format!(
                "{}. **{}**\n\n{}",
                index + 1,
                sentence_fragment(&event.headline),
                sentence(event.summary.as_deref().unwrap_or(
                    "No event summary is attached, so the market read stays descriptive"
                ))
            );
            block.push_str(&format!(
                "\n\n*Source: {} ({}{}, {}) | Topic: {} | Bound market: {}*",
                clean_text(&event.domain),
                tier_label(event.source_tier),
                inferred_label(event.source_tier),
                clean_text(
                    event
                        .independence
                        .as_deref()
                        .unwrap_or("independence unknown")
                ),
                clean_text(event.topic.as_deref().unwrap_or("unclassified")),
                clean_text(event.bound_market.as_deref().unwrap_or("none")),
            ));
            block
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_silence_signals(signals: &[NewsVolumeSignal]) -> String {
    if signals.is_empty() {
        return "News-silence analytics are unavailable for this build. Do not infer whether a quiet topic is meaningful without a refreshed baseline.".to_string();
    }

    let mut table = String::from(
        "News-volume context:\n\n| Topic | 24h Count | Baseline | Status | Caveat |\n|---|---:|---:|---|---|\n",
    );
    for signal in signals.iter().take(5) {
        table.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            clean_cell(&signal.topic),
            signal.current_count,
            format_baseline(signal.baseline_count),
            clean_cell(&signal.status),
            clean_cell(signal.caveat.as_deref().unwrap_or("n/a")),
        ));
    }
    table.trim_end().to_string()
}

fn render_tomorrows_calendar(events: &[EconomicCalendarEvent]) -> String {
    if events.is_empty() {
        return "No economic-calendar rows are attached to this build.".to_string();
    }

    events
        .iter()
        .take(7)
        .map(|event| {
            format!(
                "- {}: {}{}{}.",
                clean_text(&event.date),
                sentence_fragment(&event.event),
                optional_label("Importance", event.importance.as_deref()),
                optional_label("Relevance", event.market_relevance.as_deref()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn optional_label(label: &str, value: Option<&str>) -> String {
    value
        .map(|value| format!(" | {label}: {}", sentence_fragment(value)))
        .unwrap_or_default()
}

fn tier_label(tier: Option<u8>) -> String {
    tier.map(|tier| format!("Tier {tier}"))
        .unwrap_or_else(|| "Tier unknown".to_string())
}

fn inferred_label(tier: Option<u8>) -> &'static str {
    if tier.is_some() {
        ""
    } else {
        ", inferred provisionally"
    }
}

fn format_baseline(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.1}"))
        .unwrap_or_else(|| "n/a".to_string())
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
    fn public_news_catalysts_ranks_top_events_and_includes_metadata() {
        let ctx = BuildContext {
            public_news_events: vec![
                event(
                    "Lower-ranked earnings recap",
                    Some("Earnings guidance was mixed across cyclicals"),
                    0.62,
                )
                .with_metadata("example.com", Some(2), Some("independent"))
                .with_topic(Some("earnings"), Some("SPY")),
                event(
                    "Fed speaker resets rate-cut expectations",
                    Some("The remarks moved front-end rates and tightened risk appetite"),
                    0.94,
                )
                .with_metadata("centralbank.test", Some(1), Some("primary-source"))
                .with_topic(Some("fed-policy"), Some("Fed funds December cut")),
                event(
                    "Shipping disruption raises energy risk",
                    Some("Freight and crude markets repriced the geopolitical tail"),
                    0.81,
                )
                .with_metadata("unknown-source.test", None, None)
                .with_topic(Some("geopolitics"), None),
            ],
            public_news_silence: vec![NewsVolumeSignal {
                topic: "china-growth".to_string(),
                current_count: 1,
                baseline_count: Some(6.5),
                status: "silent".to_string(),
                caveat: Some("baseline mature".to_string()),
            }],
            economic_calendar: vec![EconomicCalendarEvent {
                date: "2026-06-02".to_string(),
                event: "JOLTS job openings".to_string(),
                importance: Some("high".to_string()),
                market_relevance: Some("labor cooling confirmation".to_string()),
            }],
            ..BuildContext::default()
        };

        let rendered = render_public_news_catalysts(&ctx).unwrap();

        assert!(rendered.starts_with("## News & Catalysts\n\n"));
        let fed_pos = rendered.find("Fed speaker resets").unwrap();
        let shipping_pos = rendered.find("Shipping disruption").unwrap();
        let earnings_pos = rendered.find("Lower-ranked earnings").unwrap();
        assert!(fed_pos < shipping_pos);
        assert!(shipping_pos < earnings_pos);
        assert!(rendered.contains(
            "*Source: centralbank.test (Tier 1, primary-source) | Topic: fed-policy | Bound market: Fed funds December cut*"
        ));
        assert!(rendered.contains(
            "*Source: unknown-source.test (Tier unknown, inferred provisionally, independence unknown) | Topic: geopolitics | Bound market: none*"
        ));
        assert!(rendered.contains("| china-growth | 1 | 6.5 | silent | baseline mature |"));
        assert!(rendered.contains("### Tomorrow's Calendar"));
        assert!(rendered.contains("- 2026-06-02: JOLTS job openings | Importance: high | Relevance: labor cooling confirmation."));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_news_catalysts_requires_metadata_on_every_event_block() {
        let ctx = BuildContext {
            public_news_events: vec![event("Topic classifier binds inflation story", None, 0.7)
                .with_metadata("macro.test", Some(3), Some("wire"))
                .with_topic(Some("inflation"), Some("CPI above consensus"))],
            ..BuildContext::default()
        };

        let rendered = render_public_news_catalysts(&ctx).unwrap();

        assert!(rendered.contains("*Source: macro.test (Tier 3, wire) | Topic: inflation | Bound market: CPI above consensus*"));
        assert!(rendered.contains("No event summary is attached"));
        assert!(rendered.contains("News-silence analytics are unavailable"));
        assert!(rendered.contains("No economic-calendar rows are attached"));
        assert_public_safe(&rendered);
    }

    #[test]
    fn public_news_catalysts_limits_event_blocks_to_five() {
        let ctx = BuildContext {
            public_news_events: (0..7)
                .map(|idx| {
                    event(&format!("Event {idx}"), Some("Summary"), idx as f64)
                        .with_metadata("example.com", Some(2), Some("independent"))
                        .with_topic(Some("topic"), Some("market"))
                })
                .collect(),
            ..BuildContext::default()
        };

        let rendered = render_public_news_catalysts(&ctx).unwrap();

        assert!(rendered.contains("1. **Event 6**"));
        assert!(rendered.contains("5. **Event 2**"));
        assert!(!rendered.contains("Event 1"));
        assert!(!rendered.contains("Event 0"));
        assert_public_safe(&rendered);
    }

    fn event(headline: &str, summary: Option<&str>, impact_score: f64) -> PublicNewsEvent {
        PublicNewsEvent {
            headline: headline.to_string(),
            summary: summary.map(str::to_string),
            domain: "example.com".to_string(),
            source_tier: Some(2),
            independence: Some("independent".to_string()),
            topic: None,
            bound_market: None,
            impact_score,
        }
    }

    trait PublicNewsEventFixture {
        fn with_metadata(
            self,
            domain: &str,
            source_tier: Option<u8>,
            independence: Option<&str>,
        ) -> Self;
        fn with_topic(self, topic: Option<&str>, bound_market: Option<&str>) -> Self;
    }

    impl PublicNewsEventFixture for PublicNewsEvent {
        fn with_metadata(
            mut self,
            domain: &str,
            source_tier: Option<u8>,
            independence: Option<&str>,
        ) -> Self {
            self.domain = domain.to_string();
            self.source_tier = source_tier;
            self.independence = independence.map(str::to_string);
            self
        }

        fn with_topic(mut self, topic: Option<&str>, bound_market: Option<&str>) -> Self {
            self.topic = topic.map(str::to_string);
            self.bound_market = bound_market.map(str::to_string);
            self
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
                "public news leaked private phrase {forbidden}: {markdown}"
            );
        }
    }
}
