#![allow(dead_code)]

use std::collections::BTreeMap;

use anyhow::Result;

use crate::report::build::daily::{
    BinaryCatalystSummary, BuildContext, EconomicCalendarEvent, PrivatePositionSnapshotRow,
};

/// Render the private "Upcoming Calendar" section.
///
/// Combines the attached economic calendar with private binary catalysts
/// (which the private build pipeline uses to land earnings releases and
/// known political/geopolitical dates), groups them by ISO date over the
/// next 3-7 days, and bolds bullets whose event text mentions a held
/// asset's ticker.
pub fn render_private_upcoming_calendar(ctx: &BuildContext) -> Result<String> {
    let mut output = String::from("## Upcoming Calendar\n\n");

    let mut events = collect_events(ctx);
    sort_ascending(&mut events);

    if events.is_empty() {
        output.push_str(EMPTY_LINE);
        return Ok(output.trim_end().to_string());
    }

    let held = held_symbols(&ctx.private_positions);
    let grouped = group_by_date(&events, MAX_DAYS);

    if grouped.is_empty() {
        output.push_str(EMPTY_LINE);
        return Ok(output.trim_end().to_string());
    }

    let mut day_blocks: Vec<String> = Vec::new();
    for (date, day_events) in &grouped {
        let mut block = format!("### {date}\n");
        for event in day_events {
            let line = format_bullet(event, &held);
            block.push_str(&line);
            block.push('\n');
        }
        day_blocks.push(block.trim_end().to_string());
    }

    output.push_str(&day_blocks.join("\n\n"));
    Ok(output.trim_end().to_string())
}

const EMPTY_LINE: &str = "No known catalysts in the next 7 days.";
const MAX_DAYS: usize = 7;

#[derive(Debug, Clone)]
struct CalendarEntry {
    date: String,
    headline: String,
    importance: Option<String>,
    relevance: Option<String>,
    category: &'static str,
}

fn collect_events(ctx: &BuildContext) -> Vec<CalendarEntry> {
    let mut entries: Vec<CalendarEntry> = Vec::new();

    for event in &ctx.economic_calendar {
        entries.push(from_economic(event));
    }
    for catalyst in &ctx.private_binary_catalysts {
        entries.push(from_binary(catalyst));
    }

    // Dedup on (date, normalized event name). Both economic_calendar and
    // private_binary_catalysts derive from the same upcoming-events list
    // and from multi-feed sources with slightly different naming
    // ("Non Farm Payrolls" vs "Non-Farm Payrolls" vs "Nonfarm Payrolls
    // Private"). Without normalization the same release renders 3-4
    // times with conflicting forecast numbers, as it did in the
    // 2026-06-05 weekly run. Preference order keeps the binary-catalyst
    // entry (richer impact label) over the economic-calendar entry when
    // both refer to the same release.
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let mut deduped: Vec<CalendarEntry> = Vec::with_capacity(entries.len());
    entries.sort_by_key(|e| match e.category {
        "binary" => 0,
        _ => 1,
    });
    for entry in entries {
        let key = (entry.date.clone(), canonical_event_key(&entry.headline));
        if seen.insert(key) {
            deduped.push(entry);
        }
    }
    deduped
}

/// Collapse common feed variants onto a single canonical key so
/// "Non Farm Payrolls", "Non-Farm Payrolls", and "Nonfarm Payrolls
/// Private" (all referring to the same monthly release) dedup to one
/// entry. Returns the lowercased, punctuation-stripped, single-spaced
/// head of the event name with known synonym families collapsed.
fn canonical_event_key(headline: &str) -> String {
    let lower = headline
        .to_lowercase()
        .replace(['-', '_'], " ");
    let collapsed: String = lower
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let normalized = collapsed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Family collapses — left side: any matching variant, right side: canonical key.
    const FAMILIES: &[(&str, &str)] = &[
        ("non farm payrolls private", "nfp"),
        ("nonfarm payrolls private", "nfp"),
        ("non farm payrolls", "nfp"),
        ("nonfarm payrolls", "nfp"),
        ("nfp", "nfp"),
        ("average hourly earnings mom", "avg-hourly-earnings-mom"),
        ("average hourly earnings yoy", "avg-hourly-earnings-yoy"),
        ("core cpi yoy", "core-cpi-yoy"),
        ("core cpi mom", "core-cpi-mom"),
        ("cpi yoy", "cpi-yoy"),
        ("cpi mom", "cpi-mom"),
        ("core pce price index", "core-pce"),
        ("core pce", "core-pce"),
        ("pce price index", "pce"),
        ("fomc", "fomc"),
        ("federal funds rate", "fomc"),
        ("interest rate decision", "fomc"),
        ("u 6 unemployment rate", "u6-unemployment"),
        ("u6 unemployment rate", "u6-unemployment"),
        ("unemployment rate", "unemployment-rate"),
    ];
    for (variant, canonical) in FAMILIES {
        if normalized.contains(variant) {
            return (*canonical).to_string();
        }
    }
    normalized
}

