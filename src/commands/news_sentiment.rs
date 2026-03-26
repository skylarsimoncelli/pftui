//! News sentiment scoring and aggregation.
//!
//! Keyword-based sentiment analysis for cached financial news.
//! Scores news headlines as bullish/bearish/neutral using domain-specific
//! word lists. Aggregates by category, source, and symbol tag.

use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;

use crate::db::backend::BackendConnection;
use crate::db::news_cache::{get_latest_news_backend, NewsEntry};

/// Sentiment label for a news item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentimentLabel {
    Bullish,
    Bearish,
    Neutral,
}

impl SentimentLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            SentimentLabel::Bullish => "bullish",
            SentimentLabel::Bearish => "bearish",
            SentimentLabel::Neutral => "neutral",
        }
    }
}

/// Scored news item with sentiment analysis.
#[derive(Debug, Clone)]
pub struct ScoredNews {
    pub entry: NewsEntry,
    pub score: i32,
    pub label: SentimentLabel,
    pub bullish_hits: Vec<String>,
    pub bearish_hits: Vec<String>,
}

/// Aggregated sentiment for a grouping (category, source, or symbol).
#[derive(Debug, Clone)]
pub struct SentimentAggregate {
    pub group: String,
    pub count: usize,
    pub avg_score: f64,
    pub bullish_count: usize,
    pub bearish_count: usize,
    pub neutral_count: usize,
    pub label: SentimentLabel,
}

// --- Keyword dictionaries ---
// Each keyword has a weight (1-3). Matched case-insensitively against title + description.

const BULLISH_KEYWORDS: &[(&str, i32)] = &[
    // Strong bullish (weight 3)
    ("surge", 3),
    ("soar", 3),
    ("rally", 3),
    ("breakout", 3),
    ("all-time high", 3),
    ("record high", 3),
    ("boom", 3),
    ("skyrocket", 3),
    ("moonshot", 3),
    // Medium bullish (weight 2)
    ("gain", 2),
    ("rise", 2),
    ("climb", 2),
    ("advance", 2),
    ("recovery", 2),
    ("uptick", 2),
    ("bullish", 2),
    ("outperform", 2),
    ("beat expectations", 2),
    ("strong earnings", 2),
    ("rate cut", 2),
    ("stimulus", 2),
    ("easing", 2),
    ("dovish", 2),
    ("ceasefire", 2),
    ("peace", 2),
    ("deal", 2),
    ("agreement", 2),
    ("approval", 2),
    ("upgrade", 2),
    ("buy signal", 2),
    ("positive", 2),
    // Mild bullish (weight 1)
    ("up", 1),
    ("higher", 1),
    ("growth", 1),
    ("expand", 1),
    ("optimism", 1),
    ("confidence", 1),
    ("demand", 1),
    ("inflow", 1),
    ("accumulation", 1),
    ("resilient", 1),
    ("stabilize", 1),
];

const BEARISH_KEYWORDS: &[(&str, i32)] = &[
    // Strong bearish (weight 3)
    ("crash", 3),
    ("plunge", 3),
    ("collapse", 3),
    ("capitulation", 3),
    ("panic", 3),
    ("liquidation", 3),
    ("circuit breaker", 3),
    ("black swan", 3),
    ("crisis", 3),
    // Medium bearish (weight 2)
    ("sell-off", 2),
    ("selloff", 2),
    ("decline", 2),
    ("drop", 2),
    ("fall", 2),
    ("recession", 2),
    ("bearish", 2),
    ("downgrade", 2),
    ("miss expectations", 2),
    ("weak earnings", 2),
    ("rate hike", 2),
    ("hawkish", 2),
    ("tightening", 2),
    ("inflation spike", 2),
    ("stagflation", 2),
    ("sanctions", 2),
    ("escalation", 2),
    ("war", 2),
    ("strike", 2),
    ("tariff", 2),
    ("default", 2),
    ("bankruptcy", 2),
    ("layoff", 2),
    ("cut jobs", 2),
    ("sell signal", 2),
    ("negative", 2),
    // Mild bearish (weight 1)
    ("down", 1),
    ("lower", 1),
    ("contraction", 1),
    ("slowdown", 1),
    ("concern", 1),
    ("fear", 1),
    ("uncertainty", 1),
    ("risk", 1),
    ("outflow", 1),
    ("volatile", 1),
    ("pressure", 1),
    ("retreat", 1),
];

