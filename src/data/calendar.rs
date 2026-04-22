//! Economic calendar data source.
//!
//! Scrapes TradingEconomics calendar for upcoming market-moving events.
//! Free, no API key required. Falls back to sample data on scrape failure.

use anyhow::{anyhow, Context, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use scraper::{ElementRef, Html, Selector};
use std::sync::OnceLock;

use crate::data::brave;

/// A calendar event (economic or earnings).
#[derive(Debug, Clone)]
pub struct Event {
    pub date: String, // YYYY-MM-DD
    pub name: String,
    pub impact: String, // "high", "medium", "low"
    pub previous: Option<String>,
    pub forecast: Option<String>,
    pub event_type: String, // "economic" or "earnings"
    pub symbol: Option<String>,
}

/// Fetch upcoming calendar events from TradingEconomics.
/// Returns events from today through `days_ahead` days.
/// Falls back to sample data on scrape failure.
pub fn fetch_events(days_ahead: i64) -> Result<Vec<Event>> {
    match scrape_tradingeconomics_calendar(days_ahead) {
        Ok(events) if !events.is_empty() => Ok(events),
        Ok(_) | Err(_) => {
            // Fallback to sample data
            let today = Utc::now().date_naive();
            let cutoff = today + Duration::days(days_ahead);

            let filtered: Vec<Event> = get_sample_events()
                .into_iter()
                .filter(|e| {
                    if let Ok(event_date) = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") {
                        event_date >= today && event_date <= cutoff
                    } else {
                        false
                    }
                })
                .collect();

            Ok(filtered)
        }
    }
}

/// Enrich calendar with key upcoming macro dates discovered via Brave web search.
pub async fn enrich_with_brave(events: &mut Vec<Event>, brave_key: &str) -> Result<()> {
    let today = Utc::now().date_naive();
    let queries = [
        ("next CPI release date", "Consumer Price Index (CPI)"),
        ("next FOMC meeting date", "FOMC Rate Decision"),
    ];

    for (query, event_name) in queries {
        let results = brave::brave_web_search(brave_key, query, Some("pm"), 5).await?;
        let mut discovered_date = None;
        for r in &results {
            let corpus = format!(
                "{} {} {}",
                r.title,
                r.description,
                r.extra_snippets.join(" ")
            );
            if let Some(d) = extract_date_from_text(&corpus) {
                if d >= today {
                    discovered_date = Some(d);
                    break;
                }
            }
        }

        if let Some(date) = discovered_date {
            let date_str = date.format("%Y-%m-%d").to_string();
            let exists = events.iter().any(|e| {
                e.date == date_str && e.name.to_lowercase().contains(&event_name.to_lowercase())
            });
            if !exists {
                events.push(Event {
                    date: date_str,
                    name: event_name.to_string(),
                    impact: "high".to_string(),
                    previous: None,
                    forecast: None,
                    event_type: "economic".to_string(),
                    symbol: None,
                });
            }
        }
    }

    events.sort_by(|a, b| a.date.cmp(&b.date).then(a.name.cmp(&b.name)));
    Ok(())
}