fn from_economic(event: &EconomicCalendarEvent) -> CalendarEntry {
    CalendarEntry {
        date: clean_text(&event.date),
        headline: clean_text(&event.event),
        importance: event.importance.as_ref().map(|v| clean_text(v)),
        relevance: event.market_relevance.as_ref().map(|v| clean_text(v)),
        category: "economic",
    }
}

fn from_binary(catalyst: &BinaryCatalystSummary) -> CalendarEntry {
    CalendarEntry {
        date: clean_text(&catalyst.date),
        headline: clean_text(&catalyst.event),
        importance: None,
        relevance: Some(clean_text(&catalyst.impact)),
        category: "binary",
    }
}

fn sort_ascending(events: &mut [CalendarEntry]) {
    events.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.headline.cmp(&b.headline)));
}

fn group_by_date(
    events: &[CalendarEntry],
    max_days: usize,
) -> BTreeMap<String, Vec<CalendarEntry>> {
    let mut grouped: BTreeMap<String, Vec<CalendarEntry>> = BTreeMap::new();
    for event in events {
        if event.date.is_empty() {
            continue;
        }
        grouped
            .entry(event.date.clone())
            .or_default()
            .push(event.clone());
        if grouped.len() > max_days {
            // BTreeMap iteration order is ascending — drop the largest key.
            if let Some(last_key) = grouped.keys().next_back().cloned() {
                grouped.remove(&last_key);
            }
        }
    }
    grouped
}