/// Score a single news entry's sentiment.
///
/// Returns a score from approximately -100 to +100 and a label.
/// Positive = bullish, negative = bearish, near-zero = neutral.
pub fn score_news(entry: &NewsEntry) -> ScoredNews {
    let text = format!(
        "{} {} {}",
        entry.title,
        entry.description,
        entry.extra_snippets.join(" ")
    )
    .to_lowercase();

    let mut bullish_score = 0i32;
    let mut bearish_score = 0i32;
    let mut bullish_hits = Vec::new();
    let mut bearish_hits = Vec::new();

    for &(keyword, weight) in BULLISH_KEYWORDS {
        if text.contains(keyword) {
            bullish_score += weight;
            bullish_hits.push(keyword.to_string());
        }
    }

    for &(keyword, weight) in BEARISH_KEYWORDS {
        if text.contains(keyword) {
            bearish_score += weight;
            bearish_hits.push(keyword.to_string());
        }
    }

    let raw_score = bullish_score - bearish_score;

    // Normalize to -100..+100 range. Cap at ±15 raw points as practical max.
    let score = (raw_score * 100 / 15).clamp(-100, 100);

    let label = if score > 15 {
        SentimentLabel::Bullish
    } else if score < -15 {
        SentimentLabel::Bearish
    } else {
        SentimentLabel::Neutral
    };

    ScoredNews {
        entry: entry.clone(),
        score,
        label,
        bullish_hits,
        bearish_hits,
    }
}

/// Score all provided news entries.
pub fn score_all(entries: &[NewsEntry]) -> Vec<ScoredNews> {
    entries.iter().map(score_news).collect()
}

/// Aggregate scored news by a grouping function.
fn aggregate_by<F: Fn(&ScoredNews) -> String>(
    scored: &[ScoredNews],
    group_fn: F,
) -> Vec<SentimentAggregate> {
    let mut groups: HashMap<String, Vec<&ScoredNews>> = HashMap::new();
    for item in scored {
        let key = group_fn(item);
        groups.entry(key).or_default().push(item);
    }

    let mut aggregates: Vec<SentimentAggregate> = groups
        .into_iter()
        .map(|(group, items)| {
            let count = items.len();
            let total_score: i32 = items.iter().map(|i| i.score).sum();
            let avg_score = if count > 0 {
                total_score as f64 / count as f64
            } else {
                0.0
            };
            let bullish_count = items.iter().filter(|i| i.label == SentimentLabel::Bullish).count();
            let bearish_count = items.iter().filter(|i| i.label == SentimentLabel::Bearish).count();
            let neutral_count = count - bullish_count - bearish_count;

            let label = if avg_score > 10.0 {
                SentimentLabel::Bullish
            } else if avg_score < -10.0 {
                SentimentLabel::Bearish
            } else {
                SentimentLabel::Neutral
            };

            SentimentAggregate {
                group,
                count,
                avg_score,
                bullish_count,
                bearish_count,
                neutral_count,
                label,
            }
        })
        .collect();

    aggregates.sort_by(|a, b| {
        b.avg_score
            .partial_cmp(&a.avg_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    aggregates
}

/// Aggregate scored news by category.
pub fn aggregate_by_category(scored: &[ScoredNews]) -> Vec<SentimentAggregate> {
    aggregate_by(scored, |item| item.entry.category.clone())
}

/// Aggregate scored news by source.
#[allow(dead_code)]
pub fn aggregate_by_source(scored: &[ScoredNews]) -> Vec<SentimentAggregate> {
    aggregate_by(scored, |item| item.entry.source.clone())
}

/// Run the `analytics news-sentiment` command.
pub fn run(
    backend: &BackendConnection,
    category: Option<&str>,
    hours: Option<i64>,
    limit: usize,
    detail: bool,
    json: bool,
) -> Result<()> {
    let entries = get_latest_news_backend(backend, limit, None, category, None, hours)?;

    if entries.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "articles": 0,
                    "overall_score": 0,
                    "overall_label": "neutral",
                    "by_category": [],
                    "items": []
                }))?
            );
        } else {
            println!("No cached news. Run `pftui data refresh` first.");
        }
        return Ok(());
    }

    let scored = score_all(&entries);
    let by_category = aggregate_by_category(&scored);

    let total_score: i32 = scored.iter().map(|s| s.score).sum();
    let overall_avg = if scored.is_empty() {
        0.0
    } else {
        total_score as f64 / scored.len() as f64
    };
    let overall_label = if overall_avg > 10.0 {
        SentimentLabel::Bullish
    } else if overall_avg < -10.0 {
        SentimentLabel::Bearish
    } else {
        SentimentLabel::Neutral
    };

    if json {
        print_json(&scored, &by_category, overall_avg, overall_label, detail)?;
    } else {
        print_terminal(&scored, &by_category, overall_avg, overall_label, detail);
    }

    Ok(())
}

