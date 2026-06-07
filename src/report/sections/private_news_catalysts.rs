#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::{
    BuildContext, NewsVolumeSignal, PrivateMacroScenarioRow, PrivateNewsCatalyst,
    PrivatePositionSnapshotRow,
};

pub const SECTION_PRIVACY: &str = "private";

const HELD_ASSET_THRESHOLD_PCT: f64 = 1.0;
const MAX_EVENT_BLOCKS: usize = 5;
const MIN_EVENT_BLOCKS: usize = 3;
const MAX_SILENCE_ROWS: usize = 5;

pub fn render_private_news_catalysts(ctx: &BuildContext) -> Result<String> {
    let held = held_symbols(&ctx.private_positions);
    let scenarios = active_scenarios(&ctx.private_macro_scenarios);
    let connected = filter_connected(&ctx.private_news_events, &held, &scenarios);
    let silence_rows = &ctx.private_news_silence;

    // Suppress the entire section when neither connected events nor
    // silence signals landed. Empty-state disclosures bloat the PDF
    // without telling the operator anything actionable.
    if connected.is_empty() && silence_rows.is_empty() {
        return Ok(String::new());
    }

    let mut body = String::new();
    if !connected.is_empty() {
        body.push_str(&render_events(&connected));
        body.push_str("\n\n");
    }
    if !silence_rows.is_empty() {
        let silence = render_silence_signals(silence_rows);
        if !silence.is_empty() {
            body.push_str(&silence);
        }
    }

    // Second-chance suppression: if no actionable sub-block produced any
    // content (e.g. all silence rows hit insufficient-baseline), drop the
    // section entirely instead of emitting a bare heading.
    if body.trim().is_empty() {
        return Ok(String::new());
    }

    let mut output = String::from("## News & Catalysts\n\n");
    output.push_str(&body);
    Ok(output.trim_end().to_string())
}

fn held_symbols(rows: &[PrivatePositionSnapshotRow]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.allocation_pct >= HELD_ASSET_THRESHOLD_PCT)
        .map(|row| row.symbol.clone())
        .collect()
}

fn active_scenarios(rows: &[PrivateMacroScenarioRow]) -> Vec<String> {
    rows.iter().map(|row| row.name.clone()).collect()
}

fn filter_connected<'a>(
    events: &'a [PrivateNewsCatalyst],
    held: &[String],
    scenarios: &[String],
) -> Vec<&'a PrivateNewsCatalyst> {
    let mut connected: Vec<&PrivateNewsCatalyst> = events
        .iter()
        .filter(|event| event_connects(event, held, scenarios))
        .collect();
    connected.sort_by(|left, right| {
        right
            .impact_score
            .total_cmp(&left.impact_score)
            .then_with(|| left.headline.cmp(&right.headline))
    });
    connected
}

fn event_connects(
    event: &PrivateNewsCatalyst,
    held: &[String],
    scenarios: &[String],
) -> bool {
    let asset_hit = event
        .related_assets
        .iter()
        .any(|asset| held.iter().any(|symbol| symbol.eq_ignore_ascii_case(asset)));
    let scenario_hit = event.related_scenarios.iter().any(|scenario| {
        scenarios
            .iter()
            .any(|active| active.eq_ignore_ascii_case(scenario))
    });
    asset_hit || scenario_hit
}