/// Scrape TradingEconomics calendar page for economic events.
fn scrape_tradingeconomics_calendar(days_ahead: i64) -> Result<Vec<Event>> {
    let today = Utc::now().date_naive();
    let cutoff = today + Duration::days(days_ahead);

    // TradingEconomics calendar page for US events
    let url = "https://tradingeconomics.com/united-states/calendar";

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client
        .get(url)
        .send()
        .context("Failed to fetch TradingEconomics calendar")?;
    let html_content = response.text()?;
    let document = Html::parse_document(&html_content);

    // Cached selectors for calendar table rows/cells.
    static ROW_SELECTOR: OnceLock<Selector> = OnceLock::new();
    static DATE_SELECTOR: OnceLock<Selector> = OnceLock::new();
    static NAME_SELECTOR: OnceLock<Selector> = OnceLock::new();
    static ACTUAL_SELECTOR: OnceLock<Selector> = OnceLock::new();
    static PREVIOUS_SELECTOR: OnceLock<Selector> = OnceLock::new();
    static FORECAST_SELECTOR: OnceLock<Selector> = OnceLock::new();

    let row_selector = cached_selector(&ROW_SELECTOR, "table#calendar tbody tr")?;
    let date_selector = cached_selector(&DATE_SELECTOR, "td:nth-child(1)")?;
    let name_selector = cached_selector(&NAME_SELECTOR, "td:nth-child(3) a.calendar-event")?;
    let actual_selector = cached_selector(&ACTUAL_SELECTOR, "td:nth-child(4)")?;
    let previous_selector = cached_selector(&PREVIOUS_SELECTOR, "td:nth-child(5)")?;
    let forecast_selector = cached_selector(&FORECAST_SELECTOR, "td:nth-child(7)")?;

    let mut events = Vec::new();
    let mut current_date = today;

    for row in document.select(row_selector) {
        current_date = extract_row_date(&row, date_selector, current_date, today.year());

        // Skip if beyond our date range
        if current_date > cutoff {
            break;
        }
        if current_date < today {
            continue;
        }

        if let Some(event) = extract_calendar_event(
            &row,
            current_date,
            name_selector,
            actual_selector,
            previous_selector,
            forecast_selector,
        ) {
            events.push(event);
        }
    }

    Ok(events)
}

fn cached_selector<'a>(slot: &'a OnceLock<Selector>, css: &str) -> Result<&'a Selector> {
    if slot.get().is_none() {
        let parsed =
            Selector::parse(css).map_err(|e| anyhow!("invalid CSS selector '{}': {:?}", css, e))?;
        let _ = slot.set(parsed);
    }
    slot.get()
        .ok_or_else(|| anyhow!("failed to initialize CSS selector '{}'", css))
}

fn extract_row_date(
    row: &ElementRef<'_>,
    date_selector: &Selector,
    current_date: NaiveDate,
    year: i32,
) -> NaiveDate {
    let Some(date_cell) = row.select(date_selector).next() else {
        return current_date;
    };

    let date_text = date_cell.text().collect::<String>().trim().to_string();
    if !date_text.is_empty() && date_text != "Time" {
        if let Ok(parsed) = parse_te_date(&date_text, year) {
            return parsed;
        }
    }

    if let Some(class_attr) = date_cell.value().attr("class") {
        for token in class_attr.split_whitespace() {
            if let Ok(parsed) = NaiveDate::parse_from_str(token, "%Y-%m-%d") {
                return parsed;
            }
        }
    }

    current_date
}

fn extract_calendar_event(
    row: &ElementRef<'_>,
    current_date: NaiveDate,
    name_selector: &Selector,
    actual_selector: &Selector,
    previous_selector: &Selector,
    forecast_selector: &Selector,
) -> Option<Event> {
    let name = extract_cell_text(row, name_selector)?;
    if name.is_empty() {
        return None;
    }

    let _actual = extract_cell_text(row, actual_selector);
    let previous = extract_cell_text(row, previous_selector);
    let forecast = extract_cell_text(row, forecast_selector);
    let impact = classify_impact(&name);

    Some(Event {
        date: current_date.format("%Y-%m-%d").to_string(),
        name,
        impact,
        previous,
        forecast,
        event_type: "economic".into(),
        symbol: None,
    })
}

fn extract_cell_text(row: &ElementRef<'_>, selector: &Selector) -> Option<String> {
    row.select(selector).next().and_then(|e| {
        let text = e.text().collect::<String>().trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    })
}

/// Parse TradingEconomics date format (e.g., "2026-03-05", "Mar 5", etc.)
fn parse_te_date(date_str: &str, year: i32) -> Result<NaiveDate> {
    // Try YYYY-MM-DD first
    if let Ok(d) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        return Ok(d);
    }

    // Try "Mar 5" format
    if let Ok(d) = NaiveDate::parse_from_str(&format!("{} {}", date_str, year), "%b %d %Y") {
        return Ok(d);
    }

    // Try "3/5" format
    if let Ok(d) = NaiveDate::parse_from_str(&format!("{}/{}", date_str, year), "%m/%d/%Y") {
        return Ok(d);
    }

    anyhow::bail!("Failed to parse date: {}", date_str)
}