fn print_json(
    scored: &[ScoredNews],
    by_category: &[SentimentAggregate],
    overall_avg: f64,
    overall_label: SentimentLabel,
    detail: bool,
) -> Result<()> {
    let categories: Vec<_> = by_category
        .iter()
        .map(|a| {
            json!({
                "category": a.group,
                "count": a.count,
                "avg_score": (a.avg_score * 10.0).round() / 10.0,
                "label": a.label.as_str(),
                "bullish": a.bullish_count,
                "bearish": a.bearish_count,
                "neutral": a.neutral_count,
            })
        })
        .collect();

    let items: Vec<_> = if detail {
        scored
            .iter()
            .map(|s| {
                json!({
                    "id": s.entry.id,
                    "title": s.entry.title,
                    "source": s.entry.source,
                    "category": s.entry.category,
                    "published_at": s.entry.published_at,
                    "score": s.score,
                    "label": s.label.as_str(),
                    "bullish_hits": s.bullish_hits,
                    "bearish_hits": s.bearish_hits,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let output = json!({
        "articles": scored.len(),
        "overall_score": (overall_avg * 10.0).round() / 10.0,
        "overall_label": overall_label.as_str(),
        "by_category": categories,
        "items": items,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_terminal(
    scored: &[ScoredNews],
    by_category: &[SentimentAggregate],
    overall_avg: f64,
    overall_label: SentimentLabel,
    detail: bool,
) {
    let bullish_count = scored
        .iter()
        .filter(|s| s.label == SentimentLabel::Bullish)
        .count();
    let bearish_count = scored
        .iter()
        .filter(|s| s.label == SentimentLabel::Bearish)
        .count();
    let neutral_count = scored.len() - bullish_count - bearish_count;

    // Overall summary
    let sentiment_icon = match overall_label {
        SentimentLabel::Bullish => "🟢",
        SentimentLabel::Bearish => "🔴",
        SentimentLabel::Neutral => "⚪",
    };

    println!(
        "\n{} News Sentiment: {} (avg score: {:.1})",
        sentiment_icon,
        overall_label.as_str().to_uppercase(),
        overall_avg
    );
    println!(
        "   {} articles: {} bullish, {} bearish, {} neutral\n",
        scored.len(),
        bullish_count,
        bearish_count,
        neutral_count
    );

    // By category
    if !by_category.is_empty() {
        println!(
            "{:<16} {:>5}  {:>6}  {:>4} {:>4} {:>4}  Label",
            "Category", "Count", "Score", "🟢", "🔴", "⚪"
        );
        println!("{}", "─".repeat(65));
        for agg in by_category {
            let icon = match agg.label {
                SentimentLabel::Bullish => "🟢",
                SentimentLabel::Bearish => "🔴",
                SentimentLabel::Neutral => "⚪",
            };
            println!(
                "{:<16} {:>5}  {:>6.1}  {:>4} {:>4} {:>4}  {} {}",
                agg.group,
                agg.count,
                agg.avg_score,
                agg.bullish_count,
                agg.bearish_count,
                agg.neutral_count,
                icon,
                agg.label.as_str(),
            );
        }
    }

    // Detail list
    if detail {
        println!("\n{:<6} {:<60} {:>6} Hits", "Score", "Title", "Label");
        println!("{}", "─".repeat(90));
        for s in scored {
            let title = if s.entry.title.len() > 58 {
                format!("{}...", &s.entry.title[..55])
            } else {
                s.entry.title.clone()
            };
            let hits: Vec<&str> = s
                .bullish_hits
                .iter()
                .map(|h| h.as_str())
                .chain(s.bearish_hits.iter().map(|h| h.as_str()))
                .collect();
            let hits_str = if hits.is_empty() {
                String::new()
            } else {
                format!("[{}]", hits.join(", "))
            };
            println!(
                "{:>+5}  {:<60} {:>6} {}",
                s.score,
                title,
                s.label.as_str(),
                hits_str,
            );
        }
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::news_cache::NewsEntry;

    fn make_entry(title: &str, category: &str) -> NewsEntry {
        NewsEntry {
            id: 1,
            title: title.to_string(),
            url: format!("https://example.com/{}", title.replace(' ', "-")),
            source: "TestSource".to_string(),
            source_type: "rss".to_string(),
            symbol_tag: None,
            description: String::new(),
            extra_snippets: vec![],
            category: category.to_string(),
            published_at: 1709610000,
            fetched_at: "2024-03-05 10:00:00".to_string(),
        }
    }

    #[test]
    fn test_score_bullish_headline() {
        let entry = make_entry("Bitcoin surges past $100k in massive rally", "crypto");
        let scored = score_news(&entry);
        assert!(scored.score > 0, "Expected positive score, got {}", scored.score);
        assert_eq!(scored.label, SentimentLabel::Bullish);
        assert!(!scored.bullish_hits.is_empty());
    }

    #[test]
    fn test_score_bearish_headline() {
        let entry = make_entry("Markets crash as recession fears spark panic selling", "markets");
        let scored = score_news(&entry);
        assert!(scored.score < 0, "Expected negative score, got {}", scored.score);
        assert_eq!(scored.label, SentimentLabel::Bearish);
        assert!(!scored.bearish_hits.is_empty());
    }

    #[test]
    fn test_score_neutral_headline() {
        let entry = make_entry("Federal Reserve announces next meeting schedule", "macro");
        let scored = score_news(&entry);
        assert_eq!(scored.label, SentimentLabel::Neutral);
    }

    #[test]
    fn test_score_mixed_headline() {
        // Contains both bullish and bearish keywords — should partially cancel
        let entry = make_entry("Gold rally continues despite recession fears", "commodities");
        let scored = score_news(&entry);
        // Has both hits
        assert!(!scored.bullish_hits.is_empty());
        assert!(!scored.bearish_hits.is_empty());
    }

    #[test]
    fn test_score_all() {
        let entries = vec![
            make_entry("Bitcoin surges to new high", "crypto"),
            make_entry("Stocks crash in selloff", "markets"),
            make_entry("Fed meeting minutes released", "macro"),
        ];
        let scored = score_all(&entries);
        assert_eq!(scored.len(), 3);
    }

    #[test]
    fn test_aggregate_by_category() {
        let entries = vec![
            make_entry("Crypto rally continues", "crypto"),
            make_entry("Bitcoin soars past resistance", "crypto"),
            make_entry("Oil prices drop sharply", "commodities"),
        ];
        let scored = score_all(&entries);
        let aggs = aggregate_by_category(&scored);
        assert_eq!(aggs.len(), 2);

        let crypto_agg = aggs.iter().find(|a| a.group == "crypto").unwrap();
        assert_eq!(crypto_agg.count, 2);
        assert!(crypto_agg.avg_score > 0.0);
    }

    #[test]
    fn test_sentiment_label_as_str() {
        assert_eq!(SentimentLabel::Bullish.as_str(), "bullish");
        assert_eq!(SentimentLabel::Bearish.as_str(), "bearish");
        assert_eq!(SentimentLabel::Neutral.as_str(), "neutral");
    }

    #[test]
    fn test_score_clamped() {
        // Extremely bullish headline should cap at 100
        let entry = make_entry(
            "Stocks surge in massive rally as markets soar to record high and boom continues with breakout gains",
            "markets",
        );
        let scored = score_news(&entry);
        assert!(scored.score <= 100);
        assert!(scored.score >= -100);
    }

    #[test]
    fn test_description_and_snippets_included() {
        let mut entry = make_entry("Boring headline", "markets");
        entry.description = "Markets crash in historic selloff".to_string();
        let scored = score_news(&entry);
        assert!(scored.score < 0, "Description keywords should be scored");
        assert!(!scored.bearish_hits.is_empty());
    }

    #[test]
    fn test_run_empty_cache_json() {
        let conn = crate::db::open_in_memory();
        let backend = BackendConnection::Sqlite { conn };
        let result = run(&backend, None, None, 20, false, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_with_entries() {
        let conn = crate::db::open_in_memory();
        crate::db::news_cache::insert_news(
            &conn,
            "Gold surges on safe-haven demand",
            "https://example.com/gold",
            "Reuters",
            "commodities",
            chrono::Utc::now().timestamp(),
        )
        .unwrap();
        let backend = BackendConnection::Sqlite { conn };
        let result = run(&backend, None, None, 20, false, true);
        assert!(result.is_ok());
    }
}
