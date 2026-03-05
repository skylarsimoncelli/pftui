//! RSS feed aggregation from free financial news sources.
//!
//! Default feed list ships with pftui:
//! - Reuters Business
//! - CoinDesk
//! - ZeroHedge
//! - Yahoo Finance
//! - MarketWatch
//!
//! User can add/remove feeds via config.toml.
//! Poll interval: 10 minutes (configurable).

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct NewsItem {
    pub title: String,
    pub url: String,
    pub source: String,
    pub category: NewsCategory,
    pub published_at: i64, // Unix timestamp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsCategory {
    Macro,
    Crypto,
    Commodities,
    Geopolitics,
    Markets,
}

impl NewsCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Macro => "macro",
            Self::Crypto => "crypto",
            Self::Commodities => "commodities",
            Self::Geopolitics => "geopolitics",
            Self::Markets => "markets",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RssFeed {
    pub name: String,
    pub url: String,
    pub category: NewsCategory,
}

/// Default feed list that ships with pftui.
pub fn default_feeds() -> Vec<RssFeed> {
    vec![
        RssFeed {
            name: "Reuters Business".to_string(),
            url: "https://www.reuters.com/rssfeed/businessNews".to_string(),
            category: NewsCategory::Macro,
        },
        RssFeed {
            name: "CoinDesk".to_string(),
            url: "https://www.coindesk.com/arc/outboundfeeds/rss/".to_string(),
            category: NewsCategory::Crypto,
        },
        RssFeed {
            name: "ZeroHedge".to_string(),
            url: "https://www.zerohedge.com/fullrss2.xml".to_string(),
            category: NewsCategory::Geopolitics,
        },
        RssFeed {
            name: "Yahoo Finance".to_string(),
            url: "https://finance.yahoo.com/news/rssindex".to_string(),
            category: NewsCategory::Markets,
        },
        RssFeed {
            name: "MarketWatch".to_string(),
            url: "https://feeds.marketwatch.com/marketwatch/marketpulse/".to_string(),
            category: NewsCategory::Markets,
        },
        RssFeed {
            name: "Kitco Gold News".to_string(),
            url: "https://www.kitco.com/rss/KitcoNews.xml".to_string(),
            category: NewsCategory::Commodities,
        },
    ]
}

#[derive(Debug, Deserialize)]
struct RssChannel {
    item: Vec<RssItem>,
}

#[derive(Debug, Deserialize)]
struct RssItem {
    title: String,
    link: String,
    #[serde(rename = "pubDate", default)]
    pub_date: Option<String>,
}

/// Fetch and parse a single RSS feed.
///
/// Returns a list of NewsItem entries with normalized timestamps.
pub async fn fetch_feed(feed: &RssFeed) -> Result<Vec<NewsItem>> {
    let client = reqwest::Client::builder()
        .user_agent("pftui/0.5.0")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let body = client
        .get(&feed.url)
        .send()
        .await
        .context("Failed to fetch RSS feed")?
        .text()
        .await?;

    let channel: RssChannel = quick_xml::de::from_str(&body)
        .context("Failed to parse RSS XML")?;

    let mut items = Vec::new();
    for item in channel.item {
        let published_at = parse_rfc2822(&item.pub_date.unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        items.push(NewsItem {
            title: item.title.trim().to_string(),
            url: item.link.trim().to_string(),
            source: feed.name.clone(),
            category: feed.category,
            published_at,
        });
    }

    Ok(items)
}

/// Fetch all configured feeds concurrently.
pub async fn fetch_all_feeds(feeds: &[RssFeed]) -> Vec<NewsItem> {
    let mut handles = Vec::new();

    for feed in feeds {
        let feed = feed.clone();
        handles.push(tokio::spawn(async move {
            fetch_feed(&feed).await.unwrap_or_default()
        }));
    }

    let mut all_items = Vec::new();
    for handle in handles {
        if let Ok(items) = handle.await {
            all_items.extend(items);
        }
    }

    // Sort by timestamp descending (newest first)
    all_items.sort_by(|a, b| b.published_at.cmp(&a.published_at));

    // Deduplicate by URL (keep first occurrence = newest)
    let mut seen_urls = std::collections::HashSet::new();
    all_items.retain(|item| seen_urls.insert(item.url.clone()));

    all_items
}

/// Parse RFC 2822 date string to Unix timestamp.
fn parse_rfc2822(s: &str) -> Option<i64> {
    use chrono::DateTime;
    DateTime::parse_from_rfc2822(s)
        .ok()
        .map(|dt| dt.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rfc2822() {
        let date = "Wed, 05 Mar 2025 04:30:00 GMT";
        let ts = parse_rfc2822(date);
        assert!(ts.is_some());
        assert!(ts.unwrap() > 0);
    }

    #[test]
    fn test_default_feeds() {
        let feeds = default_feeds();
        assert_eq!(feeds.len(), 6);
        assert!(feeds.iter().any(|f| f.name == "Reuters Business"));
        assert!(feeds.iter().any(|f| f.name == "CoinDesk"));
        assert!(feeds.iter().any(|f| f.category == NewsCategory::Crypto));
    }

    #[test]
    fn test_category_as_str() {
        assert_eq!(NewsCategory::Macro.as_str(), "macro");
        assert_eq!(NewsCategory::Crypto.as_str(), "crypto");
    }
}