/// Classify event impact based on event name.
fn classify_impact(name: &str) -> String {
    let name_lower = name.to_lowercase();

    // High impact events
    let high_impact = [
        "fomc",
        "federal funds",
        "interest rate",
        "nonfarm payroll",
        "nfp",
        "unemployment",
        "cpi",
        "inflation",
        "gdp",
        "pce",
        "retail sales",
        "jobless claims",
        "ism",
        "pmi",
        "jolts",
        "adp",
        "consumer confidence",
    ];

    // Medium impact events
    let medium_impact = [
        "housing",
        "durable goods",
        "factory orders",
        "wholesale",
        "trade balance",
        "business inventories",
        "capacity utilization",
    ];

    for keyword in &high_impact {
        if name_lower.contains(keyword) {
            return "high".into();
        }
    }

    for keyword in &medium_impact {
        if name_lower.contains(keyword) {
            return "medium".into();
        }
    }

    "low".into()
}

fn extract_date_from_text(text: &str) -> Option<NaiveDate> {
    // 2026-03-18
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| ",.;:()[]{}".contains(c));
        if let Ok(d) = NaiveDate::parse_from_str(cleaned, "%Y-%m-%d") {
            return Some(d);
        }
    }

    // Month day year, e.g. "March 18, 2026"
    let normalized = text.replace(',', "");
    let words: Vec<String> = normalized
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| ".;:()[]{}".contains(c))
                .to_string()
        })
        .collect();
    for window in words.windows(3) {
        let candidate = format!("{} {} {}", window[0], window[1], window[2]);
        if let Ok(d) = NaiveDate::parse_from_str(&candidate, "%B %d %Y") {
            return Some(d);
        }
        if let Ok(d) = NaiveDate::parse_from_str(&candidate, "%b %d %Y") {
            return Some(d);
        }
    }
    None
}