fn render_events(events: &[&PrivateNewsCatalyst]) -> String {
    if events.is_empty() {
        return "No last-24h news events connect to held assets above 1% or active scenarios. This does not prove catalyst absence; it only means no connected sourceable rows were attached to this private build.".to_string();
    }

    events
        .iter()
        .take(MAX_EVENT_BLOCKS)
        .enumerate()
        .map(|(index, event)| render_event_block(index + 1, event))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_event_block(rank: usize, event: &PrivateNewsCatalyst) -> String {
    let mut block = format!("{}. **{}**\n\n", rank, sentence_fragment(&event.headline));
    block.push_str(&format!(
        "- What happened: {}\n",
        sentence(event.what_happened.as_deref().unwrap_or(
            "No what-happened summary is attached; treat as descriptive only"
        ))
    ));
    block.push_str(&format!(
        "- Where the money moved: {}\n",
        sentence(event.money_moved.as_deref().unwrap_or(
            "No price-action read is attached; do not infer flow direction"
        ))
    ));
    block.push_str(&format!(
        "- Who benefits: {}\n",
        sentence(event.who_benefits.as_deref().unwrap_or(
            "Beneficiary attribution is not attached"
        ))
    ));
    block.push_str(&format!(
        "- What it means: {}\n",
        sentence(event.what_it_means.as_deref().unwrap_or(
            "Operator implication is not attached; no portfolio action is recommended on this row"
        ))
    ));
    block.push_str(&format!(
        "\n*Source: {} ({}{}, {}) | Topic: {} | Held assets: {} | Scenarios: {}*",
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
        join_or_none(&event.related_assets),
        join_or_none(&event.related_scenarios),
    ));
    block
}

fn render_silence_signals(signals: &[NewsVolumeSignal]) -> String {
    let usable: Vec<&NewsVolumeSignal> = signals
        .iter()
        .filter(|signal| !is_insufficient_baseline(signal))
        .collect();
    if usable.is_empty() {
        // Return empty so the parent suppresses the section entirely
        // when only the "unavailable" disclaimer would have rendered.
        // The 2026-06-07 weekly emitted the bare disclaimer as the
        // ONLY content of the News & Catalysts section — wasted page.
        return String::new();
    }

    let mut table = String::from(
        "News-volume context:\n\n| Topic | 24h Count | Baseline | Status | Caveat |\n|---|---:|---:|---|---|\n",
    );
    for signal in usable.iter().take(MAX_SILENCE_ROWS) {
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

fn is_insufficient_baseline(signal: &NewsVolumeSignal) -> bool {
    let status = signal.status.to_ascii_lowercase();
    let caveat_insufficient = signal
        .caveat
        .as_deref()
        .map(|caveat| caveat.to_ascii_lowercase().contains("insufficient"))
        .unwrap_or(false);
    status.contains("insufficient")
        || status == "no-baseline"
        || signal.baseline_count.is_none()
        || caveat_insufficient
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

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values
            .iter()
            .map(|value| clean_text(value))
            .collect::<Vec<_>>()
            .join(", ")
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
    value.replace('|', "/").trim().to_string()
}

fn clean_cell(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

// Re-export so other tooling can know the intended minimum.
pub const _MIN_EVENT_BLOCKS_HINT: usize = MIN_EVENT_BLOCKS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_news_catalysts_renders_connected_events_with_metadata_line() {
        let rendered = render_private_news_catalysts(&fixture_context()).unwrap();

        assert!(rendered.starts_with("## News & Catalysts\n\n"));
        assert!(rendered.contains("1. **Fed shifts cut path**"));
        assert!(rendered.contains("- What happened: The dot plot moved hawkish."));
        assert!(rendered.contains("- Where the money moved: Front-end rates jumped 12bps; SPY gave back 0.8%."));
        assert!(rendered.contains("- Who benefits: USD bulls and short-duration credit."));
        assert!(rendered.contains("- What it means: A reset of cut odds tightens risk appetite into the next CPI print."));
        assert!(rendered.contains(
            "*Source: centralbank.test (Tier 1, primary-source) | Topic: fed-policy | Held assets: SPY | Scenarios: Hard Landing*"
        ));
    }

    #[test]
    fn private_news_catalysts_filters_out_events_with_no_connection() {
        let rendered = render_private_news_catalysts(&fixture_context()).unwrap();

        // Unrelated event has impact_score 0.99 (highest) but should be filtered.
        assert!(!rendered.contains("Crypto miner hardware launch"));
    }

    #[test]
    fn private_news_catalysts_connects_via_scenario_when_no_asset_matches() {
        let ctx = BuildContext {
            private_positions: vec![position("BTC", 30.0)],
            private_macro_scenarios: vec![scenario("Hard Landing", 35.0, 30.0)],
            private_news_events: vec![event(
                "Recession indicator flips",
                Some("Sahm rule triggered"),
                Some("Yield curve un-inverted with weak claims"),
                Some("Treasury duration and gold"),
                Some("Hard Landing odds reprice higher"),
                "macro.test",
                Some(1),
                Some("primary-source"),
                Some("macro"),
                vec![],
                vec!["Hard Landing".to_string()],
                0.88,
            )],
            ..BuildContext::default()
        };

        let rendered = render_private_news_catalysts(&ctx).unwrap();

        assert!(rendered.contains("Recession indicator flips"));
        assert!(rendered.contains("Held assets: none | Scenarios: Hard Landing"));
    }

    #[test]
    fn private_news_catalysts_requires_metadata_line_on_every_block() {
        let rendered = render_private_news_catalysts(&fixture_context()).unwrap();
        let block_count = rendered.matches("\n. **").count() + rendered.matches("1. **").count();
        assert!(block_count >= 1, "at least one block expected: {rendered}");

        // Every rendered numbered block must end with the metadata line.
        let metadata_lines = rendered.matches("*Source: ").count();
        let header_lines = rendered.matches(". **").count();
        assert_eq!(
            metadata_lines, header_lines,
            "metadata line must be present for every event block: {rendered}"
        );
    }

    #[test]
    fn private_news_catalysts_skips_insufficient_baseline_silence_rows() {
        let ctx = BuildContext {
            private_positions: vec![position("SPY", 40.0)],
            private_macro_scenarios: vec![scenario("Hard Landing", 35.0, 30.0)],
            private_news_events: vec![event(
                "Fed shifts cut path",
                Some("The dot plot moved hawkish"),
                Some("Front-end rates jumped"),
                Some("USD bulls"),
                Some("Reset of cut odds tightens risk"),
                "centralbank.test",
                Some(1),
                Some("primary-source"),
                Some("fed-policy"),
                vec!["SPY".to_string()],
                vec!["Hard Landing".to_string()],
                0.94,
            )],
            private_news_silence: vec![
                silence(
                    "china-growth",
                    1,
                    Some(6.5),
                    "silent",
                    Some("baseline mature"),
                ),
                silence(
                    "new-topic-x",
                    0,
                    Some(0.2),
                    "insufficient-baseline",
                    Some("only 2 samples"),
                ),
                silence(
                    "another-new",
                    0,
                    None,
                    "no-baseline",
                    Some("first observation"),
                ),
                silence(
                    "still-warming",
                    1,
                    Some(1.1),
                    "normal",
                    Some("baseline insufficient for inference"),
                ),
            ],
            ..BuildContext::default()
        };

        let rendered = render_private_news_catalysts(&ctx).unwrap();

        assert!(rendered.contains("| china-growth | 1 | 6.5 | silent | baseline mature |"));
        assert!(!rendered.contains("new-topic-x"));
        assert!(!rendered.contains("another-new"));
        assert!(!rendered.contains("still-warming"));
    }

    #[test]
    fn private_news_catalysts_empty_events_suppresses_section() {
        // Empty news events + empty silence => suppress entire section.
        let ctx = BuildContext {
            private_positions: vec![position("SPY", 40.0)],
            private_macro_scenarios: vec![scenario("Hard Landing", 35.0, 30.0)],
            private_news_events: vec![],
            private_news_silence: vec![],
            ..BuildContext::default()
        };

        let rendered = render_private_news_catalysts(&ctx).unwrap();
        assert!(rendered.is_empty());
    }

    #[test]
    fn private_news_catalysts_caps_at_five_blocks() {
        let mut events = Vec::new();
        for idx in 0..7 {
            events.push(event(
                &format!("Held event {idx}"),
                Some("happened"),
                Some("moved"),
                Some("benefits"),
                Some("means"),
                "example.test",
                Some(2),
                Some("independent"),
                Some("topic"),
                vec!["SPY".to_string()],
                vec![],
                idx as f64,
            ));
        }
        let ctx = BuildContext {
            private_positions: vec![position("SPY", 40.0)],
            private_news_events: events,
            ..BuildContext::default()
        };

        let rendered = render_private_news_catalysts(&ctx).unwrap();

        assert!(rendered.contains("1. **Held event 6**"));
        assert!(rendered.contains("5. **Held event 2**"));
        assert!(!rendered.contains("Held event 1"));
        assert!(!rendered.contains("Held event 0"));
    }

    #[test]
    fn private_news_catalysts_section_is_private_only() {
        assert_eq!(SECTION_PRIVACY, "private");
    }

    fn fixture_context() -> BuildContext {
        BuildContext {
            private_positions: vec![
                position("SPY", 40.0),
                position("BTC", 15.0),
                position("GLD", 0.4),
            ],
            private_macro_scenarios: vec![
                scenario("Hard Landing", 35.0, 30.0),
                scenario("Soft Landing", 30.0, 35.0),
            ],
            private_news_events: vec![
                event(
                    "Fed shifts cut path",
                    Some("The dot plot moved hawkish"),
                    Some("Front-end rates jumped 12bps; SPY gave back 0.8%"),
                    Some("USD bulls and short-duration credit"),
                    Some("A reset of cut odds tightens risk appetite into the next CPI print"),
                    "centralbank.test",
                    Some(1),
                    Some("primary-source"),
                    Some("fed-policy"),
                    vec!["SPY".to_string()],
                    vec!["Hard Landing".to_string()],
                    0.94,
                ),
                event(
                    "ETF inflows accelerate",
                    Some("Spot BTC ETFs added $480M in 24h"),
                    Some("BTC pushed through prior resistance"),
                    Some("Mid-cap miners and prime-broker desks"),
                    Some("Sustained ETF bid extends the secular allocation thesis"),
                    "etfwire.test",
                    Some(2),
                    Some("wire"),
                    Some("crypto-flows"),
                    vec!["BTC".to_string()],
                    vec![],
                    0.81,
                ),
                event(
                    "Crypto miner hardware launch",
                    Some("New ASIC released by a private vendor"),
                    Some("No measurable spot impact"),
                    Some("Hardware OEM"),
                    Some("Not portfolio-relevant"),
                    "unrelated.test",
                    Some(3),
                    Some("vendor"),
                    Some("crypto-hardware"),
                    vec!["UNHELD".to_string()],
                    vec!["Unrelated Scenario".to_string()],
                    0.99,
                ),
            ],
            private_news_silence: vec![silence(
                "china-growth",
                1,
                Some(6.5),
                "silent",
                Some("baseline mature"),
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

    fn scenario(name: &str, probability: f64, prior_7d: f64) -> PrivateMacroScenarioRow {
        PrivateMacroScenarioRow {
            name: name.to_string(),
            probability,
            prior_7d,
        }
    }

    fn silence(
        topic: &str,
        current_count: u32,
        baseline_count: Option<f64>,
        status: &str,
        caveat: Option<&str>,
    ) -> NewsVolumeSignal {
        NewsVolumeSignal {
            topic: topic.to_string(),
            current_count,
            baseline_count,
            status: status.to_string(),
            caveat: caveat.map(str::to_string),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn event(
        headline: &str,
        what_happened: Option<&str>,
        money_moved: Option<&str>,
        who_benefits: Option<&str>,
        what_it_means: Option<&str>,
        domain: &str,
        source_tier: Option<u8>,
        independence: Option<&str>,
        topic: Option<&str>,
        related_assets: Vec<String>,
        related_scenarios: Vec<String>,
        impact_score: f64,
    ) -> PrivateNewsCatalyst {
        PrivateNewsCatalyst {
            headline: headline.to_string(),
            what_happened: what_happened.map(str::to_string),
            money_moved: money_moved.map(str::to_string),
            who_benefits: who_benefits.map(str::to_string),
            what_it_means: what_it_means.map(str::to_string),
            domain: domain.to_string(),
            source_tier,
            independence: independence.map(str::to_string),
            topic: topic.map(str::to_string),
            related_assets,
            related_scenarios,
            impact_score,
        }
    }
}
