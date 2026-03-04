//! Economic calendar data source.
//!
//! Provides upcoming market-moving events: economic releases (FOMC, CPI, NFP) and earnings.
//! Current implementation uses curated sample data. Future: integrate Finnhub free tier API.
//!
//! Sample data covers Mar-Apr 2026 with realistic high-impact events.

use anyhow::Result;
use chrono::{Duration, Utc};

/// A calendar event (economic or earnings).
#[derive(Debug, Clone)]
pub struct Event {
    pub date: String,      // YYYY-MM-DD
    pub name: String,
    pub impact: String,    // "high", "medium", "low"
    pub previous: Option<String>,
    pub forecast: Option<String>,
    pub event_type: String, // "economic" or "earnings"
    pub symbol: Option<String>,
}

/// Fetch upcoming calendar events from data source.
/// Returns events from today through `days_ahead` days.
pub fn fetch_events(days_ahead: i64) -> Result<Vec<Event>> {
    let today = Utc::now().date_naive();
    let cutoff = today + Duration::days(days_ahead);

    let all_events = get_sample_events();

    // Filter to requested date range
    let filtered: Vec<Event> = all_events
        .into_iter()
        .filter(|e| {
            if let Ok(event_date) = chrono::NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") {
                event_date >= today && event_date <= cutoff
            } else {
                false
            }
        })
        .collect();

    Ok(filtered)
}

/// Hardcoded sample calendar events for Mar-Apr 2026.
/// TODO: Replace with Finnhub API integration or similar free source.
fn get_sample_events() -> Vec<Event> {
    vec![
        Event {
            date: "2026-03-04".into(),
            name: "JOLTS Job Openings".into(),
            impact: "high".into(),
            previous: Some("7.6M".into()),
            forecast: Some("7.5M".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-05".into(),
            name: "ADP Employment Change".into(),
            impact: "medium".into(),
            previous: Some("183K".into()),
            forecast: Some("175K".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-06".into(),
            name: "Initial Jobless Claims".into(),
            impact: "medium".into(),
            previous: Some("213K".into()),
            forecast: None,
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-06".into(),
            name: "Coinbase Q4 2025 Earnings".into(),
            impact: "high".into(),
            previous: None,
            forecast: None,
            event_type: "earnings".into(),
            symbol: Some("COIN".into()),
        },
        Event {
            date: "2026-03-07".into(),
            name: "Non-Farm Payrolls".into(),
            impact: "high".into(),
            previous: Some("143K".into()),
            forecast: Some("160K".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-07".into(),
            name: "Unemployment Rate".into(),
            impact: "high".into(),
            previous: Some("4.0%".into()),
            forecast: Some("4.0%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-10".into(),
            name: "Producer Price Index (PPI)".into(),
            impact: "medium".into(),
            previous: Some("2.6%".into()),
            forecast: Some("2.5%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-12".into(),
            name: "Consumer Price Index (CPI)".into(),
            impact: "high".into(),
            previous: Some("3.0%".into()),
            forecast: Some("2.9%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-12".into(),
            name: "Robinhood Q4 2025 Earnings".into(),
            impact: "medium".into(),
            previous: None,
            forecast: None,
            event_type: "earnings".into(),
            symbol: Some("HOOD".into()),
        },
        Event {
            date: "2026-03-18".into(),
            name: "FOMC Rate Decision".into(),
            impact: "high".into(),
            previous: Some("3.50%".into()),
            forecast: Some("3.50%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-18".into(),
            name: "FOMC Press Conference".into(),
            impact: "high".into(),
            previous: None,
            forecast: None,
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-19".into(),
            name: "Initial Jobless Claims".into(),
            impact: "medium".into(),
            previous: Some("215K".into()),
            forecast: None,
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-25".into(),
            name: "Durable Goods Orders".into(),
            impact: "medium".into(),
            previous: Some("2.8%".into()),
            forecast: Some("1.5%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-26".into(),
            name: "GDP (Preliminary)".into(),
            impact: "high".into(),
            previous: Some("2.3%".into()),
            forecast: Some("2.5%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-03-31".into(),
            name: "Core PCE Price Index".into(),
            impact: "high".into(),
            previous: Some("2.8%".into()),
            forecast: Some("2.7%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-04-01".into(),
            name: "ISM Manufacturing PMI".into(),
            impact: "medium".into(),
            previous: Some("50.9".into()),
            forecast: Some("51.2".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-04-02".into(),
            name: "Initial Jobless Claims".into(),
            impact: "medium".into(),
            previous: Some("220K".into()),
            forecast: None,
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-04-04".into(),
            name: "Non-Farm Payrolls".into(),
            impact: "high".into(),
            previous: Some("160K".into()),
            forecast: Some("170K".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-04-10".into(),
            name: "Consumer Price Index (CPI)".into(),
            impact: "high".into(),
            previous: Some("2.9%".into()),
            forecast: Some("2.8%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-04-15".into(),
            name: "Retail Sales".into(),
            impact: "medium".into(),
            previous: Some("0.4%".into()),
            forecast: Some("0.5%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
        Event {
            date: "2026-04-29".into(),
            name: "FOMC Rate Decision".into(),
            impact: "high".into(),
            previous: Some("3.50%".into()),
            forecast: Some("3.25%".into()),
            event_type: "economic".into(),
            symbol: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_events_returns_non_empty() {
        let events = fetch_events(90).unwrap();
        // Sample data has ~20 events in Mar-Apr 2026
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
            assert!(
                first.impact == "high" || first.impact == "medium" || first.impact == "low"
            );
            assert!(
                first.event_type == "economic" || first.event_type == "earnings"
            );
        }
    }
}