/// Hardcoded sample calendar events for Mar-Apr 2026.
/// TODO: Replace with Finnhub API integration or similar free source.
fn get_sample_events() -> Vec<Event> {
    vec![
        // ── March 2026 ─────────────────────────────────────────
        Event { date: "2026-03-04".into(), name: "JOLTS Job Openings".into(), impact: "high".into(), previous: Some("7.6M".into()), forecast: Some("7.5M".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-05".into(), name: "ADP Employment Change".into(), impact: "medium".into(), previous: Some("183K".into()), forecast: Some("175K".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-06".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("213K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-07".into(), name: "Non-Farm Payrolls".into(), impact: "high".into(), previous: Some("143K".into()), forecast: Some("160K".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-07".into(), name: "Unemployment Rate".into(), impact: "high".into(), previous: Some("4.0%".into()), forecast: Some("4.0%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-10".into(), name: "Producer Price Index (PPI)".into(), impact: "medium".into(), previous: Some("2.6%".into()), forecast: Some("2.5%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-12".into(), name: "Consumer Price Index (CPI)".into(), impact: "high".into(), previous: Some("3.0%".into()), forecast: Some("2.9%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-18".into(), name: "FOMC Rate Decision".into(), impact: "high".into(), previous: Some("3.50%".into()), forecast: Some("3.50%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-18".into(), name: "FOMC Press Conference".into(), impact: "high".into(), previous: None, forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-19".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("215K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-25".into(), name: "Durable Goods Orders".into(), impact: "medium".into(), previous: Some("2.8%".into()), forecast: Some("1.5%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-26".into(), name: "GDP (Preliminary)".into(), impact: "high".into(), previous: Some("2.3%".into()), forecast: Some("2.5%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-26".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("218K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-03-31".into(), name: "Core PCE Price Index".into(), impact: "high".into(), previous: Some("2.8%".into()), forecast: Some("2.7%".into()), event_type: "economic".into(), symbol: None },
        // ── April 2026 ─────────────────────────────────────────
        Event { date: "2026-04-01".into(), name: "ISM Manufacturing PMI".into(), impact: "high".into(), previous: Some("50.9".into()), forecast: Some("51.2".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-02".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("220K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-02".into(), name: "ADP Employment Change".into(), impact: "medium".into(), previous: Some("175K".into()), forecast: Some("180K".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-03".into(), name: "ISM Services PMI".into(), impact: "high".into(), previous: Some("53.5".into()), forecast: Some("54.0".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-04".into(), name: "Non-Farm Payrolls".into(), impact: "high".into(), previous: Some("160K".into()), forecast: Some("170K".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-04".into(), name: "Unemployment Rate".into(), impact: "high".into(), previous: Some("4.0%".into()), forecast: Some("4.0%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-09".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("220K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-10".into(), name: "Consumer Price Index (CPI)".into(), impact: "high".into(), previous: Some("2.9%".into()), forecast: Some("2.8%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-10".into(), name: "Producer Price Index (PPI)".into(), impact: "medium".into(), previous: Some("2.5%".into()), forecast: Some("2.4%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-15".into(), name: "Retail Sales".into(), impact: "medium".into(), previous: Some("0.4%".into()), forecast: Some("0.5%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-16".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("218K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-22".into(), name: "JOLTS Job Openings".into(), impact: "high".into(), previous: Some("7.5M".into()), forecast: Some("7.4M".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-23".into(), name: "Initial Jobless Claims".into(), impact: "medium".into(), previous: Some("215K".into()), forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-28".into(), name: "Durable Goods Orders".into(), impact: "medium".into(), previous: Some("1.5%".into()), forecast: Some("1.8%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-29".into(), name: "FOMC Rate Decision".into(), impact: "high".into(), previous: Some("3.50%".into()), forecast: Some("3.25%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-29".into(), name: "FOMC Press Conference".into(), impact: "high".into(), previous: None, forecast: None, event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-30".into(), name: "GDP (Advance)".into(), impact: "high".into(), previous: Some("2.5%".into()), forecast: Some("2.2%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-04-30".into(), name: "Core PCE Price Index".into(), impact: "high".into(), previous: Some("2.7%".into()), forecast: Some("2.6%".into()), event_type: "economic".into(), symbol: None },
        // ── May 2026 ───────────────────────────────────────────
        Event { date: "2026-05-01".into(), name: "ISM Manufacturing PMI".into(), impact: "high".into(), previous: Some("51.2".into()), forecast: Some("51.5".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-02".into(), name: "Non-Farm Payrolls".into(), impact: "high".into(), previous: Some("170K".into()), forecast: Some("175K".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-02".into(), name: "Unemployment Rate".into(), impact: "high".into(), previous: Some("4.0%".into()), forecast: Some("3.9%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-05".into(), name: "ISM Services PMI".into(), impact: "high".into(), previous: Some("54.0".into()), forecast: Some("54.2".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-12".into(), name: "Consumer Price Index (CPI)".into(), impact: "high".into(), previous: Some("2.8%".into()), forecast: Some("2.7%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-13".into(), name: "Producer Price Index (PPI)".into(), impact: "medium".into(), previous: Some("2.4%".into()), forecast: Some("2.3%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-16".into(), name: "Consumer Sentiment (UMich)".into(), impact: "medium".into(), previous: Some("79.4".into()), forecast: Some("80.0".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-17".into(), name: "FOMC Rate Decision".into(), impact: "high".into(), previous: Some("3.25%".into()), forecast: Some("3.00%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-28".into(), name: "GDP (Second Estimate)".into(), impact: "high".into(), previous: Some("2.2%".into()), forecast: Some("2.3%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-05-29".into(), name: "Core PCE Price Index".into(), impact: "high".into(), previous: Some("2.6%".into()), forecast: Some("2.5%".into()), event_type: "economic".into(), symbol: None },
        // ── June 2026 ──────────────────────────────────────────
        Event { date: "2026-06-05".into(), name: "Non-Farm Payrolls".into(), impact: "high".into(), previous: Some("175K".into()), forecast: Some("180K".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-06-10".into(), name: "Consumer Price Index (CPI)".into(), impact: "high".into(), previous: Some("2.7%".into()), forecast: Some("2.6%".into()), event_type: "economic".into(), symbol: None },
        Event { date: "2026-06-17".into(), name: "FOMC Rate Decision".into(), impact: "high".into(), previous: Some("3.00%".into()), forecast: Some("3.00%".into()), event_type: "economic".into(), symbol: None },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_events_returns_non_empty() {
        let events = fetch_events(120).unwrap();
        // Sample data has ~45 events in Mar-Jun 2026
        assert!(!events.is_empty(), "Should return sample events");
    }

    #[test]
    fn test_fetch_events_filters_by_days() {
        let events_7d = fetch_events(7).unwrap();
        let events_90d = fetch_events(90).unwrap();
        // 90d should have more events than 7d (unless we're past Apr 2026)
        assert!(
            events_90d.len() >= events_7d.len(),
            "90-day window should include 7-day events"
        );
    }

    #[test]
    fn test_event_structure() {
        let events = fetch_events(30).unwrap();
        if let Some(first) = events.first() {
            assert!(!first.date.is_empty());
            assert!(!first.name.is_empty());
            assert!(first.impact == "high" || first.impact == "medium" || first.impact == "low");
            assert!(first.event_type == "economic" || first.event_type == "earnings");
        }
    }

    #[test]
    fn test_extract_date_from_text() {
        assert_eq!(
            extract_date_from_text("Next FOMC meeting on March 18, 2026."),
            NaiveDate::from_ymd_opt(2026, 3, 18)
        );
        assert_eq!(
            extract_date_from_text("Release date: 2026-04-10"),
            NaiveDate::from_ymd_opt(2026, 4, 10)
        );
    }

    #[test]
    fn scrape_row_uses_event_column_not_numeric_cells() {
        let html = r#"
            <table id="calendar"><tbody>
            <tr data-url="/united-states/jobless-claims">
                <td class="2026-04-23"><span>12:30 PM</span></td>
                <td class="calendar-item">US</td>
                <td style="max-width: 250px; overflow-x: hidden;">
                    <a class="calendar-event" href="/united-states/jobless-claims">Initial Jobless Claims</a>
                    <span class="calendar-reference">APR/18</span>
                </td>
                <td class="calendar-item calendar-item-positive"><span id="actual"></span></td>
                <td class="calendar-item calendar-item-positive"><span id="previous">207K</span></td>
                <td class="calendar-item calendar-item-positive"><a id="consensus">212K</a></td>
                <td class="calendar-item calendar-item-positive"><a id="forecast">218.0K</a></td>
            </tr>
            </tbody></table>
        "#;
        let document = Html::parse_document(html);
        let row_selector = Selector::parse("tr").unwrap();
        let date_selector = Selector::parse("td:nth-child(1)").unwrap();
        let name_selector = Selector::parse("td:nth-child(3) a.calendar-event").unwrap();
        let actual_selector = Selector::parse("td:nth-child(4)").unwrap();
        let previous_selector = Selector::parse("td:nth-child(5)").unwrap();
        let forecast_selector = Selector::parse("td:nth-child(7)").unwrap();
        let row = document.select(&row_selector).next().unwrap();

        let date = extract_row_date(
            &row,
            &date_selector,
            NaiveDate::from_ymd_opt(2026, 4, 22).unwrap(),
            2026,
        );
        let event = extract_calendar_event(
            &row,
            date,
            &name_selector,
            &actual_selector,
            &previous_selector,
            &forecast_selector,
        )
        .unwrap();

        assert_eq!(event.date, "2026-04-23");
        assert_eq!(event.name, "Initial Jobless Claims");
        assert_eq!(event.previous.as_deref(), Some("207K"));
        assert_eq!(event.forecast.as_deref(), Some("218.0K"));
    }
}