fn held_symbols(positions: &[PrivatePositionSnapshotRow]) -> Vec<String> {
    let mut symbols: Vec<String> = positions
        .iter()
        .map(|p| p.symbol.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

fn format_bullet(event: &CalendarEntry, held: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(event.headline.clone());
    if let Some(importance) = &event.importance {
        if !importance.is_empty() {
            parts.push(format!("importance: {}", trim_terminator(importance)));
        }
    }
    if let Some(relevance) = &event.relevance {
        if !relevance.is_empty() {
            parts.push(format!("relevance: {}", trim_terminator(relevance)));
        }
    }
    let body = parts.join(" — ");
    let body = sentence(&body);

    if affects_held(event, held) {
        format!("- **{body}**")
    } else {
        format!("- {body}")
    }
}

fn affects_held(event: &CalendarEntry, held: &[String]) -> bool {
    if held.is_empty() {
        return false;
    }
    let haystack = format!(
        "{} {} {}",
        event.headline,
        event.importance.clone().unwrap_or_default(),
        event.relevance.clone().unwrap_or_default()
    )
    .to_ascii_uppercase();
    held.iter().any(|sym| contains_token(&haystack, sym))
}

fn contains_token(haystack: &str, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    let needle = token.as_bytes();
    let mut idx = 0usize;
    while let Some(found) = haystack[idx..].find(token) {
        let abs = idx + found;
        let before_ok = abs == 0 || !is_word_byte(bytes[abs - 1]);
        let end = abs + needle.len();
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        idx = abs + 1;
        if idx >= bytes.len() {
            break;
        }
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn trim_terminator(value: &str) -> String {
    value.trim().trim_end_matches(['.', '!', '?']).to_string()
}

fn sentence(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

fn clean_text(value: &str) -> String {
    value.replace('|', "/").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(date: &str, headline: &str, relevance: Option<&str>) -> EconomicCalendarEvent {
        EconomicCalendarEvent {
            date: date.to_string(),
            event: headline.to_string(),
            importance: Some("high".to_string()),
            market_relevance: relevance.map(|s| s.to_string()),
        }
    }

    fn binary(date: &str, event: &str, impact: &str) -> BinaryCatalystSummary {
        BinaryCatalystSummary {
            date: date.to_string(),
            event: event.to_string(),
            impact: impact.to_string(),
        }
    }

    fn position(symbol: &str) -> PrivatePositionSnapshotRow {
        PrivatePositionSnapshotRow {
            symbol: symbol.to_string(),
            price: None,
            daily_change: None,
            allocation_pct: 0.0,
            unrealized_pnl: None,
        }
    }

    #[test]
    fn upcoming_calendar_sorts_dates_ascending() {
        let ctx = BuildContext {
            economic_calendar: vec![
                ev("2026-06-05", "Payrolls", None),
                ev("2026-06-03", "FOMC decision", None),
                ev("2026-06-04", "ECB minutes", None),
            ],
            ..BuildContext::default()
        };

        let rendered = render_private_upcoming_calendar(&ctx).unwrap();
        let pos_03 = rendered.find("2026-06-03").expect("06-03 missing");
        let pos_04 = rendered.find("2026-06-04").expect("06-04 missing");
        let pos_05 = rendered.find("2026-06-05").expect("06-05 missing");

        assert!(pos_03 < pos_04, "{rendered}");
        assert!(pos_04 < pos_05, "{rendered}");
        assert!(rendered.starts_with("## Upcoming Calendar\n\n"));
    }

    #[test]
    fn upcoming_calendar_bolds_held_asset_relevance() {
        let ctx = BuildContext {
            economic_calendar: vec![
                ev(
                    "2026-06-03",
                    "FOMC decision",
                    Some("Rates path repricing affects QQQ and SPY"),
                ),
                ev("2026-06-04", "ECB minutes", Some("Eurozone rates context")),
            ],
            private_positions: vec![position("QQQ"), position("BTC")],
            ..BuildContext::default()
        };

        let rendered = render_private_upcoming_calendar(&ctx).unwrap();
        assert!(
            rendered.contains("- **FOMC decision"),
            "expected FOMC bullet bolded: {rendered}"
        );
        assert!(
            rendered.contains("- ECB minutes"),
            "expected ECB bullet not bolded: {rendered}"
        );
        assert!(
            !rendered.contains("- **ECB minutes"),
            "ECB minutes should not be bolded: {rendered}"
        );
    }

    #[test]
    fn upcoming_calendar_empty_calendar_emits_no_known_catalysts_line() {
        let ctx = BuildContext::default();
        let rendered = render_private_upcoming_calendar(&ctx).unwrap();

        assert!(rendered.starts_with("## Upcoming Calendar\n\n"));
        assert!(
            rendered.contains("No known catalysts in the next 7 days."),
            "{rendered}"
        );
        assert!(
            !rendered.contains("###"),
            "no day blocks should render on empty: {rendered}"
        );
    }

    #[test]
    fn upcoming_calendar_groups_by_date_and_caps_to_seven_days() {
        let mut events = Vec::new();
        for day in 1..=10 {
            events.push(ev(
                &format!("2026-06-{day:02}"),
                &format!("Event {day}"),
                None,
            ));
        }
        let ctx = BuildContext {
            economic_calendar: events,
            ..BuildContext::default()
        };

        let rendered = render_private_upcoming_calendar(&ctx).unwrap();
        let day_headers = rendered.matches("### ").count();
        assert_eq!(day_headers, 7, "expected exactly 7 day blocks: {rendered}");
        assert!(rendered.contains("### 2026-06-01"));
        assert!(rendered.contains("### 2026-06-07"));
        assert!(!rendered.contains("### 2026-06-08"));
    }

    #[test]
    fn upcoming_calendar_merges_binary_catalysts() {
        let ctx = BuildContext {
            economic_calendar: vec![ev("2026-06-03", "FOMC decision", None)],
            private_binary_catalysts: vec![binary(
                "2026-06-04",
                "AAPL earnings",
                "Mega-cap earnings could move QQQ",
            )],
            private_positions: vec![position("AAPL")],
            ..BuildContext::default()
        };

        let rendered = render_private_upcoming_calendar(&ctx).unwrap();
        assert!(rendered.contains("### 2026-06-03"));
        assert!(rendered.contains("### 2026-06-04"));
        assert!(
            rendered.contains("- **AAPL earnings"),
            "AAPL earnings bullet should be bolded for AAPL holder: {rendered}"
        );
    }

    #[test]
    fn upcoming_calendar_word_boundary_held_match() {
        // Held BTC must not match BITCOIN-ETF substring as if it were a ticker.
        let ctx = BuildContext {
            economic_calendar: vec![ev(
                "2026-06-03",
                "Crypto regulation hearing",
                Some("Could affect crypto sentiment broadly"),
            )],
            private_positions: vec![position("BTC")],
            ..BuildContext::default()
        };
        let rendered = render_private_upcoming_calendar(&ctx).unwrap();
        assert!(
            !rendered.contains("- **Crypto"),
            "no BTC token in headline/relevance should not bold: {rendered}"
        );
    }

    #[test]
    fn upcoming_calendar_is_not_public_mode_content() {
        let ctx = BuildContext {
            economic_calendar: vec![ev("2026-06-03", "FOMC decision", None)],
            ..BuildContext::default()
        };
        let rendered = render_private_upcoming_calendar(&ctx).unwrap();

        assert!(rendered.contains("## Upcoming Calendar"));
        assert!(!rendered.contains("## Executive Summary"));
        assert!(!rendered.contains("## Methodology"));
        assert!(!rendered.contains("for informational purposes only"));
    }
}
