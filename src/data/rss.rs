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
    /// RSS snippet/summary from the <description> element. May be absent or empty.
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FeedError {
    pub feed_name: String,
    pub feed_url: String,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct FeedFetchReport {
    pub items: Vec<NewsItem>,
    pub errors: Vec<FeedError>,
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
            name: "Bloomberg Markets".to_string(),
            url: "https://feeds.bloomberg.com/markets/news.rss".to_string(),
            category: NewsCategory::Markets,
        },
        RssFeed {
            name: "Bloomberg Economics".to_string(),
            url: "https://feeds.bloomberg.com/economics/news.rss".to_string(),
            category: NewsCategory::Macro,
        },
        RssFeed {
            name: "Bloomberg Commodities".to_string(),
            url: "https://feeds.bloomberg.com/commodities/news.rss".to_string(),
            category: NewsCategory::Commodities,
        },
        RssFeed {
            name: "Bloomberg Crypto".to_string(),
            url: "https://feeds.bloomberg.com/crypto/news.rss".to_string(),
            category: NewsCategory::Crypto,
        },
        RssFeed {
            name: "Bloomberg Politics".to_string(),
            url: "https://feeds.bloomberg.com/politics/news.rss".to_string(),
            category: NewsCategory::Geopolitics,
        },
    ]
}

#[derive(Debug, Deserialize)]
struct Rss {
    channel: RssChannel,
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
    #[serde(default)]
    description: Option<String>,
}

/// Strip HTML tags and entity-decode a string for plain-text use.
fn strip_html(raw: &str) -> String {
    // Remove CDATA wrappers if present
    let s = raw
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim();

    // Strip all < ... > tags
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    // Decode common HTML entities
    let decoded = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Collapse whitespace
    decoded
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(512) // cap at 512 chars for storage
        .collect()
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

    let rss: Rss = quick_xml::de::from_str(&body).context("Failed to parse RSS XML")?;

    let mut items = Vec::new();
    for item in rss.channel.item {
        let published_at = parse_rfc2822(&item.pub_date.unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        let description = item
            .description
            .as_deref()
            .map(strip_html)
            .filter(|s| !s.is_empty());

        items.push(NewsItem {
            title: item.title.trim().to_string(),
            url: item.link.trim().to_string(),
            source: feed.name.clone(),
            category: feed.category,
            published_at,
            description,
        });
    }

    Ok(items)
}

/// Fetch all configured feeds concurrently.
pub async fn fetch_all_feeds(feeds: &[RssFeed]) -> Vec<NewsItem> {
    fetch_all_feeds_detailed(feeds).await.items
}

/// Fetch all configured feeds concurrently with per-feed diagnostics.
pub async fn fetch_all_feeds_detailed(feeds: &[RssFeed]) -> FeedFetchReport {
    let mut handles = Vec::new();

    for feed in feeds {
        let feed = feed.clone();
        handles.push(tokio::spawn(async move {
            match fetch_feed(&feed).await {
                Ok(items) => Ok(items),
                Err(err) => Err(FeedError {
                    feed_name: feed.name,
                    feed_url: feed.url,
                    error: format!("{err:#}"),
                }),
            }
        }));
    }

    let mut report = FeedFetchReport::default();
    for handle in handles {
        match handle.await {
            Ok(Ok(items)) => report.items.extend(items),
            Ok(Err(err)) => report.errors.push(err),
            Err(join_err) => report.errors.push(FeedError {
                feed_name: "unknown".to_string(),
                feed_url: "".to_string(),
                error: format!("RSS task join failure: {join_err}"),
            }),
        }
    }

    // Sort by timestamp descending (newest first)
    report.items.sort_by(|a, b| b.published_at.cmp(&a.published_at));

    // Deduplicate by URL (keep first occurrence = newest)
    let mut seen_urls = std::collections::HashSet::new();
    report.items.retain(|item| seen_urls.insert(item.url.clone()));

    report
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
        assert_eq!(feeds.len(), 5);
        assert!(feeds.iter().any(|f| f.name == "Bloomberg Markets"));
        assert!(feeds.iter().any(|f| f.name == "Bloomberg Crypto"));
        assert!(feeds.iter().any(|f| f.category == NewsCategory::Crypto));
    }

    #[test]
    fn test_category_as_str() {
        assert_eq!(NewsCategory::Macro.as_str(), "macro");
        assert_eq!(NewsCategory::Crypto.as_str(), "crypto");
    }

    #[tokio::test]
    async fn test_fetch_all_feeds_detailed_empty_input() {
        let report = fetch_all_feeds_detailed(&[]).await;
        assert!(report.items.is_empty());
        assert!(report.errors.is_empty());
    }
}
